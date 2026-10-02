//! Display enumeration and the effective window mode reported to programs.

use nana_ui_platform::{DisplayId, DisplayInfo, FullscreenMode};
use winit::monitor::{Fullscreen, MonitorHandle};
#[cfg(target_os = "macos")]
use winit::platform::macos::WindowExtMacOS;

use super::*;
use crate::WindowError;

pub(super) fn display_info(
    monitor: &MonitorHandle,
    primary: Option<&MonitorHandle>,
) -> DisplayInfo {
    let mode = monitor.current_video_mode();
    DisplayInfo {
        id: DisplayId(monitor.id()),
        name: monitor.name().map(|name| name.into_owned()),
        physical_position: monitor.position().map(|position| (position.x, position.y)),
        physical_size: mode.map(|mode| {
            let size = mode.size();
            (size.width, size.height)
        }),
        scale_factor: monitor.scale_factor(),
        refresh_rate_millihertz: mode.and_then(|mode| mode.refresh_rate_millihertz()),
        primary: primary == Some(monitor),
    }
}

pub(super) fn display_infos(event_loop: &dyn ActiveEventLoop) -> Vec<DisplayInfo> {
    let primary = event_loop.primary_monitor();
    event_loop
        .available_monitors()
        .map(|monitor| display_info(&monitor, primary.as_ref()))
        .collect()
}

fn find_monitor(event_loop: &dyn ActiveEventLoop, display: DisplayId) -> Option<MonitorHandle> {
    event_loop
        .available_monitors()
        .find(|monitor| monitor.id() == display.0)
}

fn winit_level(level: WindowLevel) -> winit::window::WindowLevel {
    match level {
        WindowLevel::Normal => winit::window::WindowLevel::Normal,
        WindowLevel::AlwaysOnTop => winit::window::WindowLevel::AlwaysOnTop,
        WindowLevel::AlwaysOnBottom => winit::window::WindowLevel::AlwaysOnBottom,
    }
}

/// Explicit requests name a connected display or keep the current one.
#[cfg(test)]
fn explicit_fullscreen_display(
    display: Option<DisplayId>,
    connected: impl Fn(DisplayId) -> bool,
) -> Result<Option<DisplayId>, WindowError> {
    match display {
        Some(display) if !connected(display) => Err(WindowError::InvalidParameter(
            "display is not connected".into(),
        )),
        display => Ok(display),
    }
}

/// Descriptor fullscreen that names a disconnected display opens windowed.
fn descriptor_fullscreen(
    request: FullscreenRequest,
    connected: impl Fn(DisplayId) -> bool,
) -> Option<FullscreenRequest> {
    match request.display {
        Some(display) if !connected(display) => None,
        _ => Some(request),
    }
}

/// winit resets the NSWindow level across a native fullscreen transition.
/// Tested on every host; only the macOS branch calls it outside tests.
#[cfg_attr(all(not(target_os = "macos"), not(test)), allow(dead_code))]
fn should_restore_level_after_fullscreen_change(
    level: WindowLevel,
    previous: Option<WindowModeState>,
    observed: WindowModeState,
) -> bool {
    level != WindowLevel::Normal
        && previous.is_some_and(|previous| previous.fullscreen != observed.fullscreen)
}

/// Pure state reads: safe inside resize handling, unlike macOS `is_zoomed`.
fn observe_mode(window: &dyn winit::window::Window, level: WindowLevel) -> WindowModeState {
    #[cfg(target_os = "macos")]
    let simple = WindowExtMacOS::simple_fullscreen(window);
    #[cfg(not(target_os = "macos"))]
    let simple = false;
    WindowModeState {
        fullscreen: if simple {
            Some(FullscreenMode::Simple)
        } else {
            window.fullscreen().map(|_| FullscreenMode::Borderless)
        },
        level,
        display: window
            .current_monitor()
            .map(|monitor| DisplayId(monitor.id())),
    }
}

/// The mode to deliver: the first observation, then only changes.
fn mode_change(
    previous: Option<WindowModeState>,
    observed: WindowModeState,
) -> Option<WindowModeState> {
    (previous != Some(observed)).then_some(observed)
}

impl<Program: RuntimeProgram> WindowManager<Program> {
    /// An explicit request naming a display that is not connected fails and
    /// leaves the window unchanged.
    pub(super) fn set_window_fullscreen(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        request: Option<FullscreenRequest>,
    ) -> Result<(), WindowError> {
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return Err(WindowError::WindowClosed);
        };
        let monitor =
            match request.and_then(|request| request.display) {
                Some(display) => Some(find_monitor(event_loop, display).ok_or_else(|| {
                    WindowError::InvalidParameter("display is not connected".into())
                })?),
                None => None,
            };
        host.pending_fullscreen = None;
        self.apply_fullscreen(id, request, monitor);
        self.sync_window_mode(event_loop, id);
        // Borderless fullscreen is asynchronous; the next redraw re-observes.
        self.request_redraw(id);
        Ok(())
    }

    fn apply_fullscreen(
        &mut self,
        id: WindowId,
        request: Option<FullscreenRequest>,
        monitor: Option<MonitorHandle>,
    ) {
        #[cfg(target_os = "macos")]
        {
            if let Some(request) = request.filter(|request| request.mode == FullscreenMode::Simple)
                && self
                    .window(id)
                    .is_some_and(|window| window.fullscreen().is_some())
            {
                // Native fullscreen animates out; simple fullscreen follows
                // once the window reports that it has left.
                if let Some(host) = self.window_contexts.get_mut(&id) {
                    host.pending_fullscreen = Some(request);
                }
                self.mutate_native_style(id, |window| window.set_fullscreen(None));
                return;
            }
            self.mutate_native_style(id, |window| {
                let simple = WindowExtMacOS::simple_fullscreen(window);
                match request.map(|request| request.mode) {
                    None => {
                        if simple {
                            let _ = WindowExtMacOS::set_simple_fullscreen(window, false);
                        }
                        if window.fullscreen().is_some() {
                            window.set_fullscreen(None);
                        }
                    }
                    Some(FullscreenMode::Borderless) => {
                        if simple {
                            let _ = WindowExtMacOS::set_simple_fullscreen(window, false);
                        }
                        window.set_fullscreen(Some(Fullscreen::Borderless(monitor)));
                    }
                    Some(FullscreenMode::Simple) => {
                        // Simple fullscreen covers the screen the window is on.
                        if let Some(position) =
                            monitor.as_ref().and_then(|monitor| monitor.position())
                        {
                            if simple {
                                let _ = WindowExtMacOS::set_simple_fullscreen(window, false);
                            }
                            window.set_outer_position(position.into());
                        }
                        let _ = WindowExtMacOS::set_simple_fullscreen(window, true);
                    }
                }
            });
        }
        #[cfg(not(target_os = "macos"))]
        self.mutate_native_style(id, |window| {
            window.set_fullscreen(request.map(|_| Fullscreen::Borderless(monitor)));
        });
    }

    pub(super) fn set_window_level(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        level: WindowLevel,
    ) {
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return;
        };
        host.level = level;
        self.mutate_native_style(id, |window| window.set_window_level(winit_level(level)));
        self.sync_window_mode(event_loop, id);
    }

    /// Observe the window, apply a deferred fullscreen request once the window
    /// can take it, and deliver `ModeChanged` when the mode differs.
    pub(super) fn sync_window_mode(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId) {
        let Some(window) = self.window(id).cloned() else {
            return;
        };
        let Some(host) = self.window_contexts.get(&id) else {
            return;
        };
        let (level, previous) = (host.level, host.mode);
        let mut observed = observe_mode(window.as_ref(), level);
        if observed.fullscreen.is_none()
            && window.is_visible() != Some(false)
            && let Some(request) = self
                .window_contexts
                .get_mut(&id)
                .and_then(|host| host.pending_fullscreen.take())
        {
            // A deferred request whose display has gone is dropped; the window
            // stays where the platform placed it.
            if let Some(request) = descriptor_fullscreen(request, |display| {
                find_monitor(event_loop, display).is_some()
            }) {
                let monitor = request
                    .display
                    .and_then(|display| find_monitor(event_loop, display));
                self.apply_fullscreen(id, Some(request), monitor);
                observed = observe_mode(window.as_ref(), level);
            }
        }
        #[cfg(target_os = "macos")]
        if should_restore_level_after_fullscreen_change(level, previous, observed) {
            // Native fullscreen transitions reset the NSWindow level.
            window.set_window_level(winit_level(level));
        }
        let Some(mode) = mode_change(previous, observed) else {
            return;
        };
        if let Some(host) = self.window_contexts.get_mut(&id) {
            host.mode = Some(mode);
            // Native fullscreen transitions restore the window buttons, in
            // visibility and in position.
            if !host.native_controls_visible {
                let _ = nana_window::set_native_window_controls_visible(
                    window.as_ref(),
                    false,
                    std::time::Duration::ZERO,
                );
            }
            super::windows::place_native_controls(host);
        }
        let update = self
            .program
            .window_event(WindowEvent::ModeChanged { id, mode }, &self.context_for(id));
        self.apply_update(event_loop, update, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_is_delivered_first_and_then_only_when_it_changes() {
        let windowed = WindowModeState {
            fullscreen: None,
            level: WindowLevel::Normal,
            display: Some(DisplayId(1)),
        };
        assert_eq!(mode_change(None, windowed), Some(windowed));
        assert_eq!(mode_change(Some(windowed), windowed), None);
        let moved = WindowModeState {
            display: Some(DisplayId(2)),
            ..windowed
        };
        assert_eq!(mode_change(Some(windowed), moved), Some(moved));
        let fullscreen = WindowModeState {
            fullscreen: Some(FullscreenMode::Borderless),
            ..moved
        };
        assert_eq!(mode_change(Some(moved), fullscreen), Some(fullscreen));
    }

    #[test]
    fn display_bounds_are_logical_and_need_position_and_size() {
        let display = DisplayInfo {
            id: DisplayId(4),
            name: None,
            physical_position: Some((2880, 0)),
            physical_size: Some((3840, 2160)),
            scale_factor: 2.0,
            refresh_rate_millihertz: None,
            primary: false,
        };
        assert_eq!(
            display.logical_bounds(display.scale_factor),
            Some(DisplayBounds {
                position: (1440.0, 0.0),
                size: (1920.0, 1080.0),
            })
        );
        let bounds = display.logical_bounds(display.scale_factor).unwrap();
        assert_eq!(
            clamp_position_to_displays((0.0, 40.0), (320.0, 240.0), &[bounds]),
            (1440.0, 40.0)
        );
        let unsized_display = DisplayInfo {
            physical_size: None,
            ..display
        };
        assert_eq!(unsized_display.logical_bounds(display.scale_factor), None);
    }

    #[test]
    fn explicit_fullscreen_rejects_a_disconnected_display_and_descriptor_falls_back() {
        let connected = |id: DisplayId| id == DisplayId(1);
        assert!(matches!(
            explicit_fullscreen_display(Some(DisplayId(9)), connected),
            Err(WindowError::InvalidParameter(_))
        ));
        assert_eq!(
            explicit_fullscreen_display(Some(DisplayId(1)), connected).unwrap(),
            Some(DisplayId(1))
        );
        assert_eq!(explicit_fullscreen_display(None, connected).unwrap(), None);
        let missing = FullscreenRequest {
            display: Some(DisplayId(9)),
            ..Default::default()
        };
        assert_eq!(descriptor_fullscreen(missing, connected), None);
        let current = FullscreenRequest {
            display: Some(DisplayId(1)),
            ..Default::default()
        };
        assert_eq!(descriptor_fullscreen(current, connected), Some(current));
        assert_eq!(
            descriptor_fullscreen(FullscreenRequest::default(), connected),
            Some(FullscreenRequest::default())
        );
    }

    #[test]
    fn fullscreen_transitions_restore_a_non_normal_level() {
        let windowed = WindowModeState {
            fullscreen: None,
            level: WindowLevel::AlwaysOnTop,
            display: Some(DisplayId(1)),
        };
        let fullscreen = WindowModeState {
            fullscreen: Some(FullscreenMode::Borderless),
            ..windowed
        };
        assert!(should_restore_level_after_fullscreen_change(
            WindowLevel::AlwaysOnTop,
            Some(windowed),
            fullscreen,
        ));
        assert!(!should_restore_level_after_fullscreen_change(
            WindowLevel::Normal,
            Some(windowed),
            fullscreen,
        ));
        assert!(!should_restore_level_after_fullscreen_change(
            WindowLevel::AlwaysOnTop,
            Some(fullscreen),
            fullscreen,
        ));
    }
}

//! Maps Runtime [`DockWorkspaceEvent`] to host window commands.
//!
//! Floating dock commands and pointer interactions use this one host boundary.
//! This helper does not build a widget tree or a second GPU context.

use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{WindowDescriptor, WindowId, WindowRole};
use nana_ui_runtime::{DockFloatingSurface, DockWorkspaceEvent, dock_surface_window_key};

use crate::runtime_host::{RuntimeProgramUpdate, RuntimeRedraw};

const FLOATING_MIN_WIDTH: f64 = 160.0;
const FLOATING_MIN_HEIGHT: f64 = 120.0;

/// Deterministic [`WindowId`] for a Runtime dock surface.
pub fn dock_workspace_window_id(surface: &str) -> WindowId {
    WindowId(dock_surface_window_key(surface))
}

/// Convert Runtime floating-dock events into [`RuntimeProgramUpdate`] window commands.
pub fn runtime_dock_window_update(
    effects: impl IntoIterator<Item = DockWorkspaceEvent>,
    title: &str,
) -> RuntimeProgramUpdate {
    let collected: Vec<DockWorkspaceEvent> = effects.into_iter().collect();
    let redraw = redraw_for_runtime_dock_effects(&collected);
    let window_commands = collected
        .into_iter()
        .map(|effect| match effect {
            DockWorkspaceEvent::OpenFloating(surface) => WindowCommand::Open {
                id: dock_workspace_window_id(&surface.id),
                settings: floating_window_settings(title, &surface),
            },
            DockWorkspaceEvent::CloseFloating(id) => {
                WindowCommand::Close(dock_workspace_window_id(&id))
            }
            DockWorkspaceEvent::MoveFloating {
                id,
                x,
                y,
                width,
                height,
            } => WindowCommand::SetBounds {
                id: dock_workspace_window_id(&id),
                position: (x, y),
                size: (width, height),
            },
            DockWorkspaceEvent::FocusFloating(id) => {
                WindowCommand::Focus(dock_workspace_window_id(&id))
            }
        })
        .collect();
    RuntimeProgramUpdate {
        redraw,
        window_commands,
        exit: false,
    }
}

fn redraw_for_runtime_dock_effects(effects: &[DockWorkspaceEvent]) -> RuntimeRedraw {
    let mut redraw = RuntimeRedraw::None;
    for effect in effects {
        let next = match effect {
            DockWorkspaceEvent::OpenFloating(_) | DockWorkspaceEvent::CloseFloating(_) => {
                RuntimeRedraw::All
            }
            DockWorkspaceEvent::MoveFloating { id, .. } | DockWorkspaceEvent::FocusFloating(id) => {
                RuntimeRedraw::Window(dock_workspace_window_id(id))
            }
        };
        redraw = match (redraw, next) {
            (RuntimeRedraw::All, _) | (_, RuntimeRedraw::All) => RuntimeRedraw::All,
            (RuntimeRedraw::None, value) | (value, RuntimeRedraw::None) => value,
            (RuntimeRedraw::Window(left), RuntimeRedraw::Window(right)) if left == right => {
                RuntimeRedraw::Window(left)
            }
            _ => RuntimeRedraw::All,
        };
    }
    redraw
}

fn floating_window_settings(title: &str, surface: &DockFloatingSurface) -> WindowDescriptor {
    WindowDescriptor {
        title: title.to_string(),
        initial_size: (f64::from(surface.width), f64::from(surface.height)),
        minimum_size: (FLOATING_MIN_WIDTH, FLOATING_MIN_HEIGHT),
        initial_position: Some((f64::from(surface.x), f64::from(surface.y))),
        role: WindowRole::Tool,
        // A floating dock panel is an ordinary opaque window; it has no reason
        // to take a compositor visual.
        surface: nana_ui_platform::WindowSurfacePreference::NativeWindow,
        ..WindowDescriptor::default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nana_ui_runtime::DockNode;

    use super::*;

    #[test]
    fn open_floating_maps_to_one_window_open_command() {
        let surface = DockFloatingSurface {
            id: Arc::from("1"),
            root: DockNode::item("console", None),
            x: 40.0,
            y: 50.0,
            width: 360.0,
            height: 280.0,
        };
        let update =
            runtime_dock_window_update([DockWorkspaceEvent::OpenFloating(surface)], "NanaUI Dock");
        assert_eq!(update.redraw, RuntimeRedraw::All);
        assert_eq!(update.window_commands.len(), 1);
        let WindowCommand::Open { id, settings } = &update.window_commands[0] else {
            panic!("open command");
        };
        assert_eq!(*id, WindowId(1));
        assert_eq!(settings.title, "NanaUI Dock");
        assert_eq!(settings.role, WindowRole::Tool);
        assert_eq!(settings.initial_position, Some((40.0, 50.0)));
        assert_eq!(settings.initial_size, (360.0, 280.0));
        assert_eq!(settings.minimum_size, (160.0, 120.0));
        assert!(!settings.system_caption);
    }

    #[test]
    fn close_move_and_focus_map_to_matching_window_commands() {
        let update = runtime_dock_window_update(
            [
                DockWorkspaceEvent::MoveFloating {
                    id: Arc::from("2"),
                    x: 80.0,
                    y: 90.0,
                    width: 360.0,
                    height: 280.0,
                },
                DockWorkspaceEvent::FocusFloating(Arc::from("2")),
                DockWorkspaceEvent::CloseFloating(Arc::from("2")),
            ],
            "NanaUI Dock",
        );
        assert_eq!(
            update.window_commands,
            [
                WindowCommand::SetBounds {
                    id: WindowId(2),
                    position: (80.0, 90.0),
                    size: (360.0, 280.0),
                },
                WindowCommand::Focus(WindowId(2)),
                WindowCommand::Close(WindowId(2)),
            ]
        );
    }
}

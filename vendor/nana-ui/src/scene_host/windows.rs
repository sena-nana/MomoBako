//! Scene host windows coordination.

use super::*;
use nana_diagnostics::{framework::window as diagnostics_window, metric};

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn uses_host_managed_drag(settings: &WindowDescriptor, button: i16) -> bool {
    settings.host_managed_drag || button != PRIMARY_MOUSE_BUTTON
}

impl<Program: RuntimeProgram> WindowManager<Program> {
    pub(super) fn apply_window_command(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        command: WindowCommand,
    ) {
        let known = self.known_window_ids();
        match route_window_command(&command, &known) {
            RoutedWindowCommand::SetMousePassthrough(id, mode) => {
                let _ = self.set_mouse_passthrough_mode(event_loop, id, mode);
            }
            RoutedWindowCommand::SetSkipTaskbar(id, skip) => {
                let _ = self.set_skip_taskbar(event_loop, id, skip);
            }
            RoutedWindowCommand::Ignore => {}
            RoutedWindowCommand::Open(id) => {
                let WindowCommand::Open { settings, .. } = command else {
                    return;
                };
                {
                    let event = self
                        .open_window(event_loop, id, settings)
                        .unwrap_or_else(|error| WindowEvent::OpenFailed { id, error });
                    let update = self.program.window_event(event, &self.context_for(id));
                    self.program
                        .sync_animation_clock(self.animation_clock.epoch());
                    self.apply_update(event_loop, update, None);
                    self.finish_ready(event_loop, id);
                }
            }
            RoutedWindowCommand::Focus(id) => self.focus_window(id),
            RoutedWindowCommand::Close(id) => self.close_window(event_loop, id),
            RoutedWindowCommand::SetTitle(id) => {
                let WindowCommand::SetTitle { title, .. } = command else {
                    return;
                };
                if let Some(window) = self.window(id) {
                    window.set_title(&title);
                }
            }
            RoutedWindowCommand::Move(id) => {
                let WindowCommand::Move { position, .. } = command else {
                    return;
                };
                self.move_window(id, position);
            }
            RoutedWindowCommand::SetBounds(id) => {
                let WindowCommand::SetBounds { position, size, .. } = command else {
                    return;
                };
                self.set_window_bounds(id, position, size);
            }
            RoutedWindowCommand::SetFullscreen(id) => {
                let WindowCommand::SetFullscreen { fullscreen, .. } = command else {
                    return;
                };
                // Batched commands have no reply; the outcome is `ModeChanged`.
                let _ = self.set_window_fullscreen(event_loop, id, fullscreen);
            }
            RoutedWindowCommand::SetMinimized(id) => {
                let WindowCommand::SetMinimized { minimized, .. } = command else {
                    return;
                };
                self.mutate_native_style(id, |window| window.set_minimized(minimized));
            }
            RoutedWindowCommand::SetMaximized(id) => {
                let WindowCommand::SetMaximized { maximized, .. } = command else {
                    return;
                };
                if self
                    .mutate_native_style(id, |window| window.set_maximized(maximized))
                    .is_none()
                {
                    return;
                }
                self.resize_window(id);
                // The cached geometry is now current, so the native resize that
                // follows will not request this frame itself.
                if self.sync_geometry(id) {
                    self.request_redraw(id);
                }
                let update = self.program.window_event(
                    WindowEvent::Resized {
                        id,
                        geometry: self.geometry_of(id),
                    },
                    &self.context_for(id),
                );
                self.apply_update(event_loop, update, None);
            }
            RoutedWindowCommand::SetAlwaysOnTop(id) => {
                let WindowCommand::SetAlwaysOnTop { always_on_top, .. } = command else {
                    return;
                };
                let level = if always_on_top {
                    WindowLevel::AlwaysOnTop
                } else {
                    WindowLevel::Normal
                };
                self.set_window_level(event_loop, id, level);
            }
            RoutedWindowCommand::SetNativeWindowControlsVisible(id) => {
                let WindowCommand::SetNativeWindowControlsVisible {
                    visible, duration, ..
                } = command
                else {
                    return;
                };
                let Some(host) = self.window_contexts.get_mut(&id) else {
                    return;
                };
                host.native_controls_visible = visible;
                let _ = nana_window::set_native_window_controls_visible(
                    host.surface.window().as_ref(),
                    visible,
                    duration,
                );
                // Showing or hiding them lays the titlebar out again, which
                // puts them back on the system spot.
                self.reapply_native_window_controls(id);
            }
            RoutedWindowCommand::SetIcon(id) => {
                let WindowCommand::SetIcon { icon, .. } = command else {
                    return;
                };
                if id == WindowId::PRIMARY {
                    // The program's icon wins over one the startup is still
                    // rendering from the descriptor.
                    self.pending_icons = None;
                }
                if let Some(window) = self.window(id) {
                    apply_scene_window_icon(
                        window.as_ref(),
                        icon.as_ref(),
                        id == WindowId::PRIMARY,
                    );
                }
            }
            RoutedWindowCommand::SetMenuBar(id) => {
                let WindowCommand::SetMenuBar { bar, .. } = command else {
                    return;
                };
                if let Some(window) = self.window(id) {
                    // The host owns the handle; the application only described
                    // the menu. An empty bar removes it.
                    let bar = bar.clone().unwrap_or_default();
                    nana_window::install_menu_bar(window.as_ref(), &bar);
                }
            }
            RoutedWindowCommand::OpenFileDialog(id) => {
                let WindowCommand::OpenFileDialog { request, .. } = command else {
                    return;
                };
                self.open_file_dialog(event_loop, id, request);
            }
            RoutedWindowCommand::SetApplicationIcon => {
                let WindowCommand::SetApplicationIcon { icon } = command else {
                    return;
                };
                match icon {
                    Some(icon) => register_application_icon(icon),
                    None => clear_registered_application_icon(),
                }
                self.pending_icons = None;
                for id in self.known_window_ids() {
                    if let Some(window) = self.window(id) {
                        apply_scene_window_icon(window.as_ref(), None, id == WindowId::PRIMARY);
                    }
                }
                apply_application_icon();
            }
            RoutedWindowCommand::Drag(id) => {
                let _ = self.begin_window_move(event_loop, id);
            }
            RoutedWindowCommand::SetShadow(id) => {
                let WindowCommand::SetShadow { shadow, .. } = command else {
                    return;
                };
                if let Some(host) = self.window_contexts.get_mut(&id) {
                    host.settings.shadow = shadow;
                }
                self.apply_window_shadow(id);
            }
        }
    }

    /// Apply the window's shadow and record what the platform applied on
    /// its presentation. Run once the window is shown, and again when its
    /// request, material or presentation changes.
    pub(super) fn apply_window_shadow(&mut self, id: WindowId) {
        let held = self.startup_presentation_held(id);
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return;
        };
        let window = Arc::clone(host.surface.window());
        let transparent =
            host.presentation.effective().effect == crate::MaterialEffect::Transparent;
        let mut intent = crate::window_shadow::intent(&host.settings, transparent);
        let body = host.shadow_body.map_or_else(
            || {
                nana_ui_platform::WindowVisualShape::client(
                    host.geometry.logical_size.0.max(1.0),
                    host.geometry.logical_size.1.max(1.0),
                )
            },
            |body| body.shape,
        );
        let plan = crate::window_shadow::plan(intent, body)
            .unwrap_or(nana_window::shadow::ShadowPlan::Disable);
        let mut applied = host.shadow.apply(window.as_ref(), plan);
        // A window without a platform frame (frameless on Windows) has no
        // shadow of its own; the default companion stands in for it.
        if applied == nana_window::shadow::ShadowApplied::Unsupported
            && intent == crate::window_shadow::ShadowIntent::Native
        {
            intent = crate::window_shadow::ShadowIntent::Companion(Default::default());
            if let Some(plan) = crate::window_shadow::plan(intent, body) {
                applied = host.shadow.apply(window.as_ref(), plan);
            }
        }
        if host.shadow.has_companion() {
            let visible = !held && window.fullscreen().is_none();
            host.shadow
                .set_shape(window.as_ref(), crate::window_shadow::shape(body), visible);
        }
        host.presentation
            .record_shadow(crate::window_shadow::outcome(intent, applied));
    }

    /// Follow the window's visible body with its shadow companion. The scene
    /// projection is checked because a compositor tick can move the body
    /// without a resize. A tick that leaves the derived body where it was
    /// only refreshes that check: the companion is not repositioned. Moving
    /// it anyway (a pointer halo does this on every sample) reorders the
    /// companion against a composition window, and DWM then shows that
    /// window empty for a frame.
    pub(super) fn sync_shadow_body(&mut self, id: WindowId, scene: &nana_ui_scene::UiScene) {
        let held = self.startup_presentation_held(id);
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return;
        };
        if !host.shadow.has_companion() {
            return;
        }
        let window = Arc::clone(host.surface.window());
        let revision = scene.projection_revision();
        let size = host.geometry.logical_size;
        let visible = !held && window.fullscreen().is_none();
        if host.shadow_body.is_some_and(|body| {
            body.revision == revision && body.size == size && body.visible == visible
        }) {
            metric!(diagnostics_window::SHADOW_SYNC_SKIPPED);
            return;
        }
        let shape = crate::window_shadow::derive_visual_shape(scene, [size.0, size.1]);
        if let Some(body) = host.shadow_body.as_mut()
            && body.shape == shape
            && body.size == size
            && body.visible == visible
        {
            body.revision = revision;
            metric!(diagnostics_window::SHADOW_SYNC_SKIPPED);
            return;
        }
        host.shadow_body = Some(super::ShadowBody {
            revision,
            size,
            visible,
            shape,
        });
        host.shadow
            .set_shape(window.as_ref(), crate::window_shadow::shape(shape), visible);
        metric!(diagnostics_window::SHADOW_SYNC_APPLIED);
    }
    fn set_mouse_passthrough_mode(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        mode: MousePassthroughMode,
    ) -> Result<(), crate::WindowError> {
        if let Some(host) = self.window_contexts.get_mut(&id) {
            host.passthrough_mode = mode;
        }
        let enabled = matches!(
            mode,
            MousePassthroughMode::Passthrough | MousePassthroughMode::Forward
        );
        self.apply_os_mouse_passthrough(event_loop, id, enabled, true)
    }

    fn apply_os_mouse_passthrough(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        enabled: bool,
        emit_always: bool,
    ) -> Result<(), crate::WindowError> {
        if !emit_always
            && self
                .window_contexts
                .get(&id)
                .is_some_and(|host| host.os_mouse_passthrough == enabled)
        {
            return Ok(());
        }
        let result = self
            .mutate_native_style(id, |window| {
                window
                    .set_cursor_hittest(!enabled)
                    .map_err(window_request_error)
            })
            .unwrap_or(Err(crate::WindowError::WindowClosed));
        if result.is_ok()
            && let Some(host) = self.window_contexts.get_mut(&id)
        {
            host.os_mouse_passthrough = enabled;
        }
        let update = self.program.window_event(
            WindowEvent::MousePassthroughChanged {
                id,
                enabled,
                result: result.as_ref().copied().map_err(ToString::to_string),
            },
            &self.context_for(id),
        );
        self.apply_update(event_loop, update, None);
        result
    }

    /// After `Ready`: deliver the descriptor's taskbar outcome, then the initial
    /// mode. A failed taskbar request does not undo creation.
    pub(super) fn finish_ready(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId) {
        if let Some(result) = self
            .window_contexts
            .get_mut(&id)
            .and_then(|host| host.skip_taskbar_report.take())
        {
            self.emit_skip_taskbar_changed(event_loop, id, &result);
        }
        self.sync_window_mode(event_loop, id);
        self.apply_window_shadow(id);
    }

    /// Always emits `SkipTaskbarChanged`, including for a missing window.
    pub(super) fn set_skip_taskbar(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        skip: bool,
    ) -> Result<(), crate::WindowError> {
        let result = self
            .window(id)
            .map(|window| native_skip_taskbar(window.as_ref(), skip))
            .unwrap_or(Err(crate::WindowError::WindowClosed));
        if let Some(host) = self.window_contexts.get_mut(&id) {
            // An explicit request supersedes a descriptor outcome not yet delivered.
            host.skip_taskbar_report = None;
            if result.is_ok() {
                host.skip_taskbar = skip;
            }
        }
        self.emit_skip_taskbar_changed(event_loop, id, &result);
        result
    }

    fn emit_skip_taskbar_changed(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        result: &Result<(), crate::WindowError>,
    ) {
        let skip_taskbar = self
            .window_contexts
            .get(&id)
            .is_some_and(|host| host.skip_taskbar);
        let update = self.program.window_event(
            WindowEvent::SkipTaskbarChanged {
                id,
                skip_taskbar,
                result: result.clone().map_err(|error| error.to_string()),
            },
            &self.context_for(id),
        );
        self.apply_update(event_loop, update, None);
    }

    fn forward_hits_content(&mut self, id: WindowId) -> bool {
        let cursor = self
            .window_contexts
            .get(&id)
            .map(|host| host.input.cursor)
            .unwrap_or((0.0, 0.0));
        if self.frame_resize_edge_at(id, cursor.0, cursor.1).is_some() {
            return true;
        }
        self.program
            .read_document(id, |document| {
                document
                    .context()
                    .pointer_target(document.document(), cursor.0, cursor.1)
                    .is_some()
            })
            .unwrap_or(false)
    }

    fn sample_client_pointer(&self, id: WindowId) -> Option<(f32, f32)> {
        let window = self.window(id)?;
        let geometry = self.geometry_of(id);
        nana_window::pointer_in_client_area(
            window.as_ref(),
            f64::from(geometry.scale_factor),
            geometry.logical_size,
        )
    }

    pub(super) fn passthrough_forward_wakeup(&self) -> Option<Instant> {
        self.window_contexts
            .values()
            .any(|host| {
                host.passthrough_mode == MousePassthroughMode::Forward && host.os_mouse_passthrough
            })
            .then(|| Instant::now() + Duration::from_millis(8))
    }

    pub(super) fn sample_passthrough_forward(&mut self, event_loop: &dyn ActiveEventLoop) {
        let ids: Vec<_> = self
            .window_contexts
            .iter()
            .filter(|(id, host)| {
                host.passthrough_mode == MousePassthroughMode::Forward
                    && host.os_mouse_passthrough
                    // The sample only asks where the cursor is, not what is
                    // on top there; it would bring an occluded window back.
                    && !self.occluded.contains(*id)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if !self.window_contexts.contains_key(&id) {
                continue;
            }
            let Some(point) = self.sample_client_pointer(id) else {
                self.observe_pointer_presence(event_loop, id, presence::PresenceSignal::Left);
                continue;
            };
            self.observe_pointer_presence(event_loop, id, presence::PresenceSignal::Inside);
            if !self.window_contexts.contains_key(&id) {
                continue;
            }
            self.input_mut(id).cursor = point;
            if self.forward_hits_content(id) {
                let _ = self.apply_os_mouse_passthrough(event_loop, id, false, false);
                if !self.window_contexts.contains_key(&id) {
                    continue;
                }
                self.dispatch_forward_move(event_loop, id, point);
            }
        }
    }

    fn dispatch_forward_move(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        point: (f32, f32),
    ) {
        self.input_mut(id).cursor = point;
        let origin = self
            .window(id)
            .and_then(|window| window_screen_origin(window.as_ref()));
        let modifiers = platform_input_modifiers(self.input_of(id).modifiers);
        let buttons = self.input_of(id).buttons;
        let pointer = self.input_of(id).pointer_event(
            mapped_pointer(1, PointerType::Mouse, true, None),
            PointerPhase::Move,
            -1,
            buttons,
            false,
            modifiers,
            origin,
            None,
        );
        let _ = self.deliver_host_input(event_loop, id, InputPayload::Pointer(pointer));
    }

    fn dispatch_forward_leave(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId) {
        self.dispatch_pointer_cancel(event_loop, id);
        self.reset_window_cursor(id);
    }

    /// Cancels the mouse gesture when the platform takes its release away (a
    /// native window drag or forward passthrough), so the tracker buttons,
    /// runtime press/capture, title-bar drag and the program all see it end.
    fn dispatch_pointer_cancel(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId) {
        let origin = self
            .window(id)
            .and_then(|window| window_screen_origin(window.as_ref()));
        let pointer = self.input_mut(id).cancel_mouse(origin);
        let _ = self.deliver_host_input(event_loop, id, InputPayload::Pointer(pointer));
    }

    pub(super) fn forward_os_passthrough_ignores_pointer(
        &self,
        id: WindowId,
        event: &WinitWindowEvent,
    ) -> bool {
        let Some(host) = self.window_contexts.get(&id) else {
            return false;
        };
        forward_pointer_action(
            host.passthrough_mode,
            host.os_mouse_passthrough,
            false,
            event,
        ) == ForwardPointerAction::IgnoreUntilRecovered
    }

    pub(super) fn forward_pointer_action_for(
        &mut self,
        id: WindowId,
        event: &WinitWindowEvent,
    ) -> ForwardPointerAction {
        let (mode, os_passthrough) = match self.window_contexts.get(&id) {
            Some(host) => (host.passthrough_mode, host.os_mouse_passthrough),
            None => return ForwardPointerAction::Dispatch,
        };
        let hits_content = mode == MousePassthroughMode::Forward
            && !os_passthrough
            && forward_pointer_event(event)
            && self.forward_hits_content(id);
        forward_pointer_action(mode, os_passthrough, hits_content, event)
    }

    pub(super) fn restore_forward_passthrough(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
    ) {
        self.dispatch_forward_leave(event_loop, id);
        if self.window_contexts.contains_key(&id) {
            let _ = self.apply_os_mouse_passthrough(event_loop, id, true, false);
        }
    }

    pub(super) fn open_window(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        settings: WindowDescriptor,
    ) -> Result<WindowEvent, String> {
        if self.shutting_down {
            return Err("window host has stopped".into());
        }
        if self.render_suspended {
            return Err("shared GPU recovery pending".into());
        }
        if self.window(id).is_some() {
            return Err("window identity is already live".into());
        }
        crate::window_service::validate_descriptor(&settings).map_err(|error| error.to_string())?;
        let mut settings = settings;
        let restoration = nana_ui_core::ViewStateStore::new(
            self.store.clone(),
            settings.restoration_scope.clone(),
        );
        restore_window_geometry(&mut settings, &restoration);
        if let Some(parent) = settings.parent
            && (self.window(parent).is_none() || self.closing_windows.contains(&parent))
        {
            return Err("parent window does not exist".into());
        }
        if settings.modal {
            let parent = settings
                .parent
                .ok_or_else(|| "modal window requires a parent".to_string())?;
            if self.window(parent).is_none() {
                return Err(format!("modal parent window {} does not exist", parent.0));
            }
            if self.active_modal_child(parent).is_some() {
                return Err(format!(
                    "modal parent window {} is already blocked",
                    parent.0
                ));
            }
        }
        let parent = settings
            .parent
            .and_then(|parent| self.window(parent).cloned());
        // This window's own target, from its own descriptor. A Settings
        // window, a dialog or a popup stays on the platform's window surface
        // even in a process whose main window is composed; the shared GPU
        // device does not care which surface target each window uses.
        let surface_target = crate::presentation::resolve_window_surface_target(
            crate::presentation::window_surface_request(
                settings.surface,
                window_wants_transparent_surface(
                    settings.transparent,
                    self.program.window_material_mode_for(id),
                ),
                self.gpu_backend_policy,
            ),
            settings.surface.requires_composition(),
            self.composition,
        );
        if let Some(reason) = surface_target.forbidden_fallback() {
            // This window asked to fail rather than present another way, and
            // failing to open is reported to the application as such.
            return Err(format!(
                "window requires a platform compositor surface: {}",
                reason.label()
            ));
        }
        let desktop = scene_desktop(event_loop, settings.constrain_to_work_area);
        let backdrop_opacity = self.program.appearance_backdrop_opacity_for(id);
        let material_mode = self.program.window_material_mode_for(id);
        let window_background = self.program.window_background();
        // A composed window is provisional here for the same reason the first
        // window is: `WS_EX_NOREDIRECTIONBITMAP` is decided at creation, and
        // every step that could reject the composition target comes after it.
        // A failed attempt drops its window and retries on the plain path
        // rather than opening nothing.
        let mut attempt = surface_target;
        let (window, surface, requested_material, applied) = loop {
            let attributes = scene_aux_window_attributes(
                &settings,
                parent.as_deref(),
                &desktop,
                attempt.resolved,
            )?;
            let window: Arc<dyn winit::window::Window> = Arc::from(
                event_loop
                    .create_window(attributes.with_visible(false))
                    .map_err(|error| error.to_string())?,
            );
            // Owns the window for the length of the attempt: an early exit
            // drops it with the local below, destroying the HWND created for a
            // target that did not work out.
            let mut provisional = PendingNativeWindow(Some(window.clone()));
            apply_scene_window_icon(
                window.as_ref(),
                settings.icon.as_ref(),
                id == WindowId::PRIMARY,
            );
            let (requested_material, applied) = apply_window_material(
                window.as_ref(),
                self.last_theme,
                &settings,
                material_mode,
                backdrop_opacity,
                window_background,
            );
            match self.graphics.create_surface_with_mode(
                Arc::clone(&window),
                requested_material.wants_transparent_surface(),
                super::surface_mode_for(attempt.resolved),
            ) {
                Ok(surface) => {
                    provisional.0 = None;
                    break (window, surface, requested_material, applied);
                }
                Err(error) => match super::next_bootstrap_attempt(attempt) {
                    Some(next) => {
                        // A composed target that could not be built for this
                        // window will not build for the next one either, so
                        // the failure narrows the whole process and no later
                        // window pays for a doomed provisional HWND.
                        self.composition =
                            crate::presentation::CompositionAvailability::Unavailable;
                        attempt = next;
                    }
                    None => return Err(error.to_string()),
                },
            }
        };
        let surface_target = attempt;
        let mut pending_native = PendingNativeWindow(Some(window.clone()));
        // The surface has answered, so this window's effective material and the
        // chrome that matches it are settled together, before it is shown.
        let presentation = ResolvedWindowPresentation::resolve(
            &settings,
            requested_material,
            applied,
            surface.wgpu_alpha_mode(),
            self.graphics.adapter_info().backend,
            surface_target,
            self.non_client,
        );
        self.note_native_chrome_write();
        apply_resolved_presentation(
            window.as_ref(),
            self.last_theme,
            &settings,
            &presentation,
            backdrop_opacity,
            window_background,
            true,
        );
        let format = surface.format();
        let _ = self.painter_mut(format);
        #[cfg(not(target_os = "android"))]
        let accessibility = {
            Some(HostedAccessibility::new(
                Arc::clone(&window),
                true,
                window.scale_factor() as f32,
            ))
        };
        let geometry = window_geometry(window.as_ref());
        #[cfg(target_os = "windows")]
        let modal_parent = settings.modal.then_some(settings.parent).flatten();
        let size_move = LiveSizeMove::install(window.as_ref())?;
        self.windows.register(id);
        let context = program_context(
            self.message_tx.clone(),
            Arc::clone(&self.host_work),
            &self.graphics,
            id,
            geometry,
            self.tasks.clone(),
            presentation,
            self.composition_work_of(id),
            window.theme().map(system_appearance_from_winit),
            &self.startup.handle,
        )
        .with_windows(&self.windows)
        .with_window_tag(settings.tag.clone())
        .with_reduced_motion(self.reduced_motion)
        .with_store(Arc::clone(&self.program_store));
        if let Err(error) = self.program.initialize_window(id, &context) {
            self.windows.unregister(id);
            self.program.discard_window(id);
            // PendingNativeWindow owns platform cleanup for every failed stage.
            return Err(error);
        }
        pending_native.0 = None;
        self.window_ids.insert(window.id(), id);
        let level = if settings.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        };
        let pending_fullscreen = settings.fullscreen;
        let skip_taskbar_report = descriptor_skip_taskbar(window.as_ref(), &settings);
        let generation = *self.input_generations.entry(id).or_insert(1);
        self.window_contexts.insert(
            id,
            WindowContext {
                surface_retry: None,
                applied_appearance: None,
                cursor_override: None,
                cursor_visible_override: None,
                passthrough_mode: MousePassthroughMode::Off,
                os_mouse_passthrough: false,
                material_override: None,
                surface,
                geometry,
                input: InputTracker::default(),
                input_source: WindowInputSource::new(
                    nana_ui_platform::InputSourceId(id.0),
                    nana_ui_platform::EndpointGeneration(generation),
                ),
                presentation,
                settings,
                #[cfg(not(target_os = "android"))]
                accessibility,
                accessibility_pending: PendingAccessibility::default(),
                size_move,
                level,
                mode: None,
                pending_fullscreen,
                native_controls_visible: true,
                native_controls: None,
                native_controls_box: std::cell::Cell::new(None),
                skip_taskbar: matches!(skip_taskbar_report, Some(Ok(()))),
                skip_taskbar_report,
                pointer_presence: presence::PointerPresence::default(),
                shadow: nana_window::shadow::WindowShadowState::default(),
                shadow_body: None,
            },
        );
        #[cfg(target_os = "windows")]
        if let Some(parent) = modal_parent.and_then(|parent| self.window(parent)) {
            parent.set_enable(false);
        }
        let settings = self.settings_of(id);
        let (visible, focus_on_show) = (settings.visible, settings.focus_on_show);
        self.mutate_window_visibility(id, |window| {
            set_native_visible(window, visible, focus_on_show)
        });
        window.request_redraw();
        self.prepare_window_chrome(id, geometry.maximized);
        crate::host_diagnostics::window_opened(id, &geometry);
        Ok(WindowEvent::Ready { id, geometry })
    }
    pub(super) fn close_window(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId) {
        if !self.window_contexts.contains_key(&id) || !self.closing_windows.insert(id) {
            return;
        }
        let generation = self.input_generations.entry(id).or_insert(1);
        *generation = generation.saturating_add(1).max(1);
        let source = nana_ui_platform::InputSourceId(id.0);
        let now = self.animation_clock.runtime_time(Instant::now());
        // Presses and captures the window held are cancelled while its
        // document is alive; a reused WindowId binds a newer generation.
        self.program.write_document(id, |document| {
            let _ = document.context_mut().unbind_input_source(source, now);
        });
        self.windows.unregister(id);
        if id == WindowId::PRIMARY {
            self.release_startup_splash();
        }
        let children: Vec<_> = self
            .window_contexts
            .iter()
            .filter_map(|(&child, host)| (host.settings.parent == Some(id)).then_some(child))
            .collect();
        for child in children {
            self.close_window(event_loop, child);
        }
        if let Some(window) = self.window(id) {
            window.set_visible(false);
        }
        self.browsers.retain(|(window, _), _| *window != id);
        self.close_file_dialog(event_loop, id);
        self.chrome.remove(&id);
        self.native_content.remove(&id);
        self.bind_after_present.remove(&id);
        #[cfg(target_os = "macos")]
        self.present_transaction_pinned.remove(&id);
        self.frame_schedules.remove(&id);
        self.texture_subscriptions.remove(&id);
        if let Ok(mut targets) = self.image_targets.lock() {
            remove_image_target_index(&mut targets, &mut self.image_window_keys, id);
        } else {
            self.image_window_keys.remove(&id);
        }
        self.occluded.remove(&id);
        for painter in self.painters.values_mut() {
            painter.remove_target(crate::RenderTargetId(id.0));
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some((_, live)) = self
            .live_frame_resize
            .take_if(|(session, _)| *session == id)
            && let Some(window) = self.window(id)
        {
            live.end(window.as_ref());
        }
        // A window that goes away takes its move with it; the gesture has no
        // document left to tell.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if self
            .live_frame_move
            .is_some_and(|(session, _, _)| session == id)
        {
            self.live_frame_move = None;
        }
        // Cleared before `Closed` runs, so a window that callback reopens with the
        // same identity is not treated as closing.
        self.closing_windows.remove(&id);
        if let Some(host) = self.window_contexts.remove(&id) {
            #[cfg(target_os = "windows")]
            if let Some(parent_id) = host
                .settings
                .modal
                .then_some(host.settings.parent)
                .flatten()
                && !self.closing_windows.contains(&parent_id)
                // A modal opened while this one was closing still blocks the parent.
                && self.active_modal_child(parent_id).is_none()
                && let Some(parent) = self.window(parent_id)
            {
                parent.set_enable(true);
                self.focus_window(parent_id);
            }
            self.window_ids.remove(&host.surface.window().id());
            drop(host);
            nana_diagnostics::event!(nana_diagnostics::framework::window::CLOSED, window = id.0);
            let update = self
                .program
                .window_event(WindowEvent::Closed { id }, &self.context_for(id));
            self.apply_update(event_loop, update, None);
        }
        if self.window_contexts.is_empty() && !self.embedded {
            event_loop.exit();
        }
    }
    pub(super) fn focus_window(&self, mut id: WindowId) {
        while let Some(modal) = self.active_modal_child(id) {
            id = modal;
        }
        // Raising a window is not a style change; only the show is, and a
        // window that is already visible does not even get that far.
        self.mutate_window_visibility(id, |window| window.set_visible(true));
        if let Some(window) = self.window(id) {
            window.focus_window();
        }
    }
    pub(super) fn move_window(&self, id: WindowId, position: (f32, f32)) {
        self.mutate_window_geometry(id, |window| move_to_desktop_position(window, position));
    }
    pub(super) fn set_window_bounds(&self, id: WindowId, position: (f32, f32), size: (f32, f32)) {
        self.mutate_window_geometry(id, |window| {
            move_to_desktop_position(window, position);
            let _ = window.request_surface_size(winit::dpi::Size::Logical(
                winit::dpi::LogicalSize::new(
                    f64::from(size.0.max(1.0)),
                    f64::from(size.1.max(1.0)),
                ),
            ));
        });
    }
    pub(super) fn active_modal_child(&self, parent: WindowId) -> Option<WindowId> {
        self.window_contexts.iter().find_map(|(id, host)| {
            (host.settings.modal
                && host.settings.parent == Some(parent)
                && !self.closing_windows.contains(id))
            .then_some(*id)
        })
    }
    pub(super) fn sync_appearance(&mut self) {
        self.last_theme = self.program.theme_mode();
        if self.render_suspended {
            return;
        }
        for id in self.known_window_ids() {
            if self.window_contexts[&id].surface_retry.is_some() {
                continue;
            }
            if let Err(error) = self.sync_window_material(id) {
                self.suspend_surface(id, error);
            }
        }
    }

    pub(super) fn sync_window_material(&mut self, id: WindowId) -> Result<(), HostedGpuError> {
        let material_override = self
            .window_contexts
            .get(&id)
            .and_then(|host| host.material_override);
        self.apply_window_material(id, material_override)
    }

    fn apply_window_material(
        &mut self,
        id: WindowId,
        material_override: Option<nana_window::MaterialEffect>,
    ) -> Result<(), HostedGpuError> {
        let mode = self.program.window_material_mode_for(id);
        let desired = WindowAppearance {
            theme: self.program.theme_mode(),
            material: material_override.unwrap_or(mode),
            opacity: AppearanceSettings::clamp_backdrop_opacity(
                self.program.appearance_backdrop_opacity_for(id),
            ),
            background: self.program.window_background(),
        };
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return Ok(());
        };
        // A material change never re-negotiates the presentation target; it
        // carries the verdict this window already reached.
        let surface_target = host.presentation.resolved_target();
        let non_client = self.non_client;
        let chrome_writes = &self.native_chrome_writes;
        let resolved = apply_changed_appearance(&mut host.applied_appearance, desired, || {
            clear_system_material(host.surface.window().as_ref());
            let (requested, applied) = apply_window_material(
                host.surface.window().as_ref(),
                desired.theme,
                &host.settings,
                desired.material,
                desired.opacity,
                desired.background,
            );
            self.graphics.apply_surface_alpha_mode(
                &mut host.surface,
                window_wants_transparent_surface(host.settings.transparent, desired.material),
            )?;
            let presentation = ResolvedWindowPresentation::resolve(
                &host.settings,
                requested,
                applied,
                host.surface.wgpu_alpha_mode(),
                self.graphics.adapter_info().backend,
                surface_target,
                non_client,
            );
            // Chrome follows this presentation in the same step, so a request
            // the surface demoted cannot leave transparent chrome behind.
            chrome_writes.set(chrome_writes.get().saturating_add(1));
            apply_resolved_presentation(
                host.surface.window().as_ref(),
                desired.theme,
                &host.settings,
                &presentation,
                desired.opacity,
                desired.background,
                true,
            );
            Ok(presentation)
        })?;
        if let Some(presentation) = resolved {
            host.presentation = presentation;
            // A new presentation starts with nothing recorded, and the
            // effective material decides between native and companion.
            self.apply_window_shadow(id);
            self.request_redraw(id);
        }
        Ok(())
    }

    /// GPU replacement requires reapplying effects even when the request is unchanged.
    pub(super) fn refresh_material(&mut self) {
        for host in self.window_contexts.values_mut() {
            host.applied_appearance = None;
        }
        self.sync_appearance();
    }
    /// Refreshes the cached window geometry from the live window state and
    /// reports whether it moved.
    pub(super) fn sync_geometry(&mut self, id: WindowId) -> bool {
        let previous = self.geometry_of(id);
        if let Some(host) = self.window_contexts.get_mut(&id) {
            host.geometry = window_geometry(host.surface.window().as_ref());
        }
        let changed = self.geometry_of(id) != previous;
        let maximized = self.geometry_of(id).maximized;
        if let Some(session) = self.chrome.get_mut(&id) {
            session.state.update(WindowChromeEvent::MaximizedChanged {
                window: id,
                maximized,
            });
        }
        // Title bars mirror only maximize transitions; moves and plain resizes
        // leave them alone, and optimistic chrome toggles are not overwritten.
        if maximized != previous.maximized {
            self.sync_title_bar_maximized(id, maximized);
        }
        if changed {
            self.persist_geometry(id);
        }
        changed
    }

    fn persist_geometry(&self, id: WindowId) {
        let Some(host) = self.window_contexts.get(&id) else {
            return;
        };
        let fullscreen = host
            .mode
            .as_ref()
            .is_some_and(|mode| mode.fullscreen.is_some());
        let minimized = host.surface.window().is_minimized() == Some(true);
        let _ = persist_live_window_geometry(
            &nana_ui_core::ViewStateStore::new(
                self.store.clone(),
                host.settings.restoration_scope.clone(),
            ),
            &host.settings,
            &host.geometry,
            fullscreen,
            minimized,
        );
    }
    /// Pins transaction presents while the OS's own frame-resize gesture is
    /// moving the window, and reports whether that gesture is active.
    #[cfg(target_os = "macos")]
    pub(super) fn sync_native_live_resize_presents(&mut self, id: WindowId) -> bool {
        let active = self
            .window(id)
            .is_some_and(|window| nana_window::native_live_resize_active(window.as_ref()));
        if active {
            self.pin_present_transaction(id);
        }
        active
    }
    #[cfg(target_os = "macos")]
    pub(super) fn pin_present_transaction(&mut self, id: WindowId) {
        if self.present_transaction_pinned.contains(&id) {
            return;
        }
        if let Some(window) = self.window(id)
            && nana_window::set_present_transaction(window.as_ref(), true)
        {
            self.present_transaction_pinned.insert(id);
        }
    }
    /// Releases transaction presents once their resize gesture is over; the
    /// pinned mode serializes every present with a Core Animation commit and
    /// costs latency in steady-state frames.
    #[cfg(target_os = "macos")]
    pub(super) fn unpin_idle_present_transactions(&mut self) {
        let pinned: Vec<WindowId> = self.present_transaction_pinned.iter().copied().collect();
        for id in pinned {
            let Some(window) = self.window(id) else {
                self.present_transaction_pinned.remove(&id);
                continue;
            };
            if nana_window::native_live_resize_active(window.as_ref()) || self.is_live_resize(id) {
                continue;
            }
            nana_window::set_present_transaction(window.as_ref(), false);
            self.present_transaction_pinned.remove(&id);
        }
    }
    pub(super) fn resize_window(&mut self, id: WindowId) {
        if self.render_suspended {
            return;
        }
        let live = self.is_live_resize(id);
        if let Some(host) = self.window_contexts.get_mut(&id)
            && host.surface_retry.is_none()
        {
            self.graphics.prepare_surface_frame(&mut host.surface, live);
        }
    }
    pub(super) fn is_live_resize(&self, id: WindowId) -> bool {
        if self.size_move_active(id) {
            return true;
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.live_frame_resize
                .as_ref()
                .is_some_and(|(session, _)| *session == id)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        false
    }
    pub(super) fn size_move_active(&self, id: WindowId) -> bool {
        self.window_contexts
            .get(&id)
            .is_some_and(|host| host.size_move.is_active())
    }
    /// Restore the native cursor after the pointer leaves this window. This
    /// must not probe the document: the last in-window target may have had
    /// `cursor:none`, and that state must not leak outside the window.
    pub(super) fn reset_window_cursor(&mut self, id: WindowId) {
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return;
        };
        host.input_source.applied_cursor = Some((CursorIcon::Default, true));
        let window = host.surface.window();
        window.set_cursor_visible(true);
        window.set_cursor(CursorIcon::Default.into());
    }

    /// Starts a window move for `WindowCommand::Drag`, the one signal any
    /// trigger uses to say "move this window with the gesture in flight".
    ///
    /// A gesture held with the primary button keeps the platform's own move
    /// unless the window asks for `host_managed_drag`: the platform path is
    /// the only one with edge snapping, and on macOS with Spaces. Any other
    /// button is followed by the host, because the platform drag assumes a
    /// primary press — AppKit ignores a drag whose current event is not one,
    /// and Win32 opens a caption move loop that only a primary release
    /// closes, leaving the window stuck to the cursor.
    pub(super) fn begin_window_move(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
    ) -> Result<(), winit::error::RequestError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(button) = held_mouse_button(self.input_of(id).buttons)
            && uses_host_managed_drag(self.settings_of(id), button)
        {
            // A gesture the host declines — a fullscreen window has nowhere to
            // move to — reports that, rather than falling back to the platform,
            // which would take a press it cannot end for a caption drag.
            return if self.start_frame_move(id, button) {
                Ok(())
            } else {
                Err(winit::error::RequestError::Ignored)
            };
        }
        let Some(window) = self.window(id).cloned() else {
            return Ok(());
        };
        drag_scene_window(window.as_ref())?;
        self.begin_native_drag_presence(id);
        self.dispatch_pointer_cancel(event_loop, id);
        Ok(())
    }

    /// Captures the window origin so the pointer drives it from here on, and
    /// answers whether the host took the gesture.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn start_frame_move(&mut self, id: WindowId, button: i16) -> bool {
        let Some(window) = self.window(id).cloned() else {
            return false;
        };
        // A fullscreen window has nowhere to move to. A maximized one leaves
        // that state first, as the system move loop does, and the restored
        // frame comes back under the cursor at the same point across its
        // width, rather than at its old restored position.
        if window.fullscreen().is_some() {
            return false;
        }
        let live = if window.is_maximized() {
            let grab = nana_window::LiveFrameMove::grab(window.as_ref());
            window.set_maximized(false);
            match grab {
                Some(grab) => nana_window::LiveFrameMove::begin_at(window.as_ref(), grab),
                None => nana_window::LiveFrameMove::begin(window.as_ref()),
            }
        } else {
            nana_window::LiveFrameMove::begin(window.as_ref())
        };
        let Some(live) = live else {
            return false;
        };
        self.live_frame_move = Some((id, button, live));
        true
    }

    /// Drives a running window move, and reports whether it took the event.
    ///
    /// Events are taken before the document sees them, so the gesture that
    /// moves the window does not also hover, press or drag anything in it.
    pub(super) fn consume_frame_move(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        input: &InputPayload,
    ) -> bool {
        // The host-driven window move exists only on macOS and Windows; the
        // rest of this function never touches the event loop.
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = (event_loop, id, input);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some((session, owner, live)) = self.live_frame_move
            && session == id
        {
            match input {
                InputPayload::Pointer(PointerInput { phase, button, .. }) => {
                    match frame_move_step(*phase, *button, owner) {
                        FrameMoveStep::Follow => {
                            if let Some(window) = self.window(id) {
                                let _ = live.update(window.as_ref());
                            }
                            // Unlike a resize, a move leaves the drawable
                            // alone — the compositor carries the frame that is
                            // already there — so the window follows the
                            // pointer without a repaint. Only the recorded
                            // geometry has to keep up, for the persisted frame
                            // and for screen coordinates.
                            self.sync_geometry(id);
                        }
                        FrameMoveStep::Finish => self.end_live_frame_move(event_loop, id),
                        FrameMoveStep::Hold => {}
                    }
                    return true;
                }
                // The pointer is holding the window, so a wheel that reaches
                // the document would zoom or scroll whatever is under it in
                // the middle of repositioning the window.
                InputPayload::Wheel(_) => return true,
                // Escape puts the window back, the same way the system move
                // loop answers it.
                InputPayload::Key(key) if key.is_pressed() && key.logical.0 == "Escape" => {
                    if let Some(window) = self.window(id) {
                        let _ = live.cancel(window.as_ref());
                    }
                    self.sync_geometry(id);
                    self.end_live_frame_move(event_loop, id);
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Ends the window move for `id` if one is running.
    ///
    /// The gesture kept its own events while the window followed it, so the
    /// document is told it ended — otherwise a press it never saw released
    /// would stay held.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn end_live_frame_move(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId) {
        if self
            .live_frame_move
            .take_if(|(session, _, _)| *session == id)
            .is_some()
        {
            self.dispatch_pointer_cancel(event_loop, id);
            self.request_redraw(id);
        }
    }

    /// Whether a host-driven window move owns `id`'s pointer right now.
    pub(super) fn frame_move_active(&self, id: WindowId) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.live_frame_move
                .as_ref()
                .is_some_and(|(session, _, _)| *session == id)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = id;
            false
        }
    }
    pub(super) fn consume_frame_resize(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        input: &InputPayload,
    ) -> bool {
        // The live frame-resize session exists only on macOS and Windows; the
        // rest of this function never touches the event loop.
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = event_loop;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some((session, live)) = self.live_frame_resize
            && session == id
        {
            // The pinned winit win32 proc synthesizes `PointerLeft` from a
            // client-rect bounds check even while the drag is captured, so a
            // fast drag crossing the window edge arrives as `Cancel` and must
            // not end the session; only Up, a fresh primary press (lost Up),
            // or focus loss does.
            match input {
                InputPayload::Pointer(PointerInput {
                    phase: PointerPhase::Move,
                    ..
                }) => {
                    if let Some(window) = self.window(id) {
                        let _ = live.update(window.as_ref());
                    }
                    // `setFrame` from inside this pointer dispatch leaves
                    // winit's `SurfaceResized` queued for the next run-loop
                    // pass, and a redraw that waits for it lets the compositor
                    // composite the moved frame with the old drawable
                    // stretched. Sync geometry and paint in this stack, like
                    // the native live-resize path already does.
                    self.sync_geometry(id);
                    self.redraw(event_loop, id);
                    self.apply_window_cursor(id);
                    return true;
                }
                InputPayload::Pointer(PointerInput {
                    phase: PointerPhase::Cancel,
                    ..
                }) => {
                    self.apply_window_cursor(id);
                    return true;
                }
                InputPayload::Pointer(PointerInput {
                    phase: PointerPhase::Up,
                    ..
                }) => {
                    self.end_live_frame_resize(id);
                    self.request_redraw(id);
                    self.apply_window_cursor(id);
                    return true;
                }
                InputPayload::Pointer(PointerInput {
                    phase: PointerPhase::Down,
                    button: 0,
                    is_primary: true,
                    ..
                }) => self.end_live_frame_resize(id),
                _ => {}
            }
        }
        let InputPayload::Pointer(PointerInput {
            phase: PointerPhase::Down,
            button: 0,
            is_primary: true,
            x,
            y,
            ..
        }) = input
        else {
            return false;
        };
        let Some(edge) = self.frame_resize_edge_at(id, *x, *y) else {
            return false;
        };
        self.start_frame_resize(id, edge);
        true
    }
    /// Ends the live frame resize for `id` if one is running, releasing the
    /// mouse capture.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn end_live_frame_resize(&mut self, id: WindowId) {
        if let Some((_, live)) = self
            .live_frame_resize
            .take_if(|(session, _)| *session == id)
            && let Some(window) = self.window(id)
        {
            live.end(window.as_ref());
        }
    }
    pub(super) fn start_frame_resize(&mut self, id: WindowId, edge: WindowResizeEdge) {
        let Some(window) = self.window(id).cloned() else {
            return;
        };
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            if let Some(live) =
                nana_window::LiveFrameResize::begin(window.as_ref(), frame_resize_edge(edge))
            {
                self.live_frame_resize = Some((id, live));
                // Apply the live present policy before the first moved frame.
                // The mode switch reconfigures the swapchain; paying it on the
                // gesture's first redraw would stall exactly the frame the
                // window starts following the pointer. The native size-move
                // path does not need this: its ENTER hook forces a repaint
                // before the first size change.
                self.resize_window(id);
                #[cfg(target_os = "macos")]
                self.pin_present_transaction(id);
                return;
            }
        }
        resize_scene_window(window.as_ref(), edge);
    }
    pub(super) fn frame_resize_edge_at(
        &mut self,
        id: WindowId,
        x: f32,
        y: f32,
    ) -> Option<WindowResizeEdge> {
        let fullscreen = self
            .window(id)
            .is_some_and(|window| window.fullscreen().is_some());
        let edge = frame_resize_edge_for(
            self.settings_of(id),
            &self.geometry_of(id),
            fullscreen,
            x,
            y,
        )?;
        if self.caption_control_at(id, x, y) {
            return None;
        }
        Some(edge)
    }
    fn caption_control_at(&mut self, id: WindowId, x: f32, y: f32) -> bool {
        self.program
            .read_document(id, |document| {
                pointer_hits_window_control(document.context(), document.document(), x, y)
            })
            .unwrap_or(false)
    }
    pub(super) fn settings_of(&self, id: WindowId) -> &WindowDescriptor {
        self.window_contexts
            .get(&id)
            .map(|host| &host.settings)
            .unwrap_or(&self.settings)
    }

    /// Moves or resizes a window, and does nothing else.
    ///
    /// Position and size are not window flags: winit stores them and calls
    /// `SetWindowPos`, so nothing rewrites `GWL_STYLE`/`GWL_EXSTYLE` and there
    /// is no chrome to reconcile. A move in particular must stay this cheap —
    /// the compositor moves the frame it already has, so a dragged window
    /// costs no redraw, no DWM corner and border rewrite, and no style guard
    /// re-arm.
    pub(super) fn mutate_window_geometry<R>(
        &self,
        id: WindowId,
        mutate: impl FnOnce(&dyn winit::window::Window) -> R,
    ) -> Option<R> {
        self.window(id).map(|window| mutate(window.as_ref()))
    }

    /// Shows or hides a window.
    ///
    /// `WS_VISIBLE` is one of winit's own window flags, so this does rewrite
    /// the style and the chrome has to be put back — but it is not a paint,
    /// and callers decide whether the window owes a frame.
    pub(super) fn mutate_window_visibility<R>(
        &self,
        id: WindowId,
        mutate: impl FnOnce(&dyn winit::window::Window) -> R,
    ) -> Option<R> {
        let result = self.window(id).map(|window| mutate(window.as_ref()));
        if result.is_some() {
            self.reconcile_native_chrome(id);
        }
        result
    }

    /// Runs a winit call that can rewrite the window's native style, then puts
    /// NanaUI's chrome back on top of whatever winit wrote.
    ///
    /// Only paths that change a `WindowFlags` bit belong here — maximize,
    /// minimize, fullscreen, window level, resizable, cursor hit-testing —
    /// because those are the ones `apply_diff` rewrites the whole style for.
    /// The frame change it sends is a visual change, so this also asks for a
    /// frame.
    pub(super) fn mutate_native_style<R>(
        &self,
        id: WindowId,
        mutate: impl FnOnce(&dyn winit::window::Window) -> R,
    ) -> Option<R> {
        let result = self.window(id).map(|window| mutate(window.as_ref()));
        if result.is_some() {
            self.reconcile_native_chrome(id);
            self.request_redraw(id);
        }
        result
    }

    /// Re-asserts this window's native chrome from its resolved presentation.
    ///
    /// The policy comes off `host.presentation`, which was settled when the
    /// surface answered; nothing here re-derives it from the request, so a
    /// window whose transparency fell back to solid cannot pick transparent
    /// chrome back up on a later style change.
    pub(super) fn reconcile_native_chrome(&self, id: WindowId) {
        let Some(host) = self.window_contexts.get(&id) else {
            return;
        };
        self.note_native_chrome_write();
        let window = host.surface.window();
        apply_native_chrome(window.as_ref(), &host.settings, &host.presentation, true);
        // `prepare_client_chrome` centers the buttons the way a window
        // without a laid-out placeholder wants them.
        place_native_controls(host);
        // Style changes can bring hidden native buttons back.
        if !host.native_controls_visible {
            let _ = nana_window::set_native_window_controls_visible(
                window.as_ref(),
                false,
                std::time::Duration::ZERO,
            );
        }
    }

    /// Records one native-chrome write.
    ///
    /// Every path that writes chrome calls this, including the ones that go
    /// through [`apply_resolved_presentation`] directly rather than through
    /// [`Self::reconcile_native_chrome`]. A path that wrote chrome without
    /// counting it would be a hole in the steady-state gate: the contract
    /// would read zero while the window rewrote its frame styles on every
    /// frame.
    pub(super) fn note_native_chrome_write(&self) {
        self.native_chrome_writes
            .set(self.native_chrome_writes.get().saturating_add(1));
    }

    /// Moves the native window buttons onto the title bar's placeholder.
    ///
    /// The document is searched only after structural changes until a
    /// placeholder exists. The move itself runs every frame: AppKit lays the
    /// titlebar out on its own schedule and puts the buttons back, and the
    /// move returns without touching them once they sit on the box.
    pub(super) fn sync_native_window_controls(
        &mut self,
        id: WindowId,
        update: &nana_ui_scene::RuntimeFrameUpdate,
    ) {
        if !cfg!(target_os = "macos") {
            return;
        }
        let Some(host) = self.window_contexts.get(&id) else {
            return;
        };
        let hint = host.native_controls;
        let structure_changed = !update.scene.added.is_empty() || update.scene.order_changed;
        if host.settings.system_caption || (hint.is_none() && !structure_changed) {
            return;
        }
        let found = self
            .program
            .read_document(id, |document| {
                crate::window_chrome::native_window_controls(
                    document.context(),
                    document.document(),
                    hint,
                )
            })
            .flatten();
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return;
        };
        host.native_controls = found.map(|(node, _)| node);
        match found {
            // A hidden placeholder keeps its box: showing the buttons again
            // lays the titlebar out, and the box is what puts them back
            // before that frame is drawn.
            Some((_, None)) => {}
            Some((_, Some(bounds))) => {
                host.native_controls_box.set(Some(bounds));
                place_native_controls(host);
            }
            // Nothing marks a spot any more; the platform owns them again.
            None => host.native_controls_box.set(None),
        }
    }

    /// Puts the native buttons back on the placeholder right after something
    /// AppKit reacts to by laying the titlebar out again.
    fn reapply_native_window_controls(&self, id: WindowId) {
        if !cfg!(target_os = "macos") {
            return;
        }
        if let Some(host) = self.window_contexts.get(&id) {
            place_native_controls(host);
        }
    }
    pub(super) fn scale_factor(&self, id: WindowId) -> f32 {
        self.window(id)
            .map(|window| normalized_scale_factor(window.scale_factor() as f32))
            .unwrap_or(1.0)
    }
    pub(super) fn title_bar_chrome_action(
        &mut self,
        id: WindowId,
        input: &InputPayload,
    ) -> Option<WindowChromeAction> {
        let program = &mut self.program;
        let chrome = &mut self.chrome;
        program
            .read_document(id, |document| {
                let session = chrome
                    .entry(id)
                    .or_insert_with(|| WindowChromeSession::new(id));
                apply_title_bar_pointer(
                    &mut session.state,
                    &mut session.drag,
                    document.context(),
                    document.document(),
                    input,
                )
            })
            .flatten()
    }
    pub(super) fn merge_title_bar_chrome(
        &mut self,
        id: WindowId,
        action: Option<WindowChromeAction>,
        mut update: RuntimeProgramUpdate,
    ) -> RuntimeProgramUpdate {
        let Some(action) = action else {
            return update;
        };
        if action == WindowChromeAction::Close {
            // The title-bar close is the same request as the system one.
            return update.merge(
                self.program
                    .window_event(WindowEvent::CloseRequested { id }, &self.context_for(id)),
            );
        }
        let maximized = self
            .chrome
            .get(&id)
            .is_some_and(|session| session.state.is_maximized());
        if action == WindowChromeAction::ToggleMaximize {
            self.sync_title_bar_maximized(id, maximized);
        }
        update
            .window_commands
            .extend(window_commands_for_chrome_action(id, action, maximized));
        update
    }
    pub(super) fn prepare_window_chrome(&mut self, id: WindowId, maximized: bool) {
        let session = self
            .chrome
            .entry(id)
            .or_insert_with(|| WindowChromeSession::new(id));
        session.state.update(WindowChromeEvent::PrepareWindow(id));
        session.state.update(WindowChromeEvent::MaximizedChanged {
            window: id,
            maximized,
        });
        self.sync_title_bar_maximized(id, maximized);
    }
    /// Align the window's title bars with its maximized state through the
    /// component index, touching only bars that differ.
    pub(super) fn sync_title_bar_maximized(&mut self, id: WindowId, maximized: bool) {
        self.program.write_document(id, |document| {
            let document_id = document.document();
            let context = document.context_mut();
            let stale = context
                .world()
                .nodes_of_component(
                    document_id,
                    nana_ui_runtime::component_descriptors::APP_TITLE_BAR.type_id,
                )
                .filter(|&node| {
                    context
                        .read(Entity::<AppTitleBar>::from_stable_id(node), |bar| {
                            bar.maximized != maximized
                        })
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>();
            for bar in stale {
                let _ = context.update_component(
                    Entity::<AppTitleBar>::from_stable_id(bar),
                    |bar, _| {
                        bar.maximized = maximized;
                    },
                );
            }
        });
    }
}

impl<Program: RuntimeProgram> WindowManager<Program> {
    pub(super) fn drain_window_requests(&mut self, event_loop: &dyn ActiveEventLoop) {
        use crate::window_service::Request;
        let remaining = super::schedule::drain_host_batch(
            || {
                if self.shutting_down || event_loop.exiting() {
                    return false;
                }
                let Some(request) = self
                    .window_requests
                    .as_ref()
                    .and_then(|requests| requests.try_recv().ok())
                else {
                    return false;
                };
                match request {
                    Request::Displays(reply) => {
                        reply.finish(Ok(display::display_infos(event_loop)));
                    }
                    Request::Material(id, generation, effect, reply) => {
                        if !self.windows.is_current(id, generation) {
                            reply.finish(Err(crate::WindowError::WindowClosed));
                            return true;
                        }
                        if let Some(host) = self.window_contexts.get(&id)
                            && (self.render_suspended || host.surface_retry.is_some())
                        {
                            reply.finish(Err(crate::WindowError::OperationFailed(
                                "window surface recovery pending".into(),
                            )));
                            return true;
                        }
                        match self.apply_window_material(id, Some(effect)) {
                            Ok(()) => {
                                self.window_contexts.get_mut(&id).unwrap().material_override =
                                    Some(effect);
                                reply.finish(Ok(self.material_of(id)));
                            }
                            Err(error) => {
                                let message = error.to_string();
                                self.suspend_surface(id, error);
                                reply.finish(Err(crate::WindowError::OperationFailed(message)));
                            }
                        }
                    }
                    Request::Create(settings, reply) => {
                        reply.finish(self.create_service_window(event_loop, settings));
                    }
                    Request::Native(id, generation, callback) => {
                        if !self.windows.is_current(id, generation) {
                            callback(Err(crate::WindowError::WindowClosed));
                            return true;
                        }
                        use raw_window_handle::HasWindowHandle;
                        match self.window(id) {
                            Some(window) => callback(
                                window
                                    .window_handle()
                                    .map_err(|e| crate::WindowError::Unsupported(e.to_string())),
                            ),
                            None => callback(Err(crate::WindowError::WindowClosed)),
                        }
                    }
                    Request::Control(id, generation, control, reply) => {
                        if !self.windows.is_current(id, generation) {
                            reply.finish(Err(crate::WindowError::WindowClosed));
                            return true;
                        }
                        reply.finish(self.control_window(event_loop, id, control));
                    }
                }
                !self.shutting_down && !event_loop.exiting()
            },
            Instant::now,
        );
        if remaining {
            self.host_work.wake();
        }
    }

    /// Allocate a host-owned identity and open a fully initialized window.
    pub(super) fn create_service_window(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        settings: WindowDescriptor,
    ) -> Result<crate::WindowHandle, crate::WindowError> {
        use crate::WindowError;
        if self.shutting_down {
            return Err(WindowError::HostStopped);
        }
        // Skip program-chosen identities (such as hashed dock windows) already live here.
        let id = loop {
            let id = WindowId(self.next_window_id);
            self.next_window_id = self.next_window_id.checked_add(1).ok_or_else(|| {
                WindowError::InitializationFailed("window identities exhausted".into())
            })?;
            if !self.window_contexts.contains_key(&id) {
                break id;
            }
        };
        let event = self
            .open_window(event_loop, id, settings)
            .map_err(WindowError::InitializationFailed)?;
        let update = self.program.window_event(event, &self.context_for(id));
        self.apply_update(event_loop, update, None);
        self.finish_ready(event_loop, id);
        // `Ready` has been delivered, so creation itself succeeded; a window its
        // own `Ready` handling closed resolves as closed, not as a failed creation.
        if self.shutting_down {
            Err(WindowError::HostStopped)
        } else if self.window(id).is_some() {
            Ok(self.windows.handle(id))
        } else {
            Err(WindowError::WindowClosed)
        }
    }

    fn control_window(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        control: crate::window_service::Control,
    ) -> Result<(), crate::WindowError> {
        use crate::window_service::{Control, validate_size};
        use crate::{WindowCursor, WindowError};
        let window = self.window(id).cloned().ok_or(WindowError::WindowClosed)?;
        match control {
            Control::Visible(visible) => {
                let focus_on_show = self.settings_of(id).focus_on_show;
                self.mutate_window_visibility(id, |window| {
                    set_native_visible(window, visible, focus_on_show)
                });
                // Frames were deferred while hidden; a shown window repaints
                // without relying on the program to request it.
                if visible {
                    self.request_redraw(id);
                } else {
                    self.hide_pointer_presence(event_loop, id);
                }
                let update = self.program.window_event(
                    WindowEvent::VisibilityChanged {
                        id,
                        hidden: !visible,
                    },
                    &self.context_for(id),
                );
                self.apply_update(event_loop, update, None);
                if visible {
                    self.sync_window_mode(event_loop, id);
                }
            }
            Control::Size(size) => {
                validate_size(size)?;
                self.mutate_window_geometry(id, |window| {
                    window.request_surface_size(winit::dpi::LogicalSize::new(size.0, size.1).into())
                });
            }
            Control::MinSize(size) => {
                if let Some(size) = size {
                    validate_size(size)?;
                }
                self.mutate_window_geometry(id, |window| {
                    window.set_min_surface_size(
                        size.map(|s| winit::dpi::LogicalSize::new(s.0, s.1).into()),
                    )
                });
            }
            Control::MaxSize(size) => {
                if let Some(size) = size {
                    validate_size(size)?;
                }
                self.mutate_window_geometry(id, |window| {
                    window.set_max_surface_size(
                        size.map(|s| winit::dpi::LogicalSize::new(s.0, s.1).into()),
                    )
                });
            }
            Control::Resizable(value) => {
                self.mutate_native_style(id, |window| window.set_resizable(value));
            }
            Control::Level(level) => self.set_window_level(event_loop, id, level),
            Control::ContentProtected(protected) => {
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                window.set_content_protected(protected);
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                {
                    let _ = protected;
                    return Err(WindowError::Unsupported("native content protection".into()));
                }
            }
            Control::CursorVisible(visible) => {
                self.window_contexts
                    .get_mut(&id)
                    .unwrap()
                    .cursor_visible_override = Some(visible);
                window.set_cursor_visible(visible);
            }
            Control::Cursor(cursor) => {
                let host = self.window_contexts.get_mut(&id).unwrap();
                let (icon, visible) = window_cursor_override(cursor);
                host.cursor_override = icon;
                if cursor == WindowCursor::Automatic || visible.is_some() {
                    host.cursor_visible_override = visible;
                } else if host.cursor_visible_override == Some(false) {
                    host.cursor_visible_override = None;
                }
                self.apply_window_cursor(id);
            }
            Control::Redraw => window.request_redraw(),
            Control::Resize(edge) => {
                if !resize_custom_frame(window.as_ref(), frame_resize_edge(edge)) {
                    window
                        .drag_resize_window(match edge {
                            WindowResizeEdge::North => winit::window::ResizeDirection::North,
                            WindowResizeEdge::South => winit::window::ResizeDirection::South,
                            WindowResizeEdge::East => winit::window::ResizeDirection::East,
                            WindowResizeEdge::West => winit::window::ResizeDirection::West,
                            WindowResizeEdge::NorthEast => {
                                winit::window::ResizeDirection::NorthEast
                            }
                            WindowResizeEdge::NorthWest => {
                                winit::window::ResizeDirection::NorthWest
                            }
                            WindowResizeEdge::SouthEast => {
                                winit::window::ResizeDirection::SouthEast
                            }
                            WindowResizeEdge::SouthWest => {
                                winit::window::ResizeDirection::SouthWest
                            }
                        })
                        .map_err(window_request_error)?;
                }
            }
            Control::Command(WindowCommand::Drag(_)) => {
                self.begin_window_move(event_loop, id)
                    .map_err(window_request_error)?;
            }
            Control::Command(WindowCommand::SetMousePassthrough { enabled, .. }) => {
                self.set_mouse_passthrough_mode(
                    event_loop,
                    id,
                    MousePassthroughMode::passthrough(enabled),
                )?;
            }
            Control::Command(WindowCommand::SetMousePassthroughForward { enabled, .. }) => {
                self.set_mouse_passthrough_mode(
                    event_loop,
                    id,
                    MousePassthroughMode::forward(enabled),
                )?;
            }
            Control::Command(WindowCommand::Move { position, .. })
                if !position.0.is_finite() || !position.1.is_finite() =>
            {
                return Err(WindowError::InvalidParameter(
                    "position must be finite".into(),
                ));
            }
            Control::Command(WindowCommand::SetSkipTaskbar { skip_taskbar, .. }) => {
                self.set_skip_taskbar(event_loop, id, skip_taskbar)?;
            }
            Control::Command(WindowCommand::SetFullscreen { fullscreen, .. }) => {
                self.set_window_fullscreen(event_loop, id, fullscreen)?;
            }
            Control::Command(command) => self.apply_window_command(event_loop, command),
        }
        // Style-changing operations restore client chrome at their mutation
        // boundary. Redraw/cursor requests must not rewrite native chrome, and
        // commands already use the same boundary in apply_window_command.
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WindowAppearance {
    theme: crate::ThemeMode,
    material: nana_window::MaterialEffect,
    opacity: f32,
    /// The host's own window-surface colour, or `None` to follow the theme.
    /// It rides here so a host that changes only this still re-applies.
    background: Option<nana_ui_core::SemanticColor>,
}

/// Moves a window's native buttons onto the placeholder box last laid out
/// for it. Without a box, or off macOS, nothing moves.
pub(super) fn place_native_controls(host: &WindowContext) {
    let Some(bounds) = host.native_controls_box.get() else {
        return;
    };
    let _ = nana_window::place_native_window_controls(
        host.surface.window().as_ref(),
        f64::from(bounds.x),
        f64::from(bounds.y),
        f64::from(bounds.width),
        f64::from(bounds.height),
    );
}

fn apply_changed_appearance<T>(
    applied: &mut Option<WindowAppearance>,
    desired: WindowAppearance,
    apply: impl FnOnce() -> Result<T, HostedGpuError>,
) -> Result<Option<T>, HostedGpuError> {
    if *applied == Some(desired) {
        return Ok(None);
    }
    let outcome = apply()?;
    *applied = Some(desired);
    Ok(Some(outcome))
}

#[cfg(test)]
mod appearance_tests {
    use super::*;

    #[test]
    fn unchanged_windows_do_not_reapply_native_material() {
        let original = WindowAppearance {
            theme: crate::ThemeMode::Dark,
            material: nana_window::MaterialEffect::Solid,
            opacity: 1.0,
            background: None,
        };
        let mut windows = [Some(original); 3];
        let changed = WindowAppearance {
            opacity: 0.5,
            ..original
        };
        let mut calls = Vec::new();
        for (index, cache) in windows.iter_mut().enumerate() {
            let desired = if index == 1 { changed } else { original };
            let result = apply_changed_appearance(cache, desired, || {
                calls.push(index);
                Ok(MaterialOutcome::chosen_solid())
            })
            .unwrap();
            assert_eq!(result.is_some(), index == 1);
        }
        assert_eq!(calls, vec![1]);
        assert_eq!(windows[1], Some(changed));
        assert!(
            apply_changed_appearance::<MaterialOutcome>(&mut windows[1], changed, || panic!(
                "unchanged material was reapplied"
            ))
            .unwrap()
            .is_none()
        );
        assert!(
            apply_changed_appearance(&mut windows[1], original, || Err::<MaterialOutcome, _>(
                HostedGpuError::SurfaceValidation
            ))
            .is_err()
        );
        assert_eq!(windows[1], Some(changed));
    }
    #[test]
    fn theme_changes_and_device_replacement_invalidate_appearance() {
        let original = WindowAppearance {
            theme: crate::ThemeMode::Dark,
            material: nana_window::MaterialEffect::Solid,
            opacity: 1.0,
            background: None,
        };
        let mut cached = Some(original);
        let light = WindowAppearance {
            theme: crate::ThemeMode::Light,
            ..original
        };
        let calls = std::cell::Cell::new(0);
        let apply = |cache: &mut Option<WindowAppearance>, desired| {
            apply_changed_appearance(cache, desired, || {
                calls.set(calls.get() + 1);
                Err::<MaterialOutcome, _>(HostedGpuError::SurfaceValidation)
            })
        };
        assert!(apply(&mut cached, light).is_err());
        cached = None;
        assert!(apply(&mut cached, original).is_err());
        assert_eq!(calls.get(), 2);
    }
}

/// Show or hide a native window. One whose descriptor declines focus on show
/// is ordered front without activation where the platform supports it, both
/// on its first show and when shown again.
pub(super) fn set_native_visible(
    window: &dyn winit::window::Window,
    visible: bool,
    focus_on_show: bool,
) {
    if visible && !focus_on_show && nana_window::show_without_activation(window) {
        return;
    }
    window.set_visible(visible);
}

fn native_skip_taskbar(
    window: &dyn winit::window::Window,
    skip: bool,
) -> Result<(), crate::WindowError> {
    nana_window::set_skip_taskbar(window, skip).map_err(|error| match error {
        nana_window::SkipTaskbarError::Unsupported(reason) => {
            crate::WindowError::Unsupported(reason)
        }
        nana_window::SkipTaskbarError::Failed(reason) => {
            crate::WindowError::OperationFailed(reason)
        }
    })
}

/// Apply `WindowDescriptor::skip_taskbar` while the window is still hidden, so
/// its first show is covered. The outcome is delivered by `finish_ready`.
pub(super) fn descriptor_skip_taskbar(
    window: &dyn winit::window::Window,
    settings: &WindowDescriptor,
) -> Option<Result<(), crate::WindowError>> {
    settings
        .skip_taskbar
        .then(|| native_skip_taskbar(window, true))
}

fn window_request_error(error: winit::error::RequestError) -> crate::WindowError {
    match error {
        winit::error::RequestError::NotSupported(_) => {
            crate::WindowError::Unsupported(error.to_string())
        }
        _ => crate::WindowError::OperationFailed(error.to_string()),
    }
}

#[cfg(test)]
mod request_error_tests {
    use super::*;

    #[test]
    fn ignored_request_is_an_operation_failure_not_a_missing_capability() {
        assert!(matches!(
            window_request_error(winit::error::RequestError::Ignored),
            crate::WindowError::OperationFailed(_)
        ));
        assert!(matches!(
            window_request_error(winit::error::RequestError::NotSupported(
                winit::error::NotSupportedError::new("fixture capability")
            )),
            crate::WindowError::Unsupported(_)
        ));
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod drag_policy_tests {
    use super::*;

    #[test]
    fn host_managed_drag_overrides_the_primary_button_native_default() {
        let native = WindowDescriptor::new("native");
        assert!(!uses_host_managed_drag(&native, PRIMARY_MOUSE_BUTTON));
        assert!(uses_host_managed_drag(&native, 1));

        let managed = WindowDescriptor::new("managed").host_managed_drag(true);
        assert!(uses_host_managed_drag(&managed, PRIMARY_MOUSE_BUTTON));
    }
}

fn move_to_desktop_position(window: &dyn winit::window::Window, position: (f32, f32)) {
    let scale = desktop_scale(window.scale_factor(), window_reference_scale(window));
    window.set_outer_position(desktop_position(
        (f64::from(position.0), f64::from(position.1)),
        scale,
    ));
}

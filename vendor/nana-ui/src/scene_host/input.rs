//! Scene host input coordination.

use super::*;

/// Actions a file drop accepts. AppKit narrows a Control-drag from Finder to
/// Link and refuses the drop when nothing matches, so macOS takes Link too;
/// elsewhere Link would only turn Shift-drags into shortcuts.
#[cfg(target_os = "macos")]
const FILE_DROP_ACTIONS: &[DndAction] = &[DndAction::Copy, DndAction::Link];
#[cfg(not(target_os = "macos"))]
const FILE_DROP_ACTIONS: &[DndAction] = &[DndAction::Copy];

impl<Program: RuntimeProgram> WindowManager<Program> {
    pub(super) fn handle_window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        event: WinitWindowEvent,
    ) {
        #[cfg(not(target_os = "android"))]
        if let Some(window) = self.window(id).cloned()
            && let Some(accessibility) = self.accessibility_mut(id)
        {
            accessibility.process_event(window.as_ref(), &event);
        }
        #[cfg(not(target_os = "android"))]
        for request in self.take_accessibility_actions(id) {
            let update = match self
                .program
                .accessibility_action(id, request, &self.context_for(id))
            {
                Ok(update) => update,
                Err(error) => {
                    self.program
                        .report_host_failure(HostFailure::AccessibilityAction {
                            window: id,
                            error: error.to_string(),
                        });
                    continue;
                }
            };
            self.apply_update(event_loop, update, None);
            // An action may close its own window; the event has no target left.
            if event_loop.exiting() || !self.window_contexts.contains_key(&id) {
                return;
            }
        }
        // Presence is window state, reported even while a modal child or Forward
        // passthrough keeps the pointer itself from reaching widgets. A window
        // the host is moving is the exception: the pointer holds it, so every
        // crossing of its own former bounds is the window leaving the pointer,
        // not the pointer leaving the window.
        if let Some(signal) = presence::presence_signal(&event, self.geometry_of(id).physical_size)
            .filter(|_| !self.frame_move_active(id))
        {
            self.observe_pointer_presence(event_loop, id, signal);
            if event_loop.exiting() || !self.window_contexts.contains_key(&id) {
                return;
            }
        }
        let pointer_left = matches!(&event, WinitWindowEvent::PointerLeft { .. });
        if let Some(modal) = self.active_modal_child(id)
            && !allows_modal_parent_event(&event)
        {
            if pointer_left {
                self.reset_window_cursor(id);
            }
            self.focus_window(modal);
            return;
        }
        if let WinitWindowEvent::ModifiersChanged(modifiers) = &event {
            self.input_mut(id).modifiers = modifiers.state();
        }
        if self.forward_os_passthrough_ignores_pointer(id, &event) {
            // Sampling owns the pointer while OS hit-testing is still off.
            // Down must not reach widgets until recover flips hit-testing on.
            return;
        }
        if let WinitWindowEvent::PointerMoved { position, .. }
        | WinitWindowEvent::PointerEntered { position, .. }
        | WinitWindowEvent::PointerButton { position, .. } = &event
        {
            let scale = self.scale_factor(id);
            self.input_mut(id).set_cursor_physical(*position, scale);
        }
        if let WinitWindowEvent::PointerLeft {
            position: Some(position),
            ..
        } = &event
        {
            let scale = self.scale_factor(id);
            self.input_mut(id).set_cursor_physical(*position, scale);
        }
        if self.forward_pointer_action_for(id, &event) == ForwardPointerAction::RestorePassthrough {
            self.restore_forward_passthrough(event_loop, id);
            return;
        }
        if pointer_left && self.frame_move_active(id) {
            // The pointer holds the window it is moving: crossing the
            // window's former bounds is not the pointer leaving it.
            self.reset_window_cursor(id);
            return;
        }
        let scale = self.scale_factor(id);
        let origin = self
            .window(id)
            .and_then(|window| window_screen_origin(window.as_ref()));
        let now = self.animation_clock.runtime_time(Instant::now());
        if let Some(lowered) = self.input_mut(id).map(&event, scale, origin, now) {
            if self.consume_frame_move(event_loop, id, &lowered.payload)
                || self.consume_frame_resize(event_loop, id, &lowered.payload)
            {
                return;
            }
            let disposition = self.deliver_input(event_loop, id, lowered);
            if !self.window_contexts.contains_key(&id) {
                return;
            }
            if pointer_left {
                // The host shows its own cursor once the pointer is gone;
                // `cursor: none` must not leak outside the window.
                self.reset_window_cursor(id);
                self.request_redraw(id);
                return;
            }
            if disposition.prevent_default || event_loop.exiting() {
                return;
            }
        }
        match &event {
            WinitWindowEvent::RedrawRequested => {
                // Moves merged since the last turn land before the frame
                // that shows them.
                self.drain_window_input(event_loop, id);
                if !self.window_contexts.contains_key(&id) {
                    return;
                }
                self.sync_window_mode(event_loop, id);
                self.redraw(event_loop, id);
            }
            // The program decides whether and when the window goes away; an
            // exit that first saves state answers with `Close` or `exit` later.
            WinitWindowEvent::CloseRequested => self.forward_window_event(event_loop, id, &event),
            WinitWindowEvent::Destroyed => self.close_window(event_loop, id),
            // A frame change that comes with a mode change (entering or
            // leaving fullscreen) records the mode first, so the frame is
            // persisted and reported knowing whether it is a fullscreen one.
            WinitWindowEvent::Moved(_) => {
                self.sync_window_mode(event_loop, id);
                self.sync_geometry(id);
                self.forward_window_event(event_loop, id, &event);
            }
            WinitWindowEvent::SurfaceResized(_) | WinitWindowEvent::ScaleFactorChanged { .. } => {
                match &event {
                    WinitWindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        nana_diagnostics::event!(
                            nana_diagnostics::framework::window::SCALE_FACTOR_CHANGED,
                            window = id.0,
                            scale = *scale_factor
                        );
                        self.rescale_startup_splash(id, *scale_factor);
                        // Shadow content is built at device resolution; the
                        // next present re-derives the body and rebuilds it.
                        if let Some(host) = self.window_contexts.get_mut(&id) {
                            let window = Arc::clone(host.surface.window());
                            host.shadow.rescale(window.as_ref());
                            host.shadow_body = None;
                        }
                    }
                    _ => nana_diagnostics::metric!(nana_diagnostics::framework::window::RESIZES),
                }
                self.sync_window_mode(event_loop, id);
                let geometry_changed = self.sync_geometry(id);
                #[cfg(target_os = "macos")]
                let native_live_resize = self.sync_native_live_resize_presents(id);
                #[cfg(not(target_os = "macos"))]
                let native_live_resize = false;
                self.forward_window_event(event_loop, id, &event);
                // Native macOS drags repaint through winit's live-resize
                // hook, and a custom chrome drag paints its steps in-stack;
                // both would only duplicate the per-step frame here.
                if geometry_changed && !native_live_resize {
                    self.request_redraw(id);
                }
            }
            WinitWindowEvent::Occluded(occluded) => {
                nana_diagnostics::event!(
                    nana_diagnostics::framework::window::OCCLUDED,
                    window = id.0,
                    occluded = *occluded
                );
                if *occluded {
                    self.occluded.insert(id);
                    // Nothing of an occluded window shows — minimized or
                    // covered — so nothing can hover it: report the leave the
                    // platform may never send, as hiding does.
                    self.hide_pointer_presence(event_loop, id);
                    if event_loop.exiting() || !self.window_contexts.contains_key(&id) {
                        return;
                    }
                } else {
                    self.occluded.remove(&id);
                    self.request_redraw(id);
                }
                self.forward_window_event(event_loop, id, &event);
                self.sync_window_mode(event_loop, id);
            }
            WinitWindowEvent::Focused(focused) => {
                if !*focused {
                    self.input_mut(id).clear_pointers();
                    #[cfg(any(target_os = "macos", target_os = "windows"))]
                    self.end_live_frame_resize(id);
                    // Ending a window move tells the document its gesture is
                    // over, so it runs last: that dispatch may close `id`.
                    #[cfg(any(target_os = "macos", target_os = "windows"))]
                    self.end_live_frame_move(event_loop, id);
                    if !self.window_contexts.contains_key(&id) {
                        return;
                    }
                }
                // Window focus is canonical input as well as a host
                // observation. Routing it first lets the Runtime cancel what
                // the window held and hand the IME its state again.
                self.deliver_host_input(event_loop, id, InputPayload::Focus { focused: *focused });
                if !self.window_contexts.contains_key(&id) {
                    return;
                }
                self.forward_window_event(event_loop, id, &event);
            }
            WinitWindowEvent::DragEntered { .. }
            | WinitWindowEvent::DragPosition { .. }
            | WinitWindowEvent::DragDropped { .. }
            | WinitWindowEvent::DragLeft { .. }
            | WinitWindowEvent::DataTransferReceived { .. } => {
                // A file drag is input: it reaches drop targets through the
                // router and the program through `input_event`.
                if let Some(drag) = self.handle_file_dnd(event_loop, id, &event) {
                    self.deliver_host_input(event_loop, id, InputPayload::FileDrag(drag));
                }
            }
            _ => {}
        }
    }
    pub(super) fn forward_window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        event: &WinitWindowEvent,
    ) {
        if let Some(window_event) = platform_window_event(event, id, self.geometry_of(id)) {
            let update = self
                .program
                .window_event(window_event, &self.context_for(id));
            self.apply_update(event_loop, update, None);
        }
    }
    pub(super) fn handle_file_dnd(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        event: &WinitWindowEvent,
    ) -> Option<FileDragInput> {
        let scale = self.scale_factor(id);
        match event {
            WinitWindowEvent::DragEntered {
                id: transfer,
                position,
            } => {
                if let Some(position) = position {
                    self.input_mut(id).set_cursor_physical(*position, scale);
                }
                if !dnd_advertises_files(event_loop, *transfer) {
                    return None;
                }
                let _ = event_loop.set_valid_dnd_actions(*transfer, FILE_DROP_ACTIONS);
                let serial = event_loop
                    .fetch_data_transfer(*transfer, &TypeHint::UriList)
                    .ok();
                self.input_mut(id).begin_file_drag(*transfer, serial);
                self.input_mut(id).map_file_window_event(event)
            }
            WinitWindowEvent::DragPosition { position, .. } => {
                self.input_mut(id).set_cursor_physical(*position, scale);
                self.input_mut(id).map_file_window_event(event)
            }
            WinitWindowEvent::DragDropped { id: transfer, .. } => {
                if !self.input_mut(id).pending_file_paths.is_empty() {
                    return self.input_mut(id).map_file_window_event(event);
                }
                match event_loop.fetch_data_transfer(*transfer, &TypeHint::UriList) {
                    Ok(serial) => {
                        self.input_mut(id).wait_for_drop_data(*transfer, serial);
                        None
                    }
                    Err(_) => self.input_mut(id).map_file_window_event(event),
                }
            }
            WinitWindowEvent::DragLeft { .. } => self.input_mut(id).map_file_window_event(event),
            WinitWindowEvent::DataTransferReceived {
                id: transfer,
                serial,
                value,
            } => {
                if !self.input_mut(id).accepts_dnd_serial(*transfer, *serial) {
                    return None;
                }
                match value.try_as_file_paths() {
                    Ok(paths) => self.input_mut(id).ingest_file_paths(*transfer, paths),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Deadlock
                        ) =>
                    {
                        None
                    }
                    // A release waiting on these paths would otherwise never end.
                    Err(_) => self.input_mut(id).abandon_drop(*transfer),
                }
            }
            _ => None,
        }
    }
    /// Stamp `lowered` into the window's endpoint. A transition drains at
    /// once, with everything queued before it; a move or wheel waits for the
    /// end of the event-loop turn, merging with the ones after it.
    /// Deliver input the host makes itself (focus, a file drag, a pointer
    /// it ends), on the device the window last heard from.
    pub(super) fn deliver_host_input(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        payload: InputPayload,
    ) -> nana_ui_platform::InputDisposition {
        let now = self.animation_clock.runtime_time(Instant::now());
        let device = self.input_of(id).last_device;
        self.deliver_input(event_loop, id, LoweredInput::event(device, payload, now))
    }

    pub(super) fn deliver_input(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        lowered: LoweredInput,
    ) -> nana_ui_platform::InputDisposition {
        // The title bar follows its gesture as the platform delivers it: a
        // native drag must start inside the event that moved past the
        // threshold, not at the end of the turn.
        let chrome_action = self.title_bar_chrome_action(id, &lowered.payload);
        let deferred = lowered.payload.is_coalescible() && chrome_action.is_none();
        if !self.enqueue_input(event_loop, id, lowered) {
            return nana_ui_platform::InputDisposition::default();
        }
        let disposition = if deferred {
            self.request_redraw(id);
            nana_ui_platform::InputDisposition::default()
        } else {
            self.drain_window_input(event_loop, id)
        };
        if chrome_action.is_some() && self.window_contexts.contains_key(&id) {
            let update =
                self.merge_title_bar_chrome(id, chrome_action, RuntimeProgramUpdate::default());
            self.apply_update(event_loop, update, None);
        }
        disposition
    }

    /// Push `lowered` (and the text it types) onto the window's endpoint.
    /// A full endpoint drains first; nothing is dropped for capacity.
    fn enqueue_input(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        lowered: LoweredInput,
    ) -> bool {
        let LoweredInput {
            device,
            payload,
            text,
            now,
        } = lowered;
        let Some(key) = self.push_input(event_loop, id, device, payload, now) else {
            return false;
        };
        if let Some(text) = text {
            self.push_input(
                event_loop,
                id,
                device,
                InputPayload::Text(nana_ui_platform::CommittedText {
                    text,
                    key: Some(key),
                }),
                now,
            );
        }
        true
    }

    fn push_input(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        device: nana_ui_platform::DeviceId,
        payload: InputPayload,
        now: Duration,
    ) -> Option<nana_ui_platform::InputSequence> {
        let host = self.window_contexts.get_mut(&id)?;
        let event = nana_ui_platform::CanonicalInputEvent {
            metadata: host.input_source.sequencer.stamp(device, now),
            payload,
        };
        let sequence = event.metadata.sequence;
        let Err(event) = host.input_source.endpoint.push(event) else {
            return Some(sequence);
        };
        // Full: route what is queued, then queue this one behind it.
        self.drain_window_input(event_loop, id);
        let host = self.window_contexts.get_mut(&id)?;
        if host.input_source.endpoint.push(event).is_err() {
            self.program
                .report_host_failure(HostFailure::InputDispatch {
                    window: id,
                    error: "window input endpoint stayed full after a drain".into(),
                });
            return None;
        }
        Some(sequence)
    }

    /// Route everything the window's endpoint holds, then let the program see
    /// each event with what routing learned about it. The one drain every
    /// native input path goes through.
    pub(super) fn drain_window_input(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
    ) -> nana_ui_platform::InputDisposition {
        let mut disposition = nana_ui_platform::InputDisposition::default();
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return disposition;
        };
        if host.input_source.endpoint.is_empty() {
            return disposition;
        }
        let window = Arc::clone(host.surface.window());
        let mut routed = std::mem::take(&mut self.routed_input);
        let text = &mut self.text;
        let (mut services, endpoint, input_id, generation) =
            NativeWindowServices::of(window.as_ref(), &self.clipboard, &mut host.input_source);
        let bound = self.program.write_document(id, |document| {
            let document_id = document.document();
            let context = document.context_mut();
            let bind = context.bind_input_source(input_id, generation, document_id);
            if let Err(nana_ui_runtime::InputBindError::DocumentRebind { .. }) = bind {
                // The program gave this window another document: what the
                // old one held is cancelled, and the source starts over.
                let _ = context.unbind_input_source(input_id, Duration::ZERO);
                let _ = context.bind_input_source(input_id, generation, document_id);
            }
            context.drain_input(endpoint, &mut services, Some(text), &mut routed);
            bind
        });
        let cursor_changed = services.cursor_changed;
        match bound {
            Some(Err(error)) => self
                .program
                .report_host_failure(HostFailure::InputDispatch {
                    window: id,
                    error: error.to_string(),
                }),
            None => {
                // No document: the batch cannot be routed; drop it rather
                // than hold it against a window that will never route it.
                if let Some(host) = self.window_contexts.get_mut(&id) {
                    while host.input_source.endpoint.pop().is_some() {}
                }
            }
            Some(Ok(())) => {}
        }
        let mut update = RuntimeProgramUpdate::default();
        let mut pointer_moved = cursor_changed;
        for routed_event in routed.drain(..) {
            let outcome = match routed_event.result {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.program
                        .report_host_failure(HostFailure::InputDispatch {
                            window: id,
                            error: error.to_string(),
                        });
                    continue;
                }
            };
            pointer_moved |= matches!(routed_event.event.payload, InputPayload::Pointer(_));
            let event_disposition = outcome.disposition();
            disposition.handled |= event_disposition.handled;
            disposition.prevent_default |= event_disposition.prevent_default;
            let program_input = self.program.input_event(
                id,
                crate::RoutedInput {
                    event: &routed_event.event,
                    pointer_hit: outcome.pointer_hit,
                    disposition: event_disposition,
                },
                &self.context_for(id),
            );
            if let Err(error) = &program_input {
                self.program.report_host_failure(HostFailure::InputHandler {
                    window: id,
                    error: error.to_string(),
                });
            }
            update = update.merge(scene_runtime_input_update(
                event_disposition,
                id,
                program_input,
            ));
            if outcome.invalidated_work {
                update = update.merge(RuntimeProgramUpdate::redraw(id));
            }
            if !self.window_contexts.contains_key(&id) {
                break;
            }
        }
        self.routed_input = routed;
        if !self.window_contexts.contains_key(&id) {
            return disposition;
        }
        if pointer_moved {
            self.apply_window_cursor(id);
        }
        if self
            .program
            .read_document(id, |document| document.context().has_program_messages())
            .unwrap_or(false)
        {
            update = update.merge(RuntimeProgramUpdate::redraw(id));
        }
        self.sync_appearance();
        self.apply_update(event_loop, update, None);
        disposition
    }

    /// Drain every window whose moves waited for the end of the turn.
    pub(super) fn drain_deferred_input(&mut self, event_loop: &dyn ActiveEventLoop) {
        let pending: Vec<WindowId> = self
            .window_contexts
            .iter()
            .filter(|(_, host)| !host.input_source.endpoint.is_empty())
            .map(|(id, _)| *id)
            .collect();
        for id in pending {
            if event_loop.exiting() {
                return;
            }
            self.drain_window_input(event_loop, id);
        }
    }

    /// Bring the window's cursor and IME in line with a world that changed
    /// without input: after a frame laid out, or a caret moved.
    pub(super) fn refresh_window_input(&mut self, id: WindowId) {
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return;
        };
        let window = Arc::clone(host.surface.window());
        let (mut services, _, input_id, _) =
            NativeWindowServices::of(window.as_ref(), &self.clipboard, &mut host.input_source);
        self.program.write_document(id, |document| {
            document
                .context_mut()
                .refresh_input_effects(input_id, &mut services);
        });
        if services.cursor_changed {
            self.apply_window_cursor(id);
        }
    }

    /// Show the cursor: the program's override, else a frame edge under the
    /// pointer, else what the Runtime asked for.
    pub(super) fn apply_window_cursor(&mut self, id: WindowId) {
        let Some(host) = self.window_contexts.get(&id) else {
            return;
        };
        let cursor = host.input.cursor;
        let frame_edge = self.frame_resize_edge_at(id, cursor.0, cursor.1);
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return;
        };
        let (icon, visible) = match frame_edge {
            Some(edge) => (frame_edge_cursor(edge), true),
            None => host_services::native_cursor(host.input_source.runtime_cursor),
        };
        let icon = host.cursor_override.unwrap_or(icon);
        let visible = host.cursor_visible_override.unwrap_or(visible);
        if host.input_source.applied_cursor == Some((icon, visible)) {
            return;
        }
        host.input_source.applied_cursor = Some((icon, visible));
        let window = host.surface.window();
        window.set_cursor_visible(visible);
        if visible {
            window.set_cursor(icon.into());
        }
    }

    pub(super) fn input_of(&self, id: WindowId) -> &InputTracker {
        &self
            .window_contexts
            .get(&id)
            .expect("input belongs to a live window")
            .input
    }
    pub(super) fn input_mut(&mut self, id: WindowId) -> &mut InputTracker {
        &mut self
            .window_contexts
            .get_mut(&id)
            .expect("input belongs to a live window")
            .input
    }
}

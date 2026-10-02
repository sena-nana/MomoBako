//! Window-level pointer presence for hover-revealed chrome.

use super::*;

/// What one platform pointer event says about the hovering pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PresenceSignal {
    Inside,
    Left,
}

/// `client` is the physical client size. A captured pointer keeps reporting
/// moves outside the client area; those do not bring it back inside.
pub(super) fn presence_signal(
    event: &WinitWindowEvent,
    client: (u32, u32),
) -> Option<PresenceSignal> {
    let within = |position: &winit::dpi::PhysicalPosition<f64>| {
        position.x >= 0.0
            && position.y >= 0.0
            && position.x < f64::from(client.0)
            && position.y < f64::from(client.1)
    };
    match event {
        WinitWindowEvent::PointerEntered { kind, .. } if !matches!(kind, PointerKind::Touch(_)) => {
            Some(PresenceSignal::Inside)
        }
        WinitWindowEvent::PointerMoved {
            source, position, ..
        } if !matches!(source, PointerSource::Touch { .. }) => {
            within(position).then_some(PresenceSignal::Inside)
        }
        WinitWindowEvent::PointerLeft { kind, .. } if !matches!(kind, PointerKind::Touch(_)) => {
            Some(PresenceSignal::Left)
        }
        _ => None,
    }
}

/// Per-window presence. A native drag hands the pointer to the platform move
/// loop, which reports a leave while the cursor stays over the window; that
/// leave is withheld until the platform reports the pointer again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PointerPresence {
    inside: bool,
    native_drag: bool,
}

impl PointerPresence {
    /// Returns the new state when it changed.
    pub(super) fn observe(&mut self, signal: PresenceSignal) -> Option<bool> {
        let inside = match signal {
            PresenceSignal::Inside => {
                self.native_drag = false;
                true
            }
            PresenceSignal::Left if self.native_drag => return None,
            PresenceSignal::Left => false,
        };
        (self.inside != inside).then(|| {
            self.inside = inside;
            inside
        })
    }

    pub(super) fn begin_native_drag(&mut self) {
        self.native_drag = true;
    }

    /// A hidden window cannot be hovered, whatever the platform last said.
    pub(super) fn hide(&mut self) -> Option<bool> {
        self.native_drag = false;
        std::mem::take(&mut self.inside).then_some(false)
    }
}

impl<Program: RuntimeProgram> WindowManager<Program> {
    pub(super) fn observe_pointer_presence(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        signal: PresenceSignal,
    ) {
        let change = self
            .window_contexts
            .get_mut(&id)
            .and_then(|host| host.pointer_presence.observe(signal));
        if let Some(inside) = change {
            self.deliver_pointer_presence(event_loop, id, inside);
        }
    }

    pub(super) fn begin_native_drag_presence(&mut self, id: WindowId) {
        if let Some(host) = self.window_contexts.get_mut(&id) {
            host.pointer_presence.begin_native_drag();
        }
    }

    pub(super) fn hide_pointer_presence(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId) {
        let change = self
            .window_contexts
            .get_mut(&id)
            .and_then(|host| host.pointer_presence.hide());
        if let Some(inside) = change {
            self.deliver_pointer_presence(event_loop, id, inside);
        }
    }

    fn deliver_pointer_presence(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        inside: bool,
    ) {
        let update = self.program.window_event(
            WindowEvent::PointerPresenceChanged { id, inside },
            &self.context_for(id),
        );
        self.apply_update(event_loop, update, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_reports_changes_only() {
        let mut presence = PointerPresence::default();
        assert_eq!(presence.observe(PresenceSignal::Inside), Some(true));
        assert_eq!(presence.observe(PresenceSignal::Inside), None);
        assert_eq!(presence.observe(PresenceSignal::Left), Some(false));
        assert_eq!(presence.observe(PresenceSignal::Left), None);
    }

    #[test]
    fn native_drag_withholds_its_leave_until_the_pointer_reports_again() {
        let mut presence = PointerPresence::default();
        presence.observe(PresenceSignal::Inside);
        presence.begin_native_drag();
        assert_eq!(presence.observe(PresenceSignal::Left), None);
        assert_eq!(presence.observe(PresenceSignal::Inside), None);
        assert_eq!(presence.observe(PresenceSignal::Left), Some(false));
    }

    #[test]
    fn hiding_leaves_once_and_clears_a_pending_drag() {
        let mut presence = PointerPresence::default();
        assert_eq!(presence.hide(), None);
        presence.observe(PresenceSignal::Inside);
        presence.begin_native_drag();
        assert_eq!(presence.hide(), Some(false));
        assert_eq!(presence.hide(), None);
        assert_eq!(presence.observe(PresenceSignal::Inside), Some(true));
    }

    #[test]
    fn touch_contacts_do_not_hover_and_captured_moves_outside_stay_outside() {
        let client = (200, 100);
        let touch = PointerKind::Touch(winit::event::FingerId::from_raw(1));
        let inside = winit::dpi::PhysicalPosition::new(4.0, 4.0);
        assert_eq!(
            presence_signal(
                &WinitWindowEvent::PointerEntered {
                    device_id: None,
                    position: inside,
                    primary: true,
                    kind: touch,
                },
                client
            ),
            None
        );
        let moved = |x| WinitWindowEvent::PointerMoved {
            device_id: None,
            position: winit::dpi::PhysicalPosition::new(x, 40.0),
            primary: true,
            source: PointerSource::Mouse,
        };
        assert_eq!(
            presence_signal(&moved(120.0), client),
            Some(PresenceSignal::Inside)
        );
        assert_eq!(presence_signal(&moved(260.0), client), None);
        assert_eq!(
            presence_signal(
                &WinitWindowEvent::PointerLeft {
                    device_id: None,
                    position: None,
                    primary: true,
                    kind: PointerKind::Mouse,
                },
                client
            ),
            Some(PresenceSignal::Left)
        );
    }
}

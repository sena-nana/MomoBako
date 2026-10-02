//! Hosted-clock adapter for the backend-neutral Runtime AnimationSystem.

use std::time::{Duration, Instant};

use nana_ui_runtime::{AnimationFrame, AppContext};

/// Maps Runtime's explicit monotonic durations to the host's `Instant` epoch.
/// It owns no timer or redraw policy; hosted programs combine the returned
/// deadline with their other wake sources and consume samples on wake.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeAnimationClock {
    epoch: Instant,
}

impl RuntimeAnimationClock {
    pub fn new(epoch: Instant) -> Self {
        Self { epoch }
    }

    pub fn now() -> Self {
        Self::new(Instant::now())
    }

    pub const fn epoch(self) -> Instant {
        self.epoch
    }

    pub fn runtime_time(self, now: Instant) -> Duration {
        now.saturating_duration_since(self.epoch)
    }

    pub fn next_wakeup(self, context: &AppContext) -> Option<Instant> {
        self.epoch.checked_add(context.next_animation_deadline()?)
    }

    pub fn wake(self, context: &mut AppContext, now: Instant) -> AnimationFrame {
        context.advance_animations(self.runtime_time(now))
    }
}

#[cfg(test)]
mod tests {
    use nana_ui_runtime::{AnimationId, AnimationSpec, Easing, NodeKind};

    use super::*;

    #[derive(Debug)]
    struct View;

    #[test]
    fn maps_runtime_deadline_without_owning_redraw_policy() {
        let epoch = Instant::now();
        let clock = RuntimeAnimationClock::new(epoch);
        let mut context = AppContext::new();
        let document = nana_ui_runtime::DocumentId::new(1).unwrap();
        let view = context
            .create_view(document, NodeKind::Document, View)
            .unwrap();
        context
            .update(view, |_view, cx| {
                let target = cx.entity().stable_id();
                cx.mutations().start_animation(AnimationSpec::new(
                    AnimationId::new(1).unwrap(),
                    target,
                    Duration::from_millis(10),
                    Duration::from_millis(20),
                    Duration::from_millis(5),
                    Easing::Linear,
                ));
            })
            .unwrap();

        assert_eq!(
            clock.next_wakeup(&context),
            epoch.checked_add(Duration::from_millis(10))
        );
        assert!(clock.wake(&mut context, epoch).samples.is_empty());
        let frame = clock.wake(&mut context, epoch + Duration::from_millis(10));
        assert_eq!(frame.samples.len(), 1);
        assert_eq!(frame.next_deadline, Some(Duration::from_millis(15)));
        assert_eq!(
            clock.next_wakeup(&context),
            epoch.checked_add(Duration::from_millis(15))
        );
    }
}

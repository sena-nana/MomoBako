//! Scene host schedule coordination.

use super::*;
use crate::runtime_host::FrameDemand;
use std::num::NonZeroU32;

/// Compositor overlay present cadence. Display refresh still caps vsync.
const COMPOSITOR_PRESENT_HZ: NonZeroU32 = match NonZeroU32::new(120) {
    Some(hz) => hz,
    None => unreachable!(),
};

pub(super) struct HostWorkWake {
    pending: std::sync::atomic::AtomicBool,
    host_thread: std::thread::ThreadId,
    proxy: EventLoopProxy,
}
impl HostWorkWake {
    pub(super) fn new(proxy: EventLoopProxy) -> Self {
        Self {
            pending: std::sync::atomic::AtomicBool::new(false),
            host_thread: std::thread::current().id(),
            proxy,
        }
    }
    pub(super) fn wake(&self) {
        let was_pending = self.pending.swap(true, std::sync::atomic::Ordering::AcqRel);
        // A callback on the host thread is already inside the loop. Re-signalling
        // its native source can starve macOS redraw observers indefinitely.
        if !was_pending && std::thread::current().id() != self.host_thread {
            self.proxy.wake_up();
        }
    }
    fn take_pending(&self) -> bool {
        self.pending
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    }
}

/// Yield between finite batches even when callbacks continuously enqueue more work.
/// One slow callback cannot be preempted, but no further callback starts after the deadline.
pub(super) fn drain_host_batch(
    mut work: impl FnMut() -> bool,
    mut now: impl FnMut() -> Instant,
) -> bool {
    let deadline = now() + Duration::from_millis(2);
    for _ in 0..64 {
        if !work() {
            return false;
        }
        if now() >= deadline {
            return true;
        }
    }
    true
}

impl<Program: RuntimeProgram> WindowManager<Program> {
    pub(super) fn drain_host_work(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.host_work.take_pending();
        self.host_work_deadline = None;
        self.drain_window_requests(event_loop);
        self.apply_pending_icons();
        self.process_startup_requests(event_loop);
        self.complete_file_dialogs(event_loop);
        self.drain_host_messages(event_loop);
        self.drain_browser_events(event_loop);
        // Views' futures whose wakers fired: what they write lands in the
        // next frame of the windows that read it.
        if nana_ui_runtime::view::poll_tasks() > 0 {
            self.request_reactive_redraws(None);
        }
    }
    pub(super) fn drain_host_messages(&mut self, event_loop: &dyn ActiveEventLoop) {
        let remaining = drain_host_batch(
            || {
                if self.shutting_down || event_loop.exiting() {
                    return false;
                }
                // Startup messages go first: they were returned before
                // anything else could be sent.
                let Some(message) = self
                    .startup_messages
                    .pop_front()
                    .or_else(|| self.messages.try_recv().ok())
                else {
                    return false;
                };
                self.process_message(event_loop, message);
                !self.shutting_down && !event_loop.exiting()
            },
            Instant::now,
        );
        if remaining {
            self.host_work.wake();
        } else {
            self.startup_messages_applied();
        }
    }
    pub(super) fn process_message(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        message: Program::Message,
    ) {
        if let Some(id) = self.window_contexts.keys().copied().min() {
            self.bind_after_present.insert(id);
        }
        let update = self.program.update(message, &self.context());
        self.sync_appearance();
        self.apply_update(event_loop, update, None);
    }
    /// Deliver a system reduce-motion change recorded by the window subclass.
    fn sync_reduced_motion(&mut self, event_loop: &dyn ActiveEventLoop) {
        if !nana_window::take_reduced_motion_change() {
            return;
        }
        let Some(reduced) = nana_window::system_reduced_motion() else {
            return;
        };
        if reduced == self.reduced_motion {
            return;
        }
        self.reduced_motion = reduced;
        for id in self.known_window_ids() {
            let update = self.program.window_event(
                WindowEvent::ReducedMotionChanged { id, reduced },
                &self.context_for(id),
            );
            self.apply_update(event_loop, update, None);
        }
    }

    pub(super) fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Pointer moves and wheel deltas merged through this turn route once.
        self.drain_deferred_input(event_loop);
        let now = Instant::now();
        if self
            .host_work_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.drain_host_work(event_loop);
        }
        #[cfg(target_os = "macos")]
        self.unpin_idle_present_transactions();
        let surface_retry_due = self
            .window_contexts
            .values()
            .any(|host| host.surface_retry.is_some_and(|deadline| now >= deadline));
        if surface_retry_due {
            // A device-loss callback may have been waiting on the last in-flight
            // submission when the window was suspended. Poll before local retry.
            let _ = self.graphics.gpu().raw_device().poll(wgpu::PollType::Poll);
        }
        if self.graphics.take_device_lost()
            || self.next_gpu_retry.is_some_and(|deadline| now >= deadline)
        {
            self.recover_device(event_loop);
        }
        if surface_retry_due {
            self.retry_surfaces(now);
        }
        let startup_latch = self.poll_startup_latch(event_loop);
        self.sample_passthrough_forward(event_loop);
        self.sync_reduced_motion(event_loop);
        if self.next_wakeup().is_some_and(|deadline| now >= deadline) {
            self.wake(event_loop, now);
        }
        let frame_deadline = self.schedule_presentations(event_loop, now);
        // Taken here, after everything above has had its chance to enqueue
        // work: a signal raised on the host thread deliberately skips the
        // proxy wake-up, so this deadline is the only thing that brings the
        // loop back for it. Taken before `retry_surfaces` and
        // `schedule_presentations`, whatever they signalled stayed pending
        // with nothing scheduled, and waited for an unrelated event.
        if self.host_work.take_pending() && !self.shutting_down {
            self.host_work_deadline
                .get_or_insert_with(|| Instant::now() + Duration::from_millis(1));
        }
        let next_wakeup = [
            self.next_gpu_retry,
            (!self.render_suspended)
                .then(|| {
                    self.window_contexts
                        .values()
                        .filter_map(|host| host.surface_retry)
                        .min()
                })
                .flatten(),
            self.next_wakeup(),
            frame_deadline,
            self.host_work_deadline,
            self.passthrough_forward_wakeup(),
            startup_latch,
        ]
        .into_iter()
        .flatten()
        .min();
        self.wake_deadline = next_wakeup;
        if !self.embedded {
            event_loop
                .set_control_flow(next_wakeup.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
        }
    }
    pub(super) fn can_present(&self, id: WindowId) -> bool {
        self.window_contexts
            .get(&id)
            .is_some_and(|host| host.surface_retry.is_none())
            && (!self.occluded.contains(&id) || self.startup_presentation_held(id))
            && self.window(id).is_some_and(|window| {
                window.is_visible() != Some(false) && window.is_minimized() != Some(true)
            })
    }

    fn schedule_presentations(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        now: Instant,
    ) -> Option<Instant> {
        let changed = std::mem::take(&mut *self.texture_redraws.lock().expect("texture redraws"));
        for id in changed {
            if self.can_present(id) && !self.startup_holds(id) {
                self.request_redraw(id);
            }
        }
        let mut next = None;
        for id in self.known_window_ids() {
            if self.render_suspended || self.window_contexts[&id].surface_retry.is_some() {
                self.frame_schedules.remove(&id);
                continue;
            }
            let demand = self.window_frame_demand(id);
            let drawable = drawable_surface(self.geometry_of(id).physical_size);
            let due = self.frame_schedules.entry(id).or_default().due(demand, now);
            // A primary window held under its splash gets no periodic frames:
            // nothing it drew could be seen. Explicit redraws still flush it,
            // and the takeover request draws the frame that ends the hold.
            let presents = self.can_present(id) && !self.startup_holds(id);
            let tick = frame_tick(presents, due, drawable);
            let (deadline, armed_at) = match tick {
                FrameTick::Present => {
                    self.request_redraw(id);
                    (
                        self.frame_schedules.entry(id).or_default().arm(demand, now),
                        now,
                    )
                }
                FrameTick::GpuOnly => {
                    let served = self.tick_hidden_gpu(event_loop, id);
                    let armed_at = Instant::now();
                    let demand = self.window_frame_demand(id);
                    let schedule = self.frame_schedules.entry(id).or_default();
                    let deadline = if served {
                        schedule.advance_served(demand, armed_at)
                    } else {
                        schedule.defer(demand, armed_at)
                    };
                    (deadline, armed_at)
                }
                FrameTick::None => (
                    self.frame_schedules.entry(id).or_default().arm(demand, now),
                    now,
                ),
            };
            if let Some(deadline) = frame_wait_target(tick, deadline, armed_at) {
                next = Some(next.map_or(deadline, |old: Instant| old.min(deadline)));
            }
        }
        next
    }

    pub(super) fn animation_deadline(&mut self) -> Option<Instant> {
        self.known_window_ids()
            .into_iter()
            .filter_map(|id| {
                self.program
                    .read_document(id, |document| {
                        self.animation_clock.next_wakeup(document.context())
                    })
                    .flatten()
            })
            .min()
    }
    pub(super) fn next_wakeup(&mut self) -> Option<Instant> {
        match (self.animation_deadline(), self.program.next_wakeup()) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(deadline), None) | (None, Some(deadline)) => Some(deadline),
            (None, None) => None,
        }
    }
    pub(super) fn drain_program_messages(&mut self, id: WindowId) -> RuntimeProgramUpdate {
        let mut update = RuntimeProgramUpdate::default();
        let mut drained = 0usize;
        for _ in 0..MAX_PROGRAM_DISPATCHES {
            let queued = self
                .program
                .write_document(id, |document| {
                    document.context_mut().take_program_messages()
                })
                .unwrap_or_default();
            if queued.is_empty() {
                break;
            }
            self.bind_after_present.insert(id);
            // Depth is what was waiting when the drain began; later rounds
            // are messages the updates themselves sent.
            if drained == 0 {
                drained = queued.len();
            }
            for boxed in queued {
                let Ok(message) = boxed.downcast::<Program::Message>() else {
                    continue;
                };
                update = update.merge(self.program.update(*message, &self.context_for(id)));
            }
        }
        if drained > 0 {
            nana_diagnostics::metric!(
                nana_diagnostics::framework::host::MESSAGE_QUEUE_DEPTH,
                drained
            );
        }
        update
    }
    pub(super) fn drain_all_program_messages(&mut self) -> RuntimeProgramUpdate {
        let mut update = RuntimeProgramUpdate::default();
        for id in self.known_window_ids() {
            update = update.merge(self.drain_program_messages(id));
        }
        update
    }
    pub(super) fn wake(&mut self, event_loop: &dyn ActiveEventLoop, now: Instant) {
        let mut update = self.drain_all_program_messages();
        update = update.merge(self.program.wake(now, &self.context()));
        for id in self.known_window_ids() {
            let due = self
                .program
                .read_document(id, |document| {
                    self.animation_clock
                        .next_wakeup(document.context())
                        .is_some_and(|deadline| deadline <= now)
                })
                .unwrap_or(false);
            if !due {
                continue;
            }
            let frame = self.program.write_document(id, |document| {
                self.animation_clock.wake(document.context_mut(), now)
            });
            let Some(frame) = frame else {
                continue;
            };
            let had_samples = frame.has_updates();
            match self
                .program
                .animation_frame(id, frame, &self.context_for(id))
            {
                Ok(frame_update) => update = update.merge(frame_update),
                Err(error) => {
                    self.program
                        .report_host_failure(HostFailure::AnimationFrame {
                            window: id,
                            error: error.to_string(),
                        });
                }
            }
            if cpu_wake_redraw(true, had_samples) {
                update = update.merge(RuntimeProgramUpdate::redraw(id));
            }
        }
        update = update.merge(self.drain_all_program_messages());
        self.sync_appearance();
        self.apply_update(event_loop, update, None);
    }

    pub(super) fn window_frame_demand(&mut self, id: WindowId) -> FrameDemand {
        let program = self.program.frame_demand(id);
        let compositor = self
            .program
            .read_document(id, |document| document.compositor_needs_tick())
            .unwrap_or(false);
        window_present_demand(program, compositor, self.can_present(id))
    }

    pub(super) fn sync_compositor_clock(&mut self, id: WindowId) {
        let now = self.animation_clock.runtime_time(Instant::now());
        let _ = self.program.write_document(id, |document| {
            if document.compositor_needs_tick() {
                document.sync_presentation_clock(now);
            }
        });
    }

    pub(super) fn bump_surface_generation(&mut self) {
        self.surface_generation = self.surface_generation.wrapping_add(1);
        let generation = self.surface_generation;
        for id in self.known_window_ids() {
            let _ = self.program.write_document(id, |document| {
                document.set_surface_generation(generation);
            });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameTick {
    None,
    Present,
    GpuOnly,
}

pub(super) fn drawable_surface(physical_size: (u32, u32)) -> bool {
    physical_size.0 > 0 && physical_size.1 > 0
}

/// Per-window present demand. Compositor overlay may keep presenting this
/// window without a CPU animation deadline. Hidden/minimized compositor does
/// not drive GpuOnly ticks: resume presents at absolute time.
pub(super) fn window_present_demand(
    program: FrameDemand,
    compositor_needs_tick: bool,
    can_present: bool,
) -> FrameDemand {
    let compositor = if compositor_needs_tick && can_present {
        FrameDemand::Continuous(COMPOSITOR_PRESENT_HZ)
    } else {
        FrameDemand::OnDemand
    };
    merge_frame_demand(program, compositor)
}

pub(super) fn merge_frame_demand(left: FrameDemand, right: FrameDemand) -> FrameDemand {
    match (left, right) {
        (FrameDemand::OnDemand, other) | (other, FrameDemand::OnDemand) => other,
        (FrameDemand::Continuous(left), FrameDemand::Continuous(right)) => {
            FrameDemand::Continuous(left.max(right))
        }
        (FrameDemand::At(left), FrameDemand::At(right)) => FrameDemand::At(left.min(right)),
        (continuous @ FrameDemand::Continuous(_), FrameDemand::At(_))
        | (FrameDemand::At(_), continuous @ FrameDemand::Continuous(_)) => continuous,
    }
}

/// CPU animation wake is per-window. A due deadline on one document does not
/// mark a static sibling for redraw.
pub(super) fn cpu_wake_redraw(deadline_due: bool, frame_has_updates: bool) -> bool {
    deadline_due && frame_has_updates
}

fn frame_tick(can_present: bool, demand_due: bool, drawable: bool) -> FrameTick {
    if !demand_due {
        FrameTick::None
    } else if can_present && drawable {
        FrameTick::Present
    } else {
        FrameTick::GpuOnly
    }
}

fn frame_wait_target(
    tick: FrameTick,
    deadline: Option<Instant>,
    armed_at: Instant,
) -> Option<Instant> {
    if deadline.is_some_and(|deadline| deadline > armed_at) {
        deadline
    } else if tick == FrameTick::GpuOnly {
        Some(armed_at)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameTick, drawable_surface, frame_tick, frame_wait_target};
    use crate::runtime_host::{FrameDemand, FrameSchedule};
    use std::time::{Duration, Instant};

    #[test]
    fn demand_ticks_only_present_with_a_drawable_visible_surface() {
        let now = Instant::now();
        for (visible, drawable, expected) in [
            (false, false, FrameTick::GpuOnly),
            (false, true, FrameTick::GpuOnly),
            (true, false, FrameTick::GpuOnly),
            (true, true, FrameTick::Present),
        ] {
            assert_eq!(frame_tick(visible, true, drawable), expected);
            let mut schedule = FrameSchedule::default();
            let demand = FrameDemand::OnDemand;
            let tick = frame_tick(visible, schedule.due(demand, now), drawable);
            assert_eq!(tick, FrameTick::None);
            assert_eq!(
                frame_wait_target(tick, schedule.arm(demand, now), now),
                None
            );
        }
    }

    #[test]
    fn zero_size_continuous_ticks_keep_waking_and_resume_presenting() {
        let demand = FrameDemand::Continuous(std::num::NonZeroU32::new(60).unwrap());
        for size in [(0, 100), (200, 0), (0, 0)] {
            for can_present in [false, true] {
                let mut schedule = FrameSchedule::default();
                let mut now = Instant::now();
                for _ in 0..8 {
                    let tick = frame_tick(
                        can_present,
                        schedule.due(demand, now),
                        drawable_surface(size),
                    );
                    assert_eq!(tick, FrameTick::GpuOnly);
                    let deadline = schedule.advance_served(demand, now);
                    let next = frame_wait_target(tick, deadline, now).expect("continuous wakeup");
                    assert!(next > now);
                    assert!(!schedule.due(demand, now));
                    now = next;
                }
                assert_eq!(
                    frame_tick(true, schedule.due(demand, now), true),
                    FrameTick::Present
                );
            }
        }
    }

    #[test]
    fn gpu_only_at_tracks_post_prepare_demand_and_failure_retry() {
        let now = Instant::now();
        let later = now + Duration::from_millis(50);
        let demand = FrameDemand::At(now);
        for (visible, drawable) in [(false, false), (false, true), (true, false)] {
            for (after, expected_deadline, expected_wake) in [
                (FrameDemand::OnDemand, None, now),
                (FrameDemand::At(later), Some(later), later),
                (demand, Some(now), now),
            ] {
                let mut schedule = FrameSchedule::default();
                let tick = frame_tick(visible, schedule.due(demand, now), drawable);
                assert_eq!(tick, FrameTick::GpuOnly);
                let deadline = schedule.advance_served(after, now);
                assert_eq!(deadline, expected_deadline);
                assert_eq!(frame_wait_target(tick, deadline, now), Some(expected_wake));
                // Cleared demand settles to idle; retained At work stays armed.
                assert_eq!(
                    schedule.due(after, expected_wake),
                    expected_deadline.is_some()
                );
            }
            let mut schedule = FrameSchedule::default();
            let tick = frame_tick(visible, schedule.due(demand, now), drawable);
            let retry = frame_wait_target(tick, schedule.defer(demand, now), now).unwrap();
            assert!(retry > now);
            assert!(!schedule.due(demand, now));
            assert!(schedule.due(demand, retry));
        }
    }

    #[test]
    fn gpu_only_waits_at_armed_when_deadline_is_not_after_now() {
        let now = Instant::now();
        assert_eq!(
            frame_wait_target(FrameTick::GpuOnly, Some(now), now),
            Some(now)
        );
        assert_eq!(frame_wait_target(FrameTick::Present, Some(now), now), None);
        let later = now + Duration::from_millis(16);
        assert_eq!(
            frame_wait_target(FrameTick::GpuOnly, Some(later), now),
            Some(later)
        );
    }

    #[test]
    fn zero_size_is_not_a_presentable_surface() {
        assert!(!drawable_surface((0, 0)));
        assert!(!drawable_surface((0, 100)));
        assert!(!drawable_surface((200, 0)));
        assert!(drawable_surface((200, 100)));
    }

    fn fps(n: u32) -> FrameDemand {
        FrameDemand::Continuous(std::num::NonZeroU32::new(n).unwrap())
    }

    #[test]
    fn compositor_tick_is_lightweight_present_not_cpu_wake() {
        let demand = super::window_present_demand(FrameDemand::OnDemand, true, true);
        assert_eq!(demand, fps(120));
        assert!(!super::cpu_wake_redraw(false, false));
        assert!(!super::cpu_wake_redraw(false, true));
    }

    #[test]
    fn compositor_tick_does_not_force_hidden_gpu_catch_up() {
        let hidden = super::window_present_demand(FrameDemand::OnDemand, true, false);
        assert_eq!(hidden, FrameDemand::OnDemand);
        assert_eq!(
            super::window_present_demand(fps(60), true, false),
            fps(60),
            "external GPU cadence still ticks when hidden; compositor does not add one"
        );
        let now = Instant::now();
        let schedule = FrameSchedule::default();
        assert_eq!(
            frame_tick(false, schedule.due(hidden, now), true),
            FrameTick::None
        );
    }

    #[test]
    fn static_window_is_not_redrawn_by_a_sibling_cpu_deadline() {
        let animated = super::cpu_wake_redraw(true, true);
        let static_window = super::cpu_wake_redraw(false, false);
        assert!(animated);
        assert!(!static_window);
        assert_eq!(
            super::window_present_demand(FrameDemand::OnDemand, false, true),
            FrameDemand::OnDemand
        );
    }

    #[test]
    fn compositor_present_merges_with_program_gpu_cadence() {
        assert_eq!(super::window_present_demand(fps(30), true, true), fps(120));
        assert_eq!(super::window_present_demand(fps(240), true, true), fps(240));
        let later = Instant::now() + Duration::from_millis(50);
        assert_eq!(
            super::window_present_demand(FrameDemand::At(later), true, true),
            fps(120)
        );
        assert_eq!(
            super::window_present_demand(FrameDemand::At(later), false, true),
            FrameDemand::At(later)
        );
    }

    #[test]
    fn compositor_continuous_skips_missed_ticks_on_resume() {
        let demand = super::window_present_demand(FrameDemand::OnDemand, true, true);
        let mut schedule = FrameSchedule::default();
        let start = Instant::now();
        assert!(schedule.due(demand, start));
        let _ = schedule.advance_served(demand, start);
        let resumed = start + Duration::from_secs(2);
        assert!(schedule.due(demand, resumed));
        let next = schedule.advance_served(demand, resumed).expect("cadence");
        assert!(next > resumed);
        assert!(!schedule.due(demand, resumed));
        assert_eq!(
            frame_tick(true, schedule.due(demand, resumed), true),
            FrameTick::None
        );
    }
}

#[cfg(test)]
mod queue_fairness_tests {
    use super::*;
    #[test]
    fn self_replenishing_producer_yields_without_losing_fifo_order() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(0).unwrap();
        let mut consumed = Vec::new();
        let now = Instant::now();
        assert!(drain_host_batch(
            || {
                let value = rx.try_recv().unwrap();
                consumed.push(value);
                tx.send(value + 1).unwrap();
                true
            },
            || now
        ));
        assert_eq!(consumed, (0..64).collect::<Vec<_>>());
        assert_eq!(rx.try_recv().unwrap(), 64);
    }
    #[test]
    fn slow_callback_yields_before_starting_another() {
        let start = Instant::now();
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let calls = std::cell::Cell::new(0);
        assert!(drain_host_batch(
            || {
                calls.set(calls.get() + 1);
                elapsed.set(Duration::from_millis(3));
                true
            },
            || start + elapsed.get()
        ));
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn empty_or_stopped_queue_does_not_schedule_another_wake() {
        assert!(!drain_host_batch(|| false, Instant::now));
    }
}

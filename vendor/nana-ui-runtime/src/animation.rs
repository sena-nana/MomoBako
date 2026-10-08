use std::{hash::Hasher, time::Duration};

use crate::StableNodeId;

pub use nana_ui_core::motion::{
    AnimatableProperty, AnimationClass, AnimationDirection, AnimationFillMode, AnimationIteration,
    AnimationPlayState, AnimationPlayback, CompiledMotion, DecayParams, Easing, FlipRect, Keyframe,
    MOTION_DESCRIPTOR_VERSION, MotionCodecError, MotionCodecId, MotionCodecInfo,
    MotionCodecRegistry, MotionCurve, MotionDescriptor, MotionDescriptorError,
    MotionDescriptorStore, MotionEvaluatorBackend, MotionGraph, MotionHandle, MotionInspectorEntry,
    MotionInterrupt, MotionLayer, MotionSample, MotionTargetId, MotionTiming, MotionTo,
    MotionTrack, MotionTrackId, MotionValue, MotionValueKind, MotionWorkCounters,
    PresentationOverlay, PresentationPair, PresentationSlot, PresentationStore, Spring,
    SpringParams, StepJump, Timeline, classify_animatable_property, compile_motion_descriptor,
    cpu_fallback_reason, decode_motion_track, evaluate_descriptor, evaluate_progress,
    evaluate_track, evaluate_track_at, invert_flip_translate, is_font_variation_settings,
    retarget_track, track_completion_deadline,
};

/// Stable identity for one logical animation. Starting the same ID again
/// atomically replaces its active timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnimationId(u64);

impl AnimationId {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn track_id(self) -> MotionTrackId {
        MotionTrackId::new(self.0).expect("AnimationId is nonzero")
    }
}

impl From<AnimationId> for MotionTrackId {
    fn from(id: AnimationId) -> Self {
        id.track_id()
    }
}

/// Component-kind tags for [`component_animation_id`]. Every component family
/// that starts its own timelines reserves one stable non-zero constant here,
/// so hashed IDs stay in per-component namespaces instead of colliding across
/// component types whose node IDs share one numeric space.
pub mod component_animation_kinds {
    /// Skeleton pulse timeline.
    pub const SKELETON: u64 = 1;
    pub const SWITCH: u64 = 2;
    pub const HOVER: u64 = 3;
    pub const SURFACE: u64 = 4;
    /// Spinner rotation timeline.
    pub const SPINNER: u64 = 5;
    /// Menu / overlay pop transform (paired with [`SURFACE`] opacity).
    pub const SURFACE_POP: u64 = 6;
    /// Sidebar section height / progress.
    pub const SIDEBAR: u64 = 7;
    /// Workspace region collapse / expand.
    pub const WORKSPACE: u64 = 8;
    /// Button / switch / card loading indicator.
    pub const LOADING: u64 = 9;
    /// TransitionGroup / list-move FLIP compositor transform.
    pub const FLIP: u64 = 10;
}

/// Derives the animation ID for one component-owned timeline from the
/// component's kind tag and its node. Re-deriving the same pair yields the
/// same ID (restarting replaces that timeline); different component kinds on
/// identically numbered nodes never replace each other. Returns `None` in the
/// negligible case of a zero hash, which [`AnimationId`] rejects.
pub fn component_animation_id(kind_tag: u64, target: StableNodeId) -> Option<AnimationId> {
    let mut hasher = crate::IdHasher::default();
    hasher.write_u64(kind_tag);
    hasher.write_u64(target.get());
    AnimationId::new(hasher.finish())
}

/// Infinite loading-indicator timeline (button / switch / card).
pub fn loading_animation(id: StableNodeId, start: Duration) -> Option<AnimationSpec> {
    let animation = component_animation_id(component_animation_kinds::LOADING, id)?;
    Some(
        AnimationSpec::new(
            animation,
            id,
            start,
            nana_ui_core::motion::LOADING_SPIN,
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_playback(AnimationPlayback {
            iteration_count: AnimationIteration::INFINITE,
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::None,
            play_state: AnimationPlayState::Running,
            paused_at: None,
        }),
    )
}

/// Layout-class workspace collapse/expand scheduler. Extent values stay on
/// [`nana_ui_core::WorkspaceModel`]; this track is the deadline authority.
pub fn workspace_animation(id: StableNodeId, start: Duration) -> Option<AnimationSpec> {
    let animation = component_animation_id(component_animation_kinds::WORKSPACE, id)?;
    Some(AnimationSpec::new(
        animation,
        id,
        start,
        nana_ui_core::WORKSPACE_REGION_TRANSITION_DURATION,
        Duration::from_millis(16),
        Easing::EaseInOutCubic,
    ))
}

/// Compositor invert hold: visual First, layout already Last.
pub fn layout_flip_hold_spec(
    target: StableNodeId,
    invert: nana_ui_core::PaintTransform,
    now: Duration,
) -> Option<AnimationSpec> {
    let id = component_animation_id(component_animation_kinds::FLIP, target)?;
    Some(
        AnimationSpec::new(
            id,
            target,
            now,
            Duration::from_nanos(1),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_property(AnimatableProperty::Transform)
        .with_range(
            MotionValue::Transform(invert),
            MotionTo::Value(MotionValue::Transform(invert)),
        )
        .with_playback(AnimationPlayback::running(
            AnimationIteration::ONCE,
            AnimationDirection::Normal,
            AnimationFillMode::Both,
        )),
    )
}

/// Play invert → identity on the FLIP compositor track.
pub fn layout_flip_play_spec(
    target: StableNodeId,
    invert: nana_ui_core::PaintTransform,
    now: Duration,
    duration: Duration,
    easing: Easing,
) -> Option<AnimationSpec> {
    let id = component_animation_id(component_animation_kinds::FLIP, target)?;
    Some(
        AnimationSpec::new(id, target, now, duration, Duration::from_millis(16), easing)
            .with_property(AnimatableProperty::Transform)
            .with_range(
                MotionValue::Transform(invert),
                MotionTo::Value(MotionValue::Transform(
                    nana_ui_core::PaintTransform::default(),
                )),
            )
            .with_interrupt(MotionInterrupt::Replace),
    )
}

/// One-shot First → Last → Invert → Play. Layout boxes stay at `last`.
pub fn layout_flip_spec(
    target: StableNodeId,
    first: FlipRect,
    last: FlipRect,
    now: Duration,
    duration: Duration,
    easing: Easing,
) -> Option<AnimationSpec> {
    layout_flip_play_spec(
        target,
        invert_flip_translate(first, last),
        now,
        duration,
        easing,
    )
}

/// Backend-neutral animation timing. This is the Motion IR timing subset:
/// [`MotionTiming`] + [`AnimationPlayback`] are stored once (no parallel
/// start/duration/playback fields). [`AnimationSpec::new`] still builds a
/// progress sample for the existing CPU deadline path; property / curve /
/// delay / keyframes compile through the same struct without forcing
/// `Progress` / Paint.
///
/// Use [`AnimationSpec::new`] for the six-field one-shot API, then
/// [`AnimationSpec::with_playback`] / [`AnimationSpec::with_property`].
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationSpec {
    pub id: AnimationId,
    pub target: StableNodeId,
    pub timing: MotionTiming,
    pub playback: AnimationPlayback,
    pub curve: MotionCurve,
    pub property: AnimatableProperty,
    pub from: MotionValue,
    pub to: MotionTo,
    pub velocity: MotionValue,
    /// Same-id start: replace the previous timeline, or retarget from the
    /// current presentation sample.
    pub interrupt: MotionInterrupt,
    /// Which layer wins when another track animates the property at once.
    pub layer: MotionLayer,
}

impl AnimationSpec {
    /// Six-field constructor. Playback is one-shot / normal / none / running.
    /// Property is unit progress so existing CPU samples stay a 0..=1 track.
    pub const fn new(
        id: AnimationId,
        target: StableNodeId,
        start: Duration,
        duration: Duration,
        frame_interval: Duration,
        easing: Easing,
    ) -> Self {
        Self {
            id,
            target,
            timing: MotionTiming::new(start, duration, frame_interval),
            playback: AnimationPlayback {
                iteration_count: AnimationIteration::ONCE,
                direction: AnimationDirection::Normal,
                fill_mode: AnimationFillMode::None,
                play_state: AnimationPlayState::Running,
                paused_at: None,
            },
            curve: MotionCurve::Easing(easing),
            property: AnimatableProperty::Progress,
            from: MotionValue::Scalar(0.0),
            to: MotionTo::Value(MotionValue::Scalar(1.0)),
            velocity: MotionValue::Scalar(0.0),
            interrupt: MotionInterrupt::Replace,
            layer: MotionLayer::Runtime,
        }
    }

    pub fn with_playback(mut self, playback: AnimationPlayback) -> Self {
        self.playback = playback;
        self
    }

    pub fn with_property(mut self, property: AnimatableProperty) -> Self {
        self.property = property;
        self
    }

    pub fn with_curve(mut self, curve: MotionCurve) -> Self {
        self.curve = curve;
        self
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.timing.delay = delay;
        self
    }

    pub fn with_range(mut self, from: MotionValue, to: MotionTo) -> Self {
        self.from = from;
        self.to = to;
        self
    }

    pub fn with_interrupt(mut self, interrupt: MotionInterrupt) -> Self {
        self.interrupt = interrupt;
        self
    }

    pub fn with_layer(mut self, layer: MotionLayer) -> Self {
        self.layer = layer;
        self
    }

    /// Whether samples of this track live in the presentation overlay:
    /// compositor properties, read at paint, and font axes, read when the
    /// style resolves.
    pub fn has_overlay(&self) -> bool {
        self.uses_presentation_overlay() || matches!(self.property, AnimatableProperty::FontAxis(_))
    }

    /// Overlay + descriptor path for compositor-safe properties only.
    /// Paint / Layout / Progress stay on the CPU sample clock.
    pub fn uses_presentation_overlay(&self) -> bool {
        self.property.animation_class() == AnimationClass::Compositor
    }

    /// Compositor-safe tracks wake on start/completion, not `frame_interval`.
    pub fn uses_completion_deadline_only(&self) -> bool {
        self.property.animation_class() == AnimationClass::Compositor
    }

    /// Compile without dropping property, class, delay, curve, or keyframes.
    pub fn to_motion_track(&self) -> Option<MotionTrack> {
        Some(MotionTrack {
            id: self.id.track_id(),
            target: MotionTargetId::new(self.target.get())?,
            property: self.property,
            from: self.from,
            to: self.to.clone(),
            timing: self.timing,
            curve: self.curve,
            playback: self.playback,
            velocity: self.velocity,
        })
    }

    pub(crate) fn end(&self) -> Option<Duration> {
        self.timing.end(self.playback)
    }

    pub(crate) fn is_valid(&self) -> bool {
        self.to_motion_track().is_some_and(|track| track.is_valid())
    }

    fn running(&self) -> bool {
        self.playback.play_state == AnimationPlayState::Running
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationSample {
    pub id: AnimationId,
    pub target: StableNodeId,
    /// Eased progress in the inclusive range `0.0..=1.0`.
    pub progress: f32,
    pub finished: bool,
    /// False when fill-mode does not apply a presentation value at `now`.
    pub applies: bool,
    pub property: AnimatableProperty,
    pub value: MotionValue,
}

impl AnimationSample {
    /// Presentation value only when [`Self::applies`] is true.
    pub fn applied_value(self) -> Option<MotionValue> {
        self.applies.then_some(self.value)
    }
}

/// Runtime-side finished / cancelled hook. Vue JS `transitionend` /
/// `animationend` stay on Workstream F / #63.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationEventKind {
    Finished,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationEvent {
    pub id: AnimationId,
    pub target: StableNodeId,
    pub kind: AnimationEventKind,
}

/// Samples due at the supplied timestamp and the next time the host should
/// wake the UI runtime. An empty static UI has no deadline.
/// `animation_deadlines_scanned` / `animations_considered` count deadline-index
/// entries examined and animation records looked up this call.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationFrame {
    pub samples: Vec<AnimationSample>,
    /// Framework-owned component lifecycle updates applied at this wake.
    /// These are already committed to retained state; applications must not
    /// re-apply them.
    pub component_updates: Vec<StableNodeId>,
    /// Completion / cancel hooks produced from deadlines or mutations since
    /// the previous advance. Not derived by scanning idle tracks.
    pub events: Vec<AnimationEvent>,
    pub next_deadline: Option<Duration>,
    pub animation_deadlines_scanned: usize,
    pub animations_considered: usize,
}

impl AnimationFrame {
    pub fn has_updates(&self) -> bool {
        !self.samples.is_empty() || !self.component_updates.is_empty() || !self.events.is_empty()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveAnimation {
    pub(crate) spec: AnimationSpec,
    pub(crate) next_deadline: Duration,
}

impl ActiveAnimation {
    pub(crate) fn new(spec: AnimationSpec) -> Self {
        let next_deadline = if spec.playback.fill_mode.applies_backwards() {
            Duration::ZERO
        } else {
            spec.timing.effective_start().unwrap_or(spec.timing.start)
        };
        Self {
            spec,
            next_deadline,
        }
    }

    pub(crate) fn has_follow_up_deadline(&self) -> bool {
        self.spec.running()
    }

    pub(crate) fn sample(&mut self, now: Duration) -> Option<AnimationSample> {
        if now < self.next_deadline {
            return None;
        }
        let track = self.spec.to_motion_track()?;
        let motion = evaluate_track(&track, now);
        let finished = motion.finished;
        if !finished && self.has_follow_up_deadline() {
            self.next_deadline = follow_up_deadline(&self.spec, &track, now);
        } else if !finished {
            // Paused hold: stay in the map, but do not wake again until replaced.
            self.next_deadline = Duration::MAX;
        }
        Some(AnimationSample {
            id: self.spec.id,
            target: self.spec.target,
            progress: motion.progress,
            finished,
            applies: motion.applies,
            property: self.spec.property,
            value: motion.value,
        })
    }
}

fn follow_up_deadline(spec: &AnimationSpec, track: &MotionTrack, now: Duration) -> Duration {
    if spec.uses_completion_deadline_only() {
        return track_completion_deadline(track)
            .filter(|end| *end > now)
            .unwrap_or(Duration::MAX);
    }
    let step = now.checked_add(spec.timing.frame_interval).unwrap_or(now);
    match spec.end() {
        Some(end) => step.min(end),
        None => step,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StableNodeId;

    fn spec(start_ms: u64, duration_ms: u64) -> AnimationSpec {
        AnimationSpec::new(
            AnimationId::new(1).unwrap(),
            StableNodeId::new(1).unwrap(),
            Duration::from_millis(start_ms),
            Duration::from_millis(duration_ms),
            Duration::from_millis(10),
            Easing::Linear,
        )
    }

    fn progress_at(spec: &AnimationSpec, now_ms: u64) -> AnimationSample {
        let mut active = ActiveAnimation::new(spec.clone());
        active.next_deadline = Duration::ZERO;
        active
            .sample(Duration::from_millis(now_ms))
            .expect("sample")
    }

    #[test]
    fn default_animation_playback_is_one_shot_normal_running() {
        let spec = spec(100, 100);
        assert_eq!(spec.playback.iteration_count, AnimationIteration::ONCE);
        assert_eq!(spec.playback.direction, AnimationDirection::Normal);
        assert_eq!(spec.playback.fill_mode, AnimationFillMode::None);
        assert_eq!(spec.playback.play_state, AnimationPlayState::Running);
        assert_eq!(spec.end(), Some(Duration::from_millis(200)));
        let mid = progress_at(&spec, 150);
        assert!((mid.progress - 0.5).abs() < f32::EPSILON);
        assert!(!mid.finished);
        let end = progress_at(&spec, 200);
        assert_eq!(end.progress, 1.0);
        assert!(end.finished);
    }

    #[test]
    fn animation_iteration_count_repeats_before_finish() {
        let spec = spec(0, 100).with_playback(AnimationPlayback::running(
            AnimationIteration::Count(2),
            AnimationDirection::Normal,
            AnimationFillMode::None,
        ));
        let first = progress_at(&spec, 50);
        assert!((first.progress - 0.5).abs() < f32::EPSILON);
        assert!(!first.finished);
        let second = progress_at(&spec, 150);
        assert!((second.progress - 0.5).abs() < f32::EPSILON);
        assert!(!second.finished);
        let done = progress_at(&spec, 200);
        assert_eq!(done.progress, 1.0);
        assert!(done.finished);
    }

    #[test]
    fn infinite_animation_never_finishes() {
        let spec = spec(0, 100).with_playback(AnimationPlayback::running(
            AnimationIteration::INFINITE,
            AnimationDirection::Normal,
            AnimationFillMode::None,
        ));
        assert_eq!(spec.end(), None);
        assert!(spec.is_valid());
        let late = progress_at(&spec, 10_000);
        assert!(!late.finished);
        assert!(late.progress >= 0.0 && late.progress <= 1.0);
    }

    #[test]
    fn reverse_direction_runs_from_one_to_zero() {
        let spec = spec(0, 100).with_playback(AnimationPlayback::running(
            AnimationIteration::ONCE,
            AnimationDirection::Reverse,
            AnimationFillMode::None,
        ));
        assert!((progress_at(&spec, 0).progress - 1.0).abs() < f32::EPSILON);
        assert!((progress_at(&spec, 50).progress - 0.5).abs() < f32::EPSILON);
        let done = progress_at(&spec, 100);
        assert_eq!(done.progress, 0.0);
        assert!(done.finished);
    }

    #[test]
    fn alternate_direction_flips_each_iteration() {
        let spec = spec(0, 100).with_playback(AnimationPlayback::running(
            AnimationIteration::Count(2),
            AnimationDirection::Alternate,
            AnimationFillMode::None,
        ));
        assert!((progress_at(&spec, 25).progress - 0.25).abs() < f32::EPSILON);
        assert!((progress_at(&spec, 125).progress - 0.75).abs() < f32::EPSILON);
        let done = progress_at(&spec, 200);
        assert_eq!(done.progress, 0.0);
        assert!(done.finished);
    }

    #[test]
    fn fill_mode_backwards_holds_start_progress_during_delay() {
        let spec = spec(50, 100).with_playback(AnimationPlayback::running(
            AnimationIteration::ONCE,
            AnimationDirection::Reverse,
            AnimationFillMode::Backwards,
        ));
        let held = progress_at(&spec, 10);
        assert_eq!(held.progress, 1.0);
        assert!(!held.finished);
    }

    #[test]
    fn fill_mode_forwards_keeps_terminal_progress() {
        let spec = spec(0, 100).with_playback(AnimationPlayback::running(
            AnimationIteration::ONCE,
            AnimationDirection::Reverse,
            AnimationFillMode::Forwards,
        ));
        let done = progress_at(&spec, 150);
        assert_eq!(done.progress, 0.0);
        assert!(done.finished);
    }

    #[test]
    fn paused_animation_does_not_schedule_further_frames() {
        let spec = spec(0, 100).with_playback(AnimationPlayback {
            iteration_count: AnimationIteration::ONCE,
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::None,
            play_state: AnimationPlayState::Paused,
            paused_at: Some(Duration::ZERO),
        });
        let mut active = ActiveAnimation::new(spec);
        let first = active
            .sample(Duration::from_millis(0))
            .expect("paused still emits the hold sample");
        assert_eq!(first.progress, 0.0);
        assert!(!first.finished);
        assert!(active.sample(Duration::from_millis(50)).is_none());
    }

    #[test]
    fn zero_iterations_is_invalid() {
        assert!(
            !spec(0, 100)
                .with_playback(AnimationPlayback::running(
                    AnimationIteration::Count(0),
                    AnimationDirection::Normal,
                    AnimationFillMode::None,
                ))
                .is_valid()
        );
    }

    #[test]
    fn runtime_sidebar_and_workspace_motion_share_ir_duration() {
        assert_eq!(
            crate::SidebarSectionState::animation_duration(),
            nana_ui_core::motion::SIDEBAR_COLLAPSE
        );
        assert_eq!(
            nana_ui_core::WORKSPACE_REGION_TRANSITION_DURATION,
            nana_ui_core::motion::SIDEBAR_COLLAPSE
        );
    }

    #[test]
    fn component_animation_ids_are_stable_and_split_by_kind() {
        let node_a = StableNodeId::new(7).unwrap();
        let node_b = StableNodeId::new(8).unwrap();
        let skeleton = component_animation_id(component_animation_kinds::SKELETON, node_a);
        assert_eq!(
            skeleton,
            component_animation_id(component_animation_kinds::SKELETON, node_a),
            "re-deriving the same pair must address the same timeline"
        );
        assert_ne!(
            skeleton,
            component_animation_id(component_animation_kinds::SKELETON, node_b)
        );
        let other_kind = component_animation_id(component_animation_kinds::SKELETON + 1, node_a);
        assert_ne!(
            skeleton, other_kind,
            "another component kind on the same node needs its own namespace"
        );
        let skeleton = skeleton.unwrap();
        assert_ne!(
            Some(skeleton),
            AnimationId::new(node_a.get()),
            "hashed IDs must not land in the raw node-ID namespace"
        );
    }

    #[test]
    fn evaluate_progress_matches_evaluate_track_on_a_paused_spec() {
        let spec = spec(0, 100).with_playback(AnimationPlayback {
            iteration_count: AnimationIteration::ONCE,
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::None,
            play_state: AnimationPlayState::Paused,
            paused_at: Some(Duration::from_millis(50)),
        });
        let track = spec.to_motion_track().expect("track");
        let now = Duration::from_millis(90);
        let progress = evaluate_progress(spec.timing, spec.playback, spec.curve, now);
        let motion = evaluate_track(&track, now);
        assert!((progress.progress - 0.5).abs() < 1e-5);
        assert_eq!(progress.progress, motion.progress);
        assert_eq!(progress.finished, motion.finished);
        let runtime = progress_at(&spec, 90);
        assert!((runtime.progress - 0.5).abs() < 1e-5);
        assert_eq!(runtime.finished, motion.finished);
    }

    #[test]
    fn animation_spec_progress_matches_motion_track_evaluator() {
        let spec = spec(0, 100)
            .with_curve(MotionCurve::Easing(Easing::EaseOutCubic))
            .with_playback(AnimationPlayback::running(
                AnimationIteration::Count(2),
                AnimationDirection::Alternate,
                AnimationFillMode::None,
            ));
        let track = spec.to_motion_track().expect("track");
        for ms in [0, 25, 100, 125, 200] {
            let sample = progress_at(&spec, ms);
            let motion = evaluate_track(&track, Duration::from_millis(ms));
            assert_eq!(sample.finished, motion.finished);
            assert!((sample.progress - motion.progress).abs() < 1e-6);
            match motion.value {
                MotionValue::Scalar(v) => {
                    assert!((v - motion.progress).abs() < 1e-6);
                }
                other => panic!("progress track must be scalar, got {other:?}"),
            }
        }
    }

    #[test]
    fn to_motion_track_preserves_property_delay_and_keyframes() {
        let spec = spec(10, 100)
            .with_delay(Duration::from_millis(40))
            .with_property(AnimatableProperty::Opacity)
            .with_curve(MotionCurve::Easing(Easing::EaseOutCubic))
            .with_range(
                MotionValue::Scalar(0.2),
                MotionTo::Keyframes(vec![
                    Keyframe {
                        offset: 0.0,
                        value: MotionValue::Scalar(0.2),
                        easing: None,
                    },
                    Keyframe {
                        offset: 1.0,
                        value: MotionValue::Scalar(0.8),
                        easing: None,
                    },
                ]),
            );
        let track = spec.to_motion_track().expect("track");
        assert_eq!(track.property, AnimatableProperty::Opacity);
        assert_eq!(track.execution_class(), AnimationClass::Compositor);
        assert_eq!(track.timing.delay, Duration::from_millis(40));
        assert_eq!(track.curve, MotionCurve::Easing(Easing::EaseOutCubic));
        assert!(matches!(track.to, MotionTo::Keyframes(_)));
        assert_ne!(track.property, AnimatableProperty::Progress);
        assert_ne!(track.execution_class(), AnimationClass::Paint);
    }

    #[test]
    fn sample_forwards_applies_and_applied_value_is_none_outside_fill() {
        let spec = spec(50, 100);
        let held = progress_at(&spec, 10);
        assert!(!held.applies);
        assert_eq!(held.applied_value(), None);
        assert_eq!(held.property, AnimatableProperty::Progress);
        let mid = progress_at(&spec, 100);
        assert!(mid.applies);
        assert_eq!(mid.applied_value(), Some(MotionValue::Scalar(0.5)));
    }

    #[test]
    fn presentation_overlay_is_compositor_only() {
        assert!(!spec(0, 100).uses_presentation_overlay());
        assert!(
            spec(0, 100)
                .with_property(AnimatableProperty::Opacity)
                .uses_presentation_overlay()
        );
        assert!(
            spec(0, 100)
                .with_property(AnimatableProperty::Transform)
                .uses_presentation_overlay()
        );
        assert!(
            !spec(0, 100)
                .with_property(AnimatableProperty::Width)
                .uses_presentation_overlay()
        );
        assert!(
            !spec(0, 100)
                .with_property(AnimatableProperty::Color)
                .uses_presentation_overlay()
        );
    }
}

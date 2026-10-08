use std::sync::Arc;
use std::time::Duration;

use crate::{
    AccessibilityRole, AccessibilityState, AlignSpec, AnimationDirection, AnimationFillMode,
    AnimationIteration, AnimationPlayState, AnimationPlayback, AnimationSpec, ComponentView,
    Easing, FlexDirection, InteractionState, JustifySpec, LengthSpec, MutationQueue, NodeKind,
    NodeStyle, SemanticColorRole, StableNodeId, StandardVisual, TextContent, TextVerticalAlignment,
    UiWorld, component_animation_id, component_animation_kinds,
};
use nana_ui_core::Icon;

fn project_visual(
    id: StableNodeId,
    world: &UiWorld,
    mutations: &mut MutationQueue,
    text: &str,
    style: NodeStyle,
    visual: StandardVisual,
    accessibility: AccessibilityState,
) {
    if world.text(id) != Some(text) {
        mutations.set_text(
            id,
            TextContent {
                value: text.to_owned().into(),
            },
        );
    }
    if world.node_style(id) != Some(&style) {
        mutations.set_style(id, style);
    }
    if world.standard_visual(id) != Some(visual.clone()) {
        mutations.set_standard_visual(id, Some(visual));
    }
    let interaction = InteractionState {
        pointer_events: false,
        focusable: false,
    };
    if world.interaction(id) != Some(interaction) {
        mutations.set_interaction(id, interaction);
    }
    project_accessibility(id, world, mutations, accessibility);
}

fn project_accessibility(
    id: StableNodeId,
    world: &UiWorld,
    mutations: &mut MutationQueue,
    accessibility: AccessibilityState,
) {
    if world.accessibility(id) != Some(&accessibility) {
        mutations.set_accessibility(id, accessibility);
    }
}

fn status_foreground(tone: nana_ui_core::StatusTone) -> SemanticColorRole {
    crate::components::status_tone_role(tone)
}

fn status_background(tone: nana_ui_core::StatusTone) -> SemanticColorRole {
    match tone {
        nana_ui_core::StatusTone::Neutral => SemanticColorRole::Subtle,
        nana_ui_core::StatusTone::Info => SemanticColorRole::AccentSoft,
        nana_ui_core::StatusTone::Success => SemanticColorRole::Subtle,
        nana_ui_core::StatusTone::Warning => SemanticColorRole::WarningSoft,
        nana_ui_core::StatusTone::Danger => SemanticColorRole::Subtle,
    }
}

/// Compact semantic status text. It is descriptive and never owns an action.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusBadge {
    pub label: Arc<str>,
    pub tone: nana_ui_core::StatusTone,
    pub compact: bool,
    pub style: NodeStyle,
}

impl StatusBadge {
    pub fn new(label: impl Into<Arc<str>>, tone: nana_ui_core::StatusTone) -> Self {
        Self {
            label: label.into(),
            tone,
            compact: true,
            style: NodeStyle::default(),
        }
    }

    pub fn tone(mut self, tone: nana_ui_core::StatusTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        style.foreground = Some(status_foreground(self.tone));
        style.background = Some(status_background(self.tone));
        style.border = None;
        let layout = Arc::make_mut(&mut style.layout);
        let (horizontal, vertical, indicator, gap) = if self.compact {
            (
                nana_ui_core::space::SM,
                nana_ui_core::space::XXS,
                nana_ui_core::space::SM,
                nana_ui_core::space::XS,
            )
        } else {
            (
                nana_ui_core::space::MD,
                nana_ui_core::space::XS,
                nana_ui_core::space::MD,
                nana_ui_core::space::SM,
            )
        };
        layout.padding_left = Some(LengthSpec::Px(horizontal + indicator + gap));
        layout.padding_right = Some(LengthSpec::Px(horizontal));
        layout.padding_top = Some(LengthSpec::Px(vertical));
        layout.padding_bottom = Some(LengthSpec::Px(vertical));
        layout.border_width = Some(0.0);
        layout.border_radius = Some(999.0);
        layout.font_size = Some(if self.compact {
            nana_ui_core::type_scale::HINT
        } else {
            nana_ui_core::type_scale::META
        });
        layout.font_weight = Some(nana_ui_core::type_scale::MEDIUM);
        style
    }
}

impl ComponentView for StatusBadge {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "status-badge".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_visual(
            id,
            world,
            mutations,
            self.label.as_ref(),
            self.effective_style(),
            StandardVisual::StatusBadge {
                label: Arc::clone(&self.label),
                tone: self.tone,
                compact: self.compact,
            },
            AccessibilityState {
                role: AccessibilityRole::Text,
                label: Some(Arc::clone(&self.label)),
                ..AccessibilityState::default()
            },
        );
    }
}

/// Inline validation feedback whose semantic intent is available to every backend.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationMessage {
    pub message: Arc<str>,
    pub intent: nana_ui_core::ValidationIntent,
    pub compact: bool,
    pub style: NodeStyle,
}

impl ValidationMessage {
    pub fn new(message: impl Into<Arc<str>>, intent: nana_ui_core::ValidationIntent) -> Self {
        Self {
            message: message.into(),
            intent,
            compact: true,
            style: NodeStyle::default(),
        }
    }

    pub fn intent(mut self, intent: nana_ui_core::ValidationIntent) -> Self {
        self.intent = intent;
        self
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        style.foreground = Some(match self.intent {
            nana_ui_core::ValidationIntent::Warning => SemanticColorRole::Warning,
            nana_ui_core::ValidationIntent::Danger => SemanticColorRole::Danger,
        });
        let layout = Arc::make_mut(&mut style.layout);
        let indicator = if self.compact {
            nana_ui_core::space::XL
        } else {
            nana_ui_core::space::XXL
        };
        let gap = if self.compact {
            nana_ui_core::space::XS
        } else {
            nana_ui_core::space::SM
        };
        layout.padding_left = Some(LengthSpec::Px(indicator + gap));
        layout.font_size = Some(if self.compact {
            nana_ui_core::type_scale::HINT
        } else {
            nana_ui_core::type_scale::META
        });
        layout.font_weight = None;
        style
    }
}

impl ComponentView for ValidationMessage {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "validation-message".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_visual(
            id,
            world,
            mutations,
            self.message.as_ref(),
            self.effective_style(),
            StandardVisual::ValidationMessage {
                message: Arc::clone(&self.message),
                intent: self.intent,
                compact: self.compact,
            },
            AccessibilityState {
                role: AccessibilityRole::Text,
                label: Some(Arc::clone(&self.message)),
                invalid: true,
                ..AccessibilityState::default()
            },
        );
    }
}

/// A flat feedback surface with intrinsic semantic content and an optional
/// application-owned action child.
#[derive(Debug, Clone, PartialEq)]
pub struct EmptyState {
    pub title: Arc<str>,
    pub message: Option<Arc<str>>,
    pub icon: Option<Icon>,
    pub compact: bool,
    pub action: Option<StableNodeId>,
    pub style: NodeStyle,
}

impl EmptyState {
    pub fn new(title: impl Into<Arc<str>>) -> Self {
        Self {
            title: title.into(),
            message: None,
            icon: None,
            compact: false,
            action: None,
            style: NodeStyle::default(),
        }
    }

    pub fn message(mut self, message: impl Into<Arc<str>>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Declare an action child identity. Mount it through
    /// [`crate::AppContext::set_empty_state_action`].
    pub fn action_child(mut self, action: StableNodeId) -> Self {
        self.action = Some(action);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        style.background = None;
        style.border = None;
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Percent(100.0));
        layout.direction = Some(FlexDirection::Column);
        layout.align_items = if self.compact {
            AlignSpec::Start
        } else {
            AlignSpec::Center
        };
        layout.gap = Some(LengthSpec::Px(0.0));
        // Surface padding follows the spacing scale; a caller that declares its
        // own keeps it.
        let (horizontal, vertical) = if self.compact {
            (nana_ui_core::space::SM, nana_ui_core::space::MD)
        } else {
            (nana_ui_core::space::XXXL, nana_ui_core::space::PAGE)
        };
        if layout.padding_left.is_none() {
            layout.padding_left = Some(LengthSpec::Px(horizontal));
        }
        if layout.padding_right.is_none() {
            layout.padding_right = Some(LengthSpec::Px(horizontal));
        }
        // The real intrinsic leading block is written after TextShaper has
        // measured title/message for the resolved content width.
        if layout.padding_top.is_none() {
            layout.padding_top = Some(LengthSpec::Px(vertical));
        }
        if layout.padding_bottom.is_none() {
            layout.padding_bottom = Some(LengthSpec::Px(vertical));
        }
        layout.border_width = Some(0.0);
        style
    }
}

impl ComponentView for EmptyState {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "empty-state".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_visual(
            id,
            world,
            mutations,
            "",
            self.effective_style(),
            StandardVisual::EmptyState {
                title: Arc::clone(&self.title),
                message: self.message.clone(),
                icon: self.icon,
                compact: self.compact,
                action: self.action,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::clone(&self.title)),
                value: self.message.clone(),
                ..AccessibilityState::default()
            },
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueEmphasis {
    Muted,
    Normal,
    #[default]
    Strong,
}

/// A compact label/value summary: label start-aligned, value end-aligned on one row.
/// An optional application-owned action child sits after the value.
/// The parent remains non-interactive; only the mounted child owns activation.
#[derive(Debug, Clone, PartialEq)]
pub struct LabeledValue {
    pub label: Arc<str>,
    pub value: Arc<str>,
    pub emphasis: ValueEmphasis,
    pub compact: bool,
    /// An existing child view. No icon or action behavior is synthesized.
    pub action: Option<StableNodeId>,
    pub style: NodeStyle,
}

impl LabeledValue {
    pub fn new(label: impl Into<Arc<str>>, value: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            emphasis: ValueEmphasis::Strong,
            compact: true,
            action: None,
            style: NodeStyle::default(),
        }
    }

    pub fn emphasis(mut self, emphasis: ValueEmphasis) -> Self {
        self.emphasis = emphasis;
        self
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Declare an action child identity. Use
    /// [`crate::AppContext::set_labeled_value_action`] to validate and mount it.
    pub fn action_child(mut self, action: StableNodeId) -> Self {
        self.action = Some(action);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        style.foreground = Some(SemanticColorRole::Text);
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Percent(100.0));
        layout.direction = Some(FlexDirection::Row);
        layout.justify_content = JustifySpec::SpaceBetween;
        layout.align_items = AlignSpec::Center;
        layout.min_height = Some(LengthSpec::Px(if self.compact { 28.0 } else { 32.0 }));
        layout.gap = Some(LengthSpec::Px(if self.compact { 4.0 } else { 8.0 }));
        style
    }
}

impl ComponentView for LabeledValue {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "labeled-value".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let value_role = match self.emphasis {
            ValueEmphasis::Muted => SemanticColorRole::Muted,
            ValueEmphasis::Normal | ValueEmphasis::Strong => SemanticColorRole::Text,
        };
        let value_weight = match self.emphasis {
            ValueEmphasis::Muted => 400,
            ValueEmphasis::Normal => 500,
            ValueEmphasis::Strong => 600,
        };
        project_visual(
            id,
            world,
            mutations,
            "",
            self.effective_style(),
            StandardVisual::LabeledValue {
                label: Arc::clone(&self.label),
                value: Arc::clone(&self.value),
                value_role,
                value_weight,
                compact: self.compact,
                action: self.action,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::clone(&self.label)),
                value: Some(Arc::clone(&self.value)),
                ..AccessibilityState::default()
            },
        );
    }
}

const PROGRESS_GIRTH: f32 = nana_ui_core::space::SM;
const PROGRESS_LABEL_SIZE: f32 = nana_ui_core::type_scale::META;
const PROGRESS_GAP: f32 = nana_ui_core::space::SM;
const PROGRESS_CANCEL_SIZE: f32 = nana_ui_core::space::PAGE;
const SPINNER_DEFAULT_SIZE: f32 = nana_ui_core::space::XXL;
const SPINNER_LABEL_SIZE: f32 = nana_ui_core::type_scale::META;
const SPINNER_GAP: f32 = nana_ui_core::space::SM;

fn sanitize_progress_max(max: f64) -> f64 {
    if max.is_finite() && max > 0.0 {
        max
    } else {
        f64::MIN_POSITIVE
    }
}

fn sanitize_spinner_size(size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        size
    } else {
        SPINNER_DEFAULT_SIZE
    }
}

/// Cancel request from a cancellable progress control. Progress does not own a timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressCancelled;

/// Determinate progress track (Subtle rail, Accent fill, 6px girth).
///
/// Optional cancel is a real hit target.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub value: f64,
    pub max: f64,
    pub label: Option<Arc<str>>,
    pub cancellable: bool,
    pub style: NodeStyle,
}

impl Progress {
    pub fn new(value: f64, max: f64) -> Self {
        Self {
            value,
            max: sanitize_progress_max(max),
            label: None,
            cancellable: false,
            style: NodeStyle::default(),
        }
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn cancellable(mut self, cancellable: bool) -> Self {
        self.cancellable = cancellable;
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn resolved_max(&self) -> f64 {
        sanitize_progress_max(self.max)
    }

    fn displayed_value(&self) -> f64 {
        let max = self.resolved_max();
        if self.value.is_finite() {
            self.value.clamp(0.0, max)
        } else {
            0.0
        }
    }

    fn value_ratio(&self) -> f32 {
        let ratio = (self.displayed_value() / self.resolved_max()) as f32;
        if ratio.is_finite() {
            ratio.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    fn heading_height(&self) -> f32 {
        if self.label.is_some() || self.cancellable {
            PROGRESS_LABEL_SIZE.max(if self.cancellable {
                PROGRESS_CANCEL_SIZE
            } else {
                0.0
            })
        } else {
            0.0
        }
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        style.foreground = Some(SemanticColorRole::Text);
        style.background = Some(SemanticColorRole::Subtle);
        style.border = None;
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Fill);
        // Padding the caller declared is kept; only the unset edges default to
        // zero. Boxes are border-box, so the declared padding is added to the
        // content height instead of squeezing the bar.
        for edge in [
            &mut layout.padding_left,
            &mut layout.padding_right,
            &mut layout.padding_top,
            &mut layout.padding_bottom,
        ] {
            if edge.is_none() {
                *edge = Some(LengthSpec::Px(0.0));
            }
        }
        let padding = layout.resolved_padding();
        let heading = self.heading_height();
        let content = if heading > 0.0 {
            heading + PROGRESS_GAP + PROGRESS_GIRTH
        } else {
            PROGRESS_GIRTH
        };
        layout.height = Some(LengthSpec::Px(content + padding.top + padding.bottom));
        layout.direction = Some(FlexDirection::Column);
        layout.gap = Some(LengthSpec::Px(PROGRESS_GAP));
        layout.border_width = Some(0.0);
        if self.label.is_some() {
            layout.font_size = Some(PROGRESS_LABEL_SIZE);
            layout.font_weight = Some(nana_ui_core::type_scale::MEDIUM);
        }
        style
    }
}

impl ComponentView for Progress {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::cancel_progress),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "progress".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let value = self.displayed_value();
        let max = self.resolved_max();
        let ratio = self.value_ratio();
        let percent = (f64::from(ratio) * 100.0).round();
        project_visual(
            id,
            world,
            mutations,
            "",
            self.effective_style(),
            StandardVisual::Progress {
                value_ratio: ratio,
                label: self.label.clone(),
                cancellable: self.cancellable,
            },
            AccessibilityState {
                role: AccessibilityRole::ProgressIndicator,
                label: self.label.clone(),
                value: Some(Arc::from(format!("{percent:.0}%"))),
                numeric_minimum: Some(0.0),
                numeric_maximum: Some(max),
                numeric_value: Some(value),
                ..AccessibilityState::default()
            },
        );
        let interaction = InteractionState {
            pointer_events: self.cancellable,
            focusable: self.cancellable,
        };
        if world.interaction(id) != Some(interaction) {
            mutations.set_interaction(id, interaction);
        }
    }
}

/// Indeterminate loading indicator with an optional muted label.
///
/// Rotation is a Motion IR infinite timeline started on mount. The host does
/// not tick `phase`; samples write the visual. Hide or unmount to stop.
#[derive(Debug, Clone, PartialEq)]
pub struct Spinner {
    pub label: Arc<str>,
    pub size: f32,
    /// Runtime-sampled rotation phase in `0.0..=1.0`. The animation dispatch
    /// owns this field, so a mounted spinner turns without the host ticking it.
    pub(crate) phase: f32,
    pub style: NodeStyle,
}

impl Spinner {
    /// Playback longhands of the infinite rotation timeline.
    pub(crate) const SPIN_PLAYBACK: AnimationPlayback = AnimationPlayback {
        iteration_count: AnimationIteration::INFINITE,
        direction: AnimationDirection::Normal,
        fill_mode: AnimationFillMode::None,
        play_state: AnimationPlayState::Running,
        paused_at: None,
    };

    /// The spinner's rotation timeline anchored at `start`, or `None` when its
    /// hashed animation ID would be zero.
    pub(crate) fn spin_animation(id: StableNodeId, start: Duration) -> Option<AnimationSpec> {
        let animation = component_animation_id(component_animation_kinds::SPINNER, id)?;
        Some(AnimationSpec::new(
            animation,
            id,
            start,
            nana_ui_core::motion::SPINNER_ROTATION,
            crate::framework::COMPONENT_FRAME_INTERVAL,
            Easing::Linear,
        ))
    }

    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            size: SPINNER_DEFAULT_SIZE,
            phase: 0.0,
            style: NodeStyle::default(),
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = sanitize_spinner_size(size);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn resolved_size(&self) -> f32 {
        sanitize_spinner_size(self.size)
    }

    fn resolved_phase(&self) -> f32 {
        if self.phase.is_finite() {
            self.phase
        } else {
            0.0
        }
    }

    fn effective_style(&self) -> NodeStyle {
        let size = self.resolved_size();
        let mut style = self.style.clone();
        style.foreground = Some(SemanticColorRole::Muted);
        style.background = None;
        style.border = None;
        style.text_vertical_alignment = TextVerticalAlignment::Center;
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Shrink);
        layout.min_width = Some(LengthSpec::Px(size));
        layout.height = Some(LengthSpec::Px(size.max(SPINNER_DEFAULT_SIZE)));
        layout.direction = Some(FlexDirection::Row);
        layout.align_items = AlignSpec::Center;
        layout.gap = Some(LengthSpec::Px(SPINNER_GAP));
        layout.padding_left = Some(LengthSpec::Px(if self.label.is_empty() {
            0.0
        } else {
            size + SPINNER_GAP
        }));
        // `padding_left` above is the spinner's own glyph lane and stays
        // framework-owned; the other three edges are the caller's.
        for edge in [
            &mut layout.padding_right,
            &mut layout.padding_top,
            &mut layout.padding_bottom,
        ] {
            if edge.is_none() {
                *edge = Some(LengthSpec::Px(0.0));
            }
        }
        layout.border_width = Some(0.0);
        layout.font_size = Some(SPINNER_LABEL_SIZE);
        style
    }
}

impl ComponentView for Spinner {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "spinner".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        // A busy indicator turns on its own: start the timeline once for a
        // freshly created or mounted node and never restart a running one, so
        // repeated projections and per-frame phase writes cannot reset it.
        // A hidden spinner generates no box, so its timeline stops rather than
        // holding the host awake for a turn nobody can see.
        if let Some(spec) = Self::spin_animation(id, Duration::ZERO) {
            let startable = !world.contains(id) || world.is_mounted(id);
            let running = world.animation_is_active(spec.id);
            if self.style.layout.hidden {
                if running {
                    mutations.stop_animation(spec.id);
                }
            } else if startable && !running {
                mutations.start_animation_with_playback(spec, Self::SPIN_PLAYBACK);
            }
        }
        project_visual(
            id,
            world,
            mutations,
            self.label.as_ref(),
            self.effective_style(),
            StandardVisual::Spinner {
                label: Arc::clone(&self.label),
                size: self.resolved_size(),
                phase: self.resolved_phase(),
            },
            AccessibilityState {
                role: AccessibilityRole::ProgressIndicator,
                label: Some(Arc::clone(&self.label)),
                busy: true,
                ..AccessibilityState::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppContext, Button, DocumentId, FrameworkError, ProgressCancelled, StandardVisual,
    };
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use unicode_segmentation::UnicodeSegmentation;

    #[derive(Default)]
    struct WrappingShaper;

    impl crate::TextShaper for WrappingShaper {
        fn shape(
            &mut self,
            _id: StableNodeId,
            text: &crate::TextContent,
            style: &crate::ComputedStyle,
            constraints: crate::TextShapeConstraints,
        ) -> crate::TextMetrics {
            let count = text.value.graphemes(true).count();
            if count == 0 {
                return crate::TextMetrics::default();
            }
            let advance = style.font_size;
            let natural_width = count as f32 * advance;
            let columns = constraints
                .max_width
                .filter(|_| constraints.wrap)
                .map(|width| (width / advance).floor().max(1.0) as usize)
                .unwrap_or(count);
            let lines = count.div_ceil(columns);
            crate::TextMetrics {
                width: constraints
                    .max_width
                    .map_or(natural_width, |width| natural_width.min(width)),
                height: lines as f32 * style.font_size * 1.2,
                ascent: None,
            }
        }
    }

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn layout(context: &mut AppContext, id: StableNodeId, width: f32, height: f32) {
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            id,
            crate::LayoutBox {
                x: 10.0,
                y: 20.0,
                width,
                height,
            },
        );
        context.commit_mutations(mutations).unwrap();
        let work = context.take_system_work();
        context
            .compat_world_mut()
            .resolve_styles(&work.style)
            .unwrap();
    }

    #[test]
    fn status_badge_projects_tone_density_and_accessible_label() {
        let mut context = AppContext::new();
        let badge = context
            .create_component(
                document(),
                StatusBadge::new("Offline", nana_ui_core::StatusTone::Danger).compact(false),
            )
            .unwrap();
        let id = badge.stable_id();

        assert_eq!(context.world().text(id), Some("Offline"));
        assert_eq!(
            context.world().node_style(id).unwrap().foreground,
            Some(SemanticColorRole::Danger)
        );
        assert_eq!(
            context.world().node_style(id).unwrap().layout.font_size,
            Some(nana_ui_core::type_scale::META)
        );
        assert_eq!(
            context.world().accessibility(id).unwrap().label.as_deref(),
            Some("Offline")
        );
        layout(&mut context, id, 96.0, 24.0);
        let crate::ComponentGeometry::StatusBadge {
            indicator,
            label,
            background,
            foreground,
        } = context.world().component_geometry(id).unwrap()
        else {
            panic!("status badge geometry")
        };
        assert!(indicator.width > 0.0);
        assert_eq!(label.font_size, nana_ui_core::type_scale::META);
        assert_eq!(label.font_weight, Some(nana_ui_core::type_scale::MEDIUM));
        assert_eq!(background[..3], foreground[..3]);
        assert!(background[3] < foreground[3]);
        let dark_foreground = foreground;
        let mut theme = MutationQueue::new();
        theme.set_theme(crate::ThemeMode::Light);
        context.commit_mutations(theme).unwrap();
        let work = context.take_system_work();
        context
            .compat_world_mut()
            .resolve_styles(&work.style)
            .unwrap();
        let crate::ComponentGeometry::StatusBadge { foreground, .. } =
            context.world().component_geometry(id).unwrap()
        else {
            panic!("status badge geometry")
        };
        assert_ne!(foreground, dark_foreground);
    }

    #[test]
    fn validation_message_projects_intent_and_invalid_semantics() {
        let mut context = AppContext::new();
        let message = context
            .create_component(
                document(),
                ValidationMessage::new(
                    "A project is required",
                    nana_ui_core::ValidationIntent::Warning,
                ),
            )
            .unwrap();
        let id = message.stable_id();

        assert_eq!(
            context.world().node_style(id).unwrap().foreground,
            Some(SemanticColorRole::Warning)
        );
        let accessibility = context.world().accessibility(id).unwrap();
        assert!(accessibility.invalid);
        assert_eq!(
            accessibility.label.as_deref(),
            Some("A project is required")
        );
        layout(&mut context, id, 180.0, 18.0);
        let crate::ComponentGeometry::ValidationMessage {
            indicator,
            label,
            foreground,
        } = context.world().component_geometry(id).unwrap()
        else {
            panic!("validation geometry")
        };
        assert!(indicator.width > 0.0);
        assert_eq!(label.font_size, nana_ui_core::type_scale::HINT);
        assert_eq!(label.color, Some(foreground));
        assert_eq!(label.font_weight, None);
        assert_eq!(
            context.world().node_style(id).unwrap().layout.font_weight,
            None
        );
    }

    #[test]
    fn empty_state_wrapped_intrinsics_drive_action_layout_and_settle() {
        let mut context = AppContext::new();
        let action = context
            .create_detached_component(document(), Button::new("继续"))
            .unwrap();
        let empty = context
            .create_component(
                document(),
                EmptyState::new("没有可显示的项目👩‍💻请稍后再试")
                    .message("较长的说明文字会根据最终内容宽度自动换行🙂")
                    .icon(Icon::Folder),
            )
            .unwrap();
        context
            .set_empty_state_action(empty, Some(action.stable_id()))
            .unwrap();
        let work = context.take_system_work();
        context
            .compat_world_mut()
            .resolve_styles(&work.style)
            .unwrap();
        let mut shaper = WrappingShaper;
        context.shape_text(&work.text, &mut shaper).unwrap();
        context
            .layout_document(document(), crate::LayoutViewport::new(120.0, 400.0))
            .unwrap();
        let first_action_y = context.world().layout_box(action.stable_id()).unwrap().y;

        assert!(
            context
                .shape_text_for_layout(document(), &mut shaper)
                .unwrap()
        );
        context
            .layout_document(document(), crate::LayoutViewport::new(120.0, 400.0))
            .unwrap();
        assert!(
            !context
                .shape_text_for_layout(document(), &mut shaper)
                .unwrap()
        );

        let crate::ComponentGeometry::EmptyState {
            title,
            message: Some(message),
            action: Some(action_bounds),
            ..
        } = context
            .world()
            .component_geometry(empty.stable_id())
            .unwrap()
        else {
            panic!("wrapped empty state geometry")
        };
        assert!(title.bounds.height > 13.0 * 1.2);
        assert!(message.bounds.height > 12.0 * 1.2);
        assert!(action_bounds.y > first_action_y);
        assert!((action_bounds.y - (message.bounds.y + message.bounds.height) - 10.0).abs() < 0.01);

        let metrics = (title.bounds.height, message.bounds.height, action_bounds.y);
        context.set_theme(crate::ThemeMode::Light).unwrap();
        let work = context.take_system_work();
        context
            .compat_world_mut()
            .resolve_styles(&work.style)
            .unwrap();
        assert!(
            !context
                .shape_text_for_layout(document(), &mut shaper)
                .unwrap()
        );
        let crate::ComponentGeometry::EmptyState {
            title,
            message: Some(message),
            action: Some(action),
            ..
        } = context
            .world()
            .component_geometry(empty.stable_id())
            .unwrap()
        else {
            panic!("light empty state geometry")
        };
        assert_eq!(
            metrics,
            (title.bounds.height, message.bounds.height, action.y)
        );
    }

    #[test]
    fn empty_state_title_only_has_no_synthetic_spacing() {
        let mut context = AppContext::new();
        let empty = context
            .create_component(document(), EmptyState::new("空").compact(true))
            .unwrap();
        let work = context.take_system_work();
        context
            .compat_world_mut()
            .resolve_styles(&work.style)
            .unwrap();
        let mut shaper = WrappingShaper;
        context.shape_text(&work.text, &mut shaper).unwrap();
        context
            .layout_document(document(), crate::LayoutViewport::new(100.0, 100.0))
            .unwrap();
        context
            .shape_text_for_layout(document(), &mut shaper)
            .unwrap();
        context
            .layout_document(document(), crate::LayoutViewport::new(100.0, 100.0))
            .unwrap();
        let crate::ComponentGeometry::EmptyState {
            icon: None,
            message: None,
            action: None,
            title,
            ..
        } = context
            .world()
            .component_geometry(empty.stable_id())
            .unwrap()
        else {
            panic!("title-only empty state geometry")
        };
        assert_eq!(title.bounds.height, 12.0 * 1.2);
        assert_eq!(
            context
                .world()
                .node_style(empty.stable_id())
                .unwrap()
                .layout
                .padding_top,
            Some(LengthSpec::Px(8.0 + title.bounds.height))
        );
    }

    #[test]
    fn normal_empty_state_centers_shaped_regions_while_preserving_start_aligned_lines() {
        let mut context = AppContext::new();
        let empty = context
            .create_component(
                document(),
                EmptyState::new("ABCDE").message("这是用于验证窄宽换行起点的较长消息文本"),
            )
            .unwrap();
        let work = context.take_system_work();
        context
            .compat_world_mut()
            .resolve_styles(&work.style)
            .unwrap();
        let mut shaper = WrappingShaper;
        context.shape_text(&work.text, &mut shaper).unwrap();
        context
            .layout_document(document(), crate::LayoutViewport::new(120.0, 200.0))
            .unwrap();
        assert!(
            context
                .shape_text_for_layout(document(), &mut shaper)
                .unwrap()
        );
        context
            .layout_document(document(), crate::LayoutViewport::new(120.0, 200.0))
            .unwrap();

        let crate::ComponentGeometry::EmptyState {
            content_clip: clip,
            title,
            message: Some(message),
            ..
        } = context
            .world()
            .component_geometry(empty.stable_id())
            .unwrap()
        else {
            panic!("normal empty state geometry")
        };
        assert!(message.bounds.height > 12.0 * 1.2);
        assert!(title.bounds.width < message.bounds.width);
        assert!(title.bounds.x > message.bounds.x);
        assert!((title.bounds.x - (clip.x + (clip.width - title.bounds.width) / 2.0)).abs() < 0.01);
        assert!(
            (message.bounds.x - (clip.x + (clip.width - message.bounds.width) / 2.0)).abs() < 0.01
        );
    }

    #[test]
    fn extremely_narrow_empty_state_keeps_intrinsics_ordered_inside_content_width() {
        let mut context = AppContext::new();
        let action = context
            .create_detached_component(document(), Button::new("Action"))
            .unwrap();
        let empty = context
            .create_component(
                document(),
                EmptyState::new("标题")
                    .icon(Icon::Folder)
                    .message("包含 emoji 🎉 的窄宽消息"),
            )
            .unwrap();
        context
            .set_empty_state_action(empty, Some(action.stable_id()))
            .unwrap();
        let work = context.take_system_work();
        context
            .compat_world_mut()
            .resolve_styles(&work.style)
            .unwrap();
        let mut shaper = WrappingShaper;
        context.shape_text(&work.text, &mut shaper).unwrap();
        context
            .layout_document(document(), crate::LayoutViewport::new(40.0, 400.0))
            .unwrap();
        assert!(
            context
                .shape_text_for_layout(document(), &mut shaper)
                .unwrap()
        );
        context
            .layout_document(document(), crate::LayoutViewport::new(40.0, 400.0))
            .unwrap();

        let crate::ComponentGeometry::EmptyState {
            content_clip: clip,
            icon: Some((_, icon, _)),
            title,
            message: Some(message),
            action: Some(action),
            ..
        } = context
            .world()
            .component_geometry(empty.stable_id())
            .unwrap()
        else {
            panic!("narrow empty state geometry")
        };
        for bounds in [icon, title.bounds, message.bounds] {
            assert!(bounds.x >= clip.x);
            assert!(bounds.x + bounds.width <= clip.x + clip.width + 0.01);
        }
        assert!(icon.y + icon.height <= title.bounds.y + 0.01);
        assert!(title.bounds.y + title.bounds.height <= message.bounds.y + 0.01);
        assert!(message.bounds.y + message.bounds.height <= action.y + 0.01);
    }

    #[test]
    fn empty_state_root_clip_bounds_action_hit_testing_and_accessibility() {
        let mut context = AppContext::new();
        let action = context
            .create_detached_component(document(), Button::new("Action"))
            .unwrap();
        let empty = context
            .create_component(document(), EmptyState::new("Empty"))
            .unwrap();
        context
            .set_empty_state_action(empty, Some(action.stable_id()))
            .unwrap();
        let mut layouts = MutationQueue::new();
        layouts.write_layout(
            empty.stable_id(),
            crate::LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
            },
        );
        layouts.write_layout(
            action.stable_id(),
            crate::LayoutBox {
                x: 80.0,
                y: 40.0,
                width: 40.0,
                height: 20.0,
            },
        );
        context.commit_mutations(layouts).unwrap();
        context.compat_world_mut().rebuild_hit_test(document());

        assert_eq!(
            context.world().hit_test(document(), 95.0, 45.0),
            Some(action.stable_id())
        );
        assert_ne!(
            context.world().hit_test(document(), 110.0, 45.0),
            Some(action.stable_id())
        );
        let action_accessibility = context
            .world()
            .project_accessibility(document())
            .into_iter()
            .find(|node| node.id == action.stable_id())
            .unwrap();
        assert_eq!(
            action_accessibility.bounds,
            crate::LayoutBox {
                x: 80.0,
                y: 40.0,
                width: 20.0,
                height: 10.0,
            }
        );

        let mut outside = MutationQueue::new();
        outside.write_layout(
            action.stable_id(),
            crate::LayoutBox {
                x: 110.0,
                y: 40.0,
                width: 20.0,
                height: 10.0,
            },
        );
        context.commit_mutations(outside).unwrap();
        context.compat_world_mut().rebuild_hit_test(document());
        assert_ne!(
            context.world().hit_test(document(), 115.0, 45.0),
            Some(action.stable_id())
        );
        let accessibility = context.world().project_accessibility(document());
        assert!(
            accessibility
                .iter()
                .all(|node| node.id != action.stable_id())
        );
        assert!(
            accessibility
                .iter()
                .find(|node| node.id == empty.stable_id())
                .unwrap()
                .children
                .is_empty()
        );
    }

    #[test]
    fn compact_empty_state_action_gap_combines_column_and_action_padding() {
        let mut context = AppContext::new();
        let action = context
            .create_detached_component(document(), Button::new("Action"))
            .unwrap();
        let empty = context
            .create_component(
                document(),
                EmptyState::new("Title").message("Message").compact(true),
            )
            .unwrap();
        context
            .set_empty_state_action(empty, Some(action.stable_id()))
            .unwrap();
        let work = context.take_system_work();
        context
            .compat_world_mut()
            .resolve_styles(&work.style)
            .unwrap();
        let mut shaper = WrappingShaper;
        context.shape_text(&work.text, &mut shaper).unwrap();
        context
            .layout_document(document(), crate::LayoutViewport::new(160.0, 120.0))
            .unwrap();
        context
            .shape_text_for_layout(document(), &mut shaper)
            .unwrap();
        context
            .layout_document(document(), crate::LayoutViewport::new(160.0, 120.0))
            .unwrap();
        let crate::ComponentGeometry::EmptyState {
            message: Some(message),
            action: Some(action),
            ..
        } = context
            .world()
            .component_geometry(empty.stable_id())
            .unwrap()
        else {
            panic!("compact action geometry")
        };
        assert!((action.y - (message.bounds.y + message.bounds.height) - 6.0).abs() < 0.01);
    }

    #[test]
    fn empty_state_owns_semantic_content_and_action_mount_is_atomic() {
        let mut context = AppContext::new();
        let action = context
            .create_detached_component(document(), Button::new("Create project").loading(true))
            .unwrap();
        let replacement = context
            .create_detached_component(document(), Button::new("Import project"))
            .unwrap();
        let empty = context
            .create_component(
                document(),
                EmptyState::new("No projects")
                    .icon(Icon::Folder)
                    .message("Create the first project"),
            )
            .unwrap();
        assert_eq!(
            context.world().mount_state(action.stable_id()),
            Some(crate::MountState::Parked)
        );
        assert!(
            !context
                .world()
                .document_order(document())
                .contains(&action.stable_id())
        );
        assert_eq!(context.next_animation_deadline(), None);
        assert_eq!(
            context.world().interaction(empty.stable_id()),
            Some(InteractionState {
                pointer_events: false,
                focusable: false,
            })
        );
        let Some(StandardVisual::EmptyState {
            icon,
            message,
            action: None,
            ..
        }) = context.world().standard_visual(empty.stable_id())
        else {
            panic!("empty state visual");
        };
        assert_eq!(icon, Some(Icon::Folder));
        assert_eq!(message.as_deref(), Some("Create the first project"));
        assert!(
            context
                .set_empty_state_action(empty, Some(action.stable_id()))
                .unwrap()
        );
        assert!(
            !context
                .set_empty_state_action(empty, Some(action.stable_id()))
                .unwrap()
        );
        assert_eq!(
            context.world().node(empty.stable_id()).unwrap().children,
            vec![action.stable_id()]
        );
        assert_eq!(context.next_animation_deadline(), Some(Duration::ZERO));

        let missing = StableNodeId::new(999).unwrap();
        assert!(matches!(
            context.set_empty_state_action(empty, Some(missing)),
            Err(FrameworkError::InvalidFeedbackSlots {
                parent,
                slot: Some(slot)
            }) if parent == empty.stable_id() && slot == missing
        ));
        assert_eq!(
            context.world().node(empty.stable_id()).unwrap().children,
            vec![action.stable_id()]
        );
        assert!(
            context
                .set_empty_state_action(empty, Some(replacement.stable_id()))
                .unwrap()
        );
        assert_eq!(
            context.world().node(empty.stable_id()).unwrap().children,
            vec![replacement.stable_id()]
        );
        assert!(
            context
                .world()
                .node(action.stable_id())
                .unwrap()
                .parent
                .is_none()
        );
        assert_eq!(
            context.world().mount_state(action.stable_id()),
            Some(crate::MountState::Parked)
        );
        assert_eq!(context.next_animation_deadline(), None);
        assert!(context.set_empty_state_action(empty, None).unwrap());
        assert!(
            context
                .world()
                .node(empty.stable_id())
                .unwrap()
                .children
                .is_empty()
        );
        assert!(
            context
                .world()
                .node(replacement.stable_id())
                .unwrap()
                .parent
                .is_none()
        );
        assert_eq!(
            context.world().mount_state(replacement.stable_id()),
            Some(crate::MountState::Parked)
        );
        layout(&mut context, empty.stable_id(), 240.0, 120.0);
        let crate::ComponentGeometry::EmptyState {
            icon,
            title,
            message,
            action,
            ..
        } = context
            .world()
            .component_geometry(empty.stable_id())
            .unwrap()
        else {
            panic!("empty state geometry")
        };
        let Some((kind, _, _)) = icon else {
            panic!("empty state icon");
        };
        assert_eq!(kind, Icon::Folder);
        assert_eq!(title.font_size, 13.0);
        assert_eq!(message.unwrap().font_size, 12.0);
        assert!(action.is_none());
    }

    #[test]
    fn labeled_value_action_is_validated_and_remains_application_owned() {
        let mut context = AppContext::new();
        let foreign_action = context
            .create_detached_component(DocumentId::new(2).unwrap(), Button::new("Foreign"))
            .unwrap();
        let action = context
            .create_detached_component(document(), Button::new("Copy"))
            .unwrap();
        let replacement = context
            .create_detached_component(document(), Button::new("Reveal"))
            .unwrap();
        let owner = context
            .create_component(document(), Button::new("Owner"))
            .unwrap();
        let owned_action = context
            .create_detached_component(document(), Button::new("Owned"))
            .unwrap();
        context.append_child(owner, owned_action).unwrap();
        let summary = context
            .create_component(
                document(),
                LabeledValue::new("Revision", "42").emphasis(ValueEmphasis::Normal),
            )
            .unwrap();
        let id = summary.stable_id();

        assert_eq!(
            context.world().interaction(id),
            Some(InteractionState {
                pointer_events: false,
                focusable: false,
            })
        );
        let missing = StableNodeId::new(999).unwrap();
        assert!(matches!(
            context.set_labeled_value_action(summary, Some(missing)),
            Err(FrameworkError::InvalidFeedbackSlots {
                parent,
                slot: Some(slot)
            }) if parent == id && slot == missing
        ));
        assert!(matches!(
            context.set_labeled_value_action(summary, Some(foreign_action.stable_id())),
            Err(FrameworkError::InvalidFeedbackSlots {
                parent,
                slot: Some(slot)
            }) if parent == id && slot == foreign_action.stable_id()
        ));
        assert!(matches!(
            context.set_labeled_value_action(summary, Some(owned_action.stable_id())),
            Err(FrameworkError::InvalidFeedbackSlots {
                parent,
                slot: Some(slot)
            }) if parent == id && slot == owned_action.stable_id()
        ));
        assert_eq!(context.read(summary, |view| view.action).unwrap(), None);
        assert!(context.world().node(id).unwrap().children.is_empty());
        assert_eq!(
            context
                .world()
                .node(owned_action.stable_id())
                .unwrap()
                .parent,
            Some(owner.stable_id())
        );

        let activations = Arc::new(Mutex::new(0));
        let observed = Arc::clone(&activations);
        context
            .on(action, move |_button, _event: &crate::Activate, _cx| {
                *observed.lock().unwrap() += 1;
            })
            .unwrap();
        assert!(
            context
                .set_labeled_value_action(summary, Some(action.stable_id()))
                .unwrap()
        );

        assert_eq!(
            context.world().standard_visual(id),
            Some(StandardVisual::LabeledValue {
                label: Arc::from("Revision"),
                value: Arc::from("42"),
                value_role: SemanticColorRole::Text,
                value_weight: 500,
                compact: true,
                action: Some(action.stable_id()),
            })
        );
        assert_eq!(context.world().text(id), Some(""));
        assert_eq!(
            context.world().interaction(id),
            Some(InteractionState {
                pointer_events: false,
                focusable: false,
            })
        );
        assert_eq!(
            context.world().node(id).unwrap().children,
            vec![action.stable_id()]
        );
        assert!(context.activate_button(action).unwrap());
        assert_eq!(*activations.lock().unwrap(), 1);
        assert!(
            context
                .set_labeled_value_action(summary, Some(replacement.stable_id()))
                .unwrap()
        );
        assert_eq!(
            context.world().node(id).unwrap().children,
            vec![replacement.stable_id()]
        );
        assert!(
            context
                .world()
                .node(action.stable_id())
                .unwrap()
                .parent
                .is_none()
        );
        assert_eq!(
            context.world().mount_state(action.stable_id()),
            Some(crate::MountState::Parked)
        );
        assert!(
            context
                .set_labeled_value_action(summary, Some(action.stable_id()))
                .unwrap()
        );
        assert!(context.activate_button(action).unwrap());
        assert_eq!(*activations.lock().unwrap(), 2);
        assert!(context.set_labeled_value_action(summary, None).unwrap());
        assert!(context.world().node(id).unwrap().children.is_empty());
        let removed = context.remove_view(action).unwrap();
        assert_eq!(removed.label, "Copy");
        assert!(!context.world().contains(action.stable_id()));
        layout(&mut context, id, 180.0, 32.0);
        let crate::ComponentGeometry::LabeledValue { label, value, .. } =
            context.world().component_geometry(id).unwrap()
        else {
            panic!("labeled value geometry")
        };
        assert_eq!(label.font_size, nana_ui_core::type_scale::HINT);
        assert_eq!(value.font_size, nana_ui_core::type_scale::META);
        assert_eq!(value.font_weight, Some(nana_ui_core::type_scale::MEDIUM));
        assert_ne!(label.color, value.color);
        assert!(
            label.bounds.x + label.bounds.width <= value.bounds.x + 0.01,
            "label must sit left of value, got label={:?} value={:?}",
            label.bounds,
            value.bounds
        );
        assert!(
            label.bounds.y < value.bounds.y + value.bounds.height
                && value.bounds.y < label.bounds.y + label.bounds.height,
            "label and value must share one row, got label={:?} value={:?}",
            label.bounds,
            value.bounds
        );
        assert!(
            (value.bounds.x + value.bounds.width - 190.0).abs() < 1.0,
            "value must hug the trailing edge, got {:?}",
            value.bounds
        );
        let accessibility = context.world().accessibility(id).unwrap();
        assert_eq!(accessibility.label.as_deref(), Some("Revision"));
        assert_eq!(accessibility.value.as_deref(), Some("42"));
    }

    /// 与 world.rs `estimated_text_width` 相同的字符分类估算(ASCII≈0.62×字号,其余≈1.0×字号),
    /// world 模块私有,测试侧镜像一份用于推导自然宽度契约。
    fn estimated_width(text: &str, font_size: f32) -> f32 {
        text.chars()
            .map(|ch| {
                if ch.is_ascii() {
                    font_size * 0.62
                } else {
                    font_size
                }
            })
            .sum::<f32>()
            .max(font_size)
    }

    #[test]
    fn labeled_value_geometry_keeps_label_natural_width_under_long_value() {
        let mut context = AppContext::new();
        let summary = context
            .create_component(
                document(),
                LabeledValue::new(
                    "路径",
                    "/Users/someone/Library/Application Support/models/xxx/model.model3.json",
                ),
            )
            .unwrap();
        let id = summary.stable_id();
        layout(&mut context, id, 180.0, 32.0);
        let crate::ComponentGeometry::LabeledValue { label, value, .. } =
            context.world().component_geometry(id).unwrap()
        else {
            panic!("labeled value geometry")
        };
        let label_natural = estimated_width("路径", label.font_size);
        assert!(
            label.bounds.width + 0.01 >= label_natural,
            "label must keep its natural width, got {:?} (natural {label_natural})",
            label.bounds
        );
        assert!(
            (value.bounds.width - (180.0 - label_natural - 4.0)).abs() < 1.0,
            "overflowing value must take the remaining width so only the excess becomes an \
             ellipsis, got {:?}",
            value.bounds
        );
        assert!(
            (value.bounds.x + value.bounds.width - 190.0).abs() < 1.0,
            "value must hug the trailing edge, got {:?}",
            value.bounds
        );
        assert!(
            label.bounds.x + label.bounds.width <= value.bounds.x + 0.01,
            "label must not overlap value, got label={:?} value={:?}",
            label.bounds,
            value.bounds
        );
    }

    #[test]
    fn labeled_value_geometry_keeps_regions_disjoint_when_both_sides_overflow() {
        let mut context = AppContext::new();
        let summary = context
            .create_component(
                document(),
                LabeledValue::new(
                    "网络请求失败后的重试策略与指数退避间隔说明",
                    "https://example.com/very/long/path/that/overflows/the/row/index.html",
                ),
            )
            .unwrap();
        let id = summary.stable_id();
        layout(&mut context, id, 180.0, 32.0);
        let crate::ComponentGeometry::LabeledValue { label, value, .. } =
            context.world().component_geometry(id).unwrap()
        else {
            panic!("labeled value geometry")
        };
        assert!(
            label.bounds.width >= 0.0 && value.bounds.width >= 0.0,
            "region widths must be non-negative, got label={:?} value={:?}",
            label.bounds,
            value.bounds
        );
        assert!(
            label.bounds.x + label.bounds.width <= value.bounds.x + 0.01,
            "regions must not overlap, got label={:?} value={:?}",
            label.bounds,
            value.bounds
        );
        assert!(
            value.bounds.width + 0.01 >= estimated_width("xx", value.font_size),
            "value must keep a minimal visible width, got {:?}",
            value.bounds
        );
        assert!(
            (value.bounds.x + value.bounds.width - 190.0).abs() < 1.0,
            "value must hug the trailing edge, got {:?}",
            value.bounds
        );
    }

    #[test]
    fn feedback_helpers_never_park_unowned_field_only_actions() {
        let mut context = AppContext::new();
        let foreign = context
            .create_detached_component(DocumentId::new(2).unwrap(), Button::new("Foreign"))
            .unwrap();
        let empty = context
            .create_component(document(), EmptyState::new("Empty"))
            .unwrap();
        context
            .update_component(empty, |empty, _cx| {
                empty.action = Some(foreign.stable_id());
            })
            .unwrap();
        assert!(matches!(
            context.set_empty_state_action(empty, None),
            Err(FrameworkError::InvalidFeedbackSlots {
                parent,
                slot: Some(slot),
            }) if parent == empty.stable_id() && slot == foreign.stable_id()
        ));
        assert_eq!(
            context.read(empty, |empty| empty.action).unwrap(),
            Some(foreign.stable_id())
        );
        assert_eq!(
            context.world().mount_state(foreign.stable_id()),
            Some(crate::MountState::Parked)
        );

        let missing = StableNodeId::new(999).unwrap();
        context
            .update_component(empty, |empty, _cx| empty.action = Some(missing))
            .unwrap();
        assert!(matches!(
            context.set_empty_state_action(empty, None),
            Err(FrameworkError::InvalidFeedbackSlots {
                slot: Some(slot),
                ..
            }) if slot == missing
        ));

        let stale = context
            .create_detached_component(document(), Button::new("Stale"))
            .unwrap();
        context
            .update_component(empty, |empty, _cx| {
                empty.action = Some(stale.stable_id());
            })
            .unwrap();
        assert!(context.set_empty_state_action(empty, None).is_err());
        assert_eq!(
            context.world().mount_state(stale.stable_id()),
            Some(crate::MountState::Parked)
        );

        let declared = context
            .create_detached_component(document(), Button::new("Declared"))
            .unwrap();
        let declared_empty = context
            .create_component(
                document(),
                EmptyState::new("Declared empty").action_child(declared.stable_id()),
            )
            .unwrap();
        assert!(
            context
                .set_empty_state_action(declared_empty, Some(declared.stable_id()))
                .unwrap()
        );
        assert_eq!(
            context.world().node(declared.stable_id()).unwrap().parent,
            Some(declared_empty.stable_id())
        );

        let owner = context
            .create_component(document(), Button::new("Owner"))
            .unwrap();
        let owned = context
            .create_detached_component(document(), Button::new("Owned"))
            .unwrap();
        context.append_child(owner, owned).unwrap();
        let summary = context
            .create_component(document(), LabeledValue::new("Key", "Value"))
            .unwrap();
        context
            .update_component(summary, |summary, _cx| {
                summary.action = Some(owned.stable_id());
            })
            .unwrap();
        assert!(matches!(
            context.set_labeled_value_action(summary, None),
            Err(FrameworkError::InvalidFeedbackSlots {
                parent,
                slot: Some(slot),
            }) if parent == summary.stable_id() && slot == owned.stable_id()
        ));
        assert_eq!(
            context.world().node(owned.stable_id()).unwrap().parent,
            Some(owner.stable_id())
        );
        assert_eq!(
            context.world().mount_state(owned.stable_id()),
            Some(crate::MountState::Mounted)
        );
        assert!(
            context
                .world()
                .node(summary.stable_id())
                .unwrap()
                .children
                .is_empty()
        );
    }

    fn progress_ratio(context: &AppContext, id: StableNodeId) -> f32 {
        match context.world().standard_visual(id) {
            Some(StandardVisual::Progress { value_ratio, .. }) => value_ratio,
            other => panic!("progress visual: {other:?}"),
        }
    }

    fn progress_fill_width(context: &AppContext, id: StableNodeId) -> (f32, f32) {
        let crate::ComponentGeometry::Progress { track, fill, .. } =
            context.world().component_geometry(id).unwrap()
        else {
            panic!("progress geometry")
        };
        (track.width, fill.width)
    }

    #[test]
    fn progress_clamps_value_and_fill_width() {
        let mut context = AppContext::new();
        let progress = context
            .create_component(document(), Progress::new(-5.0, 100.0))
            .unwrap();
        let id = progress.stable_id();
        assert_eq!(progress_ratio(&context, id), 0.0);
        assert!(Progress::new(1.0, 0.0).max > 0.0);
        assert!(Progress::new(1.0, f64::NAN).max.is_finite());
        assert!(Progress::new(1.0, f64::INFINITY).max.is_finite());

        context
            .update_component(progress, |progress, _| {
                progress.value = 50.0;
            })
            .unwrap();
        assert_eq!(progress_ratio(&context, id), 0.5);
        layout(&mut context, id, 100.0, 6.0);
        let (track, fill) = progress_fill_width(&context, id);
        assert_eq!(track, 100.0);
        assert!((fill - 50.0).abs() < 0.01);

        context
            .update_component(progress, |progress, _| {
                progress.value = 0.0;
            })
            .unwrap();
        layout(&mut context, id, 100.0, 6.0);
        let (_, fill) = progress_fill_width(&context, id);
        assert_eq!(fill, 0.0);

        context
            .update_component(progress, |progress, _| {
                progress.value = 125.0;
            })
            .unwrap();
        assert_eq!(progress_ratio(&context, id), 1.0);
        layout(&mut context, id, 100.0, 6.0);
        let (track, fill) = progress_fill_width(&context, id);
        assert_eq!(fill, track);
    }

    #[test]
    fn progress_cancellable_reserves_cancel_hit_target() {
        let mut context = AppContext::new();
        let progress = context
            .create_component(
                document(),
                Progress::new(40.0, 100.0)
                    .label("Copying")
                    .cancellable(true),
            )
            .unwrap();
        let id = progress.stable_id();
        layout(&mut context, id, 160.0, 36.0);
        let crate::ComponentGeometry::Progress {
            cancel: Some(cancel),
            label: Some(label),
            ..
        } = context.world().component_geometry(id).unwrap()
        else {
            panic!("cancellable progress geometry");
        };
        assert!(cancel.width > 0.0);
        assert!(label.bounds.width < 160.0);
        assert!(context.world().interaction(id).unwrap().pointer_events);

        let cancelled = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&cancelled);
        context
            .on(
                progress,
                move |_progress, _event: &ProgressCancelled, _cx| {
                    *flag.lock().unwrap() = true;
                },
            )
            .unwrap();
        assert!(context.cancel_progress(progress).unwrap());
        assert!(*cancelled.lock().unwrap());
    }

    #[test]
    fn progress_idle_project_does_not_dirty() {
        let mut context = AppContext::new();
        let progress = context
            .create_component(document(), Progress::new(40.0, 100.0).label("Copying"))
            .unwrap();
        let _ = context.take_system_work();
        context.reproject_component(progress).unwrap();
        assert!(context.take_system_work().is_empty());
        let accessibility = context.world().accessibility(progress.stable_id()).unwrap();
        assert_eq!(accessibility.role, AccessibilityRole::ProgressIndicator);
        assert_eq!(accessibility.value.as_deref(), Some("40%"));
        assert_eq!(accessibility.label.as_deref(), Some("Copying"));
    }

    #[test]
    fn spinner_projects_visual_and_label() {
        let mut context = AppContext::new();
        let spinner = context
            .create_component(document(), Spinner::new("Loading"))
            .unwrap();
        let id = spinner.stable_id();
        assert_eq!(context.world().text(id), Some("Loading"));
        match context.world().standard_visual(id) {
            Some(StandardVisual::Spinner { label, size, phase }) => {
                assert_eq!(&*label, "Loading");
                assert_eq!(size, 14.0);
                assert_eq!(phase, 0.0);
            }
            other => panic!("spinner visual: {other:?}"),
        }
        let accessibility = context.world().accessibility(id).unwrap();
        assert_eq!(accessibility.role, AccessibilityRole::ProgressIndicator);
        assert!(accessibility.busy);
        assert_eq!(accessibility.label.as_deref(), Some("Loading"));
        assert_eq!(
            context.world().node_style(id).unwrap().foreground,
            Some(SemanticColorRole::Muted)
        );
        assert_eq!(
            context.world().node_style(id).unwrap().layout.height,
            Some(LengthSpec::Px(14.0))
        );
    }

    #[test]
    fn a_mounted_spinner_turns_without_the_host_ticking_it() {
        fn projected_phase(context: &AppContext, id: StableNodeId) -> f32 {
            match context.world().standard_visual(id) {
                Some(StandardVisual::Spinner { phase, .. }) => phase,
                other => panic!("expected a spinner visual, got {other:?}"),
            }
        }

        let mut context = AppContext::new();
        let spinner = context
            .create_component(document(), Spinner::new("Loading"))
            .unwrap();
        let id = spinner.stable_id();
        let _ = context.take_system_work();

        let steady = context.advance_animations(Duration::ZERO);
        assert!(steady.has_updates());
        assert_eq!(projected_phase(&context, id), 0.0);
        assert!(
            steady
                .next_deadline
                .is_some_and(|deadline| deadline > Duration::ZERO),
            "the timeline schedules its own follow-up frames"
        );

        context.advance_animations(Duration::from_millis(450));
        let half = projected_phase(&context, id);
        assert!((half - 0.5).abs() < 1e-4, "half a turn at half the period");
        assert!(!context.take_system_work().is_empty());

        context.advance_animations(Duration::from_millis(675));
        assert!(projected_phase(&context, id) > half, "the turn continues");
        assert!(
            context.next_animation_deadline().is_some(),
            "an indeterminate spinner never settles"
        );

        // A hidden spinner must not hold the host awake for an invisible turn.
        context
            .update_component(spinner, |spinner, _| {
                Arc::make_mut(&mut spinner.style.layout).hidden = true;
            })
            .unwrap();
        assert_eq!(context.next_animation_deadline(), None);
        context
            .update_component(spinner, |spinner, _| {
                Arc::make_mut(&mut spinner.style.layout).hidden = false;
            })
            .unwrap();
        assert!(
            context.next_animation_deadline().is_some(),
            "showing it again resumes the turn"
        );
    }
}

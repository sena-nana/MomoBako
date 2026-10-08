use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, ControlSize, FlexDirection, Icon, JustifySpec, LayoutStyle, LengthSpec,
    LineHeightSpec, SemanticColorRole,
};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, InteractionStyle,
    MutationQueue, NodeKind, NodeStyle, SemanticPaint, StableNodeId, StandardVisual, TextContent,
    TextHorizontalAlignment, TextVerticalAlignment, UiWorld,
};

/// Backend-neutral selection orientation. `Segmented` and `Tabs` are
/// horizontal by design; `Radio` stacks vertically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionOrientation {
    #[default]
    Horizontal,
    Vertical,
}

/// Shared selection contract with three product surfaces.
///
/// `Segmented` keeps the bordered pill. `Tabs` is the same RadioGroup/roving
/// behavior on an independent tab strip (no outer chrome). `Radio` is the same
/// behavior again as a stacked list of ring indicators, so a radio group costs
/// no second selection engine. Professional reorder, close, and drag/lease
/// behavior lives on [`crate::Tabs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionChrome {
    #[default]
    Segmented,
    Tabs,
    Radio,
}

impl SelectionChrome {
    /// Orientation this chrome lays its options out in.
    pub const fn orientation(self) -> SelectionOrientation {
        match self {
            Self::Segmented | Self::Tabs => SelectionOrientation::Horizontal,
            Self::Radio => SelectionOrientation::Vertical,
        }
    }
}

/// Selection-independent intent consumed by roving-focus components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RovingFocusIntent {
    Previous,
    Next,
    First,
    Last,
}

/// Resolves one enabled tab stop without knowing or changing selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RovingFocusPolicy {
    pub wrap: bool,
}

impl Default for RovingFocusPolicy {
    fn default() -> Self {
        Self { wrap: true }
    }
}

impl RovingFocusPolicy {
    pub fn resolve<T: Copy + Eq>(
        self,
        items: &[(T, bool)],
        current: Option<T>,
        intent: RovingFocusIntent,
    ) -> Option<T> {
        let enabled = items
            .iter()
            .filter_map(|(id, enabled)| enabled.then_some(*id))
            .collect::<Vec<_>>();
        if enabled.is_empty() {
            return None;
        }
        match intent {
            RovingFocusIntent::First => enabled.first().copied(),
            RovingFocusIntent::Last => enabled.last().copied(),
            RovingFocusIntent::Previous | RovingFocusIntent::Next => {
                let current =
                    current.and_then(|current| enabled.iter().position(|id| *id == current));
                let next = match (current, intent) {
                    (Some(0), RovingFocusIntent::Previous) if self.wrap => enabled.len() - 1,
                    (Some(0), RovingFocusIntent::Previous) => 0,
                    (Some(index), RovingFocusIntent::Previous) => index - 1,
                    (Some(index), RovingFocusIntent::Next) if index + 1 < enabled.len() => {
                        index + 1
                    }
                    (Some(_), RovingFocusIntent::Next) if self.wrap => 0,
                    (Some(index), RovingFocusIntent::Next) => index,
                    (None, RovingFocusIntent::Previous) => enabled.len() - 1,
                    (None, RovingFocusIntent::Next) => 0,
                    _ => unreachable!(),
                };
                enabled.get(next).copied()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SegmentedControl {
    pub label: Option<Arc<str>>,
    pub orientation: SelectionOrientation,
    pub(crate) chrome: SelectionChrome,
    pub(crate) fill: bool,
    pub(crate) size: ControlSize,
    pub(crate) options: Vec<StableNodeId>,
    pub(crate) selected: Option<StableNodeId>,
    pub(crate) focus_target: Option<StableNodeId>,
    pub(crate) roving_focus: RovingFocusPolicy,
    pub style: NodeStyle,
}

impl SegmentedControl {
    /// The surface its options take: size, chrome and fill.
    pub(crate) fn option_surface(&self) -> (ControlSize, SelectionChrome, bool) {
        (self.size, self.chrome, self.fill)
    }

    pub fn new() -> Self {
        Self {
            label: None,
            orientation: SelectionOrientation::Horizontal,
            chrome: SelectionChrome::Segmented,
            fill: false,
            size: ControlSize::Medium,
            options: Vec::new(),
            selected: None,
            focus_target: None,
            roving_focus: RovingFocusPolicy::default(),
            style: selection_chrome_style(SelectionChrome::Segmented, ControlSize::Medium, false),
        }
    }

    /// Same selection engine, radio chrome: a vertical stack of ring options.
    pub fn radio_group() -> Self {
        Self::new().chrome(SelectionChrome::Radio)
    }

    pub fn chrome(mut self, chrome: SelectionChrome) -> Self {
        self.chrome = chrome;
        self.orientation = chrome.orientation();
        self.style = selection_chrome_style(chrome, self.size, self.fill);
        self
    }

    pub const fn chrome_value(&self) -> SelectionChrome {
        self.chrome
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn size(mut self, size: ControlSize) -> Self {
        self.apply_size(size);
        self
    }

    pub(crate) fn apply_size(&mut self, size: ControlSize) {
        self.size = size;
        self.style = selection_chrome_style(self.chrome, size, self.fill);
    }
    pub fn fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self.style = selection_chrome_style(self.chrome, self.size, fill);
        self
    }
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub const fn fill_value(&self) -> bool {
        self.fill
    }

    pub const fn size_value(&self) -> ControlSize {
        self.size
    }

    pub fn options(&self) -> &[StableNodeId] {
        &self.options
    }

    pub const fn selected(&self) -> Option<StableNodeId> {
        self.selected
    }

    pub const fn focus_target(&self) -> Option<StableNodeId> {
        self.focus_target
    }
}

impl Default for SegmentedControl {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentView for SegmentedControl {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        slot_assembler: Some(crate::AppContext::assemble_segmented_control),
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
            tag: "segmented-control".into(),
        }
    }
    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &self.style,
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::RadioGroup,
                label: self.label.clone(),
                orientation: Some(self.orientation),
                ..AccessibilityState::default()
            },
        );
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SegmentedOption {
    pub label: Arc<str>,
    pub icon: Option<Icon>,
    pub(crate) selected: bool,
    pub(crate) disabled: bool,
    pub(crate) size: ControlSize,
    pub(crate) chrome: SelectionChrome,
    pub(crate) fill: bool,
    pub style: NodeStyle,
}

impl SegmentedOption {
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            selected: false,
            disabled: false,
            size: ControlSize::Medium,
            chrome: SelectionChrome::Segmented,
            fill: false,
            style: segmented_option_style(ControlSize::Medium, SelectionChrome::Segmented, false),
        }
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn size(mut self, size: ControlSize) -> Self {
        self.synchronize_surface(size, self.chrome, self.fill);
        self
    }
    pub fn surface(mut self, size: ControlSize, chrome: SelectionChrome, fill: bool) -> Self {
        self.synchronize_surface(size, chrome, fill);
        self
    }
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub const fn selected(&self) -> bool {
        self.selected
    }

    pub const fn disabled_value(&self) -> bool {
        self.disabled
    }

    pub const fn size_value(&self) -> ControlSize {
        self.size
    }

    pub(crate) fn synchronize_surface(
        &mut self,
        size: ControlSize,
        chrome: SelectionChrome,
        fill: bool,
    ) {
        self.size = size;
        self.chrome = chrome;
        self.fill = fill;
        self.style = segmented_option_style(size, chrome, fill);
    }
}

/// The segmented track's own inner padding.
const SEGMENTED_TRACK_PADDING: f32 = nana_ui_core::space::XXS;
/// The segmented track's rule.
const SEGMENTED_TRACK_BORDER: f32 = nana_ui_core::HAIRLINE;
/// How far the selected pill sits inside the track's outer edge.
///
/// Both terms are load-bearing. The pill's corners have to stay concentric
/// with the track's, so its radius is the track's minus **this** — padding
/// *and* rule. Issue #101's constant convergence replaced the literal `3.0`
/// here with `space::XXS`, which dropped the rule and left the pill a pixel
/// rounder than the corner it sits in. Deriving the inset from the two values
/// that produce it is what stops that happening again.
const SEGMENTED_PILL_INSET: f32 = SEGMENTED_TRACK_PADDING + SEGMENTED_TRACK_BORDER;

pub(crate) fn selection_chrome_style(
    chrome: SelectionChrome,
    size: ControlSize,
    fill: bool,
) -> NodeStyle {
    let (gap, padding, border_width, background, border) = match chrome {
        SelectionChrome::Segmented => (
            nana_ui_core::space::XXS,
            SEGMENTED_TRACK_PADDING,
            SEGMENTED_TRACK_BORDER,
            Some(SemanticColorRole::Background),
            Some(SemanticColorRole::Border),
        ),
        SelectionChrome::Tabs => (4.0, 0.0, 0.0, None, None),
        SelectionChrome::Radio => (2.0, 0.0, 0.0, None, None),
    };
    let vertical = matches!(chrome.orientation(), SelectionOrientation::Vertical);
    let layout = LayoutStyle {
        direction: Some(if vertical {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        }),
        gap: Some(LengthSpec::Px(gap)),
        padding: Some(LengthSpec::Px(padding)),
        width: Some(if fill {
            LengthSpec::Percent(100.0)
        } else {
            LengthSpec::Shrink
        }),
        height: if vertical {
            Some(LengthSpec::Shrink)
        } else {
            None
        },
        align_items: if vertical {
            AlignSpec::Stretch
        } else {
            AlignSpec::Center
        },
        border_width: Some(border_width),
        ..LayoutStyle::default()
    };
    NodeStyle {
        layout: Arc::new(layout),
        background,
        border,
        control_height: (!vertical).then_some(nana_ui_core::ControlHeight::Exact(size)),
        // A segmented control is a control, so it takes the controls' step
        // like Button and TextInput; `Md` is the card and panel step, which
        // on a compact track reads as a pill beside square-cornered buttons.
        radius: matches!(chrome, SelectionChrome::Segmented)
            .then_some(nana_ui_core::RadiusTier::Sm),
        ..NodeStyle::default()
    }
}

fn segmented_option_style(size: ControlSize, chrome: SelectionChrome, fill: bool) -> NodeStyle {
    let radio = matches!(chrome, SelectionChrome::Radio);
    let (padding_left, padding_right) = if radio {
        (
            Some(LengthSpec::Px(size.radio_lead())),
            Some(LengthSpec::Px(nana_ui_core::RADIO_ROW_INSET)),
        )
    } else {
        // Segmented / Tabs inset is `padding_x_in + 2`, spent at project against
        // the installed metrics so a density change can move it.
        (None, None)
    };
    let layout = LayoutStyle {
        padding_left,
        padding_right,
        font_size: Some(size.text_size()),
        font_weight: Some(if radio {
            nana_ui_core::type_scale::REGULAR
        } else {
            nana_ui_core::type_scale::MEDIUM
        }),
        line_height: Some(LineHeightSpec::Absolute(size.line_height())),
        white_space_nowrap: true,
        align_self: Some(if radio {
            AlignSpec::Stretch
        } else {
            AlignSpec::Center
        }),
        justify_content: if radio {
            JustifySpec::Start
        } else {
            JustifySpec::Center
        },
        border_radius: None,
        flex_grow: fill.then_some(1.0),
        flex_shrink: fill.then_some(1.0),
        width: fill.then_some(LengthSpec::Fill),
        ..LayoutStyle::default()
    };
    // A radio row keeps its label at rest colour and never fills: selection
    // reads from the ring, not from a highlighted row.
    let selected = if radio {
        SemanticPaint::default()
    } else {
        SemanticPaint {
            foreground: Some(SemanticColorRole::Text),
            background: Some(SemanticColorRole::Selected),
            ..SemanticPaint::default()
        }
    };
    NodeStyle {
        layout: Arc::new(layout),
        foreground: Some(if radio {
            SemanticColorRole::Text
        } else {
            SemanticColorRole::Muted
        }),
        interaction: InteractionStyle {
            selected,
            selected_hovered: SemanticPaint {
                background: Some(if radio {
                    SemanticColorRole::Hover
                } else {
                    SemanticColorRole::SelectedHover
                }),
                ..SemanticPaint::default()
            },
            selected_pressed: SemanticPaint {
                background: Some(if radio {
                    SemanticColorRole::Active
                } else {
                    SemanticColorRole::SelectedPressed
                }),
                ..SemanticPaint::default()
            },
            hovered: SemanticPaint {
                foreground: Some(SemanticColorRole::Text),
                background: Some(SemanticColorRole::Hover),
                ..SemanticPaint::default()
            },
            pressed: SemanticPaint {
                background: Some(SemanticColorRole::Active),
                ..SemanticPaint::default()
            },
            focused: SemanticPaint::FOCUS_SURFACE,
            disabled: SemanticPaint {
                foreground: Some(SemanticColorRole::Faint),
                ..SemanticPaint::default()
            },
            ..Default::default()
        },
        text_horizontal_alignment: if radio {
            TextHorizontalAlignment::Start
        } else {
            TextHorizontalAlignment::Center
        },
        text_vertical_alignment: TextVerticalAlignment::Center,
        control_height: (!matches!(chrome, SelectionChrome::Segmented))
            .then_some(nana_ui_core::ControlHeight::Exact(size)),
        ..NodeStyle::default()
    }
}

impl ComponentView for SegmentedOption {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::activate_segmented_option),
        slot_assembler: Some(crate::AppContext::assemble_segmented_option),
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
            tag: match self.chrome {
                SelectionChrome::Segmented => "segmented-option".into(),
                SelectionChrome::Tabs => "tab".into(),
                SelectionChrome::Radio => "radio".into(),
            },
        }
    }

    fn wants_metrics_reproject() -> bool {
        true
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.text(id) != Some(self.label.as_ref()) {
            mutations.set_text(
                id,
                TextContent {
                    value: self.label.to_string().into(),
                },
            );
        }
        let visual = StandardVisual::SelectionOption {
            label: Arc::clone(&self.label),
            icon: self.icon,
            selected: self.selected,
            disabled: self.disabled,
            size: self.size,
            show_focus_ring: matches!(self.chrome, SelectionChrome::Radio),
            indicator: matches!(self.chrome, SelectionChrome::Radio),
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        let mut effective_style = self.style.clone();
        let metrics = world.theme_metrics();
        let radius = match self.chrome {
            SelectionChrome::Segmented => (metrics.radius_sm - SEGMENTED_PILL_INSET).max(0.0),
            SelectionChrome::Tabs | SelectionChrome::Radio => metrics.radius_sm,
        };
        {
            let layout = Arc::make_mut(&mut effective_style.layout);
            layout.border_radius = Some(radius);
            if matches!(self.chrome, SelectionChrome::Segmented) {
                layout.height = Some(LengthSpec::Px(
                    (self.size.height_in(metrics) - nana_ui_core::space::SM).max(0.0),
                ));
            }
        }
        {
            let radio = matches!(self.chrome, SelectionChrome::Radio);
            let inset = if radio {
                self.size.radio_lead()
            } else {
                self.size.padding_x_in(world.theme_metrics()) + nana_ui_core::space::XXS
            };
            // The icon has no box of its own: it is drawn inside the leading
            // inset, so the inset has to grow by the icon and its gap or the
            // label is measured into a box the icon already occupies. The
            // trailing inset is the plain one either way — an icon on the left
            // is no reason to leave the label flush against the right edge, and
            // dropping it also measured every icon option short by one inset.
            let layout = Arc::make_mut(&mut effective_style.layout);
            layout.padding_left = Some(LengthSpec::Px(if self.icon.is_some() {
                inset + self.size.icon_size() + nana_ui_core::space::XS
            } else {
                inset
            }));
            if !radio {
                layout.padding_right = Some(LengthSpec::Px(inset));
            }
        }
        project_common(
            id,
            world,
            mutations,
            &effective_style,
            InteractionState {
                pointer_events: !self.disabled,
                focusable: !self.disabled,
            },
            AccessibilityState {
                role: match self.chrome {
                    SelectionChrome::Segmented | SelectionChrome::Radio => AccessibilityRole::Radio,
                    SelectionChrome::Tabs => AccessibilityRole::Tab,
                },
                label: Some(Arc::clone(&self.label)),
                disabled: self.disabled,
                selected: matches!(self.chrome, SelectionChrome::Tabs).then_some(self.selected),
                checked: (!matches!(self.chrome, SelectionChrome::Tabs)).then_some(self.selected),
                ..AccessibilityState::default()
            },
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentedSelectionRequested {
    pub option: StableNodeId,
}

/// Emitted on the option the user chose, after [`SegmentedSelectionRequested`]
/// on its control, so each option can carry its own handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentedOptionChosen;

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> StableNodeId {
        StableNodeId::new(value).unwrap()
    }

    #[test]
    fn roving_focus_skips_disabled_and_wraps() {
        let items = [(id(1), true), (id(2), false), (id(3), true)];
        let policy = RovingFocusPolicy::default();
        assert_eq!(
            policy.resolve(&items, Some(id(1)), RovingFocusIntent::Next),
            Some(id(3))
        );
        assert_eq!(
            policy.resolve(&items, Some(id(3)), RovingFocusIntent::Next),
            Some(id(1))
        );
        assert_eq!(
            policy.resolve(&items, Some(id(1)), RovingFocusIntent::Previous),
            Some(id(3))
        );
        assert_eq!(
            policy.resolve(&items, None, RovingFocusIntent::First),
            Some(id(1))
        );
        assert_eq!(
            policy.resolve(&items, None, RovingFocusIntent::Last),
            Some(id(3))
        );
    }

    #[test]
    fn segmented_sizes_preserve_concentric_layout_and_semantic_states() {
        for size in [ControlSize::Small, ControlSize::Medium, ControlSize::Large] {
            let option = SegmentedOption::new("Preview").size(size);
            assert!(option.style.layout.height.is_none());
            assert_eq!(option.style.layout.font_size, Some(size.text_size()));
            assert!(option.style.layout.white_space_nowrap);
            assert!(option.style.layout.border_radius.is_none());
            assert_eq!(
                option.style.interaction.selected.background,
                Some(SemanticColorRole::Selected)
            );
            let control = SegmentedControl::new().size(size);
            assert_eq!(
                control.style.control_height,
                Some(nana_ui_core::ControlHeight::Exact(size))
            );
            assert_eq!(control.style.radius, Some(nana_ui_core::RadiusTier::Sm));
            assert_eq!(control.style.layout.border_width, Some(1.0));
            assert!(control.style.layout.border_radius.is_none());
            assert_eq!(control.style.layout.width, Some(LengthSpec::Shrink));
        }
    }

    #[test]
    fn tabs_chrome_uses_independent_surface_and_tab_roles() {
        let tabs = crate::Tabs::new("preview");
        assert_eq!(tabs.size, ControlSize::Small);
        assert_eq!(tabs.style.layout.border_width, Some(0.0));
        assert_eq!(tabs.style.layout.gap, Some(LengthSpec::Px(4.0)));
        assert!(tabs.style.background.is_none());
        assert_eq!(tabs.node_kind(), NodeKind::Element { tag: "tabs".into() });

        let option = SegmentedOption::new("Preview").surface(
            ControlSize::Small,
            SelectionChrome::Tabs,
            false,
        );
        assert_eq!(
            option.style.control_height,
            Some(nana_ui_core::ControlHeight::Exact(ControlSize::Small))
        );
        assert_eq!(option.node_kind(), NodeKind::Element { tag: "tab".into() });
    }

    #[test]
    fn tabs_options_do_not_request_a_focus_ring() {
        let mut context = crate::AppContext::new();
        let document = crate::DocumentId::new(1).unwrap();
        let tab = context
            .create_component(
                document,
                SegmentedOption::new("Code").surface(
                    ControlSize::Small,
                    SelectionChrome::Tabs,
                    false,
                ),
            )
            .unwrap();
        let segmented = context
            .create_component(
                document,
                SegmentedOption::new("Code").size(ControlSize::Medium),
            )
            .unwrap();
        assert!(matches!(
            context.world().standard_visual(tab.stable_id()),
            Some(StandardVisual::SelectionOption {
                show_focus_ring: false,
                ..
            })
        ));
        assert!(matches!(
            context.world().standard_visual(segmented.stable_id()),
            Some(StandardVisual::SelectionOption {
                show_focus_ring: false,
                ..
            })
        ));
    }

    #[test]
    fn radio_chrome_stacks_vertically_and_leaves_room_for_the_ring() {
        let group = SegmentedControl::radio_group();
        assert_eq!(group.chrome_value(), SelectionChrome::Radio);
        assert_eq!(group.orientation, SelectionOrientation::Vertical);
        assert_eq!(group.style.layout.direction, Some(FlexDirection::Column));
        assert_eq!(group.style.layout.height, Some(LengthSpec::Shrink));
        assert_eq!(group.style.layout.border_width, Some(0.0));
        assert!(group.style.background.is_none());

        for size in [ControlSize::Small, ControlSize::Medium, ControlSize::Large] {
            let option =
                SegmentedOption::new("Manual").surface(size, SelectionChrome::Radio, false);
            assert_eq!(
                option.style.layout.padding_left,
                Some(LengthSpec::Px(size.radio_lead()))
            );
            assert_eq!(
                option.style.control_height,
                Some(nana_ui_core::ControlHeight::Exact(size))
            );
            assert_eq!(option.style.layout.justify_content, JustifySpec::Start);
            // Selection reads from the ring, so the row is never filled.
            assert!(option.style.interaction.selected.background.is_none());
            assert_eq!(
                option.node_kind(),
                NodeKind::Element {
                    tag: "radio".into()
                }
            );
        }
    }

    #[test]
    fn a_radio_group_selects_one_option_and_publishes_ring_geometry() {
        let mut context = crate::AppContext::new();
        let document = crate::DocumentId::new(1).unwrap();
        let group = context
            .create_component(document, SegmentedControl::radio_group().label("Mode"))
            .unwrap();
        let manual = context
            .create_component(document, SegmentedOption::new("Manual"))
            .unwrap();
        let automatic = context
            .create_component(document, SegmentedOption::new("Automatic"))
            .unwrap();
        context.append_child(group, manual).unwrap();
        context.append_child(group, automatic).unwrap();
        assert!(
            context
                .set_segmented_options(group, vec![manual, automatic], Some(automatic))
                .unwrap()
        );

        // Both options inherit radio chrome from the group, not from their own
        // construction, so the group stays the single source of surface truth.
        for option in [manual, automatic] {
            assert!(matches!(
                context.world().standard_visual(option.stable_id()),
                Some(StandardVisual::SelectionOption {
                    indicator: true,
                    show_focus_ring: true,
                    ..
                })
            ));
            assert_eq!(
                context
                    .world()
                    .accessibility(option.stable_id())
                    .map(|state| state.role),
                Some(AccessibilityRole::Radio)
            );
        }
        assert_eq!(
            context
                .world()
                .accessibility(manual.stable_id())
                .and_then(|state| state.checked),
            Some(false)
        );
        assert_eq!(
            context
                .world()
                .accessibility(automatic.stable_id())
                .and_then(|state| state.checked),
            Some(true)
        );
        assert_eq!(
            context
                .world()
                .accessibility(group.stable_id())
                .and_then(|state| state.orientation),
            Some(SelectionOrientation::Vertical)
        );

        let mut mutations = crate::MutationQueue::new();
        for (index, option) in [manual, automatic].iter().enumerate() {
            mutations.write_layout(
                option.stable_id(),
                crate::LayoutBox {
                    x: 0.0,
                    y: index as f32 * 30.0,
                    width: 160.0,
                    height: 28.0,
                },
            );
        }
        context.commit_mutations(mutations).unwrap();

        let ring = |option: crate::Entity<SegmentedOption>| match context
            .world()
            .component_geometry(option.stable_id())
        {
            Some(crate::ComponentGeometry::SelectionOption { indicator, .. }) => indicator,
            other => panic!("expected selection option geometry, got {other:?}"),
        };
        let unselected = ring(manual).expect("radio ring");
        let selected = ring(automatic).expect("radio ring");
        assert!(unselected.dot.is_none());
        let (dot, _) = selected.dot.expect("selected radio dot");
        // The dot sits concentric inside the ring.
        assert!(
            (dot.x + dot.width / 2.0 - (selected.ring.x + selected.ring.width / 2.0)).abs() < 0.001
        );
        assert!(dot.width < selected.ring.width);
        assert_eq!(
            unselected.ring.x,
            nana_ui_core::RADIO_ROW_INSET,
            "ring hugs the row inset"
        );
    }
}

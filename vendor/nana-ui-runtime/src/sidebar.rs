use std::{sync::Arc, time::Duration};

use nana_ui_core::{
    AlignSpec, ButtonKind, ControlSize, FlexDirection, Icon, JustifySpec, LayoutStyle, LengthSpec,
    LineHeightSpec, OverflowSpec, SemanticColorRole, TooltipConfig, UI_METRICS,
};

use crate::{
    AccessibilityRole, AccessibilityState, AppContext, ComponentView, Entity, FrameworkError,
    InteractionState, InteractionStyle, MutationQueue, NodeKind, NodeStyle, OverlayHostState,
    SemanticPaint, StableNodeId, StandardVisual, TextContent, TooltipVisual, UiWorld,
    view_components::{
        IconButton, List, ListItem, ListItemSlots, ScrollAxes, ScrollView, Text, project_common,
    },
};

const FRAME_PADDING_TOP: f32 = nana_ui_core::space::LG;
/// Narrower than the left inset: the scrollbar overlay lives on this edge.
const FRAME_PADDING_RIGHT: f32 = nana_ui_core::space::MD;
const FRAME_PADDING_BOTTOM: f32 = nana_ui_core::space::LG;
const FRAME_PADDING_LEFT: f32 = nana_ui_core::space::XL;
const FRAME_GAP: f32 = nana_ui_core::space::XXL;
const ROW_PADDING_LEFT: f32 = nana_ui_core::space::MD;
const ROW_PADDING_RIGHT: f32 = nana_ui_core::space::MD;
const ROW_ICON_SIZE: f32 = ControlSize::Small.icon_size();
const ROW_TREE_FIRST_DEPTH_INSET: f32 = nana_ui_core::space::PAGE + nana_ui_core::space::SM;
use crate::popover::TREE_DEPTH_STEP as ROW_TREE_DEPTH_STEP;
const SECTION_ANIMATION_DURATION: Duration = nana_ui_core::motion::SIDEBAR_COLLAPSE;
const SECTION_HEADER_GAP: f32 = nana_ui_core::space::XS;
const SECTION_HEADER_TITLE_SIZE: f32 = nana_ui_core::type_scale::HINT;
const SECTION_HEADER_TITLE_WEIGHT: u16 = nana_ui_core::type_scale::BOLD;
const SECTION_DISCLOSURE_SIZE: f32 = nana_ui_core::type_scale::META;
const SECTION_TOOL_EDGE: f32 = nana_ui_core::space::PAGE_TIGHT;
const SECTION_COUNT_SIZE: f32 = nana_ui_core::type_scale::HINT;
const SECTION_BODY_GAP: f32 = nana_ui_core::space::XXS;
const SECTION_EMPTY_HEIGHT: f32 = nana_ui_core::space::PAGE + nana_ui_core::space::SM;
const SECTION_EMPTY_PADDING_Y: f32 = nana_ui_core::space::SM;
const SECTION_EMPTY_PADDING_X: f32 = nana_ui_core::space::MD;

fn tool_column_center_inset(metrics: nana_ui_core::ThemeMetrics) -> f32 {
    ROW_PADDING_RIGHT + metrics.icon_button_size / 2.0
}

fn tool_column_trailing_margin(metrics: nana_ui_core::ThemeMetrics) -> f32 {
    tool_column_center_inset(metrics) - ROW_PADDING_RIGHT - SECTION_TOOL_EDGE / 2.0
}
const SECTION_EMPTY_FONT_SIZE: f32 = nana_ui_core::type_scale::META;
const FOOTER_GAP: f32 = nana_ui_core::space::XXS;

/// Visual selection contract for a navigation row. This is not a generic list item.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SidebarRowState {
    #[default]
    Idle,
    Active,
    AncestorActive,
    Disabled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SidebarRowTone {
    #[default]
    Default,
    Warning,
    Error,
}

pub fn sidebar_row_depth_inset(depth: u16) -> f32 {
    if depth == 0 {
        ROW_PADDING_LEFT
    } else {
        ROW_TREE_FIRST_DEPTH_INSET + f32::from(depth - 1) * ROW_TREE_DEPTH_STEP
    }
}

/// Standard trailing tool for the sidebar top bar; drops into an unpadded
/// `Stack::bar` and lands on the shared tool column by itself.
pub fn sidebar_top_bar_tool_button(icon: Icon, label: impl Into<Arc<str>>) -> IconButton {
    let label: Arc<str> = label.into();
    let mut button = IconButton::new(icon, Arc::clone(&label))
        .kind(ButtonKind::Text)
        .with_tooltip(label);
    {
        let layout = Arc::make_mut(&mut button.style.layout);
        layout.padding_left = Some(LengthSpec::Px(0.0));
        layout.padding_right = Some(LengthSpec::Px(0.0));
        layout.margin_right = Some(LengthSpec::Px(ROW_PADDING_RIGHT));
    }
    button.style.control_padding_x = None;
    button
}

/// Inline small tool for section headers, sized to the header row; the header
/// carries it on the shared tool column by itself.
pub fn sidebar_section_tool_button(icon: Icon, label: impl Into<Arc<str>>) -> IconButton {
    sidebar_tool_button(
        icon,
        label,
        SECTION_TOOL_EDGE,
        tool_column_trailing_margin(UI_METRICS),
    )
}

/// Inline small tools for data rows; drop into the row tools host, which lands
/// the trailing tool on the shared tool column so a cluster keeps its own gap.
pub fn sidebar_row_tool_button(icon: Icon, label: impl Into<Arc<str>>) -> IconButton {
    sidebar_tool_button(icon, label, SECTION_TOOL_EDGE, 0.0)
}

fn sidebar_tool_button(
    icon: Icon,
    label: impl Into<Arc<str>>,
    edge: f32,
    trailing_margin: f32,
) -> IconButton {
    let label: Arc<str> = label.into();
    let mut button = IconButton::new(icon, Arc::clone(&label))
        .kind(ButtonKind::Text)
        .size(ControlSize::Small)
        .with_tooltip(label);
    {
        let layout = Arc::make_mut(&mut button.style.layout);
        let edge = LengthSpec::Px(edge);
        layout.min_width = Some(edge);
        layout.min_height = Some(edge);
        layout.width = Some(edge);
        layout.height = Some(edge);
        layout.padding_left = Some(LengthSpec::Px(0.0));
        layout.padding_right = Some(LengthSpec::Px(0.0));
        layout.margin_right = Some(LengthSpec::Px(trailing_margin));
    }
    button.style.square = None;
    button.style.control_padding_x = None;
    button.style.radius = Some(nana_ui_core::RadiusTier::Sm);
    button
}

/// Boolean expansion target. Visual progress is the SIDEBAR Motion IR track,
/// sampled onto [`SidebarSection::animation_progress`].
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarSectionState {
    expanded: bool,
}

impl SidebarSectionState {
    pub fn new(expanded: bool) -> Self {
        Self { expanded }
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn set_expanded(&mut self, expanded: bool, _now: Duration) -> bool {
        if self.expanded == expanded {
            return false;
        }
        self.expanded = expanded;
        true
    }

    pub fn toggle(&mut self, now: Duration) -> bool {
        self.set_expanded(!self.expanded, now)
    }

    pub fn animation_duration() -> Duration {
        SECTION_ANIMATION_DURATION
    }
}

impl Default for SidebarSectionState {
    fn default() -> Self {
        Self::new(true)
    }
}

/// Fixed top/footer with an independently scrolling body.
///
/// `body` is the vertical [`ScrollView`] slot. Content is a child of that
/// scrollport; top and footer stay unscoped siblings.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarFrame {
    pub top: Option<StableNodeId>,
    pub body: Option<StableNodeId>,
    pub footer: Option<StableNodeId>,
    pub gap: f32,
    pub style: NodeStyle,
}

fn apply_body_scroll_layout(layout: &mut LayoutStyle) {
    layout.overflow_y = OverflowSpec::Scroll;
    if layout.flex_grow.is_none_or(|grow| grow <= 0.0) {
        layout.flex_grow = Some(1.0);
    }
    if layout.flex_shrink.is_none() {
        layout.flex_shrink = Some(1.0);
    }
    if layout.height.is_none() {
        layout.height = Some(LengthSpec::Fill);
    }
    if layout.width.is_none() {
        layout.width = Some(LengthSpec::Fill);
    }
    if layout.min_height.is_none() {
        layout.min_height = Some(LengthSpec::Px(0.0));
    }
    if layout.min_width.is_none() {
        layout.min_width = Some(LengthSpec::Px(0.0));
    }
    layout.align_items = AlignSpec::Stretch;
}

impl SidebarFrame {
    pub fn new() -> Self {
        Self {
            top: None,
            body: None,
            footer: None,
            gap: FRAME_GAP,
            style: NodeStyle::default(),
        }
    }

    /// Vertical Runtime scrollport used as the independently scrolling body.
    pub fn vertical_body_scroll() -> ScrollView {
        Self::scroll_body(LayoutStyle::default())
    }

    /// Project host layout onto the body slot as a vertical [`ScrollView`].
    pub fn scroll_body(layout: LayoutStyle) -> ScrollView {
        let mut style = NodeStyle {
            layout: Arc::new(layout),
            ..NodeStyle::default()
        };
        apply_body_scroll_layout(Arc::make_mut(&mut style.layout));
        ScrollView::new(ScrollAxes::Vertical).style(style)
    }

    fn project_body_scroll(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        let Some(body) = self.body else {
            return;
        };
        let mut style = world.node_style(body).cloned().unwrap_or_default();
        apply_body_scroll_layout(Arc::make_mut(&mut style.layout));
        ScrollView::new(ScrollAxes::Vertical)
            .style(style)
            .project_scrollport(body, world, mutations);
    }

    pub fn top(mut self, top: StableNodeId) -> Self {
        self.top = Some(top);
        self
    }

    pub fn body(mut self, body: StableNodeId) -> Self {
        self.body = Some(body);
        self
    }

    pub fn footer(mut self, footer: StableNodeId) -> Self {
        self.footer = Some(footer);
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.background = None;
        layout.direction = Some(layout.direction.unwrap_or(FlexDirection::Column));
        layout.align_items = AlignSpec::Stretch;
        layout.width = Some(layout.width.unwrap_or(LengthSpec::Fill));
        layout.height = Some(layout.height.unwrap_or(LengthSpec::Fill));
        layout.gap = Some(layout.gap.unwrap_or(LengthSpec::Px(self.gap)));
        if layout.padding.is_none()
            && layout.padding_top.is_none()
            && layout.padding_right.is_none()
            && layout.padding_bottom.is_none()
            && layout.padding_left.is_none()
        {
            layout.padding_top = Some(LengthSpec::Px(FRAME_PADDING_TOP));
            layout.padding_right = Some(LengthSpec::Px(FRAME_PADDING_RIGHT));
            layout.padding_bottom = Some(LengthSpec::Px(FRAME_PADDING_BOTTOM));
            layout.padding_left = Some(LengthSpec::Px(FRAME_PADDING_LEFT));
        }
        if !layout.overflow_x.clips() {
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
        }
        if !layout.overflow_y.clips() {
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        }
        style
    }
}

impl Default for SidebarFrame {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentView for SidebarFrame {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    /// Lends its body node to a scrollport that another component projects too.
    const ALWAYS_REPROJECT: bool = true;

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "sidebar-frame".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.text(id) != Some("") {
            mutations.set_text(
                id,
                TextContent {
                    value: String::new().into(),
                },
            );
        }
        if world.standard_visual(id).is_some() {
            mutations.set_standard_visual(id, None);
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                ..AccessibilityState::default()
            },
        );
        self.project_body_scroll(world, mutations);
    }
}

/// Navigation row. Reuses ListItem slot paint; keeps a distinct catalog identity.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarRow {
    pub label: Arc<str>,
    pub slots: ListItemSlots,
    pub tools: Option<StableNodeId>,
    pub depth: u16,
    pub size: ControlSize,
    pub gap: f32,
    pub state: SidebarRowState,
    pub tone: SidebarRowTone,
    pub disclosure: Option<bool>,
    pub hovered: bool,
    pub style: NodeStyle,
}

impl SidebarRow {
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            slots: ListItemSlots::default(),
            tools: None,
            depth: 0,
            size: ControlSize::Small,
            gap: nana_ui_core::space::SM,
            state: SidebarRowState::Idle,
            tone: SidebarRowTone::Default,
            disclosure: None,
            hovered: false,
            style: NodeStyle::default(),
        }
    }

    pub fn slots(mut self, slots: ListItemSlots) -> Self {
        self.slots = slots;
        self
    }

    pub fn tools(mut self, tools: StableNodeId) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn depth(mut self, depth: u16) -> Self {
        self.depth = depth;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    pub fn state(mut self, state: SidebarRowState) -> Self {
        self.state = state;
        self
    }

    pub fn tone(mut self, tone: SidebarRowTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn disclosure(mut self, expanded: bool) -> Self {
        self.disclosure = Some(expanded);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn selected(&self) -> bool {
        matches!(
            self.state,
            SidebarRowState::Active | SidebarRowState::AncestorActive
        )
    }

    pub fn disabled(&self) -> bool {
        self.state == SidebarRowState::Disabled
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(FlexDirection::Row);
        layout.align_items = AlignSpec::Center;
        layout.justify_content = if self.visible_tools().is_some() {
            if self.slots.leading.is_some() {
                JustifySpec::SpaceBetween
            } else {
                JustifySpec::End
            }
        } else {
            JustifySpec::Start
        };
        layout.width = Some(LengthSpec::Fill);
        layout.gap = Some(LengthSpec::Px(self.gap));
        layout.padding_left = Some(LengthSpec::Px(sidebar_row_depth_inset(self.depth)));
        layout.padding_right = Some(LengthSpec::Px(ROW_PADDING_RIGHT));
        layout.font_size = Some(self.size.text_size());
        layout.line_height = Some(LineHeightSpec::Absolute(self.size.line_height()));
        layout.font_weight = Some(if self.state == SidebarRowState::AncestorActive {
            600
        } else {
            500
        });
        // Only the default: an author who set an explicit px radius keeps it.
        let unset_radius = layout.border_radius.is_none();
        if unset_radius {
            style.radius = Some(nana_ui_core::RadiusTier::Sm);
        }
        style.control_height = Some(nana_ui_core::ControlHeight::Exact(self.size));
        // Default rows share Text; the Selected plate marks the current route.
        style.foreground = Some(match (self.tone, self.state) {
            (SidebarRowTone::Warning, _) => SemanticColorRole::Warning,
            (SidebarRowTone::Error, _) => SemanticColorRole::Danger,
            (_, SidebarRowState::Disabled) => SemanticColorRole::Faint,
            _ => SemanticColorRole::Text,
        });
        style.background = None;
        style.interaction = InteractionStyle {
            selected: SemanticPaint {
                background: Some(SemanticColorRole::Selected),
                ..SemanticPaint::default()
            },
            selected_hovered: SemanticPaint {
                background: Some(SemanticColorRole::SelectedHover),
                ..SemanticPaint::default()
            },
            selected_pressed: SemanticPaint {
                background: Some(SemanticColorRole::SelectedPressed),
                ..SemanticPaint::default()
            },
            hovered: SemanticPaint {
                background: Some(if self.selected() {
                    SemanticColorRole::SelectedHover
                } else {
                    SemanticColorRole::Hover
                }),
                ..SemanticPaint::default()
            },
            pressed: SemanticPaint {
                background: Some(if self.selected() {
                    SemanticColorRole::SelectedPressed
                } else {
                    SemanticColorRole::Active
                }),
                ..SemanticPaint::default()
            },
            focused: SemanticPaint::FOCUS_SURFACE,
            disabled: SemanticPaint {
                foreground: Some(SemanticColorRole::Faint),
                ..SemanticPaint::default()
            },
            ..InteractionStyle::default()
        };
        style.text_vertical_alignment = crate::TextVerticalAlignment::Center;
        style
    }

    /// Row tools stay out of the way until the pointer is on the row, so a long
    /// label owns the full width at rest instead of being clipped by chrome the
    /// user cannot reach yet.
    fn visible_tools(&self) -> Option<StableNodeId> {
        self.tools.filter(|_| self.hovered)
    }

    fn projected_slots(&self) -> ListItemSlots {
        ListItemSlots {
            leading: self.slots.leading,
            content: self.slots.content,
            trailing: self.visible_tools().or(self.slots.trailing),
        }
    }

    fn project_tools_layout(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        let Some(tools) = self.tools else {
            return;
        };
        if world.node(tools).is_none() {
            return;
        }
        let mut style = world.node_style(tools).cloned().unwrap_or_default();
        let hidden = !self.hovered;
        let layout = Arc::make_mut(&mut style.layout);
        let mut changed = false;
        if layout.flex_grow != Some(0.0) {
            layout.flex_grow = Some(0.0);
            changed = true;
        }
        if layout.flex_shrink != Some(0.0) {
            layout.flex_shrink = Some(0.0);
            changed = true;
        }
        if layout.width != Some(LengthSpec::Shrink) {
            layout.width = Some(LengthSpec::Shrink);
            changed = true;
        }
        // The tools slot lands its trailing tool on the shared glyph column
        // behind the row's trailing padding; tools themselves stay margin-free
        // so a cluster keeps the host gap between its buttons.
        let column_margin = Some(LengthSpec::Px(tool_column_trailing_margin(
            world.theme_metrics(),
        )));
        if layout.margin_right != column_margin {
            layout.margin_right = column_margin;
            changed = true;
        }
        if layout.hidden != hidden {
            layout.hidden = hidden;
            changed = true;
        }
        if changed {
            mutations.set_style(tools, style);
        }
    }
}

impl ComponentView for SidebarRow {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::activate_sidebar_row),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    /// Patches the layout of the tools slot, which another component owns.
    const ALWAYS_REPROJECT: bool = true;

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "sidebar-row".into(),
        }
    }

    fn wants_metrics_reproject() -> bool {
        true
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let mut item = ListItem::new(self.label.as_ref())
            .slots(self.projected_slots())
            .gap(self.gap)
            .size(self.size)
            .selected(self.selected())
            .disabled(self.disabled());
        item.style = self.effective_style();
        item.project(id, world, mutations);
        self.project_tools_layout(world, mutations);
    }
}

/// Leading glyph sized to the row text. Not an IconButton (those force a ~28px target).
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarRowIcon {
    pub icon: Icon,
}

impl SidebarRowIcon {
    pub fn new(icon: Icon) -> Self {
        Self { icon }
    }

    fn style() -> NodeStyle {
        let mut style = NodeStyle::default();
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(ROW_ICON_SIZE));
        layout.height = Some(LengthSpec::Px(ROW_ICON_SIZE));
        layout.min_width = Some(LengthSpec::Px(ROW_ICON_SIZE));
        layout.min_height = Some(LengthSpec::Px(ROW_ICON_SIZE));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        style
    }
}

impl ComponentView for SidebarRowIcon {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "sidebar-row-icon".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.text(id) != Some("") {
            mutations.set_text(
                id,
                TextContent {
                    value: String::new().into(),
                },
            );
        }
        let visual = StandardVisual::Icon {
            icon: self.icon,
            size: ROW_ICON_SIZE,
            tooltip: None,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        project_common(
            id,
            world,
            mutations,
            &Self::style(),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState::default(),
        );
    }
}

/// Application-owned section slots. Header chrome reuses ListItem + Text + Icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SidebarSectionSlots {
    pub header: Option<StableNodeId>,
    pub body: Option<StableNodeId>,
    pub tools: Option<StableNodeId>,
}

/// Titled, optionally collapsible group. Expansion progress is Motion IR.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarSection {
    pub title: Arc<str>,
    pub count: Option<usize>,
    pub tools: Option<StableNodeId>,
    pub empty_text: Option<Arc<str>>,
    pub size: ControlSize,
    pub collapsible: bool,
    pub disabled: bool,
    pub state: SidebarSectionState,
    pub animation_progress: f32,
    pub header: Option<StableNodeId>,
    pub body: Option<StableNodeId>,
    pub disclosure: Option<StableNodeId>,
    pub title_slot: Option<StableNodeId>,
    pub count_slot: Option<StableNodeId>,
    pub header_hovered: bool,
    pub style: NodeStyle,
}

impl SidebarSection {
    pub fn new(title: impl Into<Arc<str>>) -> Self {
        Self {
            title: title.into(),
            count: None,
            tools: None,
            empty_text: None,
            size: ControlSize::Small,
            collapsible: false,
            disabled: false,
            state: SidebarSectionState::new(true),
            animation_progress: 1.0,
            header: None,
            body: None,
            disclosure: None,
            title_slot: None,
            count_slot: None,
            header_hovered: false,
            style: NodeStyle::default(),
        }
    }

    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    pub fn tools(mut self, tools: StableNodeId) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn empty_text(mut self, empty_text: impl Into<Arc<str>>) -> Self {
        self.empty_text = Some(empty_text.into());
        self
    }

    pub fn collapsible(mut self, collapsible: bool) -> Self {
        self.collapsible = collapsible;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn expanded(mut self, expanded: bool) -> Self {
        self.state = SidebarSectionState::new(expanded);
        self.animation_progress = if expanded { 1.0 } else { 0.0 };
        self
    }

    pub fn header(mut self, header: StableNodeId) -> Self {
        self.header = Some(header);
        self
    }

    pub fn body(mut self, body: StableNodeId) -> Self {
        self.body = Some(body);
        self
    }

    pub fn disclosure(mut self, disclosure: StableNodeId) -> Self {
        self.disclosure = Some(disclosure);
        self
    }

    pub fn title_slot(mut self, title: StableNodeId) -> Self {
        self.title_slot = Some(title);
        self
    }

    pub fn count_slot(mut self, count: StableNodeId) -> Self {
        self.count_slot = Some(count);
        self
    }

    pub fn slots(mut self, slots: SidebarSectionSlots) -> Self {
        self.header = slots.header;
        self.body = slots.body;
        self.tools = slots.tools;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn slot_ids(&self) -> SidebarSectionSlots {
        SidebarSectionSlots {
            header: self.header,
            body: self.body,
            tools: self.tools,
        }
    }

    pub fn header_item(&self) -> ListItem {
        let mut item = ListItem::new(self.header_title())
            .gap(SECTION_HEADER_GAP)
            .size(self.size)
            .slots(self.header_list_slots());
        item.style = self.header_style();
        item
    }

    /// 12px disclosure host. Projection paints [`StandardVisual::Icon`].
    pub fn disclosure_mark(&self) -> IconButton {
        IconButton::new(self.disclosure_icon(), Arc::from(""))
            .style(disclosure_host_style(self.collapsible))
    }

    fn disclosure_icon(&self) -> Icon {
        disclosure_icon_kind(self.expansion_ratio())
    }

    pub fn title_label(&self) -> Text {
        let mut label = Text::new(self.header_title());
        label.style = title_slot_style();
        label
    }

    pub fn count_label(&self) -> Text {
        let mut label = Text::new(self.count_text());
        label.style = count_slot_style(self.count_visible());
        label
    }

    /// Clip port for application-owned section children.
    ///
    /// Expanded ports are unconstrained. `List::project` must not store a
    /// permanent height of 0, or it will clobber the section's body layout.
    pub fn body_port() -> List {
        let mut body = List::new();
        body.style = body_port_style(1.0, None, 0.0);
        body
    }

    fn header_title(&self) -> String {
        self.title.to_uppercase()
    }

    fn count_text(&self) -> String {
        self.count
            .map(|count| count.to_string())
            .unwrap_or_default()
    }

    fn toggleable(&self) -> bool {
        self.collapsible && !self.disabled
    }

    fn pointer_target(&self) -> bool {
        !self.disabled && (self.collapsible || self.tools.is_some())
    }

    fn expansion_ratio(&self) -> f32 {
        if self.collapsible {
            self.animation_progress.clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    fn count_visible(&self) -> bool {
        self.count.is_some() && !(self.header_hovered && self.tools.is_some())
    }

    fn header_list_slots(&self) -> ListItemSlots {
        ListItemSlots {
            leading: self.disclosure.filter(|_| self.collapsible),
            content: self.title_slot,
            trailing: if self.header_hovered {
                self.tools.or(self.count_slot)
            } else {
                self.count_slot
            },
        }
    }

    fn header_style(&self) -> NodeStyle {
        let mut style = NodeStyle::default();
        style.foreground = Some(SemanticColorRole::Faint);
        style.background =
            (self.header_hovered && self.toggleable()).then_some(SemanticColorRole::Hover);
        style.text_vertical_alignment = crate::TextVerticalAlignment::Center;
        style.control_height = Some(nana_ui_core::ControlHeight::Exact(self.size));
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(FlexDirection::Row);
        layout.align_items = AlignSpec::Center;
        layout.width = Some(LengthSpec::Fill);
        layout.gap = Some(LengthSpec::Px(SECTION_HEADER_GAP));
        layout.padding_left = Some(LengthSpec::Px(ROW_PADDING_LEFT));
        layout.padding_right = Some(LengthSpec::Px(ROW_PADDING_RIGHT));
        layout.font_size = Some(SECTION_HEADER_TITLE_SIZE);
        layout.line_height = Some(LineHeightSpec::Absolute(SECTION_HEADER_TITLE_SIZE));
        layout.font_weight = Some(SECTION_HEADER_TITLE_WEIGHT);
        layout.white_space_nowrap = true;
        layout.text_overflow_ellipsis = true;
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        style.radius = Some(nana_ui_core::RadiusTier::Sm);
        style
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(FlexDirection::Column);
        layout.width = Some(LengthSpec::Fill);
        layout.align_items = AlignSpec::Stretch;
        // Only the default: an author who set an explicit px radius keeps it.
        let unset_radius = layout.border_radius.is_none();
        if unset_radius {
            style.radius = Some(nana_ui_core::RadiusTier::Sm);
        }
        style.foreground = Some(SemanticColorRole::Faint);
        style.interaction = if self.toggleable() {
            InteractionStyle {
                pressed: SemanticPaint {
                    background: Some(SemanticColorRole::Active),
                    ..SemanticPaint::default()
                },
                focused: SemanticPaint::FOCUS_SURFACE,
                ..InteractionStyle::default()
            }
        } else {
            InteractionStyle::default()
        };
        style
    }

    fn project_header(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        let Some(header) = self.header else {
            return;
        };
        self.header_item().project(header, world, mutations);
        project_common(
            header,
            world,
            mutations,
            &self.header_style(),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                ..AccessibilityState::default()
            },
        );
    }

    fn project_disclosure(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        let Some(id) = self.disclosure else {
            return;
        };
        if world.text(id) != Some("") {
            mutations.set_text(
                id,
                TextContent {
                    value: String::new().into(),
                },
            );
        }
        let visual = StandardVisual::Icon {
            icon: self.disclosure_icon(),
            size: SECTION_DISCLOSURE_SIZE,
            tooltip: None,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        project_common(
            id,
            world,
            mutations,
            &disclosure_host_style(self.collapsible),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState::default(),
        );
    }

    fn project_title_slot(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        let Some(id) = self.title_slot else {
            return;
        };
        let mut label = Text::new(self.header_title());
        label.style = title_slot_style();
        label.project(id, world, mutations);
    }

    fn project_count_slot(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        let Some(id) = self.count_slot else {
            return;
        };
        let mut label = Text::new(self.count_text());
        label.style = count_slot_style(self.count_visible());
        label.project(id, world, mutations);
    }

    fn project_tools_visibility(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        let Some(id) = self.tools else {
            return;
        };
        let Some(current) = world.node_style(id) else {
            return;
        };
        let hide = !self.header_hovered;
        if current.layout.hidden == hide {
            return;
        }
        let mut style = current.clone();
        Arc::make_mut(&mut style.layout).hidden = hide;
        mutations.set_style(id, style);
    }

    fn project_body(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        let Some(body) = self.body else {
            return;
        };
        let expansion = self.expansion_ratio();
        let children_empty = world.node(body).is_none_or(|node| node.children.is_empty());
        let empty = children_empty.then(|| self.empty_text.clone()).flatten();
        let content_height = self.body_content_height(world);
        let style = body_port_style(expansion, empty.as_deref(), content_height);
        let text = empty.as_deref().unwrap_or("");
        if world.text(body) != Some(text) {
            mutations.set_text(
                body,
                TextContent {
                    value: text.to_owned().into(),
                },
            );
        }
        if world.standard_visual(body).is_some() {
            mutations.set_standard_visual(body, None);
        }
        project_common(
            body,
            world,
            mutations,
            &style,
            InteractionState {
                pointer_events: true,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                ..AccessibilityState::default()
            },
        );
    }

    fn body_content_height(&self, world: &UiWorld) -> f32 {
        let Some(body) = self.body else {
            return if self.empty_text.is_some() {
                SECTION_EMPTY_HEIGHT
            } else {
                0.0
            };
        };
        let children = world
            .node(body)
            .map(|node| node.children)
            .unwrap_or_default();
        let visible = children
            .into_iter()
            .filter(|id| {
                world
                    .node_style(*id)
                    .is_none_or(|style| !style.layout.hidden)
            })
            .collect::<Vec<_>>();
        if visible.is_empty() {
            return if self.empty_text.is_some() {
                SECTION_EMPTY_HEIGHT
            } else {
                0.0
            };
        }
        let mut total = 0.0;
        for (index, id) in visible.iter().enumerate() {
            if index > 0 {
                total += SECTION_BODY_GAP;
            }
            total += child_block_height(world, *id, self.size.height_in(world.theme_metrics()));
        }
        total
    }
}

fn length_px(spec: Option<LengthSpec>) -> Option<f32> {
    match spec {
        Some(LengthSpec::Px(value)) => Some(value),
        _ => None,
    }
}

fn child_block_height(world: &UiWorld, id: StableNodeId, fallback: f32) -> f32 {
    world
        .node_style(id)
        .and_then(|style| {
            length_px(style.layout.height).or_else(|| length_px(style.layout.min_height))
        })
        .or_else(|| world.layout_box(id).map(|bounds| bounds.height))
        .filter(|height| *height > 0.0)
        .unwrap_or(fallback)
}

fn disclosure_icon_kind(expansion: f32) -> Icon {
    if expansion < 0.5 {
        Icon::ChevronRight
    } else {
        Icon::ChevronDown
    }
}

fn disclosure_host_style(visible: bool) -> NodeStyle {
    let mut style = NodeStyle::default();
    style.foreground = Some(SemanticColorRole::Faint);
    let layout = Arc::make_mut(&mut style.layout);
    layout.width = Some(LengthSpec::Px(SECTION_DISCLOSURE_SIZE));
    layout.height = Some(LengthSpec::Px(SECTION_DISCLOSURE_SIZE));
    layout.min_width = Some(LengthSpec::Px(SECTION_DISCLOSURE_SIZE));
    layout.min_height = Some(LengthSpec::Px(SECTION_DISCLOSURE_SIZE));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.hidden = !visible;
    style
}

fn title_slot_style() -> NodeStyle {
    let mut style = NodeStyle::default();
    style.foreground = Some(SemanticColorRole::Faint);
    style.text_vertical_alignment = crate::TextVerticalAlignment::Center;
    let layout = Arc::make_mut(&mut style.layout);
    layout.font_size = Some(SECTION_HEADER_TITLE_SIZE);
    layout.line_height = Some(LineHeightSpec::Absolute(SECTION_HEADER_TITLE_SIZE));
    layout.font_weight = Some(SECTION_HEADER_TITLE_WEIGHT);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.width = Some(LengthSpec::Fill);
    layout.white_space_nowrap = true;
    layout.text_overflow_ellipsis = true;
    style
}

fn count_slot_style(visible: bool) -> NodeStyle {
    let mut style = NodeStyle::default();
    style.foreground = Some(SemanticColorRole::Faint);
    style.text_vertical_alignment = crate::TextVerticalAlignment::Center;
    let layout = Arc::make_mut(&mut style.layout);
    layout.font_size = Some(SECTION_COUNT_SIZE);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.hidden = !visible;
    style
}

fn body_port_style(expansion: f32, empty_text: Option<&str>, content_height: f32) -> NodeStyle {
    let mut style = NodeStyle::default();
    style.foreground = Some(SemanticColorRole::Faint);
    let layout = Arc::make_mut(&mut style.layout);
    layout.direction = Some(FlexDirection::Column);
    layout.align_items = AlignSpec::Stretch;
    layout.width = Some(LengthSpec::Fill);
    layout.overflow_x = OverflowSpec::Hidden;
    layout.overflow_y = OverflowSpec::Hidden;
    layout.gap = Some(LengthSpec::Px(SECTION_BODY_GAP));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    if empty_text.is_some() {
        layout.padding_top = Some(LengthSpec::Px(SECTION_EMPTY_PADDING_Y));
        layout.padding_bottom = Some(LengthSpec::Px(SECTION_EMPTY_PADDING_Y));
        layout.padding_left = Some(LengthSpec::Px(SECTION_EMPTY_PADDING_X));
        layout.padding_right = Some(LengthSpec::Px(SECTION_EMPTY_PADDING_X));
        layout.font_size = Some(SECTION_EMPTY_FONT_SIZE);
    }
    let clip_height = content_height * expansion.clamp(0.0, 1.0);
    // 收缩中的剪裁口同步淡出内容；稳态不写 opacity，绘制与既有样式逐位一致。
    if expansion < 1.0 {
        layout.opacity = Some(expansion.clamp(0.0, 1.0));
    }
    if expansion <= 0.0 {
        layout.height = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.max_height = Some(LengthSpec::Px(0.0));
    } else if expansion < 1.0 {
        layout.height = Some(LengthSpec::Px(clip_height));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.max_height = Some(LengthSpec::Px(clip_height));
    } else if empty_text.is_some() {
        layout.height = Some(LengthSpec::Px(SECTION_EMPTY_HEIGHT));
        layout.min_height = None;
        layout.max_height = None;
    } else {
        layout.height = None;
        layout.min_height = None;
        layout.max_height = None;
    }
    style
}

impl ComponentView for SidebarSection {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::activate_sidebar_section),
        slot_assembler: Some(AppContext::assemble_sidebar_section),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    /// Writes text, visual and style onto its body port, a `List` with its own projection.
    const ALWAYS_REPROJECT: bool = true;

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "sidebar-section".into(),
        }
    }

    fn wants_child_reproject() -> bool {
        true
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let root_text = if self.header.is_some() {
            ""
        } else {
            self.title.as_ref()
        };
        if world.text(id) != Some(root_text) {
            mutations.set_text(
                id,
                TextContent {
                    value: root_text.to_owned().into(),
                },
            );
        }
        if world.standard_visual(id).is_some() {
            mutations.set_standard_visual(id, None);
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: self.pointer_target(),
                focusable: self.toggleable(),
            },
            AccessibilityState {
                role: AccessibilityRole::Button,
                label: Some(Arc::clone(&self.title)),
                value: self.count.map(|count| Arc::from(count.to_string())),
                disabled: self.disabled,
                selected: self.collapsible.then_some(self.state.expanded()),
                ..AccessibilityState::default()
            },
        );
        self.project_header(world, mutations);
        self.project_disclosure(world, mutations);
        self.project_title_slot(world, mutations);
        self.project_count_slot(world, mutations);
        self.project_tools_visibility(world, mutations);
        self.project_body(world, mutations);
    }
}

/// Fixed footer row. Children stay application-owned IconButton actions.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarFooter {
    pub style: NodeStyle,
}

impl SidebarFooter {
    pub fn new() -> Self {
        Self {
            style: NodeStyle::default(),
        }
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(FlexDirection::Row);
        layout.align_items = AlignSpec::Center;
        layout.justify_content = JustifySpec::Start;
        layout.width = Some(LengthSpec::Fill);
        layout.gap = Some(layout.gap.unwrap_or(LengthSpec::Px(FOOTER_GAP)));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        layout.height = None;
        layout.min_height = None;
        style
    }
}

impl Default for SidebarFooter {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentView for SidebarFooter {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "sidebar-footer".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.text(id) != Some("") {
            mutations.set_text(
                id,
                TextContent {
                    value: String::new().into(),
                },
            );
        }
        if world.standard_visual(id).is_some() {
            mutations.set_standard_visual(id, None);
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                ..AccessibilityState::default()
            },
        );
    }
}

/// Footer icon action. Paints [`StandardVisual::Icon`] with tooltip.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarFooterButton {
    pub label: Arc<str>,
    pub icon: Icon,
    pub size: ControlSize,
    pub selected: bool,
    pub disabled: bool,
}

impl SidebarFooterButton {
    pub fn new(label: impl Into<Arc<str>>, icon: Icon) -> Self {
        Self {
            label: label.into(),
            icon,
            size: ControlSize::Small,
            selected: false,
            disabled: false,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn style(&self) -> NodeStyle {
        footer_button_style(self.size, self.selected)
    }
}

fn footer_button_style(size: ControlSize, selected: bool) -> NodeStyle {
    let mut style = NodeStyle::default();
    style.foreground = Some(if selected {
        SemanticColorRole::Text
    } else {
        SemanticColorRole::Muted
    });
    // Selected idle fill is invisible on the sidebar surface; keep selected
    // as a brighter glyph, not a Selected plate.
    style.background = None;
    style.text_horizontal_alignment = crate::TextHorizontalAlignment::Center;
    style.text_vertical_alignment = crate::TextVerticalAlignment::Center;
    style.interaction = InteractionStyle {
        selected: SemanticPaint {
            foreground: Some(SemanticColorRole::Text),
            ..SemanticPaint::default()
        },
        selected_hovered: SemanticPaint {
            foreground: Some(SemanticColorRole::Text),
            background: Some(SemanticColorRole::Hover),
            ..SemanticPaint::default()
        },
        selected_pressed: SemanticPaint {
            foreground: Some(SemanticColorRole::Text),
            background: Some(SemanticColorRole::Active),
            ..SemanticPaint::default()
        },
        hovered: SemanticPaint {
            foreground: Some(SemanticColorRole::Text),
            background: Some(SemanticColorRole::Hover),
            ..SemanticPaint::default()
        },
        pressed: SemanticPaint {
            foreground: Some(SemanticColorRole::Text),
            background: Some(SemanticColorRole::Active),
            ..SemanticPaint::default()
        },
        focused: SemanticPaint::FOCUS_SURFACE,
        disabled: SemanticPaint {
            foreground: Some(SemanticColorRole::Faint),
            ..SemanticPaint::default()
        },
        ..InteractionStyle::default()
    };
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.aspect_ratio = Some(1.0);
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        layout.padding_left = Some(LengthSpec::Px(0.0));
        layout.padding_right = Some(LengthSpec::Px(0.0));
        layout.padding_top = Some(LengthSpec::Px(0.0));
        layout.padding_bottom = Some(LengthSpec::Px(0.0));
    }
    style.radius = Some(nana_ui_core::RadiusTier::Sm);
    style.square = Some(nana_ui_core::SquareSize::Control(size));
    style.control_height = Some(nana_ui_core::ControlHeight::Exact(size));
    style
}

impl ComponentView for SidebarFooterButton {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::activate_sidebar_footer_button),
        ..crate::TypeBehavior::NONE
    };

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "sidebar-footer-button".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.overlay_host(id).is_none() {
            mutations.set_overlay_host(id, OverlayHostState::default());
        }
        if world.text(id) != Some("") {
            mutations.set_text(
                id,
                TextContent {
                    value: String::new().into(),
                },
            );
        }
        let visual = StandardVisual::Icon {
            icon: self.icon,
            size: self.size.icon_size(),
            tooltip: Some(TooltipVisual {
                label: Arc::clone(&self.label),
                config: TooltipConfig::default(),
                open: false,
            }),
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        project_common(
            id,
            world,
            mutations,
            &self.style(),
            InteractionState {
                pointer_events: !self.disabled,
                focusable: !self.disabled,
            },
            AccessibilityState {
                role: AccessibilityRole::Button,
                label: Some(Arc::clone(&self.label)),
                disabled: self.disabled,
                selected: Some(self.selected),
                ..AccessibilityState::default()
            },
        );
    }
}

impl AppContext {
    /// Give a section the chrome it has not been given and put its content
    /// in place: a header item holding the disclosure (when collapsible),
    /// the title, the count (when set) and the application's `tools`; a body
    /// port; and every other child of the section moved into the body, after
    /// the rows already there. The section's projection then keeps them all
    /// current. The view layer runs this after it builds a section and after
    /// a binding changes one (`TypeBehavior::slot_assembler`), so a view
    /// states only the section, its rows and its tools; a section built by
    /// hand with its own chrome needs no call. Returns whether anything
    /// changed.
    pub fn assemble_sidebar_section(
        &mut self,
        section: Entity<SidebarSection>,
    ) -> Result<bool, FrameworkError> {
        let id = section.stable_id();
        let document = self
            .world()
            .node(id)
            .ok_or(FrameworkError::MissingView(id))?
            .document;
        let spec = self.read(section, Clone::clone)?;
        let live =
            |cx: &Self, slot: Option<StableNodeId>| slot.filter(|id| cx.world().contains(*id));
        let mut created = false;
        let title = match live(self, spec.title_slot) {
            Some(title) => title,
            None => {
                created = true;
                self.create_detached_component(document, spec.title_label())?
                    .stable_id()
            }
        };
        let count = match live(self, spec.count_slot) {
            Some(count) => Some(count),
            None if spec.count.is_some() => {
                created = true;
                Some(
                    self.create_detached_component(document, spec.count_label())?
                        .stable_id(),
                )
            }
            None => None,
        };
        let disclosure = match live(self, spec.disclosure) {
            Some(disclosure) => Some(disclosure),
            None if spec.collapsible => {
                created = true;
                Some(
                    self.create_detached_component(document, spec.disclosure_mark())?
                        .stable_id(),
                )
            }
            None => None,
        };
        let header = match live(self, spec.header) {
            Some(header) => header,
            None => {
                created = true;
                let mut chrome = spec.clone().title_slot(title);
                chrome.count_slot = count;
                chrome.disclosure = disclosure;
                self.create_detached_component(document, chrome.header_item())?
                    .stable_id()
            }
        };
        let body = match live(self, spec.body) {
            Some(body) => body,
            None => {
                created = true;
                self.create_detached_component(document, SidebarSection::body_port())?
                    .stable_id()
            }
        };
        if created {
            self.update_component(section, |section, _| {
                section.title_slot = Some(title);
                section.count_slot = count;
                section.disclosure = disclosure;
                section.header = Some(header);
                section.body = Some(body);
            })?;
        }
        let mut changed = created;
        let chrome: Vec<StableNodeId> = disclosure
            .into_iter()
            .chain([title])
            .chain(count)
            .chain(live(self, spec.tools))
            .collect();
        changed |= self.reconcile_children(header, &chrome)?;
        let content: Vec<StableNodeId> = self
            .world()
            .node(id)
            .map(|node| node.children)
            .unwrap_or_default()
            .into_iter()
            .filter(|child| *child != header && *child != body)
            .collect();
        if !content.is_empty() {
            let mut rows = self
                .world()
                .node(body)
                .map(|node| node.children)
                .unwrap_or_default();
            rows.extend(content);
            changed |= self.reconcile_children(body, &rows)?;
        }
        changed |= self.reconcile_children(id, &[header, body])?;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::{AppContext, DocumentId, Entity, IconButton, List, ListItem, StandardVisual, Text};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn icon_of(visual: Option<StandardVisual>) -> Option<Icon> {
        match visual {
            Some(StandardVisual::Icon { icon, .. }) => Some(icon),
            _ => None,
        }
    }

    #[test]
    fn section_collapse_progress_comes_from_motion_ir() {
        let mut context = AppContext::new();
        let (section, _, _, _, _) = mount_section(
            &mut context,
            SidebarSection::new("资源").collapsible(true).expanded(true),
            &["条目"],
            None,
        );
        assert_eq!(
            context
                .read(section, |section| section.animation_progress)
                .unwrap(),
            1.0
        );
        assert!(context.activate_sidebar_section(section).unwrap());
        let id = section.stable_id();
        let animation =
            crate::component_animation_id(crate::component_animation_kinds::SIDEBAR, id)
                .expect("sidebar animation id");
        assert!(context.world().animation_is_active(animation));
        let _ = context.advance_animations(SECTION_ANIMATION_DURATION / 4);
        let quarter = context
            .read(section, |section| section.animation_progress)
            .unwrap();
        assert!((quarter - 0.9375).abs() < 1e-5);
        let reversed_at = SECTION_ANIMATION_DURATION / 2;
        let _ = context.advance_animations(reversed_at);
        let middle = context
            .read(section, |section| section.animation_progress)
            .unwrap();
        assert!((middle - 0.5).abs() < 1e-5);
        assert!(context.activate_sidebar_section(section).unwrap());
        assert_eq!(
            context
                .read(section, |section| section.animation_progress)
                .unwrap(),
            middle
        );
        let _ = context.advance_animations(reversed_at + SECTION_ANIMATION_DURATION);
        assert_eq!(
            context
                .read(section, |section| section.animation_progress)
                .unwrap(),
            1.0
        );
        assert!(!context.world().animation_is_active(animation));
    }

    #[test]
    fn section_body_fades_content_while_expansion_is_mid_flight() {
        let mut context = AppContext::new();
        let (section, _, body, _, _) = mount_section(
            &mut context,
            SidebarSection::new("资源").collapsible(true).expanded(true),
            &["条目"],
            None,
        );
        let body_id = body.stable_id();
        assert_eq!(
            context.world().node_style(body_id).unwrap().layout.opacity,
            None,
            "fully expanded body must stay opacity-free"
        );
        assert!(context.activate_sidebar_section(section).unwrap());
        let _ = context.advance_animations(Duration::from_millis(80));
        let progress = context
            .read(section, |section| section.animation_progress)
            .unwrap();
        assert!(progress > 0.0 && progress < 1.0);
        assert_eq!(
            context.world().node_style(body_id).unwrap().layout.opacity,
            Some(progress),
            "mid-collapse body opacity must follow the sampled expansion"
        );
        let _ = context.advance_animations(SECTION_ANIMATION_DURATION);
        let closed = context.world().node_style(body_id).unwrap();
        assert_eq!(closed.layout.opacity, Some(0.0));
        assert_eq!(closed.layout.height, Some(LengthSpec::Px(0.0)));
    }

    #[test]
    fn tree_depth_aligns_the_first_child_with_a_leading_row_label() {
        assert_eq!(sidebar_row_depth_inset(0), 8.0);
        assert_eq!(sidebar_row_depth_inset(1), 30.0);
        assert_eq!(sidebar_row_depth_inset(2), 42.0);
    }

    #[test]
    fn sidebar_tools_land_on_one_glyph_column_after_layout() {
        use crate::Stack;

        let mut context = AppContext::new();
        let doc = document();
        let column = context.create_component(doc, Stack::column(6.0)).unwrap();

        // Top bar replica: unpadded full-width row, filler, then the 28px tool.
        let bar = context.create_component(doc, Stack::bar(6.0)).unwrap();
        let search = context
            .create_component(doc, sidebar_top_bar_tool_button(Icon::Search, "搜索"))
            .unwrap();
        let bar_filler = context.create_component(doc, Stack::fill_row(0.0)).unwrap();
        context.append_child(column, bar).unwrap();
        context.append_child(bar, bar_filler).unwrap();
        context.append_child(bar, search).unwrap();

        // Section header replica: full-width row padded 8 both sides, fill
        // title, then the 20px inline tool.
        let header = context
            .create_component(doc, SidebarSection::new("项目").header_item())
            .unwrap();
        let add = context
            .create_component(doc, sidebar_section_tool_button(Icon::Add, "添加"))
            .unwrap();
        let header_filler = context.create_component(doc, Stack::fill_row(0.0)).unwrap();
        context.append_child(column, header).unwrap();
        context.append_child(header, header_filler).unwrap();
        context.append_child(header, add).unwrap();

        context
            .layout_document(doc, crate::LayoutViewport::new(220.0, 120.0))
            .unwrap();
        let search_box = context.world().layout_box(search.stable_id()).unwrap();
        let add_box = context.world().layout_box(add.stable_id()).unwrap();
        let search_center = search_box.x + search_box.width / 2.0;
        let add_center = add_box.x + add_box.width / 2.0;
        assert_eq!(search_box.width, UI_METRICS.icon_button_size);
        assert_eq!(add_box.width, SECTION_TOOL_EDGE);
        assert_eq!(
            search_center, add_center,
            "sidebar tool glyphs must share one column"
        );
    }

    #[test]
    fn frame_is_layout_only_with_fixed_outer_slots() {
        let mut context = AppContext::new();
        let top = context
            .create_component(document(), SidebarRow::new("返回"))
            .unwrap();
        let body = context
            .create_component(document(), SidebarFrame::vertical_body_scroll())
            .unwrap();
        let footer = context
            .create_component(document(), SidebarFooter::new())
            .unwrap();
        let frame = context
            .create_component(
                document(),
                SidebarFrame::new()
                    .top(top.stable_id())
                    .body(body.stable_id())
                    .footer(footer.stable_id()),
            )
            .unwrap();
        let id = frame.stable_id();
        assert_eq!(
            context.world().node(id).unwrap().kind,
            NodeKind::Element {
                tag: "sidebar-frame".into(),
            }
        );
        assert_eq!(context.world().standard_visual(id), None);
        let layout = &context.world().node_style(id).unwrap().layout;
        assert_eq!(layout.direction, Some(FlexDirection::Column));
        assert_eq!(layout.gap, Some(LengthSpec::Px(FRAME_GAP)));
        assert_eq!(
            layout.padding_left,
            Some(LengthSpec::Px(FRAME_PADDING_LEFT))
        );
        assert!(!context.world().interaction(id).unwrap().focusable);
        let body_layout = &context.world().node_style(body.stable_id()).unwrap().layout;
        assert_eq!(body_layout.overflow_y, OverflowSpec::Scroll);
        assert_eq!(
            context.world().node(body.stable_id()).unwrap().kind,
            NodeKind::Element {
                tag: "scroll".into(),
            }
        );
        assert!(
            !context
                .world()
                .node_style(top.stable_id())
                .unwrap()
                .layout
                .overflow_y
                .scrolls()
        );
        assert!(
            !context
                .world()
                .node_style(footer.stable_id())
                .unwrap()
                .layout
                .overflow_y
                .scrolls()
        );
    }

    #[test]
    fn frame_drops_css_layout_background() {
        let mut context = AppContext::new();
        let mut layout = nana_ui_core::LayoutStyle::default();
        layout.background = Some([1.0, 0.0, 0.0, 1.0]);
        let frame = context
            .create_component(
                document(),
                SidebarFrame::new().style(NodeStyle {
                    layout: Arc::new(layout),
                    ..NodeStyle::default()
                }),
            )
            .unwrap();
        assert!(
            context
                .world()
                .node_style(frame.stable_id())
                .unwrap()
                .layout
                .background
                .is_none(),
            "CSS --bg-elev must not cover the parent Surface region"
        );
    }

    #[test]
    fn frame_body_scrolls_without_moving_top_or_footer() {
        let mut context = AppContext::new();
        let top = context
            .create_component(document(), SidebarRow::new("返回"))
            .unwrap();
        let body = context
            .create_component(document(), SidebarFrame::vertical_body_scroll())
            .unwrap();
        let mut rows = Vec::new();
        for label in [
            "外观",
            "工作区",
            "资源",
            "设置",
            "关于",
            "日志",
            "调试",
            "扩展",
        ] {
            let row = context
                .create_component(document(), SidebarRow::new(label))
                .unwrap();
            context.append_child(body, row).unwrap();
            rows.push(row);
        }
        let footer = context
            .create_component(document(), SidebarFooter::new())
            .unwrap();
        let frame = context
            .create_component(
                document(),
                SidebarFrame::new()
                    .top(top.stable_id())
                    .body(body.stable_id())
                    .footer(footer.stable_id()),
            )
            .unwrap();
        context.append_child(frame, top).unwrap();
        context.append_child(frame, body).unwrap();
        context.append_child(frame, footer).unwrap();

        context
            .layout_document(document(), crate::LayoutViewport::new(220.0, 160.0))
            .unwrap();
        let top_before = context.world().layout_box(top.stable_id()).unwrap();
        let body_box = context.world().layout_box(body.stable_id()).unwrap();
        let footer_before = context.world().layout_box(footer.stable_id()).unwrap();
        let content = rows[0];
        let content_before = context.world().layout_box(content.stable_id()).unwrap();
        assert!(top_before.y < body_box.y);
        assert!(footer_before.y >= body_box.y + body_box.height - 0.5);
        assert!(
            context
                .world()
                .node_style(body.stable_id())
                .unwrap()
                .layout
                .overflow_y
                .scrolls()
        );

        context
            .set_scroll_metrics(
                body,
                crate::ScrollMetrics {
                    viewport_width: body_box.width,
                    viewport_height: body_box.height,
                    content_width: body_box.width,
                    content_height: body_box.height + 80.0,
                    origin_x: 0.0,
                    origin_y: 0.0,
                },
            )
            .unwrap();
        assert!(
            context
                .scroll_to(body, crate::ScrollOffset { x: 0.0, y: 40.0 })
                .unwrap()
        );
        assert_eq!(
            context.world().scroll_offset(body.stable_id()),
            Some(crate::ScrollOffset { x: 0.0, y: 40.0 })
        );
        assert_eq!(
            context.world().layout_box(top.stable_id()).unwrap(),
            top_before
        );
        assert_eq!(
            context.world().layout_box(footer.stable_id()).unwrap(),
            footer_before
        );
        assert_eq!(
            context.world().layout_box(content.stable_id()).unwrap(),
            content_before
        );

        context.rebuild_hit_test(document());
        assert_eq!(
            context
                .world()
                .hit_test(document(), top_before.x + 4.0, top_before.y + 4.0),
            Some(top.stable_id())
        );
        let footer_hit =
            context
                .world()
                .hit_test(document(), footer_before.x + 4.0, footer_before.y + 4.0);
        assert_ne!(footer_hit, Some(content.stable_id()));
        assert_ne!(footer_hit, Some(body.stable_id()));
        let scrolled_y = content_before.y - 40.0 + 4.0;
        if scrolled_y >= body_box.y && scrolled_y < body_box.y + body_box.height {
            assert_eq!(
                context
                    .world()
                    .hit_test(document(), content_before.x + 4.0, scrolled_y),
                Some(content.stable_id())
            );
        }
        assert_ne!(
            context
                .world()
                .hit_test(document(), content_before.x + 4.0, content_before.y + 4.0),
            Some(content.stable_id())
        );
    }

    #[test]
    fn row_select_and_disabled_project_list_item_slots() {
        let mut context = AppContext::new();
        let leading = context
            .create_component(document(), SidebarRowIcon::new(Icon::Workspace))
            .unwrap();
        let selected = context
            .create_component(
                document(),
                SidebarRow::new("工作区")
                    .state(SidebarRowState::Active)
                    .slots(ListItemSlots {
                        leading: Some(leading.stable_id()),
                        content: None,
                        trailing: None,
                    })
                    .depth(1)
                    .tone(SidebarRowTone::Warning),
            )
            .unwrap();
        context.append_child(selected, leading).unwrap();
        let id = selected.stable_id();
        assert_eq!(
            context.world().node(id).unwrap().kind,
            NodeKind::Element {
                tag: "sidebar-row".into(),
            }
        );
        assert_eq!(
            context.world().standard_visual(id),
            Some(StandardVisual::ListItem {
                leading: Some(leading.stable_id()),
                content: None,
                trailing: None,
                detail: None,
            })
        );
        assert_eq!(context.world().text(id), Some("工作区"));
        let style = context.world().node_style(id).unwrap();
        assert_eq!(style.foreground, Some(SemanticColorRole::Warning));
        assert_eq!(style.background, None);
        assert_eq!(
            style.interaction.selected.background,
            Some(SemanticColorRole::Selected)
        );
        assert_eq!(
            style.layout.padding_left,
            Some(LengthSpec::Px(sidebar_row_depth_inset(1)))
        );
        assert_eq!(style.layout.font_size, Some(ControlSize::Small.text_size()));
        assert_eq!(
            style.layout.line_height,
            Some(LineHeightSpec::Absolute(ControlSize::Small.line_height()))
        );
        assert_eq!(
            context.world().standard_visual(leading.stable_id()),
            Some(StandardVisual::Icon {
                icon: Icon::Workspace,
                size: ROW_ICON_SIZE,
                tooltip: None,
            })
        );
        let icon_style = context.world().node_style(leading.stable_id()).unwrap();
        let icon_layout = &icon_style.layout;
        assert_eq!(icon_style.foreground, None);
        assert_eq!(icon_layout.width, Some(LengthSpec::Px(ROW_ICON_SIZE)));
        assert_eq!(icon_layout.height, Some(LengthSpec::Px(ROW_ICON_SIZE)));
        assert!(context.world().interaction(id).unwrap().focusable);
        assert_eq!(
            context.world().accessibility(id).unwrap().selected,
            Some(true)
        );

        let disabled = context
            .create_component(
                document(),
                SidebarRow::new("停用").state(SidebarRowState::Disabled),
            )
            .unwrap();
        let disabled_id = disabled.stable_id();
        assert!(
            !context
                .world()
                .interaction(disabled_id)
                .unwrap()
                .pointer_events
        );
        assert!(!context.world().interaction(disabled_id).unwrap().focusable);
        assert!(context.world().accessibility(disabled_id).unwrap().disabled);
        assert!(!context.activate_sidebar_row(disabled).unwrap());
        assert!(context.activate_sidebar_row(selected).unwrap());
    }

    #[test]
    fn row_tools_project_as_trailing_list_item_slot() {
        let mut context = AppContext::new();
        let close = context
            .create_component(
                document(),
                IconButton::new(Icon::Close, "关闭").size(ControlSize::Small),
            )
            .unwrap();
        let leading = context
            .create_component(document(), SidebarRowIcon::new(Icon::File))
            .unwrap();
        let row = context
            .create_component(
                document(),
                SidebarRow::new("着色器")
                    .slots(ListItemSlots {
                        leading: Some(leading.stable_id()),
                        content: None,
                        trailing: None,
                    })
                    .tools(close.stable_id()),
            )
            .unwrap();
        context.append_child(row, leading).unwrap();
        context.append_child(row, close).unwrap();
        assert_eq!(
            context.world().standard_visual(row.stable_id()),
            Some(StandardVisual::ListItem {
                leading: Some(leading.stable_id()),
                content: None,
                trailing: None,
                detail: None,
            }),
            "row tools stay out of the trailing slot until the row is hovered"
        );
        assert!(
            context
                .world()
                .node_style(close.stable_id())
                .unwrap()
                .layout
                .hidden
        );

        context
            .set_pointer_hover(document(), 1, Some(row.stable_id()))
            .unwrap();
        assert_eq!(
            context.world().standard_visual(row.stable_id()),
            Some(StandardVisual::ListItem {
                leading: Some(leading.stable_id()),
                content: None,
                trailing: Some(close.stable_id()),
                detail: None,
            })
        );
        let tools_layout = &context
            .world()
            .node_style(close.stable_id())
            .unwrap()
            .layout;
        assert_eq!(tools_layout.flex_grow, Some(0.0));
        assert_eq!(tools_layout.flex_shrink, Some(0.0));
        assert_eq!(tools_layout.width, Some(LengthSpec::Shrink));
        assert_eq!(
            tools_layout.margin_right,
            Some(LengthSpec::Px(tool_column_trailing_margin(UI_METRICS)))
        );
        context
            .layout_document(document(), crate::LayoutViewport::new(220.0, 80.0))
            .unwrap();
        let row_box = context.world().layout_box(row.stable_id()).unwrap();
        let tools_box = context.world().layout_box(close.stable_id()).unwrap();
        assert!(tools_box.x > row_box.x);
        assert!(tools_box.x + tools_box.width <= row_box.x + row_box.width + 0.5);
        let Some(crate::ComponentGeometry::ListItem {
            content: Some(label_box),
            ..
        }) = context.world().component_geometry(row.stable_id())
        else {
            panic!("row should keep a label content box");
        };
        assert!(
            label_box.width > 48.0,
            "tools must not collapse the row label, got {}",
            label_box.width
        );
        assert!(label_box.x + label_box.width <= tools_box.x + 0.5);
        assert_eq!(context.world().text(row.stable_id()), Some("着色器"));

        context.set_pointer_hover(document(), 1, None).unwrap();
        assert!(
            context
                .world()
                .node_style(close.stable_id())
                .unwrap()
                .layout
                .hidden
        );
    }

    #[test]
    fn row_tool_clusters_keep_host_gap_on_one_glyph_column() {
        use crate::Stack;

        let mut context = AppContext::new();
        let doc = document();
        let cluster = context.create_component(doc, Stack::row(2.0)).unwrap();
        let draft = context
            .create_component(doc, sidebar_row_tool_button(Icon::Add, "新对话"))
            .unwrap();
        let menu = context
            .create_component(doc, sidebar_row_tool_button(Icon::More, "更多"))
            .unwrap();
        let single = context
            .create_component(doc, sidebar_row_tool_button(Icon::More, "更多"))
            .unwrap();
        let clustered = context
            .create_component(doc, SidebarRow::new("项目").tools(cluster.stable_id()))
            .unwrap();
        let lone = context
            .create_component(doc, SidebarRow::new("任务").tools(single.stable_id()))
            .unwrap();
        context.append_child(clustered, cluster).unwrap();
        context.append_child(cluster, draft).unwrap();
        context.append_child(cluster, menu).unwrap();
        context.append_child(lone, single).unwrap();

        context
            .set_pointer_hover(document(), 1, Some(clustered.stable_id()))
            .unwrap();
        context
            .layout_document(doc, crate::LayoutViewport::new(220.0, 120.0))
            .unwrap();
        let row_box = context.world().layout_box(clustered.stable_id()).unwrap();
        let draft_box = context.world().layout_box(draft.stable_id()).unwrap();
        let menu_box = context.world().layout_box(menu.stable_id()).unwrap();
        assert_eq!(
            (menu_box.x - draft_box.x - draft_box.width).round(),
            2.0,
            "cluster buttons keep the tools host gap between their boxes"
        );
        let trailing_center = menu_box.x + menu_box.width / 2.0;
        assert!(
            (trailing_center - (row_box.x + row_box.width - tool_column_center_inset(UI_METRICS)))
                .abs()
                <= 0.5,
            "trailing cluster tool lands on the shared glyph column"
        );

        context
            .set_pointer_hover(document(), 1, Some(lone.stable_id()))
            .unwrap();
        context
            .layout_document(doc, crate::LayoutViewport::new(220.0, 120.0))
            .unwrap();
        let lone_row_box = context.world().layout_box(lone.stable_id()).unwrap();
        let single_box = context.world().layout_box(single.stable_id()).unwrap();
        let single_center = single_box.x + single_box.width / 2.0;
        assert!(
            (single_center
                - (lone_row_box.x + lone_row_box.width - tool_column_center_inset(UI_METRICS)))
            .abs()
                <= 0.5,
            "a single row tool still lands on the shared glyph column"
        );
    }

    #[test]
    fn row_default_text_uses_text_role() {
        let mut context = AppContext::new();
        let idle = context
            .create_component(document(), SidebarRow::new("外观"))
            .unwrap();
        let active = context
            .create_component(
                document(),
                SidebarRow::new("工作区").state(SidebarRowState::Active),
            )
            .unwrap();
        let ancestor = context
            .create_component(
                document(),
                SidebarRow::new("项目").state(SidebarRowState::AncestorActive),
            )
            .unwrap();
        let disabled = context
            .create_component(
                document(),
                SidebarRow::new("停用").state(SidebarRowState::Disabled),
            )
            .unwrap();
        let idle_style = context.world().node_style(idle.stable_id()).unwrap();
        let active_style = context.world().node_style(active.stable_id()).unwrap();
        let ancestor_style = context.world().node_style(ancestor.stable_id()).unwrap();
        let disabled_style = context.world().node_style(disabled.stable_id()).unwrap();
        assert_eq!(idle_style.foreground, Some(SemanticColorRole::Text));
        assert_eq!(active_style.foreground, Some(SemanticColorRole::Text));
        assert_eq!(ancestor_style.foreground, Some(SemanticColorRole::Text));
        assert_eq!(disabled_style.foreground, Some(SemanticColorRole::Faint));
        assert_eq!(idle_style.layout.color, None);
        assert_eq!(active_style.layout.color, None);
        assert_eq!(ancestor_style.layout.color, None);
        assert_eq!(
            active_style.interaction.selected.background,
            Some(SemanticColorRole::Selected)
        );
    }

    fn mount_section(
        context: &mut AppContext,
        spec: SidebarSection,
        rows: &[&str],
        tools: Option<Entity<SidebarFooterButton>>,
    ) -> (
        Entity<SidebarSection>,
        Entity<ListItem>,
        Entity<List>,
        Entity<IconButton>,
        Entity<Text>,
    ) {
        let disclosure = context
            .create_component(document(), spec.disclosure_mark())
            .unwrap();
        let title = context
            .create_component(document(), spec.title_label())
            .unwrap();
        let count = context
            .create_component(document(), spec.count_label())
            .unwrap();
        let spec = spec
            .disclosure(disclosure.stable_id())
            .title_slot(title.stable_id())
            .count_slot(count.stable_id());
        let spec = match tools {
            Some(tools) => spec.tools(tools.stable_id()),
            None => spec,
        };
        let header = context
            .create_component(document(), spec.header_item())
            .unwrap();
        let body = context
            .create_component(document(), SidebarSection::body_port())
            .unwrap();
        for label in rows {
            let row = context
                .create_component(document(), SidebarRow::new(*label))
                .unwrap();
            context.append_child(body, row).unwrap();
        }
        context.append_child(header, disclosure).unwrap();
        context.append_child(header, title).unwrap();
        context.append_child(header, count).unwrap();
        if let Some(tools) = tools {
            context.append_child(header, tools).unwrap();
        }
        let section = context
            .create_component(
                document(),
                spec.header(header.stable_id()).body(body.stable_id()),
            )
            .unwrap();
        context.append_child(section, header).unwrap();
        context.append_child(section, body).unwrap();
        (section, header, body, disclosure, count)
    }

    #[test]
    fn expanded_section_body_lays_out_child_rows() {
        let mut context = AppContext::new();
        let (_, _, body, _, _) = mount_section(
            &mut context,
            SidebarSection::new("表演"),
            &["待机", "动作"],
            None,
        );
        context.reproject_component(body).unwrap();
        context
            .layout_document(document(), crate::LayoutViewport::new(220.0, 320.0))
            .unwrap();
        let gallery_body = context.world().layout_box(body.stable_id()).unwrap();
        let row_height = ControlSize::Small.height_in(UI_METRICS);
        assert!(gallery_body.height >= row_height * 2.0);
        context
            .read(body, |list| {
                assert_eq!(list.style.layout.height, None);
                assert_eq!(list.style.layout.min_height, None);
                assert_eq!(list.style.layout.max_height, None);
            })
            .unwrap();

        let spec = SidebarSection::new("资源");
        let title = context
            .create_component(document(), spec.title_label())
            .unwrap();
        let spec = spec.title_slot(title.stable_id());
        let header = context
            .create_component(document(), spec.header_item())
            .unwrap();
        context.append_child(header, title).unwrap();
        let late_body = context
            .create_component(document(), SidebarSection::body_port())
            .unwrap();
        let late_section = context
            .create_component(
                document(),
                spec.header(header.stable_id()).body(late_body.stable_id()),
            )
            .unwrap();
        context.append_child(late_section, header).unwrap();
        context.append_child(late_section, late_body).unwrap();
        for label in ["模型", "动作"] {
            let row = context
                .create_component(document(), SidebarRow::new(label))
                .unwrap();
            context.append_child(late_body, row).unwrap();
        }
        context.reproject_component(late_body).unwrap();
        context
            .layout_document(document(), crate::LayoutViewport::new(220.0, 320.0))
            .unwrap();
        let late_box = context.world().layout_box(late_body.stable_id()).unwrap();
        assert!(late_box.height >= row_height * 2.0);
    }

    #[test]
    fn section_header_toggles_when_collapsible() {
        let mut context = AppContext::new();
        let (section, header, body, disclosure, _) = mount_section(
            &mut context,
            SidebarSection::new("资源")
                .count(3)
                .empty_text("暂无")
                .collapsible(true)
                .expanded(true),
            &[],
            None,
        );
        let id = section.stable_id();
        assert_eq!(context.world().text(id), Some(""));
        assert_eq!(context.world().text(header.stable_id()), Some(""));
        let title_id = context
            .read(section, |section| section.title_slot)
            .unwrap()
            .expect("title slot");
        assert_eq!(context.world().text(title_id), Some("资源"));
        assert_eq!(context.world().text(body.stable_id()), Some("暂无"));
        assert_eq!(
            context
                .world()
                .node_style(body.stable_id())
                .unwrap()
                .layout
                .height,
            Some(LengthSpec::Px(SECTION_EMPTY_HEIGHT))
        );
        assert_eq!(
            context.world().standard_visual(disclosure.stable_id()),
            Some(StandardVisual::Icon {
                icon: Icon::ChevronDown,
                size: SECTION_DISCLOSURE_SIZE,
                tooltip: None,
            })
        );
        assert!(
            context
                .world()
                .node_style(disclosure.stable_id())
                .unwrap()
                .layout
                .transform
                .is_none()
        );
        assert_eq!(
            context.world().accessibility(id).unwrap().selected,
            Some(true)
        );
        assert!(context.world().interaction(id).unwrap().focusable);
        assert!(context.activate_sidebar_section(section).unwrap());
        context
            .read(section, |section| {
                assert!(!section.state.expanded());
            })
            .unwrap();
        let animation = crate::component_animation_id(
            crate::component_animation_kinds::SIDEBAR,
            section.stable_id(),
        )
        .expect("sidebar animation id");
        assert!(context.world().animation_is_active(animation));
        let _ = context.advance_animations(std::time::Duration::from_millis(80));
        context
            .read(section, |section| {
                assert!(section.animation_progress > 0.0 && section.animation_progress < 1.0);
            })
            .unwrap();
        assert_eq!(
            context.world().accessibility(id).unwrap().selected,
            Some(false)
        );

        let locked = context
            .create_component(
                document(),
                SidebarSection::new("锁定").collapsible(true).disabled(true),
            )
            .unwrap();
        assert!(
            !context
                .world()
                .interaction(locked.stable_id())
                .unwrap()
                .focusable
        );
        assert!(!context.activate_sidebar_section(locked).unwrap());

        let static_section = context
            .create_component(document(), SidebarSection::new("固定"))
            .unwrap();
        assert!(
            !context
                .world()
                .interaction(static_section.stable_id())
                .unwrap()
                .focusable
        );
        assert!(!context.activate_sidebar_section(static_section).unwrap());
        assert!(
            context
                .world()
                .node_style(static_section.stable_id())
                .unwrap()
                .interaction
                .hovered
                .is_empty()
        );
    }

    #[test]
    fn section_disclosure_uses_static_chevrons() {
        assert_eq!(disclosure_icon_kind(0.0), Icon::ChevronRight);
        assert_eq!(disclosure_icon_kind(0.49), Icon::ChevronRight);
        assert_eq!(disclosure_icon_kind(0.5), Icon::ChevronDown);
        assert_eq!(disclosure_icon_kind(1.0), Icon::ChevronDown);

        let mut context = AppContext::new();
        let (_, _, _, expanded, _) = mount_section(
            &mut context,
            SidebarSection::new("展开").collapsible(true).expanded(true),
            &[],
            None,
        );
        assert_eq!(
            context.world().standard_visual(expanded.stable_id()),
            Some(StandardVisual::Icon {
                icon: Icon::ChevronDown,
                size: SECTION_DISCLOSURE_SIZE,
                tooltip: None,
            })
        );
        assert!(
            context
                .world()
                .node_style(expanded.stable_id())
                .unwrap()
                .layout
                .transform
                .is_none()
        );

        let (_, _, _, collapsed, _) = mount_section(
            &mut context,
            SidebarSection::new("折叠")
                .collapsible(true)
                .expanded(false),
            &[],
            None,
        );
        assert_eq!(
            context.world().standard_visual(collapsed.stable_id()),
            Some(StandardVisual::Icon {
                icon: Icon::ChevronRight,
                size: SECTION_DISCLOSURE_SIZE,
                tooltip: None,
            })
        );
        assert!(
            context
                .world()
                .node_style(collapsed.stable_id())
                .unwrap()
                .layout
                .transform
                .is_none()
        );

        let (section, _, _, disclosure, _) = mount_section(
            &mut context,
            SidebarSection::new("动画").collapsible(true).expanded(true),
            &[],
            None,
        );
        assert!(context.activate_sidebar_section(section).unwrap());
        let _ = context.advance_animations(std::time::Duration::from_millis(200));
        assert_eq!(
            context.world().standard_visual(disclosure.stable_id()),
            Some(StandardVisual::Icon {
                icon: Icon::ChevronRight,
                size: SECTION_DISCLOSURE_SIZE,
                tooltip: None,
            })
        );
    }

    #[test]
    fn section_animation_clips_body_without_changing_frame_contract() {
        let mut context = AppContext::new();
        let top = context
            .create_component(document(), SidebarRow::new("返回"))
            .unwrap();
        let frame_body = context
            .create_component(document(), SidebarFrame::vertical_body_scroll())
            .unwrap();
        let (section, _, body, _, _) = mount_section(
            &mut context,
            SidebarSection::new("资源").collapsible(true).expanded(true),
            &["外观", "工作区"],
            None,
        );
        context.append_child(frame_body, section).unwrap();
        let footer = context
            .create_component(document(), SidebarFooter::new())
            .unwrap();
        let frame = context
            .create_component(
                document(),
                SidebarFrame::new()
                    .top(top.stable_id())
                    .body(frame_body.stable_id())
                    .footer(footer.stable_id()),
            )
            .unwrap();
        context.append_child(frame, top).unwrap();
        context.append_child(frame, frame_body).unwrap();
        context.append_child(frame, footer).unwrap();

        context
            .layout_document(document(), crate::LayoutViewport::new(220.0, 320.0))
            .unwrap();
        let frame_layout_before = context
            .world()
            .node_style(frame.stable_id())
            .unwrap()
            .layout
            .clone();
        let expanded_body = context.world().layout_box(body.stable_id()).unwrap();
        assert!(expanded_body.height > 0.0);
        assert_eq!(
            context
                .world()
                .node_style(body.stable_id())
                .unwrap()
                .layout
                .overflow_y,
            OverflowSpec::Hidden
        );

        assert!(context.activate_sidebar_section(section).unwrap());
        let _ = context.advance_animations(std::time::Duration::from_millis(80));
        context
            .layout_document(document(), crate::LayoutViewport::new(220.0, 320.0))
            .unwrap();
        let progress = context
            .read(section, |section| section.animation_progress)
            .unwrap();
        let clipped = context.world().layout_box(body.stable_id()).unwrap();
        assert!(progress > 0.0 && progress < 1.0);
        assert!(clipped.height < expanded_body.height);
        assert!((clipped.height - expanded_body.height * progress).abs() < 1.0);
        assert_eq!(
            context
                .world()
                .node_style(body.stable_id())
                .unwrap()
                .layout
                .overflow_y,
            OverflowSpec::Hidden
        );
        let frame_layout_after = context
            .world()
            .node_style(frame.stable_id())
            .unwrap()
            .layout
            .clone();
        assert_eq!(
            frame_layout_before.padding_top,
            frame_layout_after.padding_top
        );
        assert_eq!(
            frame_layout_before.padding_right,
            frame_layout_after.padding_right
        );
        assert_eq!(
            frame_layout_before.padding_bottom,
            frame_layout_after.padding_bottom
        );
        assert_eq!(
            frame_layout_before.padding_left,
            frame_layout_after.padding_left
        );
        assert_eq!(frame_layout_before.gap, frame_layout_after.gap);
        assert_eq!(
            frame_layout_after.padding_left,
            Some(LengthSpec::Px(FRAME_PADDING_LEFT))
        );
        assert_eq!(frame_layout_after.gap, Some(LengthSpec::Px(FRAME_GAP)));
    }

    #[test]
    fn section_count_and_tools_keep_slot_identities() {
        let mut context = AppContext::new();
        let tools = context
            .create_component(document(), SidebarFooterButton::new("刷新", Icon::Restore))
            .unwrap();
        let tools_visual = context.world().standard_visual(tools.stable_id());
        let (section, header, _, _, count) = mount_section(
            &mut context,
            SidebarSection::new("资源")
                .count(3)
                .collapsible(true)
                .expanded(true),
            &[],
            Some(tools),
        );
        let count_id = count.stable_id();
        let tools_id = tools.stable_id();
        assert_eq!(context.world().text(count_id), Some("3"));
        assert!(!context.world().node_style(count_id).unwrap().layout.hidden);
        assert!(context.world().node_style(tools_id).unwrap().layout.hidden);
        assert_eq!(
            context.world().standard_visual(header.stable_id()),
            Some(StandardVisual::ListItem {
                leading: context.read(section, |section| section.disclosure).unwrap(),
                content: context.read(section, |section| section.title_slot).unwrap(),
                trailing: Some(count_id),
                detail: None,
            })
        );

        context
            .set_pointer_hover(document(), 1, Some(section.stable_id()))
            .unwrap();
        assert!(context.world().node_style(count_id).unwrap().layout.hidden);
        assert!(!context.world().node_style(tools_id).unwrap().layout.hidden);
        assert_eq!(
            context.world().standard_visual(header.stable_id()),
            Some(StandardVisual::ListItem {
                leading: context.read(section, |section| section.disclosure).unwrap(),
                content: context.read(section, |section| section.title_slot).unwrap(),
                trailing: Some(tools_id),
                detail: None,
            })
        );
        assert_eq!(context.world().standard_visual(tools_id), tools_visual);
        assert_eq!(
            context.read(section, |section| section.slot_ids()).unwrap(),
            SidebarSectionSlots {
                header: Some(header.stable_id()),
                body: context.read(section, |section| section.body).unwrap(),
                tools: Some(tools_id),
            }
        );

        context.set_pointer_hover(document(), 1, None).unwrap();
        assert!(!context.world().node_style(count_id).unwrap().layout.hidden);
        assert!(context.world().node_style(tools_id).unwrap().layout.hidden);
        assert_eq!(count.stable_id(), count_id);
        assert_eq!(tools.stable_id(), tools_id);
    }

    #[test]
    fn footer_hugs_icon_button_children() {
        let mut context = AppContext::new();
        let footer = context
            .create_component(document(), SidebarFooter::new())
            .unwrap();
        let settings = context
            .create_component(document(), SidebarFooterButton::new("设置", Icon::Settings))
            .unwrap();
        let search = context
            .create_component(document(), SidebarFooterButton::new("搜索", Icon::Search))
            .unwrap();
        context.append_child(footer, settings).unwrap();
        context.append_child(footer, search).unwrap();
        let layout = &context
            .world()
            .node_style(footer.stable_id())
            .unwrap()
            .layout;
        assert_eq!(layout.direction, Some(FlexDirection::Row));
        assert_eq!(layout.align_items, AlignSpec::Center);
        assert_eq!(layout.width, Some(LengthSpec::Fill));
        assert_eq!(layout.gap, Some(LengthSpec::Px(FOOTER_GAP)));
        assert_eq!(layout.flex_grow, Some(0.0));
        assert_eq!(layout.flex_shrink, Some(0.0));
        assert_eq!(
            context.world().node(footer.stable_id()).unwrap().children,
            vec![settings.stable_id(), search.stable_id()]
        );
        assert_eq!(
            icon_of(context.world().standard_visual(settings.stable_id())),
            Some(Icon::Settings)
        );
        assert_eq!(
            icon_of(context.world().standard_visual(search.stable_id())),
            Some(Icon::Search)
        );
        assert_eq!(
            context
                .world()
                .node_style(settings.stable_id())
                .unwrap()
                .foreground,
            Some(SemanticColorRole::Muted)
        );
        assert_eq!(
            context
                .world()
                .node_style(settings.stable_id())
                .unwrap()
                .background,
            None
        );
    }

    #[test]
    fn footer_button_press_is_real() {
        let mut context = AppContext::new();
        let pressed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = pressed.clone();
        let button = context
            .create_component(
                document(),
                SidebarFooterButton::new("设置", Icon::Settings).selected(true),
            )
            .unwrap();
        context
            .on(button, move |_button, _event: &crate::Activate, _cx| {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            })
            .unwrap();
        assert!(context.activate_sidebar_footer_button(button).unwrap());
        assert!(pressed.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(
            context
                .world()
                .accessibility(button.stable_id())
                .unwrap()
                .label
                .as_deref(),
            Some("设置")
        );
        assert_eq!(
            context
                .world()
                .accessibility(button.stable_id())
                .unwrap()
                .selected,
            Some(true)
        );
        let selected_id = button.stable_id();
        assert_eq!(
            context.world().standard_visual(selected_id),
            Some(StandardVisual::Icon {
                icon: Icon::Settings,
                size: ControlSize::Small.icon_size(),
                tooltip: Some(crate::TooltipVisual {
                    label: Arc::from("设置"),
                    config: TooltipConfig::default(),
                    open: false,
                }),
            })
        );
        assert!(!matches!(
            context.world().standard_visual(selected_id),
            Some(StandardVisual::Button { .. })
        ));
        let selected_style = context.world().node_style(selected_id).unwrap();
        assert_eq!(selected_style.foreground, Some(SemanticColorRole::Text));
        assert_eq!(selected_style.background, None);
        assert_ne!(
            selected_style.foreground,
            Some(SemanticColorRole::AccentOnSoft)
        );
        assert_eq!(selected_style.interaction.selected.background, None);
        assert_eq!(
            selected_style.interaction.selected.foreground,
            Some(SemanticColorRole::Text)
        );
        assert_eq!(
            selected_style.square,
            Some(nana_ui_core::SquareSize::Control(ControlSize::Small))
        );
        assert_eq!(
            selected_style.control_height,
            Some(nana_ui_core::ControlHeight::Exact(ControlSize::Small))
        );
        let extracted = &context.world().extract_nodes(&[selected_id])[0]
            .source_style
            .layout;
        assert_eq!(
            extracted.width,
            Some(LengthSpec::Px(ControlSize::Small.height_in(UI_METRICS)))
        );
        assert_eq!(
            extracted.height,
            Some(LengthSpec::Px(ControlSize::Small.height_in(UI_METRICS)))
        );
        assert_eq!(selected_style.interaction.focused.border, None);

        let disabled = context
            .create_component(
                document(),
                SidebarFooterButton::new("禁用", Icon::Settings).disabled(true),
            )
            .unwrap();
        assert!(!context.activate_sidebar_footer_button(disabled).unwrap());
    }
}

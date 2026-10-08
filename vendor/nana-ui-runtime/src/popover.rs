use std::sync::Arc;

#[cfg(test)]
use nana_ui_core::UI_METRICS;
use nana_ui_core::{
    FlexDirection, Icon, LengthSpec, OverflowSpec, PopoverAlignment, PopoverPlacement,
    PositionSpec, SemanticColorRole, SemanticPalette, UI_BASE_TEXT_SIZE,
};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentElevation, ComponentGeometry,
    ComponentTextRegion, InteractionState, LayoutBox, MenuSurfaceKind, MutationQueue, NodeKind,
    NodeStyle, StableNodeId, StandardVisual, TriggeredMenuOverlay, UiWorld,
};

pub(crate) const POPOVER_WIDTH: f32 = 240.0;
const POPOVER_PADDING: f32 = nana_ui_core::space::LG;
const POPOVER_GAP: f32 = nana_ui_core::space::SM;
pub(crate) const ACTION_MENU_WIDTH: f32 = 200.0;
/// Inner padding of a menu list surface (action menu, context menu, the
/// `Select` drop-down). One authority so the three families cannot drift.
pub(crate) const MENU_SURFACE_PADDING: f32 = nana_ui_core::space::XS;
const ACTION_MENU_PADDING: f32 = MENU_SURFACE_PADDING;
const ACTION_MENU_GAP: f32 = nana_ui_core::space::XS;
pub(crate) const MENU_MIN_WIDTH: f32 = 120.0;
pub(crate) const MENU_ITEM_GAP: f32 = nana_ui_core::space::XXS;
/// Indentation per tree level, shared by `TreeView` and sidebar tree rows.
pub(crate) const TREE_DEPTH_STEP: f32 = nana_ui_core::space::XL;
/// The `z-index` an open Popover, ActionMenu or HoverCard stacks at: its
/// content is viewport-fixed at this level in the root stacking context, and
/// its surface paints and takes the pointer there too, above the page.
pub const MENU_OVERLAY_Z_INDEX: i32 = 1_000;
/// The trigger is a real button, so it matches the compact control height
/// rather than hugging its glyphs.
#[cfg(test)]
pub(crate) const TRIGGER_HEIGHT: f32 = UI_METRICS.compact_control_height;
/// Icon triggers draw the standard control glyph size, centered in the chrome.
const TRIGGER_ICON_SIZE: f32 = UI_BASE_TEXT_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopoverToggled {
    pub open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopoverClosed;

/// In-flow trigger with an optional surface painted below it.
///
/// The trigger is the popover's own node: it takes the press, the focus and
/// Enter / Space, and the surface hangs off it. It shows a label
/// ([`Popover::trigger`]), a glyph ([`Popover::trigger_icon`]), or content of
/// the application's ([`Popover::trigger_content`], a view's `.trigger(..)`
/// slot): a row the trigger lays out inside its chrome — an icon, a label and
/// a count — while the popover stays the control.
#[derive(Debug, Clone, PartialEq)]
pub struct Popover {
    /// Text trigger label, or the accessible name when [`Popover::trigger_icon`]
    /// or [`Popover::trigger_content`] draws the trigger instead.
    pub trigger: Arc<str>,
    pub trigger_icon: Option<Icon>,
    /// A child that draws the trigger. It stays shown while the surface is
    /// closed and is not one of the surface's items; it is display only (the
    /// press goes to the popover), so it holds no control of its own.
    pub trigger_content: Option<StableNodeId>,
    pub open: bool,
    pub placement: PopoverPlacement,
    pub alignment: PopoverAlignment,
    pub gap: f32,
    pub width: f32,
    pub padding: f32,
    pub close_on_escape: bool,
    pub close_on_outside: bool,
    /// Resting trigger has no fill and no border. Hover, press and the open
    /// state still wash the trigger so the hit target stays visible. Off by
    /// default: a trigger keeps the raised menu-button chrome.
    pub bare_trigger: bool,
}

impl Popover {
    pub fn new() -> Self {
        Self {
            trigger: Arc::from(""),
            trigger_icon: None,
            trigger_content: None,
            open: false,
            placement: PopoverPlacement::Bottom,
            alignment: PopoverAlignment::Center,
            gap: POPOVER_GAP,
            width: POPOVER_WIDTH,
            padding: POPOVER_PADDING,
            close_on_escape: true,
            close_on_outside: true,
            bare_trigger: false,
        }
    }

    pub fn trigger(mut self, trigger: impl Into<Arc<str>>) -> Self {
        self.trigger = trigger.into();
        self
    }

    /// Icon trigger with an accessible name; the glyph is drawn centered in a
    /// square chrome instead of riding the label's text metrics.
    pub fn trigger_icon(mut self, icon: Icon, label: impl Into<Arc<str>>) -> Self {
        self.trigger = label.into();
        self.trigger_icon = Some(icon);
        self
    }

    /// Draw the trigger with the child `content` (already a child of the
    /// popover, or placed by a view's `.trigger(..)` slot); `trigger` stays
    /// its accessible name.
    pub fn trigger_content(mut self, content: StableNodeId) -> Self {
        self.trigger_content = Some(content);
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn placement(mut self, placement: PopoverPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn alignment(mut self, alignment: PopoverAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(MENU_MIN_WIDTH);
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding.max(0.0);
        self
    }

    pub fn close_on_escape(mut self, enabled: bool) -> Self {
        self.close_on_escape = enabled;
        self
    }

    pub fn close_on_outside(mut self, enabled: bool) -> Self {
        self.close_on_outside = enabled;
        self
    }

    /// Drop the trigger's resting fill and border. Hover, press and open still
    /// wash it. The hanging surface is unchanged.
    pub fn bare_trigger(mut self, bare: bool) -> Self {
        self.bare_trigger = bare;
        self
    }
}

impl Default for Popover {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::ComponentView for Popover {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::toggle_popover),
        activate_at: Some(crate::AppContext::activate_popover_at),
        lifecycle: Some(crate::AppContext::settle_popover),
        ..crate::TypeBehavior::NONE
    };

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "popover".into(),
        }
    }

    /// Items attach and detach under the open surface, so the anchored
    /// projection must re-run with them to keep the origin math current.
    fn wants_child_reproject() -> bool {
        true
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_menu_surface(
            id,
            world,
            mutations,
            MenuSurfaceKind::Popover,
            Some(Arc::clone(&self.trigger)).filter(|value| !value.is_empty()),
            self.trigger_icon,
            self.trigger_content,
            self.open,
            self.width,
            self.padding,
            self.gap,
            self.placement,
            self.alignment,
            "popover",
            self.bare_trigger,
        );
    }
}

/// Trigger-bound action menu. Same surface as Popover with start alignment.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionMenu {
    pub popover: Popover,
}

impl ActionMenu {
    pub fn new() -> Self {
        Self {
            popover: Popover::new()
                .alignment(PopoverAlignment::Start)
                .gap(ACTION_MENU_GAP)
                .width(ACTION_MENU_WIDTH)
                .padding(ACTION_MENU_PADDING),
        }
    }

    pub fn trigger(mut self, trigger: impl Into<Arc<str>>) -> Self {
        self.popover = self.popover.trigger(trigger);
        self
    }

    pub fn trigger_icon(mut self, icon: Icon, label: impl Into<Arc<str>>) -> Self {
        self.popover = self.popover.trigger_icon(icon, label);
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.popover.open = open;
        self
    }

    pub fn placement(mut self, placement: PopoverPlacement) -> Self {
        self.popover = self.popover.placement(placement);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.popover = self.popover.width(width);
        self
    }

    pub fn bare_trigger(mut self, bare: bool) -> Self {
        self.popover = self.popover.bare_trigger(bare);
        self
    }
}

impl Default for ActionMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::ComponentView for ActionMenu {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::toggle_action_menu),
        activate_at: Some(crate::AppContext::activate_action_menu_at),
        lifecycle: Some(crate::AppContext::settle_action_menu),
        ..crate::TypeBehavior::NONE
    };

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "action-menu".into(),
        }
    }

    /// Items attach and detach under the open surface, so the anchored
    /// projection must re-run with them to keep the origin math current.
    fn wants_child_reproject() -> bool {
        true
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_menu_surface(
            id,
            world,
            mutations,
            MenuSurfaceKind::ActionMenu,
            Some(Arc::clone(&self.popover.trigger)).filter(|value| !value.is_empty()),
            self.popover.trigger_icon,
            self.popover.trigger_content,
            self.popover.open,
            self.popover.width,
            self.popover.padding,
            self.popover.gap,
            self.popover.placement,
            self.popover.alignment,
            "action-menu",
            self.popover.bare_trigger,
        );
    }
}

pub(crate) fn project_menu_surface(
    id: StableNodeId,
    world: &UiWorld,
    mutations: &mut MutationQueue,
    kind: MenuSurfaceKind,
    trigger: Option<Arc<str>>,
    trigger_icon: Option<Icon>,
    trigger_content: Option<StableNodeId>,
    open: bool,
    width: f32,
    padding: f32,
    gap: f32,
    placement: PopoverPlacement,
    alignment: PopoverAlignment,
    label: &str,
    bare_trigger: bool,
) {
    let open = world.project_menu_presence(id, open, mutations);
    // A child that draws the trigger only counts while it is one.
    let trigger_content = trigger_content.filter(|content| world.parent_id(*content) == Some(id));
    let has_trigger = trigger.is_some() || trigger_icon.is_some() || trigger_content.is_some();
    let has_chrome = open || has_trigger;
    if has_chrome {
        let visual = StandardVisual::MenuSurface {
            kind,
            open,
            // The label is the accessible name only while content draws it.
            trigger: trigger.clone().filter(|_| trigger_content.is_none()),
            trigger_icon,
            trigger_image: None,
            gap,
            overlay: has_trigger.then_some(TriggeredMenuOverlay {
                placement,
                alignment,
                width: width.max(MENU_MIN_WIDTH),
                padding,
                gap,
                trigger_content,
            }),
            query: None,
            rows: Arc::from([]),
            highlighted: None,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
    } else if world.standard_visual(id).is_some() {
        mutations.set_standard_visual(id, None);
    }
    // The trigger label is measured like any other button label, so the closed
    // surface can size itself to the text instead of a fixed box. Icon
    // triggers keep their label as the accessible name only; the square chrome
    // comes from the style instead of text measurement.
    let trigger_text = if trigger_icon.is_some() || trigger_content.is_some() {
        ""
    } else {
        trigger.as_deref().unwrap_or("")
    };
    if world.text(id) != Some(trigger_text) {
        mutations.set_text(
            id,
            crate::TextContent {
                value: trigger_text.to_string().into(),
            },
        );
    }
    let style = if trigger_content.is_some() {
        trigger_content_style()
    } else {
        triggered_menu_style(
            trigger_icon.is_some(),
            trigger.as_deref(),
            open,
            width,
            padding,
        )
    };
    let style = if bare_trigger && has_trigger {
        bare_trigger_style(style, open)
    } else {
        style
    };
    project_common(
        id,
        world,
        mutations,
        &style,
        InteractionState {
            pointer_events: open || has_trigger,
            focusable: has_trigger,
        },
        AccessibilityState {
            role: AccessibilityRole::Menu,
            label: Some(trigger.unwrap_or_else(|| Arc::from(label))),
            ..AccessibilityState::default()
        },
    );
}

/// The surface an open triggered menu (Popover, ActionMenu, HoverCard) hangs
/// above the page, in viewport coordinates: its items' box and padding.
/// `None` while closed, and for a menu with no trigger, whose surface is its
/// own in-flow box.
pub(crate) fn hanging_surface(world: &UiWorld, id: StableNodeId) -> Option<LayoutBox> {
    let crate::StandardVisual::MenuSurface {
        open: true,
        overlay: Some(overlay),
        trigger,
        trigger_icon,
        trigger_image,
        ..
    } = world.standard_visual_ref(id)?
    else {
        return None;
    };
    let has_trigger = trigger.is_some()
        || trigger_icon.is_some()
        || trigger_image.is_some()
        || overlay.trigger_content.is_some();
    let surface = overlay_surface_from_items(world, id, Some(overlay));
    (has_trigger && surface.width > 0.0 && surface.height > 0.0).then_some(surface)
}

pub(crate) fn overlay_surface_from_items(
    world: &UiWorld,
    id: StableNodeId,
    overlay: Option<&TriggeredMenuOverlay>,
) -> LayoutBox {
    let Some(overlay) = overlay else {
        return LayoutBox::default();
    };
    let children = world.node(id).map(|node| node.children).unwrap_or_default();
    let children = children
        .into_iter()
        .filter(|child| overlay.trigger_content != Some(*child));
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    let mut any = false;
    for child in children {
        // A closed branch leaves the items' last boxes behind until layout
        // writes zeros. Those boxes are not on screen; counting them paints
        // the card after it has already closed.
        if !world.is_overlay_reachable(child) {
            continue;
        }
        let Some(child_box) = world.layout_box(child) else {
            continue;
        };
        if child_box.width <= 0.0 && child_box.height <= 0.0 {
            continue;
        }
        any = true;
        min_x = min_x.min(child_box.x);
        min_y = min_y.min(child_box.y);
        max_x = max_x.max(child_box.x + child_box.width);
        max_y = max_y.max(child_box.y + child_box.height);
    }
    if !any {
        return LayoutBox::default();
    }
    LayoutBox {
        x: min_x - overlay.padding,
        y: min_y - overlay.padding,
        width: (max_x - min_x) + overlay.padding * 2.0,
        height: (max_y - min_y) + overlay.padding * 2.0,
    }
}

fn triggered_menu_style(
    icon_trigger: bool,
    trigger: Option<&str>,
    open: bool,
    width: f32,
    padding: f32,
) -> NodeStyle {
    if icon_trigger {
        return trigger_icon_button_style();
    }
    if trigger.is_some() {
        return trigger_button_style();
    }
    if !open {
        // Nothing to press and nothing to show, so the node keeps the smallest
        // box that stays out of the way.
        return NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                width: Some(LengthSpec::Px(nana_ui_core::HAIRLINE)),
                height: Some(LengthSpec::Px(nana_ui_core::HAIRLINE)),
                overflow_x: OverflowSpec::Hidden,
                overflow_y: OverflowSpec::Hidden,
                ..nana_ui_core::LayoutStyle::default()
            }),
            foreground: Some(SemanticColorRole::Text),
            ..NodeStyle::default()
        };
    }
    NodeStyle {
        layout: Arc::new(nana_ui_core::LayoutStyle {
            width: Some(LengthSpec::Px(width.max(MENU_MIN_WIDTH))),
            min_width: Some(LengthSpec::Px(MENU_MIN_WIDTH)),
            direction: Some(FlexDirection::Column),
            gap: Some(LengthSpec::Px(MENU_ITEM_GAP)),
            padding_left: Some(LengthSpec::Px(padding)),
            padding_right: Some(LengthSpec::Px(padding)),
            padding_top: Some(LengthSpec::Px(padding)),
            padding_bottom: Some(LengthSpec::Px(padding)),
            ..nana_ui_core::LayoutStyle::default()
        }),
        foreground: Some(SemanticColorRole::Text),
        radius: Some(nana_ui_core::RadiusTier::Md),
        ..NodeStyle::default()
    }
}

/// The trigger carries the same chrome contract as [`ButtonKind::Menu`]
/// (`Button`), so a standalone menu button and an in-place action-menu trigger
/// read as one control; hover and press colours resolve through the usual
/// interaction overlay.
pub(crate) fn trigger_button_style() -> NodeStyle {
    let mut style = NodeStyle {
        layout: Arc::new(nana_ui_core::LayoutStyle {
            // A button hugs its label. Without this the surrounding stack
            // stretches the trigger and it reads as a field, not a control.
            align_self: Some(nana_ui_core::AlignSpec::Start),
            border_width: Some(nana_ui_core::HAIRLINE),
            ..nana_ui_core::LayoutStyle::default()
        }),
        background: Some(SemanticColorRole::Subtle),
        border: Some(SemanticColorRole::BorderSoft),
        foreground: Some(SemanticColorRole::Text),
        radius: Some(nana_ui_core::RadiusTier::Sm),
        control_height: Some(nana_ui_core::ControlHeight::Exact(
            nana_ui_core::ControlSize::Small,
        )),
        control_padding_x: Some(nana_ui_core::ControlPadding::Standard),
        ..NodeStyle::default()
    };
    style.interaction.hovered.background = Some(SemanticColorRole::Hover);
    style.interaction.pressed.background = Some(SemanticColorRole::Active);
    style
}

/// A trigger drawn by a child shares the text trigger's chrome and lays the
/// child out inside it, centred on the cross axis.
pub(crate) fn trigger_content_style() -> NodeStyle {
    let mut style = trigger_button_style();
    let layout = Arc::make_mut(&mut style.layout);
    layout.direction = Some(FlexDirection::Row);
    layout.align_items = nana_ui_core::AlignSpec::Center;
    style
}

/// Icon triggers share the text trigger's chrome but take a square min box, so
/// the glyph centers geometrically instead of riding text metrics.
pub(crate) fn trigger_icon_button_style() -> NodeStyle {
    let mut style = trigger_button_style();
    style.square = Some(nana_ui_core::SquareSize::Control(
        nana_ui_core::ControlSize::Small,
    ));
    style
}

/// Resting trigger without a fill or a border. Hover and press keep the wash
/// already on `style`. While open, the same hover wash marks the anchor.
fn bare_trigger_style(mut style: NodeStyle, open: bool) -> NodeStyle {
    style.background = if open {
        Some(SemanticColorRole::Hover)
    } else {
        None
    };
    style.border = None;
    let layout = Arc::make_mut(&mut style.layout);
    layout.border_width = Some(0.0);
    style
}

pub(crate) fn menu_surface_style(width: f32, padding: f32) -> NodeStyle {
    NodeStyle {
        layout: Arc::new(nana_ui_core::LayoutStyle {
            position: PositionSpec::Fixed,
            width: Some(LengthSpec::Px(width)),
            min_width: Some(LengthSpec::Px(MENU_MIN_WIDTH)),
            direction: Some(FlexDirection::Column),
            gap: Some(LengthSpec::Px(MENU_ITEM_GAP)),
            padding_left: Some(LengthSpec::Px(padding)),
            padding_right: Some(LengthSpec::Px(padding)),
            padding_top: Some(LengthSpec::Px(padding)),
            padding_bottom: Some(LengthSpec::Px(padding)),
            border_width: Some(nana_ui_core::HAIRLINE),
            z_index: Some(MENU_OVERLAY_Z_INDEX),
            ..nana_ui_core::LayoutStyle::default()
        }),
        background: Some(SemanticColorRole::Surface),
        border: Some(SemanticColorRole::BorderSoft),
        foreground: Some(SemanticColorRole::Text),
        radius: Some(nana_ui_core::RadiusTier::Md),
        ..NodeStyle::default()
    }
}

pub(crate) fn project_anchored_menu(
    id: StableNodeId,
    world: &UiWorld,
    mutations: &mut MutationQueue,
    kind: MenuSurfaceKind,
    style: &NodeStyle,
    open: bool,
    label: &str,
) {
    let open = world.project_menu_presence(id, open, mutations);
    if open {
        let visual = StandardVisual::MenuSurface {
            kind,
            open,
            trigger: None,
            trigger_icon: None,
            trigger_image: None,
            gap: 0.0,
            overlay: None,
            query: None,
            rows: Arc::from([]),
            highlighted: None,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
    } else if world.standard_visual(id).is_some() {
        mutations.set_standard_visual(id, None);
    }
    let mut style = style.clone();
    Arc::make_mut(&mut style.layout).hidden = !open;
    project_common(
        id,
        world,
        mutations,
        &style,
        InteractionState {
            pointer_events: open,
            focusable: false,
        },
        AccessibilityState {
            role: AccessibilityRole::Menu,
            label: Some(Arc::from(label)),
            ..AccessibilityState::default()
        },
    );
}

pub(crate) fn menu_surface_geometry(
    bounds: LayoutBox,
    trigger: Option<&Arc<str>>,
    trigger_icon: Option<Icon>,
    trigger_image: Option<&Arc<str>>,
    trigger_content: bool,
    style: &crate::ComputedStyle,
    palette: &SemanticPalette,
    metrics: nana_ui_core::ThemeMetrics,
    surface: LayoutBox,
) -> ComponentGeometry {
    let is_light = palette.background.as_rgba_array()[0] > 0.5;
    let has_trigger =
        trigger.is_some() || trigger_icon.is_some() || trigger_image.is_some() || trigger_content;
    let trigger_h = if has_trigger { bounds.height } else { 0.0 };
    // In icon and image modes the trigger text is only the accessible name, so
    // no text region is emitted and the chrome owns the visuals.
    let label = if trigger_icon.is_none() && trigger_image.is_none() {
        trigger.filter(|value| !value.is_empty())
    } else {
        None
    };
    let trigger_bounds = LayoutBox {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: trigger_h,
    };
    let icon_extent = TRIGGER_ICON_SIZE
        .min(trigger_bounds.width)
        .min(trigger_bounds.height);
    ComponentGeometry::MenuSurface {
        trigger: label.map(|value| ComponentTextRegion {
            bounds: LayoutBox {
                x: trigger_bounds.x + nana_ui_core::ControlSize::Medium.padding_x_in(metrics),
                width: (trigger_bounds.width
                    - nana_ui_core::ControlSize::Medium.padding_x_in(metrics) * 2.0)
                    .max(0.0),
                ..trigger_bounds
            },
            content: Arc::clone(value).into(),
            color: Some(style.color.unwrap_or_else(|| palette.text.as_rgba_array())),
            font_size: nana_ui_core::type_scale::BODY,
            font_weight: None,
        }),
        trigger_icon: trigger_icon.map(|icon| {
            (
                icon,
                LayoutBox {
                    x: trigger_bounds.x + (trigger_bounds.width - icon_extent) / 2.0,
                    y: trigger_bounds.y + (trigger_bounds.height - icon_extent) / 2.0,
                    width: icon_extent,
                    height: icon_extent,
                },
            )
        }),
        trigger_image: trigger_image.cloned(),
        // Hover and press already resolved into the computed style, so the
        // trigger reads its chrome from there rather than the raw palette.
        trigger_surface: has_trigger.then_some(crate::ComponentTriggerSurface {
            bounds: trigger_bounds,
            background: style.background,
            border: style.border_color,
        }),
        surface,
        search: None,
        search_field: None,
        options: Vec::new(),
        elevation: ComponentElevation {
            color: [0.0, 0.0, 0.0, if is_light { 0.30 } else { 0.55 }],
            offset_x: 0.0,
            offset_y: 4.0,
            blur_radius: if is_light { 14.0 } else { 18.0 },
            spread_radius: 0.0,
            inset: false,
        },
        background: palette.surface.as_rgba_array(),
        border: palette.border_soft.as_rgba_array(),
    }
}

/// Where an anchored surface of `surface_width` × `surface_height` opens
/// against `trigger` inside `viewport`.
///
/// The surface takes the requested side unless it would overflow the viewport
/// there while the opposite side has room for it; then it flips, as a
/// tooltip does. Whatever side it lands on, it is then clamped into the
/// viewport along both axes, so a surface with room on neither side overlaps
/// its trigger rather than leaving the window.
pub fn resolve_popover_origin(
    trigger: LayoutBox,
    surface_width: f32,
    surface_height: f32,
    viewport: LayoutBox,
    placement: PopoverPlacement,
    alignment: PopoverAlignment,
    gap: f32,
) -> (f32, f32) {
    let fits_before = |start: f32, extent: f32, origin: f32| origin - gap - extent >= start;
    let fits_after = |end: f32, extent: f32, origin: f32| origin + gap + extent <= end;
    let (right, bottom) = (viewport.x + viewport.width, viewport.y + viewport.height);
    let placement = match placement {
        PopoverPlacement::Bottom
            if !fits_after(bottom, surface_height, trigger.y + trigger.height)
                && fits_before(viewport.y, surface_height, trigger.y) =>
        {
            PopoverPlacement::Top
        }
        PopoverPlacement::Top
            if !fits_before(viewport.y, surface_height, trigger.y)
                && fits_after(bottom, surface_height, trigger.y + trigger.height) =>
        {
            PopoverPlacement::Bottom
        }
        PopoverPlacement::Right
            if !fits_after(right, surface_width, trigger.x + trigger.width)
                && fits_before(viewport.x, surface_width, trigger.x) =>
        {
            PopoverPlacement::Left
        }
        PopoverPlacement::Left
            if !fits_before(viewport.x, surface_width, trigger.x)
                && fits_after(right, surface_width, trigger.x + trigger.width) =>
        {
            PopoverPlacement::Right
        }
        placement => placement,
    };
    let mut x = match placement {
        PopoverPlacement::Top | PopoverPlacement::Bottom => match alignment {
            PopoverAlignment::Start => trigger.x,
            PopoverAlignment::Center => trigger.x + trigger.width / 2.0 - surface_width / 2.0,
            PopoverAlignment::End => trigger.x + trigger.width - surface_width,
        },
        PopoverPlacement::Left => trigger.x - surface_width - gap,
        PopoverPlacement::Right => trigger.x + trigger.width + gap,
    };
    let mut y = match placement {
        PopoverPlacement::Top => trigger.y - surface_height - gap,
        PopoverPlacement::Bottom => trigger.y + trigger.height + gap,
        PopoverPlacement::Left | PopoverPlacement::Right => match alignment {
            PopoverAlignment::Start => trigger.y,
            PopoverAlignment::Center => trigger.y + trigger.height / 2.0 - surface_height / 2.0,
            PopoverAlignment::End => trigger.y + trigger.height - surface_height,
        },
    };
    let max_x = (right - surface_width).max(viewport.x);
    let max_y = (bottom - surface_height).max(viewport.y);
    x = x.clamp(viewport.x, max_x);
    y = y.clamp(viewport.y, max_y);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use crate::LayoutViewport;
    use crate::framework::AppContext;
    use std::time::Duration;

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    /// The card is the union of its items' boxes. Once the close motion has
    /// finished, that union is empty: the items have left the branch, and a
    /// leftover box must not keep the surface painted on the trigger.
    #[test]
    fn a_closed_menu_drops_its_hanging_surface() {
        let mut context = AppContext::new();
        let menu = context
            .create_component(
                document(),
                ActionMenu::new().trigger_icon(crate::Icon::Settings, "播放设置"),
            )
            .unwrap();
        let item = context
            .create_component(document(), crate::ActionMenuItem::new("剧场"))
            .unwrap();
        context.append_child(menu, item).unwrap();
        let viewport = LayoutViewport::new(480.0, 800.0);
        let layout = |context: &mut AppContext| {
            context.layout_document(document(), viewport).unwrap();
        };
        layout(&mut context);
        context.toggle_action_menu(menu).unwrap();
        layout(&mut context);
        let open_surface = match context.world().component_geometry(menu.stable_id()) {
            Some(crate::ComponentGeometry::MenuSurface { surface, .. }) => surface,
            other => panic!("open menu geometry: {other:?}"),
        };
        assert!(
            open_surface.width > 1.0 && open_surface.height > 1.0,
            "{open_surface:?}"
        );
        context.toggle_action_menu(menu).unwrap();
        context.advance_animations(Duration::from_millis(400));
        layout(&mut context);
        let closed_surface = match context.world().component_geometry(menu.stable_id()) {
            Some(crate::ComponentGeometry::MenuSurface { surface, .. }) => surface,
            other => panic!("closed menu geometry: {other:?}"),
        };
        assert!(
            closed_surface.width <= 1.0 && closed_surface.height <= 1.0,
            "card still painted: {closed_surface:?}"
        );
        assert!(!context.read(menu, |menu| menu.popover.open).unwrap());
    }

    #[test]
    fn choosing_an_item_closes_the_menu_that_offered_it() {
        let mut context = AppContext::new();
        let menu = context
            .create_component(document(), ActionMenu::new().trigger("Actions").open(true))
            .unwrap();
        let item = context
            .create_component(document(), crate::ActionMenuItem::new("Rename"))
            .unwrap();
        context.append_child(menu, item).unwrap();
        assert!(context.activate_action_menu_item(item).unwrap());
        assert!(!context.read(menu, |menu| menu.popover.open).unwrap());
    }

    #[test]
    fn first_action_item_beats_later_siblings_under_bottom_placement() {
        let mut context = AppContext::new();
        let menu = context
            .create_component(
                document(),
                crate::ActionMenu::new().trigger("Actions").open(true),
            )
            .unwrap();
        let first = context
            .create_component(document(), crate::ActionMenuItem::new("First"))
            .unwrap();
        let last = context
            .create_component(document(), crate::ActionMenuItem::new("Last"))
            .unwrap();
        context.append_child(menu, first).unwrap();
        context.append_child(menu, last).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        context.rebuild_hit_test(document());
        let first_id = first.stable_id();
        let first_box = context.world().layout_box(first_id).unwrap();
        let last_box = context.world().layout_box(last.stable_id()).unwrap();
        assert!(
            last_box.y > first_box.y,
            "items stack below the trigger: first={first_box:?} last={last_box:?}"
        );
        let hit = context.pointer_target(
            document(),
            first_box.x + first_box.width / 2.0,
            first_box.y + first_box.height / 2.0,
        );
        assert_eq!(
            hit,
            Some(first_id),
            "first={first_box:?} last={last_box:?} hit={hit:?}"
        );
    }

    /// The trigger is the only pressable affordance a closed menu has, so it
    /// must carry its own background rather than reading as bare text.
    #[test]
    fn a_closed_trigger_paints_pressable_button_chrome() {
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        let id = StableNodeId::new(1).unwrap();
        queue.create(
            id,
            document(),
            NodeKind::Element {
                tag: "action-menu".into(),
            },
        );
        queue.write_layout(
            id,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: TRIGGER_HEIGHT,
            },
        );
        queue.set_style(id, trigger_button_style());
        queue.set_standard_visual(
            id,
            Some(StandardVisual::MenuSurface {
                kind: MenuSurfaceKind::ActionMenu,
                open: false,
                trigger: Some(Arc::from("Actions")),
                trigger_icon: None,
                trigger_image: None,
                gap: 0.0,
                overlay: None,
                query: None,
                rows: Arc::from([]),
                highlighted: None,
            }),
        );
        world.commit(queue).unwrap();
        world.resolve_styles(&[id]).unwrap();
        let Some(ComponentGeometry::MenuSurface {
            trigger,
            trigger_surface,
            ..
        }) = world.component_geometry(id)
        else {
            panic!("expected menu surface geometry");
        };
        let chrome = trigger_surface.expect("trigger chrome");
        let idle = chrome.background.expect("trigger has a filled surface");
        assert_eq!(chrome.bounds.height, TRIGGER_HEIGHT);
        let label = trigger.expect("trigger label");
        assert!(label.bounds.x > 0.0, "label sits inside the button padding");

        world.set_pointer_hover(document(), 1, Some(id)).unwrap();
        world.advance_animations(nana_ui_core::motion::HOVER_COLOR);
        world.resolve_styles(&[id]).unwrap();
        let Some(ComponentGeometry::MenuSurface {
            trigger_surface, ..
        }) = world.component_geometry(id)
        else {
            panic!("expected menu surface geometry");
        };
        let hovered = trigger_surface
            .expect("trigger chrome")
            .background
            .expect("hovered trigger stays filled");
        assert_ne!(idle, hovered, "the trigger answers the pointer");
    }

    /// The trigger and the surface share one box, so a closed menu whose items
    /// still took part in layout would be stretched to their width.
    #[test]
    fn a_closed_menu_keeps_its_items_out_of_the_layout() {
        let mut context = AppContext::new();
        let menu = context
            .create_component(document(), ActionMenu::new().trigger("Actions"))
            .unwrap();
        let item = context
            .create_component(document(), crate::ActionMenuItem::new("Rename"))
            .unwrap();
        context.append_child(menu, item).unwrap();
        let omits_box = |context: &AppContext| {
            context
                .world()
                .layout_style(item.stable_id())
                .unwrap()
                .omits_box()
        };
        assert!(omits_box(&context));
        context.toggle_action_menu(menu).unwrap();
        assert!(!omits_box(&context));
    }

    #[test]
    fn popover_closed_keeps_the_trigger_and_open_reserves_surface_padding() {
        let mut context = AppContext::new();
        let popover = context
            .create_component(document(), Popover::new().trigger("Details"))
            .unwrap();
        let id = popover.stable_id();
        assert!(matches!(
            context.world().standard_visual(id),
            Some(StandardVisual::MenuSurface {
                kind: MenuSurfaceKind::Popover,
                trigger: Some(label),
                ..
            }) if label.as_ref() == "Details"
        ));
        let closed = context.world().node_style(id).unwrap();
        assert!(!closed.layout.hidden);
        assert_eq!(
            closed.control_height,
            Some(nana_ui_core::ControlHeight::Exact(
                nana_ui_core::ControlSize::Small
            ))
        );
        assert_eq!(
            context.world().extract_nodes(&[id])[0]
                .source_style
                .layout
                .height,
            Some(LengthSpec::Px(TRIGGER_HEIGHT))
        );
        // A closed trigger is a pressable button, not bare text.
        assert_eq!(closed.background, Some(SemanticColorRole::Subtle));
        assert_eq!(closed.border, Some(SemanticColorRole::BorderSoft));
        assert!(closed.interaction.hovered.background.is_some());
        assert!(closed.interaction.pressed.background.is_some());
        assert!(closed.layout.width.is_none(), "trigger sizes to its label");
        context
            .update_component(popover, |popover, _| {
                popover.open = true;
            })
            .unwrap();
        let open = context.world().node_style(id).unwrap();
        assert_eq!(
            context.world().extract_nodes(&[id])[0]
                .source_style
                .layout
                .height,
            Some(LengthSpec::Px(TRIGGER_HEIGHT))
        );
        assert_eq!(open.background, Some(SemanticColorRole::Subtle));
        assert_eq!(open.layout.position, PositionSpec::Static);
    }

    /// A bare icon trigger is an empty glyph at rest. The wash appears only
    /// while the pointer is on it, and again while the surface is open.
    #[test]
    fn bare_icon_trigger_rests_without_a_fill_and_washes_on_hover() {
        let mut context = AppContext::new();
        let popover = context
            .create_component(
                document(),
                Popover::new()
                    .trigger_icon(Icon::Add, "更多")
                    .bare_trigger(true),
            )
            .unwrap();
        let id = popover.stable_id();
        let closed = context.world().node_style(id).unwrap();
        assert_eq!(closed.background, None);
        assert_eq!(closed.border, None);
        assert_eq!(closed.layout.border_width, Some(0.0));
        assert_eq!(
            closed.interaction.hovered.background,
            Some(SemanticColorRole::Hover)
        );
        assert_eq!(
            closed.interaction.pressed.background,
            Some(SemanticColorRole::Active)
        );
        let layout = &context.world().extract_nodes(&[id])[0].source_style.layout;
        assert_eq!(layout.min_width, Some(LengthSpec::Px(TRIGGER_HEIGHT)));
        assert_eq!(layout.min_height, Some(LengthSpec::Px(TRIGGER_HEIGHT)));

        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        let geometry_id = StableNodeId::new(1).unwrap();
        queue.create(
            geometry_id,
            document(),
            NodeKind::Element {
                tag: "popover".into(),
            },
        );
        queue.write_layout(
            geometry_id,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: TRIGGER_HEIGHT,
                height: TRIGGER_HEIGHT,
            },
        );
        queue.set_style(
            geometry_id,
            bare_trigger_style(trigger_icon_button_style(), false),
        );
        queue.set_standard_visual(
            geometry_id,
            Some(StandardVisual::MenuSurface {
                kind: MenuSurfaceKind::Popover,
                open: false,
                trigger: None,
                trigger_icon: Some(Icon::Add),
                trigger_image: None,
                gap: 0.0,
                overlay: None,
                query: None,
                rows: Arc::from([]),
                highlighted: None,
            }),
        );
        world.commit(queue).unwrap();
        world.resolve_styles(&[geometry_id]).unwrap();
        let Some(ComponentGeometry::MenuSurface {
            trigger_surface, ..
        }) = world.component_geometry(geometry_id)
        else {
            panic!("expected menu surface geometry");
        };
        let chrome = trigger_surface.expect("trigger chrome");
        assert!(
            chrome.background.is_none(),
            "a bare trigger has no resting fill"
        );
        assert!(chrome.border.is_none(), "a bare trigger has no border");

        world
            .set_pointer_hover(document(), 1, Some(geometry_id))
            .unwrap();
        world.advance_animations(nana_ui_core::motion::HOVER_COLOR);
        world.resolve_styles(&[geometry_id]).unwrap();
        let Some(ComponentGeometry::MenuSurface {
            trigger_surface, ..
        }) = world.component_geometry(geometry_id)
        else {
            panic!("expected menu surface geometry");
        };
        assert!(
            trigger_surface
                .expect("trigger chrome")
                .background
                .is_some(),
            "hover washes the bare trigger"
        );

        context
            .update_component(popover, |popover, _| {
                popover.open = true;
            })
            .unwrap();
        let open = context.world().node_style(id).unwrap();
        assert_eq!(open.background, Some(SemanticColorRole::Hover));
        assert_eq!(open.border, None);
    }

    #[test]
    fn trigger_icon_swaps_the_label_for_an_accessible_glyph_trigger() {
        let mut context = AppContext::new();
        let menu = context
            .create_component(
                document(),
                ActionMenu::new().trigger_icon(Icon::Add, "添加"),
            )
            .unwrap();
        let id = menu.stable_id();
        assert!(matches!(
            context.world().standard_visual(id),
            Some(StandardVisual::MenuSurface {
                trigger: Some(trigger),
                trigger_icon: Some(icon),
                ..
            }) if trigger.as_ref() == "添加" && icon == Icon::Add
        ));
        // The label stays the accessible name only; the icon trigger measures
        // no text and its hit target is the square trigger box.
        assert!(context.world().text(id).is_none_or(|text| text.is_empty()));
        let layout = &context.world().extract_nodes(&[id])[0].source_style.layout;
        assert_eq!(layout.min_width, Some(LengthSpec::Px(TRIGGER_HEIGHT)));
        assert_eq!(layout.min_height, Some(LengthSpec::Px(TRIGGER_HEIGHT)));
    }

    /// The icon trigger's glyph must center geometrically in the chrome, not
    /// ride text line metrics — bare symbols ride high inside their em box.
    #[test]
    fn an_icon_trigger_centers_its_glyph_in_the_chrome() {
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        let id = StableNodeId::new(1).unwrap();
        queue.create(
            id,
            document(),
            NodeKind::Element {
                tag: "action-menu".into(),
            },
        );
        queue.write_layout(
            id,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: TRIGGER_HEIGHT,
                height: TRIGGER_HEIGHT,
            },
        );
        queue.set_style(id, trigger_icon_button_style());
        queue.set_standard_visual(
            id,
            Some(StandardVisual::MenuSurface {
                kind: MenuSurfaceKind::ActionMenu,
                open: false,
                trigger: None,
                trigger_icon: Some(Icon::Add),
                trigger_image: None,
                gap: 0.0,
                overlay: None,
                query: None,
                rows: Arc::from([]),
                highlighted: None,
            }),
        );
        world.commit(queue).unwrap();
        world.resolve_styles(&[id]).unwrap();
        let Some(ComponentGeometry::MenuSurface {
            trigger,
            trigger_icon,
            trigger_surface,
            ..
        }) = world.component_geometry(id)
        else {
            panic!("expected menu surface geometry");
        };
        assert!(trigger.is_none(), "icon trigger carries no text region");
        let (icon, icon_bounds) = trigger_icon.expect("trigger glyph box");
        assert_eq!(icon, Icon::Add);
        let chrome = trigger_surface.expect("trigger chrome").bounds;
        assert_eq!(
            icon_bounds.x + icon_bounds.width / 2.0,
            chrome.x + chrome.width / 2.0
        );
        assert_eq!(
            icon_bounds.y + icon_bounds.height / 2.0,
            chrome.y + chrome.height / 2.0
        );
    }

    #[test]
    fn action_menu_defaults_start_alignment_and_compact_padding() {
        let menu = ActionMenu::new();
        assert_eq!(menu.popover.alignment, PopoverAlignment::Start);
        assert_eq!(menu.popover.gap, ACTION_MENU_GAP);
        assert_eq!(menu.popover.width, ACTION_MENU_WIDTH);
        assert_eq!(menu.popover.padding, ACTION_MENU_PADDING);
    }

    #[test]
    fn popover_origin_flips_to_the_side_with_room_then_clamps() {
        let viewport = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 120.0,
        };
        let origin = |trigger, placement| {
            resolve_popover_origin(
                trigger,
                80.0,
                60.0,
                viewport,
                placement,
                PopoverAlignment::Center,
                6.0,
            )
        };
        // No room below the trigger, room above: it opens upward, and the
        // cross axis clamps into the viewport.
        let low = LayoutBox {
            x: 90.0,
            y: 80.0,
            width: 20.0,
            height: 20.0,
        };
        assert_eq!(origin(low, PopoverPlacement::Bottom), (40.0, 14.0));
        // No room to the left, room to the right: it opens rightward.
        let tight = LayoutBox {
            x: 4.0,
            y: 4.0,
            width: 20.0,
            height: 20.0,
        };
        assert_eq!(origin(tight, PopoverPlacement::Left), (30.0, 0.0));
        // Room on the requested side: no flip.
        assert_eq!(origin(tight, PopoverPlacement::Bottom), (0.0, 30.0));
        // Room on neither side: it keeps the requested side and is clamped
        // over the trigger rather than leaving the viewport.
        let middle = LayoutBox {
            x: 50.0,
            y: 50.0,
            width: 20.0,
            height: 20.0,
        };
        assert_eq!(origin(middle, PopoverPlacement::Bottom), (20.0, 60.0));
    }

    /// An open menu's items leave the host row: the trigger keeps the closed
    /// box, neighbours stay put, and the panel hangs below the trigger.
    #[test]
    fn an_open_menu_stops_participating_in_its_host_layout() {
        let mut context = AppContext::new();
        let row = context
            .create_component(document(), crate::Stack::row(8.0))
            .unwrap();
        let sibling = context
            .create_component(document(), crate::Button::new("侧边"))
            .unwrap();
        let slot = context
            .create_component(document(), crate::Stack::column(4.0))
            .unwrap();
        let menu = context
            .create_component(document(), ActionMenu::new().trigger("询问"))
            .unwrap();
        context.append_child(row, sibling).unwrap();
        context.append_child(row, slot).unwrap();
        context.append_child(slot, menu).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        let closed_slot = context.world().layout_box(slot.stable_id()).unwrap();
        let closed_sibling = context.world().layout_box(sibling.stable_id()).unwrap();
        let closed_trigger = context.world().layout_box(menu.stable_id()).unwrap();

        context
            .update_component(menu, |menu, _| {
                menu.popover.open = true;
            })
            .unwrap();
        let item = context
            .create_component(document(), crate::ActionMenuItem::new("只读"))
            .unwrap();
        context.append_child(menu, item).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();

        let open_trigger = context.world().layout_box(menu.stable_id()).unwrap();
        let open_slot = context.world().layout_box(slot.stable_id()).unwrap();
        let open_item = context.world().layout_box(item.stable_id()).unwrap();
        let style = context.world().node_style(menu.stable_id()).unwrap();
        assert_eq!(style.layout.position, PositionSpec::Static);
        assert_eq!(open_trigger, closed_trigger);
        assert_eq!(open_slot, closed_slot);
        assert_eq!(
            context.world().layout_box(sibling.stable_id()).unwrap(),
            closed_sibling
        );
        assert!(
            open_item.y >= open_trigger.y + open_trigger.height + ACTION_MENU_GAP - 1.0,
            "panel hangs below the trigger: trigger={open_trigger:?} item={open_item:?}"
        );
        let item_style = context.world().layout_style(item.stable_id()).unwrap();
        assert_eq!(item_style.position, PositionSpec::Fixed);
    }

    /// `Top` placement anchors the panel above the trigger: the trigger stays
    /// in place and the item column sits one gap above it.
    #[test]
    fn top_placement_opens_the_surface_above_its_slot() {
        let mut context = AppContext::new();
        let column = context
            .create_component(document(), crate::Stack::column(0.0))
            .unwrap();
        let spacer = context
            .create_component(
                document(),
                crate::Stack::column(0.0).height(LengthSpec::Px(240.0)),
            )
            .unwrap();
        let row = context
            .create_component(document(), crate::Stack::row(8.0))
            .unwrap();
        let slot = context
            .create_component(document(), crate::Stack::column(4.0))
            .unwrap();
        let menu = context
            .create_component(
                document(),
                ActionMenu::new()
                    .trigger("工作树")
                    .placement(PopoverPlacement::Top),
            )
            .unwrap();
        context.append_child(column, spacer).unwrap();
        context.append_child(column, row).unwrap();
        context.append_child(row, slot).unwrap();
        context.append_child(slot, menu).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        let closed_trigger = context.world().layout_box(menu.stable_id()).unwrap();

        context
            .update_component(menu, |menu, _| {
                menu.popover.open = true;
            })
            .unwrap();
        let item = context
            .create_component(document(), crate::ActionMenuItem::new("当前仓库"))
            .unwrap();
        context.append_child(menu, item).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();

        let open_trigger = context.world().layout_box(menu.stable_id()).unwrap();
        let open_item = context.world().layout_box(item.stable_id()).unwrap();
        assert_eq!(
            open_trigger, closed_trigger,
            "trigger stays in the host row"
        );
        assert!(
            open_item.y + open_item.height <= open_trigger.y - ACTION_MENU_GAP + 1.0,
            "panel opens above the trigger: trigger={open_trigger:?} item={open_item:?}"
        );
    }

    /// The `Menu` button kind paints the trigger chrome so an app-built menu
    /// button and an in-place action-menu trigger read as one control.
    #[test]
    fn menu_button_kind_paints_menu_trigger_chrome() {
        let mut context = AppContext::new();
        let button = context
            .create_component(
                document(),
                crate::Button::new("询问").kind(nana_ui_core::ButtonKind::Menu),
            )
            .unwrap();
        let style = context.world().node_style(button.stable_id()).unwrap();
        assert_eq!(
            style.background,
            Some(nana_ui_core::SemanticColorRole::Subtle)
        );
        assert_eq!(
            style.border,
            Some(nana_ui_core::SemanticColorRole::BorderSoft)
        );
        assert_eq!(
            style.interaction.hovered.background,
            Some(nana_ui_core::SemanticColorRole::Hover)
        );
    }

    #[test]
    fn scrolled_popover_overlay_hit_matches_accessibility_bounds() {
        let mut context = AppContext::new();
        let page = context
            .create_component(
                document(),
                crate::ScrollView::new(crate::ScrollAxes::Vertical).style(
                    crate::Stack::column(0.0)
                        .width(LengthSpec::Fill)
                        .height(LengthSpec::Px(240.0))
                        .node_style(),
                ),
            )
            .unwrap();
        let column = context
            .create_component(
                document(),
                crate::Stack::column(0.0).width(LengthSpec::Fill),
            )
            .unwrap();
        let spacer = context
            .create_component(
                document(),
                crate::Stack::column(0.0).height(LengthSpec::Px(400.0)),
            )
            .unwrap();
        let popover = context
            .create_component(
                document(),
                Popover::new().trigger("选择草稿").width(240.0).open(true),
            )
            .unwrap();
        context.append_child(page, column).unwrap();
        context.append_child(column, spacer).unwrap();
        context.append_child(column, popover).unwrap();
        let overlay = context
            .create_component(
                document(),
                crate::ScrollView::new(crate::ScrollAxes::Vertical).style(
                    crate::Stack::column(0.0)
                        .width(LengthSpec::Fill)
                        .height(LengthSpec::Px(200.0))
                        .node_style(),
                ),
            )
            .unwrap();
        let list = context
            .create_component(
                document(),
                crate::Stack::column(6.0).width(LengthSpec::Fill),
            )
            .unwrap();
        context.append_child(popover, overlay).unwrap();
        context.append_child(overlay, list).unwrap();
        let mut last = None;
        for index in 0..24 {
            let item = context
                .create_component(document(), crate::Button::new(format!("草稿 {index}")))
                .unwrap();
            context.append_child(list, item).unwrap();
            last = Some(item);
        }
        let last = last.unwrap();
        let viewport = LayoutViewport::new(320.0, 400.0);
        context.layout_document(document(), viewport).unwrap();
        context
            .scroll_to(page, crate::ScrollOffset { x: 0.0, y: 160.0 })
            .unwrap();
        context
            .scroll_to(
                overlay,
                crate::ScrollOffset {
                    x: 0.0,
                    y: 1_000_000.0,
                },
            )
            .unwrap();
        context.layout_document(document(), viewport).unwrap();
        context.rebuild_hit_test(document());
        assert_eq!(
            context.world().scroll_offset(page.stable_id()).map(|o| o.y),
            Some(160.0)
        );
        let last_id = last.stable_id();
        let bounds = context
            .world()
            .project_accessibility(document())
            .into_iter()
            .find(|entry| entry.id == last_id)
            .expect("last item is accessible")
            .bounds;
        let hit = context.pointer_target(
            document(),
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        );
        assert_eq!(
            hit,
            Some(last_id),
            "overlay a11y center must ignore ancestor page scroll: {bounds:?} -> {hit:?}"
        );
    }
}

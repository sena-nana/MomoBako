use std::sync::Arc;

use nana_ui_core::{
    AnchoredMenuPlacement, ControlSize, Icon, LengthSpec, LineHeightSpec, PositionSpec,
    SemanticColorRole, SemanticPalette, ThemeMetrics, icon_y_on_text_glyph_center,
};

use crate::popover::{menu_surface_style, project_anchored_menu};
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentElevation, ComponentGeometry,
    ComponentTextRegion, ComputedStyle, InteractionState, InteractionStyle, LayoutBox,
    MenuSurfaceKind, MutationQueue, NodeKind, NodeStyle, SelectOptionData, SemanticPaint,
    StableNodeId, StandardVisual, TextContent, TextInputState, TextVerticalAlignment, UiWorld,
};

const MENU_WIDTH: f32 = crate::popover::ACTION_MENU_WIDTH;
const MENU_PADDING: f32 = crate::popover::MENU_SURFACE_PADDING;
use crate::popover::MENU_MIN_WIDTH;
const MENU_MIN_HEIGHT: f32 = nana_ui_core::ControlSize::Medium.height_in(nana_ui_core::UI_METRICS);
const ICON_GAP: f32 = nana_ui_core::space::MD;

/// Selectable row shared by action menus and context menus.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionMenuItem {
    pub label: Arc<str>,
    pub hint: Option<Arc<str>>,
    pub leading: Option<Icon>,
    pub size: ControlSize,
    pub active: bool,
    pub danger: bool,
    pub disabled: bool,
    pub style: NodeStyle,
}

impl ActionMenuItem {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        let size = ControlSize::Small;
        Self {
            label: label.into(),
            hint: None,
            leading: None,
            size,
            active: false,
            danger: false,
            disabled: false,
            style: item_style(size),
        }
    }

    pub fn hint(mut self, hint: impl Into<Arc<str>>) -> Self {
        let hint = hint.into();
        self.hint = (!hint.is_empty()).then_some(hint);
        self
    }

    pub fn leading(mut self, leading: Icon) -> Self {
        self.leading = Some(leading);
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self.style = item_style(size);
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        style.foreground = Some(if self.disabled {
            SemanticColorRole::Faint
        } else if self.danger {
            SemanticColorRole::Danger
        } else {
            SemanticColorRole::Text
        });
        style.background = if self.active && self.danger {
            Some(SemanticColorRole::AccentSoft)
        } else if self.active {
            Some(SemanticColorRole::Hover)
        } else {
            None
        };
        style.interaction.hovered.background = Some(if self.danger {
            SemanticColorRole::AccentSoft
        } else {
            SemanticColorRole::Hover
        });
        style.interaction.pressed.background = Some(if self.danger {
            SemanticColorRole::AccentSoftPressed
        } else {
            SemanticColorRole::Active
        });
        style.interaction.selected.background = style.background;
        style
    }
}

impl crate::ComponentView for ActionMenuItem {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::activate_action_menu_item),
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
            tag: "action-menu-item".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let visual = StandardVisual::ActionMenuItem {
            label: Arc::clone(&self.label),
            hint: self.hint.clone(),
            icon: self.leading,
            danger: self.danger,
            active: self.active,
            disabled: self.disabled,
            size: self.size,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        if world.text(id) != Some(self.label.as_ref()) {
            mutations.set_text(
                id,
                TextContent {
                    value: self.label.to_string().into(),
                },
            );
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: !self.disabled,
                focusable: !self.disabled,
            },
            AccessibilityState {
                role: AccessibilityRole::MenuItem,
                label: Some(Arc::clone(&self.label)),
                description: self.hint.clone(),
                disabled: self.disabled,
                selected: Some(self.active),
                ..AccessibilityState::default()
            },
        );
    }
}

/// Viewport-level menu surface pinned to a logical anchor.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchoredActionMenu {
    pub open: bool,
    pub x: f32,
    pub y: f32,
    pub placement: AnchoredMenuPlacement,
    pub width: f32,
    pub height: f32,
    pub style: NodeStyle,
}

impl AnchoredActionMenu {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn new(x: f32, y: f32) -> Self {
        let mut menu = Self {
            open: true,
            x,
            y,
            placement: AnchoredMenuPlacement::BottomStart,
            width: MENU_WIDTH,
            height: 0.0,
            style: menu_surface_style(MENU_WIDTH, MENU_PADDING),
        };
        menu.apply_anchor();
        menu
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn placement(mut self, placement: AnchoredMenuPlacement) -> Self {
        self.placement = placement;
        self.apply_anchor();
        self
    }

    pub fn menu_size(mut self, width: f32, height: f32) -> Self {
        self.width = width.max(MENU_MIN_WIDTH);
        self.height = if height >= MENU_MIN_HEIGHT {
            height
        } else {
            0.0
        };
        let layout = Arc::make_mut(&mut self.style.layout);
        layout.width = Some(LengthSpec::Px(self.width));
        if self.height > 0.0 {
            layout.height = Some(LengthSpec::Px(self.height));
        } else {
            layout.height = None;
        }
        self.apply_anchor();
        self
    }

    pub fn place_in(&mut self, viewport: LayoutBox) {
        let origin = resolve_anchored_origin(
            self.x,
            self.y,
            self.width,
            self.height,
            viewport,
            self.placement,
        );
        let layout = Arc::make_mut(&mut self.style.layout);
        layout.offset_left = Some(LengthSpec::Px(origin.0));
        layout.offset_top = Some(LengthSpec::Px(origin.1));
    }

    fn apply_anchor(&mut self) {
        let layout = Arc::make_mut(&mut self.style.layout);
        layout.position = PositionSpec::Fixed;
        layout.offset_left = Some(LengthSpec::Px(self.x));
        layout.offset_top = Some(LengthSpec::Px(self.y));
        layout.height = (self.height > 0.0).then_some(LengthSpec::Px(self.height));
    }
}

impl crate::ComponentView for AnchoredActionMenu {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "anchored-action-menu".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_anchored_menu(
            id,
            world,
            mutations,
            MenuSurfaceKind::ActionMenu,
            &self.style,
            self.open,
            "menu",
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenuItem {
    pub value: Arc<str>,
    pub label: Arc<str>,
    pub hint: Option<Arc<str>>,
    pub icon: Option<Icon>,
    pub disabled: bool,
    pub danger: bool,
}

impl ContextMenuItem {
    pub fn new(value: impl Into<Arc<str>>, label: impl Into<Arc<str>>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            hint: None,
            icon: None,
            disabled: false,
            danger: false,
        }
    }

    pub fn hint(mut self, hint: impl Into<Arc<str>>) -> Self {
        let hint = hint.into();
        self.hint = (!hint.is_empty()).then_some(hint);
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuEvent {
    Select(Arc<str>),
    Search(Arc<str>),
    Dismiss,
}

/// Pointer-anchored menu. Slash-separated values (`parent/child`) are a tree;
/// [`Self::query`] filters the current level or matching leaves. Searchable
/// menus own a committed [`TextInputState`] for the filter field.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextMenu {
    pub items: Vec<ContextMenuItem>,
    pub open: bool,
    pub anchor_x: f32,
    pub anchor_y: f32,
    pub highlighted: Option<usize>,
    pub width: f32,
    pub style: NodeStyle,
    pub active_path: Vec<Arc<str>>,
    pub query: Arc<str>,
    pub searchable: bool,
    pub state: TextInputState,
    /// Bounds the surface is kept inside; `None` anchors verbatim.
    /// See [`Self::place_in`].
    pub viewport: Option<LayoutBox>,
}

impl ContextMenu {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn new(anchor_x: f32, anchor_y: f32) -> Self {
        let mut menu = Self {
            items: Vec::new(),
            open: true,
            anchor_x,
            anchor_y,
            highlighted: None,
            width: MENU_WIDTH,
            style: menu_surface_style(MENU_WIDTH, MENU_PADDING),
            active_path: Vec::new(),
            query: Arc::from(""),
            searchable: false,
            state: TextInputState::new(""),
            viewport: None,
        };
        menu.apply_anchor();
        menu
    }

    pub fn items(mut self, items: impl IntoIterator<Item = ContextMenuItem>) -> Self {
        self.items = items.into_iter().collect();
        self.apply_anchor();
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(MENU_MIN_WIDTH);
        Arc::make_mut(&mut self.style.layout).width = Some(LengthSpec::Px(self.width));
        self
    }

    pub fn active_path(mut self, path: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        self.active_path = path.into_iter().map(Into::into).collect();
        self.apply_anchor();
        self
    }

    pub fn query(mut self, query: impl Into<Arc<str>>) -> Self {
        let query = query.into();
        self.query = Arc::clone(&query);
        self.state = TextInputState::new(query.as_ref());
        self.apply_anchor();
        self
    }

    pub fn searchable(mut self, searchable: bool) -> Self {
        self.searchable = searchable;
        self.apply_anchor();
        self
    }

    pub fn set_query(&mut self, query: impl Into<Arc<str>>) {
        let query = query.into();
        self.query = Arc::clone(&query);
        self.state = TextInputState::new(query.as_ref());
        self.apply_anchor();
    }

    pub(crate) fn sync_query_from_state(&mut self) {
        self.query = Arc::from(self.state.value.as_str());
        self.apply_anchor();
    }

    /// Rows at the current nested level, or matching leaves when `query` is set.
    pub fn visible_items(&self) -> Vec<ContextMenuItem> {
        if self.query.trim().is_empty() {
            self.current_level_items()
        } else {
            self.matching_leaves()
        }
    }

    /// Keep the surface inside `viewport` from now on. The bounds are retained,
    /// so filtering and drilling into a submenu — both of which re-anchor the
    /// surface as its row count changes — stay inside it too. Without this a
    /// pointer-anchored menu can lay out past the window with no way to reach
    /// its lower rows.
    pub fn place_in(&mut self, viewport: LayoutBox) {
        self.viewport = Some(viewport);
        self.apply_anchor();
    }

    pub fn select_index(&mut self, index: usize) -> Option<ContextMenuEvent> {
        let item = self.visible_items().get(index)?.clone();
        if item.disabled || !self.open {
            return None;
        }
        if self.has_descendants(&item.value) {
            self.active_path = value_segments(&item.value)
                .into_iter()
                .map(Arc::from)
                .collect();
            self.highlighted = None;
            self.apply_anchor();
            return None;
        }
        self.open = false;
        self.highlighted = None;
        self.active_path.clear();
        Some(ContextMenuEvent::Select(item.value))
    }

    /// Pop the current nested level. Returns `true` when the path changed.
    pub fn back(&mut self) -> bool {
        if self.active_path.pop().is_none() {
            return false;
        }
        self.highlighted = None;
        self.apply_anchor();
        true
    }

    pub fn dismiss(&mut self) {
        self.open = false;
        self.highlighted = None;
        self.active_path.clear();
    }

    fn apply_anchor(&mut self) {
        let height = context_menu_height(
            self.visible_items().len(),
            self.searchable,
            nana_ui_core::UI_METRICS,
        );
        let (x, y) = match self.viewport {
            Some(viewport) => resolve_anchored_origin(
                self.anchor_x,
                self.anchor_y,
                self.width,
                height,
                viewport,
                AnchoredMenuPlacement::BottomStart,
            ),
            None => (self.anchor_x, self.anchor_y),
        };
        let layout = Arc::make_mut(&mut self.style.layout);
        layout.position = PositionSpec::Fixed;
        layout.offset_left = Some(LengthSpec::Px(x));
        layout.offset_top = Some(LengthSpec::Px(y));
        layout.height = Some(LengthSpec::Px(height));
    }

    fn current_level_items(&self) -> Vec<ContextMenuItem> {
        let prefix: Vec<&str> = self.active_path.iter().map(|part| part.as_ref()).collect();
        let mut level: Vec<(String, ContextMenuItem)> = Vec::new();
        for item in &self.items {
            let segments = value_segments(&item.value);
            if segments.len() <= prefix.len() || segments[..prefix.len()] != prefix {
                continue;
            }
            let next = segments[prefix.len()];
            let next_value = if prefix.is_empty() {
                next.to_string()
            } else {
                format!("{}/{next}", prefix.join("/"))
            };
            let is_exact = segments.len() == prefix.len() + 1;
            if let Some((_, existing)) = level.iter_mut().find(|(key, _)| key == &next_value) {
                if is_exact {
                    *existing = item.clone();
                }
            } else if is_exact {
                level.push((next_value, item.clone()));
            } else {
                level.push((
                    next_value.clone(),
                    ContextMenuItem::new(Arc::<str>::from(next_value), Arc::<str>::from(next)),
                ));
            }
        }
        level.into_iter().map(|(_, item)| item).collect()
    }

    fn matching_leaves(&self) -> Vec<ContextMenuItem> {
        self.items
            .iter()
            .filter(|item| {
                !self.has_descendants(&item.value) && item_matches_query(item, &self.query)
            })
            .cloned()
            .collect()
    }

    fn has_descendants(&self, value: &str) -> bool {
        let prefix = value_segments(value);
        self.items.iter().any(|item| {
            let segments = value_segments(&item.value);
            segments.len() > prefix.len() && segments[..prefix.len()] == prefix
        })
    }
}

impl crate::ComponentView for ContextMenu {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::dismiss_context_menu),
        activate_at: Some(crate::AppContext::activate_context_menu_at),
        editable: Some(&crate::framework::Editable::<Self>::HOOKS),
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
            tag: "context-menu".into(),
        }
    }

    fn wants_metrics_reproject() -> bool {
        true
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let open = world.project_menu_presence(id, self.open, mutations);
        let rows: Arc<[SelectOptionData]> = self
            .visible_items()
            .into_iter()
            .map(|item| SelectOptionData {
                label: item.label,
                hint: item.hint,
                disabled: item.disabled,
                checked: false,
                icon: item.icon,
            })
            .collect();
        if open {
            let visual = StandardVisual::MenuSurface {
                kind: MenuSurfaceKind::ContextMenu,
                open: true,
                trigger: None,
                trigger_icon: None,
                trigger_image: None,
                gap: 0.0,
                overlay: None,
                query: self.searchable.then(|| Arc::clone(&self.query)),
                rows,
                highlighted: self.highlighted,
            };
            if world.standard_visual(id) != Some(visual.clone()) {
                mutations.set_standard_visual(id, Some(visual));
            }
        } else if world.standard_visual(id).is_some() {
            mutations.set_standard_visual(id, None);
        }
        if self.searchable && self.open {
            if world
                .text_input(id)
                .is_none_or(|current| current != self.state)
            {
                mutations.set_text_input(id, Some(self.state.clone()));
            }
        } else if world.text_input(id).is_some() {
            mutations.set_text_input(id, None);
        }
        let mut style = self.style.clone();
        {
            let layout = Arc::make_mut(&mut style.layout);
            layout.hidden = !open;
            let height = context_menu_height(
                self.visible_items().len(),
                self.searchable,
                world.theme_metrics(),
            );
            layout.height = Some(LengthSpec::Px(height));
            let (x, y) = match self.viewport {
                Some(viewport) => resolve_anchored_origin(
                    self.anchor_x,
                    self.anchor_y,
                    self.width,
                    height,
                    viewport,
                    AnchoredMenuPlacement::BottomStart,
                ),
                None => (self.anchor_x, self.anchor_y),
            };
            layout.offset_left = Some(LengthSpec::Px(x));
            layout.offset_top = Some(LengthSpec::Px(y));
        }
        project_common(
            id,
            world,
            mutations,
            &style,
            InteractionState {
                pointer_events: self.open,
                focusable: self.open && self.searchable,
            },
            AccessibilityState {
                role: AccessibilityRole::Menu,
                label: Some(Arc::from("context-menu")),
                value: (self.searchable && !self.query.is_empty()).then(|| Arc::clone(&self.query)),
                editable: self.searchable && self.open,
                ..AccessibilityState::default()
            },
        );
    }
}

/// Hint text renders at a fixed compact size regardless of control size.
const HINT_TEXT_SIZE: f32 = nana_ui_core::type_scale::HINT;

pub(crate) fn action_menu_item_geometry(
    bounds: LayoutBox,
    label: &Arc<str>,
    hint: Option<&Arc<str>>,
    icon: Option<Icon>,
    danger: bool,
    disabled: bool,
    size: ControlSize,
    style: &ComputedStyle,
    palette: &SemanticPalette,
    metrics: ThemeMetrics,
    measure: crate::text_width::ChromeTextMeasure<'_>,
) -> ComponentGeometry {
    let pad = size.padding_x_in(metrics);
    let icon_color = if disabled {
        palette.faint.as_rgba_array()
    } else if danger {
        palette.danger.as_rgba_array()
    } else {
        palette.muted.as_rgba_array()
    };
    let (cursor, icon) = menu_option_icon(bounds, icon, size, icon_color, metrics);
    let hint_width = hint
        .map(|hint| measure.width(hint, HINT_TEXT_SIZE, None))
        .unwrap_or(0.0);
    let hint_gap = if hint.is_some() { ICON_GAP } else { 0.0 };
    let label_right = bounds.x + bounds.width - pad - hint_width - hint_gap;
    let foreground = if disabled {
        palette.faint.as_rgba_array()
    } else if danger {
        palette.danger.as_rgba_array()
    } else {
        style.color.unwrap_or_else(|| palette.text.as_rgba_array())
    };
    ComponentGeometry::ActionMenuItem {
        icon,
        label: ComponentTextRegion {
            bounds: LayoutBox {
                x: cursor,
                y: bounds.y,
                width: (label_right - cursor).max(0.0),
                height: bounds.height,
            },
            content: Arc::clone(label).into(),
            color: Some(foreground),
            font_size: size.text_size(),
            font_weight: Some(500),
        },
        hint: hint.map(|hint| ComponentTextRegion {
            bounds: LayoutBox {
                x: label_right + hint_gap,
                y: bounds.y,
                width: hint_width,
                height: bounds.height,
            },
            content: Arc::clone(hint).into(),
            color: Some(palette.muted.as_rgba_array()),
            font_size: HINT_TEXT_SIZE,
            font_weight: None,
        }),
        background: style.background,
    }
}

pub fn resolve_anchored_origin(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    viewport: LayoutBox,
    placement: AnchoredMenuPlacement,
) -> (f32, f32) {
    let mut origin = match placement {
        AnchoredMenuPlacement::TopStart => (x, y - height),
        AnchoredMenuPlacement::TopEnd => (x - width, y - height),
        AnchoredMenuPlacement::BottomStart => (x, y),
        AnchoredMenuPlacement::BottomEnd => (x - width, y),
    };
    let max_x = (viewport.x + viewport.width - width).max(viewport.x);
    let max_y = (viewport.y + viewport.height - height).max(viewport.y);
    origin.0 = origin.0.clamp(viewport.x, max_x);
    origin.1 = origin.1.clamp(viewport.y, max_y);
    origin
}

const SEARCH_FIELD_GAP: f32 = nana_ui_core::space::XS;

fn search_field_height(metrics: nana_ui_core::ThemeMetrics) -> f32 {
    ControlSize::Small.height_in(metrics)
}

fn context_menu_height(
    item_count: usize,
    searchable: bool,
    metrics: nana_ui_core::ThemeMetrics,
) -> f32 {
    let count = item_count.max(1) as f32;
    let item = ControlSize::Small.height_in(metrics);
    let list =
        MENU_PADDING * 2.0 + count * item + (count - 1.0).max(0.0) * crate::popover::MENU_ITEM_GAP;
    if searchable {
        list + search_field_height(metrics) + SEARCH_FIELD_GAP
    } else {
        list
    }
}

pub(crate) fn context_menu_geometry(
    bounds: LayoutBox,
    query: Option<&Arc<str>>,
    rows: &[SelectOptionData],
    highlighted: Option<usize>,
    palette: &SemanticPalette,
    metrics: ThemeMetrics,
) -> ComponentGeometry {
    let is_light = palette.background.as_rgba_array()[0] > 0.5;
    let searchable = query.is_some();
    let search_height = search_field_height(metrics);
    let search_field = searchable.then(|| LayoutBox {
        x: bounds.x + MENU_PADDING,
        y: bounds.y + MENU_PADDING,
        width: (bounds.width - MENU_PADDING * 2.0).max(0.0),
        height: search_height,
    });
    let search = search_field.map(|field| {
        let empty = query.is_none_or(|query| query.trim().is_empty());
        ComponentTextRegion {
            bounds: field,
            content: if empty {
                crate::TextValue::from("搜索操作")
            } else {
                Arc::clone(query.expect("searchable query")).into()
            },
            color: Some(if empty {
                palette.faint.as_rgba_array()
            } else {
                palette.text.as_rgba_array()
            }),
            font_size: ControlSize::Small.text_size(),
            font_weight: None,
        }
    });
    let list_top = if searchable {
        bounds.y + MENU_PADDING + search_height + SEARCH_FIELD_GAP
    } else {
        bounds.y + MENU_PADDING
    };
    let item_height = ControlSize::Small.height_in(metrics);
    let size = ControlSize::Small;
    let options = rows
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let y = list_top + index as f32 * (item_height + crate::popover::MENU_ITEM_GAP);
            let selected = highlighted == Some(index);
            let row = LayoutBox {
                x: bounds.x + MENU_PADDING,
                y,
                width: (bounds.width - MENU_PADDING * 2.0).max(0.0),
                height: item_height,
            };
            let icon_color = if option.disabled {
                palette.faint.as_rgba_array()
            } else {
                palette.muted.as_rgba_array()
            };
            let (label_x, icon) = menu_option_icon(row, option.icon, size, icon_color, metrics);
            let label_right = row.x + row.width - size.padding_x_in(metrics);
            crate::SelectOptionGeometry {
                bounds: row,
                label: ComponentTextRegion {
                    bounds: LayoutBox {
                        x: label_x,
                        y: row.y,
                        width: (label_right - label_x).max(0.0),
                        height: row.height,
                    },
                    content: crate::select::menu_option_label(option).into(),
                    color: Some(if option.disabled {
                        palette.faint.as_rgba_array()
                    } else {
                        palette.text.as_rgba_array()
                    }),
                    font_size: size.text_size(),
                    font_weight: Some(500),
                },
                selected,
                checked: option.checked,
                disabled: option.disabled,
                background: selected.then_some(palette.hover.as_rgba_array()),
                icon,
            }
        })
        .collect();
    ComponentGeometry::MenuSurface {
        trigger_surface: None,
        trigger: None,
        trigger_icon: None,
        trigger_image: None,
        surface: bounds,
        search,
        search_field,
        options,
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

pub(crate) fn context_menu_option_at(
    geometry: &ComponentGeometry,
    x: f32,
    y: f32,
) -> Option<usize> {
    let ComponentGeometry::MenuSurface { options, .. } = geometry else {
        return None;
    };
    options
        .iter()
        .position(|option| !option.disabled && option.bounds.contains(x, y))
}

fn menu_option_icon(
    row: LayoutBox,
    icon: Option<Icon>,
    size: ControlSize,
    color: [f32; 4],
    metrics: ThemeMetrics,
) -> (f32, Option<(Icon, LayoutBox, [f32; 4])>) {
    let icon_size = size.icon_size();
    let mut cursor = row.x + size.padding_x_in(metrics);
    let icon = icon.map(|icon| {
        let bounds = LayoutBox {
            x: cursor,
            y: icon_y_on_text_glyph_center(
                row.y,
                row.height,
                size.text_size(),
                Some(LineHeightSpec::Absolute(size.line_height())),
                true,
                icon_size,
            ),
            width: icon_size,
            height: icon_size,
        };
        cursor += icon_size + ICON_GAP;
        (icon, bounds, color)
    });
    (cursor, icon)
}

fn value_segments(value: &str) -> Vec<&str> {
    value
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn item_matches_query(item: &ContextMenuItem, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let query = query.to_lowercase();
    item.label.to_lowercase().contains(&query)
        || item.value.to_lowercase().contains(&query)
        || item
            .hint
            .as_ref()
            .is_some_and(|hint| hint.to_lowercase().contains(&query))
}

fn item_style(size: ControlSize) -> NodeStyle {
    NodeStyle {
        layout: Arc::new(nana_ui_core::LayoutStyle {
            width: Some(LengthSpec::Fill),
            font_size: Some(size.text_size()),
            line_height: Some(LineHeightSpec::Absolute(size.line_height())),
            ..nana_ui_core::LayoutStyle::default()
        }),
        radius: Some(nana_ui_core::RadiusTier::Sm),
        control_height: Some(nana_ui_core::ControlHeight::Exact(size)),
        control_padding_x: Some(size.into()),
        foreground: Some(SemanticColorRole::Text),
        interaction: InteractionStyle {
            hovered: SemanticPaint {
                background: Some(SemanticColorRole::Hover),
                ..SemanticPaint::default()
            },
            pressed: SemanticPaint {
                background: Some(SemanticColorRole::Active),
                ..SemanticPaint::default()
            },
            selected: SemanticPaint {
                background: Some(SemanticColorRole::Hover),
                ..SemanticPaint::default()
            },
            disabled: SemanticPaint {
                foreground: Some(SemanticColorRole::Faint),
                ..SemanticPaint::default()
            },
            ..InteractionStyle::default()
        },
        text_vertical_alignment: TextVerticalAlignment::Center,
        ..NodeStyle::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::AppContext;
    use crate::{Activate, DocumentId, StandardVisual};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    #[test]
    fn action_menu_item_projects_label_hint_and_danger() {
        let mut context = AppContext::new();
        let item = context
            .create_component(
                document(),
                ActionMenuItem::new("Delete")
                    .hint("⌫")
                    .danger(true)
                    .leading(Icon::Close),
            )
            .unwrap();
        let id = item.stable_id();
        assert!(matches!(
            context.world().standard_visual(id),
            Some(StandardVisual::ActionMenuItem { danger: true, .. })
        ));
        assert_eq!(context.world().text(id), Some("Delete"));
        assert_eq!(
            context.world().node_style(id).unwrap().foreground,
            Some(SemanticColorRole::Danger)
        );
        let a11y = context.world().accessibility(id).unwrap();
        assert_eq!(a11y.role, AccessibilityRole::MenuItem);
        assert_eq!(a11y.description.as_deref(), Some("⌫"));
    }

    #[test]
    fn disabled_action_menu_item_does_not_activate() {
        let mut context = AppContext::new();
        let item = context
            .create_component(document(), ActionMenuItem::new("Rename").disabled(true))
            .unwrap();
        let fired = Arc::new(std::sync::Mutex::new(false));
        let flag = Arc::clone(&fired);
        context
            .on(item, move |_item, _event: &Activate, _| {
                *flag.lock().unwrap() = true;
            })
            .unwrap();
        assert!(!context.activate_action_menu_item(item).unwrap());
        assert!(!*fired.lock().unwrap());
    }

    #[test]
    fn context_menu_selects_and_dismisses() {
        let mut menu = ContextMenu::new(24.0, 36.0).items([
            ContextMenuItem::new("rename", "Rename"),
            ContextMenuItem::new("delete", "Delete").danger(true),
        ]);
        assert!(menu.open);
        assert_eq!(
            menu.select_index(1),
            Some(ContextMenuEvent::Select(Arc::from("delete")))
        );
        assert!(!menu.open);
        menu.open = true;
        menu.dismiss();
        assert!(!menu.open);
        assert!(menu.active_path.is_empty());
    }

    #[test]
    fn context_menu_opens_nested_child_and_selects_leaf() {
        let mut menu = ContextMenu::new(24.0, 36.0).items([
            ContextMenuItem::new("file", "File"),
            ContextMenuItem::new("file/rename", "Rename"),
            ContextMenuItem::new("file/delete", "Delete"),
            ContextMenuItem::new("edit", "Edit"),
        ]);
        let visible: Vec<_> = menu
            .visible_items()
            .into_iter()
            .map(|item| item.value)
            .collect();
        assert_eq!(
            visible,
            [Arc::<str>::from("file"), Arc::<str>::from("edit")]
        );
        assert_eq!(menu.select_index(0), None);
        assert!(menu.open);
        assert_eq!(menu.active_path.as_slice(), &[Arc::<str>::from("file")]);
        let visible: Vec<_> = menu
            .visible_items()
            .into_iter()
            .map(|item| item.value)
            .collect();
        assert_eq!(
            visible,
            [
                Arc::<str>::from("file/rename"),
                Arc::<str>::from("file/delete")
            ]
        );
        assert_eq!(
            menu.select_index(0),
            Some(ContextMenuEvent::Select(Arc::from("file/rename")))
        );
        assert!(!menu.open);
        assert!(menu.active_path.is_empty());

        menu.open = true;
        assert_eq!(menu.select_index(0), None);
        assert!(menu.back());
        assert!(menu.active_path.is_empty());
        assert!(!menu.back());
        assert_eq!(
            menu.select_index(1),
            Some(ContextMenuEvent::Select(Arc::from("edit")))
        );
    }

    #[test]
    fn context_menu_query_hides_non_matching_items() {
        let menu = ContextMenu::new(24.0, 36.0)
            .items([
                ContextMenuItem::new("file/rename", "Rename").hint("Ctrl+R"),
                ContextMenuItem::new("file/delete", "Delete"),
                ContextMenuItem::new("edit", "Edit"),
            ])
            .searchable(true)
            .query("REN");
        let visible: Vec<_> = menu
            .visible_items()
            .into_iter()
            .map(|item| item.value)
            .collect();
        assert_eq!(visible, [Arc::<str>::from("file/rename")]);

        let by_hint = menu.clone().query("ctrl");
        assert_eq!(
            by_hint
                .visible_items()
                .into_iter()
                .map(|item| item.value)
                .collect::<Vec<_>>(),
            [Arc::<str>::from("file/rename")]
        );

        let empty = menu.clone().query("");
        let root: Vec<_> = empty
            .visible_items()
            .into_iter()
            .map(|item| item.value)
            .collect();
        assert_eq!(root, [Arc::<str>::from("file"), Arc::<str>::from("edit")]);
        assert!(matches!(
            empty.style.layout.height,
            Some(LengthSpec::Px(full)) if matches!(
                menu.style.layout.height,
                Some(LengthSpec::Px(filtered)) if filtered < full
            )
        ));
        let Some(LengthSpec::Px(searchable_height)) = menu.style.layout.height else {
            panic!("searchable menu height");
        };
        assert!(searchable_height > context_menu_height(1, false, nana_ui_core::UI_METRICS));
    }

    #[test]
    fn context_menu_projects_item_icons_into_menu_surface_rows() {
        let mut context = AppContext::new();
        let menu = context
            .create_component(
                document(),
                ContextMenu::new(8.0, 12.0).items([
                    ContextMenuItem::new("add", "Add").icon(Icon::Add),
                    ContextMenuItem::new("rename", "Rename"),
                ]),
            )
            .unwrap();
        match context.world().standard_visual(menu.stable_id()) {
            Some(StandardVisual::MenuSurface { rows, .. }) => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].icon, Some(Icon::Add));
                assert_eq!(rows[1].icon, None);
            }
            other => panic!("context menu visual: {other:?}"),
        }
    }

    #[test]
    fn context_menu_geometry_reserves_icon_box_and_keeps_iconless_rows() {
        let palette = SemanticPalette::for_mode(nana_ui_core::ThemeMode::Dark);
        let rows = [
            SelectOptionData {
                label: Arc::from("Add"),
                hint: None,
                disabled: false,
                checked: false,
                icon: Some(Icon::Add),
            },
            SelectOptionData {
                label: Arc::from("Rename"),
                hint: None,
                disabled: false,
                checked: false,
                icon: None,
            },
        ];
        let ComponentGeometry::MenuSurface { options, .. } = context_menu_geometry(
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 80.0,
            },
            None,
            &rows,
            None,
            &palette,
            nana_ui_core::UI_METRICS,
        ) else {
            panic!("context menu geometry");
        };
        assert_eq!(options.len(), 2);
        let Some((kind, icon_box, _)) = options[0].icon else {
            panic!("expected Add icon geometry");
        };
        assert_eq!(kind, Icon::Add);
        assert!(icon_box.width > 0.0 && icon_box.height > 0.0);
        assert!(options[0].label.bounds.x >= icon_box.x + icon_box.width);
        assert!(options[0].label.bounds.width > 0.0);
        assert_eq!(options[1].icon, None);
        assert!(options[1].label.bounds.width > 0.0);
        assert!(options[1].label.bounds.height > 0.0);
        assert!(options[1].label.bounds.x < options[0].label.bounds.x);
    }

    #[test]
    fn searchable_context_menu_projects_query_state_and_rows() {
        let mut context = AppContext::new();
        let menu = context
            .create_component(
                document(),
                ContextMenu::new(8.0, 12.0)
                    .items([
                        ContextMenuItem::new("rename", "Rename"),
                        ContextMenuItem::new("delete", "Delete"),
                    ])
                    .searchable(true)
                    .query("del"),
            )
            .unwrap();
        let id = menu.stable_id();
        assert_eq!(
            context.world().text_input(id).map(|state| state.value),
            Some("del")
        );
        assert!(context.world().interaction(id).unwrap().focusable);
        match context.world().standard_visual(id) {
            Some(StandardVisual::MenuSurface {
                query: Some(query),
                rows,
                ..
            }) => {
                assert_eq!(query.as_ref(), "del");
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].label.as_ref(), "Delete");
            }
            other => panic!("searchable menu visual: {other:?}"),
        }
    }

    #[test]
    fn context_menu_disabled_items_stay_unselectable() {
        let mut menu = ContextMenu::new(24.0, 36.0).items([
            ContextMenuItem::new("file", "File"),
            ContextMenuItem::new("file/rename", "Rename").disabled(true),
            ContextMenuItem::new("paste", "Paste").disabled(true),
            ContextMenuItem::new("delete", "Delete").danger(true),
        ]);
        assert_eq!(menu.select_index(1), None);
        assert!(menu.open);
        assert!(menu.active_path.is_empty());

        assert_eq!(menu.select_index(0), None);
        assert_eq!(menu.active_path.as_slice(), &[Arc::<str>::from("file")]);
        assert_eq!(menu.select_index(0), None);
        assert!(menu.open);
        assert_eq!(menu.active_path.as_slice(), &[Arc::<str>::from("file")]);
        assert!(menu.back());

        assert_eq!(
            menu.select_index(2),
            Some(ContextMenuEvent::Select(Arc::from("delete")))
        );
        assert!(!menu.open);
    }

    #[test]
    fn anchored_origin_clamps_to_the_viewport() {
        let viewport = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 160.0,
        };
        assert_eq!(
            resolve_anchored_origin(
                180.0,
                140.0,
                80.0,
                60.0,
                viewport,
                AnchoredMenuPlacement::BottomStart,
            ),
            (120.0, 100.0)
        );
    }

    #[test]
    fn hint_region_keeps_its_end_edge_and_stays_clear_of_the_label() {
        let bounds = LayoutBox {
            x: 40.0,
            y: 10.0,
            width: 220.0,
            height: 28.0,
        };
        let style = ComputedStyle::default();
        let palette = SemanticPalette::dark();
        let geometry = |hint: &Arc<str>| match action_menu_item_geometry(
            bounds,
            &Arc::from("复制到剪贴板"),
            Some(hint),
            None,
            false,
            false,
            ControlSize::Medium,
            &style,
            &palette,
            nana_ui_core::UI_METRICS,
            crate::text_width::ChromeTextMeasure::ESTIMATE,
        ) {
            ComponentGeometry::ActionMenuItem { label, hint, .. } => {
                (label, hint.expect("hint region"))
            }
            _ => panic!("action menu item geometry"),
        };
        let (short_label, short_hint) = geometry(&Arc::from("⌘C"));
        let (long_label, long_hint) = geometry(&Arc::from("Ctrl+Shift+P"));

        for (label, hint) in [(&short_label, &short_hint), (&long_label, &long_hint)] {
            let hint_width =
                crate::text_width::estimated_text_width(hint.content.as_ref(), HINT_TEXT_SIZE);
            assert!((hint.bounds.width - hint_width).abs() < 0.5);
            // End edge stays pinned to the trailing padding edge.
            let hint_end = hint.bounds.x + hint.bounds.width;
            assert!(
                (hint_end
                    - (bounds.x + bounds.width
                        - ControlSize::Medium.padding_x_in(nana_ui_core::UI_METRICS)))
                .abs()
                    < 0.5
            );
            // The label region ends before the hint region begins.
            assert!(label.bounds.x + label.bounds.width <= hint.bounds.x);
        }
        // A wider hint shifts its start further left while the end edge holds.
        assert!(long_hint.bounds.x < short_hint.bounds.x);
    }

    /// `place_in` 的视口是持久属性：筛选或进子级都会按新的行数重新锚定，
    /// 那些重新锚定同样不得把面板放到视口外面去。
    #[test]
    fn a_placed_menu_stays_inside_its_viewport_after_filtering() {
        let viewport = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 900.0,
        };
        let items = (0..40)
            .map(|index| ContextMenuItem::new(format!("item{index}"), format!("Item {index}")))
            .collect::<Vec<_>>();
        let mut menu = ContextMenu::new(1200.0, 700.0)
            .items(items)
            .searchable(true);
        menu.place_in(viewport);
        let inside = |menu: &ContextMenu| {
            let layout = &menu.style.layout;
            let (Some(LengthSpec::Px(x)), Some(LengthSpec::Px(y)), Some(LengthSpec::Px(height))) =
                (layout.offset_left, layout.offset_top, layout.height)
            else {
                panic!("anchored surface must carry a resolved box");
            };
            x >= viewport.x
                && x + menu.width <= viewport.x + viewport.width
                && y >= viewport.y
                && (height > viewport.height || y + height <= viewport.y + viewport.height)
        };
        assert!(inside(&menu), "首次放置就落在视口内");

        // 筛选后行数变少、面板变矮，重新锚定仍须留在视口内。
        menu.set_query("Item 3");
        assert!(menu.visible_items().len() < 40, "筛选缩短了列表");
        assert!(inside(&menu), "筛选后重新锚定仍在视口内");
    }
}

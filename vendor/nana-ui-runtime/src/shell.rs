use std::collections::HashSet;
use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, ControlSize, FlexDirection, Icon, JustifySpec, LengthSpec, OverflowSpec,
    PositionSpec, RegionId, SemanticColorRole, TITLE_BAR_HEIGHT, WINDOW_CONTROL_GAP,
    WINDOW_CONTROL_PADDING, WINDOW_CONTROL_WIDTH, WindowChrome, WorkspaceModel,
};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, AppContext, ComponentView, DocumentId, Entity,
    FrameworkError, IconButton, InteractionState, MutationQueue, NodeKind, NodeStyle, OverlayHost,
    SidebarFrame, StableNodeId, StandardVisual, Text, TextContent, TextHorizontalAlignment,
    TextVerticalAlignment, UiWorld, Workspace, WorkspaceRegionSlot,
};

const SLOT_PADDING: f32 = nana_ui_core::space::SM;
const CENTER_PADDING_X: f32 = nana_ui_core::space::XXL;
const DEFAULT_CENTER_WIDTH: f32 = 168.0;
/// Extra gap after the native traffic-light exclusion so leading chrome
/// (sidebar toggle) cannot sit on the caption buttons.
const NATIVE_LEADING_CLEARANCE: f32 = nana_ui_core::space::MD;
/// Width of the native window-button placeholder: the macOS traffic-light
/// cluster centered with equal margins.
const NATIVE_WINDOW_CONTROLS_WIDTH: f32 = 78.0;
const TITLE_FONT_SIZE: f32 = nana_ui_core::type_scale::BODY;
const TITLE_FONT_WEIGHT: u16 = nana_ui_core::type_scale::SEMIBOLD;
const OVERLAY_Z_INDEX: i32 = 1;
const STATUS_OVERLAY_Z_INDEX: i32 = 2;

const LEADING_COLUMN_TAG: &str = "app-title-bar-leading";
const CENTER_COLUMN_TAG: &str = "app-title-bar-center";
const TRAILING_COLUMN_TAG: &str = "app-title-bar-trailing";

/// Typed window command. Runtime never touches a platform window handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowChromeAction {
    Minimize,
    ToggleMaximize,
    Close,
}

impl WindowChromeAction {
    pub const ALL: [Self; 3] = [Self::Minimize, Self::ToggleMaximize, Self::Close];

    pub fn icon(self, maximized: bool) -> Icon {
        match self {
            Self::Minimize => Icon::Minimize,
            Self::ToggleMaximize if maximized => Icon::Restore,
            Self::ToggleMaximize => Icon::Maximize,
            Self::Close => Icon::Close,
        }
    }

    pub fn label(self, maximized: bool) -> &'static str {
        match self {
            Self::Minimize => "Minimize",
            Self::ToggleMaximize if maximized => "Restore",
            Self::ToggleMaximize => "Maximize",
            Self::Close => "Close",
        }
    }
}

/// 36px application title bar. Leading / center / trailing / controls are host-mounted.
#[derive(Debug, Clone, PartialEq)]
pub struct AppTitleBar {
    pub title: Arc<str>,
    pub transparent: bool,
    pub drag_enabled: bool,
    pub leading: Option<StableNodeId>,
    pub center: Option<StableNodeId>,
    pub trailing: Option<StableNodeId>,
    pub controls: Option<StableNodeId>,
    pub center_width: f32,
    /// Window buttons are the platform's native ones (macOS traffic lights):
    /// the controls slot is an empty leading placeholder the host moves them
    /// onto. Otherwise it holds the custom trailing buttons.
    pub native_controls: bool,
    pub show_window_controls: bool,
    pub maximized: bool,
    pub style: NodeStyle,
}

impl AppTitleBar {
    pub fn new(title: impl Into<Arc<str>>) -> Self {
        Self {
            title: title.into(),
            transparent: false,
            drag_enabled: true,
            leading: None,
            center: None,
            trailing: None,
            controls: None,
            center_width: DEFAULT_CENTER_WIDTH,
            native_controls: !WindowChrome::platform_default().uses_custom_controls(),
            show_window_controls: true,
            maximized: false,
            style: NodeStyle::default(),
        }
    }

    /// Removes only the title-bar background, preserving foreground and slot content.
    pub fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    /// Enables native window dragging from the bar's non-interactive content.
    pub fn drag_enabled(mut self, enabled: bool) -> Self {
        self.drag_enabled = enabled;
        self
    }

    pub fn leading(mut self, leading: StableNodeId) -> Self {
        self.leading = Some(leading);
        self
    }

    pub fn center(mut self, center: StableNodeId) -> Self {
        self.center = Some(center);
        self
    }

    pub fn trailing(mut self, trailing: StableNodeId) -> Self {
        self.trailing = Some(trailing);
        self
    }

    pub fn controls(mut self, controls: StableNodeId) -> Self {
        self.controls = Some(controls);
        self
    }

    pub fn center_width(mut self, width: f32) -> Self {
        self.center_width = finite_positive(width, DEFAULT_CENTER_WIDTH).max(1.0);
        self
    }

    pub fn native_controls(mut self, native: bool) -> Self {
        self.native_controls = native;
        self
    }

    pub fn show_window_controls(mut self, show: bool) -> Self {
        self.show_window_controls = show;
        self
    }

    pub fn maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn resolved_center_width(&self) -> f32 {
        finite_positive(self.center_width, DEFAULT_CENTER_WIDTH).max(1.0)
    }

    /// Whether the native placeholder leads the bar.
    fn leads_with_native_controls(&self) -> bool {
        self.native_controls && self.show_window_controls
    }

    /// An unassembled bar has no placeholder, so it keeps the native band
    /// clear with padding instead.
    fn chrome_padding_left(&self) -> f32 {
        if self.leads_with_native_controls() && self.controls.is_none() {
            NATIVE_WINDOW_CONTROLS_WIDTH + NATIVE_LEADING_CLEARANCE
        } else {
            0.0
        }
    }

    fn effective_style(&self, world: &UiWorld, id: StableNodeId) -> NodeStyle {
        let columns = title_bar_has_columns(world, id);
        let mut style = self.style.clone();
        style.foreground = Some(SemanticColorRole::Text);
        style.background = (!self.transparent).then_some(SemanticColorRole::Titlebar);
        style.text_horizontal_alignment = TextHorizontalAlignment::Center;
        style.text_vertical_alignment = TextVerticalAlignment::Center;
        let layout = Arc::make_mut(&mut style.layout);
        layout.background = None;
        layout.direction = Some(FlexDirection::Row);
        layout.align_items = AlignSpec::Center;
        layout.justify_content = JustifySpec::Start;
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Px(TITLE_BAR_HEIGHT));
        layout.min_height = Some(LengthSpec::Px(TITLE_BAR_HEIGHT));
        layout.max_height = Some(LengthSpec::Px(TITLE_BAR_HEIGHT));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        layout.font_size = Some(TITLE_FONT_SIZE);
        layout.font_weight = Some(TITLE_FONT_WEIGHT);
        if columns {
            layout.padding_left = Some(LengthSpec::Px(0.0));
            layout.padding_right = Some(LengthSpec::Px(0.0));
        } else {
            layout.padding_left = Some(LengthSpec::Px(self.chrome_padding_left()));
            layout.padding_right = Some(LengthSpec::Px(0.0));
        }
        layout.overflow_x = OverflowSpec::Hidden;
        style
    }

    fn project_slots(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let children = world.node(id).map(|node| node.children).unwrap_or_default();
        let center_has_content = children.iter().any(|&child| {
            node_tag(world, child).as_deref() == Some(CENTER_COLUMN_TAG)
                && world.node(child).is_some_and(|slot| {
                    slot.children.iter().any(|&content| {
                        world.node(content).is_some_and(|node| {
                            !matches!(node.kind, NodeKind::Text)
                                || world.text(content).is_some_and(|text| !text.is_empty())
                        })
                    })
                })
        });
        let content_only = self.title.is_empty() && self.center.is_none() && !center_has_content;
        let mut saw_columns = false;
        for child in children {
            match node_tag(world, child).as_deref() {
                Some(LEADING_COLUMN_TAG) => {
                    saw_columns = true;
                    patch_layout(world, mutations, child, |layout| {
                        apply_fill_column(layout, JustifySpec::Start, false);
                        layout.overflow_x = OverflowSpec::Hidden;
                        // The native placeholder sits flush with the bar edge
                        // and carries its own clearance.
                        layout.padding_left = Some(LengthSpec::Px(
                            if self.leads_with_native_controls() && self.controls.is_some() {
                                0.0
                            } else {
                                SLOT_PADDING + self.chrome_padding_left()
                            },
                        ));
                        layout.padding_right = Some(LengthSpec::Px(SLOT_PADDING));
                        layout.padding_top = Some(LengthSpec::Px(0.0));
                        layout.padding_bottom = Some(LengthSpec::Px(0.0));
                    });
                }
                Some(CENTER_COLUMN_TAG) => {
                    saw_columns = true;
                    patch_layout(world, mutations, child, |layout| {
                        apply_center_column(layout, self.resolved_center_width());
                        layout.hidden = content_only;
                    });
                }
                Some(TRAILING_COLUMN_TAG) => {
                    saw_columns = true;
                    patch_layout(world, mutations, child, |layout| {
                        if content_only {
                            apply_hug_slot(layout, AlignSpec::Center, JustifySpec::End);
                            layout.hidden = false;
                        } else {
                            apply_fill_column(layout, JustifySpec::End, true);
                        }
                        layout.padding_left = Some(LengthSpec::Px(SLOT_PADDING));
                        // Custom controls hug the window edge; native or
                        // absent controls keep the shared slot padding.
                        layout.padding_right = Some(LengthSpec::Px(
                            if self.show_window_controls && !self.native_controls {
                                0.0
                            } else {
                                SLOT_PADDING
                            },
                        ));
                        layout.padding_top = Some(LengthSpec::Px(0.0));
                        layout.padding_bottom = Some(LengthSpec::Px(0.0));
                    });
                }
                _ => {}
            }
        }
        if !saw_columns {
            // No-slot bars keep the legacy direct-child slots. The patches
            // must only touch nodes that are currently children of this bar:
            // before `assemble` runs, a mounted center (e.g. a `Breadcrumb`
            // component) is still detached, and stamping the column style on
            // it would clobber the component's own layout for good.
            let is_bar_child = |node: StableNodeId| world.parent_id(node) == Some(id);
            if let Some(leading) = self.leading.filter(|&n| is_bar_child(n)) {
                patch_layout(world, mutations, leading, |layout| {
                    apply_hug_slot(layout, AlignSpec::Center, JustifySpec::Start);
                    layout.padding_left = Some(LengthSpec::Px(SLOT_PADDING));
                    layout.padding_right = Some(LengthSpec::Px(SLOT_PADDING));
                    layout.padding_top = Some(LengthSpec::Px(0.0));
                    layout.padding_bottom = Some(LengthSpec::Px(0.0));
                });
            }
            if let Some(center) = self.center.filter(|&n| is_bar_child(n)) {
                patch_layout(world, mutations, center, |layout| {
                    apply_center_column(layout, self.resolved_center_width());
                });
            }
            if let Some(trailing) = self.trailing.filter(|&n| is_bar_child(n)) {
                patch_layout(world, mutations, trailing, |layout| {
                    apply_hug_slot(layout, AlignSpec::Center, JustifySpec::End);
                    layout.padding_left = Some(LengthSpec::Px(SLOT_PADDING));
                    layout.padding_right = Some(LengthSpec::Px(SLOT_PADDING));
                    layout.padding_top = Some(LengthSpec::Px(0.0));
                    layout.padding_bottom = Some(LengthSpec::Px(0.0));
                });
            }
        }
        self.project_controls(world, mutations);
    }

    fn project_controls(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        let Some(controls) = self.controls else {
            return;
        };
        if !self.show_window_controls {
            patch_layout(world, mutations, controls, |layout| {
                layout.hidden = true;
            });
            return;
        }
        AppTitleBarControls::new(self.maximized)
            .native(self.native_controls)
            .project(controls, world, mutations);
    }
}

impl ComponentView for AppTitleBar {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        slot_assembler: Some(AppContext::assemble_app_title_bar),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    /// Lays out the leading, center and trailing slot nodes the application owns.
    const ALWAYS_REPROJECT: bool = true;

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "app-title-bar".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let root_text = if self.center.is_some() || title_bar_has_columns(world, id) {
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
            &self.effective_style(world, id),
            InteractionState {
                pointer_events: true,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::clone(&self.title)),
                ..AccessibilityState::default()
            },
        );
        self.project_slots(id, world, mutations);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleBarColumn {
    Leading,
    Center,
    Trailing,
}

impl TitleBarColumn {
    fn tag(self) -> &'static str {
        match self {
            Self::Leading => LEADING_COLUMN_TAG,
            Self::Center => CENTER_COLUMN_TAG,
            Self::Trailing => TRAILING_COLUMN_TAG,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct AppTitleBarSlot {
    column: TitleBarColumn,
}

impl ComponentView for AppTitleBarSlot {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: self.column.tag().into(),
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
            &NodeStyle::default(),
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
}

/// The window's system buttons. Custom controls are host-mounted Minimize /
/// Maximize-or-Restore / Close icons; native controls are an empty placeholder
/// the size of the platform's own buttons, which the window host moves onto
/// the placeholder's layout box.
///
/// A title bar's controls slot is the usual home for the placeholder, and
/// `assemble_app_title_bar` puts it there. It is an ordinary component
/// though: mounted anywhere in a window's document, it still says where that
/// window's native buttons belong.
#[derive(Debug, Clone, PartialEq)]
pub struct AppTitleBarControls {
    pub maximized: bool,
    pub native: bool,
    pub minimize: Option<StableNodeId>,
    pub maximize: Option<StableNodeId>,
    pub close: Option<StableNodeId>,
    pub style: NodeStyle,
}

impl AppTitleBarControls {
    pub fn new(maximized: bool) -> Self {
        Self {
            maximized,
            native: !WindowChrome::platform_default().uses_custom_controls(),
            minimize: None,
            maximize: None,
            close: None,
            style: NodeStyle::default(),
        }
    }

    pub fn native(mut self, native: bool) -> Self {
        self.native = native;
        self
    }

    pub fn minimize(mut self, minimize: StableNodeId) -> Self {
        self.minimize = Some(minimize);
        self
    }

    pub fn maximize(mut self, maximize: StableNodeId) -> Self {
        self.maximize = Some(maximize);
        self
    }

    pub fn close(mut self, close: StableNodeId) -> Self {
        self.close = Some(close);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        if self.native {
            let width = NATIVE_WINDOW_CONTROLS_WIDTH;
            layout.width = Some(LengthSpec::Px(width));
            layout.min_width = Some(LengthSpec::Px(width));
            layout.max_width = Some(LengthSpec::Px(width));
            layout.height = Some(LengthSpec::Fill);
            layout.padding_left = Some(LengthSpec::Px(0.0));
            layout.padding_right = Some(LengthSpec::Px(0.0));
            layout.margin_right = Some(LengthSpec::Px(SLOT_PADDING + NATIVE_LEADING_CLEARANCE));
            layout.flex_grow = Some(0.0);
            layout.flex_shrink = Some(0.0);
            layout.hidden = false;
            return style;
        }
        layout.direction = Some(FlexDirection::Row);
        layout.align_items = AlignSpec::Center;
        layout.justify_content = JustifySpec::End;
        layout.gap = Some(layout.gap.unwrap_or(LengthSpec::Px(WINDOW_CONTROL_GAP)));
        layout.padding_left = Some(LengthSpec::Px(WINDOW_CONTROL_PADDING));
        layout.padding_right = Some(LengthSpec::Px(WINDOW_CONTROL_PADDING));
        layout.height = Some(LengthSpec::Fill);
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        layout.hidden = false;
        style
    }

    fn control_ids(&self, world: &UiWorld, id: StableNodeId) -> [Option<StableNodeId>; 3] {
        let explicit = [self.minimize, self.maximize, self.close];
        if explicit.iter().any(Option::is_some) {
            return explicit;
        }
        let children = world.node(id).map(|node| node.children).unwrap_or_default();
        [
            children.first().copied(),
            children.get(1).copied(),
            children.get(2).copied(),
        ]
    }
}

impl Default for AppTitleBarControls {
    fn default() -> Self {
        Self::new(false)
    }
}

impl ComponentView for AppTitleBarControls {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "app-title-bar-controls".into(),
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
            // The native placeholder takes the pointer so the title bar never
            // starts a drag over the buttons it stands for.
            InteractionState {
                pointer_events: self.native,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                ..AccessibilityState::default()
            },
        );
        if self.native {
            return;
        }
        for (action, child) in WindowChromeAction::ALL
            .into_iter()
            .zip(self.control_ids(world, id))
        {
            let Some(child) = child else {
                continue;
            };
            project_window_control(child, action, self.maximized, world, mutations);
        }
    }
}

/// Title bar + fill body + optional overlay stack sibling.
#[derive(Debug, Clone, PartialEq)]
pub struct AppShell {
    pub title_bar: Option<StableNodeId>,
    pub body: Option<StableNodeId>,
    pub overlay: Option<StableNodeId>,
    pub style: NodeStyle,
}

impl AppShell {
    pub fn new() -> Self {
        Self {
            title_bar: None,
            body: None,
            overlay: None,
            style: NodeStyle::default(),
        }
    }

    pub fn title_bar(mut self, title_bar: StableNodeId) -> Self {
        self.title_bar = Some(title_bar);
        self
    }

    pub fn body(mut self, body: StableNodeId) -> Self {
        self.body = Some(body);
        self
    }

    pub fn overlay(mut self, overlay: StableNodeId) -> Self {
        self.overlay = Some(overlay);
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
        layout.background = None;
        layout.direction = Some(FlexDirection::Column);
        layout.align_items = AlignSpec::Stretch;
        layout.justify_content = JustifySpec::Start;
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Fill);
        layout.position = PositionSpec::Relative;
        style
    }

    fn project_slots(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        if let Some(title_bar) = self.title_bar {
            patch_layout(world, mutations, title_bar, |layout| {
                layout.width = Some(LengthSpec::Fill);
                layout.height = Some(LengthSpec::Px(TITLE_BAR_HEIGHT));
                layout.min_height = Some(LengthSpec::Px(TITLE_BAR_HEIGHT));
                layout.flex_grow = Some(0.0);
                layout.flex_shrink = Some(0.0);
            });
        }
        if let Some(body) = self.body {
            patch_layout(world, mutations, body, |layout| {
                layout.width = Some(LengthSpec::Fill);
                layout.height = Some(LengthSpec::Fill);
                layout.min_height = Some(LengthSpec::Px(0.0));
                layout.flex_grow = Some(1.0);
                layout.flex_shrink = Some(1.0);
                layout.position = PositionSpec::Static;
            });
        }
        if let Some(overlay) = self.overlay {
            patch_layout(world, mutations, overlay, |layout| {
                layout.position = PositionSpec::Absolute;
                layout.offset_top = Some(LengthSpec::Px(0.0));
                layout.offset_right = Some(LengthSpec::Px(0.0));
                layout.offset_bottom = Some(LengthSpec::Px(0.0));
                layout.offset_left = Some(LengthSpec::Px(0.0));
                layout.width = Some(LengthSpec::Fill);
                layout.height = Some(LengthSpec::Fill);
                layout.flex_grow = Some(0.0);
                layout.flex_shrink = Some(0.0);
                layout.z_index = Some(layout.z_index.unwrap_or(OVERLAY_Z_INDEX));
            });
            // An `OverlayHost` decides this from the active overlay's kind, and
            // that activation changes without re-projecting the shell, so the
            // host has to own the field. Any other node the caller supplied as
            // the overlay region keeps the content-presence rule.
            if world.overlay_host(overlay).is_none() {
                let has_content = world
                    .node(overlay)
                    .is_some_and(|node| !node.children.is_empty());
                let interaction = InteractionState {
                    pointer_events: has_content,
                    focusable: false,
                };
                if world.interaction(overlay) != Some(interaction) {
                    mutations.set_interaction(overlay, interaction);
                }
            }
        }
    }
}

impl Default for AppShell {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentView for AppShell {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        slot_assembler: Some(AppContext::assemble_app_shell),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    /// Patches the title bar, body and overlay nodes, which other components own and project.
    const ALWAYS_REPROJECT: bool = true;

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "app-shell".into(),
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
        self.project_slots(world, mutations);
    }
}

/// Page-level desktop composer.
///
/// Wires host-mounted [`AppTitleBar`], [`Workspace`], [`AppShell`] layout, and
/// [`OverlayHost`]. Application content stays in the supplied region slots.
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopShell {
    pub title_bar: Option<StableNodeId>,
    pub workspace: Option<StableNodeId>,
    pub overlay: Option<StableNodeId>,
    pub status: Option<StableNodeId>,
    pub primary: Option<StableNodeId>,
    pub navigation: Option<StableNodeId>,
    pub navigation_footer: Option<StableNodeId>,
    pub inspector: Option<StableNodeId>,
    pub bottom: Option<StableNodeId>,
    pub extra_regions: Vec<(RegionId, StableNodeId)>,
    pub overlays: Vec<StableNodeId>,
    pub status_overlays: Vec<StableNodeId>,
    pub navigation_frame: Option<StableNodeId>,
    pub title: Option<Arc<str>>,
    pub title_leading: Option<StableNodeId>,
    pub title_center: Option<StableNodeId>,
    pub title_trailing: Option<StableNodeId>,
    pub title_center_width: Option<f32>,
    pub title_window_controls: Option<bool>,
    /// Whether the workspace rounds the corners of its main region
    /// ([`crate::Workspace::workspace_corners`]); the shell hands it to the
    /// workspace it assembles. On by default.
    pub workspace_corners: bool,
    pub model: WorkspaceModel,
    pub style: NodeStyle,
}

impl DesktopShell {
    pub fn new() -> Self {
        Self {
            title_bar: None,
            workspace: None,
            overlay: None,
            status: None,
            primary: None,
            navigation: None,
            navigation_footer: None,
            inspector: None,
            bottom: None,
            extra_regions: Vec::new(),
            overlays: Vec::new(),
            status_overlays: Vec::new(),
            navigation_frame: None,
            title: None,
            title_leading: None,
            title_center: None,
            title_trailing: None,
            title_center_width: None,
            title_window_controls: None,
            workspace_corners: true,
            model: WorkspaceModel::new(),
            style: NodeStyle::default(),
        }
    }

    pub fn from_model(model: WorkspaceModel) -> Self {
        Self {
            model,
            ..Self::new()
        }
    }

    pub fn title_bar(mut self, title_bar: StableNodeId) -> Self {
        self.title_bar = Some(title_bar);
        self
    }

    /// Create an [`AppTitleBar`] on assemble when no host-mounted bar is set.
    pub fn title(mut self, title: impl Into<Arc<str>>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn title_leading(mut self, leading: StableNodeId) -> Self {
        self.title_leading = Some(leading);
        self
    }

    pub fn title_center(mut self, center: StableNodeId) -> Self {
        self.title_center = Some(center);
        self
    }

    pub fn title_trailing(mut self, trailing: StableNodeId) -> Self {
        self.title_trailing = Some(trailing);
        self
    }

    /// Width of the centred title column when the shell creates the title bar.
    ///
    /// The default suits a short product title; a breadcrumb needs more room or
    /// it wraps and clips inside the fixed column.
    pub fn title_center_width(mut self, width: f32) -> Self {
        self.title_center_width = Some(width);
        self
    }

    /// Whether the shell-created title bar mounts its own window controls.
    ///
    /// The mounted controls are presentation only. Hosts that bind their own
    /// minimize / maximize / close buttons pass `false` to avoid a second,
    /// inert set of buttons.
    pub fn title_window_controls(mut self, show: bool) -> Self {
        self.title_window_controls = Some(show);
        self
    }

    /// Round the corners of the workspace's main region, or square them
    /// (the Appearance "workspace corners" setting).
    pub fn workspace_corners(mut self, rounded: bool) -> Self {
        self.workspace_corners = rounded;
        self
    }

    pub fn primary(mut self, primary: StableNodeId) -> Self {
        self.primary = Some(primary);
        self
    }

    pub fn navigation(mut self, navigation: StableNodeId) -> Self {
        self.navigation = Some(navigation);
        self
    }

    pub fn navigation_footer(mut self, footer: StableNodeId) -> Self {
        self.navigation_footer = Some(footer);
        self
    }

    /// Right column below a Primary-scoped toolbar. Inspector tabs belong here,
    /// not on [`RegionId::PrimaryToolbar`].
    pub fn inspector(mut self, inspector: StableNodeId) -> Self {
        self.inspector = Some(inspector);
        self
    }

    pub fn bottom(mut self, bottom: StableNodeId) -> Self {
        self.bottom = Some(bottom);
        self
    }

    /// Extra region content. For [`RegionId::PrimaryToolbar`], pass only
    /// document / edit / kind actions — inspector tabs belong in [`Self::inspector`].
    pub fn region(mut self, id: RegionId, content: StableNodeId) -> Self {
        if let Some((_, existing)) = self
            .extra_regions
            .iter_mut()
            .find(|(region, _)| *region == id)
        {
            *existing = content;
        } else {
            self.extra_regions.push((id, content));
        }
        self
    }

    /// Push a modal overlay child onto the assembled dialog [`OverlayHost`].
    pub fn overlay(mut self, overlay: StableNodeId) -> Self {
        self.overlays.push(overlay);
        self
    }

    /// Push a status overlay (toast). It uses a second host so a confirm
    /// dialog can stay open.
    pub fn status(mut self, overlay: StableNodeId) -> Self {
        self.status_overlays.push(overlay);
        self
    }

    pub fn workspace_model(mut self, model: WorkspaceModel) -> Self {
        self.model = model;
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
}

impl Default for DesktopShell {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentView for DesktopShell {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        slot_assembler: Some(AppContext::assemble_desktop_shell),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    /// Patches the body and status nodes, which other components own and project.
    const ALWAYS_REPROJECT: bool = true;

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "desktop-shell".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        AppShell {
            title_bar: self.title_bar,
            body: self.workspace,
            overlay: self.overlay,
            style: self.style.clone(),
        }
        .project(id, world, mutations);
        if let Some(status) = self.status {
            patch_layout(world, mutations, status, |layout| {
                layout.position = PositionSpec::Absolute;
                layout.offset_top = Some(LengthSpec::Px(0.0));
                layout.offset_right = Some(LengthSpec::Px(0.0));
                layout.offset_bottom = Some(LengthSpec::Px(0.0));
                layout.offset_left = Some(LengthSpec::Px(0.0));
                layout.width = Some(LengthSpec::Fill);
                layout.height = Some(LengthSpec::Fill);
                layout.flex_grow = Some(0.0);
                layout.flex_shrink = Some(0.0);
                layout.z_index = Some(layout.z_index.unwrap_or(STATUS_OVERLAY_Z_INDEX));
            });
        }
    }
}

impl AppContext {
    /// Reconcile title bar, body, and optional overlay, then re-project slots.
    ///
    /// Host-mounted slots are kept. A title bar or overlay node is created
    /// only when that slot was already assigned or already present as a child.
    pub fn assemble_app_shell(&mut self, shell: Entity<AppShell>) -> Result<bool, FrameworkError> {
        let parent = shell.stable_id();
        let document = document_of(self, parent)?;
        let snapshot = self.read(shell, Clone::clone)?;
        let title_bar = resolve_app_title_bar(self, document, parent, &snapshot)?;
        let mut changed = false;
        // Assemble the title bar before resolving body so nested shell slots
        // are reparented out of leading extras and become siblings.
        if let Some(title_bar) = title_bar {
            changed |= self
                .assemble_app_title_bar(Entity::<AppTitleBar>::from_stable_id(title_bar))
                .unwrap_or(false);
        }
        let overlay = resolve_app_overlay(self, document, parent, &snapshot)?;
        let body = snapshot
            .body
            .filter(|id| {
                self.world().contains(*id) && Some(*id) != title_bar && Some(*id) != overlay
            })
            .or_else(|| find_app_shell_body_child(self, parent, title_bar, overlay));
        let fields_changed =
            title_bar != snapshot.title_bar || body != snapshot.body || overlay != snapshot.overlay;
        if fields_changed {
            self.update_component(shell, |shell, _| {
                shell.title_bar = title_bar;
                shell.body = body;
                shell.overlay = overlay;
            })?;
        }
        let children = app_shell_child_ids(title_bar, body, overlay);
        changed |= reconcile_ids(self, parent, &children)?;
        self.reproject_component(shell)?;
        if let Some(title_bar) = title_bar {
            changed |= self
                .assemble_app_title_bar(Entity::<AppTitleBar>::from_stable_id(title_bar))
                .unwrap_or(false);
            // Title-bar assemble must not swallow body/overlay; keep them
            // stacked under the shell.
            changed |= reconcile_ids(self, parent, &children)?;
        }
        Ok(changed || fields_changed)
    }

    /// Restore the three-column title bar and mount window controls.
    ///
    /// Leading and trailing columns fill leftover width; the center column is a
    /// fixed title slot. Custom Minimize / Maximize / Close buttons live in the
    /// trailing column; the native window-button placeholder leads the leading
    /// column. Host-mounted slots and Vue extras are reparented, not recreated.
    pub fn assemble_app_title_bar(
        &mut self,
        bar: Entity<AppTitleBar>,
    ) -> Result<bool, FrameworkError> {
        let parent = bar.stable_id();
        let document = document_of(self, parent)?;
        let snapshot = self.read(bar, Clone::clone)?;
        let leading_slot = ensure_title_bar_slot(self, document, parent, TitleBarColumn::Leading)?;
        let center_slot = ensure_title_bar_slot(self, document, parent, TitleBarColumn::Center)?;
        let trailing_slot =
            ensure_title_bar_slot(self, document, parent, TitleBarColumn::Trailing)?;

        let mut controls = snapshot
            .controls
            .filter(|id| self.world().contains(*id))
            .or_else(|| find_title_bar_controls_child(self, parent));
        let mut changed = false;
        // A native placeholder is mounted even while hidden, so showing the
        // controls later still has a box for the platform buttons to follow.
        if snapshot.show_window_controls || snapshot.native_controls {
            let mounted = ensure_window_controls(
                self,
                document,
                parent,
                controls,
                snapshot.maximized,
                snapshot.native_controls,
            )?;
            changed |= controls != Some(mounted);
            controls = Some(mounted);
            if changed {
                self.update_component(bar, |bar, _| {
                    bar.controls = Some(mounted);
                })?;
            }
        }

        let owned = [
            snapshot.leading,
            snapshot.center,
            snapshot.trailing,
            controls,
            Some(leading_slot),
            Some(center_slot),
            Some(trailing_slot),
        ];
        let extras = unclassified_title_bar_children(self, parent, &owned);
        let shell_parent = app_shell_parent(self, parent);
        let (shell_body, shell_overlay) = shell_parent
            .and_then(|id| {
                self.read(Entity::<AppShell>::from_stable_id(id), |shell| {
                    (shell.body, shell.overlay)
                })
                .ok()
            })
            .unwrap_or((None, None));

        let controls = controls.filter(|id| self.world().contains(*id));
        let mut leading_children = Vec::new();
        if let Some(controls) = controls.filter(|_| snapshot.native_controls) {
            leading_children.push(controls);
        }
        if let Some(leading) = snapshot.leading.filter(|id| self.world().contains(*id)) {
            leading_children.push(leading);
        }
        let mut center_children = Vec::new();
        if let Some(center) = snapshot.center.filter(|id| self.world().contains(*id)) {
            center_children.push(center);
        }
        let mut trailing_children = Vec::new();
        if let Some(trailing) = snapshot.trailing.filter(|id| self.world().contains(*id)) {
            trailing_children.push(trailing);
        }
        if let Some(controls) = controls.filter(|_| !snapshot.native_controls) {
            trailing_children.push(controls);
        }
        let mut reserved_shell_children = Vec::new();
        for extra in extras {
            if is_reserved_shell_slot(extra, shell_body, shell_overlay)
                || (shell_parent.is_some() && view_is::<OverlayHost>(self, extra))
                || (shell_parent.is_some()
                    && shell_body.is_none()
                    && !is_title_label_node(self, extra)
                    && !is_title_bar_chrome(self, extra))
            {
                reserved_shell_children.push(extra);
            } else if center_children.is_empty() && is_title_label_node(self, extra) {
                center_children.push(extra);
            } else {
                leading_children.push(extra);
            }
        }
        if let Some(shell) = shell_parent
            && !reserved_shell_children.is_empty()
        {
            let mut mutations = MutationQueue::new();
            for id in &reserved_shell_children {
                mutations.insert(shell, *id, None);
            }
            self.commit_mutations(mutations)?;
            changed = true;
        }
        if snapshot.center.is_none() && center_children.is_empty() {
            center_children.push(ensure_title_label(
                self,
                document,
                center_slot,
                snapshot.title.as_ref(),
            )?);
        }

        changed |= reconcile_ids(self, leading_slot, &leading_children)?;
        changed |= reconcile_ids(self, center_slot, &center_children)?;
        changed |= reconcile_ids(self, trailing_slot, &trailing_children)?;
        changed |= reconcile_ids(self, parent, &[leading_slot, center_slot, trailing_slot])?;
        self.reproject_component(bar)?;
        Ok(changed)
    }

    /// Replace the primary and inspector region content on an assembled shell.
    ///
    /// Chrome is reused. Previous region content is parked, not destroyed.
    /// Unchanged slots return without assembling.
    pub fn set_desktop_slots(
        &mut self,
        shell: Entity<DesktopShell>,
        primary: Option<StableNodeId>,
        inspector: Option<StableNodeId>,
    ) -> Result<bool, FrameworkError> {
        let changed = self.read(shell, |desktop| {
            desktop.primary != primary || desktop.inspector != inspector
        })?;
        if !changed {
            return Ok(false);
        }
        self.update_component(shell, |desktop, _| {
            desktop.primary = primary;
            desktop.inspector = inspector;
        })?;
        self.assemble_desktop_shell(shell)
    }

    /// Mount title bar, workspace regions, and overlay host on `shell`.
    ///
    /// Created chrome is reused on the next call. Host content nodes are
    /// reparented, not recreated. Floating dock windows are out of scope.
    pub fn assemble_desktop_shell(
        &mut self,
        shell: Entity<DesktopShell>,
    ) -> Result<bool, FrameworkError> {
        let document = document_of(self, shell.stable_id())?;
        let snapshot = self.read(shell, Clone::clone)?;
        let previous_slots = snapshot
            .workspace
            .filter(|id| view_is::<Workspace>(self, *id))
            .map(|id| {
                self.read(Entity::<Workspace>::from_stable_id(id), |workspace| {
                    workspace.slots.clone()
                })
            })
            .transpose()?
            .unwrap_or_default();
        let previous_used = used_ids(&snapshot, &previous_slots);

        let workspace_id = ensure_workspace(self, document, snapshot.workspace)?;
        let overlay_id = ensure_overlay_host(self, document, snapshot.overlay)?;
        let status_id = ensure_overlay_host(
            self,
            document,
            snapshot.status.filter(|id| *id != overlay_id),
        )?;
        let title_bar = ensure_title_bar(
            self,
            document,
            snapshot.title_bar,
            snapshot.title.as_ref(),
            snapshot.title_leading,
            snapshot.title_center,
            snapshot.title_trailing,
            snapshot.title_center_width,
            snapshot.title_window_controls,
        )?;
        if let Some(title_bar) = title_bar {
            let _ = self.assemble_app_title_bar(Entity::<AppTitleBar>::from_stable_id(title_bar));
        }
        let (resources, navigation_frame) = resolve_navigation(
            self,
            document,
            snapshot.navigation,
            snapshot.navigation_footer,
            snapshot.navigation_frame,
        )?;

        let chrome_changed = snapshot.title_bar != title_bar
            || snapshot.workspace != Some(workspace_id)
            || snapshot.overlay != Some(overlay_id)
            || snapshot.status != Some(status_id)
            || snapshot.navigation_frame != navigation_frame;
        let mut assembled = snapshot;
        assembled.title_bar = title_bar;
        assembled.workspace = Some(workspace_id);
        assembled.overlay = Some(overlay_id);
        assembled.status = Some(status_id);
        assembled.navigation_frame = navigation_frame;
        let slots = region_slots(self, &assembled, resources);
        let next_used = used_ids(&assembled, &slots);
        park_unused(self, shell.stable_id(), &previous_used, &next_used)?;
        if chrome_changed {
            self.update_component(shell, |desktop, _| {
                desktop.title_bar = title_bar;
                desktop.workspace = Some(workspace_id);
                desktop.overlay = Some(overlay_id);
                desktop.status = Some(status_id);
                desktop.navigation_frame = navigation_frame;
            })?;
        }
        let mut shell_children = Vec::new();
        if let Some(title_bar) = title_bar {
            shell_children.push(title_bar);
        }
        shell_children.push(workspace_id);
        shell_children.push(overlay_id);
        shell_children.push(status_id);
        let mut changed = reconcile_ids(self, shell.stable_id(), &shell_children)?;

        let workspace = Entity::<Workspace>::from_stable_id(workspace_id);
        let slots_changed = self.read(workspace, |workspace| workspace.slots != slots)?;
        let corners = assembled.workspace_corners;
        self.update_component(workspace, |workspace, _| {
            workspace.refresh_from_model(&assembled.model);
            workspace.slots = slots;
            workspace.workspace_corners = corners;
        })?;
        changed |= self.assemble_workspace(workspace)?;
        changed |= reconcile_ids(self, overlay_id, &assembled.overlays)?;
        changed |= reconcile_ids(self, status_id, &assembled.status_overlays)?;
        if changed || chrome_changed {
            self.reproject_component(shell)?;
        }
        Ok(changed || chrome_changed || slots_changed)
    }
}

fn document_of(context: &AppContext, id: StableNodeId) -> Result<DocumentId, FrameworkError> {
    context
        .world()
        .node(id)
        .map(|node| node.document)
        .ok_or(FrameworkError::MissingView(id))
}

fn view_is<C: ComponentView>(context: &AppContext, id: StableNodeId) -> bool {
    context
        .read(Entity::<C>::from_stable_id(id), |_| ())
        .is_ok()
}

fn resolve_app_title_bar(
    context: &mut AppContext,
    document: DocumentId,
    parent: StableNodeId,
    snapshot: &AppShell,
) -> Result<Option<StableNodeId>, FrameworkError> {
    if let Some(id) = snapshot
        .title_bar
        .filter(|id| context.world().contains(*id))
    {
        return Ok(Some(id));
    }
    if let Some(id) = find_title_bar_child(context, parent) {
        return Ok(Some(id));
    }
    if snapshot.title_bar.is_none() {
        return Ok(None);
    }
    let title = recovered_title(context, parent).unwrap_or_else(|| Arc::from(""));
    Ok(Some(
        context
            .create_detached_component(document, AppTitleBar::new(title))?
            .stable_id(),
    ))
}

fn resolve_app_overlay(
    context: &mut AppContext,
    document: DocumentId,
    parent: StableNodeId,
    snapshot: &AppShell,
) -> Result<Option<StableNodeId>, FrameworkError> {
    if let Some(id) = snapshot.overlay.filter(|id| context.world().contains(*id)) {
        return Ok(Some(id));
    }
    if let Some(id) = find_overlay_child(context, parent) {
        return Ok(Some(id));
    }
    if snapshot.overlay.is_none() {
        return Ok(None);
    }
    Ok(Some(
        context
            .create_detached_component(document, OverlayHost::new())?
            .stable_id(),
    ))
}

fn find_title_bar_controls_child(
    context: &AppContext,
    parent: StableNodeId,
) -> Option<StableNodeId> {
    let mut stack = context
        .world()
        .node(parent)
        .map(|node| node.children)
        .unwrap_or_default();
    while let Some(id) = stack.pop() {
        if view_is::<AppTitleBarControls>(context, id)
            || matches!(
                context.world().node(id).map(|node| node.kind),
                Some(NodeKind::Element { tag }) if tag == "app-title-bar-controls"
            )
        {
            return Some(id);
        }
        if let Some(children) = context.world().node(id).map(|node| node.children) {
            stack.extend(children);
        }
    }
    None
}

fn title_bar_has_columns(world: &UiWorld, id: StableNodeId) -> bool {
    world.node(id).is_some_and(|node| {
        node.children
            .iter()
            .any(|child| is_title_bar_column_tag(node_tag(world, *child).as_deref()))
    })
}

fn is_title_bar_column_tag(tag: Option<&str>) -> bool {
    matches!(
        tag,
        Some(LEADING_COLUMN_TAG | CENTER_COLUMN_TAG | TRAILING_COLUMN_TAG)
    )
}

fn node_tag(world: &UiWorld, id: StableNodeId) -> Option<String> {
    match world.node(id).map(|node| node.kind) {
        Some(NodeKind::Element { tag }) => Some(tag),
        _ => None,
    }
}

fn ensure_title_bar_slot(
    context: &mut AppContext,
    document: DocumentId,
    parent: StableNodeId,
    column: TitleBarColumn,
) -> Result<StableNodeId, FrameworkError> {
    let tag = column.tag();
    let existing = context
        .world()
        .node(parent)
        .into_iter()
        .flat_map(|node| node.children)
        .find(|&id| node_tag(context.world(), id).as_deref() == Some(tag))
        .or_else(|| {
            context
                .world()
                .node(parent)
                .into_iter()
                .flat_map(|node| node.children)
                .flat_map(|id| {
                    context
                        .world()
                        .node(id)
                        .map(|node| node.children)
                        .unwrap_or_default()
                })
                .find(|&id| node_tag(context.world(), id).as_deref() == Some(tag))
        });
    if let Some(id) = existing.filter(|id| context.world().contains(*id)) {
        return Ok(id);
    }
    Ok(context
        .create_detached_component(document, AppTitleBarSlot { column })?
        .stable_id())
}

fn ensure_title_label(
    context: &mut AppContext,
    document: DocumentId,
    center_slot: StableNodeId,
    title: &str,
) -> Result<StableNodeId, FrameworkError> {
    if let Some(existing) = context
        .world()
        .node(center_slot)
        .into_iter()
        .flat_map(|node| node.children)
        .find(|&id| view_is::<Text>(context, id) || is_title_label_node(context, id))
    {
        if view_is::<Text>(context, existing) {
            context.update_component(Entity::<Text>::from_stable_id(existing), |text, _| {
                if text.value != title {
                    text.value = title.to_owned();
                }
            })?;
        } else if context.world().text(existing) != Some(title) {
            let mut mutations = MutationQueue::new();
            mutations.set_text(
                existing,
                TextContent {
                    value: title.to_owned().into(),
                },
            );
            context.commit_mutations(mutations)?;
        }
        return Ok(existing);
    }
    Ok(context
        .create_detached_component(document, Text::new(title).style(title_label_style()))?
        .stable_id())
}

fn title_label_style() -> NodeStyle {
    let mut style = NodeStyle::default();
    style.foreground = Some(SemanticColorRole::Text);
    style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    style.text_vertical_alignment = TextVerticalAlignment::Center;
    let layout = Arc::make_mut(&mut style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.height = Some(LengthSpec::Fill);
    layout.font_size = Some(TITLE_FONT_SIZE);
    layout.font_weight = Some(TITLE_FONT_WEIGHT);
    layout.overflow_x = OverflowSpec::Hidden;
    layout.overflow_y = OverflowSpec::Hidden;
    layout.white_space_nowrap = true;
    layout.text_overflow_ellipsis = true;
    style
}

fn is_title_label_node(context: &AppContext, id: StableNodeId) -> bool {
    view_is::<Text>(context, id)
        || matches!(
            context.world().node(id).map(|node| node.kind),
            Some(NodeKind::Text)
        )
}

fn unclassified_title_bar_children(
    context: &AppContext,
    parent: StableNodeId,
    owned: &[Option<StableNodeId>],
) -> Vec<StableNodeId> {
    let owned: HashSet<StableNodeId> = owned.iter().copied().flatten().collect();
    context
        .world()
        .node(parent)
        .map(|node| node.children)
        .unwrap_or_default()
        .into_iter()
        .filter(|id| {
            !owned.contains(id)
                && !is_title_bar_column_tag(node_tag(context.world(), *id).as_deref())
        })
        .collect()
}

fn app_shell_child_ids(
    title_bar: Option<StableNodeId>,
    body: Option<StableNodeId>,
    overlay: Option<StableNodeId>,
) -> Vec<StableNodeId> {
    let mut children = Vec::new();
    if let Some(title_bar) = title_bar {
        children.push(title_bar);
    }
    if let Some(body) = body {
        children.push(body);
    }
    if let Some(overlay) = overlay {
        children.push(overlay);
    }
    children
}

fn app_shell_parent(context: &AppContext, title_bar: StableNodeId) -> Option<StableNodeId> {
    let parent = context.world().parent_id(title_bar)?;
    if view_is::<AppShell>(context, parent) {
        return Some(parent);
    }
    match node_tag(context.world(), parent).as_deref() {
        Some("app-shell" | "nana-app-shell") => Some(parent),
        _ => None,
    }
}

fn is_reserved_shell_slot(
    extra: StableNodeId,
    shell_body: Option<StableNodeId>,
    shell_overlay: Option<StableNodeId>,
) -> bool {
    Some(extra) == shell_body || Some(extra) == shell_overlay
}

fn is_title_bar_chrome(context: &AppContext, id: StableNodeId) -> bool {
    view_is::<IconButton>(context, id)
        || view_is::<AppTitleBarControls>(context, id)
        || matches!(
            context.world().standard_visual(id),
            Some(StandardVisual::Icon { .. })
        )
        || context
            .world()
            .accessibility(id)
            .is_some_and(|state| state.role == AccessibilityRole::Button)
}

fn find_app_shell_body_child(
    context: &AppContext,
    parent: StableNodeId,
    title_bar: Option<StableNodeId>,
    overlay: Option<StableNodeId>,
) -> Option<StableNodeId> {
    context
        .world()
        .node(parent)
        .into_iter()
        .flat_map(|node| node.children)
        .find(|&id| {
            Some(id) != title_bar
                && Some(id) != overlay
                && !is_title_bar_column_tag(node_tag(context.world(), id).as_deref())
        })
}

fn window_control_buttons(
    context: &mut AppContext,
    document: DocumentId,
    maximized: bool,
) -> Result<[StableNodeId; 3], FrameworkError> {
    let mut button = |action: WindowChromeAction| {
        context
            .create_detached_component(
                document,
                IconButton::new(action.icon(maximized), action.label(maximized))
                    .size(ControlSize::Small),
            )
            .map(|entity| entity.stable_id())
    };
    Ok([
        button(WindowChromeAction::Minimize)?,
        button(WindowChromeAction::ToggleMaximize)?,
        button(WindowChromeAction::Close)?,
    ])
}

fn ensure_window_controls(
    context: &mut AppContext,
    document: DocumentId,
    parent: StableNodeId,
    existing: Option<StableNodeId>,
    maximized: bool,
    native: bool,
) -> Result<StableNodeId, FrameworkError> {
    let controls = existing
        .filter(|id| context.world().contains(*id))
        .or_else(|| find_title_bar_controls_child(context, parent));
    if native {
        // The platform draws the buttons; the placeholder has no children.
        if let Some(controls) = controls {
            if view_is::<AppTitleBarControls>(context, controls) {
                context.update_component(
                    Entity::<AppTitleBarControls>::from_stable_id(controls),
                    |controls, _| {
                        controls.maximized = maximized;
                        controls.native = true;
                    },
                )?;
            }
            // Custom buttons left from an earlier mode have no place here.
            if context
                .world()
                .node(controls)
                .is_some_and(|node| !node.children.is_empty())
            {
                reconcile_ids(context, controls, &[])?;
            }
            return Ok(controls);
        }
        let controls = context.create_detached_component(
            document,
            AppTitleBarControls::new(maximized).native(true),
        )?;
        return Ok(controls.stable_id());
    }
    if let Some(controls) = controls {
        let count = context
            .world()
            .node(controls)
            .map(|node| node.children.len())
            .unwrap_or(0);
        if count < 3 {
            let [minimize, maximize, close] = window_control_buttons(context, document, maximized)?;
            if view_is::<AppTitleBarControls>(context, controls) {
                context.update_component(
                    Entity::<AppTitleBarControls>::from_stable_id(controls),
                    |controls, _| {
                        controls.maximized = maximized;
                        controls.native = false;
                        controls.minimize = Some(minimize);
                        controls.maximize = Some(maximize);
                        controls.close = Some(close);
                    },
                )?;
            }
            reconcile_ids(context, controls, &[minimize, maximize, close])?;
        } else if view_is::<AppTitleBarControls>(context, controls) {
            context.update_component(
                Entity::<AppTitleBarControls>::from_stable_id(controls),
                |controls, _| {
                    controls.maximized = maximized;
                    controls.native = false;
                },
            )?;
        }
        return Ok(controls);
    }
    let [minimize, maximize, close] = window_control_buttons(context, document, maximized)?;
    let controls = context.create_detached_component(
        document,
        AppTitleBarControls::new(maximized)
            .native(false)
            .minimize(minimize)
            .maximize(maximize)
            .close(close),
    )?;
    reconcile_ids(context, controls.stable_id(), &[minimize, maximize, close])?;
    Ok(controls.stable_id())
}

fn find_title_bar_child(context: &AppContext, parent: StableNodeId) -> Option<StableNodeId> {
    context
        .world()
        .node(parent)
        .into_iter()
        .flat_map(|node| node.children)
        .find(|&id| {
            view_is::<AppTitleBar>(context, id)
                || matches!(
                    context.world().node(id).map(|node| node.kind),
                    Some(NodeKind::Element { tag })
                        if tag.contains("title-bar") || tag.contains("titlebar")
                )
        })
}

fn find_overlay_child(context: &AppContext, parent: StableNodeId) -> Option<StableNodeId> {
    context
        .world()
        .node(parent)
        .into_iter()
        .flat_map(|node| node.children)
        .find(|&id| {
            view_is::<OverlayHost>(context, id)
                || matches!(
                    context.world().node(id).map(|node| node.kind),
                    Some(NodeKind::Element { tag }) if tag.contains("overlay")
                )
        })
}

fn recovered_title(context: &AppContext, parent: StableNodeId) -> Option<Arc<str>> {
    let id = find_title_bar_child(context, parent)?;
    if let Ok(title) = context.read(Entity::<AppTitleBar>::from_stable_id(id), |bar| {
        Arc::clone(&bar.title)
    }) && !title.is_empty()
    {
        return Some(title);
    }
    if let Some(text) = context.world().text(id).filter(|text| !text.is_empty()) {
        return Some(Arc::from(text));
    }
    context
        .world()
        .accessibility(id)
        .and_then(|state| state.label.clone())
        .filter(|label| !label.is_empty())
}

fn ensure_workspace(
    context: &mut AppContext,
    document: DocumentId,
    existing: Option<StableNodeId>,
) -> Result<StableNodeId, FrameworkError> {
    if let Some(id) = existing.filter(|id| view_is::<Workspace>(context, *id)) {
        return Ok(id);
    }
    Ok(context
        .create_detached_component(document, Workspace::new())?
        .stable_id())
}

fn ensure_overlay_host(
    context: &mut AppContext,
    document: DocumentId,
    existing: Option<StableNodeId>,
) -> Result<StableNodeId, FrameworkError> {
    if let Some(id) = existing.filter(|id| context.world().contains(*id)) {
        return Ok(id);
    }
    Ok(context
        .create_detached_component(document, OverlayHost::new())?
        .stable_id())
}

#[allow(clippy::too_many_arguments)]
fn ensure_title_bar(
    context: &mut AppContext,
    document: DocumentId,
    existing: Option<StableNodeId>,
    title: Option<&Arc<str>>,
    leading: Option<StableNodeId>,
    center: Option<StableNodeId>,
    trailing: Option<StableNodeId>,
    center_width: Option<f32>,
    window_controls: Option<bool>,
) -> Result<Option<StableNodeId>, FrameworkError> {
    if let Some(id) = existing.filter(|id| context.world().contains(*id)) {
        return Ok(Some(id));
    }
    let Some(title) = title else {
        return Ok(None);
    };
    let leading = leading.filter(|id| context.world().contains(*id));
    let center = center.filter(|id| context.world().contains(*id));
    let trailing = trailing.filter(|id| context.world().contains(*id));
    let mut bar = AppTitleBar::new(Arc::clone(title));
    if let Some(width) = center_width {
        bar = bar.center_width(width);
    }
    if let Some(show) = window_controls {
        bar = bar.show_window_controls(show);
    }
    if let Some(leading) = leading {
        bar = bar.leading(leading);
    }
    if let Some(center) = center {
        bar = bar.center(center);
    }
    if let Some(trailing) = trailing {
        bar = bar.trailing(trailing);
    }
    let title_bar = context
        .create_detached_component(document, bar)?
        .stable_id();
    let mut children = Vec::new();
    if let Some(leading) = leading {
        children.push(leading);
    }
    if let Some(center) = center {
        children.push(center);
    }
    if let Some(trailing) = trailing {
        children.push(trailing);
    }
    reconcile_ids(context, title_bar, &children)?;
    Ok(Some(title_bar))
}

fn resolve_navigation(
    context: &mut AppContext,
    document: DocumentId,
    navigation: Option<StableNodeId>,
    footer: Option<StableNodeId>,
    existing_frame: Option<StableNodeId>,
) -> Result<(Option<StableNodeId>, Option<StableNodeId>), FrameworkError> {
    let Some(navigation) = navigation.filter(|id| context.world().contains(*id)) else {
        return Ok((None, None));
    };
    let footer = footer.filter(|id| context.world().contains(*id));
    if view_is::<SidebarFrame>(context, navigation) {
        if let Some(footer) = footer {
            context.update_component(
                Entity::<SidebarFrame>::from_stable_id(navigation),
                |frame, _| {
                    frame.footer = Some(footer);
                },
            )?;
            let children = context
                .world()
                .node(navigation)
                .map(|node| node.children)
                .unwrap_or_default();
            if !children.contains(&footer) {
                let mut mutations = MutationQueue::new();
                mutations.insert(navigation, footer, None);
                context.commit_mutations(mutations)?;
            }
        }
        return Ok((Some(navigation), None));
    }

    let frame = if let Some(id) = existing_frame.filter(|id| view_is::<SidebarFrame>(context, *id))
    {
        id
    } else {
        context
            .create_detached_component(document, SidebarFrame::new())?
            .stable_id()
    };
    let scroll = match context.read(Entity::<SidebarFrame>::from_stable_id(frame), |frame| {
        frame.body
    })? {
        Some(id) if is_scroll_node(context, id) => id,
        _ => context
            .create_detached_component(document, SidebarFrame::vertical_body_scroll())?
            .stable_id(),
    };
    context.update_component(Entity::<SidebarFrame>::from_stable_id(frame), |frame, _| {
        frame.body = Some(scroll);
        frame.footer = footer;
    })?;
    reconcile_ids(context, scroll, &[navigation])?;
    let mut frame_children = vec![scroll];
    if let Some(footer) = footer {
        frame_children.push(footer);
    }
    reconcile_ids(context, frame, &frame_children)?;
    Ok((Some(frame), Some(frame)))
}

fn is_scroll_node(context: &AppContext, id: StableNodeId) -> bool {
    matches!(
        context.world().node(id).map(|node| node.kind),
        Some(NodeKind::Element { tag }) if tag == "scroll"
    )
}

fn region_slots(
    context: &AppContext,
    shell: &DesktopShell,
    resources: Option<StableNodeId>,
) -> Vec<WorkspaceRegionSlot> {
    let present = |id: Option<StableNodeId>| id.filter(|id| context.world().contains(*id));
    let region_slot = |id, content| {
        if is_scroll_node(context, content) {
            WorkspaceRegionSlot::borrowed(id, content)
        } else {
            WorkspaceRegionSlot::new(id, content)
        }
    };
    let mut slots = Vec::new();
    if let Some(content) = present(resources) {
        slots.push(region_slot(RegionId::Resources, content));
    }
    if let Some(content) = present(shell.primary) {
        slots.push(region_slot(RegionId::Primary, content));
    }
    if let Some(content) = present(shell.inspector) {
        slots.push(region_slot(RegionId::Inspector, content));
    }
    if let Some(content) = present(shell.bottom) {
        slots.push(region_slot(RegionId::Diagnostics, content));
    }
    for (id, content) in &shell.extra_regions {
        let Some(content) = present(Some(*content)) else {
            continue;
        };
        if let Some(existing) = slots.iter_mut().find(|slot| slot.id == *id) {
            *existing = region_slot(id.clone(), content);
        } else {
            slots.push(region_slot(id.clone(), content));
        }
    }
    slots
}

fn used_ids(shell: &DesktopShell, slots: &[WorkspaceRegionSlot]) -> HashSet<StableNodeId> {
    let mut ids = HashSet::new();
    for id in [
        shell.title_bar,
        shell.workspace,
        shell.overlay,
        shell.status,
        shell.primary,
        shell.navigation,
        shell.navigation_footer,
        shell.inspector,
        shell.bottom,
        shell.navigation_frame,
        shell.title_leading,
        shell.title_center,
        shell.title_trailing,
    ]
    .into_iter()
    .flatten()
    {
        ids.insert(id);
    }
    for (_, content) in &shell.extra_regions {
        ids.insert(*content);
    }
    ids.extend(shell.overlays.iter().copied());
    ids.extend(shell.status_overlays.iter().copied());
    for slot in slots {
        if let Some(content) = slot.content {
            ids.insert(content);
        }
    }
    ids
}

fn park_unused(
    context: &mut AppContext,
    shell: StableNodeId,
    previous: &HashSet<StableNodeId>,
    next: &HashSet<StableNodeId>,
) -> Result<(), FrameworkError> {
    let mut mutations = MutationQueue::new();
    for id in previous {
        if *id == shell || next.contains(id) || !context.world().contains(*id) {
            continue;
        }
        mutations.park_subtree(*id);
    }
    if mutations.as_slice().is_empty() {
        return Ok(());
    }
    context.commit_mutations(mutations)?;
    Ok(())
}

fn reconcile_ids(
    context: &mut AppContext,
    parent: StableNodeId,
    ordered: &[StableNodeId],
) -> Result<bool, FrameworkError> {
    let ordered = ordered
        .iter()
        .copied()
        .filter(|id| *id != parent && context.world().contains(*id))
        .collect::<Vec<_>>();
    let current = context
        .world()
        .node(parent)
        .ok_or(FrameworkError::MissingView(parent))?
        .children
        .clone();
    if current.as_slice() == ordered.as_slice() {
        return Ok(false);
    }
    let keep = ordered.iter().copied().collect::<HashSet<_>>();
    let mut mutations = MutationQueue::new();
    for child in &current {
        if !keep.contains(child) {
            mutations.park_subtree(*child);
        }
    }
    for child in ordered {
        mutations.insert(parent, child, None);
    }
    context.commit_mutations(mutations)?;
    Ok(true)
}

fn project_window_control(
    id: StableNodeId,
    action: WindowChromeAction,
    maximized: bool,
    world: &UiWorld,
    mutations: &mut MutationQueue,
) {
    if world.node(id).is_none() {
        return;
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
        icon: action.icon(maximized),
        size: ControlSize::Small.icon_size(),
        tooltip: None,
    };
    if world.standard_visual(id) != Some(visual.clone()) {
        mutations.set_standard_visual(id, Some(visual));
    }
    project_common(
        id,
        world,
        mutations,
        &window_control_style(action == WindowChromeAction::Close),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
        AccessibilityState {
            role: AccessibilityRole::Button,
            label: Some(Arc::from(action.label(maximized))),
            ..AccessibilityState::default()
        },
    );
}

fn window_control_style(danger: bool) -> NodeStyle {
    let mut style = IconButton::new(Icon::Close, "")
        .size(ControlSize::Small)
        .style;
    if danger {
        style.interaction.hovered.foreground = Some(SemanticColorRole::Danger);
        style.interaction.hovered.background = Some(SemanticColorRole::DangerSoftHover);
        style.interaction.pressed.foreground = Some(SemanticColorRole::Danger);
        style.interaction.pressed.background = Some(SemanticColorRole::DangerSoftPressed);
    }
    let layout = Arc::make_mut(&mut style.layout);
    layout.width = Some(LengthSpec::Px(WINDOW_CONTROL_WIDTH));
    layout.height = Some(LengthSpec::Px(WINDOW_CONTROL_WIDTH));
    layout.min_width = Some(LengthSpec::Px(WINDOW_CONTROL_WIDTH));
    layout.min_height = Some(LengthSpec::Px(WINDOW_CONTROL_WIDTH));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    layout.padding_top = Some(LengthSpec::Px(0.0));
    layout.padding_bottom = Some(LengthSpec::Px(0.0));
    layout.border_width = Some(0.0);
    style
}

fn apply_hug_slot(layout: &mut nana_ui_core::LayoutStyle, align: AlignSpec, justify: JustifySpec) {
    layout.direction = Some(FlexDirection::Row);
    layout.align_items = align;
    layout.justify_content = justify;
    layout.width = Some(LengthSpec::Shrink);
    layout.height = Some(LengthSpec::Fill);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
}

/// A side column shares the space the centre leaves, evenly so the centre
/// stays centred. The trailing one, which holds the window controls, never
/// goes below what its content measures: the centre gives way first
/// ([`apply_center_column`]), so the controls are never cut. The leading one
/// may be squeezed.
fn apply_fill_column(
    layout: &mut nana_ui_core::LayoutStyle,
    justify: JustifySpec,
    keeps_content: bool,
) {
    layout.direction = Some(FlexDirection::Row);
    layout.align_items = AlignSpec::Center;
    layout.justify_content = justify;
    if keeps_content {
        // Sized by its content (what the minimum keeps), then grown into the
        // space the centre leaves.
        layout.width = None;
        layout.min_width = Some(LengthSpec::MaxContent);
    } else {
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }
    layout.height = Some(LengthSpec::Fill);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.hidden = false;
}

fn apply_center_column(layout: &mut nana_ui_core::LayoutStyle, width: f32) {
    layout.direction = Some(FlexDirection::Row);
    layout.align_items = AlignSpec::Center;
    layout.justify_content = JustifySpec::Center;
    layout.width = Some(LengthSpec::Px(width));
    layout.max_width = Some(LengthSpec::Px(width));
    layout.height = Some(LengthSpec::Fill);
    layout.flex_grow = Some(0.0);
    // Its width is what it asks for when there is room; in a narrow window
    // it shrinks, and its content ellipsizes, before the side columns do.
    layout.flex_shrink = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.padding_left = Some(LengthSpec::Px(CENTER_PADDING_X));
    layout.padding_right = Some(LengthSpec::Px(CENTER_PADDING_X));
    layout.padding_top = Some(LengthSpec::Px(0.0));
    layout.padding_bottom = Some(LengthSpec::Px(0.0));
    layout.overflow_x = OverflowSpec::Hidden;
    layout.overflow_y = OverflowSpec::Hidden;
    layout.white_space_nowrap = true;
    layout.text_overflow_ellipsis = true;
    layout.hidden = false;
}

fn patch_layout(
    world: &UiWorld,
    mutations: &mut MutationQueue,
    id: StableNodeId,
    patch: impl FnOnce(&mut nana_ui_core::LayoutStyle),
) {
    if world.node(id).is_none() {
        return;
    }
    let mut style = world.node_style(id).cloned().unwrap_or_default();
    patch(Arc::make_mut(&mut style.layout));
    if world.node_style(id) != Some(&style) {
        mutations.set_style(id, style);
    }
}

fn finite_positive(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::{
        AppContext, Card, DocumentId, Entity, IconButton, LayoutViewport, MountState, SidebarFrame,
        StableNodeId, StandardVisual, Text, TextHorizontalAlignment, UiWorld,
    };
    use nana_ui_core::{
        RegionId, RegionPlacement, RegionRole, RegionScope, RegionState, WorkspaceLayout,
        WorkspaceModel,
    };

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn is_descendant(world: &UiWorld, ancestor: StableNodeId, node: StableNodeId) -> bool {
        let mut current = world.parent_id(node);
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = world.parent_id(id);
        }
        false
    }

    fn icon_of(visual: Option<StandardVisual>) -> Option<Icon> {
        match visual {
            Some(StandardVisual::Icon { icon, .. }) => Some(icon),
            _ => None,
        }
    }

    #[test]
    fn empty_title_bar_keeps_implicitly_mounted_center_text() {
        let mut context = AppContext::new();
        let text = context
            .create_component(
                document(),
                Text::new("Mounted title").style(title_label_style()),
            )
            .unwrap();
        let bar = context
            .create_component(document(), AppTitleBar::new(""))
            .unwrap();
        context.append_child(bar, text).unwrap();
        context.assemble_app_title_bar(bar).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(640.0, 400.0))
            .unwrap();
        let center = context
            .world()
            .node(text.stable_id())
            .unwrap()
            .parent
            .unwrap();
        assert!(!context.world().node_style(center).unwrap().layout.hidden);
        let bounds = context.world().layout_box(text.stable_id()).unwrap();
        assert!(bounds.width > 0.0 && bounds.height > 0.0);
        assert_eq!(
            context.world().text(text.stable_id()),
            Some("Mounted title")
        );
        assert_eq!(
            context.world().layout_box(center).unwrap().width,
            DEFAULT_CENTER_WIDTH
        );
    }

    #[test]
    fn empty_title_bar_reserves_trailing_content_at_narrow_width() {
        let mut context = AppContext::new();
        let leading = context
            .create_component(
                document(),
                Text::new("A long stage name").style(NodeStyle {
                    layout: Arc::new(nana_ui_core::LayoutStyle {
                        width: Some(LengthSpec::Fill),
                        min_width: Some(LengthSpec::Px(0.0)),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            )
            .unwrap();
        let trailing = context
            .create_component(
                document(),
                Text::new("Status").style(NodeStyle {
                    layout: Arc::new(nana_ui_core::LayoutStyle {
                        width: Some(LengthSpec::Px(280.0)),
                        flex_shrink: Some(0.0),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            )
            .unwrap();
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("")
                    .leading(leading.stable_id())
                    .trailing(trailing.stable_id())
                    .show_window_controls(true)
                    .native_controls(false),
            )
            .unwrap();
        context.assemble_app_title_bar(bar).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(640.0, 400.0))
            .unwrap();
        let controls = context.read(bar, |bar| bar.controls.unwrap()).unwrap();
        let a = context.world().layout_box(leading.stable_id()).unwrap();
        let b = context.world().layout_box(trailing.stable_id()).unwrap();
        let c = context.world().layout_box(controls).unwrap();
        assert!((b.width - 280.0).abs() < 0.1);
        assert!(a.x + a.width <= b.x + 0.1);
        assert!(b.x + b.width <= c.x + 0.1);
        assert!((c.x + c.width - 640.0).abs() < 0.1);
        assert!(a.width > 0.0);
    }

    #[test]
    fn transparent_title_bar_preserves_slot_content_and_geometry() {
        let mut context = AppContext::new();
        let text = context
            .create_component(document(), Text::new("Status"))
            .unwrap();
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("Nana").trailing(text.stable_id()),
            )
            .unwrap();
        context.assemble_app_title_bar(bar).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();
        let before = context.world().layout_box(text.stable_id()).unwrap();
        let parent = context.world().node(text.stable_id()).unwrap().parent;
        for transparent in [true, false] {
            context
                .update_component(bar, |bar, _| bar.transparent = transparent)
                .unwrap();
            context
                .layout_document(document(), LayoutViewport::new(800.0, 400.0))
                .unwrap();
            let style = context.world().node_style(bar.stable_id()).unwrap();
            assert_eq!(
                style.background,
                (!transparent).then_some(SemanticColorRole::Titlebar)
            );
            assert!(style.layout.background.is_none());
            assert_eq!(style.foreground, Some(SemanticColorRole::Text));
            assert_eq!(
                context.world().layout_box(text.stable_id()).unwrap(),
                before
            );
            assert_eq!(
                context.world().node(text.stable_id()).unwrap().parent,
                parent
            );
            assert_eq!(context.world().text(text.stable_id()), Some("Status"));
        }
    }

    #[test]
    fn title_bar_is_36px_and_projects_title_without_center() {
        let mut context = AppContext::new();
        let bar = context
            .create_component(document(), AppTitleBar::new("Nana"))
            .unwrap();
        let id = bar.stable_id();
        assert_eq!(
            context.world().node(id).unwrap().kind,
            NodeKind::Element {
                tag: "app-title-bar".into(),
            }
        );
        assert_eq!(context.world().text(id), Some("Nana"));
        let style = context.world().node_style(id).unwrap();
        assert_eq!(style.layout.height, Some(LengthSpec::Px(TITLE_BAR_HEIGHT)));
        assert_eq!(style.layout.font_size, Some(TITLE_FONT_SIZE));
        assert_eq!(style.layout.font_weight, Some(TITLE_FONT_WEIGHT));
        assert_eq!(style.foreground, Some(SemanticColorRole::Text));
        assert_eq!(style.background, Some(SemanticColorRole::Titlebar));
        assert!(style.layout.background.is_none());
        assert_eq!(style.layout.direction, Some(FlexDirection::Row));
        let chrome = WindowChrome::platform_default();
        // Unassembled, a native bar has no placeholder and pads the band clear.
        let expected_leading_pad = if chrome.uses_custom_controls() {
            0.0
        } else {
            NATIVE_WINDOW_CONTROLS_WIDTH + NATIVE_LEADING_CLEARANCE
        };
        assert_eq!(
            style.layout.padding_left,
            Some(LengthSpec::Px(expected_leading_pad))
        );

        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();
        let bounds = context.world().layout_box(id).unwrap();
        assert_eq!(bounds.height, TITLE_BAR_HEIGHT);
        assert_eq!(bounds.width, 800.0);
        let bar_view = context.read(bar, |bar| bar.clone()).unwrap();
        assert!(bar_view.show_window_controls);
        assert_eq!(bar_view.native_controls, !chrome.uses_custom_controls());
    }

    #[test]
    fn assemble_title_bar_mounts_custom_window_controls_when_enabled() {
        let mut context = AppContext::new();
        let bar = context
            .create_component(document(), AppTitleBar::new("Nana").native_controls(false))
            .unwrap();
        let mounted = context.assemble_app_title_bar(bar).unwrap();
        let snapshot = context.read(bar, Clone::clone).unwrap();
        {
            assert!(mounted);
            let controls = snapshot.controls.expect("custom chrome mounts controls");
            assert_eq!(
                context.world().node(controls).unwrap().children.len(),
                3,
                "custom title bar must own minimize, maximize, and close"
            );
            let columns = context.world().node(bar.stable_id()).unwrap().children;
            assert_eq!(columns.len(), 3, "title bar is leading | center | trailing");
            assert_eq!(
                node_tag(context.world(), columns[0]).as_deref(),
                Some(LEADING_COLUMN_TAG)
            );
            assert_eq!(
                node_tag(context.world(), columns[1]).as_deref(),
                Some(CENTER_COLUMN_TAG)
            );
            assert_eq!(
                node_tag(context.world(), columns[2]).as_deref(),
                Some(TRAILING_COLUMN_TAG)
            );
            assert_eq!(
                context
                    .world()
                    .node(columns[2])
                    .unwrap()
                    .children
                    .last()
                    .copied(),
                Some(controls)
            );
            assert!(
                context
                    .world()
                    .node(controls)
                    .unwrap()
                    .children
                    .iter()
                    .all(|id| {
                        context
                            .world()
                            .accessibility(*id)
                            .is_some_and(|state| state.role == AccessibilityRole::Button)
                    })
            );
            assert!(!context.assemble_app_title_bar(bar).unwrap());
        }
    }

    fn native_title_bar(
        context: &mut AppContext,
        style: NodeStyle,
    ) -> (Entity<AppTitleBar>, Entity<IconButton>) {
        let leading = context
            .create_component(
                document(),
                IconButton::new(Icon::Sidebar, "sidebar").size(ControlSize::Small),
            )
            .unwrap();
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("")
                    .native_controls(true)
                    .leading(leading.stable_id())
                    .style(style),
            )
            .unwrap();
        context.assemble_app_title_bar(bar).unwrap();
        (bar, leading)
    }

    #[test]
    fn explicit_custom_controls_are_never_treated_as_native() {
        let mut context = AppContext::new();
        let bar = context
            .create_component(document(), AppTitleBar::new("Nana").native_controls(false))
            .unwrap();
        context.assemble_app_title_bar(bar).unwrap();
        let controls = context.read(bar, |bar| bar.controls).unwrap().unwrap();
        let stored = context
            .read(
                Entity::<AppTitleBarControls>::from_stable_id(controls),
                |controls| controls.native,
            )
            .unwrap();
        assert!(!stored);
        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();
        assert_eq!(context.world().node(controls).unwrap().children.len(), 3);
    }

    #[test]
    fn native_window_controls_are_an_empty_leading_placeholder() {
        let mut context = AppContext::new();
        let (bar, leading) = native_title_bar(&mut context, NodeStyle::default());
        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();
        let controls = context
            .read(bar, |bar| bar.controls)
            .unwrap()
            .expect("native chrome mounts a placeholder");
        assert!(context.world().node(controls).unwrap().children.is_empty());
        let columns = context.world().node(bar.stable_id()).unwrap().children;
        assert_eq!(
            context.world().node(columns[0]).unwrap().children.first(),
            Some(&controls)
        );
        let placeholder = context.world().layout_box(controls).unwrap();
        assert_eq!(placeholder.x, 0.0);
        assert_eq!(placeholder.width, NATIVE_WINDOW_CONTROLS_WIDTH);
        assert_eq!(placeholder.height, TITLE_BAR_HEIGHT);
        let leading = context.world().layout_box(leading.stable_id()).unwrap();
        assert!(
            leading.x + 0.5 >= placeholder.x + placeholder.width + NATIVE_LEADING_CLEARANCE,
            "leading chrome overlapped the native buttons"
        );
        assert!(!context.assemble_app_title_bar(bar).unwrap());
    }

    #[test]
    fn native_window_controls_follow_an_inset_title_bar() {
        let mut context = AppContext::new();
        let root = context
            .create_component(document(), crate::Stack::fill_column(0.0))
            .unwrap();
        let mut style = NodeStyle::default();
        {
            let layout = Arc::make_mut(&mut style.layout);
            layout.position = PositionSpec::Absolute;
            layout.offset_top = Some(LengthSpec::Px(12.0));
            layout.offset_left = Some(LengthSpec::Px(12.0));
            layout.offset_right = Some(LengthSpec::Px(12.0));
        }
        let (bar, leading) = native_title_bar(&mut context, style);
        context.append_child(root, bar).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();
        let controls = context.read(bar, |bar| bar.controls).unwrap().unwrap();
        let placeholder = context.world().layout_box(controls).unwrap();
        assert_eq!((placeholder.x, placeholder.y), (12.0, 12.0));
        let leading = context.world().layout_box(leading.stable_id()).unwrap();
        assert!(leading.x + 0.5 >= placeholder.x + placeholder.width);
    }

    #[test]
    fn hidden_native_controls_release_the_leading_band() {
        let mut context = AppContext::new();
        let (bar, leading) = native_title_bar(&mut context, NodeStyle::default());
        context
            .update_component(bar, |bar, _| bar.show_window_controls = false)
            .unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();
        let leading = context.world().layout_box(leading.stable_id()).unwrap();
        assert!(leading.x < NATIVE_WINDOW_CONTROLS_WIDTH);
    }

    #[test]
    fn custom_window_controls_are_inset_square_buttons() {
        let window_width = 800.0;
        let mut context = AppContext::new();
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("Nana")
                    .native_controls(false)
                    .show_window_controls(true),
            )
            .unwrap();
        context.assemble_app_title_bar(bar).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(window_width, 400.0))
            .unwrap();
        let world = context.world();
        let controls = find_title_bar_controls_child(&context, bar.stable_id())
            .expect("custom chrome mounts controls");
        let buttons = world.node(controls).unwrap().children.clone();
        assert_eq!(buttons.len(), 3);
        let container = world.layout_box(controls).unwrap();
        assert_eq!(container.height, TITLE_BAR_HEIGHT);
        assert_eq!(container.y, 0.0);
        assert_eq!(
            container.x + container.width,
            window_width,
            "controls must sit flush against the window edge"
        );
        for (index, button) in buttons.iter().enumerate() {
            let bounds = world.layout_box(*button).unwrap();
            assert_eq!(bounds.width, WINDOW_CONTROL_WIDTH);
            assert_eq!(bounds.height, WINDOW_CONTROL_WIDTH);
            assert_eq!(bounds.y, (TITLE_BAR_HEIGHT - WINDOW_CONTROL_WIDTH) / 2.0);
            assert_eq!(
                bounds.x,
                container.x
                    + WINDOW_CONTROL_PADDING
                    + index as f32 * (WINDOW_CONTROL_WIDTH + WINDOW_CONTROL_GAP),
                "buttons keep the shared control gap"
            );
        }
        let close = world.layout_box(buttons[2]).unwrap();
        assert_eq!(close.x + close.width, window_width - WINDOW_CONTROL_PADDING);
        assert!(close.y > container.y);
    }

    /// A wide centre column gives way before the trailing cluster and the
    /// window controls do: at 800 px a 420 px centre moves over so the
    /// account button and all three controls stay inside the window, at
    /// 600 px it shrinks, and with room to spare it keeps its width, centred.
    #[test]
    fn a_narrow_window_shrinks_the_centre_before_the_trailing_cluster() {
        let mut context = AppContext::new();
        let sized = |context: &mut AppContext, width: f32| {
            context
                .create_detached_component(
                    document(),
                    crate::Stack::row(0.0).with_layout(move |layout| {
                        layout.width = Some(LengthSpec::Px(width));
                        layout.height = Some(LengthSpec::Px(24.0));
                        layout.flex_shrink = Some(0.0);
                    }),
                )
                .unwrap()
        };
        let leading = sized(&mut context, 60.0);
        let center = sized(&mut context, 300.0);
        let trailing = sized(&mut context, 110.0);
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("Nana")
                    .native_controls(false)
                    .center_width(420.0)
                    .leading(leading.stable_id())
                    .center(center.stable_id())
                    .trailing(trailing.stable_id()),
            )
            .unwrap();
        context.assemble_app_title_bar(bar).unwrap();
        let controls = find_title_bar_controls_child(&context, bar.stable_id()).unwrap();
        let close = *context
            .world()
            .node(controls)
            .unwrap()
            .children
            .last()
            .unwrap();
        let column = |context: &AppContext, tag: &str| {
            context
                .world()
                .node(bar.stable_id())
                .unwrap()
                .children
                .into_iter()
                .find(|child| node_tag(context.world(), *child).as_deref() == Some(tag))
                .unwrap()
        };
        let center_column = column(&context, CENTER_COLUMN_TAG);
        for width in [800.0, 600.0] {
            context
                .layout_document(document(), LayoutViewport::new(width, 400.0))
                .unwrap();
            let world = context.world();
            let close_box = world.layout_box(close).unwrap();
            assert!(
                close_box.x + close_box.width <= width,
                "{width}: close {close_box:?} is inside the window"
            );
            let trailing_box = world.layout_box(trailing.stable_id()).unwrap();
            let center_box = world.layout_box(center_column).unwrap();
            assert!(
                trailing_box.x >= center_box.x + center_box.width,
                "{width}: the trailing cluster {trailing_box:?} is not under the centre {center_box:?}"
            );
            if width < 700.0 {
                assert!(center_box.width < 420.0, "{width}: the centre gave way");
            }
        }
        context
            .layout_document(document(), LayoutViewport::new(1400.0, 400.0))
            .unwrap();
        let center_box = context.world().layout_box(center_column).unwrap();
        assert_eq!(center_box.width, 420.0);
        assert!((center_box.x + center_box.width / 2.0 - 700.0).abs() < 0.5);
    }

    #[test]
    fn blank_title_bar_is_hittable_and_child_button_wins() {
        let mut context = AppContext::new();
        let button = context
            .create_component(
                document(),
                IconButton::new(Icon::Sidebar, "toggle").size(ControlSize::Small),
            )
            .unwrap();
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("Nana").leading(button.stable_id()),
            )
            .unwrap();
        context.append_child(bar, button).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();
        context.rebuild_hit_test(document());

        let bounds = context.world().layout_box(bar.stable_id()).unwrap();
        let blank_x = bounds.x + bounds.width - 24.0;
        let blank_y = bounds.y + bounds.height / 2.0;
        assert_eq!(
            context.pointer_target(document(), blank_x, blank_y),
            Some(bar.stable_id())
        );

        let button_box = context.world().layout_box(button.stable_id()).unwrap();
        let button_x = button_box.x + button_box.width / 2.0;
        let button_y = button_box.y + button_box.height / 2.0;
        assert_eq!(
            context.pointer_target(document(), button_x, button_y),
            Some(button.stable_id())
        );
    }

    #[test]
    fn center_slot_suppresses_default_title_text() {
        let mut context = AppContext::new();
        let center = context
            .create_component(document(), Text::new("Workspace / File"))
            .unwrap();
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("Nana").center(center.stable_id()),
            )
            .unwrap();
        assert_eq!(context.world().text(bar.stable_id()), Some(""));
        assert_eq!(
            context.world().text(center.stable_id()),
            Some("Workspace / File")
        );

        context
            .update_component(bar, |bar, _| {
                bar.center = None;
            })
            .unwrap();
        assert_eq!(context.world().text(bar.stable_id()), Some("Nana"));
    }

    #[test]
    fn assembled_title_bar_uses_three_columns() {
        let mut context = AppContext::new();
        let leading = context
            .create_component(
                document(),
                IconButton::new(Icon::Sidebar, "sidebar").size(ControlSize::Small),
            )
            .unwrap();
        let center = context
            .create_component(document(), Text::new("NanaShader"))
            .unwrap();
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("Nana")
                    .leading(leading.stable_id())
                    .center(center.stable_id()),
            )
            .unwrap();
        context.append_child(bar, leading).unwrap();
        context.append_child(bar, center).unwrap();
        assert!(context.assemble_app_title_bar(bar).unwrap());
        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();

        let bar_box = context.world().layout_box(bar.stable_id()).unwrap();
        let columns = context.world().node(bar.stable_id()).unwrap().children;
        assert_eq!(columns.len(), 3);
        let leading_col = context.world().layout_box(columns[0]).unwrap();
        let center_col = context.world().layout_box(columns[1]).unwrap();
        let trailing_col = context.world().layout_box(columns[2]).unwrap();
        let leading_box = context.world().layout_box(leading.stable_id()).unwrap();
        let center_layout = &context.world().node_style(columns[1]).unwrap().layout;
        assert_eq!(
            center_layout.width,
            Some(LengthSpec::Px(DEFAULT_CENTER_WIDTH))
        );
        assert!(
            leading_box.width < 80.0,
            "leading chrome must hug, got width {}",
            leading_box.width
        );
        if !WindowChrome::platform_default().uses_custom_controls() {
            assert!(
                leading_box.x + 0.5 >= bar_box.x + NATIVE_WINDOW_CONTROLS_WIDTH,
                "leading must start after the traffic lights"
            );
        }
        let title_mid = center_col.x + center_col.width / 2.0;
        assert!(
            (title_mid - (bar_box.x + bar_box.width / 2.0)).abs() < 48.0,
            "title column mid {title_mid} should stay near the window center"
        );
        assert!(
            leading_col.x + leading_col.width <= center_col.x + 0.5,
            "title must sit after leading chrome"
        );
        assert!(
            trailing_col.width > bar_box.width * 0.2,
            "trailing column should fill leftover, got {}",
            trailing_col.width
        );

        if WindowChrome::platform_default().uses_custom_controls() {
            let controls = context
                .read(bar, |bar| bar.controls)
                .unwrap()
                .expect("custom chrome mounts controls");
            let controls_box = context.world().layout_box(controls).unwrap();
            assert!(
                controls_box.x + controls_box.width > bar_box.x + bar_box.width * 0.75,
                "window controls must sit on the trailing edge, got x={}",
                controls_box.x
            );
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn default_title_bar_places_window_controls_on_the_right() {
        let mut context = AppContext::new();
        let bar = context
            .create_component(document(), AppTitleBar::new("NanaLive"))
            .unwrap();
        assert!(context.assemble_app_title_bar(bar).unwrap());
        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();
        context.rebuild_hit_test(document());

        let bar_box = context.world().layout_box(bar.stable_id()).unwrap();
        let controls = context
            .read(bar, |bar| bar.controls)
            .unwrap()
            .expect("custom chrome mounts controls");
        let controls_box = context.world().layout_box(controls).unwrap();
        assert!(
            controls_box.x > bar_box.x + bar_box.width * 0.7,
            "controls were on the left: x={} width={}",
            controls_box.x,
            bar_box.width
        );
        let close = context.world().node(controls).unwrap().children[2];
        let close_box = context.world().layout_box(close).unwrap();
        assert!(
            close_box.x + close_box.width > bar_box.x + bar_box.width - 48.0,
            "close button must sit near the trailing edge"
        );
    }

    #[test]
    fn controls_helper_projects_three_icons_and_restore() {
        let mut context = AppContext::new();
        let minimize = context
            .create_component(
                document(),
                IconButton::new(Icon::Search, "x").size(ControlSize::Small),
            )
            .unwrap();
        let maximize = context
            .create_component(
                document(),
                IconButton::new(Icon::Search, "x").size(ControlSize::Small),
            )
            .unwrap();
        let close = context
            .create_component(
                document(),
                IconButton::new(Icon::Search, "x").size(ControlSize::Small),
            )
            .unwrap();
        let controls = context
            .create_component(
                document(),
                AppTitleBarControls::new(false)
                    .native(false)
                    .minimize(minimize.stable_id())
                    .maximize(maximize.stable_id())
                    .close(close.stable_id()),
            )
            .unwrap();

        assert_eq!(
            context.world().node(controls.stable_id()).unwrap().kind,
            NodeKind::Element {
                tag: "app-title-bar-controls".into(),
            }
        );
        assert_eq!(
            icon_of(context.world().standard_visual(minimize.stable_id())),
            Some(Icon::Minimize)
        );
        assert_eq!(
            icon_of(context.world().standard_visual(maximize.stable_id())),
            Some(Icon::Maximize)
        );
        assert_eq!(
            icon_of(context.world().standard_visual(close.stable_id())),
            Some(Icon::Close)
        );
        assert_eq!(
            context
                .world()
                .node_style(minimize.stable_id())
                .unwrap()
                .layout
                .width,
            Some(LengthSpec::Px(WINDOW_CONTROL_WIDTH))
        );
        assert_eq!(
            context
                .world()
                .accessibility(maximize.stable_id())
                .unwrap()
                .label
                .as_deref(),
            Some("Maximize")
        );

        context
            .update_component(controls, |controls, _| {
                controls.maximized = true;
            })
            .unwrap();
        assert_eq!(
            icon_of(context.world().standard_visual(maximize.stable_id())),
            Some(Icon::Restore)
        );
        assert_eq!(
            context
                .world()
                .accessibility(maximize.stable_id())
                .unwrap()
                .label
                .as_deref(),
            Some("Restore")
        );
        assert_eq!(
            icon_of(context.world().standard_visual(minimize.stable_id())),
            Some(Icon::Minimize)
        );
        assert_eq!(
            context
                .world()
                .node_style(close.stable_id())
                .unwrap()
                .interaction
                .focused
                .border,
            None
        );
    }

    #[test]
    fn window_controls_are_omitted_when_disabled() {
        let mut context = AppContext::new();
        let minimize = context
            .create_component(
                document(),
                IconButton::new(Icon::Minimize, "Minimize").size(ControlSize::Small),
            )
            .unwrap();
        let maximize = context
            .create_component(
                document(),
                IconButton::new(Icon::Maximize, "Maximize").size(ControlSize::Small),
            )
            .unwrap();
        let close = context
            .create_component(
                document(),
                IconButton::new(Icon::Close, "Close").size(ControlSize::Small),
            )
            .unwrap();
        let controls = context
            .create_component(
                document(),
                AppTitleBarControls::new(false)
                    .native(false)
                    .minimize(minimize.stable_id())
                    .maximize(maximize.stable_id())
                    .close(close.stable_id()),
            )
            .unwrap();
        context.append_child(controls, minimize).unwrap();
        context.append_child(controls, maximize).unwrap();
        context.append_child(controls, close).unwrap();
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("Nana")
                    .native_controls(false)
                    .controls(controls.stable_id())
                    .show_window_controls(false),
            )
            .unwrap();
        assert!(
            context
                .world()
                .node_style(controls.stable_id())
                .unwrap()
                .layout
                .hidden
        );
        assert_eq!(context.world().standard_visual(bar.stable_id()), None);
        assert!(
            context
                .world()
                .node(bar.stable_id())
                .unwrap()
                .children
                .is_empty()
        );

        context
            .update_component(bar, |bar, _| {
                bar.show_window_controls = true;
                bar.maximized = true;
            })
            .unwrap();
        assert!(
            !context
                .world()
                .node_style(controls.stable_id())
                .unwrap()
                .layout
                .hidden
        );
        assert_eq!(
            icon_of(context.world().standard_visual(maximize.stable_id())),
            Some(Icon::Restore)
        );
    }

    #[test]
    fn app_shell_stacks_title_then_fill_body_with_overlay_out_of_flow() {
        let mut context = AppContext::new();
        let title = context
            .create_component(document(), AppTitleBar::new("Nana"))
            .unwrap();
        let body = context
            .create_component(document(), Text::new("workspace"))
            .unwrap();
        let overlay = context
            .create_component(document(), Text::new("overlay"))
            .unwrap();
        let shell = context
            .create_component(
                document(),
                AppShell::new()
                    .title_bar(title.stable_id())
                    .body(body.stable_id())
                    .overlay(overlay.stable_id()),
            )
            .unwrap();
        context.append_child(shell, title).unwrap();
        context.append_child(shell, body).unwrap();
        context.append_child(shell, overlay).unwrap();

        assert_eq!(
            context.world().node(shell.stable_id()).unwrap().kind,
            NodeKind::Element {
                tag: "app-shell".into(),
            }
        );
        assert_eq!(
            context.world().node(shell.stable_id()).unwrap().children,
            vec![title.stable_id(), body.stable_id(), overlay.stable_id()]
        );
        let title_layout = &context
            .world()
            .node_style(title.stable_id())
            .unwrap()
            .layout;
        assert_eq!(title_layout.height, Some(LengthSpec::Px(TITLE_BAR_HEIGHT)));
        assert_eq!(title_layout.flex_grow, Some(0.0));
        let shell_layout = &context
            .world()
            .node_style(shell.stable_id())
            .unwrap()
            .layout;
        assert!(
            shell_layout.padding.is_none()
                && shell_layout.padding_left.is_none()
                && shell_layout.padding_right.is_none()
                && shell_layout.padding_top.is_none()
                && shell_layout.padding_bottom.is_none()
        );
        let body_layout = &context.world().node_style(body.stable_id()).unwrap().layout;
        assert_eq!(body_layout.flex_grow, Some(1.0));
        assert_eq!(body_layout.height, Some(LengthSpec::Fill));
        assert_eq!(body_layout.min_height, Some(LengthSpec::Px(0.0)));
        assert_eq!(body_layout.position, PositionSpec::Static);
        let overlay_layout = &context
            .world()
            .node_style(overlay.stable_id())
            .unwrap()
            .layout;
        assert_eq!(overlay_layout.position, PositionSpec::Absolute);
        assert_eq!(overlay_layout.width, Some(LengthSpec::Fill));
        assert_eq!(overlay_layout.height, Some(LengthSpec::Fill));
        assert_eq!(overlay_layout.flex_grow, Some(0.0));

        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();
        let title_box = context.world().layout_box(title.stable_id()).unwrap();
        let body_box = context.world().layout_box(body.stable_id()).unwrap();
        let overlay_box = context.world().layout_box(overlay.stable_id()).unwrap();
        let shell_box = context.world().layout_box(shell.stable_id()).unwrap();
        assert_eq!(title_box.height, TITLE_BAR_HEIGHT);
        assert_eq!(title_box.y, shell_box.y);
        assert_eq!(body_box.y, title_box.y + title_box.height);
        assert_eq!(body_box.height, 400.0 - TITLE_BAR_HEIGHT);
        assert_eq!(overlay_box.x, shell_box.x);
        assert_eq!(overlay_box.y, shell_box.y);
        assert_eq!(overlay_box.width, shell_box.width);
        assert_eq!(overlay_box.height, shell_box.height);
        assert_eq!(shell_box.height, 400.0);
    }

    #[test]
    fn title_bar_drops_css_layout_background() {
        let mut context = AppContext::new();
        let mut layout = nana_ui_core::LayoutStyle::default();
        layout.background = Some([1.0, 0.0, 0.0, 1.0]);
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("Nana").style(NodeStyle {
                    layout: Arc::new(layout),
                    ..NodeStyle::default()
                }),
            )
            .unwrap();
        let style = context.world().node_style(bar.stable_id()).unwrap();
        assert_eq!(style.background, Some(SemanticColorRole::Titlebar));
        assert!(
            style.layout.background.is_none(),
            "CSS fill must not cover Titlebar token alpha"
        );
    }

    #[test]
    fn app_shell_default_background_is_none_and_title_bar_keeps_surface() {
        let mut context = AppContext::new();
        let title = context
            .create_component(document(), AppTitleBar::new("Nana"))
            .unwrap();
        let shell = context
            .create_component(document(), AppShell::new().title_bar(title.stable_id()))
            .unwrap();
        context.append_child(shell, title).unwrap();

        assert_eq!(
            context
                .world()
                .node_style(shell.stable_id())
                .unwrap()
                .background,
            None
        );
        assert_eq!(
            context
                .world()
                .node_style(title.stable_id())
                .unwrap()
                .background,
            Some(SemanticColorRole::Titlebar)
        );
    }

    #[test]
    fn app_shell_preserves_caller_set_background() {
        let mut context = AppContext::new();
        let shell = context
            .create_component(
                document(),
                AppShell::new().style(NodeStyle {
                    background: Some(SemanticColorRole::Background),
                    ..NodeStyle::default()
                }),
            )
            .unwrap();

        assert_eq!(
            context
                .world()
                .node_style(shell.stable_id())
                .unwrap()
                .background,
            Some(SemanticColorRole::Background)
        );
    }

    #[test]
    fn idle_project_does_not_dirty() {
        let mut context = AppContext::new();
        let center = context
            .create_component(document(), Text::new("crumbs"))
            .unwrap();
        let body = context
            .create_component(document(), Text::new("workspace"))
            .unwrap();
        let bar = context
            .create_component(
                document(),
                AppTitleBar::new("Nana").center(center.stable_id()),
            )
            .unwrap();
        let shell = context
            .create_component(
                document(),
                AppShell::new()
                    .title_bar(bar.stable_id())
                    .body(body.stable_id()),
            )
            .unwrap();
        let _ = context.take_system_work();
        context.reproject_component(bar).unwrap();
        context.reproject_component(shell).unwrap();
        assert!(context.take_system_work().is_empty());
    }

    #[test]
    fn assemble_app_shell_reconciles_title_bar_and_body() {
        let mut context = AppContext::new();
        let title = context
            .create_detached_component(document(), AppTitleBar::new("Nana"))
            .unwrap();
        let body = context
            .create_detached_component(document(), Text::new("workspace"))
            .unwrap();
        let shell = context
            .create_component(
                document(),
                AppShell::new()
                    .title_bar(title.stable_id())
                    .body(body.stable_id()),
            )
            .unwrap();

        assert!(context.assemble_app_shell(shell).unwrap());
        assert_eq!(
            context.world().node(shell.stable_id()).unwrap().children,
            vec![title.stable_id(), body.stable_id()]
        );
        let title_layout = &context
            .world()
            .node_style(title.stable_id())
            .unwrap()
            .layout;
        assert_eq!(title_layout.height, Some(LengthSpec::Px(TITLE_BAR_HEIGHT)));
        assert_eq!(
            title_layout.min_height,
            Some(LengthSpec::Px(TITLE_BAR_HEIGHT))
        );
        assert_eq!(title_layout.flex_grow, Some(0.0));
        let body_layout = &context.world().node_style(body.stable_id()).unwrap().layout;
        assert_eq!(body_layout.flex_grow, Some(1.0));
        assert_eq!(body_layout.height, Some(LengthSpec::Fill));
        assert_eq!(body_layout.min_height, Some(LengthSpec::Px(0.0)));
        assert!(
            context
                .read(shell, |shell| shell.overlay)
                .unwrap()
                .is_none()
        );
        assert_eq!(context.world().text(title.stable_id()), Some(""));
        assert_eq!(
            context
                .world()
                .accessibility(title.stable_id())
                .and_then(|state| state.label.as_deref()),
            Some("Nana")
        );
        let columns = context.world().node(title.stable_id()).unwrap().children;
        assert_eq!(
            node_tag(context.world(), columns[1]).as_deref(),
            Some(CENTER_COLUMN_TAG)
        );
        let title_label = context
            .world()
            .node(columns[1])
            .unwrap()
            .children
            .first()
            .copied()
            .expect("center column title");
        assert_eq!(context.world().text(title_label), Some("Nana"));
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        assert!(
            !is_descendant(context.world(), title.stable_id(), body.stable_id()),
            "body must stay a sibling of the title bar, not a descendant"
        );
        let title_box = context.world().layout_box(title.stable_id()).unwrap();
        let body_box = context.world().layout_box(body.stable_id()).unwrap();
        assert!(
            body_box.y + 0.5 >= title_box.y + TITLE_BAR_HEIGHT,
            "body.y={} must sit below title bar y={} height={}",
            body_box.y,
            title_box.y,
            title_box.height
        );
    }

    #[test]
    fn assemble_app_shell_keeps_nested_body_out_of_title_bar() {
        let mut context = AppContext::new();
        let title = context
            .create_detached_component(document(), AppTitleBar::new("Nana"))
            .unwrap();
        let body = context
            .create_detached_component(document(), Card::new())
            .unwrap();
        let shell = context
            .create_component(document(), AppShell::new().title_bar(title.stable_id()))
            .unwrap();
        context.append_child(shell, title).unwrap();
        context.append_child(title, body).unwrap();

        assert!(context.assemble_app_shell(shell).unwrap());
        assert_eq!(
            context.world().node(shell.stable_id()).unwrap().children,
            vec![title.stable_id(), body.stable_id()]
        );
        assert_eq!(
            context.read(shell, |shell| shell.body).unwrap(),
            Some(body.stable_id())
        );
        assert!(
            !is_descendant(context.world(), title.stable_id(), body.stable_id()),
            "nested AppShell body must be lifted out of the title bar"
        );
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        let title_box = context.world().layout_box(title.stable_id()).unwrap();
        let body_box = context.world().layout_box(body.stable_id()).unwrap();
        assert!(
            body_box.y + 0.5 >= title_box.y + TITLE_BAR_HEIGHT,
            "body.y={} must sit below title bar y={} height={}",
            body_box.y,
            title_box.y,
            title_box.height
        );
        let leading = context
            .world()
            .node(title.stable_id())
            .unwrap()
            .children
            .iter()
            .copied()
            .find(|&id| node_tag(context.world(), id).as_deref() == Some(LEADING_COLUMN_TAG))
            .expect("leading column");
        assert!(
            !context
                .world()
                .node(leading)
                .unwrap()
                .children
                .contains(&body.stable_id()),
            "body must not land in the title-bar leading column"
        );
    }

    #[test]
    fn assemble_app_shell_empty_title_bar_still_has_columns() {
        let mut context = AppContext::new();
        let title = context
            .create_detached_component(document(), AppTitleBar::new(""))
            .unwrap();
        let body = context
            .create_detached_component(document(), Text::new("workspace"))
            .unwrap();
        let shell = context
            .create_component(
                document(),
                AppShell::new()
                    .title_bar(title.stable_id())
                    .body(body.stable_id()),
            )
            .unwrap();

        assert!(context.assemble_app_shell(shell).unwrap());
        assert_eq!(context.world().text(title.stable_id()), Some(""));
        let columns = context.world().node(title.stable_id()).unwrap().children;
        assert_eq!(columns.len(), 3, "empty title still uses three columns");
        assert_eq!(
            node_tag(context.world(), columns[0]).as_deref(),
            Some(LEADING_COLUMN_TAG)
        );
        assert_eq!(
            node_tag(context.world(), columns[1]).as_deref(),
            Some(CENTER_COLUMN_TAG)
        );
        assert_eq!(
            node_tag(context.world(), columns[2]).as_deref(),
            Some(TRAILING_COLUMN_TAG)
        );
        let title_label = context
            .world()
            .node(columns[1])
            .unwrap()
            .children
            .first()
            .copied()
            .expect("center column title");
        assert_eq!(context.world().text(title_label), Some(""));
    }

    #[test]
    fn assemble_app_shell_is_idempotent() {
        let mut context = AppContext::new();
        let title = context
            .create_detached_component(document(), AppTitleBar::new("Nana"))
            .unwrap();
        let body = context
            .create_detached_component(document(), Text::new("workspace"))
            .unwrap();
        let shell = context
            .create_component(
                document(),
                AppShell::new()
                    .title_bar(title.stable_id())
                    .body(body.stable_id()),
            )
            .unwrap();
        context.assemble_app_shell(shell).unwrap();
        let children = context.world().node(shell.stable_id()).unwrap().children;

        assert!(!context.assemble_app_shell(shell).unwrap());
        assert_eq!(
            context
                .read(shell, |shell| (shell.title_bar, shell.body, shell.overlay))
                .unwrap(),
            (Some(title.stable_id()), Some(body.stable_id()), None)
        );
        assert_eq!(
            context.world().node(shell.stable_id()).unwrap().children,
            children
        );
    }

    #[test]
    fn assemble_app_shell_keeps_overlay_absolute_fill() {
        let mut context = AppContext::new();
        let title = context
            .create_detached_component(document(), AppTitleBar::new("Nana"))
            .unwrap();
        let body = context
            .create_detached_component(document(), Text::new("workspace"))
            .unwrap();
        let overlay = context
            .create_detached_component(document(), Text::new("overlay"))
            .unwrap();
        let shell = context
            .create_component(
                document(),
                AppShell::new()
                    .title_bar(title.stable_id())
                    .body(body.stable_id())
                    .overlay(overlay.stable_id()),
            )
            .unwrap();

        context.assemble_app_shell(shell).unwrap();
        assert_eq!(
            context.world().node(shell.stable_id()).unwrap().children,
            vec![title.stable_id(), body.stable_id(), overlay.stable_id()]
        );
        let overlay_layout = &context
            .world()
            .node_style(overlay.stable_id())
            .unwrap()
            .layout;
        assert_eq!(overlay_layout.position, PositionSpec::Absolute);
        assert_eq!(overlay_layout.offset_top, Some(LengthSpec::Px(0.0)));
        assert_eq!(overlay_layout.offset_right, Some(LengthSpec::Px(0.0)));
        assert_eq!(overlay_layout.offset_bottom, Some(LengthSpec::Px(0.0)));
        assert_eq!(overlay_layout.offset_left, Some(LengthSpec::Px(0.0)));
        assert_eq!(overlay_layout.width, Some(LengthSpec::Fill));
        assert_eq!(overlay_layout.height, Some(LengthSpec::Fill));
        assert_eq!(overlay_layout.flex_grow, Some(0.0));
        assert_eq!(overlay_layout.z_index, Some(OVERLAY_Z_INDEX));
        assert_eq!(
            context.read(shell, |shell| shell.overlay).unwrap(),
            Some(overlay.stable_id())
        );
    }

    fn mounted_roots(context: &AppContext, document: DocumentId) -> Vec<StableNodeId> {
        context
            .world()
            .document_order(document)
            .into_iter()
            .filter(|id| {
                context.world().is_mounted(*id)
                    && context
                        .world()
                        .node(*id)
                        .is_some_and(|node| node.parent.is_none())
            })
            .collect()
    }

    fn assemble_shell(
        context: &mut AppContext,
        shell: DesktopShell,
    ) -> crate::Entity<DesktopShell> {
        let entity = context
            .create_component(document(), shell)
            .expect("desktop shell");
        context
            .assemble_desktop_shell(entity)
            .expect("assemble desktop shell");
        entity
    }

    #[test]
    fn desktop_shell_forwards_center_width_and_suppresses_mounted_controls() {
        let mut context = AppContext::new();
        let center = context
            .create_detached_component(document(), Text::new("Nana › 很长的面包屑上下文标题"))
            .unwrap();
        let shell = assemble_shell(
            &mut context,
            DesktopShell::new()
                .title("Nana")
                .title_center(center.stable_id())
                .title_center_width(420.0)
                .title_window_controls(false),
        );
        let bar = context
            .read(shell, |shell| shell.title_bar)
            .unwrap()
            .expect("shell creates a title bar");
        let snapshot = context
            .read(Entity::<AppTitleBar>::from_stable_id(bar), Clone::clone)
            .unwrap();
        assert_eq!(snapshot.center_width, 420.0);
        assert!(!snapshot.show_window_controls);
        // Only the hidden, empty native placeholder may remain mounted.
        assert!(
            snapshot
                .controls
                .is_none_or(|controls| snapshot.native_controls
                    && context.world().node(controls).unwrap().children.is_empty()
                    && context.world().node_style(controls).unwrap().layout.hidden),
            "suppressed chrome must not mount a second control strip"
        );

        context
            .layout_document(document(), LayoutViewport::new(1180.0, 760.0))
            .unwrap();
        let center_slot = context
            .world()
            .node(bar)
            .unwrap()
            .children
            .into_iter()
            .find(|&id| node_tag(context.world(), id).as_deref() == Some(CENTER_COLUMN_TAG))
            .expect("center column");
        assert_eq!(
            context.world().layout_box(center_slot).unwrap().width,
            420.0
        );
    }

    /// The shell carries the workspace-corners setting and hands it to the
    /// workspace it builds, on the first assembly and on later ones.
    #[test]
    fn desktop_shell_hands_its_corner_setting_to_its_workspace() {
        let mut context = AppContext::new();
        let primary = context
            .create_detached_component(document(), Text::new("workspace"))
            .unwrap();
        let shell = assemble_shell(
            &mut context,
            DesktopShell::new()
                .primary(primary.stable_id())
                .workspace_corners(false),
        );
        let workspace = Entity::<Workspace>::from_stable_id(
            context
                .read(shell, |shell| shell.workspace)
                .unwrap()
                .unwrap(),
        );
        let rounded = |context: &AppContext| {
            context
                .read(workspace, |workspace| workspace.workspace_corners)
                .unwrap()
        };
        assert!(!rounded(&context));
        context
            .update_component(shell, |shell, _| shell.workspace_corners = true)
            .unwrap();
        context.assemble_desktop_shell(shell).unwrap();
        assert!(rounded(&context));
    }

    #[test]
    fn desktop_shell_stacks_title_workspace_and_overlay() {
        let mut context = AppContext::new();
        let title = context
            .create_detached_component(document(), AppTitleBar::new("Nana"))
            .unwrap();
        let primary = context
            .create_detached_component(document(), Text::new("workspace"))
            .unwrap();
        let overlay = context
            .create_detached_component(document(), Text::new("overlay"))
            .unwrap();
        let shell = assemble_shell(
            &mut context,
            DesktopShell::new()
                .title_bar(title.stable_id())
                .primary(primary.stable_id())
                .overlay(overlay.stable_id()),
        );
        let (workspace, overlay_host, status_host) = context
            .read(shell, |shell| {
                (shell.workspace, shell.overlay, shell.status)
            })
            .unwrap();
        let workspace = workspace.expect("workspace");
        let overlay_host = overlay_host.expect("overlay host");
        let status_host = status_host.expect("status host");

        assert_eq!(
            context.world().node(shell.stable_id()).unwrap().kind,
            NodeKind::Element {
                tag: "desktop-shell".into(),
            }
        );
        assert_eq!(
            context.world().node(shell.stable_id()).unwrap().children,
            vec![title.stable_id(), workspace, overlay_host, status_host]
        );
        let title_layout = &context
            .world()
            .node_style(title.stable_id())
            .unwrap()
            .layout;
        assert_eq!(title_layout.height, Some(LengthSpec::Px(TITLE_BAR_HEIGHT)));
        assert_eq!(title_layout.flex_grow, Some(0.0));
        let shell_layout = &context
            .world()
            .node_style(shell.stable_id())
            .unwrap()
            .layout;
        assert!(
            shell_layout.padding.is_none()
                && shell_layout.padding_left.is_none()
                && shell_layout.padding_right.is_none()
                && shell_layout.padding_top.is_none()
                && shell_layout.padding_bottom.is_none()
        );
        let body_layout = &context.world().node_style(workspace).unwrap().layout;
        assert_eq!(body_layout.flex_grow, Some(1.0));
        assert_eq!(body_layout.height, Some(LengthSpec::Fill));
        assert_eq!(body_layout.min_height, Some(LengthSpec::Px(0.0)));
        assert_eq!(body_layout.position, PositionSpec::Static);
        let overlay_layout = &context.world().node_style(overlay_host).unwrap().layout;
        assert_eq!(overlay_layout.position, PositionSpec::Absolute);
        assert_eq!(overlay_layout.width, Some(LengthSpec::Fill));
        assert_eq!(overlay_layout.height, Some(LengthSpec::Fill));
        assert_eq!(overlay_layout.flex_grow, Some(0.0));
        assert_eq!(
            context.world().node(overlay_host).unwrap().children,
            vec![overlay.stable_id()]
        );

        context
            .layout_document(document(), LayoutViewport::new(800.0, 400.0))
            .unwrap();
        let title_box = context.world().layout_box(title.stable_id()).unwrap();
        let body_box = context.world().layout_box(workspace).unwrap();
        let overlay_box = context.world().layout_box(overlay_host).unwrap();
        let shell_box = context.world().layout_box(shell.stable_id()).unwrap();
        assert_eq!(title_box.height, TITLE_BAR_HEIGHT);
        assert_eq!(title_box.y, shell_box.y);
        assert_eq!(body_box.y, title_box.y + title_box.height);
        assert_eq!(body_box.height, 400.0 - TITLE_BAR_HEIGHT);
        assert_eq!(overlay_box.x, shell_box.x);
        assert_eq!(overlay_box.y, shell_box.y);
        assert_eq!(overlay_box.width, shell_box.width);
        assert_eq!(overlay_box.height, shell_box.height);
        assert_eq!(shell_box.height, 400.0);
    }

    #[test]
    fn desktop_shell_maps_navigation_and_primary_to_workspace_slots() {
        let mut context = AppContext::new();
        let navigation = context
            .create_detached_component(document(), SidebarFrame::new())
            .unwrap();
        let primary = context
            .create_detached_component(document(), Text::new("primary"))
            .unwrap();
        let shell = assemble_shell(
            &mut context,
            DesktopShell::new()
                .navigation(navigation.stable_id())
                .primary(primary.stable_id()),
        );
        let workspace_id = context
            .read(shell, |shell| shell.workspace)
            .unwrap()
            .expect("workspace");
        let workspace = Entity::<Workspace>::from_stable_id(workspace_id);
        let (slots, middle, primary_column) = context
            .read(workspace, |workspace| {
                (
                    workspace.slots.clone(),
                    workspace.middle,
                    workspace.primary_column,
                )
            })
            .unwrap();
        assert_eq!(
            slots,
            vec![
                WorkspaceRegionSlot::new(RegionId::Resources, navigation.stable_id()),
                WorkspaceRegionSlot::new(RegionId::Primary, primary.stable_id()),
            ]
        );
        let middle = middle.expect("assembled start track");
        let primary_column = primary_column.expect("assembled primary column");
        assert_eq!(
            context.world().node(middle).unwrap().children,
            vec![navigation.stable_id(), primary_column]
        );
        assert!(
            context
                .world()
                .node(workspace_id)
                .unwrap()
                .children
                .contains(&middle)
        );
        let nav_layout = &context
            .world()
            .node_style(navigation.stable_id())
            .unwrap()
            .layout;
        assert_eq!(nav_layout.padding_left, Some(LengthSpec::Px(12.0)));
        assert_eq!(nav_layout.padding_right, Some(LengthSpec::Px(8.0)));
        assert_eq!(nav_layout.padding_top, Some(LengthSpec::Px(10.0)));
        assert_eq!(nav_layout.padding_bottom, Some(LengthSpec::Px(10.0)));
        assert_eq!(nav_layout.gap, Some(LengthSpec::Px(14.0)));
        assert_eq!(nav_layout.width, Some(LengthSpec::Px(260.0)));
    }

    #[test]
    fn desktop_shell_keeps_dragged_sidebar_size_after_assemble() {
        let mut context = AppContext::new();
        let navigation = context
            .create_detached_component(document(), SidebarFrame::new())
            .unwrap();
        let primary = context
            .create_detached_component(document(), Text::new("primary"))
            .unwrap();
        let shell = assemble_shell(
            &mut context,
            DesktopShell::new()
                .navigation(navigation.stable_id())
                .primary(primary.stable_id()),
        );
        let workspace_id = context
            .read(shell, |shell| shell.workspace)
            .unwrap()
            .expect("workspace");
        let workspace = Entity::<Workspace>::from_stable_id(workspace_id);
        let handle = context
            .read(workspace, |workspace| {
                workspace.handles.get(&RegionId::Resources).copied()
            })
            .unwrap()
            .expect("resources handle");
        assert!(
            context
                .begin_workspace_resize(
                    document(),
                    1,
                    handle,
                    260.0,
                    20.0,
                    std::time::Duration::ZERO
                )
                .unwrap()
        );
        assert!(
            context
                .update_workspace_resize(document(), 1, 320.0, 20.0, std::time::Duration::ZERO)
                .unwrap()
        );
        assert!(
            context
                .end_workspace_resize(document(), 1, std::time::Duration::ZERO)
                .unwrap()
        );
        assert_eq!(
            context
                .read(shell, |shell| shell
                    .model
                    .region_extent(&RegionId::Resources))
                .unwrap(),
            320.0
        );
        context.assemble_desktop_shell(shell).unwrap();
        assert_eq!(
            context
                .read(workspace, |workspace| workspace
                    .region_extent(&RegionId::Resources))
                .unwrap(),
            320.0
        );
        assert_eq!(
            context
                .world()
                .node_style(navigation.stable_id())
                .unwrap()
                .layout
                .width,
            Some(LengthSpec::Px(320.0))
        );
    }

    #[test]
    fn desktop_shell_omits_missing_inspector_and_bottom() {
        let mut context = AppContext::new();
        let navigation = context
            .create_detached_component(document(), SidebarFrame::new())
            .unwrap();
        let primary = context
            .create_detached_component(document(), Text::new("primary"))
            .unwrap();
        let shell = assemble_shell(
            &mut context,
            DesktopShell::new()
                .navigation(navigation.stable_id())
                .primary(primary.stable_id()),
        );
        let workspace_id = context
            .read(shell, |shell| shell.workspace)
            .unwrap()
            .expect("workspace");
        let slots = context
            .read(
                Entity::<Workspace>::from_stable_id(workspace_id),
                |workspace| workspace.slots.clone(),
            )
            .unwrap();
        assert!(
            !slots
                .iter()
                .any(|slot| slot.id == RegionId::Inspector || slot.id == RegionId::Diagnostics)
        );
        assert_eq!(slots.len(), 2);
        assert!(context.world().node(navigation.stable_id()).is_some());
        assert!(context.world().node(primary.stable_id()).is_some());
    }

    #[test]
    fn desktop_shell_reassemble_after_primary_change_keeps_single_root() {
        let mut context = AppContext::new();
        let title = context
            .create_detached_component(document(), AppTitleBar::new("Nana"))
            .unwrap();
        let first = context
            .create_detached_component(document(), Text::new("first"))
            .unwrap();
        let second = context
            .create_detached_component(document(), Text::new("second"))
            .unwrap();
        let shell = assemble_shell(
            &mut context,
            DesktopShell::new()
                .title_bar(title.stable_id())
                .primary(first.stable_id()),
        );
        let first_workspace = context
            .read(shell, |shell| shell.workspace)
            .unwrap()
            .expect("workspace");
        assert_eq!(mounted_roots(&context, document()), vec![shell.stable_id()]);

        context
            .update_component(shell, |shell, _| {
                shell.primary = Some(second.stable_id());
            })
            .unwrap();
        context.assemble_desktop_shell(shell).unwrap();
        let workspace_id = context
            .read(shell, |shell| shell.workspace)
            .unwrap()
            .expect("workspace");
        let slots = context
            .read(
                Entity::<Workspace>::from_stable_id(workspace_id),
                |workspace| workspace.slots.clone(),
            )
            .unwrap();

        assert_eq!(workspace_id, first_workspace);
        assert_eq!(mounted_roots(&context, document()), vec![shell.stable_id()]);
        assert_eq!(
            slots,
            vec![WorkspaceRegionSlot::new(
                RegionId::Primary,
                second.stable_id()
            )]
        );
        assert!(context.world().is_mounted(second.stable_id()));
        assert!(!context.world().is_mounted(first.stable_id()));
        assert_eq!(
            context.world().node(shell.stable_id()).unwrap().children[0],
            title.stable_id()
        );
    }

    #[test]
    fn set_desktop_slots_parks_previous_inspector_and_remounts_it() {
        let mut context = AppContext::new();
        let navigation = context
            .create_detached_component(document(), SidebarFrame::new())
            .unwrap();
        let primary = context
            .create_detached_component(document(), Text::new("primary"))
            .unwrap();
        let first = context
            .create_detached_component(document(), Text::new("inspector-a"))
            .unwrap();
        let second = context
            .create_detached_component(document(), Text::new("inspector-b"))
            .unwrap();
        let shell = assemble_shell(
            &mut context,
            DesktopShell::new()
                .navigation(navigation.stable_id())
                .primary(primary.stable_id())
                .inspector(first.stable_id()),
        );
        assert!(context.world().is_mounted(first.stable_id()));

        assert!(
            context
                .set_desktop_slots(shell, Some(primary.stable_id()), Some(second.stable_id()),)
                .unwrap()
        );
        assert!(context.world().is_mounted(second.stable_id()));
        assert!(!context.world().is_mounted(first.stable_id()));
        assert_eq!(
            context.world().mount_state(first.stable_id()),
            Some(MountState::Parked)
        );
        assert!(
            !context
                .world()
                .document_order(document())
                .contains(&first.stable_id())
        );

        assert!(
            context
                .set_desktop_slots(shell, Some(primary.stable_id()), Some(first.stable_id()),)
                .unwrap()
        );
        assert!(context.world().is_mounted(first.stable_id()));
        assert_eq!(
            context.world().mount_state(second.stable_id()),
            Some(MountState::Parked)
        );
        assert!(
            context
                .world()
                .document_order(document())
                .contains(&first.stable_id())
        );
        assert!(
            !context
                .set_desktop_slots(shell, Some(primary.stable_id()), Some(first.stable_id()),)
                .unwrap()
        );
    }

    #[test]
    fn set_desktop_slots_repeated_inspector_swap_reuses_parked_trees() {
        let mut context = AppContext::new();
        let navigation = context
            .create_detached_component(document(), SidebarFrame::new())
            .unwrap();
        let primary = context
            .create_detached_component(document(), Text::new("primary"))
            .unwrap();
        let first = context
            .create_detached_component(document(), Text::new("inspector-a"))
            .unwrap();
        let second = context
            .create_detached_component(document(), Text::new("inspector-b"))
            .unwrap();
        let shell = assemble_shell(
            &mut context,
            DesktopShell::new()
                .navigation(navigation.stable_id())
                .primary(primary.stable_id())
                .inspector(first.stable_id()),
        );
        for round in 0..32 {
            let inspector = if round % 2 == 0 {
                second.stable_id()
            } else {
                first.stable_id()
            };
            assert!(
                context
                    .set_desktop_slots(shell, Some(primary.stable_id()), Some(inspector))
                    .unwrap()
            );
        }
        assert!(context.world().is_mounted(first.stable_id()));
        assert_eq!(
            context.world().mount_state(second.stable_id()),
            Some(MountState::Parked)
        );
        assert!(
            !context
                .set_desktop_slots(shell, Some(primary.stable_id()), Some(first.stable_id()),)
                .unwrap()
        );
    }

    #[test]
    fn desktop_shell_keeps_title_off_trailing_edge_and_sidebar_below_chrome() {
        let mut context = AppContext::new();
        let leading = context
            .create_detached_component(
                document(),
                IconButton::new(Icon::Sidebar, "切换侧栏").size(ControlSize::Small),
            )
            .unwrap();
        let files = context
            .create_detached_component(document(), Text::new("文件"))
            .unwrap();
        let navigation = context
            .create_detached_component(document(), SidebarFrame::new().body(files.stable_id()))
            .unwrap();
        context.append_child(navigation, files).unwrap();
        let toolbar = context
            .create_detached_component(document(), Text::new("toolbar"))
            .unwrap();
        let primary = context
            .create_detached_component(document(), Text::new("primary"))
            .unwrap();
        let inspector = context
            .create_detached_component(document(), Text::new("通道"))
            .unwrap();
        let layout = WorkspaceLayout::new([
            RegionState::new(RegionId::Resources, RegionRole::Resources).size(220.0),
            RegionState::new(RegionId::PrimaryToolbar, RegionRole::Utility)
                .placement(RegionPlacement::Top)
                .scope(RegionScope::Primary)
                .size(34.0),
            RegionState::new(RegionId::Primary, RegionRole::Primary).fill_priority(1),
            RegionState::new(RegionId::Inspector, RegionRole::Inspector).size(286.0),
        ])
        .expect("editor regions");
        let shell = assemble_shell(
            &mut context,
            DesktopShell::from_model(WorkspaceModel::with_layout(layout))
                .title("NanaShader")
                .title_leading(leading.stable_id())
                .navigation(navigation.stable_id())
                .primary(primary.stable_id())
                .inspector(inspector.stable_id())
                .region(RegionId::PrimaryToolbar, toolbar.stable_id()),
        );
        context
            .layout_document(document(), LayoutViewport::new(1200.0, 800.0))
            .unwrap();

        let title_bar = context
            .read(shell, |shell| shell.title_bar)
            .unwrap()
            .expect("assembled title bar");
        assert_eq!(context.world().text(title_bar), Some(""));
        let columns = context.world().node(title_bar).unwrap().children;
        assert_eq!(columns.len(), 3);
        let title_label = context
            .world()
            .node(columns[1])
            .unwrap()
            .children
            .first()
            .copied()
            .expect("center column title");
        assert_eq!(context.world().text(title_label), Some("NanaShader"));
        let title_style = context.world().node_style(title_bar).unwrap();
        assert_eq!(
            title_style.text_horizontal_alignment,
            TextHorizontalAlignment::Center
        );

        let bar_box = context.world().layout_box(title_bar).unwrap();
        let center_box = context.world().layout_box(columns[1]).unwrap();
        let title_mid = center_box.x + center_box.width / 2.0;
        assert!(
            title_mid < bar_box.x + bar_box.width * 0.65,
            "title sat on the trailing edge at {title_mid}"
        );
        let leading_box = context.world().layout_box(leading.stable_id()).unwrap();
        assert!(
            leading_box.width < 80.0,
            "leading chrome must hug, got {}",
            leading_box.width
        );
        if !WindowChrome::platform_default().uses_custom_controls() {
            assert!(
                leading_box.x + 0.5 >= bar_box.x + NATIVE_WINDOW_CONTROLS_WIDTH,
                "toggle overlapped the traffic lights"
            );
        }

        let nav_box = context.world().layout_box(navigation.stable_id()).unwrap();
        let files_box = context.world().layout_box(files.stable_id()).unwrap();
        assert!(
            nav_box.y + 0.5 >= bar_box.y + bar_box.height,
            "sidebar entered the title-bar / traffic-light band"
        );
        assert!(
            files_box.y + 0.5 >= bar_box.y + bar_box.height,
            "文件 header overlapped native window chrome"
        );

        let toolbar_box = context.world().layout_box(toolbar.stable_id()).unwrap();
        let primary_box = context.world().layout_box(primary.stable_id()).unwrap();
        let inspector_box = context.world().layout_box(inspector.stable_id()).unwrap();
        assert!(
            inspector_box.y + 0.5 >= toolbar_box.y + toolbar_box.height,
            "inspector tabs must not sit on the window toolbar row"
        );
        assert!(
            inspector_box.x + 0.5 >= primary_box.x + primary_box.width,
            "inspector must be a right column beside the workspace"
        );
    }
}

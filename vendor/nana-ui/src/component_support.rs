//! Public, read-only component capabilities.
//!
//! This catalog projects build availability and behavior and is the single
//! source for NanaUI's internal default-backend decision. It must not be used
//! to maintain parallel application state.

use std::fmt;

/// Stable semantic identity of a NanaUI component across backend migrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentId(&'static str);

impl ComponentId {
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ComponentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// Stable behavior that consumers can rely on for a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ComponentCapability {
    Render,
    Pointer,
    Keyboard,
    Focus,
    Ime,
    Accessibility,
    Animation,
    Overlay,
    Gpu,
    Persistence,
}

/// Broad catalog grouping; this does not own application navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ComponentFamily {
    Primitive,
    Control,
    Data,
    Feedback,
    Navigation,
    Overlay,
    Workspace,
    Media,
    Gpu,
}

/// Immutable support facts for one public component identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentSupport {
    pub id: ComponentId,
    pub name: &'static str,
    pub family: ComponentFamily,
    /// Cargo feature required by the current public implementation, if any.
    pub required_feature: Option<&'static str>,
    /// Whether the required implementation is compiled in this build.
    pub compiled: bool,
    pub capabilities: &'static [ComponentCapability],
}

impl ComponentSupport {
    pub fn supports(&self, capability: ComponentCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}

macro_rules! component_catalog {
    ($(
        $constant:ident => {
            id: $id:literal,
            name: $name:literal,
            family: $family:ident,
            feature: $feature:expr,
            compiled: $compiled:expr,
            capabilities: [$($capability:ident),* $(,)?]
        }
    ),* $(,)?) => {
        /// Stable component identities. New identities are additive; existing
        /// values must not be renamed as part of a backend migration.
        pub mod component_ids {
            use super::ComponentId;

            $(pub const $constant: ComponentId = ComponentId::new($id);)*
        }

        static COMPONENT_CATALOG: &[ComponentSupport] = &[
            $(ComponentSupport {
                id: component_ids::$constant,
                name: $name,
                family: ComponentFamily::$family,
                required_feature: $feature,
                compiled: $compiled,
                capabilities: &[$(ComponentCapability::$capability),*],
            }),*
        ];
    };
}

component_catalog! {
    TEXT => { id: "text", name: "Text", family: Primitive, feature: None, compiled: true, capabilities: [Render, Accessibility] },
    BUTTON => { id: "button", name: "Button", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    TEXT_INPUT => { id: "text-input", name: "TextInput", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Ime, Accessibility] },
    CHECKBOX => { id: "checkbox", name: "Checkbox", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    ICON_BUTTON => { id: "icon-button", name: "IconButton", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    CARD => { id: "card", name: "Card", family: Primitive, feature: None, compiled: true, capabilities: [Render, Accessibility, Animation] },
    SWITCH => { id: "switch", name: "Switch", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Animation] },
    TEXTAREA => { id: "textarea", name: "Textarea", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Ime, Accessibility] },
    HOSTED_TEXTAREA => { id: "hosted-textarea", name: "HostedTextarea", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Ime, Accessibility] },
    RANGE_FIELD => { id: "range-field", name: "RangeField", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    SELECT => { id: "select", name: "Select", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    SEGMENTED_CONTROL => { id: "segmented-control", name: "SegmentedControl", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    TABS => { id: "tabs", name: "Tabs", family: Navigation, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    REORDER_LIST => { id: "reorder-list", name: "ReorderList", family: Navigation, feature: Some("controls"), compiled: nana_ui_runtime::component_descriptors::REORDER_LIST.compiled, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    CALENDAR_HEATMAP => { id: "calendar-heatmap", name: "CalendarHeatmap", family: Data, feature: Some("calendar"), compiled: nana_ui_runtime::component_descriptors::CALENDAR_HEATMAP.compiled, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    DONUT_CHART => { id: "donut-chart", name: "DonutChart", family: Data, feature: Some("charts"), compiled: nana_ui_runtime::component_descriptors::DONUT_CHART.compiled, capabilities: [Render, Accessibility, Pointer, Overlay] },
    TIME_SERIES_CHART => { id: "time-series-chart", name: "TimeSeriesChart", family: Data, feature: Some("charts"), compiled: nana_ui_runtime::component_descriptors::TIME_SERIES_CHART.compiled, capabilities: [Render, Accessibility, Pointer, Overlay] },
    PROGRESS => { id: "progress", name: "Progress", family: Feedback, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    SPINNER => { id: "spinner", name: "Spinner", family: Feedback, feature: None, compiled: true, capabilities: [Render, Animation, Accessibility] },
    SKELETON => { id: "skeleton", name: "Skeleton", family: Feedback, feature: None, compiled: true, capabilities: [Render, Animation] },
    LEVEL_METER => { id: "level-meter", name: "LevelMeter", family: Feedback, feature: None, compiled: true, capabilities: [Render, Accessibility] },
    STATUS_BADGE => { id: "status-badge", name: "StatusBadge", family: Feedback, feature: None, compiled: true, capabilities: [Render, Accessibility] },
    VALIDATION_MESSAGE => { id: "validation-message", name: "ValidationMessage", family: Feedback, feature: None, compiled: true, capabilities: [Render, Accessibility] },
    TOAST => { id: "toast", name: "Toast", family: Feedback, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    COMMAND_PALETTE => { id: "command-palette", name: "CommandPalette", family: Overlay, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Ime, Accessibility, Overlay] },
    DIALOG => { id: "dialog", name: "Dialog", family: Overlay, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    CONFIRM_DIALOG => { id: "confirm-dialog", name: "ConfirmDialog", family: Overlay, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    DRAWER => { id: "drawer", name: "Drawer", family: Overlay, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    TOOLTIP => { id: "tooltip", name: "Tooltip", family: Overlay, feature: None, compiled: true, capabilities: [Render, Accessibility, Overlay] },
    POPOVER => { id: "popover", name: "Popover", family: Overlay, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    ACTION_MENU => { id: "action-menu", name: "ActionMenu", family: Overlay, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    CONTEXT_MENU => { id: "context-menu", name: "ContextMenu", family: Overlay, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    ACTION_MENU_ITEM => { id: "action-menu-item", name: "ActionMenuItem", family: Overlay, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    ANCHORED_ACTION_MENU => { id: "anchored-action-menu", name: "AnchoredActionMenu", family: Overlay, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    OVERLAY_HOST => { id: "overlay-host", name: "OverlayHost", family: Overlay, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    DROPDOWN => { id: "dropdown", name: "Dropdown", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Overlay] },
    SEARCH_DROPDOWN => { id: "search-dropdown", name: "SearchDropdown", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Ime, Accessibility, Overlay] },
    TREE_VIEW => { id: "tree-view", name: "TreeView", family: Navigation, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    SIDEBAR_FRAME => { id: "sidebar-frame", name: "SidebarFrame", family: Navigation, feature: None, compiled: true, capabilities: [Render, Accessibility] },
    SIDEBAR_SECTION => { id: "sidebar-section", name: "SidebarSection", family: Navigation, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Animation] },
    SIDEBAR_ROW => { id: "sidebar-row", name: "SidebarRow", family: Navigation, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    SIDEBAR_FOOTER => { id: "sidebar-footer", name: "SidebarFooter", family: Navigation, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    KEY_CAPTURE_LAYER => { id: "key-capture-layer", name: "KeyCaptureLayer", family: Control, feature: None, compiled: true, capabilities: [Keyboard, Focus, Accessibility] },
    KEYMAP_LAYER => { id: "keymap-layer", name: "KeymapLayer", family: Control, feature: None, compiled: true, capabilities: [Keyboard, Focus] },
    NATIVE_MARKDOWN => { id: "native-markdown", name: "NativeMarkdown", family: Data, feature: Some("rich-text"), compiled: nana_ui_runtime::component_descriptors::NATIVE_MARKDOWN.compiled, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    SELECTABLE_RICH_TEXT => { id: "selectable-rich-text", name: "SelectableRichText", family: Data, feature: Some("rich-text"), compiled: nana_ui_runtime::component_descriptors::NATIVE_MARKDOWN.compiled, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    QR_CODE => { id: "qr-code", name: "QrCodeCanvas", family: Data, feature: None, compiled: true, capabilities: [Render, Accessibility] },
    GRAPH_CANVAS => { id: "graph-canvas", name: "GraphCanvas", family: Data, feature: Some("graph-canvas"), compiled: nana_ui_runtime::component_descriptors::GRAPH_CANVAS.compiled, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Gpu] },
    GRAPH_MINIMAP => { id: "graph-minimap", name: "GraphMinimap", family: Data, feature: Some("graph-canvas"), compiled: nana_ui_runtime::component_descriptors::GRAPH_CANVAS.compiled, capabilities: [Render, Pointer, Accessibility] },
    IMAGE_VIEWER => { id: "image-viewer", name: "ImageViewer", family: Media, feature: Some("image-viewer"), compiled: nana_ui_runtime::component_descriptors::IMAGE_VIEWER.compiled, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    XY_PAD => { id: "xy-pad", name: "XYPad", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    FORM_FIELD => { id: "form-field", name: "FormField", family: Control, feature: None, compiled: true, capabilities: [Render, Accessibility] },
    LABELED_VALUE => { id: "labeled-value", name: "LabeledValue", family: Data, feature: None, compiled: true, capabilities: [Render, Accessibility] },
    EMPTY_STATE => { id: "empty-state", name: "EmptyState", family: Feedback, feature: None, compiled: true, capabilities: [Render, Accessibility] },
    INTERACTIVE_CARD => { id: "interactive-card", name: "InteractiveCard", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    LIST_ITEM => { id: "list-item", name: "ListItem", family: Navigation, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    THUMBNAIL => { id: "thumbnail", name: "Thumbnail", family: Primitive, feature: None, compiled: true, capabilities: [Render, Gpu, Animation, Accessibility] },
    CHIP => { id: "chip", name: "Chip", family: Control, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    AVATAR => { id: "avatar", name: "Avatar", family: Primitive, feature: None, compiled: true, capabilities: [Render, Gpu, Accessibility] },
    DOCK_PANEL => { id: "dock-panel", name: "DockPanel", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Persistence] },
    WORKSPACE => { id: "workspace", name: "Workspace", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Animation, Persistence] },
    DOCK => { id: "dock", name: "Dock", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Animation, Persistence] },
    SPLIT_PANE => { id: "split-pane", name: "SplitPane", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Persistence] },
    PANE_CHROME => { id: "pane-chrome", name: "PaneChrome", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    PANE_TREE => { id: "pane-tree", name: "PaneTree", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Persistence] },
    APP_SHELL => { id: "app-shell", name: "AppShell", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    APP_TITLE_BAR => { id: "app-title-bar", name: "AppTitleBar", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility] },
    SETTINGS => { id: "settings", name: "Settings", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Persistence] },
    APPEARANCE_SECTION => { id: "appearance-section", name: "AppearanceSection", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Persistence] },
    ABOUT_SECTION => { id: "about-section", name: "AboutSection", family: Workspace, feature: None, compiled: true, capabilities: [Render, Accessibility] },
    SETTINGS_COLLAPSIBLE_CARD => { id: "settings-collapsible-card", name: "SettingsCollapsibleCard", family: Workspace, feature: None, compiled: true, capabilities: [Render, Pointer, Keyboard, Focus, Accessibility, Persistence] },
    GPU_VIEW => { id: "gpu-view", name: "GpuView", family: Gpu, feature: Some("gpu"), compiled: cfg!(feature = "gpu"), capabilities: [Render, Pointer, Gpu] },
    GPU_TEXTURE_VIEW => { id: "gpu-texture-view", name: "GpuTextureView", family: Gpu, feature: Some("gpu"), compiled: cfg!(feature = "gpu"), capabilities: [Render, Gpu] },
}

/// Complete catalog for this NanaUI build.
pub const fn component_catalog() -> &'static [ComponentSupport] {
    COMPONENT_CATALOG
}

/// Look up support facts without selecting or changing a renderer.
pub fn component_support(id: ComponentId) -> Option<&'static ComponentSupport> {
    COMPONENT_CATALOG.iter().find(|support| support.id == id)
}

/// Internal default-backend routing derived from the same declaration as the
/// public catalog. Availability follows the active build features.
#[doc(hidden)]
pub fn component_uses_runtime(id: ComponentId) -> bool {
    component_support(id).is_some_and(|support| support.compiled)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn catalog_has_unique_stable_identities_and_real_capabilities() {
        let mut ids = BTreeSet::new();
        for support in component_catalog() {
            assert!(!support.id.as_str().is_empty());
            assert!(!support.name.is_empty());
            assert!(!support.capabilities.is_empty());
            assert!(ids.insert(support.id));
            assert_eq!(component_support(support.id), Some(support));
        }
    }

    #[test]
    fn qualified_components_route_only_reviewed_runtime_paths() {
        for id in [
            component_ids::TEXT,
            component_ids::BUTTON,
            component_ids::TEXT_INPUT,
            component_ids::CHECKBOX,
            component_ids::ICON_BUTTON,
            component_ids::SWITCH,
            component_ids::CARD,
            component_ids::LIST_ITEM,
            component_ids::THUMBNAIL,
            component_ids::CHIP,
            component_ids::AVATAR,
            component_ids::RANGE_FIELD,
            component_ids::SEGMENTED_CONTROL,
            component_ids::STATUS_BADGE,
            component_ids::VALIDATION_MESSAGE,
            component_ids::EMPTY_STATE,
            component_ids::LABELED_VALUE,
            component_ids::PROGRESS,
            component_ids::SPINNER,
            component_ids::TABS,
            component_ids::SKELETON,
            component_ids::LEVEL_METER,
            component_ids::FORM_FIELD,
            component_ids::INTERACTIVE_CARD,
            component_ids::TEXTAREA,
            component_ids::HOSTED_TEXTAREA,
            component_ids::REORDER_LIST,
            component_ids::CALENDAR_HEATMAP,
            component_ids::TIME_SERIES_CHART,
            component_ids::KEY_CAPTURE_LAYER,
            component_ids::KEYMAP_LAYER,
            component_ids::NATIVE_MARKDOWN,
            component_ids::SELECTABLE_RICH_TEXT,
            component_ids::IMAGE_VIEWER,
            component_ids::GRAPH_CANVAS,
            component_ids::GPU_VIEW,
            component_ids::GPU_TEXTURE_VIEW,
            component_ids::TOOLTIP,
            component_ids::DIALOG,
            component_ids::CONFIRM_DIALOG,
            component_ids::DRAWER,
            component_ids::TOAST,
            component_ids::XY_PAD,
            component_ids::QR_CODE,
            component_ids::SELECT,
            component_ids::POPOVER,
            component_ids::ACTION_MENU,
            component_ids::ACTION_MENU_ITEM,
            component_ids::ANCHORED_ACTION_MENU,
            component_ids::CONTEXT_MENU,
            component_ids::OVERLAY_HOST,
            component_ids::DROPDOWN,
            component_ids::SEARCH_DROPDOWN,
            component_ids::COMMAND_PALETTE,
            component_ids::TREE_VIEW,
            component_ids::SIDEBAR_ROW,
            component_ids::SETTINGS,
            component_ids::SIDEBAR_FRAME,
            component_ids::SIDEBAR_SECTION,
            component_ids::SIDEBAR_FOOTER,
            component_ids::APPEARANCE_SECTION,
            component_ids::ABOUT_SECTION,
            component_ids::SETTINGS_COLLAPSIBLE_CARD,
            component_ids::WORKSPACE,
            component_ids::DOCK,
            component_ids::DOCK_PANEL,
            component_ids::SPLIT_PANE,
            component_ids::PANE_CHROME,
            component_ids::PANE_TREE,
            component_ids::APP_SHELL,
            component_ids::APP_TITLE_BAR,
        ] {
            let support = component_support(id).expect("qualified component is cataloged");

            assert_eq!(component_uses_runtime(id), support.compiled);
        }
    }

    #[test]
    fn hosted_textarea_public_export_is_the_runtime_highlighter() {
        let _hosted = component_support(component_ids::HOSTED_TEXTAREA)
            .expect("hosted textarea is cataloged");

        assert!(component_uses_runtime(component_ids::HOSTED_TEXTAREA));
        let _: nana_ui_runtime::HostedTextarea =
            crate::runtime::HostedTextarea::new("fn main() {}", "rs");
        let _: nana_ui_runtime::HostedTextarea =
            crate::components::HostedTextarea::new("fn main() {}", "rs");
        let _: nana_ui_runtime::KeyCaptureLayer = crate::runtime::KeyCaptureLayer::new();
        let _: nana_ui_runtime::KeymapLayer = crate::runtime::KeymapLayer::new(
            nana_ui_runtime::Keymap::new([]),
            nana_ui_core::KeyContext::default(),
            nana_ui_runtime::ActionRegistry::new(),
        );
    }

    #[cfg(all(
        feature = "calendar",
        feature = "charts",
        feature = "controls",
        feature = "rich-text",
        feature = "image-viewer"
    ))]
    #[test]
    #[cfg(feature = "calendar")]
    #[cfg(feature = "charts")]
    #[cfg(feature = "controls")]
    #[cfg(feature = "rich-text")]
    #[cfg(feature = "image-viewer")]
    fn candidate_cutover_public_exports_include_new_runtime_leaves() {
        let _: nana_ui_runtime::CalendarHeatmap = crate::runtime::CalendarHeatmap::new([]);
        let _: nana_ui_runtime::TimeSeriesChart = crate::runtime::TimeSeriesChart::new([1.0]);
        let _: nana_ui_runtime::ReorderList = crate::runtime::ReorderList::new([]);
        let _: nana_ui_runtime::NativeMarkdown = crate::runtime::NativeMarkdown::new();
        let _: nana_ui_runtime::SelectableRichText = crate::runtime::SelectableRichText::new([]);
        let _: nana_ui_runtime::ImageViewer =
            crate::runtime::ImageViewer::new(nana_ui_runtime::ImageViewerContent::None);
        let _: nana_ui_runtime::CalendarHeatmap = crate::components::CalendarHeatmap::new([]);
        let _: nana_ui_runtime::NativeMarkdown = crate::components::NativeMarkdown::new();
    }

    #[cfg(all(feature = "graph-canvas", feature = "gpu"))]
    #[test]
    #[cfg(feature = "graph-canvas")]
    fn graph_and_gpu_public_exports_are_runtime_components() {
        let _: nana_ui_runtime::GraphCanvas =
            crate::runtime::GraphCanvas::new("main", nana_ui_core::GraphModel::empty());
        let _: nana_ui_runtime::GraphMinimap =
            crate::runtime::GraphMinimap::new(nana_ui_core::GraphModel::empty());
        let _: nana_ui_runtime::GpuView = crate::runtime::GpuView::new(1);
        let _: nana_ui_runtime::GpuTextureView = crate::runtime::GpuTextureView::new("slot");
        let _: nana_ui_runtime::Thumbnail = crate::runtime::Thumbnail::empty();
        let _: nana_ui_runtime::Thumbnail = crate::components::Thumbnail::empty();
        let _: nana_ui_runtime::GraphCanvas =
            crate::components::GraphCanvas::new("main", nana_ui_core::GraphModel::empty());
    }

    #[test]
    fn first_batch_public_exports_are_runtime_components() {
        let _: nana_ui_runtime::Text = crate::runtime::Text::new("Status");
        let _: nana_ui_runtime::Button = crate::runtime::Button::new("Run");
        let _: nana_ui_runtime::TextInput = crate::runtime::TextInput::new("main");
        let _: nana_ui_runtime::Checkbox = crate::runtime::Checkbox::new("Enabled", true);

        let _: nana_ui_runtime::Text = crate::components::Text::new("Status");
        let _: nana_ui_runtime::Button = crate::components::Button::new("Run");
        let _: nana_ui_runtime::TextInput = crate::components::TextInput::new("main");
        let _: nana_ui_runtime::Checkbox = crate::components::Checkbox::new("Enabled", true);
    }

    #[test]
    fn third_batch_public_exports_are_runtime_components() {
        let _: nana_ui_runtime::StatusBadge =
            crate::runtime::StatusBadge::new("Ready", nana_ui_runtime::StatusTone::Neutral);
        let _: nana_ui_runtime::ValidationMessage = crate::runtime::ValidationMessage::new(
            "Required",
            nana_ui_runtime::ValidationIntent::Danger,
        );
        let _: nana_ui_runtime::EmptyState = crate::runtime::EmptyState::new("Nothing here");
        let _: nana_ui_runtime::LabeledValue = crate::runtime::LabeledValue::new("Revision", "42");
        let _: nana_ui_runtime::SegmentedControl = crate::runtime::SegmentedControl::new();

        let _: nana_ui_runtime::StatusBadge =
            crate::components::StatusBadge::new("Ready", nana_ui_runtime::StatusTone::Neutral);
        let _: nana_ui_runtime::ValidationMessage = crate::components::ValidationMessage::new(
            "Required",
            nana_ui_runtime::ValidationIntent::Danger,
        );
        let _: nana_ui_runtime::EmptyState = crate::components::EmptyState::new("Nothing here");
        let _: nana_ui_runtime::LabeledValue =
            crate::components::LabeledValue::new("Revision", "42");
        let _: nana_ui_runtime::SegmentedControl = crate::components::SegmentedControl::new();
    }

    #[test]
    fn fourth_batch_public_exports_are_runtime_components() {
        let _: nana_ui_runtime::Progress = crate::runtime::Progress::new(1.0, 2.0);
        let _: nana_ui_runtime::Spinner = crate::runtime::Spinner::new("Loading");
        let _: nana_ui_runtime::Thumbnail = crate::runtime::Thumbnail::empty();
        let _: nana_ui_runtime::Skeleton =
            crate::runtime::Skeleton::new(nana_ui_core::LengthSpec::Fill, 16.0);
        let _: nana_ui_runtime::LevelMeter = crate::runtime::LevelMeter::new(0.5);
        let _: nana_ui_runtime::FormField = crate::runtime::FormField::new("Name");
        let _: nana_ui_runtime::InteractiveCard = crate::runtime::InteractiveCard::new();
        let _: nana_ui_runtime::Tabs = crate::runtime::Tabs::new("code");

        let _: nana_ui_runtime::Progress = crate::components::Progress::new(1.0, 2.0);
        let _: nana_ui_runtime::Spinner = crate::components::Spinner::new("Loading");
        let _: nana_ui_runtime::Thumbnail = crate::components::Thumbnail::empty();
        let _: nana_ui_runtime::Skeleton =
            crate::components::Skeleton::new(nana_ui_core::LengthSpec::Fill, 16.0);
        let _: nana_ui_runtime::LevelMeter = crate::components::LevelMeter::new(0.5);
        let _: nana_ui_runtime::FormField = crate::components::FormField::new("Name");
        let _: nana_ui_runtime::InteractiveCard = crate::components::InteractiveCard::new();
        let _: nana_ui_runtime::Tabs = crate::components::Tabs::new("code");
    }

    #[test]
    fn candidate_cutover_public_exports_are_runtime_components() {
        let _: nana_ui_runtime::TextArea = crate::runtime::TextArea::new("notes");
        let _: nana_ui_runtime::Tooltip = crate::runtime::Tooltip::new("Hint");
        let _: nana_ui_runtime::Dialog = crate::runtime::Dialog::new("Rename");
        let _: nana_ui_runtime::ConfirmDialog =
            crate::runtime::ConfirmDialog::new("Delete", "This cannot be undone.");
        let _: nana_ui_runtime::Drawer = crate::runtime::Drawer::new("Inspector");
        let _: nana_ui_runtime::Toast =
            crate::runtime::Toast::new("Saved", nana_ui_runtime::ToastTone::Info);
        let _: nana_ui_runtime::XYPad =
            crate::runtime::XYPad::new(nana_ui_core::XYPadValue::new(0.5, 0.5));
        let _: nana_ui_runtime::QrCode = crate::runtime::QrCode::from_modules(vec![false], 1, 64.0)
            .expect("single module encodes");
        let _: nana_ui_runtime::Select = crate::runtime::Select::new(Some("code"));
        let _: nana_ui_runtime::Popover = crate::runtime::Popover::new();
        let _: nana_ui_runtime::ActionMenu = crate::runtime::ActionMenu::new();
        let _: nana_ui_runtime::ActionMenuItem = crate::runtime::ActionMenuItem::new("Rename");
        let _: nana_ui_runtime::AnchoredActionMenu =
            crate::runtime::AnchoredActionMenu::new(24.0, 36.0);
        let _: nana_ui_runtime::ContextMenu = crate::runtime::ContextMenu::new(24.0, 36.0);
        let _: nana_ui_runtime::OverlayHost = crate::runtime::OverlayHost::new();
        let _: nana_ui_runtime::Dropdown = crate::runtime::Dropdown::single(Some("code"));
        let _: nana_ui_runtime::SearchDropdown = crate::runtime::SearchDropdown::new(None::<&str>);
        let _: nana_ui_runtime::CommandPalette =
            crate::runtime::CommandPalette::new("命令面板", []);
        let _: nana_ui_runtime::TreeView = crate::runtime::TreeView::new([]);

        let _: nana_ui_runtime::TextArea = crate::components::TextArea::new("notes");
        let _: nana_ui_runtime::Tooltip = crate::components::Tooltip::new("Hint");
        let _: nana_ui_runtime::Dialog = crate::components::Dialog::new("Rename");
        let _: nana_ui_runtime::ConfirmDialog =
            crate::components::ConfirmDialog::new("Delete", "This cannot be undone.");
        let _: nana_ui_runtime::Drawer = crate::components::Drawer::new("Inspector");
        let _: nana_ui_runtime::Toast =
            crate::components::Toast::new("Saved", nana_ui_runtime::ToastTone::Info);
        let _: nana_ui_runtime::XYPad =
            crate::components::XYPad::new(nana_ui_core::XYPadValue::new(0.5, 0.5));
        let _: nana_ui_runtime::QrCode =
            crate::components::QrCode::from_modules(vec![false], 1, 64.0)
                .expect("single module encodes");
        let _: nana_ui_runtime::Select = crate::components::Select::new(Some("code"));
        let _: nana_ui_runtime::Popover = crate::components::Popover::new();
        let _: nana_ui_runtime::ActionMenu = crate::components::ActionMenu::new();
        let _: nana_ui_runtime::ActionMenuItem = crate::components::ActionMenuItem::new("Rename");
        let _: nana_ui_runtime::AnchoredActionMenu =
            crate::components::AnchoredActionMenu::new(24.0, 36.0);
        let _: nana_ui_runtime::ContextMenu = crate::components::ContextMenu::new(24.0, 36.0);
        let _: nana_ui_runtime::OverlayHost = crate::components::OverlayHost::new();
        let _: nana_ui_runtime::Dropdown = crate::components::Dropdown::single(Some("code"));
        let _: nana_ui_runtime::SearchDropdown =
            crate::components::SearchDropdown::new(None::<&str>);
        let _: nana_ui_runtime::CommandPalette =
            crate::components::CommandPalette::new("命令面板", []);
        let _: nana_ui_runtime::TreeView = crate::components::TreeView::new([]);
    }

    #[test]
    fn sidebar_and_settings_leaf_exports_are_runtime_components() {
        let _: nana_ui_runtime::SidebarRow = crate::runtime::SidebarRow::new("工作区");
        let _: nana_ui_runtime::SettingsRow = crate::runtime::SettingsRow::new("主题");
        let _: nana_ui_runtime::SettingsCard = crate::runtime::SettingsCard::new("外观");
        let _: nana_ui_runtime::SidebarFrame = crate::runtime::SidebarFrame::new();
        let _: nana_ui_runtime::SidebarSection = crate::runtime::SidebarSection::new("资源");
        let _: nana_ui_runtime::SidebarFooter = crate::runtime::SidebarFooter::new();
        let _: nana_ui_runtime::AppearanceSection = crate::runtime::AppearanceSection::new(
            nana_ui_core::ThemeMode::Dark,
            nana_ui_core::AppearanceSettings::default(),
        );
        let _: nana_ui_runtime::AboutSection =
            crate::runtime::AboutSection::new(nana_ui_runtime::AboutMetadata::new("NanaUI", "0"));
        let _: nana_ui_runtime::SettingsCollapsibleCard =
            crate::runtime::SettingsCollapsibleCard::new(true);

        let _: nana_ui_runtime::SidebarRow = crate::components::SidebarRow::new("工作区");
        let _: nana_ui_runtime::SettingsRow = crate::components::SettingsRow::new("主题");
        let _: nana_ui_runtime::SettingsCard = crate::components::SettingsCard::new("外观");
        let _: nana_ui_runtime::SidebarFrame = crate::components::SidebarFrame::new();
        let _: nana_ui_runtime::SidebarSection = crate::components::SidebarSection::new("资源");
        let _: nana_ui_runtime::SidebarFooter = crate::components::SidebarFooter::new();
        let _: nana_ui_runtime::AppearanceSection = crate::components::AppearanceSection::new(
            nana_ui_core::ThemeMode::Dark,
            nana_ui_core::AppearanceSettings::default(),
        );
        let _: nana_ui_runtime::AboutSection = crate::components::AboutSection::new(
            nana_ui_runtime::AboutMetadata::new("NanaUI", "0"),
        );
        let _: nana_ui_runtime::SettingsCollapsibleCard =
            crate::components::SettingsCollapsibleCard::new(true);
    }

    #[test]
    fn workspace_family_public_exports_are_runtime_components() {
        let _: nana_ui_runtime::Workspace = crate::runtime::Workspace::new();
        let _: nana_ui_runtime::Dock =
            crate::runtime::Dock::new(nana_ui_runtime::DockNode::item("main", None));
        let _: nana_ui_runtime::DockPanel = crate::runtime::DockPanel::new();
        let _: fn(
            &nana_ui_core::SplitPaneModel,
            nana_ui_runtime::StableNodeId,
            nana_ui_runtime::StableNodeId,
        ) -> nana_ui_runtime::SplitPane = crate::runtime::SplitPane::from_model;
        let _: nana_ui_runtime::PaneChrome = crate::runtime::PaneChrome::new();
        let _: nana_ui_runtime::PaneTree =
            crate::runtime::PaneTree::new(nana_ui_runtime::PaneTreeNode::leaf("editor"));
        let _: nana_ui_runtime::AppShell = crate::runtime::AppShell::new();
        let _: nana_ui_runtime::AppTitleBar = crate::runtime::AppTitleBar::new("NanaUI");
    }

    #[test]
    fn feature_availability_reports_the_current_build() {
        let switch = component_support(component_ids::SWITCH).unwrap();
        assert_eq!(switch.required_feature, None);
        assert!(switch.compiled);

        let gpu = component_support(component_ids::GPU_VIEW).unwrap();
        assert_eq!(gpu.required_feature, Some("gpu"));
        assert_eq!(gpu.compiled, cfg!(feature = "gpu"));
    }
}

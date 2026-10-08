//! Builtin identity declarations shared by registration and capability queries.
//! These are metadata only; ComponentRegistry remains the sole instantiation ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentDescriptor {
    pub type_id: &'static str,
    pub tags: &'static [&'static str],
    pub required_feature: Option<&'static str>,
    pub compiled: bool,
}
macro_rules! descriptors {
    ($($name:ident => { type_id: $id:literal, tags: $tags:expr $(, feature: $feature:literal)? }),* $(,)?) => {
        $(pub const $name: ComponentDescriptor = ComponentDescriptor {
            type_id: $id,
            tags: $tags,
            required_feature: descriptors!(@feature $($feature)?),
            compiled: descriptors!(@compiled $($feature)?),
        };)*
        pub const BUILTIN_COMPONENTS: &[ComponentDescriptor] = &[$($name),*];
    };
    (@feature) => { None };
    (@feature $feature:literal) => { Some($feature) };
    (@compiled) => { true };
    (@compiled $feature:literal) => { cfg!(feature = $feature) };
}
descriptors! {
    STACK => { type_id: "nana.stack", tags: &["stack"] },
    TEXT => { type_id: "nana.text", tags: &["text"] },
    BUTTON => { type_id: "nana.button", tags: &["button"] },
    ICON_BUTTON => { type_id: "nana.icon-button", tags: &["icon-button"] },
    ICON_GLYPH => { type_id: "nana.icon", tags: &["icon", "i"] },
    CHECKBOX => { type_id: "nana.checkbox", tags: &["checkbox"] },
    DIVIDER => { type_id: "nana.divider", tags: &["divider"] },
    NUMBER_INPUT => { type_id: "nana.number-input", tags: &["number-input"] },
    SWITCH => { type_id: "nana.switch", tags: &["switch"] },
    CARD => { type_id: "nana.card", tags: &["card"] },
    LIST_ITEM => { type_id: "nana.list-item", tags: &["list-item"] },
    THUMBNAIL => { type_id: "nana.thumbnail", tags: &["thumbnail"] },
    CHIP => { type_id: "nana.chip", tags: &["chip"] },
    DATE_PICKER => { type_id: "nana.date-picker", tags: &["date-picker"] },
    TOOLBAR => { type_id: "nana.toolbar", tags: &["toolbar"] },
    STATUS_BAR => { type_id: "nana.status-bar", tags: &["status-bar"] },
    MEDIA_TRANSPORT_BAR => { type_id: "nana.media-transport-bar", tags: &["media-transport-bar"] },
    AVATAR => { type_id: "nana.avatar", tags: &["avatar"] },
    TEXT_INPUT => { type_id: "nana.text-input", tags: &["text-input"] },
    TEXT_AREA => { type_id: "nana.textarea", tags: &["textarea"] },
    HOSTED_TEXTAREA => { type_id: "nana.hosted-textarea", tags: &["hosted-textarea"] },
    TERMINAL => { type_id: "nana.terminal", tags: &["terminal"] },
    DIFF => { type_id: "nana.diff", tags: &["diff"] },
    RANGE_FIELD => { type_id: "nana.range-field", tags: &["range-field"] },
    PROGRESS => { type_id: "nana.progress", tags: &["progress"] },
    SPINNER => { type_id: "nana.spinner", tags: &["spinner"] },
    STATUS_BADGE => { type_id: "nana.status-badge", tags: &["status-badge"] },
    VALIDATION_MESSAGE => { type_id: "nana.validation-message", tags: &["validation-message"] },
    EMPTY_STATE => { type_id: "nana.empty-state", tags: &["empty-state"] },
    LABELED_VALUE => { type_id: "nana.labeled-value", tags: &["labeled-value"] },
    DIALOG => { type_id: "nana.dialog", tags: &["dialog"] },
    CONFIRM_DIALOG => { type_id: "nana.confirm-dialog", tags: &["confirm-dialog"] },
    SELECT => { type_id: "nana.select", tags: &["select"] },
    TABS => { type_id: "nana.tabs", tags: &["tabs"] },
    SEGMENTED_CONTROL => { type_id: "nana.segmented", tags: &["segmented"] },
    DROPDOWN => { type_id: "nana.dropdown", tags: &["dropdown"] },
    SEARCH_DROPDOWN => { type_id: "nana.search-dropdown", tags: &["search-dropdown"] },
    PANEL => { type_id: "nana.panel", tags: &["panel"] },
    DRAWER => { type_id: "nana.drawer", tags: &["drawer"] },
    POPOVER => { type_id: "nana.popover", tags: &["popover"] },
    CONTEXT_MENU => { type_id: "nana.context-menu", tags: &["context-menu"] },
    TOAST => { type_id: "nana.toast", tags: &["toast"] },
    ACTION_MENU => { type_id: "nana.action-menu", tags: &["action-menu"] },
    ACTION_MENU_ITEM => { type_id: "nana.action-menu-item", tags: &["action-menu-item"] },
    TOOLTIP => { type_id: "nana.tooltip", tags: &["tooltip"] },
    X_Y_PAD => { type_id: "nana.xy-pad", tags: &["xy-pad"] },
    QR_CODE => { type_id: "nana.qr-code", tags: &["qr-code"] },
    FORM_FIELD => { type_id: "nana.form-field", tags: &["form-field"] },
    COLOR_FIELD => { type_id: "nana.color-field", tags: &["color-field"] },
    PATH_FIELD => { type_id: "nana.path-field", tags: &["path-field"] },
    INTERACTIVE_CARD => { type_id: "nana.interactive-card", tags: &["interactive-card"] },
    SKELETON => { type_id: "nana.skeleton", tags: &["skeleton"] },
    LEVEL_METER => { type_id: "nana.level-meter", tags: &["level-meter"] },
    COMMAND_PALETTE => { type_id: "nana.command-palette", tags: &["command-palette"] },
    TREE_VIEW => { type_id: "nana.tree-view", tags: &["tree-view"] },
    CALENDAR_HEATMAP => { type_id: "nana.calendar-heatmap", tags: &["calendar-heatmap"], feature: "calendar" },
    IMAGE_VIEWER => { type_id: "nana.image-viewer", tags: &["image-viewer"], feature: "image-viewer" },
    NATIVE_MARKDOWN => { type_id: "nana.native-markdown", tags: &["native-markdown"], feature: "rich-text" },
    GRAPH_CANVAS => { type_id: "nana.graph-canvas", tags: &["graph-canvas"], feature: "graph-canvas" },
    WORKSPACE => { type_id: "nana.workspace", tags: &["workspace"] },
    DOCK => { type_id: "nana.dock", tags: &["dock"] },
    SPLIT_PANE => { type_id: "nana.split-pane", tags: &["split-pane"] },
    APP_SHELL => { type_id: "nana.app-shell", tags: &["app-shell"] },
    SIDEBAR_FRAME => { type_id: "nana.sidebar-frame", tags: &["sidebar-frame"] },
    SIDEBAR_ROW => { type_id: "nana.sidebar-row", tags: &["sidebar-row"] },
    SETTINGS_ROW => { type_id: "nana.settings-row", tags: &["settings-row"] },
    SETTINGS_CARD => { type_id: "nana.settings-card", tags: &["settings-card"] },
    SETTINGS_PAGE => { type_id: "nana.settings-page", tags: &["settings-page"] },
    SETTINGS_COLLAPSIBLE_CARD => { type_id: "nana.settings-collapsible-card", tags: &["settings-collapsible-card"] },
    LIST => { type_id: "nana.list", tags: &["list"] },
    SCROLL_VIEW => { type_id: "nana.scroll-view", tags: &["scroll-view"] },
    TABLE => { type_id: "nana.table", tags: &["table"] },
    TABLE_ROW => { type_id: "nana.table-row", tags: &["tr"] },
    TABLE_CELL => { type_id: "nana.table-cell", tags: &["td"] },
    REORDER_LIST => { type_id: "nana.reorder-list", tags: &["reorder-list"], feature: "controls" },
    DONUT_CHART => { type_id: "nana.donut-chart", tags: &["donut-chart"], feature: "charts" },
    TIME_SERIES_CHART => { type_id: "nana.time-series-chart", tags: &["time-series-chart"], feature: "charts" },
    DESKTOP_SHELL => { type_id: "nana.desktop-shell", tags: &["desktop-shell"] },
    APP_TITLE_BAR => { type_id: "nana.app-title-bar", tags: &["app-title-bar"] },
    PANE_CHROME => { type_id: "nana.pane-chrome", tags: &["pane-chrome"] },
    SIDEBAR_SECTION => { type_id: "nana.sidebar-section", tags: &["sidebar-section"] },
    SIDEBAR_FOOTER => { type_id: "nana.sidebar-footer", tags: &["sidebar-footer"] },
}
/// Looks up a declared tag even when its implementation is compiled out.
pub fn builtin_component(tag: &str) -> Option<&'static ComponentDescriptor> {
    let normalized = crate::normalize_tag(tag);
    // Declared tags are already canonical; normalize only the input.
    BUILTIN_COMPONENTS
        .iter()
        .find(|entry| entry.type_id == tag || entry.tags.contains(&normalized.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declarations_match_installed_component_registry() {
        let context = crate::AppContext::new();
        for descriptor in BUILTIN_COMPONENTS {
            for tag in descriptor.tags {
                assert_eq!(
                    context
                        .resolve_component_tag(tag)
                        .map(crate::ComponentTypeId::as_str),
                    descriptor.compiled.then_some(descriptor.type_id),
                    "{tag}"
                );
            }
        }
    }
}

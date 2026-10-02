//! Command-palette surface on top of the Runtime action vocabulary.
//!
//! The action record, its registry, key strokes, bindings and the keymap all
//! live in `nana-ui-runtime` and are re-exported here: there is one
//! [`ActionDescriptor`] and one [`ActionRegistry`], shared by the keymap and by
//! the palette. Only the picker's own view state is defined in this module.

pub use nana_ui_core::{
    ActionId, ActionPickerNavigation, CommandPaletteEvent, CommandPaletteItem, ContextPredicate,
    KeyContext,
};
/// The host-facing name for a single recorded stroke.
pub use nana_ui_runtime::CapturedStroke as KeyStroke;
pub use nana_ui_runtime::{
    ActionDescriptor, ActionMatch, ActionRegistry, ActionRegistryError, KeyBinding, KeyModifiers,
    Keymap, KeymapMatch, KeymapState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPickerSelection {
    pub action: ActionId,
    pub restore_focus: Option<String>,
}

pub fn action_picker_from_key_name(key: &str) -> Option<ActionPickerNavigation> {
    match key {
        "ArrowUp" | "Up" => Some(ActionPickerNavigation::Previous),
        "ArrowDown" | "Down" => Some(ActionPickerNavigation::Next),
        "Home" => Some(ActionPickerNavigation::First),
        "End" => Some(ActionPickerNavigation::Last),
        "Enter" => Some(ActionPickerNavigation::Confirm),
        "Escape" => Some(ActionPickerNavigation::Dismiss),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionPickerState {
    open: bool,
    query: String,
    selected: usize,
    restore_focus: Option<String>,
}

impl ActionPickerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn open(&mut self, restore_focus: Option<String>) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.restore_focus = restore_focus;
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.selected = 0;
    }

    pub fn move_selection(&mut self, offset: isize, result_count: usize) {
        if result_count == 0 {
            self.selected = 0;
            return;
        }
        self.selected =
            (self.selected as isize + offset).rem_euclid(result_count as isize) as usize;
    }

    pub fn navigate(&mut self, navigation: ActionPickerNavigation, result_count: usize) {
        match navigation {
            ActionPickerNavigation::Previous => self.move_selection(-1, result_count),
            ActionPickerNavigation::Next => self.move_selection(1, result_count),
            ActionPickerNavigation::First => self.selected = 0,
            ActionPickerNavigation::Last if result_count > 0 => self.selected = result_count - 1,
            ActionPickerNavigation::Last
            | ActionPickerNavigation::Confirm
            | ActionPickerNavigation::Dismiss => {}
        }
    }

    pub fn sync_results(&mut self, result_count: usize) {
        if result_count == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(result_count - 1);
        }
    }

    pub fn dismiss(&mut self) -> Option<String> {
        self.open = false;
        self.query.clear();
        self.selected = 0;
        self.restore_focus.take()
    }

    pub fn confirm(&mut self, action: ActionId) -> ActionPickerSelection {
        let restore_focus = self.dismiss();
        ActionPickerSelection {
            action,
            restore_focus,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> ActionRegistry {
        let mut registry = ActionRegistry::new();
        registry
            .register(
                ActionDescriptor::labeled("workspace.open", "打开工作区")
                    .category("工作区")
                    .keywords(["folder", "project"]),
            )
            .unwrap();
        registry
            .register(
                ActionDescriptor::labeled("editor.save", "保存文件")
                    .category("编辑器")
                    .when(ContextPredicate::always().all_of(["editor"])),
            )
            .unwrap();
        registry
            .register(ActionDescriptor::labeled("editor.close", "关闭文件").enabled(false))
            .unwrap();
        registry
    }

    #[test]
    fn registry_exposes_only_enabled_actions_in_the_active_context() {
        let registry = registry();
        let global = registry.available(&KeyContext::default());
        assert_eq!(global.len(), 1);
        assert_eq!(global[0].id.as_str(), "workspace.open");

        let editor = registry.available(&KeyContext::new(["editor"]));
        assert_eq!(editor.len(), 2);
        assert_eq!(editor[1].id.as_str(), "editor.save");
    }

    #[test]
    fn registry_search_ranks_labels_before_keyword_and_subsequence_matches() {
        let registry = registry();
        let context = KeyContext::new(["editor"]);
        let label = registry.search("保存", &context);
        assert_eq!(label[0].action.id.as_str(), "editor.save");
        let keyword = registry.search("folder", &context);
        assert_eq!(keyword[0].action.id.as_str(), "workspace.open");
        let subsequence = registry.search("dkq", &context);
        assert!(subsequence.is_empty());
    }

    #[test]
    fn keymap_resolves_chords_against_context_and_retries_after_a_broken_prefix() {
        let registry = registry();
        let context = KeyContext::new(["editor"]);
        let primary = KeyModifiers::primary();
        let keymap = Keymap::new([
            KeyBinding::sequence(
                "workspace.open",
                [KeyStroke::new("k", primary), KeyStroke::new("o", primary)],
            ),
            KeyBinding::new("editor.save", KeyStroke::new("s", primary))
                .when(ContextPredicate::always().all_of(["editor"])),
        ]);
        let mut state = KeymapState::default();
        assert_eq!(
            keymap.resolve(
                &mut state,
                KeyStroke::new("k", primary),
                &context,
                &registry,
            ),
            KeymapMatch::Pending
        );
        assert_eq!(
            keymap.resolve(
                &mut state,
                KeyStroke::new("s", primary),
                &context,
                &registry,
            ),
            KeymapMatch::Dispatch(ActionId::from("editor.save"))
        );
    }

    #[test]
    fn picker_wraps_selection_and_returns_the_prior_focus_on_confirm() {
        let mut picker = ActionPickerState::new();
        picker.open(Some("editor.body".to_owned()));
        picker.move_selection(-1, 3);
        assert_eq!(picker.selected(), 2);
        picker.set_query("save");
        assert_eq!(picker.selected(), 0);
        let selected = picker.confirm(ActionId::from("editor.save"));
        assert_eq!(selected.action.as_str(), "editor.save");
        assert_eq!(selected.restore_focus.as_deref(), Some("editor.body"));
        assert!(!picker.is_open());
    }
}

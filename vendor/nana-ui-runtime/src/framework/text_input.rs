//! AppContext text input operations.

use super::*;

impl AppContext {
    /// Native composition owns its pending text and committed insertion range.
    /// Ordinary key edits must wait for the IME commit/cancel event.
    pub(super) fn has_focused_ime_composition(&self, document: DocumentId) -> bool {
        self.world
            .focused_text_input(document)
            .is_some_and(|(target, _)| self.node_composing(target))
    }

    /// Whether an editor has a composition in progress. An empty preedit
    /// between keystrokes is not one.
    pub(super) fn node_composing(&self, node: StableNodeId) -> bool {
        self.world
            .ime(node)
            .is_some_and(|composition| !composition.text.is_empty())
    }

    /// Submit only a focused native single-line input. IME confirmation never
    /// submits a form; non-input editors retain their existing Enter behavior.
    pub fn submit_focused_text_input(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(entity) = self.focused_editor::<TextInput>(document) else {
            return Ok(false);
        };
        if self.world.ime(entity.id).is_some() || !self.read(entity, EditableText::accepts_input)? {
            return Ok(false);
        }
        self.update(entity, |input, cx| {
            cx.emit(crate::TextSubmitted {
                value: input.state.value.to_string(),
            });
        })?;
        Ok(true)
    }

    pub fn set_ime_preedit(
        &mut self,
        document: DocumentId,
        text: String,
        selection: Option<(usize, usize)>,
    ) -> Result<bool, FrameworkError> {
        let Some((target, _)) = self.world.focused_text_input(document) else {
            return Ok(false);
        };
        if !self
            .world
            .accessibility(target)
            .is_some_and(|state| state.editable)
        {
            return Ok(false);
        }
        let composition = crate::ImeComposition { text, selection };
        if self.world.ime(target).is_some_and(|ime| ime == composition) {
            return Ok(false);
        }
        let mut mutations = MutationQueue::new();
        mutations.set_ime(target, Some(composition));
        self.world.commit(mutations)?;
        Ok(true)
    }

    pub fn clear_ime(&mut self, document: DocumentId) -> Result<bool, FrameworkError> {
        let Some((target, _)) = self.world.focused_text_input(document) else {
            return Ok(false);
        };
        if self.world.ime(target).is_none() {
            return Ok(false);
        }
        let mut mutations = MutationQueue::new();
        mutations.set_ime(target, None);
        self.world.commit(mutations)?;
        Ok(true)
    }

    pub fn commit_ime(&mut self, document: DocumentId, text: &str) -> Result<bool, FrameworkError> {
        // Every component editor commits through its component: the world
        // path alone would leave the component's value behind, and its next
        // projection would write the commit away.
        if let Some((node, hooks)) = self.focused_editable(document) {
            return (hooks.commit_ime)(self, node, text);
        }
        self.replace_world_text_input_selection(document, text)
    }

    /// Delete UTF-8 bytes surrounding the focused editor's selection.
    ///
    /// Leaves IME preedit in place. Returns `Ok(false)` when no focused
    /// editable field can apply the requested span.
    pub fn delete_ime_surrounding(
        &mut self,
        document: DocumentId,
        before_bytes: usize,
        after_bytes: usize,
    ) -> Result<bool, FrameworkError> {
        if let Some((node, hooks)) = self.focused_editable(document) {
            return (hooks.delete_surrounding)(self, node, before_bytes, after_bytes);
        }
        self.delete_world_text_input_surrounding(document, before_bytes, after_bytes)
    }

    /// Replace the selection of a focused text input with no editor
    /// component: a field a host projects as text-input state alone, as
    /// Vue's are. Typed text and IME commits reach it the same way.
    /// Set a text input's value and selections. An editor component holds
    /// the state its next projection writes, so its state is set through it;
    /// a field with no component (a host's projection) is written directly.
    pub fn set_text_input_state(
        &mut self,
        node: StableNodeId,
        state: TextInputState,
    ) -> Result<(), FrameworkError> {
        if let Some(plain) = self.plain_editor(node) {
            return (plain.set_state)(self, node, state);
        }
        let mut mutations = MutationQueue::new();
        mutations.set_text_input(node, Some(state));
        self.world.commit(mutations)?;
        Ok(())
    }

    pub(super) fn replace_world_text_input_selection(
        &mut self,
        document: DocumentId,
        text: &str,
    ) -> Result<bool, FrameworkError> {
        let Some((target, state)) = self.world.focused_text_input(document) else {
            return Ok(false);
        };
        let mut next = state.to_state();
        if !self
            .world
            .accessibility(target)
            .is_some_and(|state| state.editable)
        {
            return Ok(false);
        }
        if !next.replace_selection(text) {
            return Ok(false);
        }
        let mut mutations = MutationQueue::new();
        mutations.set_ime(target, None);
        mutations.set_text_input(target, Some(next));
        self.world.commit(mutations)?;
        Ok(true)
    }

    pub(super) fn delete_world_text_input_surrounding(
        &mut self,
        document: DocumentId,
        before_bytes: usize,
        after_bytes: usize,
    ) -> Result<bool, FrameworkError> {
        let Some((target, state)) = self.world.focused_text_input(document) else {
            return Ok(false);
        };
        if !self
            .world
            .accessibility(target)
            .is_some_and(|state| state.editable)
        {
            return Ok(false);
        }
        let composing = self.node_composing(target);
        let mut next = state.to_state();
        if !next.delete_ime_surrounding(before_bytes, after_bytes, composing) {
            return Ok(false);
        }
        let mut mutations = MutationQueue::new();
        mutations.set_text_input(target, Some(next));
        self.world.commit(mutations)?;
        Ok(true)
    }

    pub(super) fn delete_editable_surrounding<C: EditableText>(
        &mut self,
        entity: Entity<C>,
        before_bytes: usize,
        after_bytes: usize,
    ) -> Result<bool, FrameworkError> {
        if !self.read(entity, EditableText::accepts_input)? {
            return Ok(false);
        }
        let composing = self.node_composing(entity.stable_id());
        let snippet = self.world.text_snippet_session(entity.stable_id());
        // Only a snippet session diffs the pre-edit value (to follow its
        // linked placeholders). Cloning the whole value for every keystroke of
        // every editor would be O(document) for nothing.
        let old = snippet
            .as_ref()
            .map(|_| self.read(entity, |editable| editable.state().value.clone()))
            .transpose()?;
        let mut linked = None;
        let changed = self.commit_editor_edit(entity, TextEditOrigin::Ime, |editable, _| {
            if !editable.delete_surrounding(before_bytes, after_bytes, composing) {
                return false;
            }
            if let Some(session) = &snippet
                && let Some(old) = old.as_deref()
                && let Some((value, selection, session)) =
                    session.linked_edit(old, &editable.state().value, editable.state().selection)
            {
                editable.state_mut().value = value.into();
                editable.state_mut().selection = selection;
                linked = Some(session);
            }
            true
        })?;
        if changed && let Some(session) = linked {
            let mut mutations = MutationQueue::new();
            mutations.set_text_input_snippet(entity.stable_id(), Some(session));
            self.world.commit(mutations)?;
        }
        Ok(changed)
    }

    pub fn replace_focused_text(
        &mut self,
        document: DocumentId,
        text: &str,
    ) -> Result<bool, FrameworkError> {
        self.insert_focused_text(document, text, TextEditOrigin::Typing)
    }

    /// Insert clipboard text at the focused editor's selection.
    ///
    /// Same write as [`Self::replace_focused_text`], but one paste is one undo
    /// step however much it inserted, where a run of typed characters collapses
    /// into a single step.
    pub fn paste_focused_text(
        &mut self,
        document: DocumentId,
        text: &str,
    ) -> Result<bool, FrameworkError> {
        self.insert_focused_text(document, text, TextEditOrigin::Paste)
    }

    fn insert_focused_text(
        &mut self,
        document: DocumentId,
        text: &str,
        origin: TextEditOrigin,
    ) -> Result<bool, FrameworkError> {
        if self.has_focused_ime_composition(document) {
            return Ok(false);
        }
        // Hosts report the control characters command keys carry ("\r" for
        // Enter, "\u{1b}" for Escape, "\u{8}" for Backspace). Typed, they are
        // commands, never text; Tab and newline are text an editor holds.
        // Pasted and cut text is the pasteboard's, and is kept as it is.
        if origin == TextEditOrigin::Typing
            && text
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\t' | '\n'))
        {
            return Ok(false);
        }
        if let Some((node, hooks)) = self.focused_editable(document) {
            return (hooks.replace_selection)(self, node, text, origin);
        }
        self.replace_world_text_input_selection(document, text)
    }

    pub fn delete_focused_text_backward(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        if self.has_focused_ime_composition(document) {
            return Ok(false);
        }
        if let Some((node, hooks)) = self.focused_editable(document) {
            return (hooks.delete_backward)(self, node);
        }
        Ok(false)
    }

    /// Text currently selected in the focused editor, or in the focused
    /// rich-text block.
    ///
    /// An empty selection reports `None` so a host copy request never replaces
    /// the pasteboard with an empty string. The Runtime does not touch the OS
    /// pasteboard; the host writes what this returns.
    pub fn focused_selected_text(&self, document: DocumentId) -> Option<String> {
        if let Some((node, hooks)) = self.focused_editable(document) {
            return (hooks.selected_text)(self, node);
        }
        #[cfg(feature = "rich-text")]
        let focused = self.world.focused(document)?;
        #[cfg(feature = "rich-text")]
        if let Some(entity) = self.view_entity::<crate::SelectableRichText>(focused) {
            return self
                .read(entity, |text| text.copy_snapshot())
                .ok()
                .flatten()
                .map(|snapshot| snapshot.text);
        }
        #[cfg(feature = "rich-text")]
        if let Some(entity) = self.view_entity::<crate::NativeMarkdown>(focused) {
            return self
                .read(entity, |markdown| markdown.copy_snapshot())
                .ok()
                .flatten()
                .map(|snapshot| snapshot.text);
        }
        None
    }

    /// Remove the focused editor's selection and report what it held.
    ///
    /// Reports `None` without editing when the selection is empty or the field
    /// rejects input, so a cut on a read-only field leaves both the value and
    /// the pasteboard alone.
    pub fn cut_focused_text(
        &mut self,
        document: DocumentId,
    ) -> Result<Option<String>, FrameworkError> {
        let Some(text) = self.focused_selected_text(document) else {
            return Ok(None);
        };
        // Its own undo step: typing right after a cut must not merge into it.
        if !self.insert_focused_text(document, "", TextEditOrigin::Cut)? {
            return Ok(None);
        }
        Ok(Some(text))
    }

    /// Select the whole value of the focused editor.
    ///
    /// Read-only and disabled fields still select, so their text can be copied.
    pub fn select_all_focused_text(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        if let Some((node, hooks)) = self.focused_editable(document) {
            return (hooks.select_all)(self, node);
        }
        Ok(false)
    }

    pub(super) fn editable_selected_text<C: EditableText>(
        &self,
        entity: Entity<C>,
    ) -> Option<String> {
        self.read(entity, |editable| {
            // What an edit of the selection would remove: a primary cutting
            // into an atom takes the atom whole, so a cut puts on the
            // pasteboard exactly what it deletes. A bare caret selects
            // nothing to copy, wherever it sits.
            let expanded = (!editable.state().selection.is_collapsed())
                .then(|| atom_expanded_primary(editable))
                .flatten()
                .map(|primary| {
                    let mut state = editable.state().clone();
                    state.selection = primary;
                    state
                });
            let state = expanded.as_ref().unwrap_or_else(|| editable.state());
            // Zed copy semantics: every selection's text, in document order,
            // joined with newlines. An empty set (bare carets only) reports
            // None so a copy never blanks the pasteboard.
            let mut parts: Vec<&str> = Vec::new();
            for selection in state.selections().iter() {
                if !selection.is_valid_for(&state.value) {
                    continue;
                }
                let range = selection.ordered();
                if !range.is_empty() {
                    parts.push(&state.value[range]);
                }
            }
            (!parts.is_empty()).then(|| parts.join("\n"))
        })
        .ok()
        .flatten()
    }

    pub(super) fn select_all_editable<C: EditableText>(
        &mut self,
        entity: Entity<C>,
    ) -> Result<bool, FrameworkError> {
        if !self.read(entity, EditableText::accepts_selection)? {
            return Ok(false);
        }
        let changed = self.update_component(entity, |editable, cx| {
            let selection = TextSelection::new(0, editable.state().value.len());
            if editable.state().selection == selection
                && !editable.state().has_additional_selections()
            {
                return false;
            }
            let state = editable.state_mut();
            state.selection = selection;
            // Select-all is a wholesale replacement of the selection set.
            state.additional_selections.clear();
            cx.emit(TextChanged {
                value: editable.state().value.clone(),
                selection,
            });
            true
        })?;
        if changed {
            self.seal_editor_history(entity.stable_id());
        }
        Ok(changed)
    }

    pub(super) fn delete_editable_backward<C: EditableText>(
        &mut self,
        entity: Entity<C>,
    ) -> Result<bool, FrameworkError> {
        use unicode_segmentation::UnicodeSegmentation;

        if !self.read(entity, EditableText::accepts_input)? {
            return Ok(false);
        }
        let snippet = self.world.text_snippet_session(entity.stable_id());
        let old = self.read(entity, |editable| editable.state().value.clone())?;
        let mut linked = None;
        let changed = self.commit_editor_edit(entity, TextEditOrigin::Delete, |editable, _| {
            {
                let state = editable.state_mut();
                if state.selection.anchor == state.selection.focus {
                    let caret = state.selection.focus;
                    let Some(previous) = state.value[..caret]
                        .grapheme_indices(true)
                        .next_back()
                        .map(|(index, _)| index)
                    else {
                        return false;
                    };
                    state.selection = TextSelection::new(previous, caret);
                }
            }
            if !editable.replace_selection("") {
                return false;
            }
            if let Some(session) = &snippet
                && let Some((value, selection, session)) =
                    session.linked_edit(&old, &editable.state().value, editable.state().selection)
            {
                editable.state_mut().value = value.into();
                editable.state_mut().selection = selection;
                linked = Some(session);
            }
            true
        })?;
        if changed && let Some(session) = linked {
            let mut mutations = MutationQueue::new();
            mutations.set_text_input_snippet(entity.stable_id(), Some(session));
            self.world.commit(mutations)?;
        }
        Ok(changed)
    }

    pub(super) fn commit_editable_ime<C: EditableText>(
        &mut self,
        entity: Entity<C>,
        text: &str,
    ) -> Result<bool, FrameworkError> {
        if !self.read(entity, EditableText::accepts_input)? {
            return Ok(false);
        }
        let snippet = self.world.text_snippet_session(entity.stable_id());
        let old = self.read(entity, |editable| editable.state().value.clone())?;
        let mut linked = None;
        let changed = self.commit_editor_edit(entity, TextEditOrigin::Ime, |editable, cx| {
            cx.mutations().set_ime(entity.stable_id(), None);
            if !editable.commit_ime_text(text) {
                return false;
            }
            if let Some(session) = &snippet
                && let Some((value, selection, session)) =
                    session.linked_edit(&old, &editable.state().value, editable.state().selection)
            {
                editable.state_mut().value = value.into();
                editable.state_mut().selection = selection;
                linked = Some(session);
            }
            true
        })?;
        if changed && let Some(session) = linked {
            let mut mutations = MutationQueue::new();
            mutations.set_text_input_snippet(entity.stable_id(), Some(session));
            self.world.commit(mutations)?;
        }
        Ok(changed)
    }

    pub(super) fn set_editable_value<C: EditableText>(
        &mut self,
        entity: Entity<C>,
        value: String,
    ) -> Result<bool, FrameworkError> {
        if !self.read(entity, EditableText::accepts_input)? {
            return Ok(false);
        }
        self.commit_editor_edit(entity, TextEditOrigin::Program, |editable, _| {
            editable.set_value(value)
        })
    }

    pub(super) fn set_editable_selection<C: EditableText>(
        &mut self,
        entity: Entity<C>,
        selection: TextSelection,
    ) -> Result<bool, FrameworkError> {
        if !self.read(entity, EditableText::accepts_selection)? {
            return Ok(false);
        }
        let changed = self.update_component(entity, |editable, cx| {
            if editable.state().selection == selection
                || !selection.is_valid_for(&editable.state().value)
            {
                return false;
            }
            editable.state_mut().selection = selection;
            editable.state_mut().normalize_selections();
            // What the editor now holds: fusing with another cursor can
            // have widened or turned it.
            cx.emit(TextChanged {
                value: editable.state().value.clone(),
                selection: editable.state().selection,
            });
            true
        })?;
        if changed {
            self.seal_editor_history(entity.stable_id());
        }
        Ok(changed)
    }

    /// Replace the active UTF-8 selection and notify typed observers without
    /// requiring an application-wide message enum.
    pub fn replace_text_input_selection(
        &mut self,
        entity: Entity<TextInput>,
        text: &str,
    ) -> Result<bool, FrameworkError> {
        self.replace_editable_selection(entity, text, TextEditOrigin::Typing)
    }

    pub fn replace_text_area_selection(
        &mut self,
        entity: Entity<TextArea>,
        text: &str,
    ) -> Result<bool, FrameworkError> {
        self.replace_editable_selection(entity, text, TextEditOrigin::Typing)
    }

    pub(super) fn replace_editable_selection<C: EditableText>(
        &mut self,
        entity: Entity<C>,
        text: &str,
        origin: TextEditOrigin,
    ) -> Result<bool, FrameworkError> {
        if !self.read(entity, EditableText::accepts_input)? {
            return Ok(false);
        }
        let snippet = self.world.text_snippet_session(entity.stable_id());
        let old = self.read(entity, |editable| editable.state().value.clone())?;
        let mut linked = None;
        let changed = self.commit_editor_edit(entity, origin, |editable, _| {
            if let Some(expanded) = atom_expanded_primary(editable) {
                editable.state_mut().selection = expanded;
            }
            if !editable.replace_selection(text) {
                return false;
            }
            if let Some(session) = &snippet
                && let Some((value, selection, session)) =
                    session.linked_edit(&old, &editable.state().value, editable.state().selection)
            {
                editable.state_mut().value = value.into();
                editable.state_mut().selection = selection;
                linked = Some(session);
            }
            true
        })?;
        if changed && let Some(session) = linked {
            let mut mutations = MutationQueue::new();
            mutations.set_text_input_snippet(entity.stable_id(), Some(session));
            self.world.commit(mutations)?;
        }
        Ok(changed)
    }
}

/// The primary selection widened over every atom it cuts into, when it cuts
/// into one: an edit replaces atoms whole ([`crate::TextAtomSpan`]).
fn atom_expanded_primary<C: EditableText>(editable: &C) -> Option<crate::TextSelection> {
    let atoms = crate::text_editing::atoms_in(&editable.state().value, editable.text_atoms());
    if atoms.is_empty() {
        return None;
    }
    let range = editable.state().selection.ordered();
    let expanded = crate::text_editing::expand_range_over_atoms(range.clone(), &atoms);
    (expanded != range).then(|| crate::TextSelection::new(expanded.start, expanded.end))
}

#[cfg(test)]
mod composition_tests {
    use super::*;
    #[test]
    fn preedit_owns_committed_range_until_commit_for_single_and_multiline_editors() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let field = context
            .create_component(document, TextInput::new("ab"))
            .unwrap();
        let area = context
            .create_component(document, TextArea::new("ab"))
            .unwrap();
        for node in [field.stable_id(), area.stable_id()] {
            assert!(context.focus_node(document, node).unwrap());
            context
                .set_ime_preedit(document, "ni".into(), None)
                .unwrap();
            let before = context.world().text_input(node).unwrap().to_state();
            let preedit = context.world().ime(node).unwrap().to_composition();
            assert!(
                !context
                    .move_focused_text_caret(document, crate::TextCaretIntent::Left, false, None)
                    .unwrap()
            );
            assert!(
                !context
                    .delete_focused_text(document, crate::TextDeleteKind::Backward)
                    .unwrap()
            );
            assert!(
                !context
                    .delete_focused_text(document, crate::TextDeleteKind::Forward)
                    .unwrap()
            );
            assert!(!context.delete_focused_text_backward(document).unwrap());
            assert!(!context.replace_focused_text(document, "raw-key").unwrap());
            assert!(context.world().text_input(node).unwrap() == before);
            assert!(context.world().ime(node).unwrap() == preedit);
            assert!(context.commit_ime(document, "你").unwrap());
            assert_eq!(context.world().text_input(node).unwrap().value, "ab你");
            assert!(context.world().ime(node).is_none());
            assert!(context.delete_focused_text_backward(document).unwrap());
            assert_eq!(context.world().text_input(node).unwrap().value, "ab");
        }
    }
    #[test]
    fn ime_surrounding_delete_keeps_the_text_a_preedit_stands_in_for() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let field = context
            .create_component(document, TextInput::new("hello foo bar"))
            .unwrap();
        let node = field.stable_id();
        context.focus_node(document, node).unwrap();
        assert!(context.select_focused_text_range(document, 6, 9).unwrap());
        context.set_ime_preedit(document, "x".into(), None).unwrap();
        assert!(context.delete_ime_surrounding(document, 1, 1).unwrap());
        let state = context.world().text_input(node).unwrap();
        assert_eq!(state.value, "hellofoobar");
        assert_eq!(
            state.selection.ordered(),
            5..8,
            "the replaced text survives"
        );
        assert!(context.clear_ime(document).unwrap());
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "hellofoobar"
        );
    }

    #[test]
    fn ime_surrounding_delete_and_empty_preedit_keep_their_explicit_edit_contracts() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let field = context
            .create_component(document, TextInput::new("ab"))
            .unwrap();
        context.focus_node(document, field.stable_id()).unwrap();
        context
            .set_ime_preedit(document, "ni".into(), None)
            .unwrap();
        assert!(context.delete_ime_surrounding(document, 1, 0).unwrap());
        assert_eq!(
            context.world().text_input(field.stable_id()).unwrap().value,
            "a"
        );
        assert_eq!(context.world().ime(field.stable_id()).unwrap().text, "ni");
        context
            .set_ime_preedit(document, String::new(), None)
            .unwrap();
        assert!(context.replace_focused_text(document, "c").unwrap());
        assert_eq!(
            context.world().text_input(field.stable_id()).unwrap().value,
            "ac"
        );
    }
}

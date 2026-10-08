//! The one place a text editor's value changes, and the undo journal built on
//! it.
//!
//! Every path that writes editor text — typing, deletion, paste, an IME
//! commit, a line transform, a snippet, the application setting a value —
//! goes through [`AppContext::commit_editor_edit`]. Concerns that apply to
//! *all* edits live there once instead of in each caller: emitting the change
//! event, refusing a read-only editor, and recording undo.
//!
//! Composition is deliberately invisible here. IME preedit lives in the
//! world's `ime` slot, not in the editor's value, so the journal never sees
//! the half-typed states of a composition — only the commit, as one step.

use std::collections::HashMap;

use crate::{StableNodeId, TextInputState};
use nana_text::TextStamp;

/// Why an edit happened. Decides whether it extends the previous undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEditOrigin {
    /// Inserted text as the user typed. A run of typing collapses into one
    /// undo step, so undo does not walk back a character at a time.
    Typing,
    /// Removed text. Consecutive deletions collapse the same way.
    Delete,
    /// One paste is one step, however much it inserted.
    Paste,
    /// One committed composition is one step.
    Ime,
    /// Removing the selection to the pasteboard. Its own step, which typing
    /// after it does not extend.
    Cut,
    /// A transform over lines or selections: move, sort, case, comment,
    /// snippet. Always its own step.
    Structural,
    /// A numeric field stepped (arrow key or spinner). A run of steps, such
    /// as a held key, collapses into one undo step.
    Step,
    /// The application wrote the value. Not the user's edit, so it clears the
    /// journal rather than becoming a step the user can undo into.
    Program,
    /// Undo or redo itself. Never recorded.
    History,
}

impl TextEditOrigin {
    /// Whether a new edit of this origin continues the previous step instead
    /// of starting a new one.
    fn merges_with(self, previous: Self) -> bool {
        matches!(
            (previous, self),
            (Self::Typing, Self::Typing) | (Self::Delete, Self::Delete) | (Self::Step, Self::Step)
        )
    }
}

/// What an editor's undo restores: its text state and, for an editor that
/// keeps one (a `NumberInput`), the committed number behind that text. The
/// number is journaled, not re-derived from the text: a draft need not
/// parse, and need not be the number the field held.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct EditorSnapshot {
    pub(super) state: TextInputState,
    pub(super) number: Option<f64>,
}

/// One undoable step: the state before and after.
///
/// Stores whole states rather than diffs. An editor's undo depth is bounded
/// and its documents are the size a person edits by hand; a diff
/// representation would buy little and has to be re-derived on every merge.
#[derive(Debug, Clone, PartialEq)]
struct TextEditStep {
    before: EditorSnapshot,
    after: EditorSnapshot,
    origin: TextEditOrigin,
}

impl TextEditStep {
    /// Keeps a later edit of the same origin from merging into this step.
    fn seal(&mut self) {
        if matches!(
            self.origin,
            TextEditOrigin::Typing | TextEditOrigin::Delete | TextEditOrigin::Step
        ) {
            self.origin = TextEditOrigin::Structural;
        }
    }
}

/// Undo journal for one editor.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct TextHistory {
    steps: Vec<TextEditStep>,
    /// Steps before this index are undoable; steps from it on are redoable.
    cursor: usize,
    held: Option<TextStamp>,
}

impl TextHistory {
    /// Undo depth per editor. Deep enough that a person does not hit it in a
    /// session, bounded so a long-lived editor cannot grow without limit.
    const CAPACITY: usize = 200;

    /// Text bytes the journal of one editor may hold, across every step.
    ///
    /// [`Self::CAPACITY`] alone bounds the journal only for documents of a
    /// hand-written size: a step keeps the value before AND after it, so 200
    /// steps of a 300 KB document would be 60–120 MB per editor. Deep undo is
    /// worth memory, but not that much of it -- past this the oldest steps go,
    /// which is what a step-count overflow does too.
    const CAPACITY_BYTES: usize = 8 * 1024 * 1024;

    /// A program write never reaches here: it clears the journal instead
    /// (see `AppContext::journal_editor_edit`).
    fn record(&mut self, before: EditorSnapshot, after: EditorSnapshot, origin: TextEditOrigin) {
        if origin == TextEditOrigin::History {
            return;
        }
        // Anything after the cursor was undone; a fresh edit replaces it.
        self.steps.truncate(self.cursor);
        if let Some(last) = self.steps.last_mut()
            && origin.merges_with(last.origin)
        {
            last.after = after;
            // A run whose text ended where it began (ArrowUp then ArrowDown)
            // is no edit the user can see: an undo onto it would change no
            // text and still be spent. Text is what makes a step, so the run
            // goes and what it left behind rides on the step before it.
            if last.before.state.value == last.after.state.value {
                let run = self.steps.pop().expect("the merged step is last");
                self.cursor = self.steps.len();
                self.amend(run.before, run.after);
            }
            return;
        }
        self.push(TextEditStep {
            before,
            after,
            origin,
        });
    }

    fn push(&mut self, step: TextEditStep) {
        self.steps.push(step);
        while self.steps.len() > Self::CAPACITY
            || (self.steps.len() > 1 && self.bytes() > Self::CAPACITY_BYTES)
        {
            self.steps.remove(0);
        }
        self.cursor = self.steps.len();
    }

    /// An edit that left the text alone but moved what rides with it (a
    /// `NumberInput`'s committed number) is no step of its own. The step the
    /// editor stands on takes it as its `after`, so redo onto that step and
    /// undo past it agree with what the field held. Like any edit, it ends
    /// the redo branch and the merge run. With no step to stand on (a fresh
    /// or cleared journal) it becomes one, so the move can still be undone.
    fn amend(&mut self, before: EditorSnapshot, after: EditorSnapshot) {
        self.steps.truncate(self.cursor);
        match self.steps.last_mut() {
            Some(last) => {
                last.after = after;
                last.seal();
            }
            // Only a real move earns a step: a run back to the same text and
            // number leaves nothing to undo.
            None if before.number != after.number => self.push(TextEditStep {
                before,
                after,
                origin: TextEditOrigin::Structural,
            }),
            None => {}
        }
    }

    /// Text bytes the steps hold. Both ends of every step: undo restores the
    /// `before`, redo the `after`.
    ///
    /// Counted per buffer, not per snapshot: a step's `after` and the next
    /// step's `before` are usually one shared buffer (Issue #182), and it
    /// is memory once.
    fn bytes(&self) -> usize {
        let mut seen = std::collections::HashSet::with_capacity(self.steps.len() * 2);
        self.steps
            .iter()
            .flat_map(|step| [&step.before.state.value, &step.after.state.value])
            .filter(|value| seen.insert((value.as_ptr() as usize, value.len())))
            .map(|value| value.len())
            .sum()
    }

    /// Ends the current merge run, so the next edit starts a new step even if
    /// it is the same origin. Moving the caret or changing focus does this:
    /// typing, arrowing away, then typing again is two steps.
    fn seal(&mut self) {
        if let Some(last) = self.steps.last_mut() {
            last.seal();
        }
    }

    /// Undo and redo end a merge run too: typing after an undo is a new
    /// step, not more of the one the undo stepped back onto.
    fn undo(&mut self) -> Option<EditorSnapshot> {
        let index = self.cursor.checked_sub(1)?;
        self.cursor = index;
        self.seal_before_cursor();
        Some(self.steps[index].before.clone())
    }

    fn redo(&mut self) -> Option<EditorSnapshot> {
        let step = self.steps.get(self.cursor)?;
        let after = step.after.clone();
        self.cursor += 1;
        self.seal_before_cursor();
        Some(after)
    }

    /// Seals the step the next edit would merge into.
    fn seal_before_cursor(&mut self) {
        if let Some(step) = self
            .cursor
            .checked_sub(1)
            .and_then(|index| self.steps.get_mut(index))
        {
            step.seal();
        }
    }

    fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    fn can_redo(&self) -> bool {
        self.cursor < self.steps.len()
    }
}

/// Per-editor journals.
#[derive(Debug, Default)]
pub(super) struct TextHistories {
    entries: HashMap<StableNodeId, TextHistory>,
    writing: Vec<StableNodeId>,
}

impl TextHistories {
    pub(super) fn record(
        &mut self,
        node: StableNodeId,
        before: EditorSnapshot,
        after: EditorSnapshot,
        origin: TextEditOrigin,
    ) {
        self.entries
            .entry(node)
            .or_default()
            .record(before, after, origin);
    }

    pub(super) fn seal(&mut self, node: StableNodeId) {
        if let Some(history) = self.entries.get_mut(&node) {
            history.seal();
        }
    }

    /// See [`TextHistory::amend`].
    pub(super) fn amend(
        &mut self,
        node: StableNodeId,
        before: EditorSnapshot,
        after: EditorSnapshot,
    ) {
        self.entries.entry(node).or_default().amend(before, after);
    }

    pub(super) fn undo(&mut self, node: StableNodeId) -> Option<EditorSnapshot> {
        self.entries.get_mut(&node)?.undo()
    }

    pub(super) fn redo(&mut self, node: StableNodeId) -> Option<EditorSnapshot> {
        self.entries.get_mut(&node)?.redo()
    }

    pub(super) fn can_undo(&self, node: StableNodeId) -> bool {
        self.entries.get(&node).is_some_and(TextHistory::can_undo)
    }

    pub(super) fn can_redo(&self, node: StableNodeId) -> bool {
        self.entries.get(&node).is_some_and(TextHistory::can_redo)
    }

    fn current(&self, node: StableNodeId, held: Option<TextStamp>) -> Option<&TextHistory> {
        self.entries
            .get(&node)
            .filter(|history| history.held == held)
    }

    pub(super) fn follow(&mut self, node: StableNodeId, held: Option<TextStamp>) {
        if self.current(node, held).is_none() {
            self.entries.remove(&node);
        }
    }

    pub(super) fn witness(&mut self, node: StableNodeId, held: Option<TextStamp>) {
        if let Some(history) = self.entries.get_mut(&node) {
            history.held = held;
        }
    }

    pub(super) fn begin_writing(&mut self, node: StableNodeId) {
        self.writing.push(node);
    }

    pub(super) fn finish_writing(&mut self, node: StableNodeId) {
        if let Some(index) = self.writing.iter().rposition(|id| *id == node) {
            self.writing.remove(index);
        }
    }

    /// Releases the journal of a node that no longer exists.
    pub(super) fn forget(&mut self, node: StableNodeId) {
        self.entries.remove(&node);
    }
}

impl crate::AppContext {
    fn editor_text_stamp(&self, node: StableNodeId) -> Option<TextStamp> {
        self.world
            .text_input(node)
            .map(|input| input.session().text().stamp())
    }

    pub(super) fn text_histories_written_by(
        &self,
        mutations: &crate::MutationQueue,
    ) -> Vec<StableNodeId> {
        if !mutations.writes_text() || self.text_histories.entries.is_empty() {
            return Vec::new();
        }
        mutations
            .as_slice()
            .iter()
            .filter_map(|mutation| match mutation {
                crate::UiMutation::SetTextInput { id, .. }
                | crate::UiMutation::ReplaceTextSelection { id, .. }
                    if self.text_histories.entries.contains_key(id)
                        && !self.text_histories.writing.contains(id) =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect()
    }

    pub(super) fn verify_text_histories(&mut self, written: Vec<StableNodeId>) {
        for node in written {
            self.text_histories
                .follow(node, self.editor_text_stamp(node));
        }
    }

    /// Start an independent undo session when an existing editor is rebound to
    /// another business object, even if the new text is identical.
    pub fn clear_text_history(&mut self, node: StableNodeId) -> Result<(), crate::FrameworkError> {
        if !self.world.contains(node) {
            return Err(crate::FrameworkError::MissingView(node));
        }
        if self.world.text_input(node).is_none() {
            return Err(crate::FrameworkError::InvalidInput);
        }
        self.text_histories.forget(node);
        Ok(())
    }

    /// The single place an editor's text state changes.
    ///
    /// `apply` mutates the component; everything that must happen for *every*
    /// edit happens here: the change event, and the undo journal. Callers pass
    /// why the edit happened so consecutive typing or deletion collapses into
    /// one undo step.
    ///
    /// Returns whether the state actually changed.
    pub(super) fn commit_editor_edit<C: super::EditableText>(
        &mut self,
        entity: crate::Entity<C>,
        origin: TextEditOrigin,
        apply: impl FnOnce(&mut C, &mut crate::ViewContext<'_, C>) -> bool,
    ) -> Result<bool, crate::FrameworkError> {
        self.journal_editor_edit(entity, origin, |editable, cx| {
            if !apply(editable, cx) {
                return false;
            }
            cx.emit(editable.change());
            true
        })
    }

    /// [`Self::commit_editor_edit`] without the text change event, for a
    /// write whose component reports it in its own terms: a `NumberInput`
    /// step, commit or revert rewrites the draft and emits `NumberChanged`
    /// when the number moves. The journal still records it, so undo and the
    /// typing runs around it stay in step with the text.
    pub(super) fn journal_editor_edit<C: super::EditableText>(
        &mut self,
        entity: crate::Entity<C>,
        origin: TextEditOrigin,
        apply: impl FnOnce(&mut C, &mut crate::ViewContext<'_, C>) -> bool,
    ) -> Result<bool, crate::FrameworkError> {
        // A program write is not the user's edit: whatever it changed (a
        // `NumberInput` can take a new number under a draft that already reads
        // it), the history starts afresh. Nothing to snapshot for that.
        if origin == TextEditOrigin::Program {
            let changed = self.update_component(entity, apply)?;
            if changed {
                self.text_histories.forget(entity.stable_id());
            }
            return Ok(changed);
        }
        let snapshot = |editable: &C| EditorSnapshot {
            state: editable.state().clone(),
            number: editable.committed_number(),
        };
        let before = self.read(entity, snapshot)?;
        self.text_histories.begin_writing(entity.stable_id());
        let changed = self.update_component(entity, apply);
        self.text_histories.finish_writing(entity.stable_id());
        let changed = changed?;
        if !changed {
            return Ok(false);
        }
        let after = self.read(entity, snapshot)?;
        // Text is what the user sees change, so it is what makes a step. A
        // commit that only moves a `NumberInput`'s number is not one: it
        // rides on the step before it, so undo past it lands on the typing
        // that wrote its draft, number included.
        if after.state.value != before.state.value {
            self.text_histories
                .record(entity.stable_id(), before, after, origin);
        } else if after.number != before.number && origin != TextEditOrigin::History {
            // Undo and redo restore a snapshot; they never rewrite one.
            self.text_histories.amend(entity.stable_id(), before, after);
        }
        self.text_histories.witness(
            entity.stable_id(),
            self.editor_text_stamp(entity.stable_id()),
        );
        Ok(true)
    }

    /// Undoes the focused editor's last edit. Returns whether anything moved.
    ///
    /// Restores the selection the edit started from, so undoing puts the caret
    /// back where the user was working rather than where the edit ended.
    /// A composition in progress is left alone: undo during preedit would fight
    /// the IME.
    pub fn undo_focused_text(
        &mut self,
        document: crate::DocumentId,
    ) -> Result<bool, crate::FrameworkError> {
        self.step_focused_text_history(document, true)
    }

    /// Redoes the edit the last [`Self::undo_focused_text`] took back.
    pub fn redo_focused_text(
        &mut self,
        document: crate::DocumentId,
    ) -> Result<bool, crate::FrameworkError> {
        self.step_focused_text_history(document, false)
    }

    fn step_focused_text_history(
        &mut self,
        document: crate::DocumentId,
        undo: bool,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.accepts_input {
            return Ok(false);
        }
        let node = focused.node;
        let Some(target) = (if undo {
            self.text_histories.undo(node)
        } else {
            self.text_histories.redo(node)
        }) else {
            return Ok(false);
        };
        self.restore_editor_snapshot(node, target)
    }

    /// Ends the current typing or deletion run for an editor, so the next edit
    /// starts a new undo step. Caret moves and focus changes call this.
    pub(super) fn seal_editor_history(&mut self, node: StableNodeId) {
        self.text_histories.seal(node);
    }

    pub(super) fn seal_blurred_editor_history(
        &mut self,
        document: crate::DocumentId,
        previous: Option<StableNodeId>,
    ) {
        if previous != self.world.focused(document)
            && let Some(previous) = previous
        {
            self.seal_editor_history(previous);
        }
    }

    /// Whether the editor has an edit to undo.
    pub fn can_undo_text(&self, node: StableNodeId) -> bool {
        self.text_histories.can_undo(node)
    }

    /// Whether the editor has an undone edit to redo.
    pub fn can_redo_text(&self, node: StableNodeId) -> bool {
        self.text_histories.can_redo(node)
    }
}

#[cfg(test)]
mod editor_tests {
    use crate::{AppContext, DocumentId, TextArea, TextDeleteKind, TextInput};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn focused_area(cx: &mut AppContext, value: &str) -> crate::Entity<TextArea> {
        let area = cx
            .create_component(document(), TextArea::new(value))
            .unwrap();
        cx.focus_node(document(), area.stable_id()).unwrap();
        area
    }

    fn value_of(cx: &AppContext, area: crate::Entity<TextArea>) -> String {
        cx.read(area, |area| area.state.value.to_string()).unwrap()
    }

    #[test]
    fn a_cut_is_its_own_undo_step_that_typing_does_not_extend() {
        let mut cx = AppContext::new();
        let area = focused_area(&mut cx, "");
        cx.replace_focused_text(document(), "abc").unwrap();
        cx.select_all_focused_text(document()).unwrap();
        assert_eq!(
            cx.cut_focused_text(document()).unwrap().as_deref(),
            Some("abc")
        );
        cx.replace_focused_text(document(), "x").unwrap();
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "", "undo takes back the typing alone");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "abc", "and then the cut");
    }

    #[test]
    fn a_snippet_insertion_is_one_undo_step() {
        let mut cx = AppContext::new();
        let area = cx
            .create_component(document(), TextArea::new("tail").code_editor(true))
            .unwrap();
        cx.focus_node(document(), area.stable_id()).unwrap();
        cx.select_focused_text_range(document(), 0, 0).unwrap();
        let snippet = crate::TextSnippet::new("let", "let $1 = $2;$0");
        assert!(
            cx.insert_focused_text_snippet(document(), &snippet)
                .unwrap()
        );
        assert_eq!(value_of(&cx, area), "let  = ;tail");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "tail");
        assert!(cx.redo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "let  = ;tail");
    }

    #[test]
    fn a_number_inputs_ime_commit_stays_after_the_next_keystroke() {
        let mut cx = AppContext::new();
        let input = cx
            .create_component(document(), crate::NumberInput::new(1.0))
            .unwrap();
        let node = input.stable_id();
        cx.focus_node(document(), node).unwrap();
        cx.select_all_focused_text(document()).unwrap();
        cx.set_ime_preedit(document(), "２".into(), None).unwrap();
        assert!(cx.commit_ime(document(), "2").unwrap());
        cx.replace_focused_text(document(), "5").unwrap();
        let component = cx
            .read(input, |input| input.state.value.to_string())
            .unwrap();
        assert_eq!(component, "25", "the commit reached the component");
        assert_eq!(cx.world().text_input(node).unwrap().value, "25");
    }

    #[test]
    fn a_number_inputs_typed_and_ime_drafts_undo_and_redo() {
        for ime in [false, true] {
            let mut cx = AppContext::new();
            let input = cx
                .create_component(document(), crate::NumberInput::new(1.0))
                .unwrap();
            cx.focus_node(document(), input.stable_id()).unwrap();
            cx.select_all_focused_text(document()).unwrap();
            if ime {
                cx.set_ime_preedit(document(), "４２".into(), None).unwrap();
                assert!(cx.commit_ime(document(), "42").unwrap());
            } else {
                cx.replace_focused_text(document(), "4").unwrap();
                cx.replace_focused_text(document(), "2").unwrap();
            }
            let draft = |cx: &AppContext| {
                cx.read(input, |input| input.state.value.to_string())
                    .unwrap()
            };
            assert_eq!(draft(&cx), "42");

            assert!(cx.undo_focused_text(document()).unwrap(), "ime: {ime}");
            assert_eq!(draft(&cx), "1", "one run of edits, one step");
            let selection = cx.read(input, |input| input.state.selection).unwrap();
            assert_eq!(
                (selection.anchor, selection.focus),
                (0, 1),
                "undo restores the selection the edit replaced"
            );
            assert!(cx.redo_focused_text(document()).unwrap());
            assert_eq!(draft(&cx), "42");

            // Typing never committed a number, so redo restores none: the
            // draft is still pending until Enter or blur.
            assert_eq!(cx.read(input, crate::NumberInput::value).unwrap(), 1.0);
            assert!(cx.commit_focused_number_input(document()).unwrap());
            assert_eq!(cx.read(input, crate::NumberInput::value).unwrap(), 42.0);
        }
    }

    fn focused_number(
        cx: &mut AppContext,
        input: crate::NumberInput,
    ) -> crate::Entity<crate::NumberInput> {
        let input = cx.create_component(document(), input).unwrap();
        cx.focus_node(document(), input.stable_id()).unwrap();
        input
    }

    fn draft_of(cx: &AppContext, input: crate::Entity<crate::NumberInput>) -> String {
        cx.read(input, |input| input.state.value.to_string())
            .unwrap()
    }

    #[test]
    fn a_number_step_is_its_own_undo_step_between_typing_runs() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(2.0));
        cx.replace_focused_text(document(), "1").unwrap();
        // The step starts from the typed draft, not the committed 2.
        assert!(cx.step_focused_number_input(document(), 1).unwrap());
        assert_eq!(draft_of(&cx, input), "22");
        assert_eq!(cx.read(input, crate::NumberInput::value).unwrap(), 22.0);
        cx.replace_focused_text(document(), "4").unwrap();
        assert_eq!(draft_of(&cx, input), "224");

        // Typing after the step does not merge into the typing before it.
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "22");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "21");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "2");
        for draft in ["21", "22", "224"] {
            assert!(cx.redo_focused_text(document()).unwrap());
            assert_eq!(draft_of(&cx, input), draft);
        }
    }

    #[test]
    fn undoing_a_step_takes_the_number_back_with_the_draft() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        let value = |cx: &AppContext| cx.read(input, crate::NumberInput::value).unwrap();
        for _ in 0..3 {
            cx.step_focused_number_input(document(), 1).unwrap();
        }
        assert_eq!(value(&cx), 4.0);
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("1", 1.0));
        // Escape has nothing to revert: it does not redo the step.
        assert!(!cx.revert_focused_number_input(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("1", 1.0));
        assert!(cx.redo_focused_text(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("4", 4.0));
    }

    #[test]
    fn undo_restores_the_number_the_field_held_not_one_read_from_the_draft() {
        // An empty draft: nothing to read a number from.
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(5.0));
        let value = |cx: &AppContext| cx.read(input, crate::NumberInput::value).unwrap();
        cx.select_all_focused_text(document()).unwrap();
        cx.delete_focused_text(document(), TextDeleteKind::Backward)
            .unwrap();
        assert!(cx.step_focused_number_input(document(), 1).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("6", 6.0));
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("", 5.0));

        // A pending draft the user never committed: undo does not commit it.
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(2.0));
        let value = |cx: &AppContext| cx.read(input, crate::NumberInput::value).unwrap();
        cx.replace_focused_text(document(), "1").unwrap();
        assert!(cx.step_focused_number_input(document(), 1).unwrap());
        assert_eq!(value(&cx), 22.0);
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("21", 2.0));
    }

    #[test]
    fn a_controlled_echo_of_the_held_number_keeps_the_draft_and_its_history() {
        let mut cx = AppContext::new();
        let input = focused_number(
            &mut cx,
            crate::NumberInput::new(1.0)
                .range(0.0, 10.0)
                .step(0.5)
                .precision(1),
        );
        cx.select_all_focused_text(document()).unwrap();
        cx.replace_focused_text(document(), "7.3").unwrap();
        assert!(cx.commit_focused_number_input(document()).unwrap());
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "7.3");
        let held = cx.read(input, crate::NumberInput::value).unwrap();
        // The application writes back the number it was just told about.
        assert!(!cx.set_number_value(input, held).unwrap());
        assert_eq!(draft_of(&cx, input), "7.3");
        assert!(
            cx.can_redo_text(input.stable_id()),
            "redo survives the echo"
        );
    }

    #[test]
    fn undoing_after_a_commit_that_kept_its_text_goes_back_to_the_typing() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        let value = |cx: &AppContext| cx.read(input, crate::NumberInput::value).unwrap();
        cx.replace_focused_text(document(), "5").unwrap();
        assert!(cx.commit_focused_number_input(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("15", 15.0));
        // One undo, visible: the draft and the number both go back.
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("1", 1.0));
        // Nothing pending is left for a blur to commit over the undo.
        assert!(!cx.commit_focused_number_input(document()).unwrap());
        assert_eq!(value(&cx), 1.0);
    }

    #[test]
    fn a_step_run_that_ends_where_it_began_leaves_no_undo_step() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        cx.replace_focused_text(document(), "0").unwrap();
        cx.seal_editor_history(input.stable_id());
        assert!(cx.step_focused_number_input(document(), 1).unwrap());
        assert!(cx.step_focused_number_input(document(), -1).unwrap());
        assert_eq!(draft_of(&cx, input), "10");
        // The next undo takes back the typing, not an empty step.
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "1");
    }

    #[test]
    fn a_dropped_step_run_leaves_its_number_on_the_step_before_it() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        let value = |cx: &AppContext| cx.read(input, crate::NumberInput::value).unwrap();
        cx.replace_focused_text(document(), "0").unwrap();
        assert!(cx.step_focused_number_input(document(), 1).unwrap());
        assert!(cx.step_focused_number_input(document(), -1).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("10", 10.0));
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("1", 1.0));
        // Redo returns to what the field held, number included.
        assert!(cx.redo_focused_text(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("10", 10.0));
    }

    #[test]
    fn a_step_run_back_to_its_text_spends_no_undo_even_when_the_caret_moved() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(100.0).range(0.0, 200.0));
        assert!(cx.step_focused_number_input(document(), -1).unwrap());
        assert!(cx.step_focused_number_input(document(), 1).unwrap());
        assert_eq!(draft_of(&cx, input), "100");
        assert!(!cx.can_undo_text(input.stable_id()));
    }

    #[test]
    fn a_number_change_that_keeps_the_text_is_an_undo_boundary() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(5.0).range(0.0, 10.3));
        let value = |cx: &AppContext| cx.read(input, crate::NumberInput::value).unwrap();
        cx.select_all_focused_text(document()).unwrap();
        cx.replace_focused_text(document(), "10").unwrap();
        // Up from 10 would clamp to 10.3 and snap back: no step at all.
        assert!(!cx.step_focused_number_input(document(), 1).unwrap());
        assert_eq!(value(&cx), 5.0);
        // A commit moves the number and keeps the text.
        assert!(cx.commit_focused_number_input(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("10", 10.0));
        assert!(cx.step_focused_number_input(document(), -1).unwrap());
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("10", 10.0));
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("5", 5.0));
    }

    #[test]
    fn a_restored_number_follows_the_fields_current_bounds() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0).range(0.0, 10.0));
        cx.select_all_focused_text(document()).unwrap();
        cx.replace_focused_text(document(), "8").unwrap();
        assert!(cx.commit_focused_number_input(document()).unwrap());
        assert!(cx.undo_focused_text(document()).unwrap());
        cx.update_component(input, |input, _| input.spec.maximum = Some(5.0))
            .unwrap();
        assert!(cx.redo_focused_text(document()).unwrap());
        assert_eq!(cx.read(input, crate::NumberInput::value).unwrap(), 5.0);
    }

    #[test]
    fn typing_never_inserts_command_control_characters() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        for text in ["\r", "\u{1b}", "\u{8}", "2\r"] {
            assert!(
                !cx.replace_focused_text(document(), text).unwrap(),
                "{text:?}"
            );
        }
        assert_eq!(draft_of(&cx, input), "1");
        let area = focused_area(&mut cx, "");
        assert!(cx.replace_focused_text(document(), "a\tb\nc").unwrap());
        assert_eq!(value_of(&cx, area), "a\tb\nc");
    }

    #[test]
    fn a_number_move_on_a_cleared_history_can_still_be_undone() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        let value = |cx: &AppContext| cx.read(input, crate::NumberInput::value).unwrap();
        cx.replace_focused_text(document(), "5").unwrap();
        cx.clear_text_history(input.stable_id()).unwrap();
        // The commit keeps the text "15" and moves the number 1 -> 15.
        assert!(cx.commit_focused_number_input(document()).unwrap());
        assert_eq!(value(&cx), 15.0);
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("15", 1.0));
    }

    #[test]
    fn a_number_draft_never_takes_control_characters() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        // The newline or tab a host reports with Enter or Tab.
        assert!(!cx.replace_focused_text(document(), "\n").unwrap());
        assert!(!cx.replace_focused_text(document(), "\t").unwrap());
        assert_eq!(draft_of(&cx, input), "1");
        // A pasted cell keeps its number and loses its line break.
        cx.select_all_focused_text(document()).unwrap();
        assert!(cx.paste_focused_text(document(), "12\r\n").unwrap());
        assert_eq!(draft_of(&cx, input), "12");
    }

    #[test]
    fn a_draft_past_an_off_grid_maximum_commits_what_it_shows() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(5.0).range(0.0, 10.3));
        let value = |cx: &AppContext| cx.read(input, crate::NumberInput::value).unwrap();
        cx.select_all_focused_text(document()).unwrap();
        cx.replace_focused_text(document(), "11").unwrap();
        assert!(cx.commit_focused_number_input(document()).unwrap());
        assert_eq!((draft_of(&cx, input).as_str(), value(&cx)), ("10", 10.0));
        // Settled: a second commit and a controlled echo move nothing.
        assert!(!cx.commit_focused_number_input(document()).unwrap());
        assert!(!cx.set_number_value(input, 10.0).unwrap());
        assert!(cx.can_undo_text(input.stable_id()));
        // Up from a draft past the reachable maximum does not go down.
        cx.select_all_focused_text(document()).unwrap();
        cx.replace_focused_text(document(), "10.6").unwrap();
        assert!(!cx.step_focused_number_input(document(), 1).unwrap());
        assert_eq!(value(&cx), 10.0);
    }

    #[test]
    fn an_ime_commit_into_a_number_draft_drops_control_characters() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(5.0));
        assert!(cx.commit_ime(document(), "1\n").unwrap());
        assert_eq!(draft_of(&cx, input), "51");
    }

    #[test]
    fn a_composition_owns_a_number_draft_until_it_ends() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        cx.replace_focused_text(document(), "5").unwrap();
        cx.set_ime_preedit(document(), "ｘ".into(), None).unwrap();
        assert!(!cx.step_focused_number_input(document(), 1).unwrap());
        assert!(!cx.revert_focused_number_input(document()).unwrap());
        assert!(
            !cx.apply_accessibility_action(
                document(),
                crate::AccessibilityActionRequest {
                    target: input.stable_id(),
                    action: crate::AccessibilityAction::SetValue("7".into()),
                },
            )
            .unwrap()
        );
        assert_eq!(draft_of(&cx, input), "15");
        assert_eq!(cx.read(input, crate::NumberInput::value).unwrap(), 1.0);
        // Nor does an explicit commit write under the live preedit.
        assert!(!cx.commit_focused_number_input(document()).unwrap());
        assert_eq!(cx.read(input, crate::NumberInput::value).unwrap(), 1.0);
        // Leaving the field cancels the composition and settles the draft.
        assert!(cx.clear_focus(document()).unwrap());
        assert!(cx.world().ime(input.stable_id()).is_none());
        assert_eq!(draft_of(&cx, input), "15");
        assert_eq!(cx.read(input, crate::NumberInput::value).unwrap(), 15.0);
    }

    #[test]
    fn a_run_of_number_steps_is_one_undo_step() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(2.0));
        cx.replace_focused_text(document(), "1").unwrap();
        // A held arrow key: many steps, one entry.
        for _ in 0..50 {
            assert!(cx.step_focused_number_input(document(), 1).unwrap());
        }
        assert_eq!(draft_of(&cx, input), "71");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "21", "the whole run undoes at once");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "2", "then the typing before it");
    }

    #[test]
    fn a_commit_splits_the_step_runs_around_it() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        for _ in 0..3 {
            cx.step_focused_number_input(document(), 1).unwrap();
        }
        // The draft already reads 4: the commit rewrites nothing.
        assert!(!cx.commit_focused_number_input(document()).unwrap());
        for _ in 0..2 {
            cx.step_focused_number_input(document(), 1).unwrap();
        }
        assert_eq!(draft_of(&cx, input), "6");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "4", "undo stops at the commit");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "1");
    }

    #[test]
    fn an_escape_that_reverts_nothing_still_splits_step_runs() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        for _ in 0..3 {
            cx.step_focused_number_input(document(), 1).unwrap();
        }
        assert!(!cx.revert_focused_number_input(document()).unwrap());
        for _ in 0..2 {
            cx.step_focused_number_input(document(), 1).unwrap();
        }
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "4", "undo stops at the Escape");
    }

    #[test]
    fn committing_and_reverting_a_number_draft_are_undo_steps() {
        let mut cx = AppContext::new();
        let input = focused_number(
            &mut cx,
            crate::NumberInput::new(1.0)
                .range(0.0, 10.0)
                .step(0.5)
                .precision(1),
        );
        cx.select_all_focused_text(document()).unwrap();
        cx.replace_focused_text(document(), "7.3").unwrap();
        assert!(cx.commit_focused_number_input(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "7.5");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "7.3", "undo takes back the snap");
        assert!(cx.redo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "7.5", "redo brings the snapped draft");

        cx.replace_focused_text(document(), "9").unwrap();
        assert!(cx.revert_focused_number_input(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "7.5");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(
            draft_of(&cx, input),
            "7.59",
            "undo takes back the revert alone"
        );
    }

    #[test]
    fn an_application_number_write_over_a_matching_draft_clears_the_history() {
        // The draft already reads the number the application writes, so the
        // text does not change, but the committed number does.
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        cx.select_all_focused_text(document()).unwrap();
        cx.replace_focused_text(document(), "5").unwrap();
        assert!(cx.set_number_value(input, 5.0).unwrap());
        assert_eq!(draft_of(&cx, input), "5");
        assert!(!cx.can_undo_text(input.stable_id()));
        assert!(!cx.undo_focused_text(document()).unwrap());
    }

    #[test]
    fn an_application_number_write_clears_the_draft_history() {
        let mut cx = AppContext::new();
        let input = focused_number(&mut cx, crate::NumberInput::new(1.0));
        cx.replace_focused_text(document(), "5").unwrap();
        assert!(cx.can_undo_text(input.stable_id()));
        assert!(cx.set_number_value(input, 50.0).unwrap());
        assert!(
            !cx.can_undo_text(input.stable_id()),
            "undo must not bring a draft back over the application's value"
        );
        assert!(!cx.undo_focused_text(document()).unwrap());
        assert_eq!(draft_of(&cx, input), "50");
    }

    #[test]
    fn left_and_right_collapse_a_selection_onto_its_edge() {
        // Right lands on the anchor, the selection's end: on the side of the
        // text it selected, as the collapse probed it.
        for (intent, landing) in [
            (crate::TextCaretIntent::Left, crate::TextSelection::caret(1)),
            (
                crate::TextCaretIntent::Right,
                crate::TextSelection::caret(4).with_affinity(crate::TextAffinity::Upstream),
            ),
        ] {
            let mut cx = AppContext::new();
            let input = cx
                .create_component(document(), TextInput::new("abcdef"))
                .unwrap();
            cx.focus_node(document(), input.stable_id()).unwrap();
            // Focus at the far end from where Left lands, and the near end
            // for Right: a step from the focus would land elsewhere.
            cx.select_focused_text_range(document(), 4, 1).unwrap();
            assert!(
                cx.move_focused_text_caret(document(), intent, false, None)
                    .unwrap()
            );
            assert_eq!(
                cx.world().text_input(input.stable_id()).unwrap().selection,
                landing,
                "{intent:?}"
            );
        }
    }

    #[test]
    fn focus_changes_separate_typing_runs_but_refocusing_the_same_editor_does_not() {
        for clear in [false, true] {
            let mut cx = AppContext::new();
            let area = focused_area(&mut cx, "");
            cx.replace_focused_text(document(), "first").unwrap();
            assert!(!cx.focus_node(document(), area.stable_id()).unwrap());
            cx.replace_focused_text(document(), " run").unwrap();
            if clear {
                cx.clear_focus(document()).unwrap();
            } else {
                let other = cx.create_component(document(), TextInput::new("")).unwrap();
                cx.focus_node(document(), other.stable_id()).unwrap();
            }
            cx.focus_node(document(), area.stable_id()).unwrap();
            cx.replace_focused_text(document(), " second").unwrap();
            assert!(cx.undo_focused_text(document()).unwrap());
            assert_eq!(value_of(&cx, area), "first run");
            assert!(cx.undo_focused_text(document()).unwrap());
            assert_eq!(value_of(&cx, area), "");
            assert!(cx.redo_focused_text(document()).unwrap());
            assert_eq!(value_of(&cx, area), "first run");
        }
    }

    #[test]
    fn select_all_and_accessibility_selection_separate_replacement_from_typing() {
        for accessibility in [false, true] {
            let mut cx = AppContext::new();
            let area = focused_area(&mut cx, "");
            cx.replace_focused_text(document(), "first").unwrap();
            if accessibility {
                assert!(
                    cx.apply_accessibility_action(
                        document(),
                        crate::AccessibilityActionRequest {
                            target: area.stable_id(),
                            action: crate::AccessibilityAction::SetSelection(
                                crate::TextSelection::new(0, 5)
                            ),
                        }
                    )
                    .unwrap()
                );
            } else {
                cx.select_all_focused_text(document()).unwrap();
            }
            cx.replace_focused_text(document(), "second").unwrap();
            assert!(cx.undo_focused_text(document()).unwrap());
            assert_eq!(value_of(&cx, area), "first");
            assert!(cx.redo_focused_text(document()).unwrap());
            assert_eq!(value_of(&cx, area), "second");
        }
    }

    #[test]
    fn rejected_focus_and_selection_leave_the_typing_run_intact() {
        let mut cx = AppContext::new();
        let area = focused_area(&mut cx, "");
        cx.replace_focused_text(document(), "first").unwrap();
        let structure = cx
            .create_component(document(), crate::Stack::column(0.0))
            .unwrap();
        assert!(!cx.focus_node(document(), structure.stable_id()).unwrap());
        assert!(
            !cx.apply_accessibility_action(
                document(),
                crate::AccessibilityActionRequest {
                    target: area.stable_id(),
                    action: crate::AccessibilityAction::SetSelection(crate::TextSelection::new(
                        0, 999
                    )),
                }
            )
            .unwrap()
        );
        assert!(
            !cx.move_focused_text_caret(document(), crate::TextCaretIntent::Right, false, None)
                .unwrap()
        );
        cx.replace_focused_text(document(), " second").unwrap();
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "");
    }

    #[test]
    fn pointer_and_public_range_selection_split_history_in_the_same_editor() {
        for pointer in [false, true] {
            let mut cx = AppContext::new();
            let area = focused_area(&mut cx, "");
            cx.replace_focused_text(document(), "first").unwrap();
            if pointer {
                let mut queue = crate::MutationQueue::new();
                queue.write_layout(
                    area.stable_id(),
                    crate::LayoutBox {
                        x: 0.0,
                        y: 0.0,
                        width: 200.0,
                        height: 80.0,
                    },
                );
                cx.commit_mutations(queue).unwrap();
                cx.resolve_styles(&[area.stable_id()]).unwrap();
                cx.shape_text(&[area.stable_id()], &mut crate::MeasureTextShaper)
                    .unwrap();
                let (content, _) = cx
                    .world()
                    .text_input_pointer_context(area.stable_id())
                    .unwrap();
                assert!(
                    cx.text_editor_pointer_press(
                        document(),
                        area.stable_id(),
                        1,
                        content.x + 1.0,
                        content.y + 1.0,
                        false,
                        false,
                        std::time::Duration::ZERO,
                        &mut crate::MeasureTextShaper
                    )
                    .unwrap()
                );
                cx.text_editor_pointer_release(1);
            } else {
                assert!(cx.select_focused_text_range(document(), 0, 0).unwrap());
            }
            assert_eq!(
                cx.read(area, |area| area.state.selection).unwrap(),
                crate::TextSelection::caret(0)
            );
            cx.replace_focused_text(document(), "second ").unwrap();
            assert!(cx.undo_focused_text(document()).unwrap());
            assert_eq!(value_of(&cx, area), "first");
            assert!(cx.redo_focused_text(document()).unwrap());
            assert_eq!(value_of(&cx, area), "second first");
        }
    }

    #[test]
    fn modal_focus_round_trip_seals_history_but_rejected_focus_batch_does_not() {
        let mut cx = AppContext::new();
        let area = focused_area(&mut cx, "");
        let host = cx
            .create_component(document(), crate::OverlayHost::new())
            .unwrap();
        let dialog = cx
            .create_component(document(), crate::Dialog::new("Dialog"))
            .unwrap();
        cx.append_child(host, dialog).unwrap();
        cx.replace_focused_text(document(), "first").unwrap();
        let mut rejected = crate::MutationQueue::new();
        rejected.request_focus(document(), Some(host.stable_id()));
        assert!(cx.commit_mutations(rejected).is_err());
        assert_eq!(cx.world().focused(document()), Some(area.stable_id()));
        cx.replace_focused_text(document(), " run").unwrap();
        cx.activate_overlay(host, dialog).unwrap();
        assert_eq!(cx.world().focused(document()), Some(dialog.stable_id()));
        cx.dismiss_overlay(host).unwrap();
        assert_eq!(cx.world().focused(document()), Some(area.stable_id()));
        cx.replace_focused_text(document(), " second").unwrap();
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "first run");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "");
    }

    #[test]
    fn style_resolution_clearing_focus_seals_history() {
        let mut cx = AppContext::new();
        let area = focused_area(&mut cx, "");
        cx.replace_focused_text(document(), "first").unwrap();
        let initial = cx.read(area, |area| area.style.clone()).unwrap();
        let mut hidden = initial.clone();
        std::sync::Arc::make_mut(&mut hidden.layout).display =
            Some(nana_ui_core::DisplaySpec::None);
        cx.update_component(area, |area, _| area.style = hidden)
            .unwrap();
        cx.resolve_styles(&[area.stable_id()]).unwrap();
        assert_eq!(cx.world().focused(document()), None);
        cx.update_component(area, |area, _| area.style = initial)
            .unwrap();
        cx.resolve_styles(&[area.stable_id()]).unwrap();
        cx.focus_node(document(), area.stable_id()).unwrap();
        cx.replace_focused_text(document(), " second").unwrap();
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "first");
    }

    #[test]
    fn identity_rebind_clears_undo_and_redo_without_changing_editor_state() {
        let mut cx = AppContext::new();
        let area = focused_area(&mut cx, "");
        cx.replace_focused_text(document(), "first").unwrap();
        assert!(cx.can_undo_text(area.stable_id()));
        cx.clear_text_history(area.stable_id()).unwrap();
        assert_eq!(value_of(&cx, area), "first");
        assert!(!cx.undo_focused_text(document()).unwrap());
        cx.replace_focused_text(document(), "second").unwrap();
        assert!(cx.undo_focused_text(document()).unwrap());
        assert!(cx.can_redo_text(area.stable_id()));
        let state = cx.read(area, |area| area.state.clone()).unwrap();
        cx.set_ime_preedit(document(), "ni".to_owned(), None)
            .unwrap();
        cx.clear_text_history(area.stable_id()).unwrap();
        assert!(!cx.can_redo_text(area.stable_id()));
        assert_eq!(cx.read(area, |area| area.state.clone()).unwrap(), state);
        assert!(cx.world().ime(area.stable_id()).is_some());
    }

    #[test]
    fn a_composition_is_one_undo_step_and_its_preedit_is_never_a_step() {
        let mut cx = AppContext::new();
        let area = focused_area(&mut cx, "");

        // Preedit lives in the world's IME slot, not in the value, so these
        // intermediate states must not become undoable steps.
        cx.set_ime_preedit(document(), "ni".to_owned(), None)
            .unwrap();
        cx.set_ime_preedit(document(), "nih".to_owned(), None)
            .unwrap();
        cx.set_ime_preedit(document(), "niha".to_owned(), None)
            .unwrap();
        assert!(
            !cx.can_undo_text(area.stable_id()),
            "composition in flight is not an edit yet"
        );

        cx.commit_ime(document(), "你好").unwrap();
        assert_eq!(value_of(&cx, area), "你好");

        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "", "one commit undoes as one step");
        assert!(!cx.can_undo_text(area.stable_id()));

        assert!(cx.redo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "你好");
    }

    #[test]
    fn a_typing_run_undoes_as_one_step_and_a_caret_move_splits_it() {
        let mut cx = AppContext::new();
        let area = focused_area(&mut cx, "");

        for character in ["a", "b", "c"] {
            cx.replace_focused_text(document(), character).unwrap();
        }
        assert_eq!(value_of(&cx, area), "abc");

        // Arrow away, then type again: the second run is a separate step.
        cx.move_focused_text_caret(document(), crate::TextCaretIntent::Left, false, None)
            .unwrap();
        cx.replace_focused_text(document(), "X").unwrap();

        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "abc", "only the second run came back");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "");
        assert!(!cx.can_undo_text(area.stable_id()));
    }

    #[test]
    fn undo_restores_the_multi_cursor_selection_the_edit_started_from() {
        let mut cx = AppContext::new();
        let area = focused_area(&mut cx, "one\ntwo\nthree");

        // Two cursors, then one edit across both.
        cx.select_focused_text_range(document(), 0, 0).unwrap();
        cx.add_focused_text_cursor(document(), false, None).unwrap();
        let before = cx
            .read(area, |area| area.state.additional_selections.len())
            .unwrap();
        assert_eq!(before, 1, "the second cursor is live");

        cx.replace_focused_text(document(), "#").unwrap();
        assert_eq!(value_of(&cx, area), "#one\n#two\nthree");

        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "one\ntwo\nthree");
        assert_eq!(
            cx.read(area, |area| area.state.additional_selections.len())
                .unwrap(),
            before,
            "the cursor set the edit started from is restored"
        );
    }

    #[test]
    fn an_application_write_is_not_something_the_user_can_undo_into() {
        let mut cx = AppContext::new();
        let area = focused_area(&mut cx, "");
        cx.replace_focused_text(document(), "draft").unwrap();
        assert!(cx.can_undo_text(area.stable_id()));

        // Loading a different document is not the user's edit.
        cx.apply_accessibility_action(
            document(),
            crate::AccessibilityActionRequest {
                target: area.stable_id(),
                action: crate::AccessibilityAction::SetValue("loaded from disk".to_owned()),
            },
        )
        .unwrap();
        assert!(
            !cx.can_undo_text(area.stable_id()),
            "undo must not walk back into the previous document"
        );
        assert_eq!(value_of(&cx, area), "loaded from disk");
    }

    #[test]
    fn a_read_only_editor_has_nothing_to_undo() {
        let mut cx = AppContext::new();
        let field = cx
            .create_component(document(), TextInput::new("fixed").read_only(true))
            .unwrap();
        cx.focus_node(document(), field.stable_id()).unwrap();

        cx.replace_focused_text(document(), "x").unwrap();
        assert!(!cx.can_undo_text(field.stable_id()));
        assert!(!cx.undo_focused_text(document()).unwrap());
    }

    #[test]
    fn deletion_collapses_into_its_own_run_separate_from_typing() {
        let mut cx = AppContext::new();
        let area = focused_area(&mut cx, "");
        cx.replace_focused_text(document(), "abcdef").unwrap();
        for _ in 0..3 {
            cx.delete_focused_text(document(), TextDeleteKind::Backward)
                .unwrap();
        }
        assert_eq!(value_of(&cx, area), "abc");

        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "abcdef", "three deletes, one step");
        assert!(cx.undo_focused_text(document()).unwrap());
        assert_eq!(value_of(&cx, area), "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(value: &str) -> EditorSnapshot {
        EditorSnapshot {
            state: TextInputState::new(value),
            number: None,
        }
    }

    fn record(history: &mut TextHistory, from: &str, to: &str, origin: TextEditOrigin) {
        history.record(state(from), state(to), origin);
    }

    /// The step count alone bounds the journal only for small documents: a
    /// step holds the value before AND after, so a deep journal of a large
    /// document would be hundreds of megabytes. Past the byte budget the
    /// oldest steps go -- and the newest edit stays undoable, however large.
    #[test]
    fn a_large_document_journal_stays_inside_its_byte_budget() {
        let big = "x".repeat(512 * 1024);
        let mut history = TextHistory::default();
        for step in 0..40 {
            // Each step is its own (Structural never merges), and each end is
            // half a megabyte.
            record(
                &mut history,
                &format!("{big}{step}"),
                &format!("{big}{step}!"),
                TextEditOrigin::Structural,
            );
            assert!(
                history.bytes() <= TextHistory::CAPACITY_BYTES + 2 * big.len(),
                "step {step}: {} bytes",
                history.bytes()
            );
        }
        assert!(history.steps.len() < 40, "old steps went");
        assert!(history.can_undo(), "the newest edit is still undoable");
        assert_eq!(
            history.undo().map(|snapshot| snapshot.state.value.len()),
            Some(big.len() + 2),
            "and it undoes to the value that edit started from"
        );

        // One step bigger than the whole budget is still kept: an editor with
        // no undo at all would be worse than one over budget.
        let huge = "y".repeat(TextHistory::CAPACITY_BYTES * 2);
        let mut history = TextHistory::default();
        record(
            &mut history,
            &huge,
            &format!("{huge}!"),
            TextEditOrigin::Paste,
        );
        assert_eq!(history.steps.len(), 1);
        assert!(history.can_undo());
    }

    #[test]
    fn a_run_of_typing_is_one_undo_step() {
        let mut history = TextHistory::default();
        record(&mut history, "", "a", TextEditOrigin::Typing);
        record(&mut history, "a", "ab", TextEditOrigin::Typing);
        record(&mut history, "ab", "abc", TextEditOrigin::Typing);

        assert_eq!(
            history
                .undo()
                .map(|snapshot| snapshot.state.value.to_string()),
            Some(String::new())
        );
        assert!(!history.can_undo(), "the run collapsed into one step");
        assert_eq!(
            history
                .redo()
                .map(|snapshot| snapshot.state.value.to_string()),
            Some("abc".to_owned())
        );
    }

    #[test]
    fn a_different_origin_starts_a_new_step() {
        let mut history = TextHistory::default();
        record(&mut history, "", "abc", TextEditOrigin::Typing);
        record(&mut history, "abc", "ab", TextEditOrigin::Delete);
        record(&mut history, "ab", "ab!", TextEditOrigin::Paste);

        assert_eq!(
            history.undo().map(|s| s.state.value.to_string()),
            Some("ab".to_owned())
        );
        assert_eq!(
            history.undo().map(|s| s.state.value.to_string()),
            Some("abc".to_owned())
        );
        assert_eq!(
            history.undo().map(|s| s.state.value.to_string()),
            Some(String::new())
        );
        assert!(!history.can_undo());
    }

    #[test]
    fn sealing_breaks_a_typing_run() {
        let mut history = TextHistory::default();
        record(&mut history, "", "ab", TextEditOrigin::Typing);
        history.seal();
        record(&mut history, "ab", "abcd", TextEditOrigin::Typing);

        assert_eq!(
            history.undo().map(|s| s.state.value.to_string()),
            Some("ab".to_owned())
        );
        assert_eq!(
            history.undo().map(|s| s.state.value.to_string()),
            Some(String::new())
        );
    }

    #[test]
    fn typing_after_an_undo_is_a_step_of_its_own() {
        let mut history = TextHistory::default();
        record(&mut history, "", "abc", TextEditOrigin::Typing);
        record(&mut history, "abc", "ab", TextEditOrigin::Delete);
        history.undo();
        record(&mut history, "abc", "abcd", TextEditOrigin::Typing);
        assert_eq!(
            history.undo().map(|s| s.state.value.to_string()),
            Some("abc".to_owned()),
            "not back through the typing before the undo"
        );

        // After a redo, likewise.
        let mut history = TextHistory::default();
        record(&mut history, "", "ab", TextEditOrigin::Typing);
        history.undo();
        history.redo();
        record(&mut history, "ab", "abc", TextEditOrigin::Typing);
        assert_eq!(
            history.undo().map(|s| s.state.value.to_string()),
            Some("ab".to_owned())
        );
    }

    #[test]
    fn editing_after_undo_drops_the_redo_tail() {
        let mut history = TextHistory::default();
        record(&mut history, "", "one", TextEditOrigin::Paste);
        record(&mut history, "one", "two", TextEditOrigin::Paste);
        history.undo();
        assert!(history.can_redo());

        record(&mut history, "one", "three", TextEditOrigin::Paste);
        assert!(!history.can_redo(), "the abandoned branch is gone");
        assert_eq!(
            history.undo().map(|s| s.state.value.to_string()),
            Some("one".to_owned())
        );
    }

    #[test]
    fn undo_and_redo_are_never_recorded_as_steps() {
        let mut history = TextHistory::default();
        record(&mut history, "", "a", TextEditOrigin::Paste);
        record(&mut history, "a", "", TextEditOrigin::History);
        assert_eq!(history.steps.len(), 1);
    }

    #[test]
    fn the_journal_is_bounded() {
        let mut history = TextHistory::default();
        for index in 0..(TextHistory::CAPACITY + 50) {
            record(
                &mut history,
                &index.to_string(),
                &(index + 1).to_string(),
                TextEditOrigin::Paste,
            );
        }
        assert_eq!(history.steps.len(), TextHistory::CAPACITY);
        assert!(history.can_undo());
    }
}

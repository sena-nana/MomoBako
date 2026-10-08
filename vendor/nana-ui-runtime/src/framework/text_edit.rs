//! Text-editor caret, selection, and code-editing commands on [`AppContext`].
//!
//! Keyboard and pointer adapters resolve intents and hand them here; this
//! module owns which focused component is editable, how offsets map onto the
//! committed value, and when change events fire. Geometry-dependent queries
//! receive the host shaper so layout stays backend-owned.

use super::{AppContext, DocumentId, EditableText, Entity, FrameworkError, StableNodeId};
use super::{TextArea, TextInputState, TextSelection};
use crate::components::TextSnippetSession;
use crate::text_editing::{
    CursorEdit, TextCaretIntent, TextLineDirection, TextReplacement, TextSearchOptions,
    apply_cursor_edits, apply_replacement, atoms_in, auto_indent_newline, auto_pair_edit,
    caret_focus, caret_offset_at_point, clamp_boundary, delete_backward_atoms,
    delete_forward_atoms, delete_lines, delete_to_line_end_atoms, delete_to_line_start_atoms,
    delete_word_backward_atoms, delete_word_forward_atoms, duplicate_lines,
    expand_range_over_atoms, expanded_selection, find_matches, find_matches_in_range,
    find_next_match, find_previous_match, indent_selection, join_lines, logical_line_range,
    matching_bracket_pair, move_lines, moved_selection, outdent_selection, page_caret_focus,
    page_caret_focus_logical, preserve_case_replacement, replace_all_matches_in_range,
    snap_moved_caret, snap_pointer_caret, sort_lines, toggle_line_comment,
    transform_selection_case, vertical_caret_focus, vertical_caret_focus_logical, word_range_at,
};
use crate::{
    CodeEditing, MutationQueue, ScrollOffset, TextAtomSpan, TextCodeFold, TextContent,
    TextShapeConstraints, TextSnippet,
};
use std::sync::Arc;

/// Delete semantics for [`AppContext::delete_focused_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDeleteKind {
    Backward,
    Forward,
    WordBackward,
    WordForward,
    LineStart,
    LineEnd,
}

/// Where a find/replace command searches in the focused editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextFindScope {
    /// The whole editor value (default; historical behavior).
    #[default]
    Document,
    /// Find in selection: only matches fully inside the primary selection.
    /// An empty primary selection falls back to the whole document.
    Selection,
}

/// The search span for a [`TextFindScope`], when one exists.
fn find_scope_range(selection: &TextSelection) -> Option<(usize, usize)> {
    let range = selection.ordered();
    (range.start < range.end).then_some((range.start, range.end))
}

/// The match list for a find scope on the editor's value: an empty
/// selection under [`TextFindScope::Selection`] falls back to the whole
/// document.
fn find_matches_in_scope(
    value: &str,
    query: &str,
    options: TextSearchOptions,
    scope: TextFindScope,
    selection: &TextSelection,
) -> Vec<std::ops::Range<usize>> {
    match scope {
        TextFindScope::Document => find_matches(value, query, options),
        TextFindScope::Selection => match find_scope_range(selection) {
            Some(range) => find_matches_in_range(value, query, options, range),
            None => find_matches(value, query, options),
        },
    }
}

/// The focused plain text editor, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusedTextEditor {
    pub node: StableNodeId,
    pub multiline: bool,
    pub accepts_input: bool,
    pub accepts_selection: bool,
    pub code_editing: Option<CodeEditing>,
    pub(crate) kind: TextEditorKind,
}

impl FocusedTextEditor {
    /// A [`crate::NumberInput`]: plain ArrowUp/ArrowDown step its value and
    /// Enter commits its draft, so a host routes those keys to the numeric
    /// field instead of the editor's caret and submit handling.
    pub fn is_numeric(&self) -> bool {
        self.kind == TextEditorKind::Number
    }
}

/// Text editing reached through the edited node's type
/// ([`crate::TypeBehavior::editable`]), so a program links the editing
/// paths of only the editors it creates.
#[doc(hidden)]
pub struct EditableHooks {
    /// A plain editor's own operations; `None` for a composite's search
    /// field.
    pub(super) plain: Option<&'static PlainEditorHooks>,
    pub(super) commit_ime: fn(&mut AppContext, StableNodeId, &str) -> Result<bool, FrameworkError>,
    pub(super) delete_surrounding:
        fn(&mut AppContext, StableNodeId, usize, usize) -> Result<bool, FrameworkError>,
    pub(super) replace_selection: fn(
        &mut AppContext,
        StableNodeId,
        &str,
        crate::TextEditOrigin,
    ) -> Result<bool, FrameworkError>,
    pub(super) delete_backward: fn(&mut AppContext, StableNodeId) -> Result<bool, FrameworkError>,
    pub(super) selected_text: fn(&AppContext, StableNodeId) -> Option<String>,
    pub(super) select_all: fn(&mut AppContext, StableNodeId) -> Result<bool, FrameworkError>,
}

/// What only a plain editor (`TextArea`, `TextInput`, `NumberInput`) does:
/// caret moves, whole-state edits, undo.
pub(super) struct PlainEditorHooks {
    pub(super) kind: TextEditorKind,
    pub(super) info: fn(&AppContext, StableNodeId, TextEditorKind) -> Option<FocusedTextEditor>,
    pub(super) state: fn(&AppContext, StableNodeId) -> Result<TextInputState, FrameworkError>,
    pub(super) update_state: fn(
        &mut AppContext,
        StableNodeId,
        &mut dyn FnMut(&mut TextInputState) -> bool,
    ) -> Result<bool, FrameworkError>,
    pub(super) replace_state: fn(
        &mut AppContext,
        StableNodeId,
        crate::TextEditOrigin,
        TextInputState,
    ) -> Result<bool, FrameworkError>,
    pub(super) restore: fn(
        &mut AppContext,
        StableNodeId,
        super::text_history::EditorSnapshot,
    ) -> Result<bool, FrameworkError>,
    pub(super) set_state:
        fn(&mut AppContext, StableNodeId, TextInputState) -> Result<(), FrameworkError>,
}

/// The editing tables of the editor type `C`.
pub(crate) struct Editable<C>(std::marker::PhantomData<C>);

impl<C: EditableText> Editable<C> {
    /// A composite's search field.
    pub(crate) const HOOKS: EditableHooks = Self::with_plain(None);

    /// A plain editor (`C::PLAIN` names which).
    pub(crate) const EDITOR_HOOKS: EditableHooks = Self::with_plain(Some(&Self::PLAIN));

    const PLAIN: PlainEditorHooks = PlainEditorHooks {
        kind: C::PLAIN.expect("a plain editor names its kind"),
        info: |cx: &AppContext, node, kind| cx.editor_info(Entity::<C>::from_stable_id(node), kind),
        state: |cx: &AppContext, node| cx.editor_state_of::<C>(node),
        update_state: |cx: &mut AppContext, node, update| {
            cx.update_editor_state_of::<C>(node, update)
        },
        replace_state: |cx: &mut AppContext, node, origin, next| {
            cx.replace_editor_state_of::<C>(node, origin, next)
        },
        restore: |cx: &mut AppContext, node, snapshot| {
            cx.restore_editor_snapshot_of::<C>(node, snapshot)
        },
        set_state: |cx: &mut AppContext, node, state| {
            cx.update_component(Entity::<C>::from_stable_id(node), |editor: &mut C, _| {
                *editor.state_mut() = state
            })
        },
    };

    const fn with_plain(plain: Option<&'static PlainEditorHooks>) -> EditableHooks {
        EditableHooks {
            plain,
            commit_ime: |cx: &mut AppContext, node, text: &str| {
                cx.commit_editable_ime(Entity::<C>::from_stable_id(node), text)
            },
            delete_surrounding: |cx: &mut AppContext, node, before, after| {
                cx.delete_editable_surrounding(Entity::<C>::from_stable_id(node), before, after)
            },
            replace_selection: |cx: &mut AppContext, node, text: &str, origin| {
                cx.replace_editable_selection(Entity::<C>::from_stable_id(node), text, origin)
            },
            delete_backward: |cx: &mut AppContext, node| {
                cx.delete_editable_backward(Entity::<C>::from_stable_id(node))
            },
            selected_text: |cx: &AppContext, node| {
                cx.editable_selected_text(Entity::<C>::from_stable_id(node))
            },
            select_all: |cx: &mut AppContext, node| {
                cx.select_all_editable(Entity::<C>::from_stable_id(node))
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextEditorKind {
    Area,
    Field,
    /// A [`crate::NumberInput`]'s draft. Edits change the draft only; the
    /// committed number is parsed from it on Enter or blur.
    Number,
}

/// 拖拽移动选中文本的状态机（见
/// [`AppContext::text_editor_pointer_press`]）。`pending → active → 落点
/// 执行`：按下落在主选区内部时进入 `pending`（不塌缩选区），指针移动
/// 超过 [`TEXT_SELECTION_DRAG_THRESHOLD`] 进入 `active` 并在世界侧发布
/// 落点指示线；释放时 `active` 执行移动/复制（单次拼接修订，一步撤销），
/// `pending` 回落为普通点击落 caret。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TextSelectionDrag {
    pub pointer_id: u64,
    pub node: StableNodeId,
    pub kind: TextEditorKind,
    /// 按下点（世界坐标），用于阈值判定。
    pub press: (f32, f32),
    /// 源选区（按下时主选区，值空间，已有序）。
    pub source: (usize, usize),
    /// 已超过阈值进入拖拽态。
    pub active: bool,
    /// Alt 按住 = 复制（macOS 惯例；普通拖拽为移动）。
    pub copy: bool,
    /// 当前落点（值空间；落在源选区内部时为 `None`，指示无效落点）。
    pub target: Option<usize>,
}

/// 进入拖拽态的指针位移阈值（像素）。
pub(crate) const TEXT_SELECTION_DRAG_THRESHOLD: f32 = 4.0;

/// A [`crate::TextShaper`] probe bound to one editor node's shaped layout.
struct EditorGeometry<'a> {
    shaper: &'a mut dyn crate::TextShaper,
    node: StableNodeId,
    text: TextContent,
    style: crate::ComputedStyle,
    constraints: TextShapeConstraints,
}

impl EditorGeometry<'_> {
    fn probe(&mut self) -> impl FnMut(usize) -> (f32, f32, f32) + '_ {
        move |offset: usize| {
            self.shaper
                .text_position(self.node, &self.text, offset, &self.style, self.constraints)
        }
    }

    /// A vertical or page move answered by the backend's own point hit test:
    /// the caret's position, the last line's, and one hit — rather than a
    /// search over position probes. `None` when the backend cannot hit test,
    /// or the intent is not vertical.
    fn vertical_by_point(
        &mut self,
        value: &str,
        selection: TextSelection,
        intent: TextCaretIntent,
        extend: bool,
        goal_x: Option<f32>,
        page_height: f32,
    ) -> Option<(TextSelection, f32)> {
        let page = match intent {
            TextCaretIntent::Up | TextCaretIntent::Down => None,
            TextCaretIntent::PageUp | TextCaretIntent::PageDown => Some(page_height),
            _ => return None,
        };
        // Asked first: a backend that cannot hit test gets its probes spent on
        // the search fallback, not twice.
        if !self.supports_point_hits() {
            return None;
        }
        let focus = clamp_focus(value, selection.focus);
        let (x, y, height) = self.shaper.text_caret_position(
            self.node,
            &self.text,
            focus,
            selection.affinity,
            &self.style,
            self.constraints,
        );
        let height = height.max(1.0);
        let goal = goal_x.unwrap_or(x);
        let upwards = matches!(intent, TextCaretIntent::Up | TextCaretIntent::PageUp);
        let target_y = if upwards {
            if y <= f32::EPSILON {
                return Some((moved_selection(selection, 0, extend), goal));
            }
            // Just inside the line above's bottom, whatever its height.
            match page {
                Some(page) => (y - page.max(height)).max(0.0) + 0.5,
                None => y - 0.5,
            }
        } else {
            let last_y = self
                .shaper
                .text_position(
                    self.node,
                    &self.text,
                    value.len(),
                    &self.style,
                    self.constraints,
                )
                .1;
            if y + f32::EPSILON >= last_y {
                return Some((moved_selection(selection, value.len(), extend), goal));
            }
            // Just inside the line below's top.
            match page {
                Some(page) => (y + page.max(height)).min(last_y) + 0.5,
                None => y + height + 0.5,
            }
        };
        let hit = self.shaper.text_hit_at_point(
            self.node,
            &self.text,
            goal,
            target_y,
            &self.style,
            self.constraints,
        )?;
        Some((
            moved_selection(selection, hit.offset, extend).with_affinity(hit.affinity),
            goal,
        ))
    }

    /// Where a non-extending Left or Right lands on a non-empty selection:
    /// its edge on that side of the screen, by `nana-text`'s rule
    /// ([`nana_text::editable::collapse_edge`]), read off the carets the
    /// backend draws at the selection's two ends.
    fn collapse_edge(&mut self, selection: TextSelection, rightwards: bool) -> usize {
        let range = selection.ordered();
        let (start_x, start_y, _) = self.shaper.text_caret_position(
            self.node,
            &self.text,
            range.start,
            crate::TextAffinity::Downstream,
            &self.style,
            self.constraints,
        );
        let (end_x, end_y, _) = self.shaper.text_caret_position(
            self.node,
            &self.text,
            range.end,
            crate::TextAffinity::Upstream,
            &self.style,
            self.constraints,
        );
        let rtl = matches!(self.style.direction, nana_ui_core::DirSpec::Rtl);
        // Across columns x says nothing about the reading order, and one
        // column's y is not "the same line": vertical text goes by order.
        let horizontal = matches!(
            self.style.writing_mode,
            nana_ui_core::WritingModeSpec::HorizontalTb
        );
        nana_text::editable::collapse_edge(
            range,
            rightwards,
            horizontal.then_some((start_x, start_y)),
            horizontal.then_some((end_x, end_y)),
            rtl,
        )
    }

    /// One visual step left or right, from the backend's own layout: the
    /// caret an arrow key lands on. `None` when the backend has no geometry,
    /// or there is nothing in that direction.
    fn visual_step(&mut self, selection: TextSelection, rightwards: bool) -> Option<TextSelection> {
        let hit = self.shaper.text_caret_visual_step(
            self.node,
            &self.text,
            selection.focus,
            selection.affinity,
            rightwards,
            &self.style,
            self.constraints,
        )?;
        Some(TextSelection {
            anchor: selection.anchor,
            focus: hit.offset,
            affinity: hit.affinity,
        })
    }

    fn supports_point_hits(&mut self) -> bool {
        self.shaper
            .text_hit_at_point(
                self.node,
                &self.text,
                0.0,
                0.0,
                &self.style,
                self.constraints,
            )
            .is_some()
    }
}

/// Whether an editor's drawn text is its value, so a geometry probe of the
/// value asks about the layout its caret is drawn in.
///
/// A secure field draws bullets and an empty one draws its placeholder.
/// Probing with the value there would answer from a layout that is not on
/// screen, and would push the value -- a password among it -- into the host's
/// retained geometry and its shape cache. Those cases step by grapheme
/// instead, which is what "left" means in a column of identical bullets
/// anyway.
///
/// A live preedit also draws text the value does not contain, but no caret
/// move can run then: [`AppContext::focused_text_editor`] refuses while
/// composing. An ENDED composition leaves an empty preedit in place, and the
/// display text is the value again then, so it must not disable this.
fn editor_draws_its_value(world: &crate::UiWorld, node: StableNodeId, value: &str) -> bool {
    !value.is_empty() && !world.text_input_is_secure(node)
}

/// Whether this intent is the horizontal arrow, and which way it points on
/// screen. `None` for every other intent: only Left/Right follow visual
/// order -- word and line intents are logical by definition, and the vertical
/// ones have their own geometry path.
fn horizontal_rightwards(intent: TextCaretIntent) -> Option<bool> {
    match intent {
        TextCaretIntent::Left => Some(false),
        TextCaretIntent::Right => Some(true),
        _ => None,
    }
}

/// Whether this move collapses a non-empty selection onto its edge: Left or
/// Right without extending. The landing is an edge of what was selected,
/// not a step from anywhere, so the atom rules for a caret leaving a chip do
/// not apply to it.
fn collapses_selection(selection: TextSelection, intent: TextCaretIntent, extend: bool) -> bool {
    !extend && selection.anchor != selection.focus && horizontal_rightwards(intent).is_some()
}

/// A caret at `edge` of `selection`. On the focus it keeps the side the focus
/// was drawn on (the end of a soft-wrapped line stays on that line); on the
/// anchor, the side of the selection it bounds -- the end of a selection
/// that stops at a soft wrap is on the line it covers, as
/// [`EditorGeometry::collapse_edge`] probed it. `nana-text` lands the same.
fn caret_at_edge(selection: TextSelection, edge: usize) -> TextSelection {
    let caret = TextSelection::caret(edge);
    if edge == selection.focus {
        caret.with_affinity(selection.affinity)
    } else if edge == selection.ordered().end {
        caret.with_affinity(crate::TextAffinity::Upstream)
    } else {
        caret
    }
}

/// Nearest char boundary at or below `offset` (clamped to the value length).
fn clamp_focus(value: &str, offset: usize) -> usize {
    crate::text_editing::clamp_boundary(value, offset)
}

fn snap_selection_over_atoms(
    previous: TextSelection,
    next: TextSelection,
    extend: bool,
    atoms: &[TextAtomSpan],
) -> TextSelection {
    if atoms.is_empty() {
        return next;
    }
    let snapped = snap_moved_caret(previous.focus, next.focus, atoms);
    if extend {
        let selection = moved_selection(previous, snapped, true);
        let range = expand_range_over_atoms(selection.ordered(), atoms);
        if selection.focus >= selection.anchor {
            TextSelection::new(range.start, range.end)
        } else {
            TextSelection::new(range.end, range.start)
        }
    } else {
        TextSelection::caret(snapped)
    }
}

/// The word/selection text occurrence search runs on: the primary selection
/// itself, or the word at its caret when the selection is bare.
fn occurrence_query(value: &str, selection: TextSelection) -> Option<(usize, usize)> {
    let range = selection.ordered();
    if range.start != range.end {
        return Some((range.start, range.end));
    }
    let (start, end) = word_range_at(value, selection.focus);
    (start != end).then_some((start, end))
}

/// Whether a candidate match intersects any active selection. Bare carets
/// never block an occurrence: a caret inside a word still lets that word be
/// selected (and fuses with it on normalize).
fn is_covered(candidate: &std::ops::Range<usize>, selections: &[TextSelection]) -> bool {
    selections.iter().any(|selection| {
        let span = selection.ordered();
        !span.is_empty() && candidate.start < span.end && span.start < candidate.end
    })
}

/// The next (or previous, wrapping) occurrence of the primary selection's
/// query that no existing cursor covers.
fn next_occurrence(
    value: &str,
    selections: &[TextSelection],
    primary: TextSelection,
    previous: bool,
) -> Option<std::ops::Range<usize>> {
    let (query_start, query_end) = occurrence_query(value, primary)?;
    let query = &value[query_start..query_end];
    if query.chars().all(char::is_whitespace) {
        return None;
    }
    let matches = find_matches(
        value,
        query,
        TextSearchOptions {
            whole_word: true,
            ..TextSearchOptions::default()
        },
    );
    if matches.is_empty() {
        return None;
    }
    let last_end = selections
        .iter()
        .map(|selection| selection.ordered().end)
        .max()
        .unwrap_or(0);
    let first_start = selections
        .iter()
        .map(|selection| selection.ordered().start)
        .min()
        .unwrap_or(0);
    let start_index = if previous {
        matches
            .iter()
            .rposition(|found| found.end <= first_start)
            .unwrap_or(matches.len() - 1)
    } else {
        matches
            .iter()
            .position(|found| found.start >= last_end)
            .unwrap_or(0)
    };
    for step in 0..matches.len() {
        let index = if previous {
            (start_index + matches.len() - step) % matches.len()
        } else {
            (start_index + step) % matches.len()
        };
        if !is_covered(&matches[index], selections) {
            return Some(matches[index].clone());
        }
    }
    None
}

/// One committed value edit: the next value and where the caret lands.
struct EditorEdit {
    value: String,
    selection: TextSelection,
}

impl From<(String, usize)> for EditorEdit {
    fn from((value, caret): (String, usize)) -> Self {
        EditorEdit {
            value,
            selection: TextSelection::caret(caret),
        }
    }
}

impl From<(String, TextSelection)> for EditorEdit {
    fn from((value, selection): (String, TextSelection)) -> Self {
        EditorEdit { value, selection }
    }
}

impl AppContext {
    /// An arrow key as the focused editor's line space names it (#59).
    ///
    /// Caret intents are in line space, where Left/Right step along a line and
    /// Up/Down cross to the neighbouring one. A host names the key it saw, so
    /// it asks this **before** it folds modifiers into intents: in a vertical
    /// writing mode the line runs down the page and the physical keys trade
    /// places — Up/Down walk the column, Left/Right cross columns, towards
    /// the next one on the left in `vertical-rl` and on the right in
    /// `vertical-lr`. Translating the key rather than the finished intent is
    /// what carries every modifier along: Cmd+Up becomes the column's start
    /// and Cmd+Left in `vertical-rl` the document's end, Alt+Up/Down move by
    /// word, and the line-block gestures move to the keys that cross columns.
    ///
    /// Anything that is not an arrow key, and every key of a horizontal
    /// editor, comes back unchanged.
    pub fn focused_text_line_space_key<'k>(&self, document: DocumentId, key: &'k str) -> &'k str {
        let Some(writing) = self
            .focused_text_editor(document)
            .and_then(|focused| self.world.computed_style(focused.node))
            .map(crate::ComputedStyle::writing_context)
            .filter(|writing| writing.is_vertical())
        else {
            return key;
        };
        let next_column_is_left = writing.block_reversed();
        match key {
            "ArrowUp" => "ArrowLeft",
            "ArrowDown" => "ArrowRight",
            "ArrowLeft" if next_column_is_left => "ArrowDown",
            "ArrowLeft" => "ArrowUp",
            "ArrowRight" if next_column_is_left => "ArrowUp",
            "ArrowRight" => "ArrowDown",
            other => other,
        }
    }

    /// Identify the focused plain text editor (`TextArea`, `TextInput`, or a
    /// `NumberInput`'s draft).
    ///
    /// Composite search surfaces (palettes, menus, dropdowns) own their
    /// navigation and are deliberately not plain editors.
    pub fn focused_text_editor(&self, document: DocumentId) -> Option<FocusedTextEditor> {
        if self.has_focused_ime_composition(document) {
            return None;
        }
        self.focused_plain_editor(document)
    }

    /// Whether a plain text editor is focused with an IME composition in
    /// progress, which hides it from [`Self::focused_text_editor`]. Its
    /// navigation keys then belong to the composition: a host keeps them
    /// from reaching enclosing navigation (tables, trees). Composite search
    /// surfaces are not plain editors and keep their own list navigation.
    pub fn focused_text_editor_composing(&self, document: DocumentId) -> bool {
        self.has_focused_ime_composition(document)
            && self.focused_plain_editor_node(document).is_some()
    }

    /// The focused plain editor's node, composition or not.
    pub(super) fn focused_plain_editor_node(&self, document: DocumentId) -> Option<StableNodeId> {
        self.focused_plain_editor_kind(document)
            .map(|(node, _)| node)
    }

    fn focused_plain_editor(&self, document: DocumentId) -> Option<FocusedTextEditor> {
        let (node, kind) = self.focused_plain_editor_kind(document)?;
        (self.plain_editor(node)?.info)(self, node, kind)
    }

    /// The one probe for which plain editor is focused, composition or not.
    /// Reads no component state.
    pub(super) fn focused_plain_editor_kind(
        &self,
        document: DocumentId,
    ) -> Option<(StableNodeId, TextEditorKind)> {
        let (target, _) = self.world.focused_text_input(document)?;
        Some((target, self.plain_editor_kind(target)?))
    }

    /// The plain editor component behind `node`, if it is one.
    pub(super) fn plain_editor_kind(&self, node: StableNodeId) -> Option<TextEditorKind> {
        Some(self.plain_editor(node)?.kind)
    }

    /// The plain editor operations of the editor type behind `node`.
    pub(super) fn plain_editor(&self, node: StableNodeId) -> Option<&'static PlainEditorHooks> {
        self.editable_hooks(node)?.plain
    }

    /// The editing table of the editor type behind `node`.
    pub(super) fn editable_hooks(&self, node: StableNodeId) -> Option<&'static EditableHooks> {
        self.behavior(node)?.editable
    }

    fn editor_info<C: EditableText>(
        &self,
        entity: Entity<C>,
        kind: TextEditorKind,
    ) -> Option<FocusedTextEditor> {
        let node = entity.id;
        self.read(entity, |editable: &C| FocusedTextEditor {
            node,
            multiline: editable.is_multiline(),
            accepts_input: editable.accepts_input(),
            accepts_selection: editable.accepts_selection(),
            code_editing: editable.code_editing().cloned(),
            kind,
        })
        .ok()
    }

    /// Resolve one caret move for one selection. Vertical intents use the
    /// host shaper when available and fall back to logical-line column
    /// preservation otherwise; single-line fields map vertical intents onto
    /// the line boundaries. Returns the next selection plus the goal column
    /// to retain for chained vertical moves.
    #[allow(clippy::too_many_arguments)]
    fn resolve_caret_move(
        value: &str,
        selection: TextSelection,
        intent: TextCaretIntent,
        extend: bool,
        multiline: bool,
        geometry: Option<&mut EditorGeometry<'_>>,
        page_height: f32,
    ) -> Option<(TextSelection, Option<f32>)> {
        let vertical = matches!(
            intent,
            TextCaretIntent::Up
                | TextCaretIntent::Down
                | TextCaretIntent::PageUp
                | TextCaretIntent::PageDown
        );
        if vertical && multiline {
            if let Some(geometry) = geometry {
                if let Some((selection, goal)) =
                    geometry.vertical_by_point(value, selection, intent, extend, None, page_height)
                {
                    return Some((selection, Some(goal)));
                }
                let moved = if matches!(intent, TextCaretIntent::Up | TextCaretIntent::Down) {
                    vertical_caret_focus(value, selection, intent, extend, None, geometry.probe())
                } else {
                    // One viewport height of visual lines: the content box is
                    // the editor's viewport.
                    page_caret_focus(
                        value,
                        selection,
                        intent,
                        extend,
                        None,
                        page_height,
                        geometry.probe(),
                    )
                };
                return moved.map(|(selection, goal)| (selection, Some(goal)));
            }
            let moved = if matches!(intent, TextCaretIntent::Up | TextCaretIntent::Down) {
                vertical_caret_focus_logical(value, selection, intent, extend)
            } else {
                page_caret_focus_logical(value, selection, intent, extend)
            };
            return moved.map(|selection| (selection, None));
        }
        if vertical {
            // Single-line fields map Up/Down and PageUp/PageDown onto the
            // line boundaries.
            let mapped = match intent {
                TextCaretIntent::Up | TextCaretIntent::PageUp => TextCaretIntent::LineStart,
                _ => TextCaretIntent::LineEnd,
            };
            return caret_focus(value, selection, mapped)
                .map(|focus| (moved_selection(selection, focus, extend), None));
        }
        // Left/Right over a non-empty selection collapse it onto its edge
        // on that side instead of stepping from the focus: the visual edge
        // with geometry, the logical one without.
        if !extend
            && selection.anchor != selection.focus
            && let Some(rightwards) = horizontal_rightwards(intent)
        {
            let edge = match geometry {
                Some(geometry) => geometry.collapse_edge(selection, rightwards),
                None => nana_text::editable::collapse_edge(
                    selection.ordered(),
                    rightwards,
                    None,
                    None,
                    false,
                ),
            };
            return Some((caret_at_edge(selection, edge), None));
        }
        if let Some(geometry) = geometry
            && let Some(rightwards) = horizontal_rightwards(intent)
            && let Some(stepped) = geometry.visual_step(selection, rightwards)
        {
            let moved = if extend {
                stepped
            } else {
                TextSelection::caret(stepped.focus).with_affinity(stepped.affinity)
            };
            return Some((moved, None));
        }
        caret_focus(value, selection, intent)
            .map(|focus| (moved_selection(selection, focus, extend), None))
    }

    /// Move the caret or selection edge of the focused text editor.
    ///
    /// With multiple cursors every selection moves by the same intent; the
    /// shared goal column only tracks the primary cursor.
    ///
    /// 折叠语义：存在折叠态区间时，移动意图在显示视图上解析（探针与
    /// 换行结构与渲染一致），结果映射回值空间——落在折叠隐藏区间上的
    /// 偏移自然钳到折叠起始行行尾。LineStart/LineEnd/Up/Down/PageUp/
    /// PageDown/DocStart/DocEnd 因此跳过折叠区间。
    pub fn move_focused_text_caret(
        &mut self,
        document: DocumentId,
        intent: TextCaretIntent,
        extend: bool,
        mut shaper: Option<&mut dyn crate::TextShaper>,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.accepts_selection {
            return Ok(false);
        }
        let state = self.editor_state(focused.node)?;
        // 折叠视图：无折叠态区间时为 None，全部按原始值解析（零成本）。
        let fold_view = self.world.text_display_view(focused.node);
        let probe_value: &str = fold_view.as_ref().map_or(&state.value, |view| &view.value);
        let probe_text = self.editor_probe_text(focused.node, fold_view.as_ref());
        let to_display = |selection: TextSelection| -> TextSelection {
            match &fold_view {
                Some(view) => TextSelection {
                    anchor: view.display_of(selection.anchor),
                    focus: view.display_of(selection.focus),
                    affinity: selection.affinity,
                },
                None => selection,
            }
        };
        let to_value = |selection: TextSelection| -> TextSelection {
            match &fold_view {
                Some(view) => TextSelection {
                    anchor: view.value_of(selection.anchor),
                    focus: view.value_of(selection.focus),
                    affinity: selection.affinity,
                },
                None => selection,
            }
        };
        // 水平移动的显示目标被覆盖区间（折叠摘要 / inlay 插入文本）钳回
        // 起点值偏移时（区间内部无 caret 边界，逐字符右移会变成空操作），
        // 前进到区间末端再重映射：一次按键跨过整个覆盖区间，值偏移步进
        // 为一。折叠摘要上的同源既有卡死一并修复；点击命中与垂直移动
        // 保持钳制语义（to_value）。
        let stepping = matches!(intent, TextCaretIntent::Right | TextCaretIntent::WordRight);
        let to_value_moved = |previous_focus: usize, moved: TextSelection| -> TextSelection {
            match &fold_view {
                Some(view) => {
                    let crossed = view.crossed_cover(moved.focus, stepping);
                    let word_into_label = matches!(intent, TextCaretIntent::WordRight)
                        && crossed.is_some_and(|span| span.fold().is_none());
                    let focus = match crossed {
                        Some(span)
                            if view.value_of(moved.focus) == previous_focus || word_into_label =>
                        {
                            view.value_of_forward(span, || {
                                crate::text_editing::caret_focus(
                                    &state.value,
                                    TextSelection::caret(previous_focus),
                                    intent,
                                )
                                .unwrap_or(previous_focus)
                            })
                        }
                        _ => view.value_of(moved.focus),
                    };
                    // A caret stays a caret: its anchor goes where its focus
                    // went, not back across the fold it just crossed (that
                    // left an invisible selection over the hidden lines).
                    let anchor = if moved.anchor == moved.focus {
                        focus
                    } else {
                        view.value_of(moved.anchor)
                    };
                    TextSelection {
                        anchor,
                        focus,
                        affinity: moved.affinity,
                    }
                }
                None => moved,
            }
        };
        let vertical = matches!(
            intent,
            TextCaretIntent::Up
                | TextCaretIntent::Down
                | TextCaretIntent::PageUp
                | TextCaretIntent::PageDown
        );
        if state.has_additional_selections() {
            self.text_edit.caret_goal_x = None;
            let page_height = self.world.text_input_page_extent(focused.node);
            let geometric = (vertical && focused.multiline)
                || (horizontal_rightwards(intent).is_some()
                    && editor_draws_its_value(&self.world, focused.node, probe_value));
            let shape_context = if geometric && shaper.is_some() {
                self.world.text_input_shape_context(focused.node)
            } else {
                None
            };
            let mut geometry =
                shape_context
                    .zip(shaper.as_deref_mut())
                    .map(|((style, constraints), shaper)| EditorGeometry {
                        shaper,
                        node: focused.node,
                        text: TextContent {
                            value: probe_text.clone(),
                        },
                        style,
                        constraints,
                    });
            let selections = state.selections().into_owned();
            let primary_index = selections
                .iter()
                .position(|selection| *selection == state.selection)
                .unwrap_or(0);
            let mut next_selections = Vec::with_capacity(selections.len());
            let mut next_goal = None;
            let mut moved_any = false;
            for (index, &selection) in selections.iter().enumerate() {
                match Self::resolve_caret_move(
                    probe_value,
                    to_display(selection),
                    intent,
                    extend,
                    focused.multiline,
                    geometry.as_mut(),
                    page_height,
                ) {
                    Some((moved, goal)) => {
                        moved_any = true;
                        if index == primary_index {
                            next_goal = goal;
                        }
                        let mapped = if vertical {
                            to_value(moved)
                        } else {
                            to_value_moved(selection.focus, moved)
                        };
                        next_selections.push(mapped);
                    }
                    None => next_selections.push(selection),
                }
            }
            if !moved_any {
                return Ok(false);
            }
            if let Some(goal) = next_goal {
                self.text_edit.caret_goal_x = Some((focused.node, goal));
            }
            let atoms = atoms_in(
                &state.value,
                &self.editor_text_atoms(focused.node, focused.kind),
            );
            if !atoms.is_empty() {
                for (index, next) in next_selections.iter_mut().enumerate() {
                    let previous = if collapses_selection(selections[index], intent, extend) {
                        *next
                    } else {
                        selections[index]
                    };
                    *next = snap_selection_over_atoms(previous, *next, extend, &atoms);
                }
            }
            let primary = next_selections[primary_index];
            let additional = next_selections
                .into_iter()
                .enumerate()
                .filter(|(index, _)| *index != primary_index)
                .map(|(_, selection)| selection)
                .collect();
            return self.write_editor_selections(focused.node, primary, additional);
        }
        let selection = to_display(state.selection);
        let selection = if vertical && focused.multiline {
            let goal = self
                .text_edit
                .caret_goal_x
                .take()
                .filter(|(id, _)| *id == focused.node)
                .map(|(_, x)| x);
            if let (Some(shaper), Some((style, constraints))) =
                (shaper, self.world.text_input_shape_context(focused.node))
            {
                let mut geometry = EditorGeometry {
                    shaper,
                    node: focused.node,
                    text: TextContent {
                        value: probe_text.clone(),
                    },
                    style,
                    constraints,
                };
                let page_height = self.world.text_input_page_extent(focused.node);
                let by_point = geometry.vertical_by_point(
                    probe_value,
                    selection,
                    intent,
                    extend,
                    goal,
                    page_height,
                );
                let moved = if by_point.is_some() {
                    by_point
                } else if matches!(intent, TextCaretIntent::Up | TextCaretIntent::Down) {
                    vertical_caret_focus(
                        probe_value,
                        selection,
                        intent,
                        extend,
                        goal,
                        geometry.probe(),
                    )
                } else {
                    // One viewport height of visual lines: the content box
                    // is the editor's viewport.
                    let page_height = self.world.text_input_page_extent(focused.node);
                    page_caret_focus(
                        probe_value,
                        selection,
                        intent,
                        extend,
                        goal,
                        page_height,
                        geometry.probe(),
                    )
                };
                match moved {
                    Some((selection, goal)) => {
                        self.text_edit.caret_goal_x = Some((focused.node, goal));
                        selection
                    }
                    None => return Ok(false),
                }
            } else {
                let moved = if matches!(intent, TextCaretIntent::Up | TextCaretIntent::Down) {
                    vertical_caret_focus_logical(probe_value, selection, intent, extend)
                } else {
                    page_caret_focus_logical(probe_value, selection, intent, extend)
                };
                match moved {
                    Some(selection) => selection,
                    None => return Ok(false),
                }
            }
        } else if vertical {
            // Single-line fields map Up/Down and PageUp/PageDown onto the
            // line boundaries.
            let mapped = match intent {
                TextCaretIntent::Up | TextCaretIntent::PageUp => TextCaretIntent::LineStart,
                _ => TextCaretIntent::LineEnd,
            };
            match caret_focus(probe_value, selection, mapped) {
                Some(focus) => moved_selection(selection, focus, extend),
                None => return Ok(false),
            }
        } else {
            self.text_edit.caret_goal_x = None;
            let draws_value = editor_draws_its_value(&self.world, focused.node, probe_value);
            // Left/Right over a non-empty selection collapse it onto its
            // edge on that side of the screen — the one `EditSession` picks —
            // rather than stepping from the focus. Logical edge without
            // geometry (a masked or empty field draws something else).
            let collapse = horizontal_rightwards(intent)
                .filter(|_| !extend && selection.anchor != selection.focus);
            let visual = if let Some(rightwards) = collapse {
                let geometric = draws_value
                    .then_some(())
                    .zip(shaper)
                    .and_then(|((), shaper)| {
                        let (style, constraints) =
                            self.world.text_input_shape_context(focused.node)?;
                        Some(EditorGeometry {
                            shaper,
                            node: focused.node,
                            text: TextContent {
                                value: probe_text.clone(),
                            },
                            style,
                            constraints,
                        })
                    });
                let edge = match geometric {
                    Some(mut geometry) => geometry.collapse_edge(selection, rightwards),
                    None => nana_text::editable::collapse_edge(
                        selection.ordered(),
                        rightwards,
                        None,
                        None,
                        false,
                    ),
                };
                Some(caret_at_edge(selection, edge))
            } else {
                // Left/Right follow visual order wherever the backend's
                // layout can say what "left" is; without geometry they are
                // grapheme steps in logical order, which is all "left" could
                // mean then.
                horizontal_rightwards(intent)
                    .filter(|_| draws_value)
                    .zip(shaper)
                    .and_then(|(rightwards, shaper)| {
                        let (style, constraints) =
                            self.world.text_input_shape_context(focused.node)?;
                        let mut geometry = EditorGeometry {
                            shaper,
                            node: focused.node,
                            text: TextContent {
                                value: probe_text.clone(),
                            },
                            style,
                            constraints,
                        };
                        geometry.visual_step(selection, rightwards)
                    })
            };
            match visual {
                Some(stepped) if extend => stepped,
                Some(stepped) => {
                    TextSelection::caret(stepped.focus).with_affinity(stepped.affinity)
                }
                None => match caret_focus(probe_value, selection, intent) {
                    Some(focus) => moved_selection(selection, focus, extend),
                    None => return Ok(false),
                },
            }
        };
        let moved = if vertical {
            to_value(selection)
        } else {
            to_value_moved(state.selection.focus, selection)
        };
        let atoms = atoms_in(
            &state.value,
            &self.editor_text_atoms(focused.node, focused.kind),
        );
        let previous = if collapses_selection(state.selection, intent, extend) {
            moved
        } else {
            state.selection
        };
        let moved = snap_selection_over_atoms(previous, moved, extend, &atoms);
        self.write_editor_selection(focused.node, moved)
    }

    /// Delete around the caret(s) of the focused text editor. With multiple
    /// cursors every cursor deletes by the same kind.
    pub fn delete_focused_text(
        &mut self,
        document: DocumentId,
        kind: TextDeleteKind,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        let atoms = self.editor_text_atoms(focused.node, focused.kind);
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Delete,
            |value, selection, _| {
                let atoms = atoms_in(value, &atoms);
                let replacement = match kind {
                    TextDeleteKind::Backward => delete_backward_atoms(value, selection, &atoms)?,
                    TextDeleteKind::Forward => delete_forward_atoms(value, selection, &atoms)?,
                    TextDeleteKind::WordBackward => {
                        delete_word_backward_atoms(value, selection, &atoms)?
                    }
                    TextDeleteKind::WordForward => {
                        delete_word_forward_atoms(value, selection, &atoms)?
                    }
                    TextDeleteKind::LineStart => {
                        delete_to_line_start_atoms(value, selection, &atoms)?
                    }
                    TextDeleteKind::LineEnd => delete_to_line_end_atoms(value, selection, &atoms)?,
                };
                Some(CursorEdit::Span(replacement))
            },
        )
    }

    /// Insert a newline into the focused `TextArea`, with auto-indent when
    /// code editing is enabled. Single-line fields never accept newlines.
    pub fn insert_focused_text_newline(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        let indent_unit = focused
            .code_editing
            .as_ref()
            .map(|code| code.indent_unit.to_string());
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Typing,
            |value, selection, _| {
                let replacement = match &indent_unit {
                    Some(unit) => auto_indent_newline(value, selection, unit),
                    None => {
                        let range = selection.ordered();
                        let caret = range.start + 1;
                        TextReplacement {
                            range,
                            insert: "\n".into(),
                            caret,
                        }
                    }
                };
                Some(CursorEdit::Span(replacement))
            },
        )
    }

    /// Apply code-editor auto-pairing for a typed character. `Ok(false)`
    /// means the character was not consumed and should type normally.
    pub fn code_edit_typed(
        &mut self,
        document: DocumentId,
        typed: char,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if focused.code_editing.is_none() || !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Typing,
            |value, selection, _| auto_pair_edit(value, selection, typed).map(CursorEdit::Span),
        )
    }

    /// Indent (`Tab`) or outdent (`Shift+Tab`) the focused code editor.
    pub fn code_edit_indent(
        &mut self,
        document: DocumentId,
        outdent: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        let Some(code) = focused.code_editing else {
            return Ok(false);
        };
        if !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        let unit = code.indent_unit.to_string();
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Structural,
            |value, selection, _| {
                let (next, selection) = if outdent {
                    outdent_selection(value, selection, &unit)?
                } else {
                    indent_selection(value, selection, &unit)?
                };
                Some(CursorEdit::Transform { next, selection })
            },
        )
    }

    /// Toggle line comments across the selections of the focused code editor.
    pub fn code_edit_toggle_comment(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        let Some(code) = focused.code_editing else {
            return Ok(false);
        };
        if !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        let prefix = code.comment_prefix.to_string();
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Structural,
            |value, selection, _| {
                let (next, selection) = toggle_line_comment(value, selection, &prefix)?;
                Some(CursorEdit::Transform { next, selection })
            },
        )
    }

    /// Move the block of lines each selection touches up or down one line,
    /// with each selection following its moved text. `Ok(false)` means there
    /// is no focused multiline editor or no block can move.
    pub fn move_focused_text_lines(
        &mut self,
        document: DocumentId,
        direction: TextLineDirection,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Structural,
            |value, selection, _| {
                let (next, selection) = move_lines(value, selection, direction)?;
                Some(CursorEdit::Transform { next, selection })
            },
        )
    }

    /// Duplicate the block of lines each selection touches on the line below
    /// and select each copy.
    pub fn duplicate_focused_text_lines(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Structural,
            |value, selection, _| {
                let (next, selection) = duplicate_lines(value, selection)?;
                Some(CursorEdit::Transform { next, selection })
            },
        )
    }

    /// Delete the block of lines each selection touches, including one
    /// adjacent newline.
    pub fn delete_focused_text_lines(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Delete,
            |value, selection, _| {
                let (next, selection) = delete_lines(value, selection)?;
                Some(CursorEdit::Transform { next, selection })
            },
        )
    }

    /// Join the lines each selection touches into one line (single-space
    /// seams, following lines' indentation removed).
    pub fn join_focused_text_lines(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Structural,
            |value, selection, _| {
                let (next, selection) = join_lines(value, selection)?;
                Some(CursorEdit::Transform { next, selection })
            },
        )
    }

    /// Uppercase (`upper`) or lowercase every selection of the focused editor.
    pub fn transform_focused_text_case(
        &mut self,
        document: DocumentId,
        upper: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Structural,
            |value, selection, _| {
                let (next, selection) = transform_selection_case(value, selection, upper)?;
                Some(CursorEdit::Transform { next, selection })
            },
        )
    }

    /// Sort the lines each selection touches by byte order, optionally
    /// descending and dropping repeated rows. Sorting is a host command-panel
    /// action; no default key binding routes here.
    pub fn sort_focused_text_lines(
        &mut self,
        document: DocumentId,
        descending: bool,
        unique: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Structural,
            |value, selection, _| {
                let (next, selection) = sort_lines(value, selection, descending, unique)?;
                Some(CursorEdit::Transform { next, selection })
            },
        )
    }

    /// Move the caret of the focused editor onto the bracket matching the
    /// one adjacent to the caret (`()[]{}`, nesting-aware).
    /// Selection-only move: no change event is emitted. `Ok(false)` means
    /// there is no focused editor or no adjacent bracket pair.
    pub fn goto_focused_text_matching_bracket(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.accepts_selection {
            return Ok(false);
        }
        let state = self.editor_state(focused.node)?;
        let Some((open, close)) = matching_bracket_pair(&state.value, state.selection.focus) else {
            return Ok(false);
        };
        let target = if state.selection.focus <= open {
            close
        } else {
            open
        };
        self.text_edit.caret_goal_x = None;
        self.write_editor_selection(
            focused.node,
            moved_selection(state.selection, target, false),
        )
    }

    /// Collapse every selection onto the primary cursor (the exit from
    /// multi-cursor mode; hosts decide when to invoke it — Escape is
    /// deliberately not bound). Selection-only move: no change event.
    /// `Ok(false)` means there is no focused editor or only one cursor.
    pub fn collapse_focused_text_selections(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        let state = self.editor_state(focused.node)?;
        if !state.has_additional_selections() {
            return Ok(false);
        }
        self.write_editor_selections(focused.node, state.selection, Vec::new())
    }

    /// 把聚焦文本编辑器的选区设为唯一选区 `start..end`（`start == end`
    /// 为光标；附加光标清除）。偏移先钳到最近的字符边界。区间任一端点
    /// 严格落在折叠隐藏区间内部时该折叠自动展开（与光标导航同源的
    /// reveal 语义，复用框架 display 判定：嵌套折叠只展开当前真正遮住
    /// 端点的父级）。纯选区变更：不发 change 事件（同 move 命令）。
    /// `Ok(false)` 表示没有聚焦编辑器，或选区本就一致。
    pub fn select_focused_text_range(
        &mut self,
        document: DocumentId,
        start: usize,
        end: usize,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        let state = self.editor_state(focused.node)?;
        let start = clamp_focus(&state.value, start);
        let end = clamp_focus(&state.value, end);
        self.text_edit.caret_goal_x = None;
        // 区间两端都参与 reveal（光标导航只看 focus；宿主跳转的起点同样
        // 必须可见）。write_editor_selections 内部还会按 focus 再对账一次，
        // 幂等无害。
        self.unfold_text_folds_containing(focused.node, &[start, end])?;
        self.write_editor_selections(focused.node, TextSelection::new(start, end), Vec::new())
    }

    /// Add one cursor on the visual line above (`above`) or below every
    /// active selection, keeping each cursor's column. Multiline editors
    /// only; hosts without a shaper fall back to logical-line columns.
    /// Targets that already hold a cursor, and document-edge clamps that
    /// would land on the same line, are skipped. Selection-only move.
    pub fn add_focused_text_cursor(
        &mut self,
        document: DocumentId,
        above: bool,
        shaper: Option<&mut dyn crate::TextShaper>,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_selection {
            return Ok(false);
        }
        let state = self.editor_state(focused.node)?;
        let intent = if above {
            TextCaretIntent::Up
        } else {
            TextCaretIntent::Down
        };
        let value = state.value.clone();
        let selections = state.selections().into_owned();
        let mut candidates: Vec<TextSelection> = Vec::with_capacity(selections.len());
        match (shaper, self.world.text_input_shape_context(focused.node)) {
            (Some(shaper), Some((style, constraints))) => {
                let mut geometry = EditorGeometry {
                    shaper,
                    node: focused.node,
                    text: TextContent {
                        value: self.editor_probe_text(focused.node, None),
                    },
                    style,
                    constraints,
                };
                let mut probe = geometry.probe();
                for &selection in &selections {
                    let Some((moved, _)) =
                        vertical_caret_focus(&value, selection, intent, false, None, &mut probe)
                    else {
                        continue;
                    };
                    // Same visual line means there is no line in that
                    // direction; skip instead of stacking on the source.
                    let (_, source_y, _) = probe(clamp_focus(&value, selection.focus));
                    let (_, moved_y, _) = probe(clamp_focus(&value, moved.focus));
                    let distinct = if above {
                        moved_y + f32::EPSILON < source_y
                    } else {
                        moved_y > source_y + f32::EPSILON
                    };
                    if distinct {
                        candidates.push(moved);
                    }
                }
            }
            _ => {
                for &selection in &selections {
                    let Some(moved) =
                        vertical_caret_focus_logical(&value, selection, intent, false)
                    else {
                        continue;
                    };
                    let source_line = logical_line_range(&value, selection.focus).0;
                    let moved_line = logical_line_range(&value, moved.focus).0;
                    let distinct = if above {
                        moved_line < source_line
                    } else {
                        moved_line > source_line
                    };
                    if distinct {
                        candidates.push(moved);
                    }
                }
            }
        }
        if candidates.is_empty() {
            return Ok(false);
        }
        let mut next = state;
        if !next.add_selections(&candidates) {
            return Ok(false);
        }
        self.write_editor_selections(focused.node, next.selection, next.additional_selections)
    }

    /// Select the next (`previous = false`) or previous literal occurrence of
    /// the primary selection's text — or of the word at the primary caret
    /// when it is bare — skipping spans that already hold a cursor and
    /// wrapping around the document. Multiline editors only.
    /// Selection-only move: no change event.
    pub fn select_focused_text_occurrence(
        &mut self,
        document: DocumentId,
        previous: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_selection {
            return Ok(false);
        }
        let state = self.editor_state(focused.node)?;
        let Some(found) =
            next_occurrence(&state.value, &state.selections(), state.selection, previous)
        else {
            return Ok(false);
        };
        let mut next = state;
        if !next.add_selections(&[TextSelection::new(found.start, found.end)]) {
            return Ok(false);
        }
        self.write_editor_selections(focused.node, next.selection, next.additional_selections)
    }

    /// Select every literal occurrence of the primary selection's text (or of
    /// the word at the bare primary caret) that does not already hold a
    /// cursor. Multiline editors only. Selection-only move: no change event.
    pub fn select_all_focused_text_occurrences(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_selection {
            return Ok(false);
        }
        let state = self.editor_state(focused.node)?;
        let selections = state.selections().into_owned();
        let Some((query_start, query_end)) = occurrence_query(&state.value, state.selection) else {
            return Ok(false);
        };
        let query = &state.value[query_start..query_end];
        let matches = find_matches(
            &state.value,
            query,
            TextSearchOptions {
                whole_word: true,
                ..TextSearchOptions::default()
            },
        );
        let candidates: Vec<TextSelection> = matches
            .iter()
            .filter(|found| !is_covered(found, &selections))
            .map(|found| TextSelection::new(found.start, found.end))
            .collect();
        if candidates.is_empty() {
            return Ok(false);
        }
        let mut next = state;
        if !next.add_selections(&candidates) {
            return Ok(false);
        }
        self.write_editor_selections(focused.node, next.selection, next.additional_selections)
    }

    /// Expand every selection of the focused editor one level — collapsed
    /// caret to its word, then to the interior of the innermost enclosing
    /// bracket pair, outward layer by layer, finally the whole document (see
    /// [`expanded_selection`]). Applied per cursor; overlapping results fuse
    /// through the normal selection write. Selection-only move. The prior
    /// selection set is remembered for
    /// [`AppContext::shrink_focused_text_selection`] and invalidated by any
    /// value edit. `Ok(false)` means no focused editor or nothing grew.
    pub fn expand_focused_text_selection(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.accepts_selection {
            return Ok(false);
        }
        let state = self.editor_state(focused.node)?;
        let selections = state.selections().into_owned();
        let primary_index = selections
            .iter()
            .position(|selection| *selection == state.selection)
            .unwrap_or(0);
        let mut next_selections = Vec::with_capacity(selections.len());
        let mut grew = false;
        for &selection in &selections {
            let next = expanded_selection(&state.value, selection).unwrap_or(selection);
            grew |= next != selection;
            next_selections.push(next);
        }
        if !grew {
            return Ok(false);
        }
        let entry = (state.selection, state.additional_selections.clone());
        match &mut self.text_edit.selection_expansions {
            Some((node, history, value)) if *node == focused.node && *value == state.value => {
                history.push(entry);
            }
            _ => {
                self.text_edit.selection_expansions =
                    Some((focused.node, vec![entry], state.value.to_string()));
            }
        }
        self.text_edit.caret_goal_x = None;
        let primary = next_selections[primary_index];
        let additional = next_selections
            .into_iter()
            .enumerate()
            .filter(|(index, _)| *index != primary_index)
            .map(|(_, selection)| selection)
            .collect();
        self.write_editor_selections(focused.node, primary, additional)
    }

    /// Undo one [`AppContext::expand_focused_text_selection`] step: restore
    /// the selection set from before the last expansion. The expansion record
    /// dies when the editor's value changes (any edit), when a different
    /// editor is focused, or once every recorded step is consumed.
    /// Selection-only move. `Ok(false)` means there is nothing to shrink to.
    pub fn shrink_focused_text_selection(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        let state = self.editor_state(focused.node)?;
        match &mut self.text_edit.selection_expansions {
            Some((node, history, value))
                if *node == focused.node && **value == state.value && !history.is_empty() =>
            {
                let (primary, additional) = history.pop().expect("non-empty checked above");
                self.text_edit.caret_goal_x = None;
                self.write_editor_selections(focused.node, primary, additional)
            }
            _ => {
                self.text_edit.selection_expansions = None;
                Ok(false)
            }
        }
    }

    /// Select the next literal match of `query` in the focused text editor,
    /// searching from the selection's end and wrapping to the first in-scope
    /// match. With [`TextFindScope::Selection`] the navigation loop stays
    /// inside the primary selection (an empty selection searches the whole
    /// document). Wrapping never leaves the scope: the match list only
    /// contains in-scope matches.
    ///
    /// Selection-only move: like [`AppContext::move_focused_text_caret`], no
    /// change event is emitted. `Ok(false)` means there is no focused editor
    /// or no match.
    pub fn find_next_focused_text_match(
        &mut self,
        document: DocumentId,
        query: &str,
        options: TextSearchOptions,
        scope: TextFindScope,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.accepts_selection {
            return Ok(false);
        }
        let state = self.editor_state(focused.node)?;
        let matches = find_matches_in_scope(&state.value, query, options, scope, &state.selection);
        let Some(found) = find_next_match(&matches, state.selection.ordered().end) else {
            return Ok(false);
        };
        self.text_edit.caret_goal_x = None;
        self.write_editor_selection(focused.node, TextSelection::new(found.start, found.end))
    }

    /// Select the previous literal match of `query` in the focused text
    /// editor, searching from the selection's start and wrapping to the last
    /// in-scope match; see [`AppContext::find_next_focused_text_match`] for
    /// scoping. Selection-only move.
    pub fn find_previous_focused_text_match(
        &mut self,
        document: DocumentId,
        query: &str,
        options: TextSearchOptions,
        scope: TextFindScope,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.accepts_selection {
            return Ok(false);
        }
        let state = self.editor_state(focused.node)?;
        let matches = find_matches_in_scope(&state.value, query, options, scope, &state.selection);
        let Some(found) = find_previous_match(&matches, state.selection.ordered().start) else {
            return Ok(false);
        };
        self.text_edit.caret_goal_x = None;
        self.write_editor_selection(focused.node, TextSelection::new(found.start, found.end))
    }

    /// Replace the focused editor's selection with `replacement` when the
    /// selection is exactly a match of `query` under `options`, then select
    /// the inserted text (so a following replace targets the same span
    /// semantics) and emit the change event. Advancing to the next match is
    /// the host's call — compose with
    /// [`AppContext::find_next_focused_text_match`]. With `preserve_case`
    /// the replacement text follows the matched text's case shape (see
    /// [`preserve_case_replacement`]); `Ok(false)` means there is no focused
    /// editor or the selection is not a match.
    ///
    /// Primary-cursor semantics: only the primary selection replaces; other
    /// cursors survive through offset remapping.
    pub fn replace_focused_text_match(
        &mut self,
        document: DocumentId,
        query: &str,
        options: TextSearchOptions,
        replacement: &str,
        preserve_case: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.accepts_input {
            return Ok(false);
        }
        self.text_edit.caret_goal_x = None;
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Structural,
            |value, selection, is_primary| {
                if !is_primary {
                    return None;
                }
                let range = selection.ordered();
                let is_match = find_matches(value, query, options).contains(&range);
                if !is_match {
                    return None;
                }
                let text = if preserve_case {
                    preserve_case_replacement(replacement, &value[range.clone()])
                } else {
                    replacement.to_owned()
                };
                let mut next =
                    String::with_capacity(value.len() - range.len().min(value.len()) + text.len());
                next.push_str(&value[..range.start]);
                next.push_str(&text);
                next.push_str(&value[range.end..]);
                Some(CursorEdit::Transform {
                    next,
                    selection: TextSelection::new(range.start, range.start + text.len()),
                })
            },
        )
    }

    /// Replace every literal match of `query` in the focused editor with
    /// `replacement` (left-to-right, non-overlapping on the original text),
    /// select the first replacement, emit one change event, and return the
    /// replacement count. [`TextFindScope::Selection`] restricts matching to
    /// the primary selection (an empty selection replaces document-wide);
    /// `preserve_case` follows [`AppContext::replace_focused_text_match`].
    /// `Ok(0)` leaves the editor untouched.
    pub fn replace_all_focused_text_matches(
        &mut self,
        document: DocumentId,
        query: &str,
        options: TextSearchOptions,
        replacement: &str,
        scope: TextFindScope,
        preserve_case: bool,
    ) -> Result<usize, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(0);
        };
        if !focused.accepts_input {
            return Ok(0);
        }
        self.text_edit.caret_goal_x = None;
        let mut replaced = 0usize;
        let applied =
            self.edit_editor(focused.node, crate::TextEditOrigin::Structural, |state| {
                let matches =
                    find_matches_in_scope(&state.value, query, options, scope, &state.selection);
                if matches.is_empty() {
                    return None;
                }
                let first = matches[0].clone();
                let first_replacement = if preserve_case {
                    preserve_case_replacement(replacement, &state.value[first.clone()])
                } else {
                    replacement.to_owned()
                };
                let (value, count) = replace_all_matches_in_range(
                    &state.value,
                    query,
                    replacement,
                    options,
                    match scope {
                        TextFindScope::Document => None,
                        TextFindScope::Selection => find_scope_range(&state.selection),
                    },
                    preserve_case,
                );
                replaced = count;
                Some(EditorEdit {
                    value,
                    selection: TextSelection::new(
                        first.start,
                        first.start + first_replacement.len(),
                    ),
                })
            })?;
        Ok(if applied { replaced } else { 0 })
    }

    /// Place or extend the selection of the focused editor from a pointer
    /// press. Single presses set the caret, Shift extends, double clicks
    /// select the word, triple clicks select the line. An Alt+click
    /// (`add_cursor`) on a multiline editor adds or removes a cursor instead:
    /// clicking an existing additional cursor removes it, any other spot gets
    /// one. Plain presses collapse back to the single primary selection.
    #[allow(clippy::too_many_arguments)]
    pub fn text_editor_pointer_press(
        &mut self,
        document: DocumentId,
        node: StableNodeId,
        pointer_id: u64,
        x: f32,
        y: f32,
        extend: bool,
        add_cursor: bool,
        now: std::time::Duration,
        shaper: &mut dyn crate::TextShaper,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if focused.node != node || !focused.accepts_selection {
            self.text_edit.text_pointer_drag = None;
            return Ok(false);
        }
        // A press on a numeric field's spinner steps the value
        // ([`Self::press_number_stepper`]). The editor still owns the press,
        // so nothing else starts a selection there, but it places no caret.
        if focused.is_numeric() && self.on_number_stepper(node, x, y) {
            self.text_edit.text_pointer_drag = None;
            // A spinner press between two text presses breaks their click
            // run: they are two single clicks, not a double click.
            self.text_edit.text_pointer_click = None;
            return Ok(true);
        }
        let state = self.editor_state(node)?;
        const DOUBLE_CLICK_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);
        const DOUBLE_CLICK_SLOP: f32 = 4.0;
        let count = match &self.text_edit.text_pointer_click {
            Some(click)
                if click.pointer_id == pointer_id
                    && click.node == node
                    && now.saturating_sub(click.at) <= DOUBLE_CLICK_WINDOW
                    && (click.x - x).abs() < DOUBLE_CLICK_SLOP
                    && (click.y - y).abs() < DOUBLE_CLICK_SLOP =>
            {
                click.count % 3 + 1
            }
            _ => 1,
        };
        self.text_edit.text_pointer_click = Some(super::TextPointerClick {
            pointer_id,
            node,
            at: now,
            x,
            y,
            count,
        });
        // 补全弹层交互优先于折叠：弹层绘制在全部编辑器覆盖层之上，
        // 命中候选行即接受该行（单击且无修饰时），不落光标。
        if count == 1
            && !extend
            && !add_cursor
            && let Some(row) = self.world.text_completion_hit(node, x, y)
        {
            self.text_edit.text_pointer_drag = None;
            return self.accept_focused_text_completion(document, Some(row));
        }
        // minimap 竖条交互：条内按下即消费（任何修饰键与连击计数）——
        // 视口滚动到点击行居中的位置并进入拖拽跟随，不移动光标、不产生
        // 选区；滚轮不经过本路径，落在条上仍按编辑器常规滚动。
        if let Some(target) = self.world.text_minimap_scroll_target(node, x, y) {
            self.text_edit.text_pointer_drag = None;
            return self.text_editor_minimap_navigate(node, focused.kind, pointer_id, target);
        }
        if count == 1
            && !extend
            && !add_cursor
            && let Some(token) = self.world.text_atom_close_hit(node, x, y)
        {
            self.text_edit.text_pointer_drag = None;
            return self.close_focused_text_atom(document, &token);
        }
        // 折叠交互优先：gutter 箭头与折叠摘要标记的点击切换折叠态，不落
        // 光标（单击且无修饰时）。
        if count == 1
            && !extend
            && !add_cursor
            && let Some(fold) = self.world.text_fold_hit(node, x, y)
        {
            self.text_edit.text_pointer_drag = None;
            let collapsed = self
                .world
                .text_fold_view_state(node)
                .map(|state| state.collapsed.contains(&fold))
                .unwrap_or(false);
            return self.set_node_text_fold(node, fold, !collapsed);
        }
        // 命中换算与拖选共用同一条 offset 路径（折叠视图按显示空间命中
        // 后映射回值空间）。
        let Some(hit) = self.text_editor_hit_offset(node, focused.kind, x, y, shaper)? else {
            return Ok(false);
        };
        let offset = hit.offset;
        // 拖拽移动选中文本：普通按下落在主选区内部（多行、单选区、非
        // IME 组合期）时不落光标，进入拖拽状态机；Alt 按住为复制（macOS
        // 惯例；取舍：选区内的 Alt+click 由多光标添加让位给拖拽复制）。
        if self
            .text_edit
            .text_selection_drag
            .is_some_and(|existing| existing.pointer_id != pointer_id || existing.node != node)
        {
            self.clear_text_selection_drag();
        }
        if count == 1
            && !extend
            && focused.multiline
            && focused.accepts_input
            && state.additional_selections.is_empty()
            && self.world.ime(node).is_none()
            && self.begin_text_selection_drag(
                node,
                focused.kind,
                pointer_id,
                x,
                y,
                offset,
                add_cursor,
                &state,
            )
        {
            self.text_edit.text_pointer_drag = None;
            return Ok(true);
        }
        // Alt+click toggles an extra cursor on multiline editors; single-line
        // fields keep their plain click semantics.
        if add_cursor && focused.multiline && count == 1 {
            return self.text_editor_toggle_cursor(node, &state, hit);
        }
        let atoms = atoms_in(&state.value, &self.editor_text_atoms(node, focused.kind));
        let (selection, additional) = match count {
            2 => {
                let (start, end) = word_range_at(&state.value, offset);
                let range = expand_range_over_atoms(start..end, &atoms);
                (TextSelection::new(range.start, range.end), Vec::new())
            }
            3 => {
                let (start, end) = logical_line_range(&state.value, offset);
                let range = expand_range_over_atoms(start..end, &atoms);
                (TextSelection::new(range.start, range.end), Vec::new())
            }
            // 单击落点带着命中的 affinity：软换行行尾点在左侧就留在这一行。
            _ if extend => (
                moved_selection(state.selection, offset, true).with_affinity(hit.affinity),
                state.additional_selections.clone(),
            ),
            // A plain click naturally collapses to one primary cursor.
            _ => (
                moved_selection(state.selection, offset, false).with_affinity(hit.affinity),
                Vec::new(),
            ),
        };
        if count != 1 {
            self.text_edit.text_pointer_drag = None;
        } else {
            let anchor = if extend { selection.anchor } else { offset };
            self.text_edit.text_pointer_drag = Some((pointer_id, node, anchor));
        }
        self.write_editor_selections(node, selection, additional)
    }

    /// minimap 导航：把目标滚动写进世界与组件（组件字段是投影权威，
    /// 不同步会在下一帧被投影改写回去），并钉住视口使光标 reveal 让位。
    fn text_editor_minimap_navigate(
        &mut self,
        node: StableNodeId,
        kind: TextEditorKind,
        pointer_id: u64,
        target: ScrollOffset,
    ) -> Result<bool, FrameworkError> {
        let mut mutations = MutationQueue::new();
        mutations.set_scroll_offset(node, target);
        self.world.commit(mutations)?;
        // 钉住提交后的实际偏移（世界侧可能再钳制），保证与几何层读取的
        // 请求滚动逐位相等。
        let applied = self.world.scroll_request(node).unwrap_or(target);
        self.world.set_text_viewport_pin(node, Some(applied));
        if kind == TextEditorKind::Area {
            self.update_component(Entity::<TextArea>::from_stable_id(node), |area, _| {
                area.scroll_offset = applied;
                false
            })?;
        }
        self.text_edit.text_minimap_drag = Some((pointer_id, node, kind));
        Ok(true)
    }

    /// Toggle an Alt+click cursor at `offset`: clicking an existing additional
    /// cursor removes it, clicking anywhere else adds one (the primary
    /// selection is never removed this way).
    fn text_editor_toggle_cursor(
        &mut self,
        node: StableNodeId,
        state: &TextInputState,
        hit: crate::TextHit,
    ) -> Result<bool, FrameworkError> {
        let offset = hit.offset;
        let mut next = state.clone();
        let before = next.additional_selections.len();
        next.additional_selections.retain(|selection| {
            let range = selection.ordered();
            !(range.start <= offset && offset <= range.end)
        });
        if next.additional_selections.len() == before {
            let primary = state.selection.ordered();
            let on_primary = primary.start <= offset && offset <= primary.end;
            if !on_primary {
                next.additional_selections
                    .push(TextSelection::caret(offset).with_affinity(hit.affinity));
            }
        }
        next.normalize_selections();
        self.write_editor_selections(node, next.selection, next.additional_selections)
    }

    /// Extend a live drag selection to the pointer position.
    pub fn text_editor_pointer_drag(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
        shaper: &mut dyn crate::TextShaper,
    ) -> Result<bool, FrameworkError> {
        // minimap 拖拽优先：按下进入的导航拖拽连续跟随指针换算视口，
        // 不做选区延伸。指针拖出条外时保持最后视口（仍消费本次拖拽）。
        if let Some((drag_id, drag_node, drag_kind)) = self.text_edit.text_minimap_drag {
            if drag_id != pointer_id {
                return Ok(false);
            }
            return match self.world.text_minimap_scroll_target(drag_node, x, y) {
                Some(target) => {
                    self.text_editor_minimap_navigate(drag_node, drag_kind, pointer_id, target)
                }
                None => Ok(true),
            };
        }
        // 拖拽移动选中文本优先：pending/active 状态机消费本次移动，不做
        // 选区延伸（拖拽期间既有点击选词等竞争语义全部让位）。
        if let Some(drag) = self.text_edit.text_selection_drag {
            if drag.pointer_id != pointer_id {
                return Ok(false);
            }
            return self.update_text_selection_drag(document, drag, x, y, shaper);
        }
        let Some((drag_id, node, anchor)) = self.text_edit.text_pointer_drag else {
            return Ok(false);
        };
        if drag_id != pointer_id {
            return Ok(false);
        }
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if focused.node != node {
            return Ok(false);
        }
        let state = self.editor_state(node)?;
        // 命中换算与点击共用同一条 offset 路径。
        let Some(hit) = self.text_editor_hit_offset(node, focused.kind, x, y, shaper)? else {
            return Ok(false);
        };
        if hit.offset == state.selection.focus && hit.affinity == state.selection.affinity {
            return Ok(false);
        }
        self.write_editor_selection(
            node,
            TextSelection::new(anchor, hit.offset).with_affinity(hit.affinity),
        )
    }

    /// End any drag selection started by this pointer.
    pub fn text_editor_pointer_release(&mut self, pointer_id: u64) {
        if self
            .text_edit
            .text_pointer_drag
            .is_some_and(|(drag_id, _, _)| drag_id == pointer_id)
        {
            self.text_edit.text_pointer_drag = None;
        }
        if self
            .text_edit
            .text_minimap_drag
            .is_some_and(|(drag_id, _, _)| drag_id == pointer_id)
        {
            self.text_edit.text_minimap_drag = None;
        }
        // 落点未经理由 `text_editor_selection_drop` 消费（指针路径未带
        // 文档/坐标等场景）时按取消处理，一并清除落点指示线。
        if self
            .text_edit
            .text_selection_drag
            .is_some_and(|drag| drag.pointer_id == pointer_id)
        {
            self.clear_text_selection_drag();
        }
    }

    /// 进入拖拽移动选中的待定态。要求：多行、单选区、非 IME 组合期，
    /// 按下点严格落在非空主选区内部。返回是否已接管本次按下。
    #[allow(clippy::too_many_arguments)]
    fn begin_text_selection_drag(
        &mut self,
        node: StableNodeId,
        kind: TextEditorKind,
        pointer_id: u64,
        x: f32,
        y: f32,
        offset: usize,
        copy: bool,
        state: &TextInputState,
    ) -> bool {
        let range = state.selection.ordered();
        if range.start == range.end || offset < range.start || offset >= range.end {
            return false;
        }
        if self.text_edit.text_selection_drag.is_some() {
            return false;
        }
        self.text_edit.text_selection_drag = Some(TextSelectionDrag {
            pointer_id,
            node,
            kind,
            press: (x, y),
            source: (range.start, range.end),
            active: false,
            copy,
            target: None,
        });
        true
    }

    /// 拖拽态移动：pending 态先做阈值判定；active 态换算落点偏移（按
    /// 折叠/软换行的既有 offset 命中换算），把落点指示线发布到世界侧。
    /// 落点在源选区内部视为无效落点（指示线消失，释放时取消）。
    fn update_text_selection_drag(
        &mut self,
        document: DocumentId,
        mut drag: TextSelectionDrag,
        x: f32,
        y: f32,
        shaper: &mut dyn crate::TextShaper,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            self.clear_text_selection_drag();
            return Ok(false);
        };
        if focused.node != drag.node {
            self.clear_text_selection_drag();
            return Ok(false);
        }
        if !drag.active {
            let (dx, dy) = (x - drag.press.0, y - drag.press.1);
            if dx * dx + dy * dy <= TEXT_SELECTION_DRAG_THRESHOLD * TEXT_SELECTION_DRAG_THRESHOLD {
                self.text_edit.text_selection_drag = Some(drag);
                return Ok(true);
            }
            drag.active = true;
        }
        let Some(target) = self
            .text_editor_hit_offset(drag.node, drag.kind, x, y, shaper)?
            .map(|hit| hit.offset)
        else {
            self.text_edit.text_selection_drag = Some(drag);
            return Ok(true);
        };
        // 源选区区间内（含边界，落点为退化 no-op）不显示指示线。
        drag.target = (!(drag.source.0..=drag.source.1).contains(&target)).then_some(target);
        if let (Some(target), Some((style, constraints))) =
            (drag.target, self.world.text_input_shape_context(drag.node))
        {
            // 折叠探测用显示值（无折叠时为编辑器原值）。
            let value =
                self.editor_probe_text(drag.node, self.world.text_display_view(drag.node).as_ref());
            let mut geometry = EditorGeometry {
                shaper,
                node: drag.node,
                text: TextContent { value },
                style,
                constraints,
            };
            let (indicator_x, indicator_y, indicator_height) = (geometry.probe())(target);
            self.world.set_text_drop_indicator(
                drag.node,
                Some(crate::LayoutBox {
                    x: indicator_x,
                    y: indicator_y,
                    width: 2.0,
                    height: indicator_height,
                }),
            );
        } else {
            self.world.set_text_drop_indicator(drag.node, None);
        }
        self.text_edit.text_selection_drag = Some(drag);
        Ok(true)
    }

    /// 指针释放的落点执行：pending 态回落为普通点击（caret 塌缩到落
    /// 点）；active 态执行移动/复制——删除+插入拼成一次值修订（单个
    /// change 事件，一步撤销），落点在源选区内部则取消（选区保持原
    /// 状）。返回是否消费本次释放。
    pub fn text_editor_selection_drop(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
        shaper: &mut dyn crate::TextShaper,
    ) -> Result<bool, FrameworkError> {
        let Some(drag) = self.text_edit.text_selection_drag else {
            return Ok(false);
        };
        if drag.pointer_id != pointer_id {
            return Ok(false);
        }
        self.clear_text_selection_drag();
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if focused.node != drag.node || !focused.accepts_input {
            return Ok(false);
        }
        if !drag.active {
            // 低于阈值：按原点击语义处理（落 caret）。
            let Some(hit) = self.text_editor_hit_offset(drag.node, drag.kind, x, y, shaper)? else {
                return Ok(false);
            };
            return self.write_editor_selections(
                drag.node,
                TextSelection::caret(hit.offset).with_affinity(hit.affinity),
                Vec::new(),
            );
        }
        let Some(target) = self
            .text_editor_hit_offset(drag.node, drag.kind, x, y, shaper)?
            .map(|hit| hit.offset)
        else {
            return Ok(true);
        };
        let state = self.editor_state(drag.node)?;
        if state.selection.ordered() != (drag.source.0..drag.source.1) {
            // 拖拽期间选区被外部改动：取消，不落文本。
            return Ok(true);
        }
        let (start, end) = drag.source;
        let length = end - start;
        // 源选区边界（含两端）都是退化落点：移动/复制均为 no-op，取消。
        if (start..=end).contains(&target) {
            return Ok(true);
        }
        let value = &state.value;
        let (next, insert_at) = if drag.copy {
            // 复制：目标点插入选中文本，原文本保留。
            (
                format!(
                    "{}{}{}",
                    &value[..target],
                    &value[start..end],
                    &value[target..]
                ),
                target,
            )
        } else if target > end {
            // 向后移动：删除源区间，剩余中段让位，文本插到目标前。
            (
                format!(
                    "{}{}{}{}",
                    &value[..start],
                    &value[end..target],
                    &value[start..end],
                    &value[target..]
                ),
                target - length,
            )
        } else {
            // 向前移动（target < start）。
            (
                format!(
                    "{}{}{}{}",
                    &value[..target],
                    &value[start..end],
                    &value[target..start],
                    &value[end..]
                ),
                target,
            )
        };
        let selection = TextSelection::new(insert_at, insert_at + length);
        self.commit_editor_value(
            drag.node,
            next.into(),
            selection,
            Vec::new(),
            crate::TextEditOrigin::Structural,
        )
    }

    /// 取消拖拽移动选中（Esc）。仅当拖拽属于该文档的聚焦编辑器时消费。
    pub fn cancel_focused_text_selection_drag(&mut self, document: DocumentId) -> bool {
        let Some(drag) = &self.text_edit.text_selection_drag else {
            return false;
        };
        let node = drag.node;
        let owns = self
            .focused_text_editor(document)
            .is_some_and(|focused| focused.node == node);
        self.clear_text_selection_drag();
        owns
    }

    fn clear_text_selection_drag(&mut self) {
        if let Some(drag) = self.text_edit.text_selection_drag.take() {
            self.world.set_text_drop_indicator(drag.node, None);
        }
    }

    /// 指针位置 → 值空间 caret（折叠视图按显示空间命中后映射回值空间；
    /// 与点击/拖选共用同一换算）。无法解析时返回 `None`。
    ///
    /// affinity 由命中点决定，随选区一路写进编辑器状态：CJK 这类没有悬挂
    /// 空白的软换行行尾即下一行行首，只有 `Upstream` 才把 caret 留在被点
    /// 中的那一行。宿主答不出命中时按 downstream 的探针搜索退化。
    fn text_editor_hit_offset(
        &self,
        node: StableNodeId,
        kind: TextEditorKind,
        x: f32,
        y: f32,
        shaper: &mut dyn crate::TextShaper,
    ) -> Result<Option<crate::TextHit>, FrameworkError> {
        let Some(frame) = self.world.editor_frame(node) else {
            return Ok(None);
        };
        let Some((style, constraints)) = self.world.text_input_shape_context(node) else {
            return Ok(None);
        };
        let state = self.editor_state(node)?;
        let fold_view = self.world.text_display_view(node);
        let probe_value: &str = fold_view.as_ref().map_or(&state.value, |view| &view.value);
        let mut geometry = EditorGeometry {
            shaper,
            node,
            text: TextContent {
                value: self.editor_probe_text(node, fold_view.as_ref()),
            },
            style,
            constraints,
        };
        // The point in the editor's text space, through the frame its caret
        // and selection are drawn with: the same scroll, centring and
        // inline-start anchoring, in every writing mode.
        let (local_x, local_y) = frame.text_point(x, y);
        let hit = match geometry.shaper.text_hit_at_point(
            node,
            &geometry.text,
            local_x,
            local_y,
            &geometry.style,
            geometry.constraints,
        ) {
            Some(hit) => hit,
            None => crate::TextHit::downstream(caret_offset_at_point(
                probe_value,
                local_x,
                local_y,
                geometry.probe(),
            )),
        };
        let offset = match &fold_view {
            Some(view) => view.value_of(hit.offset),
            None => hit.offset,
        };
        let atoms = atoms_in(&state.value, &self.editor_text_atoms(node, kind));
        let snapped = snap_pointer_caret(offset, &atoms);
        // 吸附到原子边界后落点不再是被点中的那个位置，affinity 随之作废。
        let affinity = if snapped == offset {
            hit.affinity
        } else {
            crate::TextAffinity::Downstream
        };
        Ok(Some(crate::TextHit {
            offset: snapped,
            affinity,
        }))
    }

    /// Delete the atom identified by `token` through the normal editor history
    /// and notify [`crate::TextAtomClosed`].
    pub fn close_focused_text_atom(
        &mut self,
        document: DocumentId,
        token: &str,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if focused.kind != TextEditorKind::Area || !focused.accepts_input {
            return Ok(false);
        }
        let atoms = atoms_in(
            &self.editor_state(focused.node)?.value,
            &self.editor_text_atoms(focused.node, focused.kind),
        );
        let Some(atom) = atoms
            .iter()
            .find(|atom| atom.token.as_ref() == token)
            .cloned()
        else {
            return Ok(false);
        };
        self.write_editor_selection(focused.node, TextSelection::new(atom.start, atom.end))?;
        if !self.delete_focused_text(document, TextDeleteKind::Backward)? {
            return Ok(false);
        }
        let entity = Entity::<TextArea>::from_stable_id(focused.node);
        self.update_component(entity, |_, cx| {
            cx.emit(crate::TextAtomClosed {
                token: Arc::clone(&atom.token),
                start: atom.start,
                end: atom.end,
            });
            false
        })?;
        Ok(true)
    }

    fn editor_text_atoms(&self, node: StableNodeId, kind: TextEditorKind) -> Vec<TextAtomSpan> {
        match kind {
            TextEditorKind::Area => self
                .view_entity::<TextArea>(node)
                .and_then(|entity| {
                    self.read(entity, |area: &TextArea| area.atom_spans.to_vec())
                        .ok()
                })
                .unwrap_or_default(),
            TextEditorKind::Field | TextEditorKind::Number => Vec::new(),
        }
    }

    /// The text an editor's geometry probes ask about: the fold / inlay
    /// view's when there is one, the committed text otherwise — as the shared,
    /// stamped values the presentation pass laid out, so the host answers a
    /// probe without comparing the text it already holds geometry for.
    fn editor_probe_text(
        &self,
        node: StableNodeId,
        fold_view: Option<&crate::world::TextDisplayView>,
    ) -> crate::TextValue {
        match fold_view {
            Some(view) => view.value.clone(),
            None => self
                .world
                .text_input(node)
                .map(|input| input.value_shared())
                .unwrap_or_default(),
        }
    }

    fn editor_state(&self, node: StableNodeId) -> Result<TextInputState, FrameworkError> {
        let hooks = self
            .plain_editor(node)
            .ok_or(FrameworkError::MissingView(node))?;
        (hooks.state)(self, node)
    }

    fn editor_state_of<C: EditableText>(
        &self,
        node: StableNodeId,
    ) -> Result<TextInputState, FrameworkError> {
        self.view_entity::<C>(node)
            .and_then(|entity| {
                self.read(entity, |editable: &C| editable.state().clone())
                    .ok()
            })
            .ok_or(FrameworkError::MissingView(node))
    }

    /// Change an editor's state in place without an edit: selection moves.
    /// `update` reports whether anything changed.
    fn update_editor_state(
        &mut self,
        node: StableNodeId,
        update: impl FnOnce(&mut TextInputState) -> bool,
    ) -> Result<bool, FrameworkError> {
        let hooks = self
            .plain_editor(node)
            .ok_or(FrameworkError::MissingView(node))?;
        let mut update = Some(update);
        (hooks.update_state)(self, node, &mut |state| {
            update.take().is_some_and(|update| update(state))
        })
    }

    fn update_editor_state_of<C: EditableText>(
        &mut self,
        node: StableNodeId,
        update: &mut dyn FnMut(&mut TextInputState) -> bool,
    ) -> Result<bool, FrameworkError> {
        self.update_component(Entity::<C>::from_stable_id(node), |editable: &mut C, _| {
            update(editable.state_mut())
        })
    }

    /// Replace an editor's whole state as one journaled edit. Editors that
    /// refuse input, or the value (a length limit), decline it.
    pub(super) fn replace_editor_state(
        &mut self,
        node: StableNodeId,
        origin: crate::TextEditOrigin,
        next: TextInputState,
    ) -> Result<bool, FrameworkError> {
        let hooks = self
            .plain_editor(node)
            .ok_or(FrameworkError::MissingView(node))?;
        (hooks.replace_state)(self, node, origin, next)
    }

    fn replace_editor_state_of<C: EditableText>(
        &mut self,
        node: StableNodeId,
        origin: crate::TextEditOrigin,
        next: TextInputState,
    ) -> Result<bool, FrameworkError> {
        self.commit_editor_edit(
            Entity::<C>::from_stable_id(node),
            origin,
            move |editable: &mut C, _| {
                if !editable.accepts_input() || !editable.accepts_edit_value(&next.value) {
                    return false;
                }
                *editable.state_mut() = next;
                true
            },
        )
    }

    /// Put an editor back to a journaled snapshot (undo/redo): its text
    /// state and, for a `NumberInput`, the committed number with it. No
    /// value check: the journal only holds states the editor already took.
    pub(super) fn restore_editor_snapshot(
        &mut self,
        node: StableNodeId,
        snapshot: super::text_history::EditorSnapshot,
    ) -> Result<bool, FrameworkError> {
        let hooks = self
            .plain_editor(node)
            .ok_or(FrameworkError::MissingView(node))?;
        (hooks.restore)(self, node, snapshot)
    }

    fn restore_editor_snapshot_of<C: EditableText>(
        &mut self,
        node: StableNodeId,
        snapshot: super::text_history::EditorSnapshot,
    ) -> Result<bool, FrameworkError> {
        let super::text_history::EditorSnapshot { state, number } = snapshot;
        // `History` keeps the restore from becoming a step of its own.
        self.commit_editor_edit(
            Entity::<C>::from_stable_id(node),
            crate::TextEditOrigin::History,
            move |editable: &mut C, cx| {
                *editable.state_mut() = state;
                if let Some(number) = number {
                    editable.restore_committed_number(number, cx);
                }
                true
            },
        )
    }

    fn write_editor_selection(
        &mut self,
        node: StableNodeId,
        selection: TextSelection,
    ) -> Result<bool, FrameworkError> {
        // 光标落在折叠隐藏区间内 → 该折叠自动展开（reveal 语义；查找导航
        // 跳转也经由此路径展开）。
        self.unfold_text_folds_containing(node, &[selection.focus])?;
        let changed = self.update_editor_state(node, |state| {
            if state.selection == selection {
                return false;
            }
            state.selection = selection;
            // The session fuses a primary that lands on another cursor; so
            // does the component, or the two disagree.
            state.normalize_selections();
            true
        })?;
        if changed {
            self.seal_editor_history(node);
        }
        Ok(changed)
    }

    /// Write the complete selection set (primary plus additional cursors),
    /// restoring the invariants in place — cursors that moved onto the same
    /// offset fuse here. Selection-only move: no change event is emitted.
    fn write_editor_selections(
        &mut self,
        node: StableNodeId,
        selection: TextSelection,
        additional: Vec<TextSelection>,
    ) -> Result<bool, FrameworkError> {
        // 任一光标落在折叠隐藏区间内 → 该折叠自动展开（附加光标进入折叠
        // 的强制展开语义；跨越折叠的范围选择不展开）。
        let focuses: Vec<usize> = std::iter::once(selection.focus)
            .chain(additional.iter().map(|selection| selection.focus))
            .collect();
        self.unfold_text_folds_containing(node, &focuses)?;
        let changed = self.update_editor_state(node, |state| {
            if state.selection == selection && state.additional_selections == additional {
                return false;
            }
            let previous = (state.selection, state.additional_selections.clone());
            state.selection = selection;
            state.additional_selections = additional;
            state.normalize_selections();
            previous.0 != state.selection || previous.1 != state.additional_selections
        })?;
        if changed {
            self.seal_editor_history(node);
        }
        Ok(changed)
    }

    /// Apply a committed value edit and emit the editor's change event.
    /// `Ok(false)` means the edit declined or changed nothing.
    fn edit_editor(
        &mut self,
        node: StableNodeId,
        origin: crate::TextEditOrigin,
        edit: impl FnOnce(&TextInputState) -> Option<EditorEdit>,
    ) -> Result<bool, FrameworkError> {
        let state = self.editor_state(node)?;
        let Some(edited) = edit(&state) else {
            return Ok(false);
        };
        if edited.value == state.value {
            return Ok(false);
        }
        let EditorEdit { value, selection } = edited;
        self.commit_editor_value(node, value.into(), selection, Vec::new(), origin)
    }

    /// Apply a per-cursor edit closure to every active selection and commit
    /// the combined result as one edit.
    ///
    /// Single-cursor fast path: the closure runs once and the write is the
    /// historical single-selection commit. Multi-cursor path: the closure
    /// runs per selection against the pre-edit snapshot,
    /// [`apply_cursor_edits`] splices everything into one output string, and
    /// the selection set is rebuilt and normalized — one intermediate String,
    /// never one per cursor.
    fn edit_editor_multi(
        &mut self,
        node: StableNodeId,
        origin: crate::TextEditOrigin,
        mut edit: impl FnMut(&str, TextSelection, bool) -> Option<CursorEdit>,
    ) -> Result<bool, FrameworkError> {
        let state = self.editor_state(node)?;
        if !state.has_additional_selections() {
            let Some(edited) = edit(&state.value, state.selection, true) else {
                return Ok(false);
            };
            let (value, selection) = match edited {
                CursorEdit::Span(replacement) => {
                    let (value, caret) = apply_replacement(&state.value, &replacement);
                    (value, TextSelection::caret(caret))
                }
                CursorEdit::Transform { next, selection } => (next, selection),
            };
            if value == state.value {
                return Ok(false);
            }
            return self.commit_editor_value(node, value.into(), selection, Vec::new(), origin);
        }
        let selections = state.selections().into_owned();
        let primary_index = selections
            .iter()
            .position(|selection| *selection == state.selection)
            .unwrap_or(0);
        let mut any_edit = false;
        let edits: Vec<(TextSelection, Option<CursorEdit>)> = selections
            .iter()
            .enumerate()
            .map(|(index, &selection)| {
                let edited = edit(&state.value, selection, index == primary_index);
                any_edit |= edited.is_some();
                (selection, edited)
            })
            .collect();
        if !any_edit {
            return Ok(false);
        }
        let Some((value, edited_selections)) = apply_cursor_edits(&state.value, &edits) else {
            return Ok(false);
        };
        if value == state.value {
            return Ok(false);
        }
        let primary = edited_selections
            .get(primary_index)
            .copied()
            .unwrap_or_else(|| TextSelection::caret(value.len()));
        let additional = edited_selections
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != primary_index)
            .map(|(_, selection)| *selection)
            .collect();
        self.commit_editor_value(node, value.into(), primary, additional, origin)
    }

    /// Commit a value edit together with the rebuilt selection set. The set
    /// is normalized here so overlapping or touching cursors fuse in the
    /// same pass that emits the change event.
    fn commit_editor_value(
        &mut self,
        node: StableNodeId,
        value: crate::TextValue,
        selection: TextSelection,
        additional: Vec<TextSelection>,
        origin: crate::TextEditOrigin,
    ) -> Result<bool, FrameworkError> {
        let linked = self.world.text_snippet_session(node).and_then(|session| {
            let old = self.editor_state(node).ok()?;
            session.linked_edit(&old.value, &value, selection)
        });
        let (value, selection, linked_session) = if let Some((value, selection, session)) = linked {
            (value.into(), selection, Some(session))
        } else {
            (value, selection, None)
        };
        let mut next = TextInputState {
            value,
            selection,
            additional_selections: additional,
        };
        next.normalize_selections();
        let changed = self.replace_editor_state(node, origin, next)?;
        if changed && let Some(session) = linked_session {
            let mut mutations = MutationQueue::new();
            mutations.set_text_input_snippet(node, Some(session));
            self.world.commit(mutations)?;
        }
        Ok(changed)
    }
}
/// 展开 snippet body：`$N` 占位从文本移除，`$1..$N` 按序号记录跳位偏移，
/// `$0` 记录初始光标位置（缺省为插入文本末尾）。`$` 后不跟数字时保持
/// 字面量。
fn expand_snippet_body(body: &str, base: usize) -> (String, Vec<usize>, usize) {
    let mut text = String::with_capacity(body.len());
    let mut stops: Vec<(u32, usize)> = Vec::new();
    let mut final_caret: Option<usize> = None;
    let mut characters = body.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '$' && characters.peek().is_some_and(|next| next.is_ascii_digit()) {
            let mut number = characters.next().unwrap().to_digit(10).unwrap_or(0);
            while let Some(next) = characters.peek()
                && next.is_ascii_digit()
            {
                number = number * 10 + next.to_digit(10).unwrap_or(0);
                characters.next();
            }
            let offset = base + text.len();
            if number == 0 {
                final_caret = Some(offset);
            } else {
                stops.push((number, offset));
            }
            continue;
        }
        text.push(character);
    }
    stops.sort_by_key(|(number, _)| *number);
    let caret = final_caret.unwrap_or(base + text.len());
    (
        text,
        stops.into_iter().map(|(_, offset)| offset).collect(),
        caret,
    )
}

/// 补全接受时被替换的词前缀字符（`[A-Za-z0-9_]`，Zed 风格标识符词）。
fn is_word_prefix_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// 从 `caret` 向前的词前缀起点（连续 `[A-Za-z0-9_]` 游程的开头）。
fn word_prefix_start(value: &str, caret: usize) -> usize {
    let bytes = value.as_bytes();
    let caret = caret.min(bytes.len());
    let mut start = caret;
    while start > 0 && is_word_prefix_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
}

impl AppContext {
    /// 聚焦多行编辑器当前活跃（非空且未关闭）的补全会话与节点。
    fn focused_completion_session(
        &self,
        document: DocumentId,
    ) -> Option<(StableNodeId, crate::store::TextCompletionViewState)> {
        let focused = self.focused_text_editor(document)?;
        if !focused.multiline || !focused.accepts_input {
            return None;
        }
        let state = self.world.text_completion_view(focused.node)?;
        (!state.items.is_empty() && !state.dismissed).then_some((focused.node, state))
    }

    /// 聚焦多行编辑器当前是否激活补全弹层（非空且未关闭的会话）。
    /// 路由层用它决定编辑键是否被弹层消费（边界上导航无可做仍消费）。
    pub fn focused_text_completion_active(&self, document: DocumentId) -> bool {
        self.focused_completion_session(document).is_some()
    }

    /// 聚焦编辑器当前是否显示签名帮助浮窗。Esc 两段式：签名在场时
    /// 路由层先消费 Esc（不关补全），由宿主撤掉签名会话。
    pub fn focused_text_signature_showing(&self, document: DocumentId) -> bool {
        self.focused_text_editor(document)
            .is_some_and(|focused| self.world.text_signature_help(focused.node).is_some())
    }

    /// 补全弹层的上下键导航：在候选间移动选中项（编辑器选区不动），
    /// 滚动窗口跟随选中项。弹层未激活或已在边界时返回 `Ok(false)`——
    /// 边界仍消费事件由路由层决定（弹层激活期间一律消费）。
    pub fn move_focused_text_completion(
        &mut self,
        document: DocumentId,
        down: bool,
    ) -> Result<bool, FrameworkError> {
        let Some((node, state)) = self.focused_completion_session(document) else {
            return Ok(false);
        };
        let len = state.items.len();
        let selected = if down {
            (state.selected + 1).min(len - 1)
        } else {
            state.selected.saturating_sub(1)
        };
        let scroll = completion_scroll_follow(selected, state.scroll, len);
        if selected == state.selected && scroll == state.scroll {
            return Ok(false);
        }
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_completion_view(node, selected, scroll);
        self.world.commit(mutations)?;
        Ok(true)
    }

    /// 接受补全候选：把主光标前的词前缀（`[A-Za-z0-9_]` 游程）替换为
    /// 候选 `label`，发一次 TextChanged。`row` 为 `Some` 时接受指定候选
    /// （弹层点击路径），`None` 接受当前选中项。多光标时只作用主光标，
    /// 附加光标经偏移重映射保留。会话本身不动：弹层的后续过滤由宿主
    /// 在 TextChanged 后重喂决定。
    pub fn accept_focused_text_completion(
        &mut self,
        document: DocumentId,
        row: Option<usize>,
    ) -> Result<bool, FrameworkError> {
        let Some((_, state)) = self.focused_completion_session(document) else {
            return Ok(false);
        };
        let index = row.unwrap_or(state.selected);
        let Some(item) = state.items.get(index) else {
            return Ok(false);
        };
        let label = item.label.clone();
        let focused = self
            .focused_text_editor(document)
            .expect("session implies editor");
        self.text_edit.caret_goal_x = None;
        if let Some(session) = self.world.text_snippet_session(focused.node)
            && let Some(group) = session
                .index
                .checked_sub(1)
                .and_then(|i| session.placeholders.get(i))
            && group.choices.iter().any(|choice| choice == &label)
            && let Some(range) = group.ranges.first()
        {
            let mut next = self.editor_state(focused.node)?;
            next.selection = TextSelection::new(range.start, range.end);
            next.replace_primary_selection(&label);
            let changed = self.commit_editor_value(
                focused.node,
                next.value,
                next.selection,
                Vec::new(),
                crate::TextEditOrigin::Structural,
            )?;
            self.dismiss_focused_text_completion(document)?;
            return Ok(changed);
        }
        if let Some(raw_edit) = &item.edit {
            let mut edit = raw_edit.clone();
            let expansion = if let Some(body) = &edit.snippet {
                let variables = crate::snippet::context_variables(
                    &edit.source,
                    TextSelection::caret(edit.caret),
                    &edit.variables,
                );
                let Some(expansion) = crate::expand_text_snippet_with_variables(body, &variables)
                else {
                    return Ok(false);
                };
                edit.text = expansion.text.clone();
                edit.stops = expansion
                    .placeholders
                    .iter()
                    .map(|g| g.ranges[0].clone())
                    .collect();
                Some(expansion)
            } else {
                None
            };
            let current = self.editor_state(focused.node)?;
            if current.value != edit.source.as_ref()
                || current.selection != TextSelection::caret(edit.caret)
            {
                return Ok(false);
            }
            let mut replacements = edit.additional.clone();
            replacements.push((edit.range.clone(), edit.text.clone()));
            replacements.sort_by_key(|(range, _)| (range.start, range.end));
            if replacements.iter().any(|(range, _)| {
                range.start > range.end
                    || range.end > current.value.len()
                    || !current.value.is_char_boundary(range.start)
                    || !current.value.is_char_boundary(range.end)
            }) || replacements
                .windows(2)
                .any(|pair| pair[0].0.end > pair[1].0.start || pair[0].0.start == pair[1].0.start)
                || edit.stops.iter().any(|stop| {
                    stop.start > stop.end
                        || !edit.text.is_char_boundary(stop.start)
                        || !edit.text.is_char_boundary(stop.end)
                })
            {
                return Ok(false);
            }
            let shift: isize = edit
                .additional
                .iter()
                .filter(|(range, _)| range.end <= edit.range.start)
                .map(|(range, text)| text.len() as isize - range.len() as isize)
                .sum();
            let base = edit
                .range
                .start
                .checked_add_signed(shift)
                .expect("validated disjoint edits");
            let mut next = current;
            for (range, text) in replacements.into_iter().rev() {
                next.selection = TextSelection::new(range.start, range.end);
                next.replace_primary_selection(&text);
            }
            next.selection = edit
                .stops
                .first()
                .map(|stop| TextSelection::new(base + stop.start, base + stop.end))
                .unwrap_or_else(|| TextSelection::caret(base + edit.text.len()));
            let changed = self.commit_editor_value(
                focused.node,
                next.value,
                next.selection,
                next.additional_selections,
                crate::TextEditOrigin::Structural,
            )?;
            if changed {
                let stops = edit
                    .stops
                    .iter()
                    .map(|range| base + range.start)
                    .collect::<Vec<_>>();
                let selection_ends = edit.stops.iter().map(|range| base + range.end).collect();
                let session = (!stops.is_empty()
                    && expansion.as_ref().is_none_or(|e| e.placeholders.len() > 1))
                .then_some(TextSnippetSession {
                    exit_on_last: true,
                    stops,
                    selection_ends,
                    placeholders: expansion
                        .map(|e| {
                            e.placeholders
                                .into_iter()
                                .map(|mut g| {
                                    for r in &mut g.ranges {
                                        r.start += base;
                                        r.end += base;
                                    }
                                    for (r, _) in &mut g.transforms {
                                        r.start += base;
                                        r.end += base;
                                    }
                                    g
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    index: 1,
                });
                let mut mutations = MutationQueue::new();
                mutations.set_text_input_snippet(focused.node, session);
                self.world.commit(mutations)?;
            }
            return Ok(changed);
        }
        self.edit_editor_multi(
            focused.node,
            crate::TextEditOrigin::Structural,
            |value, selection, is_primary| {
                if !is_primary {
                    return None;
                }
                let caret = clamp_boundary(value, selection.focus);
                let start = word_prefix_start(value, caret);
                Some(CursorEdit::Span(TextReplacement {
                    range: start..caret,
                    insert: label.clone(),
                    caret: start + label.len(),
                }))
            },
        )
    }

    /// Esc 关闭补全弹层。会话数据保留：宿主重喂相同列表不复活弹层，
    /// 换新列表（打字后过滤结果变化）重新打开。无活跃弹层时返回
    /// `Ok(false)`，Esc 落到后续优先级（多光标塌缩等）。
    pub fn dismiss_focused_text_completion(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        let Some(state) = self.world.text_completion_view(focused.node) else {
            return Ok(false);
        };
        if state.dismissed {
            return Ok(false);
        }
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_completion_dismissed(focused.node);
        self.world.commit(mutations)?;
        Ok(true)
    }

    /// 重开补全弹层（宿主显式重触发，如 Esc 关闭后再次 Ctrl+Space）：
    /// 清除关闭标记并把选中归零，候选列表保持不变。弹层未激活或未处
    /// 于关闭态时返回 `Ok(false)`。
    pub fn reopen_focused_text_completion(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        let Some(state) = self.world.text_completion_view(focused.node) else {
            return Ok(false);
        };
        if !state.dismissed {
            return Ok(false);
        }
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_completion_reopened(focused.node);
        self.world.commit(mutations)?;
        Ok(true)
    }

    /// 滚轮滚动补全弹层（`rows` 为正表示向列表末尾方向）。滚动位置钳在
    /// 候选范围内；弹层未激活时返回 `Ok(false)`。
    pub fn scroll_focused_text_completion(
        &mut self,
        document: DocumentId,
        rows: isize,
    ) -> Result<bool, FrameworkError> {
        let Some((node, state)) = self.focused_completion_session(document) else {
            return Ok(false);
        };
        let len = state.items.len();
        let scroll = clamp_scroll(state.scroll as isize + rows, len);
        if scroll == state.scroll {
            return Ok(true);
        }
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_completion_view(node, state.selected, scroll);
        self.world.commit(mutations)?;
        Ok(true)
    }

    /// 指针位置命中的补全候选（弹层点击接受路径的查询入口）。未聚焦
    /// 编辑器或未命中时为 `None`。
    pub fn completion_hit_at(
        &self,
        document: DocumentId,
        x: f32,
        y: f32,
    ) -> Result<Option<usize>, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(None);
        };
        if !focused.multiline {
            return Ok(None);
        }
        Ok(self.world.text_completion_hit(focused.node, x, y))
    }

    /// 滚轮落在锚定浮层内时的路由：优先滚动补全弹层（绘制在最上，会话
    /// 锚定聚焦编辑器），其次滚动 hover 浮窗正文。hover 命中测试驱动：
    /// hover 显示不要求焦点，滚轮落点命中任意编辑器的浮窗面板即滚动该
    /// 面板。都不在浮层上时返回 `Ok(false)`，滚轮落回编辑器/文档滚动。
    pub fn scroll_text_overlay_at(
        &mut self,
        document: DocumentId,
        x: f32,
        y: f32,
        rows: isize,
    ) -> Result<bool, FrameworkError> {
        if let Some(focused) = self.focused_text_editor(document)
            && self.world.text_completion_panel_hit(focused.node, x, y)
        {
            return self.scroll_focused_text_completion(document, rows);
        }
        let Some(node) = self.world.text_hover_panel_at(document, x, y) else {
            return Ok(false);
        };
        let Some(state) = self.world.text_hover_view(node) else {
            return Ok(false);
        };
        let line_count = state.doc.body.lines().count();
        let scroll = clamp_scroll(state.scroll as isize + rows, line_count);
        if scroll == state.scroll {
            return Ok(true);
        }
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_hover_scroll(node, scroll);
        self.world.commit(mutations)?;
        Ok(true)
    }

    /// 指针落在诊断 squiggle 或行尾文案上时打开诊断 hover；离开则撤掉
    /// 诊断占用的浮窗（不撤宿主符号 hover）。停在已打开的诊断浮窗上保持。
    pub fn update_text_diagnostic_hover(
        &mut self,
        document: DocumentId,
        x: f32,
        y: f32,
    ) -> Result<(), FrameworkError> {
        if self.diagnostic_hover_panel_holds(document, x, y) {
            return Ok(());
        }
        let target = self.world.hit_test(document, x, y);
        self.update_text_diagnostic_hover_hit(document, x, y, target)
    }

    /// The pointer rests on an open diagnostic hover panel, which keeps it.
    fn diagnostic_hover_panel_holds(&self, document: DocumentId, x: f32, y: f32) -> bool {
        self.world
            .text_hover_panel_at(document, x, y)
            .is_some_and(|node| {
                self.world
                    .text_hover_view(node)
                    .is_some_and(|state| state.diagnostic)
            })
    }

    /// [`Self::update_text_diagnostic_hover`] with the point already
    /// hit-tested: `target` is the topmost node there.
    pub fn update_text_diagnostic_hover_hit(
        &mut self,
        document: DocumentId,
        x: f32,
        y: f32,
        target: Option<StableNodeId>,
    ) -> Result<(), FrameworkError> {
        if self.diagnostic_hover_panel_holds(document, x, y) {
            return Ok(());
        }
        // Nothing to open and nothing open: the common move over plain UI.
        if target.is_none_or(|id| self.world.editor_frame(id).is_none())
            && self.world.diagnostic_hover_ids().is_empty()
        {
            return Ok(());
        }
        let hit = target.and_then(|id| {
            let (local_x, local_y) = self.world.editor_frame(id)?.text_point(x, y);
            self.world
                .text_diagnostic_hit(id, local_x, local_y)
                .map(|hover| (id, hover))
        });
        let mut mutations = crate::MutationQueue::new();
        let mut dirty = false;
        if let Some((id, hover)) = hit.as_ref() {
            mutations.set_text_input_diagnostic_hover(*id, Some(hover.clone()));
            dirty = true;
        }
        for id in self.world.diagnostic_hover_ids() {
            if hit.as_ref().is_some_and(|(hit_id, _)| *hit_id == id) {
                continue;
            }
            mutations.set_text_input_diagnostic_hover(id, None);
            dirty = true;
        }
        if dirty {
            self.world.commit(mutations)?;
        }
        Ok(())
    }
}

/// 键盘导航的滚动窗口跟随：选中项离开可见窗口时把窗口滑到包含它的
/// 最近位置。
fn completion_scroll_follow(selected: usize, scroll: usize, len: usize) -> usize {
    let visible = crate::components::TEXT_COMPLETION_VISIBLE_ROWS.min(len);
    if selected < scroll {
        selected
    } else if selected >= scroll + visible {
        selected + 1 - visible
    } else {
        scroll
    }
}

/// 滚动位置钳制：非空列表钳在 `[0, len - 1]`，空列表恒为 0。
fn clamp_scroll(scroll: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    scroll.clamp(0, (len - 1) as isize) as usize
}

impl AppContext {
    /// 光标落点所在的折叠自动展开。`offsets` 里的偏移严格落在某个折叠
    /// 的隐藏区间内部（折叠起始行行尾不算）时，该折叠展开。
    fn unfold_text_folds_containing(
        &mut self,
        node: StableNodeId,
        offsets: &[usize],
    ) -> Result<(), FrameworkError> {
        let Some(view) = self.world.text_display_view(node) else {
            return Ok(());
        };
        let to_unfold: Vec<TextCodeFold> = view
            .spans
            .iter()
            .filter(|span| offsets.iter().any(|offset| view.span_hides(span, *offset)))
            .filter_map(|span| span.fold())
            .collect();
        if to_unfold.is_empty() {
            return Ok(());
        }
        let mut state = self.world.text_fold_view_state(node).unwrap_or_default();
        state.collapsed.retain(|fold| !to_unfold.contains(fold));
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_fold_collapsed(node, state.collapsed.into());
        self.world.commit(mutations)?;
        Ok(())
    }

    /// 写回单个节点的折叠态（toggle 命令与点击交互共用）。
    fn set_node_text_fold(
        &mut self,
        node: StableNodeId,
        fold: TextCodeFold,
        collapse: bool,
    ) -> Result<bool, FrameworkError> {
        let mut state = self.world.text_fold_view_state(node).unwrap_or_default();
        let changed = if collapse {
            if state.collapsed.contains(&fold) {
                false
            } else {
                state.collapsed.push(fold);
                state.collapsed.sort_by_key(|fold| (fold.start, fold.end));
                true
            }
        } else {
            let before = state.collapsed.len();
            state.collapsed.retain(|entry| *entry != fold);
            state.collapsed.len() != before
        };
        if !changed {
            return Ok(false);
        }
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_fold_collapsed(node, state.collapsed.into());
        self.world.commit(mutations)?;
        Ok(true)
    }

    /// 切换聚焦代码编辑器的一个折叠区间。`offset` 为 `Some` 时取包含该
    /// 偏移的最内层折叠，`None` 时取主光标所在的；光标不在任何折叠上时
    /// 返回 `Ok(false)`。折叠是纯视图状态：不改值、不发 change 事件。
    pub fn toggle_focused_text_fold(
        &mut self,
        document: DocumentId,
        offset: Option<usize>,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_selection {
            return Ok(false);
        }
        let state = self.editor_state(focused.node)?;
        let view = self.world.text_fold_view_state(focused.node);
        let offered = view.as_ref().map_or(&[][..], |state| &state.offered);
        let at = offset.unwrap_or(state.selection.focus);
        let at = clamp_boundary(&state.value, at);
        // 包含该偏移的最内层折叠：区间覆盖，或偏移落在折叠起始行上
        // （Zed 语义：光标在 `{` 所在行即可切换该折叠）。
        let fold = offered
            .iter()
            .filter(|fold| {
                let line_start = logical_line_range(&state.value, fold.start).0;
                line_start <= at && at <= fold.end
            })
            .max_by_key(|fold| fold.start)
            .copied();
        let Some(fold) = fold else {
            return Ok(false);
        };
        let collapsed = view.is_some_and(|state| state.collapsed.contains(&fold));
        self.set_node_text_fold(focused.node, fold, !collapsed)
    }

    /// 展开聚焦编辑器的全部折叠。返回是否发生了变化。
    pub fn unfold_all_focused_text_folds(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        let Some(state) = self.world.text_fold_view_state(focused.node) else {
            return Ok(false);
        };
        if state.collapsed.is_empty() {
            return Ok(false);
        }
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_fold_collapsed(focused.node, Arc::from([]));
        self.world.commit(mutations)?;
        Ok(true)
    }

    /// 查询聚焦编辑器当前的折叠态区间（值空间，按 `start` 排序）。宿主
    /// 测试与状态面板的只读入口；无折叠时为空表。
    pub fn focused_text_collapsed_folds(&self, document: DocumentId) -> Vec<TextCodeFold> {
        self.focused_text_editor(document)
            .and_then(|focused| self.world.text_fold_view_state(focused.node))
            .map(|state| state.collapsed)
            .unwrap_or_default()
    }

    /// 在聚焦多行编辑器的当前主选区处插入 snippet。占位 `$N` 从文本中
    /// 移除，`$1..$N` 按序号成为 Tab 跳位，`$0`（缺省为插入文本末尾）是
    /// 插入后的光标位置并开启会话；无占位的 snippet 不开启会话。
    ///
    /// 多光标限制：snippet 只作用于主光标（附加光标随编辑平移保留），
    /// 不做占位镜显。
    pub fn insert_focused_text_snippet(
        &mut self,
        document: DocumentId,
        snippet: &TextSnippet,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_input {
            return Ok(false);
        }
        let node = focused.node;
        self.text_edit.caret_goal_x = None;
        let state = self.editor_state(node)?;
        let (insert, stops, final_caret) =
            expand_snippet_body(&snippet.body, state.selection.ordered().start);
        let mut next = state.clone();
        // 主光标独占：与 IME 提交同款路径，附加光标经偏移重映射保留。
        if !next.replace_primary_selection(&insert) {
            return Ok(false);
        }
        let session = (!stops.is_empty()).then_some(TextSnippetSession {
            exit_on_last: false,
            stops,
            selection_ends: Vec::new(),
            placeholders: Vec::new(),
            index: 0,
        });
        // 选区落在 $0（缺省为插入文本末尾）。
        next.selection = TextSelection::caret(final_caret);
        // 走与其它编辑同一个入口：一次插入是一个撤销步，只读与长度上限
        // 照常把关。旧会话先结束——新插入不是旧占位的联动编辑；插入被拒
        // 时原样恢复。
        let previous = self.world.text_snippet_session(node);
        if previous.is_some() {
            let mut mutations = crate::MutationQueue::new();
            mutations.set_text_input_snippet(node, None);
            self.world.commit(mutations)?;
        }
        if !self.commit_editor_value(
            node,
            next.value,
            next.selection,
            next.additional_selections,
            crate::TextEditOrigin::Structural,
        )? {
            if previous.is_some() {
                let mut mutations = crate::MutationQueue::new();
                mutations.set_text_input_snippet(node, previous);
                self.world.commit(mutations)?;
            }
            return Ok(false);
        }
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_snippet(node, session);
        self.world.commit(mutations)?;
        Ok(true)
    }

    /// snippet 会话内 Tab/Shift+Tab 跳位：前进到下一个 `$N`、后退到上一
    /// 个已访问跳位；越过最后一个跳位（或后退越过第一个）时结束会话并
    /// 放行 Tab（返回 `Ok(false)`，缩进等既有行为接手）。无会话时返回
    /// `Ok(false)`。
    pub fn advance_focused_text_snippet(
        &mut self,
        document: DocumentId,
        reverse: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if !focused.multiline || !focused.accepts_selection {
            return Ok(false);
        }
        let node = focused.node;
        let session = self.world.text_snippet_session(node);
        let Some(mut session) = session else {
            return Ok(false);
        };
        let mut ended = false;
        // 不变式：`index` 指向下一个前进跳位（前进落到 `stops[index]` 后
        // 加一）。后退先回退两步取"上一个已访问跳位"，再恢复不变式。
        let caret = if reverse {
            if session.index <= 1 {
                ended = true;
                None
            } else {
                session.index -= 2;
                let caret = session.stops[session.index];
                session.index += 1;
                Some(caret)
            }
        } else if session.index >= session.stops.len() {
            ended = true;
            None
        } else {
            let caret = session.stops[session.index];
            session.index += 1;
            Some(caret)
        };
        if let Some(caret) = caret {
            let state = self.editor_state(node)?;
            if clamp_boundary(&state.value, caret) != caret {
                // 跳位失效（文本边界已不在字符边界上）：结束会话。
                ended = true;
            } else {
                let end = session
                    .selection_ends
                    .get(session.index.saturating_sub(1))
                    .copied()
                    .unwrap_or(caret);
                self.write_editor_selection(node, TextSelection::new(caret, end))?;
            }
        }
        if session.exit_on_last && session.index == session.stops.len() {
            ended = true;
        }
        let session = (!ended).then_some(session);
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_snippet(node, session);
        self.world.commit(mutations)?;
        Ok(true)
    }

    /// 结束聚焦编辑器的 snippet 会话（Esc）。返回是否存在活跃会话。
    pub fn cancel_focused_text_snippet(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            return Ok(false);
        };
        if self.world.text_snippet_session(focused.node).is_none() {
            return Ok(false);
        }
        let mut mutations = crate::MutationQueue::new();
        mutations.set_text_input_snippet(focused.node, None);
        self.world.commit(mutations)?;
        Ok(true)
    }
}

#[cfg(test)]
mod fold_snippet_tests {
    use super::*;
    use crate::{AppContext, DocumentId, TextSnippet};

    /// "fn a() {\n    x();\n    y();\n}\nfn b() {}"，块折叠区间
    /// `{`（7）到 `}` 之后（28），隐藏三行。
    fn fold_value() -> &'static str {
        "fn a() {\n    x();\n    y();\n}\nfn b() {}"
    }

    const FOLD_BLOCK: TextCodeFold = TextCodeFold { start: 7, end: 28 };

    fn focused_editor(value: &str) -> (AppContext, DocumentId, Entity<TextArea>, StableNodeId) {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let area = context
            .create_component(document, TextArea::new(value).code_editor(true))
            .unwrap();
        let node = area.stable_id();
        context.focus_node(document, node).unwrap();
        (context, document, area, node)
    }

    fn offered_editor(
        value: &str,
        folds: &[TextCodeFold],
    ) -> (AppContext, DocumentId, Entity<TextArea>, StableNodeId) {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let area = context
            .create_component(
                document,
                TextArea::new(value)
                    .code_editor(true)
                    .code_folds(Arc::from(folds.to_vec().into_boxed_slice())),
            )
            .unwrap();
        let node = area.stable_id();
        context.focus_node(document, node).unwrap();
        (context, document, area, node)
    }

    fn selection_of(context: &AppContext, node: StableNodeId) -> (usize, usize) {
        let state = context.world().text_input(node).unwrap();
        (state.selection.anchor, state.selection.focus)
    }

    #[test]
    fn toggle_focused_text_fold_switches_and_reports_state() {
        let (mut context, document, _area, node) = offered_editor(fold_value(), &[FOLD_BLOCK]);
        // 光标不在任何折叠上：不消费。
        assert!(!context.toggle_focused_text_fold(document, None).unwrap());
        assert!(context.focused_text_collapsed_folds(document).is_empty());

        // 光标移到折叠起始行：toggle 折叠，查询接口回报状态。
        context
            .update_component(_area, |area, _| {
                area.state.selection = TextSelection::caret(3);
            })
            .unwrap();
        assert!(context.toggle_focused_text_fold(document, None).unwrap());
        assert_eq!(
            context.focused_text_collapsed_folds(document),
            vec![FOLD_BLOCK]
        );
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            fold_value()
        );
        // 折叠是纯视图：值不变、change 事件零条（由宿主值监听保证）。

        // 再次 toggle 展开；指定偏移与光标等价。
        assert!(context.toggle_focused_text_fold(document, None).unwrap());
        assert!(context.focused_text_collapsed_folds(document).is_empty());
        assert!(
            context
                .toggle_focused_text_fold(document, Some(10))
                .unwrap()
        );
        assert_eq!(
            context.focused_text_collapsed_folds(document),
            vec![FOLD_BLOCK]
        );

        // 全部展开。
        assert!(context.unfold_all_focused_text_folds(document).unwrap());
        assert!(context.focused_text_collapsed_folds(document).is_empty());
        assert!(!context.unfold_all_focused_text_folds(document).unwrap());
    }

    #[test]
    fn caret_movement_skips_collapsed_regions() {
        let (mut context, document, _area, node) = offered_editor(fold_value(), &[FOLD_BLOCK]);
        context
            .update_component(_area, |area, _| {
                area.state.selection = TextSelection::caret(0);
            })
            .unwrap();
        assert!(context.toggle_focused_text_fold(document, None).unwrap());
        let mut shaper = crate::MeasureTextShaper;

        // Down：跳过隐藏的三行，落到下一可见行 `fn b()` 行首。
        assert!(
            context
                .move_focused_text_caret(document, TextCaretIntent::Down, false, None)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (29, 29));

        // Up：回到文档首行行首（隐藏行的偏移不可达）。
        assert!(
            context
                .move_focused_text_caret(document, TextCaretIntent::Up, false, None)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (0, 0));

        // LineEnd 在折叠起始行：落到可见行尾（摘要标记之后），随后
        // LineStart 回到行首。
        context
            .update_component(_area, |area, _| {
                area.state.selection = TextSelection::caret(3);
            })
            .unwrap();
        assert!(
            context
                .move_focused_text_caret(document, TextCaretIntent::LineEnd, false, None)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (28, 28));
        assert!(
            context
                .move_focused_text_caret(document, TextCaretIntent::LineStart, false, None)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (0, 0));

        // 带几何探针的垂直移动同样按显示视图解析。
        context
            .update_component(_area, |area, _| {
                area.state.selection = TextSelection::caret(0);
            })
            .unwrap();
        assert!(
            context
                .move_focused_text_caret(document, TextCaretIntent::Down, false, Some(&mut shaper))
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (29, 29));

        // DocEnd / DocStart 始终到达边界。
        assert!(
            context
                .move_focused_text_caret(document, TextCaretIntent::DocEnd, false, None)
                .unwrap()
        );
        assert_eq!(
            selection_of(&context, node),
            (fold_value().len(), fold_value().len())
        );
        assert!(
            context
                .move_focused_text_caret(document, TextCaretIntent::DocStart, false, None)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (0, 0));
    }

    #[test]
    fn find_next_match_unfolds_fold_hiding_the_match() {
        let (mut context, document, _area, node) = offered_editor(fold_value(), &[FOLD_BLOCK]);
        assert!(
            context
                .toggle_focused_text_fold(document, Some(10))
                .unwrap()
        );
        assert_eq!(
            context.focused_text_collapsed_folds(document),
            vec![FOLD_BLOCK]
        );

        // "x();" 只出现在折叠区间内：导航命中后该折叠自动展开（reveal
        // 语义），选区落在匹配上。
        let found = context
            .find_next_focused_text_match(
                document,
                "x();",
                crate::TextSearchOptions::default(),
                TextFindScope::Document,
            )
            .unwrap();
        assert!(found);
        assert_eq!(context.focused_text_collapsed_folds(document), vec![]);
        assert_eq!(selection_of(&context, node), (13, 17));
    }

    #[test]
    fn select_all_occurrences_unfolds_fold_containing_new_cursor() {
        let value = "fn a() {\n    x();\n    y();\n}\nlet x();";
        let fold = TextCodeFold { start: 7, end: 28 };
        let (mut context, document, area, node) = offered_editor(value, &[fold]);
        context
            .update_component(area, |area_view, _| {
                area_view.state.selection = TextSelection::new(13, 17);
            })
            .unwrap();
        assert!(
            context
                .select_all_focused_text_occurrences(document)
                .unwrap()
        );
        // 新光标（第二处出现）落在折叠外，但主光标在折叠隐藏区间内——
        // 该折叠自动展开；两个光标都落在真实出现上。
        assert_eq!(context.focused_text_collapsed_folds(document), vec![]);
        let state = context.world().text_input(node).unwrap();
        assert_eq!(
            state.selections().into_owned(),
            vec![TextSelection::new(13, 17), TextSelection::new(33, 37),]
        );
        assert_eq!(state.value, value);
    }

    #[test]
    fn snippet_insertion_lands_on_zero_and_tabs_through_stops() {
        let (mut context, document, _area, node) = focused_editor("fn main() {}");
        context
            .update_component(_area, |area, _| {
                area.state.selection = TextSelection::caret(0);
            })
            .unwrap();
        let snippet = TextSnippet::new("let", "let $1 = $2;$0");
        assert!(
            context
                .insert_focused_text_snippet(document, &snippet)
                .unwrap()
        );
        // `$N` 移除后插入原文，光标停在 `$0`（插入文本末尾）。
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "let  = ;fn main() {}"
        );
        assert_eq!(selection_of(&context, node), (8, 8));

        // Tab 依次跳 $1、$2；跳完后会话结束，caret 停在最后跳位。
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (4, 4));
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (7, 7));
        // 会话仍在：最后一个跳位之后的 Tab 结束会话（消费最后一次）。
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (7, 7));
        // 会话已结束：Tab 不再被 snippet 消费。
        assert!(
            !context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
    }

    #[test]
    fn snippet_shift_tab_walks_stops_backwards_and_ends_session() {
        let (mut context, document, _area, node) = focused_editor("");
        let snippet = TextSnippet::new("let", "let $1 = $2;$0");
        assert!(
            context
                .insert_focused_text_snippet(document, &snippet)
                .unwrap()
        );
        // 前进到 $2（第二个跳位）。
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (7, 7));
        // Shift+Tab 回到 $1；在第一个跳位上再后退即结束会话。
        assert!(
            context
                .advance_focused_text_snippet(document, true)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (4, 4));
        assert!(
            context
                .advance_focused_text_snippet(document, true)
                .unwrap()
        );
        assert!(
            !context
                .advance_focused_text_snippet(document, true)
                .unwrap()
        );
    }

    #[test]
    fn snippet_tab_precedes_indent_and_escape_ends_session() {
        let (mut context, document, _area, node) = focused_editor("");
        let snippet = TextSnippet::new("if", "if $1 {\n\t$2\n}$0");
        assert!(
            context
                .insert_focused_text_snippet(document, &snippet)
                .unwrap()
        );
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "if  {\n\t\n}"
        );

        // 会话内 Tab 跳位而不是插入缩进（值不变，只有光标移动）。
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (3, 3));
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "if  {\n\t\n}"
        );
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (7, 7));

        // Esc 结束会话；之后再 Esc 没有会话可结束。
        assert!(context.cancel_focused_text_snippet(document).unwrap());
        assert!(!context.cancel_focused_text_snippet(document).unwrap());

        // 无会话且代码编辑器：Tab 回到缩进行为。
        assert!(
            !context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert!(context.code_edit_indent(document, false).unwrap());
        assert_ne!(
            context.world().text_input(node).unwrap().value,
            "if  {\n\t\n}"
        );
    }

    #[test]
    fn snippet_insert_keeps_additional_cursors_on_primary_only() {
        let (mut context, document, area, node) = focused_editor("ab\ncd");
        context
            .update_component(area, |area_view, _| {
                area_view.state.selection = TextSelection::caret(2);
                area_view.state.additional_selections = vec![TextSelection::caret(5)];
            })
            .unwrap();
        let snippet = TextSnippet::new("x", "[$1]$0");
        assert!(
            context
                .insert_focused_text_snippet(document, &snippet)
                .unwrap()
        );
        let state = context.world().text_input(node).unwrap();
        // 只作用主光标；附加光标随编辑平移保留（多光标限制，声明行为）。
        assert_eq!(state.value, "ab[]\ncd");
        assert_eq!((state.selection.anchor, state.selection.focus), (4, 4));
        assert_eq!(state.additional_selections, vec![TextSelection::caret(7)]);
    }

    #[test]
    fn external_edit_shifts_snippet_stops_and_ends_session_on_invalidated_stop() {
        let (mut context, document, area, node) = focused_editor("");
        let snippet = TextSnippet::new("let", "let $1 = $2;$0");
        context
            .insert_focused_text_snippet(document, &snippet)
            .unwrap();

        // 在 $0 处打字（插入文本末尾）：跳位不受影响，会话保持。
        context.replace_focused_text(document, "X").unwrap();
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (4, 4));

        // 编辑区间严格覆盖 $1 跳位（删除覆盖它的 span）→ 会话失效结束。
        context
            .update_component(area, |area_view, _| {
                area_view.state.selection = TextSelection::new(2, 6);
            })
            .unwrap();
        assert!(
            context
                .delete_focused_text(document, crate::TextDeleteKind::Backward)
                .unwrap()
        );
        assert!(
            !context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
    }

    #[test]
    fn snippet_without_stops_never_opens_session() {
        let (mut context, document, _area, node) = focused_editor("");
        let snippet = TextSnippet::new("fn", "fn main() {}");
        assert!(
            context
                .insert_focused_text_snippet(document, &snippet)
                .unwrap()
        );
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "fn main() {}"
        );
        assert_eq!(selection_of(&context, node), (12, 12));
        assert!(
            !context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
    }

    #[test]
    fn select_range_sets_sole_selection_clamped_and_clears_extra_cursors() {
        // "a中文"：字节 1..4 是"中"，4..7 是"文"。
        let (mut context, document, area, node) = focused_editor("a中文");
        context
            .update_component(area, |area_view, _| {
                area_view.state.selection = TextSelection::caret(0);
                area_view.state.additional_selections = vec![TextSelection::caret(1)];
            })
            .unwrap();

        // 越界偏移钳到值长度；附加光标清除；start == end 为光标。
        assert!(
            context
                .select_focused_text_range(document, 100, 200)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (7, 7));
        assert!(
            context
                .world()
                .text_input(node)
                .unwrap()
                .additional_selections
                .is_empty()
        );

        // 字符边界钳制：落在"中"内部（2）回退到 1，落在"文"内部（5）回退到 4。
        assert!(context.select_focused_text_range(document, 2, 5).unwrap());
        assert_eq!(selection_of(&context, node), (1, 4));

        // 无聚焦编辑器：不消费。
        let mut other = AppContext::new();
        let other_document = DocumentId::new(9).unwrap();
        assert!(
            !other
                .select_focused_text_range(other_document, 0, 1)
                .unwrap()
        );
    }

    #[test]
    fn select_range_unfolds_fold_hiding_endpoint() {
        let (mut context, document, _area, node) = offered_editor(fold_value(), &[FOLD_BLOCK]);
        assert!(context.toggle_focused_text_fold(document, Some(3)).unwrap());
        assert_eq!(
            context.focused_text_collapsed_folds(document),
            vec![FOLD_BLOCK]
        );

        // 选区端点落在隐藏区间内部：该折叠自动展开（reveal 语义）。
        assert!(context.select_focused_text_range(document, 13, 17).unwrap());
        assert_eq!(context.focused_text_collapsed_folds(document), vec![]);
        assert_eq!(selection_of(&context, node), (13, 17));

        // 起点在折叠外、终点落在隐藏区间内：区间覆盖同样展开。
        assert!(context.toggle_focused_text_fold(document, Some(3)).unwrap());
        assert!(context.select_focused_text_range(document, 0, 15).unwrap());
        assert_eq!(context.focused_text_collapsed_folds(document), vec![]);
        assert_eq!(selection_of(&context, node), (0, 15));
    }

    #[test]
    fn select_range_into_nested_folds_unfolds_only_covering_parent() {
        let child = TextCodeFold::new(12, 20);
        let (mut context, document, _area, node) =
            offered_editor(fold_value(), &[FOLD_BLOCK, child]);
        // 先折叠父级，再折叠子级（偏移 15 的最内层折叠）。
        assert!(context.toggle_focused_text_fold(document, Some(3)).unwrap());
        assert!(
            context
                .toggle_focused_text_fold(document, Some(15))
                .unwrap()
        );
        assert_eq!(
            context.focused_text_collapsed_folds(document),
            vec![FOLD_BLOCK, child]
        );

        // display 语义：被父级覆盖的子折叠不进入显示视图，偏移由父级
        // 遮住——只展开父级，子级保持折叠。
        assert!(context.select_focused_text_range(document, 15, 16).unwrap());
        assert_eq!(context.focused_text_collapsed_folds(document), vec![child]);
        assert_eq!(selection_of(&context, node), (15, 16));
    }

    #[test]
    fn select_range_is_selection_only_and_emits_no_change() {
        let (mut context, document, area, node) = offered_editor(fold_value(), &[FOLD_BLOCK]);
        let changes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&changes);
        context
            .on(area, move |_view, event: &crate::TextChanged, _cx| {
                sink.lock().unwrap().push(event.value.clone());
            })
            .unwrap();
        assert!(context.toggle_focused_text_fold(document, Some(3)).unwrap());

        // 选区写入 + 折叠展开都不发 change 事件（同 move 命令）。
        assert!(context.select_focused_text_range(document, 15, 17).unwrap());
        assert_eq!(context.focused_text_collapsed_folds(document), vec![]);
        assert_eq!(selection_of(&context, node), (15, 17));
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            fold_value()
        );
        assert!(changes.lock().unwrap().is_empty());

        // 选区本就一致：返回 false，依旧零事件。
        assert!(!context.select_focused_text_range(document, 15, 17).unwrap());
        assert!(changes.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod completion_tests {
    use crate::{
        AppContext, DocumentId, Entity, StableNodeId, TextArea, TextCompletion, TextSignatureHelp,
    };
    use std::sync::Arc;

    fn items(labels: &[&str]) -> Arc<[TextCompletion]> {
        labels
            .iter()
            .map(|label| TextCompletion::new(*label, "fn"))
            .collect::<Vec<_>>()
            .into()
    }

    fn selection_of(context: &AppContext, node: StableNodeId) -> (usize, usize) {
        let state = context.world().text_input(node).unwrap();
        (state.selection.anchor, state.selection.focus)
    }

    fn completion_editor(
        value: &str,
        completions: Arc<[TextCompletion]>,
    ) -> (AppContext, DocumentId, Entity<TextArea>, StableNodeId) {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let area = context
            .create_component(document, TextArea::new(value).completions(completions))
            .unwrap();
        let node = area.stable_id();
        context.focus_node(document, node).unwrap();
        (context, document, area, node)
    }

    fn session_of(context: &AppContext, node: StableNodeId) -> Option<(usize, usize, bool, usize)> {
        context.world().text_completion_view(node).map(|state| {
            (
                state.items.len(),
                state.selected,
                state.dismissed,
                state.scroll,
            )
        })
    }

    fn caret_to(context: &mut AppContext, area: Entity<TextArea>, caret: usize) {
        context
            .update_component(area, |view, _| {
                view.state.selection = crate::TextSelection::caret(caret);
            })
            .unwrap();
    }

    #[test]
    fn feed_activates_session_and_same_feed_keeps_selection() {
        let (mut context, document, area, node) =
            completion_editor("fn f", items(&["fn", "format"]));
        // 喂入非空列表激活会话：选中第一条。
        assert_eq!(session_of(&context, node), Some((2, 0, false, 0)));

        // Down：钳在最后一条（导航无可做，返回 false，但弹层仍活跃）。
        assert!(
            context
                .move_focused_text_completion(document, true)
                .unwrap()
        );
        assert_eq!(session_of(&context, node), Some((2, 1, false, 0)));
        assert!(
            !context
                .move_focused_text_completion(document, true)
                .unwrap()
        );
        assert_eq!(session_of(&context, node), Some((2, 1, false, 0)));

        // 重喂相同列表：键盘选中保持。
        context
            .update_component(area, |view, _| {
                view.completions = items(&["fn", "format"]);
            })
            .unwrap();
        assert_eq!(session_of(&context, node), Some((2, 1, false, 0)));

        // 重喂不同列表：视为新会话（选中归零、重新打开）。
        context
            .update_component(area, |view, _| {
                view.completions = items(&["format"]);
            })
            .unwrap();
        assert_eq!(session_of(&context, node), Some((1, 0, false, 0)));

        // 喂空列表：会话移除（弹层关闭）。
        context
            .update_component(area, |view, _| {
                view.completions = Arc::from([]);
            })
            .unwrap();
        assert_eq!(session_of(&context, node), None);
    }

    #[test]
    fn navigation_scrolls_window_and_clamps() {
        let (mut context, document, _area, node) = completion_editor(
            "",
            items(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]),
        );
        // Down 走到第 9 条：第 9 步时窗口滑出一页（scroll = 8 + 1 - 8 = 1）。
        for _ in 0..8 {
            assert!(
                context
                    .move_focused_text_completion(document, true)
                    .unwrap()
            );
        }
        assert_eq!(session_of(&context, node), Some((10, 8, false, 1)));
        // 第 10 条：窗口再次滑动使选中项可见（scroll = 9 + 1 - 8 = 2）。
        assert!(
            context
                .move_focused_text_completion(document, true)
                .unwrap()
        );
        assert_eq!(session_of(&context, node), Some((10, 9, false, 2)));
        // 边界：再 Down 不再移动（导航返回 false 表示无可做）。
        assert!(
            !context
                .move_focused_text_completion(document, true)
                .unwrap()
        );
        // Up 回到第一条：scroll 跟回 0。
        for _ in 0..9 {
            assert!(
                context
                    .move_focused_text_completion(document, false)
                    .unwrap()
            );
        }
        assert_eq!(session_of(&context, node), Some((10, 0, false, 0)));
        assert!(
            !context
                .move_focused_text_completion(document, false)
                .unwrap()
        );
    }

    #[test]
    fn accept_replaces_word_prefix_and_emits_one_change() {
        let (mut context, document, area, node) =
            completion_editor("let x = fo", items(&["food", "foobar"]));
        caret_to(&mut context, area, "let x = fo".len());
        let changes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&changes);
        context
            .on(area, move |_view, event: &crate::TextChanged, _cx| {
                sink.lock().unwrap().push(event.value.clone());
            })
            .unwrap();

        // 接受选中项（第一条）：光标前的词前缀 `fo` 替换为 label，一次
        // TextChanged，光标停在插入末尾。
        assert!(
            context
                .accept_focused_text_completion(document, None)
                .unwrap()
        );
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "let x = food"
        );
        assert_eq!(
            selection_of(&context, node),
            ("let x = food".len(), "let x = food".len())
        );
        assert_eq!(*changes.lock().unwrap(), vec!["let x = food".to_string()]);

        // 点击接受第二行候选：替换同一词前缀，再发一次 TextChanged。
        caret_to(&mut context, area, "let x = food".len());
        assert!(
            context
                .accept_focused_text_completion(document, Some(1))
                .unwrap()
        );
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "let x = foobar"
        );
        assert_eq!(changes.lock().unwrap().len(), 2);
    }

    #[test]
    fn dismiss_sticks_for_same_list_and_reopens_on_new_list() {
        let (mut context, document, area, node) = completion_editor("f", items(&["fn", "format"]));
        assert_eq!(session_of(&context, node), Some((2, 0, false, 0)));

        // Esc 关闭：会话数据保留但 dismissed 置位；重复关闭返回 false
        // （Esc 落到后续优先级）。
        assert!(context.dismiss_focused_text_completion(document).unwrap());
        assert_eq!(session_of(&context, node), Some((2, 0, true, 0)));
        assert!(!context.dismiss_focused_text_completion(document).unwrap());

        // 光标移动触发组件重投影、宿主重喂相同列表：弹层不复活。
        caret_to(&mut context, area, 0);
        assert_eq!(session_of(&context, node), Some((2, 0, true, 0)));

        // 打字后宿主重喂不同列表：新会话（重新打开、选中归零）。
        caret_to(&mut context, area, 1);
        context
            .update_component(area, |view, _| {
                view.completions = items(&["format"]);
            })
            .unwrap();
        assert_eq!(session_of(&context, node), Some((1, 0, false, 0)));

        // 无会话时 dismiss 与导航都返回 false。
        context
            .update_component(area, |view, _| {
                view.completions = Arc::from([]);
            })
            .unwrap();
        assert!(!context.dismiss_focused_text_completion(document).unwrap());
        assert!(
            !context
                .move_focused_text_completion(document, true)
                .unwrap()
        );
        assert!(
            !context
                .accept_focused_text_completion(document, None)
                .unwrap()
        );
    }

    #[test]
    fn completion_wheel_scroll_clamps_into_range() {
        let (mut context, document, _area, node) =
            completion_editor("", items(&["a", "b", "c", "d", "e"]));
        // 向列表末尾方向滚过边界：钳在最后一条可见。
        assert!(context.scroll_focused_text_completion(document, 7).unwrap());
        assert_eq!(session_of(&context, node), Some((5, 0, false, 4)));
        // 回滚到顶。
        assert!(
            context
                .scroll_focused_text_completion(document, -9)
                .unwrap()
        );
        assert_eq!(session_of(&context, node), Some((5, 0, false, 0)));
        // 无会话时不消费。
        context
            .update_component(_area, |view, _| {
                view.completions = Arc::from([]);
            })
            .unwrap();
        assert!(!context.scroll_focused_text_completion(document, 1).unwrap());
    }

    #[test]
    fn signature_showing_is_reported_for_escape_two_stage() {
        let (mut context, document, area, _node) =
            completion_editor("mix(", items(&["mix", "max"]));
        assert!(!context.focused_text_signature_showing(document));
        assert!(context.focused_text_completion_active(document));
        context
            .update_component(area, |view, _| {
                view.signature = Some(TextSignatureHelp::new(
                    "mix",
                    "",
                    vec![("e1".into(), String::new())],
                    0,
                ));
            })
            .unwrap();
        assert!(context.focused_text_signature_showing(document));
        assert!(
            context.focused_text_completion_active(document),
            "签名在场时补全会话仍保持"
        );
    }

    #[test]
    fn accept_only_edits_primary_cursor_and_maps_additional() {
        let (mut context, document, area, node) = completion_editor("fo\nfo", items(&["food"]));
        context
            .update_component(area, |view, _| {
                view.state.selection = crate::TextSelection::caret(2);
                view.state.additional_selections = vec![crate::TextSelection::caret(5)];
            })
            .unwrap();
        assert!(
            context
                .accept_focused_text_completion(document, None)
                .unwrap()
        );
        let state = context.world().text_input(node).unwrap();
        // 只作用主光标；附加光标随编辑平移保留（与 snippet 同款限制）。
        assert_eq!(state.value, "food\nfo");
        assert_eq!(state.selection, crate::TextSelection::caret(4));
        assert_eq!(
            state.additional_selections,
            vec![crate::TextSelection::caret(7)]
        );
    }

    #[test]
    fn completion_transaction_inserts_server_text_and_additions_once_with_placeholder_selection() {
        let source = "// header\nfo_tail";
        let edit = crate::TextCompletionEdit {
            snippet: None,
            variables: Default::default(),
            source: Arc::from(source),
            caret: 12,
            range: 10..source.len(),
            text: "foo(value, other)".into(),
            additional: vec![(0..0, "// added\n".into())],
            stops: vec![4..9, 11..16, 17..17],
        };
        let candidate = TextCompletion::new("display label", "fn").edit(edit);
        let (mut context, document, area, node) = completion_editor(source, Arc::from([candidate]));
        caret_to(&mut context, area, 12);
        let changes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let emitted = Arc::clone(&changes);
        context
            .on(area, move |_, event: &crate::TextChanged, _| {
                emitted.lock().unwrap().push(event.value.clone());
            })
            .unwrap();
        assert!(
            context
                .accept_focused_text_completion(document, None)
                .unwrap()
        );
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "// added\n// header\nfoo(value, other)"
        );
        assert_eq!(
            changes.lock().unwrap().len(),
            1,
            "one change produces one host undo transaction"
        );
        assert_eq!(selection_of(&context, node), (23, 28));
        assert!(context.replace_focused_text(document, "x").unwrap());
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (26, 31));
    }

    #[test]
    fn snippet_completion_links_utf8_edits_and_choices_with_tab_navigation() {
        let candidate = TextCompletion::new("linked", "snippet").edit(crate::TextCompletionEdit {
            snippet: Some("${1:颜色} + $1 + ${2|red,blue|} + $2$0".into()),
            variables: Default::default(),
            source: Arc::from("fo"),
            caret: 2,
            range: 0..2,
            text: String::new(),
            additional: vec![],
            stops: vec![],
        });
        let (mut context, document, area, node) = completion_editor("fo", Arc::from([candidate]));
        caret_to(&mut context, area, 2);
        let changes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let emitted = Arc::clone(&changes);
        context
            .on(area, move |_, event: &crate::TextChanged, _| {
                emitted.lock().unwrap().push(event.value.clone());
            })
            .unwrap();
        assert!(
            context
                .accept_focused_text_completion(document, None)
                .unwrap()
        );
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "颜色 + 颜色 + red + red"
        );
        assert_eq!(changes.lock().unwrap().len(), 1);
        assert!(context.replace_focused_text(document, "蓝").unwrap());
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "蓝 + 蓝 + red + red"
        );
        assert!(context.commit_ime(document, "色").unwrap());
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "蓝色 + 蓝色 + red + red"
        );
        assert_eq!(
            changes.lock().unwrap().len(),
            3,
            "one event per linked edit"
        );
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        context.reproject_component(area).unwrap();
        assert!(context.focused_text_completion_active(document));
        assert!(
            context
                .move_focused_text_completion(document, true)
                .unwrap()
        );
        assert!(
            context
                .accept_focused_text_completion(document, None)
                .unwrap()
        );
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "蓝色 + 蓝色 + blue + blue"
        );
        assert!(
            context
                .advance_focused_text_snippet(document, true)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (0, 6));
        assert!(context.replace_focused_text(document, "x").unwrap());
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "x + x + blue + blue"
        );
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        assert!(
            context
                .advance_focused_text_snippet(document, false)
                .unwrap()
        );
        let end = context.world().text_input(node).unwrap().value.len();
        assert_eq!(selection_of(&context, node), (end, end));
    }
    #[test]
    fn completion_transaction_rejects_stale_source_caret_and_overlapping_edits() {
        for (source, caret, additional) in [
            ("old", 2, vec![]),
            ("fo", 1, vec![]),
            ("fo", 2, vec![(0..1, "x".into())]),
        ] {
            let candidate = TextCompletion::new("label", "fn").edit(crate::TextCompletionEdit {
                snippet: None,
                variables: Default::default(),
                source: Arc::from(source),
                caret,
                range: 0..2,
                text: "foo()".into(),
                additional,
                stops: vec![],
            });
            let (mut context, document, area, node) =
                completion_editor("fo", Arc::from([candidate]));
            caret_to(&mut context, area, 2);
            assert!(
                !context
                    .accept_focused_text_completion(document, None)
                    .unwrap()
            );
            assert_eq!(context.world().text_input(node).unwrap().value, "fo");
        }
    }
}

#[cfg(test)]
mod minimap_tests {
    use crate::{
        AppContext, DocumentId, Entity, MeasureTextShaper, MutationQueue, StableNodeId, TextArea,
    };
    use std::time::Duration;

    /// 30 行 minimap 编辑器：行高 10、布局 200×100，已布局并 shape，
    /// 且已聚焦。
    fn minimap_editor() -> (AppContext, DocumentId, Entity<TextArea>, StableNodeId) {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let value = (0..30)
            .map(|index| format!("line{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut view = TextArea::new(value).minimap(true);
        {
            use nana_ui_core::{LengthSpec, LineHeightSpec};
            let layout = std::sync::Arc::make_mut(&mut view.style.layout);
            layout.width = Some(LengthSpec::Px(200.0));
            layout.height = Some(LengthSpec::Px(100.0));
            layout.line_height = Some(LineHeightSpec::Absolute(10.0));
            // Narrow enough glyphs that no line wraps beside the minimap: the
            // editor keeps its 30 line breaks, and 30 lines are 300 px.
            layout.font_size = Some(8.0);
            layout.padding_left = Some(LengthSpec::Px(0.0));
            layout.padding_right = Some(LengthSpec::Px(0.0));
            layout.padding_top = Some(LengthSpec::Px(0.0));
            layout.padding_bottom = Some(LengthSpec::Px(0.0));
        }
        let area = context.create_component(document, view).unwrap();
        let node = area.stable_id();
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            node,
            crate::LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            },
        );
        context.commit_mutations(mutations).unwrap();
        context.compat_world_mut().resolve_styles(&[node]).unwrap();
        context
            .compat_world_mut()
            .shape_text(&[node], &mut MeasureTextShaper)
            .unwrap();
        context.focus_node(document, node).unwrap();
        (context, document, area, node)
    }

    fn minimap_panel(context: &AppContext, node: StableNodeId) -> crate::LayoutBox {
        match context.world().component_geometry(node).unwrap() {
            crate::ComponentGeometry::TextInput {
                minimap: Some(minimap),
                ..
            } => minimap.panel,
            other => panic!("expected minimap geometry, got {other:?}"),
        }
    }

    fn press(
        context: &mut AppContext,
        document: DocumentId,
        node: StableNodeId,
        x: f32,
        y: f32,
    ) -> bool {
        context
            .text_editor_pointer_press(
                document,
                node,
                1,
                x,
                y,
                false,
                false,
                Duration::ZERO,
                &mut MeasureTextShaper,
            )
            .unwrap()
    }

    fn scroll_y_of(context: &AppContext, node: StableNodeId) -> f32 {
        context.world().scroll_offset(node).unwrap().y
    }

    /// 点击条内 `offset_y`（面板内偏移）后的期望滚动：命中行居中并钳到
    /// 文档范围（行高 10，30 行；内容盒从世界侧读取，含边框）。
    fn expected_scroll(context: &AppContext, node: StableNodeId, offset_y: f32) -> f32 {
        let (content, _) = context.world().text_input_pointer_context(node).unwrap();
        let line = (offset_y / 2.0).floor();
        let max_scroll = (30.0 * 10.0 - content.height).max(0.0);
        (line * 10.0 + 5.0 - content.height / 2.0).clamp(0.0, max_scroll)
    }

    #[test]
    fn minimap_press_scrolls_viewport_without_moving_caret_or_selection() {
        let (mut context, document, area, node) = minimap_editor();
        let state = context.world().text_input(node).unwrap();
        let selection_before = state.selection;
        let additional_before = state.additional_selections.to_vec();

        // 条内中部点击：点击行（面板内 50px → 第 25 行）居中。
        let panel = minimap_panel(&context, node);
        assert!(press(
            &mut context,
            document,
            node,
            panel.x + 32.0,
            panel.y + 50.0
        ));
        let expected = expected_scroll(&context, node, 50.0);
        assert_eq!(scroll_y_of(&context, node), expected);
        // 组件字段同步（投影权威），下一帧不被改写回去。
        assert_eq!(
            context
                .read(area, |view: &TextArea| view.scroll_offset.y)
                .unwrap(),
            expected
        );
        // 按下被消费：光标与选区纹丝不动。
        let state = context.world().text_input(node).unwrap();
        assert_eq!(state.selection, selection_before);
        assert_eq!(state.additional_selections, additional_before);
    }

    #[test]
    fn minimap_drag_follows_pointer_until_release() {
        let (mut context, document, _area, node) = minimap_editor();
        let panel = minimap_panel(&context, node);
        assert!(press(
            &mut context,
            document,
            node,
            panel.x + 32.0,
            panel.y + 10.0
        ));
        assert_eq!(
            scroll_y_of(&context, node),
            expected_scroll(&context, node, 10.0)
        );

        // 按住拖动：视口连续跟随换算（面板内 50px → 第 25 行居中）。
        assert!(
            context
                .text_editor_pointer_drag(
                    document,
                    1,
                    panel.x + 32.0,
                    panel.y + 50.0,
                    &mut MeasureTextShaper
                )
                .unwrap()
        );
        assert_eq!(
            scroll_y_of(&context, node),
            expected_scroll(&context, node, 50.0)
        );

        // 释放后指针移动不再跟随。
        context.text_editor_pointer_release(1);
        assert!(
            !context
                .text_editor_pointer_drag(
                    document,
                    1,
                    panel.x + 32.0,
                    panel.y + 70.0,
                    &mut MeasureTextShaper
                )
                .unwrap()
        );
        assert_eq!(
            scroll_y_of(&context, node),
            expected_scroll(&context, node, 50.0)
        );
    }

    #[test]
    fn minimap_click_outside_strip_places_caret_normally() {
        let (mut context, document, _area, node) = minimap_editor();
        let panel = minimap_panel(&context, node);
        // 条外（内容区左侧）点击：常规 caret 落点，非 minimap 消费语义。
        assert!(press(&mut context, document, node, 30.0, panel.y + 10.0));
        let state = context.world().text_input(node).unwrap();
        assert_ne!(
            state.selection.focus, 0,
            "plain press inside text must move the caret"
        );
    }

    #[test]
    fn minimap_scroll_survives_caret_reveal_until_caret_moves() {
        let (mut context, document, _area, node) = minimap_editor();
        let panel = minimap_panel(&context, node);
        // 光标在第 0 行；点击条内底部 → 视口滚到文档尾部（钳到 max）。
        assert!(press(
            &mut context,
            document,
            node,
            panel.x + 32.0,
            panel.y + 70.0
        ));
        let navigated = scroll_y_of(&context, node);
        assert!(navigated > 150.0, "click near the bottom must scroll far");

        // 重跑几何：视口不回弹到光标处（视口钉住生效）——文本区随滚动
        // 停在导航位置。
        let (content, _) = context.world().text_input_pointer_context(node).unwrap();
        let extracted = &context.world().extract_nodes(&[node])[0];
        let crate::ComponentGeometry::TextInput { text, .. } =
            extracted.component_geometry.as_deref().unwrap()
        else {
            panic!("expected text input geometry");
        };
        assert!((text.bounds.y - (content.y - navigated)).abs() < 0.01);

        // 光标移动（选区写入）：钉住失效，reveal 恢复权威——视口回到光标。
        assert!(
            context
                .select_focused_text_range(document, 0, "line0".len())
                .unwrap()
        );
        context
            .compat_world_mut()
            .shape_text(&[node], &mut MeasureTextShaper)
            .unwrap();
        let extracted = &context.world().extract_nodes(&[node])[0];
        let crate::ComponentGeometry::TextInput { text, .. } =
            extracted.component_geometry.as_deref().unwrap()
        else {
            panic!("expected text input geometry");
        };
        // 光标在第 0 行（y=0），reveal 后视口回顶部：文本区 y 回到内容顶。
        assert!((text.bounds.y - content.y).abs() < 0.01);
    }

    /// The scroll the editor is drawn at: its text origin above the content
    /// box.
    fn drawn_scroll_y(context: &AppContext, node: StableNodeId) -> f32 {
        let (content, _) = context.world().text_input_pointer_context(node).unwrap();
        let extracted = &context.world().extract_nodes(&[node])[0];
        let crate::ComponentGeometry::TextInput { text, .. } =
            extracted.component_geometry.as_deref().unwrap()
        else {
            panic!("expected text input geometry");
        };
        content.y - text.bounds.y
    }

    fn move_caret(
        context: &mut AppContext,
        area: Entity<TextArea>,
        node: StableNodeId,
        offset: usize,
    ) {
        context
            .update_component(area, |area, _| {
                area.state.selection = crate::TextSelection::caret(offset);
                true
            })
            .unwrap();
        context
            .compat_world_mut()
            .shape_text(&[node], &mut MeasureTextShaper)
            .unwrap();
    }

    /// The caret reveal scrolls a focused editor to its caret; the editor
    /// reports that scroll, so a wheel carries on from what is on screen
    /// instead of from the offset set before the caret moved.
    #[test]
    fn a_wheel_after_the_caret_reveal_scrolls_from_what_is_drawn() {
        let (mut context, _document, area, node) = minimap_editor();
        let end = context.world().text_input(node).unwrap().value.len();
        move_caret(&mut context, area, node, end);
        // 30 lines of 10 px in a 100 px box: the last line is revealed at the
        // bottom, so the editor is drawn scrolled to the end.
        let revealed = drawn_scroll_y(&context, node);
        assert!(
            revealed > 150.0,
            "caret reveal scrolls to the end: {revealed}"
        );
        assert_eq!(scroll_y_of(&context, node), revealed);

        assert!(
            context
                .scroll_node_by(node, crate::ScrollOffset { x: 0.0, y: -10.0 })
                .unwrap()
        );
        assert_eq!(scroll_y_of(&context, node), revealed - 10.0);
        assert_eq!(drawn_scroll_y(&context, node), revealed - 10.0);
    }

    /// Shortening the value leaves the request past the new end; the editor
    /// is drawn clamped, and a wheel moves from there on its first tick.
    #[test]
    fn a_wheel_after_the_value_shrinks_moves_on_its_first_tick() {
        let (mut context, _document, area, node) = minimap_editor();
        move_caret(&mut context, area, node, 0);
        assert_eq!(scroll_y_of(&context, node), 0.0);
        assert!(
            context
                .scroll_node_by(node, crate::ScrollOffset { x: 0.0, y: 1_000.0 })
                .unwrap()
        );
        let bottom = scroll_y_of(&context, node);
        assert!(bottom > 150.0, "scrolled to the end: {bottom}");

        // 15 lines of 10 px: 50 px past a 100 px box. The caret stays on the
        // first line, so the wheel's pin holds.
        let shorter = (0..15)
            .map(|index| format!("line{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        context
            .update_component(area, |area, _| {
                area.state.value = shorter.into();
                true
            })
            .unwrap();
        context
            .compat_world_mut()
            .shape_text(&[node], &mut MeasureTextShaper)
            .unwrap();
        let end = scroll_y_of(&context, node);
        assert_eq!(drawn_scroll_y(&context, node), end);
        assert!(end < bottom, "drawn clamped to the shorter value: {end}");

        assert!(
            context
                .scroll_node_by(node, crate::ScrollOffset { x: 0.0, y: -10.0 })
                .unwrap()
        );
        assert_eq!(drawn_scroll_y(&context, node), end - 10.0);
    }
}

#[cfg(test)]
mod find_scope_smart_select_tests {
    use super::*;
    use crate::{AppContext, DocumentId, Entity, StableNodeId, TextArea};

    fn editor(value: &str) -> (AppContext, DocumentId, Entity<TextArea>, StableNodeId) {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let area = context
            .create_component(document, TextArea::new(value).code_editor(true))
            .unwrap();
        let node = area.stable_id();
        context.focus_node(document, node).unwrap();
        (context, document, area, node)
    }

    fn set_selection(
        context: &mut AppContext,
        area: Entity<TextArea>,
        selection: TextSelection,
        additional: Vec<TextSelection>,
    ) {
        context
            .update_component(area, |area, _| {
                area.state.selection = selection;
                area.state.additional_selections = additional;
                true
            })
            .unwrap();
    }

    fn selection_span(context: &AppContext, node: StableNodeId) -> (usize, usize) {
        let state = context.world().text_input(node).unwrap();
        let span = state.selection.ordered();
        (span.start, span.end)
    }

    #[test]
    fn find_in_selection_scopes_navigation_and_replace_all() {
        //            0123456789...
        let value = "ab cd ab cd ab";
        let (mut context, document, area, node) = editor(value);
        let options = TextSearchOptions::default();

        // 无选区时 Selection 退化为全局（与 Document 一致）。
        set_selection(&mut context, area, TextSelection::caret(0), Vec::new());
        assert!(
            context
                .find_next_focused_text_match(document, "ab", options, TextFindScope::Selection)
                .unwrap()
        );
        assert_eq!(selection_span(&context, node), (0, 2));

        // 选区 3..12（"cd ab cd"）：范围内只有一个 "ab"，导航反复循环在
        // 6..8，不越界到边界外或范围外的命中。
        set_selection(&mut context, area, TextSelection::new(3, 12), Vec::new());
        assert!(
            context
                .find_next_focused_text_match(document, "ab", options, TextFindScope::Selection)
                .unwrap()
        );
        assert_eq!(selection_span(&context, node), (6, 8));
        // 命中已是当前选区：无移动，返回 false 但选区保持。
        assert!(
            !context
                .find_next_focused_text_match(document, "ab", options, TextFindScope::Selection)
                .unwrap()
        );
        assert_eq!(selection_span(&context, node), (6, 8));
        // 范围内唯一的命中已被选中：再次导航原地不动（返回 false），循环
        // 不越界；上一个同样停在同一命中。
        assert!(
            !context
                .find_next_focused_text_match(document, "ab", options, TextFindScope::Selection)
                .unwrap()
        );
        assert_eq!(selection_span(&context, node), (6, 8));
        assert!(
            !context
                .find_previous_focused_text_match(document, "ab", options, TextFindScope::Selection)
                .unwrap()
        );
        assert_eq!(selection_span(&context, node), (6, 8));

        // Document 范围不受限：从 6..8 起下一个命中是 12..14。
        assert!(
            context
                .find_next_focused_text_match(document, "ab", options, TextFindScope::Document)
                .unwrap()
        );
        assert_eq!(selection_span(&context, node), (12, 14));

        // 替换全部限定在选区内：只有范围内的两个 "cd" 被替换。
        set_selection(&mut context, area, TextSelection::new(3, 12), Vec::new());
        let replaced = context
            .replace_all_focused_text_matches(
                document,
                "cd",
                options,
                "X",
                TextFindScope::Selection,
                false,
            )
            .unwrap();
        assert_eq!(replaced, 2);
        assert_eq!(
            context.world().text_input(node).unwrap().value,
            "ab X ab X ab"
        );
    }

    #[test]
    fn replace_all_preserves_case_shapes() {
        let (mut context, document, _area, node) = editor("foo FOO Foo foo");
        let replaced = context
            .replace_all_focused_text_matches(
                document,
                "foo",
                TextSearchOptions::default(),
                "bar",
                TextFindScope::Document,
                true,
            )
            .unwrap();
        assert_eq!(replaced, 4);
        let state = context.world().text_input(node).unwrap();
        assert_eq!(state.value, "bar BAR Bar bar");
        // 第一个替换文本被选中。
        assert_eq!(selection_span(&context, node), (0, 3));
    }

    #[test]
    fn expand_and_shrink_selection_layers() {
        let value = "fn a() { greeting }";
        let (mut context, document, area, node) = editor(value);
        set_selection(&mut context, area, TextSelection::caret(13), Vec::new());

        // 词 → 括号内部 → 全文档。
        assert!(context.expand_focused_text_selection(document).unwrap());
        assert_eq!(selection_span(&context, node), (9, 17));
        assert!(context.expand_focused_text_selection(document).unwrap());
        assert_eq!(selection_span(&context, node), (8, 18));
        assert!(context.expand_focused_text_selection(document).unwrap());
        assert_eq!(selection_span(&context, node), (0, value.len()));
        // 已在全文档：不能再扩展。
        assert!(!context.expand_focused_text_selection(document).unwrap());

        // 收缩按扩展历史逐层回退，最后无可收缩。
        assert!(context.shrink_focused_text_selection(document).unwrap());
        assert_eq!(selection_span(&context, node), (8, 18));
        assert!(context.shrink_focused_text_selection(document).unwrap());
        assert_eq!(selection_span(&context, node), (9, 17));
        assert!(context.shrink_focused_text_selection(document).unwrap());
        assert_eq!(selection_span(&context, node), (13, 13));
        assert!(!context.shrink_focused_text_selection(document).unwrap());

        // 编辑后历史失效：扩展一次再改值，收缩拒绝。
        assert!(context.expand_focused_text_selection(document).unwrap());
        context
            .update_component(area, |area, _| {
                area.state.value = "fn a() { greeting!! }".into();
                true
            })
            .unwrap();
        assert!(!context.shrink_focused_text_selection(document).unwrap());
    }

    #[test]
    fn expand_selection_is_independent_per_cursor() {
        // "(aa bb) x (cc dd)"：两个光标分别落在两个括号组的词里。
        let value = "(aa bb) x (cc dd)";
        let (mut context, document, area, node) = editor(value);
        set_selection(
            &mut context,
            area,
            TextSelection::caret(1),
            vec![TextSelection::caret(14)],
        );

        let spans = |context: &AppContext, node: StableNodeId| -> Vec<(usize, usize)> {
            let state = context.world().text_input(node).unwrap();
            state
                .selections()
                .iter()
                .map(|s| (s.ordered().start, s.ordered().end))
                .collect()
        };

        // 第一次扩展：各自选中自己的词。
        assert!(context.expand_focused_text_selection(document).unwrap());
        assert_eq!(spans(&context, node), vec![(1, 3), (14, 16)]);
        // 第二次扩展：各自落到自己所在括号对的内部。
        assert!(context.expand_focused_text_selection(document).unwrap());
        assert_eq!(spans(&context, node), vec![(1, 6), (11, 16)]);
        // 第三次扩展：都到全文档，融合为一个选区。
        assert!(context.expand_focused_text_selection(document).unwrap());
        assert_eq!(spans(&context, node), vec![(0, value.len())]);
        // 收缩逐层回退：先回到括号内部双光标，再回词，再回光标。
        assert!(context.shrink_focused_text_selection(document).unwrap());
        assert_eq!(spans(&context, node), vec![(1, 6), (11, 16)]);
        assert!(context.shrink_focused_text_selection(document).unwrap());
        assert_eq!(spans(&context, node), vec![(1, 3), (14, 16)]);
        assert!(context.shrink_focused_text_selection(document).unwrap());
        // 历史恢复的是扩展前的双光标集合。
        let state = context.world().text_input(node).unwrap();
        assert_eq!(state.additional_selections, vec![TextSelection::caret(14)]);
        assert_eq!(selection_span(&context, node), (1, 1));
        assert!(!context.shrink_focused_text_selection(document).unwrap());
    }
}

#[cfg(test)]
mod atom_tests {
    use super::*;
    use crate::{AppContext, DocumentId, TextAtomSpan};
    use std::sync::Arc;

    fn focused_editor(value: &str) -> (AppContext, DocumentId, Entity<TextArea>, StableNodeId) {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let area = context
            .create_component(document, TextArea::new(value))
            .unwrap();
        let node = area.stable_id();
        context.focus_node(document, node).unwrap();
        (context, document, area, node)
    }

    fn selection_of(context: &AppContext, node: StableNodeId) -> (usize, usize) {
        let state = context.world().text_input(node).unwrap();
        (state.selection.anchor, state.selection.focus)
    }

    #[test]
    fn delete_and_caret_treat_atom_spans_as_one_unit() {
        let value = "ab[chip]cd";
        let atom = TextAtomSpan::new(2, 8);
        let (mut context, document, area, node) = focused_editor(value);
        context
            .update_component(area, |area, _| {
                area.atom_spans = Arc::from([atom.clone()]);
                area.state.selection = TextSelection::caret(8);
            })
            .unwrap();
        assert!(
            context
                .delete_focused_text(document, TextDeleteKind::Backward)
                .unwrap()
        );
        assert_eq!(context.world().text_input(node).unwrap().value, "abcd");
        assert_eq!(selection_of(&context, node), (2, 2));

        context
            .update_component(area, |area, _| {
                area.state.replace_value(value);
                area.atom_spans = Arc::from([atom.clone()]);
                area.state.selection = TextSelection::caret(2);
            })
            .unwrap();
        assert!(
            context
                .move_focused_text_caret(document, TextCaretIntent::Right, false, None)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (8, 8));
        assert!(
            context
                .move_focused_text_caret(document, TextCaretIntent::Left, false, None)
                .unwrap()
        );
        assert_eq!(selection_of(&context, node), (2, 2));

        context
            .update_component(area, |area, _| {
                area.state.selection = TextSelection::caret(5);
            })
            .unwrap();
        assert!(context.replace_focused_text(document, "x").unwrap());
        assert_eq!(context.world().text_input(node).unwrap().value, "abxcd");
    }

    #[test]
    fn cutting_into_an_atom_puts_on_the_pasteboard_what_it_deletes() {
        let value = "Hi [bob]!";
        let (mut context, document, area, node) = focused_editor(value);
        context
            .update_component(area, |area, _| {
                area.atom_spans = Arc::from([TextAtomSpan::new(3, 8)]);
                area.state.selection = TextSelection::new(0, 6);
            })
            .unwrap();
        let cut = context.cut_focused_text(document).unwrap();
        assert_eq!(context.world().text_input(node).unwrap().value, "!");
        assert_eq!(
            cut.as_deref(),
            Some("Hi [bob]"),
            "the whole chip, as deleted"
        );
    }

    #[test]
    fn left_and_right_collapse_onto_a_selection_edge_that_touches_an_atom() {
        // A selection ending on a chip: collapsing lands on the selection's
        // edge, not where a caret stepping off the chip would.
        let value = "Hi [bob]";
        let atom = TextAtomSpan::new(3, 8);
        for (anchor, focus, intent, landing) in [
            (0, 8, TextCaretIntent::Left, 0),
            (8, 0, TextCaretIntent::Right, 8),
        ] {
            let (mut context, document, area, node) = focused_editor(value);
            context
                .update_component(area, |area, _| {
                    area.atom_spans = Arc::from([atom.clone()]);
                    area.state.selection = TextSelection::new(anchor, focus);
                })
                .unwrap();
            assert!(
                context
                    .move_focused_text_caret(document, intent, false, None)
                    .unwrap()
            );
            assert_eq!(
                selection_of(&context, node),
                (landing, landing),
                "{intent:?}"
            );
        }
    }

    #[test]
    fn close_focused_text_atom_deletes_the_labeled_unit() {
        let value = "ab[chip]cd";
        let atom = TextAtomSpan::new(2, 8).label("chip").token("file-1");
        let (mut context, document, area, node) = focused_editor(value);
        let closed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = closed.clone();
        context
            .on(area, move |_, event: &crate::TextAtomClosed, _| {
                sink.lock().unwrap().push(event.token.to_string());
            })
            .unwrap();
        context
            .update_component(area, |area, _| {
                area.atom_spans = Arc::from([atom]);
                area.state.selection = TextSelection::caret(8);
            })
            .unwrap();
        assert!(context.close_focused_text_atom(document, "file-1").unwrap());
        assert_eq!(context.world().text_input(node).unwrap().value, "abcd");
        assert_eq!(*closed.lock().unwrap(), vec!["file-1".to_string()]);
    }
}

#[derive(Default)]
pub(super) struct TextEditSession {
    /// Live text drag-selection: pointer id, node, and the anchor offset.
    pub(super) text_pointer_drag: Option<(u64, StableNodeId, usize)>,
    /// Document-level `user-select` drag: pointer, document, node, anchor.
    pub(super) document_select_drag: Option<(u64, DocumentId, StableNodeId, usize)>,
    /// Live minimap navigation drag: pointer id, node, and editor kind.
    pub(super) text_minimap_drag: Option<(u64, StableNodeId, TextEditorKind)>,
    /// 拖拽移动选中文本的状态机（多行编辑器、单选区、非 IME；按下落在
    /// 主选区内部时进入，超过阈值激活，释放执行移动/复制或回落为点击）。
    pub(super) text_selection_drag: Option<crate::framework::text_edit::TextSelectionDrag>,
    /// Last press inside a text editor for double/triple click counting.
    pub(super) text_pointer_click: Option<super::TextPointerClick>,
    /// Horizontal goal column retained across chained vertical moves.
    pub(super) caret_goal_x: Option<(StableNodeId, f32)>,
    /// Expand/shrink selection record: prior selection sets per node, valid
    /// only while the editor's value is unchanged (any edit clears it).
    pub(super) selection_expansions: Option<(
        StableNodeId,
        Vec<(crate::TextSelection, Vec<crate::TextSelection>)>,
        String,
    )>,
}

#[cfg(test)]
mod read_only_tests {
    use super::*;
    fn editor(value: &str) -> (AppContext, DocumentId, Entity<TextArea>) {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let area = context
            .create_component(document, TextArea::new(value).code_editor(true))
            .unwrap();
        context.focus_node(document, area.stable_id()).unwrap();
        (context, document, area)
    }
    #[test]
    fn read_only_text_area_keeps_focus_find_selection_copy_and_other_view_independence() {
        let (mut context, document, area) = editor("猫 and 猫");
        let other = context
            .create_component(document, TextArea::new("猫 and 猫").read_only(true))
            .unwrap();
        context
            .update_component(area, |area, _| area.read_only = true)
            .unwrap();
        assert_eq!(context.world().focused(document), Some(area.stable_id()));
        assert!(
            !context
                .world()
                .accessibility(area.stable_id())
                .unwrap()
                .editable
        );
        assert!(
            !context
                .world()
                .accessibility(area.stable_id())
                .unwrap()
                .disabled
        );
        context
            .move_focused_text_caret(document, TextCaretIntent::DocStart, false, None)
            .unwrap();
        assert!(
            context
                .find_next_focused_text_match(
                    document,
                    "猫",
                    TextSearchOptions::default(),
                    TextFindScope::Document
                )
                .unwrap()
        );
        assert_eq!(
            context.focused_selected_text(document).as_deref(),
            Some("猫")
        );
        assert_eq!(
            context.read(area, |area| area.state.selection).unwrap(),
            TextSelection::new(0, 3)
        );
        assert_eq!(
            context.read(other, |area| area.state.selection).unwrap(),
            TextSelection::caret("猫 and 猫".len())
        );
        assert!(
            context
                .find_previous_focused_text_match(
                    document,
                    "猫",
                    TextSearchOptions::default(),
                    TextFindScope::Document
                )
                .unwrap()
        );
        assert!(context.select_all_focused_text(document).unwrap());
        assert_eq!(
            context.focused_selected_text(document).as_deref(),
            Some("猫 and 猫")
        );
        assert!(!context.replace_text_area_selection(area, "new").unwrap());
        assert!(
            !context
                .delete_focused_text(document, TextDeleteKind::Backward)
                .unwrap()
        );
        assert!(!context.insert_focused_text_newline(document).unwrap());
        assert!(context.cut_focused_text(document).unwrap().is_none());
        assert!(
            !context
                .replace_focused_text_match(
                    document,
                    "猫",
                    TextSearchOptions::default(),
                    "dog",
                    false
                )
                .unwrap()
        );
        assert_eq!(
            context
                .replace_all_focused_text_matches(
                    document,
                    "猫",
                    TextSearchOptions::default(),
                    "dog",
                    TextFindScope::Document,
                    false
                )
                .unwrap(),
            0
        );
        assert_eq!(
            context.read(area, |area| area.state.value.clone()).unwrap(),
            "猫 and 猫"
        );
        context.focus_node(document, other.stable_id()).unwrap();
        assert_eq!(
            context.read(area, |area| area.state.selection).unwrap(),
            TextSelection::new(0, "猫 and 猫".len())
        );
    }
    #[test]
    fn entering_read_only_cancels_preedit_and_pending_text_drop_without_changing_value() {
        let (mut context, document, area) = editor("alpha beta");
        assert!(
            context
                .set_ime_preedit(document, "候選".into(), Some((0, 6)))
                .unwrap()
        );
        context.text_edit.text_selection_drag = Some(TextSelectionDrag {
            pointer_id: 1,
            node: area.stable_id(),
            kind: TextEditorKind::Area,
            press: (0.0, 0.0),
            source: (0, 5),
            active: true,
            copy: false,
            target: Some(10),
        });
        context
            .update_component(area, |area, _| area.read_only = true)
            .unwrap();
        assert!(context.world().ime(area.stable_id()).is_none());
        assert_eq!(context.world().focused(document), Some(area.stable_id()));
        assert!(
            !context
                .set_ime_preedit(document, "new".into(), None)
                .unwrap()
        );
        assert!(!context.commit_ime(document, "候選").unwrap());
        assert!(!context.delete_ime_surrounding(document, 1, 1).unwrap());
        assert!(
            !context
                .text_editor_selection_drop(document, 1, 100.0, 10.0, &mut crate::MeasureTextShaper)
                .unwrap()
        );
        assert!(context.text_edit.text_selection_drag.is_none());
        assert_eq!(
            context.read(area, |area| area.state.value.clone()).unwrap(),
            "alpha beta"
        );
        context
            .update_component(area, |area, _| area.read_only = false)
            .unwrap();
        assert!(context.commit_ime(document, "!").unwrap());
        assert_eq!(
            context.read(area, |area| area.state.value.clone()).unwrap(),
            "alpha beta!"
        );
    }
    #[test]
    fn disabled_text_area_still_loses_focus_and_rejects_selection_and_input() {
        let (mut context, document, area) = editor("alpha");
        context
            .update_component(area, |area, _| {
                area.read_only = true;
                area.disabled = true;
            })
            .unwrap();
        assert_ne!(context.world().focused(document), Some(area.stable_id()));
        context.focus_node(document, area.stable_id()).unwrap();
        assert_ne!(context.world().focused(document), Some(area.stable_id()));
        assert!(
            !context
                .find_next_focused_text_match(
                    document,
                    "alpha",
                    TextSearchOptions::default(),
                    TextFindScope::Document
                )
                .unwrap()
        );
        assert!(!context.select_all_focused_text(document).unwrap());
        assert!(!context.replace_text_area_selection(area, "new").unwrap());
        assert_eq!(
            context.read(area, |area| area.state.value.clone()).unwrap(),
            "alpha"
        );
    }
}

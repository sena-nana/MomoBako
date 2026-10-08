//! What routing keeps the host showing: the cursor under the pointer and the
//! focused editor's text-input state. Both are latest-value slots. Each is
//! derived from the world after an event, compared with what the host was
//! last told through a cheap key, and sent only when it differs.

use std::time::Duration;

use nana_ui_core::{CursorSpec, LogicalRect};
use nana_ui_input::{
    CursorIcon, HostServices, InputSourceId, SurroundingText, TextInputContext, TextInputPurpose,
};

use crate::{AppContext, DocumentId, LayoutBox, StableNodeId};

/// How often a moving pointer re-probes for resize handles near it. A probe
/// off every handle asks each split, dock and workspace for its handles, and
/// moves arrive faster than frames; the cursor lagging one frame is not
/// visible. Presses, releases and a refresh after a frame always probe.
const HANDLE_PROBE_INTERVAL: Duration = Duration::from_millis(8);

/// What decides the text-input state, without building it: comparing two of
/// these tells whether the host must hear about a change.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TextInputKey {
    Terminal {
        caret: Option<LayoutBox>,
    },
    Editor {
        node: StableNodeId,
        revisions: nana_text::EditRevisions,
        caret: Option<LayoutBox>,
        purpose: TextInputPurpose,
    },
}

/// The slots of one source.
#[derive(Debug, Default)]
pub(super) struct SourceEffects {
    /// The cursor the host was last given; `None` when the host shows its
    /// own default (before the first pointer event and after a leave).
    cursor: Option<CursorIcon>,
    /// The resize handle the last probe found, and when it ran.
    handle: Option<StableNodeId>,
    probed_at: Option<Duration>,
    /// What the host's text input was last set from; `None` before the
    /// first time.
    text_input: Option<Option<TextInputKey>>,
}

impl SourceEffects {
    /// The host restored its own cursor.
    pub(super) fn forget_cursor(&mut self) {
        self.cursor = None;
        self.probed_at = None;
    }

    fn probe_due(&mut self, now: Option<Duration>) -> bool {
        let Some(now) = now else {
            self.probed_at = None;
            return true;
        };
        if self
            .probed_at
            .is_some_and(|last| now.saturating_sub(last) < HANDLE_PROBE_INTERVAL)
        {
            return false;
        }
        self.probed_at = Some(now);
        true
    }
}

impl AppContext {
    /// Send the cursor for a pointer at `(x, y)` over `target` if it changed.
    /// `throttle` is the event time of a move, whose handle probe may reuse
    /// the last one; `None` probes now.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sync_cursor(
        &mut self,
        source: InputSourceId,
        document: DocumentId,
        x: f32,
        y: f32,
        target: Option<StableNodeId>,
        throttle: Option<Duration>,
        services: &mut dyn HostServices,
    ) {
        let Some(state) = self.input.sources.get_mut(&source) else {
            return;
        };
        let handle = if state.effects.probe_due(throttle) {
            let [split, dock, workspace] = self.reachable_handle_near(document, x, y, target);
            let handle = split.or(dock).or(workspace);
            if let Some(state) = self.input.sources.get_mut(&source) {
                state.effects.handle = handle;
            }
            handle
        } else {
            state.effects.handle
        };
        let icon = self.cursor_icon(handle, target);
        let Some(state) = self.input.sources.get_mut(&source) else {
            return;
        };
        if state.effects.cursor == Some(icon) {
            return;
        }
        // Before any pointer event the host already shows its default.
        let initial_default = state.effects.cursor.is_none() && icon == CursorIcon::Default;
        state.effects.cursor = Some(icon);
        if !initial_default {
            self.input.counters.cursor_updates += 1;
            services.set_cursor(icon);
        }
    }

    /// The cursor for a pointer over `target`, near `handle`: a resize
    /// handle's own, else the node's CSS cursor, else a text beam over a text
    /// field, else the default.
    fn cursor_icon(
        &self,
        handle: Option<StableNodeId>,
        target: Option<StableNodeId>,
    ) -> CursorIcon {
        if let Some(bounds) = handle.and_then(|handle| self.world.layout_box(handle)) {
            return if bounds.width <= bounds.height {
                CursorIcon::EwResize
            } else {
                CursorIcon::NsResize
            };
        }
        let css = target
            .and_then(|node| self.world.computed_style(node))
            .and_then(|style| style.cursor_specified.then_some(style.cursor));
        match css {
            Some(spec) => cursor_icon_for(spec),
            None if target.is_some_and(|node| self.world.text_input(node).is_some()) => {
                CursorIcon::Text
            }
            None => CursorIcon::Default,
        }
    }

    /// Send the text-input state if it changed, or unconditionally when
    /// `force` (the platform dropped it, as it does when a window loses
    /// focus).
    pub(super) fn sync_text_input(
        &mut self,
        source: InputSourceId,
        document: DocumentId,
        force: bool,
        services: &mut dyn HostServices,
    ) {
        let key = self.text_input_key(document);
        let Some(state) = self.input.sources.get_mut(&source) else {
            return;
        };
        if !force && state.effects.text_input == Some(key) {
            return;
        }
        state.effects.text_input = Some(key);
        self.input.counters.text_input_updates += 1;
        let context = key.and_then(|_| self.text_input_context(document));
        services.set_text_input(context.as_ref());
    }

    fn text_input_key(&self, document: DocumentId) -> Option<TextInputKey> {
        if self.terminal_accepts_input(document) {
            return Some(TextInputKey::Terminal {
                caret: self.terminal_caret_bounds(document),
            });
        }
        let (node, view) = self.editable_focused_text_input(document)?;
        Some(TextInputKey::Editor {
            node,
            revisions: view.session().revisions(),
            caret: self.text_input_caret(node),
            purpose: self.text_input_purpose(node),
        })
    }

    /// The text-input state of `document`'s focused editor or terminal:
    /// purpose, where the IME anchors its candidates, and at most
    /// [`SurroundingText::MAX_BYTES`] of the text around the selection.
    pub(super) fn text_input_context(&self, document: DocumentId) -> Option<TextInputContext> {
        if self.terminal_accepts_input(document) {
            return Some(TextInputContext {
                purpose: TextInputPurpose::Terminal,
                cursor_area: self.terminal_caret_bounds(document).map(logical_rect),
                surrounding: None,
            });
        }
        let (node, view) = self.editable_focused_text_input(document)?;
        let purpose = self.text_input_purpose(node);
        let surrounding = (purpose != TextInputPurpose::Password)
            .then(|| surrounding_window(view.value, view.selection.focus, view.selection.anchor))
            .flatten();
        Some(TextInputContext {
            purpose,
            cursor_area: self.text_input_caret(node).map(logical_rect),
            surrounding,
        })
    }

    /// The focused text input when it takes typing.
    pub(super) fn editable_focused_text_input(
        &self,
        document: DocumentId,
    ) -> Option<(StableNodeId, crate::TextInputView<'_>)> {
        self.world.focused_text_input(document).filter(|(node, _)| {
            self.world
                .accessibility(*node)
                .is_some_and(|state| state.editable)
        })
    }

    fn text_input_caret(&self, node: StableNodeId) -> Option<LayoutBox> {
        match self.world.component_geometry(node) {
            Some(crate::ComponentGeometry::TextInput {
                caret: Some(caret), ..
            }) => Some(caret),
            _ => self.world.layout_box(node),
        }
    }

    fn text_input_purpose(&self, node: StableNodeId) -> TextInputPurpose {
        if matches!(
            self.world.standard_visual(node),
            Some(crate::StandardVisual::TextInput { secure: true, .. })
        ) {
            TextInputPurpose::Password
        } else {
            TextInputPurpose::Normal
        }
    }
}

fn logical_rect(bounds: LayoutBox) -> LogicalRect {
    LogicalRect::new(bounds.x, bounds.y, bounds.width, bounds.height)
}

fn cursor_icon_for(spec: CursorSpec) -> CursorIcon {
    match spec {
        CursorSpec::Default => CursorIcon::Default,
        CursorSpec::Pointer => CursorIcon::Pointer,
        CursorSpec::Text => CursorIcon::Text,
        CursorSpec::Move => CursorIcon::Move,
        CursorSpec::Grab => CursorIcon::Grab,
        CursorSpec::Grabbing => CursorIcon::Grabbing,
        CursorSpec::NotAllowed => CursorIcon::NotAllowed,
        CursorSpec::Crosshair => CursorIcon::Crosshair,
        CursorSpec::Help => CursorIcon::Help,
        CursorSpec::Wait => CursorIcon::Wait,
        CursorSpec::Progress => CursorIcon::Progress,
        CursorSpec::ZoomIn => CursorIcon::ZoomIn,
        CursorSpec::ZoomOut => CursorIcon::ZoomOut,
        CursorSpec::None => CursorIcon::Hidden,
    }
}

/// At most [`SurroundingText::MAX_BYTES`] of `text` around the selection.
///
/// A selection that fits is reported whole, with the rest of the budget split
/// around it (unused space on one side goes to the other); only a selection
/// longer than the budget is cut, around its cursor. The window never splits
/// a character: [`nana_text::editable::ime::surrounding_window`].
fn surrounding_window(text: &str, cursor: usize, anchor: usize) -> Option<SurroundingText> {
    const MAX: usize = SurroundingText::MAX_BYTES;
    if !text.is_char_boundary(cursor) || !text.is_char_boundary(anchor) {
        return None;
    }
    let selection = cursor.min(anchor)..cursor.max(anchor);
    let window = if selection.len() <= MAX {
        let spare = MAX - selection.len();
        let after_available = text.len() - selection.end;
        let before = selection
            .start
            .min((spare / 2).max(spare.saturating_sub(after_available)));
        nana_text::editable::ime::surrounding_window(text, selection, before, spare - before)
    } else {
        let half = MAX / 2;
        nana_text::editable::ime::surrounding_window(text, cursor..cursor, half, half)
    };
    if window.is_empty() && !text.is_empty() {
        return None;
    }
    let local = |offset: usize| offset.clamp(window.start, window.end) - window.start;
    Some(SurroundingText {
        text: text[window.clone()].to_string(),
        cursor: local(cursor),
        anchor: local(anchor),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutBox, MutationQueue, TextArea, TextInput};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    #[test]
    fn a_secure_field_is_a_password_and_a_read_only_one_takes_no_text() {
        let mut context = AppContext::new();
        let input = context
            .create_component(document(), TextInput::new("secret").secure(true))
            .unwrap();
        assert!(context.focus_node(document(), input.stable_id()).unwrap());
        let state = context.text_input_context(document()).unwrap();
        assert_eq!(state.purpose, TextInputPurpose::Password);
        assert_eq!(state.surrounding, None);
        context
            .update_component(input, |input, _cx| input.read_only = true)
            .unwrap();
        assert_eq!(context.text_input_context(document()), None);
    }

    #[test]
    fn a_text_area_takes_text_while_focused_and_enabled() {
        let mut context = AppContext::new();
        let area = context
            .create_component(document(), TextArea::new("第一行\n第二行"))
            .unwrap();
        assert_eq!(context.text_input_context(document()), None);
        assert!(context.focus_node(document(), area.stable_id()).unwrap());
        let state = context.text_input_context(document()).unwrap();
        assert_eq!(state.purpose, TextInputPurpose::Normal);
        assert_eq!(
            state.surrounding.map(|surrounding| surrounding.text),
            Some("第一行\n第二行".to_owned())
        );
        context
            .update_component(area, |area, _cx| area.disabled = true)
            .unwrap();
        assert_eq!(context.text_input_context(document()), None);
    }

    #[test]
    fn a_terminal_anchors_the_ime_at_its_grid_cursor() {
        let mut context = AppContext::new();
        let mut screen = crate::TerminalScreen::blank(10, 4);
        screen.cursor = Some(crate::TerminalCursor {
            position: crate::TerminalPosition { row: 2, column: 3 },
            shape: crate::TerminalCursorShape::Bar,
            visible: true,
        });
        let terminal = context
            .create_component(document(), crate::TerminalView::new(screen))
            .unwrap();
        let mut layout = MutationQueue::new();
        layout.write_layout(
            terminal.stable_id(),
            LayoutBox {
                x: 10.0,
                y: 20.0,
                width: 80.0,
                height: 72.0,
            },
        );
        context.commit_mutations(layout).unwrap();
        context
            .focus_node(document(), terminal.stable_id())
            .unwrap();
        let state = context.text_input_context(document()).unwrap();
        assert_eq!(state.purpose, TextInputPurpose::Terminal);
        assert_eq!(
            state.cursor_area,
            Some(LogicalRect::new(34.0, 56.0, 8.0, 18.0))
        );
        assert_eq!(state.surrounding, None);
        context
            .update_component(terminal, |view, _| view.read_only = true)
            .unwrap();
        assert_eq!(context.text_input_context(document()), None);
    }

    #[test]
    fn a_selection_that_fits_is_reported_whole_in_a_full_budget() {
        let text = "a".repeat(SurroundingText::MAX_BYTES * 3);
        let window = surrounding_window(&text, text.len(), text.len() - 3000).unwrap();
        assert_eq!(window.text.len(), SurroundingText::MAX_BYTES);
        assert_eq!(window.cursor - window.anchor, 3000);
        let short = "ab中cd";
        let window = surrounding_window(short, 5, 2).unwrap();
        assert_eq!(
            (window.text.as_str(), window.cursor, window.anchor),
            (short, 5, 2)
        );
    }

    #[test]
    fn a_short_field_is_reported_whole() {
        let window = surrounding_window("hello", 5, 1).unwrap();
        assert_eq!(window.text, "hello");
        assert_eq!((window.cursor, window.anchor), (5, 1));
    }

    #[test]
    fn a_long_field_is_windowed_around_the_selection() {
        let text = "a".repeat(10_000) + "界" + &"b".repeat(10_000);
        let caret = 10_000 + "界".len();
        let window = surrounding_window(&text, caret, caret).unwrap();
        assert!(window.text.len() <= SurroundingText::MAX_BYTES);
        assert!(window.text.is_char_boundary(window.cursor));
        assert_eq!(
            &window.text[window.cursor - "界".len()..window.cursor],
            "界"
        );
    }

    #[test]
    fn a_selection_longer_than_the_budget_is_cut_around_the_cursor() {
        let text = "x".repeat(20_000);
        let window = surrounding_window(&text, 15_000, 100).unwrap();
        assert!(window.text.len() <= SurroundingText::MAX_BYTES);
        assert_eq!(window.cursor, SurroundingText::MAX_BYTES / 2);
    }
}

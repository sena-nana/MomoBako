use std::sync::{Arc, Mutex};

use super::*;
use crate::{Entity, TerminalEvent, TerminalScreen, TerminalView};

fn terminal_fixture() -> (
    AppContext,
    DocumentId,
    Entity<TerminalView>,
    Arc<Mutex<Vec<TerminalEvent>>>,
) {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let terminal = context
        .create_component(document, TerminalView::new(TerminalScreen::blank(4, 2)))
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = events.clone();
    context
        .on(terminal, move |_, event: &TerminalEvent, _| {
            observed.lock().unwrap().push(event.clone())
        })
        .unwrap();
    context.focus_node(document, terminal.stable_id()).unwrap();
    (context, document, terminal, events)
}

fn typed(key: &str, text: Option<&str>) -> Gesture {
    key_fixture! {
        pressed: true,
        key: key.into(),
        text: text.map(str::to_owned),
        code: key.into(),
        repeat: false,
        modifiers: InputModifiers::default(),
    }
}

#[test]
fn terminal_ime_preedit_does_not_send_until_commit_and_focus_is_scoped() {
    let (mut context, document, _terminal, events) = terminal_fixture();
    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Update {
                    text: "zhong".into(),
                    selection: None
                }
            )
            .unwrap()
            .prevent_default
    );
    assert!(events.lock().unwrap().is_empty());
    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Commit("中文".into())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        events.lock().unwrap().as_slice(),
        &[TerminalEvent::Input("中文".as_bytes().to_vec())]
    );
    let editor = context
        .create_component(document, crate::TextInput::new(""))
        .unwrap();
    context.focus_node(document, editor.stable_id()).unwrap();
    adapter
        .dispatch_ime(
            &mut context,
            document,
            &CompositionInput::Commit("字".into()),
        )
        .unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
}

/// A key and the text it types arrive as two events. A printable key sends
/// nothing itself and its text is typed once; a key the terminal encodes
/// itself (Enter) sends its bytes, and the text naming that press is
/// dropped, so the terminal never sees `\r` twice.
#[test]
fn a_terminal_types_each_key_once() {
    let (mut context, document, _terminal, events) = terminal_fixture();
    let mut adapter = TestInput::default();
    adapter
        .dispatch(&mut context, document, &typed("a", Some("a")))
        .unwrap();
    adapter
        .dispatch(&mut context, document, &typed("Enter", Some("\r")))
        .unwrap();
    assert_eq!(
        events.lock().unwrap().as_slice(),
        &[
            TerminalEvent::Input(b"a".to_vec()),
            TerminalEvent::Input(b"\r".to_vec()),
        ]
    );
}

/// Copy and paste chords reach the terminal through the host clipboard,
/// at the terminal's place in the key chain.
#[test]
fn terminal_paste_reads_the_host_clipboard() {
    let (mut context, document, _terminal, events) = terminal_fixture();
    let mut adapter = TestInput::with_clipboard("ls");
    let paste = key_fixture! {
        pressed: true,
        key: "v".into(),
        text: None,
        code: "KeyV".into(),
        repeat: false,
        modifiers: InputModifiers {
            meta: true,
            ..InputModifiers::default()
        },
    };
    assert!(
        adapter
            .dispatch(&mut context, document, &paste)
            .unwrap()
            .handled
    );
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, TerminalEvent::Input(bytes) if bytes.ends_with(b"ls")))
    );
}

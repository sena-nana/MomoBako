use nana_ui::{HeadlessInput, runtime::*};
use nana_ui_platform::{CompositionInput, InputModifiers, KeyInput, KeyState};
use std::borrow::Cow;

/// A key press and, for a plain character, the text it types.
fn key(input: &mut HeadlessInput, cx: &mut AppContext, value: &str, shortcut: bool) {
    let key = KeyInput {
        physical: nana_ui_platform::PhysicalKey(Cow::Owned(value.into())),
        logical: nana_ui_platform::LogicalKey(Cow::Owned(value.into())),
        state: KeyState::Pressed,
        repeat: false,
        modifiers: InputModifiers {
            control: shortcut,
            ..Default::default()
        },
    };
    let text = (!shortcut && value.chars().count() == 1).then_some(value);
    input.press(cx, key, text, None).unwrap();
}

#[test]
fn maxlength_normal_keyboard_paste_selection_and_ime_commit_share_utf16_budget() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let input = cx
        .create_component(doc, TextInput::new("ab").max_length(5))
        .unwrap();
    cx.focus_node(doc, input.stable_id()).unwrap();
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    key(&mut adapter, &mut cx, "😀", false);
    assert_eq!(cx.world().text(input.stable_id()), Some("ab😀"));
    adapter.services_mut().set_clipboard(Some("😀z".into()));
    key(&mut adapter, &mut cx, "v", true);
    assert_eq!(cx.world().text(input.stable_id()), Some("ab😀"));
    adapter.services_mut().set_clipboard(Some("Z😀".into()));
    key(&mut adapter, &mut cx, "v", true);
    assert_eq!(cx.world().text(input.stable_id()), Some("ab😀Z"));
    key(&mut adapter, &mut cx, "a", true);
    adapter.services_mut().set_clipboard(Some("😀😀😀".into()));
    key(&mut adapter, &mut cx, "v", true);
    assert_eq!(cx.world().text(input.stable_id()), Some("😀😀"));
    adapter
        .composition(
            &mut cx,
            CompositionInput::Update {
                text: "你好世界".into(),
                selection: None,
            },
        )
        .unwrap();
    assert_eq!(cx.world().ime(input.stable_id()).unwrap().text, "你好世界");
    assert_eq!(cx.world().text(input.stable_id()), Some("😀😀"));
    adapter
        .composition(&mut cx, CompositionInput::Commit("你好世界".into()))
        .unwrap();
    assert_eq!(cx.world().text(input.stable_id()), Some("😀😀你"));
    assert!(cx.world().ime(input.stable_id()).is_none());
}

#[test]
fn maxlength_programmatic_overlong_values_are_preserved_but_user_growth_is_blocked() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let input = cx
        .create_component(doc, TextInput::new("oversized").max_length(3))
        .unwrap();
    cx.focus_node(doc, input.stable_id()).unwrap();
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    key(&mut adapter, &mut cx, "X", false);
    assert_eq!(cx.world().text(input.stable_id()), Some("oversized"));
    key(&mut adapter, &mut cx, "Backspace", false);
    assert_eq!(cx.world().text(input.stable_id()), Some("oversize"));
    cx.update_component(input, |view, _| {
        view.state = TextInputState::new("loaded 😀 value")
    })
    .unwrap();
    assert_eq!(cx.world().text(input.stable_id()), Some("loaded 😀 value"));
    key(&mut adapter, &mut cx, "a", true);
    key(&mut adapter, &mut cx, "好", false);
    assert_eq!(cx.world().text(input.stable_id()), Some("好"));
}

#[test]
fn maxlength_multiple_cursors_share_total_budget_and_ime_only_replaces_primary() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let mut view = TextInput::new("ab").max_length(7);
    view.state.selection = TextSelection::caret(0);
    view.state.additional_selections = vec![TextSelection::caret(2)];
    let input = cx.create_component(doc, view).unwrap();
    cx.focus_node(doc, input.stable_id()).unwrap();
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    key(&mut adapter, &mut cx, "😀", false);
    assert_eq!(cx.world().text(input.stable_id()), Some("😀ab😀"));
    key(&mut adapter, &mut cx, "X", false);
    assert_eq!(cx.world().text(input.stable_id()), Some("😀ab😀"));
    adapter
        .composition(&mut cx, CompositionInput::Commit("你好".into()))
        .unwrap();
    assert_eq!(cx.world().text(input.stable_id()), Some("😀你ab😀"));
    assert_eq!(
        cx.read(input, |view| view.state.additional_selections.len())
            .unwrap(),
        1
    );
}

#[test]
fn maxlength_advanced_replace_rejects_growth_without_changing_selection_and_accepts_shortening() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let input = cx
        .create_component(doc, TextInput::new("ab ab").max_length(5))
        .unwrap();
    cx.focus_node(doc, input.stable_id()).unwrap();
    cx.update_component(input, |view, _| {
        view.state.selection = TextSelection::new(0, 2)
    })
    .unwrap();
    let before = cx.read(input, |view| view.state.clone()).unwrap();
    assert!(
        !cx.replace_focused_text_match(doc, "ab", TextSearchOptions::default(), "😀ab", false)
            .unwrap()
    );
    assert_eq!(cx.read(input, |view| view.state.clone()).unwrap(), before);
    assert_eq!(
        cx.replace_all_focused_text_matches(
            doc,
            "ab",
            TextSearchOptions::default(),
            "abcdef",
            TextFindScope::Document,
            false
        )
        .unwrap(),
        0
    );
    assert_eq!(cx.read(input, |view| view.state.clone()).unwrap(), before);
    assert_eq!(
        cx.replace_all_focused_text_matches(
            doc,
            "ab",
            TextSearchOptions::default(),
            "x",
            TextFindScope::Document,
            false
        )
        .unwrap(),
        2
    );
    assert_eq!(cx.world().text(input.stable_id()), Some("x x"));
}

#[test]
fn maxlength_identical_paste_and_ime_still_collapse_the_replaced_selection() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let input = cx
        .create_component(doc, TextInput::new("abc").max_length(3))
        .unwrap();
    cx.focus_node(doc, input.stable_id()).unwrap();
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    adapter.services_mut().set_clipboard(Some("abc".into()));
    key(&mut adapter, &mut cx, "a", true);
    key(&mut adapter, &mut cx, "v", true);
    assert_eq!(
        cx.read(input, |view| view.state.selection).unwrap(),
        TextSelection::caret(3)
    );
    key(&mut adapter, &mut cx, "a", true);
    adapter
        .composition(
            &mut cx,
            CompositionInput::Update {
                text: "abc".into(),
                selection: None,
            },
        )
        .unwrap();
    adapter
        .composition(&mut cx, CompositionInput::Commit("abc".into()))
        .unwrap();
    assert_eq!(cx.world().text(input.stable_id()), Some("abc"));
    assert_eq!(
        cx.read(input, |view| view.state.selection).unwrap(),
        TextSelection::caret(3)
    );
    assert!(cx.world().ime(input.stable_id()).is_none());
}

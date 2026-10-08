use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use super::*;
use crate::{Activate, Button, Entity, HoverCard, LayoutViewport, QrCode, Stack, TextInput};

struct Fixture {
    cx: AppContext,
    input: TestInput,
    doc: DocumentId,
    editor: Entity<TextInput>,
    other: Entity<TextInput>,
    card: Entity<HoverCard>,
    qr: Entity<QrCode>,
    refresh: Entity<Button>,
    activations: Arc<AtomicUsize>,
}

impl Fixture {
    fn new() -> Self {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let root = cx.create_component(doc, Stack::column(12.0)).unwrap();
        let editor = cx.create_component(doc, TextInput::new("abcdef")).unwrap();
        let card = cx
            .create_component(
                doc,
                HoverCard::new()
                    .trigger("account")
                    .preserve_editor_focus(true)
                    .open_delay(100)
                    .close_delay(120),
            )
            .unwrap();
        let body = cx
            .create_component(doc, Stack::column(8.0).hittable())
            .unwrap();
        let qr = cx
            .create_component(
                doc,
                QrCode::encode("https://example.com/login", 100.0).unwrap(),
            )
            .unwrap();
        let refresh = cx.create_component(doc, Button::new("refresh")).unwrap();
        let other = cx.create_component(doc, TextInput::new("other")).unwrap();
        cx.append_child(root, editor).unwrap();
        cx.append_child(root, card).unwrap();
        cx.append_child(root, other).unwrap();
        cx.append_child(card, body).unwrap();
        cx.append_child(body, qr).unwrap();
        cx.append_child(body, refresh).unwrap();
        let activations = Arc::new(AtomicUsize::new(0));
        let observed = activations.clone();
        cx.on(refresh, move |_, _: &Activate, _| {
            observed.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        cx.focus_node(doc, editor.stable_id()).unwrap();
        let mut f = Self {
            cx,
            input: TestInput::default(),
            doc,
            editor,
            other,
            card,
            qr,
            refresh,
            activations,
        };
        f.layout();
        f.key("ArrowLeft", true, None);
        f.assert_editor();
        f
    }
    fn layout(&mut self) {
        self.cx
            .layout_document(self.doc, LayoutViewport::new(800.0, 600.0))
            .unwrap();
    }
    fn key(&mut self, key: &str, shift: bool, text: Option<&str>) {
        self.input
            .dispatch(
                &mut self.cx,
                self.doc,
                &key_fixture! {
                    pressed: true,
                    key: key.into(),
                    code: key.into(),
                    text: text.map(str::to_owned),
                    repeat: false,
                    modifiers: InputModifiers {
                        shift,
                        ..Default::default()
                    },
                },
            )
            .unwrap_or_else(|error| panic!("key {key}: {error:?}"));
    }
    fn pointer(&mut self, phase: PointerPhase, target: Option<StableNodeId>, ms: u64) {
        self.cx.rebuild_hit_test(self.doc);
        let (x, y) = target
            .map(|id| {
                let b = self.cx.world().layout_box(id).unwrap();
                (b.x + b.width / 2.0, b.y + b.height / 2.0)
            })
            .unwrap_or((799.0, 599.0));
        self.input
            .dispatch_at(
                &mut self.cx,
                self.doc,
                &pointer_fixture! {
                    phase,
                    pointer_id: 1,
                    pointer_type: PointerType::Mouse,
                    x,
                    y,
                    screen_x: x,
                    screen_y: y,
                    button: 0,
                    buttons: u16::from(phase == PointerPhase::Down),
                    pressure: 0.0,
                    tangential_pressure: 0.0,
                    tilt_x: 0,
                    tilt_y: 0,
                    twist: 0,
                    is_primary: true,
                    activation_click: false,
                    modifiers: Default::default(),
                },
                Duration::from_millis(ms),
            )
            .unwrap();
    }
    fn click(&mut self, target: StableNodeId) {
        self.pointer(PointerPhase::Down, Some(target), 200);
        self.pointer(PointerPhase::Up, Some(target), 201);
    }
    fn open(&mut self) {
        self.cx
            .update_component(self.card, |card, _| card.open = true)
            .unwrap();
        self.layout();
    }
    fn assert_editor(&self) {
        assert_eq!(
            self.cx.world().focused(self.doc),
            Some(self.editor.stable_id())
        );
        let state = self.cx.world().text_input(self.editor.stable_id()).unwrap();
        assert_eq!(
            (state.value, state.selection.anchor, state.selection.focus),
            ("abcdef", 6, 5)
        );
    }
}

#[test]
fn hover_card_pointer_actions_preserve_editor_selection_and_deliver_activation() {
    let mut f = Fixture::new();
    f.cx.rebuild_hit_test(f.doc);
    let b = f.cx.world().layout_box(f.card.stable_id()).unwrap();
    assert_eq!(
        f.cx.pointer_target(f.doc, b.x + b.width / 2.0, b.y + b.height / 2.0),
        Some(f.card.stable_id())
    );
    f.pointer(PointerPhase::Move, Some(f.card.stable_id()), 0);
    f.cx.advance_animations(Duration::from_millis(0));
    f.cx.advance_animations(Duration::from_millis(99));
    assert!(!f.cx.read(f.card, |card| card.open).unwrap());
    f.cx.advance_animations(Duration::from_millis(101));
    f.layout();
    assert!(f.cx.read(f.card, |card| card.open).unwrap());
    f.assert_editor();
    assert_eq!(
        f.cx.world().accessibility(f.qr.stable_id()).unwrap().role,
        crate::AccessibilityRole::Image
    );
    for target in [f.card.stable_id(), f.qr.stable_id(), f.refresh.stable_id()] {
        if !f.cx.read(f.card, |card| card.open).unwrap() {
            f.open();
        }
        f.click(target);
        assert_eq!(
            f.cx.world().focused(f.doc),
            Some(f.editor.stable_id()),
            "click {target:?}"
        );
        f.assert_editor();
    }
    assert_eq!(f.activations.load(Ordering::SeqCst), 1);
    f.pointer(PointerPhase::Move, None, 300);
    f.cx.advance_animations(Duration::from_millis(419));
    assert!(f.cx.read(f.card, |card| card.open).unwrap());
    f.cx.advance_animations(Duration::from_millis(421));
    assert!(!f.cx.read(f.card, |card| card.open).unwrap());
    f.assert_editor();
    f.open();
    f.assert_editor();
    f.key("Escape", false, None);
    f.assert_editor();
    f.key("x", false, Some("x"));
    assert_eq!(
        f.cx.world().text_input(f.editor.stable_id()).unwrap().value,
        "abcdex"
    );
}

#[test]
fn hover_card_keyboard_navigation_and_close_restore_editor_without_stealing_new_focus() {
    let mut f = Fixture::new();
    f.open();
    f.assert_editor();
    for _ in 0..5 {
        f.key("Tab", false, None);
        if f.cx.world().focused(f.doc) == Some(f.refresh.stable_id()) {
            break;
        }
    }
    assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
    f.key("Enter", false, None);
    assert_eq!(f.activations.load(Ordering::SeqCst), 1);
    f.key("Tab", true, None);
    f.key("Tab", false, None);
    assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
    f.key("Escape", false, None);
    assert!(!f.cx.read(f.card, |card| card.open).unwrap());
    f.assert_editor();
    f.open();
    f.click(f.other.stable_id());
    assert_eq!(f.cx.world().focused(f.doc), Some(f.other.stable_id()));
    f.key("Escape", false, None);
    assert_eq!(f.cx.world().focused(f.doc), Some(f.other.stable_id()));
}

#[test]
fn hover_card_default_pointer_policy_keeps_button_focus_behavior() {
    let mut f = Fixture::new();
    f.cx.update_component(f.card, |card, _| card.preserve_editor_focus = false)
        .unwrap();
    f.open();
    f.click(f.refresh.stable_id());
    assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
    assert_eq!(f.activations.load(Ordering::SeqCst), 1);
}

#[test]
fn hover_card_programmatic_close_restores_keyboard_focus() {
    let mut f = Fixture::new();
    f.open();
    for _ in 0..5 {
        f.key("Tab", false, None);
        if f.cx.world().focused(f.doc) == Some(f.refresh.stable_id()) {
            break;
        }
    }
    assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
    f.cx.update_component(f.card, |card, _| card.open = false)
        .unwrap();
    f.assert_editor();
}

#[test]
fn hover_card_close_never_restores_hidden_disabled_or_removed_editor() {
    for invalidation in 0..4 {
        let mut f = Fixture::new();
        f.open();
        for _ in 0..5 {
            f.key("Tab", false, None);
            if f.cx.world().focused(f.doc) == Some(f.refresh.stable_id()) {
                break;
            }
        }
        assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
        if invalidation == 0 {
            f.cx.remove_view(f.editor).unwrap();
        } else if invalidation == 1 {
            f.cx.update_component(f.editor, |editor, _| editor.disabled = true)
                .unwrap();
        } else {
            let mut hidden =
                f.cx.world()
                    .node_style(f.editor.stable_id())
                    .unwrap()
                    .clone();
            if invalidation == 3 {
                Arc::make_mut(&mut hidden.layout).paint.visibility =
                    Some(nana_ui_core::VisibilitySpec::Hidden);
            } else {
                Arc::make_mut(&mut hidden.layout).display = Some(nana_ui_core::DisplaySpec::None);
            }
            let mut mutations = crate::MutationQueue::new();
            mutations.set_style(f.editor.stable_id(), hidden);
            f.cx.commit_mutations(mutations).unwrap();
        }
        f.key("Escape", false, None);
        assert_eq!(f.cx.world().focused(f.doc), None);
        assert!(!f.cx.read(f.card, |card| card.open).unwrap());
        f.key("Enter", false, None);
        assert_eq!(f.activations.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn hover_card_close_honors_explicit_focus_in_the_same_update() {
    let mut f = Fixture::new();
    f.open();
    f.key("Tab", false, None);
    assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
    let other = f.other.stable_id();
    let doc = f.doc;
    f.cx.update_component(f.card, |card, cx| {
        card.open = false;
        cx.mutations().request_focus(doc, Some(other));
    })
    .unwrap();
    assert!(!f.cx.read(f.card, |card| card.open).unwrap());
    assert_eq!(f.cx.world().focused(f.doc), Some(other));
}

#[test]
fn hover_card_close_validates_restore_target_after_the_whole_update() {
    for invalidation in 0..4 {
        let mut f = Fixture::new();
        f.open();
        f.key("Tab", false, None);
        assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
        let editor = f.editor.stable_id();
        let mut hidden = f.cx.world().node_style(editor).unwrap().clone();
        if invalidation == 3 {
            Arc::make_mut(&mut hidden.layout).paint.visibility =
                Some(nana_ui_core::VisibilitySpec::Hidden);
        } else {
            Arc::make_mut(&mut hidden.layout).display = Some(nana_ui_core::DisplaySpec::None);
        }
        let mut disabled = f.cx.world().accessibility(editor).unwrap().clone();
        disabled.disabled = true;
        f.cx.update_component(f.card, |card, cx| {
            card.open = false;
            match invalidation {
                0 => cx.mutations().despawn_subtree(editor),
                1 => {
                    cx.mutations().set_accessibility(editor, disabled);
                    cx.mutations().set_interaction(
                        editor,
                        crate::InteractionState {
                            pointer_events: false,
                            focusable: false,
                        },
                    );
                }
                _ => cx.mutations().set_style(editor, hidden),
            }
        })
        .unwrap();
        assert!(!f.cx.read(f.card, |card| card.open).unwrap());
        assert_eq!(f.cx.world().focused(f.doc), None);
        f.key("Enter", false, None);
        assert_eq!(f.activations.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn hover_card_keyboard_entry_restores_the_most_recent_external_editor() {
    let mut f = Fixture::new();
    f.open();
    f.click(f.other.stable_id());
    assert_eq!(f.cx.world().focused(f.doc), Some(f.other.stable_id()));
    f.key("Tab", true, None);
    assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
    f.key("Escape", false, None);
    assert!(!f.cx.read(f.card, |card| card.open).unwrap());
    assert_eq!(f.cx.world().focused(f.doc), Some(f.other.stable_id()));
    f.key("x", false, Some("x"));
    assert!(
        f.cx.world()
            .text_input(f.other.stable_id())
            .unwrap()
            .value
            .contains('x')
    );
    assert_eq!(
        f.cx.world().text_input(f.editor.stable_id()).unwrap().value,
        "abcdef"
    );
}

#[test]
fn hover_card_pending_hover_survives_closed_property_projection() {
    for preserve in [false, true] {
        let mut f = Fixture::new();
        f.cx.update_component(f.card, |card, _| card.preserve_editor_focus = preserve)
            .unwrap();
        f.pointer(PointerPhase::Move, Some(f.card.stable_id()), 0);
        f.cx.advance_animations(Duration::from_millis(50));
        f.cx.update_component(f.card, |card, _| card.close_delay_ms = 150)
            .unwrap();
        f.cx.advance_animations(Duration::from_millis(101));
        f.layout();
        assert!(
            f.cx.read(f.card, |card| card.open).unwrap(),
            "preserve={preserve}"
        );
        f.assert_editor();
    }
}

#[test]
fn hover_card_close_honors_focus_scope_restoration_in_same_update() {
    let mut f = Fixture::new();
    let scope = f.cx.create_component(f.doc, Stack::column(0.0)).unwrap();
    let root =
        f.cx.world()
            .node(f.other.stable_id())
            .unwrap()
            .parent
            .unwrap();
    let mut changes = crate::MutationQueue::new();
    changes.insert(root, scope.stable_id(), None);
    changes.insert(scope.stable_id(), f.other.stable_id(), None);
    f.cx.commit_mutations(changes).unwrap();
    f.cx.compat_world_mut()
        .register_focus_scope(scope.stable_id())
        .unwrap();
    f.layout();
    f.click(f.other.stable_id());
    f.cx.focus_node(f.doc, f.editor.stable_id()).unwrap();
    f.assert_editor();
    f.open();
    f.key("Tab", false, None);
    assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
    f.cx.update_component(f.card, |card, cx| {
        card.open = false;
        cx.mutations().restore_focus_within(scope.stable_id());
    })
    .unwrap();
    assert_eq!(f.cx.world().focused(f.doc), Some(f.other.stable_id()));
    f.key("x", false, Some("x"));
    assert!(
        f.cx.world()
            .text_input(f.other.stable_id())
            .unwrap()
            .value
            .contains('x')
    );
}

#[test]
fn hover_card_removed_with_keyboard_focus_restores_editor() {
    let mut f = Fixture::new();
    f.open();
    f.key("Tab", false, None);
    assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
    f.cx.remove_view(f.card).unwrap();
    f.layout();
    f.assert_editor();
    f.key("x", false, Some("x"));
    assert_eq!(
        f.cx.world().text_input(f.editor.stable_id()).unwrap().value,
        "abcdex"
    );
}

#[test]
fn hover_card_hidden_or_parked_subtree_restores_editor() {
    for invalidation in 0..6 {
        let mut f = Fixture::new();
        let parent = f.cx.create_component(f.doc, Stack::column(0.0)).unwrap();
        let root =
            f.cx.world()
                .node(f.card.stable_id())
                .unwrap()
                .parent
                .unwrap();
        let mut changes = crate::MutationQueue::new();
        changes.insert(root, parent.stable_id(), Some(f.other.stable_id()));
        changes.insert(parent.stable_id(), f.card.stable_id(), None);
        f.cx.commit_mutations(changes).unwrap();
        f.open();
        f.key("Tab", false, None);
        assert_eq!(f.cx.world().focused(f.doc), Some(f.refresh.stable_id()));
        let mut changes = crate::MutationQueue::new();
        let target = if invalidation < 2 || invalidation == 4 {
            parent.stable_id()
        } else {
            f.card.stable_id()
        };
        if invalidation == 1 || invalidation == 3 {
            changes.park_subtree(target);
        } else {
            let mut style = f.cx.world().node_style(target).unwrap().clone();
            if invalidation >= 4 {
                Arc::make_mut(&mut style.layout).paint.visibility =
                    Some(nana_ui_core::VisibilitySpec::Hidden);
            } else {
                Arc::make_mut(&mut style.layout).display = Some(nana_ui_core::DisplaySpec::None);
            }
            changes.set_style(target, style);
        }
        f.cx.commit_mutations(changes).unwrap();
        f.layout();
        f.assert_editor();
    }
}

#[test]
fn hover_card_refresh_invalidation_then_close_preserves_editor() {
    for invalidation in 0..4 {
        let mut f = Fixture::new();
        f.open();
        f.key("Tab", false, None);
        f.key("Enter", false, None);
        assert_eq!(f.activations.load(Ordering::SeqCst), 1);
        match invalidation {
            0 => {
                f.cx.remove_view(f.refresh).unwrap();
            }
            1 => {
                f.cx.update_component(f.refresh, |button, _| button.disabled = true)
                    .unwrap();
            }
            _ => {
                let mut style =
                    f.cx.world()
                        .node_style(f.refresh.stable_id())
                        .unwrap()
                        .clone();
                if invalidation == 3 {
                    Arc::make_mut(&mut style.layout).paint.visibility =
                        Some(nana_ui_core::VisibilitySpec::Hidden);
                } else {
                    Arc::make_mut(&mut style.layout).display =
                        Some(nana_ui_core::DisplaySpec::None);
                }
                let mut changes = crate::MutationQueue::new();
                changes.set_style(f.refresh.stable_id(), style);
                f.cx.commit_mutations(changes).unwrap();
            }
        }
        f.assert_editor();
        f.layout();
        f.cx.advance_animations(Duration::from_millis(500));
        f.key("Escape", false, None);
        f.assert_editor();
        f.key("x", false, Some("x"));
        assert_eq!(
            f.cx.world().text_input(f.editor.stable_id()).unwrap().value,
            "abcdex"
        );
    }
}

#[test]
fn hover_card_pointer_actions_preserve_active_ime_composition() {
    let mut f = Fixture::new();
    f.input
        .dispatch_ime(
            &mut f.cx,
            f.doc,
            &CompositionInput::Update {
                text: "你".into(),
                selection: Some((0, 3)),
            },
        )
        .unwrap();
    f.open();
    for target in [f.card.stable_id(), f.qr.stable_id(), f.refresh.stable_id()] {
        if !f.cx.read(f.card, |card| card.open).unwrap() {
            f.open();
        }
        f.click(target);
        f.assert_editor();
        assert_eq!(
            f.cx.world()
                .ime(f.editor.stable_id())
                .map(|ime| (ime.text, ime.selection)),
            Some(("你", Some((0, 3))))
        );
    }
    assert_eq!(f.activations.load(Ordering::SeqCst), 1);
    f.cx.update_component(f.card, |card, _| card.open = false)
        .unwrap();
    f.assert_editor();
    f.input
        .dispatch_ime(&mut f.cx, f.doc, &CompositionInput::Commit("你".into()))
        .unwrap();
    assert_eq!(
        f.cx.world().text_input(f.editor.stable_id()).unwrap().value,
        "abcde你"
    );
    assert!(f.cx.world().ime(f.editor.stable_id()).is_none());
}

#[test]
fn hover_card_closed_content_is_skipped_by_tab_navigation() {
    let mut f = Fixture::new();
    f.open();
    f.key("Escape", false, None);
    f.layout();
    f.key("Tab", false, None);
    assert_eq!(f.cx.world().focused(f.doc), Some(f.other.stable_id()));
    f.key("Tab", true, None);
    f.assert_editor();
    assert_eq!(f.activations.load(Ordering::SeqCst), 0);
}

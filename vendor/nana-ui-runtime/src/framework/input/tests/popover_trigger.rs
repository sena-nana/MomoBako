//! A popover whose trigger is drawn by the application's content: the
//! popover stays the control that opens, anchors and takes focus.

use std::sync::{Arc, Mutex};

use super::*;
use crate::view::{button, entity_ref, row, widget, with_refs};
use crate::{Entity, LayoutViewport, LengthSpec, Popover, PopoverToggled, Stack};

struct Fixture {
    cx: AppContext,
    input: TestInput,
    doc: DocumentId,
    popover: Entity<Popover>,
    count: StableNodeId,
    content: StableNodeId,
    item: StableNodeId,
    toggles: Arc<Mutex<Vec<bool>>>,
}

impl Fixture {
    fn new() -> Self {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let root = cx.create_component(doc, Stack::column(0.0)).unwrap();
        let (_, (popover, count, content, item)) = cx
            .mount_view(root.stable_id(), || {
                let popover = entity_ref::<Popover>();
                // Boxes the size an icon and a count would measure to.
                let count = entity_ref::<Stack>();
                let sized = |width: f32| {
                    widget(Stack::row(0.0).with_layout(move |layout| {
                        layout.width = Some(LengthSpec::Px(width));
                        layout.height = Some(LengthSpec::Px(16.0));
                    }))
                };
                let content = entity_ref::<Stack>();
                let item = entity_ref::<crate::Button>();
                let view = widget(Popover::new().trigger("收藏"))
                    .entity_ref(popover)
                    .trigger(
                        row()
                            .gap(4.0)
                            .entity_ref(content)
                            .children((sized(16.0), sized(40.0).entity_ref(count))),
                    )
                    .children(button("加入收藏夹").entity_ref(item));
                with_refs(view, (popover, count, content, item))
            })
            .unwrap();
        let toggles = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&toggles);
        cx.on(popover, move |_, event: &PopoverToggled, _| {
            heard.lock().unwrap().push(event.open);
        })
        .unwrap();
        let mut fixture = Self {
            cx,
            input: TestInput::default(),
            doc,
            popover,
            count: count.stable_id(),
            content: content.stable_id(),
            item: item.stable_id(),
            toggles,
        };
        fixture.layout();
        fixture
    }

    fn layout(&mut self) {
        self.cx
            .layout_document(self.doc, LayoutViewport::new(800.0, 600.0))
            .unwrap();
        self.cx.rebuild_hit_test(self.doc);
    }

    fn open(&self) -> bool {
        self.cx.read(self.popover, |popover| popover.open).unwrap()
    }

    /// Laid out with an area and reachable through the menu it is in.
    fn shown(&self, id: StableNodeId) -> bool {
        let world = self.cx.world();
        world.is_overlay_reachable(id)
            && world
                .layout_box(id)
                .is_some_and(|b| b.width > 0.0 && b.height > 0.0)
    }

    fn center(&self, id: StableNodeId) -> (f32, f32) {
        let b = self.cx.world().layout_box(id).unwrap();
        (b.x + b.width / 2.0, b.y + b.height / 2.0)
    }

    fn pointer(&mut self, phase: PointerPhase, at: (f32, f32), ms: u64) {
        let (x, y) = at;
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
        self.layout();
    }

    fn click(&mut self, at: (f32, f32), ms: u64) {
        self.pointer(PointerPhase::Down, at, ms);
        self.pointer(PointerPhase::Up, at, ms + 1);
    }

    fn key(&mut self, key: &str) {
        self.input
            .dispatch(
                &mut self.cx,
                self.doc,
                &key_fixture! {
                    pressed: true,
                    key: key.into(),
                    code: key.into(),
                    text: None,
                    repeat: false,
                    modifiers: InputModifiers::default(),
                },
            )
            .unwrap();
        self.layout();
    }
}

/// The content draws the trigger while the surface is closed; the popover's
/// own box holds it, and a press on it is a press on the popover.
#[test]
fn content_draws_the_trigger_and_a_press_on_it_opens_the_popover() {
    let mut f = Fixture::new();
    let popover = f.popover.stable_id();
    assert!(f.shown(f.content) && f.shown(f.count));
    assert!(!f.shown(f.item), "the items wait for the surface");
    let trigger = f.cx.world().layout_box(popover).unwrap();
    let content = f.cx.world().layout_box(f.content).unwrap();
    assert!(
        content.x >= trigger.x
            && content.x + content.width <= trigger.x + trigger.width
            && content.height <= trigger.height,
        "{content:?} inside {trigger:?}"
    );
    assert!(content.width > 0.0);
    // The popover names the trigger; its label is not painted over the content.
    let accessibility = f.cx.world().accessibility(popover).unwrap();
    assert_eq!(accessibility.label.as_deref(), Some("收藏"));
    assert_eq!(f.cx.world().text(popover), Some(""));

    let on_count = f.center(f.count);
    assert_eq!(
        f.cx.world().hit_test(f.doc, on_count.0, on_count.1),
        Some(popover)
    );
    f.click(on_count, 100);
    assert!(f.open());
    assert!(f.shown(f.content) && f.shown(f.item));
    // The surface hangs under the trigger, not in its flow.
    let item = f.cx.world().layout_box(f.item).unwrap();
    let trigger_after = f.cx.world().layout_box(popover).unwrap();
    assert_eq!(trigger_after, trigger, "opening does not grow the trigger");
    assert!(
        item.y >= trigger.y + trigger.height,
        "{item:?} under {trigger:?}"
    );
    // The trigger again closes it.
    f.click(on_count, 200);
    assert!(!f.open());
    assert!(f.shown(f.count));
    assert_eq!(*f.toggles.lock().unwrap(), [true, false]);

    // Back to a text trigger: the child is one of the items again, and waits
    // for the surface with them.
    f.cx.update_component(f.popover, |popover, _| popover.trigger_content = None)
        .unwrap();
    // Past the close motion, so the surface is closed, not closing.
    f.cx.advance_animations(Duration::from_secs(5));
    f.layout();
    assert!(!f.shown(f.content));
    assert_eq!(f.cx.world().text(popover), Some("收藏"));
}

/// Enter opens it from the keyboard, Escape and a press elsewhere dismiss it,
/// and focus comes back to the trigger when it closes around focus.
#[test]
fn a_content_trigger_opens_by_keyboard_dismisses_and_returns_focus() {
    let mut f = Fixture::new();
    let popover = f.popover.stable_id();
    assert!(f.cx.focus_node(f.doc, popover).unwrap());
    f.key("Enter");
    assert!(f.open());
    // Focus moves into the surface, then Escape closes it around the focus.
    assert!(f.cx.focus_node(f.doc, f.item).unwrap());
    f.key("Escape");
    assert!(!f.open());
    assert_eq!(f.cx.world().focused(f.doc), Some(popover));

    f.key(" ");
    assert!(f.open());
    f.click((790.0, 590.0), 300);
    assert!(!f.open(), "a press elsewhere dismisses it");
    assert!(f.shown(f.count));
    assert_eq!(*f.toggles.lock().unwrap(), [true, false, true, false]);
}

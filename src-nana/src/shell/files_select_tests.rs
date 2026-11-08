//! 文件卡片的点选和框选读修饰键，和 Vue `selectionModeFromEvent`、框选的 `additive` 对齐。
//!
//! 运行时路由完输入以后宿主记下修饰键（`window_host::note_modifiers`），卡片发出的单击消息归约时
//! 读它：Shift 是从锚点到这一张的范围，Ctrl / Meta 是切换，否则替换；按下时按着 Ctrl 的框选并进
//! 原来的选择。测试挂完整壳层，从指针事件开始走，右侧详情跟着主选中项走（Vue `currentFileEntry`）。

use nana_ui::runtime::{AccessibilityNode, AccessibilityRole, component_descriptors};
use nana_ui::{InputModifiers, PointerPhase};

use super::super::ShellViewModel;
use super::super::view_harness::ShellHarness;

const PLAIN: InputModifiers = InputModifiers {
    alt: false,
    control: false,
    meta: false,
    shift: false,
};
const SHIFT: InputModifiers = InputModifiers {
    shift: true,
    ..PLAIN
};
const CONTROL: InputModifiers = InputModifiers {
    control: true,
    ..PLAIN
};

/// 列表模式的文件页：assets 文件夹、cover.png 和 notes/page.pdf，没有选择。
fn list_page() -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == "files-list-mode")
        .map(|(_, model)| model)
        .expect("列表模式的文件页场景")
}

/// 文件列表里的卡片（键是 `file-row-…`），侧栏文件夹树里的同名项不算。
fn cards(harness: &ShellHarness) -> Vec<AccessibilityNode> {
    let context = harness.document().context();
    harness
        .nodes()
        .into_iter()
        .filter(|node| node.role == AccessibilityRole::ListItem)
        .filter(|node| {
            context.assembly_path(node.id).is_some_and(|path| {
                path.rsplit('/')
                    .next()
                    .is_some_and(|key| key.starts_with("file-row-"))
            })
        })
        .collect()
}

/// 名为 `name` 的文件卡片。
fn card(harness: &ShellHarness, name: &str) -> AccessibilityNode {
    cards(harness)
        .into_iter()
        .find(|node| node.label.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("没有文件卡片 {name}"))
}

fn center(node: &AccessibilityNode) -> (f32, f32) {
    (
        node.bounds.x + node.bounds.width / 2.0,
        node.bounds.y + node.bounds.height / 2.0,
    )
}

/// 按着 `modifiers` 点一下卡片，把这次点击发出的消息按生产的先后归约。
fn click(harness: &mut ShellHarness, name: &str, modifiers: InputModifiers) {
    let (x, y) = center(&card(harness, name));
    let messages = harness.click_with(x, y, modifiers);
    assert!(!messages.is_empty(), "点 {name} 没有发出消息");
    for message in messages {
        harness.apply(message);
    }
    harness.flush();
}

fn selected(harness: &ShellHarness) -> Vec<String> {
    let mut paths = harness.model.files.selected_paths().to_vec();
    paths.sort_unstable();
    paths
}

/// 卡片所在列表滚动区里不压在任何卡片上的一点：先试上方两角（内容上边距里）。
fn empty_list_point(harness: &ShellHarness, name: &str) -> (f32, f32) {
    let world = harness.document().context().world();
    let mut cursor = world.parent_id(card(harness, name).id);
    let list = loop {
        let node = cursor.expect("文件卡片上面没有列表滚动区");
        let scroll = world.component_type(node).is_some_and(|kind| {
            std::borrow::Borrow::<str>::borrow(kind) == component_descriptors::SCROLL_VIEW.type_id
        });
        if scroll {
            break world.layout_box(node).expect("列表滚动区的布局盒");
        }
        cursor = world.parent_id(node);
    };
    let inside = |bounds: &nana_ui::runtime::LayoutBox, (x, y): (f32, f32)| {
        x >= bounds.x
            && y >= bounds.y
            && x < bounds.x + bounds.width
            && y < bounds.y + bounds.height
    };
    let cards = cards(harness);
    [
        (list.x + list.width - 2.0, list.y + 2.0),
        (list.x + 2.0, list.y + 2.0),
    ]
    .into_iter()
    .find(|point| cards.iter().all(|card| !inside(&card.bounds, *point)))
    .unwrap_or_else(|| panic!("列表 {list:?} 上方没有空白点"))
}

/// Shift 点从锚点拉出一段，Ctrl 点拿掉一张（不是主选中项时详情不动），修饰键松开后的单击又是替换。
#[test]
fn modifier_clicks_extend_and_toggle_the_selection() {
    let mut harness = ShellHarness::mount(list_page());
    click(&mut harness, "cover.png", PLAIN);
    assert_eq!(selected(&harness), ["cover.png"]);

    click(&mut harness, "page.pdf", SHIFT);
    assert_eq!(
        selected(&harness),
        ["cover.png", "notes/page.pdf"],
        "Shift 点是从锚点到这一张的范围"
    );
    assert_eq!(
        harness.model.files.primary.as_deref(),
        Some("notes/page.pdf")
    );
    assert_eq!(
        harness.model.inspect.target_path.as_deref(),
        Some("notes/page.pdf"),
        "详情跟着主选中项"
    );

    click(&mut harness, "cover.png", CONTROL);
    assert_eq!(selected(&harness), ["notes/page.pdf"], "Ctrl 点是切换");
    assert_eq!(
        harness.model.inspect.target_path.as_deref(),
        Some("notes/page.pdf"),
        "拿掉的不是主选中项，详情不动"
    );

    click(&mut harness, "assets", PLAIN);
    assert_eq!(selected(&harness), ["assets"], "松开修饰键以后单击是替换");
}

/// 从列表空白拖到 cover.png：没过 3px 不画选框；拖过以后选框盖住这段指针，松手选框消失、选择留下。
#[test]
fn box_drag_paints_the_vue_selection_rectangle() {
    let mut harness = ShellHarness::mount(list_page());
    let start = empty_list_point(&harness, "cover.png");
    let end = center(&card(&harness, "cover.png"));
    assert!(box_opacity(&harness) < 0.01, "还没拖时选框不该出现");

    harness.pointer(PointerPhase::Down, start.0, start.1, PLAIN);
    harness.frame();
    harness.pointer(PointerPhase::Move, start.0 + 2.0, start.1, PLAIN);
    harness.frame();
    assert!(
        box_opacity(&harness) < 0.01,
        "2px 还没到 Vue 的 3px，选框不该出现"
    );
    assert!(selected(&harness).is_empty(), "没拖过 3px 时选择不动");

    harness.pointer(PointerPhase::Move, end.0, end.1, PLAIN);
    harness.frame();
    let (style, bounds) = selection_box(&harness);
    assert!(
        (style.layout.opacity.unwrap_or(0.0) - 1.0).abs() < 0.01,
        "拖过 3px 以后选框该出现：{style:?}"
    );
    let left = start.0.min(end.0);
    let top = start.1.min(end.1);
    let width = (end.0 - start.0).abs();
    let height = (end.1 - start.1).abs();
    assert!(
        (bounds.x - left).abs() <= 2.0 && (bounds.y - top).abs() <= 2.0,
        "选框左上应落在指针范围上：{bounds:?} 期望 ({left}, {top})"
    );
    assert!(
        (bounds.width - width).abs() <= 2.0 && (bounds.height - height).abs() <= 2.0,
        "选框大小应是指针两端的距离：{bounds:?} 期望 {width} x {height}"
    );
    assert_eq!(
        style.interaction.base.background_mix,
        Some(nana_ui_core::SemanticColorMix::alpha(
            nana_ui::runtime::SemanticColorRole::Accent,
            0.18
        )),
        "选框底色是强调色 18%"
    );
    assert_eq!(
        style.interaction.base.border_mix,
        Some(nana_ui_core::SemanticColorMix::new(
            nana_ui::runtime::SemanticColorRole::Accent,
            nana_ui::runtime::SemanticColorRole::OnMedia,
            0.82,
        )),
        "选框描边是强调色 82% 混白"
    );
    assert!(
        selected(&harness).iter().any(|path| path == "cover.png"),
        "选框盖住 cover.png 时它该被选中"
    );

    harness.pointer(PointerPhase::Up, end.0, end.1, PLAIN);
    harness.frame();
    assert!(box_opacity(&harness) < 0.01, "松手以后选框消失");
    assert!(
        selected(&harness).iter().any(|path| path == "cover.png"),
        "松手以后选择留下"
    );
}

/// 先选中 page.pdf，再从列表上方空白处拖到 cover.png：按着 Ctrl 时框住的并进原来的选择，不按时替换。
#[test]
fn control_box_select_appends_to_the_selection() {
    for (modifiers, expected) in [
        (CONTROL, vec!["assets", "cover.png", "notes/page.pdf"]),
        (PLAIN, vec!["assets", "cover.png"]),
    ] {
        let mut harness = ShellHarness::mount(list_page());
        click(&mut harness, "page.pdf", PLAIN);
        let start = empty_list_point(&harness, "cover.png");
        let end = center(&card(&harness, "cover.png"));
        harness.pointer(PointerPhase::Down, start.0, start.1, modifiers);
        harness.frame();
        harness.pointer(PointerPhase::Move, end.0, end.1, modifiers);
        harness.frame();
        harness.pointer(PointerPhase::Up, end.0, end.1, modifiers);
        harness.frame();
        assert_eq!(selected(&harness), expected, "修饰键 {modifiers:?}");
    }
}

/// 选框节点的样式和布局。节点一直在，没拖时透明且没有尺寸。
fn selection_box(
    harness: &ShellHarness,
) -> (nana_ui::runtime::NodeStyle, nana_ui::runtime::LayoutBox) {
    let id = harness
        .keyed("file-selection-box")
        .expect("列表里应该有选框");
    let world = harness.document().context().world();
    let style = world.node_style(id).expect("选框没有样式").clone();
    let bounds = world.layout_box(id).expect("选框没有布局");
    (style, bounds)
}

fn box_opacity(harness: &ShellHarness) -> f32 {
    selection_box(harness).0.layout.opacity.unwrap_or(1.0)
}

//! 文件右键菜单是 NanaUI 的 `AnchoredActionMenu`：打开时焦点落在首项，方向键在项之间走；
//! 指针或焦点进到分组那一行（含子菜单）就在右边展开子菜单，都离开就收起；点分组不收起菜单；
//! 点外面、Escape、在菜单上按右键都收起，收起以后浮层层空出来；禁用的分组不展开。

use crate::backend::services::repository::PlaylistSummary;
use crate::shell::files::FilesMessage;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{GapMessage, ShellMessage, SidebarMessage, WorkspacePanel};
use nana_ui::runtime::{LayoutBox, StableNodeId};

/// 「缩略图」分组的最后一项。
const LAST_CHILD: &str = "刷新缩略图";

/// 打开着 cover.png 右键菜单的文件页。
fn menu() -> ShellHarness {
    let model = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == "live-menu")
        .expect("场景 live-menu")
        .1;
    let mut harness = ShellHarness::mount(model);
    harness.take_messages();
    harness
}

fn node(harness: &ShellHarness, key: &str) -> StableNodeId {
    harness
        .keyed(key)
        .unwrap_or_else(|| panic!("菜单缺少 {key}"))
}

/// 键是 `key` 的节点的中心。
fn point(harness: &ShellHarness, key: &str) -> (f32, f32) {
    harness.center(node(harness, key))
}

fn bounds(harness: &ShellHarness, id: StableNodeId) -> LayoutBox {
    harness
        .document()
        .context()
        .world()
        .layout_box(id)
        .expect("布局盒")
}

/// 浮层层里还有几块。菜单收起以后应该一块都不剩，页面不再被挡住。
fn overlay_blocks(harness: &ShellHarness) -> usize {
    let world = harness.document().context().world();
    let layer = harness.keyed("shell-overlay").expect("浮层层");
    world.node(layer).map_or(0, |node| node.children.len())
}

fn children_shown(harness: &ShellHarness) -> bool {
    harness.find(LAST_CHILD).is_some()
}

/// 打开时框架把焦点放进首项；方向键、Home、End 在项之间走，两头相接。
#[test]
fn the_menu_opens_on_its_first_item_and_arrows_walk_it() {
    let mut harness = menu();
    assert_eq!(
        harness.focused(),
        harness.keyed("file-menu-preview"),
        "打开以后焦点在首项「预览」"
    );
    harness.press_key("ArrowDown");
    assert_eq!(harness.focused(), harness.keyed("file-menu-open"));
    harness.press_key("End");
    assert_eq!(harness.focused(), harness.keyed("file-menu-delete"));
    harness.press_key("ArrowDown");
    assert_eq!(
        harness.focused(),
        harness.keyed("file-menu-preview"),
        "越过末项回到首项"
    );
    assert!(harness.model.files.entry_menu.is_some(), "走动焦点不关菜单");
}

/// 把指针移到键是 `key` 的节点中间，这一下发出的消息逐条归约。
fn hover(harness: &mut ShellHarness, key: &str) {
    let (x, y) = point(harness, key);
    harness.pointer(
        nana_ui::PointerPhase::Move,
        x,
        y,
        nana_ui::InputModifiers::default(),
    );
    let messages = harness.take_messages();
    harness.settle(messages);
}

/// 指针进到分组那一行，子菜单在右边 4px 处展开，顶边和这一行齐；指针移进子菜单还开着，
/// 移到别的项上就收起，没有延迟。
#[test]
fn hovering_a_group_opens_its_submenu_beside_it() {
    let mut harness = menu();
    assert!(!children_shown(&harness), "子菜单一开始收着");
    hover(&mut harness, "file-menu-thumbnail");
    assert_eq!(harness.model.files.menu_branch.open(), Some("thumbnail"));
    assert!(children_shown(&harness), "指针进来就展开");
    let group = bounds(&harness, node(&harness, "file-menu-thumbnail"));
    let first = bounds(&harness, node(&harness, "file-menu-thumbnail-custom-file"));
    assert!(
        first.x >= group.x + group.width,
        "子菜单在分组右边：{group:?} {first:?}"
    );
    assert!(
        (first.y - group.y).abs() < 8.0,
        "子菜单顶边和分组齐：{group:?} {first:?}"
    );

    hover(&mut harness, "file-menu-thumbnail-custom-file");
    assert!(children_shown(&harness), "移进子菜单还开着");
    hover(&mut harness, "file-menu-preview");
    assert!(!children_shown(&harness), "移到别的项上就收起");
    assert!(harness.model.files.entry_menu.is_some());
}

/// 点分组本身不收起菜单，焦点落在分组上，子菜单跟着开着。
#[test]
fn clicking_a_group_keeps_the_menu_open() {
    let mut harness = menu();
    let (x, y) = point(&harness, "file-menu-thumbnail");
    let clicked = harness.click_messages(x, y);
    assert!(
        clicked.iter().all(|message| matches!(
            message,
            ShellMessage::Files(FilesMessage::MenuBranchChanged { inside: true, .. })
        )),
        "点分组只让子菜单展开：{} 条消息",
        clicked.len()
    );
    harness.settle(clicked);
    assert!(harness.model.files.entry_menu.is_some(), "点分组不关菜单");
    assert!(children_shown(&harness), "子菜单开着");
}

/// 键盘：焦点走到分组上子菜单就展开，往下一格进到子菜单的首项，走出子菜单就收起。
#[test]
fn focus_on_a_group_opens_its_submenu_and_arrows_walk_into_it() {
    let mut harness = menu();
    let group = node(&harness, "file-menu-thumbnail");
    harness.focus(group);
    let messages = harness.take_messages();
    harness.settle(messages);
    assert!(children_shown(&harness), "焦点进来就展开");
    harness.press_key("ArrowDown");
    assert_eq!(
        harness.focused(),
        harness.keyed("file-menu-thumbnail-custom-file"),
        "方向键走进子菜单"
    );
    assert!(children_shown(&harness), "焦点在子菜单里还开着");
    harness.press_key("ArrowUp");
    harness.press_key("ArrowUp");
    assert_ne!(harness.focused(), Some(group));
    assert!(!children_shown(&harness), "焦点走出分组就收起");
}

/// 回收站里「缩略图」是禁用的：指针和焦点进来都不展开子菜单。
#[test]
fn a_disabled_group_does_not_open() {
    let mut model = menu().model;
    model.workspace.panel = WorkspacePanel::Trash;
    let mut harness = ShellHarness::mount(model);
    harness.take_messages();
    hover(&mut harness, "file-menu-thumbnail");
    assert!(!children_shown(&harness), "禁用的分组展开了子菜单");
}

/// 点在菜单外面：框架收起菜单，只发关闭消息，这一下不落到下面的页面；浮层层随即空出来。
#[test]
fn a_press_outside_closes_the_menu_and_frees_the_page() {
    let mut harness = menu();
    let messages = harness.click_messages(1150.0, 760.0);
    assert!(
        matches!(
            messages.as_slice(),
            [ShellMessage::Files(FilesMessage::CloseEntryMenu)]
        ),
        "点在菜单外只收起：{} 条消息",
        messages.len()
    );
    harness.settle(messages);
    assert!(harness.model.files.entry_menu.is_none());
    assert_eq!(
        overlay_blocks(&harness),
        0,
        "菜单收起以后浮层层不再挡住页面"
    );
}

/// Escape 由框架交给菜单：只发关闭消息，不再走全局 Escape。
#[test]
fn escape_closes_the_menu_through_the_framework() {
    let mut harness = menu();
    let messages = harness.escape_messages();
    assert!(
        matches!(
            messages.as_slice(),
            [ShellMessage::Files(FilesMessage::CloseEntryMenu)]
        ),
        "Escape 收起菜单，不另发全局 Escape：{} 条消息",
        messages.len()
    );
    assert!(!messages.iter().any(|message| matches!(
        message,
        ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::Escape))
    )));
    harness.settle(messages);
    assert!(harness.model.files.entry_menu.is_none());
    assert_eq!(overlay_blocks(&harness), 0);
}

/// ViewModel 自己关掉菜单（滚动、失焦、窗口缩放都走这条）：浮层块换下这一块，经宿主收起，
/// 不再回头发关闭消息，浮层层空出来。
#[test]
fn closing_from_the_view_model_dismisses_the_menu_quietly() {
    let mut harness = menu();
    harness.apply(ShellMessage::Files(FilesMessage::CloseEntryMenu));
    harness.flush();
    assert!(harness.take_messages().is_empty(), "换下时不再发关闭消息");
    assert_eq!(overlay_blocks(&harness), 0);
    assert!(harness.keyed("file-context-menu").is_none());
}

/// 在已经打开的菜单上再按右键：菜单自己没有提供者，右键只把菜单收起，
/// 不执行这一项。文件菜单和文件夹菜单共用这一层。
#[test]
fn a_right_click_on_the_open_menu_closes_it() {
    let mut harness = menu();
    let rename = point(&harness, "file-menu-rename");
    let messages = harness.secondary_click_messages(rename.0, rename.1);
    assert!(
        matches!(
            messages.as_slice(),
            [ShellMessage::Files(FilesMessage::CloseEntryMenu)]
        ),
        "右键落在菜单上应只收起，不执行这一项：{} 条消息",
        messages.len()
    );
    harness.settle(messages);
    assert!(
        harness.model.files.entry_menu.is_none(),
        "右键以后菜单还开着"
    );
    assert!(harness.take_messages().is_empty(), "收起只发一次关闭消息");
}

/// 点「刷新缩略图」请后端按源文件重新生成（`refresh`），不再把条目自己的路径当缩略图文件去解码；结果写回以后
/// 重新解码同一个缓存文件，纹理跟着换。点了以后框架收起菜单。
#[test]
fn refreshing_a_thumbnail_asks_the_backend_to_regenerate_it() {
    let mut harness = menu();
    harness.model.files.take_effects();
    let (x, y) = point(&harness, "file-menu-thumbnail");
    let clicked = harness.click_messages(x, y);
    harness.settle(clicked);
    let (x, y) = harness.center(harness.node(LAST_CHILD));
    let clicked = harness.click_messages(x, y);
    assert!(
        clicked
            .iter()
            .any(|message| matches!(message, ShellMessage::Files(FilesMessage::CloseEntryMenu))),
        "选了一项菜单收起"
    );
    for message in clicked {
        harness.apply(message);
    }
    assert!(harness.model.files.entry_menu.is_none());
    let effects = harness.model.files.take_effects();
    let repo_id = harness
        .model
        .workspace
        .active_repo_id
        .clone()
        .expect("验收仓库");
    assert!(
        matches!(
            effects.as_slice(),
            [crate::shell::FilesEffect::MutateThumbnail { repo_id: requested, path, kind, action, source_path: None, image_bytes: None }]
                if *requested == repo_id && path == "cover.png" && kind == "file" && action == "refresh"
        ),
        "刷新缩略图要请后端重新生成：{effects:?}"
    );

    let cached = "C:/acceptance/.momo/thumbnails/cover.jpg".to_string();
    harness.apply(ShellMessage::Files(FilesMessage::ThumbnailSaved {
        path: "cover.png".into(),
        thumbnail_path: Some(cached.clone()),
        custom: false,
    }));
    let effects = harness.model.files.take_effects();
    assert!(
        matches!(effects.as_slice(), [crate::shell::FilesEffect::DecodeThumbnails { paths }] if paths == &[cached.clone()]),
        "刷新回来以后重新解码缓存文件：{effects:?}"
    );
}

/// 播放集子项画的是播放集名称。动作标签是「加入 早晨」，展开后画面上这一项是「早晨」。
#[test]
fn playlist_group_draws_the_playlist_name() {
    let mut model = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == "live-menu")
        .expect("场景 live-menu")
        .1;
    model
        .files
        .entry_menu
        .as_mut()
        .expect("场景里文件右键菜单是开着的")
        .path = "assets".into();
    let repo_id = model.sidebar.bound_repo_id().expect("验收仓库").to_string();
    model.reduce(ShellMessage::Sidebar(
        SidebarMessage::SidebarPlaylistsLoaded {
            repo_id,
            result: Ok(vec![morning_playlist()]),
        },
    ));
    model.reduce(ShellMessage::Files(FilesMessage::MenuBranchChanged {
        branch: "playlist-membership".into(),
        source: crate::shell::files::BranchSource::Pointer,
        inside: true,
    }));
    let harness = ShellHarness::mount(model);
    let drawn = harness.nodes().into_iter().any(|node| {
        node.label.as_deref() == Some("早晨")
            && harness
                .document()
                .context()
                .assembly_path(node.id)
                .is_some_and(|path| path.contains("file-menu-playlist-"))
    });
    assert!(drawn, "菜单上可见的是播放集名称「早晨」");
}

/// 夹具目录 `assets` 能加入的那张播放集。名称和「加入 早晨」那条动作标签是同一张。
fn morning_playlist() -> PlaylistSummary {
    PlaylistSummary {
        playlist_id: "pl-1".into(),
        repo_id: "acceptance-repo".into(),
        name: "早晨".into(),
        player_type_id: "momobako.playlist.audio-sequence".into(),
        player_plugin_id: "momobako.player.audio".into(),
        player_label: "音频顺序播放".into(),
        file_class: "audio".into(),
        item_count: 0,
        sort_order: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

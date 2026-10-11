//! 焦点归还：从真实的键盘和指针输入打开菜单、弹层和对话框，关掉以后看焦点落在哪。
//! 打开它的控件还在就回到它；随菜单、弹层收起了就回到菜单、弹层的主人（文件条目、文件夹行、仓库头、
//! 「导入」）；被删掉了就回到它那一片里合理的控件；没有打开它的控件（自己弹出来的对话框）时回到
//! 它所属的区域。

use nana_ui::runtime::StableNodeId;

use crate::backend::services::repository::{FileBrowserSnapshot, FileTreeNode, RepositoryStructureCacheState};
use crate::shell::files::{FileDialog, FilesMessage};
use crate::shell::sidebar::{PopoverMode, SidebarSmartFolder, SidebarTree};
use crate::shell::sidebar_view::folder_row_key;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{AdminMessage, ShellMessage, ShellViewModel, SidebarMessage};

/// 验收底子里的仓库。
const REPO: &str = "acceptance-repo";
/// 仓库头的无障碍名。
const HEADER: &str = "资源库 · 默认资源库";
/// 加进插件列表、选了以后进后端表单的来源。
const FORM_BACKEND: &str = "WebDAV 测试";

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 焦点放到无障碍名是 `label` 的控件上，按一下 `key`。
fn press_on(harness: &mut ShellHarness, label: &str, key: &'static str) {
    let id = harness.node(label);
    harness.focus(id);
    harness.press_key(key);
}

/// 按 Escape，再把开合动效走完、照生产准备帧的收尾还焦点。
fn escape(harness: &mut ShellHarness) {
    harness.press_escape();
    harness.finish_motion();
}

/// 对话框里无障碍名是 `label` 的控件（页面上可能有同名的）。
fn in_dialog(harness: &ShellHarness, label: &str) -> StableNodeId {
    let surface = harness.keyed("dialog-surface").expect("对话框");
    let world = harness.document().context().world();
    harness
        .nodes()
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label) && world.is_descendant_or_self(node.id, surface))
        .unwrap_or_else(|| panic!("对话框里没有 {label}"))
        .id
}

/// 焦点在文件条目卡片上。
fn on_card(harness: &ShellHarness) -> bool {
    harness.focused().and_then(|id| harness.document().context().assembly_path(id)).is_some_and(|path| {
        path.rsplit('/').next().is_some_and(|key| key.starts_with("file-row-"))
    })
}

/// 菜单里的项打开对话框：键盘和指针打开的右键菜单都一样，对话框关掉以后焦点回到这个文件条目。
#[test]
fn a_dialog_from_the_entry_menu_returns_focus_to_the_entry() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let card = harness.node("cover.png");
    press_on(&mut harness, "cover.png", "ContextMenu");
    while harness.focused_label().as_deref() != Some("复制到…") {
        harness.press_key("ArrowDown");
    }
    harness.press_key("Enter");
    assert_eq!(harness.model.files.dialog, FileDialog::Copy);
    harness.finish_motion();
    assert_eq!(harness.focused_label().as_deref(), Some("复制到文件夹"), "焦点进了对话框");
    escape(&mut harness);
    assert_eq!(harness.model.files.dialog, FileDialog::Closed);
    assert_eq!(harness.focused(), Some(card), "键盘：关掉以后回到文件条目");

    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let card = harness.node("cover.png");
    harness.right_click(card);
    assert!(harness.model.files.entry_menu.is_some(), "右键打开条目菜单");
    harness.click_label("复制到…");
    harness.finish_motion();
    assert_eq!(harness.focused_label().as_deref(), Some("复制到文件夹"));
    harness.click_label("取消");
    harness.finish_motion();
    assert_eq!(harness.model.files.dialog, FileDialog::Closed);
    assert_eq!(harness.focused(), Some(card), "指针：关掉以后回到右键的那个文件条目");
}

/// 右键菜单换到另一个条目上重新打开（焦点从菜单回到页面、在另一张卡片上按菜单键）：关掉以后回到
/// 后来的这个条目，不回到第一次打开它的那个。
#[test]
fn a_reopened_entry_menu_returns_focus_to_its_own_entry() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    press_on(&mut harness, "cover.png", "ContextMenu");
    let pdf = harness.node("page.pdf");
    press_on(&mut harness, "page.pdf", "ContextMenu");
    assert!(harness.model.files.entry_menu.as_ref().is_some_and(|menu| menu.path == "notes/page.pdf"));
    assert_eq!(harness.focused_label().as_deref(), Some("预览"), "焦点进了新的菜单");
    escape(&mut harness);
    assert!(harness.model.files.entry_menu.is_none());
    assert_eq!(harness.focused(), Some(pdf));
}

/// 「导入」菜单里的项打开对话框，关掉以后焦点回到「导入」。
#[test]
fn an_import_dialog_returns_focus_to_the_import_button() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let import = harness.node("导入");
    harness.click_label("导入");
    harness.click_label("从文件夹导入");
    harness.finish_motion();
    assert_eq!(harness.model.files.dialog, FileDialog::Import);
    escape(&mut harness);
    assert_eq!(harness.focused(), Some(import), "指针：关掉以后回到「导入」");

    press_on(&mut harness, "导入", "ArrowDown");
    assert_eq!(harness.focused_label().as_deref(), Some("从文件夹导入"));
    harness.press_key("Enter");
    harness.finish_motion();
    assert_eq!(harness.model.files.dialog, FileDialog::Import);
    escape(&mut harness);
    assert_eq!(harness.focused(), Some(import), "键盘：关掉以后回到「导入」");
}

/// 插件列表里多一个既不选目录、也不登录的来源：添加菜单里选它进后端表单。
fn with_form_backend() -> ShellViewModel {
    let mut model = scene("live-files-plain");
    let mut plugin = model
        .admin
        .plugins
        .iter()
        .find(|plugin| plugin.plugin_id == "momobako.local-filesystem")
        .cloned()
        .expect("内置的本地文件夹来源");
    plugin.plugin_id = "momobako.source.webdav-test".into();
    plugin.name = FORM_BACKEND.into();
    plugin.capabilities.clear();
    model.admin.plugins.push(plugin);
    model
}

/// 后端表单：从添加菜单换过来时焦点落在第一个输入框；关掉以后焦点回到仓库头（添加菜单的项和表单都随
/// 弹层收起了）。
#[test]
fn the_backend_form_takes_focus_and_gives_it_back_to_the_repository_header() {
    let mut harness = ShellHarness::mount(with_form_backend());
    let header = harness.node(HEADER);
    press_on(&mut harness, HEADER, "ArrowDown");
    harness.press_key("ArrowDown");
    assert_eq!(harness.focused_label().as_deref(), Some("添加资源库"));
    harness.press_key("Enter");
    assert_eq!(harness.model.sidebar.popover, PopoverMode::AddMenu);
    harness.press_key("ArrowUp");
    assert_eq!(harness.focused_label().as_deref(), Some(FORM_BACKEND), "越过首项到末项");
    harness.press_key("Enter");
    assert_eq!(harness.model.sidebar.popover, PopoverMode::BackendForm);
    assert_eq!(harness.focused(), Some(harness.input("资源库名称")), "换到表单，焦点落在第一个输入框");
    escape(&mut harness);
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Closed);
    assert_eq!(harness.focused(), Some(header), "键盘：表单关掉以后回到仓库头");

    let mut harness = ShellHarness::mount(with_form_backend());
    let header = harness.node(HEADER);
    harness.click_label(HEADER);
    harness.click_label("添加资源库");
    harness.click_label(FORM_BACKEND);
    assert_eq!(harness.model.sidebar.popover, PopoverMode::BackendForm);
    assert_eq!(harness.focused(), Some(harness.input("资源库名称")));
    harness.click_label("取消");
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Closed);
    assert_eq!(harness.focused(), Some(header), "指针：取消以后回到仓库头");
}

/// 仓库弹层里的「删除当前资源库」打开对话框：取消以后回到仓库头。
#[test]
fn the_repository_delete_dialog_returns_focus_to_the_repository_header() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let header = harness.node(HEADER);
    harness.click_label(HEADER);
    harness.click_label("删除当前资源库");
    harness.finish_motion();
    assert!(!harness.model.workspace.dialogs.is_empty(), "删除资源库的对话框打开");
    escape(&mut harness);
    assert!(harness.model.workspace.dialogs.is_empty());
    assert_eq!(harness.focused(), Some(header));
}

/// 文件夹菜单里的项打开对话框：关掉以后回到右键的那一行。
#[test]
fn a_dialog_from_the_folder_menu_returns_focus_to_the_folder_row() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let row = harness.keyed(&folder_row_key("assets")).expect("文件夹行");
    harness.right_click(row);
    assert!(harness.model.sidebar.folder_menu.is_some());
    harness.click_label("重命名");
    harness.finish_motion();
    assert!(harness.model.sidebar.folder_dialog.open);
    escape(&mut harness);
    assert!(!harness.model.sidebar.folder_dialog.open);
    assert_eq!(harness.focused(), Some(row));
}

/// 删除确认：确认以后对话框关掉、焦点先回到这一行，目录树重读、这一行没了，焦点落在同一棵树里还在的
/// 文件夹行上。
#[test]
fn a_deleted_folder_hands_focus_to_its_tree() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.click_label("展开文件夹");
    let covers_key = folder_row_key("assets/covers");
    let covers = harness.keyed(&covers_key).expect("展开以后有 covers");
    harness.focus(covers);
    harness.press_key("ContextMenu");
    harness.press_key("ArrowUp");
    assert_eq!(harness.focused_label().as_deref(), Some("删除"));
    harness.press_key("Enter");
    harness.finish_motion();
    assert!(harness.model.sidebar.folder_delete_open(), "处理文件夹的对话框打开");
    harness.click_label("移入回收站");
    assert!(harness.model.files.mutating, "交给文件服务删除");
    harness.apply(ShellMessage::Files(FilesMessage::MutationSnapshot { result: Ok(root_snapshot()), created_name: None }));
    harness.finish_motion();
    assert!(!harness.model.sidebar.folder_delete_open(), "删完关掉对话框");
    assert_eq!(harness.focused(), Some(covers), "目录树还没重读，焦点先回到这一行");

    let assets = FileTreeNode { path: "assets".into(), label: "assets".into(), file_count: 0, children: Vec::new() };
    harness.apply(ShellMessage::Sidebar(SidebarMessage::SidebarTreeLoaded {
        repo_id: REPO.into(),
        result: Ok(SidebarTree::from_nodes(&[assets])),
    }));
    harness.frame();
    assert!(harness.keyed(&covers_key).is_none(), "covers 已经不在树里");
    assert_eq!(harness.focused(), harness.keyed(&folder_row_key("assets")), "焦点落在同一棵树里还在的文件夹行上");
}

/// 删掉 covers 以后读回的根目录。
fn root_snapshot() -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: REPO.into(),
        root_path: "C:/acceptance".into(),
        backend_plugin_id: "momobako.source.local-filesystem".into(),
        backend_kind: "local-filesystem".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: String::new(),
        total_entries: 0,
        loaded_count: 0,
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries: Vec::new(),
    }
}

/// 两个智能文件夹，id 是名字。
fn smart(names: &[&str]) -> Vec<SidebarSmartFolder> {
    names
        .iter()
        .map(|name| SidebarSmartFolder { id: name.to_string(), parent_id: None, name: name.to_string(), filter: Default::default(), children: Vec::new() })
        .collect()
}

/// 删除智能文件夹：从行上的删除按钮确认，删完行随列表换新没了，焦点落在同一棵树里还在的智能文件夹上。
#[test]
fn a_deleted_smart_folder_hands_focus_to_its_tree() {
    let mut model = scene("live-files-plain");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SidebarSmartFoldersLoaded { repo_id: REPO.into(), result: Ok(smart(&["高评分", "待整理"])) }));
    let mut harness = ShellHarness::mount(model);
    press_on(&mut harness, "删除智能文件夹", "Enter");
    harness.finish_motion();
    assert!(harness.model.sidebar.smart_delete_open(), "删除智能文件夹的确认打开");
    let confirm = harness.keyed("smart-delete-confirm").expect("确认删除");
    harness.focus(confirm);
    harness.press_key("Enter");
    harness.apply(ShellMessage::Sidebar(SidebarMessage::SmartFolderSaved { repo_id: REPO.into(), result: Ok(smart(&["待整理"])) }));
    harness.finish_motion();
    assert!(!harness.model.sidebar.smart_delete_open(), "删完关掉确认");
    assert_eq!(harness.focused(), harness.keyed(&format!("smart-row-{}", "待整理")), "焦点落在还在的智能文件夹上");
}

/// 删除插件：确认以后插件列表换新、这张卡片没了，焦点落在同一组里还在的插件上，不回到页首。
#[test]
fn a_deleted_plugin_hands_focus_to_its_group() {
    let mut model = scene("live-files-plain");
    let template = model.admin.plugins.iter().find(|plugin| plugin.plugin_id == "momobako.local-filesystem").cloned().expect("内置插件");
    for id in ["user.one", "user.two"] {
        let mut plugin = template.clone();
        plugin.plugin_id = id.into();
        plugin.name = id.into();
        plugin.source = "user".into();
        model.admin.plugins.push(plugin);
    }
    let remaining = model.admin.plugins.iter().filter(|plugin| plugin.plugin_id != "user.one").cloned().collect::<Vec<_>>();
    let mut harness = ShellHarness::mount(model);
    harness.apply(ShellMessage::OpenSettings);
    // 设置包已经读回来（插件列表就是上面这份）。
    harness.model.admin.loading_settings = false;
    harness.model.mark_surface_dirty();
    harness.sync();
    harness.flush();
    let delete = harness.keyed("admin-plugin-delete-user.one").expect("user.one 的删除");
    harness.focus(delete);
    harness.press_key("Enter");
    harness.finish_motion();
    assert_eq!(harness.model.admin.pending_delete.as_deref(), Some("user.one"));
    let confirm = in_dialog(&harness, "删除");
    harness.focus(confirm);
    harness.press_key("Enter");
    harness.apply(ShellMessage::Admin(AdminMessage::PluginsReplaced(Ok(remaining))));
    harness.finish_motion();
    assert!(harness.model.admin.pending_delete.is_none(), "删完关掉确认");
    assert!(harness.keyed("admin-plugin-card-user.one").is_none(), "卡片已经没了");
    let path = harness.focused().and_then(|id| harness.document().context().assembly_path(id)).unwrap_or_default();
    assert!(path.contains("admin-plugin-section-"), "焦点落在插件分组里，实际是 {path}");
}

/// 任务弹层：从「任务」打开，点弹层里的关闭以后回到「任务」。
#[test]
fn the_task_popover_returns_focus_to_its_button() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let tasks = harness.node("任务");
    harness.click_label("任务");
    assert!(harness.model.admin.popover_open);
    harness.click_label("关闭任务");
    assert!(!harness.model.admin.popover_open);
    assert_eq!(harness.focused(), Some(tasks));
}

/// 侧栏的新建按钮打开的对话框：关掉以后回到这个按钮。
#[test]
fn sidebar_create_dialogs_return_focus_to_their_buttons() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    for (button, open) in [
        ("在当前目录新建文件夹", (|model: &ShellViewModel| model.sidebar.folder_dialog.open) as fn(&ShellViewModel) -> bool),
        ("新建智能文件夹", |model| model.sidebar.smart_draft.open),
        ("新建播放集", |model| model.playlist_dialog_open),
    ] {
        let id = harness.node(button);
        press_on(&mut harness, button, "Enter");
        harness.finish_motion();
        assert!(open(&harness.model), "{button} 打开对话框");
        escape(&mut harness);
        assert!(!open(&harness.model));
        assert_eq!(harness.focused(), Some(id), "{button}：关掉以后回到按钮");
    }
}

/// 自己弹出来的对话框（打开前没有焦点）：关掉以后焦点落在它所属的文件列表里的条目上。
#[test]
fn dialogs_without_an_opener_hand_focus_to_their_area() {
    for name in ["hardlink-dialog", "export-dialog", "source-download-target"] {
        let mut harness = ShellHarness::mount(scene(name));
        harness.finish_motion();
        assert!(harness.focused().is_some(), "{name}：焦点进了对话框");
        escape(&mut harness);
        assert!(on_card(&harness), "{name}：关掉以后焦点在文件条目上，实际是 {:?}", harness.focused_label());
    }
}

/// 换着打开的对话框（关闭确认压在新建文件夹上）照旧按打开的先后还回去，最后回到打开新建文件夹的按钮。
#[test]
fn stacked_dialogs_still_hand_focus_back_in_order() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.model.settings.close_behavior = "confirm".into();
    press_on(&mut harness, "在当前目录新建文件夹", "Enter");
    harness.finish_motion();
    harness.apply(ShellMessage::WindowAction(crate::shell::WindowAction::Close));
    harness.finish_motion();
    assert!(harness.model.input.pending_close, "关闭确认压在上面");
    escape(&mut harness);
    assert!(!harness.model.input.pending_close && harness.model.sidebar.folder_dialog.open, "新建文件夹回来");
    escape(&mut harness);
    assert!(!harness.model.sidebar.folder_dialog.open);
    assert_eq!(harness.focused(), Some(harness.node("在当前目录新建文件夹")), "回到打开新建文件夹的按钮");
}

//! 统一对话框框架的回归：每个对话框都由宿主激活；三种关闭手势（Escape、点外面、关闭位）
//! 都只发一次自己的关闭消息，处理中一条也不发；打开期间无关更新不换节点，相关
//! 更新只改绑定的字段；对话框里的输入框在组合输入中不被打断；文件页的对话框只在浮层里出现一份；
//! 浮层的先后和 Escape 一致。

use nana_ui::runtime::{AccessibilityRole, StableNodeId};

use super::OverlayKey;
use crate::backend::services::repository::{SystemLogLocation, SystemLogRecord, SystemLogSource, TaskProgressSnapshot};
use crate::shell::files::{FileDialog, FilesMessage};
use crate::shell::host_events::HostMessage;
use crate::shell::input::InputMessage;
use crate::shell::player::PlayerMessage;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{AdminMessage, GapMessage, InspectMessage, ShellMessage, ShellPage, ShellViewModel, SidebarMessage, ThumbnailFrame};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 和对话框无关的后台消息：日志广播、缩略图、任务进度、播放音量和标题栏搜索。
fn background_messages() -> Vec<ShellMessage> {
    vec![
        ShellMessage::Host(HostMessage::LogRecorded(SystemLogRecord {
            id: "log-1".into(),
            timestamp: "2026-10-09T08:00:00Z".into(),
            level: "info".into(),
            category: "repository".into(),
            action: "sync".into(),
            message: "后台同步完成".into(),
            source: SystemLogSource { kind: "host".into(), label: Some("MomoBako".into()), plugin_id: None, repo_id: None },
            location: SystemLogLocation::default(),
            context: serde_json::json!({}),
        })),
        ShellMessage::ThumbnailPixels(vec![ThumbnailFrame {
            path: "cover.png".into(),
            natural_width: 2,
            natural_height: 2,
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        }]),
        ShellMessage::TaskProgressLoaded(vec![TaskProgressSnapshot {
            task_id: "task-1".into(),
            protocol_id: "momobako.sync".into(),
            status: "running".into(),
            phase: Some("scanning".into()),
            label: Some("扫描文件".into()),
            current: Some(4),
            total: Some(10),
            percent: Some(40.0),
            error: None,
            updated_at: "now".into(),
        }]),
        ShellMessage::Player(PlayerMessage::SetVolume(0.5)),
        ShellMessage::Inspect(InspectMessage::SetQuery("封面".into())),
    ]
}

/// 一个对话框的用例。
struct Case {
    name: &'static str,
    model: fn() -> ShellViewModel,
    key: OverlayKey,
    /// 这个对话框的关闭消息。
    closes: fn(&ShellMessage) -> bool,
    /// 把它标成处理中；没有处理中状态的为 `None`。
    busy: Option<fn(&mut ShellViewModel)>,
    /// 标题右侧有关闭位。
    close_button: bool,
}

fn opened(name: &str, message: ShellMessage) -> ShellViewModel {
    let mut model = scene(name);
    model.reduce(message);
    model
}

fn gap(message: GapMessage) -> ShellMessage {
    ShellMessage::Sidebar(SidebarMessage::Gap(message))
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "folder-create-dialog",
            model: || scene("folder-create-dialog"),
            key: OverlayKey::FolderDialog,
            closes: |message| matches!(message, ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseFolderDialog))),
            busy: Some(|model| model.sidebar.folder_dialog.submitting = true),
            close_button: false,
        },
        Case {
            name: "folder-delete",
            model: || opened("live-files-plain", gap(GapMessage::OpenFolderDelete { path: "assets".into(), label: "assets".into() })),
            key: OverlayKey::FolderDelete,
            closes: |message| matches!(message, ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseFolderDelete))),
            busy: Some(|model| model.sidebar.folder_delete_submitting = true),
            close_button: false,
        },
        Case {
            name: "smart-delete",
            model: || opened("live-files-plain", gap(GapMessage::OpenSmartDelete { id: "smart-1".into(), label: "高评分".into() })),
            key: OverlayKey::SmartDelete,
            closes: |message| matches!(message, ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseSmartDelete))),
            busy: Some(|model| model.sidebar.smart_draft.busy = true),
            close_button: false,
        },
        Case {
            name: "repo-delete-dialog",
            model: || scene("repo-delete-dialog"),
            key: OverlayKey::RepositoryDelete,
            closes: |message| matches!(message, ShellMessage::MissingCloseDelete),
            busy: Some(|model| model.workspace.deleting_mode = Some(crate::shell::workspace::DeleteMode::RecordOnly)),
            close_button: false,
        },
        Case {
            name: "plugin-delete",
            model: || {
                let mut model = scene("live-files-plain");
                model.admin.pending_delete = Some("user.plugin".into());
                model
            },
            key: OverlayKey::PluginDelete,
            closes: |message| matches!(message, ShellMessage::Admin(AdminMessage::CancelDelete)),
            busy: Some(|model| model.admin.managing = true),
            close_button: false,
        },
        Case {
            name: "source-playlist",
            model: || {
                opened(
                    "live-files-plain",
                    ShellMessage::Input(InputMessage::OpenSourcePlaylist {
                        plugin_id: "source".into(),
                        method: "playlist.create".into(),
                        payload: serde_json::json!({}),
                        repository_id: None,
                    }),
                )
            },
            key: OverlayKey::SourcePlaylist,
            closes: |message| matches!(message, ShellMessage::Input(InputMessage::CloseSourcePlaylist)),
            busy: None,
            close_button: false,
        },
        Case {
            name: "playlist-create-dialog",
            model: || scene("playlist-create-dialog"),
            key: OverlayKey::PlaylistCreate,
            closes: |message| matches!(message, ShellMessage::ClosePlaylistDialog),
            busy: None,
            close_button: false,
        },
        Case {
            name: "smart-folder-dialog",
            model: || scene("smart-folder-dialog"),
            key: OverlayKey::SmartFolderDialog,
            closes: |message| matches!(message, ShellMessage::Sidebar(SidebarMessage::CloseSmartFolderDialog)),
            busy: Some(|model| model.sidebar.smart_draft.busy = true),
            close_button: false,
        },
        Case {
            name: "copy-dialog",
            model: || scene("copy-dialog"),
            key: OverlayKey::FileDialog,
            closes: |message| matches!(message, ShellMessage::Files(FilesMessage::CloseDialog)),
            busy: Some(|model| model.files.mutating = true),
            close_button: false,
        },
        Case {
            name: "hardlink-dialog",
            model: || scene("hardlink-dialog"),
            key: OverlayKey::FileDialog,
            closes: |message| matches!(message, ShellMessage::Files(FilesMessage::SkipHardlink)),
            busy: Some(|model| model.files.mutating = true),
            close_button: false,
        },
        Case {
            name: "export-dialog",
            model: || scene("export-dialog"),
            key: OverlayKey::ExportDialog,
            closes: |message| matches!(message, ShellMessage::Files(FilesMessage::CloseExport)),
            busy: Some(|model| model.files.export.busy = true),
            close_button: true,
        },
        Case {
            name: "close-confirm",
            model: || {
                let mut model = scene("live-files-plain");
                model.settings.close_behavior = "confirm".into();
                model.reduce(ShellMessage::WindowAction(crate::shell::WindowAction::Close));
                model
            },
            key: OverlayKey::CloseConfirm,
            closes: |message| matches!(message, ShellMessage::Input(InputMessage::ConfirmCloseAnswer(false))),
            busy: None,
            close_button: false,
        },
    ]
}

/// 归约之外直接改了 ViewModel：照生产标脏（状态版本加一），同步并刷新一帧。
fn changed(harness: &mut ShellHarness) {
    harness.model.mark_surface_dirty();
    harness.sync();
    harness.flush();
}

/// 对话框里无障碍名是 `label` 的节点（标题栏也有同名的「关闭」）。
fn within_dialog(harness: &ShellHarness, label: &str) -> StableNodeId {
    let surface = harness.keyed("dialog-surface").expect("对话框");
    let world = harness.document().context().world();
    harness
        .nodes()
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label) && world.is_descendant_or_self(node.id, surface))
        .unwrap_or_else(|| panic!("对话框里没有 {label}"))
        .id
}

/// 窗口左下角：在任何对话框卡片外面。
const OUTSIDE: (f32, f32) = (8.0, 792.0);

fn mounted(case: &Case) -> ShellHarness {
    let harness = ShellHarness::mount((case.model)());
    assert_eq!(OverlayKey::of(&harness.model), Some(case.key), "{}：场景里应该开着这个对话框", case.name);
    let host = harness.keyed("dialog-host").unwrap_or_else(|| panic!("{}：对话框没有挂在宿主下", case.name));
    let surface = harness.keyed("dialog-surface").unwrap_or_else(|| panic!("{}：没有对话框", case.name));
    let active = harness.document().context().world().overlay_host(host).and_then(|state| state.active);
    assert_eq!(active, Some(surface), "{}：对话框应该由宿主激活", case.name);
    harness
}

/// 只有一条消息，就是这个对话框的关闭消息；归约后对话框关了。
fn assert_closes(case: &Case, gesture: &str, harness: &mut ShellHarness, messages: Vec<ShellMessage>) {
    assert_eq!(messages.len(), 1, "{}：{gesture}应该只发一条消息", case.name);
    assert!((case.closes)(&messages[0]), "{}：{gesture}发的不是关闭消息", case.name);
    for message in messages {
        harness.apply(message);
    }
    harness.flush();
    assert_ne!(OverlayKey::of(&harness.model), Some(case.key), "{}：{gesture}后对话框还开着", case.name);
}

#[test]
fn every_dialog_sends_one_close_message_per_gesture() {
    for case in cases() {
        let mut harness = mounted(&case);
        let messages = harness.escape_messages();
        assert_closes(&case, "Escape", &mut harness, messages);

        let mut harness = mounted(&case);
        let messages = harness.click_messages(OUTSIDE.0, OUTSIDE.1);
        assert_closes(&case, "点外面", &mut harness, messages);

        if case.close_button {
            let mut harness = mounted(&case);
            let close = within_dialog(&harness, "关闭");
            let (x, y) = harness.center(close);
            let messages = harness.click_messages(x, y);
            assert_closes(&case, "关闭位", &mut harness, messages);
        }
    }
}

#[test]
fn busy_dialogs_ignore_every_close_gesture() {
    for case in cases() {
        let Some(busy) = case.busy else {
            continue;
        };
        let mut harness = mounted(&case);
        busy(&mut harness.model);
        changed(&mut harness);
        assert!(harness.escape_messages().is_empty(), "{}：处理中 Escape 不该发消息", case.name);
        assert!(harness.click_messages(OUTSIDE.0, OUTSIDE.1).is_empty(), "{}：处理中点外面不该发消息", case.name);
        if case.close_button {
            let close = within_dialog(&harness, "关闭");
            let (x, y) = harness.center(close);
            assert!(harness.click_messages(x, y).is_empty(), "{}：处理中点关闭位不该发消息", case.name);
        }
        assert_eq!(OverlayKey::of(&harness.model), Some(case.key), "{}：处理中对话框应该还开着", case.name);
        assert!(harness.content_roots().0.is_some());
    }
}

/// 浮层根下面所有节点，按文档顺序。
fn subtree(harness: &ShellHarness, root: StableNodeId) -> Vec<StableNodeId> {
    let world = harness.document().context().world();
    let mut nodes = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        nodes.push(id);
        if let Some(node) = world.node(id) {
            stack.extend(node.children.iter().rev().copied());
        }
    }
    nodes
}

#[test]
fn unrelated_updates_keep_every_dialog_node() {
    for case in cases() {
        let mut harness = mounted(&case);
        let root = harness.content_roots().0.expect("对话框在浮层里");
        let before = subtree(&harness, root);
        let remounts = harness.view_stats().remounts;
        for message in background_messages() {
            harness.apply(message);
            harness.flush();
            assert_eq!(harness.content_roots().0, Some(root), "{}：无关更新换掉了浮层根", case.name);
        }
        for _ in 0..6 {
            harness.frame();
        }
        assert_eq!(subtree(&harness, root), before, "{}：无关更新换掉了对话框里的节点", case.name);
        assert!(harness.view_stats().remounts >= remounts);
        harness.assert_same_as_fresh_mount();
    }
}

/// 输入框的文字。
fn value(harness: &ShellHarness, label: &str) -> String {
    harness.value(harness.input(label))
}

#[test]
fn dialog_state_changes_patch_the_bound_fields_in_place() {
    // 新建文件夹：出错、文件服务开始忙，标题、输入框和按钮都还是原来的节点。
    let mut harness = ShellHarness::mount(scene("folder-create-dialog"));
    let root = harness.content_roots().0.expect("对话框");
    let before = subtree(&harness, root);
    harness.model.sidebar.folder_dialog.error = "名称已存在".into();
    harness.model.files.mutating = true;
    changed(&mut harness);
    assert!(harness.find("名称已存在").is_some(), "错误没有显示");
    assert_eq!(subtree(&harness, root), before, "出错不该换节点");
    harness.assert_same_as_fresh_mount();

    // 导出：切到 Git 显示远端、分支和提交信息，压缩包字段藏起；导出中主按钮写「处理中」。
    let mut harness = ShellHarness::mount(scene("export-dialog"));
    let root = harness.content_roots().0.expect("对话框");
    let before = subtree(&harness, root);
    assert!(harness.find("远端").is_none());
    harness.apply(ShellMessage::Files(FilesMessage::SetExportField { field: "target".into(), value: "git".into() }));
    harness.flush();
    assert!(harness.find("远端").is_some(), "切到 Git 后应该有远端");
    assert!(harness.find("格式").is_none(), "切到 Git 后压缩包字段应该藏起");
    harness.model.files.export.busy = true;
    changed(&mut harness);
    assert!(harness.find("处理中").is_some(), "导出中主按钮应该写处理中");
    assert_eq!(subtree(&harness, root), before, "切换分段和导出中都不该换节点");
    harness.assert_same_as_fresh_mount();

    // 删除资源库：删除中左下角出现「处理中...」。
    let mut harness = ShellHarness::mount(scene("repo-delete-dialog"));
    let root = harness.content_roots().0.expect("对话框");
    let before = subtree(&harness, root);
    assert!(harness.find("处理中...").is_none());
    harness.model.workspace.deleting_mode = Some(crate::shell::workspace::DeleteMode::RecordOnly);
    changed(&mut harness);
    assert!(harness.find("处理中...").is_some(), "删除中应该显示处理中");
    assert_eq!(subtree(&harness, root), before);
    harness.assert_same_as_fresh_mount();

    // 文件对话框换了一种（复制换成重命名）：浮层按身份换块。
    let mut harness = ShellHarness::mount(scene("copy-dialog"));
    let root = harness.content_roots().0.expect("对话框");
    assert_eq!(value(&harness, "目标目录"), "notes");
    harness.apply(ShellMessage::Files(FilesMessage::OpenDialog(FileDialog::Rename)));
    harness.flush();
    if OverlayKey::of(&harness.model) == Some(OverlayKey::FileDialog) && harness.model.files.dialog == FileDialog::Rename {
        assert_ne!(harness.content_roots().0, Some(root), "换一种文件对话框应该换块");
        harness.assert_same_as_fresh_mount();
    }
}

/// 每个带输入框的对话框：焦点放进输入框开始组合，后台消息和动效帧都不打断，提交后文字进草稿。
#[test]
fn composing_in_dialog_inputs_is_never_interrupted() {
    let cases: [(&str, fn() -> ShellViewModel, &str); 4] = [
        ("smart-folder-dialog", || scene("smart-folder-dialog"), "名称"),
        ("copy-dialog", || scene("copy-dialog"), "目标目录"),
        ("playlist-create-dialog", || scene("playlist-create-dialog"), "名称"),
        ("source-playlist", || cases().into_iter().find(|case| case.name == "source-playlist").map(|case| (case.model)()).expect("用例"), "播放列表名称"),
    ];
    for (name, model, label) in cases {
        let mut harness = ShellHarness::mount(model());
        let field = harness.input(label);
        harness.focus(field);
        let typed = value(&harness, label);
        harness.compose("pinyin");
        for message in background_messages() {
            harness.apply(message);
            harness.flush();
            assert_eq!(harness.input(label), field, "{name}：后台消息换掉了输入框");
            assert_eq!(harness.preedit(field).as_deref(), Some("pinyin"), "{name}：后台消息打断了组合");
            assert_eq!(harness.focused(), Some(field), "{name}：焦点离开了输入框");
        }
        for _ in 0..6 {
            harness.frame();
            assert_eq!(harness.preedit(field).as_deref(), Some("pinyin"), "{name}：动效帧打断了组合");
        }
        harness.commit("字");
        for message in harness.take_messages() {
            harness.apply(message);
        }
        harness.flush();
        assert_eq!(harness.input(label), field, "{name}：提交后换了输入框");
        let committed = value(&harness, label);
        assert!(committed.contains('字') && committed.chars().count() == typed.chars().count() + 1, "{name}：提交的文字没有进输入框：{committed}");
        harness.assert_same_as_fresh_mount();
    }
}

/// 文件页的对话框原来画在文件页里，现在只在浮层里出现一份。
#[test]
fn file_page_dialogs_show_once_in_the_overlay() {
    for name in ["copy-dialog", "export-dialog", "hardlink-dialog"] {
        let harness = ShellHarness::mount(scene(name));
        let root = harness.content_roots().0.expect("对话框在浮层里");
        let world = harness.document().context().world();
        let dialogs = harness
            .nodes()
            .into_iter()
            .filter(|node| matches!(node.role, AccessibilityRole::Dialog | AccessibilityRole::AlertDialog))
            .collect::<Vec<_>>();
        assert_eq!(dialogs.len(), 1, "{name}：对话框应该只有一份");
        assert!(world.is_descendant_or_self(dialogs[0].id, root), "{name}：对话框不在浮层里");
    }
}

/// 关闭确认不管停在哪个页面都在最上面（原来只在文件页里画）。
#[test]
fn close_confirm_shows_on_every_page_above_other_dialogs() {
    let mut settings = ShellViewModel::for_page(ShellPage::Settings);
    settings.input.pending_close = true;
    settings.input.notice = "确认关闭 MomoBako？".into();
    let harness = ShellHarness::mount(settings);
    assert_eq!(OverlayKey::of(&harness.model), Some(OverlayKey::CloseConfirm));
    assert!(harness.nodes().iter().any(|node| node.role == AccessibilityRole::AlertDialog), "设置页上没有关闭确认");

    let mut model = scene("folder-create-dialog");
    model.input.pending_close = true;
    assert_eq!(OverlayKey::of(&model), Some(OverlayKey::CloseConfirm), "关闭确认应该压在别的对话框上面");
}

/// 浮层的先后：文件页的对话框排在弹层和菜单后面（原来画在主区里），弹层关掉以后才露出来；
/// Escape 按同样的先后一层一层关。
#[test]
fn file_dialogs_wait_behind_popovers_and_escape_follows_the_same_order() {
    let mut model = scene("copy-dialog");
    model.reduce(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    assert_eq!(OverlayKey::of(&model), Some(OverlayKey::TaskPopover));
    let mut harness = ShellHarness::mount(model);
    assert!(harness.find("复制到文件夹").is_none(), "弹层开着时复制对话框排在后面");
    assert!(harness.press_escape());
    assert!(!harness.model.admin.popover_open, "Escape 先关显示着的任务弹层");
    assert_eq!(OverlayKey::of(&harness.model), Some(OverlayKey::FileDialog));
    assert!(harness.find("复制到文件夹").is_some(), "弹层关掉后复制对话框露出来");
    assert!(harness.press_escape());
    assert_eq!(harness.model.files.dialog, FileDialog::Closed, "再按 Escape 关掉复制对话框");
    harness.assert_same_as_fresh_mount();
}

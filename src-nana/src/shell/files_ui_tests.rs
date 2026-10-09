//! 文件页画面状态的测试：去扩展名标题、单击只选中、右键菜单的选中和二次确认、
//! Escape 关闭顺序、标签组展开、列表宽度回报，以及标签草稿和导出保存位置的转发；
//! 还有别处（搜索结果、外部打开）读回详情时照样进入预览。

use std::collections::BTreeMap;

use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::WindowId;

use crate::backend::services::repository::{AssetDetail, AssetSummary, FileBrowserEntry};
use crate::shell::{ShellMessage, ShellPage, ShellViewModel, WorkspaceRepository};

use super::{FileContext, FileDialog, FileRow, FilesEffect, FilesMessage, FilesState};

fn row(path: &str, kind: &str, extension: Option<&str>) -> FileRow {
    FileRow::from_entry(&FileBrowserEntry {
        path: path.into(),
        name: path.rsplit('/').next().unwrap_or(path).into(),
        kind: kind.into(),
        extension: extension.map(String::from),
        size_bytes: None,
        size_label: None,
        modified_at: None,
        asset_id: (kind == "file").then(|| path.replace('/', "-")),
        status: None,
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata: BTreeMap::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    })
}

fn writable() -> FileContext {
    FileContext {
        repo_id: Some("repo".into()),
        writable: true,
        missing: false,
        trash: false,
        smart_folder: false,
        category_virtual: false,
    }
}

fn listed() -> FilesState {
    let mut state = FilesState::default();
    state.rows = vec![row("assets", "directory", None), row("cover.png", "file", Some("png")), row("notes/page.pdf", "file", Some("pdf"))];
    state
}

#[test]
fn display_title_drops_only_the_matching_extension() {
    assert_eq!(row("cover.png", "file", Some("png")).display_title(), "cover");
    assert_eq!(row("Cover.PNG", "file", Some("png")).display_title(), "Cover");
    assert_eq!(row("archive.tar.gz", "file", Some("gz")).display_title(), "archive.tar");
    assert_eq!(row("notes.md", "file", Some("txt")).display_title(), "notes.md");
    assert_eq!(row("README", "file", None).display_title(), "README");
    assert_eq!(row("photos.v2", "directory", Some("v2")).display_title(), "photos.v2");
}

#[test]
fn single_click_keeps_the_list_and_double_click_opens_the_preview() {
    let mut state = listed();
    state.reduce(&writable(), FilesMessage::ActivateRow("cover.png".into()));
    assert_eq!(state.selected, vec!["cover.png".to_string()]);
    assert!(!state.preview_open(Some("cover.png")), "单击文件只在右侧详情里看");
    assert!(matches!(state.take_effects().as_slice(), [FilesEffect::LoadAsset { asset_id, .. }] if asset_id == "cover.png"));

    state.reduce(&writable(), FilesMessage::OpenRow("cover.png".into()));
    assert!(state.preview_open(Some("cover.png")), "双击或「预览」进入预览页");

    state.reduce(&writable(), FilesMessage::ActivateRow("assets".into()));
    assert_eq!(state.select_only, None, "目录没有预览，不记单击选中的文件");
    assert!(!state.preview_open(None));
}

#[test]
fn context_menu_selects_like_a_click_and_confirms_permanent_delete_twice() {
    let mut state = listed();
    state.reduce(&writable(), FilesMessage::ActivateRow("assets".into()));
    let _ = state.take_effects();
    state.reduce(&writable(), FilesMessage::OpenEntryMenu { path: "notes/page.pdf".into(), x: 400.0, y: 470.0 });
    assert_eq!(state.selected, vec!["notes/page.pdf".to_string()], "右键没选中的条目时先替换成它");
    assert_eq!(state.select_only.as_deref(), Some("notes/page.pdf"));
    assert!(state.entry_menu.is_some());
    assert!(matches!(state.take_effects().as_slice(), [FilesEffect::LoadAsset { .. }]));

    state.reduce(&writable(), FilesMessage::ToggleMenuBranch("thumbnail".into()));
    assert_eq!(state.menu_branch.as_deref(), Some("thumbnail"));
    state.reduce(&writable(), FilesMessage::ToggleMenuBranch("thumbnail".into()));
    assert_eq!(state.menu_branch, None, "再点一次收起子菜单");

    state.reduce(&writable(), FilesMessage::ArmMenuConfirm("delete".into()));
    assert_eq!(state.menu_pending.as_deref(), Some("delete"), "第一次只进入待确认");
    assert!(state.entry_menu.is_some(), "待确认时菜单不关");
    state.reduce(&writable(), FilesMessage::CloseEntryMenu);
    assert!(state.entry_menu.is_none() && state.menu_pending.is_none() && state.menu_branch.is_none());
}

#[test]
fn escape_closes_menus_before_dialogs_and_keeps_busy_dialogs() {
    let mut state = listed();
    state.reduce(&writable(), FilesMessage::ActivateRow("cover.png".into()));
    state.reduce(&writable(), FilesMessage::OpenEntryMenu { path: "cover.png".into(), x: 0.0, y: 0.0 });
    state.import_open = true;
    assert!(state.dismiss_overlay());
    assert!(state.entry_menu.is_none() && state.import_open, "先关右键菜单");
    assert!(state.dismiss_overlay());
    assert!(!state.import_open);
    assert!(!state.dismiss_overlay(), "没有浮层时交给壳层");

    state.reduce(&writable(), FilesMessage::OpenDialog(FileDialog::Copy));
    assert_eq!(state.dialog, FileDialog::Copy);
    state.mutating = true;
    assert!(!state.dismiss_overlay(), "变更进行中的对话框不关");
    state.mutating = false;
    assert!(state.dismiss_overlay());
    assert_eq!(state.dialog, FileDialog::Closed);
}

#[test]
fn tag_group_follows_vue_default_until_toggled_for_that_file() {
    let mut state = FilesState::default();
    assert!(!state.tags_expanded("cover.png", false), "没有标签时默认收起");
    assert!(state.tags_expanded("cover.png", true), "有标签时默认展开");
    state.reduce(&writable(), FilesMessage::ToggleTags { path: "cover.png".into(), expanded: false });
    assert!(state.tags_expanded("cover.png", false));
    assert!(!state.tags_expanded("page.pdf", false), "别的文件仍按默认");
}

#[test]
fn list_width_ignores_subpixel_jitter_and_invalid_values() {
    let mut state = FilesState::default();
    state.reduce(&writable(), FilesMessage::ListResized(516.0));
    assert_eq!(state.list_width, Some(516.0));
    state.reduce(&writable(), FilesMessage::ListResized(516.3));
    assert_eq!(state.list_width, Some(516.0));
    state.reduce(&writable(), FilesMessage::ListResized(f32::NAN));
    state.reduce(&writable(), FilesMessage::ListResized(-4.0));
    assert_eq!(state.list_width, Some(516.0));
    state.reduce(&writable(), FilesMessage::ListResized(276.0));
    assert_eq!(state.list_width, Some(276.0));
}

fn library() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.startup.finish();
    model.workspace.apply_repository_list(None, Ok(vec![WorkspaceRepository {
        repo_id: "repo".into(),
        name: "动画素材".into(),
        path: "D:/Libraries/Anime".into(),
        status: "ready".into(),
        backend_plugin_id: "local".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
    }]));
    model
}

#[test]
fn tag_draft_submits_through_the_inspect_draft() {
    let mut model = library();
    model.files.tag_draft = "参考".into();
    model.reduce(ShellMessage::Files(FilesMessage::SubmitTagDraft("  参考 ".into())));
    assert_eq!(model.inspect.draft_tags(), ["参考".to_string()]);
    assert!(model.files.tag_draft.is_empty(), "加进草稿后清空输入");
    model.reduce(ShellMessage::Files(FilesMessage::SubmitTagDraft("   ".into())));
    assert_eq!(model.inspect.draft_tags().len(), 1, "空标签不加");
}

#[test]
fn archive_export_without_a_path_asks_for_one_first() {
    let mut model = library();
    model.files.present_export();
    model.reduce(ShellMessage::Files(FilesMessage::SubmitExport));
    assert!(model.files.take_effects().iter().all(|effect| !matches!(effect, FilesEffect::ExportArchive { .. })));
    let commands = model.input.take_platform_commands(WindowId(1), false);
    assert!(
        commands.iter().any(|command| matches!(command, WindowCommand::OpenFileDialog { .. })),
        "没有保存位置时先弹系统保存对话框：{commands:?}"
    );
    assert!(model.files.export.error.is_empty());
}

#[test]
fn only_the_detail_a_single_click_asked_for_stays_out_of_the_preview() {
    let mut state = listed();
    state.reduce(&writable(), FilesMessage::ActivateRow("cover.png".into()));
    state.note_detail_arrived("cover.png");
    assert!(!state.preview_open(Some("cover.png")), "单击读回来的详情只在右侧看");
    state.note_detail_arrived("cover.png");
    assert!(state.preview_open(Some("cover.png")), "别处再读同一个文件就进入预览");

    state.reduce(&writable(), FilesMessage::ActivateRow("notes/page.pdf".into()));
    state.reduce(&writable(), FilesMessage::OpenRow("notes/page.pdf".into()));
    state.note_detail_arrived("notes/page.pdf");
    assert!(state.preview_open(Some("notes/page.pdf")), "双击盖过还没回来的单击读取");
}

/// 只有摘要的素材详情，模拟搜索结果读回来的应答。
fn bare_detail(path: &str) -> AssetDetail {
    AssetDetail {
        summary: AssetSummary {
            asset_id: path.replace('/', "-"),
            repo_id: "acceptance-repo".into(),
            path: path.into(),
            filename: path.rsplit('/').next().unwrap_or(path).into(),
            extension: path.rsplit('.').next().unwrap_or_default().into(),
            size_bytes: 0,
            size_label: String::new(),
            status: "ready".into(),
            modified_at: String::new(),
            last_accessed_at: None,
            version: 1,
            tags: Vec::new(),
            thumbnail_path: None,
            hardlink_group_id: None,
            hardlink_state: None,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        },
        metadata: Vec::new(),
        revisions: Vec::new(),
    }
}

#[test]
fn a_search_hit_on_the_clicked_file_still_opens_the_preview() {
    let (_, mut model) = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == "live-files-selected")
        .expect("单击选中 notes/page.pdf 的场景");
    assert_eq!(model.page, ShellPage::SelectedFile);
    assert!(!model.files.preview_open(model.inspect.target_path.as_deref()), "单击只在右侧详情里看");
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(bare_detail("notes/page.pdf"))));
    assert!(model.files.preview_open(model.inspect.target_path.as_deref()), "搜索结果读回同一个文件后进入预览");
}

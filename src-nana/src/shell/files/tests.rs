//! 文件浏览状态机测试。只调用状态方法，不写用户的展示方式文件。

use std::collections::BTreeMap;

use crate::backend::services::repository::{
    FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState,
};

use super::super::ShellViewModel;
use super::{
    DisplayMode, FileContext, FileDialog, FileRow, FilesEffect, FilesMessage, FilesState,
    HardlinkPrompt, SelectionMode,
};

mod browse;
mod protocol;
mod reselect;

fn entry(path: &str, name: &str, kind: &str) -> FileBrowserEntry {
    FileBrowserEntry {
        path: path.into(),
        name: name.into(),
        kind: kind.into(),
        extension: None,
        size_bytes: None,
        size_label: None,
        modified_at: None,
        asset_id: None,
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
    }
}

fn row(path: &str, name: &str, kind: &str) -> FileRow {
    FileRow::from_entry(&entry(path, name, kind))
}

fn snapshot(path: &str, total: usize, entries: Vec<FileBrowserEntry>) -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: "repo".into(),
        root_path: "C:/repo".into(),
        backend_plugin_id: "local".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: path.into(),
        total_entries: total,
        loaded_count: entries.len(),
        next_offset: None,
        has_more: entries.len() < total,
        special_location: None,
        tree: None,
        entries,
    }
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

fn trash_ctx() -> FileContext {
    FileContext {
        trash: true,
        ..writable()
    }
}

fn smart_ctx() -> FileContext {
    FileContext {
        smart_folder: true,
        ..writable()
    }
}

fn category_ctx() -> FileContext {
    FileContext {
        category_virtual: true,
        ..writable()
    }
}

mod copy_reselect;

fn reduce_open(state: &mut FilesState, ctx: &FileContext, dialog: FileDialog) -> bool {
    let before = state.dialog;
    state.reduce(ctx, FilesMessage::OpenDialog(dialog));
    state.dialog != before && state.dialog == dialog
}

/// 在工具栏输入框里写下名字再点「建文件」，返回是否开始变更。
fn create_file_for(state: &mut FilesState, ctx: &FileContext, name: &str) -> bool {
    state.reduce(ctx, FilesMessage::SetCreateName(name.into()));
    let before = state.mutating;
    state.reduce(ctx, FilesMessage::SubmitCreateFile);
    state.mutating && !before
}

fn submit_for(state: &mut FilesState, ctx: &FileContext) -> bool {
    let before = state.mutating;
    state.reduce(ctx, FilesMessage::SubmitDialog);
    state.mutating && !before
}

fn delete_selected_for(state: &mut FilesState, ctx: &FileContext) -> bool {
    state.reduce(ctx, FilesMessage::DeleteSelected);
    state.mutating
}

/// 点当前这一级面包屑，或侧栏再点正在看的文件夹：静默重读。
/// 列表和选择留着，不换成「正在读取目录」；这次读取失败也只记日志。换到别的目录仍显示读取中。
#[test]
fn reloading_the_open_directory_keeps_the_list_on_screen() {
    let mut state = FilesState::default();
    let ctx = writable();
    assert!(state.request_browse(&ctx, "assets"), "第一次打开目录");
    assert!(state.loading, "还没有目录快照时要显示正在读取目录");
    let _ = state.take_effects();
    assert!(state.apply_browser(
        &snapshot(
            "assets",
            2,
            vec![
                entry("assets/a.png", "a.png", "file"),
                entry("assets/b.png", "b.png", "file")
            ]
        ),
        false,
    ));
    state.reduce(&ctx, FilesMessage::ActivateRow("assets/a.png".into()));
    assert_eq!(state.selected, vec!["assets/a.png".to_string()]);

    state.reduce(&ctx, FilesMessage::OpenPath("assets".into()));
    assert!(!state.loading, "再点当前面包屑不把列表换成正在读取目录");
    assert_eq!(
        state.entry_names(),
        vec!["a.png".to_string(), "b.png".to_string()]
    );
    assert_eq!(
        state.selected,
        vec!["assets/a.png".to_string()],
        "静默重读不清选择"
    );
    assert!(
        matches!(
            state.take_effects().as_slice(),
            [FilesEffect::Browse { path, limit, append: false, .. }] if path == "assets" && *limit >= 2
        ),
        "仍会重读这一层，而且不少于已经列出的条数"
    );
    assert!(state.note_load_failed("读取失败"));
    assert!(state.error.is_empty(), "静默重读失败不把列表换成错误");
    assert_eq!(
        state.entry_names(),
        vec!["a.png".to_string(), "b.png".to_string()]
    );
    assert!(!state.loading);

    state.reduce(&ctx, FilesMessage::OpenPath("other".into()));
    assert!(state.loading, "换到别的目录仍显示正在读取目录");
    assert!(state.selected.is_empty(), "离开当前目录才清选择");

    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    assert!(model.files.apply_browser(
        &snapshot("assets", 1, vec![entry("assets/a.png", "a.png", "file")]),
        false
    ));
    model
        .files
        .reduce(&ctx, FilesMessage::ActivateRow("assets/a.png".into()));
    model.note_sidebar_browse("assets", false, false);
    assert!(
        !model.files.loading,
        "侧栏再点正在看的文件夹也不换成正在读取目录"
    );
    assert_eq!(model.files.selected, vec!["assets/a.png".to_string()]);
    assert!(model.files.note_load_failed("读取失败"));
    assert!(
        model.files.error.is_empty(),
        "侧栏这次静默重读失败也不换掉列表"
    );
    model.note_sidebar_browse("covers", false, false);
    assert!(model.files.loading, "侧栏点开别的文件夹仍显示正在读取目录");
}

//! 打开和定位的绝对路径，对照 Vue `resolveWorkspaceEntryPath`。
//!
//! 有本机文件就用它；虚拟条目或来源仓库没有本地文件时不拼仓库根，也不交给系统。
//! 不启动仓库服务，不打开真窗口。

use std::collections::BTreeMap;

use crate::backend::services::repository::FileBrowserEntry;
use crate::host_api::HostRequest;
use crate::shell::files::FileRow;
use crate::shell::player::QueueItem;
use crate::shell::{ShellMessage, ShellViewModel, WorkspaceRepository};

use super::InputMessage;

/// 预览页和文件右键的打开、定位：有本机绝对路径就用它；没有时不发出打开。
#[test]
fn opening_a_virtual_file_uses_its_local_path_and_skips_when_there_is_none() {
    let cache = "D:\\cache\\晴天.mp3";
    let mut source = source_repo();
    source.files.rows = vec![virtual_row("songs/晴天.mp3", Some(format!("  {cache}  ")))];
    source.inspect.target_path = Some("songs/晴天.mp3".into());
    assert_eq!(
        super::resolve_entry_absolute(&source, "songs/晴天.mp3"),
        cache,
        "去掉空白后的本机路径优先"
    );
    let (can_open, can_reveal, absolute) = super::super::inspect_view::preview_open_target(&source);
    assert!(can_open && can_reveal, "有本机文件时预览页打开和定位可用");
    assert_eq!(absolute, cache);
    let (menu_open, menu_reveal) =
        super::super::files_view::open_and_reveal_targets(&source, "songs/晴天.mp3")
            .expect("菜单里有这一条");
    assert_eq!((menu_open.as_str(), menu_reveal.as_str()), (cache, cache));
    send(
        &mut source,
        InputMessage::OpenEntry {
            has_repo: true,
            absolute_path: cache.into(),
            path: "songs/晴天.mp3".into(),
        },
    );
    assert!(matches!(
        source.input.host_requests.last(),
        Some(HostRequest::OpenExternal(request)) if !request.reveal && request.target == cache
    ));

    source.files.rows = vec![virtual_row("songs/晴天.mp3", Some("   ".into()))];
    assert!(
        super::resolve_entry_absolute(&source, "songs/晴天.mp3").is_empty(),
        "空白的本机路径不算"
    );
    let (can_open, _, absolute) = super::super::inspect_view::preview_open_target(&source);
    assert!(
        !can_open && absolute.is_empty(),
        "没有本地文件时预览页不假装能打开"
    );
    let (menu_open, menu_reveal) =
        super::super::files_view::open_and_reveal_targets(&source, "songs/晴天.mp3")
            .expect("菜单还在");
    assert!(menu_open.is_empty() && menu_reveal.is_empty());
    let before = source.input.host_requests.len();
    send(
        &mut source,
        InputMessage::OpenEntry {
            has_repo: true,
            absolute_path: String::new(),
            path: "songs/晴天.mp3".into(),
        },
    );
    send(
        &mut source,
        InputMessage::RevealEntry {
            absolute_path: String::new(),
        },
    );
    assert_eq!(
        source.input.host_requests.len(),
        before,
        "没有本地文件时不交给系统"
    );

    source.files.rows.clear();
    source.player.queue = vec![virtual_queue("songs/only-queue.mp3", cache)];
    source.inspect.target_path = Some("songs/only-queue.mp3".into());
    assert_eq!(
        super::resolve_entry_absolute(&source, "songs/only-queue.mp3"),
        cache,
        "不在文件列表里时用播放队列上的本机路径"
    );

    let mut local = ready("repo");
    local.workspace.repositories.push(filesystem_repo("repo"));
    local.files.rows = vec![virtual_row("photos/cover.png", None)];
    local.files.rows[0].is_virtual = false;
    local.inspect.target_path = Some("photos/cover.png".into());
    assert_eq!(
        super::resolve_entry_absolute(&local, "photos/cover.png"),
        "C:/repo/photos/cover.png"
    );
}

fn send(model: &mut ShellViewModel, message: InputMessage) {
    model.reduce(ShellMessage::Input(message));
}

fn ready(repo: &str) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some(repo.into());
    model.repository_id = Some(repo.into());
    model.files.current_path = "photos".into();
    model
}

fn source_repo() -> ShellViewModel {
    let mut model = ready("netease");
    model.workspace.repositories.push(WorkspaceRepository {
        repo_id: "netease".into(),
        name: "网易云".into(),
        path: "netease".into(),
        status: "ready".into(),
        backend_plugin_id: "momobako.source.netease-cloud-music".into(),
        capabilities: vec!["browse".into(), "read".into()],
        cache_required: false,
        cache_status: String::new(),
        cache_path: None,
        login_expired: false,
        logged_out: false,
    });
    model
}

fn filesystem_repo(id: &str) -> WorkspaceRepository {
    WorkspaceRepository {
        repo_id: id.into(),
        name: id.into(),
        path: "C:/repo".into(),
        status: "ready".into(),
        backend_plugin_id: "momobako.local-filesystem".into(),
        capabilities: vec!["localRootPath".into(), "write".into()],
        cache_required: false,
        cache_status: String::new(),
        cache_path: None,
        login_expired: false,
        logged_out: false,
    }
}

fn virtual_row(path: &str, local_absolute_path: Option<String>) -> FileRow {
    FileRow::from_entry(&FileBrowserEntry {
        path: path.into(),
        name: path.rsplit('/').next().unwrap_or(path).into(),
        kind: "file".into(),
        extension: Some("mp3".into()),
        size_bytes: None,
        size_label: None,
        modified_at: None,
        asset_id: Some("song".into()),
        status: None,
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata: BTreeMap::new(),
        is_virtual: true,
        provider_id: Some("netease".into()),
        provider_item_id: Some("1".into()),
        source_payload: None,
        local_absolute_path,
    })
}

fn virtual_queue(path: &str, local: &str) -> QueueItem {
    QueueItem {
        id: "song".into(),
        playlist_id: "pl".into(),
        asset_id: "song".into(),
        path: path.into(),
        filename: "only-queue.mp3".into(),
        extension: "mp3".into(),
        status: "ready".into(),
        status_reason: None,
        transient: false,
        player_type_id: "audio".into(),
        player_label: "音频".into(),
        file_class: "audio".into(),
        thumbnail_path: None,
        is_virtual: true,
        local_absolute_path: Some(local.into()),
    }
}

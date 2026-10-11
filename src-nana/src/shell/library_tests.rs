//! 访问记录的测试：侧栏「最近使用」的计数照 Vue 当场更新，「清空记录」当场归零，「打开」记一次访问，
//! 访问时间只留最近的 50 条。

use crate::backend::services::repository::AssetSummary;

use super::super::input::InputMessage;
use super::super::{ShellMessage, ShellPage, ShellViewModel, SidebarMessage};
use super::{LibraryAssets, LibraryEffect};

const REPO: &str = "acceptance-repo";

/// 验收底子：根目录 assets 文件夹、cover.png 和 notes/page.pdf（摘要里两个素材，都没有访问时间）。
fn model() -> ShellViewModel {
    ShellViewModel::for_page(ShellPage::FileList)
}

fn accessed(model: &mut ShellViewModel, path: &str, at: &str) {
    let result = Ok(at.to_string());
    model.reduce(ShellMessage::EntryAccessRecorded { repo_id: REPO.into(), path: path.into(), result });
}

/// Vue `patchActiveSnapshotAssetAccess`：记下访问以后侧栏「最近使用」的计数当场变，不重读目录、不重读摘要；
/// 同一条再访问不重复计数。记录失败、别的资源库、摘要里没有的路径都不动。
#[test]
fn an_access_updates_the_recent_count_on_the_spot() {
    let mut model = model();
    assert_eq!(model.sidebar.counts.recent, 0);
    let _ = model.files.take_effects();
    let _ = model.workspace.take_effects();

    accessed(&mut model, "notes/page.pdf", "2026-10-11T08:00:00Z");
    assert_eq!(model.sidebar.counts.recent, 1);
    accessed(&mut model, "cover.png", "2026-10-11T08:00:05Z");
    accessed(&mut model, "cover.png", "2026-10-11T08:00:09Z");
    assert_eq!(model.sidebar.counts.recent, 2, "同一条再访问不重复计数");
    assert!(model.files.take_effects().is_empty(), "当场改素材，不重读目录");
    assert!(model.workspace.take_effects().is_empty(), "也不重读摘要");
    assert_eq!((model.sidebar.counts.all, model.sidebar.counts.untagged), (2, 2), "别的计数不变");

    let failed = Err("拒绝访问".to_string());
    model.reduce(ShellMessage::EntryAccessRecorded { repo_id: REPO.into(), path: "assets".into(), result: failed });
    model.reduce(ShellMessage::EntryAccessRecorded { repo_id: "other".into(), path: "cover.png".into(), result: Ok("2026-10-12T00:00:00Z".into()) });
    accessed(&mut model, "missing.png", "2026-10-12T00:00:00Z");
    assert_eq!(model.sidebar.counts.recent, 2);
}

/// 「清空记录」成功照 Vue `clearActiveSnapshotRecentAccess` 当场清掉素材的访问时间：计数归零，之后再访问从 1 数起。
#[test]
fn clearing_recent_history_resets_the_count_at_once() {
    let mut model = model();
    accessed(&mut model, "cover.png", "2026-10-11T08:00:00Z");
    accessed(&mut model, "notes/page.pdf", "2026-10-11T08:00:01Z");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RecentCleared { repo_id: REPO.into(), result: Ok(2) }));
    assert_eq!(model.sidebar.counts.recent, 0);
    accessed(&mut model, "cover.png", "2026-10-11T08:00:02Z");
    assert_eq!(model.sidebar.counts.recent, 1, "清掉的是素材里的访问时间，不只是计数");
}

/// 「打开」照 Vue `openWorkspaceEntry` 记一次访问：系统程序照常打开，访问记录排给宿主，带仓库内的相对路径。
#[test]
fn opening_an_entry_records_an_access() {
    let mut model = model();
    model.reduce(ShellMessage::Input(InputMessage::OpenEntry {
        has_repo: true,
        absolute_path: "C:/acceptance/cover.png".into(),
        path: "cover.png".into(),
    }));
    assert_eq!(model.input.host_requests.len(), 1, "照常交给系统程序打开");
    assert_eq!(
        model.library.take_effects(),
        [LibraryEffect::RecordAccess { repo_id: REPO.into(), path: "cover.png".into() }]
    );
}

fn asset(path: &str, accessed_at: Option<&str>, modified_at: &str) -> AssetSummary {
    let filename = path.rsplit('/').next().unwrap_or(path).to_string();
    AssetSummary {
        asset_id: path.replace('/', "-"),
        repo_id: REPO.into(),
        path: path.into(),
        filename,
        extension: "mp3".into(),
        size_bytes: 1,
        size_label: "1 B".into(),
        status: "ready".into(),
        modified_at: modified_at.into(),
        last_accessed_at: accessed_at.map(str::to_string),
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
    }
}

fn library(assets: Vec<AssetSummary>) -> LibraryAssets {
    LibraryAssets { repo_id: Some(REPO.into()), assets, effects: Vec::new() }
}

fn accessed_paths(library: &LibraryAssets) -> Vec<&str> {
    library.assets.iter().filter(|asset| asset.last_accessed_at.is_some()).map(|asset| asset.path.as_str()).collect()
}

/// 和后端一样只留最近的 50 条访问时间：第 51 条访问记下以后，最早的那条清掉。
#[test]
fn an_access_keeps_only_the_latest_fifty() {
    let assets = (0..51)
        .map(|index| {
            let at = (index < 50).then(|| format!("2026-10-11T08:{:02}:00Z", index));
            asset(&format!("track-{index:02}.mp3"), at.as_deref(), "2026-01-01T00:00:00Z")
        })
        .collect();
    let mut library = library(assets);
    assert!(library.note_access(REPO, "track-50.mp3", "2026-10-11T09:00:00Z"));
    let kept = accessed_paths(&library);
    assert_eq!(kept.len(), 50);
    assert!(kept.contains(&"track-50.mp3"));
    assert!(!kept.contains(&"track-00.mp3"), "最早的访问被修剪");
}

/// 修剪的先后照 Vue：访问时间相同时修改时间早的先清；删掉的素材不记访问。
#[test]
fn pruning_follows_vue_ordering_and_skips_deleted_assets() {
    let mut assets: Vec<AssetSummary> = (0..49)
        .map(|index| asset(&format!("keep-{index:02}.mp3"), Some("2026-10-11T09:00:00Z"), "2026-01-01T00:00:00Z"))
        .collect();
    assets.push(asset("older-file.mp3", Some("2026-10-11T08:00:00Z"), "2026-01-01T00:00:00Z"));
    assets.push(asset("newer-file.mp3", Some("2026-10-11T08:00:00Z"), "2026-05-01T00:00:00Z"));
    let mut deleted = asset("gone.mp3", None, "2026-01-01T00:00:00Z");
    deleted.status = "deleted".into();
    assets.push(deleted);
    let mut library = library(assets);
    assert!(!library.note_access(REPO, "gone.mp3", "2026-10-11T10:00:00Z"), "删掉的素材不记访问");
    // 51 条里修剪掉一条：访问时间最早的两条里修改时间早的那一条。
    library.prune_recent();
    let kept = accessed_paths(&library);
    assert_eq!(kept.len(), 50);
    assert!(kept.contains(&"newer-file.mp3") && !kept.contains(&"older-file.mp3"));
}

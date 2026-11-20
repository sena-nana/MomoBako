//! 搜索与筛选状态机测试。
//!
//! 不启动仓库服务，也不写用户目录。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{
    AssetSummary, FileBrowserEntry, FileBrowserSnapshot, RepositoryBackendSummary,
    RepositoryOverview, RepositorySnapshot, RepositoryStructureCacheState, RepositorySummary,
};

use super::super::super::files::FilesMessage;
use super::super::super::files_view::previewing;
use super::super::super::workspace::{LibraryCategory, WorkspacePanel, WorkspaceRepository};
use super::super::super::{FilesEffect, ShellMessage, ShellPage, ShellViewModel};
use super::super::{InspectEffect, InspectMessage, InspectState};
use super::{AdvancedField, FilterList, MetadataInput, SearchRow, SortDirection};

mod criteria;
mod navigation;

fn row(repo_id: &str, asset_id: &str, path: &str) -> SearchRow {
    SearchRow {
        repo_id: repo_id.into(),
        asset_id: asset_id.into(),
        path: path.into(),
        filename: path.rsplit('/').next().unwrap_or(path).into(),
        repo_name: "图库".into(),
        tags: Vec::new(),
        metadata: BTreeMap::new(),
    }
}

fn repository(repo_id: &str, writable: bool) -> WorkspaceRepository {
    WorkspaceRepository {
        repo_id: repo_id.into(),
        name: format!("库 {repo_id}"),
        path: format!("C:/{repo_id}"),
        status: "ready".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: if writable {
            vec!["write".into()]
        } else {
            Vec::new()
        },
        cache_required: false,
        cache_status: String::new(),
        cache_path: None,
        login_expired: false,
        logged_out: false,
    }
}

/// 启动完成、活动仓库是可写的 `repo` 的壳层。
fn shell() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.present_repository(repository("repo", true));
    model.repository_id = Some("repo".into());
    model
}

fn send(model: &mut ShellViewModel, message: InspectMessage) {
    model.reduce(ShellMessage::Inspect(message));
}

fn last_search(state: &mut InspectState) -> super::SearchRequestDraft {
    match state.take_effects().pop() {
        Some(InspectEffect::Search { request, .. }) => request,
        other => panic!("没有搜索请求：{other:?}"),
    }
}

fn browser(repo_id: &str, path: &str, files: &[&str]) -> FileBrowserSnapshot {
    let entries = files
        .iter()
        .map(|file| FileBrowserEntry {
            path: (*file).into(),
            name: file.rsplit('/').next().unwrap_or(file).into(),
            kind: "file".into(),
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
        })
        .collect::<Vec<_>>();
    FileBrowserSnapshot {
        repo_id: repo_id.into(),
        root_path: format!("C:/{repo_id}"),
        backend_plugin_id: "filesystem".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: path.into(),
        total_entries: entries.len(),
        loaded_count: entries.len(),
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries,
    }
}

fn snapshot(repo_id: &str, assets: &[(&str, &str, &[&str])]) -> RepositorySnapshot {
    let summary = RepositorySummary {
        repo_id: repo_id.into(),
        name: format!("库 {repo_id}"),
        path: format!("C:/{repo_id}"),
        backend: RepositoryBackendSummary {
            plugin_id: "filesystem".into(),
            kind: "local".into(),
            name: "本地".into(),
            capabilities: vec!["write".into()],
        },
        status: "ready".into(),
        asset_count: assets.len() as i64,
        updated_at: String::new(),
        local_cache: None,
        authentication: None,
    };
    RepositorySnapshot {
        repository: summary,
        folder_label: String::new(),
        folders: Vec::new(),
        assets: assets
            .iter()
            .map(|(path, extension, tags)| AssetSummary {
                asset_id: (*path).into(),
                repo_id: repo_id.into(),
                path: (*path).into(),
                filename: (*path).into(),
                extension: (*extension).into(),
                size_bytes: 0,
                size_label: "0 B".into(),
                status: "ready".into(),
                modified_at: String::new(),
                last_accessed_at: None,
                version: 1,
                tags: tags.iter().map(|tag| tag.to_string()).collect(),
                thumbnail_path: None,
                hardlink_group_id: None,
                hardlink_state: None,
                is_virtual: false,
                provider_id: None,
                provider_item_id: None,
                source_payload: None,
                local_absolute_path: None,
            })
            .collect(),
        playlists: Vec::new(),
        quick_access: Vec::new(),
        tag_groups: Vec::new(),
        metadata_fields: Vec::new(),
        recent_revision_count: 0,
        overview: RepositoryOverview {
            total_size_bytes: 0,
            total_size_label: "0 B".into(),
            file_count: assets.len() as i64,
            folder_count: 0,
            trash_count: 0,
            readme_content: None,
        },
    }
}

#[allow(dead_code)]
fn metadata(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect()
}

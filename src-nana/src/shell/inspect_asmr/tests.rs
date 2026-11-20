//! ASMR 资源库扩展的回归测试：候选按原始类型写回、导入的候选落在自己的文件上、抓取经插件调用、
//! 作品队列点了打开预览、作品信息的数组和插件开关、音轨的自然序。
//!
//! 场景照验收底子（`for_page`，内置插件全部启用），根目录换成给定的 ASMR 文件，再像双击一样读回当前文件的详情。

use std::collections::BTreeMap;

use nana_ui::runtime::{Entity, ScrollOffset, ScrollView};
use serde_json::{Value, json};

use crate::backend::services::repository::{
    AssetDetail, AssetSummary, MetadataEntry, PlaylistDetail, PlaylistItem, PlaylistSummary,
};
use crate::shell::admin::AdminEffect;
use crate::shell::files::{EnsuredThumbnail, FileRow, FilesEffect, FilesMessage};
use crate::shell::inspect_library::CoverButton;
use crate::shell::player::PlayerMessage;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{InspectEffect, InspectMessage, ShellMessage, ShellPage, ShellViewModel};

use super::AsmrMessage;

mod candidates;
mod queue;

const REPO: &str = "acceptance-repo";

type Meta = BTreeMap<String, Value>;

/// ASMR 音频的元数据：库类型、条目类型，作品目录（有时），再加 `extra`。
pub(super) fn audio(work_root: Option<&str>, extra: &[(&str, Value)]) -> Meta {
    let mut metadata = Meta::from([
        ("libraryKind".to_string(), json!("asmr")),
        ("asmrEntryKind".to_string(), json!("audio")),
    ]);
    if let Some(root) = work_root {
        metadata.insert("workRoot".into(), json!(root));
    }
    metadata.extend(
        extra
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone())),
    );
    metadata
}

pub(super) fn asset_id(path: &str) -> String {
    path.replace('/', "-")
}

pub(super) fn row(path: &str, metadata: &Meta) -> FileRow {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    FileRow {
        path: path.into(),
        extension: name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_string()),
        name,
        kind: "file".into(),
        asset_id: Some(asset_id(path)),
        is_virtual: false,
        local_absolute_path: None,
        thumbnail_path: None,
        hardlink_state: None,
        pixel_width: 0,
        pixel_height: 0,
        texture_ready: false,
        palette: Vec::new(),
        size_label: String::new(),
        modified_at: String::new(),
        tags: Vec::new(),
        alias_paths: Vec::new(),
        thumbnail_custom: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        metadata: metadata.clone(),
    }
}

pub(super) fn detail(path: &str, metadata: &Meta, version: i64) -> AssetDetail {
    let filename = path.rsplit('/').next().unwrap_or(path).to_string();
    AssetDetail {
        summary: AssetSummary {
            asset_id: asset_id(path),
            repo_id: REPO.into(),
            path: path.into(),
            extension: filename
                .rsplit_once('.')
                .map(|(_, extension)| extension.to_string())
                .unwrap_or_default(),
            filename,
            size_bytes: 1,
            size_label: "1 B".into(),
            status: "ready".into(),
            modified_at: String::new(),
            last_accessed_at: None,
            version,
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
        metadata: metadata
            .iter()
            .map(|(key, value)| MetadataEntry {
                key: key.clone(),
                value_type: "json".into(),
                value: value.clone(),
                version,
                updated_at: String::new(),
            })
            .collect(),
        revisions: Vec::new(),
    }
}

/// 根目录换成 `files` 的验收模型，读回 `current` 的详情，停在它的预览上。
pub(super) fn scene(files: &[(&str, Meta)], current: &str) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
    model.files.rows = files
        .iter()
        .map(|(path, metadata)| row(path, metadata))
        .collect();
    open(&mut model, files, current);
    model
}

/// 像双击一样选中 `path` 并读回它的详情。
pub(super) fn open(model: &mut ShellViewModel, files: &[(&str, Meta)], path: &str) {
    let metadata = files
        .iter()
        .find(|(item, _)| *item == path)
        .map(|(_, metadata)| metadata.clone())
        .expect("场景里的文件");
    model.reduce(ShellMessage::SelectFile {
        path: path.into(),
        asset_id: Some(asset_id(path)),
    });
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        path, &metadata, 1,
    ))));
    model.inspect.take_effects();
    model.files.take_effects();
    model.admin.take_effects();
}

/// 这次归约排下的元数据保存请求里的元数据。
pub(super) fn saved(model: &mut ShellViewModel) -> Option<BTreeMap<String, Value>> {
    model
        .inspect
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            InspectEffect::SaveMetadata { metadata, .. } => Some(metadata),
            _ => None,
        })
}

fn asmr(model: &mut ShellViewModel, message: AsmrMessage) {
    model.reduce(ShellMessage::Asmr(message));
}

fn dlsite_candidate() -> Value {
    json!({
        "source": "dlsite",
        "confidence": "external-id",
        "fields": {
            "workId": "RJ01234567",
            "voiceActors": ["甲", "乙"],
            "price": 1320,
            "rateAverage": 4.5,
            "nsfw": false,
            "sourceUrl": "https://www.dlsite.com/maniax/work/=/product_id/RJ01234567.html",
            "comment": "候选不写注释",
            "listeningStatus": "listened"
        }
    })
}

fn queue_of(model: &ShellViewModel) -> Option<(String, Vec<(String, String, String)>)> {
    let panels = super::preview_panels(model)?;
    let (count, items) = panels.queue?;
    Some((
        count,
        items
            .into_iter()
            .map(|item| (item.path, item.title, item.status))
            .collect(),
    ))
}

fn assert_queue(model: &ShellViewModel, count: &str, lines: &[(&str, &str, &str)]) {
    let (got_count, items) = queue_of(model).expect("有作品队列");
    assert_eq!(got_count, count, "{items:?}");
    let got = items
        .iter()
        .map(|(path, title, status)| (path.as_str(), title.as_str(), status.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(got, lines, "队列不跟详情或草稿");
}

fn playlist_page(path: &str, metadata: &Meta) -> PlaylistDetail {
    PlaylistDetail {
        playlist: PlaylistSummary {
            playlist_id: "pl".into(),
            repo_id: REPO.into(),
            name: "专辑".into(),
            player_type_id: "audio".into(),
            player_plugin_id: "audio".into(),
            player_label: "音频".into(),
            file_class: "audio".into(),
            item_count: 1,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        },
        items: vec![PlaylistItem {
            playlist_item_id: "item".into(),
            playlist_id: "pl".into(),
            asset_id: asset_id(path),
            path: path.into(),
            filename: path.rsplit('/').next().unwrap_or(path).into(),
            extension: "mp3".into(),
            thumbnail_path: None,
            status: "ready".into(),
            status_reason: None,
            sort_order: 0,
            added_at: String::new(),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            metadata: Some(serde_json::to_value(metadata).expect("播放集条目的元数据")),
            local_absolute_path: None,
        }],
    }
}

fn works_cover(model: &ShellViewModel) -> Option<CoverButton> {
    let custom = super::display_custom(model);
    let mut sections = crate::shell::inspect_library::library_sections(&custom);
    super::attach_cover(&mut sections, model);
    sections
        .into_iter()
        .find(|section| section.key == "inspect-asmr-works")
        .and_then(|section| section.cover)
}

/// 详情列比窗口高。布局盒不算滚动偏移，所以先把 `key` 滚到滚动区上沿下 80px，再点那个视觉位置。
fn click_revealed(harness: &mut ShellHarness, key: &str) {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    let path = harness
        .document()
        .context()
        .assembly_path(id)
        .unwrap_or_default();
    let scroll_key = path
        .split('/')
        .find(|part| part.starts_with("file-detail-scroll-") || part.starts_with("preview-stats-"))
        .unwrap_or_else(|| panic!("按钮不在详情滚动区里：{path}"))
        .to_string();
    let scroll = harness
        .keyed(&scroll_key)
        .unwrap_or_else(|| panic!("没有滚动区 {scroll_key}"));
    let (x, button_y) = harness.center(id);
    let scroll_box = harness
        .document()
        .context()
        .world()
        .layout_box(scroll)
        .expect("滚动区布局");
    let offset = (button_y - scroll_box.y - 80.0).max(0.0);
    harness
        .window
        .document
        .context_mut()
        .scroll_to(
            Entity::<ScrollView>::from_stable_id(scroll),
            ScrollOffset { x: 0.0, y: offset },
        )
        .expect("把封面按钮滚进视口");
    harness.flush();
    let actual = harness
        .document()
        .context()
        .world()
        .scroll_offset(scroll)
        .map(|item| item.y)
        .unwrap_or(0.0);
    let y = button_y - actual;
    let messages = harness.click_messages(x, y);
    harness.settle(messages);
}

fn cover_saves(model: &mut ShellViewModel) -> Vec<(String, String, String)> {
    model
        .files
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            FilesEffect::SaveCover {
                repo_id,
                path,
                source_url,
            } => Some((repo_id, path, source_url)),
            _ => None,
        })
        .collect()
}

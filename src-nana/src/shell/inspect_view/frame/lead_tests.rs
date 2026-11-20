//! 预览页头部的测试：说明行停在已保存条目上，标题使用条目名与扩展名。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::backend::services::repository::{
    AssetDetail, AssetSummary, FileBrowserEntry, MetadataEntry, PlaylistDetail, PlaylistItem,
    PlaylistSummary,
};
use crate::shell::player::QueueItem;
use crate::shell::{FileRow, InspectMessage, ShellMessage, ShellViewModel};

use super::head_view;

fn entry(path: &str, metadata: BTreeMap<String, Value>) -> FileBrowserEntry {
    FileBrowserEntry {
        path: path.into(),
        name: path.rsplit('/').next().unwrap_or(path).into(),
        kind: "file".into(),
        extension: Some("mp3".into()),
        size_bytes: Some(12),
        size_label: Some("12 B".into()),
        modified_at: None,
        asset_id: Some("asset".into()),
        status: Some("ready".into()),
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata,
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    }
}

fn detail(path: &str, comment: &str, link: &str) -> AssetDetail {
    AssetDetail {
        summary: AssetSummary {
            asset_id: "asset".into(),
            repo_id: "repo".into(),
            path: path.into(),
            filename: "a.mp3".into(),
            extension: "mp3".into(),
            size_bytes: 12,
            size_label: "12 B".into(),
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
        metadata: vec![
            MetadataEntry {
                key: "comment".into(),
                value_type: "string".into(),
                value: json!(comment),
                version: 1,
                updated_at: String::new(),
            },
            MetadataEntry {
                key: "link".into(),
                value_type: "string".into(),
                value: json!(link),
                version: 1,
                updated_at: String::new(),
            },
        ],
        revisions: Vec::new(),
    }
}

/// 页头注释和链接跟着列表里已经保存的条目走。编辑器草稿变了不提前出现；
/// 保存写回这一行以后才换。列表里没有这条时用已保存的素材详情，仍不读草稿。
/// 注释字段是空的时候用 note。
#[test]
fn preview_header_lead_stays_on_the_saved_entry_while_the_draft_changes() {
    let path = "voice/a.mp3";
    let mut metadata = BTreeMap::new();
    metadata.insert("note".into(), json!("列表备注"));
    metadata.insert("link".into(), json!(" https://saved.example "));
    let mut model = ShellViewModel::default();
    model.files.rows = vec![FileRow::from_entry(&entry(path, metadata))];
    model
        .inspect
        .note_selected_detail(&detail(path, "详情注释", "https://detail.example"), false);
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "草稿注释".into(),
    )));
    model.reduce(ShellMessage::Inspect(InspectMessage::SetLink(
        "https://draft.example".into(),
    )));

    let head = head_view(&model);
    assert_eq!(
        head.comment, "列表备注",
        "空 comment 用列表里的 note，不读草稿或详情"
    );
    assert_eq!(
        head.link, "https://saved.example",
        "链接用列表里已保存的，去掉首尾空白"
    );
    assert_eq!(model.inspect.draft_comment(), "草稿注释");

    model.files.rows[0]
        .metadata
        .insert("comment".into(), json!("已写入"));
    model.files.rows[0]
        .metadata
        .insert("link".into(), json!("https://written.example"));
    let written = head_view(&model);
    assert_eq!(
        written.comment, "已写入",
        "保存写回这一行以后，页头换成已保存的注释"
    );
    assert_eq!(written.link, "https://written.example");

    model.files.rows.clear();
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "还是草稿".into(),
    )));
    let fallback = head_view(&model);
    assert_eq!(
        fallback.comment, "详情注释",
        "列表里没有这条时用已保存的详情，不读草稿"
    );
    assert_eq!(fallback.link, "https://detail.example");
    assert_eq!(model.inspect.draft_comment(), "还是草稿");
}

/// 页头标题按条目的扩展名去掉后缀，不按路径最后一段猜扩展名。
///
/// 当前列表优先：扩展名对不上时保留原名，目录保留带点的名字。列表里没有时用正在播放的播放集；
/// 播放集是空的再用打开着的播放集页。正在播放的队列里没有这条时，不用另一页的文件名。
#[test]
fn preview_header_title_uses_the_entry_name_and_extension() {
    let mut model = ShellViewModel::default();
    model.inspect.target_path = Some("dir/notes.md".into());
    model.files.rows = vec![named_row("dir/notes.md", "notes.md", "file", Some("txt"))];
    let listed = head_view(&model);
    assert_eq!(listed.name, "notes.md", "扩展名不是 md，标题保留原名");
    assert_eq!(listed.path, "dir/notes.md", "路径和名称不同时仍写出路径");

    model.files.rows = vec![named_row(
        "albums/photos.v2",
        "photos.v2",
        "directory",
        Some("v2"),
    )];
    model.inspect.target_path = Some("albums/photos.v2".into());
    let directory = head_view(&model);
    assert_eq!(directory.name, "photos.v2", "目录不去掉扩展名");
    assert_eq!(directory.path, "albums/photos.v2");

    model.inspect.target_path = Some("dir/notes.md".into());
    model.files.rows = vec![named_row("dir/notes.md", "列表.md", "file", Some("txt"))];
    model.player.queue = vec![queue_item("dir/notes.md", "队列.md", "md", false)];
    assert_eq!(head_view(&model).name, "列表.md", "当前列表优先于播放集");

    model.files.rows.clear();
    model.player.queue.clear();
    model.player.listed = Some(playlist_page("album/track.mp3", "夜曲.mp3", "mp3"));
    model.inspect.target_path = Some("album/track.mp3".into());
    let page = head_view(&model);
    assert_eq!(
        page.name, "夜曲",
        "没在播放时用打开着的播放集页，扩展名对得上才去掉"
    );
    assert_eq!(page.path, "album/track.mp3");

    model.player.queue = vec![queue_item("album/track.mp3", "track.mp3", "mp3", true)];
    assert_eq!(
        head_view(&model).name,
        "夜曲",
        "预览临时条目不算正在播放的播放集"
    );

    model.player.queue = vec![queue_item("other/a.mp3", "别的.mp3", "mp3", false)];
    let skipped = head_view(&model);
    assert_eq!(
        skipped.name, "track",
        "正在播放的播放集里没有这条，不用另一页的文件名"
    );
    assert_ne!(skipped.name, "夜曲");
}

fn named_row(path: &str, name: &str, kind: &str, extension: Option<&str>) -> FileRow {
    let mut row = FileRow::from_entry(&entry(path, BTreeMap::new()));
    row.name = name.into();
    row.kind = kind.into();
    row.extension = extension.map(str::to_string);
    row
}

fn queue_item(path: &str, filename: &str, extension: &str, transient: bool) -> QueueItem {
    QueueItem {
        id: "item".into(),
        playlist_id: "pl".into(),
        asset_id: "asset".into(),
        path: path.into(),
        filename: filename.into(),
        extension: extension.into(),
        status: "ready".into(),
        status_reason: None,
        transient,
        player_type_id: "audio".into(),
        player_label: "音频".into(),
        file_class: "audio".into(),
        thumbnail_path: None,
        is_virtual: false,
        local_absolute_path: None,
    }
}

fn playlist_page(path: &str, filename: &str, extension: &str) -> PlaylistDetail {
    PlaylistDetail {
        playlist: PlaylistSummary {
            playlist_id: "pl".into(),
            repo_id: "repo".into(),
            name: "早晨".into(),
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
            asset_id: "asset".into(),
            path: path.into(),
            filename: filename.into(),
            extension: extension.into(),
            thumbnail_path: None,
            status: "ready".into(),
            status_reason: None,
            sort_order: 0,
            added_at: String::new(),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            metadata: None,
            local_absolute_path: None,
        }],
    }
}

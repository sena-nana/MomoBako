//! 预览右侧四行事实的测试。
//!
//! 类型、大小、硬链接和修改时间跟 Vue `previewFileEntry`：当前列表优先；列表里没有、
//! 播放队列是空的，才用打开着的播放集页的扩展名。`selectAsset` 把素材详情写回来以后，
//! 这四行仍是这一条，不换成素材摘要。

use std::collections::BTreeMap;

use serde_json::json;

use crate::backend::services::repository::{
    AssetDetail, AssetSummary, FileBrowserEntry, MetadataEntry, PlaylistDetail, PlaylistItem,
    PlaylistSummary,
};
use crate::shell::files::local_time;
use crate::shell::player::{PlayerMessage, QueueItem};
use crate::shell::{
    FileRow, InspectMessage, ShellMessage, ShellPage, ShellViewModel, files::FilesMessage,
};

use super::super::super::inspect_library::FactsView;

const WHEN: &str = "2026-03-04T05:06:07Z";

/// 详情还没回来时，右侧四行用当前列表。列表里没有时，空队列才用打开着的播放集页的扩展名；
/// 正在播放的队列里没有这条时，不用另一页的扩展名，也不编大小和修改时间。
#[test]
fn preview_stats_follow_the_list_entry_before_the_detail() {
    let mut model = ShellViewModel::default();
    model.page = ShellPage::SelectedFile;
    model.files.rows = vec![FileRow::from_entry(&file_entry(
        "voice/a.mp3",
        "mp3",
        "4 KB",
        WHEN,
        "linked",
    ))];
    model.reduce(ShellMessage::Files(FilesMessage::OpenRow(
        "voice/a.mp3".into(),
    )));
    let listed = FactsView::of(&model.inspect.facts);
    assert_eq!(listed.extension, "mp3");
    assert_eq!(listed.size, "4 KB");
    assert_eq!(listed.hardlink, "硬链接关联");
    assert_eq!(listed.modified, local_time::format_or(WHEN, "未记录"));
    assert!(model.inspect.current_asset_id().is_none(), "详情还没回来");

    let mut playlist = ShellViewModel::default();
    playlist.player.listed = Some(page("voice/b.wav", "b.wav", "wav"));
    playlist.reduce(ShellMessage::Player(PlayerMessage::OpenPreview {
        item_id: Some("item".into()),
    }));
    let from_page = FactsView::of(&playlist.inspect.facts);
    assert_eq!(from_page.extension, "wav", "空队列用打开着的播放集页");
    assert_eq!(from_page.size, "未知");
    assert_eq!(from_page.modified, "未记录");
    assert!(from_page.hardlink.is_empty());

    let mut playing = ShellViewModel::default();
    playing.player.queue = vec![queue_item("other", "other.mp3", "mp3")];
    playing.player.listed = Some(page("voice/b.wav", "b.wav", "wav"));
    playing.reduce(ShellMessage::Player(PlayerMessage::OpenPreview {
        item_id: Some("item".into()),
    }));
    let skipped = FactsView::of(&playing.inspect.facts);
    assert_eq!(
        skipped.extension, "文件",
        "正在播放的队列里没有这条时，不用另一页的扩展名"
    );
    assert_eq!(skipped.size, "未知");
}

/// 详情回来以后，四行仍是 Vue `previewFileEntry`：列表上的值不被素材摘要换掉；
/// 播放集条目没有大小和修改时间，详情里的真实字段也不补上。
#[test]
fn preview_stats_stay_on_the_entry_after_the_detail() {
    let path = "voice/a.mp3";
    let mut listed = ShellViewModel::default();
    listed.page = ShellPage::SelectedFile;
    listed.files.rows = vec![FileRow::from_entry(&file_entry(
        path, "mp3", "4 KB", WHEN, "linked",
    ))];
    listed.reduce(ShellMessage::Files(FilesMessage::OpenRow(path.into())));
    listed.reduce(ShellMessage::AssetDetailLoaded(Ok(asset_detail(
        path,
        "bin",
        "99 KB",
        "1999-01-01T00:00:00Z",
        Some("broken"),
    ))));
    let after = FactsView::of(&listed.inspect.facts);
    assert!(listed.inspect.current_asset_id().is_some(), "详情已经回来");
    assert_eq!(after.extension, "mp3");
    assert_eq!(after.size, "4 KB");
    assert_eq!(after.hardlink, "硬链接关联");
    assert_eq!(after.modified, local_time::format_or(WHEN, "未记录"));

    let mut playlist = ShellViewModel::default();
    playlist.player.listed = Some(page("voice/b.wav", "b.wav", "wav"));
    playlist.reduce(ShellMessage::Player(PlayerMessage::OpenPreview {
        item_id: Some("item".into()),
    }));
    playlist.reduce(ShellMessage::AssetDetailLoaded(Ok(asset_detail(
        "voice/b.wav",
        "bin",
        "99 KB",
        WHEN,
        Some("linked"),
    ))));
    let from_page = FactsView::of(&playlist.inspect.facts);
    assert!(
        playlist.inspect.current_asset_id().is_some(),
        "播放集这条的详情已经回来"
    );
    assert_eq!(from_page.extension, "wav");
    assert_eq!(from_page.size, "未知", "播放集条目没有大小，详情不补");
    assert_eq!(
        from_page.modified, "未记录",
        "播放集条目没有修改时间，详情不补"
    );
    assert!(
        from_page.hardlink.is_empty(),
        "播放集条目没有硬链接，详情不补"
    );
}

/// 预览页编辑器跟 Vue `previewFileEntry`：四个输入来自列表条目，不跟详情卡导语的叠层。
///
/// 空 comment 退回 note。详情编号不同、注释和链接不同也不盖住。正在输入时详情再来一次，草稿留着。
/// 单击这条只核对详情里的非空注释会进详情卡；叠层的其余规则另测。
#[test]
fn preview_editor_draft_follows_the_list_entry_not_the_asset_detail() {
    let path = "voice/a.mp3";
    let mut metadata = BTreeMap::new();
    metadata.insert("comment".into(), json!(""));
    metadata.insert("note".into(), json!("列表备注"));
    metadata.insert("link".into(), json!(" https://list.example "));
    metadata.insert("rating".into(), json!(2));
    metadata.insert("tagGroups".into(), json!(["列表"]));
    let mut listed = ShellViewModel::default();
    listed.page = ShellPage::SelectedFile;
    listed.files.rows = vec![FileRow::from_entry(&entry_with(path, metadata))];
    listed.reduce(ShellMessage::Files(FilesMessage::OpenRow(path.into())));
    assert_eq!(
        listed.inspect.draft_comment(),
        "列表备注",
        "空 comment 用列表里的 note"
    );
    assert_eq!(
        listed.inspect.draft_link(),
        " https://list.example ",
        "链接留着列表里的空白"
    );
    assert_eq!(listed.inspect.draft_rating(), 2);
    assert_eq!(listed.inspect.draft_tags(), ["列表".to_string()]);

    listed.reduce(ShellMessage::AssetDetailLoaded(Ok(editor_detail(
        path,
        "other-asset",
        "详情注释",
        "https://detail.example",
        5,
        "详情",
    ))));
    assert_eq!(
        listed.inspect.draft_comment(),
        "列表备注",
        "详情注释不盖列表"
    );
    assert_eq!(listed.inspect.draft_link(), " https://list.example ");
    assert_eq!(listed.inspect.draft_rating(), 2);
    assert_eq!(listed.inspect.draft_tags(), ["列表".to_string()]);
    assert_eq!(
        FactsView::of(&listed.inspect.facts).extension,
        "mp3",
        "右侧类型仍是列表"
    );

    listed.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "正在输入".into(),
    )));
    listed.reduce(ShellMessage::AssetDetailLoaded(Ok(editor_detail(
        path,
        "other-asset",
        "又一份详情",
        "https://again.example",
        1,
        "又",
    ))));
    assert_eq!(
        listed.inspect.draft_comment(),
        "正在输入",
        "正在输入时详情不冲掉草稿"
    );

    let mut clicked = ShellViewModel::default();
    let mut meta = BTreeMap::new();
    meta.insert("note".into(), json!("列表备注"));
    clicked.files.rows = vec![FileRow::from_entry(&entry_with(path, meta))];
    clicked.reduce(ShellMessage::Files(FilesMessage::ActivateRow(path.into())));
    clicked.reduce(ShellMessage::AssetDetailLoaded(Ok(editor_detail(
        path,
        "asset",
        "详情注释",
        "https://detail.example",
        5,
        "详情",
    ))));
    assert!(
        !super::super::super::files_view::previewing(&clicked),
        "单击不进预览页"
    );
    assert_eq!(
        clicked.inspect.draft_comment(),
        "详情注释",
        "详情里的非空注释盖住列表的 note"
    );
}

/// 只读行：详情卡跟 Vue `currentFileEntry` 的叠层，预览页和播放集条目跟 `previewFileEntry`。
///
/// 编号对得上时详情盖住列表上的日期、尺寸、大小、色板和来源；对不上仍是列表。
/// 预览页开着时详情再来也不换。列表没有的创建时间和原始大小不拿详情补。
/// 文件修改时间没有元数据时用这一条自己的修改时间，不用素材摘要。
#[test]
fn recorded_rows_follow_the_detail_overlay_and_the_preview_list() {
    let path = "voice/a.mp3";
    let list_added = local_time::format_or("2020-01-02T03:04:05Z", "未记录");
    let detail_added = local_time::format_or("1999-06-07T08:09:10Z", "未记录");
    let detail_created = local_time::format_or("2011-12-13T14:15:16Z", "未记录");
    let detail_modified = local_time::format_or("2018-04-05T06:07:08Z", "未记录");
    let entry_modified = local_time::format_or(WHEN, "未记录");

    let mut clicked = ShellViewModel::default();
    clicked.files.rows = vec![FileRow::from_entry(&entry_with(
        path,
        list_recorded_metadata(),
    ))];
    clicked.reduce(ShellMessage::Files(FilesMessage::ActivateRow(path.into())));
    assert!(
        !super::super::super::files_view::previewing(&clicked),
        "单击不进预览页"
    );
    assert_list_recorded(
        &super::super::super::inspect_metadata_view::recorded_rows(&clicked),
        &list_added,
        &entry_modified,
    );

    let mut mismatched = ShellViewModel::default();
    mismatched.files.rows = vec![FileRow::from_entry(&entry_with(
        path,
        list_recorded_metadata(),
    ))];
    mismatched.reduce(ShellMessage::Files(FilesMessage::ActivateRow(path.into())));
    mismatched.reduce(ShellMessage::AssetDetailLoaded(Ok(recorded_detail(
        path,
        "other-asset",
    ))));
    assert!(
        !super::super::super::files_view::previewing(&mismatched),
        "编号对不上仍停在详情卡"
    );
    assert_list_recorded(
        &super::super::super::inspect_metadata_view::recorded_rows(&mismatched),
        &list_added,
        &entry_modified,
    );

    let mut matched_model = ShellViewModel::default();
    matched_model.files.rows = vec![FileRow::from_entry(&entry_with(
        path,
        list_recorded_metadata(),
    ))];
    matched_model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(path.into())));
    matched_model.reduce(ShellMessage::AssetDetailLoaded(Ok(recorded_detail(
        path, "asset",
    ))));
    assert!(
        !super::super::super::files_view::previewing(&matched_model),
        "对得上的详情仍停在详情卡"
    );
    let matched = super::super::super::inspect_metadata_view::recorded_rows(&matched_model);
    assert_eq!(matched.added, detail_added, "对得上的详情盖住列表日期");
    assert_eq!(matched.created, detail_created, "列表没有的创建时间用详情");
    assert_eq!(
        matched.file_modified, detail_modified,
        "详情里的文件修改时间盖住条目修改时间"
    );
    assert_eq!(matched.dimensions, "1920 × 1080");
    assert_eq!(matched.original, "2.0 KB");
    assert_eq!(matched.palette, ["#AABBCC"]);
    assert_eq!(matched.source_url, "https://detail.example/a");
    assert_eq!(matched.origin_title, "详情标题");

    let mut preview = ShellViewModel::default();
    preview.page = ShellPage::SelectedFile;
    preview.files.rows = vec![FileRow::from_entry(&entry_with(
        path,
        list_recorded_metadata(),
    ))];
    preview.reduce(ShellMessage::Files(FilesMessage::OpenRow(path.into())));
    preview.reduce(ShellMessage::AssetDetailLoaded(Ok(recorded_detail(
        path, "asset",
    ))));
    assert!(
        super::super::super::files_view::previewing(&preview),
        "双击停在预览页"
    );
    assert_list_recorded(
        &super::super::super::inspect_metadata_view::recorded_rows(&preview),
        &list_added,
        &entry_modified,
    );

    let mut playlist = ShellViewModel::default();
    let mut detail = page("voice/b.wav", "b.wav", "wav");
    detail.items[0].metadata = Some(json!({
        "addedToLibraryAt": "2020-01-02T03:04:05Z",
        "sourceUrl": "https://playlist.example/a",
    }));
    playlist.player.listed = Some(detail);
    playlist.reduce(ShellMessage::Player(PlayerMessage::OpenPreview {
        item_id: Some("item".into()),
    }));
    playlist.reduce(ShellMessage::AssetDetailLoaded(Ok(recorded_detail(
        "voice/b.wav",
        "asset",
    ))));
    let from_item = super::super::super::inspect_metadata_view::recorded_rows(&playlist);
    assert_eq!(from_item.added, list_added, "播放集条目的日期不被详情换掉");
    assert_eq!(from_item.source_url, "https://playlist.example/a");
    assert_eq!(from_item.created, "未记录", "条目没有创建时间，详情不补");
    assert_eq!(
        from_item.file_modified, "未记录",
        "播放集条目没有修改时间，详情不补"
    );
    assert_eq!(from_item.dimensions, "未记录");
    assert_eq!(from_item.original, "未记录");
    assert!(from_item.origin_title.is_empty());
}

/// 作品信息和自定义行跟只读行同一条叠层：详情卡用 `currentFileEntry`，预览页和播放集只用条目。
///
/// 编号对得上时详情盖住同名字段，详情没有的键留着列表的，两边都没有的不编造。
/// 正在输入和还没保存的补丁不写进这两块，详情到达也不冲掉输入框里的草稿。
/// 索引标签和多归属位置仍只读列表条目（Vue `withActiveAssetMetadata` 只换 metadata）。
#[test]
fn work_info_and_custom_rows_follow_the_detail_overlay() {
    let path = "voice/a.mp3";
    let mut listed = asmr_card(path, list_work_metadata());
    listed.files.rows[0].tags = vec!["列表标签".into()];
    listed.files.rows[0].alias_paths = vec![path.into(), "别的/a.mp3".into()];
    assert_work(
        &listed,
        "列表作品",
        "列表社团",
        "列表画师",
        "https://list.example/cover.jpg",
    );

    listed.reduce(ShellMessage::AssetDetailLoaded(Ok(work_detail(
        path,
        "asset",
        detail_work_metadata(),
    ))));
    assert!(
        !super::super::super::files_view::previewing(&listed),
        "单击不进预览页"
    );
    assert_work(
        &listed,
        "详情作品",
        "列表社团",
        "详情画师",
        "https://detail.example/cover.jpg",
    );
    assert_no_series(&listed);
    let recorded = super::super::super::inspect_metadata_view::recorded_rows(&listed);
    assert_eq!(recorded.indexed_tags, "列表标签", "索引标签不叠详情摘要");
    assert_eq!(recorded.aliases, "别的/a.mp3", "多归属位置不叠详情");
    assert_eq!(listed.inspect.draft_comment(), "详情注释");

    let patched = listed.inspect.apply_metadata_patch(
        "asset",
        BTreeMap::from([("circle".into(), json!("草稿社团"))]),
    );
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert_work(
        &listed,
        "详情作品",
        "列表社团",
        "详情画师",
        "https://detail.example/cover.jpg",
    );
    assert_eq!(
        listed
            .inspect
            .draft_custom()
            .get("circle")
            .and_then(|value| value.as_str()),
        Some("草稿社团")
    );
    assert_eq!(
        listed.inspect.draft_comment(),
        "详情注释",
        "没保存的补丁不冲掉注释"
    );

    let mut saved = detail_work_metadata();
    saved.insert("circle".into(), json!("服务器社团"));
    listed.reduce(ShellMessage::Inspect(InspectMessage::MetadataSaved(Ok((
        "success".into(),
        work_detail(path, "asset", saved),
    )))));
    assert_work(
        &listed,
        "详情作品",
        "服务器社团",
        "详情画师",
        "https://detail.example/cover.jpg",
    );

    let mut mismatched = asmr_card(path, list_work_metadata());
    mismatched.reduce(ShellMessage::AssetDetailLoaded(Ok(work_detail(
        path,
        "other-asset",
        detail_work_metadata(),
    ))));
    assert_work(
        &mismatched,
        "列表作品",
        "列表社团",
        "列表画师",
        "https://list.example/cover.jpg",
    );

    let mut typing = asmr_card(path, list_work_metadata());
    typing.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "正在输入".into(),
    )));
    typing.reduce(ShellMessage::AssetDetailLoaded(Ok(work_detail(
        path,
        "asset",
        detail_work_metadata(),
    ))));
    assert_eq!(
        typing.inspect.draft_comment(),
        "正在输入",
        "正在输入时详情不冲掉草稿"
    );
    assert_work(
        &typing,
        "详情作品",
        "列表社团",
        "详情画师",
        "https://detail.example/cover.jpg",
    );

    let mut preview = asmr_card(path, list_work_metadata());
    preview.page = ShellPage::SelectedFile;
    preview.reduce(ShellMessage::Files(FilesMessage::OpenRow(path.into())));
    preview.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "正在输入".into(),
    )));
    preview.reduce(ShellMessage::AssetDetailLoaded(Ok(work_detail(
        path,
        "asset",
        detail_work_metadata(),
    ))));
    assert!(
        super::super::super::files_view::previewing(&preview),
        "双击停在预览页"
    );
    assert_eq!(
        preview.inspect.draft_comment(),
        "正在输入",
        "预览页正在输入时详情不冲掉草稿"
    );
    assert_work(
        &preview,
        "列表作品",
        "列表社团",
        "列表画师",
        "https://list.example/cover.jpg",
    );

    let mut playlist = ShellViewModel::for_page(ShellPage::FileList);
    let mut detail = page("voice/b.wav", "b.wav", "wav");
    detail.items[0].metadata = Some(json!({
        "libraryKind": "asmr",
        "workTitle": "播放集作品",
        "circle": "播放集社团",
        "artist": "播放集画师",
        "coverUrl": "https://playlist.example/cover.jpg",
    }));
    playlist.player.listed = Some(detail);
    playlist.reduce(ShellMessage::Player(PlayerMessage::OpenPreview {
        item_id: Some("item".into()),
    }));
    let mut foreign = detail_work_metadata();
    foreign.insert("series".into(), json!("不该出现"));
    playlist.reduce(ShellMessage::AssetDetailLoaded(Ok(work_detail(
        "voice/b.wav",
        "asset",
        foreign,
    ))));
    assert_work(
        &playlist,
        "播放集作品",
        "播放集社团",
        "播放集画师",
        "https://playlist.example/cover.jpg",
    );
    assert_no_series(&playlist);
}

fn list_recorded_metadata() -> BTreeMap<String, serde_json::Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert("addedToLibraryAt".into(), json!("2020-01-02T03:04:05Z"));
    metadata.insert("sourceUrl".into(), json!("https://list.example/a"));
    metadata.insert("width".into(), json!(100));
    metadata.insert("height".into(), json!(50));
    metadata.insert("palette".into(), json!(["#112233"]));
    metadata
}

fn recorded_detail(path: &str, asset_id: &str) -> AssetDetail {
    let mut detail = asset_detail(path, "bin", "99 KB", "1999-01-01T00:00:00Z", Some("broken"));
    detail.summary.asset_id = asset_id.into();
    let meta = |key: &str, value: serde_json::Value| MetadataEntry {
        key: key.into(),
        value_type: "json".into(),
        value,
        version: 1,
        updated_at: String::new(),
    };
    detail.metadata = vec![
        meta("addedToLibraryAt", json!("1999-06-07T08:09:10Z")),
        meta("fileCreatedAt", json!("2011-12-13T14:15:16Z")),
        meta("fileModifiedAt", json!("2018-04-05T06:07:08Z")),
        meta("width", json!(1920)),
        meta("height", json!(1080)),
        meta("originalSizeBytes", json!(2048)),
        meta("palette", json!(["#AABBCC"])),
        meta("sourceUrl", json!("https://detail.example/a")),
        meta("originTitle", json!("详情标题")),
    ];
    detail
}

fn assert_list_recorded(
    rows: &super::super::super::inspect_metadata_view::RecordedView,
    added: &str,
    modified: &str,
) {
    assert_eq!(rows.added, added);
    assert_eq!(rows.created, "未记录");
    assert_eq!(
        rows.file_modified, modified,
        "没有文件修改时间时用这一条自己的修改时间"
    );
    assert_eq!(rows.dimensions, "100 × 50");
    assert_eq!(rows.original, "未记录");
    assert_eq!(rows.palette, ["#112233"]);
    assert_eq!(rows.source_url, "https://list.example/a");
    assert!(rows.origin_title.is_empty());
}

fn editor_detail(
    path: &str,
    asset_id: &str,
    comment: &str,
    link: &str,
    rating: i64,
    tag: &str,
) -> AssetDetail {
    let mut detail = asset_detail(path, "bin", "99 KB", "1999-01-01T00:00:00Z", Some("broken"));
    detail.summary.asset_id = asset_id.into();
    detail.metadata = vec![
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
        MetadataEntry {
            key: "rating".into(),
            value_type: "number".into(),
            value: json!(rating),
            version: 1,
            updated_at: String::new(),
        },
        MetadataEntry {
            key: "tagGroups".into(),
            value_type: "json".into(),
            value: json!([tag]),
            version: 1,
            updated_at: String::new(),
        },
    ];
    detail
}

fn entry_with(path: &str, metadata: BTreeMap<String, serde_json::Value>) -> FileBrowserEntry {
    let mut entry = file_entry(path, "mp3", "4 KB", WHEN, "linked");
    entry.metadata = metadata;
    entry
}

fn asset_detail(
    path: &str,
    extension: &str,
    size: &str,
    modified: &str,
    hardlink: Option<&str>,
) -> AssetDetail {
    AssetDetail {
        summary: AssetSummary {
            asset_id: "asset".into(),
            repo_id: "repo".into(),
            path: path.into(),
            filename: path.rsplit('/').next().unwrap_or(path).into(),
            extension: extension.into(),
            size_bytes: 99,
            size_label: size.into(),
            status: "ready".into(),
            modified_at: modified.into(),
            last_accessed_at: None,
            version: 1,
            tags: Vec::new(),
            thumbnail_path: None,
            hardlink_group_id: None,
            hardlink_state: hardlink.map(str::to_string),
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

fn file_entry(
    path: &str,
    extension: &str,
    size: &str,
    modified: &str,
    hardlink: &str,
) -> FileBrowserEntry {
    FileBrowserEntry {
        path: path.into(),
        name: "a.mp3".into(),
        kind: "file".into(),
        extension: Some(extension.into()),
        size_bytes: Some(4096),
        size_label: Some(size.into()),
        modified_at: Some(modified.into()),
        asset_id: Some("asset".into()),
        status: Some("ready".into()),
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: Some(hardlink.into()),
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

fn page(path: &str, filename: &str, extension: &str) -> PlaylistDetail {
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

/// 单击选中一条 ASMR 文件，插件用验收底子里的资源库扩展。详情还没回来。
fn asmr_card(path: &str, metadata: BTreeMap<String, serde_json::Value>) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.files.rows = vec![FileRow::from_entry(&entry_with(path, metadata))];
    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(path.into())));
    model
}

fn list_work_metadata() -> BTreeMap<String, serde_json::Value> {
    BTreeMap::from([
        ("libraryKind".into(), json!("asmr")),
        ("workTitle".into(), json!("列表作品")),
        ("circle".into(), json!("列表社团")),
        ("artist".into(), json!("列表画师")),
        ("coverUrl".into(), json!("https://list.example/cover.jpg")),
        ("comment".into(), json!("")),
        ("note".into(), json!("列表备注")),
    ])
}

fn detail_work_metadata() -> BTreeMap<String, serde_json::Value> {
    BTreeMap::from([
        ("libraryKind".into(), json!("asmr")),
        ("workTitle".into(), json!("详情作品")),
        ("artist".into(), json!("详情画师")),
        ("coverUrl".into(), json!("https://detail.example/cover.jpg")),
        ("comment".into(), json!("详情注释")),
    ])
}

fn work_detail(
    path: &str,
    asset_id: &str,
    metadata: BTreeMap<String, serde_json::Value>,
) -> AssetDetail {
    let mut detail = asset_detail(path, "mp3", "4 KB", WHEN, Some("linked"));
    detail.summary.asset_id = asset_id.into();
    detail.summary.tags = vec!["详情标签".into()];
    detail.metadata = metadata
        .into_iter()
        .map(|(key, value)| MetadataEntry {
            key,
            value_type: "json".into(),
            value,
            version: 1,
            updated_at: String::new(),
        })
        .collect();
    detail
}

fn assert_work(model: &ShellViewModel, title: &str, circle: &str, artist: &str, cover: &str) {
    let (works, custom, button) =
        super::super::super::inspect_metadata_view::work_and_custom_rows(model);
    assert_eq!(
        labeled(&works, "标题").as_deref(),
        Some(title),
        "作品标题：{works:?}"
    );
    assert_eq!(
        labeled(&works, "社团").as_deref(),
        Some(circle),
        "社团：{works:?}"
    );
    assert_eq!(
        custom_value(&custom, "artist").as_deref(),
        Some(artist),
        "自定义行：{custom:?}"
    );
    assert!(
        custom.iter().all(|(key, _)| key != "workTitle"),
        "作品标题不进自定义行：{custom:?}"
    );
    assert_eq!(button.as_deref(), Some(cover), "封面跟同一条叠层");
}

fn assert_no_series(model: &ShellViewModel) {
    let (works, custom, _) =
        super::super::super::inspect_metadata_view::work_and_custom_rows(model);
    assert!(
        labeled(&works, "系列").is_none(),
        "没有系列就不画：{works:?}"
    );
    assert!(
        custom.iter().all(|(key, _)| key != "series"),
        "没有系列就不编进自定义行：{custom:?}"
    );
}

fn labeled(rows: &[(String, String)], label: &str) -> Option<String> {
    rows.iter()
        .find(|(item, _)| item == label)
        .map(|(_, value)| value.clone())
}

fn custom_value(rows: &[(String, String)], key: &str) -> Option<String> {
    rows.iter()
        .find(|(item, _)| item == key)
        .map(|(_, value)| value.clone())
}

fn queue_item(id: &str, path: &str, extension: &str) -> QueueItem {
    QueueItem {
        id: id.into(),
        playlist_id: "playing".into(),
        asset_id: id.into(),
        path: path.into(),
        filename: path.into(),
        extension: extension.into(),
        status: "ready".into(),
        status_reason: None,
        transient: false,
        player_type_id: "audio".into(),
        player_label: "音频".into(),
        file_class: "audio".into(),
        thumbnail_path: None,
        is_virtual: false,
        local_absolute_path: None,
    }
}

//! 文件详情区的测试：详情卡说明行叠加匹配资源的元数据，编辑器跟随说明行。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::backend::services::repository::{
    AssetDetail, AssetSummary, FileBrowserEntry, MetadataEntry,
};
use crate::shell::files::local_time;
use crate::shell::{FileRow, InspectMessage, ShellMessage, ShellViewModel, WorkspacePanel};

use super::Projection;

const WHEN: &str = "2026-03-04T05:06:07Z";

/// 单击选中的详情卡：素材 id 对得上时，注释、链接和删除时间叠上详情元数据；
/// 列表行的元数据和四行事实不改，编辑器草稿不写进导语。对不上的素材不叠。
#[test]
fn detail_card_lead_overlays_the_matching_asset_metadata() {
    let path = "voice/a.mp3";
    let mut listed = click_selected(
        path,
        "asset",
        list_metadata("列表注释", " https://list.example ", "列表备注", "列表时间"),
    );
    listed.workspace.panel = WorkspacePanel::Trash;
    let before = Projection::of(&listed).entry;
    assert_eq!(before.comment, "列表注释", "详情还没回来时导语用列表");
    assert_eq!(before.link, "https://list.example");
    assert_eq!(before.deleted, "列表时间");

    listed.reduce(ShellMessage::AssetDetailLoaded(Ok(asset_detail(
        path,
        "asset",
        "bin",
        "99 KB",
        "1999-01-01T00:00:00Z",
        Some("broken"),
        vec![
            meta("comment", json!("详情注释")),
            meta("link", json!("https://detail.example")),
            meta("deletedAt", json!("详情时间")),
        ],
    ))));
    let after = Projection::of(&listed).entry;
    assert_eq!(after.comment, "详情注释", "同键以详情为准");
    assert_eq!(after.link, "https://detail.example");
    assert_eq!(after.deleted, "详情时间", "回收站删除时间也叠详情");
    assert_eq!(after.type_label, "mp3");
    assert_eq!(after.size, "4 KB");
    assert_eq!(after.hardlink, "硬链接关联");
    assert_eq!(after.modified, local_time::format_or(WHEN, "未记录"));
    let row = &listed.files.rows[0];
    assert_eq!(
        row.metadata.get("comment").and_then(Value::as_str),
        Some("列表注释"),
        "列表行元数据不改"
    );
    assert_eq!(row.extension.as_deref(), Some("mp3"));
    assert_eq!(row.size_label, "4 KB");
    assert_eq!(row.hardlink_state.as_deref(), Some("linked"));
    assert_eq!(row.modified_at, WHEN);

    listed.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "草稿注释".into(),
    )));
    assert_eq!(
        Projection::of(&listed).entry.comment,
        "详情注释",
        "编辑器草稿不写进导语"
    );

    let mut blank_comment = click_selected(
        path,
        "asset",
        list_metadata("列表注释", "https://list.example", "列表备注", "列表时间"),
    );
    blank_comment.reduce(ShellMessage::AssetDetailLoaded(Ok(asset_detail(
        path,
        "asset",
        "mp3",
        "4 KB",
        WHEN,
        Some("linked"),
        vec![meta("comment", json!(""))],
    ))));
    assert_eq!(
        Projection::of(&blank_comment).entry.comment,
        "列表备注",
        "空 comment 退回列表里的 note"
    );

    let mut other = click_selected(
        path,
        "asset",
        list_metadata("列表注释", "https://list.example", "列表备注", "列表时间"),
    );
    other.reduce(ShellMessage::AssetDetailLoaded(Ok(asset_detail(
        path,
        "other",
        "bin",
        "99 KB",
        WHEN,
        Some("broken"),
        vec![
            meta("comment", json!("别人的注释")),
            meta("link", json!("https://other.example")),
        ],
    ))));
    let skipped = Projection::of(&other).entry;
    assert_eq!(skipped.comment, "列表注释", "素材 id 对不上不叠");
    assert_eq!(skipped.link, "https://list.example");
    assert_eq!(skipped.size, "4 KB");
}

/// 单击选中时，详情卡编辑器的四个输入跟导语同一套叠层，不按原始素材详情填。
///
/// 详情还没到时用列表。编号对得上时同键以详情为准，详情没有的键留着列表的；空 comment 退回 note。
/// 不退回素材摘要上的索引标签。编号对不上不叠。正在输入时这一次详情不盖草稿，导语仍不读草稿。
#[test]
fn detail_card_editor_follows_the_lead_overlay() {
    let path = "voice/a.mp3";
    let mut listed = activate(path, editor_list());
    assert_eq!(
        listed.inspect.draft_comment(),
        "列表备注",
        "详情还没回来，空 comment 用列表 note"
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
        "asset",
        vec![meta("comment", json!("详情注释"))],
        vec!["索引".into()],
    ))));
    assert!(!super::super::previewing(&listed), "单击不进预览页");
    assert_eq!(listed.inspect.draft_comment(), "详情注释", "同键以详情为准");
    assert_eq!(
        listed.inspect.draft_link(),
        " https://list.example ",
        "详情没有链接，不改成空"
    );
    assert_eq!(listed.inspect.draft_rating(), 2, "详情没有评分，不改成 0");
    assert_eq!(
        listed.inspect.draft_tags(),
        ["列表".to_string()],
        "没有标签组时不退回索引标签"
    );
    assert_eq!(
        Projection::of(&listed).entry.comment,
        "详情注释",
        "导语和编辑器读同一套叠层"
    );
    assert_eq!(
        listed.files.rows[0]
            .metadata
            .get("comment")
            .and_then(Value::as_str),
        Some(""),
        "列表行元数据不改"
    );

    let mut blank = activate(path, editor_list());
    blank.reduce(ShellMessage::AssetDetailLoaded(Ok(editor_detail(
        path,
        "asset",
        vec![meta("comment", json!(""))],
        Vec::new(),
    ))));
    assert_eq!(
        blank.inspect.draft_comment(),
        "列表备注",
        "空 comment 退回列表里的 note"
    );
    assert_eq!(Projection::of(&blank).entry.comment, "列表备注");

    let mut other = activate(path, editor_list());
    other.reduce(ShellMessage::AssetDetailLoaded(Ok(editor_detail(
        path,
        "other",
        vec![
            meta("comment", json!("别人的注释")),
            meta("link", json!("https://other.example")),
            meta("rating", json!(5)),
            meta("tagGroups", json!(["别人"])),
        ],
        vec!["索引".into()],
    ))));
    assert_eq!(
        other.inspect.draft_comment(),
        "列表备注",
        "素材 id 对不上不叠"
    );
    assert_eq!(other.inspect.draft_link(), " https://list.example ");
    assert_eq!(other.inspect.draft_rating(), 2);
    assert_eq!(other.inspect.draft_tags(), ["列表".to_string()]);
    assert_eq!(Projection::of(&other).entry.comment, "列表备注");

    let mut typing = activate(path, editor_list());
    typing.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "正在输入".into(),
    )));
    typing.reduce(ShellMessage::AssetDetailLoaded(Ok(editor_detail(
        path,
        "asset",
        vec![
            meta("comment", json!("详情注释")),
            meta("link", json!("https://detail.example")),
            meta("rating", json!(5)),
        ],
        Vec::new(),
    ))));
    assert!(!super::super::previewing(&typing), "这一次详情仍留在详情卡");
    assert_eq!(
        typing.inspect.draft_comment(),
        "正在输入",
        "正在输入时详情不冲掉草稿"
    );
    assert_eq!(typing.inspect.draft_link(), " https://list.example ");
    assert_eq!(typing.inspect.draft_rating(), 2);
    assert_eq!(
        Projection::of(&typing).entry.comment,
        "详情注释",
        "导语不读未保存草稿"
    );
}

/// 单击选中一条带编辑器元数据的文件，详情还没回来。
fn activate(path: &str, metadata: BTreeMap<String, Value>) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.files.rows = vec![FileRow::from_entry(&file_entry(path, "asset", metadata))];
    model.reduce(ShellMessage::Files(
        crate::shell::files::FilesMessage::ActivateRow(path.into()),
    ));
    model
}

fn editor_list() -> BTreeMap<String, Value> {
    let mut metadata = list_metadata("", " https://list.example ", "列表备注", "列表时间");
    metadata.insert("rating".into(), json!(2));
    metadata.insert("tagGroups".into(), json!(["列表"]));
    metadata
}

fn editor_detail(
    path: &str,
    asset_id: &str,
    metadata: Vec<MetadataEntry>,
    tags: Vec<String>,
) -> AssetDetail {
    let mut detail = asset_detail(
        path,
        asset_id,
        "bin",
        "99 KB",
        "1999-01-01T00:00:00Z",
        Some("broken"),
        metadata,
    );
    detail.summary.tags = tags;
    detail
}

/// 单击选中、详情还没把预览页打开。`click_load` 对得上时详情回来仍留在列表。
fn click_selected(path: &str, asset_id: &str, metadata: BTreeMap<String, Value>) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.files.rows = vec![FileRow::from_entry(&file_entry(path, asset_id, metadata))];
    model.files.primary = Some(path.to_string());
    model.files.selected = vec![path.to_string()];
    model.files.select_only = Some(path.to_string());
    model.files.click_load = Some(path.to_string());
    model
}

fn list_metadata(comment: &str, link: &str, note: &str, deleted: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("comment".into(), json!(comment)),
        ("link".into(), json!(link)),
        ("note".into(), json!(note)),
        ("deletedAt".into(), json!(deleted)),
    ])
}

fn meta(key: &str, value: Value) -> MetadataEntry {
    MetadataEntry {
        key: key.into(),
        value_type: "string".into(),
        value,
        version: 1,
        updated_at: String::new(),
    }
}

fn file_entry(path: &str, asset_id: &str, metadata: BTreeMap<String, Value>) -> FileBrowserEntry {
    FileBrowserEntry {
        path: path.into(),
        name: "a.mp3".into(),
        kind: "file".into(),
        extension: Some("mp3".into()),
        size_bytes: Some(4096),
        size_label: Some("4 KB".into()),
        modified_at: Some(WHEN.into()),
        asset_id: Some(asset_id.into()),
        status: Some("ready".into()),
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: Some("linked".into()),
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

fn asset_detail(
    path: &str,
    asset_id: &str,
    extension: &str,
    size: &str,
    modified: &str,
    hardlink: Option<&str>,
    metadata: Vec<MetadataEntry>,
) -> AssetDetail {
    AssetDetail {
        summary: AssetSummary {
            asset_id: asset_id.into(),
            repo_id: "repo".into(),
            path: path.into(),
            filename: "a.mp3".into(),
            extension: extension.into(),
            size_bytes: 99,
            size_label: size.into(),
            status: "ready".into(),
            modified_at: modified.into(),
            last_accessed_at: None,
            version: 2,
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
        metadata,
        revisions: Vec::new(),
    }
}

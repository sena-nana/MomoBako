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

/// L1：「应用」按原始 JSON 类型写回——声优还是数组、价格和评分还是数字、布尔还是布尔，来源链接也写；
/// 注释和收听状态受保护，不写。
#[test]
fn applying_a_candidate_keeps_json_types_and_the_source_url() {
    let files = [(
        "voice.mp3",
        audio(None, &[("providerCandidates", json!([dlsite_candidate()]))]),
    )];
    let mut model = scene(&files, "voice.mp3");
    asmr(&mut model, AsmrMessage::Apply(0));
    model.reduce(ShellMessage::Inspect(InspectMessage::SaveMetadata));
    let metadata = saved(&mut model).expect("应用候选以后保存");
    assert_eq!(metadata.get("voiceActors"), Some(&json!(["甲", "乙"])));
    assert_eq!(metadata.get("price"), Some(&json!(1320)));
    assert_eq!(metadata.get("rateAverage"), Some(&json!(4.5)));
    assert_eq!(metadata.get("nsfw"), Some(&json!(false)));
    assert_eq!(metadata.get("workId"), Some(&json!("RJ01234567")));
    assert_eq!(
        metadata.get("sourceUrl"),
        Some(&json!(
            "https://www.dlsite.com/maniax/work/=/product_id/RJ01234567.html"
        ))
    );
    assert!(
        !metadata.contains_key("comment") && !metadata.contains_key("listeningStatus"),
        "受保护的字段不写：{metadata:?}"
    );
}

/// L1：导入的候选追加进当前文件的 `providerCandidates` 并保存；换到别的文件以后它不出现，
/// 在别的文件上也就应用不到它。
#[test]
fn imported_candidates_are_saved_on_their_own_file_and_do_not_follow_other_files() {
    let files = [
        (
            "a.mp3",
            audio(None, &[("providerCandidates", json!([dlsite_candidate()]))]),
        ),
        ("b.mp3", audio(None, &[])),
    ];
    let mut model = scene(&files, "a.mp3");
    asmr(&mut model, AsmrMessage::ToggleImport);
    asmr(
        &mut model,
        AsmrMessage::SetImportDraft(r#"{"circle":"手填的社团"}"#.into()),
    );
    asmr(&mut model, AsmrMessage::ImportCandidate);
    model.reduce(ShellMessage::Inspect(InspectMessage::SaveMetadata));
    let metadata = saved(&mut model).expect("导入候选以后保存当前文件");
    assert_eq!(
        metadata.get("providerCandidates"),
        Some(
            &json!([dlsite_candidate(), { "source": "manual", "confidence": "manual", "fields": { "circle": "手填的社团" } }])
        ),
        "追加在已有候选后面，已有的原样保留"
    );

    open(&mut model, &files, "b.mp3");
    let custom = super::display_custom(&model);
    let view = super::candidates_view(&model, &custom);
    assert!(view.shown, "b.mp3 也是 ASMR 条目");
    assert!(
        view.candidates.is_empty(),
        "a.mp3 导入的候选不跟到 b.mp3：{:?}",
        view.candidates
    );
}

/// L2：「抓取候选」调来源插件的 `provider.lookupMetadataCandidate`，载荷是作品 ID，不带仓库。
#[test]
fn lookup_calls_the_provider_plugin_with_the_work_id() {
    let files = [("voice.mp3", audio(None, &[("rjCode", json!("RJ01234567"))]))];
    let mut model = scene(&files, "voice.mp3");
    asmr(&mut model, AsmrMessage::ToggleImport);
    asmr(&mut model, AsmrMessage::Lookup);
    let calls = model
        .admin
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            AdminEffect::CallPlugin {
                plugin_id,
                method,
                payload,
                repository_id,
                ..
            } => Some((plugin_id, method, payload, repository_id)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        calls,
        [(
            "momobako.service.provider.dlsite".to_string(),
            "provider.lookupMetadataCandidate".to_string(),
            json!({ "id": "RJ01234567" }),
            None
        )]
    );
}

/// L3：作品队列里点一条，像双击一样打开它的预览：选中它、读它的素材详情；详情回来以后预览换成它。
#[test]
fn clicking_a_queue_track_opens_its_preview() {
    let files = [
        (
            "work/01.mp3",
            audio(Some("work"), &[("trackTitle", json!("序章"))]),
        ),
        (
            "work/02.mp3",
            audio(Some("work"), &[("trackTitle", json!("第二轨"))]),
        ),
    ];
    let mut model = scene(&files, "work/01.mp3");
    asmr(&mut model, AsmrMessage::Open("work/02.mp3".into()));
    let loads = model
        .files
        .take_effects()
        .into_iter()
        .filter(|effect| matches!(effect, FilesEffect::LoadAsset { asset_id, .. } if asset_id == "work-02.mp3"))
        .count();
    assert_eq!(loads, 1, "读第二轨的素材详情");
    assert_eq!(model.inspect.target_path.as_deref(), Some("work/02.mp3"));
    assert_eq!(model.files.selected, ["work/02.mp3"]);
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        "work/02.mp3",
        &files[1].1,
        1,
    ))));
    assert_eq!(model.page, ShellPage::SelectedFile);
    assert!(
        super::super::files_view::previewing(&model),
        "停在预览页，换成第二轨"
    );
    assert_eq!(model.inspect.current_asset_id(), Some("work-02.mp3"));
}

/// L3：换文件以后、新文件的详情回来之前，作品信息不再叠着上一个文件的草稿值。
#[test]
fn the_previous_file_draft_does_not_leak_into_the_next_file() {
    let files = [
        ("a.mp3", audio(None, &[("workTitle", json!("上一部作品"))])),
        ("b.mp3", audio(None, &[])),
    ];
    let mut model = scene(&files, "a.mp3");
    model.reduce(ShellMessage::SelectFile {
        path: "b.mp3".into(),
        asset_id: Some("b.mp3".into()),
    });
    let custom = super::display_custom(&model);
    assert!(
        !custom.contains_key("workTitle"),
        "详情回来之前只看 b.mp3 的列表行：{custom:?}"
    );
}

/// L7：作品信息里的数组连成「甲，乙」，不显示成 JSON。
#[test]
fn work_info_lists_arrays_as_text() {
    let files = [(
        "voice.mp3",
        audio(
            None,
            &[
                ("voiceActors", json!(["甲", "乙"])),
                ("scenarioTags", json!(["治愈"])),
            ],
        ),
    )];
    let model = scene(&files, "voice.mp3");
    let sections = super::super::inspect_library::library_sections(&super::display_custom(&model));
    let rows = sections
        .iter()
        .flat_map(|section| &section.rows)
        .filter_map(|row| match row {
            super::super::inspect_library::LibraryRow::Value { label, value, .. } => {
                Some((label.to_string(), value.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        rows.contains(&("声优".to_string(), "甲，乙".to_string())),
        "{rows:?}"
    );
    assert!(
        rows.contains(&("标签".to_string(), "治愈".to_string())),
        "{rows:?}"
    );
}

/// L7：停用 ASMR 资源库插件以后，补全候选、作品队列和播放列表都不出现（Vue 里扩展跟着插件卸载）。
#[test]
fn extension_panels_need_the_asmr_plugin() {
    let files = [
        ("work/01.mp3", audio(Some("work"), &[])),
        ("work/02.mp3", audio(Some("work"), &[])),
    ];
    let mut model = scene(&files, "work/01.mp3");
    assert!(super::preview_panels(&model).is_some());
    assert!(super::candidates_view(&model, &super::display_custom(&model)).shown);
    for plugin in model
        .admin
        .plugins
        .iter_mut()
        .filter(|plugin| plugin.plugin_id == "momobako.library.asmr")
    {
        plugin.enabled = false;
    }
    assert!(
        super::preview_panels(&model).is_none(),
        "作品队列和播放列表跟着插件停用"
    );
    assert!(
        !super::candidates_view(&model, &super::display_custom(&model)).shown,
        "补全候选跟着插件停用"
    );
}

/// L6：文件列表上的 ASMR 摘要（Vue `fileSummary`）：RJ 号、作品名、收听状态、进度和「歌词」用「 · 」连起来；
/// 后端种下的标题就是文件名，不当作品名；不是 ASMR 条目没有。
#[test]
fn file_summary_joins_the_work_fields() {
    let summary = |name: &str, metadata: Meta| {
        super::super::inspect_library::file_summary(&row(name, &metadata))
    };
    let full = audio(
        None,
        &[
            ("rjCode", json!("RJ01234567")),
            ("workTitle", json!("夜曲")),
            ("listeningStatus", json!("listening")),
            ("listeningProgress", json!(40.4)),
            ("lyricStatus", json!("local")),
        ],
    );
    assert_eq!(
        summary("01.mp3", full),
        "RJ01234567 · 夜曲 · 收听中 · 40% · 歌词"
    );
    assert_eq!(
        summary("01.mp3", audio(None, &[("title", json!("01.mp3"))])),
        "",
        "标题就是文件名时不算作品名"
    );
    assert_eq!(
        summary(
            "01.mp3",
            audio(
                None,
                &[("workId", json!("RJ1")), ("title", json!("夜曲 01"))]
            )
        ),
        "RJ1 · 夜曲 01"
    );
    assert_eq!(
        summary(
            "01.mp3",
            Meta::from([("workTitle".to_string(), json!("夜曲"))])
        ),
        "",
        "不是 ASMR 条目"
    );
}

/// L6：列表里的 ASMR 文件卡片在标题下多一行摘要，不看当前选中的是哪个文件；ASMR 资源库插件停用时不画。
#[test]
fn asmr_cards_carry_the_summary_line_while_the_plugin_is_on() {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    let metadata = audio(
        None,
        &[
            ("rjCode", json!("RJ01234567")),
            ("listeningStatus", json!("listening")),
        ],
    );
    model.files.rows = vec![row("01.mp3", &metadata)];
    // 列表是虚拟的：排一次版以后才按视口建出卡片。
    let labels = |model: &ShellViewModel| {
        let mut harness = crate::shell::view_harness::ShellHarness::mount(model.clone());
        harness.frame();
        harness
            .nodes()
            .into_iter()
            .filter_map(|node| node.label.map(|label| label.to_string()))
            .collect::<Vec<_>>()
    };
    let shown = labels(&model);
    assert!(
        shown.iter().any(|label| label == "01"),
        "卡片建出来了：{shown:?}"
    );
    assert!(
        shown.iter().any(|label| label == "RJ01234567 · 收听中"),
        "{shown:?}"
    );
    for plugin in model
        .admin
        .plugins
        .iter_mut()
        .filter(|plugin| plugin.plugin_id == "momobako.library.asmr")
    {
        plugin.enabled = false;
    }
    assert!(
        !labels(&model)
            .iter()
            .any(|label| label.contains("RJ01234567")),
        "插件停用后不画摘要"
    );
}

/// L7：作品队列按音轨路径的自然序排，`2.mp3` 在 `10.mp3` 前面。
#[test]
fn work_queue_sorts_tracks_naturally() {
    let files = [
        ("work/1.mp3", audio(Some("work"), &[])),
        ("work/10.mp3", audio(Some("work"), &[])),
        ("work/2.mp3", audio(Some("work"), &[])),
    ];
    let model = scene(&files, "work/1.mp3");
    let panels = super::preview_panels(&model).expect("有作品队列");
    let (count, items) = panels.queue.expect("作品队列");
    assert_eq!(count, "3 轨");
    assert_eq!(
        items
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>(),
        ["work/2.mp3", "work/10.mp3"]
    );
}

/// 作品队列行是左标题、右状态，不是「标题 状态」一整句。状态是 `listeningStatus` 原文，
/// 没有这个字段时是 Vue 的占位 `unlistened`。正在预览的音轨不在队列里，
/// 行底保持透明（Vue 作品队列不加 `is-active`）；没有别的音轨时不编造作品。
#[test]
fn work_queue_splits_the_title_from_the_status() {
    let files = [
        (
            "work/01.mp3",
            audio(
                Some("work"),
                &[
                    ("trackTitle", json!("序章")),
                    ("listeningStatus", json!("listening")),
                ],
            ),
        ),
        (
            "work/02.mp3",
            audio(Some("work"), &[("trackTitle", json!("第二轨"))]),
        ),
    ];
    let model = scene(&files, "work/01.mp3");
    let panels = super::preview_panels(&model).expect("有作品队列");
    let (count, items) = panels.queue.expect("作品队列");
    assert_eq!(count, "2 轨");
    assert_eq!(items.len(), 1, "正在预览的序章不进作品队列");
    assert_eq!(items[0].path, "work/02.mp3");
    assert_eq!(items[0].title, "第二轨");
    assert_eq!(
        items[0].status, "unlistened",
        "没有 listeningStatus 时用 Vue 的占位，不编中文"
    );

    let other = scene(&files, "work/02.mp3");
    let (_, siblings) = super::preview_panels(&other)
        .expect("有作品队列")
        .queue
        .expect("作品队列");
    assert_eq!(siblings[0].title, "序章");
    assert_eq!(
        siblings[0].status, "listening",
        "有字段时原样显示，不翻成「收听中」"
    );

    let alone = scene(
        &[("solo.mp3", audio(None, &[("trackTitle", json!("独奏"))]))],
        "solo.mp3",
    );
    assert!(
        super::preview_panels(&alone).is_none_or(|panels| panels.queue.is_none()),
        "没有别的音轨时不编造作品队列"
    );

    let mut harness = crate::shell::view_harness::ShellHarness::mount(model);
    harness.frame();
    let labels = harness
        .nodes()
        .into_iter()
        .filter_map(|node| node.label.map(|label| label.to_string()))
        .collect::<Vec<_>>();
    assert!(labels.iter().any(|label| label == "第二轨"), "{labels:?}");
    assert!(
        labels.iter().any(|label| label == "unlistened"),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label.contains("第二轨 ")),
        "标题和状态还粘在一句里：{labels:?}"
    );

    let row_key = "inspect-asmr-queue-work\u{2215}02.mp3";
    let row = harness.keyed(row_key).expect("作品队列行");
    let row_style = harness
        .document()
        .context()
        .world()
        .node_style(row)
        .expect("行样式");
    assert_eq!(
        row_style.background, None,
        "作品队列没有 is-active，不铺 accent-soft"
    );
    assert_eq!(
        row_style.interaction.hovered.background,
        Some(nana_ui::runtime::SemanticColorRole::Hover)
    );

    let status = harness
        .keyed(&format!("{row_key}-status"))
        .expect("右侧状态");
    let status_style = harness
        .document()
        .context()
        .world()
        .node_style(status)
        .expect("状态样式");
    assert_eq!(status_style.layout.font_size, Some(11.0));
    assert_eq!(
        status_style.foreground,
        Some(nana_ui::runtime::SemanticColorRole::Muted)
    );
}

/// 作品队列跟 Vue `workAudioEntries(previewFileEntry, activeFileEntries)`：
/// 当前文件在列表里就用这一行的元数据，不在列表里再用播放集条目。
/// 详情叠层和还没保存的草稿不改分组、标题、状态。没有作品目录时不把别的音轨编进来。
/// 列表行没有的标题不编，收听状态仍用 Vue 的占位 `unlistened`。
#[test]
fn work_queue_follows_the_preview_list_not_the_draft() {
    let files = [
        (
            "work/01.mp3",
            audio(Some("work-a"), &[("trackTitle", json!("当前轨"))]),
        ),
        (
            "work/02.mp3",
            audio(
                Some("work-a"),
                &[
                    ("trackTitle", json!("列表第二轨")),
                    ("listeningStatus", json!("listening")),
                ],
            ),
        ),
        ("work/04.mp3", audio(Some("work-a"), &[])),
        (
            "work/03.mp3",
            audio(Some("work-b"), &[("trackTitle", json!("另一部"))]),
        ),
    ];
    let listed = [
        ("work/02.mp3", "列表第二轨", "listening"),
        ("work/04.mp3", "04.mp3", "unlistened"),
    ];
    let mut model = scene(&files, "work/01.mp3");
    assert_queue(&model, "3 轨", &listed);

    let overlaid = audio(
        Some("work-b"),
        &[
            ("trackTitle", json!("详情音轨")),
            ("series", json!("不该出现")),
        ],
    );
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        "work/01.mp3",
        &overlaid,
        2,
    ))));
    assert!(
        super::super::files_view::previewing(&model),
        "详情到达后仍在预览页"
    );
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("workRoot")
            .and_then(Value::as_str),
        Some("work-b"),
        "草稿已换成详情里的作品目录"
    );
    assert_queue(&model, "3 轨", &listed);

    let patched = model.inspect.apply_metadata_patch(
        &asset_id("work/01.mp3"),
        BTreeMap::from([
            ("workRoot".into(), json!("work-b")),
            ("trackTitle".into(), json!("草稿轨")),
            ("listeningStatus".into(), json!("drafted")),
        ]),
    );
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("trackTitle")
            .and_then(Value::as_str),
        Some("草稿轨")
    );
    assert_queue(&model, "3 轨", &listed);

    let bare = [
        ("solo.mp3", audio(None, &[("trackTitle", json!("独奏"))])),
        (
            "work/02.mp3",
            audio(Some("work-a"), &[("trackTitle", json!("不该进来"))]),
        ),
    ];
    let mut alone = scene(&bare, "solo.mp3");
    assert!(queue_of(&alone).is_none(), "列表没有作品目录时不编造队列");
    let forced = audio(Some("work-a"), &[("trackTitle", json!("详情目录"))]);
    alone.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        "solo.mp3", &forced, 2,
    ))));
    assert!(
        queue_of(&alone).is_none(),
        "详情里的作品目录不把别的音轨编进来"
    );
    let patched = alone.inspect.apply_metadata_patch(
        &asset_id("solo.mp3"),
        BTreeMap::from([("workRoot".into(), json!("work-a"))]),
    );
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert!(
        queue_of(&alone).is_none(),
        "草稿里的作品目录不把别的音轨编进来"
    );

    let mut playlist = ShellViewModel::for_page(ShellPage::FileList);
    playlist.files.rows = vec![
        row(
            "album/02.mp3",
            &audio(
                Some("album"),
                &[
                    ("trackTitle", json!("列表轨")),
                    ("listeningStatus", json!("listening")),
                ],
            ),
        ),
        row(
            "album/03.mp3",
            &audio(Some("other"), &[("trackTitle", json!("另一部"))]),
        ),
    ];
    playlist.player.listed = Some(playlist_page(
        "album/01.mp3",
        &audio(Some("album"), &[("trackTitle", json!("播放集轨"))]),
    ));
    playlist.reduce(ShellMessage::Player(PlayerMessage::OpenPreview {
        item_id: Some("item".into()),
    }));
    assert!(
        super::super::files_view::previewing(&playlist),
        "播放集打开预览页"
    );
    assert!(
        playlist.inspect.current_asset_id().is_none(),
        "详情还没回来，队列不靠草稿"
    );
    assert_queue(
        &playlist,
        "2 轨",
        &[("album/02.mp3", "列表轨", "listening")],
    );

    let foreign = audio(Some("other"), &[("trackTitle", json!("详情音轨"))]);
    playlist.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        "album/01.mp3",
        &foreign,
        2,
    ))));
    assert_eq!(
        playlist
            .inspect
            .draft_custom()
            .get("workRoot")
            .and_then(Value::as_str),
        Some("other")
    );
    assert_queue(
        &playlist,
        "2 轨",
        &[("album/02.mp3", "列表轨", "listening")],
    );
    let patched = playlist.inspect.apply_metadata_patch(
        &asset_id("album/01.mp3"),
        BTreeMap::from([
            ("workRoot".into(), json!("other")),
            ("trackTitle".into(), json!("草稿轨")),
        ]),
    );
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert_queue(
        &playlist,
        "2 轨",
        &[("album/02.mp3", "列表轨", "listening")],
    );
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

/// 作品信息的「保存封面」和候选上的「封面」按已有的 http(s) 地址下载，写成自定义缩略图。
/// 别的网址、回收站和目录不下载；没写出文件时不编造封面。
#[test]
fn asmr_save_cover_downloads_the_https_cover_and_writes_the_thumbnail() {
    let ftp = scene(
        &[(
            "voice.mp3",
            audio(None, &[("coverUrl", json!("ftp://cdn.example/a.jpg"))]),
        )],
        "voice.mp3",
    );
    assert!(works_cover(&ftp).is_none(), "ftp 不是封面地址");
    let blank = scene(
        &[("voice.mp3", audio(None, &[("cover", json!("  "))]))],
        "voice.mp3",
    );
    assert!(works_cover(&blank).is_none(), "空白封面不画按钮");

    let cover = "HTTP://cdn.example/work.jpg";
    let candidate_cover = "https://cdn.example/candidate.jpg";
    let files = [(
        "voice.mp3",
        audio(
            None,
            &[
                ("cover", json!(cover)),
                (
                    "providerCandidates",
                    json!([{ "source": "dlsite", "fields": { "circle": "社团", "coverUrl": candidate_cover } }]),
                ),
            ],
        ),
    )];
    let mut model = scene(&files, "voice.mp3");
    let offer = works_cover(&model).expect("cover 字段的 http 地址");
    assert_eq!(offer.url, cover);
    assert_eq!(offer.label, "保存封面");
    assert!(!offer.disabled);

    let mut directory = scene(&files, "voice.mp3");
    directory.files.rows[0].kind = "directory".into();
    assert!(works_cover(&directory).expect("目录仍画出按钮").disabled);
    directory.reduce(ShellMessage::Asmr(AsmrMessage::SaveCover(cover.into())));
    assert!(cover_saves(&mut directory).is_empty(), "目录不能保存封面");

    model.reduce(ShellMessage::Asmr(AsmrMessage::SaveCover(
        "https://evil.example/a.jpg".into(),
    )));
    assert!(
        cover_saves(&mut model).is_empty(),
        "不是这个文件或候选的地址不下载"
    );
    model.files.trash = true;
    model.reduce(ShellMessage::Asmr(AsmrMessage::SaveCover(cover.into())));
    assert!(cover_saves(&mut model).is_empty(), "回收站不保存封面");
    model.files.trash = false;

    let repo = model.workspace.active_repo_id.clone().expect("活动仓库");
    model.reduce(ShellMessage::Files(FilesMessage::ThumbnailEnsured {
        repo_id: repo.clone(),
        path: "voice.mp3".into(),
        result: Ok(EnsuredThumbnail {
            thumbnail_path: Some("thumbs/old.jpg".into()),
            custom: false,
            metadata: None,
        }),
    }));
    let _ = model.files.take_effects();

    let mut harness = ShellHarness::mount(model);
    click_revealed(&mut harness, "inspect-asmr-save-cover");
    assert_eq!(
        cover_saves(&mut harness.model),
        vec![(repo.clone(), "voice.mp3".into(), cover.into())]
    );
    assert!(harness.find("保存中").is_some(), "保存期间按钮写保存中");
    assert!(harness.find("保存封面").is_none());
    click_revealed(&mut harness, "inspect-asmr-save-cover");
    if harness.keyed("inspect-asmr-cover-0").is_some() {
        click_revealed(&mut harness, "inspect-asmr-cover-0");
    }
    assert!(
        cover_saves(&mut harness.model).is_empty(),
        "进行中不重复提交"
    );

    harness.apply(ShellMessage::Asmr(AsmrMessage::CoverSaved {
        repo_id: repo.clone(),
        path: "other.mp3".into(),
        result: Ok(EnsuredThumbnail {
            thumbnail_path: Some("thumbs/nope.jpg".into()),
            custom: true,
            metadata: None,
        }),
    }));
    assert_eq!(
        harness.model.files.rows[0].thumbnail_path.as_deref(),
        Some("thumbs/old.jpg")
    );
    harness.flush();
    assert!(
        harness.find("保存中").is_some(),
        "别的路径的结果不结束这次保存"
    );

    harness.apply(ShellMessage::Asmr(AsmrMessage::CoverSaved {
        repo_id: repo.clone(),
        path: "voice.mp3".into(),
        result: Ok(EnsuredThumbnail {
            thumbnail_path: None,
            custom: true,
            metadata: None,
        }),
    }));
    assert_eq!(
        harness.model.files.rows[0].thumbnail_path.as_deref(),
        Some("thumbs/old.jpg"),
        "没有路径就不编造封面"
    );
    harness.flush();
    assert!(harness.find("保存封面").is_some());

    click_revealed(&mut harness, "inspect-asmr-save-cover");
    assert_eq!(cover_saves(&mut harness.model).len(), 1);
    harness.apply(ShellMessage::Asmr(AsmrMessage::CoverSaved {
        repo_id: repo.clone(),
        path: "voice.mp3".into(),
        result: Ok(EnsuredThumbnail {
            thumbnail_path: Some("thumbs/cover.jpg".into()),
            custom: true,
            metadata: None,
        }),
    }));
    assert_eq!(
        harness.model.files.rows[0].thumbnail_path.as_deref(),
        Some("thumbs/cover.jpg")
    );
    assert!(harness.model.files.rows[0].thumbnail_custom);
    assert!(
        matches!(harness.model.files.take_effects().as_slice(), [FilesEffect::DecodeThumbnails { paths }] if paths == &["thumbs/cover.jpg".to_string()])
    );
    harness.flush();
    assert!(harness.find("保存封面").is_some());

    click_revealed(&mut harness, "inspect-asmr-cover-0");
    assert_eq!(
        cover_saves(&mut harness.model),
        vec![(repo.clone(), "voice.mp3".into(), candidate_cover.into())]
    );
    harness.apply(ShellMessage::Asmr(AsmrMessage::CoverSaved {
        repo_id: repo,
        path: "voice.mp3".into(),
        result: Err("拒绝访问".into()),
    }));
    assert_eq!(harness.model.files.error, "拒绝访问");
    assert_eq!(
        harness.model.files.rows[0].thumbnail_path.as_deref(),
        Some("thumbs/cover.jpg")
    );
    harness.flush();
    assert!(harness.find("保存封面").is_some());
}

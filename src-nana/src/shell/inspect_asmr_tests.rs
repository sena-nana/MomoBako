//! ASMR 资源库扩展的回归测试：候选按原始类型写回、导入的候选落在自己的文件上、抓取经插件调用、
//! 作品队列点了打开预览、作品信息的数组和插件开关、音轨的自然序。
//!
//! 场景照验收底子（`for_page`，内置插件全部启用），根目录换成给定的 ASMR 文件，再像双击一样读回当前文件的详情。

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::backend::services::repository::{AssetDetail, AssetSummary, MetadataEntry};
use crate::shell::admin::AdminEffect;
use crate::shell::files::{FileRow, FilesEffect};
use crate::shell::{InspectEffect, InspectMessage, ShellMessage, ShellPage, ShellViewModel};

use super::AsmrMessage;

const REPO: &str = "acceptance-repo";

type Meta = BTreeMap<String, Value>;

/// ASMR 音频的元数据：库类型、条目类型，作品目录（有时），再加 `extra`。
pub(super) fn audio(work_root: Option<&str>, extra: &[(&str, Value)]) -> Meta {
    let mut metadata = Meta::from([("libraryKind".to_string(), json!("asmr")), ("asmrEntryKind".to_string(), json!("audio"))]);
    if let Some(root) = work_root {
        metadata.insert("workRoot".into(), json!(root));
    }
    metadata.extend(extra.iter().map(|(key, value)| (key.to_string(), value.clone())));
    metadata
}

fn asset_id(path: &str) -> String {
    path.replace('/', "-")
}

fn row(path: &str, metadata: &Meta) -> FileRow {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    FileRow {
        path: path.into(),
        extension: name.rsplit_once('.').map(|(_, extension)| extension.to_string()),
        name,
        kind: "file".into(),
        asset_id: Some(asset_id(path)),
        is_virtual: false,
        thumbnail_path: None,
        hardlink_state: None,
        pixel_width: 0,
        pixel_height: 0,
        texture_ready: false,
        palette: Vec::new(),
        size_label: String::new(),
        modified_at: String::new(),
        tags: Vec::new(),
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
            extension: filename.rsplit_once('.').map(|(_, extension)| extension.to_string()).unwrap_or_default(),
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
            .map(|(key, value)| MetadataEntry { key: key.clone(), value_type: "json".into(), value: value.clone(), version, updated_at: String::new() })
            .collect(),
        revisions: Vec::new(),
    }
}

/// 根目录换成 `files` 的验收模型，读回 `current` 的详情，停在它的预览上。
pub(super) fn scene(files: &[(&str, Meta)], current: &str) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
    model.files.rows = files.iter().map(|(path, metadata)| row(path, metadata)).collect();
    open(&mut model, files, current);
    model
}

/// 像双击一样选中 `path` 并读回它的详情。
pub(super) fn open(model: &mut ShellViewModel, files: &[(&str, Meta)], path: &str) {
    let metadata = files.iter().find(|(item, _)| *item == path).map(|(_, metadata)| metadata.clone()).expect("场景里的文件");
    model.reduce(ShellMessage::SelectFile { path: path.into(), asset_id: Some(asset_id(path)) });
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(path, &metadata, 1))));
    model.inspect.take_effects();
    model.files.take_effects();
    model.admin.take_effects();
}

/// 这次归约排下的元数据保存请求里的元数据。
pub(super) fn saved(model: &mut ShellViewModel) -> Option<BTreeMap<String, Value>> {
    model.inspect.take_effects().into_iter().find_map(|effect| match effect {
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
    let files = [("voice.mp3", audio(None, &[("providerCandidates", json!([dlsite_candidate()]))]))];
    let mut model = scene(&files, "voice.mp3");
    asmr(&mut model, AsmrMessage::Apply(0));
    model.reduce(ShellMessage::Inspect(InspectMessage::SaveMetadata));
    let metadata = saved(&mut model).expect("应用候选以后保存");
    assert_eq!(metadata.get("voiceActors"), Some(&json!(["甲", "乙"])));
    assert_eq!(metadata.get("price"), Some(&json!(1320)));
    assert_eq!(metadata.get("rateAverage"), Some(&json!(4.5)));
    assert_eq!(metadata.get("nsfw"), Some(&json!(false)));
    assert_eq!(metadata.get("workId"), Some(&json!("RJ01234567")));
    assert_eq!(metadata.get("sourceUrl"), Some(&json!("https://www.dlsite.com/maniax/work/=/product_id/RJ01234567.html")));
    assert!(!metadata.contains_key("comment") && !metadata.contains_key("listeningStatus"), "受保护的字段不写：{metadata:?}");
}

/// L1：导入的候选追加进当前文件的 `providerCandidates` 并保存；换到别的文件以后它不出现，
/// 在别的文件上也就应用不到它。
#[test]
fn imported_candidates_are_saved_on_their_own_file_and_do_not_follow_other_files() {
    let files = [("a.mp3", audio(None, &[("providerCandidates", json!([dlsite_candidate()]))])), ("b.mp3", audio(None, &[]))];
    let mut model = scene(&files, "a.mp3");
    asmr(&mut model, AsmrMessage::ToggleImport);
    asmr(&mut model, AsmrMessage::SetImportDraft(r#"{"circle":"手填的社团"}"#.into()));
    asmr(&mut model, AsmrMessage::ImportCandidate);
    model.reduce(ShellMessage::Inspect(InspectMessage::SaveMetadata));
    let metadata = saved(&mut model).expect("导入候选以后保存当前文件");
    assert_eq!(
        metadata.get("providerCandidates"),
        Some(&json!([dlsite_candidate(), { "source": "manual", "confidence": "manual", "fields": { "circle": "手填的社团" } }])),
        "追加在已有候选后面，已有的原样保留"
    );

    open(&mut model, &files, "b.mp3");
    let custom = super::display_custom(&model);
    let view = super::candidates_view(&model, &custom);
    assert!(view.shown, "b.mp3 也是 ASMR 条目");
    assert!(view.candidates.is_empty(), "a.mp3 导入的候选不跟到 b.mp3：{:?}", view.candidates);
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
            AdminEffect::CallPlugin { plugin_id, method, payload, repository_id, .. } => Some((plugin_id, method, payload, repository_id)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        calls,
        [("momobako.service.provider.dlsite".to_string(), "provider.lookupMetadataCandidate".to_string(), json!({ "id": "RJ01234567" }), None)]
    );
}

/// L3：作品队列里点一条，像双击一样打开它的预览：选中它、读它的素材详情；详情回来以后预览换成它。
#[test]
fn clicking_a_queue_track_opens_its_preview() {
    let files = [
        ("work/01.mp3", audio(Some("work"), &[("trackTitle", json!("序章"))])),
        ("work/02.mp3", audio(Some("work"), &[("trackTitle", json!("第二轨"))])),
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
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail("work/02.mp3", &files[1].1, 1))));
    assert_eq!(model.page, ShellPage::SelectedFile);
    assert!(super::super::files_view::previewing(&model), "停在预览页，换成第二轨");
    assert_eq!(model.inspect.current_asset_id(), Some("work-02.mp3"));
}

/// L3：换文件以后、新文件的详情回来之前，作品信息不再叠着上一个文件的草稿值。
#[test]
fn the_previous_file_draft_does_not_leak_into_the_next_file() {
    let files = [("a.mp3", audio(None, &[("workTitle", json!("上一部作品"))])), ("b.mp3", audio(None, &[]))];
    let mut model = scene(&files, "a.mp3");
    model.reduce(ShellMessage::SelectFile { path: "b.mp3".into(), asset_id: Some("b.mp3".into()) });
    let custom = super::display_custom(&model);
    assert!(!custom.contains_key("workTitle"), "详情回来之前只看 b.mp3 的列表行：{custom:?}");
}

/// L7：作品信息里的数组连成「甲，乙」，不显示成 JSON。
#[test]
fn work_info_lists_arrays_as_text() {
    let files = [("voice.mp3", audio(None, &[("voiceActors", json!(["甲", "乙"])), ("scenarioTags", json!(["治愈"]))]))];
    let model = scene(&files, "voice.mp3");
    let sections = super::super::inspect_library::library_sections(&super::display_custom(&model));
    let rows = sections
        .iter()
        .flat_map(|section| &section.rows)
        .filter_map(|row| match row {
            super::super::inspect_library::LibraryRow::Value { label, value, .. } => Some((label.to_string(), value.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(rows.contains(&("声优".to_string(), "甲，乙".to_string())), "{rows:?}");
    assert!(rows.contains(&("标签".to_string(), "治愈".to_string())), "{rows:?}");
}

/// L7：停用 ASMR 资源库插件以后，补全候选、作品队列和播放列表都不出现（Vue 里扩展跟着插件卸载）。
#[test]
fn extension_panels_need_the_asmr_plugin() {
    let files = [("work/01.mp3", audio(Some("work"), &[])), ("work/02.mp3", audio(Some("work"), &[]))];
    let mut model = scene(&files, "work/01.mp3");
    assert!(super::preview_panels(&model).is_some());
    assert!(super::candidates_view(&model, &super::display_custom(&model)).shown);
    for plugin in model.admin.plugins.iter_mut().filter(|plugin| plugin.plugin_id == "momobako.library.asmr") {
        plugin.enabled = false;
    }
    assert!(super::preview_panels(&model).is_none(), "作品队列和播放列表跟着插件停用");
    assert!(!super::candidates_view(&model, &super::display_custom(&model)).shown, "补全候选跟着插件停用");
}

/// L6：文件列表上的 ASMR 摘要（Vue `fileSummary`）：RJ 号、作品名、收听状态、进度和「歌词」用「 · 」连起来；
/// 后端种下的标题就是文件名，不当作品名；不是 ASMR 条目没有。
#[test]
fn file_summary_joins_the_work_fields() {
    let summary = |name: &str, metadata: Meta| super::super::inspect_library::file_summary(&row(name, &metadata));
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
    assert_eq!(summary("01.mp3", full), "RJ01234567 · 夜曲 · 收听中 · 40% · 歌词");
    assert_eq!(summary("01.mp3", audio(None, &[("title", json!("01.mp3"))])), "", "标题就是文件名时不算作品名");
    assert_eq!(summary("01.mp3", audio(None, &[("workId", json!("RJ1")), ("title", json!("夜曲 01"))])), "RJ1 · 夜曲 01");
    assert_eq!(summary("01.mp3", Meta::from([("workTitle".to_string(), json!("夜曲"))])), "", "不是 ASMR 条目");
}

/// L6：列表里的 ASMR 文件卡片在标题下多一行摘要，不看当前选中的是哪个文件；ASMR 资源库插件停用时不画。
#[test]
fn asmr_cards_carry_the_summary_line_while_the_plugin_is_on() {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    let metadata = audio(None, &[("rjCode", json!("RJ01234567")), ("listeningStatus", json!("listening"))]);
    model.files.rows = vec![row("01.mp3", &metadata)];
    // 列表是虚拟的：排一次版以后才按视口建出卡片。
    let labels = |model: &ShellViewModel| {
        let mut harness = crate::shell::view_harness::ShellHarness::mount(model.clone());
        harness.frame();
        harness.nodes().into_iter().filter_map(|node| node.label.map(|label| label.to_string())).collect::<Vec<_>>()
    };
    let shown = labels(&model);
    assert!(shown.iter().any(|label| label == "01"), "卡片建出来了：{shown:?}");
    assert!(shown.iter().any(|label| label == "RJ01234567 · 收听中"), "{shown:?}");
    for plugin in model.admin.plugins.iter_mut().filter(|plugin| plugin.plugin_id == "momobako.library.asmr") {
        plugin.enabled = false;
    }
    assert!(!labels(&model).iter().any(|label| label.contains("RJ01234567")), "插件停用后不画摘要");
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
    assert_eq!(items.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(), ["work/2.mp3", "work/10.mp3"]);
}

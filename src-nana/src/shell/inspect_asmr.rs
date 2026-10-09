//! ASMR 预览队列和补全候选。
//!
//! 作品队列、播放列表和候选都来自已经在元数据或当前目录里的条目。
//! 没有歌词正文、封面像素、时间戳或任务数时不补。

use std::collections::BTreeMap;

use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, LengthSpec, RadiusTier, Select, SelectChanged, SelectOption, SemanticColorRole, SettingsCard, Stack,
    Text, TextChanged, TextInput,
};
use nana_ui::{ButtonKind, ControlSize};
use serde_json::Value;

use super::inspect::InspectMessage;
use super::{ShellMessage, ShellViewModel};

const DLSITE: &str = "momobako.service.provider.dlsite";
const ASMR_ONE: &str = "momobako.service.provider.asmr-one";

const PROTECTED: &[&str] = &[
    "comment",
    "rating",
    "listeningProgress",
    "listeningStatus",
    "lastListenedAt",
    "trackPositionMs",
    "trackDurationMs",
];

const ALLOWED: &[&str] = &[
    "workId",
    "rjCode",
    "title",
    "workTitle",
    "circle",
    "creator",
    "voiceActors",
    "characters",
    "series",
    "scenarioTags",
    "audioTraits",
    "nsfw",
    "ageRating",
    "language",
    "releaseDate",
    "price",
    "dlCount",
    "sales",
    "reviewCount",
    "rateCount",
    "rateAverage",
    "rateCountDetail",
    "rank",
    "cover",
    "coverUrl",
    "coverSourceWorkId",
    "sourceUrl",
    "purchaseSource",
];

/// 导入框、内存播放列表和已经导入的候选。不写进仓库，避免把空表当成真实任务。
#[derive(Clone, Debug, Default)]
pub struct AsmrUi {
    import_open: bool,
    import_draft: String,
    import_error: String,
    lookup_provider: String,
    lookup_id: String,
    playlist: Vec<PlaylistItem>,
    imported: Vec<Candidate>,
}

#[derive(Clone, Debug)]
pub enum AsmrMessage {
    ToggleImport,
    SetProvider(String),
    SetLookupId(String),
    SetImportDraft(String),
    ImportCandidate,
    Lookup,
    AddWork,
    AddRandom,
    ClearPlaylist,
    Open(String),
    Apply(usize),
}

#[derive(Clone, Debug)]
struct PlaylistItem {
    repo_id: String,
    path: String,
    title: String,
    work_title: String,
    status: String,
}

#[derive(Clone, Debug)]
struct Candidate {
    source: String,
    confidence: String,
    fields: Vec<(String, String)>,
    skipped: Vec<String>,
}

pub(super) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    let ShellMessage::Asmr(message) = message else {
        return Some(message);
    };
    match message {
        AsmrMessage::ToggleImport => toggle_import(model),
        AsmrMessage::SetProvider(value) => model.asmr.lookup_provider = value,
        AsmrMessage::SetLookupId(value) => model.asmr.lookup_id = value,
        AsmrMessage::SetImportDraft(value) => model.asmr.import_draft = value,
        AsmrMessage::ImportCandidate => import_candidate(model),
        AsmrMessage::Lookup => lookup(model),
        AsmrMessage::AddWork => add_work(model),
        AsmrMessage::AddRandom => add_random(model),
        AsmrMessage::ClearPlaylist => {
            let repo = repo_id(model);
            model.asmr.playlist.retain(|item| item.repo_id != repo);
        }
        AsmrMessage::Open(path) => open_path(model, &path),
        AsmrMessage::Apply(index) => apply_candidate(model, index),
    }
    None
}

/// 草稿优先，文件行上的文本和数字补上还没编辑的键。数组留给候选解析。
pub(super) fn display_custom(model: &ShellViewModel) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Some(row) = current_row(model) {
        for (key, value) in &row.metadata {
            if let Some(text) = scalar_text(value) {
                map.insert(key.clone(), text);
            }
        }
    }
    for (key, value) in model.inspect.draft_custom() {
        let value = value.trim();
        if !value.is_empty() {
            map.insert(key.clone(), value.to_string());
        }
    }
    map
}

/// 预览列下方的作品队列和播放列表。不是 ASMR，或两边都空，就不占位。
pub(super) fn preview_panels(model: &ShellViewModel) -> Option<AnyView> {
    let custom = display_custom(model);
    if !super::inspect_library::matches_asmr(&custom) {
        return None;
    }
    let mut rows = Vec::new();
    if let Some(queue) = work_queue(model, &custom) {
        rows.push(queue);
    }
    if let Some(playlist) = playlist_card(model, &custom) {
        rows.push(playlist);
    }
    if rows.is_empty() {
        return None;
    }
    Some(widget(Stack::column(12.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).key("inspect-asmr-preview").children(rows).into_any())
}

/// 元数据列里的补全候选。没有候选就写「暂无候选」，不造一条来源。
pub(super) fn candidate_section(model: &ShellViewModel, custom: &BTreeMap<String, String>) -> Vec<AnyView> {
    if !super::inspect_library::matches_asmr(custom) {
        return Vec::new();
    }
    let locked = !asmr_can_edit(model);
    let mut rows = vec![
        text("ASMR Provider").key("inspect-asmr-provider-eyebrow").into_any(),
        text("补全候选").key("inspect-asmr-provider-title").into_any(),
        widget(Button::new("导入").kind(ButtonKind::Ghost).disabled(locked))
            .key("inspect-asmr-import")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::ToggleImport)))
            .into_any(),
    ];
    if model.asmr.import_open {
        rows.push(import_form(model, custom, locked));
    }
    let candidates = listed_candidates(model);
    if candidates.is_empty() {
        if !model.asmr.import_open {
            rows.push(text("暂无候选").key("inspect-asmr-candidate-empty").into_any());
        }
    } else {
        for (index, candidate) in candidates.into_iter().enumerate() {
            rows.push(candidate_card(candidate, index, locked));
        }
    }
    vec![widget(SettingsCard::new("补全候选")).key("inspect-asmr-candidates").children(rows).into_any()]
}

fn import_form(model: &ShellViewModel, custom: &BTreeMap<String, String>, locked: bool) -> AnyView {
    let provider = if model.asmr.lookup_provider.is_empty() { DLSITE.to_string() } else { model.asmr.lookup_provider.clone() };
    let lookup_id = if model.asmr.lookup_id.is_empty() {
        custom.get("workId").or_else(|| custom.get("rjCode")).cloned().unwrap_or_default()
    } else {
        model.asmr.lookup_id.clone()
    };
    let options = vec![SelectOption::new(DLSITE, "DLsite"), SelectOption::new(ASMR_ONE, "ASMR One")];
    let select = Select::new(Some(provider)).options(options).placeholder("ASMR Provider").size(ControlSize::Small).disabled(locked);
    let mut body = vec![
        widget(select).key("inspect-asmr-provider").on_cx(|_, event: &SelectChanged, cx| {
            cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::SetProvider(event.value.to_string())));
        }).into_any(),
        widget(TextInput::new(lookup_id).placeholder("RJ123456").size(ControlSize::Small).disabled(locked))
            .key("inspect-asmr-work-id")
            .on_cx(|_, event: &TextChanged, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::SetLookupId(event.value.to_string()))))
            .into_any(),
        widget(Button::new("抓取候选").kind(ButtonKind::Ghost).disabled(locked)).key("inspect-asmr-lookup").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::Lookup));
        }).into_any(),
        widget(TextInput::new(model.asmr.import_draft.clone()).placeholder("ASMR 候选 JSON").disabled(locked))
            .key("inspect-asmr-import-json")
            .on_cx(|_, event: &TextChanged, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::SetImportDraft(event.value.to_string()))))
            .into_any(),
    ];
    if !model.asmr.import_error.is_empty() {
        body.push(text(model.asmr.import_error.clone()).key("inspect-asmr-import-error").into_any());
    }
    body.push(
        widget(Button::new("导入候选").kind(ButtonKind::Ghost).disabled(locked)).key("inspect-asmr-import-submit").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::ImportCandidate));
        }).into_any(),
    );
    widget(Stack::column(8.0)).key("inspect-asmr-import-form").children(body).into_any()
}

fn candidate_card(candidate: Candidate, index: usize, locked: bool) -> AnyView {
    let title = if candidate.confidence.is_empty() {
        candidate.source.clone()
    } else {
        format!("{} {}", candidate.source, candidate.confidence)
    };
    let mut rows = vec![text(title).key(format!("inspect-asmr-candidate-{index}")).into_any()];
    let apply_index = index;
    rows.push(
        widget(Button::new("应用").kind(ButtonKind::Ghost).disabled(locked || candidate.fields.is_empty()))
            .key(format!("inspect-asmr-apply-{index}"))
            .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::Apply(apply_index))))
            .into_any(),
    );
    for (key, value) in &candidate.fields {
        rows.push(text(format!("{key}={value}")).key(format!("inspect-asmr-field-{index}-{key}")).into_any());
    }
    if !candidate.skipped.is_empty() {
        rows.push(text(format!("跳过 {}", candidate.skipped.join("，"))).key(format!("inspect-asmr-skipped-{index}")).into_any());
    }
    widget(Stack::column(4.0)).key(format!("inspect-asmr-candidate-card-{index}")).children(rows).into_any()
}

fn work_queue(model: &ShellViewModel, custom: &BTreeMap<String, String>) -> Option<AnyView> {
    let current = model.inspect.target_path.as_deref().unwrap_or("");
    let siblings = work_audio(model, custom).into_iter().filter(|row| row.path != current).collect::<Vec<_>>();
    if siblings.is_empty() {
        return None;
    }
    let count = siblings.len() + 1;
    let mut rows = vec![queue_head("作品队列", &format!("{count} 轨"), "inspect-asmr-queue-head")];
    for row in siblings {
        let path = row.path.clone();
        let title = meta_text(row, custom, "trackTitle").unwrap_or_else(|| row.name.clone());
        let status = meta_text(row, custom, "listeningStatus").unwrap_or_else(|| "unlistened".into());
        rows.push(
            widget(Button::new(format!("{title} {status}")).kind(ButtonKind::Ghost))
                .key(format!("inspect-asmr-queue-{path}"))
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::Open(path.clone()))))
                .into_any(),
        );
    }
    Some(queue_card("inspect-asmr-queue", rows))
}

fn playlist_card(model: &ShellViewModel, custom: &BTreeMap<String, String>) -> Option<AnyView> {
    let audio = current_row(model).is_some_and(|row| is_audio(row, custom));
    let repo = repo_id(model);
    let items = model.asmr.playlist.iter().filter(|item| item.repo_id == repo).cloned().collect::<Vec<_>>();
    if !audio && items.is_empty() {
        return None;
    }
    let mut rows = vec![queue_head("播放列表", &format!("{} 项", items.len()), "inspect-asmr-playlist-head")];
    rows.push(
        widget(Stack::row(8.0).align(AlignSpec::Center))
            .key("inspect-asmr-playlist-actions")
            .children((
                widget(Button::new("加入作品").kind(ButtonKind::Ghost).disabled(!audio)).key("inspect-asmr-add-work").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::AddWork));
                }),
                widget(Button::new("随机").kind(ButtonKind::Ghost).disabled(!audio)).key("inspect-asmr-random").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::AddRandom));
                }),
                widget(Button::new("清空").kind(ButtonKind::Ghost).disabled(items.is_empty())).key("inspect-asmr-clear").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::ClearPlaylist));
                }),
            ))
            .into_any(),
    );
    if items.is_empty() {
        rows.push(text("暂无播放队列").key("inspect-asmr-playlist-empty").into_any());
    } else {
        let current = model.inspect.target_path.clone().unwrap_or_default();
        for item in items {
            let path = item.path.clone();
            let note = if item.status.is_empty() { item.work_title.clone() } else { item.status.clone() };
            let label = if note.is_empty() { item.title.clone() } else { format!("{} {note}", item.title) };
            let active = path == current;
            rows.push(
                widget(Button::new(if active { format!("当前 {label}") } else { label }).kind(ButtonKind::Ghost))
                    .key(format!("inspect-asmr-playlist-{path}"))
                    .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::Open(path.clone()))))
                    .into_any(),
            );
        }
    }
    Some(queue_card("inspect-asmr-playlist", rows))
}

fn queue_head(label: &str, count: &str, key: &str) -> AnyView {
    let row = super::workbench::with_bottom_divider(
        Stack::row(8.0).align(AlignSpec::Center).padding_xy(10.0, 9.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)),
    );
    widget(row)
        .key(key.to_string())
        .children((
            widget(super::workbench::meta(label)),
            widget(Text::new(count.to_string()).color(SemanticColorRole::Text).font_size(12.0).font_weight(600)),
        ))
        .into_any()
}

fn queue_card(key: &str, rows: Vec<AnyView>) -> AnyView {
    widget(
        Stack::column(8.0)
            .padding_xy(0.0, 0.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Md),
    )
    .key(key.to_string())
    .children(rows)
    .into_any()
}

fn toggle_import(model: &mut ShellViewModel) {
    model.asmr.import_open = !model.asmr.import_open;
    model.asmr.import_error.clear();
    if model.asmr.import_open && model.asmr.lookup_id.trim().is_empty() {
        let custom = display_custom(model);
        if let Some(id) = custom.get("workId").or_else(|| custom.get("rjCode")) {
            model.asmr.lookup_id = id.clone();
        }
    }
    if model.asmr.lookup_provider.is_empty() {
        model.asmr.lookup_provider = DLSITE.into();
    }
}

fn lookup(model: &mut ShellViewModel) {
    let custom = display_custom(model);
    let id = model.asmr.lookup_id.trim().to_string();
    let id = if id.is_empty() { custom.get("workId").or_else(|| custom.get("rjCode")).cloned().unwrap_or_default() } else { id };
    if id.trim().is_empty() {
        model.asmr.import_error = "缺少作品 ID".into();
        return;
    }
    // 没有插件返回值时不写入候选，避免把查询词当成作品资料。
    eprintln!("Nana ASMR 候选抓取没有返回值：{id}");
    model.asmr.import_error.clear();
}

fn import_candidate(model: &mut ShellViewModel) {
    if !asmr_can_edit(model) {
        eprintln!("Nana ASMR 当前文件不能导入候选");
        return;
    }
    match parse_candidate(&model.asmr.import_draft) {
        Ok(candidate) => {
            model.asmr.import_error.clear();
            model.asmr.import_draft.clear();
            model.asmr.import_open = false;
            model.asmr.imported.push(candidate);
        }
        Err(error) => model.asmr.import_error = error,
    }
}

fn apply_candidate(model: &mut ShellViewModel, index: usize) {
    if !asmr_can_edit(model) {
        eprintln!("Nana ASMR 当前文件不能应用候选");
        return;
    }
    let Some(candidate) = listed_candidates(model).into_iter().nth(index) else {
        eprintln!("Nana ASMR 候选下标不存在：{index}");
        return;
    };
    if candidate.fields.is_empty() {
        return;
    }
    let writable = true;
    let repo = repo_id(model);
    for (key, value) in candidate.fields {
        model.inspect.reduce(writable, Some(repo.as_str()), InspectMessage::SetCustom { key, value });
    }
}

fn add_work(model: &mut ShellViewModel) {
    let custom = display_custom(model);
    let Some(row) = current_row(model) else {
        return;
    };
    if !is_audio(row, &custom) {
        return;
    }
    let repo = repo_id(model);
    let paths = work_audio(model, &custom).into_iter().map(|row| row.path.clone()).collect::<Vec<_>>();
    for path in paths {
        let Some(item) = model.files.rows.iter().find(|row| row.path == path).map(|row| playlist_item(&repo, row)) else {
            continue;
        };
        if model.asmr.playlist.iter().any(|existing| existing.repo_id == item.repo_id && existing.path == item.path) {
            continue;
        }
        model.asmr.playlist.push(item);
    }
}

fn add_random(model: &mut ShellViewModel) {
    let custom = display_custom(model);
    let repo = repo_id(model);
    let paths = work_audio(model, &custom).into_iter().map(|row| row.path.clone()).collect::<Vec<_>>();
    if paths.is_empty() {
        return;
    }
    let selected = paths
        .iter()
        .find(|path| !model.asmr.playlist.iter().any(|item| item.repo_id == repo && item.path == **path))
        .cloned()
        .or_else(|| paths.first().cloned());
    let Some(selected) = selected else {
        return;
    };
    eprintln!("Nana ASMR 随机取候选池里尚未加入的第一条");
    let Some(item) = model.files.rows.iter().find(|row| row.path == selected).map(|row| playlist_item(&repo, row)) else {
        return;
    };
    if !model.asmr.playlist.iter().any(|existing| existing.path == item.path && existing.repo_id == item.repo_id) {
        model.asmr.playlist.push(item);
    }
}

fn playlist_item(repo: &str, row: &super::files::FileRow) -> PlaylistItem {
    let own = display_row(row);
    PlaylistItem {
        repo_id: repo.into(),
        path: row.path.clone(),
        title: own.get("trackTitle").cloned().filter(|text| !text.is_empty()).unwrap_or_else(|| row.name.clone()),
        work_title: own.get("workTitle").cloned().unwrap_or_default(),
        status: listening_label(own.get("listeningStatus").map(String::as_str).unwrap_or("")),
    }
}

fn open_path(model: &mut ShellViewModel, path: &str) {
    if !model.files.rows.iter().any(|row| row.path == path) {
        eprintln!("Nana ASMR 播放列表路径不在当前目录：{path}");
        return;
    }
    model.selected_path = Some(path.to_string());
    model.files.set_drag_selection(vec![path.to_string()], Some(path.to_string()), Some(path.to_string()));
    model.inspect.begin_selection(path);
    model.inspect.loading = false;
    model.inspect.activity.clear();
}

fn listed_candidates(model: &ShellViewModel) -> Vec<Candidate> {
    let mut candidates = current_row(model).map(|row| read_candidates(&row.metadata)).unwrap_or_default();
    candidates.extend(model.asmr.imported.clone());
    candidates
}

fn read_candidates(metadata: &BTreeMap<String, Value>) -> Vec<Candidate> {
    let raw = metadata.get("providerCandidates").or_else(|| metadata.get("asmrProviderCandidates"));
    let Some(Value::Array(items)) = raw else {
        if raw.is_some() {
            eprintln!("Nana ASMR 候选不是数组");
        }
        return Vec::new();
    };
    items.iter().filter_map(candidate_from_value).collect()
}

fn candidate_from_value(value: &Value) -> Option<Candidate> {
    let Value::Object(object) = value else {
        return None;
    };
    let source = object.get("source").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).unwrap_or("provider");
    let confidence = object.get("confidence").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).unwrap_or("候选");
    let fields_value = object.get("fields").or_else(|| object.get("metadata")).unwrap_or(value);
    let Value::Object(fields) = fields_value else {
        return None;
    };
    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    for (key, field) in fields {
        if key == "source" || key == "confidence" || key == "fields" || key == "metadata" {
            continue;
        }
        if PROTECTED.contains(&key.as_str()) || !ALLOWED.contains(&key.as_str()) {
            skipped.push(key.clone());
            continue;
        }
        let text = field_text(field);
        if text.is_empty() {
            continue;
        }
        kept.push((key.clone(), text));
    }
    if kept.is_empty() && skipped.is_empty() {
        return None;
    }
    Some(Candidate { source: source.into(), confidence: confidence.into(), fields: kept, skipped })
}

fn parse_candidate(raw: &str) -> Result<Candidate, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("候选 JSON 为空".into());
    }
    let parsed: Value = serde_json::from_str(trimmed).map_err(|error| {
        eprintln!("Nana ASMR 候选 JSON 解析失败：{error}");
        "候选 JSON 格式不正确".to_string()
    })?;
    if !parsed.is_object() {
        return Err("候选 JSON 必须是对象".into());
    }
    candidate_from_value(&parsed).filter(|item| !item.fields.is_empty()).ok_or_else(|| "候选 JSON 没有可读取字段".into())
}

fn work_audio<'a>(model: &'a ShellViewModel, custom: &BTreeMap<String, String>) -> Vec<&'a super::files::FileRow> {
    let root = current_row(model).and_then(|row| meta_text(row, custom, "workRoot"));
    let Some(root) = root else {
        return current_row(model).filter(|row| is_audio(row, custom)).into_iter().collect();
    };
    let mut rows = model
        .files
        .rows
        .iter()
        .filter(|row| {
            let own = display_row(row);
            is_audio(row, &own) && meta_text(row, &own, "workRoot").as_deref() == Some(root.as_str())
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        let left_key = meta_text(left, &display_row(left), "trackPath").unwrap_or_else(|| left.path.clone());
        let right_key = meta_text(right, &display_row(right), "trackPath").unwrap_or_else(|| right.path.clone());
        left_key.cmp(&right_key)
    });
    rows
}

fn asmr_can_edit(model: &ShellViewModel) -> bool {
    current_row(model).is_some_and(|row| row.kind == "file" && row.asset_id.as_ref().is_some_and(|id| !id.is_empty()) && !row.is_virtual)
}

fn is_audio(row: &super::files::FileRow, custom: &BTreeMap<String, String>) -> bool {
    row.kind == "file" && meta_text(row, custom, "asmrEntryKind").as_deref() == Some("audio") && super::inspect_library::matches_asmr(custom)
}

fn current_row(model: &ShellViewModel) -> Option<&super::files::FileRow> {
    let path = model.inspect.target_path.as_deref()?;
    model.files.rows.iter().find(|row| row.path == path)
}

fn display_row(row: &super::files::FileRow) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (key, value) in &row.metadata {
        if let Some(text) = scalar_text(value) {
            map.insert(key.clone(), text);
        }
    }
    map
}

fn meta_text(row: &super::files::FileRow, custom: &BTreeMap<String, String>, key: &str) -> Option<String> {
    display_row(row).get(key).cloned().filter(|text| !text.is_empty()).or_else(|| custom.get(key).cloned().filter(|text| !text.is_empty()))
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn field_text(value: &Value) -> String {
    match value {
        Value::Array(items) => items.iter().filter_map(scalar_text).collect::<Vec<_>>().join("，"),
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn listening_label(status: &str) -> String {
    match status {
        "unlistened" => "未收听".into(),
        "listening" => "收听中".into(),
        "listened" => "已听完".into(),
        other => other.to_string(),
    }
}

fn repo_id(model: &ShellViewModel) -> String {
    model.workspace.active_repo_id.clone().or_else(|| model.repository_id.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::ShellPage;

    fn audio_model() -> ShellViewModel {
        let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
        let mut metadata = BTreeMap::new();
        metadata.insert("libraryKind".into(), Value::String("asmr".into()));
        metadata.insert("asmrEntryKind".into(), Value::String("audio".into()));
        model.files.rows = vec![super::super::files::FileRow {
            path: "works/voice.mp3".into(),
            name: "voice.mp3".into(),
            kind: "file".into(),
            asset_id: Some("works-voice.mp3".into()),
            is_virtual: false,
            thumbnail_path: None,
            hardlink_state: None,
            extension: Some("mp3".into()),
            pixel_width: 0,
            pixel_height: 0,
            texture_ready: false,
            thumbnail_rgba: None,
            page_rgba: None,
            palette: Vec::new(),
            size_label: String::new(),
            modified_at: String::new(),
            tags: Vec::new(),
            thumbnail_custom: false,
            provider_id: None,
            source_payload: None,
            metadata,
        }];
        model.inspect.begin_selection("works/voice.mp3");
        model.inspect.loading = false;
        model.workspace.active_repo_id = Some("acceptance-repo".into());
        model
    }

    #[test]
    fn empty_audio_has_playlist_shell_and_no_invented_lyrics() {
        let model = audio_model();
        let custom = display_custom(&model);
        assert!(super::super::inspect_library::matches_asmr(&custom));
        assert!(super::super::inspect_library::library_sections(&custom).len() == 1);
        assert!(preview_panels(&model).is_some());
        assert_eq!(candidate_section(&model, &custom).len(), 1);
        assert!(model.asmr.playlist.is_empty());
        assert!(model.asmr.imported.is_empty());
    }

    #[test]
    fn add_work_uses_the_file_name_and_clear_removes_it() {
        let mut model = audio_model();
        add_work(&mut model);
        assert_eq!(model.asmr.playlist.len(), 1);
        assert_eq!(model.asmr.playlist[0].title, "voice.mp3");
        assert!(model.asmr.playlist[0].work_title.is_empty());
        assert!(model.asmr.playlist[0].status.is_empty());
        model.reduce(ShellMessage::Asmr(AsmrMessage::ClearPlaylist));
        assert!(model.asmr.playlist.is_empty());
    }

    #[test]
    fn import_rejects_empty_json_and_does_not_invent_a_candidate() {
        let mut model = audio_model();
        import_candidate(&mut model);
        assert_eq!(model.asmr.import_error, "候选 JSON 为空");
        assert!(model.asmr.imported.is_empty());
        model.asmr.import_draft = r#"{"source":"DLsite","circle":"已有社团"}"#.into();
        import_candidate(&mut model);
        assert_eq!(model.asmr.imported.len(), 1);
        assert_eq!(model.asmr.imported[0].fields, vec![("circle".into(), "已有社团".into())]);
    }

    #[test]
    fn lookup_without_work_id_does_not_create_a_candidate() {
        let mut model = audio_model();
        lookup(&mut model);
        assert_eq!(model.asmr.import_error, "缺少作品 ID");
        assert!(model.asmr.imported.is_empty());
    }
}

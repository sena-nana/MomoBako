//! ASMR 补全候选：从素材元数据 `providerCandidates` 读出候选画成卡片；「应用」把候选字段按原始 JSON 类型
//! 写回素材，「导入候选」和「抓取候选」把新候选追加进当前素材的 `providerCandidates` 并保存。
//!
//! 对应 Vue `library-asmr/src/asmrCandidates.js` 和 `register.js` 的元数据面板。候选、导入框和抓取都只对着
//! 正在看的素材：写入经 [`InspectState::apply_metadata_patch`](super::super::inspect::InspectState::apply_metadata_patch)，
//! 素材换了就不写，候选不会串到别的文件上。抓取经插件调用 `provider.lookupMetadataCandidate`（DLsite、ASMR One）。

use std::collections::{BTreeMap, BTreeSet};

use nana_ui::runtime::view::{fields, text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, Button, SelectOption, Stack};
use nana_ui::ButtonKind;
use serde_json::{Map, Value};

use super::super::admin::{AdminEffect, PluginCallOrigin};
use super::super::inspect::support::display_value;
use super::super::{ShellMessage, ShellViewModel};
use super::AsmrMessage;

pub(super) const DLSITE: &str = "momobako.service.provider.dlsite";
const ASMR_ONE: &str = "momobako.service.provider.asmr-one";
/// 元数据候选插件的方法（`docs/api-design.md` 的 Plugin API）。
const LOOKUP_METHOD: &str = "provider.lookupMetadataCandidate";

/// 候选不写的字段：注释、评分和收听进度归用户和播放器（Vue `protectedCandidateFields`）。
const PROTECTED: &[&str] = &["comment", "rating", "listeningProgress", "listeningStatus", "lastListenedAt", "trackPositionMs", "trackDurationMs"];

/// 候选能写的作品字段（Vue `providerCandidateFields`）。别的键列在「跳过」里。
const ALLOWED: &[&str] = &[
    "workId", "rjCode", "title", "workTitle", "circle", "creator", "voiceActors", "characters", "series", "scenarioTags",
    "audioTraits", "nsfw", "ageRating", "language", "releaseDate", "price", "dlCount", "sales", "reviewCount", "rateCount",
    "rateAverage", "rateCountDetail", "rank", "cover", "coverUrl", "coverSourceWorkId", "sourceUrl", "purchaseSource",
];

/// 候选对象自己的键，不算字段。
const ENVELOPE: &[&str] = &["source", "confidence", "fields", "metadata"];

/// 候选区的界面状态：导入框和抓取表单、选的来源、正在抓取的素材。不写进仓库。
#[derive(Clone, Debug, Default)]
pub(super) struct CandidateState {
    form: CandidateForm,
    /// 下拉框里选的来源，不跟着文件换；没选过时是 DLsite。
    provider: String,
    /// 正在抓取候选的素材：（仓库，素材 id）。按钮写「抓取中」，结果回来时按它认是哪个素材的。
    lookups: BTreeSet<(String, String)>,
}

/// 导入框、候选 JSON 草稿、作品 ID 和就近显示的错误。属于 `asset` 这个素材：换了文件按初始状态显示，
/// 下一次改动先清空，上一个文件填的作品 ID 不会拿去抓别的文件的候选。
#[derive(Clone, Debug, Default)]
struct CandidateForm {
    asset: Option<String>,
    import_open: bool,
    import_draft: String,
    lookup_id: String,
    error: String,
}

/// 元数据列里「补全候选」要显示的东西：只有 ASMR 条目才有这一块。
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::shell) struct CandidatesView {
    pub shown: bool,
    /// 当前文件不能导入或应用候选：没读完、虚拟条目、正在保存或有没处理的冲突。
    pub locked: bool,
    pub import_open: bool,
    /// 正在给当前文件抓取候选：来源、作品 ID 和按钮都不能动，按钮写「抓取中」。
    pub looking_up: bool,
    /// 下拉框里的来源，没选过时是 DLsite。
    pub provider: String,
    pub import_error: String,
    pub candidates: Vec<CandidateCard>,
}

/// 一条候选。按序号和内容认，内容变了整张重建。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::shell) struct CandidateCard {
    pub index: usize,
    pub title: String,
    pub fields: Vec<(String, String)>,
    pub skipped: Vec<String>,
}

/// 一条补全候选：来源、置信度和原始字段（保留 JSON 类型，应用时原样写回）。
#[derive(Clone, Debug, PartialEq)]
struct Candidate {
    source: String,
    confidence: Option<String>,
    fields: Map<String, Value>,
}

impl Candidate {
    /// 元数据里存的一条（Vue `readMetadataCandidates`）：只认对象，来源缺省是 provider，没有字段的不算。
    fn read(value: &Value) -> Option<Self> {
        let Value::Object(object) = value else {
            return None;
        };
        let fields = candidate_fields(object);
        (!fields.is_empty()).then(|| Self { source: name(object.get("source")).unwrap_or_else(|| "provider".into()), confidence: name(object.get("confidence")), fields })
    }

    /// 导入框里的 JSON（Vue `parseCandidateJson`）：必须是有字段的对象，来源和置信度缺省是 manual。
    fn imported(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("候选 JSON 为空".into());
        }
        let parsed: Value = serde_json::from_str(trimmed).map_err(|error| {
            eprintln!("Nana ASMR 候选 JSON 解析失败：{error}");
            "候选 JSON 格式不正确".to_string()
        })?;
        let Value::Object(object) = parsed else {
            return Err("候选 JSON 必须是对象".into());
        };
        let fields = candidate_fields(&object);
        if fields.is_empty() {
            return Err("候选 JSON 没有可读取字段".into());
        }
        Ok(Self {
            source: name(object.get("source")).unwrap_or_else(|| "manual".into()),
            confidence: Some(name(object.get("confidence")).unwrap_or_else(|| "manual".into())),
            fields,
        })
    }

    /// 插件抓回来的结果：`{ provider, id, sourceUrl, fetchedAt, candidate: { source, confidence, fields } }`，
    /// 没有 `candidate` 时整个结果就是候选（Vue `response.payload.candidate ?? response.payload`）。
    /// 来源缺省用结果里的 `provider`。没有字段时报错，不存一条空候选。
    fn looked_up(payload: &Value) -> Result<Self, String> {
        let candidate = payload.get("candidate").filter(|candidate| candidate.is_object()).unwrap_or(payload);
        let Value::Object(object) = candidate else {
            eprintln!("Nana ASMR 抓取结果不是对象：{payload}");
            return Err("抓取结果格式不正确".into());
        };
        let fields = candidate_fields(object);
        if fields.is_empty() {
            eprintln!("Nana ASMR 抓取结果没有字段：{payload}");
            return Err("抓取结果没有可读取字段".into());
        }
        let source = name(object.get("source")).or_else(|| name(payload.get("provider"))).unwrap_or_else(|| "provider".into());
        Ok(Self { source, confidence: name(object.get("confidence")), fields })
    }

    /// 存进 `providerCandidates` 的样子（Vue `appendMetadataCandidate`）：来源、置信度（有时）和字段。
    fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("source".into(), Value::String(self.source.clone()));
        if let Some(confidence) = &self.confidence {
            object.insert("confidence".into(), Value::String(confidence.clone()));
        }
        object.insert("fields".into(), Value::Object(self.fields.clone()));
        Value::Object(object)
    }

    /// 应用时写的字段和跳过的键（Vue `buildCandidateSummary`）。受保护和不认识的键跳过；没有内容的值
    /// （null、空白文字、空数组）既不写也不列：写进去只会把文件上已有的值清掉。
    fn patch(&self) -> (BTreeMap<String, Value>, Vec<String>) {
        let mut patch = BTreeMap::new();
        let mut skipped = Vec::new();
        for (key, value) in &self.fields {
            if PROTECTED.contains(&key.as_str()) || !ALLOWED.contains(&key.as_str()) {
                skipped.push(key.clone());
            } else if has_content(value) {
                patch.insert(key.clone(), value.clone());
            }
        }
        (patch, skipped)
    }
}

/// Vue `normalizeCandidateFields`：字段在 `fields` 里，没有时在 `metadata` 里，都没有时就是候选对象本身；
/// 去掉候选自己的四个键。
fn candidate_fields(object: &Map<String, Value>) -> Map<String, Value> {
    let source = ["fields", "metadata"].iter().find_map(|key| object.get(*key).and_then(Value::as_object)).unwrap_or(object);
    source.iter().filter(|(key, _)| !ENVELOPE.contains(&key.as_str())).map(|(key, value)| (key.clone(), value.clone())).collect()
}

/// Vue `normalizeProviderName`：去掉首尾空白的非空文字。
fn name(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).map(str::to_string)
}

fn has_content(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        _ => true,
    }
}

/// 元数据里已存的候选列表，原样保留：读 `providerCandidates`，没有时读旧键 `asmrProviderCandidates`（Vue 的 `??`）。
fn stored_candidates(metadata: &BTreeMap<String, Value>) -> Vec<Value> {
    let present = |key: &str| metadata.get(key).filter(|value| !value.is_null());
    match present("providerCandidates").or_else(|| present("asmrProviderCandidates")) {
        Some(Value::Array(items)) => items.clone(),
        Some(other) => {
            eprintln!("Nana ASMR 候选不是数组，追加时从空列表开始：{other}");
            Vec::new()
        }
        None => Vec::new(),
    }
}

/// 卡片上列出的候选：读得出、而且有要写的字段或跳过的键。卡片序号和「应用」按这份列表认。
fn listed_candidates(metadata: &BTreeMap<String, Value>) -> Vec<Candidate> {
    stored_candidates(metadata)
        .iter()
        .filter_map(Candidate::read)
        .filter(|candidate| {
            let (patch, skipped) = candidate.patch();
            !patch.is_empty() || !skipped.is_empty()
        })
        .collect()
}

/// 当前文件的仓库和素材 id，前提是候选能写：素材读完了、不是虚拟条目、没在保存、没有没处理的冲突。
fn editable_asset(model: &ShellViewModel) -> Option<(String, String)> {
    let inspect = &model.inspect;
    if !inspect.can_edit() || !inspect.conflict.is_empty() {
        return None;
    }
    Some((inspect.current_repo_id()?.to_string(), inspect.current_asset_id()?.to_string()))
}

impl CandidateForm {
    /// 属于 `asset` 的初始表单。
    fn fresh(asset: Option<&str>) -> Self {
        Self { asset: asset.map(str::to_string), ..Self::default() }
    }
}

impl CandidateState {
    /// 表单属于当前素材时的样子；属于别的素材时按初始状态。
    fn form_for(&self, asset: Option<&str>) -> CandidateForm {
        if self.form.asset.as_deref() == asset { self.form.clone() } else { CandidateForm::fresh(asset) }
    }

    /// 要改表单：表单属于别的素材时先回到初始状态。
    fn form_mut(&mut self, asset: Option<&str>) -> &mut CandidateForm {
        if self.form.asset.as_deref() != asset {
            self.form = CandidateForm::fresh(asset);
        }
        &mut self.form
    }

    fn provider(&self) -> String {
        if self.provider.is_empty() { DLSITE.to_string() } else { self.provider.clone() }
    }
}

fn form_mut(model: &mut ShellViewModel) -> &mut CandidateForm {
    let asset = model.inspect.current_asset_id().map(str::to_string);
    model.asmr.candidates.form_mut(asset.as_deref())
}

/// 下拉框的两个来源。
pub(in crate::shell) fn provider_options() -> Vec<SelectOption> {
    vec![SelectOption::new(DLSITE, "DLsite"), SelectOption::new(ASMR_ONE, "ASMR One")]
}

/// 元数据列里的补全候选。不是 ASMR 条目或扩展没开时整块不占位；没有候选就写「暂无候选」，不造一条来源。
pub(in crate::shell) fn candidates_view(model: &ShellViewModel, custom: &BTreeMap<String, String>) -> CandidatesView {
    if !super::asmr_entry(model, custom) {
        return CandidatesView::default();
    }
    let state = &model.asmr.candidates;
    let asset = model.inspect.current_asset_id();
    let form = state.form_for(asset);
    let looking_up = model.inspect.current_repo_id().zip(asset).is_some_and(|(repo, asset)| state.lookups.contains(&(repo.to_string(), asset.to_string())));
    let candidates = listed_candidates(&super::current_metadata(model))
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| {
            let (patch, skipped) = candidate.patch();
            CandidateCard {
                index,
                title: format!("{} {}", candidate.source, candidate.confidence.as_deref().unwrap_or("候选")),
                fields: patch.iter().map(|(key, value)| (key.clone(), display_value(value))).collect(),
                skipped,
            }
        })
        .collect();
    CandidatesView {
        shown: true,
        locked: editable_asset(model).is_none(),
        import_open: form.import_open,
        looking_up,
        provider: state.provider(),
        import_error: form.error,
        candidates,
    }
}

/// 作品 ID 输入框里显示的值：没填过时取元数据里的 RJ 号或作品 ID（Vue 的 `workId`：`rjCode || workId`）。
pub(in crate::shell) fn lookup_id_shown(model: &ShellViewModel, custom: &BTreeMap<String, String>) -> String {
    let form = model.asmr.candidates.form_for(model.inspect.current_asset_id());
    if form.lookup_id.is_empty() {
        default_work_id(custom)
    } else {
        form.lookup_id
    }
}

fn default_work_id(custom: &BTreeMap<String, String>) -> String {
    ["rjCode", "workId"].iter().find_map(|key| custom.get(*key).map(|value| value.trim()).filter(|value| !value.is_empty())).unwrap_or_default().to_string()
}

/// 导入框里的候选 JSON 草稿。
pub(in crate::shell) fn import_draft(model: &ShellViewModel) -> String {
    model.asmr.candidates.form_for(model.inspect.current_asset_id()).import_draft
}

/// 一条候选：来源和置信度、「应用」，下面是要写入的字段和跳过的字段。
pub(in crate::shell) fn candidate_card(candidate: CandidateCard, apply_disabled: impl Fn() -> bool + Send + 'static) -> AnyView {
    let index = candidate.index;
    let empty = candidate.fields.is_empty();
    let mut rows = vec![text(candidate.title).key(format!("inspect-asmr-candidate-{index}")).into_any()];
    rows.push(
        widget(Button::new("应用").kind(ButtonKind::Ghost).disabled(true))
            .prop::<bool, fields::button::disabled>(move || empty || apply_disabled())
            .key(format!("inspect-asmr-apply-{index}"))
            .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::Apply(index))))
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

/// 候选区的消息：导入框、来源、作品 ID、导入、抓取和应用。播放列表的消息不归这里。
pub(super) fn reduce(model: &mut ShellViewModel, message: AsmrMessage) {
    match message {
        AsmrMessage::ToggleImport => {
            let form = form_mut(model);
            form.import_open = !form.import_open;
            form.error.clear();
        }
        AsmrMessage::SetProvider(value) => model.asmr.candidates.provider = value,
        AsmrMessage::SetLookupId(value) => form_mut(model).lookup_id = value,
        AsmrMessage::SetImportDraft(value) => form_mut(model).import_draft = value,
        AsmrMessage::ImportCandidate => import_candidate(model),
        AsmrMessage::Lookup => lookup(model),
        AsmrMessage::LookupFinished { repo_id, asset_id, result } => note_lookup(model, repo_id, asset_id, result),
        AsmrMessage::Apply(index) => apply_candidate(model, index),
        other => eprintln!("Nana ASMR 候选区收到别的消息：{other:?}"),
    }
}

/// 「导入候选」：解析导入框里的 JSON，追加进当前素材的 `providerCandidates` 并保存，成功后收起导入框。
fn import_candidate(model: &mut ShellViewModel) {
    let Some((_, asset_id)) = editable_asset(model) else {
        eprintln!("Nana ASMR 当前文件不能导入候选");
        return;
    };
    let draft = form_mut(model).import_draft.clone();
    match Candidate::imported(&draft).and_then(|candidate| append_candidate(model, &asset_id, candidate)) {
        Ok(()) => {
            let form = form_mut(model);
            form.import_draft.clear();
            form.import_open = false;
            form.error.clear();
        }
        Err(error) => {
            eprintln!("Nana ASMR 导入候选失败：{error}");
            form_mut(model).error = error;
        }
    }
}

/// 把一条候选追加到当前素材已有的候选后面，按原始类型写回 `providerCandidates`。
fn append_candidate(model: &mut ShellViewModel, asset_id: &str, candidate: Candidate) -> Result<(), String> {
    let mut list = stored_candidates(&super::current_metadata(model));
    list.push(candidate.to_value());
    model.inspect.apply_metadata_patch(asset_id, BTreeMap::from([("providerCandidates".to_string(), Value::Array(list))]))
}

/// 「抓取候选」：调来源插件的 `provider.lookupMetadataCandidate`，载荷 `{ id }`，不带仓库（候选插件不是仓库的来源插件）。
/// 作品 ID 没填时用元数据里的 RJ 号或作品 ID。同一个素材已经在抓时不再发。
fn lookup(model: &mut ShellViewModel) {
    let Some((repo_id, asset_id)) = editable_asset(model) else {
        eprintln!("Nana ASMR 当前文件不能抓取候选");
        return;
    };
    let typed = form_mut(model).lookup_id.trim().to_string();
    let id = if typed.is_empty() { default_work_id(&super::display_custom(model)) } else { typed };
    if id.is_empty() {
        form_mut(model).error = "缺少作品 ID".into();
        return;
    }
    let key = (repo_id.clone(), asset_id.clone());
    if model.asmr.candidates.lookups.contains(&key) {
        eprintln!("Nana ASMR 这个文件的候选还在抓取：{asset_id}");
        return;
    }
    form_mut(model).error.clear();
    let plugin_id = model.asmr.candidates.provider();
    model.asmr.candidates.lookups.insert(key);
    model.admin.push_effect(AdminEffect::CallPlugin {
        plugin_id,
        method: LOOKUP_METHOD.into(),
        payload: serde_json::json!({ "id": id }),
        repository_id: None,
        origin: PluginCallOrigin::AsmrLookup { repo_id, asset_id },
    });
}

/// 抓取结果回来：还是正在看的那个素材时把候选追加进它的 `providerCandidates` 并保存，失败把原因写在导入框里；
/// 已经换了文件就丢掉，不写到别的文件上。
fn note_lookup(model: &mut ShellViewModel, repo_id: String, asset_id: String, result: Result<Value, String>) {
    model.asmr.candidates.lookups.remove(&(repo_id.clone(), asset_id.clone()));
    let current = model.inspect.current_asset_id() == Some(asset_id.as_str()) && model.inspect.current_repo_id() == Some(repo_id.as_str());
    if !current {
        eprintln!("Nana ASMR 候选抓取结果回来时已经换了文件，不写入：{asset_id}");
        return;
    }
    let outcome = result
        .map_err(|error| format!("抓取失败：{error}"))
        .and_then(|payload| Candidate::looked_up(&payload))
        .and_then(|candidate| append_candidate(model, &asset_id, candidate));
    match outcome {
        Ok(()) => form_mut(model).error.clear(),
        Err(error) => {
            eprintln!("Nana ASMR 候选抓取没有写入：{error}");
            form_mut(model).error = error;
        }
    }
}

/// 「应用」：把这条候选能写的字段按原始 JSON 类型写进当前素材并保存（数组还是数组、数字还是数字，来源链接也写）。
fn apply_candidate(model: &mut ShellViewModel, index: usize) {
    let Some((_, asset_id)) = editable_asset(model) else {
        eprintln!("Nana ASMR 当前文件不能应用候选");
        return;
    };
    let Some(candidate) = listed_candidates(&super::current_metadata(model)).into_iter().nth(index) else {
        eprintln!("Nana ASMR 候选下标不存在：{index}");
        return;
    };
    let (patch, _) = candidate.patch();
    if patch.is_empty() {
        return;
    }
    if let Err(error) = model.inspect.apply_metadata_patch(&asset_id, patch) {
        eprintln!("Nana ASMR 应用候选失败：{error}");
        form_mut(model).error = error;
    }
}

#[cfg(test)]
#[path = "inspect_asmr_candidates_tests.rs"]
mod tests;

//! ASMR 资源库扩展：预览列下方的作品队列和播放列表，元数据列里的补全候选。
//!
//! 扩展只在 ASMR 资源库插件启用、当前文件是 ASMR 条目时出现（Vue 只给插件注册的扩展认得的条目挂面板）。
//! 作品队列和播放列表来自当前列表里的条目，候选来自素材元数据；没有歌词正文、封面像素、时间戳或任务数时不补。
//! - `inspect_asmr_candidates.rs`：候选的读取、展示、应用、导入和抓取；
//! - `inspect_asmr_playlist.rs`：播放列表的加入、随机、清空和持久化，以及音轨的自然序。
//!
//! 这里只出投影（[`AsmrPanels`]）和按投影建的小块；预览页按投影整块重建作品队列，
//! 元数据列里的「补全候选」常驻，输入框受控，见 `inspect_metadata_view.rs`。

use std::borrow::Cow;
use std::collections::BTreeMap;

use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, Button, LengthSpec, RadiusTier, SemanticColorRole, Stack, Text};
use nana_ui::ButtonKind;
use serde_json::Value;

use super::files::{FileContext, FileRow, FilesMessage};
use super::inspect::support::display_value;
use super::inspect_library::{asmr_enabled, listening_label, matches_asmr};
use super::{ShellMessage, ShellViewModel};

#[path = "inspect_asmr_candidates.rs"]
mod candidates;
#[path = "inspect_asmr_playlist.rs"]
mod playlist;

pub(super) use candidates::{candidate_card, candidates_view, import_draft, lookup_id_shown, provider_options, CandidateCard, CandidatesView};
pub use playlist::asmr_playlist_path;

/// 候选区和播放列表的界面状态。候选写进素材元数据，播放列表写进设置目录，这里不存仓库数据。
#[derive(Clone, Debug, Default)]
pub struct AsmrUi {
    candidates: candidates::CandidateState,
    playlist: playlist::Playlist,
}

#[derive(Clone, Debug)]
pub enum AsmrMessage {
    ToggleImport,
    SetProvider(String),
    SetLookupId(String),
    SetImportDraft(String),
    ImportCandidate,
    Lookup,
    /// 抓取候选的插件调用回来了：发起时的仓库和素材，以及插件交回的结果。
    LookupFinished { repo_id: String, asset_id: String, result: Result<Value, String> },
    AddWork,
    AddRandom,
    ClearPlaylist,
    /// 作品队列或播放列表里点了一条：打开它的预览。
    Open(String),
    Apply(usize),
}

pub(super) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    let ShellMessage::Asmr(message) = message else {
        return Some(message);
    };
    match message {
        AsmrMessage::AddWork => playlist::add_work(model),
        AsmrMessage::AddRandom => playlist::add_random(model),
        AsmrMessage::ClearPlaylist => playlist::clear(model),
        AsmrMessage::Open(path) => open_path(model, &path),
        message => candidates::reduce(model, message),
    }
    None
}

/// 文件列表里现在看得见的行，不复制：智能文件夹是查询结果，分类视图去掉文件夹，其余是当前目录
/// （和 `FilesState::visible_rows` 同一规则）。作品队列、播放列表和打开都只认这些行。
fn visible_rows(model: &ShellViewModel) -> Vec<&FileRow> {
    let ctx = FileContext::from_model(model);
    if ctx.smart_folder {
        return model.files.virtual_rows.iter().collect();
    }
    model.files.rows.iter().filter(|row| !ctx.category_virtual || row.kind != "directory").collect()
}

/// 正在看的文件在列表里的那一行。从播放集打开、不在当前列表里的文件没有。
fn current_row(model: &ShellViewModel) -> Option<&FileRow> {
    let path = model.inspect.target_path.as_deref()?;
    visible_rows(model).into_iter().find(|row| row.path == path)
}

/// 正在看的文件的元数据：列表行上的，素材详情读回以后再叠上编辑草稿（草稿优先，含还没存完的补丁）。
/// 草稿在换文件时就清空，叠上来的只会是这个文件自己的。
fn current_metadata(model: &ShellViewModel) -> Cow<'_, BTreeMap<String, Value>> {
    let row = current_row(model).map(|row| &row.metadata);
    if model.inspect.current_asset_id().is_none() {
        return row.map(Cow::Borrowed).unwrap_or_default();
    }
    let draft = model.inspect.draft_custom();
    match row {
        None => Cow::Borrowed(draft),
        Some(row) => {
            let mut merged = row.clone();
            merged.extend(draft.iter().map(|(key, value)| (key.clone(), value.clone())));
            Cow::Owned(merged)
        }
    }
}

/// 列表里一行的元数据：正在看的那一行用 [`current_metadata`]，别的行用列表上的。
fn row_metadata<'a>(model: &'a ShellViewModel, row: &'a FileRow) -> Cow<'a, BTreeMap<String, Value>> {
    if model.inspect.target_path.as_deref() == Some(row.path.as_str()) {
        current_metadata(model)
    } else {
        Cow::Borrowed(&row.metadata)
    }
}

/// 当前文件的元数据画成文字：数组连成「甲，乙」，空值不收。作品信息、候选和自定义行都用它。
pub(super) fn display_custom(model: &ShellViewModel) -> BTreeMap<String, String> {
    current_metadata(model)
        .iter()
        .map(|(key, value)| (key.clone(), display_value(value)))
        .filter(|(_, text)| !text.is_empty())
        .collect()
}

/// ASMR 资源库扩展对这个文件开着：扩展插件启用，而且元数据认得是 ASMR 条目。
pub(super) fn asmr_entry(model: &ShellViewModel, custom: &BTreeMap<String, String>) -> bool {
    asmr_enabled(&model.admin.plugins) && matches_asmr(custom)
}

/// Vue `metadataText`：文字原样，数字、布尔转成文字，别的（数组、对象、没有）是空串。
fn entry_text(metadata: &BTreeMap<String, Value>, key: &str) -> String {
    match metadata.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

/// Vue `isAsmrAudioEntry`：文件、条目类型是 audio，而且是 ASMR 条目。
fn is_asmr_audio(kind: &str, metadata: &BTreeMap<String, Value>) -> bool {
    kind == "file" && entry_text(metadata, "asmrEntryKind") == "audio" && super::inspect_shortcuts::matches_entry("asmr", metadata)
}

/// 当前文件所在作品的音轨（Vue `workAudioEntries`）：同一个作品目录（`workRoot`）里的 ASMR 音频，按音轨路径的
/// 自然序排；当前文件没有作品目录时只有它自己（它是 ASMR 音频时）。
fn work_audio(model: &ShellViewModel) -> Vec<&FileRow> {
    let current = current_metadata(model);
    let work_root = entry_text(&current, "workRoot");
    if work_root.is_empty() {
        return current_row(model).filter(|row| is_asmr_audio(&row.kind, &current)).into_iter().collect();
    }
    let mut rows = visible_rows(model)
        .into_iter()
        .map(|row| (row, row_metadata(model, row)))
        .filter(|(row, metadata)| is_asmr_audio(&row.kind, metadata) && entry_text(metadata, "workRoot") == work_root)
        .map(|(row, metadata)| (playlist::track_key(row, &metadata), row))
        .collect::<Vec<_>>();
    rows.sort_by(|(left, _), (right, _)| playlist::natural_cmp(left, right));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// 作品队列或播放列表里点一条：像双击一样打开它的预览（Vue `previewEntry` 走 `previewFileEntryByDoubleClick`），
/// 选中它、读素材详情、换掉预览，音频照常接管播放条。不在当前列表里的条目打不开（Vue 也只认当前列表），只记日志。
fn open_path(model: &mut ShellViewModel, path: &str) {
    if !visible_rows(model).iter().any(|row| row.path == path && row.kind == "file") {
        eprintln!("Nana ASMR 要打开的文件不在当前列表：{path}");
        return;
    }
    if super::files::reduce_message(model, ShellMessage::Files(FilesMessage::OpenRow(path.to_string()))).is_some() {
        eprintln!("Nana ASMR 打开文件的消息没有被文件表面收下：{path}");
    }
}

/// 预览列下方作品队列和播放列表要显示的东西。内容变了整块按新内容重建。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct AsmrPanels {
    /// 同一作品目录里的其它音轨：「N 轨」和每轨的路径、文字。
    queue: Option<(String, Vec<(String, String)>)>,
    playlist: Option<playlist::PlaylistPanel>,
}

/// 预览列下方的作品队列和播放列表。扩展没开、不是 ASMR 条目，或两边都空，就没有。
pub(super) fn preview_panels(model: &ShellViewModel) -> Option<AsmrPanels> {
    if !asmr_entry(model, &display_custom(model)) {
        return None;
    }
    let panels = AsmrPanels { queue: work_queue(model), playlist: playlist::panel(model) };
    (panels.queue.is_some() || panels.playlist.is_some()).then_some(panels)
}

/// 作品队列：当前作品里别的音轨，标题是音轨名（没有时是文件名），后面是收听状态（没有时是「未收听」）。
fn work_queue(model: &ShellViewModel) -> Option<(String, Vec<(String, String)>)> {
    let current = model.inspect.target_path.as_deref().unwrap_or("");
    let siblings = work_audio(model).into_iter().filter(|row| row.path != current).collect::<Vec<_>>();
    if siblings.is_empty() {
        return None;
    }
    let count = format!("{} 轨", siblings.len() + 1);
    let items = siblings
        .into_iter()
        .map(|row| {
            let title = Some(entry_text(&row.metadata, "trackTitle")).filter(|text| !text.is_empty()).unwrap_or_else(|| row.name.clone());
            let status = Some(entry_text(&row.metadata, "listeningStatus")).filter(|text| !text.is_empty()).unwrap_or_else(|| "unlistened".into());
            (row.path.clone(), format!("{title} {}", listening_label(&status)))
        })
        .collect();
    Some((count, items))
}

/// 作品队列和播放列表两张卡片，行距 12。
pub(super) fn panels_view(panels: &AsmrPanels) -> AnyView {
    let mut rows = Vec::new();
    if let Some((count, items)) = &panels.queue {
        let mut card = vec![queue_head("作品队列", count, "inspect-asmr-queue-head")];
        card.extend(items.iter().map(|(path, label)| open_button(label.clone(), "inspect-asmr-queue", path)));
        rows.push(queue_card("inspect-asmr-queue", card));
    }
    if let Some(playlist) = &panels.playlist {
        rows.push(playlist_view(playlist));
    }
    widget(Stack::column(12.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).key("inspect-asmr-preview").children(rows).into_any()
}

/// 队列里的一条：点了打开这个文件。键里的路径换掉分隔符，带子目录的路径也能挂上。
fn open_button(label: String, prefix: &str, path: &str) -> AnyView {
    let target = path.to_string();
    widget(Button::new(label).kind(ButtonKind::Ghost))
        .key(format!("{prefix}-{}", super::files_view::style::key_part(path)))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::Open(target.clone()))))
        .into_any()
}

fn playlist_view(playlist: &playlist::PlaylistPanel) -> AnyView {
    let audio = playlist.audio;
    let mut rows = vec![queue_head("播放列表", &playlist.count, "inspect-asmr-playlist-head")];
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
                widget(Button::new("清空").kind(ButtonKind::Ghost).disabled(playlist.items.is_empty())).key("inspect-asmr-clear").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::ClearPlaylist));
                }),
            ))
            .into_any(),
    );
    if playlist.items.is_empty() {
        rows.push(text("暂无播放队列".to_string()).key("inspect-asmr-playlist-empty").into_any());
    } else {
        rows.extend(playlist.items.iter().map(|(path, label)| open_button(label.clone(), "inspect-asmr-playlist", path)));
    }
    queue_card("inspect-asmr-playlist", rows)
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

#[cfg(test)]
#[path = "inspect_asmr_tests.rs"]
mod tests;

//! ASMR 资源库扩展：预览列下方的作品队列和播放列表，元数据列里的补全候选。
//!
//! 扩展只在 ASMR 资源库插件启用、当前文件是 ASMR 条目时出现（Vue 只给插件注册的扩展认得的条目挂面板）。
//! 作品队列认正在预览的列表条目，列表里没有再认播放集条目，不跟未保存草稿，也不跟详情叠层。
//! 加入播放列表时写入的快照，以及播放列表卡片是否出现，同样认列表行或播放集条目自己的字段，不跟草稿。
//! 已加入、还在当前列表里的条目，标题和状态按这一行自己的字段重算，不跟草稿。
//! 候选来自素材元数据。没有歌词正文、封面像素、时间戳或任务数时不补。
//! 作品信息上的「保存封面」只下载元数据或候选里已经写着的 http(s) 地址，见 `inspect_asmr_cover.rs`。
//! - `inspect_asmr_candidates.rs`：候选的读取、展示、应用、导入和抓取；
//! - `inspect_asmr_playlist.rs`：播放列表的加入、随机、清空和持久化，以及音轨的自然序。
//!
//! 这里只出投影（[`AsmrPanels`]）和按投影建的小块；预览页按投影整块重建作品队列，
//! 元数据列里的「补全候选」常驻，输入框受控，见 `inspect_metadata_view.rs`。

use std::borrow::Cow;
use std::collections::BTreeMap;

use nana_ui::ButtonKind;
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, JustifySpec, LengthSpec, ListItem, ListItemRole, RadiusTier,
    ScrollAxes, ScrollView, SemanticColorRole, Stack, Text,
};
use serde_json::Value;

use super::files::{EnsuredThumbnail, FileContext, FileRow, FilesMessage};
use super::inspect::support::display_value;
use super::inspect_library::{asmr_enabled, matches_asmr};
use super::{ShellMessage, ShellViewModel};

#[path = "inspect_asmr_candidates.rs"]
mod candidates;
#[path = "inspect_asmr_cover.rs"]
mod cover;
#[path = "inspect_asmr_playlist.rs"]
mod playlist;

pub(super) use candidates::{
    CandidateCard, CandidatesView, candidate_card, candidates_view, import_draft, lookup_id_shown,
    provider_options,
};

/// 给作品信息补上「保存封面」。没有 http(s) 地址时不动。
pub(super) fn attach_cover(
    sections: &mut [super::inspect_library::LibrarySection],
    model: &ShellViewModel,
) {
    cover::attach(sections, model);
}
pub use playlist::asmr_playlist_path;

/// 候选区和播放列表的界面状态。候选写进素材元数据，播放列表写进设置目录，这里不存仓库数据。
#[derive(Clone, Debug, Default)]
pub struct AsmrUi {
    candidates: candidates::CandidateState,
    playlist: playlist::Playlist,
    cover: cover::CoverSave,
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
    LookupFinished {
        repo_id: String,
        asset_id: String,
        result: Result<Value, String>,
    },
    AddWork,
    AddRandom,
    ClearPlaylist,
    /// 作品队列或播放列表里点了一条：打开它的预览。
    Open(String),
    Apply(usize),
    /// 作品信息「保存封面」或候选「封面」。地址是按钮上的那一个 http(s)。
    SaveCover(String),
    /// `save + sourceUrl` 回来了。按发起时的仓库和路径认。
    CoverSaved {
        repo_id: String,
        path: String,
        result: Result<EnsuredThumbnail, String>,
    },
}

pub(super) fn reduce_message(
    model: &mut ShellViewModel,
    message: ShellMessage,
) -> Option<ShellMessage> {
    let ShellMessage::Asmr(message) = message else {
        return Some(message);
    };
    match message {
        AsmrMessage::AddWork => playlist::add_work(model),
        AsmrMessage::AddRandom => playlist::add_random(model),
        AsmrMessage::ClearPlaylist => playlist::clear(model),
        AsmrMessage::Open(path) => open_path(model, &path),
        AsmrMessage::SaveCover(url) => cover::begin(model, url),
        AsmrMessage::CoverSaved {
            repo_id,
            path,
            result,
        } => cover::finish(model, repo_id, path, result),
        message => candidates::reduce(model, message),
    }
    None
}

/// 文件列表里现在看得见的行，不复制。作品队列、播放列表和打开都只认这些行。
fn visible_rows(model: &ShellViewModel) -> Vec<&FileRow> {
    model.files.listed_rows(&FileContext::from_model(model))
}

/// 正在看的文件在列表里的那一行。从播放集打开、不在当前列表里的文件没有。
fn current_row(model: &ShellViewModel) -> Option<&FileRow> {
    let path = model.inspect.target_path.as_deref()?;
    visible_rows(model).into_iter().find(|row| row.path == path)
}

/// 正在看的文件的元数据：列表行上的，素材详情读回以后再叠上编辑草稿（草稿优先，含还没存完的补丁）。
/// 草稿在换文件时就清空，叠上来的只会是这个文件自己的。
/// 详情卡的作品信息、自定义行、补全候选不走这里。作品队列也不走这里，它认列表或播放集条目。
/// 已加入条目的标题重算、加入时的快照和播放列表卡片也不走这里。
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
            merged.extend(
                draft
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
            Cow::Owned(merged)
        }
    }
}

/// 当前文件的元数据画成文字：数组连成「甲，乙」，空值不收。
/// 作品信息仍用这份草稿合并。已加入条目的标题重算、作品队列、加入时的快照和播放列表卡片不读这里。
/// 补全候选另读详情卡叠层。
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
    kind == "file"
        && entry_text(metadata, "asmrEntryKind") == "audio"
        && super::inspect_shortcuts::matches_entry("asmr", metadata)
}

/// 作品队列认的当前条目（Vue `previewFileEntry`）：列表里有就用这一行的元数据，没有再读播放集条目。
/// 不叠素材详情，也不并进 `draft_custom`。列表和播放集都没有时是空的，不拿草稿编一份。
fn queue_entry_metadata(model: &ShellViewModel) -> Cow<'_, BTreeMap<String, Value>> {
    if let Some(row) = current_row(model) {
        return Cow::Borrowed(&row.metadata);
    }
    let path = model.inspect.target_path.as_deref().unwrap_or("");
    if path.is_empty() || !super::files_view::previewing(model) {
        if !path.is_empty() {
            eprintln!(
                "Nana 作品队列在列表里找不到当前文件，也不在预览页，不拿草稿或素材详情编：{path}"
            );
        }
        return Cow::Owned(BTreeMap::new());
    }
    Cow::Owned(super::inspect_metadata_view::current_card_metadata(model))
}

/// Vue 只给 `previewFileEntry` 挂预览扩展：插件开着，而且这一条自己的元数据是 ASMR。
/// 作品队列和播放列表卡片都先过这一关。不跟未保存草稿，也不跟详情叠层。
fn preview_extension_matches(model: &ShellViewModel) -> bool {
    asmr_enabled(&model.admin.plugins)
        && super::inspect_shortcuts::matches_entry("asmr", &queue_entry_metadata(model))
}

/// 作品队列该不该出现。条件和预览扩展挂上一样。
fn queue_visible(model: &ShellViewModel) -> bool {
    preview_extension_matches(model)
}

/// 当前文件所在作品的音轨（Vue `workAudioEntries`）：同一个作品目录（`workRoot`）里的 ASMR 音频，按音轨路径的
/// 自然序排。作品目录和音轨字段都读列表行自己的元数据；当前文件不在列表里时，作品目录读播放集条目。
/// 没有作品目录时只有当前这一行（它是 ASMR 音频、并且在列表里）。不跟草稿，也不跟详情叠层，没有的音轨不编。
fn work_audio(model: &ShellViewModel) -> Vec<&FileRow> {
    let work_root = entry_text(&queue_entry_metadata(model), "workRoot");
    if work_root.is_empty() {
        return current_row(model)
            .filter(|row| is_asmr_audio(&row.kind, &row.metadata))
            .into_iter()
            .collect();
    }
    let mut rows = visible_rows(model)
        .into_iter()
        .filter(|row| {
            is_asmr_audio(&row.kind, &row.metadata)
                && entry_text(&row.metadata, "workRoot") == work_root
        })
        .map(|row| (playlist::track_key(row, &row.metadata), row))
        .collect::<Vec<_>>();
    rows.sort_by(|(left, _), (right, _)| playlist::natural_cmp(left, right));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// 作品队列或播放列表里点一条：像双击一样打开它的预览（Vue `previewEntry` 走 `previewFileEntryByDoubleClick`），
/// 选中它、读素材详情、换掉预览，音频照常接管播放条。不在当前列表里的条目打不开（Vue 也只认当前列表），只记日志。
fn open_path(model: &mut ShellViewModel, path: &str) {
    if !visible_rows(model)
        .iter()
        .any(|row| row.path == path && row.kind == "file")
    {
        eprintln!("Nana ASMR 要打开的文件不在当前列表：{path}");
        return;
    }
    if super::files::reduce_message(
        model,
        ShellMessage::Files(FilesMessage::OpenRow(path.to_string())),
    )
    .is_some()
    {
        eprintln!("Nana ASMR 打开文件的消息没有被文件表面收下：{path}");
    }
}

/// 作品队列里的一行。标题在左，收听状态在右。正在预览的音轨不在这张表里。
#[derive(Clone, Debug, PartialEq)]
struct QueueLine {
    path: String,
    title: String,
    /// `listeningStatus` 的原文。没有这个字段、或是空串时，用 Vue 的占位 `unlistened`。
    status: String,
}

/// 预览列下方作品队列和播放列表要显示的东西。内容变了整块按新内容重建。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct AsmrPanels {
    /// 同一作品目录里的其它音轨：「N 轨」和每轨的路径、标题、状态。
    queue: Option<(String, Vec<QueueLine>)>,
    playlist: Option<playlist::PlaylistPanel>,
}

/// 预览列下方的作品队列和播放列表。扩展没开、不是 ASMR 条目，或两边都空，就没有。
pub(super) fn preview_panels(model: &ShellViewModel) -> Option<AsmrPanels> {
    let queue = work_queue(model);
    let playlist = preview_extension_matches(model)
        .then(|| playlist::panel(model))
        .flatten();
    let panels = AsmrPanels { queue, playlist };
    (panels.queue.is_some() || panels.playlist.is_some()).then_some(panels)
}

/// 作品队列：当前作品里别的音轨。标题是音轨名（没有时是文件名）。
/// 状态是 `listeningStatus` 的原文（Vue `metadataText(entry, "listeningStatus") || "unlistened"`），不翻译。
/// 标题和状态读列表行自己的元数据，不读草稿，也不读详情叠层。没有的字段不另编。
/// 正在预览的这一条不列进来，和 Vue `queueEntries` 一样；没有别的音轨时整张卡片不出现，不编造作品。
fn work_queue(model: &ShellViewModel) -> Option<(String, Vec<QueueLine>)> {
    if !queue_visible(model) {
        return None;
    }
    let current = model.inspect.target_path.as_deref().unwrap_or("");
    let siblings = work_audio(model)
        .into_iter()
        .filter(|row| row.path != current)
        .collect::<Vec<_>>();
    if siblings.is_empty() {
        return None;
    }
    let count = format!("{} 轨", siblings.len() + 1);
    let items = siblings
        .into_iter()
        .map(|row| {
            let title = Some(entry_text(&row.metadata, "trackTitle"))
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| row.name.clone());
            let status = Some(entry_text(&row.metadata, "listeningStatus"))
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "unlistened".into());
            QueueLine {
                path: row.path.clone(),
                title,
                status,
            }
        })
        .collect();
    Some((count, items))
}

/// 作品队列和播放列表两张卡片，间距 12，对应每张 `.files-preview-page__queue` 的上外边距。
pub(super) fn panels_view(panels: &AsmrPanels) -> AnyView {
    let mut rows = Vec::new();
    if let Some((count, items)) = &panels.queue {
        let mut card = vec![queue_head("作品队列", count, "inspect-asmr-queue-head")];
        card.push(queue_list(
            work_queue_rows(items),
            "inspect-asmr-queue-list",
        ));
        rows.push(queue_card("inspect-asmr-queue", card));
    }
    if let Some(playlist) = &panels.playlist {
        rows.push(playlist_view(playlist));
    }
    widget(
        Stack::column(12.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .key("inspect-asmr-preview")
    .children(rows)
    .into_any()
}

/// 作品队列有条目时的行。左右结构和播放列表相同，但 Vue 的作品队列按钮不加 `is-active`
/// （当前音轨已经滤掉），所以不铺 `--accent-soft`，底保持透明。没有条目时不调用。
fn work_queue_rows(items: &[QueueLine]) -> Vec<AnyView> {
    let last = items.len().saturating_sub(1);
    items
        .iter()
        .enumerate()
        .map(|(index, line)| {
            queue_item(
                format!(
                    "inspect-asmr-queue-{}",
                    super::files_view::style::key_part(&line.path)
                ),
                &line.title,
                &line.status,
                &line.path,
                false,
                index != last,
            )
        })
        .collect()
}

/// 播放列表有条目时的行。正在预览的那一条铺 `--accent-soft`（Vue `.is-active`），悬停也留着这层底。没有条目时不调用。
fn playlist_lines(items: &[playlist::PlaylistLine]) -> Vec<AnyView> {
    let last = items.len().saturating_sub(1);
    items
        .iter()
        .enumerate()
        .map(|(index, line)| {
            queue_item(
                format!(
                    "inspect-asmr-playlist-{}",
                    super::files_view::style::key_part(&line.path)
                ),
                &line.title,
                &line.status,
                &line.path,
                line.current,
                index != last,
            )
        })
        .collect()
}

/// 队列里的一行，对齐 `.files-preview-page__queue-item`：左标题、右 11px 弱色状态，间距 12。
/// 整行至少 36px，内边距 7×10，除最后一条外底边 border-soft。`current` 为真时铺 `--accent-soft`。
/// 点了打开这个文件。
fn queue_item(
    key: String,
    title: &str,
    status: &str,
    path: &str,
    current: bool,
    ruled: bool,
) -> AnyView {
    let target = path.to_string();
    let mut item = ListItem::new(title.to_string()).role(ListItemRole::Button);
    let mut style = item.style.clone();
    style.background = current.then_some(SemanticColorRole::AccentSoft);
    style.radius = None;
    style.control_height = None;
    style.control_padding_x = None;
    style.foreground = Some(SemanticColorRole::Text);
    let resting = if current {
        Some(SemanticColorRole::AccentSoft)
    } else {
        Some(SemanticColorRole::Hover)
    };
    style.interaction.hovered.background = resting;
    style.interaction.pressed.background = resting;
    if ruled {
        style.border = Some(SemanticColorRole::BorderSoft);
    }
    let layout = std::sync::Arc::make_mut(&mut style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.min_height = Some(LengthSpec::Px(36.0));
    layout.padding_left = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(10.0));
    layout.padding_top = Some(LengthSpec::Px(7.0));
    layout.padding_bottom = Some(LengthSpec::Px(7.0));
    layout.font_size = None;
    layout.line_height = None;
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    if ruled {
        layout.border_bottom_width = Some(1.0);
    }
    item = item.style(style);
    let mut children = vec![widget(queue_title(title)).into_any()];
    if !status.is_empty() {
        children.push(
            widget(queue_status(status))
                .key(format!("{key}-status"))
                .into_any(),
        );
    }
    widget(item)
        .content(
            widget(
                Stack::row(12.0)
                    .align(AlignSpec::Center)
                    .justify(JustifySpec::SpaceBetween)
                    .width(LengthSpec::Fill)
                    .min_width(LengthSpec::Px(0.0)),
            )
            .children(children),
        )
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::Open(target.clone())))
        })
        .into_any()
}

/// 行左的标题。字号跟着页面正文走，过长省略。
fn queue_title(title: &str) -> Text {
    let mut text = Text::new(title.to_string()).color(SemanticColorRole::Text);
    let layout = std::sync::Arc::make_mut(&mut text.style.layout);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.width = Some(LengthSpec::Px(0.0));
    layout.text_overflow_ellipsis = true;
    layout.white_space_nowrap = true;
    text
}

/// 行右的状态。对应 `.files-preview-page__queue-item small`：11px、弱色、不收缩。
fn queue_status(status: &str) -> Text {
    let mut text = Text::new(status.to_string())
        .color(SemanticColorRole::Muted)
        .font_size(11.0);
    let layout = std::sync::Arc::make_mut(&mut text.style.layout);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.width = Some(LengthSpec::Shrink);
    text
}

fn playlist_view(playlist: &playlist::PlaylistPanel) -> AnyView {
    let audio = playlist.audio;
    let mut rows = vec![queue_head(
        "播放列表",
        &playlist.count,
        "inspect-asmr-playlist-head",
    )];
    rows.push(playlist_actions(audio, playlist.items.is_empty()));
    if playlist.items.is_empty() {
        rows.push(queue_empty("暂无播放队列", "inspect-asmr-playlist-empty"));
    } else {
        rows.push(queue_list(
            playlist_lines(&playlist.items),
            "inspect-asmr-playlist-list",
        ));
    }
    queue_card("inspect-asmr-playlist", rows)
}

/// 「加入作品」「随机」「清空」。对齐 `.files-preview-page__queue-actions`：间距 6、内边距 8×10、底边 border-soft。
fn playlist_actions(audio: bool, empty: bool) -> AnyView {
    let row = super::workbench::with_bottom_divider(
        Stack::row(6.0)
            .align(AlignSpec::Center)
            .wrap(true)
            .padding_xy(10.0, 8.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    );
    widget(row)
        .key("inspect-asmr-playlist-actions")
        .children((
            widget(queue_action("加入作品", !audio))
                .key("inspect-asmr-add-work")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::AddWork));
                }),
            widget(queue_action("随机", !audio))
                .key("inspect-asmr-random")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::AddRandom));
                }),
            widget(queue_action("清空", empty))
                .key("inspect-asmr-clear")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::ClearPlaylist));
                }),
        ))
        .into_any()
}

/// 队列操作钮：至少 28px 高，左右 8、上下 4，12px 字。对应 `.files-preview-page__queue-actions .ghost`。
fn queue_action(label: &str, disabled: bool) -> Button {
    let button = Button::new(label)
        .kind(ButtonKind::Ghost)
        .disabled(disabled);
    let mut style = button.style.clone();
    style.control_height = None;
    style.control_padding_x = None;
    style.control_padding_y = None;
    let layout = std::sync::Arc::make_mut(&mut style.layout);
    layout.min_height = Some(LengthSpec::Px(28.0));
    layout.height = Some(LengthSpec::Px(28.0));
    layout.padding_left = Some(LengthSpec::Px(8.0));
    layout.padding_right = Some(LengthSpec::Px(8.0));
    layout.padding_top = Some(LengthSpec::Px(4.0));
    layout.padding_bottom = Some(LengthSpec::Px(4.0));
    layout.font_size = Some(12.0);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.width = Some(LengthSpec::Shrink);
    button.style(style)
}

/// 空队列。对齐 `.files-preview-page__queue-empty`：10px 内边距，12px 弱色。不编造条目。
fn queue_empty(label: &str, key: &str) -> AnyView {
    let copy = Text::new(label.to_string())
        .color(SemanticColorRole::Muted)
        .font_size(12.0)
        .line_height(18.6);
    widget(
        Stack::column(0.0)
            .padding(10.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children((widget(copy).key(key.to_string()),))
    .key(format!("{key}-row"))
    .into_any()
}

/// 标题在左、计数在右，底边 border-soft。对齐 `.files-preview-page__queue-head` 的两端对齐。
fn queue_head(label: &str, count: &str, key: &str) -> AnyView {
    let row = super::workbench::with_bottom_divider(
        Stack::row(12.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::SpaceBetween)
            .padding_xy(10.0, 9.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    );
    widget(row)
        .key(key.to_string())
        .children((
            widget(super::workbench::meta(label)),
            widget(
                Text::new(count.to_string())
                    .color(SemanticColorRole::Text)
                    .font_size(12.0)
                    .font_weight(700),
            ),
        ))
        .into_any()
}

/// 条目列表最高 176px，多了就在卡片里滚动，整张队列仍留在页底播放条上方。
fn queue_list(items: Vec<AnyView>, key: &str) -> AnyView {
    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.max_height = Some(LengthSpec::Px(176.0));
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
    }))
    .children(items)
    .key(key.to_string())
    .into_any()
}

/// `.files-preview-page__queue`：bg-elev 底、border-soft 细边、中圆角，子项紧贴、不留段间距。
fn queue_card(key: &str, rows: Vec<AnyView>) -> AnyView {
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md)
            .with_layout(|layout| {
                layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
                layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            }),
    )
    .key(key.to_string())
    .children(rows)
    .into_any()
}

#[cfg(test)]
#[path = "inspect_asmr_tests.rs"]
mod tests;

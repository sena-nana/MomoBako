//! 播放集页和底部播放条的组合，对齐 Vue `WorkspacePlaylistPage`。
//!
//! 播放集页是一张 bg-elev 面板：眉题、标题、「播放器 · N 项」和右上角播放；下面每条是
//! 白底圆角行（拖动柄、扩展名方块、文件名和路径、播放与移除），最后是播放条。
//! 没点开播放集时只有虚线空框。文件页和预览页只挂播放条。排序用 `ReorderList`。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, JustifySpec, LengthSpec, RadiusTier, ReorderItem, ReorderList, ReorderListEvent,
    SemanticColorRole, SemanticPaint, Stack, Text, TextHorizontalAlignment,
};
use nana_ui::{ButtonKind, Icon};

use crate::backend::services::repository::PlaylistItem;

use super::player::PlayerMessage;
use super::{ShellMessage, ShellViewModel};

#[path = "player_icons.rs"]
pub(super) mod icons;
#[path = "player_paint.rs"]
pub(super) mod paint;
#[path = "player_bar.rs"]
pub(super) mod bar;

/// 拼进键的外部文本（条目编号、路径）：`/` 是键路径分隔符，`%` 和 `/` 转义成 `%25`、`%2F`，
/// 不同的原文不会撞成同一个键。
pub(crate) fn key_part(text: &str) -> String {
    text.replace('%', "%25").replace('/', "%2F")
}

/// 文件页、预览页和播放集页共用的播放表面。播放集面板时是整页，其余是播放条。
/// 文件预览页自己把播放条贴在页底（Vue `files-preview-page` 里的 `WorkspacePlayerBar`），
/// 这时这里只给一个空位，不画第二条。
pub(super) fn player_surface(model: &ShellViewModel) -> AnyView {
    if model.workspace.panel == super::workspace::WorkspacePanel::Playlist {
        return playlist_page(model);
    }
    if preview_hosts_bar(model) {
        return widget(Stack::column(0.0)).key("player-surface-in-preview").into_any();
    }
    hosted_bar(model)
}

/// 播放条，下载进行时下面多一行进度。预览页的页底也用它。
pub(super) fn hosted_bar(model: &ShellViewModel) -> AnyView {
    with_download(model, bar::player_bar(model))
}

/// 文件面板正在显示文件预览页（和文件列的 `previewing` 同一判定：单击只选中时不算）。
fn preview_hosts_bar(model: &ShellViewModel) -> bool {
    model.workspace.panel == super::workspace::WorkspacePanel::Files
        && model.page == super::ShellPage::SelectedFile
        && model.inspect.has_target()
        && model.files.preview_open(model.inspect.target_path.as_deref())
}

/// 下载进度只在下载进行时写在播放条下面一行。
fn with_download(model: &ShellViewModel, bar: AnyView) -> AnyView {
    let download = model.player.download_text();
    if download.is_empty() {
        return bar;
    }
    widget(Stack::column(6.0))
        .children((bar, widget(Text::new(download).color(SemanticColorRole::Muted).font_size(12.0)).key("player-download")))
        .key("player-surface")
        .into_any()
}

/// 播放集页。没点开或详情还没读回时是虚线空框。
fn playlist_page(model: &ShellViewModel) -> AnyView {
    let Some(detail) = model.player.listed.as_ref() else {
        return dashed_empty("选择一个播放集", "在左侧播放集区选择要查看或播放的列表。", "playlist-page-empty");
    };
    let has_player = !playlist_plugin_missing(model, &detail.playlist.player_type_id);
    let mut subline = format!("{} · {} 项", detail.playlist.player_label, detail.items.len());
    if !has_player {
        subline.push_str(" · 缺少对应播放插件");
    }
    let mut body = vec![header(&detail.playlist.name, &subline, has_player)];
    if detail.items.is_empty() {
        body.push(dashed_empty(
            "播放集还是空的",
            "在文件浏览区右键文件，使用“加入播放列表”把内容加入这里。",
            "playlist-page-no-items",
        ));
    } else {
        body.push(playlist_items(model, &detail.items, has_player));
    }
    body.push(with_download(model, bar::player_bar(model)));
    let radius = crate::theme_map::radius_2xl(model.admin.corner_radius as f32);
    widget(Stack::column(16.0).padding(18.0).surface(SemanticColorRole::Surface).radius_px(radius))
        .children(body)
        .key("playlist-page")
        .into_any()
}

/// 眉题、22px 标题、13px 状态行；右上角是播放整张列表。
fn header(name: &str, subline: &str, has_player: bool) -> AnyView {
    let mut eyebrow = Text::new("播放集").color(SemanticColorRole::Faint).font_size(11.0).font_weight(600).line_height(17.05);
    std::sync::Arc::make_mut(&mut eyebrow.style.layout).letter_spacing = Some(0.4);
    let titles = widget(Stack::column(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
        widget(eyebrow).key("playlist-page-eyebrow"),
        widget(spaced(Text::new(name).color(SemanticColorRole::Text).font_size(22.0).font_weight(700).line_height(34.1), 4.0))
            .key("playlist-page-title"),
        widget(spaced(Text::new(subline).color(SemanticColorRole::Muted).font_size(13.0).line_height(20.15), 8.0))
            .key("playlist-page-status"),
    ));
    let play = widget(toolbar_button("播放", icons::PLAY, ButtonKind::Ghost, !has_player, SemanticColorRole::Surface))
        .key("playlist-play")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(player_message(PlayerMessage::PlayListed { item_id: None })));
    widget(Stack::bar(16.0).align(AlignSpec::Start))
        .children((titles, widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0)).children((play,))))
        .key("playlist-page-header")
        .into_any()
}

/// 文字上方留出 `margin`，对应 Vue 标题和状态行的上外边距。
fn spaced(mut text: Text, margin: f32) -> Text {
    std::sync::Arc::make_mut(&mut text.style.layout).margin_top = Some(LengthSpec::Px(margin));
    text
}

/// 条目列表。整行可拖动排序，排序结果交回壳层持久化。
fn playlist_items(model: &ShellViewModel, items: &[PlaylistItem], has_player: bool) -> AnyView {
    let playlist_id = model
        .selected_playlist_id
        .clone()
        .or_else(|| model.player.listed.as_ref().map(|detail| detail.playlist.playlist_id.clone()))
        .unwrap_or_default();
    let reorder = items
        .iter()
        .map(|item| {
            ReorderItem::new(item.playlist_item_id.clone(), item.filename.clone())
                .selected(model.player.current_id.as_deref() == Some(item.playlist_item_id.as_str()))
        })
        .collect::<Vec<_>>();
    let rows = items.iter().map(|item| playlist_row(item, &playlist_id, has_player)).collect::<Vec<_>>();
    widget(ReorderList::new(reorder).live_rows(true).spacing(10.0).label("播放集条目"))
        .key("playlist-reorder")
        .on_cx(|_, event: &ReorderListEvent, cx| {
            if let ReorderListEvent::Reorder { source, before } = event {
                cx.dispatch_program_all(player_message(PlayerMessage::Reorder {
                    source: source.to_string(),
                    before: before.as_ref().map(|value| value.to_string()),
                }));
            }
        })
        .children(rows)
        .into_any()
}

/// 一条：拖动柄、扩展名方块、文件名和路径、播放与移除。不可播放的条目整行变淡，次行写原因。
fn playlist_row(item: &PlaylistItem, playlist_id: &str, has_player: bool) -> AnyView {
    let id = item.playlist_item_id.clone();
    let ready = item.status == "ready";
    let detail = if ready {
        widget(meta_line(&item.path, SemanticColorRole::Muted)).key(format!("playlist-item-path-{}", key_part(id.as_ref()))).into_any()
    } else {
        let reason = item.status_reason.clone().unwrap_or_else(|| item.status.clone());
        widget(meta_line(&reason, SemanticColorRole::Danger)).key(format!("playlist-item-status-{}", key_part(id.as_ref()))).into_any()
    };
    let meta = widget(Stack::column(4.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0))
        .children((item_title(&id, &item.filename), detail));
    let play_id = id.clone();
    let remove_playlist = playlist_id.to_string();
    let remove_id = id.clone();
    let actions = widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0)).children((
        widget(toolbar_button("播放", icons::PLAY, ButtonKind::Ghost, !ready || !has_player, SemanticColorRole::Background))
            .key(format!("playlist-item-play-{}", key_part(id.as_ref())))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(player_message(PlayerMessage::PlayListed { item_id: Some(play_id.clone()) }));
            }),
        widget(toolbar_button("移除", icons::TRASH_2, ButtonKind::Danger, false, SemanticColorRole::Background))
            .key(format!("playlist-remove-{}", key_part(id.as_ref())))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::RemovePlaylistItem { playlist_id: remove_playlist.clone(), item_id: remove_id.clone() });
            }),
    ));
    widget(
        Stack::row(12.0)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .surface(SemanticColorRole::Background)
            .radius(RadiusTier::Xl)
            .with_layout(|layout| {
                layout.padding_top = Some(LengthSpec::Px(10.0));
                layout.padding_bottom = Some(LengthSpec::Px(10.0));
                layout.padding_left = Some(LengthSpec::Px(12.0));
                layout.padding_right = Some(LengthSpec::Px(12.0));
                layout.opacity = (!ready).then_some(0.72);
            }),
    )
    .children((drag_handle(&id), extension_mark(&id, &item.extension), meta, actions))
    .key(format!("playlist-item-{}", key_part(id.as_ref())))
    .into_any()
}

/// 拖动柄：32px 方块里一枚弱色竖握把。整行都能拖，柄只是提示。
fn drag_handle(id: &str) -> AnyView {
    widget(
        Stack::column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Px(32.0))
            .height(LengthSpec::Px(32.0))
            .grow(0.0)
            .shrink(0.0),
    )
    .children((widget(nana_ui::runtime::IconGlyph::new(icons::GRIP_VERTICAL).size(16.0).role(SemanticColorRole::Faint)),))
    .key(format!("playlist-item-drag-{}", key_part(id.as_ref())))
    .into_any()
}

/// 58px 扩展名方块：bg-subtle 底、12px 圆角、11px 粗体弱色字。点它打开预览。
fn extension_mark(id: &str, extension: &str) -> AnyView {
    let label = extension.trim().to_ascii_uppercase();
    let item_id = id.to_string();
    let mut button = Button::new(if label.is_empty() { " ".to_string() } else { label });
    button.style.background = Some(SemanticColorRole::Subtle);
    button.style.foreground = Some(SemanticColorRole::Muted);
    button.style.border = None;
    button.style.radius = Some(RadiusTier::Xl);
    button.style.control_height = None;
    button.style.control_padding_x = None;
    button.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    button.style.interaction.hovered.background = Some(SemanticColorRole::Hover);
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    for length in [&mut layout.width, &mut layout.height, &mut layout.min_width, &mut layout.min_height] {
        *length = Some(LengthSpec::Px(58.0));
    }
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.font_size = Some(11.0);
    layout.font_weight = Some(700);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    let style = button.style.clone();
    widget(Button::new(button.label).style(style))
        .key(format!("playlist-item-ext-{}", key_part(id.as_ref())))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::OpenPreview { item_id: Some(item_id.clone()) }));
        })
        .into_any()
}

/// 文件名按钮：整行 32px 高的可点区域，14px 半粗正文色字靠左。点它打开这一条的预览，不开始播放。
/// Vue 的全局按钮把字居中，和下面靠左的路径错开，这里按设计意图靠左。
fn item_title(id: &str, title: &str) -> AnyView {
    let item_id = id.to_string();
    let hit = widget(bar::hit_area(title, false, 0.0, 6.0))
        .key(format!("playlist-item-title-{}", key_part(id.as_ref())))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::OpenPreview { item_id: Some(item_id.clone()) }));
        });
    let mut label = Text::new(title).color(SemanticColorRole::Text).font_size(14.0).font_weight(600).line_height(21.7).truncating();
    std::sync::Arc::make_mut(&mut label.style.layout).min_width = Some(LengthSpec::Px(0.0));
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .height(LengthSpec::Px(32.0))
            .min_height(LengthSpec::Px(32.0)),
    )
    .children((hit, widget(label)))
    .into_any()
}

/// 12px 次行，超长省略。
fn meta_line(label: &str, role: SemanticColorRole) -> Text {
    let mut text = Text::new(label).color(role).font_size(12.0).line_height(18.6).truncating();
    let layout = std::sync::Arc::make_mut(&mut text.style.layout);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Fill);
    text
}

/// 工具栏按钮：34px 高、14px 图标在字前、6px 间距。普通操作透明底，危险操作是红字，悬停铺淡红。
/// 禁用时按 Vue 的整颗 0.45 不透明度处理：字色在 sRGB 里和按钮所在的底色 `base` 混。
fn toolbar_button(label: &str, icon: Icon, kind: ButtonKind, disabled: bool, base: SemanticColorRole) -> Button {
    let mut button = Button::new(label).kind(kind).icon(icon).icon_size(14.0).icon_gap(6.0).disabled(disabled);
    button.style.interaction.disabled = SemanticPaint::default();
    if disabled {
        let role = if kind == ButtonKind::Danger { SemanticColorRole::Danger } else { SemanticColorRole::Text };
        button.style.interaction.base.foreground_mix = Some(nana_ui_core::SemanticColorMix::new(role, base, bar::DISABLED_OPACITY));
    }
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.height = Some(LengthSpec::Px(34.0));
    layout.min_height = Some(LengthSpec::Px(34.0));
    layout.padding_left = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(10.0));
    layout.font_size = Some(14.0);
    layout.font_weight = Some(500);
    layout.width = Some(LengthSpec::Shrink);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    button
}

/// 虚线空框：bg-subtle 底、border-strong 虚线、12px 圆角，至少 280px 高，标题和说明居中。
fn dashed_empty(title: &str, message: &str, key: &'static str) -> AnyView {
    let mut heading = Text::new(title).color(SemanticColorRole::Text).font_size(18.0).font_weight(700).line_height(27.9);
    heading.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut note = Text::new(message).color(SemanticColorRole::Muted).font_size(13.0).line_height(20.15);
    note.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    widget(
        Stack::column(10.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderStrong, 1.0)
            .radius(RadiusTier::Xl)
            .min_height(LengthSpec::Px(280.0))
            .with_layout(|layout| layout.border_style = Some(nana_ui_core::BorderStyle::Dashed)),
    )
    .children((widget(heading).key(key), widget(note).key(format!("{key}-hint"))))
    .key(format!("{key}-frame"))
    .into_any()
}

/// 没有对应播放插件时，页眉和行上的播放都不可点。
pub(super) fn playlist_plugin_missing(model: &ShellViewModel, player_type_id: &str) -> bool {
    let ready = model.player.candidates.iter().any(|candidate| candidate.player_type_id == player_type_id)
        || model.player.contributions.iter().any(|player| player.player_type_id == player_type_id);
    !ready
}

fn player_message(message: PlayerMessage) -> ShellMessage {
    ShellMessage::Player(message)
}

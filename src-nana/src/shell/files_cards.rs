//! 文件列表里的一张条目卡片，对应 Vue `.files-list__item`。
//!
//! 四种展示方式共用：白底、`Lg` 圆角、1px 边（常态透明），选中时底色混入 10% 强调色、
//! 边线混入 34% 强调色；外部或内部拖到文件夹上时是放置态。网格固定 148×190，
//! 自适应和瀑布流按缩略图宽高比算宽高，列表是缩略图、标题路径和右对齐元信息三栏。
//! 卡片外面套一层框，框里放隐藏的路径标记，实况指针按它找到条目。

use std::sync::Arc;

use nana_ui::icons_tabler::{FILE, FILE_MUSIC, FOLDER, PHOTO, VIDEO};
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, ContentFit, JustifySpec, LengthSpec, ListItem, NodeStyle, RadiusTier, SecondaryPress,
    SemanticColorRole, SemanticPaint, Stack, Thumbnail,
};
use nana_ui_core::{Icon, SemanticColorMix};

use super::super::files::{hardlink_label, DisplayMode, FileRow, FilesMessage};
use super::file_message;
use super::style::{self, PreviewPainter, Tone};

/// 一张卡片的外框尺寸：宽（0 表示撑满所在列）和高（0 表示随内容）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CardBox {
    pub width: f32,
    pub height: f32,
    pub preview_width: f32,
    pub preview_height: f32,
}

/// 卡片要画的状态。
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CardState {
    pub selected: bool,
    pub drop_target: bool,
}

/// Vue 的缩略图宽高比：已知原始宽高时夹到 0.55–2.4，否则按正方形。
pub(super) fn aspect(row: &FileRow) -> f32 {
    super::super::thumbs::content_aspect(row.pixel_width, row.pixel_height)
}

/// 自适应：宽 `clamp(118, 120·比例 + 16, 300)`，缩略图高 120、宽 120·比例，居中。
pub(super) fn adaptive_box(row: &FileRow) -> CardBox {
    let ratio = aspect(row);
    let preview_width = 120.0 * ratio;
    let width = (preview_width + 16.0).clamp(118.0, 300.0);
    CardBox { width, height: 0.0, preview_width: preview_width.min(width - 2.0), preview_height: 120.0 }
}

/// 网格：卡片 148×190，缩略图撑满内容宽、高 112。
pub(super) const GRID_BOX: CardBox = CardBox { width: 148.0, height: 190.0, preview_width: 130.0, preview_height: 112.0 };

/// 瀑布流：卡片撑满列宽，缩略图撑满内容宽、高按宽高比。
pub(super) fn masonry_box(row: &FileRow, column_width: f32) -> CardBox {
    let content = (column_width - 18.0).max(1.0);
    CardBox { width: column_width, height: 0.0, preview_width: content, preview_height: content / aspect(row) }
}

/// 瀑布流卡片的估算高度：边框、上下内边距、缩略图、间距和标题区。用来把条目分到各列。
pub(super) fn masonry_height(row: &FileRow, column_width: f32) -> f32 {
    let preview = masonry_box(row, column_width).preview_height;
    let extra = body_lines(row, DisplayMode::Masonry).len() as f32 * (4.0 + 12.0 * 1.55);
    2.0 + 8.0 + preview + 8.0 + 38.0 + extra + 8.0
}

/// 列表：撑满一行，至少 72 高。
pub(super) const LIST_BOX: CardBox = CardBox { width: 0.0, height: 0.0, preview_width: 56.0, preview_height: 56.0 };

/// 条目卡片。单击选中、连点进入或预览、右键打开菜单都挂在卡片上。
pub(super) fn card(row: &FileRow, mode: DisplayMode, geometry: CardBox, state: CardState) -> AnyView {
    let list = mode.is_list();
    let content = if list { list_content(row, geometry) } else { tile_content(row, mode, geometry) };
    let path = row.path.clone();
    let menu_path = row.path.clone();
    let item = widget(ListItem::new(row.name.clone()).selected(state.selected).style(card_style(mode, geometry, state)))
        .content(content)
        .key(view_key("file-row", row))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(file_message(super::grid::row_activation(&path))))
        .on_cx(move |_, event: &SecondaryPress, cx| {
            cx.dispatch_program(file_message(FilesMessage::OpenEntryMenu { path: menu_path.clone(), x: event.x, y: event.y }));
        })
        .into_any();
    let mut frame = Stack::column(0.0).grow(0.0).shrink(0.0).min_width(LengthSpec::Px(0.0));
    frame = if geometry.width > 0.0 { frame.width(LengthSpec::Px(geometry.width)) } else { frame.width(LengthSpec::Fill) };
    if !list && mode != DisplayMode::Masonry {
        frame = frame.with_layout(|layout| layout.align_self = Some(AlignSpec::Start));
    }
    widget(frame).children((item, path_marker(&row.kind, &row.path))).into_any()
}

/// 卡片底、边和内边距。选中和放置态用主题混色，不写死 RGB。
fn card_style(mode: DisplayMode, geometry: CardBox, state: CardState) -> NodeStyle {
    let list = mode.is_list();
    let mut style = NodeStyle::default();
    style.background = Some(SemanticColorRole::Background);
    style.radius = Some(RadiusTier::Lg);
    style.foreground = Some(SemanticColorRole::Text);
    let selected = SemanticPaint {
        background_mix: Some(SemanticColorMix::new(SemanticColorRole::Background, SemanticColorRole::Accent, 0.9)),
        border_mix: Some(SemanticColorMix::new(SemanticColorRole::Accent, SemanticColorRole::BorderStrong, 0.34)),
        ..SemanticPaint::default()
    };
    style.interaction.hovered = SemanticPaint { background: Some(SemanticColorRole::Hover), ..SemanticPaint::default() };
    style.interaction.pressed = style.interaction.hovered;
    style.interaction.selected = selected;
    style.interaction.selected_hovered = selected;
    style.interaction.selected_pressed = selected;
    if state.drop_target {
        style.interaction.base = SemanticPaint {
            background_mix: Some(SemanticColorMix::new(SemanticColorRole::Accent, SemanticColorRole::Active, 0.14)),
            border_mix: Some(SemanticColorMix::alpha(SemanticColorRole::Accent, 0.45)),
            ..SemanticPaint::default()
        };
        style.interaction.hovered = style.interaction.base;
    }
    let layout = Arc::make_mut(&mut style.layout);
    layout.direction = Some(nana_ui_core::FlexDirection::Column);
    layout.border_width = Some(1.0);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.align_items = AlignSpec::Stretch;
    let (padding_x, padding_y) = if list { (10.0, 8.0) } else { (8.0, 8.0) };
    layout.padding_left = Some(LengthSpec::Px(padding_x));
    layout.padding_right = Some(LengthSpec::Px(padding_x));
    layout.padding_top = Some(LengthSpec::Px(padding_y));
    layout.padding_bottom = Some(LengthSpec::Px(padding_y));
    if geometry.height > 0.0 {
        layout.height = Some(LengthSpec::Px(geometry.height));
    }
    if list {
        layout.min_height = Some(LengthSpec::Px(72.0));
    }
    style
}

/// 网格、自适应和瀑布流：缩略图在上，标题居中在下。
fn tile_content(row: &FileRow, mode: DisplayMode, geometry: CardBox) -> AnyView {
    let mut lines = vec![widget(title(row, false)).key(view_key("file-title", row)).into_any()];
    for (index, line) in body_lines(row, mode).into_iter().enumerate() {
        lines.push(widget(style::centered(style::small_muted(line).truncating())).key(format!("{}-{index}", view_key("file-line", row))).into_any());
    }
    let body = widget(Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).justify(JustifySpec::Center))
        .children(lines)
        .into_any();
    widget(Stack::column(8.0).align(AlignSpec::Center).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((preview(row, geometry.preview_width, geometry.preview_height, 24.0, RadiusTier::Md), body))
        .into_any()
}

/// 列表：56 方缩略图、标题和路径、右对齐的类型 / 大小 / 硬链接 / 修改时间。
///
/// 三栏宽度按 `56px | minmax(0, 1.5fr) | minmax(220px, 1fr)` 分：标题列和元信息列 3:2 分剩余宽，
/// 元信息列不足 220 时取 220，其余归标题列。
fn list_content(row: &FileRow, geometry: CardBox) -> AnyView {
    let body = widget(
        Stack::column(4.0)
            .grow(1.5)
            .shrink(1.0)
            .min_width(LengthSpec::Px(0.0))
            .width(LengthSpec::Px(0.0))
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((
        widget(title(row, true)).key(view_key("file-title", row)),
        widget(style::small_muted(row.path.clone()).truncating()).key(view_key("file-path", row)),
    ))
    .into_any();
    let meta = widget(
        Stack::row(12.0)
            .wrap(true)
            .justify(JustifySpec::End)
            .align(AlignSpec::Center)
            .grow(1.0)
            .shrink(1.0)
            .min_width(LengthSpec::Px(220.0))
            .width(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                layout.flex_basis = Some(LengthSpec::Px(0.0));
                layout.row_gap = Some(LengthSpec::Px(6.0));
            }),
    )
    .children(
        list_meta(row)
            .into_iter()
            .enumerate()
            .map(|(index, text)| widget(style::small_muted(text).truncating()).key(format!("{}-{index}", view_key("file-meta", row))).into_any())
            .collect::<Vec<_>>(),
    )
    .into_any();
    widget(Stack::row(12.0).align(AlignSpec::Start).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((preview(row, geometry.preview_width, geometry.preview_height, 24.0, RadiusTier::Sm), body, meta))
        .into_any()
}

/// 卡片标题：14px/700，去掉扩展名，最多两行，网格里居中并至少占两行高。
fn title(row: &FileRow, list: bool) -> nana_ui::runtime::Text {
    let mut node = style::text(row.display_title(), 14.0, 700, SemanticColorRole::Text, 14.0 * 1.35);
    {
        let layout = Arc::make_mut(&mut node.style.layout);
        layout.line_clamp = Some(2);
        layout.text_overflow_ellipsis = true;
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
        if !list {
            layout.min_height = Some(LengthSpec::Px(38.0));
        }
    }
    if list { node } else { style::centered(node) }
}

/// 网格类卡片标题下的小字：硬链接状态。列表把它放进右侧元信息。
fn body_lines(row: &FileRow, mode: DisplayMode) -> Vec<String> {
    if mode.is_list() || row.kind == "directory" {
        return Vec::new();
    }
    let hardlink = hardlink_label(row.hardlink_state.as_deref());
    if hardlink.is_empty() { Vec::new() } else { vec![hardlink.to_string()] }
}

/// 列表右侧：文件夹写「文件夹 / 大小或目录项 / 修改时间」，文件写「扩展名或文件 / 大小或未知 / 硬链接 / 修改时间」。
fn list_meta(row: &FileRow) -> Vec<String> {
    let modified = super::super::files::local_time::format_or(&row.modified_at, "未记录");
    if row.kind == "directory" {
        let size = if row.size_label.trim().is_empty() { "目录项".to_string() } else { row.size_label.clone() };
        return vec!["文件夹".into(), size, modified];
    }
    let mut meta = vec![
        row.extension.clone().filter(|ext| !ext.trim().is_empty()).unwrap_or_else(|| "文件".into()),
        if row.size_label.trim().is_empty() { "未知".into() } else { row.size_label.clone() },
    ];
    let hardlink = hardlink_label(row.hardlink_state.as_deref());
    if !hardlink.is_empty() {
        meta.push(hardlink.to_string());
    }
    meta.push(modified);
    meta
}

/// 缩略图盒：已有纹理用 cover 铺满；只有像素时自绘 cover；都没有时按类型画白色图标。
pub(super) fn preview(row: &FileRow, width: f32, height: f32, icon_size: f32, radius: RadiusTier) -> AnyView {
    preview_box(row, Some(width), height, icon_size, radius)
}

/// 撑满所在列宽的缩略图盒，详情卡片用。
pub(super) fn preview_fill(row: &FileRow, height: f32, icon_size: f32, radius: RadiusTier) -> AnyView {
    preview_box(row, None, height, icon_size, radius)
}

fn preview_box(row: &FileRow, width: Option<f32>, height: f32, icon_size: f32, radius: RadiusTier) -> AnyView {
    let tone = if row.kind == "directory" { Tone::Folder } else { Tone::Placeholder };
    let texture = row
        .texture_ready
        .then(|| row.thumbnail_path.as_deref().map(str::trim).filter(|path| !path.is_empty()))
        .flatten();
    let image = if texture.is_none() {
        row.thumbnail_rgba
            .as_ref()
            .and_then(|(w, h, rgba)| super::super::inspect::rgba_png_data_url(*w, *h, rgba))
            .map(Arc::<str>::from)
    } else {
        None
    };
    let icon = (texture.is_none() && image.is_none()).then(|| entry_icon(row));
    let painter = PreviewPainter { tone, icon, icon_size, radius, image };
    let frame = match width {
        Some(width) => style::fixed(Stack::column(0.0), width, height),
        None => Stack::column(0.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .height(LengthSpec::Px(height))
            .min_height(LengthSpec::Px(height)),
    };
    let frame = frame.painter(painter).with_layout(|layout| {
        layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
        layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
    });
    let thumb = texture.map(|path| {
        let mut thumb = Thumbnail::new(super::super::thumbs::thumbnail_slot(path))
            .fit(ContentFit::Cover)
            .aspect(aspect(row));
        let layout = Arc::make_mut(&mut thumb.style.layout);
        layout.width = Some(width.map_or(LengthSpec::Fill, LengthSpec::Px));
        layout.height = Some(LengthSpec::Px(height));
        thumb.style.radius = Some(radius);
        widget(thumb).into_any()
    });
    widget(frame).children((thumb,)).key(view_key("file-preview", row)).into_any()
}

/// Vue 的类型图标：文件夹、视频、音频、有预览插件的文件，其余是图片图标。
pub(super) fn entry_icon(row: &FileRow) -> Icon {
    if row.kind == "directory" {
        return FOLDER;
    }
    let extension = row.extension.as_deref().unwrap_or("").trim().to_ascii_lowercase();
    if matches!(extension.as_str(), "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v") {
        return VIDEO;
    }
    if matches!(extension.as_str(), "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus") {
        return FILE_MUSIC;
    }
    let previewable = !extension.is_empty()
        && !matches!(
            super::super::inspect::support::classify(&extension, &super::super::inspect::native_preview::builtin_bindings()),
            super::super::inspect::PreviewKind::Unsupported
        );
    if previewable { FILE } else { PHOTO }
}

/// 组装键不能带路径分隔符。`notes/page.pdf` 里的 `/` 会让整行挂载失败。
pub(super) fn view_key(prefix: &str, row: &FileRow) -> String {
    let body = row.key().replace(['/', '\\'], "\u{2215}");
    format!("{prefix}-{body}")
}

/// 行上的隐藏路径。命中卡片后沿父节点找到它，实况指针才能对上条目。
fn path_marker(kind: &str, path: &str) -> AnyView {
    let mut marker = nana_ui::runtime::Text::new(format!("momobako-entry:{kind}:{path}"));
    let layout = Arc::make_mut(&mut marker.style.layout);
    layout.hidden = true;
    layout.height = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Px(0.0));
    widget(marker).into_any()
}

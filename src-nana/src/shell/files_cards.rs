//! 文件列表里的一张条目卡片，对应 Vue `.files-list__item`。
//!
//! 四种展示方式共用：白底、`Lg` 圆角、1px 边（常态透明），选中时底色混入 10% 强调色、
//! 边线混入 34% 强调色；外部或内部拖到文件夹上时是放置态。网格固定 148×190，
//! 自适应和瀑布流按缩略图宽高比算宽高，列表是缩略图、标题路径和右对齐元信息三栏。
//! 卡片外面套一层框，框里放隐藏的路径标记，实况指针按它找到条目。
//!
//! 卡片常驻：按条目键建一次，字段绑在卡片仓里这一张的值上（[`CardView`]）。选中、放置态、
//! 标题、元信息和缩略图到达都只改这一张卡片的字段，别的卡片不动。外框尺寸跟着所在的行走，
//! 尺寸变了的卡片所在的行整行重建（见 `files_virtual.rs`）。

use std::sync::{Arc, OnceLock};

use nana_ui::icons_tabler::{FILE, FILE_MUSIC, FOLDER, PHOTO, VIDEO};
use nana_ui::runtime::view::fields::{self, HiddenWhenEmpty};
use nana_ui::runtime::view::{untrack, widget, AnyView, IntoView, Item, Store, StoreList, StorePath};
use nana_ui::runtime::{
    Activate, AlignSpec, ContentFit, JustifySpec, LengthSpec, ListItem, NodeStyle, RadiusTier, SecondaryPress,
    SemanticColorRole, SemanticPaint, Stack, Text, Thumbnail,
};
use nana_ui_core::{Icon, LengthAtom, SemanticColorMix};

use super::super::files::{hardlink_label, DisplayMode, FileRow, FilesMessage};
use super::super::inspect::PreviewBinding;
use super::bind::StyleField;
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

impl CardBox {
    /// 按位比较用的四个数，行的身份里用它。
    pub(super) fn bits(&self) -> [u32; 4] {
        [self.width.to_bits(), self.height.to_bits(), self.preview_width.to_bits(), self.preview_height.to_bits()]
    }
}

/// 缩略图盒的样子：底色、已有纹理、已有像素或类型图标。三者按「纹理、像素、图标」的先后只取一样。
#[derive(Clone, Debug)]
pub(super) struct PreviewLook {
    pub tone: Tone,
    /// 宿主已经能注册纹理的缩略图路径。
    pub texture: Option<String>,
    /// 已有像素缩小后的缩略图，PNG data URL。
    pub image: Option<Arc<str>>,
    /// 纹理和像素都没有时画的类型图标。
    pub icon: Option<Icon>,
    /// 缩略图宽高比，按 Vue 的范围夹过。
    pub aspect: f32,
}

impl Default for PreviewLook {
    fn default() -> Self {
        Self { tone: Tone::Placeholder, texture: None, image: None, icon: None, aspect: 1.0 }
    }
}

/// 图标按几何指针比较。
impl PartialEq for PreviewLook {
    fn eq(&self, other: &Self) -> bool {
        self.tone == other.tone
            && self.texture == other.texture
            && self.image == other.image
            && self.icon.map(Icon::as_ptr) == other.icon.map(Icon::as_ptr)
            && self.aspect.to_bits() == other.aspect.to_bits()
    }
}

impl PreviewLook {
    /// 文件行的缩略图盒。`bindings` 是内置预览分派，判断类型图标用。
    pub(super) fn of(row: &FileRow, bindings: &[PreviewBinding]) -> Self {
        let tone = if row.kind == "directory" { Tone::Folder } else { Tone::Placeholder };
        let texture = row
            .texture_ready
            .then(|| row.thumbnail_path.as_deref().map(str::trim).filter(|path| !path.is_empty()).map(str::to_string))
            .flatten();
        let image = if texture.is_none() {
            row.thumbnail_rgba
                .as_ref()
                .and_then(|(w, h, rgba)| super::super::inspect::rgba_png_data_url(*w, *h, rgba))
                .map(Arc::<str>::from)
        } else {
            None
        };
        let icon = (texture.is_none() && image.is_none()).then(|| entry_icon(row, bindings));
        Self { tone, texture, image, icon, aspect: aspect(row.pixel_width, row.pixel_height) }
    }

    /// 缩略图槽；没有纹理时为空串（缩略图节点藏着）。
    fn resource(&self) -> Arc<str> {
        self.texture.as_deref().map(super::super::thumbs::thumbnail_slot).unwrap_or_default().into()
    }
}

/// 一张卡片由文件行算出的部分。行没变时卡片仓沿用上一份（同一个 `Arc`）。
#[derive(Debug)]
pub(super) struct CardFace {
    /// 条目键 `kind:path`，同名的文件和文件夹不会撞。
    pub key: Arc<str>,
    pub kind: Arc<str>,
    pub path: Arc<str>,
    pub name: String,
    /// 去掉扩展名的标题。
    pub title: String,
    /// 硬链接状态的说明，没有时为空。
    pub hardlink: &'static str,
    /// 列表右侧第一项：文件夹写「文件夹」，文件写扩展名或「文件」。
    pub kind_label: String,
    /// 列表右侧的大小：文件夹没有大小时写「目录项」，文件写「未知」。
    pub size: String,
    /// 修改时间，没有记录时写「未记录」。
    pub modified: String,
    /// 缩略图盒；宽高比也在这里，自适应和瀑布流按它算卡片尺寸。
    pub preview: PreviewLook,
}

impl CardFace {
    /// 从文件行算出卡片的文字、元信息和缩略图。
    pub(super) fn of(row: &FileRow, bindings: &[PreviewBinding]) -> Self {
        let directory = row.kind == "directory";
        let kind_label = if directory {
            "文件夹".to_string()
        } else {
            row.extension.clone().filter(|ext| !ext.trim().is_empty()).unwrap_or_else(|| "文件".into())
        };
        let size = match (row.size_label.trim().is_empty(), directory) {
            (true, true) => "目录项".to_string(),
            (true, false) => "未知".to_string(),
            (false, _) => row.size_label.clone(),
        };
        Self {
            key: row.key().into(),
            kind: row.kind.as_str().into(),
            path: row.path.as_str().into(),
            name: row.name.clone(),
            title: row.display_title(),
            hardlink: hardlink_label(row.hardlink_state.as_deref()),
            kind_label,
            size,
            modified: super::super::files::local_time::format_or(&row.modified_at, "未记录"),
            preview: PreviewLook::of(row, bindings),
        }
    }

    fn directory(&self) -> bool {
        &*self.kind == "directory"
    }

    /// 宽高比，按 Vue 的范围夹过。
    pub(super) fn aspect(&self) -> f32 {
        self.preview.aspect
    }

    /// 网格类卡片标题下的小字行数：文件有硬链接状态时一行。
    pub(super) fn body_lines(&self) -> usize {
        usize::from(!self.directory() && !self.hardlink.is_empty())
    }
}

/// 卡片仓里的一张卡片：行算出的部分，加上选中和放置态。
#[derive(Clone, Debug)]
pub(super) struct CardView {
    pub face: Arc<CardFace>,
    pub selected: bool,
    pub drop_target: bool,
}

impl CardView {
    /// 和 `other` 画出来一样：行算出的部分是同一份，选中和放置态也一样。
    pub(super) fn same_as(&self, other: &CardView) -> bool {
        Arc::ptr_eq(&self.face, &other.face) && self.selected == other.selected && self.drop_target == other.drop_target
    }
}

/// 卡片仓：本目录的全部卡片，按条目键找到一张。
pub(super) type Cards = Store<Vec<CardView>>;
/// 卡片仓里的一张。
pub(super) type CardItem = Item<Cards, Arc<str>, CardView>;

/// 卡片仓的键。
pub(super) fn card_key(card: &CardView) -> Arc<str> {
    card.face.key.clone()
}

/// 卡片仓里条目键是 `key` 的那一张。
pub(super) fn card_item(cards: Cards, key: &Arc<str>) -> CardItem {
    cards.keyed(card_key as fn(&CardView) -> Arc<str>).at(key)
}

/// 行里的一格：条目键、类型、路径和外框尺寸。成员或尺寸变了的行整行重建，所以这些是常量。
#[derive(Clone, Debug)]
pub(super) struct CardCell {
    pub key: Arc<str>,
    pub kind: Arc<str>,
    pub path: Arc<str>,
    pub geometry: CardBox,
    /// 标题下的小字行数（硬链接状态）。
    pub lines: usize,
}

/// Vue 的缩略图宽高比：已知原始宽高时夹到 0.55–2.4，否则按正方形。
pub(super) fn aspect(width: u32, height: u32) -> f32 {
    super::super::thumbs::content_aspect(width, height)
}

/// 自适应：宽 `clamp(118, 120·比例 + 16, 300)`，缩略图高 120、宽 120·比例，居中。
pub(super) fn adaptive_box(ratio: f32) -> CardBox {
    let preview_width = 120.0 * ratio;
    let width = (preview_width + 16.0).clamp(118.0, 300.0);
    CardBox { width, height: 0.0, preview_width: preview_width.min(width - 2.0), preview_height: 120.0 }
}

/// 网格：卡片 148×190，缩略图撑满内容宽、高 112。
pub(super) const GRID_BOX: CardBox = CardBox { width: 148.0, height: 190.0, preview_width: 130.0, preview_height: 112.0 };

/// 瀑布流：卡片撑满列宽，缩略图撑满内容宽、高按宽高比。
pub(super) fn masonry_box(ratio: f32, column_width: f32) -> CardBox {
    let content = (column_width - 18.0).max(1.0);
    CardBox { width: column_width, height: 0.0, preview_width: content, preview_height: content / ratio }
}

/// 瀑布流卡片的估算高度：边框、上下内边距、缩略图、间距和标题区。用来把条目分到各列。
pub(super) fn masonry_height(face: &CardFace, column_width: f32) -> f32 {
    let preview = masonry_box(face.aspect(), column_width).preview_height;
    let extra = face.body_lines() as f32 * (4.0 + 12.0 * 1.55);
    2.0 + 8.0 + preview + 8.0 + 38.0 + extra + 8.0
}

/// 列表：撑满一行，至少 72 高。
pub(super) const LIST_BOX: CardBox = CardBox { width: 0.0, height: 0.0, preview_width: 56.0, preview_height: 56.0 };
/// 列表元信息列的宽度下限（`minmax(220px, 1fr)`）和窄卡片里标题列至少保留的宽度。
const LIST_META_MIN: f32 = 220.0;
const LIST_TITLE_MIN: f32 = 80.0;

/// 条目卡片。单击选中、连点进入或预览、右键打开菜单都挂在卡片上；处理器只带条目路径这个常量。
pub(super) fn card(cell: &CardCell, mode: DisplayMode, cards: Cards) -> AnyView {
    let item = card_item(cards, &cell.key);
    let current = untrack(|| item.try_with(CardView::clone)).unwrap_or_else(|| {
        eprintln!("Nana 文件卡片在卡片仓里找不到：{}", cell.key);
        placeholder(cell)
    });
    let list = mode.is_list();
    let geometry = cell.geometry;
    let content = if list { list_content(cell, item, &current) } else { tile_content(cell, mode, item, &current) };
    let path = cell.path.to_string();
    let menu_path = cell.path.to_string();
    let entry = ListItem::new(current.face.name.clone())
        .selected(current.selected)
        .style(card_style(mode, geometry, current.drop_target));
    let entry = widget(entry)
        .prop::<String, fields::list_item::label>(move || item.try_with(|card| card.face.name.clone()).unwrap_or_default())
        .prop::<bool, fields::list_item::selected>(move || item.try_with(|card| card.selected).unwrap_or(false))
        .prop::<NodeStyle, StyleField>(move || card_style(mode, geometry, item.try_with(|card| card.drop_target).unwrap_or(false)))
        .content(content)
        .key(view_key("file-row", &cell.key))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(file_message(super::grid::row_activation(&path))))
        .on_cx(move |_, event: &SecondaryPress, cx| {
            cx.dispatch_program_all(file_message(FilesMessage::OpenEntryMenu { path: menu_path.clone(), x: event.x, y: event.y }));
        })
        .into_any();
    let mut frame = Stack::column(0.0).grow(0.0).shrink(0.0).min_width(LengthSpec::Px(0.0));
    frame = if geometry.width > 0.0 { frame.width(LengthSpec::Px(geometry.width)) } else { frame.width(LengthSpec::Fill) };
    if !list && mode != DisplayMode::Masonry {
        frame = frame.with_layout(|layout| layout.align_self = Some(AlignSpec::Start));
    }
    widget(frame).children((entry, path_marker(&cell.kind, &cell.path))).into_any()
}

/// 卡片仓里没有这一张时（不该发生）画的空卡片：只有键、类型和路径。
fn placeholder(cell: &CardCell) -> CardView {
    let face = CardFace {
        key: cell.key.clone(),
        kind: cell.kind.clone(),
        path: cell.path.clone(),
        name: String::new(),
        title: String::new(),
        hardlink: "",
        kind_label: String::new(),
        size: String::new(),
        modified: String::new(),
        preview: PreviewLook::default(),
    };
    CardView { face: Arc::new(face), selected: false, drop_target: false }
}

/// 卡片底、边和内边距。选中和放置态用主题混色，不写死 RGB。
fn card_style(mode: DisplayMode, geometry: CardBox, drop_target: bool) -> NodeStyle {
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
    if drop_target {
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

/// 网格、自适应和瀑布流：缩略图在上，标题居中在下；文件有硬链接状态时标题下多一行。
fn tile_content(cell: &CardCell, mode: DisplayMode, item: CardItem, current: &CardView) -> AnyView {
    let face = &current.face;
    let geometry = cell.geometry;
    let mut lines = vec![
        widget(title(face.title.clone(), false))
            .prop::<String, fields::text::value>(move || item.try_with(|card| card.face.title.clone()).unwrap_or_default())
            .key(view_key("file-title", &cell.key))
            .into_any(),
    ];
    if !mode.is_list() && !face.directory() {
        lines.push(
            widget(style::centered(style::small_muted(face.hardlink).truncating()))
                .prop::<String, HiddenWhenEmpty<fields::text::value>>(move || {
                    item.try_with(|card| card.face.hardlink.to_string()).unwrap_or_default()
                })
                .key(format!("{}-0", view_key("file-line", &cell.key)))
                .into_any(),
        );
    }
    let body = widget(Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).justify(JustifySpec::Center))
        .children(lines)
        .into_any();
    // 网格卡片定高 190，内容比它矮。Vue 的按钮把内容竖直居中，这里同样居中。
    let mut column = Stack::column(8.0).align(AlignSpec::Center).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0));
    if geometry.height > 0.0 {
        column = column.height(LengthSpec::Fill).justify(JustifySpec::Center);
    }
    let look = move || item.try_with(|card| card.face.preview.clone()).unwrap_or_default();
    let preview = preview_box(look, Some(geometry.preview_width), geometry.preview_height, 24.0, RadiusTier::Md, &cell.key);
    widget(column).children((preview, body)).into_any()
}

/// 列表：56 方缩略图、标题和路径、右对齐的类型 / 大小 / 硬链接 / 修改时间。
///
/// 三栏宽度按 `56px | minmax(0, 1.5fr) | minmax(220px, 1fr)` 分：标题列和元信息列 3:2 分剩余宽，
/// 元信息列不足 220 时取 220，其余归标题列。
///
/// 卡片窄到放不下 220 的元信息列时，Vue 的网格把标题列压成 0、内容溢出卡片；这里元信息列的下限
/// 改成 `min(220, 卡片内容宽 - 缩略图 - 两个间距 - 80)`，标题至少留 80 宽，元信息在自己的列里换行或截断。
fn list_content(cell: &CardCell, item: CardItem, current: &CardView) -> AnyView {
    let face = &current.face;
    let geometry = cell.geometry;
    let body = widget(
        Stack::column(4.0)
            .grow(1.5)
            .shrink(1.0)
            .min_width(LengthSpec::Px(0.0))
            .width(LengthSpec::Px(0.0))
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((
        widget(title(face.title.clone(), true))
            .prop::<String, fields::text::value>(move || item.try_with(|card| card.face.title.clone()).unwrap_or_default())
            .key(view_key("file-title", &cell.key)),
        widget(style::small_muted(cell.path.to_string()).truncating()).key(view_key("file-path", &cell.key)),
    ))
    .into_any();
    let meta_key = view_key("file-meta", &cell.key);
    let mut meta = vec![
        meta_text(face.kind_label.clone(), &format!("{meta_key}-0"))
            .prop::<String, fields::text::value>(move || item.try_with(|card| card.face.kind_label.clone()).unwrap_or_default())
            .into_any(),
        meta_text(face.size.clone(), &format!("{meta_key}-1"))
            .prop::<String, fields::text::value>(move || item.try_with(|card| card.face.size.clone()).unwrap_or_default())
            .into_any(),
    ];
    if !face.directory() {
        meta.push(
            meta_text(face.hardlink.to_string(), &format!("{meta_key}-2"))
                .prop::<String, HiddenWhenEmpty<fields::text::value>>(move || {
                    item.try_with(|card| card.face.hardlink.to_string()).unwrap_or_default()
                })
                .into_any(),
        );
    }
    meta.push(
        meta_text(face.modified.clone(), &format!("{meta_key}-3"))
            .prop::<String, fields::text::value>(move || item.try_with(|card| card.face.modified.clone()).unwrap_or_default())
            .into_any(),
    );
    let meta = widget(
        Stack::row(12.0)
            .wrap(true)
            .justify(JustifySpec::End)
            .align(AlignSpec::Center)
            .grow(1.0)
            .shrink(1.0)
            .min_width(LengthSpec::Min2(
                LengthAtom::Px(LIST_META_MIN),
                LengthAtom::CalcPercent { percent: 100.0, offset_px: -(geometry.preview_width + 12.0 * 2.0 + LIST_TITLE_MIN) },
            ))
            .width(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                layout.flex_basis = Some(LengthSpec::Px(0.0));
                layout.row_gap = Some(LengthSpec::Px(6.0));
            }),
    )
    .children(meta)
    .into_any();
    let look = move || item.try_with(|card| card.face.preview.clone()).unwrap_or_default();
    let preview = preview_box(look, Some(geometry.preview_width), geometry.preview_height, 24.0, RadiusTier::Sm, &cell.key);
    widget(Stack::row(12.0).align(AlignSpec::Start).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((preview, body, meta))
        .into_any()
}

/// 列表右侧的一段：一行放不下时每段可以缩到 0，用省略号截断，不伸出元信息列。
fn meta_text(text: String, key: &str) -> nana_ui::runtime::view::El<Text> {
    let mut node = style::small_muted(text).truncating();
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.flex_shrink = Some(1.0);
    widget(node).key(key.to_string())
}

/// 卡片标题：14px/700，去掉扩展名，最多两行，网格里居中并至少占两行高。
fn title(label: String, list: bool) -> Text {
    let mut node = style::text(label, 14.0, 700, SemanticColorRole::Text, 14.0 * 1.35);
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

/// 缩略图盒：已有纹理用 cover 铺满；只有像素时自绘 cover；都没有时按类型画白色图标。
///
/// 盒子的自绘和纹理节点都绑在 `look` 上：缩略图到达时只换这两样，盒子不重建。没有纹理时纹理节点
/// 藏着、不占布局。`width` 为空时撑满所在列宽（详情卡片用）。
pub(super) fn preview_box<F>(look: F, width: Option<f32>, height: f32, icon_size: f32, radius: RadiusTier, key: &str) -> AnyView
where
    F: Fn() -> PreviewLook + Clone + Send + 'static,
{
    let current = untrack(&look);
    let frame_look = look.clone();
    let thumb_look = look.clone();
    let resource_look = look.clone();
    let mut thumb = Thumbnail::new(current.resource()).fit(ContentFit::Cover).aspect(current.aspect);
    {
        let layout = Arc::make_mut(&mut thumb.style.layout);
        layout.width = Some(width.map_or(LengthSpec::Fill, LengthSpec::Px));
        layout.height = Some(LengthSpec::Px(height));
        thumb.style.radius = Some(radius);
    }
    let thumb = widget(thumb)
        .visible(move || thumb_look().texture.is_some())
        .prop::<Arc<str>, fields::thumbnail::resource>(move || resource_look().resource())
        .prop::<f32, fields::thumbnail::aspect>(move || look().aspect);
    let frame = Stack::column(0.0).style(preview_frame_style(&current, width, height, icon_size, radius));
    widget(frame)
        .prop::<NodeStyle, StyleField>(move || preview_frame_style(&frame_look(), width, height, icon_size, radius))
        .children((thumb,))
        .key(format!("file-preview-{}", style::key_part(key)))
        .into_any()
}

/// 缩略图盒的样式：固定宽高（或撑满列宽）、底色和图标的自绘、裁掉溢出。
fn preview_frame_style(look: &PreviewLook, width: Option<f32>, height: f32, icon_size: f32, radius: RadiusTier) -> NodeStyle {
    let painter = PreviewPainter { tone: look.tone, icon: look.icon, icon_size, radius, image: look.image.clone() };
    let frame = match width {
        Some(width) => style::fixed(Stack::column(0.0), width, height),
        None => Stack::column(0.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .height(LengthSpec::Px(height))
            .min_height(LengthSpec::Px(height)),
    };
    frame
        .painter(painter)
        .with_layout(|layout| {
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        })
        .node_style()
}

/// 内置预览分派，判断卡片类型图标用。只建一次。
pub(super) fn builtin_bindings() -> &'static [PreviewBinding] {
    static BINDINGS: OnceLock<Vec<PreviewBinding>> = OnceLock::new();
    BINDINGS.get_or_init(super::super::inspect::native_preview::builtin_bindings)
}

/// Vue 的类型图标：文件夹、视频、音频、有预览插件的文件，其余是图片图标。
pub(super) fn entry_icon(row: &FileRow, bindings: &[PreviewBinding]) -> Icon {
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
        && !matches!(super::super::inspect::support::classify(&extension, bindings), super::super::inspect::PreviewKind::Unsupported);
    if previewable { FILE } else { PHOTO }
}

/// 组装键不能带路径分隔符。`notes/page.pdf` 里的 `/` 会让整行挂载失败。
pub(super) fn view_key(prefix: &str, key: &str) -> String {
    format!("{prefix}-{}", style::key_part(key))
}

/// 行上的隐藏路径。命中卡片后沿父节点找到它，实况指针才能对上条目。
fn path_marker(kind: &str, path: &str) -> AnyView {
    let mut marker = Text::new(format!("momobako-entry:{kind}:{path}"));
    let layout = Arc::make_mut(&mut marker.style.layout);
    layout.hidden = true;
    layout.height = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Px(0.0));
    widget(marker).into_any()
}

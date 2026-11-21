//! 文件页工作区：文件卡片、播放条和详情卡片。
//!
//! 左列竖排文件卡片和播放条（间距 `PageTight`），
//! 右列是 300px 宽的详情卡片。双击或「预览」打开预览页时，整块换成预览页，播放条在预览页底部。
//! 外边距由壳层主区统一给，这里只负责两列本身；面板固定高度，列表和详情各自滚动。
//!
//! 常驻：文件列只建一次。工作台和预览页都留着，按「是否在预览」`.visible` 显隐；排法（窄窗口上下排）
//! 和外部文件拖入的描边按字段绑定，卡片圆角是主题的 2xl 档，跟着设置里的圆角半径变。播放条是播放器
//! 模块的旧视图，这里只在工作台左列底下和预览页底部各留一个占位节点，内容由主区块按岛刷新（见
//! `route_files.rs`）；文件对话框和右键菜单在浮层里。外部文件拖放的处理器只发意图消息，当时能不能放、
//! 放到哪由归约按当时的 ViewModel 决定。

use nana_ui::runtime::view::{AnyView, IntoView, NodeRef, Signal, node_ref, signal, widget};
use nana_ui::runtime::{
    AlignSpec, FileDropEvent, LengthSpec, NodeStyle, PositionSpec, RadiusTier, SemanticColorRole,
    Stack,
};

use super::hot::HotSignals;
use super::inspect_metadata_view::MetadataSignals;
use super::inspect_view::PreviewSignals;
use super::{ShellMessage, ShellPage, ShellViewModel};

pub(super) mod bind;
mod grid;
mod header;
pub(super) mod style;
pub(crate) use grid::load_more_if_sentinel_near;
#[cfg(test)]
pub(crate) use grid::reset_row_click_state;
mod cards;
mod virtual_rows;
pub(crate) use board::estimated_list_width;
pub(crate) use virtual_rows::{RowSlot, row_slots};
mod board;
mod detail;
mod menu;
#[cfg(any(test, feature = "acceptance"))]
pub(crate) use menu::EntryMenuView;
#[cfg(test)]
pub(crate) use menu::open_and_reveal_targets;

use crate::theme_map::tiers;
use bind::StyleField;
use nana_ui_core::{SpacingStep, TextWeight};

/// 详情卡片宽。
const DETAIL_WIDTH: f32 = 300.0;
/// 两列之间、文件卡片和播放条之间的间距档。
const DETAIL_GAP_STEP: SpacingStep = SpacingStep::PageTight;
/// 两列之间的间距（默认主题），估算列表宽度用。
const DETAIL_GAP: f32 = tiers::space_px(DETAIL_GAP_STEP);
/// 窗口窄于 900 时详情排到文件卡片下面。
const STACKED_VIEWPORT_PX: f32 = 900.0;

/// 文件右键菜单。没有目标时不占浮层。
pub(super) fn entry_menu(model: &ShellViewModel) -> Option<AnyView> {
    menu::entry_menu(model)
}

/// 右键菜单所属的文件条目卡片的键；菜单没开或条目已经不在列表里时为 `None`。
pub(super) fn entry_menu_card_key(model: &ShellViewModel) -> Option<String> {
    let path = &model.files.entry_menu.as_ref()?.path;
    let ctx = super::files::FileContext::from_model(model);
    let row = model
        .files
        .visible_rows(&ctx)
        .into_iter()
        .find(|row| &row.path == path)?;
    Some(cards::view_key("file-row", &row.key()))
}

/// 文件列的样子：在不在预览、排法和外部文件拖入。
#[derive(Clone, Copy, Debug, PartialEq)]
struct ColumnLook {
    previewing: bool,
    stacked: bool,
    dragging: bool,
}

impl ColumnLook {
    fn of(model: &ShellViewModel) -> Self {
        Self {
            previewing: previewing(model),
            stacked: model.viewport_width <= STACKED_VIEWPORT_PX,
            dragging: model.input.dragging_files,
        }
    }
}

/// 预览页替换文件列表：已选文件、而且是双击或「预览」打开的（单击只在右侧详情里看）。
pub(crate) fn previewing(model: &ShellViewModel) -> bool {
    model.page == ShellPage::SelectedFile
        && model
            .files
            .preview_open(model.inspect.target_path.as_deref())
}

/// 文件列里旧视图岛的占位节点，由文件路由登记。
#[derive(Clone, Copy)]
pub(super) struct ColumnSlots {
    /// 工作台左列底下的播放条。
    pub player: NodeRef,
    /// 预览页底部的播放条。
    pub preview_bar: NodeRef,
}

impl ColumnSlots {
    pub(super) fn new() -> Self {
        Self {
            player: node_ref(),
            preview_bar: node_ref(),
        }
    }
}

/// 文件列的常驻信号。
#[derive(Clone, Copy)]
pub(super) struct ColumnSignals {
    look: Signal<ColumnLook>,
    /// 工作台左列底下有播放条。
    player: Signal<bool>,
    header: header::HeaderSignals,
    list: grid::ListSignals,
    detail: detail::DetailSignals,
}

impl ColumnSignals {
    /// 在常驻作用域里建信号，初值是这一刻的投影。
    pub(super) fn new(model: &ShellViewModel) -> Self {
        Self {
            look: signal(ColumnLook::of(model)),
            player: signal(model.player_surface_visible()),
            header: header::HeaderSignals::new(model),
            list: grid::ListSignals::new(model),
            detail: detail::DetailSignals::new(model),
        }
    }

    /// 写入投影，只写变了的。
    pub(super) fn write(&self, model: &ShellViewModel) {
        self.look.try_set_if_changed(ColumnLook::of(model));
        self.player
            .try_set_if_changed(model.player_surface_visible());
        self.header.write(model);
        self.list.write(model);
        self.detail.write(model);
    }
}

/// 文件面板主体：工作台和预览页二选一显示。整列都是外部文件的拖放目标。
pub(super) fn file_column(
    signals: ColumnSignals,
    slots: ColumnSlots,
    metadata: MetadataSignals,
    preview: PreviewSignals,
    hot: HotSignals,
) -> AnyView {
    let look = signals.look;
    let drop = node_ref();
    super::input::accept_file_drops(drop);
    let workbench = widget(fill())
        .visible(move || look.with(|look| !look.previewing))
        .children((workbench(signals, slots.player, metadata, hot),))
        .key("file-workbench");
    let preview_page = widget(fill())
        .visible(move || look.with(|look| look.previewing))
        .children((super::inspect_view::preview_page(
            preview,
            metadata,
            slots.preview_bar,
        ),))
        .key("file-preview-page");
    widget(fill())
        .node_ref(drop)
        .on_cx(|_, event: &FileDropEvent, cx| {
            cx.dispatch_program_all(file_message(super::files::FilesMessage::HostDrop(
                event.clone(),
            )))
        })
        .children((workbench, preview_page))
        .key("file-column")
        .into_any()
}

/// 铺满父级、可以收窄的一列。
fn fill() -> Stack {
    Stack::fill_column(0.0)
        .min_width(LengthSpec::Px(0.0))
        .min_height(LengthSpec::Px(0.0))
}

/// 工作台：文件卡片列和详情卡片列。窄窗口里详情排到下面。
fn workbench(
    signals: ColumnSignals,
    player: NodeRef,
    metadata: MetadataSignals,
    hot: HotSignals,
) -> AnyView {
    let look = signals.look;
    let shown = signals.player;
    let main = widget(
        Stack::fill_column(0.0)
            .gap_theme(DETAIL_GAP_STEP)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((
        browser_card(signals, hot),
        // 播放条不随内容伸缩，宽度跟着左列。内容是播放条的旧视图岛。
        widget(
            Stack::column(0.0)
                .grow(0.0)
                .shrink(0.0)
                .width(LengthSpec::Fill)
                .min_width(LengthSpec::Px(0.0)),
        )
        .visible(shown)
        .node_ref(player)
        .key("file-player-slot"),
    ))
    .key("file-workbench-main")
    .into_any();
    let initial = look.get_untracked();
    widget(Stack::column(0.0).style(workbench_style(initial.stacked)))
        .prop::<NodeStyle, StyleField>(move || workbench_style(look.with(|look| look.stacked)))
        .children((main, detail::detail_aside(signals.detail, metadata)))
        .key("file-browser")
        .into_any()
}

fn workbench_style(stacked: bool) -> NodeStyle {
    let frame = if stacked {
        Stack::fill_column(0.0).gap_theme(DETAIL_GAP_STEP)
    } else {
        Stack::fill_row(0.0)
            .gap_theme(DETAIL_GAP_STEP)
            .align(AlignSpec::Stretch)
    };
    frame
        .min_width(LengthSpec::Px(0.0))
        .min_height(LengthSpec::Px(0.0))
        .node_style()
}

/// 文件卡片：抬升底色、`--radius-2xl` 的大卡片。外部文件拖进来时描强调色发丝边，
/// 平时四边先留一根发丝边宽的内边距。
///
/// 卡片自己不裁内容：裁的话导入菜单也会被裁掉，窗口矮时下面几项点不到。
/// 列表的滚动区自己裁内容，留在卡片里的内容都在圆角以内，导入菜单可以伸出卡片。
fn browser_card(signals: ColumnSignals, hot: HotSignals) -> AnyView {
    let look = signals.look;
    let initial = look.get_untracked();
    widget(Stack::column(0.0).style(card_style(initial)))
        .prop::<NodeStyle, StyleField>(move || card_style(look.get()))
        .children((
            header::header(signals.header),
            grid::body(signals.list, hot),
            drop_hint(look),
        ))
        .key("file-browser-card")
        .into_any()
}

fn card_style(look: ColumnLook) -> NodeStyle {
    let mut card = Stack::fill_column(0.0)
        .min_width(LengthSpec::Px(0.0))
        .min_height(LengthSpec::Px(0.0))
        .with_layout(style::hairline_padding)
        .surface(SemanticColorRole::Surface)
        .radius(RadiusTier::Xxl)
        .with_layout(|layout| layout.position = PositionSpec::Relative);
    if look.dragging {
        card = style::hairline(card, SemanticColorRole::Accent);
    }
    card.node_style()
}

/// 拖入提示：四周内缩 `Xl` 的虚线框，中间写「拖放到此处添加」。拖入时才显示。
fn drop_hint(look: Signal<ColumnLook>) -> AnyView {
    let frame = style::hairline(
        Stack::column(0.0)
            .align(AlignSpec::Center)
            .justify(nana_ui::runtime::JustifySpec::Center),
        SemanticColorRole::Accent,
    )
    .radius(RadiusTier::Xl)
    .with_layout(|layout| {
        layout.position = PositionSpec::Absolute;
        style::inset(layout, SpacingStep::Xl);
        layout.z_index = Some(2);
        layout.border_style = Some(nana_ui_core::BorderStyle::Dashed);
        layout.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
    });
    let mut style = frame.node_style();
    style.interaction.base.background_mix = Some(nana_ui_core::SemanticColorMix::alpha(
        SemanticColorRole::Surface,
        0.78,
    ));
    widget(frame.style(style))
        .visible(move || look.with(|look| look.dragging))
        .children((widget(style::text(
            "拖放到此处添加",
            14.0,
            TextWeight::Bold,
            SemanticColorRole::Accent,
            21.7,
        ))
        .key("file-drop-hint"),))
        .key("file-drop-overlay")
        .into_any()
}

/// 壳层消息的简写。
pub(super) fn file_message(message: super::files::FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

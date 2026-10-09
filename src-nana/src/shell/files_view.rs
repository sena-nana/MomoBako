//! 文件页工作区，对应 Vue `WorkspaceFilesSurface.vue` 和 `FileBrowserPanel.vue`。
//!
//! 版式和 `.files-workbench` 一致：左列 `.files-workbench__main` 竖排文件卡片和播放条（间距 18），
//! 右列是 300px 宽的详情卡片。双击或「预览」打开预览页时，整块换成预览页，播放条在预览页底部。
//! 外边距由壳层主区统一给，这里只负责两列本身；面板固定高度，列表和详情各自滚动。
//!
//! 常驻：文件列只建一次。工作台和预览页都留着，按「是否在预览」`.visible` 显隐；排法（窄窗口上下排）、
//! 卡片圆角和外部文件拖入的描边按字段绑定。播放条、关闭确认和文件对话框是别的模块的旧视图，
//! 这里只留占位节点，内容由主区块按岛刷新（见 `route_files.rs`）。外部文件拖放的处理器只发意图消息，
//! 当时能不能放、放到哪由归约按当时的 ViewModel 决定。

use nana_ui::runtime::view::{node_ref, signal, widget, AnyView, IntoView, NodeRef, Signal};
use nana_ui::runtime::{AlignSpec, FileDropEvent, LengthSpec, NodeStyle, PositionSpec, SemanticColorRole, Stack};

use super::hot::HotSignals;
use super::inspect_metadata_view::MetadataSignals;
use super::inspect_view::PreviewSignals;
use super::{ShellMessage, ShellPage, ShellViewModel};

#[path = "files_style.rs"]
pub(super) mod style;
#[path = "files_bind.rs"]
pub(super) mod bind;
#[path = "files_header.rs"]
mod header;
#[path = "files_grid.rs"]
mod grid;
#[path = "files_cards.rs"]
mod cards;
#[path = "files_virtual.rs"]
mod virtual_rows;
#[path = "files_board.rs"]
mod board;
#[path = "files_detail.rs"]
mod detail;
#[path = "files_menu.rs"]
mod menu;

use bind::StyleField;

/// Vue `--workspace-file-detail-width`。
const DETAIL_WIDTH: f32 = 300.0;
/// Vue `--workspace-file-detail-gap`：两列之间、文件卡片和播放条之间都是 18。
const DETAIL_GAP: f32 = 18.0;
/// Vue `@media (max-width: 900px)`：窗口再窄时详情排到文件卡片下面。
const STACKED_VIEWPORT_PX: f32 = 900.0;

/// 文件右键菜单。没有目标时不占浮层。
pub(super) fn entry_menu(model: &ShellViewModel) -> Option<AnyView> {
    menu::entry_menu(model)
}

/// 文件列的样子：在不在预览、排法、卡片圆角和外部文件拖入。
#[derive(Clone, Copy, Debug, PartialEq)]
struct ColumnLook {
    previewing: bool,
    stacked: bool,
    radius: f32,
    dragging: bool,
}

impl ColumnLook {
    fn of(model: &ShellViewModel) -> Self {
        Self {
            previewing: previewing(model),
            stacked: model.viewport_width <= STACKED_VIEWPORT_PX,
            radius: card_radius(model),
            dragging: model.input.dragging_files,
        }
    }
}

/// 预览页替换文件列表：已选文件、而且是双击或「预览」打开的（单击只在右侧详情里看）。
pub(super) fn previewing(model: &ShellViewModel) -> bool {
    model.page == ShellPage::SelectedFile && model.files.preview_open(model.inspect.target_path.as_deref())
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
        Self { player: node_ref(), preview_bar: node_ref() }
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
        self.player.try_set_if_changed(model.player_surface_visible());
        self.header.write(model);
        self.list.write(model);
        self.detail.write(model);
    }
}

/// 文件面板主体：工作台和预览页二选一显示，对话框盖在整个窗口上。整列都是外部文件的拖放目标。
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
        .children((super::inspect_view::preview_page(preview, metadata, slots.preview_bar),))
        .key("file-preview-page");
    widget(fill())
        .node_ref(drop)
        .on_cx(|_, event: &FileDropEvent, cx| cx.dispatch_program_all(file_message(super::files::FilesMessage::HostDrop(event.clone()))))
        .children((workbench, preview_page))
        .key("file-column")
        .into_any()
}

/// 铺满父级、可以收窄的一列。
fn fill() -> Stack {
    Stack::fill_column(0.0).min_width(LengthSpec::Px(0.0)).min_height(LengthSpec::Px(0.0))
}

/// `.files-workbench`：文件卡片列和详情卡片列。窄窗口里详情排到下面。
fn workbench(signals: ColumnSignals, player: NodeRef, metadata: MetadataSignals, hot: HotSignals) -> AnyView {
    let look = signals.look;
    let shown = signals.player;
    let main = widget(
        Stack::fill_column(DETAIL_GAP)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((
        browser_card(signals, hot),
        // 播放条不随内容伸缩，宽度跟着左列。内容是播放条的旧视图岛。
        widget(Stack::column(0.0).grow(0.0).shrink(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
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
        Stack::fill_column(DETAIL_GAP)
    } else {
        Stack::fill_row(DETAIL_GAP).align(AlignSpec::Stretch)
    };
    frame.min_width(LengthSpec::Px(0.0)).min_height(LengthSpec::Px(0.0)).node_style()
}

/// `.files-browser`：抬升底色的大卡片。外部文件拖进来时描强调色边。
///
/// Vue 给卡片 `overflow: hidden` 裁圆角，导入菜单也一起被裁，窗口矮时下面几项点不到。
/// 这里卡片不裁：列表的滚动区自己裁内容，留在卡片里的内容都在圆角以内，导入菜单可以伸出卡片。
fn browser_card(signals: ColumnSignals, hot: HotSignals) -> AnyView {
    let look = signals.look;
    let initial = look.get_untracked();
    widget(Stack::column(0.0).style(card_style(initial)))
        .prop::<NodeStyle, StyleField>(move || card_style(look.get()))
        .children((header::header(signals.header), grid::body(signals.list, hot), drop_hint(look)))
        .key("file-browser-card")
        .into_any()
}

fn card_style(look: ColumnLook) -> NodeStyle {
    let mut card = Stack::fill_column(0.0)
        .min_width(LengthSpec::Px(0.0))
        .min_height(LengthSpec::Px(0.0))
        .padding(1.0)
        .surface(SemanticColorRole::Surface)
        .radius_px(look.radius)
        .with_layout(|layout| layout.position = PositionSpec::Relative);
    if look.dragging {
        card = card.outline(SemanticColorRole::Accent, 1.0);
    }
    card.node_style()
}

/// `.files-browser.is-dragging::after`：四周内缩 12 的虚线框，中间写「拖放到此处添加」。拖入时才显示。
fn drop_hint(look: Signal<ColumnLook>) -> AnyView {
    let frame = Stack::column(0.0)
        .align(AlignSpec::Center)
        .justify(nana_ui::runtime::JustifySpec::Center)
        .outline(SemanticColorRole::Accent, 1.0)
        .radius(nana_ui::runtime::RadiusTier::Xl)
        .with_layout(|layout| {
            layout.position = PositionSpec::Absolute;
            layout.offset_top = Some(LengthSpec::Px(12.0));
            layout.offset_left = Some(LengthSpec::Px(12.0));
            layout.offset_right = Some(LengthSpec::Px(12.0));
            layout.offset_bottom = Some(LengthSpec::Px(12.0));
            layout.z_index = Some(2);
            layout.border_style = Some(nana_ui_core::BorderStyle::Dashed);
            layout.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
        });
    let mut style = frame.node_style();
    style.interaction.base.background_mix = Some(nana_ui_core::SemanticColorMix::alpha(SemanticColorRole::Surface, 0.78));
    widget(frame.style(style))
        .visible(move || look.with(|look| look.dragging))
        .children((widget(style::text("拖放到此处添加", 14.0, 700, SemanticColorRole::Accent, 21.7)).key("file-drop-hint"),))
        .key("file-drop-overlay")
        .into_any()
}

/// Vue `--radius-2xl`：圆角基数的两倍，跟着设置里的圆角半径走。
pub(super) fn card_radius(model: &ShellViewModel) -> f32 {
    crate::theme_map::radius_2xl(model.admin.corner_radius as f32)
}

/// 壳层消息的简写。
pub(super) fn file_message(message: super::files::FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

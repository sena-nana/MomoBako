//! 文件页工作区，对应 Vue `WorkspaceFilesSurface.vue` 和 `FileBrowserPanel.vue`。
//!
//! 版式和 `.files-workbench` 一致：左列 `.files-workbench__main` 竖排文件卡片和播放条（间距 18），
//! 右列是 300px 宽的详情卡片。双击或「预览」打开预览页时，整块换成预览和播放条。
//! 外边距由壳层主区统一给，这里只负责两列本身；面板固定高度，列表和详情各自滚动。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{AlignSpec, FileDropEvent, LengthSpec, PositionSpec, SemanticColorRole, Stack};

use super::{ShellMessage, ShellPage, ShellViewModel};

#[path = "files_style.rs"]
pub(super) mod style;
#[path = "files_header.rs"]
mod header;
#[path = "files_grid.rs"]
mod grid;
#[path = "files_cards.rs"]
mod cards;
#[path = "files_detail.rs"]
mod detail;
#[path = "files_menu.rs"]
mod menu;

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

/// 文件面板主体。预览页和目录列表二选一，对话框盖在整个窗口上。
pub(super) fn live_file_column(model: &ShellViewModel) -> AnyView {
    let previewing = model.page == ShellPage::SelectedFile && model.files.preview_open(model.inspect.target_path.as_deref());
    let body = if previewing { preview_page(model) } else { workbench(model) };
    widget(Stack::fill_column(0.0).min_width(LengthSpec::Px(0.0)).min_height(LengthSpec::Px(0.0)))
        .on_cx({
            let flags = super::input::file_drop_flags(model);
            move |_, event: &FileDropEvent, cx| {
                cx.dispatch_program(super::input::file_drop_message(&flags, event));
            }
        })
        .children((
            super::input::drop_marker("files"),
            super::input::close_prompt(model),
            body,
            super::workspace_dialogs::file_dialog(model),
            super::workspace_dialogs::export_dialog(model),
        ))
        .key("file-column")
        .into_any()
}

/// 预览页：预览和元数据在上，播放条固定在下面，不把高度挤出窗口。
fn preview_page(model: &ShellViewModel) -> AnyView {
    let inspect = widget(
        Stack::fill_column(0.0)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((super::inspect_view::inspect_surface(model),))
    .key("file-preview-inspect")
    .into_any();
    let player = model.player_surface_visible().then(|| player_slot(model));
    widget(Stack::fill_column(DETAIL_GAP).min_width(LengthSpec::Px(0.0)).min_height(LengthSpec::Px(0.0)))
        .children((inspect, player))
        .key("file-preview-page")
        .into_any()
}

/// `.files-workbench`：文件卡片列和详情卡片列。
fn workbench(model: &ShellViewModel) -> AnyView {
    let stacked = model.viewport_width <= STACKED_VIEWPORT_PX;
    let player = model.player_surface_visible().then(|| player_slot(model));
    let main = widget(
        Stack::fill_column(DETAIL_GAP)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((browser_card(model), player))
    .key("file-workbench-main")
    .into_any();
    let detail = detail::detail_aside(model, if stacked { None } else { Some(DETAIL_WIDTH) });
    let frame = if stacked {
        Stack::fill_column(DETAIL_GAP)
    } else {
        Stack::fill_row(DETAIL_GAP).align(AlignSpec::Stretch)
    };
    widget(frame.min_width(LengthSpec::Px(0.0)).min_height(LengthSpec::Px(0.0)))
        .children((main, detail))
        .key("file-browser")
        .into_any()
}

/// `.files-browser`：抬升底色的大卡片。外部文件拖进来时描强调色边。
///
/// Vue 给卡片 `overflow: hidden` 裁圆角，导入菜单也一起被裁，窗口矮时下面几项点不到。
/// 这里卡片不裁：列表的滚动区自己裁内容，留在卡片里的内容都在圆角以内，导入菜单可以伸出卡片。
fn browser_card(model: &ShellViewModel) -> AnyView {
    let dragging = model.input.dragging_files;
    let mut card = Stack::fill_column(0.0)
        .min_width(LengthSpec::Px(0.0))
        .min_height(LengthSpec::Px(0.0))
        .padding(1.0)
        .surface(SemanticColorRole::Surface)
        .radius_px(card_radius(model))
        .with_layout(|layout| layout.position = PositionSpec::Relative);
    if dragging {
        card = card.outline(SemanticColorRole::Accent, 1.0);
    }
    widget(card)
        .children((header::header(model), grid::body(model), dragging.then(drop_hint)))
        .key("file-browser-card")
        .into_any()
}

/// `.files-browser.is-dragging::after`：四周内缩 12 的虚线框，中间写「拖放到此处添加」。
fn drop_hint() -> AnyView {
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
        .children((widget(style::text("拖放到此处添加", 14.0, 700, SemanticColorRole::Accent, 21.7)).key("file-drop-hint"),))
        .key("file-drop-overlay")
        .into_any()
}

/// 播放条不随内容伸缩，宽度跟着左列。
fn player_slot(model: &ShellViewModel) -> AnyView {
    widget(Stack::column(0.0).grow(0.0).shrink(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((super::player_view::player_surface(model),))
        .key("file-player-slot")
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

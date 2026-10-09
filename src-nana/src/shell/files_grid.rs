//! 文件卡片的列表区，对应 Vue `.files-list` 和读取、处理、出错三种状态框。
//!
//! 列表纵向滚动，内边距 16 / 20 / 20。文件夹一组、分隔线、文件一组，各段保持自身高度、段间距 14，
//! 内容比列表高时由列表滚动。两组卡片都走虚拟列表（见 `files_virtual.rs`），只建视口附近的行。
//! Vue 把这三段放进 CSS grid 的自动行，内容矮时行被拉伸、内容高时行被压扁让卡片盖到分隔线上，
//! 这两种怪癖都不照抄。

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use nana_ui::icons_tabler::LOADER_2;
use nana_ui::runtime::view::{node_ref, widget, AnyView, IntoView, NodeRef};
use nana_ui::runtime::{
    Activate, AlignSpec, LengthSpec, RadiusTier, ScrollAxes, ScrollView, SemanticColorRole, SizeChanged, Stack,
};

use super::super::files::{FileContext, FileRow, FilesMessage};
use super::super::workspace::{LibraryCategory, WorkspacePanel};
use super::super::ShellViewModel;
use super::cards::CardState;
use super::file_message;
use super::style;
use super::virtual_rows::{self, CardSpec, GAP, MASONRY_COLUMN};

/// 列表区左右内边距。
const LIST_PADDING_X: f32 = 20.0;

thread_local! {
    static LAST_ROW_CLICK: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

/// 400 毫秒内连点同一条算双击，第二次才进入目录或打开预览。
pub(super) fn row_activation(path: &str) -> FilesMessage {
    let open = LAST_ROW_CLICK.with(|slot| {
        let mut slot = slot.borrow_mut();
        let now = Instant::now();
        let open = slot.as_ref().is_some_and(|(previous, at)| previous == path && now.duration_since(*at).as_millis() < 400);
        *slot = Some((path.to_string(), now));
        open
    });
    if open {
        FilesMessage::OpenRow(path.to_string())
    } else {
        FilesMessage::ActivateRow(path.to_string())
    }
}

/// 列表区：出错、读取中、处理中时只显示状态框，否则是分组列表。
pub(super) fn body(model: &ShellViewModel) -> AnyView {
    let files = &model.files;
    if !files.error.is_empty() {
        return state_box(files.error.clone(), true, false, model);
    }
    if files.loading {
        return state_box("正在读取目录".into(), false, true, model);
    }
    if files.mutating {
        return state_box("正在处理文件".into(), false, true, model);
    }
    list(model)
}

/// `.asset-browser__state`：圆角小框，白底描边；出错时是浅红底红字。
fn state_box(label: String, error: bool, spinner: bool, model: &ShellViewModel) -> AnyView {
    let (fill, border, color) = if error {
        (None, SemanticColorRole::Danger, SemanticColorRole::Danger)
    } else {
        (Some(SemanticColorRole::Background), SemanticColorRole::Border, SemanticColorRole::Muted)
    };
    let mut frame = Stack::row(8.0).align(AlignSpec::Center).padding_xy(12.0, 10.0).radius(RadiusTier::Xl).with_layout(|layout| {
        layout.margin_top = Some(LengthSpec::Px(16.0));
        layout.margin_left = Some(LengthSpec::Px(LIST_PADDING_X));
        layout.margin_right = Some(LengthSpec::Px(LIST_PADDING_X));
        layout.align_self = Some(AlignSpec::Start);
    });
    let mut node_style = frame.node_style();
    node_style.background = fill;
    node_style.border = Some(border);
    if error {
        node_style.interaction.base.background_mix = Some(nana_ui_core::SemanticColorMix::alpha(SemanticColorRole::Danger, 0.1));
        node_style.interaction.base.border_mix = Some(nana_ui_core::SemanticColorMix::alpha(SemanticColorRole::Danger, 0.36));
    }
    Arc::make_mut(&mut node_style.layout).border_width = Some(1.0);
    frame = frame.style(node_style);
    let icon = spinner.then(|| {
        let degrees = model.motion.spinner_degrees();
        let mut glyph = nana_ui::runtime::IconGlyph::new(LOADER_2).size(16.0).role(color);
        Arc::make_mut(&mut glyph.style.layout).transform = Some(super::super::motion::spin_transform(degrees));
        // 旋转角逐帧走，绑在热信号上，不重挂文件列。
        widget(glyph)
            .prop::<f32, super::super::hot::SpinField>(super::super::hot::prop(|signals| signals.spinner, degrees))
            .key("file-state-spinner")
    });
    widget(frame)
        .children((icon, widget(style::text(label, 14.0, 400, color, 21.7)).key("file-state-text")))
        .key(if error { "file-error" } else { "file-state" })
        .into_any()
}

/// 分组列表。宽度按实际布局回报；还没回报时按窗口宽估算，保证首帧就是对的列数。
/// 两组卡片都跟着这个滚动区虚拟化，滚动区的引用交给它们。
fn list(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let rows = model.files.visible_rows(&ctx);
    let virtual_view = ctx.is_virtual();
    let (directories, files): (Vec<FileRow>, Vec<FileRow>) = if virtual_view {
        (Vec::new(), rows)
    } else {
        rows.into_iter().partition(|row| row.kind == "directory")
    };
    let mode = model.files.display_mode;
    let width = model.files.list_width.unwrap_or_else(|| estimated_list_width(model));
    let picked = picked_paths(&model.files);
    let drop_folder = (model.input.internal_active || model.input.dragging_files).then(|| model.input.hover_folder.clone()).flatten();
    let specs = |rows: Vec<FileRow>| {
        rows.into_iter()
            .map(|row| {
                let state = CardState {
                    selected: picked.iter().any(|path| path == &row.path),
                    drop_target: row.kind == "directory" && drop_folder.as_deref() == Some(row.path.as_str()),
                };
                CardSpec { row, state }
            })
            .collect::<Vec<_>>()
    };
    let scroll = node_ref();
    let has_directories = !directories.is_empty();
    let has_files = !files.is_empty();
    let mut sections = Vec::new();
    if has_directories {
        let group = virtual_rows::group(specs(directories), mode, width, scroll, "file-virtual-directories");
        sections.push(section(group, "file-group-directories"));
    }
    if has_directories && has_files {
        sections.push(section(divider(), "file-group-divider"));
    }
    if has_files {
        let group = virtual_rows::group(specs(files), mode, width, scroll, "file-virtual-files");
        sections.push(section(group, "file-group-files"));
    }
    if let Some(sentinel) = load_more(model) {
        sections.push(section(sentinel, "file-group-more"));
    }
    scroller(model, sections, scroll)
}

/// 列表滚动区和里面的内容列。内容列回报实际宽度，网格换行和瀑布流列数按它算。
fn scroller(model: &ShellViewModel, sections: Vec<AnyView>, scroll: NodeRef) -> AnyView {
    let content = widget(
        Stack::column(GAP)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .with_layout(|layout| {
                layout.padding_top = Some(LengthSpec::Px(16.0));
                layout.padding_left = Some(LengthSpec::Px(LIST_PADDING_X));
                layout.padding_right = Some(LengthSpec::Px(LIST_PADDING_X));
                layout.padding_bottom = Some(LengthSpec::Px(20.0));
            }),
    )
    .children(sections)
    .key("file-list-content")
    .on_cx(|_, event: &SizeChanged, cx| {
        cx.dispatch_program_all(file_message(FilesMessage::ListResized(event.width - LIST_PADDING_X * 2.0)));
    })
    .into_any();
    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
    }))
    .node_ref(scroll)
    .children((content,))
    .key(list_scroll_key(model))
    .into_any()
}

/// 列表滚动区的键，按显示的内容取：仓库、面板或分类、所在目录或智能文件夹。
///
/// 壳层重挂时按键路径保留滚动位置。进另一个目录、回收站或智能文件夹时键变了，列表从顶部开始；
/// 同一目录里因为选中、缩略图到达而重挂时键不变，位置保持。Vue 换目录时不主动回顶，只靠浏览器
/// 按新内容高度钳住偏移；文件管理器换目录从顶部开始更合理，这里按后者做。
fn list_scroll_key(model: &ShellViewModel) -> String {
    let repo = model.workspace.active_repo_id.as_deref().unwrap_or("none");
    let view = match model.workspace.panel {
        WorkspacePanel::Trash => format!("trash-{}", model.files.current_path),
        WorkspacePanel::SmartFolder => format!("smart-{}", model.sidebar.active_smart_folder_id.as_deref().unwrap_or("")),
        _ => match model.workspace.library_category {
            LibraryCategory::All => format!("files-{}", model.files.current_path),
            category => format!("category-{category:?}"),
        },
    };
    format!("files-scroll-{}-{}", style::key_part(repo), style::key_part(&view))
}

/// 一段：保持自身高度，段间距 14。
fn section(content: AnyView, key: &'static str) -> AnyView {
    let section = Stack::column(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0));
    widget(section).children((content,)).key(key).into_any()
}

/// `.files-list__divider`：1px `--border-strong`，上下各 2px。
fn divider() -> AnyView {
    let line = Stack::column(0.0)
        .width(LengthSpec::Fill)
        .height(LengthSpec::Px(1.0))
        .min_height(LengthSpec::Px(1.0))
        .surface(SemanticColorRole::BorderStrong)
        .with_layout(|layout| {
            layout.margin_top = Some(LengthSpec::Px(2.0));
            layout.margin_bottom = Some(LengthSpec::Px(2.0));
        });
    widget(line).key("file-list-divider").into_any()
}

/// 还没有布局回报时，按窗口宽估算列表内容宽：减去侧栏、主区左右 24、详情 300 和间距 18、
/// 卡片描边和列表左右内边距。窗口不超过 900 时详情排到下面，不再减详情宽。
fn estimated_list_width(model: &ShellViewModel) -> f32 {
    let sidebar = model.motion.sidebar_presented_width();
    let detail = if model.viewport_width <= 900.0 { 0.0 } else { 300.0 + 18.0 };
    (model.viewport_width - sidebar - 48.0 - detail - 2.0 - LIST_PADDING_X * 2.0).max(MASONRY_COLUMN)
}

/// 多选路径加主选。主选不在多选里时也算选中。
fn picked_paths(files: &super::super::files::FilesState) -> Vec<String> {
    let mut paths = files.selected.clone();
    if let Some(primary) = &files.primary {
        if !paths.iter().any(|item| item == primary) {
            paths.push(primary.clone());
        }
    }
    paths
}

/// 还有下一页或正在续读时，跟在卡片后面的「滚动继续加载」。点一下也会续读。
fn load_more(model: &ShellViewModel) -> Option<AnyView> {
    let ctx = FileContext::from_model(model);
    let loading = model.files.loading_more;
    if !model.files.has_more && !loading {
        return None;
    }
    let label = if loading { "继续读取目录..." } else { "滚动继续加载" };
    let mut button = style::styled_button(label, loading.then_some(LOADER_2), style::ButtonLook {
        height: 32.0,
        padding_x: 0.0,
        hover: None,
        weight: 400,
        ..style::ButtonLook::TOOLBAR
    });
    button = button.disabled(!model.files.can_load_more(&ctx) && !loading);
    Some(
        widget(button)
            .key("file-load-more")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(file_message(FilesMessage::LoadMore)))
            .into_any(),
    )
}

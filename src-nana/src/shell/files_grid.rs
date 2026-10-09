//! 文件卡片的列表区，对应 Vue `.files-list` 和读取、处理、出错三种状态框。
//!
//! 列表纵向滚动，内边距 16 / 20 / 20。文件夹一组、分隔线、文件一组，各段保持自身高度、段间距 14，
//! 内容比列表高时由列表滚动。两组卡片都走虚拟列表（见 `files_virtual.rs`），只建视口附近的行。
//! Vue 把这三段放进 CSS grid 的自动行，内容矮时行被拉伸、内容高时行被压扁让卡片盖到分隔线上，
//! 这两种怪癖都不照抄。
//!
//! 常驻：状态框和列表之间、换了显示内容（目录、回收站、分类、智能文件夹）时整块换，列表滚动区的键
//! 带上内容身份，所以换目录回到顶部；同一内容里滚动区一直留着，滚动位置自然保持。段的有无用
//! `.visible`，「滚动继续加载」的字、图标和可点按字段绑定。

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use nana_ui::icons_tabler::LOADER_2;
use nana_ui::runtime::view::{dynamic, fields, node_ref, signal, widget, AnyView, IntoView, Signal};
use nana_ui::runtime::{
    Activate, AlignSpec, LengthSpec, RadiusTier, ScrollAxes, ScrollView, SemanticColorRole, SizeChanged, Stack,
};
use nana_ui_core::Icon;

use super::super::files::{FileContext, FilesMessage};
use super::super::hot::{HotSignals, SpinField};
use super::super::workspace::{LibraryCategory, WorkspacePanel};
use super::super::ShellViewModel;
use super::bind::{fill_container, ButtonIconField};
use super::board::{BoardSignals, GroupPresence};
use super::file_message;
use super::style;
use super::virtual_rows::{self, GAP};

/// 列表区左右内边距。
pub(super) const LIST_PADDING_X: f32 = 20.0;

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

/// 列表区现在显示什么。换了值整块换。
#[derive(Clone, Debug, PartialEq)]
enum ListFrame {
    /// 出错、读取中、处理中时的状态框。
    State { label: String, error: bool, spinner: bool },
    /// 分组列表。`scroll` 是列表滚动区的键，带上显示内容的身份。
    List { scroll: String },
}

/// 列表里各段有没有，以及「滚动继续加载」的样子。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Sections {
    groups: GroupPresence,
    /// 还有下一页或正在续读时显示；`loading` 时是「继续读取目录...」。
    more: Option<LoadMore>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LoadMore {
    loading: bool,
    enabled: bool,
}

/// 列表区的常驻信号：显示什么、各段的有无，以及卡片仓。
#[derive(Clone, Copy)]
pub(super) struct ListSignals {
    frame: Signal<ListFrame>,
    sections: Signal<Sections>,
    board: BoardSignals,
}

impl ListSignals {
    /// 在常驻作用域里建信号，初值是这一刻的投影。
    pub(super) fn new(model: &ShellViewModel) -> Self {
        let board = BoardSignals::new();
        let groups = board.write(model);
        Self { frame: signal(frame(model)), sections: signal(sections(model, groups)), board }
    }

    /// 写入投影，只写变了的。
    pub(super) fn write(&self, model: &ShellViewModel) {
        let groups = self.board.write(model);
        self.frame.try_set_if_changed(frame(model));
        self.sections.try_set_if_changed(sections(model, groups));
    }
}

/// 出错、读取中、处理中时只显示状态框，否则是分组列表。
fn frame(model: &ShellViewModel) -> ListFrame {
    let files = &model.files;
    if !files.error.is_empty() {
        return ListFrame::State { label: files.error.clone(), error: true, spinner: false };
    }
    if files.loading {
        return ListFrame::State { label: "正在读取目录".into(), error: false, spinner: true };
    }
    if files.mutating {
        return ListFrame::State { label: "正在处理文件".into(), error: false, spinner: true };
    }
    ListFrame::List { scroll: list_scroll_key(model) }
}

fn sections(model: &ShellViewModel, groups: GroupPresence) -> Sections {
    let ctx = FileContext::from_model(model);
    let loading = model.files.loading_more;
    let more = (model.files.has_more || loading).then(|| LoadMore { loading, enabled: model.files.can_load_more(&ctx) || loading });
    Sections { groups, more }
}

/// 列表滚动区的键，按显示的内容取：仓库、面板或分类、所在目录或智能文件夹。
///
/// 换目录、回收站或智能文件夹时键变了，列表从顶部开始；同一内容里滚动区常驻，位置保持。
/// Vue 换目录时不主动回顶，只靠浏览器按新内容高度钳住偏移；文件管理器换目录从顶部开始更合理，这里按后者做。
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

/// 列表区：状态框或分组列表，占满卡片头部以下的高度。
pub(super) fn body(signals: ListSignals, hot: HotSignals) -> AnyView {
    dynamic(signals.frame, move |frame: &ListFrame| match frame {
        ListFrame::State { label, error, spinner } => state_box(label.clone(), *error, *spinner, hot),
        ListFrame::List { scroll } => list(scroll, signals),
    })
    .css(fill_container())
    .into_any()
}

/// `.asset-browser__state`：圆角小框，白底描边；出错时是浅红底红字。
fn state_box(label: String, error: bool, spinner: bool, hot: HotSignals) -> AnyView {
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
        let degrees = hot.spinner.get_untracked();
        let mut glyph = nana_ui::runtime::IconGlyph::new(LOADER_2).size(16.0).role(color);
        Arc::make_mut(&mut glyph.style.layout).transform = Some(super::super::motion::spin_transform(degrees));
        // 旋转角逐帧走，绑在热信号上。
        widget(glyph).prop::<f32, SpinField>(hot.spinner).key("file-state-spinner")
    });
    widget(frame)
        .children((icon, widget(style::text(label, 14.0, 400, color, 21.7)).key("file-state-text")))
        .key(if error { "file-error" } else { "file-state" })
        .into_any()
}

/// 分组列表。宽度按实际布局回报；还没回报时按窗口宽估算，保证首帧就是对的列数。
/// 两组卡片都跟着这个滚动区虚拟化，滚动区的引用交给它们。
fn list(scroll_key: &str, signals: ListSignals) -> AnyView {
    let scroll = node_ref();
    let (sections, board) = (signals.sections, signals.board);
    let groups = move || sections.with(|sections| sections.groups);
    let directories = virtual_rows::group_view(board.groups[0].shape, board.groups[0].lines, board.cards, scroll, "file-virtual-directories");
    let files = virtual_rows::group_view(board.groups[1].shape, board.groups[1].lines, board.cards, scroll, "file-virtual-files");
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
    .children((
        section(directories, "file-group-directories", move || groups().directories),
        section(divider(), "file-group-divider", move || groups().directories && groups().files),
        section(files, "file-group-files", move || groups().files),
        section(load_more(sections), "file-group-more", move || sections.with(|sections| sections.more.is_some())),
    ))
    .key("file-list-content")
    .on_cx(|_, event: &SizeChanged, cx| {
        // 预览页打开时工作台藏起来，列表量出来是 0 宽：这不是新宽度，不回报。
        if event.width <= 0.0 {
            return;
        }
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
    .key(scroll_key.to_string())
    .into_any()
}

/// 一段：保持自身高度，段间距 14。没有内容时藏着，不占位也不多一个间距。
fn section(content: AnyView, key: &'static str, shown: impl Fn() -> bool + Send + 'static) -> AnyView {
    let section = Stack::column(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0));
    widget(section).visible(shown).children((content,)).key(key).into_any()
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

/// 还有下一页或正在续读时，跟在卡片后面的「滚动继续加载」。点一下也会续读。
fn load_more(sections: Signal<Sections>) -> AnyView {
    let more = move || sections.with(|sections| sections.more.unwrap_or(LoadMore { loading: false, enabled: false }));
    let current = sections.with_untracked(|sections| sections.more.unwrap_or(LoadMore { loading: false, enabled: false }));
    let look = style::ButtonLook { height: 32.0, padding_x: 0.0, hover: None, weight: 400, ..style::ButtonLook::TOOLBAR };
    let button = style::styled_button(more_label(current.loading), more_icon(current.loading), look).disabled(!current.enabled);
    widget(button)
        .prop::<String, fields::button::label>(move || more_label(more().loading).to_string())
        .prop::<Option<Icon>, ButtonIconField>(move || more_icon(more().loading))
        .prop::<bool, fields::button::disabled>(move || !more().enabled)
        .key("file-load-more")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(file_message(FilesMessage::LoadMore)))
        .into_any()
}

fn more_label(loading: bool) -> &'static str {
    if loading { "继续读取目录..." } else { "滚动继续加载" }
}

fn more_icon(loading: bool) -> Option<Icon> {
    loading.then_some(LOADER_2)
}

//! 关闭确认、系统文件拖放和空库页。关闭确认是浮层槽位里的确认框，走统一对话框框架。

use std::path::PathBuf;
use std::sync::Arc;

use nana_ui::runtime::view::{node_ref, on_mount, signal, widget, AnyView, IntoView, NodeRef};
use nana_ui::runtime::{
    AlignSpec, FileDropEvent, JustifySpec, LengthSpec, RadiusTier, SemanticColorRole, Stack, Text, TextHorizontalAlignment,
};
use nana_ui::ButtonKind;
use nana_ui_core::{DropAccepts, DropEffect};
use super::{HostDragPhase, InputMessage};
use crate::shell::view_part_overlay::dialog::{intent_button, DialogFrame};
use crate::shell::view_part_overlay::session::Projected;
use crate::shell::{MainRegion, ShellMessage, ShellViewModel, WorkspacePanel};

/// 挂上拖放时从壳层抄下来的仓库条件。事件回调里不再借壳层。
#[derive(Clone)]
pub(crate) struct FileDropFlags {
    has_repository: bool,
    missing_repository: bool,
    writable: bool,
    files_panel: bool,
    has_snapshot: bool,
    repo_root: String,
}

pub(crate) fn file_drop_flags(model: &ShellViewModel) -> FileDropFlags {
    let repository = model.workspace.active_repository();
    FileDropFlags {
        has_repository: model.workspace.active_repo_id.is_some(),
        missing_repository: model.navigation_locked(),
        writable: repository.as_ref().is_some_and(|item| {
            crate::shell::files::repository_is_writable(&item.status, &item.capabilities)
        }),
        files_panel: model.workspace.panel == WorkspacePanel::Files,
        has_snapshot: model.workspace.main_region() == MainRegion::HasRepository,
        repo_root: repository.map(|item| item.path.clone()).unwrap_or_default(),
    }
}

/// 把 Nana 的文件拖放收成和 Vue `onDragDropEvent` 一样的宿主拖放消息。
pub(crate) fn file_drop_message(flags: &FileDropFlags, event: &FileDropEvent) -> ShellMessage {
    let phase = match event {
        FileDropEvent::Hovered { .. } => HostDragPhase::Over,
        FileDropEvent::Dropped { .. } => HostDragPhase::Drop,
        FileDropEvent::Left => HostDragPhase::Leave,
    };
    let paths = match event {
        FileDropEvent::Hovered { paths, .. } | FileDropEvent::Dropped { paths, .. } => {
            paths.iter().map(path_text).collect()
        }
        FileDropEvent::Left => Vec::new(),
    };
    ShellMessage::Input(InputMessage::HostDrag {
        phase,
        paths,
        has_repository: flags.has_repository,
        missing_repository: flags.missing_repository,
        writable: flags.writable,
        files_panel: flags.files_panel,
        has_snapshot: flags.has_snapshot,
        repo_root: flags.repo_root.clone(),
    })
}

fn path_text(path: &PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

/// 空库页，对应 `EmptyRepositoryState.vue`：标题、拖入说明和附加失败的错误。
///
/// 整节都是拖放目标，悬停和放下走同一条宿主拖放消息；拖着文件夹经过时面板换成
/// `--accent-soft` 底和 1px 强调色描边。
pub(crate) fn empty_repository_panel(model: &ShellViewModel) -> AnyView {
    let dragging = model.input.dragging_repository_folder;
    let error = &model.input.empty_repository_error;
    let mut rows = vec![
        widget(centered_text("还没有可用资源库", 18.0, 600, SemanticColorRole::Text, 18.0 * 1.55)).key("empty-title").into_any(),
        widget(centered_text("拖入一个本地文件夹创建资源库。", 13.0, 400, SemanticColorRole::Muted, 13.0 * 1.55))
            .key("empty-detail")
            .into_any(),
    ];
    if !error.is_empty() {
        let mut copy = centered_text(error.clone(), 13.0, 400, SemanticColorRole::Danger, 13.0 * 1.45);
        Arc::make_mut(&mut copy.style.layout).width = Some(LengthSpec::Fill);
        rows.push(
            widget(
                Stack::column(0.0)
                    .width(LengthSpec::Fill)
                    .padding_xy(10.0, 8.0)
                    .painter(crate::shell::shell_tint::SoftFill::err())
                    .with_layout(|layout| layout.max_width = Some(LengthSpec::Px(420.0))),
            )
            .children((widget(copy).key("empty-error"),))
            .into_any(),
        );
    }
    let mut panel = Stack::column(10.0)
        .width(LengthSpec::Fill)
        .min_height(LengthSpec::Px(220.0))
        .padding_xy(16.0, 28.0)
        .radius(RadiusTier::Md)
        .align(AlignSpec::Center)
        .justify(JustifySpec::Center)
        .with_layout(|layout| layout.max_width = Some(LengthSpec::Px(520.0)));
    if dragging {
        panel = panel.surface(SemanticColorRole::AccentSoft).outline(SemanticColorRole::Accent, 1.0);
    }
    let drop = node_ref();
    accept_file_drops(drop);
    widget(crate::shell::startup_view::fill_section())
        .node_ref(drop)
        .on_cx({
            let flags = file_drop_flags(model);
            move |_, event: &FileDropEvent, cx| {
                cx.dispatch_program_all(file_drop_message(&flags, event));
            }
        })
        .children((widget(panel).children(rows).key("empty-panel").into_any(),))
        .key("empty-repository-page")
        .into_any()
}

/// 居中的一段字，行高写成像素。对应 `.empty-state-page` 的 `text-align: center`。
fn centered_text(value: impl Into<String>, size: f32, weight: u16, color: SemanticColorRole, line_height: f32) -> Text {
    let mut node = Text::new(value).font_size(size).font_weight(weight).color(color).line_height(line_height);
    node.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    node
}

/// `target` 建好后登记成系统文件拖放目标：接受文件、复制效果，整块子树都算。事件由视图的
/// `FileDropEvent` 处理器收成 `HostDrag`。在建 `target` 的那段视图里调用，跟着它的作用域走。
pub(crate) fn accept_file_drops(target: NodeRef) {
    on_mount(move |cx| {
        let Some(id) = target.get_untracked() else {
            eprintln!("Nana 文件拖放目标没有建出来，不登记拖放");
            return;
        };
        if let Err(error) = cx.set_drop_target_node(id, DropAccepts::files().effect(DropEffect::Copy)) {
            eprintln!("Nana 文件拖放目标没有挂上：{error}");
        }
    });
}

/// 关闭确认要显示的说明。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CloseConfirmView {
    pub notice: String,
}

impl CloseConfirmView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        if !model.input.pending_close {
            return None;
        }
        let notice = if model.input.notice.is_empty() { "确认关闭 MomoBako？".to_string() } else { model.input.notice.clone() };
        Some(Self { notice })
    }
}

fn answer(accept: bool) -> ShellMessage {
    ShellMessage::Input(InputMessage::ConfirmCloseAnswer(accept))
}

/// 有待确认的关闭时用确认框，放在浮层槽位最上面，停在哪个页面都看得到。取消和三种关闭手势
/// 都是「不关」，「确认关闭」才关窗口。
pub(crate) fn close_prompt(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(CloseConfirmView::project(model)?);
    Projected::register(view, CloseConfirmView::project);
    let cancel = intent_button("取消", ButtonKind::Ghost, false, "close-confirm-cancel");
    let confirm = intent_button("确认关闭", ButtonKind::Primary, false, "close-confirm-accept");
    Some(DialogFrame::new("close-confirm", || "关闭 MomoBako".to_string(), || answer(false)).confirm(
        move || view.with(|view| view.notice.clone()),
        cancel,
        confirm,
        || answer(true),
    ))
}

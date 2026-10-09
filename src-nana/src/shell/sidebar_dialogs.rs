//! 侧栏发起的对话框：新建 / 重命名文件夹、处理文件夹、删除智能文件夹、新建播放集和删除资源库。
//! 智能文件夹的编辑对话框字段多，在 `sidebar_smart_dialog.rs`。
//!
//! 文案、字段和按钮照 Vue `WorkspaceSidebarFolderDialogs.vue`、`WorkspaceSidebarSmartFolderDialogs.vue`、
//! `WorkspaceSidebarPlaylistDialog.vue` 和 `RepositoryDeleteDialog.vue`。外壳都走统一的对话框框架
//! （`overlay_dialog.rs`）：处理中三种关闭手势都不关，危险对话框标题用错误色。
//!
//! 每个对话框常驻：打开时按投影建一次，之后浮层同步只把新的投影写进信号，文字、禁用和错误这些
//! 字段原地改；输入框用 `.model` 加草稿信号受控。按钮和选项卡片的事件只发意图消息。

use std::sync::Arc;

use nana_ui::runtime::view::{signal, widget, AnyView, IntoProp, IntoView, Signal};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, ListItem, RadiusTier, SemanticColorRole, Stack};
use nana_ui::ButtonKind;

use super::super::sidebar::{FolderDeleteMode, GapMessage};
use super::super::view_part_overlay::dialog::{
    action, busy_note, error_line, footer, paragraph, select_field, text_field, wrapping_text, Choices, DialogFrame,
    DimmedDisabled,
};
use super::super::view_part_overlay::session::{Draft, Projected};
use super::super::workspace::{DeleteMode, WorkspaceDialog};
use super::super::{ShellMessage, ShellViewModel, SidebarMessage};
use super::parts;
use super::sidebar_message;

#[path = "sidebar_smart_dialog.rs"]
mod smart;
pub use smart::smart_folder_dialog;

fn gap(message: GapMessage) -> ShellMessage {
    sidebar_message(SidebarMessage::Gap(message))
}

/// 新建或重命名文件夹要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FolderDialogView {
    pub title: &'static str,
    pub summary: String,
    pub placeholder: &'static str,
    pub action: &'static str,
    pub error: String,
    /// 文件服务忙或已提交、等结果：不能关、不能再提交。
    pub busy: bool,
    /// 主按钮不可用：忙，或者名称是空的。
    pub blocked: bool,
}

impl FolderDialogView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let dialog = &model.sidebar.folder_dialog;
        if !dialog.open {
            return None;
        }
        let busy = model.files.mutating || dialog.submitting;
        Some(Self {
            title: dialog.title(),
            summary: dialog.summary(),
            placeholder: dialog.placeholder(),
            action: dialog.action_label(),
            error: dialog.error.clone(),
            busy,
            blocked: busy || dialog.value.trim().is_empty(),
        })
    }
}

/// 新建或重命名文件夹。名称为空或文件服务忙时主按钮不可用，回车和主按钮都提交，提交后等结果再关。
pub fn folder_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(FolderDialogView::project(model)?);
    Projected::register(view, FolderDialogView::project);
    let draft = Draft::register(model, |model| model.sidebar.folder_dialog.open.then(|| model.sidebar.folder_dialog.value.clone()));
    let busy = move || view.with(|view| view.busy);
    let submit = || gap(GapMessage::SubmitFolderDialog);
    let body = widget(Stack::column(12.0).width(LengthSpec::Fill)).children((
        paragraph(move || view.with(|view| view.summary.clone()), true, "folder-dialog-summary"),
        text_field(
            "文件夹名称",
            "folder-dialog-name",
            draft,
            move || Arc::<str>::from(view.with(|view| view.placeholder)),
            false,
            busy,
            |value| gap(GapMessage::SetFolderValue(value)),
            Some(Arc::new(submit)),
        ),
        error_line(move || view.with(|view| view.error.clone()), "folder-dialog-error"),
    ));
    let buttons = vec![
        action("取消", ButtonKind::Ghost, busy, "folder-dialog-cancel", || gap(GapMessage::CloseFolderDialog)),
        action(move || view.with(|view| view.action.to_string()), ButtonKind::Primary, move || view.with(|view| view.blocked), "folder-dialog-submit", submit),
    ];
    Some(
        DialogFrame::new("folder-dialog", move || view.with(|view| view.title.to_string()), || gap(GapMessage::CloseFolderDialog))
            .busy(busy)
            .dialog(body, footer(None, buttons)),
    )
}

/// 处理文件夹要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FolderDeleteView {
    pub copy: String,
    pub error: String,
    pub busy: bool,
}

impl FolderDeleteView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let sidebar = &model.sidebar;
        if !sidebar.folder_delete_open() {
            return None;
        }
        Some(Self {
            copy: format!("将处理文件夹“{}”。请选择内部内容的处理方式。", sidebar.folder_delete_label),
            error: sidebar.folder_delete_error.clone(),
            busy: model.files.mutating || sidebar.folder_delete_submitting,
        })
    }
}

/// 处理文件夹：转移到上级目录，或移入回收站。对应 Vue 的 `folder-delete-dialog`。
pub fn folder_delete_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(FolderDeleteView::project(model)?);
    Projected::register(view, FolderDeleteView::project);
    let busy = move || view.with(|view| view.busy);
    let options = widget(Stack::column(10.0).width(LengthSpec::Fill)).children((
        option_card(
            "转移到上级目录",
            "保留内部文件和子文件夹，只删除当前这一层目录。",
            false,
            busy,
            "folder-delete-move",
            || gap(GapMessage::ConfirmFolderDelete(FolderDeleteMode::MoveToParent)),
        ),
        option_card(
            "移入回收站",
            "将该目录及其全部内容移入回收站，可在回收站中还原。",
            true,
            busy,
            "folder-delete-trash",
            || gap(GapMessage::ConfirmFolderDelete(FolderDeleteMode::Delete)),
        ),
    ));
    let body = widget(Stack::column(12.0).width(LengthSpec::Fill)).children((
        paragraph(move || view.with(|view| view.copy.clone()), false, "folder-delete-copy"),
        options,
        error_line(move || view.with(|view| view.error.clone()), "folder-delete-error"),
    ));
    let buttons = vec![action("取消", ButtonKind::Ghost, busy, "folder-delete-cancel", || gap(GapMessage::CloseFolderDelete))];
    Some(
        DialogFrame::new("folder-delete-dialog", || super::super::sidebar::FOLDER_DELETE_TITLE.to_string(), || {
            gap(GapMessage::CloseFolderDelete)
        })
        .danger()
        .busy(busy)
        .dialog(body, footer(None, buttons)),
    )
}

/// 带标题和说明的选项卡片。对应 `.folder-delete-dialog__option` / `.repository-delete-dialog__option`：
/// 内边距 14px、`--radius-xl`、`--bg` 底，悬停 `--bg-hover`；危险项描一圈浅红、悬停浅红底。
/// `detail` 和 `disabled` 可以读信号；禁用时整张卡 0.45 透明度，框架不派发点击。
fn option_card(
    title: &'static str,
    detail: impl IntoProp<String>,
    danger: bool,
    disabled: impl IntoProp<bool>,
    key: impl Into<String>,
    message: impl Fn() -> ShellMessage + Send + Sync + 'static,
) -> AnyView {
    let mut style = parts::row_style(0.0, 14.0, 14.0, 6.0, parts::ActiveTone::Accent, false);
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(nana_ui_core::FlexDirection::Column);
        layout.align_items = AlignSpec::Stretch;
        layout.height = None;
        layout.min_height = None;
        layout.padding_top = Some(LengthSpec::Px(14.0));
        layout.padding_bottom = Some(LengthSpec::Px(14.0));
        layout.white_space_nowrap = false;
        layout.text_overflow_ellipsis = false;
        if danger {
            layout.border_width = Some(1.0);
        }
    }
    style.radius = Some(RadiusTier::Xl);
    style.background = Some(SemanticColorRole::Background);
    if danger {
        style.border = Some(SemanticColorRole::DangerSoftHover);
        style.interaction.hovered.background = Some(SemanticColorRole::DangerSoftHover);
        style.interaction.pressed.background = Some(SemanticColorRole::DangerSoftPressed);
    }
    let detail_text = widget(wrapping_text(12.0, 18.0, SemanticColorRole::Muted)).prop::<String, nana_ui::runtime::view::fields::text::value>(detail);
    let content = widget(Stack::column(6.0).width(LengthSpec::Fill)).children((
        widget(parts::label_text(title, 14.0, 600, Some(SemanticColorRole::Text))),
        detail_text,
    ));
    widget(ListItem::new(title).style(style))
        .content(content)
        .key(key.into())
        .prop::<bool, DimmedDisabled>(disabled)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(message()))
        .into_any()
}

/// 删除智能文件夹要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SmartDeleteView {
    pub title: &'static str,
    pub copy: String,
    pub busy: bool,
}

impl SmartDeleteView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let sidebar = &model.sidebar;
        if !sidebar.smart_delete_open() {
            return None;
        }
        Some(Self {
            title: sidebar.smart_delete_title(),
            copy: format!("将删除“{}”及其子智能文件夹。实际文件和真实目录不会被删除。", sidebar.smart_delete_label),
            busy: sidebar.smart_draft.busy,
        })
    }
}

/// 删除智能文件夹确认。只删除筛选，不动真实文件。
pub fn smart_delete_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(SmartDeleteView::project(model)?);
    Projected::register(view, SmartDeleteView::project);
    let busy = move || view.with(|view| view.busy);
    let buttons = vec![
        action("取消", ButtonKind::Ghost, busy, "smart-delete-cancel", || gap(GapMessage::CloseSmartDelete)),
        action("删除", ButtonKind::Danger, busy, "smart-delete-confirm", || gap(GapMessage::ConfirmSmartDelete)),
    ];
    Some(
        DialogFrame::new("smart-delete-dialog", move || view.with(|view| view.title.to_string()), || gap(GapMessage::CloseSmartDelete))
            .danger()
            .busy(busy)
            .dialog(paragraph(move || view.with(|view| view.copy.clone()), false, "smart-delete-copy"), footer(None, buttons)),
    )
}

/// 新建播放集要显示的东西。名称草稿不在这里，见 [`Draft`]。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaylistCreateView {
    pub players: Choices,
    pub selected: Option<String>,
    /// 名称为空或没有选类型时「创建」不可用。
    pub blocked: bool,
}

impl PlaylistCreateView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        if !model.playlist_dialog_open {
            return None;
        }
        let selected = model.selected_new_playlist_player_type_id.clone();
        Some(Self {
            players: model
                .playlist_players
                .iter()
                .map(|player| (player.player_type_id.clone(), format!("{} · {}", player.label, player.file_class)))
                .collect(),
            blocked: model.new_playlist_name.trim().is_empty() || selected.is_none(),
            selected,
        })
    }
}

/// 新建播放集：名称和播放类型。播放类型的候选读完以后原地换上。
pub fn playlist_create_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(PlaylistCreateView::project(model)?);
    Projected::register(view, PlaylistCreateView::project);
    let draft = Draft::register(model, |model| model.playlist_dialog_open.then(|| model.new_playlist_name.clone()));
    let body = widget(Stack::column(12.0).width(LengthSpec::Fill)).children((
        text_field(
            "名称",
            "playlist-dialog-name",
            draft,
            "例如 通勤歌单 / 参考分镜",
            false,
            false,
            ShellMessage::NewPlaylistNameChanged,
            Some(Arc::new(|| ShellMessage::CreatePlaylist)),
        ),
        select_field(
            "播放类型",
            "playlist-dialog-type",
            move || view.with(|view| view.selected.clone()),
            move || view.with(|view| view.players.clone()),
            false,
            ShellMessage::SelectPlaylistPlayer,
        ),
    ));
    let buttons = vec![
        action("取消", ButtonKind::Ghost, false, "playlist-dialog-cancel", || ShellMessage::ClosePlaylistDialog),
        action("创建", ButtonKind::Primary, move || view.with(|view| view.blocked), "create-playlist", || ShellMessage::CreatePlaylist),
    ];
    Some(DialogFrame::new("playlist-dialog", || "新建播放集".to_string(), || ShellMessage::ClosePlaylistDialog).dialog(body, footer(None, buttons)))
}

/// 删除资源库要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RepositoryDeleteView {
    /// 「资源库…位于…」，找不到仓库时为空、不显示。
    pub summary: String,
    /// 三个选项的说明和能不能点，按 [`DELETE_MODES`] 的顺序。
    pub modes: [(String, bool); 3],
    pub error: String,
    /// 删除进行中：选项和取消都禁用，左下角显示「处理中...」。
    pub deleting: bool,
}

/// 删除资源库的三个选项，最后一个是危险项。
const DELETE_MODES: [DeleteMode; 3] = [DeleteMode::RecordOnly, DeleteMode::DeleteMetadata, DeleteMode::DeleteFolder];

impl RepositoryDeleteView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let workspace = &model.workspace;
        let WorkspaceDialog::Delete(dialog) = workspace.dialogs.last()?;
        let deleting = workspace.deleting_mode.is_some();
        let summary = workspace
            .repositories
            .iter()
            .find(|item| item.repo_id == dialog.repo_id)
            .map(|item| format!("资源库“{}”位于 {}。下面每个操作都会移除当前注册记录。", item.name, item.path))
            .unwrap_or_default();
        Some(Self {
            summary,
            modes: DELETE_MODES.map(|mode| (workspace.delete_mode_detail(mode), deleting || !workspace.delete_mode_enabled(mode))),
            error: dialog.error.clone(),
            deleting,
        })
    }
}

/// 删除资源库：只删记录、删 .momo 数据、删整个文件夹三个选项，处理中左下角显示「处理中...」。
pub fn repository_delete_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(RepositoryDeleteView::project(model)?);
    Projected::register(view, RepositoryDeleteView::project);
    let deleting = move || view.with(|view| view.deleting);
    // `.repository-delete-dialog__summary`：12px、行高 1.6，颜色继承正文。
    let summary = widget(wrapping_text(12.0, 19.2, SemanticColorRole::Text))
        .key("delete-dialog-summary")
        .prop::<String, nana_ui::runtime::view::fields::HiddenWhenEmpty<nana_ui::runtime::view::fields::text::value>>(move || {
            view.with(|view| view.summary.clone())
        });
    let options = DELETE_MODES
        .into_iter()
        .enumerate()
        .map(|(index, mode)| delete_option(view, index, mode))
        .collect::<Vec<_>>();
    let body = widget(Stack::column(12.0).width(LengthSpec::Fill)).children((
        summary,
        widget(Stack::column(10.0).width(LengthSpec::Fill)).children(options),
        error_line(move || view.with(|view| view.error.clone()), "delete-dialog-error"),
    ));
    let buttons = vec![action("取消", ButtonKind::Ghost, deleting, "delete-dialog-cancel", || ShellMessage::MissingCloseDelete)];
    Some(
        DialogFrame::new("repository-delete-dialog", || "删除资源库".to_string(), || ShellMessage::MissingCloseDelete)
            .danger()
            .busy(deleting)
            .dialog(body, footer(Some(busy_note(deleting, "delete-dialog-busy")), buttons)),
    )
}

/// 删除资源库的一个选项：说明和禁用读投影里这一项。
fn delete_option(view: Signal<RepositoryDeleteView>, index: usize, mode: DeleteMode) -> AnyView {
    option_card(
        mode.label(),
        move || view.with(|view| view.modes[index].0.clone()),
        mode == DeleteMode::DeleteFolder,
        move || view.with(|view| view.modes[index].1),
        format!("delete-mode-{}", mode.label()),
        move || ShellMessage::MissingConfirmDelete(mode),
    )
}

#[cfg(test)]
#[path = "sidebar_dialogs_tests.rs"]
mod tests;

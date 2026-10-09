//! 文件页的对话框：重命名、新建、复制 / 移动到文件夹、导入路径和硬链接确认，导出资源库在
//! `workspace_export_dialog.rs`。它们是浮层槽位里的一块（`OverlayKey::FileDialog` /
//! `OverlayKey::ExportDialog`），外壳走统一对话框框架：文件变更进行中三种关闭手势都不关，
//! 点外面等于取消，硬链接确认点外面等于跳过（Vue `@click.self`）。
//!
//! 文案、字段和按钮照 Vue `WorkspaceFilesSurface.vue`、`CopyTargetDialog.vue` 和
//! `HardlinkCandidateDialog.vue`。对话框常驻：同一种对话框打开期间只改绑定的字段，输入框用
//! `.model` 加草稿信号受控；换一种对话框时浮层块按身份换块。

use std::sync::Arc;

use nana_ui::runtime::view::{fields, signal, widget, AnyView, IntoView, Signal};
use nana_ui::runtime::{LengthSpec, RadiusTier, SemanticColorRole, Stack};
use nana_ui::ButtonKind;

use super::files::{FileDialog, FilesMessage, FilesState};
use super::files_view::style;
use super::view_part_overlay::dialog::{action, footer, text_field, wrapping_text, DialogFrame, DialogWidth};
use super::view_part_overlay::session::{Draft, Projected};
use super::{ShellMessage, ShellViewModel};

#[path = "workspace_export_dialog.rs"]
mod export;
pub(super) use export::export_dialog;

fn file_message(message: FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

/// 一种文本对话框：标题、宽度、字段标签、占位、主按钮文案，以及它改的是哪一份草稿。
struct TextDialog {
    title: Title,
    width: DialogWidth,
    label: &'static str,
    placeholder: &'static str,
    submit: &'static str,
    draft: fn(&FilesState) -> &str,
}

/// 文本对话框的标题。Eagle 导入按复制还是剪切换标题。
enum Title {
    Fixed(&'static str),
    Eagle,
}

impl Title {
    fn text(&self, files: &FilesState) -> String {
        match self {
            Self::Fixed(title) => (*title).to_string(),
            Self::Eagle => if files.eagle_mode == "move" { "从 Eagle 剪切导入" } else { "从 Eagle 复制导入" }.to_string(),
        }
    }
}

fn name_draft(files: &FilesState) -> &str {
    &files.name_draft
}

fn target_draft(files: &FilesState) -> &str {
    &files.target_draft
}

fn import_draft(files: &FilesState) -> &str {
    &files.import_draft
}

/// 文本对话框的配置。硬链接确认和关着的时候是 `None`。
fn text_dialog_spec(dialog: FileDialog) -> Option<TextDialog> {
    let spec = |title, width, label, placeholder, submit, draft| TextDialog { title, width, label, placeholder, submit, draft };
    let normal = DialogWidth::Normal;
    Some(match dialog {
        FileDialog::Rename => spec(Title::Fixed("重命名文件"), DialogWidth::Narrow, "新名称", "输入新的文件名", "保存", name_draft),
        FileDialog::CreateFile => spec(Title::Fixed("建文件"), normal, "文件名", "新建空文件，例如 note.txt", "创建", name_draft),
        FileDialog::CreateDirectory => spec(Title::Fixed("新建文件夹"), normal, "文件夹名称", "输入文件夹名称", "创建", name_draft),
        FileDialog::Copy => spec(Title::Fixed("复制到文件夹"), normal, "目标目录", "留空表示根目录", "复制", target_draft),
        FileDialog::Move => spec(Title::Fixed("移动到文件夹"), normal, "目标目录", "留空表示根目录", "移动", target_draft),
        FileDialog::Import => {
            spec(Title::Fixed("从文件夹导入"), normal, "多个路径用分号分隔", "D:/素材/图片; D:/素材/参考", "导入", import_draft)
        }
        FileDialog::ImportArchive => spec(Title::Fixed("从 ZIP 导入"), normal, "压缩包路径", "D:/素材/归档.zip", "导入", import_draft),
        FileDialog::ImportEagle => spec(Title::Eagle, normal, "Eagle 资源库路径", "D:/素材/示例.library", "导入", import_draft),
        FileDialog::Hardlink | FileDialog::Closed => return None,
    })
}

/// 文本对话框要显示的东西。草稿不在这里，见 [`Draft`]。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextDialogView {
    pub title: String,
    /// 文件变更进行中：输入和按钮禁用，主按钮写「处理中...」，三种关闭手势都不关。
    pub busy: bool,
    pub submit: String,
}

impl TextDialogView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let files = &model.files;
        let spec = text_dialog_spec(files.dialog)?;
        Some(Self {
            title: spec.title.text(files),
            busy: files.mutating,
            submit: if files.mutating { "处理中...".to_string() } else { spec.submit.to_string() },
        })
    }
}

/// 文件变更对话框。没有打开时不占位。
pub(super) fn file_dialog(model: &ShellViewModel) -> Option<AnyView> {
    match model.files.dialog {
        FileDialog::Hardlink => hardlink_dialog(model),
        dialog => text_dialog(model, dialog),
    }
}

/// 一个输入框的对话框：字段标签、输入，底部取消和主操作。回车提交。
fn text_dialog(model: &ShellViewModel, dialog: FileDialog) -> Option<AnyView> {
    let spec = text_dialog_spec(dialog)?;
    let view = signal(TextDialogView::project(model)?);
    Projected::register(view, TextDialogView::project);
    let read = spec.draft;
    let draft = Draft::register(model, move |model| (model.files.dialog == dialog).then(|| read(&model.files).to_string()));
    let busy = move || view.with(|view| view.busy);
    let submit = || file_message(FilesMessage::SubmitDialog);
    let close = || file_message(FilesMessage::CloseDialog);
    let body = text_field(
        spec.label,
        "file-dialog-input",
        draft,
        spec.placeholder,
        false,
        busy,
        |value| file_message(FilesMessage::DraftChanged(value)),
        Some(Arc::new(submit)),
    );
    let buttons = vec![
        action("取消", ButtonKind::Ghost, busy, "file-dialog-cancel", close),
        action(move || view.with(|view| view.submit.clone()), ButtonKind::Primary, busy, "file-dialog-submit", submit),
    ];
    Some(
        DialogFrame::new("file-dialog", move || view.with(|view| view.title.clone()), close)
            .width(spec.width)
            .busy(busy)
            .dialog(body, footer(None, buttons)),
    )
}

/// 硬链接确认要显示的东西：队列里当前这一条。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HardlinkView {
    pub message: String,
    pub existing_path: String,
    pub new_path: String,
    pub busy: bool,
    pub confirm: String,
}

impl HardlinkView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let files = &model.files;
        if files.dialog != FileDialog::Hardlink {
            return None;
        }
        let prompt = files.current_hardlink()?;
        Some(Self {
            message: prompt.message(),
            existing_path: prompt.existing_path.clone(),
            new_path: prompt.new_path.clone(),
            busy: files.mutating,
            confirm: if files.mutating { "处理中..." } else { "加入关联" }.to_string(),
        })
    }
}

/// 硬链接候选：说明一段，两条路径放在浅底框里，底部跳过和加入关联。点外面等于跳过；
/// 跳过一条以后队列里的下一条原地换上。
fn hardlink_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(HardlinkView::project(model)?);
    Projected::register(view, HardlinkView::project);
    let busy = move || view.with(|view| view.busy);
    let skip = || file_message(FilesMessage::SkipHardlink);
    let message = widget(wrapping_text(13.0, 19.5, SemanticColorRole::Text))
        .key("file-hardlink-message")
        .prop::<String, fields::text::value>(move || view.with(|view| view.message.clone()));
    let paths = widget(
        Stack::column(6.0)
            .width(LengthSpec::Fill)
            .padding(10.0)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((path_line(view, |view| view.existing_path.clone(), "file-hardlink-existing"), path_line(view, |view| view.new_path.clone(), "file-hardlink-new")))
    .key("file-hardlink-paths");
    let body = widget(Stack::column(12.0).width(LengthSpec::Fill)).children((message, paths));
    let buttons = vec![
        action("跳过", ButtonKind::Ghost, busy, "file-hardlink-skip", skip),
        action(move || view.with(|view| view.confirm.clone()), ButtonKind::Primary, busy, "file-hardlink-confirm", || {
            file_message(FilesMessage::ConfirmHardlink)
        }),
    ];
    Some(DialogFrame::new("hardlink-dialog", || "加入硬链接关联".to_string(), skip).busy(busy).dialog(body, footer(None, buttons)))
}

/// 路径框里的一行：12px 弱色、任意处折行。
fn path_line(view: Signal<HardlinkView>, read: fn(&HardlinkView) -> String, key: &'static str) -> AnyView {
    widget(style::wrapping(style::small_muted(String::new())))
        .key(key)
        .prop::<String, fields::text::value>(move || view.with(read))
        .into_any()
}

#[cfg(test)]
#[path = "workspace_dialogs_tests.rs"]
mod tests;

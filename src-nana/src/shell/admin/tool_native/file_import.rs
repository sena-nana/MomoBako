//! 文件导入工具页的进行中和完成提示。
//!
//! 对应 `External/Plugins/file-manager/src/register.js` 的 `runImport`：只有从这一页点
//! 「从文件夹导入」或「从 ZIP 导入」，系统选择器交回路径并真正提交以后，文件夹按钮才写成
//! 「处理中...」（ZIP 按钮文字不变，两个都因导入进行中禁用）。协议成功后写
//! 「文件夹导入完成。 目标目录：…」或「ZIP 导入完成。 目标目录：…」，根目录写成「/」。
//! 取消、空白、对话框失败，以及文件菜单和拖放触发的导入，都不写这句。失败清掉上一句完成提示，
//! 错误仍由文件列的错误行显示。

use nana_ui::runtime::view::{AnyView, IntoView, Signal, widget};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, Stack};

use super::super::super::workspace::WorkspacePanel;
use super::super::ShellViewModel;
use super::super::ToolFileImport;
use super::super::support::TOOL_FILE_MANAGER;
use super::{ActionDisabled, ImportView, action, import_buttons, import_message, style};

/// 从文件导入工具页打开选择器时记下种类。不在这一页时清掉上次没交回的等待。
pub(crate) fn arm(model: &mut ShellViewModel, folder: bool) {
    let on_page = model.workspace.panel == WorkspacePanel::Extensions
        && model.admin.active_tool_page_id.as_deref() == Some(TOOL_FILE_MANAGER);
    if !on_page {
        model.admin.file_import_armed = None;
        return;
    }
    model.admin.file_import_armed = Some(kind_of(folder));
}

/// 选择器取消、失败或没有提交时，不再等这一次的完成提示。
pub(crate) fn drop_arm(model: &mut ShellViewModel) {
    model.admin.file_import_armed = None;
}

/// 路径已经提交。和等待的种类一致时进入「处理中...」，并清掉上一句完成提示。
pub(crate) fn begin(model: &mut ShellViewModel, folder: bool) {
    let kind = kind_of(folder);
    if model.admin.file_import_armed != Some(kind) {
        if model.admin.file_import_armed.is_some() {
            eprintln!("Nana 文件导入工具页等的是另一种导入，这次结果不记完成提示");
        }
        model.admin.file_import_armed = None;
        return;
    }
    model.admin.file_import_armed = None;
    model.admin.file_import_busy = Some(kind);
    model.admin.file_import_notice.clear();
}

/// 协议结果回来。成功按当时的当前目录写完成提示；失败只清提示。不是这一页提交的结果忽略。
pub(crate) fn note_settled(model: &mut ShellViewModel, succeeded: bool) {
    let Some(kind) = model.admin.file_import_busy.take() else {
        return;
    };
    if !succeeded {
        model.admin.file_import_notice.clear();
        eprintln!("Nana 文件导入工具页导入失败，不写完成提示");
        return;
    }
    let head = match kind {
        ToolFileImport::Folder => "文件夹导入完成。",
        ToolFileImport::Zip => "ZIP 导入完成。",
    };
    model.admin.file_import_notice = format!("{head} 目标目录：{}", directory_label(model));
}

/// 目标目录的显示：空路径是「/」，和工具页卡片上的目录相同。
pub(super) fn directory_label(model: &ShellViewModel) -> String {
    let path = model.files.current_path.trim();
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

/// 文件夹按钮的文字。进行中是「处理中...」，ZIP 导入进行中也是这句，和 Vue 共用的 `isBusy` 一样。
pub(super) fn folder_button_label(busy: bool) -> &'static str {
    if busy {
        "处理中..."
    } else {
        "从文件夹导入"
    }
}

fn kind_of(folder: bool) -> ToolFileImport {
    if folder {
        ToolFileImport::Folder
    } else {
        ToolFileImport::Zip
    }
}

/// 完成提示。没有提示时不占位。
pub(super) fn notice(view: Signal<ImportView>) -> AnyView {
    let text = move || view.with(|view| view.notice.clone());
    style::state_notice(text, false, "admin-tool-file-import-notice")
        .visible(move || view.with(|view| !view.notice.is_empty()))
        .into_any()
}

/// 文件导入页的两个按钮。能不能点是绑定；文件夹按钮在这次导入进行中换成「处理中...」。
pub(super) fn action_row(page_id: &str, view: Signal<ImportView>) -> AnyView {
    let enabled = move || view.with(|view| view.enabled);
    let buttons = import_buttons(page_id)
        .into_iter()
        .map(|button| {
            let message = button.message;
            let idle = button.label;
            let folder = button.id == "folder";
            let label_of = view;
            let text = move || {
                if folder {
                    folder_button_label(label_of.with(|view| view.busy)).to_string()
                } else {
                    idle.to_string()
                }
            };
            widget(action(idle, None, button.tone, false))
                .prop::<String, nana_ui::runtime::view::fields::button::label>(text)
                .prop::<bool, ActionDisabled>(move || !enabled())
                .key(format!("admin-tool-action-{page_id}-{}", button.id))
                .on_cx(move |_, _: &Activate, cx| {
                    if let Some(outgoing) =
                        import_message(view.with_untracked(|view| view.enabled), message.clone())
                    {
                        cx.dispatch_program_all(outgoing);
                    }
                })
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(
        Stack::row(8.0)
            .wrap(true)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill),
    )
    .children(buttons)
    .into_any()
}

#[cfg(test)]
mod tests {
    use nana_ui_platform::WindowId;
    use nana_ui_platform::host::WindowCommand;

    use crate::backend::services::repository::{
        FileBrowserSnapshot, RepositoryStructureCacheState,
    };
    use crate::shell::files::{FileDialog, FilesEffect, FilesMessage};
    use crate::shell::input::InputMessage;
    use crate::shell::workspace::WorkspaceRepository;
    use crate::shell::{ShellMessage, ShellViewModel};

    use super::super::ImportView;
    use super::TOOL_FILE_MANAGER;
    use super::folder_button_label;

    fn writable() -> ShellViewModel {
        let mut model = ShellViewModel::default();
        model.workspace.active_repo_id = Some("repo".into());
        model.workspace.repositories = vec![WorkspaceRepository {
            repo_id: "repo".into(),
            name: "资料库".into(),
            path: "C:/repo".into(),
            status: "ready".into(),
            backend_plugin_id: "filesystem".into(),
            capabilities: vec!["write".into()],
            cache_required: false,
            cache_status: String::new(),
            cache_path: None,
            login_expired: false,
            logged_out: false,
        }];
        model.workspace.panel = super::WorkspacePanel::Extensions;
        model.admin.active_tool_page_id = Some(TOOL_FILE_MANAGER.into());
        model
    }

    fn finish(model: &mut ShellViewModel, result: Result<(), String>) {
        model.reduce(ShellMessage::Files(FilesMessage::ProtocolFinished {
            result,
            reload: false,
            hardlinks: false,
            folder_tree: None,
        }));
    }

    /// 打开选择器并取回这次对话框的编号。
    fn open_picker(model: &mut ShellViewModel, dialog: FileDialog) -> u64 {
        model.reduce(ShellMessage::Files(FilesMessage::OpenDialog(dialog)));
        let commands = model.input.take_platform_commands(WindowId(1), false);
        match commands.as_slice() {
            [WindowCommand::OpenFileDialog { request, .. }] => request.id,
            other => panic!("文件导入工具页没有排队选择器：{other:?}"),
        }
    }

    fn pick(model: &mut ShellViewModel, request_id: u64, path: &str) {
        model.reduce(ShellMessage::Input(InputMessage::FileDialogCompleted {
            request_id,
            paths: vec![path.into()],
            failed: None,
        }));
    }

    /// 从工具页导入：进行中文件夹按钮是「处理中...」，成功按当前目录写完成提示；根目录写成「/」。
    /// 取消不写。文件菜单的导入不写，失败清掉上一句。
    #[test]
    fn file_import_tool_page_reports_the_finished_directory() {
        let mut model = writable();
        let request_id = open_picker(&mut model, FileDialog::Import);
        model.reduce(ShellMessage::Input(InputMessage::FileDialogCompleted {
            request_id,
            paths: Vec::new(),
            failed: None,
        }));
        let idle = ImportView::project(&model, TOOL_FILE_MANAGER);
        assert!(idle.notice.is_empty() && !idle.busy, "取消选择不写完成提示");
        assert_eq!(folder_button_label(idle.busy), "从文件夹导入");

        let request_id = open_picker(&mut model, FileDialog::Import);
        pick(&mut model, request_id, "D:/素材");
        assert!(matches!(
            model.files.take_effects().as_slice(),
            [FilesEffect::Import { .. }]
        ));
        let busy = ImportView::project(&model, TOOL_FILE_MANAGER);
        assert!(busy.busy && busy.notice.is_empty() && !busy.enabled);
        assert_eq!(folder_button_label(busy.busy), "处理中...");
        finish(&mut model, Ok(()));
        let done = ImportView::project(&model, TOOL_FILE_MANAGER);
        assert_eq!(done.notice, "文件夹导入完成。 目标目录：/");
        assert!(!done.busy && done.enabled);
        assert_eq!(folder_button_label(done.busy), "从文件夹导入");

        model.reduce(ShellMessage::FileBrowserLoaded(Ok(FileBrowserSnapshot {
            repo_id: "repo".into(),
            root_path: "C:/repo".into(),
            backend_plugin_id: "filesystem".into(),
            backend_kind: "local".into(),
            cache_state: RepositoryStructureCacheState::Ready,
            indexed_at: None,
            current_path: "inbox".into(),
            total_entries: 0,
            loaded_count: 0,
            next_offset: None,
            has_more: false,
            special_location: None,
            tree: None,
            entries: Vec::new(),
        })));
        let request_id = open_picker(&mut model, FileDialog::ImportArchive);
        assert_eq!(
            ImportView::project(&model, TOOL_FILE_MANAGER).notice,
            "文件夹导入完成。 目标目录：/",
            "选择器还没交回时留着上一句"
        );
        pick(&mut model, request_id, "D:/归档.zip");
        let zipping = ImportView::project(&model, TOOL_FILE_MANAGER);
        assert!(zipping.notice.is_empty() && zipping.busy);
        assert_eq!(folder_button_label(zipping.busy), "处理中...");
        finish(&mut model, Ok(()));
        assert_eq!(
            ImportView::project(&model, TOOL_FILE_MANAGER).notice,
            "ZIP 导入完成。 目标目录：inbox"
        );

        let request_id = open_picker(&mut model, FileDialog::Import);
        pick(&mut model, request_id, "D:/另一份");
        finish(&mut model, Err("导入失败".into()));
        let failed = ImportView::project(&model, TOOL_FILE_MANAGER);
        assert!(failed.notice.is_empty() && !failed.busy);
        assert_eq!(model.files.error, "导入失败");

        model.workspace.panel = super::WorkspacePanel::Files;
        let request_id = open_picker(&mut model, FileDialog::Import);
        pick(&mut model, request_id, "D:/菜单");
        let menu = ImportView::project(&model, TOOL_FILE_MANAGER);
        assert!(
            !menu.busy && menu.notice.is_empty(),
            "文件菜单的导入不记工具页提示"
        );
        assert_eq!(folder_button_label(menu.busy), "从文件夹导入");
        finish(&mut model, Ok(()));
        assert!(
            ImportView::project(&model, TOOL_FILE_MANAGER)
                .notice
                .is_empty()
        );
    }
}

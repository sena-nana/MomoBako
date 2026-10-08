//! 还没挂到主界面的仓库对话框。
//!
//! `RepositoryExportDialog.vue` 没有出现在首页。这里只在显式打开时画 Nana `Dialog`，
//! 不在文件列表上加导出入口，也不假装导出已经完成。

use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, BrowseRequested, Button, Dialog, PathField, Select, SelectChanged, SelectOption, Stack, TextChanged, TextInput,
};

use super::files::FilesMessage;
use super::workbench;
use super::{ShellMessage, ShellViewModel};

/// 导出资源库。压缩包和 Git 两个区块都在，提交不会编造成功。
pub(super) fn export_dialog(model: &ShellViewModel) -> Option<AnyView> {
    if !model.files.export.open {
        return None;
    }
    let archive = model.files.export.target != "git";
    let repository = model.workspace.active_repository();
    let name = repository.map(|item| item.name.clone()).unwrap_or_else(|| "未选择资源库".into());
    let path = repository.map(|item| item.path.clone()).unwrap_or_else(|| "未返回路径".into());
    let body = widget(Stack::column(8.0)).children((
        widget(workbench::section_title(name)).key("export-repo-name"),
        widget(workbench::meta(path)).key("export-repo-path"),
        widget(Stack::row(8.0)).children((
            target_button("压缩包", "archive", archive),
            target_button("Git", "git", !archive),
        )),
        if archive { archive_fields(model) } else { git_fields(model) },
        (!model.files.export.notice.is_empty()).then(|| text(model.files.export.notice.clone()).key("export-notice")),
        (!model.files.export.error.is_empty()).then(|| text(model.files.export.error.clone()).key("export-error")),
    ));
    Some(
        widget(Dialog::new("导出资源库"))
            .body(body)
            .footer(widget(Stack::row(8.0)).children((
                widget(workbench::ghost_button("取消")).key("export-cancel").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(file_message(FilesMessage::CloseExport));
                }),
                widget(workbench::primary_button("导出")).key("export-submit").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(file_message(FilesMessage::SubmitExport));
                }),
            )))
            .into_any(),
    )
}

fn archive_fields(model: &ShellViewModel) -> AnyView {
    let encrypt = model.files.export.encrypt;
    widget(Stack::column(8.0))
        .children((
            output_path_row(&model.files.export.output_path),
            choice_row("格式", "export-format", &model.files.export.format, &[("zip", "zip"), ("7z", "7z"), ("tar", "tar")]),
            choice_row(
                "压缩",
                "export-compression",
                &model.files.export.compression,
                &[("none", "不压缩"), ("fast", "快速"), ("balanced", "均衡"), ("maximum", "最大")],
            ),
            widget(workbench::ghost_button(if encrypt { "已开启加密" } else { "加密压缩包" }))
                .key("export-encrypt")
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(file_message(FilesMessage::SetExportField {
                        field: "encrypt".into(),
                        value: if encrypt { "0".into() } else { "1".into() },
                    }));
                }),
            encrypt.then(|| {
                widget(TextInput::new(model.files.export.password.clone()).label("密码").placeholder("用于压缩包加密"))
                    .key("export-password")
                    .on_cx(|_, event: &TextChanged, cx| {
                        cx.dispatch_program(file_message(FilesMessage::SetExportField {
                            field: "password".into(),
                            value: event.value.to_string(),
                        }));
                    })
            }),
        ))
        .key("export-archive")
        .into_any()
}

/// 压缩包必须先有保存路径。浏览沿用系统保存对话框，取消不调用导出协议。
fn output_path_row(path: &str) -> AnyView {
    widget(Stack::row(8.0).align(nana_ui::runtime::AlignSpec::Center).wrap(true))
        .children((
            widget(PathField::new(path.to_string()).label("输出路径").placeholder("选择保存路径"))
                .key("export-output")
                .on_cx(|_, event: &TextChanged, cx| {
                    cx.dispatch_program(file_message(FilesMessage::SetExportField {
                        field: "output".into(),
                        value: event.value.to_string(),
                    }));
                })
                .on_cx(|_, _: &BrowseRequested, cx| {
                    cx.dispatch_program(file_message(FilesMessage::ChooseExportOutput));
                }),
            widget(Button::new("选择输出路径")).key("export-output-browse").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::ChooseExportOutput));
            }),
        ))
        .key("export-output-row")
        .into_any()
}

fn git_fields(model: &ShellViewModel) -> AnyView {
    widget(Stack::column(8.0))
        .children((
            draft_field("远端", "export-remote", &model.files.export.remote, "origin", "remote"),
            draft_field("分支", "export-branch", &model.files.export.branch, "默认当前分支", "branch"),
            draft_field("提交信息", "export-git-message", &model.files.export.message, "导出资源库", "message"),
        ))
        .key("export-git")
        .into_any()
}

/// 格式和压缩用下拉，不用一排按钮。选中值只出现在控件里一次。
fn choice_row(label: &str, key_name: &'static str, current: &str, options: &[(&str, &str)]) -> AnyView {
    let field = key_name.trim_start_matches("export-").to_string();
    let options = options.iter().map(|(value, caption)| SelectOption::new(*value, *caption)).collect::<Vec<_>>();
    widget(Stack::column(4.0))
        .children((
            widget(workbench::meta(label)).key(format!("{key_name}-label")),
            widget(Select::new(Some(current.to_string())).options(options).placeholder(label))
                .key(key_name)
                .on_cx(move |_, event: &SelectChanged, cx| {
                    cx.dispatch_program(file_message(FilesMessage::SetExportField {
                        field: field.clone(),
                        value: event.value.to_string(),
                    }));
                }),
        ))
        .into_any()
}

fn draft_field(label: &str, key_name: &'static str, value: &str, placeholder: &str, field: &'static str) -> AnyView {
    widget(TextInput::new(value.to_string()).label(label).placeholder(placeholder))
        .key(key_name)
        .on_cx(move |_, event: &TextChanged, cx| {
            cx.dispatch_program(file_message(FilesMessage::SetExportField { field: field.into(), value: event.value.to_string() }));
        })
        .into_any()
}

fn target_button(label: &str, value: &'static str, selected: bool) -> AnyView {
    let caption = if selected { format!("已选 {label}") } else { label.to_string() };
    widget(if selected { workbench::primary_button(caption) } else { workbench::ghost_button(caption) })
        .key(format!("export-target-{value}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(file_message(FilesMessage::SetExportField { field: "target".into(), value: value.into() }));
        })
        .into_any()
}

fn file_message(message: FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

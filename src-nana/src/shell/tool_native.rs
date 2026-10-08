//! 拓展页里的文件导入和 Eagle 导入工具页。
//!
//! 内容照 `External/Plugins/{file-manager,eagle-importer}/src/register.js`：眉题、插件名标题、
//! 说明、目标仓库和目录、不能导入的原因、导入按钮和备注。Vue 的 `.tool-page-shell` 没有任何
//! 样式（内容贴着面板边、标题是浏览器默认 2em），这里按 API Playground 的版式画：内边距 18、
//! 22 号标题、卡片包住目标信息。导入走文件面板已有的导入对话框和状态机。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, Stack};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};

use super::super::files::{FileContext, FileDialog, FilesMessage};
use super::super::{ShellMessage, ShellViewModel};
use super::style::{self, action, column, label, mono, pad, row, wrapping, Tone};
use super::support::{TOOL_EAGLE_IMPORTER, TOOL_FILE_MANAGER};
use super::ToolPageEntry;

/// 工具页上的一个按钮。禁用时不派发消息。
pub struct ImportAction {
    pub id: &'static str,
    pub label: &'static str,
    pub tone: Tone,
    pub enabled: bool,
    pub message: FilesMessage,
}

/// 内置的文件导入或 Eagle 导入页。
pub(super) fn import_page(model: &ShellViewModel, page: &ToolPageEntry) -> AnyView {
    let id = page.id.as_str();
    let (eyebrow, subline) = if id == TOOL_EAGLE_IMPORTER {
        ("Eagle 导入", "导入目标固定为当前工作区的当前目录。")
    } else {
        ("文件导入", "目标固定为当前工作区的当前目录。")
    };
    let title = plugin_name(model, id).unwrap_or_else(|| if id == TOOL_EAGLE_IMPORTER { "Eagle Importer".into() } else { "File Manager".into() });
    let (repository, directory) = import_target(model);
    let mut body = vec![
        widget(column(0.0))
            .children((
                widget(style::eyebrow_text(eyebrow)).key(format!("admin-tool-eyebrow-{id}")),
                widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0)).children((widget(style::label_lh(title, 22.0, 700, Role::Text, 1.25)).key(format!("admin-tool-title-{id}")),)),
                widget(pad(column(0.0), 8.0, 0.0, 0.0, 0.0)).children((widget(wrapping(label(subline, 13.0, 400, Role::Muted))).key(format!("admin-tool-subline-{id}")),)),
            ))
            .into_any(),
        target_card(vec![
            target_row("目标仓库", widget(label(repository, 14.0, 600, Role::Text)).key(format!("admin-tool-repo-{id}")).into_any()),
            target_row("目标目录", code(directory, format!("admin-tool-dir-{id}"))),
        ]),
    ];
    if let Some(reason) = import_block_reason(model) {
        body.push(style::state_notice(reason.into(), false, "admin-tool-reason"));
    }
    if !model.files.error.is_empty() {
        body.push(style::state_notice(model.files.error.clone(), true, "admin-tool-error"));
    }
    if id == TOOL_EAGLE_IMPORTER {
        body.push(target_card(vec![
            target_row("EagleLibrary", code("尚未选择".into(), "admin-tool-eagle-path".into())),
            action_row(model, id),
        ]));
    } else {
        body.push(action_row(model, id));
        body.push(widget(wrapping(label("ZIP 首版固定支持 .zip，并按解压导入保留内部目录结构。", 12.0, 400, Role::Muted))).key(format!("admin-tool-note-{id}")).into_any());
    }
    widget(pad(column(14.0), 18.0, 18.0, 18.0, 18.0)).children(body).key(format!("admin-tool-native-{id}")).into_any()
}

fn plugin_name(model: &ShellViewModel, tool_id: &str) -> Option<String> {
    model.admin.plugins.iter().find(|plugin| plugin.plugin_id == tool_id).map(|plugin| plugin.name.clone())
}

/// 目标卡片：主背景、`border-soft` 边线、md 圆角，内边距 12/14，行间 8。
fn target_card(rows: Vec<AnyView>) -> AnyView {
    widget(pad(column(8.0), 13.0, 15.0, 13.0, 15.0).surface(Role::Background).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Md))
        .children(rows)
        .into_any()
}

fn target_row(name: &str, value: AnyView) -> AnyView {
    widget(row(8.0).wrap(true)).children((widget(label(name, 13.0, 400, Role::Muted)), value)).into_any()
}

/// 行内代码：等宽 12 号、`bg-subtle`、`border-soft` 边线、xs 圆角。
fn code(text: String, key: String) -> AnyView {
    widget(pad(row(0.0), 1.0, 6.0, 1.0, 6.0).surface(Role::Subtle).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Xs))
        .children((widget(mono(text, 12.0, 400, Role::Text)).key(key),))
        .into_any()
}

/// 不能导入时返回和 Vue 相同的原因。能否导入仍走文件状态机的 `can_import`。
pub fn import_block_reason(model: &ShellViewModel) -> Option<&'static str> {
    let ctx = FileContext::from_model(model);
    if model.files.can_import(&ctx) {
        return None;
    }
    if ctx.repo_id.as_deref().map(str::trim).filter(|id| !id.is_empty()).is_none() {
        return Some("当前没有可用仓库。");
    }
    if !ctx.writable || ctx.missing {
        return Some("当前仓库处于只读状态。");
    }
    if ctx.trash {
        return Some("回收站视图不支持导入。");
    }
    if ctx.is_virtual() {
        return Some("虚拟视图不支持导入。");
    }
    Some("当前视图不能导入。")
}

/// 文件导入和 Eagle 导入的按钮。只携带已有的文件消息。
pub fn import_actions(model: &ShellViewModel, page_id: &str) -> Vec<ImportAction> {
    let enabled = import_block_reason(model).is_none() && !model.files.mutating;
    match page_id {
        TOOL_FILE_MANAGER => vec![
            ImportAction { id: "folder", label: "从文件夹导入", tone: Tone::Primary, enabled, message: FilesMessage::OpenDialog(FileDialog::Import) },
            ImportAction { id: "zip", label: "从 ZIP 导入", tone: Tone::Plain, enabled, message: FilesMessage::OpenDialog(FileDialog::ImportArchive) },
        ],
        TOOL_EAGLE_IMPORTER => vec![
            ImportAction { id: "copy", label: "复制导入", tone: Tone::Primary, enabled, message: FilesMessage::OpenEagle("copy".into()) },
            ImportAction { id: "move", label: "剪切导入", tone: Tone::Plain, enabled, message: FilesMessage::OpenEagle("move".into()) },
        ],
        other => {
            eprintln!("Nana 工具页没有导入按钮：{other}");
            Vec::new()
        }
    }
}

/// 禁用的导入按钮不进入文件状态机。
pub fn import_message(enabled: bool, message: FilesMessage) -> Option<ShellMessage> {
    if !enabled {
        eprintln!("Nana 工具页当前不能导入");
        return None;
    }
    Some(ShellMessage::Files(message))
}

fn import_target(model: &ShellViewModel) -> (String, String) {
    let repository = model
        .workspace
        .active_repository()
        .map(|item| item.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "未选择仓库".into());
    let path = model.files.current_path.trim();
    let directory = if path.is_empty() { "/".to_string() } else { path.to_string() };
    (repository, directory)
}

fn action_row(model: &ShellViewModel, page_id: &str) -> AnyView {
    let buttons = import_actions(model, page_id)
        .into_iter()
        .map(|item| {
            let enabled = item.enabled;
            let message = item.message;
            widget(action(item.label, None, item.tone, !enabled))
                .key(format!("admin-tool-action-{page_id}-{}", item.id))
                .on_cx(move |_, _: &Activate, cx| {
                    if let Some(outgoing) = import_message(enabled, message.clone()) {
                        cx.dispatch_program(outgoing);
                    }
                })
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(Stack::row(8.0).wrap(true).align(AlignSpec::Center).width(LengthSpec::Fill)).children(buttons).into_any()
}

//! 三个内置工具页的原生表面，以及来源登录按钮。
//!
//! 文件导入和 Eagle 导入只派发文件状态机已有的对话框消息。
//! API Playground 只列出已加载的设计快照，不发请求，也不调用插件。
//! 来源登录按钮按认证声明派发插件调用，不解析二维码。
//! 设置页把有登录方法的插件收成一张顶部卡片，没有方法时不占位。

use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, Button, Stack};

use super::super::files::{FileContext, FileDialog, FilesMessage};
use super::super::{ShellMessage, ShellViewModel};
use super::support::{TOOL_API_PLAYGROUND, TOOL_EAGLE_IMPORTER, TOOL_FILE_MANAGER};
use super::{AdminMessage, SourceAuthCall, ToolPageEntry};
use crate::backend::services::repository::{ApiDesignSnapshot, PluginManifest};

/// 工具页上的一个导入按钮。禁用时不派发消息。
pub struct ImportAction {
    pub id: &'static str,
    pub label: &'static str,
    pub enabled: bool,
    pub message: FilesMessage,
}

/// 内置工具页。其它 id 打日志并留空。
pub fn tool_surface(model: &ShellViewModel, page: &ToolPageEntry) -> AnyView {
    let body = match page.id.as_str() {
        TOOL_FILE_MANAGER => import_page(
            model,
            page,
            "文件导入",
            "目标固定为当前工作区的当前目录。",
            "ZIP 只接受 .zip，解压后保留目录结构。",
        ),
        TOOL_EAGLE_IMPORTER => import_page(
            model,
            page,
            "Eagle 导入",
            "导入目标固定为当前工作区的当前目录。",
            "库路径在接下来的导入对话框里填写。",
        ),
        TOOL_API_PLAYGROUND => api_page(model, page),
        other => {
            eprintln!("Nana 工具页没有原生界面：{other}");
            Vec::new()
        }
    };
    widget(Stack::column(8.0)).children(body).key(format!("admin-tool-native-{}", page.id)).into_any()
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
    let enabled = import_block_reason(model).is_none();
    match page_id {
        TOOL_FILE_MANAGER => vec![
            ImportAction {
                id: "folder",
                label: "从文件夹导入",
                enabled,
                message: FilesMessage::OpenDialog(FileDialog::Import),
            },
            ImportAction {
                id: "zip",
                label: "从 ZIP 导入",
                enabled,
                message: FilesMessage::OpenDialog(FileDialog::ImportArchive),
            },
        ],
        TOOL_EAGLE_IMPORTER => vec![
            ImportAction {
                id: "copy",
                label: "复制导入",
                enabled,
                message: FilesMessage::OpenEagle("copy".into()),
            },
            ImportAction {
                id: "move",
                label: "剪切导入",
                enabled,
                message: FilesMessage::OpenEagle("move".into()),
            },
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

/// 只列出快照里的方法、路径和摘要。没有快照或端点为空时给同一句空状态。
pub fn api_lines(snapshot: Option<&ApiDesignSnapshot>) -> Vec<String> {
    let Some(snapshot) = snapshot.filter(|item| !item.endpoints.is_empty()) else {
        return vec!["还没有 API 设计快照".into()];
    };
    snapshot
        .endpoints
        .iter()
        .map(|endpoint| {
            let summary = endpoint.summary.trim();
            if summary.is_empty() {
                format!("{} {}", endpoint.method, endpoint.path)
            } else {
                format!("{} {} · {summary}", endpoint.method, endpoint.path)
            }
        })
        .collect()
}

fn import_target(model: &ShellViewModel) -> (String, String) {
    let repository = model
        .workspace
        .active_repository()
        .map(|item| item.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "未选择仓库".into());
    let directory = {
        let path = model.files.current_path.trim();
        if path.is_empty() { "/".into() } else { path.to_string() }
    };
    (repository, directory)
}

fn import_page(model: &ShellViewModel, page: &ToolPageEntry, eyebrow: &'static str, subline: &'static str, note: &'static str) -> Vec<AnyView> {
    let (repository, directory) = import_target(model);
    let id = page.id.as_str();
    let mut rows = vec![
        text(eyebrow).key(format!("admin-tool-eyebrow-{id}")).into_any(),
        text(page.label.clone()).key(format!("admin-tool-title-{id}")).into_any(),
        text(subline).key(format!("admin-tool-subline-{id}")).into_any(),
        text(format!("目标仓库 {repository}")).key(format!("admin-tool-repo-{id}")).into_any(),
        text(format!("目标目录 {directory}")).key(format!("admin-tool-dir-{id}")).into_any(),
    ];
    if let Some(reason) = import_block_reason(model) {
        rows.push(text(reason).key(format!("admin-tool-reason-{id}")).into_any());
    }
    rows.push(action_row(model, id));
    if !note.is_empty() {
        rows.push(text(note).key(format!("admin-tool-note-{id}")).into_any());
    }
    rows
}

fn action_row(model: &ShellViewModel, page_id: &str) -> AnyView {
    let buttons = import_actions(model, page_id)
        .into_iter()
        .map(|action| {
            let enabled = action.enabled;
            let message = action.message;
            let id = action.id;
            widget(Button::new(action.label).disabled(!enabled))
                .key(format!("admin-tool-action-{page_id}-{id}"))
                .on_cx(move |_, _: &Activate, cx| {
                    if let Some(outgoing) = import_message(enabled, message.clone()) {
                        cx.dispatch_program(outgoing);
                    }
                })
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(Stack::row(8.0).wrap(true)).children(buttons).into_any()
}

/// 来源登录的一个按钮。没有对应方法名时不出现。
pub struct SourceAuthAction {
    pub id: &'static str,
    pub label: &'static str,
    pub slot: SourceAuthCall,
}

const SOURCE_AUTH_BUTTONS: &[(&str, &str, &str, SourceAuthCall)] = &[
    ("create", "创建登录会话", "createSessionMethod", SourceAuthCall::CreateSession),
    ("status", "查询登录状态", "statusMethod", SourceAuthCall::Status),
    ("clear", "退出登录", "clearMethod", SourceAuthCall::Clear),
];

/// 当前插件认证对象里实际声明了的登录按钮。
pub fn source_auth_actions(plugin: &PluginManifest) -> Vec<SourceAuthAction> {
    SOURCE_AUTH_BUTTONS
        .iter()
        .filter(|(_, _, key, _)| super::support::named_auth_method(plugin, key).is_some())
        .map(|(id, label, _, slot)| SourceAuthAction { id, label, slot: *slot })
        .collect()
}

/// 有登录方法的插件。没有方法名的不进设置页顶部卡片。
pub fn source_login_plugin_ids(plugins: &[PluginManifest]) -> Vec<String> {
    plugins
        .iter()
        .filter(|plugin| !source_auth_actions(plugin).is_empty())
        .map(|plugin| plugin.plugin_id.clone())
        .collect()
}

/// 设置页顶部的来源登录卡片。一个都没有时不占位。
pub fn source_login_card(plugins: &[PluginManifest]) -> Option<AnyView> {
    let mut rows = Vec::new();
    for plugin in plugins {
        let Some(buttons) = source_auth_row(plugin) else {
            continue;
        };
        if let Some(summary) = super::support::source_account_summary(plugin) {
            rows.push(text(summary).key(format!("admin-source-summary-{}", plugin.plugin_id)).into_any());
        }
        rows.push(buttons);
    }
    if rows.is_empty() {
        return None;
    }
    Some(super::super::workbench::section_card("来源登录", rows))
}

/// 登录按钮行。一个方法都没有时不占位。
pub fn source_auth_row(plugin: &PluginManifest) -> Option<AnyView> {
    let actions = source_auth_actions(plugin);
    if actions.is_empty() {
        return None;
    }
    let plugin_id = plugin.plugin_id.clone();
    let buttons = actions
        .into_iter()
        .map(|action| {
            let id = plugin_id.clone();
            let slot = action.slot;
            widget(Button::new(action.label))
                .key(format!("admin-source-auth-{}-{plugin_id}", action.id))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::CallSourceAuth { plugin_id: id.clone(), slot }));
                })
                .into_any()
        })
        .collect::<Vec<_>>();
    Some(widget(Stack::row(8.0).wrap(true)).children(buttons).key(format!("admin-source-auth-{plugin_id}")).into_any())
}

fn api_page(model: &ShellViewModel, page: &ToolPageEntry) -> Vec<AnyView> {
    let mut rows = vec![
        text("API Playground").key("admin-tool-api-eyebrow").into_any(),
        text(page.label.clone()).key("admin-tool-api-title").into_any(),
    ];
    for (index, line) in api_lines(model.admin.api_design.as_ref()).into_iter().enumerate() {
        rows.push(text(line).key(format!("admin-tool-api-line-{index}")).into_any());
    }
    rows
}

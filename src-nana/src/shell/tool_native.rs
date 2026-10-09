//! 拓展页里的文件导入和 Eagle 导入工具页。
//!
//! 内容照 `External/Plugins/{file-manager,eagle-importer}/src/register.js`：眉题、插件名标题、
//! 说明、目标仓库和目录、不能导入的原因、导入按钮和备注。Vue 的 `.tool-page-shell` 没有任何
//! 样式（内容贴着面板边、标题是浏览器默认 2em），这里按 API Playground 的版式画：内边距 18、
//! 22 号标题、卡片包住目标信息。导入走文件面板已有的导入对话框和状态机。
//!
//! 页面按工具页 id 建一次，只读 [`ImportSignals`]：标题、目标、原因、错误和按钮能不能点都是绑定；
//! 按钮的处理器点下去时现读能不能导入，不能时不进文件状态机。

use nana_ui::runtime::view::{signal, widget, AnyView, IntoView, Signal};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, Stack};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};

use super::super::files::{FileContext, FileDialog, FilesMessage};
use super::super::{ShellMessage, ShellViewModel};
use super::bind::ActionDisabled;
use super::style::{self, action, column, label, mono, pad, row, wrapping, Tone};
use super::support::{TOOL_EAGLE_IMPORTER, TOOL_FILE_MANAGER};

/// 工具页上的一个导入按钮：标识、文字、语气和要发的文件消息。能不能点看 [`ImportView::enabled`]，
/// 不能点时不派发消息。
pub(crate) struct ImportButton {
    pub id: &'static str,
    pub label: &'static str,
    pub tone: Tone,
    pub message: FilesMessage,
}

/// 导入页要显示的东西。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ImportView {
    pub title: String,
    pub repository: String,
    pub directory: String,
    /// 不能导入的原因。
    pub reason: Option<&'static str>,
    /// 文件面板最近的错误。
    pub error: String,
    /// 导入按钮能不能点。
    pub enabled: bool,
}

impl ImportView {
    /// 从 ViewModel 取 `page_id` 这个导入页的投影。
    pub(crate) fn project(model: &ShellViewModel, page_id: &str) -> Self {
        let (repository, directory) = import_target(model);
        let reason = import_block_reason(model);
        Self {
            title: plugin_name(model, page_id).unwrap_or_else(|| if page_id == TOOL_EAGLE_IMPORTER { "Eagle Importer".into() } else { "File Manager".into() }),
            repository,
            directory,
            reason,
            error: model.files.error.clone(),
            enabled: reason.is_none() && !model.files.mutating,
        }
    }
}

/// 导入页的信号。
#[derive(Clone, Copy)]
pub(crate) struct ImportSignals {
    view: Signal<ImportView>,
}

impl ImportSignals {
    /// 空的信号，进路由之前由同步写入第一份投影。
    pub(crate) fn new() -> Self {
        Self { view: signal(ImportView::default()) }
    }

    /// 写入当前显示的导入页的投影，没变时不写。
    pub(crate) fn write(&self, model: &ShellViewModel, page_id: &str) {
        self.view.try_set_if_changed(ImportView::project(model, page_id));
    }
}

/// 内置的文件导入或 Eagle 导入页。
pub(super) fn import_page(id: &str, signals: ImportSignals) -> AnyView {
    let view = signals.view;
    let (eyebrow, subline) = if id == TOOL_EAGLE_IMPORTER {
        ("Eagle 导入", "导入目标固定为当前工作区的当前目录。")
    } else {
        ("文件导入", "目标固定为当前工作区的当前目录。")
    };
    let text = move |pick: fn(&ImportView) -> &String| move || view.with(|view| pick(view).clone());
    let mut body = vec![
        widget(column(0.0))
            .children((
                widget(style::eyebrow_text(eyebrow)).key(format!("admin-tool-eyebrow-{id}")),
                widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0))
                    .children((style::bound(style::label_lh(String::new(), 22.0, 700, Role::Text, 1.25), text(|view| &view.title)).key(format!("admin-tool-title-{id}")),)),
                widget(pad(column(0.0), 8.0, 0.0, 0.0, 0.0)).children((widget(wrapping(label(subline, 13.0, 400, Role::Muted))).key(format!("admin-tool-subline-{id}")),)),
            ))
            .into_any(),
        target_card(vec![
            target_row("目标仓库", style::bound(label(String::new(), 14.0, 600, Role::Text), text(|view| &view.repository)).key(format!("admin-tool-repo-{id}")).into_any()),
            target_row("目标目录", code(text(|view| &view.directory), format!("admin-tool-dir-{id}"))),
        ]),
        style::state_notice(move || view.with(|view| view.reason.unwrap_or_default().to_string()), false, "admin-tool-reason")
            .visible(move || view.with(|view| view.reason.is_some()))
            .into_any(),
        style::state_notice(text(|view| &view.error), true, "admin-tool-error").visible(move || view.with(|view| !view.error.is_empty())).into_any(),
    ];
    if id == TOOL_EAGLE_IMPORTER {
        body.push(target_card(vec![target_row("EagleLibrary", code("尚未选择", "admin-tool-eagle-path".into())), action_row(id, view)]));
    } else {
        body.push(action_row(id, view));
        body.push(widget(wrapping(label("ZIP 首版固定支持 .zip，并按解压导入保留内部目录结构。", 12.0, 400, Role::Muted))).key(format!("admin-tool-note-{id}")).into_any());
    }
    widget(pad(column(14.0), 18.0, 18.0, 18.0, 18.0)).children(body).key(format!("admin-tool-native-{id}")).into_any()
}

fn plugin_name(model: &ShellViewModel, tool_id: &str) -> Option<String> {
    model.admin.plugins.iter().find(|plugin| plugin.plugin_id == tool_id).map(|plugin| plugin.name.clone())
}

/// 目标卡片：主背景、`border-soft` 边线、md 圆角，内边距 12/14，行间 8。
fn target_card(rows: Vec<AnyView>) -> AnyView {
    widget(pad(column(8.0), 12.0, 14.0, 12.0, 14.0).surface(Role::Background).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Md))
        .children(rows)
        .into_any()
}

fn target_row(name: &str, value: AnyView) -> AnyView {
    widget(row(8.0).wrap(true)).children((widget(label(name, 13.0, 400, Role::Muted)), value)).into_any()
}

/// 行内代码：等宽 12 号、`bg-subtle`、`border-soft` 边线、xs 圆角。文字可以绑定。
fn code(text: impl nana_ui::runtime::view::IntoProp<String>, key: String) -> AnyView {
    widget(pad(row(0.0), 1.0, 6.0, 1.0, 6.0).surface(Role::Subtle).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Xs))
        .children((style::bound(mono(String::new(), 12.0, 400, Role::Text), text).key(key),))
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
pub(crate) fn import_buttons(page_id: &str) -> Vec<ImportButton> {
    let button = |id, label, tone, message| ImportButton { id, label, tone, message };
    match page_id {
        TOOL_FILE_MANAGER => vec![
            button("folder", "从文件夹导入", Tone::Primary, FilesMessage::OpenDialog(FileDialog::Import)),
            button("zip", "从 ZIP 导入", Tone::Plain, FilesMessage::OpenDialog(FileDialog::ImportArchive)),
        ],
        TOOL_EAGLE_IMPORTER => vec![
            button("copy", "复制导入", Tone::Primary, FilesMessage::OpenEagle("copy".into())),
            button("move", "剪切导入", Tone::Plain, FilesMessage::OpenEagle("move".into())),
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

/// 导入按钮行。按钮能不能点是绑定，处理器点下去时现读。
fn action_row(page_id: &str, view: Signal<ImportView>) -> AnyView {
    let enabled = move || view.with(|view| view.enabled);
    let buttons = import_buttons(page_id)
        .into_iter()
        .map(|button| {
            let message = button.message;
            widget(action(button.label, None, button.tone, false))
                .prop::<bool, ActionDisabled>(move || !enabled())
                .key(format!("admin-tool-action-{page_id}-{}", button.id))
                .on_cx(move |_, _: &Activate, cx| {
                    if let Some(outgoing) = import_message(view.with_untracked(|view| view.enabled), message.clone()) {
                        cx.dispatch_program_all(outgoing);
                    }
                })
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(Stack::row(8.0).wrap(true).align(AlignSpec::Center).width(LengthSpec::Fill)).children(buttons).into_any()
}

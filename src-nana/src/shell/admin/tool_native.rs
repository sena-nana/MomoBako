//! 拓展页里的文件导入和 Eagle 导入工具页。
//!
//! 内容照 `External/Plugins/{file-manager,eagle-importer}/src/register.js`：眉题、插件名标题、
//! 说明、目标仓库和目录、不能导入的原因、导入按钮和备注。按 API Playground 的版式画：内边距 18、
//! 22 号标题、卡片包住目标信息。文件导入走系统选择器。从这一页提交成功后，文件夹按钮写成
//! 「处理中...」，协议成功再写「文件夹导入完成。 目标目录：…」或「ZIP 导入完成。 目标目录：…」。
//!
//! Eagle 导入页另有「选择目录」，走和下载目录一样的系统文件夹对话框。选中的路径写进
//! `eagle_library_path`，没选时这一行仍是「尚未选择」。复制和剪切只在已有路径时提交现有的
//! Eagle 导入协议。协议真正成功才写「复制导入完成，目标目录：…」或「剪切导入完成，目标目录：…」。
//! 根目录和只有斜杠的路径，完成提示跟卡片一样写成「/」。失败和取消不写。
//! 复制按钮只在这次导入进行中写成「处理中...」，剪切按钮始终是「剪切导入」。
//! 结果卡只要响应里的 `summary` 是真值就出现，眉题写「导入结果」。
//! 九个计数缺了或是 null 时显示 0，这个 0 不是导入出来的条数；小数和负数不画。
//! 警告区只在 `warnings` 是非空数组时出现，每条写类型和一句说明，最多 20 条。
//!
//! 页面按工具页 id 建一次，只读 [`ImportSignals`]：标题、目标、原因、错误和按钮能不能点都是绑定；
//! 按钮的处理器点下去时现读能不能导入，不能时不进文件状态机。

use nana_ui::runtime::view::{AnyView, IntoView, Signal, fields, signal, widget};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, Stack};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};

use super::super::files::{
    EagleImportReport, EagleImportWarning, FileContext, FileDialog, FilesMessage,
};
use super::super::{ShellMessage, ShellViewModel};
use super::AdminMessage;
use super::bind::ActionDisabled;
use super::style::{self, Tone, action, column, label, mono, pad, row, wrapping};
use super::support::{TOOL_EAGLE_IMPORTER, TOOL_FILE_MANAGER};
use crate::backend::services::repository::{
    EagleLibraryImportResponse, EagleLibraryImportSummary, EagleLibraryImportWarning,
};

mod file_import;

pub(crate) use file_import::{
    arm as arm_tool_file_import, begin as begin_tool_file_import,
    drop_arm as drop_tool_file_import, note_settled as note_file_import_settled,
};

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
    /// 导入按钮能不能点。Eagle 的「选择目录」用这个；复制和剪切还要已有库路径。
    pub enabled: bool,
    /// Eagle 导入页选中的库目录。空的时候页面显示「尚未选择」。
    pub library_path: String,
    /// 协议响应里的导入结果。没有真值 `summary` 时不画结果卡。
    pub report: Option<EagleImportReport>,
    /// 这次工具页导入还没回来。复制按钮据此写成「处理中...」。
    pub busy: bool,
    /// 成功提示。没有真实目标目录、失败或取消时为空。
    pub notice: String,
}

impl ImportView {
    /// 从 ViewModel 取 `page_id` 这个导入页的投影。
    pub(crate) fn project(model: &ShellViewModel, page_id: &str) -> Self {
        let (repository, directory) = import_target(model);
        let reason = import_block_reason(model);
        Self {
            title: plugin_name(model, page_id).unwrap_or_else(|| {
                if page_id == TOOL_EAGLE_IMPORTER {
                    "Eagle Importer".into()
                } else {
                    "File Manager".into()
                }
            }),
            repository,
            directory,
            reason,
            error: model.files.error.clone(),
            enabled: reason.is_none() && !model.files.mutating,
            library_path: model.admin.eagle_library_path.clone(),
            report: (page_id == TOOL_EAGLE_IMPORTER)
                .then(|| model.admin.eagle_import.clone())
                .flatten(),
            busy: if page_id == TOOL_EAGLE_IMPORTER {
                model.admin.eagle_importing
            } else {
                model.admin.file_import_busy.is_some()
            },
            notice: if page_id == TOOL_EAGLE_IMPORTER {
                model.admin.eagle_notice.clone()
            } else if page_id == TOOL_FILE_MANAGER {
                model.admin.file_import_notice.clone()
            } else {
                String::new()
            },
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
        Self {
            view: signal(ImportView::default()),
        }
    }

    /// 写入当前显示的导入页的投影，没变时不写。
    pub(crate) fn write(&self, model: &ShellViewModel, page_id: &str) {
        self.view
            .try_set_if_changed(ImportView::project(model, page_id));
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
    let mut body =
        vec![
            widget(column(0.0))
                .children((
                    widget(style::eyebrow_text(eyebrow)).key(format!("admin-tool-eyebrow-{id}")),
                    widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0)).children((style::bound(
                        style::label_lh(String::new(), 22.0, 700, Role::Text, 1.25),
                        text(|view| &view.title),
                    )
                    .key(format!("admin-tool-title-{id}")),)),
                    widget(pad(column(0.0), 8.0, 0.0, 0.0, 0.0))
                        .children((widget(wrapping(label(subline, 13.0, 400, Role::Muted)))
                            .key(format!("admin-tool-subline-{id}")),)),
                ))
                .into_any(),
            target_card(vec![
                target_row(
                    "目标仓库",
                    style::bound(
                        label(String::new(), 14.0, 600, Role::Text),
                        text(|view| &view.repository),
                    )
                    .key(format!("admin-tool-repo-{id}"))
                    .into_any(),
                ),
                target_row(
                    "目标目录",
                    code(text(|view| &view.directory), format!("admin-tool-dir-{id}")),
                ),
            ]),
            style::state_notice(
                move || view.with(|view| view.reason.unwrap_or_default().to_string()),
                false,
                "admin-tool-reason",
            )
            .visible(move || view.with(|view| view.reason.is_some()))
            .into_any(),
            style::state_notice(text(|view| &view.error), true, "admin-tool-error")
                .visible(move || view.with(|view| !view.error.is_empty()))
                .into_any(),
        ];
    if id == TOOL_EAGLE_IMPORTER {
        let library = move || view.with(|view| eagle_path_label(&view.library_path));
        let notice = move || view.with(|view| view.notice.clone());
        body.push(
            style::state_notice(notice, false, "admin-tool-eagle-notice")
                .visible(move || view.with(|view| !view.notice.is_empty()))
                .into_any(),
        );
        body.push(target_card(vec![
            target_row(
                "EagleLibrary",
                code(library, "admin-tool-eagle-path".into()),
            ),
            eagle_action_row(id, view),
        ]));
        body.push(eagle_result_card(view));
    } else {
        body.push(file_import::notice(view));
        body.push(file_import::action_row(id, view));
        body.push(
            widget(wrapping(label(
                "ZIP 首版固定支持 .zip，并按解压导入保留内部目录结构。",
                12.0,
                400,
                Role::Muted,
            )))
            .key(format!("admin-tool-note-{id}"))
            .into_any(),
        );
    }
    widget(pad(column(14.0), 18.0, 18.0, 18.0, 18.0))
        .children(body)
        .key(format!("admin-tool-native-{id}"))
        .into_any()
}

fn plugin_name(model: &ShellViewModel, tool_id: &str) -> Option<String> {
    model
        .admin
        .plugins
        .iter()
        .find(|plugin| plugin.plugin_id == tool_id)
        .map(|plugin| plugin.name.clone())
}

/// 目标卡片：主背景、`border-soft` 边线、md 圆角，内边距 12/14，行间 8。
fn target_card(rows: Vec<AnyView>) -> AnyView {
    widget(
        pad(column(8.0), 12.0, 14.0, 12.0, 14.0)
            .surface(Role::Background)
            .border(Role::BorderSoft, 1.0)
            .radius(RadiusTier::Md),
    )
    .children(rows)
    .into_any()
}

fn target_row(name: &str, value: AnyView) -> AnyView {
    widget(row(8.0).wrap(true))
        .children((widget(label(name, 13.0, 400, Role::Muted)), value))
        .into_any()
}

/// 行内代码：等宽 12 号、`bg-subtle`、`border-soft` 边线、xs 圆角。文字可以绑定。
fn code(text: impl nana_ui::runtime::view::IntoProp<String>, key: String) -> AnyView {
    widget(
        pad(row(0.0), 1.0, 6.0, 1.0, 6.0)
            .surface(Role::Subtle)
            .border(Role::BorderSoft, 1.0)
            .radius(RadiusTier::Xs),
    )
    .children((style::bound(mono(String::new(), 12.0, 400, Role::Text), text).key(key),))
    .into_any()
}

/// 不能导入时返回原因。导入进行中只禁用按钮，不当成不能导入。
pub fn import_block_reason(model: &ShellViewModel) -> Option<&'static str> {
    let ctx = FileContext::from_model(model);
    // `can_import` 把进行中也算不能导入。这里的原因不看进行中，把进行中仍看成目标可用。
    let mut allowed = model.files.can_import(&ctx) || model.files.mutating;
    // 四条原因看的是面板。文件快照仍是回收站、面板已经离开时，那四条都对不上，
    // 这次又不能按普通目录导入，落到「当前目标不可导入。」。进行中不改这句。
    if model.admin.active_tool_page_id.as_deref() == Some(TOOL_EAGLE_IMPORTER)
        && model.files.trash
        && !ctx.trash
        && !model.files.mutating
    {
        allowed = false;
    }
    block_reason(&ctx, allowed)
}

/// 四条具体原因对不上、目标又确实不能导入时，用「当前目标不可导入。」。
fn block_reason(ctx: &FileContext, allowed: bool) -> Option<&'static str> {
    if ctx
        .repo_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .is_none()
    {
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
    if allowed {
        return None;
    }
    Some("当前目标不可导入。")
}

/// 文件导入页的按钮。只携带已有的文件消息。Eagle 导入页的按钮在 [`eagle_action_row`]。
pub(crate) fn import_buttons(page_id: &str) -> Vec<ImportButton> {
    let button = |id, label, tone, message| ImportButton {
        id,
        label,
        tone,
        message,
    };
    match page_id {
        TOOL_FILE_MANAGER => vec![
            button(
                "folder",
                "从文件夹导入",
                Tone::Primary,
                FilesMessage::OpenDialog(FileDialog::Import),
            ),
            button(
                "zip",
                "从 ZIP 导入",
                Tone::Plain,
                FilesMessage::OpenDialog(FileDialog::ImportArchive),
            ),
        ],
        TOOL_EAGLE_IMPORTER => Vec::new(),
        other => {
            eprintln!("Nana 工具页没有导入按钮：{other}");
            Vec::new()
        }
    }
}

/// 导入结果投影成结果卡：计数原样取摘要，警告最多列前 20 条。
pub(crate) fn eagle_import_report(response: &EagleLibraryImportResponse) -> EagleImportReport {
    EagleImportReport {
        summary: response.summary.clone(),
        warning_count: response.warnings.len(),
        warnings: response
            .warnings
            .iter()
            .take(20)
            .map(warning_line)
            .collect(),
    }
}

/// 一条警告：类型为空时写 `warning`；说明依次取原因、素材 id、来源 id，都没有时写细节 JSON。
fn warning_line(warning: &EagleLibraryImportWarning) -> EagleImportWarning {
    let kind = if warning.warning_type.is_empty() {
        "warning".to_string()
    } else {
        warning.warning_type.clone()
    };
    let text = [&warning.reason, &warning.asset_id, &warning.source_id]
        .into_iter()
        .flatten()
        .find(|text| !text.trim().is_empty())
        .cloned()
        .or_else(|| {
            warning
                .details
                .as_ref()
                .filter(|details| !details.is_null())
                .map(ToString::to_string)
        })
        .unwrap_or_default();
    EagleImportWarning { kind, text }
}

/// 导入结果卡：导入完成后出现，九项计数各一行，警告区只在有警告时出现。
fn eagle_result_card(view: Signal<ImportView>) -> AnyView {
    let shown = move || view.with(|view| view.report.is_some());
    let mut rows = vec![
        widget(style::eyebrow_text("导入结果"))
            .key("admin-tool-eagle-result-eyebrow")
            .into_any(),
        count_row(view, "文件", "admin-tool-eagle-files", |summary| {
            summary.imported_files
        }),
        count_row(view, "目录", "admin-tool-eagle-dirs", |summary| {
            summary.imported_directories
        }),
        count_row(view, "回收站", "admin-tool-eagle-trash", |summary| {
            summary.imported_trash_entries
        }),
        count_row(
            view,
            "快捷入口",
            "admin-tool-eagle-shortcuts",
            |summary| summary.imported_shortcuts,
        ),
        count_row(
            view,
            "智能文件夹",
            "admin-tool-eagle-smart-folders",
            |summary| summary.imported_smart_folders,
        ),
        count_row(view, "仓库动作", "admin-tool-eagle-actions", |summary| {
            summary.imported_repository_actions
        }),
        count_row(view, "标签组", "admin-tool-eagle-tag-groups", |summary| {
            summary.imported_tag_groups
        }),
        count_row(view, "Alias 组", "admin-tool-eagle-aliases", |summary| {
            summary.imported_alias_groups
        }),
        count_row(
            view,
            "硬链接组",
            "admin-tool-eagle-hardlinks",
            |summary| summary.imported_hardlink_groups,
        ),
        warning_title(view),
    ];
    rows.extend((0..20).map(|index| warning_row(view, index)));
    widget(column(0.0))
        .children((target_card(rows),))
        .visible(shown)
        .key("admin-tool-eagle-result")
        .into_any()
}

/// 结果卡上的一项计数。
fn count_row(
    view: Signal<ImportView>,
    name: &'static str,
    key: &'static str,
    read: fn(&EagleLibraryImportSummary) -> usize,
) -> AnyView {
    let text = move || {
        view.with(|view| {
            view.report
                .as_ref()
                .map(|report| read(&report.summary).to_string())
                .unwrap_or_default()
        })
    };
    widget(row(8.0).wrap(true))
        .children((
            widget(label(name, 13.0, 400, Role::Muted)),
            code(text, key.into()),
        ))
        .into_any()
}

/// 「警告 N」，N 是警告数组的长度。数组为空时不画。
fn warning_title(view: Signal<ImportView>) -> AnyView {
    let text_view = view;
    let shown_view = view;
    let text = move || {
        text_view.with(|view| {
            view.report
                .as_ref()
                .filter(|report| report.warning_count > 0)
                .map(|report| format!("警告 {}", report.warning_count))
                .unwrap_or_default()
        })
    };
    let shown = move || {
        shown_view.with(|view| {
            view.report
                .as_ref()
                .is_some_and(|report| report.warning_count > 0)
        })
    };
    style::bound(label(String::new(), 11.0, 600, Role::Faint), text)
        .key("admin-tool-eagle-warnings")
        .visible(shown)
        .into_any()
}

/// 一条警告：类型在左，说明在右。超过 20 条的不建这一行。
fn warning_row(view: Signal<ImportView>, index: usize) -> AnyView {
    let kind_view = view;
    let text_view = view;
    let shown_view = view;
    let kind = move || {
        kind_view.with(|view| {
            warning_at(view, index)
                .map(|item| item.kind.clone())
                .unwrap_or_default()
        })
    };
    let text = move || {
        text_view.with(|view| {
            warning_at(view, index)
                .map(|item| item.text.clone())
                .unwrap_or_default()
        })
    };
    let shown = move || shown_view.with(|view| warning_at(view, index).is_some());
    let key = format!("admin-tool-eagle-warning-{index}");
    widget(row(8.0).wrap(true))
        .children((
            style::bound(label(String::new(), 13.0, 400, Role::Muted), kind)
                .key(format!("{key}-kind")),
            code(text, format!("{key}-text")),
        ))
        .visible(shown)
        .key(key)
        .into_any()
}

fn warning_at(view: &ImportView, index: usize) -> Option<&EagleImportWarning> {
    view.report
        .as_ref()
        .and_then(|report| report.warnings.get(index))
}

/// 没选目录时显示「尚未选择」，选中后显示去掉首尾空白的真实路径。
pub fn eagle_path_label(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        "尚未选择".into()
    } else {
        path.to_string()
    }
}

/// 「选择目录」。当前不能导入时不打开对话框。
pub fn choose_eagle_library(enabled: bool) -> Option<ShellMessage> {
    if !enabled {
        eprintln!("Nana 工具页当前不能选择 EagleLibrary 目录");
        return None;
    }
    Some(ShellMessage::Admin(AdminMessage::ChooseEagleLibrary))
}

/// 「复制导入」或「剪切导入」。没有库路径、模式不对或当前不能导入时不提交。
pub fn eagle_import_message(view: &ImportView, mode: &str) -> Option<ShellMessage> {
    if !view.enabled {
        eprintln!("Nana 工具页当前不能导入");
        return None;
    }
    if view.library_path.trim().is_empty() {
        eprintln!("Nana 请先选择 EagleLibrary 目录");
        return None;
    }
    if mode != "copy" && mode != "move" {
        eprintln!("Nana Eagle 导入模式无效：{mode}");
        return None;
    }
    Some(ShellMessage::Admin(AdminMessage::RunEagleImport(
        mode.to_string(),
    )))
}

/// 排队 EagleLibrary 目录对话框。变更进行中或当前不能导入时只记日志。
pub(super) fn begin_library_dialog(model: &mut ShellViewModel) {
    if model.files.mutating {
        eprintln!("Nana 文件变更进行中，不能选择 EagleLibrary 目录");
        return;
    }
    if import_block_reason(model).is_some() {
        eprintln!("Nana 工具页当前不能选择 EagleLibrary 目录");
        return;
    }
    model.input.queue_eagle_library_dialog();
}

/// 把对话框交回的目录写进页面。空白视为没选，保留原来的路径。
pub(super) fn set_library_path(model: &mut ShellViewModel, path: &str) {
    let path = path.trim();
    if path.is_empty() {
        eprintln!("Nana EagleLibrary 目录是空的");
        return;
    }
    model.admin.eagle_library_path = path.to_string();
}

/// 用页面上的库路径提交导入。没有路径或不满足导入条件时不调用协议，页面上也不写成功。
pub(super) fn run_import(model: &mut ShellViewModel, mode: &str) {
    if model.files.mutating {
        eprintln!("Nana 文件变更进行中，不能导入 Eagle");
        return;
    }
    let library_path = model.admin.eagle_library_path.trim().to_string();
    if library_path.is_empty() {
        model
            .files
            .note_error("请先选择 EagleLibrary 目录。".into());
        return;
    }
    if mode != "copy" && mode != "move" {
        eprintln!("Nana Eagle 导入模式无效：{mode}");
        return;
    }
    if let Some(reason) = import_block_reason(model) {
        model.files.note_error(reason.to_string());
        return;
    }
    let directory = submitted_directory(&model.files.current_path);
    if !model
        .files
        .import_eagle_library(&FileContext::from_model(model), mode, &library_path)
    {
        eprintln!("Nana Eagle 导入没有提交，不显示完成提示");
        return;
    }
    // 请求发出时清掉上一次的提示，成功回来才按这次的模式和目录重写。
    model.admin.eagle_notice.clear();
    model.admin.eagle_importing = true;
    model.admin.eagle_pending_mode = mode.to_string();
    model.admin.eagle_pending_directory = directory.unwrap_or_default();
}

/// 这次提交时的当前目录。根目录和只有斜杠记成空，完成提示再写成卡片上的「/」。
fn submitted_directory(current_path: &str) -> Option<String> {
    let path = super::super::files::normalize_path(current_path);
    if path.is_empty() { None } else { Some(path) }
}

/// 工具页这次导入的协议结果。没从工具页提交的结果不写提示。
/// 成功时按提交时的目录写完成提示；根目录写成「/」，和目标卡片一致。失败清掉上一句。
pub(crate) fn note_import_settled(model: &mut ShellViewModel, succeeded: bool) {
    if !std::mem::take(&mut model.admin.eagle_importing) {
        return;
    }
    let directory = std::mem::take(&mut model.admin.eagle_pending_directory);
    let mode = std::mem::take(&mut model.admin.eagle_pending_mode);
    if !succeeded {
        model.admin.eagle_notice.clear();
        return;
    }
    // match 的右花括号和 let-else 的 else 相邻时，当前 rustc 不接受，括起来才是同一表达式。
    let Some(verb) = (match mode.as_str() {
        "copy" => Some("复制导入"),
        "move" => Some("剪切导入"),
        other => {
            eprintln!("Nana Eagle 导入成功但模式无效，不显示完成提示：{other}");
            None
        }
    }) else {
        return;
    };
    // 空路径在目标卡片上是「/」，完成提示写同一个字。
    let directory = if directory.is_empty() {
        "/".to_string()
    } else {
        directory
    };
    model.admin.eagle_notice = format!("{verb}完成，目标目录：{directory}");
}

/// 复制按钮忙时是「处理中...」，空闲时是「复制导入」。剪切按钮忙时也不换字。
pub fn eagle_submit_label(view: &ImportView, mode: &str) -> &'static str {
    if mode == "copy" && view.busy {
        "处理中..."
    } else if mode == "move" {
        "剪切导入"
    } else {
        "复制导入"
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
    let directory = file_import::directory_label(model);
    (repository, directory)
}

/// Eagle 导入页的三个按钮：选择目录、复制导入、剪切导入。
fn eagle_action_row(page_id: &str, view: Signal<ImportView>) -> AnyView {
    let buttons = vec![
        eagle_choose_button(page_id, view),
        eagle_submit_button(page_id, view, "copy", "复制导入", Tone::Primary),
        eagle_submit_button(page_id, view, "move", "剪切导入", Tone::Plain),
    ];
    widget(
        Stack::row(8.0)
            .wrap(true)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill),
    )
    .children(buttons)
    .into_any()
}

fn eagle_choose_button(page_id: &str, view: Signal<ImportView>) -> AnyView {
    widget(action("选择目录", None, Tone::Plain, false))
        .prop::<bool, ActionDisabled>(move || view.with(|view| !view.enabled))
        .key(format!("admin-tool-action-{page_id}-choose"))
        .on_cx(move |_, _: &Activate, cx| {
            let enabled = view.with_untracked(|view| view.enabled);
            if let Some(outgoing) = choose_eagle_library(enabled) {
                cx.dispatch_program_all(outgoing);
            }
        })
        .into_any()
}

fn eagle_submit_button(
    page_id: &str,
    view: Signal<ImportView>,
    mode: &'static str,
    label: &'static str,
    tone: Tone,
) -> AnyView {
    let label_view = view;
    let text = move || label_view.with(|view| eagle_submit_label(view, mode).to_string());
    widget(action(label, None, tone, false))
        .prop::<String, fields::button::label>(text)
        .prop::<bool, ActionDisabled>(move || {
            view.with(|view| !view.enabled || view.library_path.trim().is_empty())
        })
        .key(format!("admin-tool-action-{page_id}-{mode}"))
        .on_cx(move |_, _: &Activate, cx| {
            let snapshot = view.with_untracked(|view| view.clone());
            if let Some(outgoing) = eagle_import_message(&snapshot, mode) {
                cx.dispatch_program_all(outgoing);
            }
        })
        .into_any()
}

#[cfg(test)]
mod tests {
    use nana_ui::FileDialogKind;
    use nana_ui_platform::WindowId;
    use nana_ui_platform::host::WindowCommand;

    use super::super::super::files::{
        EagleImportWarning, FileContext, FileDialog, FilesEffect, FilesMessage,
    };
    use super::super::super::input::InputMessage;
    use super::super::super::workspace::WorkspaceRepository;
    use super::super::super::{ShellMessage, ShellViewModel};
    use super::super::AdminMessage;
    use super::super::support::TOOL_EAGLE_IMPORTER;
    use super::{
        EagleLibraryImportResponse, EagleLibraryImportSummary, EagleLibraryImportWarning,
        ImportView, block_reason, choose_eagle_library, eagle_import_message, eagle_import_report,
        eagle_path_label, eagle_submit_label, import_block_reason,
    };

    fn send(model: &mut ShellViewModel, message: ShellMessage) {
        model.reduce(message);
    }

    /// 可写的当前仓库。没选库目录时导入按钮不能提交。
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
        model.files.current_path = "inbox".into();
        model
    }

    /// 取走刚排队的 EagleLibrary 目录对话框，并核对它是选文件夹。
    fn dialog_id(model: &mut ShellViewModel) -> u64 {
        let commands = model.input.take_platform_commands(WindowId(1), false);
        match commands.as_slice() {
            [WindowCommand::OpenFileDialog { request, .. }] => {
                assert_eq!(request.kind, FileDialogKind::PickFolder);
                assert_eq!(request.title.as_deref(), Some("选择 EagleLibrary 目录"));
                request.id
            }
            other => panic!("expected folder dialog, got {other:?}"),
        }
    }

    #[test]
    fn eagle_library_picker_writes_the_chosen_path_and_import_stays_honest() {
        let mut model = ShellViewModel::default();
        assert!(choose_eagle_library(false).is_none());
        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
        );
        assert!(
            model
                .input
                .take_platform_commands(WindowId(1), false)
                .is_empty(),
            "没有仓库时不打开目录对话框"
        );

        let mut model = writable();
        let page = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
        assert_eq!(eagle_path_label(&page.library_path), "尚未选择");
        assert!(eagle_import_message(&page, "copy").is_none());
        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::RunEagleImport("copy".into())),
        );
        assert_eq!(model.files.error, "请先选择 EagleLibrary 目录。");
        assert!(model.files.take_effects().is_empty());
        assert!(model.operation.shown().is_none());
        assert_eq!(model.files.dialog, FileDialog::Closed);

        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
        );
        let request_id = dialog_id(&mut model);
        send(
            &mut model,
            ShellMessage::Input(InputMessage::FileDialogCompleted {
                request_id,
                paths: Vec::new(),
                failed: None,
            }),
        );
        assert!(model.admin.eagle_library_path.is_empty());
        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
        );
        let request_id = dialog_id(&mut model);
        send(
            &mut model,
            ShellMessage::Input(InputMessage::FileDialogCompleted {
                request_id,
                paths: vec!["  ".into()],
                failed: None,
            }),
        );
        assert!(model.admin.eagle_library_path.is_empty());
        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
        );
        let request_id = dialog_id(&mut model);
        send(
            &mut model,
            ShellMessage::Input(InputMessage::FileDialogCompleted {
                request_id,
                paths: vec!["D:/Eagle".into()],
                failed: Some("Busy".into()),
            }),
        );
        assert!(
            model.admin.eagle_library_path.is_empty(),
            "选择失败不写入路径"
        );

        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
        );
        let request_id = dialog_id(&mut model);
        send(
            &mut model,
            ShellMessage::Input(InputMessage::FileDialogCompleted {
                request_id,
                paths: vec![" D:/Eagle/动画.library ".into()],
                failed: None,
            }),
        );
        assert_eq!(model.admin.eagle_library_path, "D:/Eagle/动画.library");
        let selected = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
        assert_eq!(
            eagle_path_label(&selected.library_path),
            "D:/Eagle/动画.library"
        );
        assert_eq!(
            model.files.error, "请先选择 EagleLibrary 目录。",
            "选中目录还不代表导入成功"
        );
        assert!(eagle_import_message(&selected, "copy").is_some());

        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::RunEagleImport("sideways".into())),
        );
        assert!(model.files.take_effects().is_empty());
        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::RunEagleImport("copy".into())),
        );
        assert!(matches!(
            model.files.take_effects().as_slice(),
            [FilesEffect::ImportEagle { library_path, mode, .. }] if library_path == "D:/Eagle/动画.library" && mode == "copy"
        ));
        assert_eq!(model.files.dialog, FileDialog::Closed);
        assert!(model.files.error.is_empty());
        let row = model.operation.shown().expect("导入进行中");
        assert_eq!(
            (row.label.as_str(), row.detail.as_str(), row.value),
            ("复制导入 Eagle", "转换 EagleLibrary", 24.0)
        );
        assert_eq!(model.admin.eagle_library_path, "D:/Eagle/动画.library");

        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::RunEagleImport("move".into())),
        );
        assert!(model.files.take_effects().is_empty(), "进行中不再提交");
        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
        );
        assert!(
            model
                .input
                .take_platform_commands(WindowId(1), false)
                .is_empty(),
            "进行中不再打开目录对话框"
        );
    }

    /// 测试用导入结果：快照随便给一份，结果卡只读摘要和警告。
    fn response(
        summary: EagleLibraryImportSummary,
        warnings: Vec<EagleLibraryImportWarning>,
    ) -> EagleLibraryImportResponse {
        EagleLibraryImportResponse {
            snapshot: crate::shell::data_load_tests::browser("repo"),
            summary,
            warnings,
        }
    }

    fn warning(kind: &str, reason: Option<&str>) -> EagleLibraryImportWarning {
        EagleLibraryImportWarning {
            warning_type: kind.into(),
            reason: reason.map(Into::into),
            ..Default::default()
        }
    }

    fn settled(summary: EagleLibraryImportSummary) -> ShellMessage {
        ShellMessage::Files(FilesMessage::EagleImportSettled {
            result: Ok(eagle_import_report(&response(summary, Vec::new()))),
        })
    }

    #[test]
    fn eagle_import_report_keeps_the_summary_and_counts_every_warning() {
        let summary = EagleLibraryImportSummary {
            imported_files: 3,
            imported_directories: 2,
            imported_shortcuts: 9,
            ..Default::default()
        };
        let report = eagle_import_report(&response(
            summary.clone(),
            vec![
                warning("skippedSmartFolder", Some("条件为空")),
                warning("", None),
            ],
        ));
        assert_eq!(report.summary, summary);
        assert_eq!(report.warning_count, 2);
        assert_eq!(
            report.warnings,
            vec![
                EagleImportWarning {
                    kind: "skippedSmartFolder".into(),
                    text: "条件为空".into()
                },
                EagleImportWarning {
                    kind: "warning".into(),
                    text: String::new()
                },
            ]
        );

        let empty = eagle_import_report(&response(Default::default(), Vec::new()));
        assert_eq!(empty.summary, EagleLibraryImportSummary::default());
        assert_eq!(empty.warning_count, 0);
        assert!(empty.warnings.is_empty());
    }

    #[test]
    fn eagle_warning_text_falls_back_in_order_and_stops_at_twenty() {
        let mut items: Vec<_> = (1..=21)
            .map(|index| warning("overflow", Some(&format!("第{index}条"))))
            .collect();
        items[0] = EagleLibraryImportWarning {
            asset_id: Some("asset".into()),
            name: Some("不该出现".into()),
            ..warning("primary", Some("有原因"))
        };
        items[1] = EagleLibraryImportWarning {
            asset_id: Some(" asset-9 ".into()),
            source_id: Some("src".into()),
            details: Some(serde_json::json!({ "folderId": "f-1" })),
            ..warning("fromFallback", Some("  "))
        };
        items[2] = EagleLibraryImportWarning {
            details: Some(serde_json::json!({ "folderId": "f-1", "reason": "条件" })),
            ..warning("", None)
        };
        items[3] = EagleLibraryImportWarning {
            name: Some("封面".into()),
            ..warning("missingThumbnail", None)
        };
        items[4] = EagleLibraryImportWarning {
            source_id: Some("src-2".into()),
            ..warning("", None)
        };
        let report = eagle_import_report(&response(Default::default(), items));
        assert_eq!(report.warning_count, 21);
        assert_eq!(report.warnings.len(), 20);
        let line = |kind: &str, text: &str| EagleImportWarning {
            kind: kind.into(),
            text: text.into(),
        };
        assert_eq!(report.warnings[0], line("primary", "有原因"));
        assert_eq!(report.warnings[1], line("fromFallback", " asset-9 "));
        assert_eq!(
            report.warnings[2],
            line("warning", r#"{"folderId":"f-1","reason":"条件"}"#)
        );
        assert_eq!(report.warnings[3], line("missingThumbnail", ""));
        assert_eq!(report.warnings[4], line("warning", "src-2"));
        assert_eq!(report.warnings[19].text, "第20条");
        assert!(report.warnings.iter().all(|item| item.text != "第21条"));
        assert!(
            report
                .warnings
                .iter()
                .all(|item| !item.text.contains("不该出现") && !item.text.contains("封面"))
        );
    }

    #[test]
    fn eagle_result_follows_the_protocol_and_the_menu_reuses_the_folder_dialog() {
        let mut model = writable();
        assert!(
            ImportView::project(&model, TOOL_EAGLE_IMPORTER)
                .report
                .is_none()
        );
        let payload = response(
            EagleLibraryImportSummary {
                imported_files: 4,
                imported_directories: 1,
                ..Default::default()
            },
            vec![EagleLibraryImportWarning {
                name: Some("封面".into()),
                ..warning("missingThumbnail", Some("缺图"))
            }],
        );
        send(
            &mut model,
            ShellMessage::Files(FilesMessage::EagleImportSettled {
                result: Ok(eagle_import_report(&payload)),
            }),
        );
        let report = ImportView::project(&model, TOOL_EAGLE_IMPORTER)
            .report
            .expect("结果卡用响应里的字段");
        assert_eq!(
            (
                report.summary.imported_files,
                report.summary.imported_directories,
                report.summary.imported_shortcuts,
                report.warning_count
            ),
            (4, 1, 0, 1)
        );
        assert_eq!(
            report.warnings,
            vec![EagleImportWarning {
                kind: "missingThumbnail".into(),
                text: "缺图".into()
            }]
        );
        assert!(model.files.error.is_empty());

        send(
            &mut model,
            ShellMessage::Files(FilesMessage::EagleImportSettled {
                result: Err("库打不开".into()),
            }),
        );
        assert!(
            ImportView::project(&model, TOOL_EAGLE_IMPORTER)
                .report
                .is_none()
        );
        assert_eq!(model.files.error, "库打不开");

        model.files.take_effects();
        model.files.mutating = false;
        send(
            &mut model,
            ShellMessage::Files(FilesMessage::OpenEagle("copy".into())),
        );
        let request_id = dialog_id(&mut model);
        send(
            &mut model,
            ShellMessage::Input(InputMessage::FileDialogCompleted {
                request_id,
                paths: Vec::new(),
                failed: None,
            }),
        );
        assert!(model.admin.eagle_library_path.is_empty());
        assert!(model.files.take_effects().is_empty(), "取消选择不导入");

        send(
            &mut model,
            ShellMessage::Files(FilesMessage::OpenEagle("move".into())),
        );
        let request_id = dialog_id(&mut model);
        send(
            &mut model,
            ShellMessage::Input(InputMessage::FileDialogCompleted {
                request_id,
                paths: vec![" D:/菜单.library ".into()],
                failed: None,
            }),
        );
        assert_eq!(model.admin.eagle_library_path, "D:/菜单.library");
        assert!(
            ImportView::project(&model, TOOL_EAGLE_IMPORTER)
                .report
                .is_none(),
            "选中目录还不是导入结果"
        );
        assert!(matches!(
            model.files.take_effects().as_slice(),
            [FilesEffect::ImportEagle { library_path, mode, .. }] if library_path == "D:/菜单.library" && mode == "move"
        ));
    }

    #[test]
    fn eagle_notice_and_copy_label_stay_honest() {
        let open = FileContext {
            repo_id: Some("repo".into()),
            writable: true,
            missing: false,
            trash: false,
            smart_folder: false,
            category_virtual: false,
        };
        assert_eq!(block_reason(&open, false), Some("当前目标不可导入。"));
        assert!(block_reason(&open, true).is_none());

        let mut model = writable();
        model.files.mutating = true;
        assert!(
            import_block_reason(&model).is_none(),
            "进行中不写成不能导入"
        );
        model.files.mutating = false;
        model.admin.eagle_library_path = "D:/Eagle/动画.library".into();

        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::RunEagleImport("copy".into())),
        );
        let busy = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
        assert!(busy.busy && busy.notice.is_empty() && busy.reason.is_none());
        assert_eq!(
            (
                eagle_submit_label(&busy, "copy"),
                eagle_submit_label(&busy, "move")
            ),
            ("处理中...", "剪切导入")
        );
        send(&mut model, settled(Default::default()));
        let copied = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
        assert_eq!(copied.notice, "复制导入完成，目标目录：inbox");
        assert!(!copied.busy && copied.report.is_some());
        assert_eq!(eagle_submit_label(&copied, "copy"), "复制导入");

        let mut moved = writable();
        moved.files.current_path = "inbox/covers".into();
        moved.admin.eagle_library_path = "D:/Eagle/动画.library".into();
        send(
            &mut moved,
            ShellMessage::Admin(AdminMessage::RunEagleImport("move".into())),
        );
        assert_eq!(
            eagle_submit_label(&ImportView::project(&moved, TOOL_EAGLE_IMPORTER), "move"),
            "剪切导入"
        );
        send(&mut moved, settled(Default::default()));
        assert_eq!(
            ImportView::project(&moved, TOOL_EAGLE_IMPORTER).notice,
            "剪切导入完成，目标目录：inbox/covers"
        );

        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::RunEagleImport("move".into())),
        );
        assert!(
            ImportView::project(&model, TOOL_EAGLE_IMPORTER)
                .notice
                .is_empty(),
            "再次提交先清掉上一次"
        );
        send(
            &mut model,
            ShellMessage::Files(FilesMessage::EagleImportSettled {
                result: Err("库打不开".into()),
            }),
        );
        let failed = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
        assert!(failed.notice.is_empty());
        assert_eq!(failed.error, "库打不开");

        model.files.mutating = false;
        model.files.error.clear();
        model.files.current_path.clear();
        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::RunEagleImport("copy".into())),
        );
        send(&mut model, settled(Default::default()));
        assert_eq!(
            ImportView::project(&model, TOOL_EAGLE_IMPORTER).notice,
            "复制导入完成，目标目录：/"
        );

        model.files.mutating = false;
        model.files.current_path = "/".into();
        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::RunEagleImport("copy".into())),
        );
        send(
            &mut model,
            settled(EagleLibraryImportSummary {
                imported_files: 1,
                ..Default::default()
            }),
        );
        let root = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
        assert_eq!(root.notice, "复制导入完成，目标目录：/");
        assert_eq!(
            root.report.map(|report| report.summary.imported_files),
            Some(1)
        );

        send(
            &mut model,
            ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
        );
        let request_id = dialog_id(&mut model);
        send(
            &mut model,
            ShellMessage::Input(InputMessage::FileDialogCompleted {
                request_id,
                paths: Vec::new(),
                failed: None,
            }),
        );
        assert_eq!(
            ImportView::project(&model, TOOL_EAGLE_IMPORTER).notice,
            "复制导入完成，目标目录：/",
            "取消选择不改已经写出的完成提示"
        );
        model.files.current_path = "inbox".into();
        send(&mut model, settled(Default::default()));
        assert_eq!(
            ImportView::project(&model, TOOL_EAGLE_IMPORTER).notice,
            "复制导入完成，目标目录：/",
            "没提交的成功结果不改写提示"
        );
    }
}

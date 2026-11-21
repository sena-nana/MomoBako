//! 导出资源库对话框：仓库名和路径、压缩包 / Git 分段、
//! 各自的字段，底部取消和导出，标题右侧有关闭位。首页没有入口，这里只在显式打开时出现。
//!
//! 外壳走统一对话框框架，导出中三种关闭手势都不关。对话框常驻：压缩包和 Git 两组字段都建好，
//! 按分段 `.visible` 切换；加密开着时才显示密码。输入框、密码和输出路径各有一份草稿信号，
//! 按 `ModelField` 的规矩回写，其余字段放在一个投影信号里。

use std::sync::Arc;

use nana_ui::icons_tabler::{ALERT_TRIANGLE, ARCHIVE, DOWNLOAD, GIT_BRANCH};
use nana_ui::runtime::view::{
    AnyView, FieldWrite, IntoView, Signal, callout, fields, signal, widget,
};
use nana_ui::runtime::{
    BrowseRequested, Checkbox, LengthSpec, PathField, RadiusTier, SegmentedControl,
    SegmentedOption, SegmentedOptionChosen, SemanticColorRole, Stack, StatusTone, TextChanged,
    ToggleChanged,
};
use nana_ui::{ButtonKind, DialogSize};
use nana_ui_core::Icon;

use crate::shell::files::FilesMessage;
use crate::shell::files_view::style;
use crate::shell::view_part_overlay::dialog::{
    Choices, DialogFrame, action, action_button, footer, select_field, text_field, two_columns,
    wrapping_text,
};
use crate::shell::view_part_overlay::session::{Draft, Projected};
use crate::shell::{ShellMessage, ShellViewModel};

/// 导出对话框要显示的东西。几个输入框的草稿不在这里，见 [`Draft`]。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExportView {
    pub repo_name: String,
    pub repo_path: String,
    /// 压缩包（否则 Git）。
    pub archive: bool,
    pub format: String,
    pub compression: String,
    pub encrypt: bool,
    pub notice: String,
    pub error: String,
    /// 导出进行中：字段和按钮禁用，主按钮转圈写「处理中」，三种关闭手势都不关。
    pub busy: bool,
}

impl ExportView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let export = &model.files.export;
        if !export.open {
            return None;
        }
        let repository = model.workspace.active_repository();
        Some(Self {
            repo_name: repository
                .map(|item| item.name.clone())
                .unwrap_or_else(|| "未选择资源库".into()),
            repo_path: repository.map(|item| item.path.clone()).unwrap_or_default(),
            archive: export.target != "git",
            format: export.format.clone(),
            compression: export.compression.clone(),
            encrypt: export.encrypt,
            notice: export.notice.clone(),
            error: export.error.clone(),
            busy: export.busy,
        })
    }
}

fn file_message(message: FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

fn export_field(field: &'static str) -> impl Fn(String) -> ShellMessage + Send + Sync + 'static {
    move |value| {
        file_message(FilesMessage::SetExportField {
            field: field.to_string(),
            value,
        })
    }
}

/// 导出对话框里的一份草稿：开着时读导出草稿里这一项。
fn draft(
    model: &ShellViewModel,
    read: fn(&crate::shell::files::FilesState) -> String,
) -> Signal<String> {
    Draft::register(model, move |model| {
        model.files.export.open.then(|| read(&model.files))
    })
}

/// 导出资源库：仓库名和路径、压缩包 / Git 分段、各自的字段，底部取消和导出。
pub(crate) fn export_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(ExportView::project(model)?);
    Projected::register(view, ExportView::project);
    let busy = move || view.with(|view| view.busy);
    let archive = move || view.with(|view| view.archive);
    let submit = || file_message(FilesMessage::SubmitExport);
    let close = || file_message(FilesMessage::CloseExport);
    let pairs = |items: &[(&str, &str)]| -> Choices {
        items
            .iter()
            .map(|(value, text)| ((*value).to_string(), (*text).to_string()))
            .collect()
    };
    let formats = pairs(&[("zip", "zip"), ("7z", "7z"), ("tar", "tar")]);
    let levels = pairs(&[
        ("none", "不压缩"),
        ("fast", "快速"),
        ("balanced", "均衡"),
        ("maximum", "最大"),
    ]);
    let password = draft(model, |files| files.export.password.clone());
    let archive_fields = widget(Stack::column(12.0).width(LengthSpec::Fill))
        .visible(archive)
        .key("export-archive-fields")
        .children((
            two_columns(
                "export-archive-grid",
                10.0,
                vec![
                    select_field(
                        "格式",
                        "export-format",
                        move || Some(view.with(|view| view.format.clone())),
                        move || formats.clone(),
                        busy,
                        export_field("format"),
                    ),
                    select_field(
                        "压缩",
                        "export-compression",
                        move || Some(view.with(|view| view.compression.clone())),
                        move || levels.clone(),
                        busy,
                        export_field("compression"),
                    ),
                ],
            ),
            output_field(draft(model, |files| files.export.output_path.clone()), busy),
            encrypt_toggle(view),
            widget(Stack::column(0.0).width(LengthSpec::Fill))
                .visible(move || view.with(|view| view.encrypt))
                .children((text_field(
                    "密码",
                    "export-password",
                    password,
                    "用于压缩包加密",
                    true,
                    busy,
                    export_field("password"),
                    Some(Arc::new(submit)),
                ),)),
        ));
    let git_fields = widget(Stack::column(12.0).width(LengthSpec::Fill))
        .visible(move || !archive())
        .key("export-git-fields")
        .children((
            two_columns(
                "export-git-grid",
                10.0,
                vec![
                    text_field(
                        "远端",
                        "export-remote",
                        draft(model, |files| files.export.remote.clone()),
                        "origin",
                        false,
                        busy,
                        export_field("remote"),
                        None,
                    ),
                    text_field(
                        "分支",
                        "export-branch",
                        draft(model, |files| files.export.branch.clone()),
                        "默认当前分支",
                        false,
                        busy,
                        export_field("branch"),
                        None,
                    ),
                ],
            ),
            text_field(
                "提交信息",
                "export-git-message",
                draft(model, |files| files.export.message.clone()),
                "导出资源库",
                false,
                busy,
                export_field("message"),
                Some(Arc::new(submit)),
            ),
        ));
    let notice = widget(wrapping_text(13.0, 19.5, SemanticColorRole::Muted))
        .key("export-notice")
        .prop::<String, fields::HiddenWhenEmpty<fields::text::value>>(move || {
            view.with(|view| view.notice.clone())
        });
    let body = widget(Stack::column(12.0).width(LengthSpec::Fill)).children((
        repository_box(view),
        segmented(view),
        archive_fields,
        git_fields,
        notice,
        error_note(view),
    ));
    let buttons = vec![
        action("取消", ButtonKind::Ghost, busy, "export-cancel", close),
        action_button(
            move || if busy() { "处理中" } else { "导出" }.to_string(),
            ButtonKind::Primary,
            busy,
            "export-submit",
            submit,
        )
        .prop::<bool, fields::button::loading>(busy)
        .into_any(),
    ];
    Some(
        DialogFrame::new("export-dialog", || "导出资源库".to_string(), close)
            .size(DialogSize::capped(560.0, 92.0))
            .title_icon(DOWNLOAD)
            .busy(busy)
            .close_button()
            .dialog(body, footer(None, buttons)),
    )
}

/// `repository-export-dialog__repo`：仓库名 14px/700、路径 12px 弱色，都单行省略；`--bg` 底、
/// `--border-soft` 描边、`Sm` 圆角，内边距 9 / 10。
fn repository_box(view: Signal<ExportView>) -> AnyView {
    widget(
        Stack::column(2.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .padding_xy(10.0, 9.0)
            .surface(SemanticColorRole::Background)
            .border(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Sm),
    )
    .children((
        widget(style::text(String::new(), 14.0, 700, SemanticColorRole::Text, 21.7).truncating())
            .key("export-repo-name")
            .prop::<String, fields::text::value>(move || view.with(|view| view.repo_name.clone())),
        widget(style::small_muted(String::new()).truncating())
            .key("export-repo-path")
            .prop::<String, fields::text::value>(move || view.with(|view| view.repo_path.clone())),
    ))
    .key("export-repo")
    .into_any()
}

/// 压缩包和 Git 两段切换：NanaUI 的 `SegmentedControl`。选中段跟着投影里的导出方式，导出中两段都禁用；
/// 选中时发 `SetExportField { target }`，控件自己不存产品状态。
fn segmented(view: Signal<ExportView>) -> AnyView {
    let option = |label: &'static str, icon: Icon, value: &'static str, archive: bool| {
        widget(SegmentedOption::new(label).icon(icon))
            .selected(move || view.with(|view| view.archive == archive))
            .disabled(move || view.with(|view| view.busy))
            .key(format!("export-target-{value}"))
            .on_cx(move |_, _: &SegmentedOptionChosen, cx| {
                cx.dispatch_program_all(file_message(FilesMessage::SetExportField {
                    field: "target".into(),
                    value: value.to_string(),
                }));
            })
    };
    widget(SegmentedControl::new().label("导出方式"))
        .children((
            option("压缩包", ARCHIVE, "archive", true),
            option("Git", GIT_BRANCH, "git", false),
        ))
        .key("export-target")
        .into_any()
}

/// 压缩包保存位置。导出协议必须带输出路径，
/// 这里用路径框和系统保存对话框让用户选，导出时还没选就先弹保存对话框。值跟着草稿信号。
fn output_field(
    path: Signal<String>,
    busy: impl Fn() -> bool + Send + Sync + Copy + 'static,
) -> AnyView {
    let input = PathField::new(path.get_untracked())
        .label("输出路径")
        .placeholder("选择保存位置");
    widget(Stack::column(6.0).width(LengthSpec::Fill))
        .children((
            widget(style::text(
                "输出路径",
                12.0,
                600,
                SemanticColorRole::Muted,
                18.6,
            ))
            .key("export-output-label"),
            widget(input)
                .key("export-output")
                .prop::<String, PathValue>(path)
                .prop::<bool, PathDisabled>(busy)
                .on_cx(|_, event: &TextChanged, cx| {
                    cx.dispatch_program_all(export_field("output")(event.value.to_string()))
                })
                .on_cx(|_, _: &BrowseRequested, cx| {
                    cx.dispatch_program_all(file_message(FilesMessage::ChooseExportOutput))
                }),
        ))
        .key("export-output-row")
        .into_any()
}

/// 路径框的值。框架把用户输入写回 `value`，所以回显同一个值时不动输入框。
struct PathValue;

impl FieldWrite<PathField, String> for PathValue {
    const FIELD: &'static str = "PathField.value";

    fn write(target: &mut PathField, value: String) {
        target.value = Arc::from(value);
    }

    fn differs(target: &PathField, value: &String) -> bool {
        target.value.as_ref() != value.as_str()
    }
}

/// 路径框禁用。
struct PathDisabled;

impl FieldWrite<PathField, bool> for PathDisabled {
    const FIELD: &'static str = "PathField.disabled";

    fn write(target: &mut PathField, disabled: bool) {
        target.disabled = disabled;
    }

    fn differs(target: &PathField, disabled: &bool) -> bool {
        target.disabled != *disabled
    }
}

/// 「加密压缩包」勾选：15px 方框和 13px/500 文字。
fn encrypt_toggle(view: Signal<ExportView>) -> AnyView {
    let mut checkbox = Checkbox::new("加密压缩包", view.with_untracked(|view| view.encrypt));
    Arc::make_mut(&mut checkbox.style.layout).font_size = Some(13.0);
    widget(checkbox)
        .key("export-encrypt")
        .prop::<bool, fields::checkbox::checked>(move || view.with(|view| view.encrypt))
        .prop::<bool, fields::checkbox::disabled>(move || view.with(|view| view.busy))
        .on_cx(|_, event: &ToggleChanged, cx| {
            cx.dispatch_program_all(export_field("encrypt")(if event.checked {
                "1".into()
            } else {
                "0".into()
            }));
        })
        .into_any()
}

/// 导出失败的说明：危险色调的 NanaUI 提示条，没有错误时文字为空，整条不占位。
fn error_note(view: Signal<ExportView>) -> AnyView {
    callout(ALERT_TRIANGLE, StatusTone::Danger, move || {
        view.with(|view| view.error.clone())
    })
    .into_any()
}

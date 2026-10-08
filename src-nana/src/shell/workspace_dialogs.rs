//! 文件页的对话框：重命名、复制 / 移动到文件夹、导入路径、硬链接确认和资源库导出。
//!
//! 外观照 Vue 的 `.modal-overlay` + `.dialog-card`：45% 黑色遮罩，卡片距顶 12vh、宽
//! `min(520px, 92vw)`（导出 560）、抬升底、`Xl` 圆角和投影；头部图标加标题、下边线，
//! 正文内边距 12 / 14，底部按钮右对齐、上边线。主操作按 DESIGN.md 5.1 用低饱和蓝。
//! 点遮罩等于取消。导出对话框在 Vue 首页没有入口，这里只在显式打开时出现。

use std::sync::Arc;

use nana_ui::icons_tabler::{ARCHIVE, COPY, DOWNLOAD, FOLDER_OPEN, GIT_BRANCH, LOADER_2, PENCIL, X};
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, BrowseRequested, Button, IconGlyph, JustifySpec, LengthSpec, ListItem, NodeStyle, PathField,
    PositionSpec, RadiusTier, Select, SelectChanged, SelectOption, SemanticColorRole, Stack, TextChanged, TextInput,
    TextSubmitted,
};
use nana_ui::ButtonKind;
use nana_ui_core::{Icon, LengthAtom, ViewportAxis};

use super::files::{FileDialog, FilesMessage, FilesState};
use super::files_view::style::{self, ButtonLook};
use super::{ShellMessage, ShellViewModel};

/// 普通对话框宽（`.modal-card`）和重命名对话框宽（`.workspace-rename-dialog`）。
const DIALOG_WIDTH: f32 = 520.0;
const RENAME_WIDTH: f32 = 460.0;
const EXPORT_WIDTH: f32 = 560.0;

/// 对话框外壳的参数。
struct Frame {
    title: String,
    icon: Icon,
    width: f32,
    /// 点遮罩或右上角关闭时发出的消息。
    cancel: FilesMessage,
    /// 导出对话框头部右侧的关闭按钮。
    close_button: bool,
    busy: bool,
}

/// 文件变更对话框。没有打开时不占位。
pub(super) fn file_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let files = &model.files;
    let mutating = files.mutating;
    let cancel = FilesMessage::CloseDialog;
    let frame = |title: &str, icon: Icon, width: f32| Frame {
        title: title.to_string(),
        icon,
        width,
        cancel: cancel.clone(),
        close_button: false,
        busy: mutating,
    };
    let view = match files.dialog {
        FileDialog::Closed => return None,
        FileDialog::Rename => text_dialog(frame("重命名文件", PENCIL, RENAME_WIDTH), "新名称", &files.name_draft, "输入新的文件名", "保存", mutating),
        FileDialog::CreateFile => text_dialog(frame("建文件", PENCIL, DIALOG_WIDTH), "文件名", &files.name_draft, "新建空文件，例如 note.txt", "创建", mutating),
        FileDialog::CreateDirectory => text_dialog(frame("新建文件夹", FOLDER_OPEN, DIALOG_WIDTH), "文件夹名称", &files.name_draft, "输入文件夹名称", "创建", mutating),
        FileDialog::Copy => text_dialog(frame("复制到文件夹", COPY, DIALOG_WIDTH), "目标目录", &files.target_draft, "留空表示根目录", "复制", mutating),
        FileDialog::Move => text_dialog(frame("移动到文件夹", FOLDER_OPEN, DIALOG_WIDTH), "目标目录", &files.target_draft, "留空表示根目录", "移动", mutating),
        FileDialog::Import => text_dialog(frame("从文件夹导入", FOLDER_OPEN, DIALOG_WIDTH), "多个路径用分号分隔", &files.import_draft, "D:/素材/图片; D:/素材/参考", "导入", mutating),
        FileDialog::ImportArchive => text_dialog(frame("从 ZIP 导入", ARCHIVE, DIALOG_WIDTH), "压缩包路径", &files.import_draft, "D:/素材/归档.zip", "导入", mutating),
        FileDialog::ImportEagle => {
            let title = if files.eagle_mode == "move" { "从 Eagle 剪切导入" } else { "从 Eagle 复制导入" };
            text_dialog(frame(title, FOLDER_OPEN, DIALOG_WIDTH), "Eagle 资源库路径", &files.import_draft, "D:/素材/示例.library", "导入", mutating)
        }
        FileDialog::Hardlink => hardlink_dialog(files, frame("加入硬链接关联", COPY, DIALOG_WIDTH))?,
    };
    Some(view)
}

/// 一个输入框的对话框：字段标签、输入，底部取消和主操作。回车提交。
fn text_dialog(frame: Frame, label: &str, draft: &str, placeholder: &str, submit: &str, mutating: bool) -> AnyView {
    let field = field(label, "file-dialog-input", draft, placeholder, mutating, FilesMessage::DraftChanged, Some(FilesMessage::SubmitDialog));
    let submit_label = if mutating { "处理中..." } else { submit };
    let actions = vec![
        action_button("取消", "file-dialog-cancel", ButtonKind::Ghost, mutating, FilesMessage::CloseDialog),
        action_button(submit_label, "file-dialog-submit", ButtonKind::Primary, mutating, FilesMessage::SubmitDialog),
    ];
    modal(frame, vec![field], 12.0, actions)
}

/// 硬链接候选：说明一段，两条路径放在浅底框里，底部跳过和加入关联。
fn hardlink_dialog(files: &FilesState, frame: Frame) -> Option<AnyView> {
    let prompt = files.current_hardlink()?;
    let mutating = files.mutating;
    let message = widget(style::wrapping(style::text(prompt.message(), 13.0, 400, SemanticColorRole::Text, 19.5))).key("file-hardlink-message").into_any();
    let paths = widget(
        Stack::column(6.0)
            .width(LengthSpec::Fill)
            .padding(10.0)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((
        widget(style::wrapping(style::small_muted(prompt.existing_path.clone()))).key("file-hardlink-existing"),
        widget(style::wrapping(style::small_muted(prompt.new_path.clone()))).key("file-hardlink-new"),
    ))
    .key("file-hardlink-paths")
    .into_any();
    let confirm = if mutating { "处理中..." } else { "加入关联" };
    let actions = vec![
        action_button("跳过", "file-hardlink-skip", ButtonKind::Ghost, mutating, FilesMessage::SkipHardlink),
        action_button(confirm, "file-hardlink-confirm", ButtonKind::Primary, mutating, FilesMessage::ConfirmHardlink),
    ];
    let frame = Frame { cancel: FilesMessage::SkipHardlink, ..frame };
    Some(modal(frame, vec![message, paths], 12.0, actions))
}

/// 导出资源库：仓库名和路径、压缩包 / Git 分段、各自的字段，底部取消和导出。
pub(super) fn export_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let export = &model.files.export;
    if !export.open {
        return None;
    }
    let busy = export.busy;
    let archive = export.target != "git";
    let repository = model.workspace.active_repository();
    let name = repository.map(|item| item.name.clone()).unwrap_or_else(|| "未选择资源库".into());
    let path = repository.map(|item| item.path.clone()).unwrap_or_default();
    let repo = widget(
        Stack::column(2.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .padding_xy(10.0, 9.0)
            .surface(SemanticColorRole::Background)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Sm),
    )
    .children((
        widget(style::text(name, 14.0, 700, SemanticColorRole::Text, 21.7).truncating()).key("export-repo-name"),
        widget(style::small_muted(path).truncating()).key("export-repo-path"),
    ))
    .key("export-repo")
    .into_any();
    let mut body = vec![repo, segmented(archive, busy)];
    if archive {
        body.push(two_columns(
            select_field("格式", "export-format", &export.format, &[("zip", "zip"), ("7z", "7z"), ("tar", "tar")], busy),
            select_field(
                "压缩",
                "export-compression",
                &export.compression,
                &[("none", "不压缩"), ("fast", "快速"), ("balanced", "均衡"), ("maximum", "最大")],
                busy,
            ),
        ));
        body.push(output_field(&export.output_path, busy));
        body.push(encrypt_toggle(export.encrypt, busy));
        if export.encrypt {
            body.push(field("密码", "export-password", &export.password, "用于压缩包加密", busy, |value| export_field("password", value), Some(FilesMessage::SubmitExport)));
        }
    } else {
        body.push(two_columns(
            field("远端", "export-remote", &export.remote, "origin", busy, |value| export_field("remote", value), None),
            field("分支", "export-branch", &export.branch, "默认当前分支", busy, |value| export_field("branch", value), None),
        ));
        body.push(field("提交信息", "export-git-message", &export.message, "导出资源库", busy, |value| export_field("message", value), Some(FilesMessage::SubmitExport)));
    }
    if !export.notice.is_empty() {
        body.push(widget(style::wrapping(style::text(export.notice.clone(), 13.0, 400, SemanticColorRole::Muted, 19.5))).key("export-notice").into_any());
    }
    if !export.error.is_empty() {
        body.push(error_note(export.error.clone()));
    }
    let mut submit = Button::new(if busy { "处理中" } else { "导出" }).kind(ButtonKind::Primary).disabled(busy);
    if busy {
        submit = submit.icon(LOADER_2).icon_size(13.0);
    }
    let actions = vec![
        action_button("取消", "export-cancel", ButtonKind::Ghost, busy, FilesMessage::CloseExport),
        widget(dialog_button(submit)).key("export-submit").on_cx(|_, _: &Activate, cx| cx.dispatch_program(file_message(FilesMessage::SubmitExport))).into_any(),
    ];
    let frame = Frame {
        title: "导出资源库".into(),
        icon: DOWNLOAD,
        width: EXPORT_WIDTH,
        cancel: FilesMessage::CloseExport,
        close_button: true,
        busy,
    };
    Some(modal(frame, body, 12.0, actions))
}

/// 遮罩、卡片、头部、正文和底部按钮。卡片吃掉自己范围里的点击，点在卡片外才取消。
fn modal(frame: Frame, body: Vec<AnyView>, gap: f32, actions: Vec<AnyView>) -> AnyView {
    let width = frame.width;
    let mut header_parts = vec![
        widget(IconGlyph::new(frame.icon).size(14.0).role(SemanticColorRole::Text)).into_any(),
        widget(style::text(frame.title.clone(), 14.0, 600, SemanticColorRole::Text, 21.7).nowrap(true)).key("dialog-title").into_any(),
    ];
    if frame.close_button {
        header_parts.push(widget(Stack::spacer()).into_any());
        let look = ButtonLook {
            height: 24.0,
            padding_x: 5.0,
            foreground: SemanticColorRole::Faint,
            hover: Some(SemanticColorRole::Hover),
            icon_size: 13.0,
            ..ButtonLook::TOOLBAR
        };
        let mut close = style::styled_button("", Some(X), look).disabled(frame.busy);
        Arc::make_mut(&mut close.style.layout).width = Some(LengthSpec::Px(24.0));
        let cancel = frame.cancel.clone();
        header_parts.push(widget(close).key("dialog-close").on_cx(move |_, _: &Activate, cx| cx.dispatch_program(file_message(cancel.clone()))).into_any());
    }
    let header = widget(style::bottom_rule(Stack::bar(8.0).align(AlignSpec::Center).padding_xy(14.0, 12.0)))
        .children(header_parts)
        .key("dialog-header")
        .into_any();
    let body = widget(Stack::column(gap).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).padding_xy(14.0, 12.0))
        .children(body)
        .key("dialog-body")
        .into_any();
    let footer = widget(style::top_rule(Stack::bar(8.0).align(AlignSpec::Center).justify(JustifySpec::End).padding_xy(14.0, 10.0), 10.0))
        .children(actions)
        .key("dialog-actions")
        .into_any();
    let card = Stack::column(0.0)
        .width(LengthSpec::Min2(LengthAtom::Px(width), LengthAtom::Viewport { axis: ViewportAxis::Width, value: 92.0 }))
        .surface(SemanticColorRole::Surface)
        .radius(RadiusTier::Xl)
        .hittable()
        .with_layout(|layout| {
            layout.max_height = Some(LengthSpec::Viewport { axis: ViewportAxis::Height, value: 72.0 });
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            layout.border_width = Some(1.0);
        });
    let card = style::with_shadows(card, vec![style::shadow(14.0, 40.0, 0.0, 0.45)]);
    let column = Stack::column(0.0).align(AlignSpec::Center).with_layout(|layout| {
        layout.position = PositionSpec::Fixed;
        layout.offset_top = Some(LengthSpec::Px(0.0));
        layout.offset_left = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Viewport { axis: ViewportAxis::Width, value: 100.0 });
        layout.height = Some(LengthSpec::Viewport { axis: ViewportAxis::Height, value: 100.0 });
        layout.padding_top = Some(LengthSpec::Viewport { axis: ViewportAxis::Height, value: 12.0 });
        layout.z_index = Some(1001);
        layout.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
    });
    let card = card.with_layout(|layout| layout.pointer_events = Some(nana_ui_core::PointerEventsSpec::Auto));
    widget(Stack::column(0.0).with_layout(|layout| layout.position = PositionSpec::Fixed))
        .children((
            scrim(frame.cancel.clone(), frame.busy),
            widget(column).children((widget(card).children((header, body, footer)).key("dialog-card"),)).key("dialog-column"),
        ))
        .key(format!("dialog-{}", frame.title))
        .into_any()
}

/// 45% 黑色遮罩和 2px 背景模糊。点它取消，处理中时不响应。
fn scrim(cancel: FilesMessage, busy: bool) -> AnyView {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.position = PositionSpec::Fixed;
    layout.offset_top = Some(LengthSpec::Px(0.0));
    layout.offset_left = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Viewport { axis: ViewportAxis::Width, value: 100.0 });
    layout.height = Some(LengthSpec::Viewport { axis: ViewportAxis::Height, value: 100.0 });
    layout.z_index = Some(1000);
    layout.background = Some([0.0, 0.0, 0.0, 0.45]);
    layout.paint.backdrop_filter = Some(nana_ui_core::BackdropFilter { blur_radius: 2.0, saturate: 1.0 });
    let mut node = widget(ListItem::new("关闭对话框").style(style)).key("dialog-scrim");
    if !busy {
        node = node.on_cx(move |_, _: &Activate, cx| cx.dispatch_program(file_message(cancel.clone())));
    }
    node.into_any()
}

/// `dialog-field`：12px/600 弱化色标签，下面是 32 高的输入（白底、`--border` 描边、`Sm` 圆角）。
fn field(
    label: &str,
    key: &str,
    value: &str,
    placeholder: &str,
    disabled: bool,
    on_change: fn(String) -> FilesMessage,
    on_submit: Option<FilesMessage>,
) -> AnyView {
    let mut input = TextInput::new(value.to_string()).label(label.to_string()).placeholder(placeholder.to_string()).disabled(disabled);
    input.style.radius = Some(RadiusTier::Sm);
    input.style.control_height = None;
    let layout = Arc::make_mut(&mut input.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.height = Some(LengthSpec::Px(32.0));
    layout.min_height = Some(LengthSpec::Px(32.0));
    layout.padding_left = Some(LengthSpec::Px(8.0));
    layout.padding_right = Some(LengthSpec::Px(8.0));
    layout.font_size = Some(14.0);
    let mut node = widget(input).key(key.to_string()).on_cx(move |_, event: &TextChanged, cx| {
        cx.dispatch_program(file_message(on_change(event.value.to_string())));
    });
    if let Some(submit) = on_submit {
        node = node.on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program(file_message(submit.clone())));
    }
    widget(Stack::column(6.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((widget(style::text(label.to_string(), 12.0, 600, SemanticColorRole::Muted, 18.6)).key(format!("{key}-label")), node))
        .key(format!("{key}-field"))
        .into_any()
}

/// 下拉字段：和输入框同一套标签和外框。
fn select_field(label: &str, key: &'static str, current: &str, options: &[(&str, &str)], disabled: bool) -> AnyView {
    let field_name = key.trim_start_matches("export-").to_string();
    let options = options.iter().map(|(value, caption)| SelectOption::new(*value, *caption)).collect::<Vec<_>>();
    let mut select = Select::new(Some(current.to_string())).options(options).placeholder(label.to_string()).disabled(disabled);
    select.style.radius = Some(RadiusTier::Sm);
    select.style.control_height = None;
    let layout = Arc::make_mut(&mut select.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.height = Some(LengthSpec::Px(32.0));
    layout.min_height = Some(LengthSpec::Px(32.0));
    layout.font_size = Some(14.0);
    widget(Stack::column(6.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((
            widget(style::text(label.to_string(), 12.0, 600, SemanticColorRole::Muted, 18.6)).key(format!("{key}-label")),
            widget(select).key(key).on_cx(move |_, event: &SelectChanged, cx| {
                cx.dispatch_program(file_message(export_field(&field_name, event.value.to_string())));
            }),
        ))
        .into_any()
}

/// 两列等宽、间距 10（`repository-export-dialog__grid`）。
fn two_columns(left: AnyView, right: AnyView) -> AnyView {
    let column = || Stack::column(0.0).grow(1.0).shrink(1.0).min_width(LengthSpec::Px(0.0)).with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0)));
    widget(Stack::bar(10.0).align(AlignSpec::Start))
        .children((widget(column()).children((left,)), widget(column()).children((right,))))
        .into_any()
}

/// 压缩包和 Git 两段切换（`.segmented`）：白底描边，选中段是 `--bg-active` 底。
fn segmented(archive: bool, busy: bool) -> AnyView {
    let tab = |label: &'static str, icon: Icon, value: &'static str, active: bool| {
        let look = ButtonLook {
            height: 28.0,
            padding_x: 12.0,
            font_size: 13.0,
            foreground: if active { SemanticColorRole::Text } else { SemanticColorRole::Muted },
            background: active.then_some(SemanticColorRole::Active),
            hover: Some(if active { SemanticColorRole::Active } else { SemanticColorRole::Hover }),
            icon_size: 13.0,
            ..ButtonLook::TOOLBAR
        };
        widget(style::styled_button(label, Some(icon), look).disabled(busy))
            .key(format!("export-target-{value}"))
            .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(file_message(export_field("target", value.to_string()))))
            .into_any()
    };
    widget(
        Stack::row(2.0)
            .align(AlignSpec::Center)
            .padding(2.0)
            .height(LengthSpec::Px(34.0))
            .surface(SemanticColorRole::Background)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((tab("压缩包", ARCHIVE, "archive", archive), tab("Git", GIT_BRANCH, "git", !archive)))
    .key("export-target")
    .into_any()
}

/// 压缩包保存位置。Vue 的导出对话框没有入口也没有这一项；Nana 的导出协议必须带输出路径，
/// 这里用路径框和系统保存对话框让用户选，导出时还没选就先弹保存对话框。
fn output_field(path: &str, busy: bool) -> AnyView {
    let mut input = PathField::new(path.to_string()).label("输出路径").placeholder("选择保存位置");
    input.disabled = busy;
    widget(Stack::column(6.0).width(LengthSpec::Fill))
        .children((
            widget(style::text("输出路径", 12.0, 600, SemanticColorRole::Muted, 18.6)).key("export-output-label"),
            widget(input)
                .key("export-output")
                .on_cx(|_, event: &TextChanged, cx| cx.dispatch_program(file_message(export_field("output", event.value.to_string()))))
                .on_cx(|_, _: &BrowseRequested, cx| cx.dispatch_program(file_message(FilesMessage::ChooseExportOutput))),
        ))
        .key("export-output-row")
        .into_any()
}

/// 「加密压缩包」勾选：15px 方框和 13px/500 文字。
fn encrypt_toggle(encrypt: bool, busy: bool) -> AnyView {
    let mut checkbox = nana_ui::runtime::Checkbox::new("加密压缩包", encrypt).disabled(busy);
    Arc::make_mut(&mut checkbox.style.layout).font_size = Some(13.0);
    widget(checkbox)
        .key("export-encrypt")
        .on_cx(move |_, event: &nana_ui::runtime::ToggleChanged, cx| {
            cx.dispatch_program(file_message(export_field("encrypt", if event.checked { "1".into() } else { "0".into() })));
        })
        .into_any()
}

/// 浅红底的错误说明（`repository-add-popover__error`）。
fn error_note(message: String) -> AnyView {
    let frame = Stack::column(0.0).width(LengthSpec::Fill).padding_xy(10.0, 8.0).radius(RadiusTier::Sm);
    let tinted = frame.node_style().surface_mix(nana_ui_core::SemanticColorMix::alpha(SemanticColorRole::Danger, 0.1));
    let frame = frame.style(tinted);
    widget(frame)
        .children((widget(style::wrapping(style::text(message, 12.0, 400, SemanticColorRole::Danger, 18.6))).key("export-error"),))
        .into_any()
}

/// 底部按钮：主操作低饱和蓝、600 字重；普通操作透明。高 32、左右 10、14px。
fn action_button(label: &str, key: &str, kind: ButtonKind, disabled: bool, message: FilesMessage) -> AnyView {
    let button = Button::new(label.to_string()).kind(kind).disabled(disabled);
    widget(dialog_button(button)).key(key.to_string()).on_cx(move |_, _: &Activate, cx| cx.dispatch_program(file_message(message.clone()))).into_any()
}

/// 对话框按钮的尺寸。颜色沿用主题按钮配方，只改尺寸和主操作字重。
fn dialog_button(mut button: Button) -> Button {
    let primary = button.kind == ButtonKind::Primary;
    button.style.control_height = None;
    button.style.control_padding_x = None;
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.height = Some(LengthSpec::Px(32.0));
    layout.min_height = Some(LengthSpec::Px(32.0));
    layout.padding_left = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(10.0));
    layout.font_size = Some(14.0);
    layout.font_weight = Some(if primary { 600 } else { 500 });
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    button
}

fn export_field(field: &str, value: String) -> FilesMessage {
    FilesMessage::SetExportField { field: field.to_string(), value }
}

fn file_message(message: FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

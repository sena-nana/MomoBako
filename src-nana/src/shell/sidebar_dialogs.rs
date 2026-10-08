//! 侧栏发起的对话框：新建 / 重命名文件夹、处理文件夹、智能文件夹、删除智能文件夹、
//! 新建播放集和删除资源库。
//!
//! 文案、字段和按钮照 Vue `WorkspaceSidebarFolderDialogs.vue`、`WorkspaceSidebarSmartFolderDialogs.vue`、
//! `WorkspaceSidebarPlaylistDialog.vue` 和 `RepositoryDeleteDialog.vue`。按钮按 DESIGN.md 5.1：
//! 主操作低饱和蓝、危险操作低饱和红、普通操作透明；操作区靠右。

use std::sync::Arc;

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    component_descriptors, Activate, AlignSpec, Button, Dialog, LengthSpec, ListItem, MutationQueue, RadiusTier,
    RuntimeDocument, Select, SelectChanged, SelectOption, SemanticColorRole, Stack, TextArea, TextChanged, TextInput,
};
use nana_ui::{ButtonKind, ControlSize};

use super::super::sidebar::{FolderDeleteMode, GapMessage, SidebarSmartFolder, SmartFolderField};
use super::super::workspace::{DeleteMode, WorkspaceDialog};
use super::super::{ShellMessage, ShellViewModel, SidebarMessage};
use super::parts::{self, primary_button};
use super::sidebar_message;

fn ghost_button(label: impl Into<String>) -> Button {
    Button::new(label).kind(ButtonKind::Ghost)
}

fn danger_button(label: impl Into<String>) -> Button {
    Button::new(label).kind(ButtonKind::Danger)
}

/// 操作区：按钮靠右，间距 8px。可在左侧放一段忙碌提示。
fn actions(leading: Option<AnyView>, buttons: Vec<AnyView>) -> AnyView {
    let mut children = Vec::new();
    if let Some(leading) = leading {
        children.push(leading);
    }
    children.push(widget(Stack::spacer()).into_any());
    children.extend(buttons);
    widget(Stack::bar(8.0).align(AlignSpec::Center)).children(children).into_any()
}

/// 对话框正文里的一段说明：13px、行高 1.5。`muted` 时用次要文字色。
fn paragraph(copy: impl Into<String>, muted: bool, key: &'static str) -> AnyView {
    let color = if muted { SemanticColorRole::Muted } else { SemanticColorRole::Text };
    let mut node = parts::label_text(copy, 13.0, 400, Some(color)).line_height(19.5);
    {
        let layout = Arc::make_mut(&mut node.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    widget(node).key(key).into_any()
}

/// 对话框里的错误：`--err` 12px。
fn error_line(copy: &str, key: &'static str) -> Option<AnyView> {
    if copy.is_empty() {
        return None;
    }
    let mut node = parts::label_text(copy, 12.0, 400, Some(SemanticColorRole::Danger)).line_height(19.0);
    Arc::make_mut(&mut node.style.layout).width = Some(LengthSpec::Fill);
    Some(widget(node).key(key).into_any())
}

/// 字段容器的键前缀。挂载后按它找到字段，给没有自身名称的下拉框补上标签名。
const FIELD_KEY_PREFIX: &str = "dialog-field-";

/// 字段：上面 12px 粗体次要色标签，下面控件，间距 6px。对应 `.dialog-field`。
fn field(label: &'static str, control: AnyView) -> AnyView {
    widget(Stack::column(6.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((widget(parts::label_text(label, 12.0, 600, Some(SemanticColorRole::Muted))), control))
        .key(format!("{FIELD_KEY_PREFIX}{label}"))
        .into_any()
}

/// 用字段标签给对话框里的下拉框命名。
///
/// Vue 里 `<label>` 包着 `<select>`，读屏读到的是字段名；Nana 的 `Select` 没有自己的名称，
/// 不命名时只读出当前选项。这里在挂载后找出放在对话框字段里的下拉框，把字段标签登记为它的名称。
pub fn bind_field_labels(document: &mut RuntimeDocument) {
    let document_id = document.document();
    let mut queue = MutationQueue::new();
    {
        let context = document.context();
        let world = context.world();
        for select in world.nodes_of_component(document_id, component_descriptors::SELECT.type_id) {
            let Some(field) = world.parent_id(select) else {
                continue;
            };
            let in_field = context
                .assembly_path(field)
                .is_some_and(|path| path.rsplit('/').next().is_some_and(|key| key.starts_with(FIELD_KEY_PREFIX)));
            if !in_field {
                continue;
            }
            let Some(label) = world.node(field).and_then(|node| node.children.first().copied()) else {
                continue;
            };
            if label != select && world.labelled_by(select) != Some(label) {
                queue.set_labelled_by(select, Some(label));
            }
        }
    }
    if queue.is_empty() {
        return;
    }
    if let Err(error) = document.context_mut().commit_mutations(queue) {
        eprintln!("Nana 对话框下拉框没有接上字段名：{error}");
    }
}

/// 新建或重命名文件夹。名称为空或文件服务忙时主按钮不可用，提交后等结果再关。
pub fn folder_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let dialog = &model.sidebar.folder_dialog;
    if !dialog.open {
        return None;
    }
    let busy = model.files.mutating || dialog.submitting;
    let blocked = busy || dialog.value.trim().is_empty();
    let input = widget(TextInput::new(dialog.value.clone()).label("文件夹名称").placeholder(dialog.placeholder()).disabled(busy))
        .key("folder-dialog-name")
        .on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::SetFolderValue(event.value.to_string()))));
        })
        .into_any();
    let body = widget(Stack::column(12.0)).children((
        paragraph(dialog.summary(), true, "folder-dialog-summary"),
        field("文件夹名称", input),
        error_line(&dialog.error, "folder-dialog-error"),
    ));
    Some(
        widget(Dialog::new(dialog.title()))
            .body(body)
            .footer(actions(None, vec![
                widget(ghost_button("取消").disabled(busy)).key("folder-dialog-cancel").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::CloseFolderDialog)));
                }).into_any(),
                widget(primary_button(dialog.action_label()).disabled(blocked)).key("folder-dialog-submit").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::SubmitFolderDialog)));
                }).into_any(),
            ]))
            .into_any(),
    )
}

/// 处理文件夹：转移到上级目录，或移入回收站。对应 Vue 的 `folder-delete-dialog`。
pub fn folder_delete_dialog(model: &ShellViewModel) -> Option<AnyView> {
    if !model.sidebar.folder_delete_open() {
        return None;
    }
    let busy = model.files.mutating || model.sidebar.folder_delete_submitting;
    let label = model.sidebar.folder_delete_label.clone();
    let options = widget(Stack::column(10.0)).children((
        option_card(
            "转移到上级目录",
            "保留内部文件和子文件夹，只删除当前这一层目录。",
            false,
            busy,
            "folder-delete-move",
            || ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::ConfirmFolderDelete(FolderDeleteMode::MoveToParent))),
        ),
        option_card(
            "移入回收站",
            "将该目录及其全部内容移入回收站，可在回收站中还原。",
            true,
            busy,
            "folder-delete-trash",
            || ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::ConfirmFolderDelete(FolderDeleteMode::Delete))),
        ),
    ));
    let body = widget(Stack::column(12.0)).children((
        paragraph(format!("将处理文件夹“{label}”。请选择内部内容的处理方式。"), false, "folder-delete-copy"),
        options,
        error_line(&model.sidebar.folder_delete_error, "folder-delete-error"),
    ));
    Some(
        widget(Dialog::new(super::super::sidebar::FOLDER_DELETE_TITLE))
            .body(body)
            .footer(actions(None, vec![widget(ghost_button("取消").disabled(busy)).key("folder-delete-cancel").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::CloseFolderDelete)));
            }).into_any()]))
            .into_any(),
    )
}

/// 带标题和说明的选项卡片。对应 `.folder-delete-dialog__option` / `.repository-delete-dialog__option`：
/// 内边距 14px、`--radius-xl`、`--bg` 底，悬停 `--bg-hover`；危险项描一圈浅红、悬停浅红底。
fn option_card(
    title: &str,
    detail: &str,
    danger: bool,
    disabled: bool,
    key: impl Into<String>,
    message: impl Fn() -> ShellMessage + Send + 'static,
) -> AnyView {
    let mut style = parts::row_style(0.0, 14.0, 14.0, 6.0, parts::ActiveTone::Accent, disabled);
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
    let mut detail_text = parts::label_text(detail, 12.0, 400, Some(SemanticColorRole::Muted)).line_height(18.0);
    {
        let layout = Arc::make_mut(&mut detail_text.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    let content = widget(Stack::column(6.0).width(LengthSpec::Fill)).children((
        widget(parts::label_text(title, 14.0, 600, Some(SemanticColorRole::Text))),
        widget(detail_text),
    ));
    widget(ListItem::new(title.to_string()).disabled(disabled).style(style))
        .content(content)
        .key(key.into())
        .on_cx(move |_, _: &Activate, cx| {
            if !disabled {
                cx.dispatch_program(message());
            }
        })
        .into_any()
}

/// 新建或编辑智能文件夹：名称、父级和全部筛选字段。名称为空或保存中时主按钮不可用。
pub fn smart_folder_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let draft = &model.sidebar.smart_draft;
    if !draft.open {
        return None;
    }
    let busy = draft.busy;
    let blocked = busy || draft.name.trim().is_empty();
    let input = |field: SmartFolderField, label: &'static str, placeholder: &'static str| -> AnyView {
        let control = widget(TextInput::new(draft.value(field).to_string()).label(label).placeholder(placeholder).disabled(busy))
            .key(format!("smart-field-{label}"))
            .on_cx(move |_, event: &TextChanged, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::SetSmartFolderField { field, value: event.value.to_string() }));
            })
            .into_any();
        self::field(label, control)
    };
    let area = |field: SmartFolderField, label: &'static str, placeholder: &'static str, rows: f32| -> AnyView {
        let control = widget(TextArea::new(draft.value(field).to_string()).label(label).placeholder(placeholder).disabled(busy).height(rows * 20.0 + 14.0))
            .key(format!("smart-field-{label}"))
            .on_cx(move |_, event: &TextChanged, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::SetSmartFolderField { field, value: event.value.to_string() }));
            })
            .into_any();
        self::field(label, control)
    };
    let select = |field: SmartFolderField, label: &'static str, options: Vec<(String, String)>| -> AnyView {
        let current = draft.value(field).to_string();
        let mut control = Select::new(Some(current))
            .options(options.into_iter().map(|(value, text)| SelectOption::new(value, text)).collect::<Vec<_>>())
            .size(ControlSize::Medium);
        control.disabled = busy;
        Arc::make_mut(&mut control.style.layout).width = Some(LengthSpec::Fill);
        let control = widget(control)
            .key(format!("smart-field-{label}"))
            .on_cx(move |_, event: &SelectChanged, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::SetSmartFolderField { field, value: event.value.to_string() }));
            })
            .into_any();
        self::field(label, control)
    };
    // `.smart-folder-dialog__grid`：两列等宽（`repeat(2, minmax(0, 1fr))`），行列间距 12px。
    let grid = |cells: Vec<AnyView>| -> AnyView {
        let column = |cell: Option<AnyView>| -> AnyView {
            widget(Stack::column(0.0).grow(1.0).shrink(1.0).min_width(LengthSpec::Px(0.0)).with_layout(|layout| {
                layout.flex_basis = Some(LengthSpec::Px(0.0));
            }))
            .children((cell,))
            .into_any()
        };
        let mut rows = Vec::new();
        let mut cells = cells.into_iter();
        while let Some(first) = cells.next() {
            let second = cells.next();
            rows.push(widget(Stack::bar(12.0).align(AlignSpec::Start)).children((column(Some(first)), column(second))).into_any());
        }
        widget(Stack::column(12.0).width(LengthSpec::Fill)).children(rows).into_any()
    };
    let mut parents = vec![(String::new(), "顶层智能文件夹".to_string())];
    flatten_smart(&model.sidebar.smart_folders, &draft.target_id, &mut parents);
    let mut body = vec![
        input(SmartFolderField::Name, "名称", "例如 高评分 PSD"),
        select(SmartFolderField::Parent, "父级", parents),
        grid(vec![
            input(SmartFolderField::Query, "关键词", "文件名、标签或元数据"),
            input(SmartFolderField::Path, "路径前缀", "Campaigns/Summer"),
            input(SmartFolderField::Formats, "格式", "psd，png"),
            input(SmartFolderField::Tags, "标签", "封面，主视觉"),
            input(SmartFolderField::Colors, "颜色", "红色，绿色"),
            input(SmartFolderField::Shapes, "形状", "方形，横版"),
            input(SmartFolderField::MinRating, "最低评分", "4"),
            select(SmartFolderField::Match, "匹配方式", vec![("and".into(), "全部匹配".into()), ("or".into(), "任一匹配".into())]),
        ]),
        area(SmartFolderField::Metadata, "元数据键值", "artist=demo\nsource=reference", 3.0),
        grid(vec![
            input(SmartFolderField::ExcludeQuery, "排除关键词", "draft，archive"),
            input(SmartFolderField::ExcludePaths, "排除路径", "Archive，Temp"),
            input(SmartFolderField::ExcludeTags, "排除标签", "草稿，临时"),
            input(SmartFolderField::ExcludeFormats, "排除格式", "gif，webp"),
            input(SmartFolderField::SortField, "排序字段", "modifiedAt / rating / metadata.width"),
            select(SmartFolderField::SortDirection, "排序方向", vec![("asc".into(), "升序".into()), ("desc".into(), "降序".into())]),
            input(SmartFolderField::Limit, "结果数量", "100"),
        ]),
        area(SmartFolderField::ExcludeMetadata, "排除元数据", "status=archived", 2.0),
        grid(vec![
            area(SmartFolderField::ExcludeNumbers, "排除数值范围", "width=0..640", 2.0),
            area(SmartFolderField::ExcludeDates, "排除日期范围", "fileCreatedAt=2024-01-01T00:00:00Z..2024-01-31T00:00:00Z", 2.0),
        ]),
        area(SmartFolderField::Numbers, "数值范围", "width=1024..4096\noriginalSizeBytes=..10485760", 2.0),
        area(SmartFolderField::Dates, "日期范围", "fileCreatedAt=2024-01-01T00:00:00Z..2024-12-31T23:59:59Z", 2.0),
    ];
    if let Some(error) = error_line(&draft.error, "smart-dialog-error") {
        body.push(error);
    }
    let scroll = widget(nana_ui::runtime::ScrollView::new(nana_ui::runtime::ScrollAxes::Vertical).with_layout(|layout| {
        layout.max_height = Some(LengthSpec::CalcViewportOffset {
            axis: nana_ui_core::ViewportAxis::Height,
            value: 72.0,
            offset_px: -120.0,
        });
    }))
    .children((widget(Stack::column(12.0).with_layout(|layout| layout.padding_right = Some(LengthSpec::Px(4.0)))).children(body),));
    Some(
        widget(Dialog::new(model.sidebar.smart_dialog_title()).size(nana_ui_core::DialogSize::Wide))
            .body(scroll)
            .footer(actions(None, vec![
                widget(ghost_button("取消").disabled(busy)).key("smart-dialog-cancel").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::CloseSmartFolderDialog));
                }).into_any(),
                widget(primary_button(draft.action_label()).loading(busy).disabled(blocked)).key("smart-create-submit").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::SubmitSmartFolder));
                }).into_any(),
            ]))
            .into_any(),
    )
}

/// 父级候选：除了正在编辑的自己，按树序列出所有智能文件夹。
fn flatten_smart(folders: &[SidebarSmartFolder], exclude: &str, out: &mut Vec<(String, String)>) {
    for folder in folders {
        if folder.id != exclude {
            out.push((folder.id.clone(), folder.name.clone()));
        }
        flatten_smart(&folder.children, exclude, out);
    }
}

/// 删除智能文件夹确认。只删除筛选，不动真实文件。
pub fn smart_delete_dialog(model: &ShellViewModel) -> Option<AnyView> {
    if !model.sidebar.smart_delete_open() {
        return None;
    }
    let busy = model.sidebar.smart_draft.busy;
    let label = model.sidebar.smart_delete_label.clone();
    Some(
        widget(Dialog::new(model.sidebar.smart_delete_title()))
            .body(paragraph(format!("将删除“{label}”及其子智能文件夹。实际文件和真实目录不会被删除。"), false, "smart-delete-copy"))
            .footer(actions(None, vec![
                widget(ghost_button("取消").disabled(busy)).key("smart-delete-cancel").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::CloseSmartDelete)));
                }).into_any(),
                widget(danger_button("删除").disabled(busy)).key("smart-delete-confirm").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::ConfirmSmartDelete)));
                }).into_any(),
            ]))
            .into_any(),
    )
}

/// 新建播放集：名称和播放类型。名称为空或没有选类型时「创建」不可用。
pub fn playlist_create_dialog(model: &ShellViewModel) -> Option<AnyView> {
    if !model.playlist_dialog_open {
        return None;
    }
    let name = model.new_playlist_name.clone();
    let selected = model.selected_new_playlist_player_type_id.clone();
    let blocked = name.trim().is_empty() || selected.is_none();
    let options = model
        .playlist_players
        .iter()
        .map(|player| SelectOption::new(player.player_type_id.clone(), format!("{} · {}", player.label, player.file_class)))
        .collect::<Vec<_>>();
    let mut select = Select::new(selected).options(options).size(ControlSize::Medium);
    Arc::make_mut(&mut select.style.layout).width = Some(LengthSpec::Fill);
    let name_input = widget(TextInput::new(name).label("名称").placeholder("例如 通勤歌单 / 参考分镜"))
        .key("playlist-dialog-name")
        .on_cx(|_, event: &TextChanged, cx| cx.dispatch_program(ShellMessage::NewPlaylistNameChanged(event.value.to_string())))
        .into_any();
    let type_select = widget(select)
        .key("playlist-dialog-type")
        .on_cx(|_, event: &SelectChanged, cx| cx.dispatch_program(ShellMessage::SelectPlaylistPlayer(event.value.to_string())))
        .into_any();
    Some(
        widget(Dialog::new("新建播放集"))
            .body(widget(Stack::column(12.0)).children((field("名称", name_input), field("播放类型", type_select))))
            .footer(actions(None, vec![
                widget(ghost_button("取消")).key("playlist-dialog-cancel").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::ClosePlaylistDialog);
                }).into_any(),
                widget(primary_button("创建").disabled(blocked)).key("create-playlist").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::CreatePlaylist);
                }).into_any(),
            ]))
            .into_any(),
    )
}

/// 删除资源库：只删记录、删 .momo 数据、删整个文件夹三个选项，处理中左下角显示「处理中...」。
pub fn repository_delete_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let WorkspaceDialog::Delete(dialog) = model.workspace.dialogs.last()?;
    let repository = model.workspace.repositories.iter().find(|item| item.repo_id == dialog.repo_id);
    let deleting = model.workspace.deleting_mode.is_some();
    let summary = repository.map(|item| {
        let mut node = parts::label_text(format!("资源库“{}”位于 {}。下面每个操作都会移除当前注册记录。", item.name, item.path), 12.0, 400, Some(SemanticColorRole::Muted)).line_height(19.0);
        {
            let layout = Arc::make_mut(&mut node.style.layout);
            layout.width = Some(LengthSpec::Fill);
            layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
        }
        widget(node).key("delete-dialog-summary").into_any()
    });
    let options = [DeleteMode::RecordOnly, DeleteMode::DeleteMetadata, DeleteMode::DeleteFolder]
        .into_iter()
        .map(|mode| {
            let enabled = model.workspace.delete_mode_enabled(mode);
            option_card(
                mode.label(),
                &model.workspace.delete_mode_detail(mode),
                mode == DeleteMode::DeleteFolder,
                deleting || !enabled,
                format!("delete-mode-{}", mode.label()),
                move || ShellMessage::MissingConfirmDelete(mode),
            )
        })
        .collect::<Vec<_>>();
    let mut body = Vec::new();
    body.extend(summary);
    body.push(widget(Stack::column(10.0)).children(options).into_any());
    if let Some(error) = error_line(&dialog.error, "delete-dialog-error") {
        body.push(error);
    }
    let busy = deleting.then(|| {
        widget(Stack::row(6.0).align(AlignSpec::Center)).children((
            widget(parts::inherit_icon(nana_ui::icons_tabler::LOADER_2, 13.0)),
            widget(parts::label_text("处理中...", 12.0, 400, Some(SemanticColorRole::Muted))),
        )).key("delete-dialog-busy").into_any()
    });
    Some(
        widget(Dialog::new("删除资源库"))
            .body(widget(Stack::column(12.0)).children(body))
            .footer(actions(busy, vec![widget(ghost_button("取消").disabled(deleting)).key("delete-dialog-cancel").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::MissingCloseDelete);
            }).into_any()]))
            .into_any(),
    )
}

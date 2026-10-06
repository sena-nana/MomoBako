//! 侧栏视图。分组、行和页脚用 Nana 侧栏组件，目录与智能文件夹用 `TreeView`。

use std::sync::Arc;

use nana_ui::icons_tabler::{ARCHIVE, CLOCK, CLIPBOARD_LIST, FOLDERS, LOGS, PLUS, PUZZLE, REFRESH, SETTINGS, TAG, TRASH};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui::runtime::view::{button, segmented_option, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    sidebar_section_tool_button, Activate, AlignSpec, Button, Dialog, Divider, FormField, Icon, IconGlyph, LengthSpec,
    SegmentedControl, SegmentedOptionChosen, SemanticColorRole, SidebarFooter, SidebarFooterButton, SidebarFrame,
    SidebarRow, SidebarRowState, SidebarSection, Stack, Text, TextChanged, TextInput, TreeNode,
    TreeView, TreeViewEvent, ValidationIntent, ValidationMessage, ViewContext,
};

use super::sidebar::{PopoverMode, ShortcutId, SmartFolderField};
use super::SidebarMessage;
use super::{ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};

/// 实况侧栏。左右都是 8px，和 Vue `.workspace-sidebar` 的 `padding: 10px 8px` 一致。
pub fn sidebar_frame() -> SidebarFrame {
    let mut frame = SidebarFrame::new();
    let layout = Arc::make_mut(&mut frame.style.layout);
    layout.padding_top = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(8.0));
    layout.padding_bottom = Some(LengthSpec::Px(10.0));
    layout.padding_left = Some(LengthSpec::Px(8.0));
    frame
}

/// 仓库切换按钮，放在侧栏顶部。
pub fn sidebar_switcher(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let name = model
        .workspace
        .active_repository()
        .map(|repository| repository.name.clone())
        .unwrap_or_else(|| "无资源库".into());
    widget(Stack::column(8.0)).children((
        widget(SidebarRow::new(format!("资源库 · {name}"))).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::OpenRepositorySwitcher));
        }),
        widget(Divider::horizontal()),
    ))
}

/// 快捷方式、目录树、智能文件夹和播放集。边距交给 `SidebarFrame`，行距交给 `SidebarSection`。
pub fn sidebar_sections(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let locked = model.navigation_locked();
    let mut sections = vec![group(model, "快捷方式", None, shortcut_rows(model, locked))];
    if !model.sidebar.quick_access.is_empty() {
        let mut rows = Vec::new();
        for shortcut in model.sidebar.quick_access.clone() {
            let id = shortcut.id.clone();
            rows.push(sidebar_row(&shortcut.label, false, locked, move |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::OpenQuickAccess(id.clone())));
            }));
        }
        sections.push(group(model, "快捷访问", None, rows));
    }
    let folder_locked = locked || model.workspace.active_repo_id.is_none() || model.workspace.panel == WorkspacePanel::Trash;
    let refresh_locked = locked || model.workspace.active_repo_id.is_none() || model.sidebar.tree_loading;
    let refresh_label = if model.sidebar.tree_loading { "正在刷新文件夹树" } else { "刷新文件夹树" };
    let mut folders = vec![folder_body(model, locked).into_any()];
    if !model.sidebar.tree_error.is_empty() {
        folders.push(hint(&model.sidebar.tree_error, "folder-tree-error"));
    }
    folders.extend(folder_actions(model, folder_locked));
    let parent = model.sidebar.current_directory.clone();
    sections.push(group(
        model,
        "文件夹",
        Some(widget(Stack::row(0.0)).children((
            section_tool(PLUS, "在当前目录新建文件夹", "folder-create", folder_locked, move || {
                ShellMessage::Sidebar(SidebarMessage::Gap(super::sidebar::GapMessage::OpenFolderCreate(parent.clone())))
            }),
            section_tool(REFRESH, refresh_label, "refresh-folder-tree", refresh_locked, || {
                ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree)
            }),
        )).into_any()),
        folders,
    ));
    let smart_locked = locked || model.workspace.active_repo_id.is_none();
    let mut smart = vec![smart_body(model, locked).into_any()];
    if !model.sidebar.smart_error.is_empty() {
        smart.push(hint(&model.sidebar.smart_error, "smart-folder-error"));
    }
    smart.extend(smart_actions(model, smart_locked));
    sections.push(group(
        model,
        "智能文件夹",
        Some(section_tool(PLUS, "新建智能文件夹", "smart-create", smart_locked, || {
            ShellMessage::Sidebar(SidebarMessage::OpenSmartFolderDialog)
        })),
        smart,
    ));
    let create_locked = locked || model.workspace.active_repo_id.is_none() || model.playlist_players.is_empty();
    let playlist_count = model.sidebar.playlists.len();
    let playlists_open = model.sidebar.playlists_expanded;
    if model.sidebar.playlists_visible(locked) {
    sections.push(playlist_group(
        playlist_count,
        playlists_open,
        section_tool(PLUS, "新建播放集", "playlist-create", create_locked, || ShellMessage::OpenPlaylistDialog),
        playlist_body(model, locked).into_any(),
    ));
    }
    widget(Stack::fill_column(10.0)).children(sections)
}

/// 设置、拓展、任务和日志。有运行中的任务时，任务按钮读出数量。
pub fn sidebar_footer(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let settings = matches!(model.page, ShellPage::Settings | ShellPage::SettingsError);
    let extensions = !settings && model.workspace.panel == WorkspacePanel::Extensions;
    let logs = !settings && model.workspace.panel == WorkspacePanel::Logs;
    let task_label = if model.active_tasks > 0 {
        format!("任务 {}", model.active_tasks)
    } else {
        "任务".into()
    };
    let footer_opacity = model.motion.footer_opacity();
    widget(SidebarFooter::new()).children((
        fade(footer_button(SETTINGS, "设置".into(), settings, || ShellMessage::Navigate(ShellPage::Settings)), footer_opacity),
        fade(footer_button(PUZZLE, "拓展".into(), extensions, || ShellMessage::SetWorkspacePanel(WorkspacePanel::Extensions)), footer_opacity),
        fade(footer_button(CLIPBOARD_LIST, task_label, model.admin.popover_open, || {
            ShellMessage::Admin(super::admin::AdminMessage::ToggleTaskPopover)
        }), footer_opacity),
        fade(footer_button(LOGS, "日志".into(), logs, || ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs)), footer_opacity),
    ))
}

/// 仓库切换和附加本地文件夹。添加时先打开系统文件夹对话框，路径框留作手填。
pub fn repository_popover(model: &ShellViewModel) -> Option<impl IntoView + use<'_>> {
    if model.sidebar.popover == PopoverMode::Closed {
        return None;
    }
    let submitting = model.sidebar.submitting;
    let error = (!model.sidebar.popover_error.is_empty()).then(|| text(model.sidebar.popover_error.clone()).key("repository-popover-error"));
    let body = if model.sidebar.popover == PopoverMode::Switcher {
        let mut rows = Vec::new();
        for repository in &model.workspace.repositories {
            let repo_id = repository.repo_id.clone();
            let active = model.workspace.active_repo_id.as_deref() == Some(repository.repo_id.as_str());
            rows.push(sidebar_row(&repository.name, active, submitting, move |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::SelectRepositoryFromSwitcher(repo_id.clone())));
            }).into_any());
        }
        rows.push(sidebar_row("添加资源库", false, submitting, |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::ShowRepositoryAddMenu));
        }).into_any());
        if model.workspace.active_repo_id.is_some() {
            rows.push(sidebar_row("删除当前资源库", false, submitting, |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::DeleteRepositoryFromSwitcher));
            }).into_any());
        }
        widget(Stack::column(6.0)).children(rows).into_any()
    } else {
        let path = model.sidebar.attach_path.clone();
        widget(Stack::column(8.0)).children((
            text("添加资源库").key("add-repository-title"),
            text("选择系统文件夹，也可以在这里填写路径。").key("add-repository-hint"),
            widget(TextInput::new(path).label("资源库文件夹")).on_cx(|_, event: &TextChanged, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::RepositoryAttachPathChanged(event.value.to_string())));
            }),
            button(if submitting { "正在附加…" } else { "附加资源库" })
                .key("attach-repository")
                .disabled(submitting)
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(sidebar_message(SidebarMessage::SubmitRepositoryAttach))),
            button("返回列表").key("repository-add-back").disabled(submitting).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::OpenRepositorySwitcher));
            }),
        )).into_any()
    };
    let panel = widget(Stack::fill_column(12.0).padding_xy(16.0, 16.0)).children((
        text("资源库").key("repository-popover-title"),
        body,
        backend_form(model),
        error,
        button("关闭").key("repository-popover-close").disabled(submitting).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::CloseRepositoryPopover));
        }),
        button("添加云盘").key("open-backend").disabled(submitting).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::OpenBackend {
                plugin_id: "cloud".into(),
            })));
        }),
    ));
    Some(panel)
}

/// 侧栏里的文件夹新建或重命名。提交后走文件服务。
pub fn folder_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let dialog = &model.sidebar.folder_dialog;
    if !dialog.open {
        return None;
    }
    let value = dialog.value.clone();
    let title = dialog.title();
    Some(
        widget(Dialog::new(title))
            .body(widget(TextInput::new(value).label(if dialog.rename { "新名称" } else { "文件夹名称" })).on_cx(|_, event: &TextChanged, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::SetFolderValue(event.value.to_string()))));
            }))
            .footer(widget(Stack::row(8.0)).children((
                button("取消").key("folder-dialog-cancel").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::CloseFolderDialog)));
                }),
                button(if dialog.rename { "保存" } else { "创建" }).key("folder-dialog-submit").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::SubmitFolderDialog)));
                }),
            )))
            .into_any(),
    )
}

/// 新建智能文件夹。名称必填；筛选留空时创建的是不带条件的文件夹。
pub fn smart_folder_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let draft = &model.sidebar.smart_draft;
    if !draft.open {
        return None;
    }
    let busy = draft.busy;
    let name = draft.name.clone();
    let blocked = busy || name.trim().is_empty();
    let parent_id = draft.parent_id.clone();
    let query = draft.query.clone();
    let path_prefix = draft.path_prefix.clone();
    let formats = draft.formats.clone();
    let tags = draft.tags.clone();
    let match_mode = draft.match_mode.clone();
    let error = (!draft.error.is_empty()).then(|| {
        widget(ValidationMessage::new(draft.error.clone(), ValidationIntent::Danger)).key("smart-dialog-error").into_any()
    });
    let mut parents = vec![choice(
        "顶层智能文件夹".into(),
        "smart-parent-root",
        parent_id.is_empty(),
        busy,
        SmartFolderField::Parent,
        String::new(),
    )];
    for (id, label) in flatten_smart_folders(&model.sidebar.smart_folders) {
        let selected = parent_id == id;
        parents.push(choice(label, format!("smart-parent-{id}"), selected, busy, SmartFolderField::Parent, id));
    }
    let mut body = vec![
        labeled_field("名称", "例如 高评分 PSD", name, "smart-name", busy, SmartFolderField::Name),
        text("父级").key("smart-parent-label").into_any(),
        widget(Stack::column(6.0)).children(parents).into_any(),
        labeled_field("关键词", "文件名、标签或元数据", query, "smart-query", busy, SmartFolderField::Query),
        labeled_field("路径前缀", "Campaigns/Summer", path_prefix, "smart-path", busy, SmartFolderField::Path),
        labeled_field("格式", "psd，png", formats, "smart-formats", busy, SmartFolderField::Formats),
        labeled_field("标签", "封面，主视觉", tags, "smart-tags", busy, SmartFolderField::Tags),
        text("匹配方式").key("smart-match-label").into_any(),
        widget(SegmentedControl::new().fill(true)).children((
            segmented_option("全部匹配").selected(match_mode != "or").disabled(busy).on_cx(|_, _: &SegmentedOptionChosen, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::SetSmartFolderField {
                    field: SmartFolderField::Match,
                    value: "and".into(),
                }));
            }),
            segmented_option("任一匹配").selected(match_mode == "or").disabled(busy).on_cx(|_, _: &SegmentedOptionChosen, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::SetSmartFolderField {
                    field: SmartFolderField::Match,
                    value: "or".into(),
                }));
            }),
        )).into_any(),
    ];
    if let Some(error) = error {
        body.push(error);
    }
    Some(
        widget(Dialog::new(model.sidebar.smart_dialog_title()))
            .body(widget(Stack::column(8.0)).children(body))
            .footer(widget(Stack::row(8.0)).children((
                button("取消").key("smart-dialog-cancel").disabled(busy).on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::CloseSmartFolderDialog));
                }),
                button(if busy { "正在保存…" } else if model.sidebar.smart_draft.mode_edit { "保存" } else { "创建" }).key("smart-create-submit").disabled(blocked).on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::SubmitSmartFolder));
                }),
            )))
            .into_any(),
    )
}

fn labeled_field(
    label: &'static str,
    placeholder: &'static str,
    value: String,
    key: &'static str,
    disabled: bool,
    field: SmartFolderField,
) -> AnyView {
    widget(FormField::new(label)).control(draft_input(label, placeholder, value, key, disabled, field)).into_any()
}

fn draft_input(
    label: &'static str,
    placeholder: &'static str,
    value: String,
    key: &'static str,
    disabled: bool,
    field: SmartFolderField,
) -> AnyView {
    widget(TextInput::new(value).label(label).placeholder(placeholder).disabled(disabled)).key(key).on_cx(
        move |_, event: &TextChanged, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::SetSmartFolderField {
                field,
                value: event.value.to_string(),
            }));
        },
    ).into_any()
}

fn choice(
    label: String,
    key: impl Into<String>,
    selected: bool,
    disabled: bool,
    field: SmartFolderField,
    value: String,
) -> AnyView {
    let caption = if selected { format!("已选 {label}") } else { label };
    button(caption).key(key.into()).disabled(disabled).on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::SetSmartFolderField { field, value: value.clone() }));
    }).into_any()
}

fn flatten_smart_folders(folders: &[super::sidebar::SidebarSmartFolder]) -> Vec<(String, String)> {
    fn walk(folders: &[super::sidebar::SidebarSmartFolder], prefix: &str, out: &mut Vec<(String, String)>) {
        for folder in folders {
            let label = if prefix.is_empty() { folder.name.clone() } else { format!("{prefix} / {}", folder.name) };
            out.push((folder.id.clone(), label.clone()));
            walk(&folder.children, &label, out);
        }
    }
    let mut out = Vec::new();
    walk(folders, "", &mut out);
    out
}

/// 一个侧栏分组。行距由 `SidebarSection` 的正文槽决定。
/// 标题工具要一直能看见；组件默认只在悬停时放进标题槽。
fn group(model: &ShellViewModel, title: &'static str, tools: Option<AnyView>, body: Vec<AnyView>) -> AnyView {
    let mut spec = SidebarSection::new(title);
    spec.header_hovered = model.motion.tools_revealed();
    let opacity = model.motion.tools_opacity();
    let section = match tools {
        Some(tools) => widget(spec).tools(fade(tools, opacity)),
        None => widget(spec),
    };
    section.children(body).into_any()
}

fn fade(view: AnyView, opacity: f32) -> AnyView {
    widget(Stack::column(0.0).with_layout(|layout| {
        layout.opacity = Some(opacity);
    }))
    .children((view,))
    .into_any()
}

/// 播放集标题。数量跟在标题后，加号留在工具列。
/// 组件计数槽和加号占同一列，所以这里分开排。折叠标记收成 12px，避免带上工具列的右边距。
fn playlist_group(count: usize, expanded: bool, tools: AnyView, body: AnyView) -> AnyView {
    let icon = if expanded { Icon::ChevronDown } else { Icon::ChevronRight };
    let label = if expanded { "收起播放集" } else { "展开播放集" };
    let mut mark = sidebar_section_tool_button(icon, label);
    let layout = Arc::make_mut(&mut mark.style.layout);
    let edge = LengthSpec::Px(12.0);
    layout.width = Some(edge);
    layout.height = Some(edge);
    layout.min_width = Some(edge);
    layout.min_height = Some(edge);
    layout.margin_right = Some(LengthSpec::Px(0.0));
    widget(Stack::column(2.0)).children((
        widget(Stack::bar(0.0).align(AlignSpec::Center).padding_xy(8.0, 0.0).height(LengthSpec::Px(28.0))).children((
            widget(Stack::row(6.0).align(AlignSpec::Center)).children((
                widget(mark).key("playlist-toggle").on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::TogglePlaylists));
                }),
                playlist_title(),
                widget(Text::new(count.to_string()).color(SemanticColorRole::Muted).font_size(11.0).font_weight(700)),
            )),
            widget(Stack::spacer()),
            tools,
        )),
        body,
    )).into_any()
}

/// 「播放集」本身也能折叠。沿用文字按钮的控件盒，只把字号收到分组标题。
fn playlist_title() -> AnyView {
    let mut button = Button::new("播放集").kind(ButtonKind::Text).size(ControlSize::Small);
    button.style.control_padding_x = None;
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.font_size = Some(11.0);
    layout.font_weight = Some(700);
    layout.width = Some(LengthSpec::Shrink);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    widget(button).key("playlist-title").on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::TogglePlaylists));
    }).into_any()
}

fn section_tool(
    icon: Icon,
    label: &'static str,
    key: &'static str,
    disabled: bool,
    message: impl Fn() -> ShellMessage + Send + 'static,
) -> AnyView {
    widget(sidebar_section_tool_button(icon, label).disabled(disabled))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(message()))
        .into_any()
}

fn footer_button(
    icon: Icon,
    label: String,
    selected: bool,
    message: impl Fn() -> ShellMessage + Send + 'static,
) -> AnyView {
    widget(SidebarFooterButton::new(label, icon).selected(selected))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(message()))
        .into_any()
}

fn hint(copy: &str, key: impl Into<String>) -> AnyView {
    widget(Stack::column(0.0).padding_xy(8.0, 0.0))
        .children((widget(Text::new(copy).color(SemanticColorRole::Muted).font_size(12.0)).key(key.into()),))
        .into_any()
}

/// 树在深度 0 自带 4px。补成和分组标题一样的左右 8px。
fn tree_inset(tree: AnyView) -> AnyView {
    widget(Stack::column(0.0).with_layout(|layout| {
        layout.padding_left = Some(LengthSpec::Px(4.0));
        layout.padding_right = Some(LengthSpec::Px(8.0));
    }))
    .children((tree,))
    .into_any()
}

fn shortcut_rows(model: &ShellViewModel, locked: bool) -> Vec<AnyView> {
    let counts = model.sidebar.counts;
    let items = [
        (ShortcutId::All, counts.all, model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == super::LibraryCategory::All),
        (ShortcutId::Uncategorized, counts.uncategorized, model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == super::LibraryCategory::Uncategorized),
        (ShortcutId::Untagged, counts.untagged, model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == super::LibraryCategory::Untagged),
        (ShortcutId::Recent, counts.recent, model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == super::LibraryCategory::Recent),
        (ShortcutId::Trash, counts.trash, model.workspace.panel == WorkspacePanel::Trash),
    ];
    items.into_iter().map(|(id, count, active)| {
        sidebar_row_with(Some(shortcut_icon(id)), id.label(), Some(count.to_string()), active, locked, move |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::SelectShortcut(id)));
        })
    }).collect()
}

fn shortcut_icon(id: ShortcutId) -> Icon {
    match id {
        ShortcutId::All => ARCHIVE,
        ShortcutId::Uncategorized => FOLDERS,
        ShortcutId::Untagged => TAG,
        ShortcutId::Recent => CLOCK,
        ShortcutId::Trash => TRASH,
    }
}

fn folder_actions(model: &ShellViewModel, locked: bool) -> Vec<AnyView> {
    if locked {
        return Vec::new();
    }
    let parent = model.sidebar.current_directory.clone();
    let mut actions = vec![button("在当前目录新建文件夹").key("folder-create-row").on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::OpenFolderCreate(parent.clone()))));
    }).into_any()];
    if model.sidebar.current_directory.is_empty() {
        return actions;
    }
    let path = model.sidebar.current_directory.clone();
    let label = folder_label(&model.sidebar.folders, &path);
    let rename_path = path.clone();
    let rename_label = label.clone();
    actions.push(button("重命名文件夹").key("folder-rename").on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::OpenFolderRename {
            path: rename_path.clone(),
            label: rename_label.clone(),
        })));
    }).into_any());
    actions.push(button("删除文件夹").key("folder-delete").on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::OpenFolderDelete {
            path: path.clone(),
            label: label.clone(),
        })));
    }).into_any());
    actions
}

fn smart_actions(model: &ShellViewModel, locked: bool) -> Vec<AnyView> {
    if locked {
        return Vec::new();
    }
    let mut actions = vec![button("新建智能文件夹").key("smart-create-row").on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::OpenSmartFolderDialog));
    }).into_any()];
    let Some(id) = model.sidebar.active_smart_folder_id.clone() else {
        return actions;
    };
    let label = smart_label(&model.sidebar.smart_folders, &id);
    let edit_id = id.clone();
    actions.push(button("编辑智能文件夹").key("smart-edit").on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::OpenSmartEdit(edit_id.clone()))));
    }).into_any());
    actions.push(button("删除智能文件夹").key("smart-delete").on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::OpenSmartDelete {
            id: id.clone(),
            label: label.clone(),
        })));
    }).into_any());
    actions
}

fn folder_label(folders: &[super::sidebar::SidebarFolder], path: &str) -> String {
    fn walk(folders: &[super::sidebar::SidebarFolder], path: &str) -> Option<String> {
        for folder in folders {
            if folder.path == path {
                return Some(folder.label.clone());
            }
            if let Some(label) = walk(&folder.children, path) {
                return Some(label);
            }
        }
        None
    }
    walk(folders, path).unwrap_or_else(|| path.to_string())
}

fn smart_label(folders: &[super::sidebar::SidebarSmartFolder], id: &str) -> String {
    fn walk(folders: &[super::sidebar::SidebarSmartFolder], id: &str) -> Option<String> {
        for folder in folders {
            if folder.id == id {
                return Some(folder.name.clone());
            }
            if let Some(name) = walk(&folder.children, id) {
                return Some(name);
            }
        }
        None
    }
    walk(folders, id).unwrap_or_else(|| id.to_string())
}

/// 确认删除侧栏文件夹。Escape 和取消都只关这一层。
pub fn folder_delete_dialog(model: &ShellViewModel) -> Option<AnyView> {
    if !model.sidebar.folder_delete_open() {
        return None;
    }
    let label = model.sidebar.folder_delete_label.clone();
    Some(widget(Dialog::new("删除文件夹")).body(text(format!("删除 {label}？")).key("folder-delete-copy")).footer(widget(Stack::row(8.0)).children((
        button("取消").key("folder-delete-cancel").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::CloseFolderDelete)));
        }),
        button("删除").key("folder-delete-confirm").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::ConfirmFolderDelete)));
        }),
    ))).into_any())
}

/// 确认删除智能文件夹。
pub fn smart_delete_dialog(model: &ShellViewModel) -> Option<AnyView> {
    if !model.sidebar.smart_delete_open() {
        return None;
    }
    let label = model.sidebar.smart_delete_label.clone();
    Some(widget(Dialog::new(model.sidebar.smart_delete_title())).body(text(format!("删除 {label}？")).key("smart-delete-copy")).footer(widget(Stack::row(8.0)).children((
        button("取消").key("smart-delete-cancel").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::CloseSmartDelete)));
        }),
        button("删除").key("smart-delete-confirm").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::ConfirmSmartDelete)));
        }),
    ))).into_any())
}

fn backend_form(model: &ShellViewModel) -> Option<AnyView> {
    if model.sidebar.popover != PopoverMode::BackendForm {
        return None;
    }
    let name = model.sidebar.backend_name.clone();
    let url = model.sidebar.backend_url.clone();
    Some(widget(Stack::column(8.0)).children((
        text("云盘或 Eagle").key("backend-title"),
        widget(TextInput::new(name).label("名称")).key("backend-name").on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::SetBackendName(event.value.to_string()))));
        }),
        widget(TextInput::new(url).label("地址")).key("backend-url").on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::SetBackendUrl(event.value.to_string()))));
        }),
        button("创建后端").key("backend-submit").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::SubmitBackend)));
        }),
    )).into_any())
}

fn folder_body(model: &ShellViewModel, locked: bool) -> impl IntoView + use<'_> {
    if model.workspace.active_repo_id.is_none() {
        return hint("先选择或添加一个资源库。", "folder-empty");
    }
    if locked {
        return hint("资源库文件夹丢失，请先在主视图修复。", "folder-missing");
    }
    if model.workspace.panel == WorkspacePanel::Trash || model.sidebar.browsing_trash {
        return hint("回收站条目在主视图中管理。", "folder-trash");
    }
    if model.sidebar.folders.is_empty() && !model.sidebar.tree_loading {
        return hint("当前仓库还没有子文件夹。", "folder-none");
    }
    let nodes = folder_nodes(&model.sidebar.folders, &model.sidebar.expanded_folders, &model.sidebar.current_directory);
    tree_inset(widget(TreeView::new(nodes)).on_cx(|_, event: &TreeViewEvent<Arc<str>>, cx| match event {
        TreeViewEvent::Toggle(id) => cx.dispatch_program(sidebar_message(SidebarMessage::ToggleFolder(id.to_string()))),
        TreeViewEvent::Select(id) => cx.dispatch_program(sidebar_message(SidebarMessage::OpenFolder(id.to_string()))),
    }).into_any())
}

fn smart_body(model: &ShellViewModel, locked: bool) -> impl IntoView + use<'_> {
    if model.workspace.active_repo_id.is_none() {
        return hint("先选择或添加一个资源库。", "smart-empty");
    }
    if locked {
        return hint("资源库修复后可继续使用智能文件夹。", "smart-missing");
    }
    if model.sidebar.smart_folders.is_empty() {
        return hint("还没有智能文件夹。", "smart-none");
    }
    let active = model.sidebar.active_smart_folder_id.clone();
    let nodes = smart_nodes(&model.sidebar.smart_folders, &model.sidebar.expanded_smart_folders, active.as_deref());
    tree_inset(widget(TreeView::new(nodes)).on_cx(|_, event: &TreeViewEvent<Arc<str>>, cx| match event {
        TreeViewEvent::Toggle(id) => cx.dispatch_program(sidebar_message(SidebarMessage::ToggleSmartFolder(id.to_string()))),
        TreeViewEvent::Select(id) => cx.dispatch_program(sidebar_message(SidebarMessage::OpenSmartFolder(id.to_string()))),
    }).into_any())
}

fn playlist_body(model: &ShellViewModel, locked: bool) -> impl IntoView + use<'_> {
    if !model.sidebar.playlists_expanded {
        return widget(Stack::column(0.0)).into_any();
    }
    if locked {
        return hint("资源库修复后可继续使用播放集。", "playlist-missing");
    }
    if model.workspace.active_repo_id.is_none() {
        return hint("先选择或添加一个资源库。", "playlist-empty");
    }
    let mut rows = Vec::new();
    for playlist in &model.sidebar.playlists {
        let id = playlist.id.clone();
        let active = model.workspace.panel == WorkspacePanel::Playlist && model.sidebar.active_playlist_id.as_deref() == Some(playlist.id.as_str());
        let subtitle = format!("{} · {} 项", playlist.player_label, playlist.item_count);
        let play_id = id.clone();
        let remove_id = id.clone();
        rows.push(widget(Stack::column(0.0)).children((
            sidebar_row(&playlist.name, active, false, move |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::OpenSidebarPlaylist(id.clone())));
            }),
            hint(&subtitle, format!("playlist-meta-{}", playlist.id)),
            button(format!("播放 {}", playlist.name)).key(format!("playlist-play-{}", playlist.id)).on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::PlayPlaylist(play_id.clone()))));
            }),
            button(format!("移除 {}", playlist.name)).key(format!("playlist-remove-{}", playlist.id)).on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::RemovePlaylist(remove_id.clone()))));
            }),
        )).into_any());
    }
    widget(Stack::column(6.0)).children(rows).into_any()
}

fn sidebar_row(
    label: impl AsRef<str>,
    active: bool,
    disabled: bool,
    on_activate: impl Fn(&mut ViewContext<SidebarRow>) + Send + 'static,
) -> AnyView {
    sidebar_row_with(None, label, None, active, disabled, on_activate)
}

fn sidebar_row_with(
    icon: Option<Icon>,
    label: impl AsRef<str>,
    trailing: Option<String>,
    active: bool,
    disabled: bool,
    on_activate: impl Fn(&mut ViewContext<SidebarRow>) + Send + 'static,
) -> AnyView {
    let state = if disabled {
        SidebarRowState::Disabled
    } else if active {
        SidebarRowState::Active
    } else {
        SidebarRowState::Idle
    };
    let label: Arc<str> = Arc::from(label.as_ref());
    let mut row = widget(SidebarRow::new(label).state(state));
    if let Some(icon) = icon {
        row = row.leading(widget(IconGlyph::new(icon).size(14.0)));
    }
    if let Some(trailing) = trailing {
        row = row.trailing(text(trailing));
    }
    row.on_cx(move |_, _: &Activate, cx| on_activate(cx)).into_any()
}

fn sidebar_message(message: SidebarMessage) -> ShellMessage {
    ShellMessage::Sidebar(message)
}

fn folder_nodes(folders: &[super::sidebar::SidebarFolder], expanded: &[String], current: &str) -> Vec<TreeNode<Arc<str>>> {
    folders.iter().map(|folder| {
        let id: Arc<str> = Arc::from(folder.path.as_str());
        let open = expanded.iter().any(|path| path == &folder.path);
        let mut node = if folder.children.is_empty() {
            TreeNode::leaf(id, folder.label.clone())
        } else {
            TreeNode::branch(id, folder.label.clone(), open, folder_nodes(&folder.children, expanded, current))
        };
        node.selected = folder.path == current;
        node
    }).collect()
}

fn smart_nodes(folders: &[super::sidebar::SidebarSmartFolder], expanded: &[String], active: Option<&str>) -> Vec<TreeNode<Arc<str>>> {
    folders.iter().map(|folder| {
        let id: Arc<str> = Arc::from(folder.id.as_str());
        let open = expanded.iter().any(|item| item == &folder.id);
        let mut node = if folder.children.is_empty() {
            TreeNode::leaf(id, folder.name.clone())
        } else {
            TreeNode::branch(id, folder.name.clone(), open, smart_nodes(&folder.children, expanded, active))
        };
        node.selected = active == Some(folder.id.as_str());
        node
    }).collect()
}

//! 文件夹树和智能文件夹树。
//!
//! 行结构照 Vue `FolderTreeNode.vue` / `SmartFolderTreeNode.vue`：最左 10px 的折叠三角，
//! 然后是 24px 高的行卡片（文件夹图标、名称、右侧计数），子级每层缩进 20px，行距 3px。
//! 当前目录的卡片是 `--bg-active` 灰底。文件夹行右键打开「打开 / 新建子文件夹 / 重命名 / 删除」菜单；
//! 智能文件夹行右侧常驻新建子级、编辑和删除三个小按钮。

use std::sync::Arc;

use nana_ui::icons_tabler::{CARET_DOWN, CARET_RIGHT, FOLDER, FOLDER_OPEN, FOLDER_PLUS, PENCIL, PLUS, TRASH};
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, ContextMenu, ContextMenuEvent, ContextMenuItem, IconButton, LengthSpec, ListItem, SecondaryPress,
    SemanticColorRole, Stack,
};
use nana_ui::ControlSize;

use super::super::sidebar::{GapMessage, SidebarFolder, SidebarSmartFolder};
use super::super::{ShellMessage, ShellViewModel, SidebarMessage, WorkspacePanel};
use super::parts::{self, empty_hint, group_header, group_title, tree_action, ActiveTone};
use super::{group, refresh_icon, sidebar_message};

/// 每层缩进，对应 `--folder-tree-indent`。
const TREE_INDENT: f32 = 20.0;
/// 折叠三角的宽度，对应 `--folder-tree-toggle-width`。
const TOGGLE_WIDTH: f32 = 10.0;
const ROW_HEIGHT: f32 = 24.0;

/// 文件夹分组：标题带「新建」和「刷新」，正文是树或空状态。对应 `WorkspaceSidebarFolders.vue`。
pub(super) fn folder_group(model: &ShellViewModel, locked: bool) -> AnyView {
    let has_repo = model.workspace.active_repo_id.is_some();
    let trash = model.workspace.panel == WorkspacePanel::Trash || model.sidebar.browsing_trash;
    let loading = model.sidebar.tree_loading;
    let parent = model.sidebar.current_directory.clone();
    let tools = vec![
        tree_action(PLUS, "在当前目录新建文件夹", "folder-create", !has_repo || locked || model.files.mutating || trash, move |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::OpenFolderCreate(parent.clone()))));
        }),
        tree_action(refresh_icon(loading), "刷新文件夹树", "refresh-folder-tree", !has_repo || locked || loading, |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::RefreshFolderTree));
        }),
    ];
    let mut body = Vec::new();
    if !has_repo {
        body.push(empty_hint("先选择或添加一个资源库。", "folder-empty"));
    } else if locked {
        body.push(empty_hint("资源库文件夹丢失，请先在主视图修复。", "folder-missing"));
    } else {
        if !trash && !model.sidebar.folders.is_empty() {
            let mut rows = Vec::new();
            for folder in &model.sidebar.folders {
                folder_rows(model, folder, 1, &mut rows);
            }
            body.push(widget(Stack::column(3.0)).children(rows).key("folder-tree").into_any());
        }
        if (trash || model.sidebar.folders.is_empty()) && !loading {
            let copy = if trash { "回收站条目在主视图中管理。" } else { "当前仓库还没有子文件夹。" };
            body.push(empty_hint(copy, "folder-none"));
        }
    }
    group(group_header(widget(group_title("文件夹")).into_any(), tools, "folder-header"), body)
}

/// 一个目录和它展开的子级。当前目录所在的一支用打开的文件夹图标。
fn folder_rows(model: &ShellViewModel, folder: &SidebarFolder, depth: u16, rows: &mut Vec<AnyView>) {
    let current = model.sidebar.current_directory.as_str();
    let expanded = model.sidebar.expanded_folders.iter().any(|path| path == &folder.path);
    let active = current == folder.path;
    let branch = active || current.starts_with(&format!("{}/", folder.path));
    let count = model.sidebar.folder_counts.get(&folder.path).copied().unwrap_or(0);
    let has_children = !folder.children.is_empty();
    let toggle_path = folder.path.clone();
    let toggle = has_children.then(|| {
        caret(expanded, if expanded { "收起文件夹" } else { "展开文件夹" }, move || {
            sidebar_message(SidebarMessage::ToggleFolder(toggle_path.clone()))
        })
    });
    let open_path = folder.path.clone();
    let menu_path = folder.path.clone();
    let menu_label = folder.label.clone();
    let content = widget(Stack::fill_row(8.0).align(AlignSpec::Center)).children((
        widget(Stack::fill_row(6.0).align(AlignSpec::Center)).children((
            widget(parts::inherit_icon(if branch { FOLDER_OPEN } else { FOLDER }, 14.0)),
            parts::fill_label(folder.label.clone(), 13.0, 500),
        )),
        widget(parts::label_text(count.to_string(), 11.0, 500, Some(SemanticColorRole::Faint))),
    ));
    let card = widget(ListItem::new(folder.label.clone()).selected(active).style(card_style()))
        .content(content)
        .key(format!("folder-row-{}", folder.path))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(sidebar_message(SidebarMessage::OpenFolder(open_path.clone()))))
        .on_cx(move |_, event: &SecondaryPress, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::OpenFolderMenu {
                path: menu_path.clone(),
                label: menu_label.clone(),
                x: event.x,
                y: event.y,
            })));
        });
    rows.push(tree_row(depth, toggle, card.into_any()));
    if has_children && expanded {
        for child in &folder.children {
            folder_rows(model, child, depth + 1, rows);
        }
    }
}

/// 树行卡片：24px、左 4 右 6 内边距；悬停 `--bg-hover`，当前 `--bg-active`。
fn card_style() -> nana_ui::runtime::NodeStyle {
    let mut style = parts::row_style(ROW_HEIGHT, 4.0, 6.0, 8.0, ActiveTone::Neutral, false);
    let layout = Arc::make_mut(&mut style.layout);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.width = Some(LengthSpec::Px(0.0));
    style
}

/// 一行：缩进、折叠三角（没有子级时留空位）和卡片。
fn tree_row(depth: u16, toggle: Option<AnyView>, card: AnyView) -> AnyView {
    let toggle = toggle.unwrap_or_else(|| {
        widget(Stack::row(0.0).width(LengthSpec::Px(TOGGLE_WIDTH)).height(LengthSpec::Px(20.0)).shrink(0.0)).into_any()
    });
    widget(
        Stack::bar(0.0).align(AlignSpec::Center).height(LengthSpec::Px(ROW_HEIGHT)).with_layout(|layout| {
            layout.padding_left = Some(LengthSpec::Px(f32::from(depth.saturating_sub(1)) * TREE_INDENT));
        }),
    )
    .children((toggle, card))
    .into_any()
}

/// 折叠三角：10×20，弱色，展开后朝下。对应 `.workspace-folder-tree__toggle-caret`。
fn caret(expanded: bool, label: &'static str, message: impl Fn() -> ShellMessage + Send + 'static) -> AnyView {
    let mut button = IconButton::new(if expanded { CARET_DOWN } else { CARET_RIGHT }, label).size(ControlSize::Small);
    let mut style = parts::icon_button_style(TOGGLE_WIDTH, nana_ui::runtime::RadiusTier::Xs, SemanticColorRole::Faint, None, false);
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.height = Some(LengthSpec::Px(20.0));
        layout.min_height = Some(LengthSpec::Px(20.0));
    }
    // Vue 的三角悬停只变色，不铺底。
    style.interaction.hovered.background = None;
    style.interaction.pressed.background = None;
    button.style = style;
    widget(button.colors_from_style()).on_cx(move |_, _: &Activate, cx| cx.dispatch_program(message())).into_any()
}

/// 智能文件夹分组：标题带「新建」，正文是树或空状态。对应 `WorkspaceSidebarSmartFolders.vue`。
pub(super) fn smart_group(model: &ShellViewModel, locked: bool) -> AnyView {
    let has_repo = model.workspace.active_repo_id.is_some();
    let busy = model.sidebar.smart_draft.busy;
    let tools = vec![tree_action(PLUS, "新建智能文件夹", "smart-create", !has_repo || locked || busy, |cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::OpenSmartFolderDialog));
    })];
    let body = if !has_repo {
        empty_hint("先选择或添加一个资源库。", "smart-empty")
    } else if locked {
        empty_hint("资源库修复后可继续使用智能文件夹。", "smart-missing")
    } else if model.sidebar.smart_folders.is_empty() {
        empty_hint("还没有智能文件夹。", "smart-none")
    } else {
        let mut rows = Vec::new();
        for folder in &model.sidebar.smart_folders {
            smart_rows(model, folder, 1, busy, &mut rows);
        }
        widget(Stack::column(3.0)).children(rows).key("smart-tree").into_any()
    };
    group(group_header(widget(group_title("智能文件夹")).into_any(), tools, "smart-header"), vec![body])
}

/// 一个智能文件夹和它展开的子级。包含当前智能文件夹的一支用打开的文件夹图标。
fn smart_rows(model: &ShellViewModel, folder: &SidebarSmartFolder, depth: u16, busy: bool, rows: &mut Vec<AnyView>) {
    let active_id = model.sidebar.active_smart_folder_id.as_deref();
    let active = active_id == Some(folder.id.as_str());
    let branch = active || active_id.is_some_and(|id| contains_smart(&folder.children, id));
    let expanded = model.sidebar.expanded_smart_folders.iter().any(|id| id == &folder.id);
    let has_children = !folder.children.is_empty();
    let toggle_id = folder.id.clone();
    let toggle = has_children.then(|| {
        caret(expanded, if expanded { "收起智能文件夹" } else { "展开智能文件夹" }, move || {
            sidebar_message(SidebarMessage::ToggleSmartFolder(toggle_id.clone()))
        })
    });
    let open_id = folder.id.clone();
    let child_id = folder.id.clone();
    let edit_id = folder.id.clone();
    let delete_id = folder.id.clone();
    let delete_label = folder.name.clone();
    let actions = widget(Stack::row(2.0).align(AlignSpec::Center).shrink(0.0)).children((
        tree_action(FOLDER_PLUS, "新建子智能文件夹", "smart-row-create", busy, move |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::OpenSmartFolderChild(child_id.clone())));
        }),
        tree_action(PENCIL, "编辑智能文件夹", "smart-row-edit", busy, move |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::OpenSmartEdit(edit_id.clone()))));
        }),
        super::danger_tree_action(TRASH, "删除智能文件夹", move |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::OpenSmartDelete {
                id: delete_id.clone(),
                label: delete_label.clone(),
            })));
        }),
    ));
    let content = widget(Stack::fill_row(8.0).align(AlignSpec::Center)).children((
        widget(Stack::fill_row(6.0).align(AlignSpec::Center)).children((
            widget(parts::inherit_icon(if branch { FOLDER_OPEN } else { FOLDER }, 14.0)),
            parts::fill_label(folder.name.clone(), 13.0, 500),
        )),
        actions,
    ));
    let mut style = card_style();
    Arc::make_mut(&mut style.layout).padding_right = Some(LengthSpec::Px(1.0));
    let card = widget(ListItem::new(folder.name.clone()).selected(active).style(style))
        .content(content)
        .key(format!("smart-row-{}", folder.id))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(sidebar_message(SidebarMessage::OpenSmartFolder(open_id.clone()))));
    rows.push(tree_row(depth, toggle, card.into_any()));
    if has_children && expanded {
        for child in &folder.children {
            smart_rows(model, child, depth + 1, busy, rows);
        }
    }
}

fn contains_smart(folders: &[SidebarSmartFolder], id: &str) -> bool {
    folders.iter().any(|folder| folder.id == id || contains_smart(&folder.children, id))
}

/// 文件夹行右键菜单：打开、新建子文件夹、重命名和删除。文件服务处理中时后三项禁用。
pub fn folder_menu(model: &ShellViewModel) -> Option<AnyView> {
    let menu = model.sidebar.folder_menu.clone()?;
    let mutating = model.files.mutating;
    let items = vec![
        ContextMenuItem::new("open", "打开").icon(FOLDER_OPEN),
        ContextMenuItem::new("create", "新建子文件夹").icon(FOLDER_PLUS).disabled(mutating),
        ContextMenuItem::new("rename", "重命名").icon(PENCIL).disabled(mutating),
        ContextMenuItem::new("delete", "删除").icon(TRASH).disabled(mutating).danger(true),
    ];
    let path = menu.path.clone();
    let label = menu.label.clone();
    Some(
        widget(ContextMenu::new(menu.x, menu.y).items(items))
            .key("folder-context-menu")
            .on_cx(move |_, event: &ContextMenuEvent, cx| match event {
                ContextMenuEvent::Search(_) => {}
                ContextMenuEvent::Dismiss => cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::CloseFolderMenu))),
                ContextMenuEvent::Select(value) => {
                    cx.dispatch_program(sidebar_message(SidebarMessage::Gap(GapMessage::CloseFolderMenu)));
                    let message = match value.as_ref() {
                        "open" => SidebarMessage::OpenFolder(path.clone()),
                        "create" => SidebarMessage::Gap(GapMessage::OpenFolderCreate(path.clone())),
                        "rename" => SidebarMessage::Gap(GapMessage::OpenFolderRename { path: path.clone(), label: label.clone() }),
                        "delete" => SidebarMessage::Gap(GapMessage::OpenFolderDelete { path: path.clone(), label: label.clone() }),
                        other => {
                            eprintln!("Nana 文件夹菜单没有这一项：{other}");
                            return;
                        }
                    };
                    cx.dispatch_program(sidebar_message(message));
                }
            })
            .into_any(),
    )
}

//! 文件夹树和智能文件夹树。
//!
//! 行结构照 Vue `FolderTreeNode.vue` / `SmartFolderTreeNode.vue`：最左 10px 的折叠三角，
//! 然后是 24px 高的行卡片（文件夹图标、名称、右侧计数），子级每层缩进 20px，行距 3px。
//! 当前目录的卡片是 `--bg-active` 灰底。文件夹行右键打开「打开 / 新建子文件夹 / 重命名 / 删除」菜单；
//! 智能文件夹行右侧常驻新建子级、编辑和删除三个小按钮。
//!
//! 两棵树按显示顺序展开成一列行，放在按键对照的 Store 里，用带键的 `each` 建：展开一个目录只插入
//! 它的子级，折叠只删掉子级；开合三角、当前目录、打开的图标和计数都是行内绑定，变了原地改。
//! 行的键是路径（或编号）加深度和有没有子级，这两样决定行的结构，变了才整行重建。

use std::sync::Arc;

use nana_ui::icons_tabler::{CARET_DOWN, CARET_RIGHT, FOLDER, FOLDER_OPEN, FOLDER_PLUS, PENCIL, PLUS, TRASH};
use nana_ui::runtime::view::{signal, widget, AnyView, FieldWrite, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, ContextMenu, ContextMenuEvent, ContextMenuItem, IconButton, LengthSpec, ListItem, SecondaryPress,
    SemanticColorRole, Stack,
};
use nana_ui::ControlSize;
use nana_ui::runtime::view::{fields, Item, Signal, Store, StoreList, StorePath};
use nana_ui::Icon;

use super::super::sidebar::GapMessage;
use super::super::view_part_sidebar::project::{
    folder_key, smart_key, FolderGroup, FolderRow, FolderRowStoreFields, SmartGroup, SmartRow, SmartRowStoreFields,
};
use super::super::view_part_overlay::session::Projected;
use super::super::{ShellMessage, ShellViewModel, SidebarMessage};
use super::parts::{self, bound_hint, group_header, group_title, tree_action, ActiveTone, GlyphIcon};
use super::super::player_view::key_part;
use super::{group, refresh_icon, sidebar_message};

/// 每层缩进，对应 `--folder-tree-indent`。
const TREE_INDENT: f32 = 20.0;
/// 折叠三角的宽度，对应 `--folder-tree-toggle-width`。
const TOGGLE_WIDTH: f32 = 10.0;
const ROW_HEIGHT: f32 = 24.0;
/// 文件夹三角展开和收起时的无障碍名。
const FOLDER_CARET: (&str, &str) = ("收起文件夹", "展开文件夹");
/// 智能文件夹三角展开和收起时的无障碍名。
const SMART_CARET: (&str, &str) = ("收起智能文件夹", "展开智能文件夹");

/// 文件夹树 Store 里的一行。
type FolderItem = Item<Store<Vec<FolderRow>>, (String, u16, bool), FolderRow>;
/// 智能文件夹树 Store 里的一行。
type SmartItem = Item<Store<Vec<SmartRow>>, (String, u16, bool), SmartRow>;

/// 文件夹分组：标题带「新建」和「刷新」，正文是树或空状态。对应 `WorkspaceSidebarFolders.vue`。
/// 虚拟条目来源不显示这一组，`visible` 现读信号。
pub(super) fn folder_group(
    state: Signal<FolderGroup>,
    rows: Store<Vec<FolderRow>>,
    visible: impl Fn() -> bool + Send + 'static,
) -> AnyView {
    let tools = vec![
        tree_action(PLUS, "在当前目录新建文件夹", "folder-create", move || state.with(|state| state.create_disabled), move |cx| {
            let parent = state.with_untracked(|state| state.parent.clone());
            cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(GapMessage::OpenFolderCreate(parent))));
        })
        .into_any(),
        tree_action(
            refresh_icon(state.with_untracked(|state| state.loading)),
            "刷新文件夹树",
            "refresh-folder-tree",
            move || state.with(|state| state.refresh_disabled),
            |cx| cx.dispatch_program_all(sidebar_message(SidebarMessage::RefreshFolderTree)),
        )
        .prop::<Icon, fields::icon_button::icon>(move || refresh_icon(state.with(|state| state.loading)))
        .into_any(),
    ];
    let tree = rows
        .keyed(folder_key)
        .each(folder_row)
        .gap(3.0)
        .key("folder-tree")
        .visible(move || state.with(|state| state.tree));
    let hint = bound_hint(move || state.with(|state| state.hint), "folder-hint");
    group(group_header(widget(group_title("文件夹")).into_any(), tools, "folder-header"), vec![tree.into_any(), hint])
        .visible(visible)
        .into_any()
}

/// 当前目录所在的一支用打开的文件夹图标。
fn folder_icon(branch: bool) -> Icon {
    if branch { FOLDER_OPEN } else { FOLDER }
}

/// 一个目录：开合三角（没有子级时留空位）和行卡片。点卡片打开目录，右键打开目录菜单。
fn folder_row(item: FolderItem) -> AnyView {
    let row = item.get_untracked();
    let path = row.path;
    let toggle = row.has_children.then(|| {
        let toggle_path = path.clone();
        caret(move || item.expanded().get(), FOLDER_CARET, move || {
            sidebar_message(SidebarMessage::ToggleFolder(toggle_path.clone()))
        })
    });
    let content = widget(Stack::fill_row(8.0).align(AlignSpec::Center)).children((
        widget(Stack::fill_row(6.0).align(AlignSpec::Center)).children((
            widget(parts::inherit_icon(folder_icon(row.branch), 14.0))
                .prop::<Icon, GlyphIcon>(move || folder_icon(item.branch().get())),
            widget(parts::fill_text(row.label.clone(), 13.0, 500)).prop::<String, fields::text::value>(item.label()),
        )),
        widget(parts::label_text(row.count.to_string(), 11.0, 500, Some(SemanticColorRole::Faint)))
            .prop::<String, fields::text::value>(move || item.count().get().to_string()),
    ));
    let open_path = path.clone();
    let menu_path = path.clone();
    let card = widget(ListItem::new(row.label).selected(row.active).style(card_style()))
        .prop::<String, fields::list_item::label>(item.label())
        .prop::<bool, fields::list_item::selected>(item.active())
        .content(content)
        .key(format!("folder-row-{}", key_part(&path)))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenFolder(open_path.clone()))))
        .on_cx(move |_, event: &SecondaryPress, cx| {
            let label = item.try_with(|row| row.label.clone()).unwrap_or_default();
            cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(GapMessage::OpenFolderMenu {
                path: menu_path.clone(),
                label,
                x: event.x,
                y: event.y,
            })));
        });
    tree_row(row.depth, toggle, card.into_any())
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
/// 图标和无障碍名跟着 `expanded` 原地换，`labels` 是展开时和收起时的说明。
fn caret(
    expanded: impl Fn() -> bool + Copy + Send + 'static,
    labels: (&'static str, &'static str),
    message: impl Fn() -> ShellMessage + Send + 'static,
) -> AnyView {
    let look = move |expanded: bool| if expanded { (CARET_DOWN, labels.0) } else { (CARET_RIGHT, labels.1) };
    let (icon, label) = look(expanded());
    let mut button = IconButton::new(icon, label).size(ControlSize::Small);
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
    widget(button.colors_from_style())
        .prop::<Icon, fields::icon_button::icon>(move || look(expanded()).0)
        .prop::<Arc<str>, fields::icon_button::label>(move || Arc::from(look(expanded()).1))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(message()))
        .into_any()
}

/// 智能文件夹分组：标题带「新建」，正文是树或空状态。对应 `WorkspaceSidebarSmartFolders.vue`。
pub(super) fn smart_group(state: Signal<SmartGroup>, rows: Store<Vec<SmartRow>>) -> AnyView {
    let tools = vec![tree_action(PLUS, "新建智能文件夹", "smart-create", move || state.with(|state| state.create_disabled), |cx| {
        cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenSmartFolderDialog));
    })
    .into_any()];
    let hint = bound_hint(move || state.with(|state| state.hint), "smart-hint");
    let tree = rows
        .keyed(smart_key)
        .each(move |item| smart_row(item, state))
        .gap(3.0)
        .key("smart-tree")
        .visible(move || state.with(|state| state.hint.is_none()));
    group(group_header(widget(group_title("智能文件夹")).into_any(), tools, "smart-header"), vec![hint, tree.into_any()])
        .into_any()
}

/// 一个智能文件夹：开合三角和行卡片，卡片右侧常驻新建子级、编辑和删除。提交中时新建子级和编辑不可用。
/// 包含当前智能文件夹的一支用打开的文件夹图标。
fn smart_row(item: SmartItem, state: Signal<SmartGroup>) -> AnyView {
    let row = item.get_untracked();
    let id = row.smart_id;
    let toggle = row.has_children.then(|| {
        let toggle_id = id.clone();
        caret(move || item.expanded().get(), SMART_CARET, move || {
            sidebar_message(SidebarMessage::ToggleSmartFolder(toggle_id.clone()))
        })
    });
    let busy = move || state.with(|state| state.busy);
    let (open_id, child_id, edit_id, delete_id) = (id.clone(), id.clone(), id.clone(), id.clone());
    let actions = widget(Stack::row(2.0).align(AlignSpec::Center).shrink(0.0)).children((
        tree_action(FOLDER_PLUS, "新建子智能文件夹", "smart-row-create", busy, move |cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenSmartFolderChild(child_id.clone())));
        }),
        tree_action(PENCIL, "编辑智能文件夹", "smart-row-edit", busy, move |cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(GapMessage::OpenSmartEdit(edit_id.clone()))));
        }),
        super::danger_tree_action(TRASH, "删除智能文件夹", move |cx| {
            let label = item.try_with(|row| row.name.clone()).unwrap_or_default();
            cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(GapMessage::OpenSmartDelete {
                id: delete_id.clone(),
                label,
            })));
        }),
    ));
    let content = widget(Stack::fill_row(8.0).align(AlignSpec::Center)).children((
        widget(Stack::fill_row(6.0).align(AlignSpec::Center)).children((
            widget(parts::inherit_icon(folder_icon(row.branch), 14.0))
                .prop::<Icon, GlyphIcon>(move || folder_icon(item.branch().get())),
            widget(parts::fill_text(row.name.clone(), 13.0, 500)).prop::<String, fields::text::value>(item.name()),
        )),
        actions,
    ));
    let mut style = card_style();
    Arc::make_mut(&mut style.layout).padding_right = Some(LengthSpec::Px(1.0));
    let card = widget(ListItem::new(row.name).selected(row.active).style(style))
        .prop::<String, fields::list_item::label>(item.name())
        .prop::<bool, fields::list_item::selected>(item.active())
        .content(content)
        .key(format!("smart-row-{}", key_part(&id)))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenSmartFolder(open_id.clone()))));
    tree_row(row.depth, toggle, card.into_any())
}

/// 文件夹右键菜单要显示的东西：文件服务是否在处理（处理中时后三项禁用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FolderMenuView {
    pub mutating: bool,
}

impl FolderMenuView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        model.sidebar.folder_menu.as_ref()?;
        Some(Self { mutating: model.files.mutating })
    }
}

/// 菜单项：打开、新建子文件夹、重命名和删除。
fn folder_menu_items(mutating: bool) -> Vec<ContextMenuItem> {
    vec![
        ContextMenuItem::new("open", "打开").icon(FOLDER_OPEN),
        ContextMenuItem::new("create", "新建子文件夹").icon(FOLDER_PLUS).disabled(mutating),
        ContextMenuItem::new("rename", "重命名").icon(PENCIL).disabled(mutating),
        ContextMenuItem::new("delete", "删除").icon(TRASH).disabled(mutating).danger(true),
    ]
}

/// 菜单项的禁用跟着文件服务走：变了才换条目。
struct MenuItemsBusy;

impl FieldWrite<ContextMenu, bool> for MenuItemsBusy {
    const FIELD: &'static str = "ContextMenu.items(mutating)";

    fn write(target: &mut ContextMenu, mutating: bool) {
        target.items = folder_menu_items(mutating);
    }

    fn differs(target: &ContextMenu, mutating: &bool) -> bool {
        target.items != folder_menu_items(*mutating)
    }
}

/// 文件夹行右键菜单：打开、新建子文件夹、重命名和删除。文件服务处理中时后三项禁用。
/// 常驻：同一行同一落点的菜单打开期间只改条目的禁用，换一行或换落点时浮层块换块。
pub fn folder_menu(model: &ShellViewModel) -> Option<AnyView> {
    let menu = model.sidebar.folder_menu.clone()?;
    let view = signal(FolderMenuView::project(model)?);
    Projected::register(view, FolderMenuView::project);
    let path = menu.path.clone();
    let label = menu.label.clone();
    Some(
        widget(ContextMenu::new(menu.x, menu.y).items(folder_menu_items(view.with_untracked(|view| view.mutating))))
            .key("folder-context-menu")
            .prop::<bool, MenuItemsBusy>(move || view.with(|view| view.mutating))
            .on_cx(move |_, event: &ContextMenuEvent, cx| match event {
                ContextMenuEvent::Search(_) => {}
                ContextMenuEvent::Dismiss => cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(GapMessage::CloseFolderMenu))),
                ContextMenuEvent::Select(value) => {
                    cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(GapMessage::CloseFolderMenu)));
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
                    cx.dispatch_program_all(sidebar_message(message));
                }
            })
            .into_any(),
    )
}

//! 文件夹树和智能文件夹树。
//!
//! 行结构照 Vue `FolderTreeNode.vue` / `SmartFolderTreeNode.vue`：最左 10px 的折叠三角，
//! 然后是 24px 高的行卡片（文件夹图标、名称、右侧计数），子级每层缩进 20px，行距 3px。
//! 深度大于 1 的行在缩进处画 `--border` 竖线，和 `.workspace-folder-tree__row::before` 一样；
//! 有子级时三角背后的底色盖住当前这一层的线。当前目录的卡片是 `--bg-active` 灰底。
//! 文件夹行右键打开「打开 / 新建子文件夹 / 重命名 / 删除」菜单，
//! 菜单键和 Shift+F10 也打开它、焦点落在首项（和文件右键菜单同一份实现，`context_menu.rs`）；
//! 智能文件夹行右侧常驻新建子级、编辑和删除三个小按钮。
//!
//! 两棵树按显示顺序展开成一列行，放在按键对照的 Store 里，用带键的 `each` 建：展开一个目录只插入
//! 它的子级，折叠只删掉子级；开合三角、当前目录、打开的图标和计数都是行内绑定，变了原地改。
//! 行的键是路径（或编号）加深度和有没有子级，这两样决定行的结构，变了才整行重建。

use std::sync::Arc;

use nana_ui::ControlSize;
use nana_ui::Icon;
use nana_ui::icons_tabler::{
    CARET_DOWN, CARET_RIGHT, FOLDER, FOLDER_OPEN, FOLDER_PLUS, PENCIL, PLUS, TRASH,
};
use nana_ui::runtime::FileDropEvent;
use nana_ui::runtime::view::node_ref;
use nana_ui::runtime::view::{AnyView, IntoView, signal, widget};
use nana_ui::runtime::view::{Item, Signal, Store, StoreList, StorePath, fields};
use nana_ui::runtime::{
    Activate, AlignSpec, IconButton, LengthSpec, ListItem, NodeStyle, SecondaryPress,
    SemanticColorRole, SemanticPaint, Stack, Text,
};
use nana_ui_core::SemanticColorMix;

use super::super::field_bind::StyleField;
use super::super::input::{HostDragPhase, InputMessage, accept_file_drops};

use super::super::context_menu::{MenuAction, MenuChrome, MenuEntry, MenuView, context_menu};
use super::super::field_bind::GlyphIcon;
use super::super::menu_focus::{MenuEdge, MenuSurface, focus_menu};
use super::super::player_view::key_part;
use super::super::sidebar::GapMessage;
use super::super::view_part_overlay::session::Projected;
use super::super::view_part_sidebar::project::{
    FolderGroup, FolderRow, FolderRowStoreFields, SmartGroup, SmartRow, SmartRowStoreFields,
    folder_key, smart_key,
};
use super::super::{ShellMessage, ShellViewModel, SidebarMessage};
use super::parts::{self, ActiveTone, bound_hint, group_header, group_title, tree_action};
use super::{group, refresh_icon, sidebar_message};

/// 每层缩进，对应 `--folder-tree-indent`。
const TREE_INDENT: f32 = 20.0;
/// 折叠三角的宽度，对应 `--folder-tree-toggle-width`。
const TOGGLE_WIDTH: f32 = 10.0;
const ROW_HEIGHT: f32 = 24.0;
/// 导引线起点：三角宽 10、行内边距 4，再加 6.5，对应 `--folder-tree-guide-offset`。
const GUIDE_OFFSET: f32 = TOGGLE_WIDTH + 4.0 + 6.5;
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
        tree_action(
            PLUS,
            "在当前目录新建文件夹",
            "folder-create",
            move || state.with(|state| state.create_disabled),
            move |cx| {
                let parent = state.with_untracked(|state| state.parent.clone());
                cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(
                    GapMessage::OpenFolderCreate(parent),
                )));
            },
        )
        .into_any(),
        tree_action(
            refresh_icon(state.with_untracked(|state| state.loading)),
            "刷新文件夹树",
            "refresh-folder-tree",
            move || state.with(|state| state.refresh_disabled),
            |cx| cx.dispatch_program_all(sidebar_message(SidebarMessage::RefreshFolderTree)),
        )
        .prop::<Icon, fields::icon_button::icon>(move || {
            refresh_icon(state.with(|state| state.loading))
        })
        .into_any(),
    ];
    let drop = node_ref();
    accept_file_drops(drop);
    let tree = widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .node_ref(drop)
    .visible(move || state.with(|state| state.tree))
    .on_cx(|_, event: &FileDropEvent, cx| cx.dispatch_program_all(sidebar_file_drag(event)))
    .children((rows
        .keyed(folder_key)
        .each(folder_row)
        .gap(3.0)
        .key("folder-tree"),))
    .key("folder-drop");
    let hint = bound_hint(move || state.with(|state| state.hint), "folder-hint");
    group(
        group_header(
            widget(group_title("文件夹")).into_any(),
            tools,
            "folder-header",
        ),
        vec![tree.into_any(), hint],
    )
    .visible(visible)
    .into_any()
}

/// 当前目录所在的一支用打开的文件夹图标。
fn folder_icon(branch: bool) -> Icon {
    if branch { FOLDER_OPEN } else { FOLDER }
}

/// 文件夹行卡片的键。
pub(crate) fn folder_row_key(path: &str) -> String {
    format!("folder-row-{}", key_part(path))
}

/// 一个目录：开合三角（没有子级时留空位）和行卡片。点卡片打开目录，右键打开目录菜单。
fn folder_row(item: FolderItem) -> AnyView {
    let row = item.get_untracked();
    let path = row.path;
    let toggle = row.has_children.then(|| {
        let toggle_path = path.clone();
        caret(
            move || item.expanded().get(),
            FOLDER_CARET,
            move || sidebar_message(SidebarMessage::ToggleFolder(toggle_path.clone())),
        )
    });
    let content = widget(Stack::fill_row(8.0).align(AlignSpec::Center)).children((
        widget(Stack::fill_row(6.0).align(AlignSpec::Center)).children((
            widget(parts::inherit_icon(folder_icon(row.branch), 14.0))
                .prop::<Icon, GlyphIcon>(move || folder_icon(item.branch().get())),
            widget(parts::fill_text(row.label.clone(), 13.0, 500))
                .prop::<String, fields::text::value>(item.label()),
        )),
        widget(parts::label_text(
            row.count.to_string(),
            11.0,
            500,
            Some(SemanticColorRole::Faint),
        ))
        .prop::<String, fields::text::value>(move || item.count().get().to_string()),
    ));
    let open_path = path.clone();
    let menu_path = path.clone();
    let card = widget(
        ListItem::new(row.label.clone())
            .selected(row.active)
            .style(card_style(row.drop_target)),
    )
    .prop::<String, fields::list_item::label>(item.label())
    .prop::<bool, fields::list_item::selected>(item.active())
    .prop::<NodeStyle, StyleField>(move || {
        card_style(item.try_with(|row| row.drop_target).unwrap_or(false))
    })
    .content(content)
    .key(folder_row_key(&path))
    .on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenFolder(
            open_path.clone(),
        )))
    })
    .on_cx(move |_, event: &SecondaryPress, cx| {
        let label = item.try_with(|row| row.label.clone()).unwrap_or_default();
        cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(
            GapMessage::OpenFolderMenu {
                path: menu_path.clone(),
                label,
                x: event.x,
                y: event.y,
            },
        )));
        if let Some(edge) = MenuEdge::of_press(event) {
            cx.dispatch_program_all(focus_menu(MenuSurface::Folder, edge));
        }
    });
    tree_row(
        row.depth,
        row.has_children,
        toggle,
        card.into_any(),
        Some(path_marker(&path)),
    )
}

/// 隐藏的路径标记。父级是这一行的卡片，实况拖放按它认出侧栏文件夹。
fn path_marker(path: &str) -> AnyView {
    let mut marker = Text::new(format!("momobako-entry:sidebar:{path}"));
    let layout = Arc::make_mut(&mut marker.style.layout);
    layout.hidden = true;
    layout.height = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Px(0.0));
    widget(marker).into_any()
}

/// 侧栏目录树收到的系统文件拖放，交给输入归约。悬停目标由坐标另记。
fn sidebar_file_drag(event: &FileDropEvent) -> ShellMessage {
    let (phase, paths) = match event {
        FileDropEvent::Hovered { paths, .. } => (HostDragPhase::Over, owned_paths(paths)),
        FileDropEvent::Dropped { paths, .. } => (HostDragPhase::Drop, owned_paths(paths)),
        FileDropEvent::Left => (HostDragPhase::Leave, Vec::new()),
    };
    ShellMessage::Input(InputMessage::SidebarFileDrag { phase, paths })
}

fn owned_paths(paths: &[std::path::PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// 树行卡片：24px、左 4 右 6 内边距；悬停 `--bg-hover`，当前 `--bg-active`。
/// 放下目标照 Vue `.workspace-folder-tree__row.is-drop-target`：强调色 14% 混进当前底，边是 55% 强调色。
fn card_style(drop_target: bool) -> NodeStyle {
    let mut style = parts::row_style(ROW_HEIGHT, 4.0, 6.0, 8.0, ActiveTone::Neutral, false);
    let layout = Arc::make_mut(&mut style.layout);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.width = Some(LengthSpec::Px(0.0));
    if drop_target {
        layout.border_width = Some(1.0);
        let paint = SemanticPaint {
            background_mix: Some(SemanticColorMix::new(
                SemanticColorRole::Accent,
                SemanticColorRole::Active,
                0.14,
            )),
            border_mix: Some(SemanticColorMix::alpha(SemanticColorRole::Accent, 0.55)),
            foreground: Some(SemanticColorRole::Text),
            ..SemanticPaint::default()
        };
        style.interaction.base = paint;
        style.interaction.hovered = paint;
        style.interaction.pressed = paint;
        style.interaction.selected = paint;
        style.interaction.selected_hovered = paint;
        style.interaction.selected_pressed = paint;
    }
    style
}

/// 一行：缩进导引线、折叠三角（没有子级时留空位）和卡片。
fn tree_row(
    depth: u16,
    has_children: bool,
    toggle: Option<AnyView>,
    card: AnyView,
    marker: Option<AnyView>,
) -> AnyView {
    let toggle = toggle.unwrap_or_else(|| {
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(TOGGLE_WIDTH))
                .height(LengthSpec::Px(20.0))
                .shrink(0.0),
        )
        .into_any()
    });
    let mut children = guide_xs(depth, has_children)
        .into_iter()
        .map(guide_line)
        .collect::<Vec<_>>();
    children.push(toggle);
    children.push(card);
    if let Some(marker) = marker {
        children.push(marker);
    }
    widget(
        Stack::bar(0.0)
            .align(AlignSpec::Center)
            .height(LengthSpec::Px(ROW_HEIGHT))
            .with_layout(|layout| {
                layout.position = nana_ui_core::PositionSpec::Relative;
                layout.padding_left = Some(LengthSpec::Px(
                    f32::from(depth.saturating_sub(1)) * TREE_INDENT,
                ));
            }),
    )
    .children(children)
    .into_any()
}

/// 这一行要画的竖线横坐标。
///
/// Vue 在行的 `::before` 上按缩进重复 1px 的 `--border`：第 i 条在 `20.5 + i * 20`。
/// 有子级时三角的 `::before` 用侧栏底色盖住当前这一层（最后一条），祖先层的线留着；
/// 没有子级时三角是 `visibility: hidden`，盖板一起藏掉，当前层的线也露出来。
/// 线高就是这一行的 24px，不伸进 3px 行距，和 `::before` 只盖住本行一样。
fn guide_xs(depth: u16, has_children: bool) -> Vec<f32> {
    let levels = usize::from(depth.saturating_sub(1));
    let shown = if has_children {
        levels.saturating_sub(1)
    } else {
        levels
    };
    (0..shown)
        .map(|index| GUIDE_OFFSET + index as f32 * TREE_INDENT)
        .collect()
}

/// 一条 1px 的缩进导引线，不接点击。
fn guide_line(x: f32) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(1.0))
            .height(LengthSpec::Px(ROW_HEIGHT))
            .min_width(LengthSpec::Px(1.0))
            .min_height(LengthSpec::Px(ROW_HEIGHT))
            .grow(0.0)
            .shrink(0.0)
            .surface(SemanticColorRole::Border)
            .with_layout(|layout| {
                layout.position = nana_ui_core::PositionSpec::Absolute;
                layout.offset_left = Some(LengthSpec::Px(x));
                layout.offset_top = Some(LengthSpec::Px(0.0));
                layout.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
            }),
    )
    .into_any()
}

/// 折叠三角：10×20，弱色，展开后朝下。对应 `.workspace-folder-tree__toggle-caret`。
/// 图标和无障碍名跟着 `expanded` 原地换，`labels` 是展开时和收起时的说明。
fn caret(
    expanded: impl Fn() -> bool + Copy + Send + 'static,
    labels: (&'static str, &'static str),
    message: impl Fn() -> ShellMessage + Send + 'static,
) -> AnyView {
    let look = move |expanded: bool| {
        if expanded {
            (CARET_DOWN, labels.0)
        } else {
            (CARET_RIGHT, labels.1)
        }
    };
    let (icon, label) = look(expanded());
    let mut button = IconButton::new(icon, label).size(ControlSize::Small);
    let mut style = parts::icon_button_style(
        TOGGLE_WIDTH,
        nana_ui::runtime::RadiusTier::Xs,
        SemanticColorRole::Faint,
        None,
    );
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
    let tools = vec![
        tree_action(
            PLUS,
            "新建智能文件夹",
            "smart-create",
            move || state.with(|state| state.create_disabled),
            |cx| {
                cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenSmartFolderDialog));
            },
        )
        .into_any(),
    ];
    let hint = bound_hint(move || state.with(|state| state.hint), "smart-hint");
    let tree = rows
        .keyed(smart_key)
        .each(move |item| smart_row(item, state))
        .gap(3.0)
        .key("smart-tree")
        .visible(move || state.with(|state| state.hint.is_none()));
    group(
        group_header(
            widget(group_title("智能文件夹")).into_any(),
            tools,
            "smart-header",
        ),
        vec![hint, tree.into_any()],
    )
    .into_any()
}

/// 一个智能文件夹：开合三角和行卡片，卡片右侧常驻新建子级、编辑和删除。提交中时新建子级和编辑不可用。
/// 包含当前智能文件夹的一支用打开的文件夹图标。
fn smart_row(item: SmartItem, state: Signal<SmartGroup>) -> AnyView {
    let row = item.get_untracked();
    let id = row.smart_id;
    let toggle = row.has_children.then(|| {
        let toggle_id = id.clone();
        caret(
            move || item.expanded().get(),
            SMART_CARET,
            move || sidebar_message(SidebarMessage::ToggleSmartFolder(toggle_id.clone())),
        )
    });
    let busy = move || state.with(|state| state.busy);
    let (open_id, child_id, edit_id, delete_id) = (id.clone(), id.clone(), id.clone(), id.clone());
    let actions = widget(Stack::row(2.0).align(AlignSpec::Center).shrink(0.0)).children((
        tree_action(
            FOLDER_PLUS,
            "新建子智能文件夹",
            "smart-row-create",
            busy,
            move |cx| {
                cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenSmartFolderChild(
                    child_id.clone(),
                )));
            },
        ),
        tree_action(
            PENCIL,
            "编辑智能文件夹",
            "smart-row-edit",
            busy,
            move |cx| {
                cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(
                    GapMessage::OpenSmartEdit(edit_id.clone()),
                )));
            },
        ),
        super::danger_tree_action(TRASH, "删除智能文件夹", move |cx| {
            let label = item.try_with(|row| row.name.clone()).unwrap_or_default();
            cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(
                GapMessage::OpenSmartDelete {
                    id: delete_id.clone(),
                    label,
                },
            )));
        }),
    ));
    let content = widget(Stack::fill_row(8.0).align(AlignSpec::Center)).children((
        widget(Stack::fill_row(6.0).align(AlignSpec::Center)).children((
            widget(parts::inherit_icon(folder_icon(row.branch), 14.0))
                .prop::<Icon, GlyphIcon>(move || folder_icon(item.branch().get())),
            widget(parts::fill_text(row.name.clone(), 13.0, 500))
                .prop::<String, fields::text::value>(item.name()),
        )),
        actions,
    ));
    let mut style = card_style(false);
    Arc::make_mut(&mut style.layout).padding_right = Some(LengthSpec::Px(1.0));
    let card = widget(ListItem::new(row.name).selected(row.active).style(style))
        .prop::<String, fields::list_item::label>(item.name())
        .prop::<bool, fields::list_item::selected>(item.active())
        .content(content)
        .key(format!("smart-row-{}", key_part(&id)))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenSmartFolder(
                open_id.clone(),
            )))
        });
    tree_row(row.depth, row.has_children, toggle, card.into_any(), None)
}

/// 文件夹右键菜单的键前缀和关闭消息。没有分组，也没有要点两次的项。
const FOLDER_MENU: MenuChrome = MenuChrome {
    prefix: "folder",
    close: || ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseFolderMenu)),
    branch: None,
    arm: None,
};

/// 文件夹右键菜单的投影。
pub(crate) struct FolderMenuView;

impl FolderMenuView {
    /// 照 Vue `FolderTreeNode.vue` 的 `contextMenuItems`：打开、新建子文件夹、重命名和删除，
    /// 文件服务处理中时后三项禁用。
    pub(crate) fn project(model: &ShellViewModel) -> Option<MenuView> {
        let menu = model.sidebar.folder_menu.as_ref()?;
        let mutating = model.files.mutating;
        let (path, label) = (menu.path.clone(), menu.label.clone());
        let entries = vec![
            MenuEntry::new("open", "打开", Some(FOLDER_OPEN))
                .message(MenuAction::OpenFolder(path.clone())),
            MenuEntry::new("create", "新建子文件夹", Some(FOLDER_PLUS))
                .disabled(mutating)
                .message(MenuAction::Gap(GapMessage::OpenFolderCreate(path.clone()))),
            MenuEntry::new("rename", "重命名", Some(PENCIL))
                .disabled(mutating)
                .message(MenuAction::Gap(GapMessage::OpenFolderRename {
                    path: path.clone(),
                    label: label.clone(),
                })),
            MenuEntry::new("delete", "删除", Some(TRASH))
                .disabled(mutating)
                .danger()
                .message(MenuAction::Gap(GapMessage::OpenFolderDelete {
                    path,
                    label,
                })),
        ];
        Some(MenuView::new(entries, None))
    }
}

/// 文件夹行右键菜单，和文件右键菜单是同一份实现（`context_menu.rs`）。常驻：同一行同一落点的菜单
/// 打开期间只重建变了的那几行（处理中切换禁用），换一行或换落点时浮层块换块。
pub fn folder_menu(model: &ShellViewModel) -> Option<AnyView> {
    let menu = model.sidebar.folder_menu.clone()?;
    let view = signal(FolderMenuView::project(model)?);
    Projected::register(view, FolderMenuView::project);
    Some(context_menu(view, menu.x, menu.y, FOLDER_MENU))
}

#[cfg(test)]
mod tests {
    use super::guide_xs;

    /// 嵌套行的导引线横坐标：顶层没有线；有子级的行盖住当前层；叶子露出当前层和祖先层。
    #[test]
    fn nested_rows_keep_ancestor_guides() {
        assert!(guide_xs(1, false).is_empty());
        assert!(guide_xs(1, true).is_empty());
        assert!(guide_xs(2, true).is_empty());
        assert_eq!(guide_xs(2, false), vec![20.5]);
        assert_eq!(guide_xs(3, true), vec![20.5]);
        assert_eq!(guide_xs(3, false), vec![20.5, 40.5]);
    }
}

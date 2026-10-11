//! 视图照 [`MenuFocus`](super::menu_focus::MenuFocus) 把焦点放进键盘打开的菜单，菜单关掉时还给打开它的控件。
//!
//! 每次整体同步收尾时调 [`MenuFocusView::sync`]：这时新打开的菜单已经挂好。按菜单项所在方向键焦点组的键
//! 找到组节点，用 NanaUI 的 `focus_roving_edge` 聚焦首项或末项（跳过禁用和藏着的项），同时记下打开前焦点
//! 在哪。之后菜单关掉（Escape、点外面、选了一项）时，焦点要是跟着菜单项没了，或者还留在藏起来的菜单里，
//! 就还给打开它的控件；选了一项打开对话框时焦点已经在对话框里，不动它。

use nana_ui::runtime::{AppContext, DocumentId, StableNodeId, UiWorld};

use super::menu_focus::MenuSurface;
use super::ShellViewModel;

/// 焦点被放进去、还开着的菜单。
#[derive(Clone, Copy, Debug)]
struct FocusedMenu {
    surface: MenuSurface,
    /// 菜单项所在的方向键焦点组。
    group: StableNodeId,
    /// 打开菜单前焦点所在的控件（菜单按钮或文件卡片）。
    trigger: Option<StableNodeId>,
}

/// 壳层视图里管键盘菜单焦点的那一份状态。
#[derive(Debug, Default)]
pub(crate) struct MenuFocusView {
    /// 已经照做过的最新请求序号。
    applied: u64,
    focused: Option<FocusedMenu>,
}

impl MenuFocusView {
    /// 新挂的视图不照做挂载以前的请求。
    pub(crate) fn new(model: &ShellViewModel) -> Self {
        Self { applied: model.menu_focus.latest().map_or(0, |(serial, _)| serial), focused: None }
    }

    /// 整体同步收尾时调：先把关掉的菜单的焦点还回去，再照新的请求聚焦。
    pub(crate) fn sync(&mut self, context: &mut AppContext, document: DocumentId, model: &ShellViewModel) {
        self.hand_back(context, document, model);
        let Some((serial, request)) = model.menu_focus.latest() else {
            return;
        };
        if serial == self.applied {
            return;
        }
        self.applied = serial;
        if !request.surface.is_open(model) {
            return;
        }
        let Some(group) = find_group(context, document, request.surface.group_key()) else {
            eprintln!("Nana 键盘打开的菜单还没有挂好，焦点留在原处：{:?}", request.surface);
            return;
        };
        let trigger = context.world().focused(document);
        match context.focus_roving_edge(group, request.edge.roving()) {
            Ok(Some(_)) => self.focused = Some(FocusedMenu { surface: request.surface, group, trigger }),
            Ok(None) => eprintln!("Nana 键盘打开的菜单里没有能聚焦的项：{:?}", request.surface),
            Err(error) => eprintln!("Nana 聚焦键盘打开的菜单失败：{:?} {error}", request.surface),
        }
    }

    /// 焦点放进去的菜单关掉了：焦点跟着菜单项一起没了，或者还留在藏起来的菜单里时，还给打开它的控件。
    fn hand_back(&mut self, context: &mut AppContext, document: DocumentId, model: &ShellViewModel) {
        let Some(menu) = self.focused else {
            return;
        };
        if menu.surface.is_open(model) {
            return;
        }
        self.focused = None;
        let world = context.world();
        let lost = world.focused(document).is_none_or(|focused| world.node(focused).is_none() || within(world, focused, menu.group));
        let Some(trigger) = menu.trigger.filter(|trigger| world.node(*trigger).is_some()) else {
            return;
        };
        if !lost {
            return;
        }
        if let Err(error) = context.focus_node(document, trigger) {
            eprintln!("Nana 菜单关掉后焦点没能回到打开它的控件：{:?} {error}", menu.surface);
        }
    }
}

/// 键路径最后一段是 `key`、声明了方向键焦点组的节点。
fn find_group(context: &AppContext, document: DocumentId, key: &str) -> Option<StableNodeId> {
    context.world().document_order(document).into_iter().find(|id| {
        context.roving_focus_group(*id).is_some() && context.assembly_path(*id).is_some_and(|path| path.rsplit('/').next() == Some(key))
    })
}

/// `node` 是 `ancestor` 或在它下面。
fn within(world: &UiWorld, node: StableNodeId, ancestor: StableNodeId) -> bool {
    let mut cursor = Some(node);
    while let Some(id) = cursor {
        if id == ancestor {
            return true;
        }
        cursor = world.parent_id(id);
    }
    false
}

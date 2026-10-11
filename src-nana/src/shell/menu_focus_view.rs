//! 视图照 [`MenuFocus`](super::menu_focus::MenuFocus) 把焦点放进键盘打开的菜单，菜单关掉时还给打开它的控件。
//!
//! 每次整体同步收尾时调 [`MenuFocusView::sync`]：这时新打开的菜单已经挂好。按菜单项所在方向键焦点组的键
//! 找到组节点，用 NanaUI 的 `focus_roving_edge` 聚焦首项或末项（跳过禁用和藏着的项），同时记下打开前焦点
//! 在哪。之后菜单关掉（Escape、点外面、选了一项）时，焦点要是跟着菜单项没了，或者还留在藏起来的菜单里，
//! 就还给打开它的控件；选了一项打开对话框时焦点已经在对话框里，不动它。仓库弹层换页（切换列表、添加菜单、
//! 后端表单）时弹层还开着：换到菜单页时焦点跟进新的一页的首项，整个弹层关掉才还给仓库头。

use nana_ui::runtime::{AppContext, DocumentId, RovingEdge, StableNodeId, UiWorld};

use super::menu_focus::MenuSurface;
use super::sidebar::PopoverMode;
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

    /// 整体同步收尾时调：先处理关掉或换页的菜单，再照新的请求聚焦。
    pub(crate) fn sync(&mut self, context: &mut AppContext, document: DocumentId, model: &ShellViewModel) {
        self.settle_closed(context, document, model);
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
        let trigger = context.world().focused(document);
        if let Some(group) = focus_edge(context, document, request.surface, request.edge.roving()) {
            self.focused = Some(FocusedMenu { surface: request.surface, group, trigger });
        }
    }

    /// 焦点放进去的菜单不在了（关掉，或者同一种菜单换到别的条目上重新打开）。仓库弹层只是换了页时跟进新的
    /// 一页；别的情况下，焦点跟着菜单项没了或者还留在藏起来的菜单里时，还给打开它的控件。
    fn settle_closed(&mut self, context: &mut AppContext, document: DocumentId, model: &ShellViewModel) {
        let Some(menu) = self.focused else {
            return;
        };
        if menu.surface.is_open(model) && context.world().node(menu.group).is_some() {
            return;
        }
        if menu.surface.in_repository_popover() && model.sidebar.popover != PopoverMode::Closed {
            self.follow_repository_page(context, document, model, menu);
            return;
        }
        self.focused = None;
        if !focus_lost(context.world(), document, menu.group) {
            return;
        }
        let Some(trigger) = menu.trigger.filter(|trigger| context.world().node(*trigger).is_some()) else {
            return;
        };
        if let Err(error) = context.focus_node(document, trigger) {
            eprintln!("Nana 菜单关掉后焦点没能回到打开它的控件：{:?} {error}", menu.surface);
        }
    }

    /// 仓库弹层还开着、换了页：新的一页是菜单、焦点又跟着上一页没了时，聚焦它的首项。后端表单不是菜单，
    /// 焦点交给 Tab；等它换回菜单页或者弹层关掉再处理。
    fn follow_repository_page(&mut self, context: &mut AppContext, document: DocumentId, model: &ShellViewModel, menu: FocusedMenu) {
        let Some(page) = MenuSurface::repository_page(model) else {
            return;
        };
        if !focus_lost(context.world(), document, menu.group) {
            return;
        }
        if let Some(group) = focus_edge(context, document, page, RovingEdge::Start) {
            self.focused = Some(FocusedMenu { surface: page, group, ..menu });
        }
    }
}

/// 聚焦 `surface` 那一组的 `edge` 一端，返回组节点。找不到组或者组里没有能聚焦的项时记日志。
fn focus_edge(context: &mut AppContext, document: DocumentId, surface: MenuSurface, edge: RovingEdge) -> Option<StableNodeId> {
    let Some(group) = find_group(context, document, surface.group_key()) else {
        eprintln!("Nana 键盘打开的菜单还没有挂好，焦点留在原处：{surface:?}");
        return None;
    };
    match context.focus_roving_edge(group, edge) {
        Ok(Some(_)) => Some(group),
        Ok(None) => {
            eprintln!("Nana 键盘打开的菜单里没有能聚焦的项：{surface:?}");
            None
        }
        Err(error) => {
            eprintln!("Nana 聚焦键盘打开的菜单失败：{surface:?} {error}");
            None
        }
    }
}

/// 键路径最后一段是 `key`、声明了方向键焦点组的节点。
fn find_group(context: &AppContext, document: DocumentId, key: &str) -> Option<StableNodeId> {
    context.world().document_order(document).into_iter().find(|id| {
        context.roving_focus_group(*id).is_some() && context.assembly_path(*id).is_some_and(|path| path.rsplit('/').next() == Some(key))
    })
}

/// 焦点没了：没有焦点、焦点节点已经卸掉，或者还在 `group` 里（菜单藏起来了）。
fn focus_lost(world: &UiWorld, document: DocumentId, group: StableNodeId) -> bool {
    world.focused(document).is_none_or(|focused| world.node(focused).is_none() || within(world, focused, group))
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

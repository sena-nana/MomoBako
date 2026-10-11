//! 视图照 [`MenuFocus`](super::menu_focus::MenuFocus) 把焦点放进键盘打开的菜单。
//!
//! 每次整体同步收尾时调 [`MenuFocusView::sync`]：这时新打开的菜单已经挂好。按菜单项所在方向键焦点组的键
//! 找到组节点，用 NanaUI 的 `focus_roving_edge` 聚焦首项或末项（跳过禁用和藏着的项）。菜单关掉以后焦点
//! 还给打开它的控件、弹层换页时跟进新的一页，都归焦点归还（`focus_return.rs`），指针和键盘打开的菜单
//! 一样处理。

use nana_ui::runtime::{AppContext, DocumentId, RovingEdge, StableNodeId};

use super::menu_focus::MenuSurface;
use super::ShellViewModel;

/// 壳层视图里管键盘菜单焦点的那一份状态。
#[derive(Debug, Default)]
pub(crate) struct MenuFocusView {
    /// 已经照做过的最新请求序号。
    applied: u64,
}

impl MenuFocusView {
    /// 新挂的视图不照做挂载以前的请求。
    pub(crate) fn new(model: &ShellViewModel) -> Self {
        Self { applied: model.menu_focus.latest().map_or(0, |(serial, _)| serial) }
    }

    /// 整体同步收尾时调：照新的请求聚焦。
    pub(crate) fn sync(&mut self, context: &mut AppContext, document: DocumentId, model: &ShellViewModel) {
        let Some((serial, request)) = model.menu_focus.latest() else {
            return;
        };
        if serial == self.applied {
            return;
        }
        self.applied = serial;
        if request.surface.is_open(model) {
            focus_edge(context, document, request.surface, request.edge.roving());
        }
    }
}

/// 聚焦 `surface` 那一组的 `edge` 一端。找不到组或者组里没有能聚焦的项时记日志。
fn focus_edge(context: &mut AppContext, document: DocumentId, surface: MenuSurface, edge: RovingEdge) {
    let Some(group) = find_group(context, document, surface.group_key()) else {
        eprintln!("Nana 键盘打开的菜单还没有挂好，焦点留在原处：{surface:?}");
        return;
    };
    match context.focus_roving_edge(group, edge) {
        Ok(Some(_)) => {}
        Ok(None) => eprintln!("Nana 键盘打开的菜单里没有能聚焦的项：{surface:?}"),
        Err(error) => eprintln!("Nana 聚焦键盘打开的菜单失败：{surface:?} {error}"),
    }
}

/// 键路径最后一段是 `key`、声明了方向键焦点组的节点。
fn find_group(context: &AppContext, document: DocumentId, key: &str) -> Option<StableNodeId> {
    context.world().document_order(document).into_iter().find(|id| {
        context.roving_focus_group(*id).is_some() && context.assembly_path(*id).is_some_and(|path| path.rsplit('/').next() == Some(key))
    })
}

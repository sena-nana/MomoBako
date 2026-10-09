//! 侧栏块（常驻）：仓库头、文件管理分组和底部入口，放进工作区的资源区。
//!
//! 信号在骨架的挂载作用域里建（[`SidebarSignals`]），侧栏视图只建一次，之后每次同步只把
//! [`SidebarView`] 投影写进信号：路由切换、换目录、后台读回只改绑定的字段和变了的那几行，
//! 节点不换。整块重挂只发生在工作台排法变了的时候（侧栏收起再展开），由 `mount` 完成。

use nana_ui::runtime::{FrameworkError, MountedView, RuntimeDocument, StableNodeId};

use super::remount_state;
use super::view_part::{composing_under, first_root, mount_detached, BodyMode, PartCx, PartId, ShellPart, Swap};
use super::ShellViewModel;

#[path = "sidebar_project.rs"]
pub(super) mod project;

use project::{SidebarSignals, SidebarView};

/// 侧栏块。主区独占时没有内容。
pub(crate) struct SidebarPart {
    signals: SidebarSignals,
    present: bool,
    view: Option<MountedView>,
}

impl ShellPart for SidebarPart {
    const ID: PartId = PartId::Sidebar;
    type Signals = SidebarSignals;

    fn signals(model: &ShellViewModel) -> Self::Signals {
        SidebarSignals::new(SidebarView::project(model))
    }

    fn new(signals: Self::Signals) -> Self {
        Self { signals, present: false, view: None }
    }

    fn root(&self) -> Option<StableNodeId> {
        first_root(self.view.as_ref())
    }

    /// 排法变了：有侧栏时按当前投影建一份，主区独占时卸掉。
    fn mount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel, mode: BodyMode) -> Result<Swap, FrameworkError> {
        self.present = mode == BodyMode::Workbench;
        self.sync(model);
        self.remount(cx, model)
    }

    fn sync(&mut self, model: &ShellViewModel) {
        self.signals.write(SidebarView::project(model));
    }

    /// 侧栏的结构都由信号驱动，没有要重挂才能跟上的变化。
    fn needs_remount(&self, _: &ShellViewModel) -> bool {
        false
    }

    fn remount(&mut self, cx: &mut PartCx<'_>, _: &ShellViewModel) -> Result<Swap, FrameworkError> {
        let document_id = cx.document.document();
        let kept = self.view.as_ref().map(|view| remount_state::capture(cx.document.context(), document_id, view.roots()));
        let fresh = if self.present {
            let (signals, hot) = (self.signals, cx.hot);
            mount_detached(cx.document, cx.hot, Self::ID, move || Some(super::sidebar_view::sidebar(signals, hot)))?
        } else {
            None
        };
        cx.stats.remounts += 1;
        let old = std::mem::replace(&mut self.view, fresh);
        Ok(Swap::replace(old, kept, self.view.as_ref()))
    }

    fn composing(&self, document: &RuntimeDocument) -> bool {
        self.view.as_ref().is_some_and(|view| composing_under(document, view.roots()))
    }
}

#[cfg(test)]
#[path = "view_part_sidebar_tests.rs"]
mod tests;

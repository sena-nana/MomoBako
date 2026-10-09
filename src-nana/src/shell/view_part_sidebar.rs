//! 侧栏块（常驻）：仓库头、文件管理分组和底部入口，放进工作区的资源区。
//!
//! 信号在骨架的挂载作用域里建（[`SidebarSignals`]），侧栏视图只建一次，之后每次同步只把
//! [`SidebarView`] 投影写进信号：路由切换、换目录、后台读回只改绑定的字段和变了的那几行，
//! 节点不换。第一次排成工作台时挂；之后收起侧栏（主区独占）时它留在停放的工作区里，展开时原样回来。

use nana_ui::runtime::{FrameworkError, MountedView, RuntimeDocument, StableNodeId};

use super::view_part::{first_root, mount_detached, BodyMode, PartCx, PartId, ShellPart, Swap};
use super::ShellViewModel;

#[path = "sidebar_project.rs"]
pub(super) mod project;

use project::{SidebarSignals, SidebarView};

/// 侧栏块。第一次排成工作台之前没有内容。
pub(crate) struct SidebarPart {
    signals: SidebarSignals,
    view: Option<MountedView>,
}

impl ShellPart for SidebarPart {
    const ID: PartId = PartId::Sidebar;
    type Signals = SidebarSignals;

    fn signals(model: &ShellViewModel) -> Self::Signals {
        SidebarSignals::new(SidebarView::project(model))
    }

    fn new(signals: Self::Signals) -> Self {
        Self { signals, view: None }
    }

    fn root(&self) -> Option<StableNodeId> {
        first_root(self.view.as_ref())
    }

    /// 排法变了：第一次排成工作台时按当前投影建侧栏；之后不重建，收起时留在停放的工作区里。
    fn mount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel, mode: BodyMode) -> Result<Swap, FrameworkError> {
        self.sync(model);
        if mode == BodyMode::Workbench && self.view.is_none() {
            return self.remount(cx, model);
        }
        Ok(Swap::default())
    }

    fn sync(&mut self, model: &ShellViewModel) {
        self.signals.write(SidebarView::project(model));
    }

    /// 侧栏的结构都由信号驱动，没有要重挂才能跟上的变化。
    fn needs_remount(&self, _: &ShellViewModel) -> bool {
        false
    }

    /// 建侧栏。只在第一次排成工作台时由 [`Self::mount`] 调。
    fn remount(&mut self, cx: &mut PartCx<'_>, _: &ShellViewModel) -> Result<Swap, FrameworkError> {
        let (signals, hot) = (self.signals, cx.hot);
        self.view = mount_detached(cx.document, cx.hot, Self::ID, move || Some(super::sidebar_view::sidebar(signals, hot)))?;
        cx.stats.remounts += 1;
        Ok(Swap::default())
    }

    /// 侧栏没有要重挂的变化，组合输入不用等。
    fn composing(&self, _: &RuntimeDocument) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "view_part_sidebar_tests.rs"]
mod tests;

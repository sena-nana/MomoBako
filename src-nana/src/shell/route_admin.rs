//! 管理路由（常驻）：首页里的日志、拓展和仓库动作面板，以及它们和设置页共用的信号。
//!
//! 三个面板各是一条路由，分支只在进入时建一次，之后同步只写 [`AdminSignals`] 里当前面板的那份。
//! 首页外框上方是常驻筛选栏，主体是纵向滚动：日志追踪时滚动跟随末尾。设置页和拓展页都有插件
//! 管理面板，共用同一份插件信号。

use nana_ui::runtime::view::{AnyView, Signal};

use super::admin::{
    ActionsSignals, ActionsView, LogsSignals, LogsView, PluginPanelSignals, PluginPanelView, SettingsSignals, SettingsView, ToolsSignals,
};
use super::inspect_search_view::{resident_filter_bar, FilterBarSignals};
use super::route_search::{home_page, home_scroll};
use super::ShellViewModel;

/// 设置、日志、拓展和动作页的常驻信号，在主区块的骨架作用域里建，进出路由都不重建。
#[derive(Clone, Copy)]
pub(crate) struct AdminSignals {
    pub(super) settings: SettingsSignals,
    pub(super) plugins: PluginPanelSignals,
    logs: LogsSignals,
    tools: ToolsSignals,
    actions: ActionsSignals,
}

impl AdminSignals {
    /// 空的信号，进路由之前由同步写入第一份投影。
    pub(crate) fn new() -> Self {
        Self {
            settings: SettingsSignals::new(),
            plugins: PluginPanelSignals::new(),
            logs: LogsSignals::new(),
            tools: ToolsSignals::new(),
            actions: ActionsSignals::new(),
        }
    }

    /// 设置页：卡片和插件管理面板。
    pub(crate) fn write_settings(&self, model: &ShellViewModel) {
        self.settings.write(SettingsView::project(model));
        self.plugins.write(PluginPanelView::project(model));
    }

    /// 日志面板。
    pub(crate) fn write_logs(&self, model: &ShellViewModel) {
        self.logs.write(LogsView::project(model));
    }

    /// 拓展页：工具区和插件管理面板。
    pub(crate) fn write_extensions(&self, model: &ShellViewModel) {
        self.tools.write(model);
        self.plugins.write(PluginPanelView::project(model));
    }

    /// 仓库动作面板。
    pub(crate) fn write_actions(&self, model: &ShellViewModel) {
        self.actions.write(ActionsView::project(model));
    }
}

/// 日志面板：追踪时主区滚动跟随末尾。
pub(super) fn logs(filter: FilterBarSignals, signals: AdminSignals) -> AnyView {
    let follow: Signal<bool> = signals.logs.follow_end;
    panel(filter, "workspace-page-scroll-HasRepository-Logs", follow, super::admin::logs_panel(signals.logs))
}

/// 拓展页。
pub(super) fn extensions(filter: FilterBarSignals, signals: AdminSignals) -> AnyView {
    panel(filter, "workspace-page-scroll-HasRepository-Extensions", false, super::admin::extensions_page(signals.tools, signals.plugins))
}

/// 仓库动作面板。
pub(super) fn actions(filter: FilterBarSignals, signals: AdminSignals) -> AnyView {
    panel(filter, "workspace-page-scroll-HasRepository-Actions", false, super::admin::actions_panel(signals.actions))
}

/// 首页外框：常驻筛选栏在上，下面是纵向滚动的面板。
fn panel(filter: FilterBarSignals, key: &'static str, follow_end: impl nana_ui::runtime::view::IntoProp<bool>, body: AnyView) -> AnyView {
    let page = super::workbench::page(vec![body]);
    home_page(Some(resident_filter_bar(filter)), home_scroll(key, follow_end, page))
}

#[cfg(test)]
#[path = "route_admin_tests.rs"]
mod tests;

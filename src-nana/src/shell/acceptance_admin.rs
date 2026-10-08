//! 设置、插件与日志的对照场景：设置页、插件管理、来源登录、拓展工具页、日志和任务。
//!
//! 场景名和 `tmp/vue-mock` 的 Vue 场景同名，数据和 Vue 夹具保持一致。

use super::super::ShellViewModel;

/// 本面板的离屏对照场景。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    Vec::new()
}

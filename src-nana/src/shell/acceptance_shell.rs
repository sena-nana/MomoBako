//! 壳层与侧栏的对照场景：标题栏、侧栏、仓库切换、启动、空库和缺失仓库。
//!
//! 场景名和 `tmp/vue-mock` 的 Vue 场景同名，数据和 Vue 夹具保持一致。

use super::super::ShellViewModel;

/// 本面板的离屏对照场景。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    Vec::new()
}

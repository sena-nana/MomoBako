//! 预览与播放的对照场景：预览页、底部播放条、播放列表页和当前队列。
//!
//! 场景名和 `tmp/vue-mock` 的 Vue 场景同名，数据和 Vue 夹具保持一致。

use super::super::ShellViewModel;

/// 本面板的离屏对照场景。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    Vec::new()
}

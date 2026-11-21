//! 菜单的键盘行为。
//!
//! 弹层和菜单都是 NanaUI 原生控件：「导入」是 `ActionMenu`，标签菜单、任务弹层和仓库弹层是 `Popover`，
//! 文件和文件夹的右键菜单是 `AnchoredActionMenu`。打开、方向键、Escape 和关上后焦点归还由框架负责；
//! 这里只剩仓库弹层各页里菜单项共用的方向键焦点组，以及从键盘走一遍这些菜单的测试。

use nana_ui::runtime::RovingFocusGroup;

/// 菜单项所在的方向键焦点组：竖排，越过两端回到另一端（WAI-ARIA 菜单）。
pub(crate) const MENU_KEYS: RovingFocusGroup = RovingFocusGroup::vertical().wrap(true);

#[cfg(test)]
mod tests;

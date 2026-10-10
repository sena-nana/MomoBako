//! 各页常驻视图共用的字段写入器：控件表里没有、又不止一处页面要绑的字段。
//!
//! NanaUI 生成的 `Button` 字段只有文字、读屏名、禁用、加载和种类，按钮前的图标要自己写入器。
//! 管理页（日志的暂停、动作的执行、来源登录的检查）和文件页（继续加载）都绑它，这里只留一份。

use nana_ui::runtime::view::FieldWrite;
use nana_ui::runtime::Button;
use nana_ui_core::Icon;

/// 按钮文字前的图标：暂停和恢复、执行中的转圈、加载更多。图标的相等就是几何指针相同。
pub(crate) struct ButtonIcon;

impl FieldWrite<Button, Option<Icon>> for ButtonIcon {
    const FIELD: &'static str = "Button.icon";

    fn write(target: &mut Button, icon: Option<Icon>) {
        target.icon = icon;
    }

    fn differs(target: &Button, icon: &Option<Icon>) -> bool {
        target.icon != *icon
    }
}

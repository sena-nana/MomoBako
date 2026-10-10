//! 各页常驻视图共用的字段写入器：控件表里没有、又不止一处页面要绑的字段，每样只留一份。
//!
//! - [`ButtonIcon`]：NanaUI 生成的 `Button` 字段只有文字、读屏名、禁用、加载和种类，按钮前的图标要自己写入器。
//!   管理页（日志的暂停、动作的执行、来源登录的检查）和文件页（继续加载）都绑它。
//! - [`StyleField`]：整份节点样式，外观随状态整套换的地方（管理页的分段按钮和状态胶囊，文件页的卡片放置态、
//!   展开排法，检视面的评分星和预览框）。
//! - [`GlyphIcon`]：独立图标节点的图标，侧栏文件夹的开合和元数据标签组的展开。

use nana_ui::runtime::view::{FieldWrite, StyledComponent};
use nana_ui::runtime::{Button, IconGlyph, NodeStyle};
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

/// 整份节点样式。绑定闭包按投影现算样式（和整块重挂时用同一个构造函数），值不同才写。
///
/// `layout.hidden` 归 `.visible` 管：同一个节点不要同时绑 `.visible` 和整份样式，整份写会把显隐冲掉；
/// 要两样都有时外面包一层。
pub(crate) struct StyleField;

impl<C: StyledComponent> FieldWrite<C, NodeStyle> for StyleField {
    const FIELD: &'static str = "style";

    fn write(target: &mut C, style: NodeStyle) {
        *target.node_style_mut() = style;
    }

    fn differs(target: &C, style: &NodeStyle) -> bool {
        target.node_style() != style
    }
}

/// 独立图标节点的图标，随状态换图标的地方用。图标的相等就是几何指针相同。
pub(crate) struct GlyphIcon;

impl FieldWrite<IconGlyph, Icon> for GlyphIcon {
    const FIELD: &'static str = "IconGlyph.icon";

    fn write(target: &mut IconGlyph, icon: Icon) {
        target.icon = icon;
    }

    fn differs(target: &IconGlyph, icon: &Icon) -> bool {
        target.icon != *icon
    }
}

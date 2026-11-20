//! 浮层外的透明点击层：弹层、菜单打开时铺在它下面，在浮层外按下就收起。
//!
//! 资源库弹层、任务弹层、导入菜单、标签菜单和右键菜单都用这一层。它是一行没有内容的 `ListItem`，
//! 按下（`Activate`）由调用处发收起消息；浮层本身画在它上面，点浮层不会打到这一层。

use std::sync::Arc;

use nana_ui::runtime::view::{El, widget};
use nana_ui::runtime::{LengthSpec, ListItem, NodeStyle, Stack};
use nana_ui_core::PositionSpec;

/// 这一层铺满什么。
#[derive(Clone, Copy, Debug)]
pub(crate) enum DismissCover {
    /// 铺满包含块：浮层层或弹层所在的那一格已经铺满窗口时用。
    Container,
    /// 固定定位、铺满窗口，排在 `z_index` 这一层。菜单挂在页面里、包含块不是整窗时用。
    Window { z_index: i32 },
}

/// 透明点击层。`label` 是读屏念的名字（「关闭资源库弹层」），调用处再接 `.key`、`.visible` 和 `.on_cx`。
pub(crate) fn dismiss_layer(label: &'static str, cover: DismissCover) -> El<ListItem> {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    match cover {
        DismissCover::Container => layout.position = PositionSpec::Absolute,
        DismissCover::Window { z_index } => {
            layout.position = PositionSpec::Fixed;
            layout.z_index = Some(z_index);
        }
    }
    layout.offset_top = Some(LengthSpec::Px(0.0));
    layout.offset_left = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Percent(100.0));
    layout.height = Some(LengthSpec::Percent(100.0));
    widget(ListItem::new(label).style(style)).content(widget(Stack::column(0.0)))
}

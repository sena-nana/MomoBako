//! 设置路由（常驻）：整页滚动的设置页。
//!
//! 分支只在进入设置页时建一次，之后同步只写设置卡片和插件管理面板的信号（见 `route_admin.rs` 的
//! [`AdminSignals`](super::route_admin::AdminSignals)）。滚动容器常驻，滚动位置自然留着。

use nana_ui::runtime::view::AnyView;

use super::route_admin::AdminSignals;

/// 设置页只有一张整页，滚动容器用固定键。
pub(super) fn view(signals: AdminSignals) -> AnyView {
    super::route_home::scroll_route(super::admin::settings_page(signals.settings, signals.plugins), "settings-scroll")
}

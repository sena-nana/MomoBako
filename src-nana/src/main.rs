// 发布版（release、dist）是 Windows 图形界面程序，不带控制台窗口；开发构建保留控制台，看得到标准错误。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> Result<(), nana_ui::HostedRunError> {
    momobako_nana::run()
}

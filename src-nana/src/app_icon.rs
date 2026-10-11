//! MomoBako 的应用图标。
//!
//! 图标源在 `src-nana/icons`（由 `yarn icons:generate` 从 `icon.svg` 生成）。`build.rs` 把 `icon.ico` 写进可执行文件，
//! 资源管理器和快捷方式用它；这里给窗口、任务栏、托盘和拖出文件时的拖动图像用 PNG。没有登记窗口图标时 NanaUI 会用
//! 自己的默认标志，所以主窗口描述符要带上它。

use nana_ui_platform::{WindowDescriptor, WindowIcon};

/// 32×32 的图标，托盘和拖出文件时的拖动图像用（都只在 Windows 上有）。
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const SMALL_PNG: &[u8] = include_bytes!("../icons/32x32.png");

/// 256×256 的图标，窗口和任务栏用。
const WINDOW_PNG: &[u8] = include_bytes!("../icons/128x128@2x.png");

/// 给主窗口描述符带上 MomoBako 图标。图标解不开时记下原因，窗口照常用 NanaUI 的默认标志。
pub fn with_window_icon(descriptor: WindowDescriptor) -> WindowDescriptor {
    match window_icon() {
        Ok(icon) => descriptor.icon(icon),
        Err(error) => {
            eprintln!("Nana 窗口图标没有用上：{error}");
            descriptor
        }
    }
}

fn window_icon() -> Result<WindowIcon, String> {
    let image = image::load_from_memory(WINDOW_PNG).map_err(|error| format!("图标解码失败：{error}"))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    WindowIcon::from_rgba(rgba.into_raw(), width, height).map_err(|error| format!("图标像素无效：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_gets_the_momobako_icon() {
        let icon = window_icon().expect("图标能解开");
        assert_eq!((icon.width, icon.height), (256, 256));
        let descriptor = with_window_icon(WindowDescriptor::new("MomoBako"));
        assert_eq!(descriptor.icon.as_ref().map(|icon| (icon.width, icon.height)), Some((256, 256)));
    }

    #[test]
    fn the_small_icon_is_32_pixels() {
        let image = image::load_from_memory(SMALL_PNG).expect("小图标能解开");
        assert_eq!((image.width(), image.height()), (32, 32));
    }
}

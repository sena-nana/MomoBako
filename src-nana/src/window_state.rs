//! 主窗口描述符和几何持久化。
//!
//! 位置、尺寸和最大化交给 NanaUI 的窗口几何持久化：描述符带 `persist_key`，宿主建窗前就套用上次的几何
//! （不会先开默认尺寸再跳过去），移动、缩放和最大化切换后写回，最小化和全屏的那一帧不记，最大化时保留
//! 上一次的正常几何。存储是应用数据目录（`app_paths.rs` 的 `window_state_dir`）下 NanaUI 的文件存储；
//! 没有数据目录或打不开时只记在内存里，下次启动用默认尺寸。
//!
//! 旧版本自己写的 `nana-main-window-state.json` 在第一次启动时搬进 NanaUI 的存储，然后删掉。

use std::path::Path;

use nana_ui::{FileStore, PersistedWindowGeometry, SharedStore, ViewStateStore, shared_store};
use nana_ui_platform::WindowDescriptor;

/// 主窗口最小宽。
pub const MIN_MAIN_WINDOW_WIDTH: u32 = 960;
/// 主窗口最小高。
pub const MIN_MAIN_WINDOW_HEIGHT: u32 = 600;

/// 主窗口几何在 NanaUI 存储里的键。
const MAIN_WINDOW_KEY: &str = "main";
/// 旧版本自写的主窗口状态文件。
const LEGACY_STATE_FILE: &str = "nana-main-window-state.json";

/// 生产窗口：初始 1200×800，最小 960×600，几何按 [`MAIN_WINDOW_KEY`] 持久化。
pub fn main_window_descriptor() -> WindowDescriptor {
    WindowDescriptor::new("MomoBako")
        .initial_size(1200.0, 800.0)
        .minimum_size(
            f64::from(MIN_MAIN_WINDOW_WIDTH),
            f64::from(MIN_MAIN_WINDOW_HEIGHT),
        )
        .persist_key(MAIN_WINDOW_KEY)
}

/// 窗口几何用的持久存储。没有应用数据目录或打不开时返回 `None`，宿主退回内存存储。
pub fn store(descriptor: &WindowDescriptor) -> Option<SharedStore> {
    let dir = crate::app_paths::current().window_state_dir.clone()?;
    let store = match FileStore::open(&dir) {
        Ok(store) => shared_store(store),
        Err(error) => {
            eprintln!("Nana 打不开窗口状态存储 {}：{error}", dir.display());
            return None;
        }
    };
    migrate_legacy(&dir, &store, descriptor);
    Some(store)
}

/// 把旧版本的状态文件搬进 NanaUI 存储：存储里还没有主窗口几何时才写，搬完删掉旧文件。
/// 读不出、尺寸小于最小值的旧记录直接丢掉。
fn migrate_legacy(dir: &Path, store: &SharedStore, descriptor: &WindowDescriptor) {
    let legacy = dir.join(LEGACY_STATE_FILE);
    let Ok(text) = std::fs::read_to_string(&legacy) else {
        return;
    };
    let view_state = ViewStateStore::new(store.clone(), descriptor.restoration_scope.clone());
    let existing = PersistedWindowGeometry::load(&view_state, MAIN_WINDOW_KEY);
    if matches!(existing, Ok(None))
        && let Some(geometry) = decode_legacy(&text)
        && let Err(error) = geometry.save(&view_state, MAIN_WINDOW_KEY)
    {
        eprintln!("Nana 旧窗口状态没有搬进存储：{error}");
        return;
    }
    if let Err(error) = std::fs::remove_file(&legacy) {
        eprintln!("Nana 删不掉旧窗口状态文件 {}：{error}", legacy.display());
    }
}

/// 旧文件是 `{ x, y, width, height, maximized }`，坐标和尺寸是逻辑像素。
fn decode_legacy(text: &str) -> Option<PersistedWindowGeometry> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let number = |key: &str| value.get(key).and_then(serde_json::Value::as_f64);
    let (width, height) = (number("width")?, number("height")?);
    if width < f64::from(MIN_MAIN_WINDOW_WIDTH) || height < f64::from(MIN_MAIN_WINDOW_HEIGHT) {
        return None;
    }
    Some(PersistedWindowGeometry {
        x: number("x")?,
        y: number("y")?,
        width,
        height,
        maximized: value
            .get("maximized")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        ..PersistedWindowGeometry::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_keeps_the_minimum_and_persists_under_the_main_key() {
        let descriptor = main_window_descriptor();
        assert_eq!(descriptor.minimum_size, (960.0, 600.0));
        assert_eq!(descriptor.persist_key.as_deref(), Some(MAIN_WINDOW_KEY));
    }

    #[test]
    fn legacy_state_moves_into_the_store_once() {
        let dir = std::env::temp_dir().join(format!(
            "momobako-window-state-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("临时目录");
        std::fs::write(
            dir.join(LEGACY_STATE_FILE),
            r#"{"x":40,"y":30,"width":1300,"height":820,"maximized":true}"#,
        )
        .expect("旧文件");
        let descriptor = main_window_descriptor();
        let store = shared_store(FileStore::open(&dir).expect("存储"));

        migrate_legacy(&dir, &store, &descriptor);

        assert!(!dir.join(LEGACY_STATE_FILE).exists(), "搬完删掉旧文件");
        let view_state = ViewStateStore::new(store, descriptor.restoration_scope.clone());
        let geometry = PersistedWindowGeometry::load(&view_state, MAIN_WINDOW_KEY)
            .expect("读存储")
            .expect("搬进来的几何");
        assert_eq!(
            (geometry.x, geometry.y, geometry.width, geometry.height),
            (40.0, 30.0, 1300.0, 820.0)
        );
        assert!(geometry.maximized);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_state_smaller_than_the_minimum_is_dropped() {
        assert!(decode_legacy(r#"{"x":0,"y":0,"width":800,"height":600}"#).is_none());
        assert!(decode_legacy("not json").is_none());
    }
}

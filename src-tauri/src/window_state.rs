use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, PhysicalPosition, PhysicalSize, WebviewWindow};
use tauri_plugin_store::StoreExt;

pub const MAIN_WINDOW_STATE_STORE_FILE: &str = "main-window-state.json";
pub const MAIN_WINDOW_STATE_KEY: &str = "mainWindow";
pub const MIN_MAIN_WINDOW_WIDTH: u32 = 960;
pub const MIN_MAIN_WINDOW_HEIGHT: u32 = 600;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MainWindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainWindowSnapshot {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

#[derive(Default)]
pub struct MainWindowStateCache {
    data: Mutex<MainWindowStateCacheData>,
}

#[derive(Default)]
struct MainWindowStateCacheData {
    latest_snapshot: Option<MainWindowSnapshot>,
    latest_normal_state: Option<MainWindowState>,
}

impl MainWindowStateCache {
    fn record(&self, snapshot: MainWindowSnapshot) {
        if let Ok(mut data) = self.data.lock() {
            data.latest_snapshot = Some(snapshot);
            if !snapshot.maximized {
                let state = snapshot.into_state();
                if is_restorable_main_window_state(&state) {
                    data.latest_normal_state = Some(state);
                }
            }
        }
    }

    fn latest(&self) -> Option<MainWindowSnapshot> {
        self.data.lock().ok().and_then(|data| data.latest_snapshot)
    }

    fn latest_normal_state(&self) -> Option<MainWindowState> {
        self.data
            .lock()
            .ok()
            .and_then(|data| data.latest_normal_state)
    }
}

impl MainWindowSnapshot {
    fn into_state(self) -> MainWindowState {
        MainWindowState {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            maximized: self.maximized,
        }
    }
}

pub fn is_restorable_main_window_state(state: &MainWindowState) -> bool {
    state.width >= MIN_MAIN_WINDOW_WIDTH && state.height >= MIN_MAIN_WINDOW_HEIGHT
}

pub fn merge_main_window_state(
    previous: Option<MainWindowState>,
    snapshot: MainWindowSnapshot,
) -> MainWindowState {
    if snapshot.maximized {
        if let Some(previous) = previous.filter(is_restorable_main_window_state) {
            return MainWindowState {
                maximized: true,
                ..previous
            };
        }
    }
    snapshot.into_state()
}

pub fn load_main_window_state(app: &AppHandle) -> Option<MainWindowState> {
    let store = app.store(MAIN_WINDOW_STATE_STORE_FILE).ok()?;
    let value = store.get(MAIN_WINDOW_STATE_KEY)?;
    serde_json::from_value::<MainWindowState>(value)
        .ok()
        .filter(is_restorable_main_window_state)
}

fn save_main_window_state(
    app: &AppHandle,
    snapshot: MainWindowSnapshot,
    session_previous: Option<MainWindowState>,
) -> Result<(), String> {
    let store = app
        .store(MAIN_WINDOW_STATE_STORE_FILE)
        .map_err(|error| format!("failed to open window state store: {error}"))?;
    let previous = session_previous.or_else(|| {
        store
            .get(MAIN_WINDOW_STATE_KEY)
            .and_then(|value| serde_json::from_value::<MainWindowState>(value).ok())
    });
    let state = merge_main_window_state(previous, snapshot);
    let value = serde_json::to_value(state).map_err(|error| error.to_string())?;
    store.set(MAIN_WINDOW_STATE_KEY, value);
    store
        .save()
        .map_err(|error| format!("failed to save window state: {error}"))
}

pub fn capture_main_window_snapshot(window: &WebviewWindow) -> Option<MainWindowSnapshot> {
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    let maximized = window.is_maximized().unwrap_or(false);
    Some(MainWindowSnapshot {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized,
    })
}

pub fn restore_main_window_state(window: &WebviewWindow, state: MainWindowState) {
    let _ = window.set_position(PhysicalPosition::new(state.x, state.y));
    let _ = window.set_size(PhysicalSize::new(state.width, state.height));
    if state.maximized {
        let _ = window.maximize();
    }
}

pub fn remember_main_window_state(cache: &MainWindowStateCache, window: &WebviewWindow) {
    if let Some(snapshot) = capture_main_window_snapshot(window) {
        cache.record(snapshot);
    }
}

pub fn persist_main_window_snapshot(
    app: &AppHandle,
    cache: &MainWindowStateCache,
    snapshot: MainWindowSnapshot,
) {
    if let Err(error) = save_main_window_state(app, snapshot, cache.latest_normal_state()) {
        crate::app_log!(
            "warn",
            "window.state",
            "persistFailed",
            "保存主窗口状态失败。",
            serde_json::json!({ "error": error })
        );
    }
}

pub fn persist_cached_main_window_state(app: &AppHandle, cache: &MainWindowStateCache) {
    if let Some(snapshot) = cache.latest() {
        persist_main_window_snapshot(app, cache, snapshot);
    }
}

pub fn persist_main_window_state(
    app: &AppHandle,
    cache: &MainWindowStateCache,
    window: &WebviewWindow,
) {
    let Some(snapshot) = capture_main_window_snapshot(window) else {
        persist_cached_main_window_state(app, cache);
        return;
    };
    cache.record(snapshot);
    persist_main_window_snapshot(app, cache, snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximized_snapshot_keeps_last_normal_geometry() {
        let previous = MainWindowState {
            x: 120,
            y: 80,
            width: 1180,
            height: 760,
            maximized: false,
        };
        let maximized_snapshot = MainWindowSnapshot {
            x: -8,
            y: -8,
            width: 1936,
            height: 1056,
            maximized: true,
        };

        let merged = merge_main_window_state(Some(previous), maximized_snapshot);

        assert_eq!(
            merged,
            MainWindowState {
                maximized: true,
                ..previous
            }
        );
    }

    #[test]
    fn normal_snapshot_replaces_previous_geometry() {
        let previous = MainWindowState {
            x: 120,
            y: 80,
            width: 1180,
            height: 760,
            maximized: true,
        };
        let normal_snapshot = MainWindowSnapshot {
            x: 320,
            y: 180,
            width: 1320,
            height: 860,
            maximized: false,
        };

        let merged = merge_main_window_state(Some(previous), normal_snapshot);

        assert_eq!(merged, normal_snapshot.into_state());
    }

    #[test]
    fn maximized_snapshot_without_restorable_previous_uses_current_snapshot() {
        let previous = MainWindowState {
            x: 120,
            y: 80,
            width: 640,
            height: 480,
            maximized: false,
        };
        let maximized_snapshot = MainWindowSnapshot {
            x: -8,
            y: -8,
            width: 1936,
            height: 1056,
            maximized: true,
        };

        let merged = merge_main_window_state(Some(previous), maximized_snapshot);

        assert_eq!(merged, maximized_snapshot.into_state());
    }

    #[test]
    fn rejects_state_smaller_than_main_window_minimum() {
        let too_narrow = MainWindowState {
            x: 120,
            y: 80,
            width: MIN_MAIN_WINDOW_WIDTH - 1,
            height: MIN_MAIN_WINDOW_HEIGHT,
            maximized: false,
        };
        let too_short = MainWindowState {
            height: MIN_MAIN_WINDOW_HEIGHT - 1,
            width: MIN_MAIN_WINDOW_WIDTH,
            ..too_narrow
        };
        let restorable = MainWindowState {
            width: MIN_MAIN_WINDOW_WIDTH,
            height: MIN_MAIN_WINDOW_HEIGHT,
            ..too_narrow
        };

        assert!(!is_restorable_main_window_state(&too_narrow));
        assert!(!is_restorable_main_window_state(&too_short));
        assert!(is_restorable_main_window_state(&restorable));
    }

    #[test]
    fn cache_keeps_latest_normal_geometry_after_maximized_snapshot() {
        let cache = MainWindowStateCache::default();
        let normal_snapshot = MainWindowSnapshot {
            x: 320,
            y: 180,
            width: 1320,
            height: 860,
            maximized: false,
        };
        let maximized_snapshot = MainWindowSnapshot {
            x: -8,
            y: -8,
            width: 1936,
            height: 1056,
            maximized: true,
        };

        cache.record(normal_snapshot);
        cache.record(maximized_snapshot);

        assert_eq!(cache.latest(), Some(maximized_snapshot));
        assert_eq!(
            cache.latest_normal_state(),
            Some(normal_snapshot.into_state())
        );
        assert_eq!(
            merge_main_window_state(cache.latest_normal_state(), maximized_snapshot),
            MainWindowState {
                maximized: true,
                ..normal_snapshot.into_state()
            }
        );
    }

    #[test]
    fn main_window_starts_hidden_until_restored() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let main_window = config["app"]["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|window| window["label"].as_str() == Some("main"))
            .unwrap();

        assert_eq!(main_window["visible"].as_bool(), Some(false));
    }
}

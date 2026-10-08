//! 主窗口位置、尺寸和最大化。
//!
//! 规则与 `src-tauri/src/window_state.rs` 相同：宽小于 960 或高小于 600 不恢复；
//! 最大化时保留上一帧正常几何。坐标用 Nana 的逻辑像素，因为窗口命令按逻辑像素下发。
//! 文件是 `%APPDATA%/com.momobako.desktop/nana-main-window-state.json`。
//! 离屏测试只覆盖合并和读写，不移动真窗口。

use std::path::{Path, PathBuf};

use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{WindowDescriptor, WindowGeometry, WindowId};

/// 与 Tauri 主窗口最小尺寸一致。
pub const MIN_MAIN_WINDOW_WIDTH: u32 = 960;
/// 与 Tauri 主窗口最小尺寸一致。
pub const MIN_MAIN_WINDOW_HEIGHT: u32 = 600;

const STATE_FILE: &str = "nana-main-window-state.json";
const ICONIC_ORIGIN: f32 = -16_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Default)]
struct CacheData {
    latest_snapshot: Option<MainWindowSnapshot>,
    latest_normal_state: Option<MainWindowState>,
}

#[derive(Debug, Default)]
struct Cache {
    data: CacheData,
}

impl Cache {
    fn record(&mut self, snapshot: MainWindowSnapshot) {
        self.data.latest_snapshot = Some(snapshot);
        if !snapshot.maximized {
            let state = snapshot.into_state();
            if is_restorable_main_window_state(&state) {
                self.data.latest_normal_state = Some(state);
            }
        }
    }

    fn latest(&self) -> Option<MainWindowSnapshot> {
        self.data.latest_snapshot
    }

    fn latest_normal_state(&self) -> Option<MainWindowState> {
        self.data.latest_normal_state
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

/// 生产窗口的初始尺寸和最小尺寸。几何持久化由本模块负责，不用 Nana 的 `persist_key`。
pub fn main_window_descriptor() -> WindowDescriptor {
    WindowDescriptor::new("MomoBako")
        .initial_size(1200.0, 800.0)
        .minimum_size(f64::from(MIN_MAIN_WINDOW_WIDTH), f64::from(MIN_MAIN_WINDOW_HEIGHT))
}

/// 小于主窗口最小值的状态不能恢复。
pub fn is_restorable_main_window_state(state: &MainWindowState) -> bool {
    state.width >= MIN_MAIN_WINDOW_WIDTH && state.height >= MIN_MAIN_WINDOW_HEIGHT
}

/// 最大化快照沿用上一帧可恢复的正常几何，只把最大化标记打开。
pub fn merge_main_window_state(previous: Option<MainWindowState>, snapshot: MainWindowSnapshot) -> MainWindowState {
    if snapshot.maximized
        && let Some(previous) = previous.filter(is_restorable_main_window_state)
    {
        return MainWindowState { maximized: true, ..previous };
    }
    snapshot.into_state()
}

/// 一次进程里的窗口几何。`path` 为空时写应用数据目录。
#[derive(Debug, Default)]
pub struct Session {
    cache: Cache,
    armed: bool,
    pending: Option<MainWindowState>,
    path: Option<PathBuf>,
}

impl Session {
    /// 测试把状态写到指定文件，避免碰到用户的应用数据目录。
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path: Some(path), ..Self::default() }
    }

    /// 启动时读回可恢复的状态，并给出窗口命令。读失败只记日志，窗口保持描述符里的默认尺寸。
    pub fn on_ready(&mut self, id: WindowId) -> Vec<WindowCommand> {
        let state = match load_main_window_state(self.path.as_deref()) {
            Ok(state) => state,
            Err(error) => {
                eprintln!("Nana 读取主窗口状态失败：{error}");
                None
            }
        };
        self.arm(id, state)
    }

    /// 记下已经读出的状态。随后到来的几何要对上这帧，才覆盖磁盘上的记录。
    pub fn arm(&mut self, id: WindowId, state: Option<MainWindowState>) -> Vec<WindowCommand> {
        self.armed = true;
        let Some(state) = state else {
            return Vec::new();
        };
        if !is_restorable_main_window_state(&state) {
            eprintln!("Nana 主窗口状态小于最小尺寸，不恢复");
            return Vec::new();
        }
        self.seed(state);
        self.pending = Some(state);
        restore_commands(id, state)
    }

    /// 移动或缩放后写入。最大化时不把铺满屏幕的那一帧当成下次的正常尺寸。
    pub fn on_geometry(&mut self, geometry: &WindowGeometry) {
        if !self.armed {
            return;
        }
        let Some(snapshot) = capture(geometry) else {
            return;
        };
        if let Some(pending) = self.pending {
            if !matches_pending(snapshot, pending) {
                return;
            }
            self.pending = None;
        }
        self.cache.record(snapshot);
        self.persist();
    }

    /// 关闭前再写一次缓存。没有新快照时不动文件。
    pub fn persist(&self) {
        let Some(snapshot) = self.cache.latest() else {
            return;
        };
        let state = merge_main_window_state(self.cache.latest_normal_state(), snapshot);
        let path = match self.path.clone().or_else(state_path) {
            Some(path) => path,
            None => {
                eprintln!("Nana 没有应用数据目录，主窗口状态没有写入");
                return;
            }
        };
        if let Err(error) = save_to(&path, &state) {
            eprintln!("Nana 保存主窗口状态失败：{error}");
        }
    }

    fn seed(&mut self, state: MainWindowState) {
        let normal = MainWindowSnapshot {
            x: state.x,
            y: state.y,
            width: state.width,
            height: state.height,
            maximized: false,
        };
        self.cache.record(normal);
        if state.maximized {
            self.cache.record(MainWindowSnapshot { maximized: true, ..normal });
        }
    }
}

/// 先放回正常位置和尺寸，再按标记最大化。这样取消最大化会回到原来的窗口，而不是铺满屏幕的那一帧。
pub fn restore_commands(id: WindowId, state: MainWindowState) -> Vec<WindowCommand> {
    let mut commands = vec![WindowCommand::SetBounds {
        id,
        position: (state.x as f32, state.y as f32),
        size: (state.width as f32, state.height as f32),
    }];
    if state.maximized {
        commands.push(WindowCommand::SetMaximized { id, maximized: true });
    }
    commands
}

fn matches_pending(snapshot: MainWindowSnapshot, pending: MainWindowState) -> bool {
    if pending.maximized && snapshot.maximized {
        return true;
    }
    snapshot.width == pending.width
        && snapshot.height == pending.height
        && (snapshot.x - pending.x).abs() <= 2
        && (snapshot.y - pending.y).abs() <= 2
}

fn capture(geometry: &WindowGeometry) -> Option<MainWindowSnapshot> {
    let (x, y) = geometry.logical_position?;
    if !x.is_finite() || !y.is_finite() || x <= ICONIC_ORIGIN || y <= ICONIC_ORIGIN {
        return None;
    }
    let (width, height) = geometry.logical_size;
    if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
        return None;
    }
    Some(MainWindowSnapshot {
        x: x.round() as i32,
        y: y.round() as i32,
        width: width.round().clamp(1.0, u32::MAX as f32) as u32,
        height: height.round().clamp(1.0, u32::MAX as f32) as u32,
        maximized: geometry.maximized,
    })
}

fn state_path() -> Option<PathBuf> {
    nana_ui_platform::app_data_dir("com.momobako.desktop").map(|dir| dir.join(STATE_FILE))
}

fn load_main_window_state(path: Option<&Path>) -> Result<Option<MainWindowState>, String> {
    let Some(path) = path.map(Path::to_path_buf).or_else(state_path) else {
        return Err("没有应用数据目录".into());
    };
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let Some(state) = decode(&text) else {
        return Err(format!("主窗口状态无法解析：{}", path.display()));
    };
    if !is_restorable_main_window_state(&state) {
        eprintln!("Nana 主窗口状态小于最小尺寸，不恢复");
        return Ok(None);
    }
    Ok(Some(state))
}

fn save_to(path: &Path, state: &MainWindowState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let payload = encode(state);
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, payload.as_bytes()).map_err(|error| error.to_string())?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn encode(state: &MainWindowState) -> String {
    serde_json::json!({
        "x": state.x,
        "y": state.y,
        "width": state.width,
        "height": state.height,
        "maximized": state.maximized,
    })
    .to_string()
}

fn decode(text: &str) -> Option<MainWindowState> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(MainWindowState {
        x: i32::try_from(value.get("x")?.as_i64()?).ok()?,
        y: i32::try_from(value.get("y")?.as_i64()?).ok()?,
        width: u32::try_from(value.get("width")?.as_u64()?).ok()?,
        height: u32::try_from(value.get("height")?.as_u64()?).ok()?,
        maximized: value.get("maximized")?.as_bool()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(x: f32, y: f32, width: f32, height: f32, maximized: bool) -> WindowGeometry {
        WindowGeometry {
            physical_position: Some((x as i32, y as i32)),
            physical_size: (width as u32, height as u32),
            logical_position: Some((x, y)),
            logical_size: (width, height),
            scale_factor: 1.0,
            maximized,
        }
    }

    #[test]
    fn maximized_snapshot_keeps_last_normal_geometry() {
        let previous = MainWindowState { x: 120, y: 80, width: 1180, height: 760, maximized: false };
        let maximized_snapshot = MainWindowSnapshot { x: -8, y: -8, width: 1936, height: 1056, maximized: true };
        assert_eq!(
            merge_main_window_state(Some(previous), maximized_snapshot),
            MainWindowState { maximized: true, ..previous }
        );
    }

    #[test]
    fn normal_snapshot_replaces_previous_geometry() {
        let previous = MainWindowState { x: 120, y: 80, width: 1180, height: 760, maximized: true };
        let normal_snapshot = MainWindowSnapshot { x: 320, y: 180, width: 1320, height: 860, maximized: false };
        assert_eq!(merge_main_window_state(Some(previous), normal_snapshot), normal_snapshot.into_state());
    }

    #[test]
    fn maximized_snapshot_without_restorable_previous_uses_current_snapshot() {
        let previous = MainWindowState { x: 120, y: 80, width: 640, height: 480, maximized: false };
        let maximized_snapshot = MainWindowSnapshot { x: -8, y: -8, width: 1936, height: 1056, maximized: true };
        assert_eq!(merge_main_window_state(Some(previous), maximized_snapshot), maximized_snapshot.into_state());
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
        let too_short = MainWindowState { height: MIN_MAIN_WINDOW_HEIGHT - 1, width: MIN_MAIN_WINDOW_WIDTH, ..too_narrow };
        let restorable = MainWindowState { width: MIN_MAIN_WINDOW_WIDTH, height: MIN_MAIN_WINDOW_HEIGHT, ..too_narrow };
        assert!(!is_restorable_main_window_state(&too_narrow));
        assert!(!is_restorable_main_window_state(&too_short));
        assert!(is_restorable_main_window_state(&restorable));
    }

    #[test]
    fn cache_keeps_latest_normal_geometry_after_maximized_snapshot() {
        let mut cache = Cache::default();
        let normal_snapshot = MainWindowSnapshot { x: 320, y: 180, width: 1320, height: 860, maximized: false };
        let maximized_snapshot = MainWindowSnapshot { x: -8, y: -8, width: 1936, height: 1056, maximized: true };
        cache.record(normal_snapshot);
        cache.record(maximized_snapshot);
        assert_eq!(cache.latest(), Some(maximized_snapshot));
        assert_eq!(cache.latest_normal_state(), Some(normal_snapshot.into_state()));
        assert_eq!(
            merge_main_window_state(cache.latest_normal_state(), maximized_snapshot),
            MainWindowState { maximized: true, ..normal_snapshot.into_state() }
        );
    }

    #[test]
    fn ready_restores_bounds_then_maximize() {
        let mut session = Session::default();
        let state = MainWindowState { x: 40, y: 24, width: 1280, height: 800, maximized: true };
        let commands = session.arm(WindowId(7), Some(state));
        assert!(matches!(
            commands.first(),
            Some(WindowCommand::SetBounds { position: (40.0, 24.0), size: (1280.0, 800.0), .. })
        ));
        assert!(matches!(commands.get(1), Some(WindowCommand::SetMaximized { maximized: true, .. })));
        assert!(session.arm(WindowId(7), Some(MainWindowState { width: 100, height: 100, ..state })).is_empty());
    }

    #[test]
    fn geometry_waits_for_the_restored_frame_then_roundtrips() {
        let path = std::env::temp_dir().join(format!("momobako-window-{}-{}.json", std::process::id(), line!()));
        let _ = std::fs::remove_file(&path);
        let mut session = Session::at(path.clone());
        let state = MainWindowState { x: 40, y: 24, width: 1280, height: 800, maximized: false };
        assert!(session.arm(WindowId(1), Some(state)).len() == 1);
        session.on_geometry(&geometry(0.0, 0.0, 1200.0, 800.0, false));
        assert!(!path.exists());
        session.on_geometry(&geometry(40.0, 24.0, 1280.0, 800.0, false));
        let loaded = load_main_window_state(Some(&path)).expect("read").expect("state");
        assert_eq!(loaded, state);
        session.on_geometry(&geometry(-8.0, -8.0, 1936.0, 1056.0, true));
        let maximized = load_main_window_state(Some(&path)).expect("read").expect("state");
        assert_eq!(maximized, MainWindowState { maximized: true, ..state });
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn descriptor_uses_the_tauri_minimum() {
        let settings = main_window_descriptor();
        assert_eq!(settings.title, "MomoBako");
        assert_eq!(settings.initial_size, (1200.0, 800.0));
        assert_eq!(settings.minimum_size, (f64::from(MIN_MAIN_WINDOW_WIDTH), f64::from(MIN_MAIN_WINDOW_HEIGHT)));
    }

    #[test]
    fn iconic_origin_is_not_captured() {
        assert!(capture(&geometry(-16_000.0, 10.0, 1200.0, 800.0, false)).is_none());
    }
}

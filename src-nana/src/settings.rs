//! Nana 宿主应用设置的校验与原子持久化边界。

use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationSettings {
    pub theme: String,
    pub thumbnail_cache_limit_mb: u32,
    pub default_playlist_player_type_id: Option<String>,
    pub close_behavior: String,
}

/// 应用设置的宿主无关存储。写入通过临时文件和重命名提交，避免半写配置被下次启动读取。
#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<ApplicationSettings, String> {
        load(&self.path)
    }

    /// 损坏配置会被保留为 `.corrupt`，应用回退默认值并返回可展示的诊断信息。
    pub fn load_or_recover(&self) -> Result<(ApplicationSettings, Option<String>), String> {
        match self.load() {
            Ok(settings) => Ok((settings, None)),
            Err(error) if self.path.exists() => {
                let backup = self.path.with_extension("corrupt");
                fs::rename(&self.path, &backup)
                    .map_err(|rename_error| format!("设置损坏且无法备份：{rename_error}"))?;
                Ok((
                    ApplicationSettings::default(),
                    Some(format!("应用设置已恢复默认值：{error}")),
                ))
            }
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, settings: &ApplicationSettings) -> Result<(), String> {
        save(&self.path, settings)
    }
}

/// 内存里的应用设置和设置文件之间怎么同步。写文件和读文件都在任务里跑，结果回来的先后不定。
///
/// - 同一时刻只有一次保存在写；写的时候又改了，记下来，等这次写完再把最新的写一次（中间的值不写）。
///   这样文件最后一定是最新的值，两次写也不会同时改同一个临时文件。
/// - 保存的结果不回写内存：内存里的值总比这次写的新或者一样，回写只会把刚改的值改回去。
/// - 读文件的结果只在读的这段时间里没有改过、也没有保存在写时才换上，免得读到的旧内容盖掉刚改的值。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsSync {
    /// 有一次保存在写。
    saving: bool,
    /// 写的时候又改了，写完要再写一次。
    resave: bool,
    /// 在读的次数。
    loads: u32,
    /// 这几次读开始以后改过设置，或者读的时候有保存在写：读到的可能是旧内容。
    loads_stale: bool,
}

impl SettingsSync {
    /// 改了一项设置。返回要不要现在排一次保存：已经有保存在写时不排，等它写完再写最新的。
    pub fn note_change(&mut self) -> bool {
        if self.loads > 0 {
            self.loads_stale = true;
        }
        if self.saving {
            self.resave = true;
            return false;
        }
        self.saving = true;
        true
    }

    /// 一次保存写完（成功或失败）。返回要不要接着把写的时候的改动再写一次。
    pub fn note_saved(&mut self) -> bool {
        if !self.saving {
            eprintln!("Nana 应用设置的保存结果没有对应的保存，忽略");
            return false;
        }
        if std::mem::take(&mut self.resave) {
            return true;
        }
        self.saving = false;
        false
    }

    /// 开始读设置文件。
    pub fn begin_load(&mut self) {
        self.loads += 1;
        if self.saving {
            self.loads_stale = true;
        }
    }

    /// 读完一次。返回读到的内容能不能换上内存里的设置。
    pub fn finish_load(&mut self) -> bool {
        let fresh = !self.loads_stale && !self.saving;
        self.loads = self.loads.saturating_sub(1);
        if self.loads == 0 {
            self.loads_stale = false;
        }
        fresh
    }
}

/// 应用设置文件的位置：发布包在应用路径的 config 下，开发时照旧在 `%LOCALAPPDATA%\MomoBako`，见 `app_paths.rs`。
/// 侧栏、展示方式、圆角、播放和上次资源库这些界面偏好都放在它旁边。
pub fn default_path() -> PathBuf {
    crate::app_paths::current().settings_dir.join("settings.json")
}

impl Default for ApplicationSettings {
    /// 关闭默认直接退出，和 Vue/Tauri 版一致；有未保存的修改时 `decide_close` 仍会先确认。
    fn default() -> Self {
        Self {
            theme: "system".into(),
            thumbnail_cache_limit_mb: 1024,
            default_playlist_player_type_id: None,
            close_behavior: "quit".into(),
        }
    }
}

impl ApplicationSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.theme.as_str(), "light" | "dark" | "system") {
            return Err("主题必须是 light、dark 或 system".into());
        }
        if !(64..=16384).contains(&self.thumbnail_cache_limit_mb) {
            return Err("缩略图缓存上限必须在 64–16384 MB 之间".into());
        }
        if !matches!(
            self.close_behavior.as_str(),
            "confirm" | "minimizeToTray" | "quit"
        ) {
            return Err("关闭行为不受支持".into());
        }
        Ok(())
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "theme": self.theme,
            "thumbnailCacheLimitMb": self.thumbnail_cache_limit_mb,
            "defaultPlaylistPlayerTypeId": self.default_playlist_player_type_id,
            "closeBehavior": self.close_behavior,
        })
    }

    fn from_json(value: serde_json::Value) -> Self {
        let defaults = Self::default();
        Self {
            theme: value
                .get("theme")
                .and_then(|v| v.as_str())
                .unwrap_or(&defaults.theme)
                .into(),
            thumbnail_cache_limit_mb: value
                .get("thumbnailCacheLimitMb")
                .and_then(|v| v.as_u64())
                .unwrap_or(defaults.thumbnail_cache_limit_mb as u64)
                as u32,
            default_playlist_player_type_id: value
                .get("defaultPlaylistPlayerTypeId")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
            close_behavior: value
                .get("closeBehavior")
                .and_then(|v| v.as_str())
                .unwrap_or(&defaults.close_behavior)
                .into(),
        }
    }
}

pub fn load(path: &Path) -> Result<ApplicationSettings, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ApplicationSettings::default());
        }
        Err(error) => return Err(format!("读取应用设置失败：{error}")),
    };
    let value = serde_json::from_str(&raw).map_err(|error| format!("解析应用设置失败：{error}"))?;
    let settings = ApplicationSettings::from_json(value);
    settings.validate()?;
    Ok(settings)
}

pub fn save(path: &Path, settings: &ApplicationSettings) -> Result<(), String> {
    settings.validate()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败：{error}"))?;
    let temporary = temporary_path(path);
    let raw = serde_json::to_vec_pretty(&settings.json())
        .map_err(|error| format!("序列化应用设置失败：{error}"))?;
    fs::write(&temporary, raw).map_err(|error| format!("写入应用设置失败：{error}"))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("提交应用设置失败：{error}"));
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}.new",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("json")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 连着改三次：只有第一次当场写，写完再写一次最新的，之后没有改动就停。
    #[test]
    fn changes_while_saving_are_written_once_after_the_save() {
        let mut sync = SettingsSync::default();
        assert!(sync.note_change(), "没有保存在写，当场写");
        assert!(!sync.note_change(), "有保存在写，等它写完");
        assert!(!sync.note_change());
        assert!(sync.note_saved(), "写完以后把最新的再写一次");
        assert!(!sync.note_saved(), "没有新的改动，不再写");
        assert!(!sync.note_saved(), "多出来的结果忽略");
        assert!(sync.note_change(), "空闲时改了当场写");
    }

    /// 读的时候改过设置、或者有保存在写，读到的不换上；之后没有改动的读照常换上。
    #[test]
    fn loads_that_overlap_a_change_are_dropped() {
        let mut sync = SettingsSync::default();
        sync.begin_load();
        assert!(sync.finish_load(), "读的时候没改过");

        sync.begin_load();
        assert!(sync.note_change());
        assert!(!sync.finish_load(), "读的时候改过");
        sync.begin_load();
        assert!(!sync.finish_load(), "读的时候还有保存在写");
        assert!(!sync.note_saved());
        sync.begin_load();
        assert!(sync.finish_load(), "保存写完以后的读照常换上");
        assert!(sync.finish_load(), "没有在读时送来的结果照常换上");
    }

    #[test]
    fn invalid_cache_limit_is_rejected() {
        let mut settings = ApplicationSettings::default();
        settings.thumbnail_cache_limit_mb = 32;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn save_and_load_round_trip_atomically() {
        let root = std::env::temp_dir().join(format!("momobako-settings-{}", std::process::id()));
        let path = root.join("settings.json");
        let settings = ApplicationSettings {
            theme: "dark".into(),
            thumbnail_cache_limit_mb: 512,
            default_playlist_player_type_id: Some("audio.default".into()),
            close_behavior: "quit".into(),
        };
        save(&path, &settings).expect("settings should save");
        assert_eq!(load(&path).expect("settings should load"), settings);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn corrupted_settings_are_backed_up_and_recovered() {
        let root = std::env::temp_dir().join(format!("momobako-settings-corrupt-{}", std::process::id()));
        let path = root.join("settings.json");
        fs::create_dir_all(&root).expect("settings test directory should exist");
        fs::write(&path, b"{broken").expect("corrupt settings should be written");
        let (settings, diagnostic) = SettingsStore::new(path.clone())
            .load_or_recover()
            .expect("corrupt settings should recover");
        assert_eq!(settings, ApplicationSettings::default());
        assert!(diagnostic.is_some());
        assert!(path.with_extension("corrupt").exists());
        let _ = fs::remove_dir_all(root);
    }
}

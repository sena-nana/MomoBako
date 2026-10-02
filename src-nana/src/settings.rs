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

impl Default for ApplicationSettings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            thumbnail_cache_limit_mb: 1024,
            default_playlist_player_type_id: None,
            close_behavior: "confirm".into(),
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
}

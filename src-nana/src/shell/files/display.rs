//! 素材展示方式（自适应、瀑布流、网格、列表）和它的持久化。
//!
//! 对应 Vue 文件面板的展示方式切换和它存下去的 `file-display.json`：文件缺失、损坏或值未知时一律
//! 用自适应，不把错误带到页面上。

use std::fs;
use std::path::{Path, PathBuf};

use crate::settings;

use super::FilesState;

/// 素材展示方式。未知值回到自适应。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DisplayMode {
    #[default]
    Adaptive,
    Masonry,
    Grid,
    List,
}

impl DisplayMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Adaptive => "自适应",
            Self::Masonry => "瀑布流",
            Self::Grid => "网格",
            Self::List => "列表",
        }
    }

    pub fn storage_value(self) -> &'static str {
        match self {
            Self::Adaptive => "adaptive",
            Self::Masonry => "masonry",
            Self::Grid => "grid",
            Self::List => "list",
        }
    }

    /// 只接受 Vue 存下去的值和中文标签。空串和其他值都是自适应。
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "adaptive" | "自适应" => Self::Adaptive,
            "masonry" | "瀑布流" => Self::Masonry,
            "grid" | "网格" => Self::Grid,
            "list" | "列表" => Self::List,
            other => {
                if !other.is_empty() {
                    eprintln!("Nana 未知的素材展示方式，改用自适应：{other}");
                }
                Self::Adaptive
            }
        }
    }

    pub fn is_list(self) -> bool {
        self == Self::List
    }
}

impl FilesState {
    /// 从展示方式文件读取。文件缺失、损坏或值未知时使用自适应。
    pub fn load_display_mode_file(&mut self, path: &Path) {
        match fs::read_to_string(path) {
            Ok(raw) => self.display_mode = display_mode_from_file(&raw),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.display_mode = DisplayMode::Adaptive;
            }
            Err(error) => {
                eprintln!("Nana 读取展示方式失败：{error}");
                self.display_mode = DisplayMode::Adaptive;
            }
        }
    }

    pub fn save_display_mode_file(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!("Nana 创建展示方式目录失败：{error}");
            }
        }
        let body = format!("{{\"mode\":\"{}\"}}\n", self.display_mode.storage_value());
        if let Err(error) = fs::write(path, body) {
            eprintln!("Nana 保存展示方式失败：{error}");
        }
    }
}

fn display_mode_from_file(raw: &str) -> DisplayMode {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        eprintln!("Nana 展示方式文件不是 JSON");
        return DisplayMode::Adaptive;
    };
    let Some(mode) = value.get("mode").and_then(|item| item.as_str()) else {
        eprintln!("Nana 展示方式文件缺少 mode");
        return DisplayMode::Adaptive;
    };
    DisplayMode::parse(mode)
}

pub fn display_mode_path() -> PathBuf {
    settings::default_path().with_file_name("file-display.json")
}

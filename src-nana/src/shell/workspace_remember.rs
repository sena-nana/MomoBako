//! 上次打开的资源库：记在设置目录旁边的 `last-repository.json`，冷启动时照 Vue 先选它。
//!
//! 对应 Vue `lifecycle.ts` 的 `readLastActiveRepositoryId` / `rememberLastActiveRepository`（本地存储键
//! `momobako.lastActiveRepositoryId`）：
//! - 读：启动时读一次，选仓库的顺序是当前仓库、记住的仓库（还在列表里时）、列表第一项；
//! - 写：读完仓库摘要以后记住这个仓库（启动、换仓库、同一个仓库的刷新）；选中的仓库缺失时立即记住；
//!   列表空了（启动、刷新、删掉最后一个资源库）时清掉。
//!
//! 文件缺失、损坏或值不是非空字符串时当作没有记住，只记日志；写失败也只记日志，不影响启动和换仓库。

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::settings;

use super::{WorkspaceEffect, WorkspaceState};

/// 与 Vue `LAST_ACTIVE_REPOSITORY_STORAGE_KEY` 相同的存储键。
pub const LAST_ACTIVE_REPOSITORY_KEY: &str = "momobako.lastActiveRepositoryId";

/// 记住的资源库文件：和 `settings.json` 放在同一个目录。
pub fn last_repository_path() -> PathBuf {
    settings::default_path().with_file_name("last-repository.json")
}

impl WorkspaceState {
    /// 记住（或清掉）上次打开的资源库。值变了才排下写文件，同一个仓库反复刷新不重复写。
    pub(crate) fn remember_repository(&mut self, repo_id: Option<String>) {
        let next = repo_id.map(|id| id.trim().to_string()).filter(|id| !id.is_empty());
        if self.last_active_repo_id == next {
            return;
        }
        self.last_active_repo_id = next;
        self.effects.push(WorkspaceEffect::PersistLastRepository);
    }

    /// 启动前读记住的资源库。读不到时当作没有记住：缺文件不记日志，其余失败记下原因。
    pub fn load_last_repository_file(&mut self, path: &Path) {
        self.last_active_repo_id = match fs::read_to_string(path) {
            Ok(raw) => remembered_id(&raw),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => {
                eprintln!("Nana 读取上次打开的资源库失败：{error}");
                None
            }
        };
    }

    /// 写下记住的资源库；没有记住时写一个不带键的对象（Vue `removeItem`）。失败只记日志。
    pub fn save_last_repository_file(&self, path: &Path) {
        let mut value = serde_json::Map::new();
        if let Some(repo_id) = &self.last_active_repo_id {
            value.insert(LAST_ACTIVE_REPOSITORY_KEY.into(), serde_json::Value::String(repo_id.clone()));
        }
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!("Nana 上次打开的资源库目录创建失败：{error}");
            return;
        }
        if let Err(error) = fs::write(path, serde_json::Value::Object(value).to_string()) {
            eprintln!("Nana 上次打开的资源库写入失败：{error}");
        }
    }
}

/// 从文件内容里取记住的仓库 id。不是 JSON、没有这个键或值不是非空字符串时为 `None`。
fn remembered_id(raw: &str) -> Option<String> {
    let value = match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Nana 上次打开的资源库文件解析失败：{error}");
            return None;
        }
    };
    let entry = value.get(LAST_ACTIVE_REPOSITORY_KEY)?;
    let Some(repo_id) = entry.as_str() else {
        eprintln!("Nana 上次打开的资源库不是字符串：{entry}");
        return None;
    };
    Some(repo_id.trim().to_string()).filter(|id| !id.is_empty())
}

#[cfg(test)]
#[path = "workspace_remember_tests.rs"]
mod tests;

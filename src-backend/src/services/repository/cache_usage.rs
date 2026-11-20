//! 资源库缓存的真实占用：缩略图缓存和 `.momo/cache` 下各子缓存（Office 预览 PDF、网易云播放缓存、
//! 下载暂存……）的文件数、字节数和最近修改时间。
//!
//! 算法：缓存目录按子目录分项，每项递归走一遍文件累加大小、取最新的修改时间；`.momo/cache` 根下的
//! 散文件合成一项「其他缓存」。读不了的目录项跳过并记日志，不让一个坏文件挡住整张卡片。

use super::*;

/// 缓存根下子目录的显示名。没登记的子目录直接用目录名。
fn cache_label(name: &str) -> String {
    match name {
        "office-preview" => "Office 预览 PDF".into(),
        "netease-playback" => "网易云播放缓存".into(),
        "netease-directory" => "网易云目录缓存".into(),
        "download-staging" => "下载暂存".into(),
        other => other.into(),
    }
}

/// 读当前资源库的缓存占用。
pub(super) fn get_cache_snapshot(
    state: &RepositoryState,
    repo_id: &str,
) -> Result<CacheSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let storage = ensure_repository_storage_paths(
        &state.root,
        &repo.summary.repo_id,
        Path::new(&repo.summary.path),
        &repo.backend_record.plugin_id,
    )?;
    let mut entries = vec![measure(
        "thumbnails",
        "缩略图",
        &storage.metadata_dir.join("thumbnails"),
    )];
    let cache_dir = storage.metadata_dir.join("cache");
    let mut subdirs = Vec::new();
    let mut loose = Usage::default();
    if let Ok(read) = fs::read_dir(&cache_dir) {
        for entry in read {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    log_unreadable(&cache_dir, &error.to_string());
                    continue;
                }
            };
            let path = entry.path();
            if path.is_dir() {
                subdirs.push(path);
            } else {
                loose.add_file(&path);
            }
        }
    }
    subdirs.sort();
    for path in subdirs {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        entries.push(measure(
            &format!("cache/{name}"),
            &cache_label(&name),
            &path,
        ));
    }
    if loose.files > 0 {
        entries.push(loose.into_entry("cache", "其他缓存", &cache_dir));
    }
    Ok(CacheSnapshot {
        repo_id: repo.summary.repo_id,
        entries,
    })
}

/// 一个目录的累计占用。
#[derive(Default)]
struct Usage {
    files: u64,
    bytes: u64,
    modified: Option<std::time::SystemTime>,
}

impl Usage {
    fn add_file(&mut self, path: &Path) {
        match fs::metadata(path) {
            Ok(metadata) => {
                self.files += 1;
                self.bytes += metadata.len();
                if let Ok(modified) = metadata.modified() {
                    self.modified = Some(self.modified.map_or(modified, |seen| seen.max(modified)));
                }
            }
            Err(error) => log_unreadable(path, &error.to_string()),
        }
    }

    fn walk(&mut self, dir: &Path) {
        let read = match fs::read_dir(dir) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                log_unreadable(dir, &error.to_string());
                return;
            }
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.walk(&path);
            } else {
                self.add_file(&path);
            }
        }
    }

    fn into_entry(self, kind: &str, label: &str, path: &Path) -> CacheUsage {
        CacheUsage {
            kind: kind.into(),
            label: label.into(),
            path: path.to_string_lossy().to_string(),
            files: self.files,
            bytes: self.bytes,
            modified_at: self
                .modified
                .and_then(|time| system_time_to_rfc3339(time).ok()),
        }
    }
}

/// 递归量一个缓存目录。目录不存在时是空的一项。
fn measure(kind: &str, label: &str, dir: &Path) -> CacheUsage {
    let mut usage = Usage::default();
    usage.walk(dir);
    usage.into_entry(kind, label, dir)
}

fn log_unreadable(path: &Path, error: &str) {
    crate::app_log!(
        "warn",
        "repository.cache",
        "cacheUsageUnreadable",
        "缓存目录项读不了，跳过。",
        serde_json::json!({ "path": path.to_string_lossy(), "error": error })
    );
}

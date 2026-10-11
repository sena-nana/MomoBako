//! 「下载到其他资源库」的暂存目录。
//!
//! 宿主给每次下载新建一个只属于它的空目录，作为载荷的 `destination.stagingPath` 交给来源插件；插件把这次下载的文件
//! 都写在里面，宿主再导入目标资源库。导入完成（包括跳过已有的同名文件）以后由宿主清理：只删插件报告的、确实在这个
//! 目录里的文件，再删这个目录里变空的文件夹和这个目录本身；别的东西一概不删。导入失败时整个目录留着，位置写进日志，
//! 方便用户找回（见 `download_dispatch.rs`）。

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 同一进程里新建暂存目录的序号，和时间、进程号一起组成目录名。
static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

/// 暂存根目录：来源资源库有本地缓存目录时放在它的 `.momo/cache/download-staging/{目标资源库}` 下（托管缓存布局，
/// 见 `docs/api-design.md`），没有时放在服务目录的 `download-staging/{目标资源库}` 下。
pub(crate) fn staging_root(managed_cache_root: Option<&str>, service_root: &Path, target_repo_id: &str) -> PathBuf {
    let key = safe_component(target_repo_id);
    match managed_cache_root.map(str::trim).filter(|root| !root.is_empty()) {
        Some(root) => Path::new(root).join(".momo").join("cache").join("download-staging").join(key),
        None => service_root.join("download-staging").join(key),
    }
}

/// 目录名里不能有的字符换成「_」，空的或只有点时用「repository」。
fn safe_component(value: &str) -> String {
    let name: String = value
        .trim()
        .chars()
        .map(|ch| if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || ch.is_control() { '_' } else { ch })
        .collect();
    if name.trim_matches('.').is_empty() {
        "repository".into()
    } else {
        name
    }
}

/// 一次下载的暂存目录。
#[derive(Debug)]
pub(crate) struct StagingDir {
    path: PathBuf,
}

/// 清理一次暂存目录的结果。
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Cleanup {
    /// 删掉的文件。
    pub removed: Vec<PathBuf>,
    /// 插件报告的、但不在这次的暂存目录里的路径：不删。
    pub outside: Vec<String>,
    /// 删不掉的文件和原因。
    pub failed: Vec<(PathBuf, String)>,
    /// 清完以后暂存目录还在：里面还有插件没报告的东西，或者有删不掉的。
    pub left: bool,
}

/// 插件报告的一个路径和这次暂存目录的关系。
enum Reported {
    /// 在暂存目录里的文件（或链接本身），可以删。
    File(PathBuf),
    /// 在暂存目录里的文件夹：只在清空以后随空文件夹一起删。
    Folder,
    /// 不在暂存目录里，或者认不出在哪：不删。
    Outside,
    /// 已经不在了。
    Gone,
}

impl StagingDir {
    /// 在 `root` 下新建一个只属于这次下载的空目录，名字是时间、进程号和序号。同名目录已经在时换下一个序号，绝不沿用
    /// 已有的目录（`create_dir` 遇到已有的目录会失败，不会把它当成新建的）。
    pub(crate) fn create(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|error| format!("建不了下载暂存目录 {}：{error}", root.display()))?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis()).unwrap_or_default();
        for _ in 0..64 {
            let name = format!("{stamp}-{}-{}", std::process::id(), NEXT_RUN.fetch_add(1, Ordering::Relaxed));
            let path = root.join(name);
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("建不了下载暂存目录 {}：{error}", path.display())),
            }
        }
        Err(format!("建不了下载暂存目录：{} 下能用的名字都被占了", root.display()))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// 导入完成以后清掉这次下载放进来的文件：`reported` 里确实在这个目录里的文件删掉（只删文件，不递归删文件夹），
    /// 再自底向上删掉这个目录里变空的文件夹，最后删这个目录本身（空了的时候）。不在这个目录里的路径一个也不删，记在
    /// 结果的 `outside` 里。
    pub(crate) fn clear(&self, reported: &[String]) -> Cleanup {
        let mut cleanup = Cleanup::default();
        match fs::canonicalize(&self.path) {
            Ok(root) => {
                for path in reported {
                    match classify(&root, path) {
                        Reported::File(file) => match fs::remove_file(&file) {
                            Ok(()) => cleanup.removed.push(file),
                            Err(error) if error.kind() == ErrorKind::NotFound => {}
                            Err(error) => cleanup.failed.push((file, error.to_string())),
                        },
                        Reported::Outside => cleanup.outside.push(path.clone()),
                        Reported::Folder | Reported::Gone => {}
                    }
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => return cleanup,
            Err(error) => cleanup.failed.push((self.path.clone(), error.to_string())),
        }
        remove_empty_folders(&self.path);
        cleanup.left = self.path.exists();
        cleanup
    }

    /// 没有要导入的文件时（插件失败、什么也没下到）收掉这个目录：只删空的文件夹，里面有东西就原样留着。交回目录还在
    /// 不在。
    pub(crate) fn discard_if_empty(&self) -> bool {
        remove_empty_folders(&self.path);
        self.path.exists()
    }
}

/// 认出插件报告的路径在不在这次的暂存目录（`root` 已经规范化）里。按它所在文件夹的规范路径判断，链接本身在暂存目录
/// 里就算在里面（删的是链接，不碰它指向的东西）。相对路径和认不出所在文件夹的都算在外面。
fn classify(root: &Path, reported: &str) -> Reported {
    let candidate = Path::new(reported);
    if !candidate.is_absolute() {
        return Reported::Outside;
    }
    let (Some(parent), Some(name)) = (candidate.parent(), candidate.file_name()) else {
        return Reported::Outside;
    };
    let parent = match fs::canonicalize(parent) {
        Ok(parent) => parent,
        Err(error) if error.kind() == ErrorKind::NotFound => return Reported::Gone,
        Err(_) => return Reported::Outside,
    };
    if !parent.starts_with(root) {
        return Reported::Outside;
    }
    let file = parent.join(name);
    match fs::symlink_metadata(&file) {
        Ok(metadata) if metadata.is_dir() => Reported::Folder,
        Ok(_) => Reported::File(file),
        Err(error) if error.kind() == ErrorKind::NotFound => Reported::Gone,
        Err(_) => Reported::Outside,
    }
}

/// 自底向上删掉 `folder` 里空的文件夹，再删 `folder` 本身（空了的时候）。不进链接和目录联接，有东西的文件夹删不掉就
/// 留着。
fn remove_empty_folders(folder: &Path) {
    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                remove_empty_folders(&entry.path());
            }
        }
    }
    let _ = fs::remove_dir(folder);
}

#[cfg(test)]
#[path = "download_staging_tests.rs"]
pub(crate) mod tests;

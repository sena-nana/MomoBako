//! 外部程序的查找顺序：随包目录、下载目录、可执行文件旁边，最后交给 `PATH`。

use std::fs;
use std::path::{Path, PathBuf};

use super::{resolve_tool, ToolLocations};

/// 测试独占的临时目录，测试结束时删掉。
struct Dirs {
    root: PathBuf,
}

impl Dirs {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "momobako-external-tools-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        for dir in ["bundled", "downloads", "sidecar"] {
            fs::create_dir_all(root.join(dir)).expect("建测试目录");
        }
        Self { root }
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// 在目录里放一个假的程序文件，返回它的路径。
    fn tool(&self, dir: &str, name: &str) -> PathBuf {
        let path = self.dir(dir).join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        fs::write(&path, b"stub").expect("写假的程序");
        path
    }

    fn locations(&self) -> ToolLocations {
        ToolLocations {
            bundled: Some(self.dir("bundled")),
            downloads: Some(self.dir("downloads")),
        }
    }
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn bundled_tools_win_over_downloads_and_the_sidecar() {
    let dirs = Dirs::new("bundled");
    let bundled = dirs.tool("bundled", "ffmpeg");
    dirs.tool("downloads", "ffmpeg");
    dirs.tool("sidecar", "ffmpeg");

    let resolved = resolve_tool("ffmpeg", Some(&dirs.locations()), Some(&dirs.dir("sidecar")));

    assert_eq!(resolved, bundled);
}

#[test]
fn downloads_come_before_the_sidecar() {
    let dirs = Dirs::new("downloads");
    let downloaded = dirs.tool("downloads", "ffprobe");
    dirs.tool("sidecar", "ffprobe");

    let resolved = resolve_tool("ffprobe", Some(&dirs.locations()), Some(&dirs.dir("sidecar")));

    assert_eq!(resolved, downloaded);
}

#[test]
fn the_sidecar_still_counts_when_the_host_set_nothing() {
    let dirs = Dirs::new("sidecar");
    let sidecar = dirs.tool("sidecar", "ffmpeg");

    assert_eq!(resolve_tool("ffmpeg", None, Some(&dirs.dir("sidecar"))), sidecar);
    assert_eq!(
        resolve_tool("ffmpeg", Some(&ToolLocations::default()), Some(&dirs.dir("sidecar"))),
        sidecar
    );
}

#[test]
fn falls_back_to_the_bare_name_for_path_lookup() {
    let dirs = Dirs::new("path");
    fs::create_dir_all(dirs.dir("bundled").join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX)))
        .expect("和程序同名的目录不算程序");

    let resolved = resolve_tool("ffmpeg", Some(&dirs.locations()), Some(Path::new("missing-dir")));

    assert_eq!(resolved, PathBuf::from("ffmpeg"));
}

//! 随附插件同步：服务目录的 `plugins/builtin` 跟着宿主交进的清单走，只动 `.momoplug`。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{bundled_plugins_dir, list_plugin_packages, sync_bundled_plugins, BundledPluginSync};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

/// 测试独占的临时目录：`bundle` 是随包目录，`service` 是服务目录，测试结束时删掉。
struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "momobako-bundled-plugins-{name}-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("bundle")).expect("随包目录");
        Self { root }
    }

    fn bundle(&self) -> PathBuf {
        self.root.join("bundle")
    }

    fn service(&self) -> PathBuf {
        self.root.join("service")
    }

    fn package(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.bundle().join(name);
        fs::write(&path, contents).expect("写随附插件");
        path
    }

    fn builtin(&self, name: &str) -> PathBuf {
        bundled_plugins_dir(&self.service()).join(name)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn names(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("读同步后的插件")
}

#[test]
fn builtin_dir_is_under_the_service_plugins_dir() {
    let root = Path::new("service-root");
    assert_eq!(bundled_plugins_dir(root), root.join("plugins").join("builtin"));
}

#[test]
fn first_sync_copies_every_package() {
    let workspace = Workspace::new("first");
    let local = workspace.package("local-filesystem-1.0.0-x86_64-pc-windows-msvc.momoplug", "local");
    let audio = workspace.package("library-audio-0.1.0-any.momoplug", "audio");

    let report = sync_bundled_plugins(&[local, audio], &workspace.service()).expect("同步");

    assert_eq!(
        report,
        BundledPluginSync {
            copied: names(&[
                "library-audio-0.1.0-any.momoplug",
                "local-filesystem-1.0.0-x86_64-pc-windows-msvc.momoplug",
            ]),
            ..BundledPluginSync::default()
        }
    );
    assert_eq!(read(&workspace.builtin("library-audio-0.1.0-any.momoplug")), "audio");
}

#[test]
fn unchanged_packages_stay_and_changed_ones_are_replaced() {
    let workspace = Workspace::new("changed");
    let stable = workspace.package("stable-0.1.0-any.momoplug", "same");
    let moving = workspace.package("moving-0.1.0-any.momoplug", "old");
    sync_bundled_plugins(&[stable.clone(), moving.clone()], &workspace.service()).expect("第一次同步");

    fs::write(&moving, "new contents").expect("换掉随附插件");
    let report = sync_bundled_plugins(&[stable, moving], &workspace.service()).expect("第二次同步");

    assert_eq!(report.copied, names(&["moving-0.1.0-any.momoplug"]));
    assert_eq!(report.unchanged, names(&["stable-0.1.0-any.momoplug"]));
    assert!(report.removed.is_empty());
    assert_eq!(read(&workspace.builtin("moving-0.1.0-any.momoplug")), "new contents");
    assert!(!workspace.builtin("moving-0.1.0-any.momoplug.new").exists(), "临时文件要换掉");
}

#[test]
fn packages_no_longer_bundled_are_removed_and_other_files_are_kept() {
    let workspace = Workspace::new("removed");
    let old = workspace.package("source-0.1.0-any.momoplug", "old version");
    sync_bundled_plugins(&[old], &workspace.service()).expect("第一次同步");
    fs::write(workspace.builtin("notes.txt"), "keep").expect("别的文件");

    let new = workspace.package("source-0.2.0-any.momoplug", "new version");
    let report = sync_bundled_plugins(&[new], &workspace.service()).expect("升级后同步");

    assert_eq!(report.copied, names(&["source-0.2.0-any.momoplug"]));
    assert_eq!(report.removed, names(&["source-0.1.0-any.momoplug"]));
    assert!(!workspace.builtin("source-0.1.0-any.momoplug").exists());
    assert_eq!(read(&workspace.builtin("notes.txt")), "keep", "不是 .momoplug 的文件不动");
}

#[test]
fn a_missing_package_fails_without_removing_the_old_ones() {
    let workspace = Workspace::new("missing");
    let kept = workspace.package("kept-0.1.0-any.momoplug", "kept");
    sync_bundled_plugins(&[kept], &workspace.service()).expect("第一次同步");

    let missing = workspace.bundle().join("missing-0.1.0-any.momoplug");
    let error = sync_bundled_plugins(&[missing], &workspace.service()).expect_err("包不在时报错");

    assert!(error.contains("missing-0.1.0-any.momoplug"), "{error}");
    assert_eq!(read(&workspace.builtin("kept-0.1.0-any.momoplug")), "kept", "失败时不删旧包");
}

#[test]
fn non_packages_and_duplicate_names_are_rejected() {
    let workspace = Workspace::new("rejected");
    let text = workspace.package("readme.txt", "not a plugin");
    assert!(sync_bundled_plugins(&[text], &workspace.service())
        .expect_err("不是 .momoplug")
        .contains("readme.txt"));

    let first = workspace.package("same-0.1.0-any.momoplug", "first");
    fs::create_dir_all(workspace.root.join("other")).expect("另一个目录");
    let second = workspace.root.join("other").join("same-0.1.0-any.momoplug");
    fs::write(&second, "second").expect("同名的包");
    assert!(sync_bundled_plugins(&[first, second], &workspace.service())
        .expect_err("同名")
        .contains("same-0.1.0-any.momoplug"));
}

#[test]
fn listing_reads_only_packages_in_name_order() {
    let workspace = Workspace::new("list");
    workspace.package("b-0.1.0-any.momoplug", "b");
    workspace.package("a-0.1.0-any.momoplug", "a");
    workspace.package("index.json", "{}");
    fs::create_dir_all(workspace.bundle().join("folder.momoplug")).expect("同名目录");

    let listed = list_plugin_packages(&workspace.bundle()).expect("列出");

    assert_eq!(
        listed,
        vec![
            workspace.bundle().join("a-0.1.0-any.momoplug"),
            workspace.bundle().join("b-0.1.0-any.momoplug"),
        ]
    );
    assert!(list_plugin_packages(&workspace.root.join("absent")).expect("不存在的目录").is_empty());
}

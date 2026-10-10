//! 暂存目录的新建和清理，用真实的临时目录：每次下载一个新的空目录；清理只删报告过的、在这次目录里的文件和变空的
//! 文件夹，别的下载留下的、旧的暂存文件和暂存目录以外的文件一个也不碰。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{staging_root, StagingDir};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// 测试独占的临时目录，测试结束时删掉。
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("momobako-staging-{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&path).expect("临时目录");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// 在临时目录里写一个文件（连同它的上级文件夹），交回它的路径。
    pub(crate) fn file(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("上级")).expect("上级文件夹");
        fs::write(&path, text).expect("写文件");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// 暂存根目录：来源有本地缓存目录时在它的 `.momo/cache/download-staging/{目标资源库}` 下，没有时在服务目录下；目标
/// 资源库标识里不能做目录名的字符换掉。
#[test]
fn the_staging_root_follows_the_managed_cache_layout() {
    let service = Path::new("D:/service");
    assert_eq!(
        staging_root(Some(" D:/MomoCache/netease "), service, "third-repo"),
        PathBuf::from("D:/MomoCache/netease").join(".momo").join("cache").join("download-staging").join("third-repo")
    );
    assert_eq!(staging_root(None, service, "netease:42/x"), service.join("download-staging").join("netease_42_x"));
    assert_eq!(staging_root(Some("  "), service, ".."), service.join("download-staging").join("repository"));
}

/// 每次下载一个新的空目录，不沿用已有的目录。
#[test]
fn every_download_gets_a_fresh_empty_folder() {
    let scratch = Scratch::new("create");
    let root = scratch.path().join("staging");
    let first = StagingDir::create(&root).expect("第一次");
    let second = StagingDir::create(&root).expect("第二次");
    assert_ne!(first.path(), second.path());
    for staging in [&first, &second] {
        assert!(staging.path().starts_with(&root));
        assert_eq!(fs::read_dir(staging.path()).expect("读目录").count(), 0, "新建的是空目录");
    }

    let blocked = scratch.file("blocked", "根目录的位置是个文件");
    let error = StagingDir::create(&blocked).expect_err("根目录建不了");
    assert!(error.starts_with("建不了下载暂存目录"), "{error}");
}

/// 导入完成以后：报告过的、在这次目录里的文件删掉，变空的文件夹和这次的目录也删掉。别的下载留下的目录、旧的暂存
/// 文件、暂存目录以外的文件（哪怕插件报告了它）都还在，没报告的文件也在。
#[test]
fn clearing_removes_only_what_this_download_reported_inside_its_folder() {
    let scratch = Scratch::new("clear");
    let root = scratch.path().join("staging");
    let older_run = scratch.file("staging/older-run/夜曲/周杰伦 - 晴天.mp3", "上次导入失败留下的");
    let legacy = scratch.file("staging/旧暂存.mp3", "插件旧的暂存文件");
    let user_file = scratch.file("Music/周杰伦 - 晴天.mp3", "用户自己的文件");
    let staging = StagingDir::create(&root).expect("暂存目录");
    let staged = |name: &str| {
        let path = staging.path().join("夜曲").join(name);
        fs::create_dir_all(path.parent().expect("上级")).expect("上级文件夹");
        fs::write(&path, name).expect("暂存文件");
        path
    };
    let audio = staged("周杰伦 - 晴天.mp3");
    let lyric = staged("周杰伦 - 晴天.lrc");
    let reported = vec![text(&audio), text(&lyric), text(&user_file), text(&older_run), "相对路径.mp3".into()];

    let cleanup = staging.clear(&reported);

    assert_eq!(cleanup.removed.len(), 2, "{cleanup:?}");
    assert!(!audio.exists() && !lyric.exists());
    assert!(!staging.path().exists(), "这次的暂存目录清空以后删掉");
    assert!(!cleanup.left);
    assert_eq!(cleanup.outside, [text(&user_file), text(&older_run), "相对路径.mp3".to_string()]);
    assert!(cleanup.failed.is_empty());
    assert_eq!(fs::read_to_string(&user_file).expect("用户文件还在"), "用户自己的文件");
    assert_eq!(fs::read_to_string(&older_run).expect("别的下载留下的还在"), "上次导入失败留下的");
    assert_eq!(fs::read_to_string(&legacy).expect("旧暂存文件还在"), "插件旧的暂存文件");
    assert!(root.is_dir(), "暂存根目录不删");
}

/// 插件没报告的文件留在这次的目录里，目录也就留着；报告过的照删。
#[test]
fn unreported_files_keep_the_folder() {
    let scratch = Scratch::new("unreported");
    let staging = StagingDir::create(&scratch.path().join("staging")).expect("暂存目录");
    let reported = staging.path().join("a.mp3");
    let unreported = staging.path().join("b.mp3.download");
    fs::write(&reported, "a").expect("a");
    fs::write(&unreported, "b").expect("b");
    let cleanup = staging.clear(&[text(&reported)]);
    assert!(!reported.exists());
    assert!(cleanup.left && unreported.exists(), "没报告的不删，目录留着");
}

/// 插件报告了一个文件夹：只在它清空以后随空文件夹一起删，不递归删里面的东西。
#[test]
fn a_reported_folder_is_never_removed_with_its_contents() {
    let scratch = Scratch::new("folder");
    let staging = StagingDir::create(&scratch.path().join("staging")).expect("暂存目录");
    let folder = staging.path().join("夜曲");
    fs::create_dir_all(&folder).expect("文件夹");
    fs::write(folder.join("没报告.mp3"), "x").expect("文件");
    let cleanup = staging.clear(&[text(&folder)]);
    assert!(cleanup.removed.is_empty());
    assert!(folder.join("没报告.mp3").exists());
    assert!(cleanup.left);
}

/// 没有要导入的文件时只收掉空目录，里面有东西就原样留着。
#[test]
fn discarding_only_removes_empty_folders() {
    let scratch = Scratch::new("discard");
    let root = scratch.path().join("staging");
    let empty = StagingDir::create(&root).expect("暂存目录");
    fs::create_dir_all(empty.path().join("夜曲")).expect("空文件夹");
    assert!(!empty.discard_if_empty(), "空的目录收掉");
    assert!(!empty.path().exists());

    let busy = StagingDir::create(&root).expect("暂存目录");
    fs::write(busy.path().join("半截.mp3"), "x").expect("文件");
    assert!(busy.discard_if_empty(), "有东西的留着");
    assert!(busy.path().join("半截.mp3").exists());
}

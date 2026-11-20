//! 缓存占用读的是磁盘上的真实缓存：缩略图目录和 `.momo/cache` 下的各子目录分项，散文件合成「其他缓存」。

use std::fs;

use crate::services::repository::test_support::{
    create_local_repository_record_for_external_tests, create_test_state,
};

#[test]
fn cache_snapshot_measures_thumbnail_and_cache_directories() {
    let (state, root, repo_root, thumbnail_root) = create_test_state("cache-usage");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    // 先读一次，让资源库的元数据目录建好。
    let empty = state.get_cache_snapshot(&repo_id).expect("空缓存也能读");
    assert_eq!(empty.repo_id, repo_id);
    assert_eq!(empty.entries[0].kind, "thumbnails");
    assert_eq!((empty.entries[0].files, empty.entries[0].bytes), (0, 0));

    fs::create_dir_all(&thumbnail_root).expect("缩略图目录");
    fs::write(thumbnail_root.join("a.png"), [0u8; 10]).expect("缩略图");
    fs::write(thumbnail_root.join("b.png"), [0u8; 5]).expect("缩略图");
    let cache = thumbnail_root.parent().expect("元数据目录").join("cache");
    fs::create_dir_all(cache.join("office-preview").join("nested")).expect("预览缓存目录");
    fs::write(cache.join("office-preview").join("nested").join("doc.pdf"), [0u8; 7]).expect("预览");
    fs::write(cache.join("stray.tmp"), [0u8; 3]).expect("散文件");

    let snapshot = state.get_cache_snapshot(&repo_id).expect("缓存占用");
    let rows: Vec<_> = snapshot
        .entries
        .iter()
        .map(|entry| (entry.kind.as_str(), entry.label.as_str(), entry.files, entry.bytes))
        .collect();
    assert_eq!(
        rows,
        [
            ("thumbnails", "缩略图", 2, 15),
            ("cache/office-preview", "Office 预览 PDF", 1, 7),
            ("cache", "其他缓存", 1, 3),
        ]
    );
    assert!(snapshot.entries[0].modified_at.is_some());
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn cache_snapshot_rejects_an_unknown_repository() {
    let (state, root, _repo_root, _thumbnail_root) = create_test_state("cache-usage-missing");
    state.ensure_initialized().expect("初始化");
    assert!(state.get_cache_snapshot("missing").is_err());
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

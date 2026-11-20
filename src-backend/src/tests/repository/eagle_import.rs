//! Eagle 资源库导入的端到端测试：长任务 `momobako.eagle.import` 走宿主内建的导入器。
//!
//! 夹具是仓库自带的 `External/EagleLibraryRichSample.library`：图片和文档、回收站、快捷方式、智能文件夹、动作、
//! 标签组、别名和硬链接组。
//! 导入后文件落进资源库、动作登记进资源库动作表，结果带上导入后的目录快照。

use std::fs;
use std::path::PathBuf;

use crate::services::repository::test_support::{
    create_local_repository_record_for_external_tests, create_test_state,
};
use crate::services::repository::{EagleLibraryImportRequest, NeverCancelled};

fn sample_library() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("External")
        .join("EagleLibraryRichSample.library")
}

/// 复制模式导入样例资源库：各类对象按样例数量导入，文件写进资源库，源资源库不动，结果带上目录快照。
#[test]
fn eagle_import_copies_the_sample_library_into_the_repository() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("eagle-import-copy");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);

    let response = state
        .import_eagle_library_cancellable(
            EagleLibraryImportRequest {
                repo_id: repo_id.clone(),
                parent_path: None,
                library_path: sample_library().to_string_lossy().to_string(),
                mode: "copy".to_string(),
            },
            &NeverCancelled,
        )
        .expect("sample Eagle library should import");

    let summary = &response.summary;
    assert_eq!(
        (
            summary.imported_files,
            summary.imported_directories,
            summary.imported_trash_entries,
            summary.imported_shortcuts,
            summary.imported_smart_folders,
            summary.imported_repository_actions,
            summary.imported_tag_groups,
            summary.imported_alias_groups,
            summary.imported_hardlink_groups,
        ),
        (4, 4, 1, 3, 2, 1, 3, 1, 1),
        "{summary:?}"
    );
    let imported: Vec<_> = walk(&repo_root)
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "png" || ext == "doc")
        })
        .collect();
    assert!(imported.len() >= 3, "{imported:?}");
    assert!(
        sample_library().join("metadata.json").is_file(),
        "copy 模式不能动源资源库"
    );
    assert!(
        !response.snapshot.entries.is_empty(),
        "导入结果要带上目录快照"
    );
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

/// 不是 Eagle 资源库的目录按失败交回，资源库里不多出文件。
#[test]
fn eagle_import_rejects_a_folder_that_is_not_an_eagle_library() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("eagle-import-invalid");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    let not_a_library = root.join("plain-folder");
    fs::create_dir_all(&not_a_library).expect("plain folder should be created");

    let error = state
        .import_eagle_library_cancellable(
            EagleLibraryImportRequest {
                repo_id,
                parent_path: None,
                library_path: not_a_library.to_string_lossy().to_string(),
                mode: "copy".to_string(),
            },
            &NeverCancelled,
        )
        .expect_err("a plain folder is not an Eagle library");

    assert!(!error.is_empty());
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)
        .expect("directory should be readable")
        .flatten()
    {
        let path = entry.path();
        if path.file_name().is_some_and(|name| name == ".momo") {
            continue;
        }
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

//! 文件页各对话框验收场景的测试。

use super::super::super::files::{FileDialog, FilesEffect};
use super::super::super::view_harness::ShellHarness;
use super::super::super::{FilesMessage, ShellMessage, ShellViewModel};
use super::{
    files_base_scene, import_eagle_dialog_scene, import_eagle_move_dialog_scene,
    import_folder_dialog_scene, import_zip_dialog_scene, rename_dialog_scene,
};

/// 重命名场景停在 cover.png 的对话框上：草稿是原名，目录没改，没有选中时打不开。
#[test]
fn fixture_dialog_scene_shows_file_rename() {
    let mut model = rename_dialog_scene();
    assert_eq!(model.files.dialog, FileDialog::Rename);
    assert_eq!(model.files.selected_paths(), ["cover.png".to_string()]);
    assert_eq!(model.files.rename_path.as_deref(), Some("cover.png"));
    assert_eq!(model.files.name_draft, "cover.png");
    assert!(!model.files.mutating, "没有提交重命名");
    assert!(model.files.take_effects().is_empty(), "没有排下重命名");
    let paths = model
        .files
        .rows
        .iter()
        .map(|row| row.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["assets", "cover.png", "notes/page.pdf"]);

    let harness = ShellHarness::mount(model);
    for label in ["重命名文件", "新名称", "取消", "保存"] {
        assert!(harness.find(label).is_some(), "重命名对话框缺少「{label}」");
    }
    assert_eq!(harness.value(harness.input("新名称")), "cover.png");
    assert!(harness.find("处理中...").is_none(), "没有在提交");

    let mut bare = files_base_scene();
    bare.reduce(ShellMessage::Files(FilesMessage::OpenDialog(
        FileDialog::Rename,
    )));
    assert_eq!(
        bare.files.dialog,
        FileDialog::Closed,
        "没有选中目标时不打开重命名"
    );
}

/// 从文件夹导入停在空路径：不提交，夹具目录不变，输入框里没有示例路径。
#[test]
fn import_folder_dialog_stays_empty() {
    let mut model = import_folder_dialog_scene();
    assert_eq!(model.files.dialog, FileDialog::Import);
    assert!(model.files.import_draft.is_empty(), "不预填路径");
    assert!(!model.files.mutating, "没有提交导入");
    let effects = model.files.take_effects();
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, FilesEffect::Import { .. })),
        "空路径不该排下导入：{effects:?}"
    );
    let paths = model
        .files
        .rows
        .iter()
        .map(|row| row.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["assets", "cover.png", "notes/page.pdf"]);

    let harness = ShellHarness::mount(model);
    for label in ["从文件夹导入", "多个路径用分号分隔", "取消", "导入"] {
        assert!(
            harness.find(label).is_some(),
            "文件夹导入对话框缺少「{label}」"
        );
    }
    assert_eq!(harness.value(harness.input("多个路径用分号分隔")), "");
    for label in ["处理中...", "D:/素材/图片; D:/素材/参考", "导入完成"] {
        assert!(harness.find(label).is_none(), "不该出现「{label}」");
    }
}

/// 从 ZIP 导入停在空路径：没有选中文件，不提交，夹具目录不变。
#[test]
fn import_zip_dialog_stays_empty() {
    assert_empty_import(
        import_zip_dialog_scene(),
        FileDialog::ImportArchive,
        "压缩包路径",
        &["从 ZIP 导入", "压缩包路径", "取消", "导入"],
        &[
            "处理中...",
            "D:/素材/归档.zip",
            "导入完成",
            "ZIP 导入完成。",
            "目标目录",
        ],
    );
}

/// Eagle 导入停在复制标题和空路径：没有选中文件，不提交，不写完成提示或结果。
#[test]
fn import_eagle_dialog_stays_empty() {
    let model = import_eagle_dialog_scene();
    assert_eq!(model.files.eagle_mode, "copy");
    assert!(
        model.admin.eagle_library_path.is_empty(),
        "工具页路径也留空"
    );
    assert!(model.admin.eagle_import.is_none(), "不编结果卡");
    assert!(!model.admin.eagle_importing, "没有进入处理中");
    assert!(model.admin.eagle_notice.is_empty(), "不写完成提示");
    assert_empty_import(
        model,
        FileDialog::ImportEagle,
        "Eagle 资源库路径",
        &["从 Eagle 复制导入", "Eagle 资源库路径", "取消", "导入"],
        &[
            "处理中...",
            "D:/素材/示例.library",
            "导入完成",
            "复制导入完成",
            "剪切导入完成",
            "当前目标不可导入。",
            "目标目录",
            "导入结果",
        ],
    );
}

/// Eagle 剪切导入停在剪切标题和空路径：没有选中文件，不提交，不写完成提示或结果。
#[test]
fn import_eagle_move_dialog_stays_empty() {
    let model = import_eagle_move_dialog_scene();
    assert_eq!(model.files.eagle_mode, "move");
    assert!(
        model.admin.eagle_library_path.is_empty(),
        "工具页路径也留空"
    );
    assert!(model.admin.eagle_import.is_none(), "不编结果卡");
    assert!(!model.admin.eagle_importing, "没有进入处理中");
    assert!(model.admin.eagle_notice.is_empty(), "不写完成提示");
    assert_empty_import(
        model,
        FileDialog::ImportEagle,
        "Eagle 资源库路径",
        &["从 Eagle 剪切导入", "Eagle 资源库路径", "取消", "导入"],
        &[
            "从 Eagle 复制导入",
            "处理中...",
            "D:/素材/示例.library",
            "导入完成",
            "复制导入完成",
            "剪切导入完成",
            "当前目标不可导入。",
            "目标目录",
            "导入结果",
            "选择 EagleLibrary 目录",
        ],
    );
}

/// 导入对话框开着、路径是空的、选择是空的，目录三条没变，也没有排下导入。
fn assert_empty_import(
    mut model: ShellViewModel,
    dialog: FileDialog,
    field: &str,
    labels: &[&str],
    absent: &[&str],
) {
    assert_eq!(model.files.dialog, dialog);
    assert!(model.files.import_draft.is_empty(), "不预填路径");
    assert!(model.files.selected.is_empty(), "没有选中文件");
    assert!(!model.files.mutating, "没有提交导入");
    let effects = model.files.take_effects();
    assert!(
        !effects.iter().any(|effect| {
            matches!(
                effect,
                FilesEffect::Import { .. }
                    | FilesEffect::ImportArchive { .. }
                    | FilesEffect::ImportEagle { .. }
            )
        }),
        "空路径不该排下导入：{effects:?}"
    );
    let paths = model
        .files
        .rows
        .iter()
        .map(|row| row.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["assets", "cover.png", "notes/page.pdf"]);

    let harness = ShellHarness::mount(model);
    for label in labels {
        assert!(harness.find(label).is_some(), "导入对话框缺少「{label}」");
    }
    assert_eq!(harness.value(harness.input(field)), "");
    for label in absent {
        assert!(harness.find(label).is_none(), "不该出现「{label}」");
    }
}

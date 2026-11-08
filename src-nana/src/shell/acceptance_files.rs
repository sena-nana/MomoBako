//! 文件页的对照场景：文件工具栏、文件列表、右侧详情、元数据编辑、右键菜单，以及复制、重命名、从文件夹导入、从 ZIP 导入、Eagle 导入、硬链接和导出对话框。
//!
//! 场景名和 `tmp/vue-mock/scenes/files.ts` 的 Vue 场景同名，数据和 Vue 夹具 `fixtures.base()` 一致：
//! 资源库「默认资源库」，根目录下 assets 文件夹、cover.png（2400000 B）和 notes/page.pdf（1820 B），
//! 修改时间都是 2026-10-08T08:00:00Z，目录树 assets → covers 不展开，默认网格展示。工作区从
//! `acceptance_base.rs` 的共用底子起步。
//! 15 页里的「冲突」「未保存」也在这里，都在同一个文件页上走单击选中、读回详情和编辑注释的产品归约。
//! 文件操作进行中和 Ctrl / Shift 多选两个场景和 `tmp/vue-mock/scenes/states.ts` 同名。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{
    AssetDetail, AssetSummary, FileBrowserSnapshot, MetadataEntry, RepositoryStructureCacheState,
};

use super::super::admin::AdminMessage;
use super::super::files::{
    DisplayMode, EntryMenu, FileDialog, FileRow, FilesMessage, HardlinkPrompt,
};
use super::super::input::{HeldModifiers, InputMessage};
use super::super::inspect::InspectMessage;
use super::super::{
    InspectEffect, ShellMessage, ShellPage, ShellViewModel, ShortcutId, SidebarEffect,
    SidebarMessage, SidebarSmartFolder, WorkspacePanel,
};
use super::base_scene::{Base, NOW, REPO_ID, REPO_PATH, dir, file, settle, tagged_file};
/// Vue `scenes/files.ts` 的右键落点。
const MENU_POINT: (f32, f32) = (400.0, 470.0);
/// 「未保存」页注释框里还没自动保存的草稿。离屏验收按这段文字找注释输入框。
const UNSAVED_COMMENT: &str = "第 2 页的表格还要核对。";
/// 「冲突」页本地改写的注释。
const LOCAL_COMMENT: &str = "封面改用暖色版本。";
/// 「冲突」页另一端已经存进服务器的注释。
const SERVER_COMMENT: &str = "封面定稿，沿用冷色版本。";

/// 验收用的最小 PDF：目录、页树和一页，文本是解析器从这一页的内容流里读出的 `MomoBako`。
pub(super) const PAGE_PDF: &[u8] = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>\nendobj\n4 0 obj\n<< /Length 14 >>\nstream\n(MomoBako) Tj\nendstream\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n";

/// 本面板的离屏对照场景。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("live-files", live_files_scene()),
        ("live-files-selected", selected_scene("notes/page.pdf")),
        ("files-selected-metadata", metadata_scene()),
        ("files-selected-folder", selected_scene("assets")),
        ("live-menu", live_menu_scene()),
        ("copy-dialog", copy_dialog_scene()),
        ("rename-dialog", rename_dialog_scene()),
        ("import-folder-dialog", import_folder_dialog_scene()),
        ("import-zip-dialog", import_zip_dialog_scene()),
        ("import-eagle-dialog", import_eagle_dialog_scene()),
        ("import-eagle-move-dialog", import_eagle_move_dialog_scene()),
        ("hardlink-dialog", hardlink_dialog_scene()),
        ("export-dialog", export_dialog_scene()),
        ("files-list-mode", mode_scene(DisplayMode::List)),
        ("files-adaptive", mode_scene(DisplayMode::Adaptive)),
        ("files-masonry", mode_scene(DisplayMode::Masonry)),
        ("file-operation-progress", file_operation_progress_scene()),
        ("files-multi-select", multi_select_scene()),
        ("trash", trash_scene()),
        ("uncategorized", category_scene(ShortcutId::Uncategorized)),
        ("untagged", category_scene(ShortcutId::Untagged)),
        ("recent", category_scene(ShortcutId::Recent)),
        ("smart-folder", smart_folder_scene()),
        ("assets-directory", assets_directory_scene()),
        ("covers-directory", covers_directory_scene()),
    ]
}

/// 与 Vue `base()` 相同的文件页：网格、根目录、没有选择。
fn files_base_scene() -> ShellViewModel {
    Base::default().model()
}

/// 15 页的「未保存」：单击选中 notes/page.pdf、读回详情，再在注释框里输入草稿。
/// 自动保存还在 260ms 的等待里，草稿和读回的详情不同。
pub(super) fn seed_unsaved_edit(model: &mut ShellViewModel) {
    Base::default().seed(model);
    select_entry(model, "notes/page.pdf");
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        UNSAVED_COMMENT.into(),
    )));
    model.page = ShellPage::UnsavedEdit;
}

/// 15 页的「冲突」：单击选中 cover.png、读回第 1 版详情，改写注释后等自动保存。
/// 另一端已经把它存成下一版，保存应答 `conflict` 带回服务器上的详情。
/// 本地草稿留在注释框里，元数据区写出冲突并给出「采用服务器版本」。
pub(super) fn seed_conflict(model: &mut ShellViewModel) {
    Base::default().seed(model);
    select_entry(model, "cover.png");
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        LOCAL_COMMENT.into(),
    )));
    let saved = super::await_inspect_effect(model, |effect| match effect {
        InspectEffect::SaveMetadata {
            expected_version, ..
        } => Some(expected_version),
        _ => None,
    });
    match saved.and_then(|expected_version| server_detail(model, "cover.png", expected_version + 1))
    {
        Some(server) => model.reduce(ShellMessage::Inspect(InspectMessage::MetadataSaved(Ok((
            "conflict".into(),
            server,
        ))))),
        None => eprintln!("Nana 冲突场景没有等到 cover.png 的自动保存"),
    }
    model.page = ShellPage::Conflict;
}

/// 服务器上被另一端写成 `version` 版之后的素材详情：注释是另一端写入的内容。
fn server_detail(model: &ShellViewModel, path: &str, version: i64) -> Option<AssetDetail> {
    let Some(row) = model.files.rows.iter().find(|row| row.path == path) else {
        eprintln!("Nana 冲突场景找不到文件行：{path}");
        return None;
    };
    let mut detail = asset_detail(row);
    detail.summary.version = version;
    detail.metadata = vec![MetadataEntry {
        key: "comment".into(),
        value_type: "string".into(),
        value: Value::String(SERVER_COMMENT.into()),
        version,
        updated_at: NOW.into(),
    }];
    Some(detail)
}

/// 导入菜单展开，Eagle 的复制和剪切也展开。
fn live_files_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    model.files.import_open = true;
    model.files.eagle_open = true;
    model
}

/// 单击选中一条：只选中、读元数据，不进入预览页。和 Vue 单击后右侧详情一致。
fn selected_scene(path: &str) -> ShellViewModel {
    let mut model = files_base_scene();
    select_entry(&mut model, path);
    model
}

/// 带完整元数据的 cover.png：色板、注释、链接、评分、标签组、尺寸、时间和索引标签。和 Vue 一样
/// cover.png 有标签「封面」，仓库摘要里它不算未标签。
fn metadata_scene() -> ShellViewModel {
    let mut base = Base::default();
    for entry in base
        .entries
        .iter_mut()
        .filter(|entry| entry.path == "cover.png")
    {
        *entry = tagged_file("cover.png", 2_400_000, &["封面"], cover_metadata());
    }
    let mut model = base.model();
    select_entry(&mut model, "cover.png");
    model
}

/// 右键 cover.png。菜单落点和 Vue 场景派发 contextmenu 的坐标一致。
fn live_menu_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    model.files.set_drag_selection(
        vec!["cover.png".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    note_selection(&mut model, "cover.png");
    model.files.entry_menu = Some(EntryMenu {
        path: "cover.png".into(),
        x: MENU_POINT.0,
        y: MENU_POINT.1,
    });
    model
}

/// 右键 cover.png →「复制到…」，目标目录填 notes。
fn copy_dialog_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    model.files.set_drag_selection(
        vec!["cover.png".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    note_selection(&mut model, "cover.png");
    model.files.present_copy("notes");
    model
}

/// `rename-dialog`：单击选中夹具里的 cover.png，再打开「重命名文件」。
///
/// 输入框停在这一条原来的名字，不提交，目录里的文件还叫 cover.png。夹具没有这条，或单击后没有
/// 正好选中它，就不打开对话框。
fn rename_dialog_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    const PATH: &str = "cover.png";
    if !model
        .files
        .rows
        .iter()
        .any(|row| row.path == PATH && row.kind == "file")
    {
        eprintln!("Nana 重命名场景的夹具里没有 {PATH}，不打开对话框");
        settle(&mut model);
        return model;
    }
    select_entry(&mut model, PATH);
    let selected = model.files.selected_paths();
    if selected.len() != 1 || selected[0] != PATH {
        eprintln!("Nana 重命名场景没有选中 {PATH}，不打开对话框");
        settle(&mut model);
        return model;
    }
    model.reduce(ShellMessage::Files(FilesMessage::OpenDialog(
        FileDialog::Rename,
    )));
    if model.files.dialog != FileDialog::Rename {
        eprintln!("Nana 重命名场景没有打开对话框");
    }
    settle(&mut model);
    model
}

/// `import-folder-dialog`：在夹具文件页打开「从文件夹导入」。
///
/// 路径留空。夹具没有可当导入来源的外部目录，不填占位符里的示例路径，也不提交，目录条目保持原样。
fn import_folder_dialog_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    let before = model
        .files
        .rows
        .iter()
        .map(|row| row.path.clone())
        .collect::<Vec<_>>();
    model.reduce(ShellMessage::Files(FilesMessage::OpenDialog(
        FileDialog::Import,
    )));
    if model.files.dialog != FileDialog::Import {
        eprintln!("Nana 文件夹导入场景没有打开对话框");
    }
    if !model.files.import_draft.is_empty() {
        eprintln!(
            "Nana 文件夹导入场景不该预填路径：{}",
            model.files.import_draft
        );
    }
    if model
        .files
        .rows
        .iter()
        .map(|row| row.path.as_str())
        .ne(before.iter().map(String::as_str))
    {
        eprintln!("Nana 文件夹导入场景不该改目录条目");
    }
    settle(&mut model);
    model
}

/// `import-zip-dialog`：在夹具文件页打开「从 ZIP 导入」。
///
/// Vue 点这项会弹出原生「选择 ZIP 压缩包」。夹具里没有压缩包，路径留空，不把占位符写进输入框，也不提交。
fn import_zip_dialog_scene() -> ShellViewModel {
    empty_import_scene(FileDialog::ImportArchive, "ZIP 导入")
}

/// `import-eagle-dialog`：在夹具文件页打开「从 Eagle 复制导入」。
///
/// Vue 的复制和剪切都会弹出原生「选择 EagleLibrary 目录」。这里没有选中文件，路径留空。
/// 模式记成复制，标题才是这一句。不提交，不写完成提示、条数或目标目录。
fn import_eagle_dialog_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    model.files.eagle_mode = "copy".into();
    finish_empty_import(&mut model, FileDialog::ImportEagle, "Eagle 导入");
    model
}

/// `import-eagle-move-dialog`：同一个 Eagle 对话框，单独打开剪切模式。
///
/// Vue `handleImportEagle` 不区分对话框，标题都是「选择 EagleLibrary 目录」，模式只在选中路径之后
/// 交给导入。离屏打不开系统选目录，标题用这个对话框在 `move` 时的那句「从 Eagle 剪切导入」。
/// 字段和按钮与复制相同。路径留空，不提交，不写完成提示、条数或目标目录。
fn import_eagle_move_dialog_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    model.files.eagle_mode = "move".into();
    finish_empty_import(&mut model, FileDialog::ImportEagle, "Eagle 剪切导入");
    model
}

/// 打开一种导入对话框并核对它停在空路径上。目录条目和选择都保持原样。
fn empty_import_scene(dialog: FileDialog, name: &str) -> ShellViewModel {
    let mut model = files_base_scene();
    finish_empty_import(&mut model, dialog, name);
    model
}

/// 在已经铺好的文件页上打开导入对话框。没打开、预填了路径或改了目录时记下来，场景仍返回当前状态。
fn finish_empty_import(model: &mut ShellViewModel, dialog: FileDialog, name: &str) {
    let before = model
        .files
        .rows
        .iter()
        .map(|row| row.path.clone())
        .collect::<Vec<_>>();
    model.reduce(ShellMessage::Files(FilesMessage::OpenDialog(dialog)));
    if model.files.dialog != dialog {
        eprintln!("Nana {name}场景没有打开对话框");
    }
    if !model.files.import_draft.is_empty() {
        eprintln!("Nana {name}场景不该预填路径：{}", model.files.import_draft);
    }
    if !model.files.selected.is_empty() {
        eprintln!("Nana {name}场景不该选中文件");
    }
    if model
        .files
        .rows
        .iter()
        .map(|row| row.path.as_str())
        .ne(before.iter().map(String::as_str))
    {
        eprintln!("Nana {name}场景不该改目录条目");
    }
    settle(model);
}

/// 启动后读到一条硬链接候选：inbox/cover.png 与 cover.png 哈希一致。
fn hardlink_dialog_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    model.files.present_hardlink(HardlinkPrompt {
        id: "link-1".into(),
        new_path: "inbox/cover.png".into(),
        existing_path: "cover.png".into(),
        size_label: "2.3 MB".into(),
    });
    model
}

/// 导出对话框本身。Vue 首页没有入口，对照场景直接挂 `RepositoryExportDialog.vue`。
fn export_dialog_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    model.files.present_export();
    model
}

fn mode_scene(mode: DisplayMode) -> ShellViewModel {
    let mut model = files_base_scene();
    model.files.display_mode = mode;
    model
}

/// 把 cover.png 拖进 assets，移动协议还没回来：任务弹层里「移动文件 · 移动到目标文件夹」从 36 每 220ms
/// 往上爬，停在上限 88；文件列写「正在处理文件」。和 Vue 场景一样点「任务」打开弹层。
fn file_operation_progress_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    model.reduce(ShellMessage::Input(InputMessage::BeginEntryDrag {
        path: "cover.png".into(),
        selected: Vec::new(),
        x: 560.0,
        y: 280.0,
        writable: true,
        trash: false,
        smart_folder: false,
        backend_kind: "filesystem".into(),
        repo_root: REPO_PATH.into(),
        hover_folder: None,
        over_browser: true,
    }));
    model.reduce(ShellMessage::Input(InputMessage::EntryDragEnd {
        hover_folder: Some("assets".into()),
        over_browser: false,
        has_pointer: true,
    }));
    // 移动请求排下了，宿主不答，变更一直在进行中。
    let _ = model.files.take_effects();
    model.reduce(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    climb_to_ceiling(&mut model);
    settle(&mut model);
    model
}

/// 单击 cover.png，Shift 单击 clip-01.png 选出一段，再 Ctrl 单击 clip-03.png 加进来。修饰键照产品由宿主在
/// 单击前记下；右侧详情跟着主选中项 clip-03.png，读回它的素材详情。
fn multi_select_scene() -> ShellViewModel {
    let mut base = Base::default();
    base.entries.extend(
        ["clip-01.png", "clip-02.png", "clip-03.png", "clip-04.png"].map(|path| file(path, 1024)),
    );
    let mut model = base.model();
    click_entry(&mut model, "cover.png", HeldModifiers::default());
    click_entry(
        &mut model,
        "clip-01.png",
        HeldModifiers {
            shift: true,
            command: false,
        },
    );
    click_entry(
        &mut model,
        "clip-03.png",
        HeldModifiers {
            shift: false,
            command: true,
        },
    );
    model.input.modifiers = HeldModifiers::default();
    note_selection(&mut model, "clip-03.png");
    model
}

/// 按着 `modifiers` 单击一条：宿主先记下修饰键，再交单击消息。读素材的请求由调用方回答。
fn click_entry(model: &mut ShellViewModel, path: &str, modifiers: HeldModifiers) {
    model.input.modifiers = modifiers;
    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(path.into())));
    let _ = model.files.take_effects();
}

/// 照产品帧时钟推进任务弹层的仓库操作行，直到它爬到上限 88 不再动。
fn climb_to_ceiling(model: &mut ShellViewModel) {
    for _ in 0..64 {
        model.operation.tick(220);
    }
}

/// `files.ts` 的 `metadataScene` 写在 cover.png 上的元数据。
fn cover_metadata() -> BTreeMap<String, Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "comment".into(),
        Value::String("封面候选，等待确认配色。".into()),
    );
    metadata.insert(
        "link".into(),
        Value::String("https://example.com/cover".into()),
    );
    metadata.insert("rating".into(), Value::from(4));
    metadata.insert("tagGroups".into(), serde_json::json!(["封面", "参考"]));
    metadata.insert(
        "palette".into(),
        serde_json::json!(["#c7a566", "#73552f", "#d8dce2", "#3b82f6", "#1f2937"]),
    );
    metadata.insert("width".into(), Value::from(1920));
    metadata.insert("height".into(), Value::from(1080));
    metadata.insert("originalSizeBytes".into(), Value::from(2_400_000));
    metadata.insert(
        "addedToLibraryAt".into(),
        Value::String("2026-10-01T02:30:00Z".into()),
    );
    metadata.insert(
        "fileCreatedAt".into(),
        Value::String("2026-09-30T12:00:00Z".into()),
    );
    metadata
}

/// 单击选中：替换选择，文件再走一次素材详情。和产品里单击条目的路径一致。
fn select_entry(model: &mut ShellViewModel, path: &str) {
    model.reduce(ShellMessage::Files(
        super::super::files::FilesMessage::ActivateRow(path.into()),
    ));
    let _ = model.files.take_effects();
    note_selection(model, path);
}

/// 文件选中后读到的素材详情：和 Vue `get_asset_detail` 应答同样的摘要和元数据。
fn note_selection(model: &mut ShellViewModel, path: &str) {
    let Some(row) = model
        .files
        .rows
        .iter()
        .find(|row| row.path == path)
        .cloned()
    else {
        eprintln!("Nana 文件场景找不到要选中的条目：{path}");
        return;
    };
    if row.kind == "directory" {
        return;
    }
    model.files.note_selected_only(path);
    model.inspect.begin_selection(path);
    let detail = asset_detail(&row);
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail)));
    let _ = model.inspect.take_effects();
}

/// Vue `get_asset_detail` 对这一行的应答。
pub(super) fn asset_detail(row: &FileRow) -> AssetDetail {
    let size_bytes = row
        .size_label
        .trim_end_matches(" B")
        .parse::<i64>()
        .unwrap_or(0);
    AssetDetail {
        summary: AssetSummary {
            asset_id: row.asset_id.clone().unwrap_or_default(),
            repo_id: REPO_ID.into(),
            path: row.path.clone(),
            filename: row.name.clone(),
            extension: row.extension.clone().unwrap_or_default(),
            size_bytes,
            size_label: row.size_label.clone(),
            status: "ready".into(),
            modified_at: row.modified_at.clone(),
            last_accessed_at: None,
            version: 1,
            tags: row.tags.clone(),
            thumbnail_path: None,
            hardlink_group_id: None,
            hardlink_state: None,
            is_virtual: row.is_virtual,
            provider_id: row.provider_id.clone(),
            provider_item_id: row.provider_item_id.clone(),
            source_payload: row.source_payload.clone(),
            local_absolute_path: None,
        },
        metadata: row
            .metadata
            .iter()
            .map(|(key, value)| MetadataEntry {
                key: key.clone(),
                value_type: value_type(value).into(),
                value: value.clone(),
                version: 1,
                updated_at: NOW.into(),
            })
            .collect(),
        revisions: Vec::new(),
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Array(_) | Value::Object(_) => "json",
        _ => "string",
    }
}

/// 侧栏点一个分类视图。摘要里已经有的素材按 Vue 的规则筛进列表，点快捷方式不再读目录。
///
/// 夹具没有访问时间，最近使用就是空列表。不另造文件、时间和标签。
fn category_scene(id: ShortcutId) -> ShellViewModel {
    let mut model = Base::default().model();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectShortcut(id)));
    let browses = model.sidebar.take_browses();
    if !browses.is_empty() {
        eprintln!("Nana 分类视图不该再读目录：{id:?}");
    }
    settle(&mut model);
    model
}

/// `trash`：侧栏点「回收站」，再交回空目录。
///
/// 夹具摘要里回收站计数是 0，列表不编一条已删除文件。点快捷方式会排一次目录读取；
/// 场景已经用这份空目录应答，把读取从队列里拿走，准备帧才不会又写成「正在读取目录」。
fn trash_scene() -> ShellViewModel {
    let mut model = Base::default().model();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectShortcut(
        ShortcutId::Trash,
    )));
    let browses = model.sidebar.take_browses();
    if browses.is_empty() {
        eprintln!("Nana 回收站场景没有排下目录读取");
    }
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(empty_trash_snapshot())));
    settle(&mut model);
    model
}

/// 打开智能文件夹以后的结果页。
///
/// 页头、空状态和列表都走现有查询协议：打开时排 `QuerySmartFolder`，查回来的行写进
/// `virtual_rows`，摘要写「N 条结果」和有值的条件（Vue `smartFolderSummary`）。夹具侧栏读回的
/// 智能文件夹是空列表，和 Vue 模拟 IPC 一样。没有文件夹就不查，也不编条目、条件和命中数；
/// 列表保持空。右侧空文案照 Vue `FileBrowserPanel.vue` 的只读虚拟视图：眉题「智能文件夹」，
/// 「选择一个文件或文件夹」，「在中间列表中选择目标查看详情。」页头眉题是「分类视图」，
/// 名字查回来以前标题是「智能文件夹」，摘要行留空。播放条留在列表下面，Vue 仍把它当文件浏览面板。
fn smart_folder_scene() -> ShellViewModel {
    let mut model = Base::default().model();
    match model.sidebar.smart_folders.first().cloned() {
        Some(folder) => open_loaded_smart_folder(&mut model, folder),
        None => {
            model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::SmartFolder));
        }
    }
    settle(&mut model);
    model
}

/// 夹具里已经有智能文件夹时打开第一项，并按查询协议交回结果。
///
/// 验收场景没有仓库数据库，查不了真实命中。查询请求排下以后不编一行、一个条件或一条命中数，
/// 列表继续空着。
fn open_loaded_smart_folder(model: &mut ShellViewModel, folder: SidebarSmartFolder) {
    let id = folder.id.clone();
    let name = folder.name.clone();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenSmartFolder(
        id.clone(),
    )));
    let queried = model.sidebar.take_effects().into_iter().any(|effect| {
        matches!(effect, SidebarEffect::QuerySmartFolder { smart_folder_id, .. } if smart_folder_id == id)
    });
    if queried {
        eprintln!("Nana 验收夹具没有查询库，智能文件夹「{name}」不编造命中");
    } else {
        eprintln!("Nana 打开智能文件夹「{name}」没有排下查询");
    }
}

/// `assets-directory`：侧栏打开夹具目录树里的 assets，再交回这一层真实存在的子项。
///
/// 页头眉题是「当前目录」，面包屑是「根目录」加上路径段（Vue `FileBrowserPanel.vue`）。
/// 没有选中、又不是回收站或分类视图时，右侧是目录摘要：目录名、路径、直属文件、直属子文件夹、
/// 当前视图总条目。列表没有条目时 Vue 不另写一句空文案，两组卡片都不出现。
/// 子项只从夹具目录树和根目录条目里取。播放条留在文件浏览面板下面。
fn assets_directory_scene() -> ShellViewModel {
    let base = Base::default();
    let entries = fixture_directory_entries(&base, "assets");
    let mut model = base.model();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenFolder(
        "assets".into(),
    )));
    let browses = model.sidebar.take_browses();
    if browses.is_empty() {
        eprintln!("Nana assets 目录场景没有排下目录读取");
    }
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(directory_snapshot(
        "assets", entries,
    ))));
    settle(&mut model);
    model
}

/// `covers-directory`：从夹具目录树进入 assets/covers，只画这一层真实存在的子项。
///
/// 夹具树里 covers 没有子文件夹，根目录条目也没有路径正好落在这一层的文件，所以列表是空的。
/// Vue `FileBrowserPanel.vue` 在普通目录没有条目时不另写空文案，两组卡片都不出现；没有选中、
/// 又不是回收站或分类视图时，右侧仍是目录摘要（目录名、路径、直属计数）。面包屑是「根目录」
/// 加上 assets、covers。侧栏停在 covers。播放条留在文件浏览面板下面。
fn covers_directory_scene() -> ShellViewModel {
    let base = Base::default();
    let entries = fixture_directory_entries(&base, "assets/covers");
    if entries.is_empty() {
        eprintln!("Nana 验收夹具的 assets/covers 没有子项，目录页不编造文件");
    }
    let mut model = base.model();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenFolder(
        "assets/covers".into(),
    )));
    let browses = model.sidebar.take_browses();
    if browses.is_empty() {
        eprintln!("Nana covers 目录场景没有排下目录读取");
    }
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(directory_snapshot(
        "assets/covers",
        entries,
    ))));
    settle(&mut model);
    model
}

/// 夹具里某个目录的直属子项：目录树里的子文件夹，再加上根目录条目里路径正好落在这一层的文件。
///
/// 不把更深层的条目提上来，也不补夹具里没有的文件。目录树找不到这一层时记日志，只留条目里对得上的。
fn fixture_directory_entries(
    base: &Base,
    path: &str,
) -> Vec<crate::backend::services::repository::FileBrowserEntry> {
    use crate::backend::services::repository::{FileBrowserEntry, FileTreeNode};
    fn find<'a>(nodes: &'a [FileTreeNode], path: &str) -> Option<&'a FileTreeNode> {
        for node in nodes {
            if node.path == path {
                return Some(node);
            }
            if let Some(found) = find(&node.children, path) {
                return Some(found);
            }
        }
        None
    }
    /// 路径正好是 `parent` 的下一级，中间不再夹别的目录。
    fn direct_child(parent: &str, path: &str) -> bool {
        let rest = if parent.is_empty() {
            path
        } else {
            let Some(rest) = path
                .strip_prefix(parent)
                .and_then(|rest| rest.strip_prefix('/'))
            else {
                return false;
            };
            rest
        };
        !rest.is_empty() && !rest.contains('/')
    }
    let Some(node) = find(&base.tree, path) else {
        eprintln!("Nana 验收夹具的目录树里没有 {path}，目录页只留条目里对得上的");
        return base
            .entries
            .iter()
            .filter(|entry| direct_child(path, &entry.path))
            .cloned()
            .collect();
    };
    let mut entries: Vec<FileBrowserEntry> =
        node.children.iter().map(|child| dir(&child.path)).collect();
    for entry in base
        .entries
        .iter()
        .filter(|entry| direct_child(path, &entry.path))
    {
        if entries
            .iter()
            .any(|item| item.path == entry.path && item.kind == entry.kind)
        {
            continue;
        }
        entries.push(entry.clone());
    }
    entries
}

/// 某一层目录的浏览快照。条目数照交回的列表写，不另造分页。
fn directory_snapshot(
    path: &str,
    entries: Vec<crate::backend::services::repository::FileBrowserEntry>,
) -> FileBrowserSnapshot {
    let total = entries.len();
    FileBrowserSnapshot {
        repo_id: REPO_ID.into(),
        root_path: REPO_PATH.into(),
        backend_plugin_id: "momobako.source.local-filesystem".into(),
        backend_kind: "local-filesystem".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: Some(NOW.into()),
        current_path: path.into(),
        total_entries: total,
        loaded_count: total,
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries,
    }
}

/// 回收站根目录的空快照。`special_location` 是 `trash`，条目数为 0。
fn empty_trash_snapshot() -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: REPO_ID.into(),
        root_path: REPO_PATH.into(),
        backend_plugin_id: "momobako.source.local-filesystem".into(),
        backend_kind: "local-filesystem".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: Some(NOW.into()),
        current_path: String::new(),
        total_entries: 0,
        loaded_count: 0,
        next_offset: None,
        has_more: false,
        special_location: Some("trash".into()),
        tree: None,
        entries: Vec::new(),
    }
}

#[cfg(test)]
mod trash_scene_tests {
    use super::super::super::workspace::WorkspacePanel;
    use super::trash_scene;

    /// 回收站场景停在回收站面板，列表是空的，不再挂着「正在读取」。
    #[test]
    fn trash_scene_shows_the_empty_bin() {
        let model = trash_scene();
        assert_eq!(model.workspace.panel, WorkspacePanel::Trash);
        assert!(model.sidebar.browsing_trash, "侧栏应停在回收站");
        assert!(model.files.rows.is_empty(), "夹具没有回收站条目，不编文件");
        assert!(!model.files.loading, "空目录已经读回");
        assert!(
            model.staged_browses.is_empty(),
            "准备帧不应再把回收站标成读取中"
        );
    }
}

#[cfg(test)]
mod category_scene_tests {
    use super::super::super::ShortcutId;
    use super::super::super::files::FileContext;
    use super::super::super::workspace::{LibraryCategory, WorkspacePanel};
    use super::category_scene;

    fn paths(model: &super::super::super::ShellViewModel) -> Vec<String> {
        model
            .files
            .visible_rows(&FileContext::from_model(model))
            .into_iter()
            .map(|row| row.path)
            .collect()
    }

    /// 三个分类都还是文件页。列表只留摘要里对得上的素材，最近使用没有访问时间所以是空的。
    #[test]
    fn category_scenes_filter_the_snapshot() {
        let uncategorized = category_scene(ShortcutId::Uncategorized);
        assert_eq!(uncategorized.workspace.panel, WorkspacePanel::Files);
        assert_eq!(
            uncategorized.workspace.library_category,
            LibraryCategory::Uncategorized
        );
        assert_eq!(paths(&uncategorized), vec!["cover.png".to_string()]);
        assert_eq!(uncategorized.sidebar.counts.uncategorized, 1);
        assert!(!uncategorized.files.loading, "分类视图不读目录");
        assert!(
            uncategorized.player_surface_visible(),
            "分类视图仍是文件浏览面板，播放条留着"
        );
        assert!(uncategorized.staged_browses.is_empty());

        let untagged = category_scene(ShortcutId::Untagged);
        assert_eq!(
            untagged.workspace.library_category,
            LibraryCategory::Untagged
        );
        assert_eq!(
            paths(&untagged),
            vec!["cover.png".to_string(), "notes/page.pdf".to_string()]
        );
        assert_eq!(untagged.sidebar.counts.untagged, 2);
        assert!(
            untagged.files.rows.iter().any(|row| row.tags.is_empty()),
            "夹具文件本来就没有标签"
        );

        let recent = category_scene(ShortcutId::Recent);
        assert_eq!(recent.workspace.library_category, LibraryCategory::Recent);
        assert!(paths(&recent).is_empty(), "夹具没有访问时间，不编最近使用");
        assert_eq!(recent.sidebar.counts.recent, 0);
        assert!(recent.player_surface_visible());
        assert!(recent.staged_browses.is_empty());
    }
}

#[cfg(test)]
mod smart_folder_scene_tests {
    use super::super::super::files::FileContext;
    use super::super::super::workspace::WorkspacePanel;
    use super::smart_folder_scene;

    fn paths(model: &super::super::super::ShellViewModel) -> Vec<String> {
        model
            .files
            .visible_rows(&FileContext::from_model(model))
            .into_iter()
            .map(|row| row.path)
            .collect()
    }

    /// 挂上和离屏同一棵壳层，读出无障碍名。
    fn labels(model: &super::super::super::ShellViewModel) -> Vec<String> {
        let document = crate::acceptance_document_for_model(model.clone()).expect("挂载壳层");
        document
            .context()
            .world()
            .project_accessibility(document.document())
            .into_iter()
            .filter_map(|node| node.label.map(|label| label.to_string()))
            .collect()
    }

    /// 夹具没有智能文件夹：结果页是空列表，空文案用 Vue 的详情，不写命中数。
    #[test]
    fn smart_folder_results_stay_empty_without_a_fixture_folder() {
        let model = smart_folder_scene();
        assert!(
            model.sidebar.smart_folders.is_empty(),
            "夹具侧栏没有智能文件夹"
        );
        assert_eq!(model.workspace.panel, WorkspacePanel::SmartFolder);
        assert!(
            model.sidebar.active_smart_folder_id.is_none(),
            "没有文件夹就不编一个 id"
        );
        assert!(model.sidebar.smart_result.is_none(), "没有查询就不写命中数");
        assert!(model.sidebar.smart_summary().is_empty(), "摘要行留空");
        assert_eq!(model.sidebar.smart_title(), "智能文件夹");
        assert!(paths(&model).is_empty(), "结果列表不拿目录里的文件充数");
        assert!(
            model.files.rows.iter().any(|row| row.path == "cover.png"),
            "目录快照还在"
        );
        assert!(!model.files.loading, "没有在读目录");
        assert!(model.player_surface_visible(), "播放条留在列表下面");
        assert!(model.staged_browses.is_empty());

        let labels = labels(&model);
        for label in [
            "分类视图",
            "智能文件夹",
            "选择一个文件或文件夹",
            "在中间列表中选择目标查看详情。",
            "还没有智能文件夹。",
            "未选择播放内容",
        ] {
            assert!(
                labels.iter().any(|item| item == label),
                "结果页缺少「{label}」"
            );
        }
        for label in ["cover.png", "page.pdf", "0 条结果", "直属文件"] {
            assert!(
                !labels.iter().any(|item| item == label),
                "结果页不该出现「{label}」"
            );
        }
    }
}

#[cfg(test)]
mod assets_directory_scene_tests {
    use super::super::super::files::FileContext;
    use super::super::super::view_harness::ShellHarness;
    use super::super::super::view_part_sidebar::project::SidebarView;
    use super::super::super::workspace::{LibraryCategory, WorkspacePanel};
    use super::assets_directory_scene;

    /// 进入 assets 后只列出夹具里的子文件夹 covers。侧栏停在这一层，播放条留着。
    #[test]
    fn assets_directory_lists_only_fixture_children() {
        let model = assets_directory_scene();
        assert_eq!(model.workspace.panel, WorkspacePanel::Files);
        assert_eq!(model.workspace.library_category, LibraryCategory::All);
        assert_eq!(model.files.current_path, "assets");
        assert_eq!(model.sidebar.current_directory, "assets");
        assert!(
            model
                .sidebar
                .expanded_folders
                .iter()
                .any(|path| path == "assets"),
            "进入后展开 assets"
        );
        let paths = model
            .files
            .visible_rows(&FileContext::from_model(&model))
            .into_iter()
            .map(|row| row.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec!["assets/covers".to_string()],
            "夹具里 assets 只有子文件夹 covers"
        );
        assert!(
            model.files.rows.iter().all(|row| row.kind == "directory"),
            "这一层没有文件"
        );
        assert!(!model.files.loading, "目录已经读回");
        assert!(model.staged_browses.is_empty(), "准备帧不应再标成读取中");
        assert!(model.player_surface_visible(), "播放条留在文件浏览面板下面");

        let sidebar = SidebarView::project(&model);
        let assets = sidebar
            .folder_rows
            .iter()
            .find(|row| row.path == "assets")
            .expect("侧栏有 assets");
        assert!(
            assets.active && assets.expanded,
            "当前目录是 assets，不是根目录"
        );
        assert!(
            sidebar
                .folder_rows
                .iter()
                .any(|row| row.path == "assets/covers" && !row.active)
        );

        let harness = ShellHarness::mount(model);
        for label in [
            "当前目录",
            "根目录",
            "assets",
            "covers",
            "直属文件",
            "直属子文件夹",
            "当前视图总条目",
            "未选择播放内容",
        ] {
            assert!(harness.find(label).is_some(), "目录页缺少「{label}」");
        }
        for label in ["cover.png", "page.pdf", "选择一个文件或文件夹"] {
            assert!(harness.find(label).is_none(), "目录页不该出现「{label}」");
        }
    }
}

#[cfg(test)]
mod covers_directory_scene_tests {
    use super::super::super::files::FileContext;
    use super::super::super::view_harness::ShellHarness;
    use super::super::super::view_part_sidebar::project::SidebarView;
    use super::super::super::workspace::{LibraryCategory, WorkspacePanel};
    use super::covers_directory_scene;

    /// 进入 covers 后夹具这一层是空的。侧栏停在 covers，面包屑能看出根目录 / assets / covers。
    #[test]
    fn covers_directory_shows_the_empty_folder() {
        let model = covers_directory_scene();
        assert_eq!(model.workspace.panel, WorkspacePanel::Files);
        assert_eq!(model.workspace.library_category, LibraryCategory::All);
        assert_eq!(model.files.current_path, "assets/covers");
        assert_eq!(model.sidebar.current_directory, "assets/covers");
        assert!(
            model
                .sidebar
                .expanded_folders
                .iter()
                .any(|path| path == "assets"),
            "进入后展开 assets"
        );
        assert!(
            model
                .sidebar
                .expanded_folders
                .iter()
                .any(|path| path == "assets/covers"),
            "进入后展开 covers"
        );
        let paths = model
            .files
            .visible_rows(&FileContext::from_model(&model))
            .into_iter()
            .map(|row| row.path.clone())
            .collect::<Vec<_>>();
        assert!(
            paths.is_empty(),
            "夹具里 covers 没有子项，不编造文件：{paths:?}"
        );
        assert!(!model.files.loading, "空目录已经读回");
        assert!(model.staged_browses.is_empty(), "准备帧不应再标成读取中");
        assert!(model.player_surface_visible(), "播放条留在文件浏览面板下面");

        let sidebar = SidebarView::project(&model);
        let assets = sidebar
            .folder_rows
            .iter()
            .find(|row| row.path == "assets")
            .expect("侧栏有 assets");
        assert!(
            !assets.active && assets.expanded,
            "assets 展开，当前目录不是它"
        );
        let covers = sidebar
            .folder_rows
            .iter()
            .find(|row| row.path == "assets/covers")
            .expect("侧栏有 covers");
        assert!(covers.active, "当前目录是 covers");

        let harness = ShellHarness::mount(model);
        for label in [
            "当前目录",
            "根目录",
            "assets",
            "covers",
            "assets/covers",
            "直属文件",
            "直属子文件夹",
            "当前视图总条目",
            "0",
            "未选择播放内容",
        ] {
            assert!(harness.find(label).is_some(), "目录页缺少「{label}」");
        }
        for label in ["cover.png", "page.pdf", "选择一个文件或文件夹"] {
            assert!(harness.find(label).is_none(), "空目录不该出现「{label}」");
        }
    }
}

#[cfg(test)]
mod fixture_dialog_scene_tests {
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
}

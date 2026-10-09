//! 文件页的对照场景：文件工具栏、文件列表、右侧详情、元数据编辑、右键菜单和三种对话框。
//!
//! 场景名和 `tmp/vue-mock/scenes/files.ts` 的 Vue 场景同名，数据和 Vue 夹具 `fixtures.base()` 一致：
//! 资源库「默认资源库」，根目录下 assets 文件夹、cover.png（2400000 B）和 notes/page.pdf（1820 B），
//! 修改时间都是 2026-10-08T08:00:00Z，目录树 assets → covers 不展开，默认网格展示。
//! 15 页里的「冲突」「未保存」也在这里，都在同一个文件页上走单击选中、读回详情和编辑注释的产品归约。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{AssetDetail, AssetSummary, FileBrowserEntry, MetadataEntry};

use super::super::files::{DisplayMode, EntryMenu, FileRow, HardlinkPrompt};
use super::super::inspect::InspectMessage;
use super::super::sidebar::SidebarFolder;
use super::super::workspace::WorkspaceRepository;
use super::super::{InspectEffect, ShellMessage, ShellPage, ShellViewModel};

const REPO_ID: &str = "acceptance-repo";
/// Vue 夹具 `NOW`。
const NOW: &str = "2026-10-08T08:00:00Z";
/// Vue `scenes/files.ts` 的右键落点。
const MENU_POINT: (f32, f32) = (400.0, 470.0);
/// 「未保存」页注释框里还没自动保存的草稿。离屏验收按这段文字找注释输入框。
const UNSAVED_COMMENT: &str = "第 2 页的表格还要核对。";
/// 「冲突」页本地改写的注释。
const LOCAL_COMMENT: &str = "封面改用暖色版本。";
/// 「冲突」页另一端已经存进服务器的注释。
const SERVER_COMMENT: &str = "封面定稿，沿用冷色版本。";

/// 验收用的最小 PDF。文本是解析器从流里读出的 `MomoBako`。
pub(super) const PAGE_PDF: &[u8] = b"%PDF-1.4\n1 0 obj\n<< /Length 14 >>\nstream\n(MomoBako) Tj\nendstream\nendobj\n%%EOF\n";

/// 本面板的离屏对照场景。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("live-files", live_files_scene()),
        ("live-files-selected", selected_scene("notes/page.pdf")),
        ("files-selected-metadata", metadata_scene()),
        ("files-selected-folder", selected_scene("assets")),
        ("live-menu", live_menu_scene()),
        ("copy-dialog", copy_dialog_scene()),
        ("hardlink-dialog", hardlink_dialog_scene()),
        ("export-dialog", export_dialog_scene()),
        ("files-list-mode", mode_scene(DisplayMode::List)),
        ("files-adaptive", mode_scene(DisplayMode::Adaptive)),
        ("files-masonry", mode_scene(DisplayMode::Masonry)),
    ]
}

/// 与 Vue `base()` 相同的文件页：网格、根目录、没有选择。
pub(super) fn files_base_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    seed_files_base(&mut model);
    model
}

/// 在给定模型上铺 Vue `base()` 的文件页。15 页的种子拿到的是已经定好页面身份的模型。
fn seed_files_base(model: &mut ShellViewModel) {
    present_vue_repository(model);
    seed_browser(model);
    model.files.display_mode = DisplayMode::Grid;
}

/// 15 页的「未保存」：单击选中 notes/page.pdf、读回详情，再在注释框里输入草稿。
/// 自动保存还在 260ms 的等待里，草稿和读回的详情不同。
pub(super) fn seed_unsaved_edit(model: &mut ShellViewModel) {
    seed_files_base(model);
    select_entry(model, "notes/page.pdf");
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(UNSAVED_COMMENT.into())));
    model.page = ShellPage::UnsavedEdit;
}

/// 15 页的「冲突」：单击选中 cover.png、读回第 1 版详情，改写注释后等自动保存。
/// 另一端已经把它存成下一版，保存应答 `conflict` 带回服务器上的详情。
/// 本地草稿留在注释框里，元数据区写出冲突并给出「采用服务器版本」。
pub(super) fn seed_conflict(model: &mut ShellViewModel) {
    seed_files_base(model);
    select_entry(model, "cover.png");
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(LOCAL_COMMENT.into())));
    let saved = super::await_inspect_effect(model, |effect| match effect {
        InspectEffect::SaveMetadata { expected_version, .. } => Some(expected_version),
        _ => None,
    });
    match saved.and_then(|expected_version| server_detail(model, "cover.png", expected_version + 1)) {
        Some(server) => model.reduce(ShellMessage::Inspect(InspectMessage::MetadataSaved(Ok(("conflict".into(), server))))),
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

/// 带完整元数据的 cover.png：色板、注释、链接、评分、标签组、尺寸、时间和索引标签。
fn metadata_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    let metadata = cover_metadata();
    if let Some(row) = model.files.rows.iter_mut().find(|row| row.path == "cover.png") {
        row.metadata = metadata.clone();
        row.palette = super::super::palette::from_metadata_map(&metadata);
        row.tags = vec!["封面".into()];
    }
    if let Some(entry) = model.browser_entries.iter_mut().find(|entry| entry.path == "cover.png") {
        entry.metadata = metadata;
        entry.tags = vec!["封面".into()];
    }
    select_entry(&mut model, "cover.png");
    model
}

/// 右键 cover.png。菜单落点和 Vue 场景派发 contextmenu 的坐标一致。
fn live_menu_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    model.files.set_drag_selection(vec!["cover.png".into()], Some("cover.png".into()), Some("cover.png".into()));
    note_selection(&mut model, "cover.png");
    model.files.entry_menu = Some(EntryMenu { path: "cover.png".into(), x: MENU_POINT.0, y: MENU_POINT.1 });
    model
}

/// 右键 cover.png →「复制到…」，目标目录填 notes。
fn copy_dialog_scene() -> ShellViewModel {
    let mut model = files_base_scene();
    model.files.set_drag_selection(vec!["cover.png".into()], Some("cover.png".into()), Some("cover.png".into()));
    note_selection(&mut model, "cover.png");
    model.files.present_copy("notes");
    model
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

/// 和 Vue 夹具 `repository()` 一致的本地文件系统仓库。
fn present_vue_repository(model: &mut ShellViewModel) {
    model.repository_id = Some(REPO_ID.into());
    model.repository_name = "默认资源库".into();
    model.workspace.present_repository(WorkspaceRepository {
        repo_id: REPO_ID.into(),
        name: "默认资源库".into(),
        path: "C:/acceptance".into(),
        status: "ready".into(),
        backend_plugin_id: "momobako.source.local-filesystem".into(),
        capabilities: ["write", "localRootPath", "list", "read", "move", "delete", "watch"].map(String::from).to_vec(),
        cache_required: false,
        cache_status: String::new(),
    });
}

/// Vue `base()` 的目录条目、目录树和侧栏计数。其他面板的场景也用它铺文件列表。
pub(super) fn seed_browser(model: &mut ShellViewModel) {
    let entries = vec![
        entry("assets", "directory", None),
        entry("cover.png", "file", Some(2_400_000)),
        entry("notes/page.pdf", "file", Some(1_820)),
    ];
    model.files.rows = entries.iter().map(FileRow::from_entry).collect();
    model.files.total_entries = entries.len();
    model.file_entries = model.files.entry_names();
    model.browser_entries = entries;
    model.sidebar.folders = vec![SidebarFolder {
        path: "assets".into(),
        label: "assets".into(),
        children: vec![SidebarFolder { path: "assets/covers".into(), label: "covers".into(), children: Vec::new() }],
    }];
    model.sidebar.expanded_folders = Vec::new();
    model.sidebar.counts.all = 2;
    model.sidebar.counts.uncategorized = 1;
    model.sidebar.counts.untagged = 2;
}

/// 只有路径和类型的行。其他面板的场景需要自己补元数据时用它。
pub(super) fn file_row(path: &str, kind: &str) -> FileRow {
    let mut row = FileRow::from_entry(&entry(path, kind, None));
    row.modified_at.clear();
    row
}

/// Vue 夹具 `dir()` / `file()` 的条目。文件的大小文案是「字节数 B」，素材 id 把 `/` 换成 `-`。
fn entry(path: &str, kind: &str, size_bytes: Option<i64>) -> FileBrowserEntry {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let file = kind == "file";
    let extension = if file { name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()) } else { None };
    FileBrowserEntry {
        path: path.into(),
        name,
        kind: kind.into(),
        extension,
        size_bytes,
        size_label: size_bytes.map(|bytes| format!("{bytes} B")),
        modified_at: Some(NOW.into()),
        asset_id: file.then(|| path.replace('/', "-")),
        status: file.then(|| "ready".to_string()),
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata: BTreeMap::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    }
}

/// `files.ts` 的 `metadataScene` 写在 cover.png 上的元数据。
fn cover_metadata() -> BTreeMap<String, Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert("comment".into(), Value::String("封面候选，等待确认配色。".into()));
    metadata.insert("link".into(), Value::String("https://example.com/cover".into()));
    metadata.insert("rating".into(), Value::from(4));
    metadata.insert("tagGroups".into(), serde_json::json!(["封面", "参考"]));
    metadata.insert("palette".into(), serde_json::json!(["#c7a566", "#73552f", "#d8dce2", "#3b82f6", "#1f2937"]));
    metadata.insert("width".into(), Value::from(1920));
    metadata.insert("height".into(), Value::from(1080));
    metadata.insert("originalSizeBytes".into(), Value::from(2_400_000));
    metadata.insert("addedToLibraryAt".into(), Value::String("2026-10-01T02:30:00Z".into()));
    metadata.insert("fileCreatedAt".into(), Value::String("2026-09-30T12:00:00Z".into()));
    metadata
}

/// 单击选中：替换选择，文件再走一次素材详情。和产品里单击条目的路径一致。
fn select_entry(model: &mut ShellViewModel, path: &str) {
    model.reduce(ShellMessage::Files(super::super::files::FilesMessage::ActivateRow(path.into())));
    let _ = model.files.take_effects();
    note_selection(model, path);
}

/// 文件选中后读到的素材详情：和 Vue `get_asset_detail` 应答同样的摘要和元数据。
fn note_selection(model: &mut ShellViewModel, path: &str) {
    let Some(row) = model.files.rows.iter().find(|row| row.path == path).cloned() else {
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

fn asset_detail(row: &FileRow) -> AssetDetail {
    let size_bytes = row.size_label.trim_end_matches(" B").parse::<i64>().unwrap_or(0);
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
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
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

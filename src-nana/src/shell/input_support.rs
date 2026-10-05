//! 工作区拖放的纯函数，对应 Vue `dragBehavior.ts` 和路径拼接。
//!
//! 几何、父子路径和“拖到自己身上”的过滤都不碰窗口。阈值 72 与 Vue 相同。

use nana_ui::{FileDialogKind, FileDialogRequest};

/// 内部拖放改走系统拖出的距离，对应 `externalDragSwitchDistance`。
pub const EXTERNAL_DRAG_SWITCH_DISTANCE: f32 = 72.0;
/// 外部连接导出对话框。完成事件用这个编号认领。
pub const DIALOG_EXPORT_ID: u64 = 1;
/// 插件包选择对话框。
pub const DIALOG_PLUGIN_ID: u64 = 2;
/// 缺失资源库重定向的文件夹对话框。
pub const DIALOG_RELOCATE_ID: u64 = 3;

/// 关闭按钮在三种设置下的决定。脏编辑只拦截真正退出。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseDecision {
    /// 立刻关闭窗口。
    CloseNow,
    /// 先问用户。`dirty` 传给确认请求。
    Ask { dirty: bool },
    /// 设置是最小化到托盘。没有托盘时保持窗口。
    HoldForTray,
}

/// `confirm` 总是询问。`quit` 在有未保存修改时也询问。未知值按确认处理。
pub fn decide_close(behavior: &str, dirty: bool) -> CloseDecision {
    match behavior {
        "quit" if dirty => CloseDecision::Ask { dirty: true },
        "quit" => CloseDecision::CloseNow,
        "minimizeToTray" => CloseDecision::HoldForTray,
        _ => CloseDecision::Ask { dirty },
    }
}

/// 可写、不是回收站、不是智能文件夹，并且后端种类是 filesystem。
pub fn can_drag_entries(writable: bool, trash: bool, smart_folder: bool, backend_kind: &str) -> bool {
    writable && !trash && !smart_folder && backend_kind == "filesystem"
}

/// 去空白、反斜杠转正斜杠、去掉首尾斜杠。
pub fn normalize_workspace_path(path: &str) -> String {
    path.trim().replace('\\', "/").trim_matches('/').to_string()
}

/// 最后一个斜杠之前的文本。没有斜杠时是空字符串。
pub fn workspace_parent_path(path: &str) -> String {
    let normalized = normalize_workspace_path(path);
    match normalized.rfind('/') {
        Some(index) => normalized[..index].to_string(),
        None => String::new(),
    }
}

/// 指针从按下点到当前位置的直线距离。
pub fn internal_drag_distance(start_x: f32, start_y: f32, last_x: f32, last_y: f32) -> f32 {
    let dx = f64::from(last_x - start_x);
    let dy = f64::from(last_y - start_y);
    dx.hypot(dy) as f32
}

/// 指针在窗口外，并且已经拖过阈值，才把内部拖放交给系统拖出。
pub fn should_delegate_to_external_drag(
    start_x: f32,
    start_y: f32,
    last_x: f32,
    last_y: f32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    threshold: f32,
) -> bool {
    let outside = x <= 0.0 || y <= 0.0 || x >= width || y >= height;
    outside && internal_drag_distance(start_x, start_y, last_x, last_y) >= threshold
}

/// 文件夹命中优先。否则指针在文件区上时用当前目录。都没有则不能放下。
pub fn resolve_drop_target(hover_folder: Option<&str>, over_browser: bool, current_directory: &str) -> Option<String> {
    if let Some(path) = hover_folder.map(str::trim).filter(|path| !path.is_empty()) {
        return Some(path.to_string());
    }
    if over_browser {
        Some(current_directory.to_string())
    } else {
        None
    }
}

/// 丢掉空白、放到自己身上、以及父目录已经是目标的路径。
pub fn normalize_move_paths(source_paths: &[String], target_path: &str) -> Vec<String> {
    let target = normalize_workspace_path(target_path);
    source_paths
        .iter()
        .filter(|source| {
            let source = normalize_workspace_path(source);
            !source.is_empty() && source != target && workspace_parent_path(&source) != target
        })
        .cloned()
        .collect()
}

/// 保留盘符根目录上的分隔符，其余去掉末尾斜杠。
pub fn trim_trailing_separators(path: &str) -> String {
    let trimmed = path.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
        return trimmed.to_string();
    }
    let cut = trimmed.trim_end_matches(['\\', '/']);
    if cut.is_empty() { trimmed.to_string() } else { cut.to_string() }
}

/// 比较外部路径时统一成小写反斜杠。
pub fn normalize_filesystem_path(path: &str) -> String {
    trim_trailing_separators(path).replace('/', "\\").to_lowercase()
}

fn path_parts(path: &str) -> Vec<String> {
    path.trim()
        .trim_matches(['\\', '/'])
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

/// 用仓库根、相对目录和文件名拼出绝对路径。分隔符跟根路径走。
pub fn join_repository_path(root: &str, relative: &str, name: Option<&str>) -> String {
    let normalized_root = trim_trailing_separators(root);
    let mut parts = path_parts(relative);
    if let Some(name) = name.filter(|name| !name.is_empty()) {
        parts.push(name.to_string());
    }
    if parts.is_empty() {
        return normalized_root;
    }
    let separator = if normalized_root.contains('\\') { "\\" } else { "/" };
    let bytes = normalized_root.as_bytes();
    let drive = bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/');
    if drive {
        format!("{normalized_root}{}", parts.join(separator))
    } else {
        format!("{normalized_root}{separator}{}", parts.join(separator))
    }
}

/// 外部文件落到自己的绝对路径上时丢掉。空白和没有文件名的也丢掉。
pub fn filter_external_import_paths(paths: &[String], repo_root: &str, target: &str) -> Vec<String> {
    paths
        .iter()
        .filter(|source| {
            let normalized = trim_trailing_separators(source);
            if normalized.is_empty() {
                return false;
            }
            let Some(name) = path_parts(&normalized).last().cloned() else {
                return false;
            };
            if name.is_empty() {
                return false;
            }
            let absolute = join_repository_path(repo_root, target, Some(&name));
            normalize_filesystem_path(&absolute) != normalize_filesystem_path(&normalized)
        })
        .cloned()
        .collect()
}

/// 拖出使用仓库根拼出的绝对路径。没有根或没有相对路径时为空。
pub fn absolute_drag_paths(paths: &[String], repo_root: &str) -> Vec<String> {
    if repo_root.trim().is_empty() {
        return Vec::new();
    }
    paths
        .iter()
        .map(|path| normalize_workspace_path(path))
        .filter(|path| !path.is_empty())
        .map(|path| join_repository_path(repo_root, &path, None))
        .filter(|path| !path.is_empty())
        .collect()
}

/// 去掉空白的系统拖入路径。
pub fn dropped_source_paths(paths: &[String]) -> Vec<String> {
    paths.iter().map(|path| path.trim().to_string()).filter(|path| !path.is_empty()).collect()
}

/// 保存外部连接 JSON 的系统对话框请求。
pub fn save_export_request() -> FileDialogRequest {
    FileDialogRequest::new(DIALOG_EXPORT_ID, FileDialogKind::SaveFile)
        .title("导出外部连接")
        .file_name("external-api.json")
}

/// 选择一个插件包的系统对话框请求。
pub fn open_plugin_request() -> FileDialogRequest {
    FileDialogRequest::new(DIALOG_PLUGIN_ID, FileDialogKind::OpenFile).title("选择插件包")
}

/// 选择一个文件夹来重定向缺失资源库。只构造请求，不打开系统对话框。
pub fn pick_folder_request() -> FileDialogRequest {
    FileDialogRequest::new(DIALOG_RELOCATE_ID, FileDialogKind::PickFolder).title("重定向资源库位置")
}

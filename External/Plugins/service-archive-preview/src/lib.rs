use std::{
    collections::BTreeSet,
    ffi::CString,
    fs::{self, File},
    io::Write,
    os::raw::c_char,
    path::{Path, PathBuf},
};

use momobako_backend_plugin_sdk::{
    free_c_string, read_request, register_host_plugin_api, response_error, response_with_error_log,
    HostPluginCallFn, HostPluginFreeFn, PluginCallEnvelope,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use unarc_rs::unified::{ArchiveFormat, UnifiedArchive};

const MANIFEST: &str = include_str!("../manifest.json");
const DEFAULT_MAX_NESTED_DEPTH: usize = 5;
const SUPPORTED_EXTENSIONS: &[&str] = &["zip", "cbz", "7z", "rar", "cbr"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchivePayload {
    archive_path: PathBuf,
    directory_path: Option<String>,
    entry_path: Option<String>,
    config: Option<ArchivePreviewConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchivePreviewConfig {
    max_nested_depth: Option<usize>,
}

#[derive(Debug, Clone)]
struct ArchiveContext {
    archive_path: PathBuf,
    cache_root: PathBuf,
    extracted_root: PathBuf,
    max_nested_depth: usize,
}

#[derive(Debug)]
struct EntryParts {
    path: String,
    name: String,
    kind: ArchiveEntryKind,
    absolute_path: PathBuf,
    nested_depth_exceeded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ArchiveEntryKind {
    Directory,
    File,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveEntry {
    path: String,
    name: String,
    kind: ArchiveEntryKind,
    extension: Option<String>,
    size_bytes: Option<i64>,
    modified_at: Option<String>,
    previewable: bool,
    nested_depth_exceeded: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreparedArchive {
    archive_path: String,
    root_path: String,
    max_nested_depth: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreparedEntryPreview {
    path: String,
    local_path: String,
    source_url: Option<String>,
    media_type: String,
    size_bytes: i64,
    modified_at: Option<String>,
}

#[no_mangle]
pub extern "C" fn momobako_plugin_manifest() -> *mut c_char {
    CString::new(MANIFEST)
        .expect("manifest should not contain null bytes")
        .into_raw()
}

#[no_mangle]
pub extern "C" fn momobako_plugin_call(input: *const c_char) -> *mut c_char {
    let request = match read_request(input) {
        Ok(request) => request,
        Err(error) => return response_error(error),
    };
    let method = request.method.clone();
    let runtime = request.runtime.clone();
    response_with_error_log(&runtime, &method, handle_call(request))
}

#[no_mangle]
pub extern "C" fn momobako_plugin_register_host_api(
    call: Option<HostPluginCallFn>,
    free: Option<HostPluginFreeFn>,
) {
    register_host_plugin_api(call, free);
}

#[no_mangle]
pub unsafe extern "C" fn momobako_plugin_free(value: *mut c_char) {
    unsafe { free_c_string(value) };
}

fn handle_call(request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    let payload: ArchivePayload =
        serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
    let plugin_data_dir = PathBuf::from(request.runtime.plugin_data_dir);
    let context = ArchiveContext::new(
        payload.archive_path,
        &plugin_data_dir,
        payload.config.unwrap_or_default(),
    )?;

    match request.method.as_str() {
        "archive.ensurePrepared" => {
            prepare_archive_cache(&context)?;
            serde_json::to_value(PreparedArchive {
                archive_path: context.archive_path.to_string_lossy().to_string(),
                root_path: context.extracted_root.to_string_lossy().to_string(),
                max_nested_depth: context.max_nested_depth,
            })
            .map_err(|error| error.to_string())
        }
        "archive.listDirectory" => {
            prepare_archive_cache(&context)?;
            let directory_path = payload.directory_path.as_deref().unwrap_or_default();
            let entries = directory_entries(&context, directory_path)?;
            serde_json::to_value(entries).map_err(|error| error.to_string())
        }
        "archive.prepareEntryPreview" => {
            prepare_archive_cache(&context)?;
            let entry_path = payload.entry_path.as_deref().ok_or("missing entryPath")?;
            let preview = prepare_entry_preview(&context, entry_path)?;
            serde_json::to_value(preview).map_err(|error| error.to_string())
        }
        method => Err(format!("unsupported method: {method}")),
    }
}

impl ArchiveContext {
    fn new(
        archive_path: PathBuf,
        plugin_data_dir: &Path,
        config: ArchivePreviewConfig,
    ) -> Result<Self, String> {
        let max_nested_depth = config
            .max_nested_depth
            .unwrap_or(DEFAULT_MAX_NESTED_DEPTH)
            .min(32);
        let cache_root = plugin_data_dir
            .join("temp")
            .join(archive_fingerprint(&archive_path)?);
        Ok(Self {
            archive_path,
            extracted_root: cache_root.join("root"),
            cache_root,
            max_nested_depth,
        })
    }
}

fn ensure_supported_archive(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!(
            "archive file does not exist: {}",
            path.to_string_lossy()
        ));
    }
    if !path.is_file() {
        return Err(format!(
            "archive path is not a file: {}",
            path.to_string_lossy()
        ));
    }
    if !is_supported_archive_path(path) {
        return Err(format!(
            "unsupported archive format: {}",
            path.to_string_lossy()
        ));
    }
    Ok(())
}

fn prepare_archive_cache(context: &ArchiveContext) -> Result<(), String> {
    ensure_supported_archive(&context.archive_path)?;
    let marker_path = context.cache_root.join("source.json");
    let fingerprint = archive_fingerprint(&context.archive_path)?;
    if context.extracted_root.is_dir() {
        if let Ok(raw) = fs::read_to_string(&marker_path) {
            if cache_marker_matches(&raw, &fingerprint, context.max_nested_depth) {
                prepare_nested_archives(context)?;
                return Ok(());
            }
        }
    }

    remove_dir_if_exists(&context.cache_root)?;
    fs::create_dir_all(&context.extracted_root).map_err(io_error)?;
    extract_archive_to_directory(&context.archive_path, &context.extracted_root)?;
    prepare_nested_archives(context)?;
    let marker = serde_json::json!({
        "archivePath": context.archive_path.to_string_lossy(),
        "fingerprint": fingerprint,
        "maxNestedDepth": context.max_nested_depth
    });
    fs::write(
        marker_path,
        serde_json::to_string_pretty(&marker).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)?;
    Ok(())
}

fn cache_marker_matches(raw: &str, fingerprint: &str, max_nested_depth: usize) -> bool {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            Some(
                value.get("fingerprint")?.as_str()? == fingerprint
                    && value.get("maxNestedDepth")?.as_u64()? == max_nested_depth as u64,
            )
        })
        .unwrap_or(false)
}

fn prepare_nested_archives(context: &ArchiveContext) -> Result<(), String> {
    let mut visited = BTreeSet::new();
    prepare_nested_archives_in(context, &context.extracted_root, 0, &mut visited)
}

fn prepare_nested_archives_in(
    context: &ArchiveContext,
    current: &Path,
    depth: usize,
    visited: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    if !current.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(current).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if path.is_dir() {
            if should_hide_entry(&path) {
                continue;
            }
            prepare_nested_archives_in(context, &path, depth, visited)?;
            continue;
        }
        if !path.is_file() || !is_supported_archive_path(&path) {
            continue;
        }
        if depth >= context.max_nested_depth {
            continue;
        }
        if !visited.insert(path.clone()) {
            continue;
        }
        let nested_dir = nested_archive_dir(&path);
        let marker_path = nested_dir.join(".momobako-archive-source");
        let fingerprint = archive_fingerprint(&path)?;
        let ready = nested_dir.is_dir()
            && fs::read_to_string(&marker_path)
                .map(|raw| raw.contains(&fingerprint))
                .unwrap_or(false);
        if !ready {
            remove_dir_if_exists(&nested_dir)?;
            fs::create_dir_all(&nested_dir).map_err(io_error)?;
            extract_archive_to_directory(&path, &nested_dir)?;
            fs::write(&marker_path, fingerprint).map_err(io_error)?;
        }
        prepare_nested_archives_in(context, &nested_dir, depth + 1, visited)?;
    }
    Ok(())
}

fn extract_archive_to_directory(archive_path: &Path, target_dir: &Path) -> Result<(), String> {
    let format = archive_format_for_path(archive_path).ok_or_else(|| {
        format!(
            "unsupported archive format: {}",
            archive_path.to_string_lossy()
        )
    })?;
    let file = File::open(archive_path).map_err(io_error)?;
    let mut archive =
        UnifiedArchive::open_with_format(file, format).map_err(|error| error.to_string())?;

    while let Some(entry) = archive.next_entry().map_err(|error| error.to_string())? {
        let Some(relative_path) = normalize_archive_entry_name(entry.name())? else {
            archive.skip(&entry).map_err(|error| error.to_string())?;
            continue;
        };
        let output_path = resolve_output_path(target_dir, &relative_path)?;
        let entry_name = entry.name().trim_end_matches(['/', '\\']);
        let looks_like_directory = entry.name().ends_with('/') || entry.name().ends_with('\\');
        if looks_like_directory || entry_name.is_empty() {
            fs::create_dir_all(&output_path).map_err(io_error)?;
            archive.skip(&entry).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut output = File::create(&output_path).map_err(io_error)?;
        archive
            .read_to(&entry, &mut output)
            .map_err(|error| error.to_string())?;
        output.flush().map_err(io_error)?;
    }
    Ok(())
}

fn directory_entries(
    context: &ArchiveContext,
    directory_path: &str,
) -> Result<Vec<ArchiveEntry>, String> {
    let normalized = normalize_relative_path(directory_path, true)?;
    let current_dir = resolve_display_path(context, &normalized)?;
    if !current_dir.is_dir() {
        return Err(format!("directory not found: {normalized}"));
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(current_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if let Some(parts) = entry_parts(context, &path)? {
            entries.push(archive_entry_from_parts(parts)?);
        }
    }
    entries.sort_by(|left, right| {
        let left_dir = left.kind == ArchiveEntryKind::Directory;
        let right_dir = right.kind == ArchiveEntryKind::Directory;
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

fn prepare_entry_preview(
    context: &ArchiveContext,
    entry_path: &str,
) -> Result<PreparedEntryPreview, String> {
    let normalized = normalize_relative_path(entry_path, false)?;
    let absolute_path = resolve_display_path(context, &normalized)?;
    if !absolute_path.exists() {
        return Err(format!("entry not found: {normalized}"));
    }
    if absolute_path.is_dir() {
        return Err(format!("entry is a directory: {normalized}"));
    }
    if is_supported_archive_path(&absolute_path) && !nested_archive_dir(&absolute_path).is_dir() {
        return Err(format!("nested archive depth limit reached: {normalized}"));
    }
    let metadata = fs::metadata(&absolute_path).map_err(io_error)?;
    let extension = absolute_path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    Ok(PreparedEntryPreview {
        path: normalized,
        local_path: absolute_path.to_string_lossy().to_string(),
        source_url: None,
        media_type: media_type_for_extension(&extension).to_string(),
        size_bytes: metadata.len() as i64,
        modified_at: metadata
            .modified()
            .ok()
            .map(system_time_to_rfc3339)
            .transpose()?,
    })
}

fn entry_parts(context: &ArchiveContext, path: &Path) -> Result<Option<EntryParts>, String> {
    if should_hide_entry(path) {
        return Ok(None);
    }
    if path.is_file() && is_supported_archive_path(path) {
        let nested_dir = nested_archive_dir(path);
        let depth_exceeded = !nested_dir.is_dir();
        if nested_dir.is_dir() {
            return Ok(Some(EntryParts {
                path: display_relative_path(context, path)?,
                name: file_name(path),
                kind: ArchiveEntryKind::Directory,
                absolute_path: nested_dir,
                nested_depth_exceeded: false,
            }));
        }
        return Ok(Some(EntryParts {
            path: display_relative_path(context, path)?,
            name: file_name(path),
            kind: ArchiveEntryKind::File,
            absolute_path: path.to_path_buf(),
            nested_depth_exceeded: depth_exceeded,
        }));
    }
    if !path.is_file() && !path.is_dir() {
        return Ok(None);
    }
    Ok(Some(EntryParts {
        path: display_relative_path(context, path)?,
        name: file_name(path),
        kind: if path.is_dir() {
            ArchiveEntryKind::Directory
        } else {
            ArchiveEntryKind::File
        },
        absolute_path: path.to_path_buf(),
        nested_depth_exceeded: false,
    }))
}

fn archive_entry_from_parts(parts: EntryParts) -> Result<ArchiveEntry, String> {
    let metadata = fs::metadata(&parts.absolute_path).map_err(io_error)?;
    Ok(ArchiveEntry {
        path: parts.path,
        name: parts.name,
        extension: if parts.kind == ArchiveEntryKind::File {
            parts
                .absolute_path
                .extension()
                .map(|value| value.to_string_lossy().to_string())
        } else {
            None
        },
        size_bytes: if parts.kind == ArchiveEntryKind::File {
            Some(metadata.len() as i64)
        } else {
            None
        },
        modified_at: metadata
            .modified()
            .ok()
            .map(system_time_to_rfc3339)
            .transpose()?,
        previewable: parts.kind == ArchiveEntryKind::File,
        kind: parts.kind,
        nested_depth_exceeded: parts.nested_depth_exceeded,
    })
}

fn resolve_display_path(context: &ArchiveContext, display_path: &str) -> Result<PathBuf, String> {
    let normalized = normalize_relative_path(display_path, true)?;
    let mut current = context.extracted_root.clone();
    if normalized.is_empty() {
        return Ok(current);
    }
    for part in normalized.split('/').filter(|part| !part.is_empty()) {
        let next = current.join(part);
        if next.is_file() && is_supported_archive_path(&next) {
            let nested_dir = nested_archive_dir(&next);
            if nested_dir.is_dir() {
                current = nested_dir;
                continue;
            }
        }
        current = next;
    }
    Ok(current)
}

fn display_relative_path(context: &ArchiveContext, absolute_path: &Path) -> Result<String, String> {
    let relative = absolute_path
        .strip_prefix(&context.extracted_root)
        .or_else(|_| absolute_path.strip_prefix(&context.cache_root))
        .map_err(|error| error.to_string())?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let part = component.as_os_str().to_string_lossy();
        if part == ".momobako-nested" {
            continue;
        }
        if part == ".momobako-archive-source" {
            return Ok(String::new());
        }
        parts.push(part.to_string());
    }
    Ok(parts.join("/"))
}

fn nested_archive_dir(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    parent.join(".momobako-nested").join(file_name(path))
}

fn should_hide_entry(path: &Path) -> bool {
    path.file_name()
        .map(|value| {
            matches!(
                value.to_string_lossy().as_ref(),
                ".momobako-nested" | ".momobako-archive-source"
            )
        })
        .unwrap_or(false)
}

fn archive_format_for_path(path: &Path) -> Option<ArchiveFormat> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())?;
    match extension.as_str() {
        "cbz" => Some(ArchiveFormat::Zip),
        "cbr" => Some(ArchiveFormat::Rar),
        "zip" => Some(ArchiveFormat::Zip),
        "7z" => Some(ArchiveFormat::SevenZ),
        "rar" => Some(ArchiveFormat::Rar),
        _ => None,
    }
}

fn is_supported_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| {
            let lower = value.to_ascii_lowercase();
            SUPPORTED_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

fn archive_fingerprint(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(io_error)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(metadata.len().to_string().as_bytes());
    hasher.update(modified.to_string().as_bytes());
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn normalize_archive_entry_name(name: &str) -> Result<Option<String>, String> {
    let raw = name.trim().replace('\\', "/");
    if raw.starts_with('/') {
        return Err("archive entry cannot use an absolute path".to_string());
    }
    if raw
        .split('/')
        .next()
        .map(|part| part.contains(':'))
        .unwrap_or(false)
    {
        return Err("archive entry cannot use a drive prefix".to_string());
    }
    let trimmed = raw.trim_matches('/').to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }
    normalize_relative_path(&trimmed, false).map(Some)
}

fn normalize_relative_path(path: &str, allow_empty: bool) -> Result<String, String> {
    let raw = path.trim().replace('\\', "/");
    if raw.starts_with('/') {
        return Err("path cannot be absolute".to_string());
    }
    if raw
        .split('/')
        .next()
        .map(|part| part.contains(':'))
        .unwrap_or(false)
    {
        return Err("path cannot use a drive prefix".to_string());
    }
    let trimmed = raw.trim_matches('/').to_string();
    if trimmed.is_empty() {
        return if allow_empty {
            Ok(String::new())
        } else {
            Err("path cannot be empty".to_string())
        };
    }
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err("path cannot escape archive root".to_string());
        }
        parts.push(part);
    }
    let normalized = parts.join("/");
    if normalized.is_empty() && !allow_empty {
        Err("path cannot be empty".to_string())
    } else {
        Ok(normalized)
    }
}

fn resolve_output_path(root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let mut target = root.to_path_buf();
    for part in normalize_relative_path(relative_path, false)?.split('/') {
        target.push(part);
    }
    let parent = target.parent().unwrap_or(root);
    fs::create_dir_all(parent).map_err(io_error)?;
    let root_canonical = root.canonicalize().map_err(io_error)?;
    let parent_canonical = parent.canonicalize().map_err(io_error)?;
    if !parent_canonical.starts_with(root_canonical) {
        return Err("archive entry cannot escape extraction root".to_string());
    }
    Ok(target)
}

fn media_type_for_extension(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "opus" => "audio/ogg",
        "flac" => "audio/flac",
        "m4a" | "aac" => "audio/aac",
        "md" | "markdown" | "mdown" | "mkd" | "mkdn" | "mdx" => "text/markdown",
        "txt" | "text" | "log" | "csv" | "tsv" | "yaml" | "yml" | "toml" | "xml" | "html"
        | "css" | "scss" | "sass" | "less" | "js" | "jsx" | "ts" | "tsx" | "vue" | "rs" | "py"
        | "rb" | "go" | "java" | "c" | "h" | "cpp" | "hpp" | "cs" | "php" | "sh" | "bash"
        | "zsh" | "ps1" | "bat" | "cmd" | "ini" | "cfg" | "conf" | "env" | "gitignore"
        | "gitattributes" => "text/plain",
        "json" | "jsonl" => "application/json",
        _ => "application/octet-stream",
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "archive".to_string())
}

fn remove_dir_if_exists(path: &Path) -> Result<(), String> {
    fs::remove_dir_all(path)
        .or_else(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(io_error)
}

fn system_time_to_rfc3339(value: std::time::SystemTime) -> Result<String, String> {
    let datetime: OffsetDateTime = value.into();
    datetime.format(&Rfc3339).map_err(|error| error.to_string())
}

fn io_error(error: std::io::Error) -> String {
    format!("io error: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new(label: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "momobako-archive-preview-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("workspace should be created");
            Self { root }
        }

        fn path(&self, child: &str) -> PathBuf {
            self.root.join(child)
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).expect("zip should be created");
        let mut zip = zip::ZipWriter::new(file);
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("zip entry should start");
            zip.write_all(bytes).expect("zip entry should write");
        }
        zip.finish().expect("zip should finish");
    }

    fn context(workspace: &TestWorkspace, archive_name: &str) -> ArchiveContext {
        ArchiveContext::new(
            workspace.path(archive_name),
            &workspace.path("plugin-data"),
            ArchivePreviewConfig::default(),
        )
        .expect("context should build")
    }

    #[test]
    fn zip_archive_lists_files_and_directories() {
        let workspace = TestWorkspace::new("zip-list");
        write_zip(
            &workspace.path("repo.zip"),
            &[("cover.jpg", b"image"), ("chapter/page.txt", b"hello")],
        );
        let context = context(&workspace, "repo.zip");

        prepare_archive_cache(&context).expect("archive should extract");
        let root_entries = directory_entries(&context, "").expect("root should list");
        let preview = prepare_entry_preview(&context, "chapter/page.txt")
            .expect("file preview should prepare");

        assert!(root_entries.iter().any(|entry| entry.path == "cover.jpg"));
        assert!(root_entries
            .iter()
            .any(|entry| entry.path == "chapter" && entry.kind == ArchiveEntryKind::Directory));
        assert_eq!(preview.media_type, "text/plain");
        assert!(Path::new(&preview.local_path).is_file());
    }

    #[test]
    fn cbz_archive_uses_zip_reader() {
        let workspace = TestWorkspace::new("cbz-list");
        write_zip(&workspace.path("book.cbz"), &[("page01.png", b"png")]);
        let context = context(&workspace, "book.cbz");

        prepare_archive_cache(&context).expect("cbz should extract");
        let entry = prepare_entry_preview(&context, "page01.png").expect("entry should preview");

        assert_eq!(entry.media_type, "image/png");
    }

    #[test]
    fn nested_archive_is_exposed_as_directory() {
        let workspace = TestWorkspace::new("nested");
        write_zip(&workspace.path("inner.zip"), &[("inside.txt", b"inside")]);
        let inner_bytes = fs::read(workspace.path("inner.zip")).expect("inner zip should read");
        write_zip(
            &workspace.path("repo.zip"),
            &[("outer.txt", b"outer"), ("inner.zip", &inner_bytes)],
        );
        let context = context(&workspace, "repo.zip");

        prepare_archive_cache(&context).expect("nested archive should extract");
        let root_entries = directory_entries(&context, "").expect("root should list");
        let nested_entries = directory_entries(&context, "inner.zip").expect("nested should list");
        let nested_file = prepare_entry_preview(&context, "inner.zip/inside.txt")
            .expect("nested file should preview");

        assert!(root_entries
            .iter()
            .any(|entry| entry.path == "inner.zip" && entry.kind == ArchiveEntryKind::Directory));
        assert!(nested_entries
            .iter()
            .any(|entry| entry.path == "inner.zip/inside.txt"));
        assert!(Path::new(&nested_file.local_path).is_file());
    }

    #[test]
    fn path_normalization_rejects_escape() {
        assert!(normalize_relative_path("../escape.txt", false).is_err());
        assert!(normalize_relative_path("/escape.txt", false).is_err());
        assert!(normalize_relative_path("C:/escape.txt", false).is_err());
        assert!(normalize_archive_entry_name("../escape.txt").is_err());
        assert!(normalize_archive_entry_name("C:/escape.txt").is_err());
        assert!(normalize_archive_entry_name("/escape.txt").is_err());
    }

    #[test]
    fn nested_depth_limit_keeps_archive_as_file() {
        let workspace = TestWorkspace::new("depth");
        write_zip(&workspace.path("inner.zip"), &[("inside.txt", b"inside")]);
        let inner_bytes = fs::read(workspace.path("inner.zip")).expect("inner zip should read");
        write_zip(&workspace.path("repo.zip"), &[("inner.zip", &inner_bytes)]);
        let context = ArchiveContext::new(
            workspace.path("repo.zip"),
            &workspace.path("plugin-data"),
            ArchivePreviewConfig {
                max_nested_depth: Some(0),
            },
        )
        .expect("context should build");

        prepare_archive_cache(&context).expect("archive should extract");
        let entries = directory_entries(&context, "").expect("root should list");

        assert!(entries.iter().any(|entry| {
            entry.path == "inner.zip"
                && entry.kind == ArchiveEntryKind::File
                && entry.nested_depth_exceeded
        }));
        assert!(prepare_entry_preview(&context, "inner.zip")
            .expect_err("depth-limited archive should not preview as regular file")
            .contains("depth limit"));
    }
}

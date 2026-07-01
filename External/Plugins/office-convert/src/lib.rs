//! Office 预览转换插件。
//!
//! 负责把 Office 文档转换为 PDF 并缓存到资源库缓存目录。

use std::{
    ffi::{c_char, CString},
    fs,
    os::raw::c_char as raw_c_char,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::Duration,
};

use momobako_backend_plugin_sdk::{
    call_host_plugin, free_c_string, read_request, register_host_plugin_api, response_error,
    response_ok, HostPluginCallFn, HostPluginFreeFn, PluginRuntimeContext,
};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const MANIFEST: &str = include_str!("../manifest.json");
const REGISTRY_FILE_NAME: &str = "repositories.db";
const REPO_META_DIR: &str = ".momo";
const OFFICE_CACHE_NAMESPACE: &str = "office-preview";
const DEFAULT_LIBREOFFICE_VERSION: &str = "25.8.3";
const DEFAULT_LIBREOFFICE_DOWNLOAD_URL: &str =
    "https://download.documentfoundation.org/libreoffice/stable/25.8.3/win/x86_64/LibreOffice_25.8.3_Win_x86-64.msi";
const DOWNLOADER_PLUGIN_ID: &str = "momobako.service.downloader";

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug)]
struct RuntimeContext {
    plugin_data_dir: PathBuf,
    service_root_dir: PathBuf,
    config: PluginConfig,
}

#[derive(Debug)]
struct PluginConfig {
    converter_mode: ConverterMode,
    auto_download_libreoffice: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConverterMode {
    Auto,
    MicrosoftOffice,
    LibreOffice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OfficeFamily {
    Word,
    Spreadsheet,
    Presentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConverterKind {
    MicrosoftOffice,
    LibreOfficeSystem,
    LibreOfficeBundled,
}

#[derive(Debug, Clone)]
struct SelectedConverter {
    kind: ConverterKind,
    family: OfficeFamily,
    executable_path: PathBuf,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnsurePreviewPdfPayload {
    repo_id: String,
    entry_path: String,
    extension: String,
    source_path: Option<String>,
    source_modified_at: Option<String>,
    source_size_bytes: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ClearPreviewCachePayload {
    repo_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConverterStatus {
    available: bool,
    path: Option<String>,
    version: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeStatusResponse {
    converter_mode: String,
    microsoft_office: ConverterStatus,
    libreoffice_system: ConverterStatus,
    libreoffice_bundle: ConverterStatus,
    daemon: serde_json::Value,
    auto_download_libre_office: bool,
    bundled_download_url: String,
}

#[no_mangle]
pub extern "C" fn momobako_plugin_manifest() -> *mut raw_c_char {
    CString::new(MANIFEST)
        .expect("manifest should not contain null bytes")
        .into_raw()
}

#[no_mangle]
pub extern "C" fn momobako_plugin_call(input: *const c_char) -> *mut c_char {
    match handle_call(input) {
        Ok(value) => response_ok(value),
        Err(error) => response_error(error),
    }
}

#[no_mangle]
pub unsafe extern "C" fn momobako_plugin_free(value: *mut c_char) {
    unsafe { free_c_string(value) };
}

#[no_mangle]
pub extern "C" fn momobako_plugin_register_host_api(
    call: Option<HostPluginCallFn>,
    free: Option<HostPluginFreeFn>,
) {
    register_host_plugin_api(call, free);
}

fn handle_call(input: *const c_char) -> Result<serde_json::Value, String> {
    let request = read_request(input)?;
    let runtime = runtime_context(request.runtime)?;

    match request.method.as_str() {
        "officeConvert.ensurePreviewPdf" => {
            let payload: EnsurePreviewPdfPayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            ensure_preview_pdf(&runtime, payload)
        }
        "officeConvert.getRuntimeStatus" => serde_json::to_value(get_runtime_status(&runtime)?)
            .map_err(|error| error.to_string()),
        "officeConvert.clearPreviewCache" => {
            let payload: ClearPreviewCachePayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            clear_preview_cache(&runtime, payload)
        }
        "officeConvert.shutdownDaemon" => shutdown_libreoffice_daemon(&runtime),
        method => Err(format!("unsupported method: {method}")),
    }
}

fn runtime_context(runtime: PluginRuntimeContext) -> Result<RuntimeContext, String> {
    let plugin_data_dir = PathBuf::from(runtime.plugin_data_dir);
    let service_root_dir = normalize_service_root_dir(
        runtime.service_root_dir,
        plugin_data_dir.as_path(),
    )?;
    fs::create_dir_all(plugin_data_dir.join("helpers").join("libreoffice")).map_err(io_error)?;
    fs::create_dir_all(plugin_data_dir.join("downloads")).map_err(io_error)?;
    cleanup_stale_libreoffice_state(&plugin_data_dir)?;
    let config = PluginConfig {
        converter_mode: ConverterMode::from_config(
            runtime
                .plugin_config
                .get("converterMode")
                .and_then(serde_json::Value::as_str),
        ),
        auto_download_libreoffice: runtime
            .plugin_config
            .get("autoDownloadLibreOffice")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
    };
    Ok(RuntimeContext {
        plugin_data_dir,
        service_root_dir,
        config,
    })
}

impl ConverterMode {
    fn from_config(value: Option<&str>) -> Self {
        match value.map(str::trim).unwrap_or_default() {
            "microsoft-office" => Self::MicrosoftOffice,
            "libreoffice" => Self::LibreOffice,
            _ => Self::Auto,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::MicrosoftOffice => "microsoft-office",
            Self::LibreOffice => "libreoffice",
        }
    }
}

fn ensure_preview_pdf(
    runtime: &RuntimeContext,
    payload: EnsurePreviewPdfPayload,
) -> Result<serde_json::Value, String> {
    let source_path = payload
        .source_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "missing sourcePath".to_string())?;
    if !source_path.is_file() {
        return Err(format!(
            "source file is not available: {}",
            source_path.to_string_lossy()
        ));
    }
    let extension = payload.extension.trim().to_ascii_lowercase();
    let source_metadata = fs::metadata(&source_path).map_err(io_error)?;
    let source_size_bytes = payload
        .source_size_bytes
        .filter(|value| *value >= 0)
        .unwrap_or(source_metadata.len() as i64);
    let source_modified_at = payload
        .source_modified_at
        .clone()
        .or_else(|| {
            source_metadata
                .modified()
                .ok()
                .and_then(|value| system_time_to_rfc3339(value).ok())
        })
        .unwrap_or_default();
    let family = office_family_from_extension(&extension)
        .ok_or_else(|| format!("unsupported office extension: {extension}"))?;
    let converter = select_converter(runtime, family)?;
    let cache_key = preview_cache_key(
        &payload.repo_id,
        &payload.entry_path,
        &source_path,
        source_size_bytes,
        &source_modified_at,
        &extension,
        &converter.cache_key_label(),
        converter.version.as_deref().unwrap_or_default(),
    );
    let cache_dir = repository_cache_dir_for_repo(runtime, &payload.repo_id)?;
    fs::create_dir_all(&cache_dir).map_err(io_error)?;
    let pdf_path = cache_dir.join(format!("{cache_key}.pdf"));
    let cached = pdf_path.is_file();
    if !cached {
        convert_office_to_pdf(runtime, &converter, &source_path, &pdf_path)?;
    }
    let metadata = fs::metadata(&pdf_path).map_err(io_error)?;
    let modified_at = metadata
        .modified()
        .ok()
        .map(system_time_to_rfc3339)
        .transpose()
        .map_err(time_error)?;
    Ok(serde_json::json!({
        "pdfPath": pdf_path.to_string_lossy().to_string(),
        "cached": cached,
        "converter": converter.result_label(),
        "cacheKey": cache_key,
        "mediaType": "application/pdf",
        "sizeBytes": metadata.len() as i64,
        "modifiedAt": modified_at
    }))
}

fn get_runtime_status(runtime: &RuntimeContext) -> Result<RuntimeStatusResponse, String> {
    let microsoft_office = detect_microsoft_office();
    let libreoffice_system = detect_system_libreoffice();
    let libreoffice_bundle = detect_bundled_libreoffice(runtime);
    let daemon_status = current_libreoffice_daemon_status(runtime)?;
    Ok(RuntimeStatusResponse {
        converter_mode: runtime.config.converter_mode.as_str().to_string(),
        microsoft_office,
        libreoffice_system,
        libreoffice_bundle,
        daemon: daemon_status,
        auto_download_libre_office: runtime.config.auto_download_libreoffice,
        bundled_download_url: DEFAULT_LIBREOFFICE_DOWNLOAD_URL.to_string(),
    })
}

fn detect_microsoft_office() -> ConverterStatus {
    for family in [
        OfficeFamily::Word,
        OfficeFamily::Spreadsheet,
        OfficeFamily::Presentation,
    ] {
        let status = detect_microsoft_office_for_family(family);
        if status.available {
            return status;
        }
    }
    ConverterStatus {
        available: false,
        path: None,
        version: None,
        reason: Some("未探测到系统 Microsoft Office 安装。".to_string()),
    }
}

fn clear_preview_cache(
    runtime: &RuntimeContext,
    payload: ClearPreviewCachePayload,
) -> Result<serde_json::Value, String> {
    let cache_dir = repository_cache_dir_for_repo(runtime, &payload.repo_id)?;
    let mut removed = 0;
    if cache_dir.is_dir() {
        removed = remove_files_recursively(&cache_dir)?;
    }
    Ok(serde_json::json!({
        "repoId": payload.repo_id,
        "removed": removed
    }))
}

/// 按策略选择真实可执行的转换器。
fn select_converter(
    runtime: &RuntimeContext,
    family: OfficeFamily,
) -> Result<SelectedConverter, String> {
    match runtime.config.converter_mode {
        ConverterMode::MicrosoftOffice => {
            select_microsoft_office(family)
        }
        ConverterMode::LibreOffice => {
            select_libreoffice(runtime, family)
        }
        ConverterMode::Auto => {
            if let Ok(converter) = select_microsoft_office(family) {
                return Ok(converter);
            }
            select_libreoffice(runtime, family)
        }
    }
}

fn preview_cache_key(
    repo_id: &str,
    entry_path: &str,
    source_path: &Path,
    source_size_bytes: i64,
    source_modified_at: &str,
    extension: &str,
    converter: &str,
    converter_version: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(entry_path.as_bytes());
    hasher.update(b"\n");
    hasher.update(source_path.to_string_lossy().as_bytes());
    hasher.update(b"\n");
    hasher.update(source_size_bytes.to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(source_modified_at.as_bytes());
    hasher.update(b"\n");
    hasher.update(extension.as_bytes());
    hasher.update(b"\n");
    hasher.update(converter.as_bytes());
    hasher.update(b"\n");
    hasher.update(converter_version.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 调用真实转换器生成 PDF。
fn convert_office_to_pdf(
    runtime: &RuntimeContext,
    converter: &SelectedConverter,
    source_path: &Path,
    pdf_path: &Path,
) -> Result<(), String> {
    if let Some(parent) = pdf_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let temp_dir = runtime.plugin_data_dir.join("temp").join("office-convert");
    fs::create_dir_all(&temp_dir).map_err(io_error)?;
    match converter.kind {
        ConverterKind::MicrosoftOffice => {
            convert_with_microsoft_office(converter, source_path, pdf_path)
        }
        ConverterKind::LibreOfficeSystem | ConverterKind::LibreOfficeBundled => {
            convert_with_libreoffice(runtime, converter, source_path, pdf_path, &temp_dir)
        }
    }
}

fn repository_cache_dir_for_repo(runtime: &RuntimeContext, repo_id: &str) -> Result<PathBuf, String> {
    let repo_root = repository_root_for_id(runtime, repo_id)?;
    Ok(repository_cache_dir(&repo_root))
}

fn repository_root_for_id(runtime: &RuntimeContext, repo_id: &str) -> Result<PathBuf, String> {
    let registry_path = runtime.service_root_dir.join(REGISTRY_FILE_NAME);
    let connection = Connection::open(&registry_path).map_err(|error| {
        format!(
            "failed to open repository registry {}: {error}",
            registry_path.display()
        )
    })?;
    let path = connection
        .query_row(
            "SELECT path FROM repositories WHERE repo_id = ?1 LIMIT 1",
            [repo_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("repository not found: {repo_id}"))?;
    Ok(PathBuf::from(path))
}

fn repository_cache_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(REPO_META_DIR).join("cache").join(OFFICE_CACHE_NAMESPACE)
}

fn select_microsoft_office(family: OfficeFamily) -> Result<SelectedConverter, String> {
    let status = detect_microsoft_office_for_family(family);
    if !status.available {
        return Err(status
            .reason
            .unwrap_or_else(|| "Microsoft Office is unavailable".to_string()));
    }
    Ok(SelectedConverter {
        kind: ConverterKind::MicrosoftOffice,
        family,
        executable_path: PathBuf::from(
            status
                .path
                .ok_or_else(|| "Microsoft Office executable path is missing".to_string())?,
        ),
        version: status.version,
    })
}

fn select_libreoffice(
    runtime: &RuntimeContext,
    family: OfficeFamily,
) -> Result<SelectedConverter, String> {
    let status = detect_system_libreoffice();
    if status.available {
        return Ok(SelectedConverter {
            kind: ConverterKind::LibreOfficeSystem,
            family,
            executable_path: PathBuf::from(
                status
                    .path
                    .ok_or_else(|| "LibreOffice executable path is missing".to_string())?,
            ),
            version: status.version,
        });
    }
    if let Some(path) = ensure_bundled_libreoffice(runtime)? {
        return Ok(SelectedConverter {
            kind: ConverterKind::LibreOfficeBundled,
            family,
            executable_path: path.clone(),
            version: command_version(&path).or_else(|| Some(DEFAULT_LIBREOFFICE_VERSION.to_string())),
        });
    }
    Err(status
        .reason
        .unwrap_or_else(|| "LibreOffice is unavailable".to_string()))
}

fn detect_microsoft_office_for_family(family: OfficeFamily) -> ConverterStatus {
    #[cfg(target_os = "windows")]
    {
        let executable_names = match family {
            OfficeFamily::Word => ["WINWORD.EXE"].as_slice(),
            OfficeFamily::Spreadsheet => ["EXCEL.EXE"].as_slice(),
            OfficeFamily::Presentation => ["POWERPNT.EXE"].as_slice(),
        };
        let mut candidates = Vec::new();
        for root in [
            r"C:\Program Files\Microsoft Office\root\Office16",
            r"C:\Program Files\Microsoft Office\Office16",
            r"C:\Program Files (x86)\Microsoft Office\root\Office16",
            r"C:\Program Files (x86)\Microsoft Office\Office16",
        ] {
            for executable_name in executable_names {
                candidates.push(PathBuf::from(root).join(executable_name));
            }
        }
        for candidate in candidates {
            if candidate.is_file() {
                return ConverterStatus {
                    available: true,
                    path: Some(candidate.to_string_lossy().to_string()),
                    version: candidate
                        .parent()
                        .and_then(|parent| parent.file_name())
                        .map(|value| value.to_string_lossy().to_string())
                        .or_else(|| Some("Office16".to_string())),
                    reason: None,
                };
            }
        }
    }
    ConverterStatus {
        available: false,
        path: None,
        version: None,
        reason: Some("未探测到适用于当前文档类型的 Microsoft Office 安装。".to_string()),
    }
}

fn detect_system_libreoffice() -> ConverterStatus {
    let candidates = if cfg!(target_os = "windows") {
        vec![
            PathBuf::from(r"C:\Program Files\LibreOffice\program\soffice.exe"),
            PathBuf::from(r"C:\Program Files (x86)\LibreOffice\program\soffice.exe"),
        ]
    } else {
        vec![PathBuf::from("/usr/bin/libreoffice"), PathBuf::from("/usr/bin/soffice")]
    };
    for candidate in candidates {
        if candidate.is_file() {
            return ConverterStatus {
                available: true,
                path: Some(candidate.to_string_lossy().to_string()),
                version: command_version(&candidate),
                reason: None,
            };
        }
    }
    ConverterStatus {
        available: false,
        path: None,
        version: None,
        reason: Some("未探测到系统 LibreOffice 安装。".to_string()),
    }
}

fn detect_bundled_libreoffice(runtime: &RuntimeContext) -> ConverterStatus {
    let runtime_dir = bundled_libreoffice_runtime_dir(runtime);
    let executable = bundled_libreoffice_executable_path(runtime);
    let installer = bundled_libreoffice_installer_path(runtime);
    if executable.is_file() {
        return ConverterStatus {
            available: true,
            path: Some(executable.to_string_lossy().to_string()),
            version: command_version(&executable)
                .or_else(|| Some(DEFAULT_LIBREOFFICE_VERSION.to_string())),
            reason: None,
        };
    }
    if installer.is_file() {
        return ConverterStatus {
            available: false,
            path: Some(installer.to_string_lossy().to_string()),
            version: Some(DEFAULT_LIBREOFFICE_VERSION.to_string()),
            reason: Some(format!(
                "已下载 LibreOffice 安装包，等待解压到 {}。",
                runtime_dir.display()
            )),
        };
    }
    ConverterStatus {
        available: false,
        path: None,
        version: None,
        reason: Some("未发现已下载的 LibreOffice 运行时。".to_string()),
    }
}

fn ensure_bundled_libreoffice(runtime: &RuntimeContext) -> Result<Option<PathBuf>, String> {
    let executable = bundled_libreoffice_executable_path(runtime);
    if executable.is_file() {
        return Ok(Some(executable));
    }
    if !runtime.config.auto_download_libreoffice {
        return Ok(None);
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("当前仅支持在 Windows 上自动下载自带 LibreOffice。".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        let installer_path = bundled_libreoffice_installer_path(runtime);
        download_runtime_via_downloader(runtime, DEFAULT_LIBREOFFICE_DOWNLOAD_URL, &installer_path)?;
        extract_libreoffice_installer(runtime, &installer_path)?;
        if executable.is_file() {
            return Ok(Some(executable));
        }
        Err(format!(
            "LibreOffice 安装包已处理，但未找到可执行文件：{}",
            executable.display()
        ))
    }
}

#[cfg(target_os = "windows")]
fn extract_libreoffice_installer(
    runtime: &RuntimeContext,
    installer_path: &Path,
) -> Result<(), String> {
    let runtime_dir = bundled_libreoffice_runtime_dir(runtime);
    fs::create_dir_all(&runtime_dir).map_err(io_error)?;
    let output = run_command(
        with_no_window(
            Command::new("msiexec.exe")
                .arg("/a")
                .arg(installer_path)
                .arg("/qn")
                .arg(format!("TARGETDIR={}", runtime_dir.display())),
        ),
        "extract bundled LibreOffice runtime",
    )?;
    if !output.status.success() {
        return Err(format!(
            "failed to extract bundled LibreOffice runtime: {}",
            command_output_message(&output)
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn extract_libreoffice_installer(
    _runtime: &RuntimeContext,
    _installer_path: &Path,
) -> Result<(), String> {
    Err("当前仅支持在 Windows 上解压自带 LibreOffice 运行时。".to_string())
}

fn download_runtime_via_downloader(
    runtime: &RuntimeContext,
    url: &str,
    target_path: &Path,
) -> Result<(), String> {
    if target_path.is_file() {
        return Ok(());
    }
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let plugin_runtime = PluginRuntimeContext {
        plugin_id: "momobako.service.office-convert".to_string(),
        plugin_data_dir: runtime.plugin_data_dir.to_string_lossy().to_string(),
        service_root_dir: runtime.service_root_dir.to_string_lossy().to_string(),
        plugin_config: Default::default(),
    };
    let _ = call_host_plugin(
        &plugin_runtime,
        DOWNLOADER_PLUGIN_ID,
        "downloader.ensureRuntime",
        serde_json::json!({}),
    )?;
    let queued = call_host_plugin(
        &plugin_runtime,
        DOWNLOADER_PLUGIN_ID,
        "downloader.enqueueDownload",
        serde_json::json!({
            "url": url,
            "destinationPath": target_path.to_string_lossy().to_string(),
            "metadata": {
                "kind": "office-runtime",
                "pluginId": "momobako.service.office-convert",
                "runtime": "libreoffice",
                "version": DEFAULT_LIBREOFFICE_VERSION
            }
        }),
    )?;
    let task_id = queued
        .get("taskId")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "downloader.enqueueDownload 未返回 taskId".to_string())?;
    let record = call_host_plugin(
        &plugin_runtime,
        DOWNLOADER_PLUGIN_ID,
        "downloader.awaitDownload",
        serde_json::json!({
            "taskId": task_id
        }),
    )?;
    let destination_path = record
        .get("destinationPath")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !Path::new(destination_path.as_str()).is_file() {
        return Err(format!(
            "LibreOffice runtime download completed but destination file is missing: {}",
            destination_path
        ));
    }
    Ok(())
}

fn convert_with_microsoft_office(
    converter: &SelectedConverter,
    source_path: &Path,
    pdf_path: &Path,
) -> Result<(), String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (converter, source_path, pdf_path);
        return Err("Microsoft Office 转换仅支持 Windows。".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        let helper_dir = pdf_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let script_path = helper_dir.join("office-convert.ps1");
        fs::write(&script_path, microsoft_office_script(converter.family)).map_err(io_error)?;
        let output = run_command(
            with_no_window(
                Command::new("powershell.exe")
                    .arg("-NoProfile")
                    .arg("-NonInteractive")
                    .arg("-ExecutionPolicy")
                    .arg("Bypass")
                    .arg("-File")
                    .arg(&script_path)
                    .arg(source_path)
                    .arg(pdf_path),
            ),
            "convert office document with Microsoft Office",
        )?;
        if !output.status.success() || !pdf_path.is_file() {
            return Err(format!(
                "Microsoft Office 转换失败：{}",
                command_output_message(&output)
            ));
        }
        Ok(())
    }
}

fn convert_with_libreoffice(
    runtime: &RuntimeContext,
    converter: &SelectedConverter,
    source_path: &Path,
    pdf_path: &Path,
    temp_dir: &Path,
) -> Result<(), String> {
    ensure_libreoffice_daemon(runtime, &converter.executable_path)?;
    write_libreoffice_conversion_status(
        runtime,
        source_path,
        pdf_path,
        "running",
        None,
    )?;
    let output_dir = temp_dir.join(format!(
        "pdf-{}",
        preview_cache_temp_key(source_path, pdf_path)
    ));
    if output_dir.is_dir() {
        let _ = fs::remove_dir_all(&output_dir);
    }
    fs::create_dir_all(&output_dir).map_err(io_error)?;
    let profile_dir = libreoffice_profile_dir(runtime);
    let output = run_command(
        with_no_window(
            Command::new(&converter.executable_path)
                .arg("--headless")
                .arg("--nologo")
                .arg("--nofirststartwizard")
                .arg("--convert-to")
                .arg("pdf")
                .arg("--outdir")
                .arg(&output_dir)
                .arg(source_path)
                .arg(format!(
                    "-env:UserInstallation={}",
                    libreoffice_profile_uri(&profile_dir)
                )),
        ),
        "convert office document with LibreOffice",
    )?;
    let generated_path = output_dir.join(format!(
        "{}.pdf",
        source_path
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("invalid source file name: {}", source_path.display()))?
    ));
    if !output.status.success() || !generated_path.is_file() {
        write_libreoffice_conversion_status(
            runtime,
            source_path,
            pdf_path,
            "failed",
            Some(command_output_message(&output)),
        )?;
        return Err(format!(
            "LibreOffice 转换失败：{}",
            command_output_message(&output)
        ));
    }
    fs::copy(&generated_path, pdf_path).map_err(io_error)?;
    write_libreoffice_conversion_status(
        runtime,
        source_path,
        pdf_path,
        "completed",
        None,
    )?;
    Ok(())
}

fn ensure_libreoffice_daemon(runtime: &RuntimeContext, executable_path: &Path) -> Result<(), String> {
    let helper_dir = libreoffice_helper_dir(runtime);
    fs::create_dir_all(&helper_dir).map_err(io_error)?;
    let pid_path = helper_dir.join("pid.txt");
    if let Some(pid) = read_pid(&pid_path)? {
        if process_is_running(pid) {
            write_libreoffice_status(runtime, Some(pid), executable_path, true, None)?;
            return Ok(());
        }
        cleanup_stale_libreoffice_state(&runtime.plugin_data_dir)?;
    }
    let profile_dir = libreoffice_profile_dir(runtime);
    fs::create_dir_all(&profile_dir).map_err(io_error)?;
    let child = {
        let mut command = Command::new(executable_path);
        with_no_window(
            command
                .arg("--headless")
                .arg("--nologo")
                .arg("--nodefault")
                .arg("--nofirststartwizard")
                .arg("--norestore")
                .arg("--invisible")
                .arg("--accept=pipe,name=momobako-office-convert;urp;StarOffice.ComponentContext")
                .arg(format!(
                    "-env:UserInstallation={}",
                    libreoffice_profile_uri(&profile_dir)
                ))
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .spawn()
        .map_err(|error| format!("failed to start LibreOffice daemon: {error}"))?
    };
    let pid = child.id();
    fs::write(&pid_path, pid.to_string()).map_err(io_error)?;
    thread::sleep(Duration::from_secs(2));
    let running = process_is_running(pid);
    write_libreoffice_status(runtime, Some(pid), executable_path, running, None)?;
    if running {
        Ok(())
    } else {
        Err("LibreOffice 守护进程启动失败。".to_string())
    }
}

fn read_helper_status(path: &Path) -> serde_json::Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .unwrap_or_else(|| {
            serde_json::json!({
                "running": false,
                "pid": null
            })
        })
}

fn current_libreoffice_daemon_status(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let status_path = libreoffice_helper_dir(runtime).join("status.json");
    let pid_path = libreoffice_helper_dir(runtime).join("pid.txt");
    let mut status = read_helper_status(&status_path);
    let pid = read_pid(&pid_path)?;
    let running = pid.map(process_is_running).unwrap_or(false);
    if let Some(object) = status.as_object_mut() {
        object.insert("running".to_string(), serde_json::Value::Bool(running));
        object.insert(
            "healthy".to_string(),
            serde_json::Value::Bool(running),
        );
        object.insert(
            "control".to_string(),
            serde_json::json!({
                "health": "pid-status",
                "shutdown": "plugin-call"
            }),
        );
        if !running {
            object.insert(
                "error".to_string(),
                serde_json::Value::String("LibreOffice 守护进程未运行。".to_string()),
            );
        }
    }
    if let Some(path) = status.get("path").and_then(serde_json::Value::as_str) {
        write_libreoffice_status(
            runtime,
            pid,
            Path::new(path),
            running,
            if running {
                None
            } else {
                Some("LibreOffice 守护进程未运行。".to_string())
            },
        )?;
        return Ok(read_helper_status(&status_path));
    }
    Ok(status)
}

fn shutdown_libreoffice_daemon(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let helper_dir = libreoffice_helper_dir(runtime);
    let pid_path = helper_dir.join("pid.txt");
    let status_path = helper_dir.join("status.json");
    let pid = read_pid(&pid_path)?;
    let Some(pid) = pid else {
        let _ = fs::remove_file(status_path);
        return Ok(serde_json::json!({
            "stopped": false,
            "reason": "daemon-not-running"
        }));
    };
    stop_process(pid)?;
    let _ = fs::remove_file(pid_path);
    let status = serde_json::json!({
        "running": false,
        "healthy": false,
        "pid": pid,
        "path": read_helper_status(&status_path).get("path").cloned().unwrap_or(serde_json::Value::Null),
        "updatedAt": OffsetDateTime::now_utc().format(&Rfc3339).map_err(time_error)?,
        "error": "LibreOffice 守护进程已关闭。",
        "control": {
            "health": "pid-status",
            "shutdown": "plugin-call"
        }
    });
    fs::write(
        &status_path,
        serde_json::to_string_pretty(&status).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)?;
    Ok(serde_json::json!({
        "stopped": true,
        "pid": pid
    }))
}

fn write_libreoffice_status(
    runtime: &RuntimeContext,
    pid: Option<u32>,
    executable_path: &Path,
    running: bool,
    error: Option<String>,
) -> Result<(), String> {
    let status_path = libreoffice_helper_dir(runtime).join("status.json");
    let updated_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(time_error)?;
    let payload = serde_json::json!({
        "running": running,
        "pid": pid,
        "path": executable_path.to_string_lossy().to_string(),
        "updatedAt": updated_at,
        "error": error,
    });
    fs::write(
        status_path,
        serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

fn write_libreoffice_conversion_status(
    runtime: &RuntimeContext,
    source_path: &Path,
    pdf_path: &Path,
    phase: &str,
    error: Option<String>,
) -> Result<(), String> {
    let status_path = libreoffice_helper_dir(runtime).join("status.json");
    let mut status = read_helper_status(&status_path);
    let updated_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(time_error)?;
    let last_convert = serde_json::json!({
        "phase": phase,
        "sourcePath": source_path.to_string_lossy().to_string(),
        "pdfPath": pdf_path.to_string_lossy().to_string(),
        "updatedAt": updated_at,
        "error": error,
    });
    let phase_error = last_convert
        .get("error")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::String("LibreOffice 转换失败。".to_string()));
    if let Some(object) = status.as_object_mut() {
        object.insert("lastConvert".to_string(), last_convert);
        if phase == "failed" {
            object.insert("error".to_string(), phase_error);
        } else if phase == "completed" {
            object.insert("error".to_string(), serde_json::Value::Null);
        }
    }
    fs::write(
        status_path,
        serde_json::to_string_pretty(&status).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

fn cleanup_stale_libreoffice_state(plugin_data_dir: &Path) -> Result<(), String> {
    let helper_dir = plugin_data_dir.join("helpers").join("libreoffice");
    let pid_path = helper_dir.join("pid.txt");
    let status_path = helper_dir.join("status.json");
    if let Some(pid) = read_pid(&pid_path)? {
        if process_is_running(pid) {
            return Ok(());
        }
    }
    let _ = fs::remove_file(pid_path);
    let _ = fs::remove_file(status_path);
    Ok(())
}

fn read_pid(path: &Path) -> Result<Option<u32>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(io_error)?;
    Ok(raw.trim().parse::<u32>().ok())
}

fn process_is_running(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output();
        let Ok(output) = output else {
            return false;
        };
        if !output.status.success() {
            return false;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains(&format!(",\"{pid}\"")) || stdout.contains(&pid.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

fn stop_process(pid: u32) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .map_err(|error| format!("failed to stop LibreOffice daemon: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        return Err(format!(
            "failed to stop LibreOffice daemon: {}",
            command_output_message(&output)
        ));
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .map_err(|error| format!("failed to stop LibreOffice daemon: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        Err(format!(
            "failed to stop LibreOffice daemon: {}",
            command_output_message(&output)
        ))
    }
}

fn normalize_service_root_dir(value: String, plugin_data_dir: &Path) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        return Ok(PathBuf::from(trimmed));
    }
    plugin_data_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "unable to infer service root directory".to_string())
}

fn office_family_from_extension(extension: &str) -> Option<OfficeFamily> {
    match extension {
        "doc" | "docx" | "docm" | "dot" | "dotx" | "dotm" => Some(OfficeFamily::Word),
        "xls" | "xlsx" | "xlsm" | "xlsb" | "xlt" | "xltx" | "xltm" => {
            Some(OfficeFamily::Spreadsheet)
        }
        "ppt" | "pptx" | "pptm" | "pps" | "ppsx" | "ppsm" | "pot" | "potx" | "potm" => {
            Some(OfficeFamily::Presentation)
        }
        _ => None,
    }
}

fn preview_cache_temp_key(source_path: &Path, pdf_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_path.to_string_lossy().as_bytes());
    hasher.update(b"\n");
    hasher.update(pdf_path.to_string_lossy().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn remove_files_recursively(root: &Path) -> Result<i64, String> {
    let mut removed = 0_i64;
    for entry in fs::read_dir(root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if path.is_dir() {
            removed += remove_files_recursively(&path)?;
            let _ = fs::remove_dir(&path);
            continue;
        }
        if path.is_file() {
            fs::remove_file(&path).map_err(io_error)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn bundled_libreoffice_installer_path(runtime: &RuntimeContext) -> PathBuf {
    runtime.plugin_data_dir.join("downloads").join(format!(
        "LibreOffice_{DEFAULT_LIBREOFFICE_VERSION}_Win_x86-64.msi"
    ))
}

fn bundled_libreoffice_runtime_dir(runtime: &RuntimeContext) -> PathBuf {
    runtime.plugin_data_dir.join("downloads").join("LibreOffice")
}

fn bundled_libreoffice_executable_path(runtime: &RuntimeContext) -> PathBuf {
    bundled_libreoffice_runtime_dir(runtime)
        .join("program")
        .join(if cfg!(target_os = "windows") {
            "soffice.exe"
        } else {
            "soffice"
        })
}

fn libreoffice_helper_dir(runtime: &RuntimeContext) -> PathBuf {
    runtime.plugin_data_dir.join("helpers").join("libreoffice")
}

fn libreoffice_profile_dir(runtime: &RuntimeContext) -> PathBuf {
    libreoffice_helper_dir(runtime).join("profile")
}

fn libreoffice_profile_uri(path: &Path) -> String {
    format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
}

#[cfg(target_os = "windows")]
fn microsoft_office_script(family: OfficeFamily) -> &'static str {
    match family {
        OfficeFamily::Word => {
            r#"
$inputPath = [System.IO.Path]::GetFullPath($args[0])
$outputPath = [System.IO.Path]::GetFullPath($args[1])
$word = $null
$doc = $null
try {
  $word = New-Object -ComObject Word.Application
  $word.Visible = $false
  $word.DisplayAlerts = 0
  $doc = $word.Documents.Open($inputPath, $false, $true)
  $doc.ExportAsFixedFormat($outputPath, 17)
} finally {
  if ($doc -ne $null) { $doc.Close($false) | Out-Null }
  if ($word -ne $null) { $word.Quit() }
}
"#
        }
        OfficeFamily::Spreadsheet => {
            r#"
$inputPath = [System.IO.Path]::GetFullPath($args[0])
$outputPath = [System.IO.Path]::GetFullPath($args[1])
$excel = $null
$workbook = $null
try {
  $excel = New-Object -ComObject Excel.Application
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $workbook = $excel.Workbooks.Open($inputPath, 0, $true)
  $workbook.ExportAsFixedFormat(0, $outputPath)
} finally {
  if ($workbook -ne $null) { $workbook.Close($false) | Out-Null }
  if ($excel -ne $null) { $excel.Quit() }
}
"#
        }
        OfficeFamily::Presentation => {
            r#"
$inputPath = [System.IO.Path]::GetFullPath($args[0])
$outputPath = [System.IO.Path]::GetFullPath($args[1])
$powerpoint = $null
$presentation = $null
try {
  $powerpoint = New-Object -ComObject PowerPoint.Application
  $presentation = $powerpoint.Presentations.Open($inputPath, $false, $false, $false)
  $presentation.SaveAs($outputPath, 32)
} finally {
  if ($presentation -ne $null) { $presentation.Close() }
  if ($powerpoint -ne $null) { $powerpoint.Quit() }
}
"#
        }
    }
}

fn run_command(command: &mut Command, description: &str) -> Result<Output, String> {
    command
        .output()
        .map_err(|error| format!("{description} failed: {error}"))
}

#[cfg(target_os = "windows")]
fn with_no_window(command: &mut Command) -> &mut Command {
    command.creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(target_os = "windows"))]
fn with_no_window(command: &mut Command) -> &mut Command {
    command
}

fn command_output_message(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    if !stdout.is_empty() {
        return stdout;
    }
    format!("process exited with status {}", output.status)
}

fn system_time_to_rfc3339(value: std::time::SystemTime) -> Result<String, String> {
    let value: OffsetDateTime = value.into();
    value.format(&Rfc3339).map_err(time_error)
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

fn time_error(error: impl ToString) -> String {
    error.to_string()
}

fn command_version(path: &Path) -> Option<String> {
    Command::new(path)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

impl SelectedConverter {
    fn cache_key_label(&self) -> &'static str {
        match self.kind {
            ConverterKind::MicrosoftOffice => "microsoft-office",
            ConverterKind::LibreOfficeSystem => "libreoffice",
            ConverterKind::LibreOfficeBundled => "libreoffice-bundled",
        }
    }

    fn result_label(&self) -> &'static str {
        self.cache_key_label()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn office_family_from_extension_supports_legacy_and_openxml_formats() {
        assert_eq!(office_family_from_extension("doc"), Some(OfficeFamily::Word));
        assert_eq!(office_family_from_extension("docx"), Some(OfficeFamily::Word));
        assert_eq!(office_family_from_extension("xls"), Some(OfficeFamily::Spreadsheet));
        assert_eq!(office_family_from_extension("xlsx"), Some(OfficeFamily::Spreadsheet));
        assert_eq!(office_family_from_extension("ppt"), Some(OfficeFamily::Presentation));
        assert_eq!(office_family_from_extension("pptx"), Some(OfficeFamily::Presentation));
        assert_eq!(office_family_from_extension("txt"), None);
    }

    #[test]
    fn preview_cache_key_changes_when_converter_version_changes() {
        let source_path = PathBuf::from("C:/Repo/demo.docx");
        let left = preview_cache_key(
            "repo-demo",
            "Docs/demo.docx",
            &source_path,
            123,
            "2026-07-01T08:00:00Z",
            "docx",
            "libreoffice",
            "24.2",
        );
        let right = preview_cache_key(
            "repo-demo",
            "Docs/demo.docx",
            &source_path,
            123,
            "2026-07-01T08:00:00Z",
            "docx",
            "libreoffice",
            "25.8",
        );
        assert_ne!(left, right);
    }

    #[test]
    fn repository_cache_dir_uses_plugin_namespace() {
        let repo_root = PathBuf::from("C:/Repo");
        assert_eq!(
            repository_cache_dir(&repo_root),
            PathBuf::from("C:/Repo/.momo/cache/office-preview")
        );
    }

    #[test]
    fn runtime_context_keeps_service_root_for_host_plugin_bridge() {
        let runtime = RuntimeContext {
            plugin_data_dir: PathBuf::from("C:/Service/plugin-data/momobako-service-office-convert"),
            service_root_dir: PathBuf::from("C:/Service"),
            config: PluginConfig {
                converter_mode: ConverterMode::Auto,
                auto_download_libreoffice: true,
            },
        };
        assert_eq!(
            runtime.service_root_dir,
            PathBuf::from("C:/Service")
        );
    }
}

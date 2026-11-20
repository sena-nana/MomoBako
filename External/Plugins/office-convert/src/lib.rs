//! Office 预览转换插件。
//!
//! 负责把 Office 文档转换为 PDF 并缓存到资源库缓存目录。

use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::Duration,
};

use momobako_mutsuki_plugin_sdk::{
    export_mutsuki_momobako_plugin, write_host_log_silently, PluginCallEnvelope,
    PluginRuntimeContext,
};
use reqwest::blocking::Client;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

mod detect;
mod helper;
mod libreoffice;
mod preview;
mod support;

use detect::*;
use helper::*;
use libreoffice::*;
use preview::*;
use support::*;

const REGISTRY_FILE_NAME: &str = "repositories.db";
const REPO_META_DIR: &str = ".momo";
const OFFICE_CACHE_NAMESPACE: &str = "office-preview";
const DEFAULT_LIBREOFFICE_VERSION: &str = "25.8.3";
const DEFAULT_LIBREOFFICE_DOWNLOAD_URL: &str =
    "https://download.documentfoundation.org/libreoffice/stable/25.8.3/win/x86_64/LibreOffice_25.8.3_Win_x86-64.msi";
const OFFICE_CONVERT_PLUGIN_ID: &str = "momobako.service.office-convert";
const LIBREOFFICE_HELPER_HEALTH_TIMEOUT_SECS: u64 = 3;
const LIBREOFFICE_HELPER_STARTUP_RETRIES: usize = 20;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug)]
struct RuntimeContext {
    plugin_data_dir: PathBuf,
    service_root_dir: PathBuf,
    plugin_runtime_dir: PathBuf,
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

#[derive(Debug, Clone, Serialize)]
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

export_mutsuki_momobako_plugin!(
    "momobako.service.office-convert",
    "0.1.0",
    protocols = [
        "officeConvert.ensurePreviewPdf",
        "officeConvert.getRuntimeStatus",
        "officeConvert.clearPreviewCache",
        "officeConvert.runRuntimeSelfCheck",
        "officeConvert.shutdownDaemon",
    ],
    requires = ["momobako.service.downloader"],
    permissions = ["filesystem:read", "filesystem:write", "network", "process"],
    handle_call
);

fn handle_call(request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    let runtime = runtime_context(request.runtime)?;

    match request.method.as_str() {
        "officeConvert.ensurePreviewPdf" => {
            let payload: EnsurePreviewPdfPayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            ensure_preview_pdf(&runtime, payload)
        }
        "officeConvert.getRuntimeStatus" => {
            serde_json::to_value(get_runtime_status(&runtime)?).map_err(|error| error.to_string())
        }
        "officeConvert.clearPreviewCache" => {
            let payload: ClearPreviewCachePayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            clear_preview_cache(&runtime, payload)
        }
        "officeConvert.runRuntimeSelfCheck" => run_runtime_self_check(&runtime),
        "officeConvert.shutdownDaemon" => shutdown_libreoffice_daemon(&runtime),
        method => Err(format!("unsupported method: {method}")),
    }
}

fn runtime_context(runtime: PluginRuntimeContext) -> Result<RuntimeContext, String> {
    let plugin_data_dir = PathBuf::from(runtime.plugin_data_dir.clone());
    let service_root_dir =
        normalize_service_root_dir(runtime.service_root_dir.clone(), plugin_data_dir.as_path())?;
    let plugin_runtime_dir = PathBuf::from(runtime.plugin_runtime_dir.clone());
    fs::create_dir_all(plugin_data_dir.join("helpers").join("libreoffice")).map_err(io_error)?;
    fs::create_dir_all(plugin_data_dir.join("downloads")).map_err(io_error)?;
    cleanup_stale_libreoffice_state(&plugin_data_dir, &runtime)?;
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
        plugin_runtime_dir,
        config,
    })
}

fn host_runtime_for_log(runtime: &RuntimeContext) -> PluginRuntimeContext {
    PluginRuntimeContext {
        plugin_id: OFFICE_CONVERT_PLUGIN_ID.to_string(),
        plugin_data_dir: runtime.plugin_data_dir.to_string_lossy().to_string(),
        service_root_dir: runtime.service_root_dir.to_string_lossy().to_string(),
        plugin_runtime_dir: runtime.plugin_runtime_dir.to_string_lossy().to_string(),
        plugin_config: Default::default(),
    }
}

fn write_runtime_warning<T: Serialize>(
    runtime: &RuntimeContext,
    action: &str,
    message: &str,
    context: T,
) {
    write_host_log_silently(
        &host_runtime_for_log(runtime),
        "warn",
        action,
        message,
        context,
    );
}

fn remove_file_with_warning(runtime: &RuntimeContext, path: &Path, action: &str, message: &str) {
    if !path.is_file() {
        return;
    }
    if let Err(error) = fs::remove_file(path) {
        write_runtime_warning(
            runtime,
            action,
            message,
            serde_json::json!({
                "path": path.to_string_lossy().to_string(),
                "error": error.to_string(),
            }),
        );
    }
}

fn remove_dir_all_with_warning(runtime: &RuntimeContext, path: &Path, action: &str, message: &str) {
    if !path.is_dir() {
        return;
    }
    if let Err(error) = fs::remove_dir_all(path) {
        write_runtime_warning(
            runtime,
            action,
            message,
            serde_json::json!({
                "path": path.to_string_lossy().to_string(),
                "error": error.to_string(),
            }),
        );
    }
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
mod test_support {
    use super::{ConverterStatus, OfficeFamily};
    use std::{cell::RefCell, path::PathBuf};

    thread_local! {
        static TEST_MICROSOFT_OFFICE: RefCell<Option<Option<ConverterStatus>>> = const { RefCell::new(None) };
        static TEST_MICROSOFT_OFFICE_WORD: RefCell<Option<Option<ConverterStatus>>> = const { RefCell::new(None) };
        static TEST_MICROSOFT_OFFICE_SPREADSHEET: RefCell<Option<Option<ConverterStatus>>> = const { RefCell::new(None) };
        static TEST_MICROSOFT_OFFICE_PRESENTATION: RefCell<Option<Option<ConverterStatus>>> = const { RefCell::new(None) };
        static TEST_SYSTEM_LIBREOFFICE: RefCell<Option<Option<ConverterStatus>>> = const { RefCell::new(None) };
        static TEST_BUNDLED_LIBREOFFICE: RefCell<Option<Option<ConverterStatus>>> = const { RefCell::new(None) };
        static TEST_ENSURED_BUNDLED_PATH: RefCell<Option<Option<PathBuf>>> = const { RefCell::new(None) };
    }

    fn test_status_from(
        cell: &'static std::thread::LocalKey<RefCell<Option<Option<ConverterStatus>>>>,
    ) -> Option<Option<ConverterStatus>> {
        cell.with(|value| value.borrow().clone())
    }

    fn set_test_option(
        cell: &'static std::thread::LocalKey<RefCell<Option<Option<ConverterStatus>>>>,
        value: Option<ConverterStatus>,
    ) {
        cell.with(|slot| {
            *slot.borrow_mut() = Some(value);
        });
    }

    pub fn reset_test_detector_overrides() {
        TEST_MICROSOFT_OFFICE.with(|slot| *slot.borrow_mut() = None);
        TEST_MICROSOFT_OFFICE_WORD.with(|slot| *slot.borrow_mut() = None);
        TEST_MICROSOFT_OFFICE_SPREADSHEET.with(|slot| *slot.borrow_mut() = None);
        TEST_MICROSOFT_OFFICE_PRESENTATION.with(|slot| *slot.borrow_mut() = None);
        TEST_SYSTEM_LIBREOFFICE.with(|slot| *slot.borrow_mut() = None);
        TEST_BUNDLED_LIBREOFFICE.with(|slot| *slot.borrow_mut() = None);
        TEST_ENSURED_BUNDLED_PATH.with(|slot| *slot.borrow_mut() = None);
    }

    pub fn set_test_microsoft_office_family_status(
        family: OfficeFamily,
        value: Option<ConverterStatus>,
    ) {
        let cell = match family {
            OfficeFamily::Word => &TEST_MICROSOFT_OFFICE_WORD,
            OfficeFamily::Spreadsheet => &TEST_MICROSOFT_OFFICE_SPREADSHEET,
            OfficeFamily::Presentation => &TEST_MICROSOFT_OFFICE_PRESENTATION,
        };
        set_test_option(cell, value);
    }

    pub fn set_test_system_libreoffice_status(value: Option<ConverterStatus>) {
        set_test_option(&TEST_SYSTEM_LIBREOFFICE, value);
    }

    pub fn set_test_bundled_libreoffice_status(value: Option<ConverterStatus>) {
        set_test_option(&TEST_BUNDLED_LIBREOFFICE, value);
    }

    pub fn set_test_ensured_bundled_libreoffice_path(value: Option<PathBuf>) {
        TEST_ENSURED_BUNDLED_PATH.with(|slot| {
            *slot.borrow_mut() = Some(value);
        });
    }

    pub fn test_detect_microsoft_office() -> Option<ConverterStatus> {
        test_status_from(&TEST_MICROSOFT_OFFICE).flatten()
    }

    pub fn test_detect_microsoft_office_for_family(
        family: OfficeFamily,
    ) -> Option<ConverterStatus> {
        let cell = match family {
            OfficeFamily::Word => &TEST_MICROSOFT_OFFICE_WORD,
            OfficeFamily::Spreadsheet => &TEST_MICROSOFT_OFFICE_SPREADSHEET,
            OfficeFamily::Presentation => &TEST_MICROSOFT_OFFICE_PRESENTATION,
        };
        test_status_from(cell).flatten()
    }

    pub fn test_detect_system_libreoffice() -> Option<ConverterStatus> {
        test_status_from(&TEST_SYSTEM_LIBREOFFICE).flatten()
    }

    pub fn test_detect_bundled_libreoffice() -> Option<ConverterStatus> {
        test_status_from(&TEST_BUNDLED_LIBREOFFICE).flatten()
    }

    pub fn test_ensure_bundled_libreoffice_path() -> Option<Option<PathBuf>> {
        TEST_ENSURED_BUNDLED_PATH.with(|slot| slot.borrow().clone())
    }
}

#[cfg(test)]
mod tests;

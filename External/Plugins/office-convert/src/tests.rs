//! Office 转换插件的单元测试：转换器选择、预览缓存、运行时自检与辅助进程状态。

use super::*;
use crate::test_support;
use rusqlite::params;
use std::time::{SystemTime, UNIX_EPOCH};

fn unavailable_status(reason: &str) -> ConverterStatus {
    ConverterStatus {
        available: false,
        path: None,
        version: None,
        reason: Some(reason.to_string()),
    }
}

fn available_status(path: &str, version: &str) -> ConverterStatus {
    ConverterStatus {
        available: true,
        path: Some(path.to_string()),
        version: Some(version.to_string()),
        reason: None,
    }
}

fn reset_test_detector_overrides() {
    test_support::reset_test_detector_overrides();
}

fn set_test_microsoft_office_family_status(family: OfficeFamily, value: Option<ConverterStatus>) {
    test_support::set_test_microsoft_office_family_status(family, value);
}

fn runtime_for_converter_test(workspace: &TestWorkspace, mode: ConverterMode) -> RuntimeContext {
    RuntimeContext {
        plugin_data_dir: workspace.path("plugin-data"),
        service_root_dir: workspace.path("service-root"),
        plugin_runtime_dir: workspace.path("runtime"),
        config: PluginConfig {
            converter_mode: mode,
            auto_download_libreoffice: true,
        },
    }
}

struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "momobako-office-convert-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("test workspace root should be created");
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

struct DetectorOverrideGuard;

impl DetectorOverrideGuard {
    fn new() -> Self {
        reset_test_detector_overrides();
        Self
    }
}

impl Drop for DetectorOverrideGuard {
    fn drop(&mut self) {
        reset_test_detector_overrides();
    }
}

#[test]
fn office_family_from_extension_supports_legacy_and_openxml_formats() {
    assert_eq!(
        office_family_from_extension("doc"),
        Some(OfficeFamily::Word)
    );
    assert_eq!(
        office_family_from_extension("docx"),
        Some(OfficeFamily::Word)
    );
    assert_eq!(
        office_family_from_extension("xls"),
        Some(OfficeFamily::Spreadsheet)
    );
    assert_eq!(
        office_family_from_extension("xlsx"),
        Some(OfficeFamily::Spreadsheet)
    );
    assert_eq!(
        office_family_from_extension("ppt"),
        Some(OfficeFamily::Presentation)
    );
    assert_eq!(
        office_family_from_extension("pptx"),
        Some(OfficeFamily::Presentation)
    );
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
fn converter_mode_from_config_supports_all_declared_values() {
    assert!(matches!(
        ConverterMode::from_config(Some("auto")),
        ConverterMode::Auto
    ));
    assert!(matches!(
        ConverterMode::from_config(Some("microsoft-office")),
        ConverterMode::MicrosoftOffice
    ));
    assert!(matches!(
        ConverterMode::from_config(Some("libreoffice")),
        ConverterMode::LibreOffice
    ));
    assert!(matches!(
        ConverterMode::from_config(Some("unexpected-value")),
        ConverterMode::Auto
    ));
}

#[test]
fn auto_mode_prefers_microsoft_office_before_libreoffice() {
    let _guard = DetectorOverrideGuard::new();
    let workspace = TestWorkspace::new("auto-prefers-microsoft");
    let runtime = runtime_for_converter_test(&workspace, ConverterMode::Auto);
    set_test_microsoft_office_family_status(
        OfficeFamily::Word,
        Some(available_status(
            "C:/Program Files/Microsoft Office/root/Office16/WINWORD.EXE",
            "Office16",
        )),
    );
    test_support::set_test_system_libreoffice_status(Some(available_status(
        "C:/Program Files/LibreOffice/program/soffice.exe",
        "25.8.3",
    )));

    let selected = select_converter(&runtime, OfficeFamily::Word)
        .expect("auto mode should choose microsoft office");

    assert_eq!(selected.kind, ConverterKind::MicrosoftOffice);
    assert_eq!(
        selected.executable_path,
        PathBuf::from("C:/Program Files/Microsoft Office/root/Office16/WINWORD.EXE")
    );
}

#[test]
fn auto_mode_falls_back_to_system_libreoffice_when_microsoft_office_is_missing() {
    let _guard = DetectorOverrideGuard::new();
    let workspace = TestWorkspace::new("auto-falls-back-system-libreoffice");
    let runtime = runtime_for_converter_test(&workspace, ConverterMode::Auto);
    set_test_microsoft_office_family_status(
        OfficeFamily::Word,
        Some(unavailable_status(
            "未探测到适用于当前文档类型的 Microsoft Office 安装。",
        )),
    );
    test_support::set_test_system_libreoffice_status(Some(available_status(
        "C:/Program Files/LibreOffice/program/soffice.exe",
        "25.8.3",
    )));

    let selected = select_converter(&runtime, OfficeFamily::Word)
        .expect("auto mode should choose system libreoffice");

    assert_eq!(selected.kind, ConverterKind::LibreOfficeSystem);
    assert_eq!(
        selected.executable_path,
        PathBuf::from("C:/Program Files/LibreOffice/program/soffice.exe")
    );
}

#[test]
fn auto_mode_falls_back_to_bundled_libreoffice_when_system_converters_are_missing() {
    let _guard = DetectorOverrideGuard::new();
    let workspace = TestWorkspace::new("auto-falls-back-bundled-libreoffice");
    let runtime = runtime_for_converter_test(&workspace, ConverterMode::Auto);
    set_test_microsoft_office_family_status(
        OfficeFamily::Word,
        Some(unavailable_status(
            "未探测到适用于当前文档类型的 Microsoft Office 安装。",
        )),
    );
    test_support::set_test_system_libreoffice_status(Some(unavailable_status(
        "未探测到系统 LibreOffice 安装。",
    )));
    test_support::set_test_bundled_libreoffice_status(
        Some(available_status(
            "C:/MomoBako/.service-data/plugin-data/momobako-service-office-convert/runtime/program/soffice.exe",
            "25.8.3",
        )),
    );
    test_support::set_test_ensured_bundled_libreoffice_path(Some(PathBuf::from(
        "C:/MomoBako/.service-data/plugin-data/momobako-service-office-convert/runtime/program/soffice.exe",
    )));

    let selected = select_converter(&runtime, OfficeFamily::Word)
        .expect("auto mode should choose bundled libreoffice");

    assert_eq!(selected.kind, ConverterKind::LibreOfficeBundled);
    assert_eq!(
        selected.executable_path,
        PathBuf::from(
            "C:/MomoBako/.service-data/plugin-data/momobako-service-office-convert/runtime/program/soffice.exe"
        )
    );
}

#[test]
fn normalize_repository_entry_path_rejects_parent_segments() {
    assert!(normalize_repository_entry_path("../Secrets/demo.docx").is_err());
    assert!(normalize_repository_entry_path("Docs/./demo.docx").is_err());
}

#[test]
fn resolve_preview_source_path_falls_back_to_repo_relative_entry_path() {
    let workspace = TestWorkspace::new("resolve-preview-source");
    let service_root = workspace.path("service-root");
    let plugin_data_dir = workspace.path("plugin-data");
    let repo_root = workspace.path("repo");
    fs::create_dir_all(&service_root).expect("service root should be created");
    fs::create_dir_all(&plugin_data_dir).expect("plugin data dir should be created");
    fs::create_dir_all(repo_root.join("Docs")).expect("repo docs dir should be created");
    let source_path = repo_root.join("Docs").join("demo.docx");
    fs::write(&source_path, b"docx").expect("source file should be written");

    let connection = Connection::open(service_root.join(REGISTRY_FILE_NAME))
        .expect("registry should be created");
    connection
        .execute(
            "CREATE TABLE repositories (repo_id TEXT PRIMARY KEY, path TEXT NOT NULL)",
            [],
        )
        .expect("repositories table should be created");
    connection
        .execute(
            "INSERT INTO repositories (repo_id, path) VALUES (?1, ?2)",
            params!["repo-demo", repo_root.to_string_lossy().to_string()],
        )
        .expect("repository row should be inserted");

    let runtime = RuntimeContext {
        plugin_data_dir,
        service_root_dir: service_root,
        plugin_runtime_dir: workspace.path("runtime"),
        config: PluginConfig {
            converter_mode: ConverterMode::Auto,
            auto_download_libreoffice: true,
        },
    };
    let payload = EnsurePreviewPdfPayload {
        repo_id: "repo-demo".to_string(),
        entry_path: "Docs/demo.docx".to_string(),
        extension: "docx".to_string(),
        source_path: None,
        source_modified_at: None,
        source_size_bytes: None,
    };

    assert_eq!(
        resolve_preview_source_path(&runtime, &payload).expect("source path should resolve"),
        source_path
    );
}

#[test]
fn ensure_preview_pdf_returns_cached_true_when_matching_pdf_already_exists() {
    let _guard = DetectorOverrideGuard::new();
    let workspace = TestWorkspace::new("ensure-preview-pdf-cache-hit");
    let service_root = workspace.path("service-root");
    let plugin_data_dir = workspace.path("plugin-data");
    let repo_root = workspace.path("repo");
    let docs_dir = repo_root.join("Docs");
    fs::create_dir_all(&service_root).expect("service root should be created");
    fs::create_dir_all(&plugin_data_dir).expect("plugin data dir should be created");
    fs::create_dir_all(&docs_dir).expect("docs dir should be created");
    let source_path = docs_dir.join("demo.docx");
    fs::write(&source_path, b"docx").expect("source file should be written");

    let connection = Connection::open(service_root.join(REGISTRY_FILE_NAME))
        .expect("registry should be created");
    connection
        .execute(
            "CREATE TABLE repositories (repo_id TEXT PRIMARY KEY, path TEXT NOT NULL)",
            [],
        )
        .expect("repositories table should be created");
    connection
        .execute(
            "INSERT INTO repositories (repo_id, path) VALUES (?1, ?2)",
            params!["repo-demo", repo_root.to_string_lossy().to_string()],
        )
        .expect("repository row should be inserted");

    let runtime = RuntimeContext {
        plugin_data_dir,
        service_root_dir: service_root,
        plugin_runtime_dir: workspace.path("runtime"),
        config: PluginConfig {
            converter_mode: ConverterMode::Auto,
            auto_download_libreoffice: true,
        },
    };

    set_test_microsoft_office_family_status(
        OfficeFamily::Word,
        Some(available_status(
            "C:/Program Files/Microsoft Office/root/Office16/WINWORD.EXE",
            "Office16",
        )),
    );

    let source_metadata = fs::metadata(&source_path).expect("source metadata should exist");
    let source_modified_at = source_metadata
        .modified()
        .ok()
        .and_then(|value| system_time_to_rfc3339(value).ok())
        .expect("source modified time should resolve");
    let cache_key = preview_cache_key(
        "repo-demo",
        "Docs/demo.docx",
        &source_path,
        source_metadata.len() as i64,
        &source_modified_at,
        "docx",
        "microsoft-office",
        "Office16",
    );
    let pdf_path = repository_cache_dir(&repo_root).join(format!("{cache_key}.pdf"));
    fs::create_dir_all(pdf_path.parent().expect("cache dir should exist"))
        .expect("cache dir should be created");
    fs::write(&pdf_path, b"%PDF-1.4 cached").expect("cached pdf should be written");

    let value = ensure_preview_pdf(
        &runtime,
        EnsurePreviewPdfPayload {
            repo_id: "repo-demo".to_string(),
            entry_path: "Docs/demo.docx".to_string(),
            extension: "docx".to_string(),
            source_path: Some(source_path.to_string_lossy().to_string()),
            source_modified_at: Some(source_modified_at),
            source_size_bytes: Some(source_metadata.len() as i64),
        },
    )
    .expect("ensure preview pdf should reuse cache");

    assert_eq!(value["cached"], serde_json::json!(true));
    assert_eq!(value["converter"], serde_json::json!("microsoft-office"));
    assert_eq!(
        value["pdfPath"],
        serde_json::json!(pdf_path.to_string_lossy().to_string())
    );
    assert_eq!(value["cacheKey"], serde_json::json!(cache_key));
    assert_eq!(value["sizeBytes"], serde_json::json!(15));
}

#[test]
fn ensure_preview_pdf_recomputes_cache_key_when_source_metadata_changes() {
    let _guard = DetectorOverrideGuard::new();
    let workspace = TestWorkspace::new("ensure-preview-pdf-cache-miss-on-source-change");
    let service_root = workspace.path("service-root");
    let plugin_data_dir = workspace.path("plugin-data");
    let repo_root = workspace.path("repo");
    let docs_dir = repo_root.join("Docs");
    fs::create_dir_all(&service_root).expect("service root should be created");
    fs::create_dir_all(&plugin_data_dir).expect("plugin data dir should be created");
    fs::create_dir_all(&docs_dir).expect("docs dir should be created");
    let source_path = docs_dir.join("demo.docx");
    fs::write(&source_path, b"docx").expect("source file should be written");

    let connection = Connection::open(service_root.join(REGISTRY_FILE_NAME))
        .expect("registry should be created");
    connection
        .execute(
            "CREATE TABLE repositories (repo_id TEXT PRIMARY KEY, path TEXT NOT NULL)",
            [],
        )
        .expect("repositories table should be created");
    connection
        .execute(
            "INSERT INTO repositories (repo_id, path) VALUES (?1, ?2)",
            params!["repo-demo", repo_root.to_string_lossy().to_string()],
        )
        .expect("repository row should be inserted");

    let runtime = RuntimeContext {
        plugin_data_dir,
        service_root_dir: service_root,
        plugin_runtime_dir: workspace.path("runtime"),
        config: PluginConfig {
            converter_mode: ConverterMode::Auto,
            auto_download_libreoffice: true,
        },
    };

    set_test_microsoft_office_family_status(
        OfficeFamily::Word,
        Some(available_status(
            "C:/Program Files/Microsoft Office/root/Office16/WINWORD.EXE",
            "Office16",
        )),
    );

    let original_cache_key = preview_cache_key(
        "repo-demo",
        "Docs/demo.docx",
        &source_path,
        4,
        "2026-07-01T08:00:00Z",
        "docx",
        "microsoft-office",
        "Office16",
    );
    let original_pdf_path =
        repository_cache_dir(&repo_root).join(format!("{original_cache_key}.pdf"));
    fs::create_dir_all(original_pdf_path.parent().expect("cache dir should exist"))
        .expect("cache dir should be created");
    fs::write(&original_pdf_path, b"%PDF-1.4 cached")
        .expect("original cached pdf should be written");

    let changed_modified_at = "2026-07-02T09:30:00Z".to_string();
    let changed_size_bytes = 6_i64;
    let changed_cache_key = preview_cache_key(
        "repo-demo",
        "Docs/demo.docx",
        &source_path,
        changed_size_bytes,
        &changed_modified_at,
        "docx",
        "microsoft-office",
        "Office16",
    );
    let changed_pdf_path =
        repository_cache_dir(&repo_root).join(format!("{changed_cache_key}.pdf"));
    fs::write(&changed_pdf_path, b"%PDF-1.4 new").expect("changed cached pdf should be written");

    let value = ensure_preview_pdf(
        &runtime,
        EnsurePreviewPdfPayload {
            repo_id: "repo-demo".to_string(),
            entry_path: "Docs/demo.docx".to_string(),
            extension: "docx".to_string(),
            source_path: Some(source_path.to_string_lossy().to_string()),
            source_modified_at: Some(changed_modified_at),
            source_size_bytes: Some(changed_size_bytes),
        },
    )
    .expect("ensure preview pdf should use the changed cache key");

    assert_ne!(original_cache_key, changed_cache_key);
    assert_eq!(value["cached"], serde_json::json!(true));
    assert_eq!(
        value["pdfPath"],
        serde_json::json!(changed_pdf_path.to_string_lossy().to_string())
    );
    assert_eq!(value["cacheKey"], serde_json::json!(changed_cache_key));
    assert_ne!(
        value["pdfPath"],
        serde_json::json!(original_pdf_path.to_string_lossy().to_string())
    );
}

#[test]
fn explicit_microsoft_office_mode_returns_diagnostic_error_when_office_is_missing() {
    let _guard = DetectorOverrideGuard::new();
    let workspace = TestWorkspace::new("missing-microsoft-office");
    let runtime = runtime_for_converter_test(&workspace, ConverterMode::MicrosoftOffice);

    set_test_microsoft_office_family_status(
        OfficeFamily::Word,
        Some(unavailable_status(
            "未探测到适用于当前文档类型的 Microsoft Office 安装。",
        )),
    );

    let error = select_converter(&runtime, OfficeFamily::Word)
        .expect_err("explicit microsoft office mode should fail when office is missing");

    assert_eq!(
        error,
        "未探测到适用于当前文档类型的 Microsoft Office 安装。"
    );
}

#[test]
fn explicit_libreoffice_mode_returns_diagnostic_error_when_libreoffice_is_missing() {
    let _guard = DetectorOverrideGuard::new();
    let workspace = TestWorkspace::new("missing-libreoffice");
    let runtime = runtime_for_converter_test(&workspace, ConverterMode::LibreOffice);

    test_support::set_test_system_libreoffice_status(Some(unavailable_status(
        "未探测到系统 LibreOffice 安装。",
    )));
    test_support::set_test_bundled_libreoffice_status(Some(unavailable_status(
        "未发现已下载的 LibreOffice 运行时。",
    )));
    test_support::set_test_ensured_bundled_libreoffice_path(None);

    let error = select_converter(&runtime, OfficeFamily::Word)
        .expect_err("explicit libreoffice mode should fail when libreoffice is missing");

    assert_eq!(error, "未探测到系统 LibreOffice 安装。");
}

#[test]
fn clear_preview_cache_removes_files_only_under_repository_cache_namespace() {
    let workspace = TestWorkspace::new("clear-preview-cache");
    let service_root = workspace.path("service-root");
    let plugin_data_dir = workspace.path("plugin-data");
    let repo_root = workspace.path("repo");
    let cache_dir = repository_cache_dir(&repo_root);
    let nested_dir = cache_dir.join("nested");
    let sibling_file = repo_root.join("outside.pdf");
    fs::create_dir_all(&service_root).expect("service root should be created");
    fs::create_dir_all(&plugin_data_dir).expect("plugin data dir should be created");
    fs::create_dir_all(&nested_dir).expect("cache dir should be created");
    fs::write(cache_dir.join("preview-a.pdf"), b"%PDF-1.4").expect("cache file should be written");
    fs::write(nested_dir.join("preview-b.pdf"), b"%PDF-1.4")
        .expect("nested cache file should be written");
    fs::write(&sibling_file, b"%PDF-1.4").expect("outside file should be written");

    let connection = Connection::open(service_root.join(REGISTRY_FILE_NAME))
        .expect("registry should be created");
    connection
        .execute(
            "CREATE TABLE repositories (repo_id TEXT PRIMARY KEY, path TEXT NOT NULL)",
            [],
        )
        .expect("repositories table should be created");
    connection
        .execute(
            "INSERT INTO repositories (repo_id, path) VALUES (?1, ?2)",
            params!["repo-demo", repo_root.to_string_lossy().to_string()],
        )
        .expect("repository row should be inserted");

    let runtime = RuntimeContext {
        plugin_data_dir,
        service_root_dir: service_root,
        plugin_runtime_dir: workspace.path("runtime"),
        config: PluginConfig {
            converter_mode: ConverterMode::Auto,
            auto_download_libreoffice: true,
        },
    };

    let value = clear_preview_cache(
        &runtime,
        ClearPreviewCachePayload {
            repo_id: "repo-demo".to_string(),
        },
    )
    .expect("cache clear should succeed");

    assert_eq!(value["repoId"], serde_json::json!("repo-demo"));
    assert_eq!(value["removed"], serde_json::json!(2));
    assert!(!cache_dir.join("preview-a.pdf").exists());
    assert!(!nested_dir.join("preview-b.pdf").exists());
    assert!(sibling_file.exists());
}

#[test]
fn runtime_context_keeps_service_root_for_host_plugin_bridge() {
    let runtime = RuntimeContext {
        plugin_data_dir: PathBuf::from("C:/Service/plugin-data/momobako-service-office-convert"),
        service_root_dir: PathBuf::from("C:/Service"),
        plugin_runtime_dir: PathBuf::from(
            "C:/Service/runtime/plugins/momobako-service-office-convert",
        ),
        config: PluginConfig {
            converter_mode: ConverterMode::Auto,
            auto_download_libreoffice: true,
        },
    };
    assert_eq!(runtime.service_root_dir, PathBuf::from("C:/Service"));
}

#[test]
fn runtime_status_response_keeps_mode_and_download_configuration_fields() {
    let workspace = TestWorkspace::new("runtime-status-shape");
    let runtime = RuntimeContext {
        plugin_data_dir: workspace.path("plugin-data"),
        service_root_dir: workspace.path("service-root"),
        plugin_runtime_dir: workspace.path("runtime"),
        config: PluginConfig {
            converter_mode: ConverterMode::LibreOffice,
            auto_download_libreoffice: false,
        },
    };

    let status = get_runtime_status(&runtime).expect("runtime status should resolve");

    assert_eq!(status.converter_mode, "libreoffice");
    assert!(!status.auto_download_libre_office);
    assert_eq!(
        status.bundled_download_url,
        DEFAULT_LIBREOFFICE_DOWNLOAD_URL
    );
    assert!(status.microsoft_office.reason.is_some() || status.microsoft_office.available);
    assert!(status.libreoffice_system.reason.is_some() || status.libreoffice_system.available);
    assert!(status.libreoffice_bundle.reason.is_some() || status.libreoffice_bundle.available);
    assert!(status.daemon.is_object());
}

#[test]
fn bundled_helper_path_prefers_runtime_sidecar_when_present() {
    let workspace = TestWorkspace::new("bundled-helper");
    let helper_name = if cfg!(target_os = "windows") {
        "office-convert-helper.exe"
    } else {
        "office-convert-helper"
    };
    let helper_path = workspace.path(helper_name);
    fs::write(&helper_path, b"helper").expect("helper sidecar should be written");
    let runtime = RuntimeContext {
        plugin_data_dir: workspace.path("plugin-data"),
        service_root_dir: workspace.path("service-root"),
        plugin_runtime_dir: workspace.root.clone(),
        config: PluginConfig {
            converter_mode: ConverterMode::Auto,
            auto_download_libreoffice: true,
        },
    };

    assert_eq!(bundled_helper_path(&runtime), Some(helper_path));
}

#[test]
fn helper_type_label_reports_native_http_when_bundled_helper_exists() {
    let workspace = TestWorkspace::new("helper-type-native");
    let helper_name = if cfg!(target_os = "windows") {
        "office-convert-helper.exe"
    } else {
        "office-convert-helper"
    };
    fs::write(workspace.path(helper_name), b"helper").expect("helper should be written");
    let runtime = RuntimeContext {
        plugin_data_dir: workspace.path("plugin-data"),
        service_root_dir: workspace.path("service-root"),
        plugin_runtime_dir: workspace.root.clone(),
        config: PluginConfig {
            converter_mode: ConverterMode::Auto,
            auto_download_libreoffice: true,
        },
    };

    if cfg!(target_os = "windows") {
        assert_eq!(helper_type_label(&runtime), "native-http");
    } else {
        assert_eq!(helper_type_label(&runtime), "native-cli-fallback");
    }
}

#[test]
fn helper_type_label_reports_fallback_without_bundled_helper() {
    let workspace = TestWorkspace::new("helper-type-fallback");
    let runtime = RuntimeContext {
        plugin_data_dir: workspace.path("plugin-data"),
        service_root_dir: workspace.path("service-root"),
        plugin_runtime_dir: workspace.path("runtime"),
        config: PluginConfig {
            converter_mode: ConverterMode::Auto,
            auto_download_libreoffice: true,
        },
    };

    if cfg!(target_os = "windows") {
        assert_eq!(helper_type_label(&runtime), "powershell-http");
    } else {
        assert_eq!(helper_type_label(&runtime), "native-cli-fallback");
    }
}

#[test]
fn helper_runtime_status_reads_soffice_fields() {
    let payload = serde_json::json!({
        "ok": true,
        "runtime": "office-convert-helper",
        "sofficeReady": true,
        "sofficePid": 4567,
        "unoAvailable": true,
        "pythonValid": true,
        "pythonPath": "C:/LibreOffice/program/python.exe"
    });
    let status = HelperRuntimeStatus {
        healthy: true,
        soffice_ready: payload
            .get("sofficeReady")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        soffice_pid: payload
            .get("sofficePid")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        uno_available: payload
            .get("unoAvailable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        python_valid: payload
            .get("pythonValid")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        python_path: payload
            .get("pythonPath")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    };

    assert!(status.healthy);
    assert!(status.soffice_ready);
    assert_eq!(status.soffice_pid, Some(4567));
    assert!(status.uno_available);
    assert!(status.python_valid);
    assert_eq!(
        status.python_path.as_deref(),
        Some("C:/LibreOffice/program/python.exe")
    );
}

#[test]
fn libreoffice_conversion_status_keeps_conversion_mode() {
    let last_convert = serde_json::json!({
        "phase": "completed",
        "conversionMode": "uno",
        "sourcePath": "C:/Repo/demo.docx",
        "pdfPath": "C:/Repo/demo.pdf",
    });

    assert_eq!(
        last_convert.get("conversionMode"),
        Some(&serde_json::json!("uno"))
    );
}

#[test]
fn minimal_docx_writer_creates_docx_package() {
    let workspace = TestWorkspace::new("self-check-docx");
    let path = workspace.path("self-check.docx");
    write_minimal_docx(&path).expect("self-check docx should write");
    let file = fs::File::open(&path).expect("docx should exist");
    let mut archive = zip::ZipArchive::new(file).expect("docx should be a zip archive");
    assert!(archive.by_name("[Content_Types].xml").is_ok());
    assert!(archive.by_name("word/document.xml").is_ok());
}

#[test]
fn runtime_self_check_payload_keeps_diagnostic_fields() {
    let payload = serde_json::json!({
        "ok": true,
        "converter": "libreoffice-bundled",
        "converterPath": "C:/LibreOffice/program/soffice.exe",
        "converterVersion": "25.8.3",
        "conversionMode": "uno",
        "samplePath": "C:/tmp/self-check.docx",
        "pdfPath": "C:/tmp/self-check.pdf",
        "pdfSizeBytes": 2048,
        "durationMs": 1234
    });

    assert_eq!(
        payload.get("converterPath"),
        Some(&serde_json::json!("C:/LibreOffice/program/soffice.exe"))
    );
    assert_eq!(payload.get("pdfSizeBytes"), Some(&serde_json::json!(2048)));
    assert_eq!(payload.get("durationMs"), Some(&serde_json::json!(1234)));
}

#[test]
fn write_libreoffice_status_keeps_existing_diagnostics() {
    let workspace = TestWorkspace::new("status-merge");
    let runtime = RuntimeContext {
        plugin_data_dir: workspace.path("plugin-data"),
        service_root_dir: workspace.path("service-root"),
        plugin_runtime_dir: workspace.path("runtime"),
        config: PluginConfig {
            converter_mode: ConverterMode::Auto,
            auto_download_libreoffice: true,
        },
    };
    let helper_dir = libreoffice_helper_dir(&runtime);
    fs::create_dir_all(&helper_dir).expect("helper dir should be created");
    let status_path = helper_dir.join("status.json");
    fs::write(
        &status_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "lastConvert": { "conversionMode": "uno" },
            "lastSelfCheck": { "ok": true }
        }))
        .expect("status json should encode"),
    )
    .expect("status json should write");

    write_libreoffice_status(
        &runtime,
        Some(123),
        Path::new("C:/LibreOffice/program/soffice.exe"),
        true,
        None,
    )
    .expect("status merge should succeed");

    let merged: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(status_path).expect("merged status should read"))
            .expect("merged status should decode");
    assert_eq!(
        merged
            .get("lastConvert")
            .and_then(|value| value.get("conversionMode")),
        Some(&serde_json::json!("uno"))
    );
    assert_eq!(
        merged
            .get("lastSelfCheck")
            .and_then(|value| value.get("ok")),
        Some(&serde_json::json!(true))
    );
    assert_eq!(merged.get("pid"), Some(&serde_json::json!(123)));
}

//! 预览 PDF 生成、运行时状态与自检、预览缓存清理、转换器选择与仓库缓存目录。

use super::*;

pub(super) fn ensure_preview_pdf(
    runtime: &RuntimeContext,
    payload: EnsurePreviewPdfPayload,
) -> Result<serde_json::Value, String> {
    let source_path = resolve_preview_source_path(runtime, &payload)?;
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

pub(super) fn resolve_preview_source_path(
    runtime: &RuntimeContext,
    payload: &EnsurePreviewPdfPayload,
) -> Result<PathBuf, String> {
    if let Some(source_path) = payload
        .source_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(source_path));
    }
    let repo_root = repository_root_for_id(runtime, &payload.repo_id)?;
    let normalized_entry_path = normalize_repository_entry_path(&payload.entry_path)?;
    Ok(repo_root.join(normalized_entry_path))
}

pub(super) fn normalize_repository_entry_path(path: &str) -> Result<PathBuf, String> {
    let normalized = path.trim().replace('\\', "/");
    let normalized = normalized.trim_matches('/');
    if normalized.is_empty() {
        return Err("entryPath is required".to_string());
    }
    let mut resolved = PathBuf::new();
    for segment in normalized.split('/') {
        let segment = segment.trim();
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(format!("invalid entryPath: {path}"));
        }
        resolved.push(segment);
    }
    Ok(resolved)
}

pub(super) fn get_runtime_status(
    runtime: &RuntimeContext,
) -> Result<RuntimeStatusResponse, String> {
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

pub(super) fn run_runtime_self_check(
    runtime: &RuntimeContext,
) -> Result<serde_json::Value, String> {
    let helper_dir = libreoffice_helper_dir(runtime);
    fs::create_dir_all(&helper_dir).map_err(io_error)?;
    let sample_dir = helper_dir.join("self-check");
    if sample_dir.is_dir() {
        remove_dir_all_with_warning(
            runtime,
            &sample_dir,
            "runtimeSelfCheckCleanupFailed",
            "Office 转换自检目录清理失败。",
        );
    }
    fs::create_dir_all(&sample_dir).map_err(io_error)?;
    let source_path = sample_dir.join("self-check.docx");
    let pdf_path = sample_dir.join("self-check.pdf");
    write_minimal_docx(&source_path)?;
    let converter = select_converter(runtime, OfficeFamily::Word)?;
    let temp_dir = sample_dir.join("temp");
    fs::create_dir_all(&temp_dir).map_err(io_error)?;
    let started_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(time_error)?;
    let result = match converter.kind {
        ConverterKind::MicrosoftOffice => {
            convert_with_microsoft_office(&converter, &source_path, &pdf_path).map(|_| None)
        }
        ConverterKind::LibreOfficeSystem | ConverterKind::LibreOfficeBundled => {
            convert_with_libreoffice(runtime, &converter, &source_path, &pdf_path, &temp_dir)
                .map(|_| latest_conversion_mode(runtime))
        }
    };
    let completed_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(time_error)?;
    let duration_ms = (OffsetDateTime::now_utc()
        - OffsetDateTime::parse(&started_at, &Rfc3339).map_err(time_error)?)
    .whole_milliseconds();
    let pdf_size_bytes = fs::metadata(&pdf_path).ok().map(|value| value.len() as i64);
    let (ok, error, conversion_mode) = match result {
        Ok(mode) => (pdf_path.is_file(), None, mode),
        Err(error) => (false, Some(error), None),
    };
    write_runtime_self_check_status(
        runtime,
        serde_json::json!({
            "startedAt": started_at,
            "completedAt": completed_at,
            "durationMs": duration_ms,
            "ok": ok,
            "converter": converter.result_label(),
            "converterPath": converter.executable_path.to_string_lossy().to_string(),
            "converterVersion": converter.version.clone(),
            "conversionMode": conversion_mode,
            "samplePath": source_path.to_string_lossy().to_string(),
            "pdfPath": if ok { Some(pdf_path.to_string_lossy().to_string()) } else { None::<String> },
            "pdfSizeBytes": pdf_size_bytes,
            "error": error,
        }),
    )?;
    Ok(serde_json::json!({
        "ok": ok,
        "converter": converter.result_label(),
        "converterPath": converter.executable_path.to_string_lossy().to_string(),
        "converterVersion": converter.version.clone(),
        "conversionMode": conversion_mode,
        "samplePath": source_path.to_string_lossy().to_string(),
        "pdfPath": if ok { Some(pdf_path.to_string_lossy().to_string()) } else { None::<String> },
        "pdfSizeBytes": pdf_size_bytes,
        "durationMs": duration_ms,
        "error": error,
    }))
}

pub(super) fn detect_microsoft_office() -> ConverterStatus {
    #[cfg(test)]
    if let Some(status) = test_support::test_detect_microsoft_office() {
        return status;
    }
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

pub(super) fn clear_preview_cache(
    runtime: &RuntimeContext,
    payload: ClearPreviewCachePayload,
) -> Result<serde_json::Value, String> {
    let cache_dir = repository_cache_dir_for_repo(runtime, &payload.repo_id)?;
    let mut removed = 0;
    if cache_dir.is_dir() {
        removed = remove_files_recursively(runtime, &cache_dir)?;
    }
    Ok(serde_json::json!({
        "repoId": payload.repo_id,
        "removed": removed
    }))
}

/// 按策略选择真实可执行的转换器。
pub(super) fn select_converter(
    runtime: &RuntimeContext,
    family: OfficeFamily,
) -> Result<SelectedConverter, String> {
    match runtime.config.converter_mode {
        ConverterMode::MicrosoftOffice => select_microsoft_office(family),
        ConverterMode::LibreOffice => select_libreoffice(runtime, family),
        ConverterMode::Auto => {
            if let Ok(converter) = select_microsoft_office(family) {
                return Ok(converter);
            }
            select_libreoffice(runtime, family)
        }
    }
}

pub(super) fn preview_cache_key(
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
pub(super) fn convert_office_to_pdf(
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

pub(super) fn repository_cache_dir_for_repo(
    runtime: &RuntimeContext,
    repo_id: &str,
) -> Result<PathBuf, String> {
    let repo_root = repository_root_for_id(runtime, repo_id)?;
    Ok(repository_cache_dir(&repo_root))
}

pub(super) fn repository_root_for_id(
    runtime: &RuntimeContext,
    repo_id: &str,
) -> Result<PathBuf, String> {
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

pub(super) fn repository_cache_dir(repo_root: &Path) -> PathBuf {
    repo_root
        .join(REPO_META_DIR)
        .join("cache")
        .join(OFFICE_CACHE_NAMESPACE)
}

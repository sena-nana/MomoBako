//! 插件包的校验、解压安装与清单读取。

use super::*;

pub(super) fn ensure_runtime_plugin_archive(
    service_root: &Path,
    archive_path: &Path,
) -> Result<(), String> {
    let runtime_root = runtime_plugins_dir(service_root);
    fs::create_dir_all(&runtime_root).map_err(io_error)?;
    let runtime_root = runtime_root.canonicalize().map_err(io_error)?;
    let archive_path = archive_path.canonicalize().map_err(io_error)?;
    if archive_path.starts_with(&runtime_root) {
        Ok(())
    } else {
        Err(format!(
            "plugin archive is outside the runtime plugin root: {}",
            archive_path.display()
        ))
    }
}

pub(super) fn install_plugin_archive(
    service_root: &Path,
    package_path: &Path,
) -> Result<PluginManifest, String> {
    if package_path.as_os_str().is_empty() {
        return Err("plugin package path cannot be empty".to_string());
    }
    if package_path.extension().and_then(|value| value.to_str()) != Some("momoplug") {
        return Err("plugin package must use the .momoplug extension".to_string());
    }
    if !package_path.is_file() {
        return Err(format!(
            "plugin package not found: {}",
            package_path.display()
        ));
    }

    let (manifest, _) = read_validated_v2_plugin_archive(package_path, "user-installed")?;
    if manifest.plugin_id.trim().is_empty() {
        return Err("plugin manifest is missing pluginId".to_string());
    }

    let existing_registry = plugin_catalog(service_root);
    if existing_registry.manifest(&manifest.plugin_id).is_some() {
        return Err(format!("plugin already exists: {}", manifest.plugin_id));
    }

    let runtime_root = runtime_plugins_dir(service_root).join("user");
    fs::create_dir_all(&runtime_root).map_err(io_error)?;
    let install_name = format!(
        "{}-{}.momoplug",
        slugify_ascii_component(&manifest.plugin_id),
        manifest.version
    );
    let target_path = runtime_root.join(install_name);
    if target_path.exists() {
        return Err(format!(
            "plugin package already exists: {}",
            target_path.display()
        ));
    }
    fs::copy(package_path, &target_path).map_err(io_error)?;
    let mut installed = manifest;
    installed.archive_path = Some(target_path.to_string_lossy().to_string());
    Ok(installed)
}

pub(super) fn find_zip_plugin_manifest<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<(usize, String), String> {
    let mut matches = Vec::new();
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|error| error.to_string())?
            .name()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        if let Some(prefix) = name.strip_suffix("/manifest.json") {
            if !prefix.is_empty() && !prefix.split('/').any(|part| part == "..") {
                matches.push((index, format!("{prefix}/")));
            }
        }
    }
    if matches.len() == 1 {
        return Ok(matches.remove(0));
    }
    if matches.is_empty() {
        return Err(
            "plugin archive must contain exactly one root directory with manifest.json".to_string(),
        );
    }
    Err("plugin archive contains multiple manifest roots".to_string())
}

/// 校验 v2 包信封、目标平台与所有声明产物，并由宿主写入来源和信任元数据。
pub(in super::super) fn read_validated_v2_plugin_archive(
    archive_path: &Path,
    provenance: &str,
) -> Result<(PluginManifest, String), String> {
    let file = File::open(archive_path).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let (manifest_index, manifest_prefix) = find_zip_plugin_manifest(&mut archive)?;
    let mut manifest_raw = String::new();
    archive
        .by_index(manifest_index)
        .map_err(|error| error.to_string())?
        .read_to_string(&mut manifest_raw)
        .map_err(io_error)?;
    let envelope_path = format!("{manifest_prefix}{PLUGIN_PACKAGE_MANIFEST}");
    let mut envelope_raw = String::new();
    archive
        .by_name(&envelope_path)
        .map_err(|_| format!("plugin package requires {PLUGIN_PACKAGE_MANIFEST}"))?
        .read_to_string(&mut envelope_raw)
        .map_err(io_error)?;
    let envelope =
        serde_json::from_str::<PluginPackageEnvelope>(&envelope_raw).map_err(json_error)?;
    let mut manifest = parse_plugin_manifest_with_source(&manifest_raw, None)?;
    validate_plugin_package_envelope(&mut archive, &manifest_prefix, &envelope, &manifest)?;
    if provenance == "user-installed" && envelope.deployment == "abi" {
        return Err(
            "user-installed native plugins must use process deployment; in-process ABI is reserved for bundled plugins"
                .to_string(),
        );
    }

    let (source, trust_level) = match provenance {
        "bundled" => ("builtin", "trusted"),
        "user-installed" => ("user", "untrusted"),
        _ => ("system", "legacy"),
    };
    manifest.source = source.to_string();
    manifest.package_format_version = Some(envelope.format_version);
    manifest.package_hash = Some(sha256_path(archive_path)?);
    manifest.provenance = Some(provenance.to_string());
    manifest.trust_level = Some(trust_level.to_string());
    manifest.deployment = Some(envelope.deployment);
    manifest.target_triple = Some(envelope.target_triple);
    Ok((manifest, manifest_prefix))
}

pub(super) fn validate_plugin_package_envelope<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    prefix: &str,
    envelope: &PluginPackageEnvelope,
    manifest: &PluginManifest,
) -> Result<(), String> {
    if envelope.format_version != PLUGIN_PACKAGE_FORMAT_VERSION {
        return Err(format!(
            "unsupported plugin package format: {}",
            envelope.format_version
        ));
    }
    if envelope.plugin_id != manifest.plugin_id || envelope.version != manifest.version {
        return Err("plugin package envelope identity mismatch".to_string());
    }
    if envelope.product_manifest != "manifest.json" {
        return Err("plugin package productManifest must be manifest.json".to_string());
    }
    let expected_deployment = match manifest.runtime.as_str() {
        "native-dylib" => "abi",
        "process" => "process",
        _ => "manifest",
    };
    if envelope.deployment != expected_deployment {
        return Err(format!(
            "plugin package deployment mismatch: expected {expected_deployment}, got {}",
            envelope.deployment
        ));
    }
    if matches!(expected_deployment, "abi" | "process") {
        if envelope.target_triple != env!("MOMO_TARGET_TRIPLE") {
            return Err(format!(
                "plugin target mismatch: expected {}, got {}",
                env!("MOMO_TARGET_TRIPLE"),
                envelope.target_triple
            ));
        }
        if envelope.runtime_manifest.as_deref() != Some("plugin.toml") {
            return Err("executable plugin package requires plugin.toml".to_string());
        }
    }
    if expected_deployment != "manifest" && envelope.artifacts.is_empty() {
        return Err("executable package requires declared artifacts".to_string());
    }

    let mut paths = HashSet::new();
    for artifact in &envelope.artifacts {
        if artifact.role.trim().is_empty() || !paths.insert(artifact.path.as_str()) {
            return Err(format!(
                "invalid or duplicate package artifact: {}",
                artifact.path
            ));
        }
        let relative_path = safe_zip_relative_path(&artifact.path)?;
        let archive_path = format!(
            "{prefix}{}",
            relative_path.to_string_lossy().replace('\\', "/")
        );
        let mut entry = archive
            .by_name(&archive_path)
            .map_err(|_| format!("plugin artifact is missing: {}", artifact.path))?;
        if entry.is_dir() {
            return Err(format!("plugin artifact is not a file: {}", artifact.path));
        }
        let expected = artifact
            .sha256
            .strip_prefix("sha256:")
            .filter(|value| {
                value.len() == 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
            .ok_or_else(|| format!("invalid plugin artifact hash: {}", artifact.sha256))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = entry.read(&mut buffer).map_err(io_error)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let actual = hex::encode(hasher.finalize());
        if actual != expected {
            return Err(format!("plugin artifact hash mismatch: {}", artifact.path));
        }
        if expected_deployment == "process" && artifact.role == "runner" && !artifact.executable {
            return Err("process runner artifact must be marked executable".to_string());
        }
    }
    Ok(())
}

pub(in super::super) fn sha256_path(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

#[allow(dead_code)]
pub(crate) fn extract_zip_plugin<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    manifest_prefix: &str,
    staging_dir: &Path,
) -> Result<(), String> {
    let prefix = manifest_prefix.trim_start_matches('/').replace('\\', "/");
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let entry_name = entry
            .name()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        if !prefix.is_empty() && !entry_name.starts_with(&prefix) {
            continue;
        }
        let relative_name = if prefix.is_empty() {
            entry_name.as_str()
        } else {
            &entry_name[prefix.len()..]
        };
        if relative_name.is_empty() {
            continue;
        }
        let relative_path = safe_zip_relative_path(relative_name)?;
        let output_path = staging_dir.join(&relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&output_path).map_err(io_error)?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut output = File::create(&output_path).map_err(io_error)?;
        std::io::copy(&mut entry, &mut output).map_err(io_error)?;
        output.flush().map_err(io_error)?;
    }
    Ok(())
}

pub(in super::super) fn safe_zip_relative_path(value: &str) -> Result<PathBuf, String> {
    let mut path = PathBuf::new();
    for part in value.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') || part.contains('\\') {
            return Err(format!("unsafe plugin archive path: {value}"));
        }
        path.push(part);
    }
    if path.as_os_str().is_empty() {
        Err(format!("unsafe plugin archive path: {value}"))
    } else {
        Ok(path)
    }
}

pub(in super::super) fn default_plugins(service_root: &Path) -> Vec<PluginManifest> {
    plugin_catalog(service_root).list_manifests()
}

pub(in super::super) fn read_plugin_manifest_from_archive(
    archive_path: &Path,
) -> Result<(String, String), String> {
    let file = File::open(archive_path).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let (manifest_index, manifest_prefix) = find_zip_plugin_manifest(&mut archive)?;
    let mut manifest_raw = String::new();
    archive
        .by_index(manifest_index)
        .map_err(|error| error.to_string())?
        .read_to_string(&mut manifest_raw)
        .map_err(io_error)?;
    Ok((manifest_raw, manifest_prefix))
}

pub(super) fn hash_text_sha256(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

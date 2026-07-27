//! Repository archive and git export helpers.

use super::*;

pub(super) fn export_repository_archive(
    repo_root: &Path,
    options: &RepositoryArchiveExportOptions,
) -> Result<(), String> {
    if !repo_root.is_dir() {
        return Err("repository path is not a directory".to_string());
    }

    let output_path = normalize_archive_output_path(options)?;
    if output_path.as_os_str().is_empty() {
        return Err("archive output path cannot be empty".to_string());
    }

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
    }
    validate_archive_output_path(repo_root, &output_path)?;
    if output_path.exists() {
        if !output_path.is_file() {
            return Err("archive output path must be a file".to_string());
        }
        fs::remove_file(&output_path).map_err(io_error)?;
    }

    match options.format.as_str() {
        "zip" => export_zip_archive(repo_root, &output_path, options),
        "7z" => export_7z_archive(repo_root, &output_path, options),
        "tar" => export_tar_archive(repo_root, &output_path, options),
        value => Err(format!("unsupported archive format: {value}")),
    }
}

pub(super) fn normalize_archive_output_path(
    options: &RepositoryArchiveExportOptions,
) -> Result<PathBuf, String> {
    let trimmed = options.output_path.trim();
    if trimmed.is_empty() {
        return Err("archive output path cannot be empty".to_string());
    }

    let output_path = PathBuf::from(trimmed);
    if output_path.extension().is_some() {
        return Ok(output_path);
    }

    Ok(output_path.with_extension(match options.format.as_str() {
        "7z" => "7z",
        "tar" if options.compression == "none" => "tar",
        "tar" => "tar.gz",
        _ => "zip",
    }))
}

pub(super) fn validate_archive_output_path(
    repo_root: &Path,
    output_path: &Path,
) -> Result<(), String> {
    let repo_canonical = repo_root.canonicalize().map_err(io_error)?;
    let parent = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_canonical = parent.canonicalize().map_err(io_error)?;
    if parent_canonical.starts_with(&repo_canonical) {
        return Err("archive output path cannot be inside the repository".to_string());
    }
    Ok(())
}

pub(super) fn export_zip_archive(
    repo_root: &Path,
    output_path: &Path,
    options: &RepositoryArchiveExportOptions,
) -> Result<(), String> {
    if let Some(binary) = find_7z_binary() {
        run_7z_archive(&binary, "zip", repo_root, output_path, options)
    } else if options.encrypt {
        Err("zip encryption requires 7z/7zz/7za in PATH".to_string())
    } else {
        run_powershell_compress_archive(repo_root, output_path, &options.compression)
    }
}

pub(super) fn export_7z_archive(
    repo_root: &Path,
    output_path: &Path,
    options: &RepositoryArchiveExportOptions,
) -> Result<(), String> {
    let binary =
        find_7z_binary().ok_or_else(|| "7z export requires 7z/7zz/7za in PATH".to_string())?;
    run_7z_archive(&binary, "7z", repo_root, output_path, options)
}

pub(super) fn export_tar_archive(
    repo_root: &Path,
    output_path: &Path,
    options: &RepositoryArchiveExportOptions,
) -> Result<(), String> {
    if options.encrypt {
        return Err("tar export does not support encryption; use zip or 7z".to_string());
    }

    let mut command = Command::new("tar");
    command
        .current_dir(repo_root)
        .arg(if options.compression == "none" {
            "-cf"
        } else {
            "-czf"
        })
        .arg(output_path)
        .arg(".");

    run_command(command, "tar export")
}

pub(super) fn run_7z_archive(
    binary: &str,
    archive_type: &str,
    repo_root: &Path,
    output_path: &Path,
    options: &RepositoryArchiveExportOptions,
) -> Result<(), String> {
    let mut command = Command::new(binary);
    command
        .current_dir(repo_root)
        .arg("a")
        .arg("-y")
        .arg(format!("-t{archive_type}"))
        .arg(compression_flag(&options.compression))
        .arg(output_path)
        .arg(".");

    if options.encrypt {
        let password = options
            .password
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "archive password cannot be empty".to_string())?;
        command.arg(format!("-p{password}"));
        if archive_type == "7z" {
            command.arg("-mhe=on");
        }
    }

    run_command(command, "7z export")
}

pub(super) fn run_powershell_compress_archive(
    repo_root: &Path,
    output_path: &Path,
    compression: &str,
) -> Result<(), String> {
    let script = format!(
        "Add-Type -AssemblyName System.IO.Compression.FileSystem; [System.IO.Compression.ZipFile]::CreateFromDirectory('{}', '{}', [System.IO.Compression.CompressionLevel]::{}, $true)",
        escape_powershell_single_quoted_path(repo_root),
        escape_powershell_single_quoted_path(output_path),
        powershell_compression_level(compression),
    );
    let mut command = Command::new("powershell");
    command.arg("-NoProfile").arg("-Command").arg(script);
    run_command(command, "zip export")
}

pub(super) fn export_repository_to_git(
    repo_root: &Path,
    options: &RepositoryGitExportOptions,
) -> Result<GitExportResult, String> {
    if !repo_root.join(".git").is_dir() {
        return Err("repository folder is not a Git repository".to_string());
    }

    run_git(repo_root, &["add", "-A"])?;

    if has_git_changes(repo_root)? {
        let message = options
            .message
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("导出资源库");
        run_git(repo_root, &["commit", "-m", message])?;
    }

    let remote = options
        .remote
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("origin")
        .to_string();
    let branch = options
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| current_git_branch(repo_root).unwrap_or_else(|_| "HEAD".to_string()));
    if branch == "HEAD" {
        return Err("cannot infer Git branch; specify a branch before uploading".to_string());
    }

    run_git(repo_root, &["push", &remote, &branch])?;
    Ok(GitExportResult {
        remote: remote.clone(),
        branch: branch.clone(),
        message: format!("资源库已上传到 {remote}/{branch}"),
    })
}

pub(super) struct GitExportResult {
    pub(super) remote: String,
    pub(super) branch: String,
    pub(super) message: String,
}

pub(super) fn has_git_changes(repo_root: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .arg("status")
        .arg("--porcelain")
        .output()
        .map_err(|error| format!("git unavailable: {error}"))?;
    if !output.status.success() {
        return Err(command_error("git status", &output));
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

pub(super) fn current_git_branch(repo_root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .arg("branch")
        .arg("--show-current")
        .output()
        .map_err(|error| format!("git unavailable: {error}"))?;
    if !output.status.success() {
        return Err(command_error("git branch", &output));
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        Ok("HEAD".to_string())
    } else {
        Ok(branch)
    }
}

pub(super) fn run_git(repo_root: &Path, args: &[&str]) -> Result<(), String> {
    let mut command = Command::new("git");
    command.current_dir(repo_root).args(args);
    run_command(command, &format!("git {}", args.join(" ")))
}

pub(super) fn find_7z_binary() -> Option<&'static str> {
    ["7z", "7zz", "7za"]
        .into_iter()
        .find(|binary| Command::new(binary).arg("--help").output().is_ok())
}

pub(super) fn compression_flag(value: &str) -> &'static str {
    match value {
        "none" => "-mx=0",
        "fast" => "-mx=3",
        "maximum" => "-mx=9",
        _ => "-mx=5",
    }
}

pub(super) fn powershell_compression_level(value: &str) -> &'static str {
    match value {
        "none" => "NoCompression",
        "fast" => "Fastest",
        _ => "Optimal",
    }
}

pub(super) fn escape_powershell_single_quoted_path(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

pub(super) fn run_command(mut command: Command, label: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("{label} failed to start: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error(label, &output))
    }
}

pub(super) fn command_error(label: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        output.status.to_string()
    };
    format!("{label} failed: {detail}")
}

pub(super) fn copy_directory_recursive_with_mode(
    source: &Path,
    source_relative_path: Option<&str>,
    target: &Path,
    target_relative_path: &str,
    hardlink_preferred: bool,
    cancellation: &dyn CancellationCheck,
) -> Result<Vec<HardlinkCopyOutcome>, String> {
    cancellation.checkpoint()?;
    fs::create_dir(target).map_err(io_error)?;
    let mut outcomes = Vec::new();
    for entry in fs::read_dir(source).map_err(io_error)? {
        cancellation.checkpoint()?;
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        let child_source = entry.path();
        let child_target = target.join(&name);
        let child_source_relative_path =
            source_relative_path.map(|parent| join_relative_path(parent, &name));
        let child_relative_path = join_relative_path(target_relative_path, &name);
        let metadata = entry.metadata().map_err(io_error)?;
        if metadata.is_dir() {
            outcomes.extend(copy_directory_recursive_with_mode(
                &child_source,
                child_source_relative_path.as_deref(),
                &child_target,
                &child_relative_path,
                hardlink_preferred,
                cancellation,
            )?);
        } else if metadata.is_file() {
            outcomes.push(copy_file_with_mode(
                &child_source,
                child_source_relative_path.as_deref(),
                &child_target,
                &child_relative_path,
                hardlink_preferred,
                cancellation,
            )?);
        }
    }

    Ok(outcomes)
}

pub(super) fn copy_file_with_mode(
    source: &Path,
    source_relative_path: Option<&str>,
    target: &Path,
    target_relative_path: &str,
    hardlink_preferred: bool,
    cancellation: &dyn CancellationCheck,
) -> Result<HardlinkCopyOutcome, String> {
    cancellation.checkpoint()?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    cancellation.checkpoint()?;
    if hardlink_preferred && fs::hard_link(source, target).is_ok() {
        return Ok(HardlinkCopyOutcome {
            source_path: source_relative_path.map(str::to_string),
            target_path: target_relative_path.to_string(),
            link_state: "linked".to_string(),
        });
    }
    copy_file_atomically(source, target, cancellation)?;
    Ok(HardlinkCopyOutcome {
        source_path: source_relative_path.map(str::to_string),
        target_path: target_relative_path.to_string(),
        link_state: "copiedFallback".to_string(),
    })
}

/// 分块复制到同目录临时文件，完整落盘后再原子发布。
fn copy_file_atomically(
    source: &Path,
    target: &Path,
    cancellation: &dyn CancellationCheck,
) -> Result<(), String> {
    let mut input = File::open(source).map_err(io_error)?;
    publish_reader_atomically(&mut input, target, cancellation, |temporary| {
        fs::set_permissions(
            temporary,
            fs::metadata(source).map_err(io_error)?.permissions(),
        )
        .map_err(io_error)
    })
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CancelAfter {
        checkpoints: AtomicUsize,
        limit: usize,
    }

    impl CancellationCheck for CancelAfter {
        fn is_cancelled(&self) -> bool {
            self.checkpoints.fetch_add(1, Ordering::AcqRel) + 1 >= self.limit
        }
    }

    #[test]
    fn cancelled_file_copy_removes_temporary_file_and_can_be_retried() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "momobako-cancel-copy-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("test directory should be created");
        let source = root.join("source.bin");
        let target = root.join("target.bin");
        fs::write(&source, vec![7_u8; 2 * 1024 * 1024]).expect("test source should be written");

        let error = copy_file_with_mode(
            &source,
            None,
            &target,
            "target.bin",
            false,
            &CancelAfter {
                checkpoints: AtomicUsize::new(0),
                limit: 4,
            },
        )
        .expect_err("copy should be cancelled between chunks");
        assert_eq!(error, "repository operation cancelled");
        assert!(!target.exists());
        assert!(fs::read_dir(&root)
            .expect("test directory should remain readable")
            .all(|entry| !entry
                .expect("directory entry should be readable")
                .file_name()
                .to_string_lossy()
                .contains(".momobako-part-")));

        copy_file_with_mode(&source, None, &target, "target.bin", false, &NeverCancelled)
            .expect("retry should publish the complete target");
        assert_eq!(
            fs::metadata(&target).expect("target should exist").len(),
            2 * 1024 * 1024
        );
        let _ = fs::remove_dir_all(root);
    }
}

pub(super) fn replace_file_with_hardlink(
    repo_root: &Path,
    source: &Path,
    target: &Path,
) -> Result<(), String> {
    if !source.is_file() {
        return Err("hardlink source is not a file".to_string());
    }
    if !target.is_file() {
        return Err("hardlink target is not a file".to_string());
    }
    let staging_dir = repository_meta_dir(repo_root).join("hardlink-staging");
    fs::create_dir_all(&staging_dir).map_err(io_error)?;
    let backup = staging_dir.join(format!(
        "{}.bak",
        sha256_hex(&[
            target.to_string_lossy().as_bytes(),
            now_rfc3339().as_bytes()
        ])
    ));
    fs::rename(target, &backup).map_err(io_error)?;
    match fs::hard_link(source, target) {
        Ok(()) => {
            fs::remove_file(&backup).map_err(io_error)?;
            Ok(())
        }
        Err(error) => {
            let restore_result = fs::rename(&backup, target);
            if let Err(restore_error) = restore_result {
                return Err(format!(
                    "hardlink failed: {error}; restore failed: {restore_error}"
                ));
            }
            Err(format!("hardlink failed: {error}"))
        }
    }
}

pub(super) fn validate_new_entry_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if trimmed.contains(['/', '\\']) {
        return Err("name cannot contain path separators".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("invalid entry name".to_string());
    }
    if is_internal_repository_dir(trimmed) {
        return Err("internal repository directory is reserved".to_string());
    }
    Ok(trimmed.to_string())
}

pub(super) fn parent_relative_path(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|parent| parent.to_string_lossy().replace('\\', "/"))
        .filter(|value| value != ".")
        .unwrap_or_default()
}

pub(super) fn join_relative_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

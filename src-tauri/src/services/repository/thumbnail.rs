//! Thumbnail, preview token, hashing, and palette utilities.

use super::*;

pub(super) fn generate_thumbnail_for_file(
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: &Path,
    file: &DiscoveredFile,
) -> Result<Option<String>, String> {
    if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
        return Ok(None);
    }

    let extension = file.extension.to_lowercase();
    if !is_image_extension(&extension)
        && !is_video_extension(&extension)
        && !is_audio_extension(&extension)
    {
        return Ok(None);
    }

    let source_path = resolve_repository_relative_path(repo_root, &file.relative_path)?;
    let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
        &repo.summary.repo_id,
        &repo.summary.path,
    ));
    fs::create_dir_all(&thumbnail_dir).map_err(io_error)?;
    let thumbnail_path = thumbnail_dir.join(thumbnail_file_name(
        &repo.summary.repo_id,
        &repo.summary.path,
        &file.relative_path,
        "file",
        "generated",
    ));

    let generated = if is_image_extension(&extension) {
        generate_image_thumbnail(&source_path, &thumbnail_path).map(|_| true)
    } else if is_audio_extension(&extension) {
        generate_audio_thumbnail(&source_path, &thumbnail_path)
    } else {
        generate_video_thumbnail(&source_path, &thumbnail_path).map(|_| true)
    };

    match generated {
        Ok(true) => Ok(Some(thumbnail_path.to_string_lossy().to_string())),
        Ok(false) => {
            let _ = fs::remove_file(&thumbnail_path);
            Ok(None)
        }
        Err(error) => {
            let _ = fs::remove_file(&thumbnail_path);
            eprintln!(
                "thumbnail generation skipped for {}: {}",
                file.relative_path, error
            );
            Ok(None)
        }
    }
}

pub(super) fn ensure_thumbnail_for_file(
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: &Path,
    file: &DiscoveredFile,
    existing_thumbnail_path: Option<String>,
    refresh: bool,
) -> Result<Option<String>, String> {
    if !refresh {
        if let Some(path) = existing_thumbnail_path {
            let expected_dir = thumbnail_root.join(thumbnail_repository_dir_name(
                &repo.summary.repo_id,
                &repo.summary.path,
            ));
            if thumbnail_path_is_valid(&expected_dir, &path) {
                return Ok(Some(path));
            }
        }
    }

    generate_thumbnail_for_file(repo, repo_root, thumbnail_root, file)
}

pub(super) fn thumbnail_path_is_valid(thumbnail_root: &Path, path: &str) -> bool {
    let thumbnail_path = Path::new(path);
    if !thumbnail_path.is_file() {
        return false;
    }

    let Ok(thumbnail_path) = canonicalize_local_path(thumbnail_path) else {
        return false;
    };
    let Ok(thumbnail_root) = canonicalize_local_path(thumbnail_root) else {
        return false;
    };
    thumbnail_path.starts_with(thumbnail_root)
}

pub(super) fn thumbnail_bytes_from_request(request: &ThumbnailRequest) -> Result<Vec<u8>, String> {
    if let Some(bytes) = &request.image_bytes {
        return Ok(bytes.clone());
    }

    if let Some(source_url) = request
        .source_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if request.action.as_deref() != Some("save") {
            return Err("thumbnail sourceUrl can only be used with save action".to_string());
        }
        return download_remote_thumbnail_bytes(source_url);
    }

    let source_path = request
        .source_path
        .as_deref()
        .ok_or_else(|| "thumbnail source is required".to_string())?;
    let path = Path::new(source_path);
    if !path.is_file() {
        return Err(format!("thumbnail source file not found: {source_path}"));
    }
    fs::read(path).map_err(io_error)
}

pub(super) fn download_remote_thumbnail_bytes(url: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("thumbnail sourceUrl only supports http and https URLs".to_string());
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent("MomoBakoThumbnail/1")
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("thumbnail download client error: {error}"))?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| format!("thumbnail download request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("thumbnail download returned HTTP {status}"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_REMOTE_THUMBNAIL_BYTES)
    {
        return Err("thumbnail source is too large".to_string());
    }
    let bytes = response
        .bytes()
        .map_err(|error| format!("thumbnail download body error: {error}"))?;
    if bytes.len() as u64 > MAX_REMOTE_THUMBNAIL_BYTES {
        return Err("thumbnail source is too large".to_string());
    }
    Ok(bytes.to_vec())
}

pub(super) fn save_custom_thumbnail_bytes(
    thumbnail_root: &Path,
    repo: &RepositoryRecord,
    entry_path: &str,
    kind: &str,
    bytes: &[u8],
) -> Result<String, String> {
    save_thumbnail_bytes(thumbnail_root, repo, entry_path, kind, "custom", bytes)
}

pub(super) fn save_thumbnail_bytes(
    thumbnail_root: &Path,
    repo: &RepositoryRecord,
    entry_path: &str,
    kind: &str,
    source: &str,
    bytes: &[u8],
) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("thumbnail image is empty".to_string());
    }

    let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
        &repo.summary.repo_id,
        &repo.summary.path,
    ));
    fs::create_dir_all(&thumbnail_dir).map_err(io_error)?;
    let thumbnail_path = thumbnail_dir.join(thumbnail_file_name(
        &repo.summary.repo_id,
        &repo.summary.path,
        entry_path,
        kind,
        source,
    ));
    let image = image::load_from_memory(bytes)
        .map_err(|error| format!("thumbnail image error: {error}"))?;
    let thumbnail = image.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
    thumbnail
        .save_with_format(&thumbnail_path, image::ImageFormat::Jpeg)
        .map_err(|error| format!("thumbnail image error: {error}"))?;
    Ok(thumbnail_path.to_string_lossy().to_string())
}

pub(super) fn thumbnail_repository_dir_name(repo_id: &str, repo_path: &str) -> String {
    sha256_hex(&[repo_id.as_bytes(), repo_path.as_bytes()])
}

pub(super) fn thumbnail_file_name(
    repo_id: &str,
    repo_path: &str,
    entry_path: &str,
    kind: &str,
    source: &str,
) -> String {
    format!(
        "{}.jpg",
        sha256_hex(&[
            repo_id.as_bytes(),
            repo_path.as_bytes(),
            entry_path.as_bytes(),
            kind.as_bytes(),
            source.as_bytes(),
        ])
    )
}

pub(super) fn preview_file_token(
    repo_id: &str,
    repo_path: &str,
    entry_path: &str,
    size_bytes: u64,
    modified_at: &str,
) -> String {
    sha256_hex(&[
        repo_id.as_bytes(),
        repo_path.as_bytes(),
        entry_path.as_bytes(),
        size_bytes.to_string().as_bytes(),
        modified_at.as_bytes(),
    ])
}

pub(super) fn preview_media_type_for_extension(extension: &str) -> &'static str {
    match extension {
        "glb" | "vrm" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "obj" => "text/plain",
        "fbx" => "application/octet-stream",
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
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "docm" => "application/vnd.ms-word.document.macroenabled.12",
        "dotx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
        "dotm" => "application/vnd.ms-word.template.macroenabled.12",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xlsm" => "application/vnd.ms-excel.sheet.macroenabled.12",
        "xlsb" => "application/vnd.ms-excel.sheet.binary.macroenabled.12",
        "xltx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.template",
        "xltm" => "application/vnd.ms-excel.template.macroenabled.12",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "pptm" => "application/vnd.ms-powerpoint.presentation.macroenabled.12",
        "ppsx" => "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
        "ppsm" => "application/vnd.ms-powerpoint.slideshow.macroenabled.12",
        "potx" => "application/vnd.openxmlformats-officedocument.presentationml.template",
        "potm" => "application/vnd.ms-powerpoint.template.macroenabled.12",
        "doc" | "dot" => "application/msword",
        "xls" | "xlt" => "application/vnd.ms-excel",
        "ppt" | "pps" | "pot" => "application/vnd.ms-powerpoint",
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

pub(super) fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part);
        hash.update([0xff]);
    }
    hex::encode(hash.finalize())
}

pub(super) fn file_sha256_hash(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("sha256:{}", hex::encode(hash.finalize())))
}

pub(super) fn file_content_hash_and_size(path: &Path) -> Result<Option<(String, i64)>, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    let size_bytes = i64::try_from(metadata.len())
        .map_err(|_| "file size exceeds supported range".to_string())?;
    let content_hash = file_sha256_hash(path)?;
    Ok(Some((content_hash, size_bytes)))
}

pub(super) fn current_file_matches_content(
    path: &Path,
    expected_hash: &str,
    expected_size_bytes: i64,
) -> Result<bool, String> {
    let Some((content_hash, size_bytes)) = file_content_hash_and_size(path)? else {
        return Ok(false);
    };
    Ok(content_hash == expected_hash && size_bytes == expected_size_bytes)
}

pub(super) fn is_content_hash(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn generate_image_thumbnail(
    source_path: &Path,
    thumbnail_path: &Path,
) -> Result<(), String> {
    let image =
        image::open(source_path).map_err(|error| format!("image thumbnail error: {error}"))?;
    let thumbnail = image.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
    thumbnail
        .save_with_format(thumbnail_path, image::ImageFormat::Jpeg)
        .map_err(|error| format!("image thumbnail error: {error}"))
}

pub(super) fn generate_video_thumbnail(
    source_path: &Path,
    thumbnail_path: &Path,
) -> Result<(), String> {
    ensure_ffmpeg_ready()?;

    let status = Command::new(ffmpeg_sidecar::paths::ffmpeg_path())
        .args(video_thumbnail_ffmpeg_args(source_path, thumbnail_path))
        .status()
        .map_err(|error| format!("ffmpeg unavailable: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg exited with status: {status}"))
    }
}

pub(super) fn generate_audio_thumbnail(
    source_path: &Path,
    thumbnail_path: &Path,
) -> Result<bool, String> {
    ensure_ffmpeg_ready()?;

    if !audio_has_cover_stream(source_path)? {
        return Ok(false);
    }

    let status = Command::new(ffmpeg_sidecar::paths::ffmpeg_path())
        .args(audio_thumbnail_ffmpeg_args(source_path, thumbnail_path))
        .status()
        .map_err(|error| format!("ffmpeg unavailable: {error}"))?;

    if status.success() {
        Ok(true)
    } else {
        Err(format!("ffmpeg exited with status: {status}"))
    }
}

pub(super) fn audio_has_cover_stream(source_path: &Path) -> Result<bool, String> {
    let output = match Command::new(ffmpeg_sidecar::ffprobe::ffprobe_path())
        .args(audio_cover_probe_args(source_path))
        .output()
    {
        Ok(output) => output,
        Err(_) => return Ok(false),
    };

    if !output.status.success() {
        return Err(format!("ffprobe exited with status: {}", output.status));
    }

    audio_cover_probe_output_has_stream(&output.stdout)
}

pub(super) fn audio_cover_probe_output_has_stream(output: &[u8]) -> Result<bool, String> {
    let value: serde_json::Value =
        serde_json::from_slice(output).map_err(|error| format!("ffprobe output error: {error}"))?;
    Ok(value
        .get("streams")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|streams| !streams.is_empty()))
}

pub(super) fn video_thumbnail_ffmpeg_args(
    source_path: &Path,
    thumbnail_path: &Path,
) -> Vec<OsString> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-ss".into(),
        "00:00:01".into(),
        "-i".into(),
        source_path.as_os_str().to_os_string(),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        "-vf".into(),
        format!("scale='min({THUMBNAIL_SIZE},iw)':-1").into(),
        thumbnail_path.as_os_str().to_os_string(),
    ]
}

pub(super) fn audio_thumbnail_ffmpeg_args(
    source_path: &Path,
    thumbnail_path: &Path,
) -> Vec<OsString> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        source_path.as_os_str().to_os_string(),
        "-map".into(),
        "0:v:0".into(),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        "-vf".into(),
        format!("scale='min({THUMBNAIL_SIZE},iw)':-1").into(),
        thumbnail_path.as_os_str().to_os_string(),
    ]
}

pub(super) fn audio_cover_probe_args(source_path: &Path) -> Vec<OsString> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-select_streams".into(),
        "v".into(),
        "-show_entries".into(),
        "stream=index".into(),
        "-of".into(),
        "json".into(),
        source_path.as_os_str().to_os_string(),
    ]
}

pub(super) fn ensure_ffmpeg_ready() -> Result<(), String> {
    FFMPEG_READY
        .get_or_init(|| {
            ffmpeg_sidecar::download::auto_download()
                .map_err(|error| format!("ffmpeg setup error: {error}"))
        })
        .clone()
}

pub(super) fn is_image_extension(extension: &str) -> bool {
    matches!(
        extension,
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff"
    )
}

#[derive(Debug, Default)]
pub(super) struct PaletteBucket {
    pub(super) count: u64,
    pub(super) red_sum: u64,
    pub(super) green_sum: u64,
    pub(super) blue_sum: u64,
}

pub(super) fn extract_image_palette(source_path: &Path, extension: &str) -> Vec<String> {
    if !is_image_extension(&extension.to_ascii_lowercase()) {
        return Vec::new();
    }

    let Ok(image) = image::open(source_path) else {
        return Vec::new();
    };
    let sampled = if image.width().max(image.height()) > 160 {
        image.thumbnail(160, 160)
    } else {
        image
    };
    let thumbnail = sampled.to_rgba8();
    let mut buckets = BTreeMap::<(u8, u8, u8), PaletteBucket>::new();

    for pixel in thumbnail.pixels() {
        let [red, green, blue, alpha] = pixel.0;
        if alpha < 128 {
            continue;
        }
        let bucket = buckets
            .entry((red & 0xf8, green & 0xf8, blue & 0xf8))
            .or_default();
        bucket.count += 1;
        bucket.red_sum += u64::from(red);
        bucket.green_sum += u64::from(green);
        bucket.blue_sum += u64::from(blue);
    }

    let mut colors = buckets
        .into_iter()
        .filter(|(_, bucket)| bucket.count > 0)
        .collect::<Vec<_>>();
    colors.sort_by(|(left_key, left), (right_key, right)| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left_key.cmp(right_key))
    });

    colors
        .into_iter()
        .take(5)
        .map(|(_, bucket)| averaged_hex_color(&bucket))
        .collect()
}

pub(super) fn averaged_hex_color(bucket: &PaletteBucket) -> String {
    let red = rounded_channel_average(bucket.red_sum, bucket.count);
    let green = rounded_channel_average(bucket.green_sum, bucket.count);
    let blue = rounded_channel_average(bucket.blue_sum, bucket.count);
    format!("#{red:02X}{green:02X}{blue:02X}")
}

pub(super) fn rounded_channel_average(sum: u64, count: u64) -> u8 {
    ((sum + count / 2) / count) as u8
}

pub(super) fn is_video_extension(extension: &str) -> bool {
    matches!(extension, "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v")
}

pub(super) fn is_audio_extension(extension: &str) -> bool {
    matches!(
        extension,
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus"
    )
}

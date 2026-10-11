//! Thumbnail, preview token, hashing, and palette utilities.

use super::*;
use crate::services::external_tools;

/// 能不能为这个文件生成缩略图：仓库能本地读，扩展名是图片、视频或音频（音频取内嵌封面）。
fn thumbnail_generator_supports(repo: &RepositoryRecord, file: &DiscoveredFile) -> bool {
    let extension = file.extension.to_lowercase();
    repository_supports_local_read_access(repo)
        && (is_image_extension(&extension)
            || is_video_extension(&extension)
            || is_audio_extension(&extension))
}

/// 这个文件生成的缩略图写在哪：仓库缩略图目录下，按仓库、路径和「generated」取 sha256 文件名。
fn generated_thumbnail_path(
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    file: &DiscoveredFile,
) -> Result<PathBuf, String> {
    let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
        &repo.summary.repo_id,
        &repo.summary.path,
    ));
    fs::create_dir_all(&thumbnail_dir).map_err(io_error)?;
    Ok(thumbnail_dir.join(thumbnail_file_name(
        &repo.summary.repo_id,
        &repo.summary.path,
        &file.relative_path,
        "file",
        "generated",
    )))
}

/// 从源文件生成缩略图。返回 `Ok(None)` 表示源文件本身没有画面（没有内嵌封面的音频），这时已有的缩略图也删掉。
///
/// 先写到同目录的临时文件，成功以后再替换缓存里的那张：生成失败时旧的缩略图原样留着，失败交给调用方决定
/// 报错还是跳过。只在 [`thumbnail_generator_supports`] 为真时调用。
fn write_generated_thumbnail(
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: &Path,
    file: &DiscoveredFile,
) -> Result<Option<String>, String> {
    let extension = file.extension.to_lowercase();
    let source_path = resolve_repository_relative_path(repo_root, &file.relative_path)?;
    let thumbnail_path = generated_thumbnail_path(repo, thumbnail_root, file)?;
    // 临时文件仍以 .jpg 结尾，ffmpeg 按扩展名选输出格式。
    let partial_path = thumbnail_path.with_extension("partial.jpg");
    let generated = if is_image_extension(&extension) {
        generate_image_thumbnail(&source_path, &partial_path).map(|_| true)
    } else if is_audio_extension(&extension) {
        generate_audio_thumbnail(&source_path, &partial_path)
    } else {
        generate_video_thumbnail(&source_path, &partial_path).map(|_| true)
    };
    match generated {
        Ok(true) => {
            if let Err(error) = fs::rename(&partial_path, &thumbnail_path) {
                let _ = fs::remove_file(&partial_path);
                return Err(format!("thumbnail replace error: {error}"));
            }
            Ok(Some(thumbnail_path.to_string_lossy().to_string()))
        }
        Ok(false) => {
            let _ = fs::remove_file(&partial_path);
            let _ = fs::remove_file(&thumbnail_path);
            Ok(None)
        }
        Err(error) => {
            let _ = fs::remove_file(&partial_path);
            Err(error)
        }
    }
}

/// 浏览时生成缩略图：不支持的类型和生成失败都不算错，失败记日志、交回没有缩略图。
fn generate_thumbnail_for_file(
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: &Path,
    file: &DiscoveredFile,
) -> Result<Option<String>, String> {
    if !thumbnail_generator_supports(repo, file) {
        return Ok(None);
    }
    match write_generated_thumbnail(repo, repo_root, thumbnail_root, file) {
        Ok(path) => Ok(path),
        Err(error) => {
            crate::app_log!(
                "warn",
                "thumbnail",
                "generateSkipped",
                "缩略图生成已跳过。",
                serde_json::json!({
                    "path": file.relative_path,
                    "error": error,
                })
            );
            Ok(None)
        }
    }
}

/// `ensure` 和 `refresh` 共用的生成逻辑。
///
/// - `ensure`：已有的有效缩略图直接用，没有才生成；生成失败不算错。
/// - `refresh`：按源文件重新生成并替换缓存。没有生成器的类型（PDF、Office、文本、模型……）刷新不了，已有的
///   缩略图留着，不清掉；重新生成失败时旧的也留着，把失败原因交回调用方。
pub(super) fn ensure_thumbnail_for_file(
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: &Path,
    file: &DiscoveredFile,
    existing_thumbnail_path: Option<String>,
    refresh: bool,
) -> Result<Option<String>, String> {
    let expected_dir = thumbnail_root.join(thumbnail_repository_dir_name(
        &repo.summary.repo_id,
        &repo.summary.path,
    ));
    let existing = existing_thumbnail_path.filter(|path| thumbnail_path_is_valid(&expected_dir, path));
    if !refresh {
        if existing.is_some() {
            return Ok(existing);
        }
        return generate_thumbnail_for_file(repo, repo_root, thumbnail_root, file);
    }
    if !thumbnail_generator_supports(repo, file) {
        return Ok(existing);
    }
    write_generated_thumbnail(repo, repo_root, thumbnail_root, file).map_err(|error| {
        crate::app_log!(
            "warn",
            "thumbnail",
            "refreshFailed",
            "缩略图刷新失败，原来的缩略图留着。",
            serde_json::json!({
                "path": file.relative_path,
                "error": error,
            })
        );
        format!("thumbnail refresh failed: {error}")
    })
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

fn save_thumbnail_bytes(
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
    let image = decode_oriented_image(image::ImageReader::new(std::io::Cursor::new(bytes)))
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

/// 预览源的媒体类型，按小写扩展名给。桌面端只在类型以 `image/` 开头时把字节当图片解码，
/// 图片扩展名必须落在 `image/*` 上（合同见 `docs/api-design.md` 的 `files:preparePreviewSource`）。
pub fn preview_media_type_for_extension(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
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
    let image = image::ImageReader::open(source_path)
        .map_err(image::ImageError::IoError)
        .and_then(decode_oriented_image)
        .map_err(|error| format!("image thumbnail error: {error}"))?;
    let thumbnail = image.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
    thumbnail
        .save_with_format(thumbnail_path, image::ImageFormat::Jpeg)
        .map_err(|error| format!("image thumbnail error: {error}"))
}

/// 解码图片并按 EXIF 方向摆正。格式按内容认（扩展名不对也能解）；照片的方向写在 EXIF 里，直接解码不会
/// 应用它，缩略图会横躺或倒置。EXIF 读不出时照原样，不让整张图失败。
fn decode_oriented_image<R: std::io::BufRead + std::io::Seek>(
    reader: image::ImageReader<R>,
) -> image::ImageResult<image::DynamicImage> {
    use image::ImageDecoder;

    let mut decoder = reader.with_guessed_format()?.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or_else(|error| {
        crate::app_log!(
            "warn",
            "thumbnail",
            "orientationUnreadable",
            "图片的 EXIF 方向读不出，按原样生成缩略图。",
            serde_json::json!({ "error": error.to_string() })
        );
        image::metadata::Orientation::NoTransforms
    });
    // 和 `image::open` 一样先按默认上限预留解码缓冲，过大的图报上限错误，不硬分配。
    image::Limits::default().reserve(decoder.total_bytes())?;
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

pub(super) fn generate_video_thumbnail(
    source_path: &Path,
    thumbnail_path: &Path,
) -> Result<(), String> {
    external_tools::ensure_ffmpeg_ready()?;

    let status = child_process::command(external_tools::ffmpeg_path())
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
    external_tools::ensure_ffmpeg_ready()?;

    if !audio_has_cover_stream(source_path)? {
        return Ok(false);
    }

    let status = child_process::command(external_tools::ffmpeg_path())
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
    let output = match child_process::command(external_tools::ffprobe_path())
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

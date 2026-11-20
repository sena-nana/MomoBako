//! 按预览状态构造图片、媒体、原生、压缩包、失败、加载与回退内容引用。

use super::*;

/// 预览的正是播放列表当前的图片：画播放器里幻灯片的这一帧，按「适应 / 填充」铺。
/// 没有帧（读不到或解不开）时交回普通图片预览，由它写失败原因。
pub(super) fn current_still(model: &ShellViewModel) -> Option<ContentRef<'_>> {
    let still = model.player.still.as_ref()?;
    let frame = still.frame.as_ref()?;
    let same_repo =
        model.player.repo_id.is_some() && model.player.repo_id == model.workspace.active_repo_id;
    if !same_repo || model.inspect.target_path.as_deref() != Some(still.path.as_str()) {
        return None;
    }
    let pixels = PixelsRef {
        width: frame.width,
        height: frame.height,
        rgba: &frame.rgba,
    };
    Some(ContentRef::Still {
        pixels,
        cover: model.player.settings.object_fit_cover,
    })
}

/// 图片：宿主已经上传纹理时用纹理槽，离屏只有像素时画成内容图，都没有时是读取中。
pub(super) fn image_ref(model: &ShellViewModel) -> ContentRef<'_> {
    match (&model.preview_token, &model.preview_pixels) {
        (Some(_), Some(_)) => ContentRef::ImageTexture,
        (_, Some(pixels)) => ContentRef::ImagePixels(PixelsRef {
            width: pixels.width,
            height: pixels.height,
            rgba: &pixels.rgba,
        }),
        (_, None) => loading_ref(model, &PreviewKind::Image),
    }
}

/// 音视频读不出、解不开，或者播放器装不上同一个文件：用失败浮层
/// 替换唱片舞台，红色标题下一行原因。标题音频写「无法预览该音频」，视频写
/// 「无法预览该媒体」；原因是解码层或播放器写下的原文（没有原生解码器、某格式解码失败……），没有原文时用
/// 「音频无法播放」「视频无法播放」。这类失败就近显示，不进全局状态区。在线条目的播放源准备失败
/// （来源登录失效、网络不通、缓存不可用）时原因是来源交回的原话，下面多一个「重试」重新准备。
pub(super) fn media_failed_ref<'a>(
    model: &'a ShellViewModel,
    error: Option<&'a str>,
) -> ContentRef<'a> {
    let video =
        super::super::super::inspect::support::is_video_extension(&model.inspect.facts.extension);
    let (title, fallback) = if video {
        ("无法预览该媒体", "视频无法播放")
    } else {
        ("无法预览该音频", "音频无法播放")
    };
    let message = error
        .map(str::trim)
        .filter(|error| !error.is_empty())
        .unwrap_or(fallback);
    ContentRef::Failed {
        title,
        message,
        toolbar: None,
        document: false,
        retry: model.inspect.media_retry,
    }
}

/// 播放器挂载：视频文件有宿主纹理时画纹理槽；离屏没有宿主纹理、但已经解出画面时，把这一帧像素画进预览。
/// 还没有画面（只解出音轨，或帧被清掉）时画缩略图或视频图标，是明确的占位，不当成音频画唱片舞台。
/// 音频文件是唱片舞台。按文件的扩展名分，不按有没有画面分。
pub(super) fn media_ref(model: &ShellViewModel) -> ContentRef<'_> {
    let cover = model.player.settings.object_fit_cover;
    let host = model
        .preview_token
        .as_deref()
        .is_some_and(|token| token.starts_with("video:"));
    if host && model.preview_pixels.is_some() {
        return ContentRef::Video { cover };
    }
    if is_video_file(model) {
        if let Some(pixels) = model.preview_pixels.as_ref() {
            let pixels = PixelsRef {
                width: pixels.width,
                height: pixels.height,
                rgba: &pixels.rgba,
            };
            return ContentRef::VideoPixels { pixels, cover };
        }
        return ContentRef::Fallback(fallback_ref(model));
    }
    let name = target_name(model);
    let entry = super::super::audio_stage::preview_entry(model);
    ContentRef::Audio {
        caption: super::super::audio_stage::audio_caption(
            name,
            &model.inspect.facts.extension,
            entry.meta,
        ),
        cover_path: entry.cover,
        name,
        viewport: model.viewport_width,
        lines: &model.inspect.lyrics,
        active: super::super::super::inspect_lyrics::active_index(
            &model.inspect.lyrics,
            media_time(model),
        ),
        pending: model.inspect.lyrics_pending,
        accent: crate::appearance::theme_mode(&model.settings.theme, None),
    }
}

/// 预览的是视频文件（mp4、mov、mkv、webm、avi、m4v）。
pub(super) fn is_video_file(model: &ShellViewModel) -> bool {
    super::super::super::inspect::support::is_video_extension(&model.inspect.facts.extension)
}

/// PDF 和 Office：顶栏、翻页和整页；压缩包是目录；模型是画面和尺寸标签。
pub(super) fn native_ref<'a>(
    model: &'a ShellViewModel,
    view_id: &str,
    label: &'a str,
    content: &'a str,
) -> ContentRef<'a> {
    let inspect = &model.inspect;
    if inspect_library::is_archive(view_id) {
        let picked = inspect.archive_selected.as_deref();
        let (stage_title, stage_detail, stage_failed) = inspect_library::archive_stage(inspect);
        let listing = inspect
            .archive_nests
            .last()
            .map(|nest| nest.text.as_str())
            .unwrap_or(content);
        return ContentRef::Archive {
            content: listing,
            picked,
            stage_title,
            stage_detail,
            stage_failed,
            name: target_name(model),
            shown: archive_shown(model),
        };
    }
    let raster = inspect
        .raster_frame()
        .map(|(width, height, rgba)| PixelsRef {
            width,
            height,
            rgba,
        });
    if inspect_library::is_model(view_id) {
        return ContentRef::Model {
            extension: &inspect.facts.extension,
            raster,
            content,
            mesh: inspect.has_mesh(),
            extent: inspect.mesh_extent(),
        };
    }
    let toolbar = document_toolbar(model, view_id, label);
    let text = if content.is_empty() { label } else { content };
    ContentRef::Document {
        toolbar,
        nav: inspect.page_nav(),
        page: raster,
        error: inspect.raster_error(),
        text,
    }
}

/// 包内文件已经取出、并且没有更新的取出在路上时，舞台画这一条。正在取下一条时先让出舞台。
/// 音频舞台跟窗口宽走，和单独打开音频时同一套。
pub(super) fn archive_shown(model: &ShellViewModel) -> Option<ArchiveShown<'_>> {
    if model.inspect.archive_request.is_some() {
        return None;
    }
    let entry = model.inspect.archive_entry.as_ref()?;
    let host_video = model
        .preview_token
        .as_deref()
        .is_some_and(|token| token.starts_with("video:"))
        && model.preview_pixels.is_some();
    archive_entry::shown(
        &entry.path,
        &entry.kind,
        model.viewport_width,
        host_video,
        model.player.settings.object_fit_cover,
    )
}

/// PDF 和 Office 才有种类、扩展名和页数这条顶栏；别的插件视图没有。
pub(super) fn document_toolbar(
    model: &ShellViewModel,
    view_id: &str,
    label: &str,
) -> Option<DocumentBar> {
    if !inspect_library::is_office_pdf(view_id) {
        return None;
    }
    inspect_library::document_bar(model, label)
}

/// 失败：浮层中间一行红色标题和一行弱色原因。PDF 保留顶栏。
pub(super) fn failed_ref<'a>(
    model: &'a ShellViewModel,
    kind: &'a PreviewKind,
    message: &'a str,
) -> ContentRef<'a> {
    let (title, toolbar, document) = match kind {
        PreviewKind::Markdown | PreviewKind::Text => ("无法预览该文本", None, false),
        PreviewKind::Image | PreviewKind::Media => ("无法预览该媒体", None, false),
        PreviewKind::Native { view_id, .. } if inspect_library::is_archive(view_id) => {
            ("无法预览该压缩包", None, false)
        }
        PreviewKind::Native { view_id, .. } if inspect_library::is_model(view_id) => {
            ("无法预览该模型", None, false)
        }
        PreviewKind::Native { view_id, label } => (
            "无法预览该文档",
            document_toolbar(model, view_id, label),
            true,
        ),
        PreviewKind::Unsupported => return ContentRef::Fallback(fallback_ref(model)),
    };
    ContentRef::Failed {
        title,
        message,
        toolbar,
        document,
        retry: false,
    }
}

/// 读取中。文本、图片、模型是左上角的状态胶囊；音视频是居中的准备卡；PDF 保留顶栏。
pub(super) fn loading_ref<'a>(model: &'a ShellViewModel, kind: &'a PreviewKind) -> ContentRef<'a> {
    let size = model.inspect.facts.size_label.trim();
    let sized = |empty: &str| {
        if size.is_empty() {
            empty.to_string()
        } else {
            format!("准备 {size}")
        }
    };
    let pill = |title, detail, progress, toolbar| {
        ContentRef::Loading(LoadingRef::Pill {
            title,
            detail,
            progress,
            toolbar,
        })
    };
    match kind {
        PreviewKind::Markdown | PreviewKind::Text => {
            pill("读取文本", sized("准备文本内容"), false, None)
        }
        PreviewKind::Image => pill("准备播放", "准备媒体".to_string(), true, None),
        PreviewKind::Media => ContentRef::Loading(LoadingRef::Media {
            name: target_name(model),
            video: is_video_file(model),
        }),
        PreviewKind::Native { view_id, .. } if inspect_library::is_archive(view_id) => {
            ContentRef::Loading(LoadingRef::Archive {
                detail: sized("准备内部目录"),
            })
        }
        PreviewKind::Native { view_id, .. } if inspect_library::is_model(view_id) => {
            pill("读取模型", "准备读取文件".to_string(), false, None)
        }
        PreviewKind::Native { view_id, label } => {
            let title = if inspect_library::is_pdf(view_id) {
                "载入 PDF"
            } else if inspect_library::is_office_pdf(view_id) {
                "转换文档"
            } else {
                "读取预览"
            };
            pill(
                title,
                sized("建立预览"),
                false,
                document_toolbar(model, view_id, label),
            )
        }
        PreviewKind::Unsupported => ContentRef::Fallback(fallback_ref(model)),
    }
}

/// 没有预览插件：有缩略图就按 contain 画缩略图（bg 底），否则画类型图标。
/// 缩略图取自当前列表里同一路径的行；不在列表里的文件（例如从搜索打开）只画图标。
pub(super) fn fallback_ref(model: &ShellViewModel) -> FallbackRef<'_> {
    thumbnail_ref(model).unwrap_or_else(|| {
        let extension = model
            .inspect
            .facts
            .extension
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        let icon = match extension.as_str() {
            "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => icons::FILE_VIDEO,
            "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus" => icons::FILE_AUDIO,
            _ => icons::FILE_IMAGE,
        };
        FallbackRef::Icon(icon)
    })
}

pub(super) fn thumbnail_ref(model: &ShellViewModel) -> Option<FallbackRef<'_>> {
    let path = model.inspect.target_path.as_deref()?;
    // 素材详情说没有缩略图就不画；有的话纹理取自列表里同一路径的行。
    model
        .inspect
        .facts
        .thumbnail_path
        .as_deref()
        .map(str::trim)
        .filter(|slot| !slot.is_empty())?;
    let files = &model.files;
    let row = files
        .rows
        .iter()
        .chain(files.virtual_rows.iter())
        .find(|row| row.path == path)?;
    let slot = row
        .thumbnail_path
        .as_deref()
        .map(str::trim)
        .filter(|slot| !slot.is_empty())?;
    row.texture_ready.then_some(FallbackRef::Texture(slot))
}

/// 预览会话的播放头。歌词用它决定哪一行是当前行。
pub(super) fn media_time(model: &ShellViewModel) -> u64 {
    model
        .inspect
        .media_session()
        .map(|session| session.current_time_ms)
        .unwrap_or(0)
}

/// 预览目标的文件名。
pub(super) fn target_name(model: &ShellViewModel) -> &str {
    let path = model.inspect.target_path.as_deref().unwrap_or_default();
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

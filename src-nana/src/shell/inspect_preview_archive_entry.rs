//! 包内这一条的现有预览：文本、图片、自绘 PDF、抽出的 Office 文本、软件光栅的模型、内存里解出的音频，以及未压缩 AVI 的真实帧。
//!
//! 面包屑在左边的名单上，这里只画这一条。Office 不还原版式。模型有三角形才给旋转和缩放；FBX 与 BLEND 只显示文件头。
//! 音频用现有唱片舞台。解出来的样本不送进声卡，这里也不播放。
//! 未压缩 AVI 按这条预览自己的播放头画已经解开的那一帧。没有宿主纹理时直接画解码像素；有宿主纹理时和文件预览一样画纹理槽。播放条不接管。

use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{LengthSpec, Stack};

use super::super::super::inspect::{ArchiveEntryKind, PageFrame};
use super::super::super::inspect_library;
use super::{PixelsRef, document_view, model_view, overlay, pixel_image, text_plugin};

/// 包内这一条借用的预览。
#[derive(Clone, Copy, Debug)]
pub(super) enum ArchiveShown<'a> {
    Text {
        path: &'a str,
        markdown: bool,
        text: &'a str,
        truncated_at: Option<u64>,
        extension: &'a str,
    },
    Image {
        path: &'a str,
        pixels: PixelsRef<'a>,
    },
    Pdf {
        path: &'a str,
        nav: Option<(usize, usize)>,
        page: Option<PixelsRef<'a>>,
        error: Option<&'a str>,
        text: &'a str,
    },
    /// 抽出的纯文本。`error` 有值时整份抽取失败。
    Office {
        path: &'a str,
        text: &'a str,
        error: Option<&'a str>,
    },
    /// 光栅或文件头。`mesh` 时右上角有旋转和缩放。`extent` 是包围盒尺寸，没有网格时为空。
    Model {
        path: &'a str,
        extension: &'a str,
        raster: Option<PixelsRef<'a>>,
        text: &'a str,
        mesh: bool,
        extent: &'a str,
        error: Option<&'a str>,
    },
    /// 现有唱片舞台。`error` 有值时是现有音频失败浮层。`viewport` 是窗口宽。
    Audio {
        path: &'a str,
        viewport: f32,
        error: Option<&'a str>,
    },
    /// 没有宿主纹理时，按播放头画出的那一帧。近黑底、contain，像素来自已经解开的缓冲区。
    Video {
        path: &'a str,
        pixels: PixelsRef<'a>,
    },
    /// 已经挂着宿主纹理时，和文件预览一样画 `file-preview` 槽。`cover` 跟播放器的适应/填充。
    VideoHost { path: &'a str, cover: bool },
}

/// 包内这一条的自有副本。路径在借用阶段已经算进身份；舞台只画画面，副本上的路径可以不读。
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(super) enum ArchiveShownOwned {
    Text {
        path: String,
        markdown: bool,
        text: std::sync::Arc<str>,
        truncated_at: Option<u64>,
        extension: String,
    },
    Image {
        path: String,
        pixels: super::Pixels,
    },
    Pdf {
        path: String,
        nav: Option<(usize, usize)>,
        page: Option<super::Pixels>,
        error: Option<String>,
        text: String,
    },
    Office {
        path: String,
        text: String,
        error: Option<String>,
    },
    Model {
        path: String,
        extension: String,
        raster: Option<super::Pixels>,
        text: String,
        mesh: bool,
        extent: String,
        error: Option<String>,
    },
    Audio {
        path: String,
        viewport: f32,
        error: Option<String>,
    },
    Video {
        path: String,
        pixels: super::Pixels,
    },
    VideoHost {
        path: String,
        cover: bool,
    },
}

/// 从已经取出的条目借用画面。嵌套压缩包仍回到名单，这里不画。`viewport` 只给音频舞台。
/// `host_video` 为真时包内 AVI 画宿主纹理槽；否则按 `position_ms` 取已经解开的帧。
pub(super) fn shown<'a>(
    path: &'a str,
    kind: &'a ArchiveEntryKind,
    viewport: f32,
    host_video: bool,
    cover: bool,
) -> Option<ArchiveShown<'a>> {
    Some(match kind {
        ArchiveEntryKind::Text {
            markdown,
            extension,
            text,
            truncated_at,
        } => ArchiveShown::Text {
            path,
            markdown: *markdown,
            text,
            truncated_at: *truncated_at,
            extension,
        },
        ArchiveEntryKind::Image {
            width,
            height,
            rgba,
        } => ArchiveShown::Image {
            path,
            pixels: PixelsRef {
                width: *width,
                height: *height,
                rgba,
            },
        },
        ArchiveEntryKind::Pdf {
            frames,
            index,
            error,
        } => pdf_shown(path, frames, *index, error.as_deref()),
        ArchiveEntryKind::Office { text, error } => ArchiveShown::Office {
            path,
            text,
            error: error.as_deref(),
        },
        ArchiveEntryKind::Model {
            text,
            frame,
            mesh,
            error,
            extent,
            ..
        } => ArchiveShown::Model {
            path,
            extension: file_extension(path),
            raster: frame.as_ref().and_then(raster_of),
            text,
            mesh: mesh.is_some() && error.is_none(),
            extent: if error.is_none() { extent.as_str() } else { "" },
            error: error.as_deref(),
        },
        ArchiveEntryKind::Audio { error, .. } => ArchiveShown::Audio {
            path,
            viewport,
            error: error.as_deref(),
        },
        ArchiveEntryKind::Video {
            frames,
            position_ms,
        } => {
            if host_video {
                ArchiveShown::VideoHost { path, cover }
            } else if let Some(frame) = video_frame(frames, *position_ms) {
                ArchiveShown::Video {
                    path,
                    pixels: PixelsRef {
                        width: frame.width,
                        height: frame.height,
                        rgba: &frame.rgba,
                    },
                }
            } else {
                eprintln!("Nana 包内 AVI 在 {position_ms} 毫秒没有可画的画面：{path}");
                return None;
            }
        }
        ArchiveEntryKind::Nested { .. } => return None,
    })
}

/// 舞台上这一条的现有预览。名单留在左边，这里不再另铺一条面包屑。
pub(super) fn view(shown: &ArchiveShownOwned, scope: &str) -> AnyView {
    let body = match shown {
        ArchiveShownOwned::Text {
            markdown,
            text,
            truncated_at,
            extension,
            ..
        } => text_plugin(*markdown, text, *truncated_at, extension, scope),
        ArchiveShownOwned::Image { pixels, .. } => {
            pixel_image(pixels, false, "inspect-archive-image")
        }
        ArchiveShownOwned::Pdf {
            path,
            nav,
            page,
            error,
            text,
        } => {
            let extension = file_extension(path).to_ascii_uppercase();
            let pages = nav.map(|(_, total)| inspect_library::ready_page_label(total));
            let toolbar = inspect_library::document_bar_parts("PDF", &extension, pages);
            document_view(
                toolbar.as_ref(),
                *nav,
                page.as_ref(),
                error.as_deref(),
                text,
                scope,
            )
        }
        ArchiveShownOwned::Office { path, text, error } => {
            let toolbar = inspect_library::document_bar_parts(
                "文档",
                &file_extension(path).to_ascii_uppercase(),
                None,
            );
            document_view(toolbar.as_ref(), None, None, error.as_deref(), text, scope)
        }
        ArchiveShownOwned::Model {
            extension,
            raster,
            text,
            mesh,
            extent,
            error,
            ..
        } => {
            if let Some(error) = error.as_deref() {
                overlay("无法预览该模型", error, "inspect-native-error")
            } else {
                model_view(extension, raster.as_ref(), text, *mesh, extent)
            }
        }
        ArchiveShownOwned::Audio {
            path,
            viewport,
            error,
        } => {
            if let Some(error) = error.as_deref() {
                super::overlay("无法预览该音频", error, "inspect-failed")
            } else {
                let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
                let extension = name
                    .rsplit_once('.')
                    .map(|(_, extension)| extension)
                    .unwrap_or("");
                let caption = super::super::audio_stage::audio_caption(
                    name,
                    extension,
                    super::super::audio_stage::AudioMeta::Empty,
                );
                super::super::audio_stage::audio_stage(*viewport, &[], None, &caption, None, false)
            }
        }
        ArchiveShownOwned::Video { pixels, .. } => {
            super::frame_pixels(pixels, false, "inspect-archive-video", true)
        }
        ArchiveShownOwned::VideoHost { cover, .. } => super::host_video(*cover),
    };
    widget(
        Stack::fill_column(0.0)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0)),
    )
    .children((body,))
    .key("inspect-archive-entry")
    .into_any()
}

impl ArchiveShown<'_> {
    /// 身份：路径和画面。大块像素和文本只认缓冲区地址和长度。
    pub(super) fn identity(&self, hasher: &mut DefaultHasher) {
        std::mem::discriminant(self).hash(hasher);
        match self {
            Self::Text {
                path,
                markdown,
                text,
                truncated_at,
                extension,
            } => {
                (
                    path,
                    markdown,
                    text.as_ptr() as usize,
                    text.len(),
                    truncated_at,
                    extension,
                )
                    .hash(hasher);
            }
            Self::Image { path, pixels } => {
                path.hash(hasher);
                pixels.identity(hasher);
            }
            Self::Pdf {
                path,
                nav,
                page,
                error,
                text,
            } => {
                (path, nav, error, text.as_ptr() as usize, text.len()).hash(hasher);
                page.iter().for_each(|page| page.identity(hasher));
            }
            Self::Office { path, text, error } => {
                (path, text.as_ptr() as usize, text.len(), error).hash(hasher)
            }
            Self::Model {
                path,
                extension,
                raster,
                text,
                mesh,
                extent,
                error,
            } => {
                (
                    path,
                    extension,
                    text.as_ptr() as usize,
                    text.len(),
                    mesh,
                    extent,
                    error,
                )
                    .hash(hasher);
                raster.iter().for_each(|raster| raster.identity(hasher));
            }
            Self::Audio {
                path,
                viewport,
                error,
            } => (path, viewport.to_bits(), error).hash(hasher),
            Self::Video { path, pixels } => {
                path.hash(hasher);
                pixels.identity(hasher);
            }
            Self::VideoHost { path, cover } => (path, cover).hash(hasher),
        }
    }

    /// 这一条是不是在画宿主纹理槽。预览框据此决定要不要上传。
    pub(super) fn uses_host_video(&self) -> bool {
        matches!(self, Self::VideoHost { .. })
    }

    /// 抄成自有的内容。只在身份变了时调用。
    pub(super) fn to_owned(self) -> ArchiveShownOwned {
        match self {
            Self::Text {
                path,
                markdown,
                text,
                truncated_at,
                extension,
            } => ArchiveShownOwned::Text {
                path: path.to_string(),
                markdown,
                text: text.into(),
                truncated_at,
                extension: extension.to_string(),
            },
            Self::Image { path, pixels } => ArchiveShownOwned::Image {
                path: path.to_string(),
                pixels: pixels.to_owned(),
            },
            Self::Pdf {
                path,
                nav,
                page,
                error,
                text,
            } => ArchiveShownOwned::Pdf {
                path: path.to_string(),
                nav,
                page: page.map(PixelsRef::to_owned),
                error: error.map(str::to_string),
                text: text.to_string(),
            },
            Self::Office { path, text, error } => ArchiveShownOwned::Office {
                path: path.to_string(),
                text: text.to_string(),
                error: error.map(str::to_string),
            },
            Self::Model {
                path,
                extension,
                raster,
                text,
                mesh,
                extent,
                error,
            } => ArchiveShownOwned::Model {
                path: path.to_string(),
                extension: extension.to_string(),
                raster: raster.map(PixelsRef::to_owned),
                text: text.to_string(),
                mesh,
                extent: extent.to_string(),
                error: error.map(str::to_string),
            },
            Self::Audio {
                path,
                viewport,
                error,
            } => ArchiveShownOwned::Audio {
                path: path.to_string(),
                viewport,
                error: error.map(str::to_string),
            },
            Self::Video { path, pixels } => ArchiveShownOwned::Video {
                path: path.to_string(),
                pixels: pixels.to_owned(),
            },
            Self::VideoHost { path, cover } => ArchiveShownOwned::VideoHost {
                path: path.to_string(),
                cover,
            },
        }
    }
}

/// 不晚于播放头、并且有像素的那一帧。和文件预览同一条选择，不手写色块。
fn video_frame(
    frames: &[super::super::super::inspect::support::VideoFrame],
    time_ms: u64,
) -> Option<&super::super::super::inspect::support::VideoFrame> {
    super::super::super::inspect::frame_at(frames, time_ms)
        .filter(|frame| frame.width > 0 && frame.height > 0 && !frame.rgba.is_empty())
}

/// 包内 PDF 的当前页。整份解析失败、或这一页画不出来时带上现有失败原因。
fn pdf_shown<'a>(
    path: &'a str,
    frames: &'a [PageFrame],
    index: usize,
    error: Option<&'a str>,
) -> ArchiveShown<'a> {
    let frame = error.is_none().then(|| frames.get(index)).flatten();
    ArchiveShown::Pdf {
        path,
        nav: (error.is_none() && !frames.is_empty()).then_some((index, frames.len())),
        page: frame.and_then(raster_of),
        error: error.or_else(|| frame.and_then(|frame| frame.error.as_deref())),
        text: frame.map(|frame| frame.text.as_str()).unwrap_or(""),
    }
}

/// 能画的一帧。空帧和带错误的帧不拿去画。
fn raster_of(frame: &PageFrame) -> Option<PixelsRef<'_>> {
    (frame.error.is_none() && frame.width > 0 && frame.height > 0 && !frame.rgba.is_empty())
        .then_some(PixelsRef {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba.as_slice(),
        })
}

fn file_extension(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or("")
}

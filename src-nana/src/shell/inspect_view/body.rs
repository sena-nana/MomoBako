//! 预览框里的内容：文本、图片、音视频、PDF / Office、压缩包、3D 模型，以及读取中、失败和不支持。
//!
//! 外框常驻，框里按身份整块换。同步时先取出借用 ViewModel 的 [`ContentRef`] 算身份，变了才抄成
//! [`PreviewContent`]。播放推进只换纹理；歌词当前行、标题、艺人、专辑、封面或主题变了才重画舞台。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui::icons_tabler::REFRESH;
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, GpuTextureView, IconGlyph, JustifySpec, LengthSpec, RadiusTier,
    ScrollAxes, ScrollView, SemanticColorRole, Stack, Text, TextHorizontalAlignment, Thumbnail,
};
use nana_ui::{ButtonKind, ContentFit};
use nana_ui_core::Icon;

use super::super::inspect::{InspectMessage, PreviewBody, PreviewKind};
use super::super::inspect_library::{self, DocumentBar};
use super::super::player_view::icons;
use super::super::{ShellMessage, ShellViewModel};
use super::frame::text;

mod archive_entry;

use archive_entry::{ArchiveShown, ArchiveShownOwned};

mod content;
mod refs;
mod widgets;

pub(super) use content::*;
use refs::*;
pub(super) use widgets::*;

/// 一块像素：宽、高和 RGBA。
#[derive(Clone, Copy, Debug)]
struct PixelsRef<'a> {
    width: u32,
    height: u32,
    rgba: &'a [u8],
}

/// 自有的一块像素，挂在信号里。
#[derive(Clone, Debug)]
pub(super) struct Pixels {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
}

impl Pixels {
    pub(super) fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub(super) fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

impl PixelsRef<'_> {
    fn to_owned(self) -> Pixels {
        Pixels {
            width: self.width,
            height: self.height,
            rgba: self.rgba.into(),
        }
    }

    fn identity(&self, hasher: &mut DefaultHasher) {
        (
            self.width,
            self.height,
            self.rgba.as_ptr() as usize,
            self.rgba.len(),
        )
            .hash(hasher);
    }
}

/// 没有预览插件时画的东西：列表里同一路径的缩略图纹理，或者类型图标。
#[derive(Clone, Copy, Debug)]
enum FallbackRef<'a> {
    Texture(&'a str),
    Icon(Icon),
}

/// 读取中的状态。
#[derive(Clone, Debug)]
enum LoadingRef<'a> {
    /// 左上角的状态胶囊；`progress` 时多一条进度。PDF 和 Office 在顶栏下面。
    Pill {
        title: &'static str,
        detail: String,
        progress: bool,
        toolbar: Option<DocumentBar>,
    },
    /// 压缩包：居中的两行。
    Archive { detail: String },
    /// 音视频：居中的准备卡，写文件名。
    /// 音视频的准备卡。`video` 时写「准备媒体」，否则写「准备音频」。
    Media { name: &'a str, video: bool },
}

/// 预览框内容，借用 ViewModel 里的大块数据。身份和自有副本都由它算出，判断只写一遍。
#[derive(Clone, Debug)]
enum ContentRef<'a> {
    /// 播放列表当前图片的这一帧，按「适应 / 填充」铺。
    Still {
        pixels: PixelsRef<'a>,
        cover: bool,
    },
    Text {
        markdown: bool,
        text: &'a str,
        truncated_at: Option<u64>,
        extension: &'a str,
    },
    /// 宿主已经上传纹理的图片。
    ImageTexture,
    ImagePixels(PixelsRef<'a>),
    /// 宿主纹理槽 `file-preview` 上的视频画面。
    Video {
        cover: bool,
    },
    /// 离屏没有宿主纹理时，直接画解码出的这一帧。
    VideoPixels {
        pixels: PixelsRef<'a>,
        cover: bool,
    },
    Audio {
        name: &'a str,
        viewport: f32,
        lines: &'a [super::super::inspect_lyrics::LyricLine],
        active: Option<usize>,
        /// 歌词文件还在读。没有行时写「读取歌词...」。
        pending: bool,
        caption: super::audio_stage::AudioCaption,
        /// 本地封面路径。空表示没有可读的图，舞台画封面圆。
        cover_path: Option<&'a str>,
        accent: nana_ui::theme::ThemeAppearance,
    },
    /// 压缩包：左边名单，右边舞台。`shown` 有值时舞台是这一条；否则标题和说明是空闲、准备、不能内联或失败。
    Archive {
        content: &'a str,
        picked: Option<&'a str>,
        stage_title: &'a str,
        stage_detail: &'a str,
        stage_failed: bool,
        name: &'a str,
        shown: Option<ArchiveShown<'a>>,
    },
    Model {
        extension: &'a str,
        raster: Option<PixelsRef<'a>>,
        content: &'a str,
        mesh: bool,
        extent: &'a str,
    },
    /// PDF 和 Office：`nav` 是当前页和总页，有就画翻页（失败的页也画，才翻得过去）；`page` 是这一页的位图。
    Document {
        toolbar: Option<DocumentBar>,
        nav: Option<(usize, usize)>,
        page: Option<PixelsRef<'a>>,
        error: Option<&'a str>,
        text: &'a str,
    },
    /// 失败浮层。`document` 是 PDF / Office：外面多包一层整列，顶栏可有可无。`retry` 是在线条目的播放源准备
    /// 失败，原因下面给「重试」。
    Failed {
        title: &'static str,
        message: &'a str,
        toolbar: Option<DocumentBar>,
        document: bool,
        retry: bool,
    },
    Loading(LoadingRef<'a>),
    Fallback(FallbackRef<'a>),
    /// 读取中、还不知道类型。
    Pending,
}

/// 预览框里的内容和外框样子，身份变了才重建。
#[derive(Clone, Debug)]
pub(super) struct PreviewContent {
    /// 插件预览：框顶一圈强调色光晕，否则是 bg 底。
    pub plugin: bool,
    /// 滚动区键里的仓库和路径（`{仓库}-{路径}`），换文件时回到顶部。
    pub scope: String,
    body: Body,
}

#[derive(Clone, Debug)]
enum Body {
    Still {
        pixels: Pixels,
        cover: bool,
    },
    Text {
        markdown: bool,
        text: Arc<str>,
        truncated_at: Option<u64>,
        extension: String,
    },
    ImageTexture,
    ImagePixels(Pixels),
    Video {
        cover: bool,
    },
    VideoPixels {
        pixels: Pixels,
        cover: bool,
    },
    Audio {
        viewport: f32,
        lines: Vec<super::super::inspect_lyrics::LyricLine>,
        active: Option<usize>,
        pending: bool,
        caption: super::audio_stage::AudioCaption,
        /// 封面 data URL。空表示没有真实像素，舞台画封面圆。
        cover: Option<String>,
        accent: nana_ui::theme::ThemeAppearance,
    },
    Archive {
        content: String,
        picked: Option<String>,
        stage_title: String,
        stage_detail: String,
        stage_failed: bool,
        name: String,
        shown: Option<ArchiveShownOwned>,
    },
    Model {
        extension: String,
        raster: Option<Pixels>,
        content: String,
        mesh: bool,
        extent: String,
    },
    Document {
        toolbar: Option<DocumentBar>,
        nav: Option<(usize, usize)>,
        page: Option<Pixels>,
        error: Option<String>,
        text: String,
    },
    Failed {
        title: &'static str,
        message: String,
        toolbar: Option<DocumentBar>,
        document: bool,
        retry: bool,
    },
    Pill {
        title: &'static str,
        detail: String,
        progress: bool,
        toolbar: Option<DocumentBar>,
    },
    ArchiveLoading {
        detail: String,
    },
    MediaLoading {
        name: String,
        video: bool,
    },
    FallbackTexture(String),
    FallbackIcon(Icon),
    Pending,
}

/// 不在预览时的身份。
const IDLE: u64 = 0;

/// 预览框内容的身份和内容。比较只看身份。
#[derive(Clone, Debug)]
pub(super) struct PreviewSnapshot {
    key: u64,
    pub content: Arc<PreviewContent>,
}

impl PartialEq for PreviewSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl PreviewSnapshot {
    /// 按 ViewModel 取预览框内容；`previous` 的身份没变时原样沿用，不抄大块数据。
    /// 不在预览时预览页藏着，内容是空的，不抄也不编码任何像素。
    pub(super) fn project(
        model: &ShellViewModel,
        previous: Option<&PreviewSnapshot>,
    ) -> PreviewSnapshot {
        if !super::super::files_view::previewing(model) {
            let idle = PreviewContent {
                plugin: true,
                scope: String::new(),
                body: Body::Pending,
            };
            return match previous.filter(|previous| previous.key == IDLE) {
                Some(previous) => previous.clone(),
                None => PreviewSnapshot {
                    key: IDLE,
                    content: Arc::new(idle),
                },
            };
        }
        let (content, plugin) = resolve(model);
        let scope = scope(model);
        let mut hasher = DefaultHasher::new();
        plugin.hash(&mut hasher);
        scope.hash(&mut hasher);
        content.identity(&mut hasher);
        let key = hasher.finish();
        if let Some(previous) = previous.filter(|previous| previous.key == key) {
            return previous.clone();
        }
        PreviewSnapshot {
            key,
            content: Arc::new(PreviewContent {
                plugin,
                scope,
                body: content.to_owned(),
            }),
        }
    }
}

/// 滚动区键里的仓库和路径。
pub(super) fn scope(model: &ShellViewModel) -> String {
    let repo = super::super::player_view::key_part(
        model
            .workspace
            .active_repo_id
            .as_deref()
            .unwrap_or_default(),
    );
    let path = super::super::player_view::key_part(
        model.inspect.target_path.as_deref().unwrap_or_default(),
    );
    format!("{repo}-{path}")
}

/// 和画面同一棵判断树：预览框里该画什么，以及外框是不是插件预览。
fn resolve(model: &ShellViewModel) -> (ContentRef<'_>, bool) {
    if let Some(still) = current_still(model) {
        return (still, false);
    }
    let inspect = &model.inspect;
    let kind = inspect.kind.as_ref();
    match &inspect.body {
        PreviewBody::Document {
            markdown,
            text,
            truncated_at,
        } => {
            let extension = inspect.facts.extension.as_str();
            (
                ContentRef::Text {
                    markdown: *markdown,
                    text,
                    truncated_at: *truncated_at,
                    extension,
                },
                true,
            )
        }
        PreviewBody::Image => (image_ref(model), true),
        PreviewBody::Media(session) if session.status == "failed" => {
            (media_failed_ref(model, session.error.as_deref()), false)
        }
        PreviewBody::Media(_) => (media_ref(model), false),
        PreviewBody::Native {
            view_id,
            label,
            content,
        } => (native_ref(model, view_id, label, content), true),
        PreviewBody::Failed(message) => match kind {
            Some(PreviewKind::Unsupported) | None => {
                (ContentRef::Fallback(fallback_ref(model)), false)
            }
            Some(kind) => (
                failed_ref(model, kind, message),
                !matches!(kind, PreviewKind::Media),
            ),
        },
        PreviewBody::Empty => match kind {
            Some(PreviewKind::Unsupported) | None if !inspect.loading => {
                (ContentRef::Fallback(fallback_ref(model)), false)
            }
            Some(kind) => (
                loading_ref(model, kind),
                !matches!(kind, PreviewKind::Media),
            ),
            None => (ContentRef::Pending, true),
        },
    }
}

/// 预览框这一帧是不是从宿主纹理槽 `file-preview` 取画面：预览页开着，画的是图片纹理或视频纹理。
/// 宿主只在这时上传 `preview_pixels`（`thumbnail_host::publish_preview`）。离屏没有宿主纹理、只有解码像素时
/// 不取槽，直接把那一帧画进预览。单击只选中时预览页藏着；预览的正是幻灯片当前项时画幻灯片的帧；
/// PDF 和模型页图编成内容图。这几种都不从纹理槽取。
pub(crate) fn wants_preview_texture(model: &ShellViewModel) -> bool {
    if !super::super::files_view::previewing(model) {
        return false;
    }
    match resolve(model).0 {
        ContentRef::ImageTexture | ContentRef::Video { .. } => true,
        ContentRef::Archive {
            shown: Some(shown), ..
        } if shown.uses_host_video() => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;

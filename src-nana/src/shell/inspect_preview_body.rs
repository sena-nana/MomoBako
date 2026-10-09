//! 预览框里的内容：文本、图片、音视频、PDF / Office、压缩包、3D 模型，以及读取中、失败和不支持。
//!
//! 结构对齐各预览插件（text-preview、media-preview、office-preview、preview-archive、
//! three-model-preview）和播放器挂载（player-audio 运行时）。Vue 里插件文字继承成白色、
//! 浮层定位到外层祖先、唱片尺寸按视口算而溢出预览框这类缺陷，这里按设计意图改正。
//!
//! 常驻：预览框外框常驻，框里的内容按身份整块换。同步时先沿着和画面同一棵判断树取出借用 ViewModel 的
//! [`ContentRef`]，算出身份（大块像素和文本按缓冲区地址和长度认，它们整份替换，不原地改）；身份变了
//! 才抄成自有的 [`PreviewContent`] 写进信号，预览框里的 `dynamic` 换出新内容。播放推进只换纹理，不改身份。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, GpuTextureView, IconGlyph, JustifySpec, LengthSpec, RadiusTier, ScrollAxes, ScrollView,
    SemanticColorRole, Stack, Text, TextHorizontalAlignment, Thumbnail,
};
use nana_ui::ContentFit;
use nana_ui_core::Icon;

use super::super::inspect::{InspectMessage, PreviewBody, PreviewKind};
use super::super::inspect_library::{self, DocumentBar};
use super::super::player_view::icons;
use super::super::{ShellMessage, ShellViewModel};
use super::frame::text;

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
        Pixels { width: self.width, height: self.height, rgba: self.rgba.into() }
    }

    fn identity(&self, hasher: &mut DefaultHasher) {
        (self.width, self.height, self.rgba.as_ptr() as usize, self.rgba.len()).hash(hasher);
    }
}

/// 没有预览插件时画的东西：列表里同一路径的缩略图像素、纹理，或者类型图标。
#[derive(Clone, Copy, Debug)]
enum FallbackRef<'a> {
    Pixels(PixelsRef<'a>),
    Texture(&'a str),
    Icon(Icon),
}

/// 读取中的状态。
#[derive(Clone, Debug)]
enum LoadingRef<'a> {
    /// 左上角的状态胶囊；`progress` 时多一条进度。PDF 和 Office 在顶栏下面。
    Pill { title: &'static str, detail: String, progress: bool, toolbar: Option<DocumentBar> },
    /// 压缩包：居中的两行。
    Archive { detail: String },
    /// 音视频：居中的准备卡，写文件名。
    Media { name: &'a str },
}

/// 预览框内容，借用 ViewModel 里的大块数据。身份和自有副本都由它算出，判断只写一遍。
#[derive(Clone, Debug)]
enum ContentRef<'a> {
    /// 播放列表当前图片的这一帧，按「适应 / 填充」铺。
    Still { pixels: PixelsRef<'a>, cover: bool },
    Text { markdown: bool, text: &'a str, truncated_at: Option<u64>, extension: &'a str },
    /// 宿主已经上传纹理的图片。
    ImageTexture,
    ImagePixels(PixelsRef<'a>),
    Video { cover: bool },
    Audio { name: &'a str, viewport: f32 },
    Archive(&'a str),
    Model { extension: &'a str, raster: Option<PixelsRef<'a>>, content: &'a str, mesh: bool },
    Document { toolbar: Option<DocumentBar>, page: Option<(PixelsRef<'a>, Option<(usize, usize)>)>, error: Option<&'a str>, text: &'a str },
    /// 失败浮层。`document` 是 PDF / Office：外面多包一层整列，顶栏可有可无。
    Failed { title: &'static str, message: &'a str, toolbar: Option<DocumentBar>, document: bool },
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
    Still { pixels: Pixels, cover: bool },
    Text { markdown: bool, text: Arc<str>, truncated_at: Option<u64>, extension: String },
    ImageTexture,
    ImagePixels(Pixels),
    Video { cover: bool },
    Audio { name: String, viewport: f32 },
    Archive(String),
    Model { extension: String, raster: Option<Pixels>, content: String, mesh: bool },
    Document { toolbar: Option<DocumentBar>, page: Option<(Pixels, Option<(usize, usize)>)>, error: Option<String>, text: String },
    Failed { title: &'static str, message: String, toolbar: Option<DocumentBar>, document: bool },
    Pill { title: &'static str, detail: String, progress: bool, toolbar: Option<DocumentBar> },
    ArchiveLoading { detail: String },
    MediaLoading { name: String },
    FallbackPixels(Pixels),
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
    pub(super) fn project(model: &ShellViewModel, previous: Option<&PreviewSnapshot>) -> PreviewSnapshot {
        if !super::super::files_view::previewing(model) {
            let idle = PreviewContent { plugin: true, scope: String::new(), body: Body::Pending };
            return match previous.filter(|previous| previous.key == IDLE) {
                Some(previous) => previous.clone(),
                None => PreviewSnapshot { key: IDLE, content: Arc::new(idle) },
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
        PreviewSnapshot { key, content: Arc::new(PreviewContent { plugin, scope, body: content.to_owned() }) }
    }
}

/// 滚动区键里的仓库和路径。
pub(super) fn scope(model: &ShellViewModel) -> String {
    let repo = super::super::player_view::key_part(model.workspace.active_repo_id.as_deref().unwrap_or_default());
    let path = super::super::player_view::key_part(model.inspect.target_path.as_deref().unwrap_or_default());
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
        PreviewBody::Document { markdown, text, truncated_at } => {
            let extension = inspect.facts.extension.as_str();
            (ContentRef::Text { markdown: *markdown, text, truncated_at: *truncated_at, extension }, true)
        }
        PreviewBody::Image => (image_ref(model), true),
        PreviewBody::Media(_) => (media_ref(model), false),
        PreviewBody::Native { view_id, label, content } => (native_ref(model, view_id, label, content), true),
        PreviewBody::Failed(message) => match kind {
            Some(PreviewKind::Unsupported) | None => (ContentRef::Fallback(fallback_ref(model)), false),
            Some(kind) => (failed_ref(model, kind, message), !matches!(kind, PreviewKind::Media)),
        },
        PreviewBody::Empty => match kind {
            Some(PreviewKind::Unsupported) | None if !inspect.loading => (ContentRef::Fallback(fallback_ref(model)), false),
            Some(kind) => (loading_ref(model, kind), !matches!(kind, PreviewKind::Media)),
            None => (ContentRef::Pending, true),
        },
    }
}

/// 预览的正是播放列表当前的图片：和 Vue 的播放器挂载一样画幻灯片这一帧，按「适应 / 填充」铺。
/// 没有帧（读不到或解不开）时交回普通图片预览，由它写失败原因。
fn current_still(model: &ShellViewModel) -> Option<ContentRef<'_>> {
    let still = model.player.still.as_ref()?;
    let frame = still.frame.as_ref()?;
    let same_repo = model.player.repo_id.is_some() && model.player.repo_id == model.workspace.active_repo_id;
    if !same_repo || model.inspect.target_path.as_deref() != Some(still.path.as_str()) {
        return None;
    }
    let pixels = PixelsRef { width: frame.width, height: frame.height, rgba: &frame.rgba };
    Some(ContentRef::Still { pixels, cover: model.player.settings.object_fit_cover })
}

/// 图片：宿主已经上传纹理时用纹理槽，离屏只有像素时画成内容图，都没有时是读取中。
fn image_ref(model: &ShellViewModel) -> ContentRef<'_> {
    match (&model.preview_token, &model.preview_pixels) {
        (Some(_), Some(_)) => ContentRef::ImageTexture,
        (_, Some(pixels)) => ContentRef::ImagePixels(PixelsRef { width: pixels.width, height: pixels.height, rgba: &pixels.rgba }),
        (_, None) => loading_ref(model, &PreviewKind::Image),
    }
}

/// 播放器挂载：视频是画面，音频是唱片舞台。
fn media_ref(model: &ShellViewModel) -> ContentRef<'_> {
    let video = model.preview_token.as_deref().is_some_and(|token| token.starts_with("video:"));
    if video && model.preview_pixels.is_some() {
        return ContentRef::Video { cover: model.player.settings.object_fit_cover };
    }
    ContentRef::Audio { name: target_name(model), viewport: model.viewport_width }
}

/// PDF 和 Office：顶栏、翻页和整页；压缩包是目录；模型是画面和尺寸标签。
fn native_ref<'a>(model: &'a ShellViewModel, view_id: &str, label: &'a str, content: &'a str) -> ContentRef<'a> {
    let inspect = &model.inspect;
    if inspect_library::is_archive(view_id) {
        return ContentRef::Archive(content);
    }
    let raster = inspect.raster_frame().map(|(width, height, rgba)| PixelsRef { width, height, rgba });
    if inspect_library::is_model(view_id) {
        return ContentRef::Model { extension: &inspect.facts.extension, raster, content, mesh: inspect.has_mesh() };
    }
    let toolbar = document_toolbar(model, view_id, label);
    let page = raster.map(|raster| (raster, inspect.page_nav()));
    let text = if content.is_empty() { label } else { content };
    ContentRef::Document { toolbar, page, error: inspect.raster_error(), text }
}

/// PDF 和 Office 才有种类、扩展名和页数这条顶栏；别的插件视图没有。
fn document_toolbar(model: &ShellViewModel, view_id: &str, label: &str) -> Option<DocumentBar> {
    if !inspect_library::is_office_pdf(view_id) {
        return None;
    }
    inspect_library::document_bar(model, label)
}

/// 失败：浮层中间一行红色标题和一行弱色原因。PDF 保留顶栏。
fn failed_ref<'a>(model: &'a ShellViewModel, kind: &'a PreviewKind, message: &'a str) -> ContentRef<'a> {
    let (title, toolbar, document) = match kind {
        PreviewKind::Markdown | PreviewKind::Text => ("无法预览该文本", None, false),
        PreviewKind::Image | PreviewKind::Media => ("无法预览该媒体", None, false),
        PreviewKind::Native { view_id, .. } if inspect_library::is_archive(view_id) => ("无法预览该压缩包", None, false),
        PreviewKind::Native { view_id, .. } if inspect_library::is_model(view_id) => ("无法预览该模型", None, false),
        PreviewKind::Native { view_id, label } => ("无法预览该文档", document_toolbar(model, view_id, label), true),
        PreviewKind::Unsupported => return ContentRef::Fallback(fallback_ref(model)),
    };
    ContentRef::Failed { title, message, toolbar, document }
}

/// 读取中。文本、图片、模型是左上角的状态胶囊；音视频是居中的准备卡；PDF 保留顶栏。
fn loading_ref<'a>(model: &'a ShellViewModel, kind: &'a PreviewKind) -> ContentRef<'a> {
    let size = model.inspect.facts.size_label.trim();
    let sized = |empty: &str| if size.is_empty() { empty.to_string() } else { format!("准备 {size}") };
    let pill = |title, detail, progress, toolbar| ContentRef::Loading(LoadingRef::Pill { title, detail, progress, toolbar });
    match kind {
        PreviewKind::Markdown | PreviewKind::Text => pill("读取文本", sized("准备文本内容"), false, None),
        PreviewKind::Image => pill("准备播放", "准备媒体".to_string(), true, None),
        PreviewKind::Media => ContentRef::Loading(LoadingRef::Media { name: target_name(model) }),
        PreviewKind::Native { view_id, .. } if inspect_library::is_archive(view_id) => {
            ContentRef::Loading(LoadingRef::Archive { detail: sized("准备内部目录") })
        }
        PreviewKind::Native { view_id, .. } if inspect_library::is_model(view_id) => pill("读取模型", "准备读取文件".to_string(), false, None),
        PreviewKind::Native { view_id, label } => {
            let title = if inspect_library::is_pdf(view_id) {
                "载入 PDF"
            } else if inspect_library::is_office_pdf(view_id) {
                "转换文档"
            } else {
                "读取预览"
            };
            pill(title, sized("建立预览"), false, document_toolbar(model, view_id, label))
        }
        PreviewKind::Unsupported => ContentRef::Fallback(fallback_ref(model)),
    }
}

/// 没有预览插件：有缩略图就按 contain 画缩略图（bg 底），否则画类型图标。
/// 缩略图取自当前列表里同一路径的行；不在列表里的文件（例如从搜索打开）只画图标。
fn fallback_ref(model: &ShellViewModel) -> FallbackRef<'_> {
    thumbnail_ref(model).unwrap_or_else(|| {
        let extension = model.inspect.facts.extension.trim().trim_start_matches('.').to_ascii_lowercase();
        let icon = match extension.as_str() {
            "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => icons::FILE_VIDEO,
            "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus" => icons::FILE_AUDIO,
            _ => icons::FILE_IMAGE,
        };
        FallbackRef::Icon(icon)
    })
}

fn thumbnail_ref(model: &ShellViewModel) -> Option<FallbackRef<'_>> {
    let path = model.inspect.target_path.as_deref()?;
    // 素材详情说没有缩略图就不画；有的话像素和纹理取自列表里同一路径的行。
    model.inspect.facts.thumbnail_path.as_deref().map(str::trim).filter(|slot| !slot.is_empty())?;
    let files = &model.files;
    let row = files.rows.iter().chain(files.virtual_rows.iter()).find(|row| row.path == path)?;
    if let Some((width, height, rgba)) = row.thumbnail_rgba.as_ref() {
        return Some(FallbackRef::Pixels(PixelsRef { width: *width, height: *height, rgba }));
    }
    let slot = row.thumbnail_path.as_deref().map(str::trim).filter(|slot| !slot.is_empty())?;
    row.texture_ready.then_some(FallbackRef::Texture(slot))
}

/// 预览目标的文件名。
fn target_name(model: &ShellViewModel) -> &str {
    let path = model.inspect.target_path.as_deref().unwrap_or_default();
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

impl ContentRef<'_> {
    /// 身份：画面读到的每样东西。大块像素和文本只认缓冲区地址和长度。
    fn identity(&self, hasher: &mut DefaultHasher) {
        std::mem::discriminant(self).hash(hasher);
        match self {
            Self::Still { pixels, cover } => {
                pixels.identity(hasher);
                cover.hash(hasher);
            }
            Self::Text { markdown, text, truncated_at, extension } => {
                (markdown, text.as_ptr() as usize, text.len(), truncated_at, extension).hash(hasher);
            }
            Self::ImageTexture | Self::Pending => {}
            Self::ImagePixels(pixels) => pixels.identity(hasher),
            Self::Video { cover } => cover.hash(hasher),
            Self::Audio { name, viewport } => (name, viewport.to_bits()).hash(hasher),
            Self::Archive(content) => content.hash(hasher),
            Self::Model { extension, raster, content, mesh } => {
                (extension, content, mesh).hash(hasher);
                raster.iter().for_each(|raster| raster.identity(hasher));
            }
            Self::Document { toolbar, page, error, text } => {
                (toolbar, error, text).hash(hasher);
                if let Some((raster, nav)) = page {
                    raster.identity(hasher);
                    nav.hash(hasher);
                }
            }
            Self::Failed { title, message, toolbar, document } => (title, message, toolbar, document).hash(hasher),
            Self::Loading(loading) => match loading {
                LoadingRef::Pill { title, detail, progress, toolbar } => (0u8, title, detail, progress, toolbar).hash(hasher),
                LoadingRef::Archive { detail } => (1u8, detail).hash(hasher),
                LoadingRef::Media { name } => (2u8, name).hash(hasher),
            },
            Self::Fallback(fallback) => match fallback {
                FallbackRef::Pixels(pixels) => pixels.identity(hasher),
                FallbackRef::Texture(slot) => slot.hash(hasher),
                FallbackRef::Icon(icon) => (icon.as_ptr() as usize).hash(hasher),
            },
        }
    }

    /// 抄成自有的内容。只在身份变了时调用。
    fn to_owned(self) -> Body {
        match self {
            Self::Still { pixels, cover } => Body::Still { pixels: pixels.to_owned(), cover },
            Self::Text { markdown, text, truncated_at, extension } => {
                Body::Text { markdown, text: text.into(), truncated_at, extension: extension.to_string() }
            }
            Self::ImageTexture => Body::ImageTexture,
            Self::ImagePixels(pixels) => Body::ImagePixels(pixels.to_owned()),
            Self::Video { cover } => Body::Video { cover },
            Self::Audio { name, viewport } => Body::Audio { name: name.to_string(), viewport },
            Self::Archive(content) => Body::Archive(content.to_string()),
            Self::Model { extension, raster, content, mesh } => {
                Body::Model { extension: extension.to_string(), raster: raster.map(PixelsRef::to_owned), content: content.to_string(), mesh }
            }
            Self::Document { toolbar, page, error, text } => Body::Document {
                toolbar,
                page: page.map(|(raster, nav)| (raster.to_owned(), nav)),
                error: error.map(str::to_string),
                text: text.to_string(),
            },
            Self::Failed { title, message, toolbar, document } => Body::Failed { title, message: message.to_string(), toolbar, document },
            Self::Loading(LoadingRef::Pill { title, detail, progress, toolbar }) => Body::Pill { title, detail, progress, toolbar },
            Self::Loading(LoadingRef::Archive { detail }) => Body::ArchiveLoading { detail },
            Self::Loading(LoadingRef::Media { name }) => Body::MediaLoading { name: name.to_string() },
            Self::Fallback(FallbackRef::Pixels(pixels)) => Body::FallbackPixels(pixels.to_owned()),
            Self::Fallback(FallbackRef::Texture(slot)) => Body::FallbackTexture(slot.to_string()),
            Self::Fallback(FallbackRef::Icon(icon)) => Body::FallbackIcon(icon),
            Self::Pending => Body::Pending,
        }
    }
}

/// 预览框里的内容。
pub(super) fn content_view(content: &PreviewContent) -> AnyView {
    let scope = &content.scope;
    match &content.body {
        Body::Still { pixels, cover } => pixel_image(pixels, *cover, "inspect-player-still"),
        Body::Text { markdown, text, truncated_at, extension } => text_plugin(*markdown, text, *truncated_at, extension, scope),
        Body::ImageTexture => {
            // `.media-preview { background: var(--bg) }`：留白处是 bg，不透出预览框的插件光晕。
            widget(Stack::fill_column(0.0).surface(SemanticColorRole::Background))
                .children((widget(GpuTextureView::new("file-preview").contain()).key("inspect-image"),))
                .key("inspect-image-frame")
                .into_any()
        }
        Body::ImagePixels(pixels) => pixel_image(pixels, false, "inspect-image"),
        Body::Video { cover } => {
            let fit = if *cover { ContentFit::Cover } else { ContentFit::Contain };
            widget(Stack::fill_column(0.0).with_layout(|layout| layout.background = Some([0.02, 0.02, 0.02, 1.0])))
                .children((widget(GpuTextureView::new("file-preview").fit(fit)).key("inspect-video"),))
                .key("inspect-video-frame")
                .into_any()
        }
        Body::Audio { name, viewport } => super::audio_stage::audio_stage(name, *viewport),
        Body::Archive(content) => inspect_library::archive_list(content),
        Body::Model { extension, raster, content, mesh } => model_view(extension, raster.as_ref(), content, *mesh),
        Body::Document { toolbar, page, error, text } => document_view(toolbar.as_ref(), page.as_ref(), error.as_deref(), text, scope),
        Body::Failed { title, message, toolbar, document } => {
            let overlay = overlay(title, message, "inspect-failed");
            if !document {
                return overlay;
            }
            let mut rows = toolbar.iter().map(inspect_library::document_toolbar_view).collect::<Vec<_>>();
            rows.push(overlay);
            widget(Stack::fill_column(0.0)).children(rows).into_any()
        }
        Body::Pill { title, detail, progress, toolbar } => {
            status_layer(status_pill(title, detail, *progress), toolbar.as_ref().map(inspect_library::document_toolbar_view))
        }
        Body::ArchiveLoading { detail } => centered_status("读取压缩包", detail),
        Body::MediaLoading { name } => super::audio_stage::loading_card(name),
        Body::FallbackPixels(pixels) => pixel_image(pixels, false, "inspect-thumbnail"),
        Body::FallbackTexture(slot) => {
            let mut thumb = Thumbnail::new(super::super::thumbs::thumbnail_slot(slot)).fit(ContentFit::Contain);
            {
                let layout = std::sync::Arc::make_mut(&mut thumb.style.layout);
                layout.width = Some(LengthSpec::Fill);
                layout.height = Some(LengthSpec::Fill);
            }
            widget(Stack::fill_column(0.0).surface(SemanticColorRole::Background))
                .children((widget(thumb).key("inspect-thumbnail-texture"),))
                .key("inspect-thumbnail")
                .into_any()
        }
        Body::FallbackIcon(icon) => widget(Stack::fill_column(0.0).align(AlignSpec::Center).justify(JustifySpec::Center))
            .children((widget(IconGlyph::new(*icon).size(54.0).role(SemanticColorRole::Faint)).key("inspect-fallback-icon"),))
            .key("inspect-fallback")
            .into_any(),
        Body::Pending => widget(Stack::fill_column(0.0)).key("inspect-pending").into_any(),
    }
}

/// 文本插件：上面一条种类和行数，下面是等宽原文。Markdown 也按原文显示。
fn text_plugin(markdown: bool, source: &str, truncated_at: Option<u64>, extension: &str, scope: &str) -> AnyView {
    let extension = extension.trim().trim_start_matches('.').to_ascii_uppercase();
    let kind = if markdown {
        "Markdown".to_string()
    } else if extension.is_empty() {
        "TEXT".to_string()
    } else {
        extension
    };
    let lines = if source.is_empty() { 0 } else { source.replace("\r\n", "\n").replace('\r', "\n").split('\n').count() };
    let mut chips = vec![chip(&kind, true, "inspect-text-kind"), chip(&format!("{lines} 行"), false, "inspect-text-lines")];
    if let Some(read) = truncated_at {
        chips.push(chip(&format!("仅显示前 {}", format_byte_count(read)), false, "inspect-text-truncated"));
    }
    let shown = if source.is_empty() { "空文件" } else { source };
    let mut body = Text::new(shown).color(SemanticColorRole::Text).font_size(13.0).line_height(21.45);
    {
        let layout = std::sync::Arc::make_mut(&mut body.style.layout);
        layout.font_family = Some("monospace".into());
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.line_break = Some(nana_ui_core::LineBreakSpec::Anywhere);
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
        // `white-space: pre-wrap`：保留原文的换行和空格，长行在框内折行。
        layout.white_space = nana_ui_core::WhiteSpaceSpec::PreWrap;
    }
    // 全局 `pre`：bg-subtle 底、border-soft 细边、8px 圆角、12/14 内边距，至少铺满内容区。
    let sheet = widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md)
            .with_layout(|layout| {
                layout.padding_top = Some(LengthSpec::Px(12.0));
                layout.padding_bottom = Some(LengthSpec::Px(12.0));
                layout.padding_left = Some(LengthSpec::Px(14.0));
                layout.padding_right = Some(LengthSpec::Px(14.0));
                layout.min_height = Some(LengthSpec::Percent(100.0));
                layout.flex_grow = Some(1.0);
            }),
    )
    .children((widget(body).key("inspect-text"),))
    .key("inspect-text-sheet");
    let content = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
        layout.padding_top = Some(LengthSpec::Px(20.0));
        layout.padding_bottom = Some(LengthSpec::Px(20.0));
        layout.padding_left = Some(LengthSpec::Px(22.0));
        layout.padding_right = Some(LengthSpec::Px(22.0));
    }))
    .children((widget(Stack::fill_column(0.0).min_height(LengthSpec::Percent(100.0))).children((sheet,)),))
    .key(format!("preview-text-{scope}"));
    // 插件根自带 bg 底，盖住预览框顶部的插件光晕，和 `.text-preview { background: var(--bg) }` 一致。
    widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0)).surface(SemanticColorRole::Background))
        .children((toolbar(chips, "inspect-text-toolbar"), content.into_any()))
        .key("inspect-text-plugin")
        .into_any()
}

/// 插件顶栏：42px 起，8/12 内边距，底边 border-soft，标签可折行。
fn toolbar(chips: Vec<AnyView>, key: &'static str) -> AnyView {
    let bar = super::super::workbench::with_bottom_divider(
        Stack::bar(8.0).wrap(true).align(AlignSpec::Center).min_height(LengthSpec::Px(42.0)).with_layout(|layout| {
            layout.padding_top = Some(LengthSpec::Px(8.0));
            layout.padding_bottom = Some(LengthSpec::Px(8.0));
            layout.padding_left = Some(LengthSpec::Px(12.0));
            layout.padding_right = Some(LengthSpec::Px(12.0));
        }),
    );
    widget(bar).children(chips).key(key).into_any()
}

/// 顶栏标签：bg-elev 底、border 细边、8px 圆角，12px 字。第一枚是种类，正文色半粗。
fn chip(label: &str, emphasis: bool, key: &str) -> AnyView {
    let role = if emphasis { SemanticColorRole::Text } else { SemanticColorRole::Muted };
    let weight = if emphasis { 600 } else { 400 };
    widget(
        Stack::row(6.0)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(26.0))
            .padding_xy(8.0, 4.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Md)
            .grow(0.0)
            .shrink(0.0),
    )
    .children((widget(text(label, 12.0, weight, role, 18.6)),))
    .key(key.to_string())
    .into_any()
}

/// 把 RGBA 按 contain（或 cover）画进铺满的盒子，底色是 bg。
fn pixel_image(pixels: &Pixels, cover: bool, key: &'static str) -> AnyView {
    let Some(url) = super::super::inspect::rgba_png_data_url(pixels.width, pixels.height, &pixels.rgba) else {
        eprintln!("Nana 预览图编码失败：{}x{}", pixels.width, pixels.height);
        return widget(Stack::fill_column(0.0)).key(key).into_any();
    };
    let image = nana_ui_core::BackgroundImage::Url {
        url,
        fit: if cover { nana_ui_core::BackgroundImageFit::Cover } else { nana_ui_core::BackgroundImageFit::Contain },
        size_width: None,
        size_height: None,
        position: nana_ui_core::BackgroundPosition::center(),
        repeat: nana_ui_core::BackgroundRepeat::NoRepeat,
        sampling: nana_ui_core::ImageSampling::Resample,
    };
    widget(Stack::fill_column(0.0).surface(SemanticColorRole::Background).with_layout(|layout| {
        layout.paint.content_image = Some(image);
    }))
    .key(key)
    .into_any()
}

/// PDF 和 Office：顶栏、翻页和整页；读不出页面时是失败浮层或抽出的文字。
fn document_view(toolbar: Option<&DocumentBar>, page: Option<&(Pixels, Option<(usize, usize)>)>, error: Option<&str>, body: &str, scope: &str) -> AnyView {
    let mut rows = Vec::new();
    if let Some(bar) = toolbar {
        rows.push(inspect_library::document_toolbar_view(bar));
    }
    if let Some((raster, nav)) = page {
        let index = nav.map(|(index, _)| index).unwrap_or(0);
        let key = format!("preview-page-{scope}-{index}");
        rows.push(super::page::page_scroller(super::page::page_bitmap(raster), *nav, key));
    } else if let Some(error) = error {
        rows.push(overlay("无法预览该文档", error, "inspect-native-error"));
    } else {
        rows.push(
            widget(Stack::fill_column(0.0).padding(18.0))
                .children((widget(text(body, 13.0, 400, SemanticColorRole::Text, 20.15)).key("inspect-native"),))
                .key("inspect-native-text")
                .into_any(),
        );
    }
    // `.office-preview { background: var(--bg) }`：插件根自带 bg 底，盖住预览框的插件光晕。
    widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0)).surface(SemanticColorRole::Background))
        .children(rows)
        .key("inspect-native-view")
        .into_any()
}

/// 3D 模型：画面铺满，左下角是扩展名和尺寸标签。旋转、缩放按钮在右上角。
fn model_view(extension: &str, raster: Option<&Pixels>, content: &str, mesh: bool) -> AnyView {
    let extension = extension.trim().trim_start_matches('.').to_ascii_uppercase();
    let picture = match raster {
        Some(raster) => super::page::page_bitmap(raster),
        None => widget(Stack::fill_column(0.0).padding(18.0))
            .children((widget(text(content, 13.0, 400, SemanticColorRole::Muted, 20.15)),))
            .into_any(),
    };
    let mut layers = vec![picture];
    if !extension.is_empty() {
        layers.push(
            widget(Stack::row(8.0).with_layout(|layout| {
                layout.position = nana_ui_core::PositionSpec::Absolute;
                layout.offset_left = Some(LengthSpec::Px(12.0));
                layout.offset_bottom = Some(LengthSpec::Px(12.0));
            }))
            .children((hud_chip(&extension, "inspect-model-hud-ext"),))
            .key("inspect-model-hud")
            .into_any(),
        );
    }
    if mesh {
        layers.push(orbit_controls());
    }
    widget(Stack::fill_column(0.0).surface(SemanticColorRole::Background)).children(layers).key("inspect-model").into_any()
}

/// 模型的旋转和缩放。Vue 用拖拽手势，这里给同样功能的按钮。
fn orbit_controls() -> AnyView {
    let buttons = [("左转", -0.4, 1.0, "inspect-orbit-left"), ("右转", 0.4, 1.0, "inspect-orbit-right"), ("放大", 0.0, 1.25, "inspect-orbit-in"), ("缩小", 0.0, 0.8, "inspect-orbit-out")]
        .into_iter()
        .map(|(label, yaw, zoom, key)| {
            widget(nana_ui::runtime::Button::new(label).kind(nana_ui::ButtonKind::Ghost))
                .key(key)
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::Orbit { yaw, zoom }));
                })
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(Stack::row(4.0).with_layout(|layout| {
        layout.position = nana_ui_core::PositionSpec::Absolute;
        layout.offset_right = Some(LengthSpec::Px(12.0));
        layout.offset_top = Some(LengthSpec::Px(12.0));
    }))
    .children(buttons)
    .key("inspect-orbit")
    .into_any()
}

/// 画面上的小标签：26px 高，bg-elev 底、border 细边、8px 圆角，12px 半粗弱色字。
fn hud_chip(label: &str, key: &'static str) -> AnyView {
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(26.0))
            .padding_xy(8.0, 4.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((widget(text(label, 12.0, 600, SemanticColorRole::Muted, 18.6)),))
    .key(key)
    .into_any()
}

/// 浮层：铺满预览框，标题红色、原因弱色，最宽 520px，居中。
fn overlay(title: &str, message: &str, key: &'static str) -> AnyView {
    let mut heading = text(title, 14.0, 700, SemanticColorRole::Danger, 21.7);
    heading.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut reason = text(message, 14.0, 400, SemanticColorRole::Muted, 21.7);
    reason.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    {
        let layout = std::sync::Arc::make_mut(&mut reason.style.layout);
        layout.max_width = Some(LengthSpec::Px(520.0));
        layout.line_break = Some(nana_ui_core::LineBreakSpec::Anywhere);
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    widget(Stack::fill_column(8.0).align(AlignSpec::Center).justify(JustifySpec::Center).padding(24.0))
        .children((widget(heading).key(key), widget(reason).key(format!("{key}-reason"))))
        .key(format!("{key}-overlay"))
        .into_any()
}

/// 居中的两行状态：压缩包读取中用它。
fn centered_status(title: &str, detail: &str) -> AnyView {
    widget(Stack::fill_column(8.0).align(AlignSpec::Center).justify(JustifySpec::Center))
        .children((
            widget(text(title, 14.0, 600, SemanticColorRole::Text, 21.7)),
            widget(text(detail, 14.0, 400, SemanticColorRole::Muted, 21.7)),
        ))
        .key("inspect-status-center")
        .into_any()
}

/// 状态胶囊盖在预览框左上角（PDF 在顶栏下面）。
fn status_layer(pill: AnyView, toolbar: Option<AnyView>) -> AnyView {
    let top = if toolbar.is_some() { 54.0 } else { 12.0 };
    let pill = widget(Stack::row(0.0).with_layout(move |layout| {
        layout.position = nana_ui_core::PositionSpec::Absolute;
        layout.offset_left = Some(LengthSpec::Px(12.0));
        layout.offset_top = Some(LengthSpec::Px(top));
        layout.max_width = Some(LengthSpec::CalcPercentOffset { percent: 100.0, offset_px: -24.0 });
    }))
    .children((pill,))
    .key("inspect-status-layer")
    .into_any();
    let mut rows = Vec::new();
    if let Some(toolbar) = toolbar {
        rows.push(toolbar);
    }
    rows.push(pill);
    widget(Stack::fill_column(0.0)).children(rows).key("inspect-loading").into_any()
}

/// 状态胶囊：bg-elev 底、border-soft 细边、8px 圆角，第一段正文色半粗。媒体多一条 132×6 的进度。
fn status_pill(title: &str, detail: &str, progress: bool) -> AnyView {
    let mut children = vec![
        widget(text(title, 12.0, 600, SemanticColorRole::Text, 18.6)).into_any(),
        widget(text(detail, 12.0, 400, SemanticColorRole::Muted, 18.6).truncating()).into_any(),
    ];
    if progress {
        children.push(super::audio_stage::progress_track(132.0, "inspect-status-progress"));
    }
    widget(
        Stack::row(8.0)
            .align(AlignSpec::Center)
            .padding_xy(8.0, 5.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md),
    )
    .children(children)
    .key("inspect-status-pill")
    .into_any()
}

/// 和 Vue `formatByteCount` 一致：1024 以下写字节，其余保留一位小数。
pub(super) fn format_byte_count(value: u64) -> String {
    if value < 1024 {
        return format!("{value} B");
    }
    if value < 1024 * 1024 {
        return format!("{:.1} KB", value as f64 / 1024.0);
    }
    format!("{:.1} MB", value as f64 / 1024.0 / 1024.0)
}

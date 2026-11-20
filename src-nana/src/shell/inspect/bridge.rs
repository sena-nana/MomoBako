//! 预览接管播放条、PDF 翻页和模型视角。
//!
//! 音视频预览读好后交给播放器：插成临时条目接管播放条，或者正是当前项时跟着播放条走。
//! 播放、暂停、跳转和音量都在播放条上，预览页只显示画面。测试构建不打开声卡。
//! PDF 与网格画面留在这里，检查状态文件不再继续变长。

use super::super::player::{ClockStep, PreviewEntry};
use super::super::{PreviewPixels, ShellViewModel};
use super::{InspectEffect, InspectMessage, InspectState, PreviewBody};

/// 一页画出来的画面。`error` 有值时预览框写错误，不画这页。
#[derive(Clone, Debug)]
pub struct PageFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub text: String,
    pub error: Option<String>,
}

/// 可旋转的三角网格。FBX 和 BLEND 不会产生它。
#[derive(Clone, Debug)]
pub struct MeshData {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// 压缩包里已经读出的一条名字。目录路径不带末尾斜杠。
#[derive(Clone, Debug)]
pub struct ArchiveName {
    pub path: String,
    pub size: u64,
    pub directory: bool,
}

/// zip、cbz 和 7z 能按这些名字进入下一层。`layered` 为假时是 rar 或 cbr 的整路径列表，不能下钻。
#[derive(Clone, Debug)]
pub struct ArchiveCatalog {
    pub names: Vec<ArchiveName>,
    pub layered: bool,
    pub truncated: bool,
}

/// 已经打开的包内文件。正文是这一条取出的真实字节交给现有预览的结果。
#[derive(Clone, Debug)]
pub struct ArchiveEntryPreview {
    pub path: String,
    pub kind: ArchiveEntryKind,
}

/// 点开的一层嵌套 zip、cbz 或 7z。只留已经读出的名字，不留条目内容。
#[derive(Clone, Debug)]
pub struct ArchiveNest {
    /// 这一层在父容器里的条目路径。
    pub entry: String,
    pub catalog: ArchiveCatalog,
    pub directory: String,
    /// 现有名单用的当前层。头部路径从外层算起，面包屑和点行都认它。
    pub text: String,
}

/// 包内文件交给哪种现有预览。
#[derive(Clone, Debug)]
pub enum ArchiveEntryKind {
    /// 纯文本或 Markdown 原文。`truncated_at` 有值时只显示了开头这么多字节。
    Text {
        markdown: bool,
        extension: String,
        text: String,
        truncated_at: Option<u64>,
    },
    /// 解码后的真实像素。
    Image {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// 自绘 PDF 的页。`error` 有值时整份解析失败，用现有失败状态，不换成另一份 PDF。
    Pdf {
        frames: Vec<PageFrame>,
        index: usize,
        error: Option<String>,
    },
    /// 嵌套 zip/7z 的名单。只有真实名字。
    Nested { catalog: ArchiveCatalog },
    /// Office 抽出的纯文本。`error` 有值时抽取失败，用现有文档失败状态，不还原版式。
    Office { text: String, error: Option<String> },
    /// 软件光栅的模型。没有网格时只留文件头或结构摘要。`yaw` 和 `zoom` 跟预览里的旋转缩放同一套。
    /// `extent` 是包围盒三轴尺寸，没有网格时为空。
    Model {
        text: String,
        frame: Option<PageFrame>,
        mesh: Option<MeshData>,
        yaw: f32,
        zoom: f32,
        error: Option<String>,
        extent: String,
    },
    /// 现有解码器在内存里解出的音频。样本不留给播放条，也不打开声卡。
    /// `error` 有值时解不开，用现有音频失败状态。
    Audio {
        duration_ms: u64,
        sample_rate: u32,
        channels: u16,
        error: Option<String>,
    },
    /// 纯 Rust 解开的未压缩 AVI。只留画面，PCM 已丢掉，播放条不接管。
    /// `position_ms` 是这条预览自己的播放头，不跟播放条。没有宿主纹理时按它画已经解开的那一帧。
    Video {
        frames: Vec<super::support::VideoFrame>,
        position_ms: u64,
    },
}

/// 原生预览读完后的文本、页面和可选网格。压缩包另外带上已读条目名。
#[derive(Clone, Debug)]
pub struct NativeLoad {
    pub text: String,
    pub frames: Vec<PageFrame>,
    pub mesh: Option<MeshData>,
    pub paged: bool,
    pub archive: Option<ArchiveCatalog>,
}

impl NativeLoad {
    pub fn text_only(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            frames: Vec::new(),
            mesh: None,
            paged: false,
            archive: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Deck {
    index: usize,
    frames: Vec<PageFrame>,
    mesh: Option<MeshData>,
    /// 有网格时的包围盒尺寸，左下角标签用。没有网格时为空。
    extent: String,
    yaw: f32,
    zoom: f32,
    paged: bool,
    video: Option<Vec<super::support::VideoFrame>>,
}

pub(super) enum Follow {
    None,
    Media {
        path: String,
        pcm: Option<super::super::player::PreviewPcm>,
    },
    Autoplay {
        path: String,
        generation: u64,
    },
    Native,
}

/// 在归约消费消息之前抄出随后要做的接管、自动播放，或者原生预览读好以后放掉旧画面。
pub(super) fn follow(message: &InspectMessage) -> Follow {
    match message {
        InspectMessage::MediaLoaded {
            path,
            result: Ok(_),
            pcm,
            ..
        } => Follow::Media {
            path: path.clone(),
            pcm: pcm.clone(),
        },
        InspectMessage::Autoplay { path, generation } => Follow::Autoplay {
            path: path.clone(),
            generation: *generation,
        },
        InspectMessage::NativeLoaded { .. } => Follow::Native,
        _ => Follow::None,
    }
}

/// 会话已经写好之后再交给播放器、换画面。过期的媒体结果不会装进游标。
pub(super) fn apply(model: &mut ShellViewModel, follow: Follow) {
    match follow {
        Follow::None => {}
        Follow::Media { path, pcm } => {
            hand_media_to_player(model, &path, pcm);
            show_video_frame(model);
        }
        Follow::Autoplay { path, generation } => autoplay(model, &path, generation),
        Follow::Native => release_preview_pixels(model),
    }
}

/// 和 Vue 预览页挂载后 `playEntry` 一样开始播放。只在预览仍是这个文件、播放器仍在放它、
/// 而且停着的时候生效；用户在此之前已经点过播放或换了文件就不再动。
fn autoplay(model: &mut ShellViewModel, path: &str, generation: u64) {
    let current = generation == model.inspect.generation
        && model.inspect.target_path.as_deref() == Some(path);
    let repo_id = model
        .inspect
        .repo_id
        .clone()
        .or_else(|| model.workspace.active_repo_id.clone());
    if !current || !model.player.mirrors_preview(repo_id.as_deref(), Some(path)) {
        eprintln!("Nana 预览已经换掉，不再自动播放：{path}");
        return;
    }
    if model.player.session.status != "paused" {
        return;
    }
    model.player.play_from_preview(&mut model.inspect);
    show_video_frame(model);
}

pub(super) fn clear_deck(state: &mut InspectState) {
    state.deck = Deck::default();
}

pub(super) fn install(
    state: &mut InspectState,
    frames: Vec<PageFrame>,
    mesh: Option<MeshData>,
    paged: bool,
) {
    let extent = mesh
        .as_ref()
        .map(super::native_preview::model_extent_label)
        .unwrap_or_default();
    state.deck = Deck {
        index: 0,
        frames,
        mesh,
        extent,
        yaw: 0.0,
        zoom: 1.0,
        paged,
        video: None,
    };
}

/// 换上这次视频解出的画面。没有画面时清掉上一份，避免旧帧留在新文件上。
pub(super) fn install_video(
    state: &mut InspectState,
    frames: Option<Vec<super::support::VideoFrame>>,
) {
    state.deck.video = frames.filter(|frames| !frames.is_empty());
}

/// 取不晚于播放头的那一帧。文件预览和包内 AVI 都走这一条，不另涂色块。
pub(crate) fn frame_at(
    frames: &[super::support::VideoFrame],
    time_ms: u64,
) -> Option<&super::support::VideoFrame> {
    frames
        .iter()
        .rev()
        .find(|frame| frame.time_ms <= time_ms)
        .or_else(|| frames.first())
}

/// 取文件预览里不晚于播放头的那一帧。
pub(super) fn video_frame(
    state: &InspectState,
    time_ms: u64,
) -> Option<&super::support::VideoFrame> {
    frame_at(state.deck.video.as_ref()?, time_ms)
}

/// 播放时钟只有播放器一份：拨动后预览页拿到同一份会话，再换上对应的视频画面。
/// 换画面只换纹理；画面从无到有或从有到无时界面结构变了，按 `Changed` 报给调用方。
pub(super) fn advance_playback(
    model: &mut super::super::ShellViewModel,
    step_ms: u64,
) -> ClockStep {
    let before = super::super::listening_progress::playing_clip(&model.player);
    let playing = before.playing;
    let step = model.player.advance_clock(step_ms, &mut model.inspect);
    if playing {
        super::super::listening_progress::on_clock(model, &before, step);
    }
    if step == ClockStep::Idle {
        return step;
    }
    let showed = model.preview_pixels.is_some();
    show_video_frame(model);
    if model.preview_pixels.is_some() != showed {
        return ClockStep::Changed;
    }
    step
}

/// 翻到相邻页。越界只记日志，不把空白页当成新内容。
pub(super) fn turn(state: &mut InspectState, delta: i32) {
    if !state.deck.paged || state.deck.frames.is_empty() {
        eprintln!("Nana 当前预览不能翻页");
        return;
    }
    let len = state.deck.frames.len() as i32;
    let next = state.deck.index as i32 + delta;
    if next < 0 || next >= len {
        eprintln!("Nana PDF 页码越界：{next}");
        return;
    }
    state.deck.index = next as usize;
    write_page_text(state);
}

/// 指针拖拽换算成旋转和缩放。向右拖是正方向，和「右转」一致；向上拖放大。
/// 大约 40px 的横向拖动等于按钮的一档 0.4 弧度。没有位移时缩放系数是 1，画面不变。
pub(crate) fn orbit_drag(dx: f32, dy: f32) -> (f32, f32) {
    (dx * 0.01, (-dy * 0.005).exp())
}

/// 绕竖直轴旋转，或按比例缩放。没有网格时不假装已经转过。
pub(super) fn orbit(state: &mut InspectState, yaw: f32, zoom: f32) {
    let Some(mesh) = state.deck.mesh.clone() else {
        eprintln!("Nana 当前预览没有可旋转的网格");
        return;
    };
    state.deck.yaw += yaw;
    if zoom > 0.0 {
        state.deck.zoom = (state.deck.zoom * zoom).clamp(0.25, 8.0);
    }
    let frame = super::native_preview::paint_mesh(&mesh, state.deck.yaw, state.deck.zoom);
    if state.deck.frames.is_empty() {
        state.deck.frames.push(frame);
    } else {
        state.deck.frames[0] = frame;
    }
    state.deck.index = 0;
}

impl InspectState {
    pub(crate) fn showing_raster(&self) -> bool {
        self.deck
            .frames
            .get(self.deck.index)
            .is_some_and(|frame| frame.error.is_none() && !frame.rgba.is_empty())
    }

    pub(crate) fn raster_error(&self) -> Option<&str> {
        self.deck
            .frames
            .get(self.deck.index)
            .and_then(|frame| frame.error.as_deref())
    }

    /// 当前页和总页。第一页和最后一页用来禁用翻页按钮。
    pub(crate) fn page_nav(&self) -> Option<(usize, usize)> {
        if !self.deck.paged || self.deck.frames.is_empty() {
            return None;
        }
        Some((self.deck.index, self.deck.frames.len()))
    }

    pub(crate) fn has_mesh(&self) -> bool {
        self.deck.mesh.is_some()
    }

    /// 当前网格的包围盒尺寸。没有网格时是空串。
    pub(crate) fn mesh_extent(&self) -> &str {
        &self.deck.extent
    }

    pub(crate) fn raster_rgba(&self) -> Option<&[u8]> {
        self.deck
            .frames
            .get(self.deck.index)
            .map(|frame| frame.rgba.as_slice())
    }

    /// 当前页的宽、高和像素。空页和带错误的页不拿去画。
    pub(crate) fn raster_frame(&self) -> Option<(u32, u32, &[u8])> {
        if !self.showing_raster() {
            return None;
        }
        let rgba = self.raster_rgba()?;
        let frame = self.deck.frames.get(self.deck.index)?;
        if frame.width == 0 || frame.height == 0 {
            return None;
        }
        Some((frame.width, frame.height, rgba))
    }
}

/// 把页位图编成 PNG data URL。离屏绘制走 `content_image`，不依赖未注册的宿主纹理槽。
pub(crate) fn rgba_png_data_url(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    let pixels = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if rgba.len() < pixels {
        eprintln!("Nana 页位图像素不够：{width}x{height}，字节 {}", rgba.len());
        return None;
    }
    let mut png = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::png::PngEncoder::new(&mut png);
    if let Err(error) = image::ImageEncoder::write_image(
        encoder,
        &rgba[..pixels],
        width,
        height,
        image::ExtendedColorType::Rgba8,
    ) {
        eprintln!("Nana 页位图编码 PNG 失败：{error}");
        return None;
    }
    Some(format!(
        "data:image/png;base64,{}",
        encode_base64(&png.into_inner())
    ))
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let packed = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | bytes[index + 2] as u32;
        out.push(TABLE[((packed >> 18) & 63) as usize] as char);
        out.push(TABLE[((packed >> 12) & 63) as usize] as char);
        out.push(TABLE[((packed >> 6) & 63) as usize] as char);
        out.push(TABLE[(packed & 63) as usize] as char);
        index += 3;
    }
    match bytes.len() - index {
        1 => {
            let packed = (bytes[index] as u32) << 16;
            out.push(TABLE[((packed >> 18) & 63) as usize] as char);
            out.push(TABLE[((packed >> 12) & 63) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let packed = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
            out.push(TABLE[((packed >> 18) & 63) as usize] as char);
            out.push(TABLE[((packed >> 12) & 63) as usize] as char);
            out.push(TABLE[((packed >> 6) & 63) as usize] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

/// 预览的音视频读好了：交给播放器接管播放条（或者正是当前项时跟着它走）。
/// 真的接管了才排一次自动播放，由宿主派发，测试和离屏截图不会出声。
fn hand_media_to_player(
    model: &mut ShellViewModel,
    path: &str,
    pcm: Option<super::super::player::PreviewPcm>,
) {
    let Some(session) = model.inspect.media_session().cloned() else {
        return;
    };
    if model.inspect.target_path.as_deref() != Some(path) {
        return;
    }
    let repo_id = model
        .inspect
        .repo_id
        .clone()
        .or_else(|| model.workspace.active_repo_id.clone())
        .unwrap_or_default();
    let asset_id = model.inspect.asset_id.clone().unwrap_or_default();
    let extension = model.inspect.facts.extension.clone();
    let is_virtual = model.inspect.virtual_asset;
    let entry = PreviewEntry {
        repo_id: &repo_id,
        path,
        extension: &extension,
        asset_id: &asset_id,
        is_virtual,
    };
    if model
        .player
        .take_over_preview(entry, session, pcm, &mut model.inspect)
    {
        let generation = model.inspect.generation;
        model.inspect.effects.push(InspectEffect::Autoplay {
            path: path.to_string(),
            generation,
        });
    }
    super::super::listening_progress::on_duration(model);
}

fn write_page_text(state: &mut InspectState) {
    let Some(frame) = state.deck.frames.get(state.deck.index) else {
        return;
    };
    let caption = page_caption(state.deck.index, state.deck.frames.len(), frame);
    if let PreviewBody::Native { content, .. } = &mut state.body {
        *content = caption;
    }
}

pub(super) fn page_caption(index: usize, total: usize, frame: &PageFrame) -> String {
    let head = format!("{} / {total}", index + 1);
    if let Some(error) = &frame.error {
        if frame.text.trim().is_empty() {
            return format!("{head}\n{error}");
        }
    }
    let body = frame.text.trim();
    if body.is_empty() {
        head
    } else {
        format!("{head}\n{body}")
    }
}

/// 按预览会话的进度换上对应的视频画面；不是视频时清掉留下的视频帧。
///
/// 已经挂着宿主纹理记号（`video:`）时，记号跟着换到这一帧，真窗口重新上传。
/// 离屏把记号清掉、解码像素还留着时，只按播放头换上已经解出的那一帧，不把未注册的记号挂回去。
/// 第一次还没有像素时仍挂记号，否则真窗口没有东西可上传。
pub(super) fn show_video_frame(model: &mut ShellViewModel) {
    let time = model
        .inspect
        .media_session()
        .map(|session| session.current_time_ms)
        .unwrap_or(0);
    let Some(frame) = video_frame(&model.inspect, time) else {
        if model
            .preview_token
            .as_deref()
            .is_some_and(|token| token.starts_with("video:"))
        {
            model.preview_pixels = None;
            model.preview_token = None;
        }
        return;
    };
    let path = model.inspect.target_path.clone().unwrap_or_default();
    let token = format!("video:{path}:{}:{}", frame.time_ms, frame.width);
    if model.preview_token.as_deref() == Some(token.as_str()) {
        return;
    }
    // 记号空着且像素还在：离屏故意不挂槽。先记下，再换成这一帧的解码像素。
    let decoded_without_host = model.preview_token.is_none() && model.preview_pixels.is_some();
    model.preview_pixels = Some(PreviewPixels {
        width: frame.width,
        height: frame.height,
        rgba: frame.rgba.clone(),
    });
    if decoded_without_host {
        return;
    }
    model.preview_token = Some(token);
}

/// PDF、Office 和模型的页图由预览框直接从 `deck` 取（`raster_frame`），编成内容图画，不复制进
/// `preview_pixels`，也不上传宿主纹理。当前预览换成原生视图时放掉上一个文件留下的图片或视频画面。
/// 过期的读取结果没有装进去时预览体不是原生视图，当前文件的画面留着。
fn release_preview_pixels(model: &mut ShellViewModel) {
    if !matches!(model.inspect.body, PreviewBody::Native { .. }) {
        return;
    }
    model.preview_pixels = None;
    model.preview_token = None;
}

#[cfg(test)]
mod tests {
    use super::orbit_drag;

    /// 横向 40px 等于「右转」一档；向上拖放大，向下拖缩小，没有位移时缩放系数是 1。
    #[test]
    fn orbit_drag_turns_rightward_and_zooms_upward() {
        let (yaw, zoom) = orbit_drag(40.0, 0.0);
        assert!((yaw - 0.4).abs() < 0.001, "{yaw}");
        assert!((zoom - 1.0).abs() < 0.001, "{zoom}");
        let (yaw, zoom) = orbit_drag(0.0, -40.0);
        assert!(yaw.abs() < 0.001);
        assert!(zoom > 1.0, "向上拖放大：{zoom}");
        let (_, zoom) = orbit_drag(0.0, 40.0);
        assert!(zoom < 1.0, "向下拖缩小：{zoom}");
    }
}

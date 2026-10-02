//! Nana-owned WGPU painter for [`UiScene`].
//!
//! This is the product Scene backend. It paints [`UiScene`] through the host GPU.
//! The host owns the [`GpuContext`] and each frame's [`FrameContext`]. HostTexture is sampled in document order
//! inside the current dest (or opacity-group) pass; groups do not open a pass
//! per HostTexture slot.

mod backdrop;
mod buffer_upload;
mod clip;
mod color;
mod dest;
mod host_texture;
mod icon;
mod image_resample;
pub(crate) mod image_url;
mod mesh;
mod motion;
mod quad;
mod text;

pub use self::text::SubpixelOrder;
pub use self::text::TextGlyphCounters;
pub(crate) mod url_texture_cache;
mod validate;

pub(crate) use clip::{covered_span, is_translation_projective, on_grid};

use std::{sync::Arc, time::Instant};

use nana_gpu::{
    __framework, FrameContext, GpuContext, GpuRenderTarget, GpuSubmission, GpuTextureFormat,
    RetainedWrites,
};
use nana_ui_core::GpuWorkObservation;
use nana_ui_platform::SharedFetchHost;
use nana_ui_scene::{RenderOperation, ScenePrimitiveKind, UiScene};
use url_texture_cache::{UrlTextureCache, egress_of};

use crate::{
    HostTextureRegistry, PhysicalRect,
    gpu_work::{GpuStageTimings, GpuWorkSink},
    scene_gpu::{
        SceneGpuBatchNode, SceneGpuBatchPassContext, SceneGpuNode, SceneGpuPassContext,
        SceneGpuPrepareContext, SceneGpuRenderContext, SceneGpuRenderer, SceneGpuRendererRegistry,
        ScenePass,
    },
};

pub use image_url::{
    resolve_background_image_url, resolved_resource_is_allowed, set_background_image_url_base,
};
use validate::validate_scene;
pub use validate::{HostTextureSceneResolver, ScenePaintError};

use backdrop::BackdropPipeline;
use clip::{
    FragmentClip, LogicalRect, PaintOrigin, extra_fragment_clips, fragment_clip, intersect_clips,
    intersect_physical, local_rect, mesh_extra_fragment_clips, overlaps_physical, paint_origin,
    paint_transform, physical_bounds, physical_scissor, transformed_aabb,
    transformed_aabb_projective, union_physical,
};
pub(crate) use color::pack_linear;
use color::with_opacity;
use dest::{DestPassCounts, DestTarget, GroupSlot};
use host_texture::{HostTexturePipeline, PreparedHostTexture};
use icon::{IconPipeline, PreparedIcon};
use mesh::{MeshPipeline, MeshRange, PathRange, StrokeStyle};
use motion::MotionGpuResources;
use quad::QuadPipeline;
use text::{PreparedText, TextPipeline};

#[derive(Clone, Copy, PartialEq)]
pub struct ScenePaintViewport {
    /// Scene dest rect size in logical pixels.
    pub logical_size: [f32; 2],
    /// Full target texture size in physical pixels.
    pub physical_size: [u32; 2],
    pub scale_factor: f32,
    pub scene_origin: [f32; 2],
    /// Logical position of scene (0, 0) on the target.
    pub target_origin: [f32; 2],
    /// Linear RGBA, like every colour the painter blends. A theme colour is
    /// sRGB and has to be converted first.
    pub clear_color: [f32; 4],
    /// Clear the whole target; otherwise keep existing pixels (`LoadOp::Load`).
    pub clear: bool,
}

pub struct SceneWgpuPainter {
    targets: std::collections::HashMap<RenderTargetId, TargetState>,
    prepared_batch: Option<PreparedBatch>,
    gpu: GpuContext,
    /// Targets whose retained GPU state was recorded into a frame that was
    /// later dropped instead of submitted; rebuilt before their next paint.
    /// Keyed by [`RenderTargetId`].
    retained_targets: RetainedWrites,
    /// The same for the state [`Self::paint`] keeps in the painter itself, in
    /// a ledger of its own so no host id can alias it.
    retained_default: RetainedWrites,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    quads: QuadPipeline,
    motion: MotionGpuResources,
    meshes: MeshPipeline,
    icons: IconPipeline,
    text: TextPipeline,
    host_textures: HostTexturePipeline,
    /// `url(...)` images for quads and HostTexture masks alike: a URL both
    /// use is fetched, decoded, uploaded and retained once per painter.
    /// Shared by every target; target-local bindings follow `image_revision`.
    url_cache: UrlTextureCache,
    backdrop: BackdropPipeline,
    dest: Option<DestTarget>,
    /// Shared across resize-driven `DestTarget` recreations so the blit
    /// pipeline is not recompiled on every interactive resize event. `None`
    /// when the host device lacks `PIPELINE_CACHE`.
    dest_pipeline_cache: Option<wgpu::PipelineCache>,
    last_gpu_work: Option<GpuWorkObservation>,
    last_gpu_timings: Option<GpuStageTimings>,
    last_dest_pass_counts: Option<DestPassCounts>,
    /// Last fully scene-described dest; host textures / GPU slots skip reuse.
    painted: Option<PaintedDest>,
    /// Which host-texture slots the last fresh prepare of the active target
    /// drew, and how large; swapped per target like `painted`.
    painted_demand: crate::painted_demand::PaintedChannel,
    image_revision: u64,
    /// Egress for `url(...)` loads of the scenes painted next.
    fetch_host: Option<SharedFetchHost>,
    /// Egress the active target's URL bindings and cached dest were built
    /// with; swapped per target like `painted`.
    bound_fetch_host: Option<SharedFetchHost>,
    alpha_encoding: AlphaEncoding,
}

/// How a paint stores its premultiplied pixels. Blending is linear either
/// way; a window compositor, though, reads a surface as `enc(color) * alpha`
/// and shows the linear `enc(color * alpha)` too bright wherever alpha is low.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlphaEncoding {
    /// `enc(color * alpha)`: for offscreen readers and opaque surfaces.
    #[default]
    Linear,
    /// `enc(color) * alpha`: for a surface a compositor blends. Opaque pixels
    /// match [`Self::Linear`]; a target that is not sRGB keeps that too.
    Gamma,
}

#[derive(Clone, Copy, PartialEq)]
struct PaintedDest {
    instance: u64,
    presentation_epoch: u64,
    viewport: ScenePaintViewport,
    size: [u32; 2],
}

/// Host-owned identity for a window or offscreen presentation target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderTargetId(pub u64);

/// The one key of the ledger behind [`SceneWgpuPainter::paint`].
const DEFAULT_TARGET: u64 = 0;

#[derive(Default)]
struct TargetState {
    prepared_batch: Option<PreparedBatch>,
    quads: Option<quad::QuadPipelineTarget>,
    meshes: Option<mesh::MeshPipelineTarget>,
    icons: Option<icon::IconPipelineTarget>,
    text: Option<text::TextPipelineTarget>,
    backdrop: Option<backdrop::BackdropPipelineTarget>,
    host_textures: crate::gpu_texture::GpuTextureTarget,
    dest: Option<DestTarget>,
    painted: Option<PaintedDest>,
    painted_demand: crate::painted_demand::PaintedChannel,
    image_revision: u64,
    bound_fetch_host: Option<SharedFetchHost>,
}

struct PreparedBatch {
    key: PaintedDest,
    resources: Vec<TextureBindingKey>,
    renderers: Vec<(usize, u64)>,
    image_revision: u64,
    commands: Vec<DrawCommand>,
    max_group_depth: usize,
    group_slots: Vec<dest::GroupSlot>,
    group_extents: Vec<GroupExtent>,
    /// Read off the document order that built `commands`; it cannot be
    /// recovered from the merged list.
    glyph_then_quad: bool,
    /// Text instances name atlas rectangles. Reusing them after the atlas has
    /// evicted or relocated a glyph would sample whatever now owns that
    /// rectangle, so a changed epoch forces the batch to be rebuilt.
    text_placement_epoch: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct TextureBindingKey {
    identity: u64,
    generation: u64,
    width: u32,
    height: u32,
    alpha: crate::HostTextureAlphaMode,
}

enum DrawCommand {
    Quads {
        range: std::ops::Range<u32>,
        scissor: PhysicalRect,
    },
    Mesh {
        range: MeshRange,
        scissor: PhysicalRect,
    },
    Path {
        range: PathRange,
        scissor: PhysicalRect,
    },
    /// A painter layer's mask, applied to the open group before it pops.
    PathMask {
        range: PathRange,
        scissor: PhysicalRect,
        mode: nana_ui_scene::LayerMaskMode,
    },
    Text {
        prepared: PreparedText,
        scissor: PhysicalRect,
    },
    Icon {
        first: usize,
        count: usize,
        scissor: PhysicalRect,
    },
    HostTexture(PreparedHostTexture),
    Custom {
        node: SceneGpuNode,
        renderer: Arc<dyn SceneGpuRenderer>,
        bounds: PhysicalRect,
        clip: PhysicalRect,
    },
    Backdrop {
        index: u32,
    },
    PushGroup {
        layer: usize,
        slot: u32,
    },
    PopGroup,
}

impl SceneWgpuPainter {
    /// A painter for targets of `format` on `gpu`. Every frame it paints must
    /// come from the same device.
    pub fn new(gpu: &GpuContext, format: GpuTextureFormat) -> Self {
        let device = __framework::device(gpu);
        let queue = __framework::queue(gpu);
        let format = __framework::format_to_wgpu(format);
        let quads = QuadPipeline::new_with_policy(device, format, Some(gpu.policy()));
        let motion_gpu_supported = __framework::adapter(gpu)
            .get_texture_format_features(wgpu::TextureFormat::Rgba32Float)
            .allowed_usages
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT);
        let motion = MotionGpuResources::new_with_policy(
            device,
            quads.motion_layout(),
            Some(gpu.policy()),
            motion_gpu_supported,
        );
        Self {
            targets: std::collections::HashMap::new(),
            prepared_batch: None,
            gpu: gpu.clone(),
            retained_targets: RetainedWrites::new(),
            retained_default: RetainedWrites::new(),
            device: device.clone(),
            queue: queue.clone(),
            format,
            quads,
            motion,
            meshes: MeshPipeline::new_with_policy(device, format, Some(gpu.policy())),
            icons: IconPipeline::new_with_policy(device, format, Some(gpu.policy())),
            text: TextPipeline::new_with_policy(device, queue, format, gpu.policy()),
            host_textures: HostTexturePipeline::new(device, queue, format, gpu.policy(), gpu),
            url_cache: UrlTextureCache::default(),
            backdrop: BackdropPipeline::new(device, format, gpu.policy()),
            dest: None,
            // Pipeline-cache reuse requires a host-enabled device feature;
            // the painter must not demand it, so degrade to per-recreate
            // compilation when the host did not opt in.
            // SAFETY: `data: None` loads no untrusted cache blob; the cache
            // only lets the driver reuse compilation state in-process.
            dest_pipeline_cache: device
                .features()
                .contains(wgpu::Features::PIPELINE_CACHE)
                .then(|| unsafe {
                    device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                        label: Some("nana-ui.scene.dest.cache"),
                        data: None,
                        fallback: true,
                    })
                }),
            last_gpu_work: None,
            last_gpu_timings: None,
            last_dest_pass_counts: None,
            painted: None,
            painted_demand: Default::default(),
            image_revision: 0,
            fetch_host: None,
            bound_fetch_host: None,
            alpha_encoding: AlphaEncoding::Linear,
        }
    }

    pub fn format(&self) -> GpuTextureFormat {
        __framework::format_from_wgpu(self.format)
    }

    /// The device this painter records on.
    pub fn gpu(&self) -> &GpuContext {
        &self.gpu
    }

    /// Wake the owning event loop when an asynchronous URL image completes.
    pub fn set_image_waker(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        let keyed: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |_| wake());
        self.set_image_update_waker(keyed);
    }

    /// Wake the owning host with the URL/resource key that completed.
    /// Hosts can use this to redraw only targets that reference the resource.
    pub fn set_image_update_waker(&mut self, wake: Arc<dyn Fn(&str) + Send + Sync>) {
        self.url_cache.set_wake(wake);
    }

    /// Policy-gated egress for the `http(s)` `url(...)` images of the scenes
    /// painted next: the host behind the document's JS `fetch()`. `None`
    /// refuses remote images; `data:`, `file:` and jailed paths are unaffected.
    ///
    /// A shared painter is given each document's host before painting it, and
    /// caches results per host, so no document sees an image another
    /// document's policy admitted. Identity is the `Arc`: pass clones of one
    /// host, or images are refetched on every change.
    pub fn set_resource_fetch_host(&mut self, host: Option<SharedFetchHost>) {
        self.url_cache.set_fetch_host(host.clone());
        self.fetch_host = host;
    }

    pub fn has_image_updates(&self) -> bool {
        self.url_cache.has_updates()
    }

    /// Offscreen hosts can await completion and paint again without polling pixels.
    pub fn has_pending_images(&self) -> bool {
        self.url_cache.has_pending()
    }

    /// Completion generation, used by layered snapshot hosts to repaint earlier layers.
    pub fn image_revision(&self) -> u64 {
        self.image_revision
    }

    /// GPU counters from the last `paint` that encoded a real command buffer.
    /// `None` if this painter has not encoded, or the last `paint` was skipped.
    pub fn last_gpu_work(&self) -> Option<GpuWorkObservation> {
        self.last_gpu_work
    }

    /// Descriptor upload counters from the last encoded `paint`. Steady
    /// timestamp frames keep `motion_descriptors_uploaded` at zero.
    pub fn last_motion_work(&self) -> nana_ui_core::MotionWorkCounters {
        self.motion.last_work()
    }

    /// Batch / upload / encode timings from the last encoded `paint`. Submit is
    /// filled only after the host calls [`Self::record_submit`].
    pub fn last_gpu_timings(&self) -> Option<GpuStageTimings> {
        self.last_gpu_timings
    }

    /// Draw upright text that lands straight on the target with per-subpixel
    /// (ClearType / LCD) coverage for a panel of this order, or go back to
    /// grayscale with `None`.
    ///
    /// Only for a target whose pixels reach the screen opaque: the blend
    /// writes a coverage per channel, which a compositor would read as
    /// colored alpha. Text inside an offscreen group stays grayscale either
    /// way. A device without dual-source blending ignores the request.
    ///
    /// The hosted runtime decides this per window from the surface's alpha
    /// mode and [`SubpixelOrder::system`]; a host that owns its surface makes
    /// the same call.
    pub fn set_subpixel_text(&mut self, order: Option<SubpixelOrder>) {
        if self.text.set_subpixel(&self.device, order) {
            // The retained batch holds instances resolved under the old mode.
            self.prepared_batch = None;
        }
    }

    /// How the next paints store alpha. Painters are shared per format, so a
    /// host sets this per window; the hosted runtime picks
    /// [`AlphaEncoding::Gamma`] for every surface that is not opaque.
    pub fn set_alpha_encoding(&mut self, encoding: AlphaEncoding) {
        self.alpha_encoding = encoding;
    }

    /// Text shape-cache counters from the last `paint`: (hits, misses,
    /// evictions). Tests use this to pin the reshaping-skip contract.
    pub fn text_shape_cache_stats(&self) -> (usize, usize, usize) {
        self.text.shape_cache_stats()
    }

    /// Native glyph renderer counters: resolve, raster cache, atlas, upload
    /// and draw. Tests pin the cache and atlas contracts through these.
    ///
    /// Summed over every render target this painter serves, so a second
    /// window painted through [`Self::paint_target`] counts too.
    pub fn text_glyph_counters(&self) -> TextGlyphCounters {
        self.text.glyph_counters_across(
            self.targets
                .values()
                .filter_map(|state| state.text.as_ref()),
        )
    }

    /// Record the submit of the frame this painter last encoded into.
    pub fn record_submit(&mut self, submission: &GpuSubmission) {
        if let Some(timings) = &mut self.last_gpu_timings {
            timings.submit = timings.submit.saturating_add(submission.cpu_duration());
        }
    }

    /// Release target-owned textures when a host closes a window or viewport.
    ///
    /// A fetch host no remaining target uses goes with it, cancelling its
    /// image requests now: URL caches only expire entries on frames that
    /// rebuild a batch, which static windows never do.
    pub fn remove_target(&mut self, id: RenderTargetId) {
        let Some(mut state) = self.targets.remove(&id) else {
            return;
        };
        self.url_cache.retire_demand(state.painted_demand.id());
        if let Some(text) = state.text.take() {
            self.text.close_target(text);
        }
        let Some(host) = state.bound_fetch_host else {
            return;
        };
        let closed = egress_of(Some(&host));
        if std::iter::once(&self.bound_fetch_host)
            .chain(self.targets.values().map(|state| &state.bound_fetch_host))
            .any(|bound| egress_of(bound.as_ref()) == closed)
        {
            return;
        }
        if egress_of(self.fetch_host.as_ref()) == closed {
            self.set_resource_fetch_host(None);
        }
        self.url_cache.release_fetch_host(&host);
    }

    /// Paint with isolated target state while sharing device pipelines/caches.
    ///
    /// The same `frame` contract as [`Self::paint`].
    #[allow(clippy::too_many_arguments)]
    pub fn paint_target(
        &mut self,
        id: RenderTargetId,
        scene: &UiScene,
        frame: &mut FrameContext,
        target: &GpuRenderTarget,
        viewport: ScenePaintViewport,
        host_textures: Option<&HostTextureRegistry>,
        gpu_renderers: Option<&SceneGpuRendererRegistry>,
    ) -> Result<(), ScenePaintError> {
        self.begin_frame(Some(id), frame, target)?;
        let mut state = self.targets.remove(&id).unwrap_or_default();
        if state.image_revision != self.image_revision {
            state.painted = None;
            QuadPipeline::invalidate_target_image_bindings(&mut state.quads);
            HostTexturePipeline::invalidate_target_image_bindings(&mut state.host_textures);
        }
        self.swap_target_state(&mut state);
        let link = crate::gpu_work::FrameLink::of(frame);
        let result = self.paint_recorded(
            scene,
            __framework::encoder(frame),
            __framework::target_view(target),
            viewport,
            host_textures,
            gpu_renderers,
            Some(link),
        );
        self.swap_target_state(&mut state);
        state.image_revision = self.image_revision;
        self.targets.insert(id, state);
        if result.is_ok() {
            frame.record_retained_writes(&self.retained_targets, id.0);
        }
        result
    }
    fn swap_target_state(&mut self, state: &mut TargetState) {
        std::mem::swap(&mut self.dest, &mut state.dest);
        std::mem::swap(&mut self.painted, &mut state.painted);
        std::mem::swap(&mut self.painted_demand, &mut state.painted_demand);
        std::mem::swap(&mut self.prepared_batch, &mut state.prepared_batch);
        std::mem::swap(&mut self.bound_fetch_host, &mut state.bound_fetch_host);
        self.quads.swap_target(&mut state.quads, &self.device);
        self.meshes.swap_target(&mut state.meshes, &self.device);
        self.icons.swap_target(&mut state.icons, &self.device);
        self.text.swap_target(&mut state.text, &self.device);
        self.backdrop.swap_target(&mut state.backdrop, &self.device);
        self.host_textures.swap_target(&mut state.host_textures);
    }

    /// Refuse a frame or target from another device, refuse to record on top
    /// of writes an unsubmitted frame still holds, and rebuild targets whose
    /// retained writes were rolled back by a dropped frame. `None` is the
    /// state behind [`Self::paint`].
    fn begin_frame(
        &mut self,
        target_id: Option<RenderTargetId>,
        frame: &FrameContext,
        target: &GpuRenderTarget,
    ) -> Result<(), ScenePaintError> {
        // A rejected frame must not report a previous target's successful work.
        self.last_gpu_work = None;
        self.last_gpu_timings = None;
        self.last_dest_pass_counts = None;
        let expected = self.gpu.generation();
        for found in [frame.generation(), target.generation()] {
            if found != expected {
                return Err(ScenePaintError::DeviceMismatch { expected, found });
            }
        }
        // Before the drain: a frame dropped on another thread in between
        // releases the key and records its rollback in one step, so checking
        // in this order never paints over a rollback the drain missed.
        let in_flight = match target_id {
            Some(id) => self.retained_targets.in_flight(id.0, frame.id()),
            None => self.retained_default.in_flight(DEFAULT_TARGET, frame.id()),
        };
        if in_flight {
            return Err(ScenePaintError::TargetInFlight(target_id));
        }
        if self.retained_default.has_rolled_back() {
            self.retained_default.drain_rolled_back(|_| {});
            self.reset_default_target();
        }
        if self.retained_targets.has_rolled_back() {
            let mut rolled_back = Vec::new();
            self.retained_targets
                .drain_rolled_back(|key| rolled_back.push(RenderTargetId(key)));
            for id in rolled_back {
                // Not `remove_target`: the window is still open, so its fetch
                // host and image caches stay.
                if let Some(mut state) = self.targets.remove(&id)
                    && let Some(text) = state.text.take()
                {
                    self.text.close_target(text);
                }
            }
        }
        Ok(())
    }

    /// Start the state [`Self::paint`] keeps in the painter itself over, as a
    /// rolled-back target's is.
    fn reset_default_target(&mut self) {
        let mut state = TargetState::default();
        self.swap_target_state(&mut state);
        if let Some(text) = state.text.take() {
            self.text.close_target(text);
        }
    }

    /// Encode `scene` into `frame`, drawing into `target`.
    ///
    /// The painter keeps geometry on the GPU between frames and records part
    /// of each frame's writes to it as copies in the frame's encoder — text
    /// blocks and the text draw order (#224) — then treats them as done. A
    /// frame dropped instead of submitted rolls that back: the next paint of
    /// the target rebuilds it. Painting a target again while an earlier frame
    /// that painted it is still unsubmitted is refused with
    /// [`ScenePaintError::TargetInFlight`], since the two could be submitted
    /// out of order. On `Err` nothing was recorded.
    pub fn paint(
        &mut self,
        scene: &UiScene,
        frame: &mut FrameContext,
        target: &GpuRenderTarget,
        viewport: ScenePaintViewport,
        host_textures: Option<&HostTextureRegistry>,
        gpu_renderers: Option<&SceneGpuRendererRegistry>,
    ) -> Result<(), ScenePaintError> {
        self.begin_frame(None, frame, target)?;
        let link = crate::gpu_work::FrameLink::of(frame);
        let result = self.paint_recorded(
            scene,
            __framework::encoder(frame),
            __framework::target_view(target),
            viewport,
            host_textures,
            gpu_renderers,
            Some(link),
        );
        if result.is_ok() {
            frame.record_retained_writes(&self.retained_default, DEFAULT_TARGET);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_recorded(
        &mut self,
        scene: &UiScene,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        viewport: ScenePaintViewport,
        host_textures: Option<&HostTextureRegistry>,
        gpu_renderers: Option<&SceneGpuRendererRegistry>,
        frame: Option<crate::gpu_work::FrameLink>,
    ) -> Result<(), ScenePaintError> {
        // A rejected frame must not report a previous target's successful work.
        self.last_gpu_work = None;
        self.last_gpu_timings = None;
        self.last_dest_pass_counts = None;
        if self.url_cache.poll() {
            self.painted = None;
            self.image_revision = self.image_revision.wrapping_add(1);
            // The active target is held in the painter fields while `paint`
            // runs; invalidate its URL-backed bindings immediately. Other
            // targets are invalidated when their revision is observed above.
            self.quads.invalidate_image_bindings();
            self.host_textures.invalidate_image_bindings();
        }
        // URL bindings and a cached dest are keyed by URL alone, so ones built
        // through another document's egress must not be reused under this one.
        if egress_of(self.fetch_host.as_ref()) != egress_of(self.bound_fetch_host.as_ref()) {
            self.bound_fetch_host = self.fetch_host.clone();
            self.painted = None;
            self.prepared_batch = None;
            self.quads.invalidate_image_bindings();
            self.host_textures.invalidate_image_bindings();
        }
        let instance = scene.instance_id();
        // Also resolves this frame's `PreparedBatch` key, so nothing below
        // walks `FramePlan::custom_nodes` again.
        let resolved = validate_scene(scene, host_textures, gpu_renderers, self.gpu.generation())?;
        if viewport.physical_size[0] == 0 || viewport.physical_size[1] == 0 {
            self.last_gpu_work = None;
            self.last_gpu_timings = None;
            self.last_dest_pass_counts = None;
            self.painted = None;
            // Nothing of this target is visible: withdraw its demand.
            self.painted_demand.begin();
            self.painted_demand.commit(host_textures);
            self.url_cache.retire_demand(self.painted_demand.id());
            return Ok(());
        }

        let scale = if viewport.scale_factor.is_finite() && viewport.scale_factor > 0.0 {
            viewport.scale_factor
        } else {
            1.0
        };
        let dest_physical = [
            (viewport.logical_size[0] * scale).round().max(1.0) as u32,
            (viewport.logical_size[1] * scale).round().max(1.0) as u32,
        ];
        let blit_origin = [
            viewport.target_origin[0] * scale,
            viewport.target_origin[1] * scale,
        ];
        let origin = PaintOrigin::new(paint_origin([0.0, 0.0], viewport.scene_origin), scale);
        let viewport_clip = LogicalRect::viewport([0.0, 0.0], viewport.logical_size);
        let gpu_work = GpuWorkSink::with_frame(&self.gpu, frame);
        let clear = wgpu::Color {
            r: viewport.clear_color[0] as f64,
            g: viewport.clear_color[1] as f64,
            b: viewport.clear_color[2] as f64,
            a: viewport.clear_color[3] as f64,
        };
        let painted = PaintedDest {
            instance,
            presentation_epoch: scene.presentation_epoch(),
            viewport,
            size: dest_physical,
        };

        let gpu_motion_live = scene
            .motion_gpu_descriptors()
            .iter()
            .any(|descriptor| descriptor.is_live());
        if self.painted == Some(painted)
            && !gpu_motion_live
            && let Some(dest) = self.dest.as_ref()
            && dest.width == dest_physical[0]
            && dest.height == dest_physical[1]
        {
            let encode_started = Instant::now();
            let mut dest_passes = DestPassCounts {
                msaa_allocated: dest.msaa_allocated,
                ..DestPassCounts::default()
            };
            dest.blit(
                encoder,
                target,
                blit_origin[0],
                blit_origin[1],
                viewport.physical_size,
                viewport.clear.then_some(clear),
                self.alpha_encoding,
                Some(&gpu_work),
                &mut dest_passes,
            );
            self.last_gpu_work = Some(gpu_work.snapshot());
            self.last_gpu_timings = Some(GpuStageTimings {
                batch: std::time::Duration::ZERO,
                gpu_upload: std::time::Duration::ZERO,
                encode: encode_started.elapsed(),
                submit: std::time::Duration::ZERO,
            });
            self.last_dest_pass_counts = Some(dest_passes);
            return Ok(());
        }

        let cached = self.prepared_batch.take().filter(|batch| {
            resolved.cacheable
                && batch.key == painted
                && batch.resources == resolved.resources
                && batch.renderers == resolved.renderers
                && batch.image_revision == self.image_revision
                && batch.text_placement_epoch == self.text.placement_epoch()
                // The order is about to give back the gaps text that stopped
                // growing left (#230); only a flush does that.
                && !self.text.order_settle_due()
        });
        let reused = cached.is_some();
        if reused {
            self.text.note_reused_frame();
        }
        let (
            commands,
            max_group_depth,
            group_slots_uniforms,
            group_extents,
            glyph_then_quad,
            batch,
            gpu_upload,
        ) = if let Some(cached) = cached {
            (
                cached.commands,
                cached.max_group_depth,
                cached.group_slots,
                cached.group_extents,
                cached.glyph_then_quad,
                std::time::Duration::ZERO,
                std::time::Duration::ZERO,
            )
        } else {
            let batch_started = Instant::now();
            self.url_cache.begin_frame();
            self.quads.begin_frame(scale);
            self.painted_demand.begin();
            self.meshes.begin_frame();
            self.icons.begin_frame(dest_physical);
            self.text.begin_frame(dest_physical);
            self.backdrop.begin_frame();

            let mut commands = Vec::new();
            let mut batching = Batching::default();
            let mut group_stack: Vec<nana_ui_scene::OpacityGroup> = Vec::new();
            let mut group_depth = 0usize;
            let mut group_slots = 0u32;
            let mut max_group_depth = 0usize;
            let mut group_slots_uniforms = Vec::new();
            // Painter layers open (Issue #217), by the node that opened
            // each. They nest inside the node's own groups.
            let mut painter_layers: Vec<nana_ui_runtime::StableNodeId> = Vec::new();
            // A clipped painter layer wholly outside the target, skipped
            // with everything in it: its node and how deep in it we are.
            // Culling cannot drop the pair itself, as its contents are
            // bounded by the layer's clip, not by their own bounds.
            let mut skipped_layer: Option<(nana_ui_runtime::StableNodeId, usize)> = None;
            let whole_dest = PhysicalRect {
                x: 0,
                y: 0,
                width: dest_physical[0],
                height: dest_physical[1],
            };
            // Scissors/text clips round to physical pixels. Include their
            // possible edge coverage even at fractional origins/low DPI.
            let edge_padding = 1.0 / scale;
            let operations = scene
                .visible_operations(nana_ui_scene::SceneRect {
                    x: viewport.scene_origin[0] - edge_padding,
                    y: viewport.scene_origin[1] - edge_padding,
                    width: viewport.logical_size[0] + edge_padding * 2.0,
                    height: viewport.logical_size[1] + edge_padding * 2.0,
                })
                .map_err(|_| ScenePaintError::InvalidRenderGraph)?;
            for operation in operations.iter() {
                let id = match operation {
                    RenderOperation::PrepareExternal(_) => continue,
                    RenderOperation::Draw(id) | RenderOperation::InvokeCustom(id) => *id,
                };
                let Some(primitive) = scene.draw_primitive(id) else {
                    continue;
                };
                if let Some((node, depth)) = &mut skipped_layer {
                    if *node == primitive.node {
                        match primitive.kind {
                            ScenePrimitiveKind::LayerBegin { .. } => *depth += 1,
                            ScenePrimitiveKind::LayerEnd { .. } => {
                                *depth -= 1;
                                if *depth == 0 {
                                    skipped_layer = None;
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    skipped_layer = None;
                }
                let encode = scene.compositor_paint_encode(
                    primitive.node,
                    &primitive.kind,
                    primitive.transform,
                    primitive.paint_opacity,
                );
                let opacity = encode.opacity;
                let encode_transform = encode.transform;
                self.quads
                    .set_motion(encode.motion_ids, encode.transform_origin);
                // A layer never outlives the node that opened it.
                if painter_layers
                    .last()
                    .is_some_and(|node| *node != primitive.node)
                {
                    batching.close_all();
                    while painter_layers.pop().is_some() {
                        group_depth = group_depth.saturating_sub(1);
                        commands.push(DrawCommand::PopGroup);
                    }
                }
                if sync_opacity_groups(
                    &mut commands,
                    &mut group_stack,
                    &mut group_depth,
                    &mut group_slots,
                    &mut max_group_depth,
                    &mut group_slots_uniforms,
                    &scene.opacity_groups(primitive.node),
                    scene,
                    origin,
                    scale,
                ) {
                    batching.close_all();
                }
                // Both ends of a painter layer reach here whatever they
                // cover, so no culling below can leave one open.
                match &primitive.kind {
                    ScenePrimitiveKind::LayerBegin {
                        opacity: layer_opacity,
                        blend,
                        clip: layer_clip,
                    } => {
                        if let Some(rect) = layer_clip
                            && !encode_transform.is_projective()
                            && !rect_meets_target(
                                *rect,
                                clip::paint_affine(encode_transform.0, origin),
                                scale,
                                dest_physical,
                            )
                        {
                            skipped_layer = Some((primitive.node, 1));
                            continue;
                        }
                        batching.close_all();
                        let frag = layer_clip
                            .map(|rect| {
                                clip::local_rect_clip(rect, encode_transform, origin)
                                    .for_physical_pixels(scale)
                            })
                            .unwrap_or(clip::FragmentClip::PASS);
                        let layer = group_depth;
                        let slot = group_slots;
                        group_slots = group_slots.saturating_add(1);
                        group_depth = group_depth.saturating_add(1);
                        max_group_depth = max_group_depth.max(group_depth);
                        group_slots_uniforms.push(GroupSlot::dest(
                            *layer_opacity,
                            [1.0, 1.0, 1.0],
                            0.0,
                            0.0,
                            blend.gpu_index(),
                            frag,
                        ));
                        commands.push(DrawCommand::PushGroup { layer, slot });
                        painter_layers.push(primitive.node);
                        continue;
                    }
                    ScenePrimitiveKind::LayerEnd { mask } => {
                        batching.close_all();
                        if painter_layers.pop().is_none() {
                            continue;
                        }
                        if let Some(mask) = mask {
                            let (affine, persp) =
                                paint_transform(encode_transform.0, encode_transform.1, origin);
                            if let Some(range) = self.meshes.push_path(
                                &mask.mesh,
                                mask.origin,
                                mesh_affine(affine, persp),
                                1.0,
                                clip::FragmentClip::PASS,
                            ) {
                                commands.push(DrawCommand::PathMask {
                                    range,
                                    scissor: whole_dest,
                                    mode: mask.mode,
                                });
                            }
                        }
                        group_depth = group_depth.saturating_sub(1);
                        commands.push(DrawCommand::PopGroup);
                        continue;
                    }
                    _ => {}
                }
                let Some(clip) = intersect_clips(viewport_clip, &primitive.clips, origin) else {
                    continue;
                };
                let Some(scissor) = physical_scissor(clip, scale, dest_physical) else {
                    continue;
                };
                let clips = match &primitive.kind {
                    ScenePrimitiveKind::Quad { surface, .. } => match &surface.polygon_clip {
                        Some(own) => clip::without_own_polygon(
                            &primitive.clips,
                            primitive.bounds,
                            primitive.transform,
                            own,
                        ),
                        None => &primitive.clips[..],
                    },
                    _ => &primitive.clips[..],
                };
                let frag_clip = fragment_clip(clips, origin);
                // Quads, icons and host textures test no polygon or ellipse:
                // an innermost one dest-wraps them with its rectangle
                // (`clip_dests_for`), which must not ramp in their own shader
                // as well, or an edge the two share would fade twice.
                let vertex_clip = if frag_clip.polygon_count > 0 {
                    FragmentClip::PASS
                } else {
                    frag_clip
                };
                let (affine, persp) =
                    paint_transform(encode_transform.0, encode_transform.1, origin);
                let bounds = local_rect(primitive.bounds);
                // Wrapping drains and re-inserts this primitive's commands
                // behind a `PushGroup`, so no batch may span the edit.
                let clip_dests = clip_dests_for(&primitive.kind, clips, origin);
                if !clip_dests.is_empty() {
                    batching.close_all();
                }
                let command_start = commands.len();
                match &primitive.kind {
                    ScenePrimitiveKind::Quad {
                        background,
                        border_color,
                        border_width,
                        corner_radius,
                        shadow,
                        surface,
                    } => {
                        let painted = quad_painted_bounds(
                            bounds,
                            shadow.as_ref(),
                            surface,
                            affine,
                            persp,
                            scale,
                            scissor,
                            dest_physical,
                        );
                        if let Some(index) = self.quads.push_with_work(
                            &self.device,
                            &self.queue,
                            &mut self.url_cache,
                            bounds,
                            clip,
                            vertex_clip,
                            affine,
                            persp,
                            *background,
                            *border_color,
                            *border_width,
                            *corner_radius,
                            *shadow,
                            opacity,
                            surface,
                            Some(&gpu_work),
                        ) {
                            // A fully transparent surface shows nothing,
                            // backdrop blur included.
                            if let Some(filter) = surface
                                .backdrop_filter
                                .filter(|f| f.is_active() && opacity > 0.0)
                            {
                                let world_bounds =
                                    transformed_aabb_projective(bounds, affine, persp);
                                let phys = physical_bounds(world_bounds, scale, scissor);
                                let radii = corner_radius.map(|r| r * scale);
                                let bidx = self.backdrop.push(
                                    index,
                                    [
                                        phys.x as f32,
                                        phys.y as f32,
                                        phys.width as f32,
                                        phys.height as f32,
                                    ],
                                    radii,
                                    filter,
                                    frag_clip.for_physical_pixels(scale),
                                    scale,
                                    dest_physical,
                                    bounds,
                                    affine,
                                );
                                batching.close_all();
                                commands.push(DrawCommand::Backdrop { index: bidx });
                            }
                            for quad_index in index..self.quads.pending_len() {
                                push_quad(
                                    &mut commands,
                                    &mut batching,
                                    quad_index,
                                    scissor,
                                    painted,
                                );
                            }
                        }
                    }
                    ScenePrimitiveKind::QuadBatch {
                        bounds: batch,
                        background,
                        border_color,
                        border_width,
                        corner_radius,
                        shadow,
                        surface,
                    } => {
                        for item in batch {
                            let item_bounds = local_rect(*item);
                            let painted = quad_painted_bounds(
                                item_bounds,
                                shadow.as_ref(),
                                surface,
                                affine,
                                persp,
                                scale,
                                scissor,
                                dest_physical,
                            );
                            if let Some(index) = self.quads.push_with_work(
                                &self.device,
                                &self.queue,
                                &mut self.url_cache,
                                item_bounds,
                                clip,
                                vertex_clip,
                                affine,
                                persp,
                                *background,
                                *border_color,
                                *border_width,
                                *corner_radius,
                                *shadow,
                                opacity,
                                surface,
                                Some(&gpu_work),
                            ) {
                                if let Some(filter) = surface
                                    .backdrop_filter
                                    .filter(|f| f.is_active() && opacity > 0.0)
                                {
                                    let world_bounds =
                                        transformed_aabb_projective(item_bounds, affine, persp);
                                    let phys = physical_bounds(world_bounds, scale, scissor);
                                    let radii = corner_radius.map(|r| r * scale);
                                    let bidx = self.backdrop.push(
                                        index,
                                        [
                                            phys.x as f32,
                                            phys.y as f32,
                                            phys.width as f32,
                                            phys.height as f32,
                                        ],
                                        radii,
                                        filter,
                                        frag_clip.for_physical_pixels(scale),
                                        scale,
                                        dest_physical,
                                        item_bounds,
                                        affine,
                                    );
                                    batching.close_all();
                                    commands.push(DrawCommand::Backdrop { index: bidx });
                                }
                                for quad_index in index..self.quads.pending_len() {
                                    push_quad(
                                        &mut commands,
                                        &mut batching,
                                        quad_index,
                                        scissor,
                                        painted,
                                    );
                                }
                            }
                        }
                    }
                    ScenePrimitiveKind::Text {
                        content,
                        color,
                        size,
                        weight,
                        family,
                        line_height,
                        wrap,
                        ellipsis,
                        max_lines,
                        shaping,
                        horizontal_alignment,
                        vertical_alignment,
                        spans,
                        letter_spacing,
                        text_shadow,
                        underline: _,
                        line_through: _,
                        font_features,
                        italic,
                        wrap_break,
                        opentype,
                        // The Runtime's retained handle: the layout this
                        // node was *measured* with. The painter draws it
                        // rather than laying the same paragraph out again,
                        // so there is one paragraph, not two that agree.
                        layout,
                    } => {
                        // A text node's glyphs are retained per node and
                        // per pass, so the shadows under a label are their
                        // own entries over the same shaped paragraph and
                        // the same glyph bitmaps.
                        let node = primitive.node.get();
                        let slot = id.slot;
                        let mut pass = 0u32;
                        let opaque_backdrop = group_depth == 0;
                        let mut push_text =
                            |commands: &mut Vec<DrawCommand>,
                             batching: &mut Batching,
                             extra_offset: [f32; 2],
                             color_override: Option<[f32; 4]>| {
                                let prepared = self.text.prepare(
                                    &self.device,
                                    bounds,
                                    clip,
                                    scale,
                                    content,
                                    color_override.or(*color),
                                    *size,
                                    *weight,
                                    family.as_deref(),
                                    *line_height,
                                    *wrap,
                                    *wrap_break,
                                    *italic,
                                    *ellipsis,
                                    *max_lines,
                                    *shaping,
                                    *horizontal_alignment,
                                    *vertical_alignment,
                                    spans,
                                    *letter_spacing,
                                    font_features,
                                    opentype,
                                    affine,
                                    persp,
                                    frag_clip,
                                    opacity,
                                    extra_offset,
                                    text::EntryKey { node, slot, pass },
                                    primitive.revision,
                                    layout.as_ref(),
                                    opaque_backdrop,
                                );
                                pass += 1;
                                if let Some(prepared) = prepared {
                                    let painted = painted_bounds(
                                        prepared.ink,
                                        0.0,
                                        affine,
                                        persp,
                                        scale,
                                        scissor,
                                    );
                                    push_text_run(
                                        commands,
                                        batching,
                                        &mut self.text,
                                        prepared,
                                        scissor,
                                        painted,
                                    );
                                }
                            };
                        if let Some(shadow) = text_shadow {
                            let base_color = with_opacity(shadow.color, opacity);
                            for (dx, dy, alpha_scale) in text_shadow_draw_offsets(*shadow) {
                                let scaled = [
                                    base_color[0],
                                    base_color[1],
                                    base_color[2],
                                    base_color[3] * alpha_scale,
                                ];
                                push_text(
                                    &mut commands,
                                    &mut batching,
                                    [shadow.offset_x + dx, shadow.offset_y + dy],
                                    Some(scaled),
                                );
                            }
                        }
                        push_text(&mut commands, &mut batching, [0.0, 0.0], None);
                    }
                    ScenePrimitiveKind::QuadColorBatch {
                        bounds: batch,
                        colors,
                        border_color,
                        border_width,
                        corner_radius,
                    } => {
                        // Per-item solid colors (editor color swatches); no
                        // shadow and no surface paint by construction, so the
                        // batch collapses to one quads.push per item.
                        let no_shadow: Option<nana_ui_runtime::ComponentElevation> = None;
                        let default_surface = nana_ui_scene::QuadSurfacePaint::default();
                        for (item, color) in batch.iter().zip(colors.iter()) {
                            let item_bounds = local_rect(*item);
                            let painted = quad_painted_bounds(
                                item_bounds,
                                None,
                                &default_surface,
                                affine,
                                persp,
                                scale,
                                scissor,
                                dest_physical,
                            );
                            if let Some(index) = self.quads.push_with_work(
                                &self.device,
                                &self.queue,
                                &mut self.url_cache,
                                item_bounds,
                                clip,
                                vertex_clip,
                                affine,
                                persp,
                                Some(*color),
                                *border_color,
                                *border_width,
                                *corner_radius,
                                no_shadow,
                                opacity,
                                &default_surface,
                                Some(&gpu_work),
                            ) {
                                for quad_index in index..self.quads.pending_len() {
                                    push_quad(
                                        &mut commands,
                                        &mut batching,
                                        quad_index,
                                        scissor,
                                        painted,
                                    );
                                }
                            }
                        }
                    }
                    ScenePrimitiveKind::Icon { icon, color } => {
                        if let Some(prepared) = self.icons.prepare_with_work(
                            &self.device,
                            &self.queue,
                            bounds,
                            affine,
                            persp,
                            scale,
                            *icon,
                            color.unwrap_or([0.0, 0.0, 0.0, 1.0]),
                            opacity,
                            vertex_clip,
                            Some(&gpu_work),
                        ) {
                            push_icon(
                                &mut commands,
                                &mut batching,
                                &self.icons,
                                prepared,
                                scissor,
                                painted_bounds(bounds, 0.0, affine, persp, scale, scissor),
                            );
                        }
                    }
                    ScenePrimitiveKind::IconBatch {
                        bounds: batch,
                        icon,
                        color,
                    } => {
                        for item in batch {
                            let item_bounds = local_rect(*item);
                            if let Some(prepared) = self.icons.prepare_with_work(
                                &self.device,
                                &self.queue,
                                item_bounds,
                                affine,
                                persp,
                                scale,
                                *icon,
                                color.unwrap_or([0.0, 0.0, 0.0, 1.0]),
                                opacity,
                                vertex_clip,
                                Some(&gpu_work),
                            ) {
                                push_icon(
                                    &mut commands,
                                    &mut batching,
                                    &self.icons,
                                    prepared,
                                    scissor,
                                    painted_bounds(item_bounds, 0.0, affine, persp, scale, scissor),
                                );
                            }
                        }
                    }
                    ScenePrimitiveKind::Spinner { phase, color } => {
                        if let Some(range) = self.meshes.push_spinner(
                            bounds,
                            mesh_affine(affine, persp),
                            *phase,
                            color.unwrap_or([0.0, 0.0, 0.0, 1.0]),
                            opacity,
                            frag_clip,
                        ) {
                            push_mesh_draw(
                                &mut commands,
                                &mut batching,
                                range,
                                scissor,
                                painted_bounds(
                                    bounds,
                                    0.0,
                                    mesh_affine(affine, persp),
                                    [0.0, 0.0],
                                    scale,
                                    scissor,
                                ),
                            );
                        }
                    }
                    ScenePrimitiveKind::Stroke {
                        points,
                        width,
                        color,
                        widths,
                        cap,
                        pattern,
                    } => {
                        let (dash, dash_offset, colors, path_length) = match pattern.as_deref() {
                            Some(pattern) => (
                                pattern.dash.as_slice(),
                                pattern.dash_offset,
                                pattern.colors.as_slice(),
                                pattern.path_length,
                            ),
                            None => ([].as_slice(), 0.0, [].as_slice(), 0.0),
                        };
                        if let Some(range) = self.meshes.push_stroke_with_path_length(
                            points,
                            StrokeStyle {
                                width: *width,
                                widths,
                                cap: *cap,
                                dash,
                                dash_offset,
                                colors,
                            },
                            mesh_affine(affine, persp),
                            *color,
                            opacity,
                            frag_clip,
                            path_length,
                        ) {
                            // Caps and joins reach half a stroke width past
                            // the polyline; one more pixel covers AA.
                            let reach =
                                widths.iter().copied().fold(*width, f32::max).max(0.0) + 1.0;
                            push_mesh_draw(
                                &mut commands,
                                &mut batching,
                                range,
                                scissor,
                                painted_bounds(
                                    polyline_bounds(points).unwrap_or(bounds),
                                    reach,
                                    mesh_affine(affine, persp),
                                    [0.0, 0.0],
                                    scale,
                                    scissor,
                                ),
                            );
                        }
                    }
                    ScenePrimitiveKind::LayerBegin { .. } | ScenePrimitiveKind::LayerEnd { .. } => {
                        unreachable!("painter layers are handled before culling")
                    }
                    ScenePrimitiveKind::Path { mesh, origin } => {
                        let path_affine = mesh_affine(affine, persp);
                        if let Some(range) =
                            self.meshes
                                .push_path(mesh, *origin, path_affine, opacity, frag_clip)
                        {
                            // The AA fringe is physical pixels past the
                            // mesh whatever the transform: one along an
                            // edge, up to the miter limit (4) at a corner.
                            let drawn =
                                transformed_aabb_projective(bounds, path_affine, [0.0, 0.0]);
                            let fringe = 5.0 / scale;
                            push_path_draw(
                                &mut commands,
                                &mut batching,
                                range,
                                scissor,
                                physical_bounds(
                                    LogicalRect::from_xywh(
                                        drawn.x - fringe,
                                        drawn.y - fringe,
                                        drawn.width + fringe * 2.0,
                                        drawn.height + fringe * 2.0,
                                    ),
                                    scale,
                                    scissor,
                                ),
                            );
                        }
                    }
                    ScenePrimitiveKind::Custom {
                        node: custom,
                        mask,
                        corner_radius,
                    } => {
                        if custom.renderer.as_ref() == "nana.host-texture" {
                            // The registry is a shared RwLock: an entry validated at
                            // frame start can be removed before prepare. Skip the
                            // node for this frame instead of panicking.
                            let Some(binding) = host_textures
                                .and_then(|registry| registry.get(custom.resource.as_ref()))
                            else {
                                continue;
                            };
                            let dest = nana_ui_core::LogicalRect::new(
                                bounds.x,
                                bounds.y,
                                bounds.width,
                                bounds.height,
                            )
                            .fitted(
                                binding.width as f32,
                                binding.height as f32,
                                custom.fit,
                            );
                            batching.close_all();
                            let prepared = self.host_textures.prepare(
                                &self.device,
                                &self.queue,
                                &mut self.url_cache,
                                binding,
                                primitive.id.node.get(),
                                primitive.id.slot,
                                LogicalRect::from_xywh(dest.x, dest.y, dest.width, dest.height),
                                affine,
                                persp,
                                scissor,
                                opacity,
                                *corner_radius,
                                bounds,
                                vertex_clip,
                                dest_physical,
                                scale,
                                mask.clone(),
                                Some(&gpu_work),
                                custom.checkerboard,
                                custom.zoom,
                                custom.sampling,
                            );
                            self.painted_demand.note(&custom.resource, prepared.painted);
                            commands.push(DrawCommand::HostTexture(prepared));
                        } else {
                            let Some(renderer) = gpu_renderers
                                .and_then(|registry| registry.get(custom.renderer.as_ref()))
                            else {
                                continue;
                            };
                            let node = SceneGpuNode {
                                id: primitive.id,
                                custom: custom.clone(),
                                opacity,
                            };
                            let custom_bounds = custom_paint_bounds(bounds, affine, persp, scale);
                            renderer.prepare(
                                &node,
                                SceneGpuPrepareContext {
                                    gpu: &self.gpu,
                                    target_format: __framework::format_from_wgpu(self.format),
                                    bounds: custom_bounds.to_core(),
                                    scale_factor: scale,
                                    dest_size: dest_physical,
                                    gpu_work: Some(&gpu_work),
                                },
                            );
                            batching.close_all();
                            commands.push(DrawCommand::Custom {
                                node,
                                renderer,
                                bounds: physical_bounds(custom_bounds, scale, scissor),
                                clip: scissor,
                            });
                        }
                    }
                }
                if !clip_dests.is_empty()
                    && wrap_drawn_with_clip_dests(
                        &mut commands,
                        command_start,
                        &mut group_depth,
                        &mut group_slots,
                        &mut max_group_depth,
                        &mut group_slots_uniforms,
                        &clip_dests,
                        scale,
                    )
                {
                    batching.close_all();
                }
            }
            if !painter_layers.is_empty() {
                batching.close_all();
                while painter_layers.pop().is_some() {
                    group_depth = group_depth.saturating_sub(1);
                    commands.push(DrawCommand::PopGroup);
                }
            }
            if sync_opacity_groups(
                &mut commands,
                &mut group_stack,
                &mut group_depth,
                &mut group_slots,
                &mut max_group_depth,
                &mut group_slots_uniforms,
                &[],
                scene,
                origin,
                scale,
            ) {
                batching.close_all();
            }
            // Text prepare stays inside the batch window: it is the same
            // work the per-primitive prepare did, only once per run.
            self.text.flush_runs();
            self.painted_demand.commit(host_textures);
            self.url_cache.commit_demand(self.painted_demand.id());
            let batch = batch_started.elapsed();

            let upload_started = Instant::now();
            self.quads.upload(
                &self.device,
                &self.queue,
                &self.url_cache,
                dest_physical,
                scale,
                Some(&gpu_work),
            );
            self.meshes.upload(
                &self.device,
                &self.queue,
                dest_physical,
                scale,
                Some(&gpu_work),
            );
            self.icons
                .upload(&self.device, &self.queue, Some(&gpu_work));
            self.text
                .upload_with(&self.device, &self.queue, encoder, Some(&gpu_work));
            self.backdrop
                .upload(&self.device, &self.queue, dest_physical, Some(&gpu_work));
            let gpu_upload = upload_started.elapsed();
            let group_extents = group_extents(&commands, &batching.painted, &group_slots_uniforms);

            (
                commands,
                max_group_depth,
                group_slots_uniforms,
                group_extents,
                batching.glyph_then_quad,
                batch,
                gpu_upload,
            )
        };

        let encode_started = Instant::now();
        // The MSAA-geometry then single-sample-glyph optimization is valid only
        // when glyphs form a suffix. A later panel, modal scrim or mesh must
        // cover earlier text/icons in document order, so `Batching` reads that
        // half off the emit order — folding a quad into an earlier batch can
        // leave glyphs at the end and must not change the sample count. The
        // commands that rule out MSAA outright are never merged, so they are
        // still read from the list.
        let gpu_interleaved = glyph_then_quad
            || commands.iter().any(|command| {
                matches!(
                    command,
                    DrawCommand::HostTexture(_)
                        | DrawCommand::Custom { .. }
                        | DrawCommand::PushGroup { .. }
                        | DrawCommand::PopGroup
                        | DrawCommand::Backdrop { .. }
                )
            })
            || self.backdrop.needs_backdrop();
        DestTarget::ensure(
            &mut self.dest,
            &self.device,
            self.dest_pipeline_cache.as_ref(),
            self.format,
            dest_physical[0],
            dest_physical[1],
            !gpu_interleaved,
            self.gpu.policy(),
        );
        if max_group_depth > 0 {
            self.dest.as_mut().expect("dest target").prepare_groups(
                &self.device,
                &self.queue,
                max_group_depth,
                &group_slots_uniforms,
                Some(&gpu_work),
            );
        }
        let dest = self.dest.as_ref().expect("dest target");
        let sample_count = if gpu_interleaved { 1 } else { 4 };
        let initial_load = if viewport.clear {
            wgpu::LoadOp::Clear(clear)
        } else {
            wgpu::LoadOp::Load
        };
        let mut dest_passes = DestPassCounts {
            msaa_allocated: dest.msaa_allocated,
            ..DestPassCounts::default()
        };
        self.motion
            .sync_with_work(&self.device, &self.queue, scene, Some(&gpu_work));
        if gpu_interleaved {
            encode_ordered(
                &mut EncodeOrdered {
                    quads: &self.quads,
                    meshes: &self.meshes,
                    icons: &self.icons,
                    text: &self.text,
                    host_textures: &self.host_textures,
                    backdrop: &mut self.backdrop,
                    gpu: &self.gpu,
                    format: __framework::format_from_wgpu(self.format),
                    gpu_work: &gpu_work,
                    motion: self.motion.bind_group(),
                    group_extents: &group_extents,
                },
                encoder,
                dest,
                dest_physical,
                initial_load,
                &commands,
                &mut dest_passes,
            );
        } else {
            let mut pass = dest.begin_msaa_pass(encoder, clear, &mut dest_passes);
            restore_dest_viewport(&mut pass, dest_physical);
            for command in &commands {
                match command {
                    DrawCommand::Quads { range, scissor } => {
                        self.quads.draw(
                            &mut pass,
                            range.clone(),
                            *scissor,
                            sample_count,
                            self.motion.bind_group(),
                            Some(&gpu_work),
                        );
                    }
                    DrawCommand::Mesh { range, scissor } => {
                        self.meshes
                            .draw(&mut pass, range, *scissor, sample_count, Some(&gpu_work));
                    }
                    DrawCommand::Path { range, scissor } => {
                        self.meshes.draw_path(
                            &mut pass,
                            range,
                            *scissor,
                            sample_count,
                            Some(&gpu_work),
                        );
                    }
                    // A mask only ever sits inside a group, and a frame with
                    // groups is encoded in order, not here.
                    DrawCommand::PathMask { .. } => {}
                    DrawCommand::Text { .. }
                    | DrawCommand::Icon { .. }
                    | DrawCommand::HostTexture(_)
                    | DrawCommand::Backdrop { .. }
                    | DrawCommand::Custom { .. }
                    | DrawCommand::PushGroup { .. }
                    | DrawCommand::PopGroup => {}
                }
            }
            drop(pass);
            if commands.iter().any(|command| {
                matches!(command, DrawCommand::Text { .. } | DrawCommand::Icon { .. })
            }) {
                let mut pass = dest.begin_color_pass(encoder, wgpu::LoadOp::Load, &mut dest_passes);
                restore_dest_viewport(&mut pass, dest_physical);
                for command in &commands {
                    match command {
                        DrawCommand::Text { prepared, scissor } => {
                            self.text
                                .draw(&mut pass, prepared, *scissor, Some(&gpu_work));
                        }
                        DrawCommand::Icon {
                            first,
                            count,
                            scissor,
                        } => {
                            self.icons
                                .draw(&mut pass, *first, *count, *scissor, Some(&gpu_work));
                        }
                        _ => {}
                    }
                }
                drop(pass);
            }
        }
        dest.blit(
            encoder,
            target,
            blit_origin[0],
            blit_origin[1],
            viewport.physical_size,
            viewport.clear.then_some(clear),
            self.alpha_encoding,
            Some(&gpu_work),
            &mut dest_passes,
        );
        let encode = encode_started.elapsed();
        if !reused {
            self.url_cache.trim();
            self.quads.finish_frame(&self.url_cache);
            self.host_textures.trim();
        }
        for _ in 0..self.text.take_frame_gpu_allocations() {
            gpu_work.record_realloc();
        }
        self.last_gpu_work = Some(gpu_work.snapshot());
        self.last_gpu_timings = Some(GpuStageTimings {
            batch,
            gpu_upload,
            encode,
            submit: std::time::Duration::ZERO,
        });
        self.last_dest_pass_counts = Some(dest_passes);
        // Host-driven pixels (host textures, GPU slots) can change while the
        // scene stands still, so only fully scene-described frames may be
        // reused by the next paint.
        self.painted = commands
            .iter()
            .all(|command| {
                !matches!(
                    command,
                    DrawCommand::HostTexture(_) | DrawCommand::Custom { .. }
                )
            })
            .then_some(painted);
        if resolved.cacheable {
            self.prepared_batch = Some(PreparedBatch {
                key: painted,
                resources: resolved.resources,
                renderers: resolved.renderers,
                image_revision: self.image_revision,
                commands,
                max_group_depth,
                group_slots: group_slots_uniforms,
                group_extents,
                glyph_then_quad,
                text_placement_epoch: self.text.placement_epoch(),
            });
        }
        Ok(())
    }
}

/// Stroke/spinner cannot take a homography without a new mesh pipeline.
/// Planar 3D paints identity here so we do not squash like `scaleX(cos)`.
fn mesh_affine(affine: [f32; 6], persp: [f32; 2]) -> [f32; 6] {
    if persp[0].abs() > 1e-8 || persp[1].abs() > 1e-8 {
        clip::IDENTITY_AFFINE
    } else {
        affine
    }
}

/// Non-HostTexture custom renderers have no projective VS. Identity dest
/// when `(g,h)` is live; 2D affine still maps the AABB.
fn custom_paint_bounds(
    bounds: LogicalRect,
    affine: [f32; 6],
    persp: [f32; 2],
    scale: f32,
) -> LogicalRect {
    if persp[0].abs() > 1e-8 || persp[1].abs() > 1e-8 {
        bounds
    } else if clip::is_translation_projective(affine, persp) {
        clip::translated_on_grid(bounds, affine, scale)
    } else {
        transformed_aabb(bounds, affine)
    }
}

fn clip_dests_for(
    kind: &ScenePrimitiveKind,
    clips: &[nana_ui_scene::ClipRegion],
    origin: PaintOrigin,
) -> Vec<FragmentClip> {
    // Custom has no vertex clip so wrap every clip the scissor cannot express;
    // built-ins wrap extras only.
    let keep_innermost = matches!(
        kind,
        ScenePrimitiveKind::Custom { node: custom, .. } if custom.renderer.as_ref() != "nana.host-texture"
    );
    if keep_innermost {
        clip::all_fragment_clips(clips, origin)
    } else if matches!(
        kind,
        ScenePrimitiveKind::Stroke { .. }
            | ScenePrimitiveKind::Spinner { .. }
            | ScenePrimitiveKind::Path { .. }
            | ScenePrimitiveKind::Text { .. }
    ) {
        // Their shaders test the innermost polygon or ellipse themselves.
        mesh_extra_fragment_clips(clips, origin)
    } else {
        extra_fragment_clips(clips, origin)
    }
}

#[allow(clippy::too_many_arguments)]
fn wrap_drawn_with_clip_dests(
    commands: &mut Vec<DrawCommand>,
    start: usize,
    depth: &mut usize,
    slots: &mut u32,
    max_depth: &mut usize,
    uniforms: &mut Vec<GroupSlot>,
    clips: &[FragmentClip],
    scale: f32,
) -> bool {
    if clips.is_empty() || commands.len() == start {
        return false;
    }
    let drawn: Vec<_> = commands.drain(start..).collect();
    for clip in clips {
        let layer = *depth;
        let slot = *slots;
        *slots = slots.saturating_add(1);
        *depth = depth.saturating_add(1);
        *max_depth = (*max_depth).max(*depth);
        uniforms.push(GroupSlot::clip(clip.for_physical_pixels(scale)));
        commands.push(DrawCommand::PushGroup { layer, slot });
    }
    commands.extend(drawn);
    for _ in clips {
        *depth = depth.saturating_sub(1);
        commands.push(DrawCommand::PopGroup);
    }
    true
}

#[expect(
    clippy::too_many_arguments,
    reason = "Explicit fields of the host or GPU projection contract"
)]
///
/// Reports whether a `PushGroup` or `PopGroup` is now in the list; no batch may
/// span one. A group that opened and closed with nothing inside takes its own
/// `PushGroup` back out, and since nothing was emitted while it was open, no
/// batch can name a command at or past it.
fn sync_opacity_groups(
    commands: &mut Vec<DrawCommand>,
    stack: &mut Vec<nana_ui_scene::OpacityGroup>,
    depth: &mut usize,
    slots: &mut u32,
    max_depth: &mut usize,
    uniforms: &mut Vec<GroupSlot>,
    needed: &[nana_ui_scene::OpacityGroup],
    scene: &nana_ui_scene::UiScene,
    origin: PaintOrigin,
    scale: f32,
) -> bool {
    let common = stack
        .iter()
        .zip(needed.iter())
        .take_while(|(open, want)| open.node == want.node)
        .count();
    let mut emitted = false;
    while stack.len() > common {
        emitted |= pop_opacity_group(commands, stack, depth, slots, uniforms);
    }
    for group in needed.iter().skip(common) {
        let layer = *depth;
        let slot = *slots;
        *slots = slots.saturating_add(1);
        *depth = depth.saturating_add(1);
        *max_depth = (*max_depth).max(*depth);
        uniforms.push(dest_group_slot(group, scene, origin, scale));
        commands.push(DrawCommand::PushGroup { layer, slot });
        stack.push(*group);
        emitted = true;
    }
    emitted
}

fn dest_group_slot(
    group: &nana_ui_scene::OpacityGroup,
    scene: &nana_ui_scene::UiScene,
    origin: PaintOrigin,
    scale: f32,
) -> GroupSlot {
    let filter = group.filter;
    let pad = filter.dest_extent_pad();
    let clip = if pad > 0.0 {
        scene
            .draw_node_bounds(group.node)
            .map(|bounds| {
                clip::FragmentClip {
                    rect: [
                        bounds.x + origin.offset[0] - pad,
                        bounds.y + origin.offset[1] - pad,
                        (bounds.width + pad * 2.0).max(0.0),
                        (bounds.height + pad * 2.0).max(0.0),
                    ],
                    inv_abcd: [1.0, 0.0, 0.0, 1.0],
                    inv_ef: [0.0, 0.0],
                    corner_radius: 0.0,
                    polygon_count: 0,
                    polygon: [[0.0; 2]; 8],
                }
                .for_physical_pixels(scale)
            })
            .unwrap_or(clip::FragmentClip::PASS)
    } else {
        clip::FragmentClip::PASS
    };
    let physical_blur = if scale.is_finite() && scale > 0.0 {
        filter.blur_radius * scale
    } else {
        filter.blur_radius
    };
    let mut slot = GroupSlot::dest(
        group.opacity * filter.opacity,
        [filter.brightness, filter.saturate, filter.contrast],
        filter.hue_rotate_deg,
        physical_blur,
        group.mix_blend.gpu_index(),
        clip,
    );
    slot.filter_invert = filter.invert;
    if let Some(shadow) = filter.drop_shadow {
        let (ox, oy, blur) = if scale.is_finite() && scale > 0.0 {
            (
                shadow.offset_x * scale,
                shadow.offset_y * scale,
                shadow.blur_radius * scale,
            )
        } else {
            (shadow.offset_x, shadow.offset_y, shadow.blur_radius)
        };
        slot.drop_shadow_offset = [ox, oy];
        slot.drop_shadow_blur = blur;
        slot.drop_shadow_color = pack_linear(shadow.color);
    }
    slot
}

fn pop_opacity_group(
    commands: &mut Vec<DrawCommand>,
    stack: &mut Vec<nana_ui_scene::OpacityGroup>,
    depth: &mut usize,
    slots: &mut u32,
    uniforms: &mut Vec<GroupSlot>,
) -> bool {
    stack.pop();
    *depth = depth.saturating_sub(1);
    if matches!(commands.last(), Some(DrawCommand::PushGroup { .. })) {
        commands.pop();
        uniforms.pop();
        *slots = slots.saturating_sub(1);
        return false;
    }
    commands.push(DrawCommand::PopGroup);
    true
}

/// A draw command later primitives may still be folded into.
struct OpenBatch {
    /// Position of the command in the display list.
    command: usize,
    /// Its place in [`Batching::painted`].
    ordinal: usize,
    /// Union of the dest-pixel bounds of everything already in it.
    painted: PhysicalRect,
}

/// Display-list batching state.
///
/// A primitive may join a batch that is not the newest one, which draws it
/// earlier than its document position. That is sound exactly when it covers no
/// pixel of anything emitted after that batch — and everything emitted after it
/// is still open, because every command whose place in the list is part of the
/// GPU contract (`HostTexture`, `Custom`, `Backdrop`, `PushGroup`, `PopGroup`)
/// closes all of them. Overlapping primitives therefore keep painter's-algorithm
/// order to the pixel, and GPU content never moves.
#[derive(Default)]
struct Batching {
    open: Vec<OpenBatch>,
    saw_glyph: bool,
    /// A quad or mesh followed a glyph in **document order**. Read from the
    /// emit order rather than from the merged list, because folding a quad into
    /// an earlier batch can leave glyphs at the end and that must not turn a
    /// single-sampled frame into a 4x MSAA one.
    glyph_then_quad: bool,
    /// Dest-pixel bounds of every batched command (quads, meshes, paths,
    /// text, icons), in list order. Indexed by order among those commands
    /// rather than list position: clip dests wrap drawn commands in groups
    /// after the fact, which moves them but never reorders them.
    painted: Vec<PhysicalRect>,
}

impl Batching {
    /// Close every batch. Required around anything that fixes a command's
    /// position or moves commands within the list.
    fn close_all(&mut self) {
        self.open.clear();
    }

    fn note_glyph(&mut self) {
        self.saw_glyph = true;
    }

    fn note_geometry(&mut self) {
        self.glyph_then_quad |= self.saw_glyph;
    }

    /// Position in `commands` of the batch this primitive may join. The
    /// candidate is identified by `extendable`, which reads the command itself
    /// — including its scissor — so a batch stores no copy of either.
    fn target(
        &mut self,
        commands: &[DrawCommand],
        painted: PhysicalRect,
        extendable: impl Fn(&DrawCommand) -> bool,
    ) -> Option<usize> {
        let position = self
            .open
            .iter()
            .rposition(|batch| extendable(&commands[batch.command]))?;
        if self.open[position + 1..]
            .iter()
            .any(|later| overlaps_physical(painted, later.painted))
        {
            return None;
        }
        let batch = &mut self.open[position];
        batch.painted = union_physical(batch.painted, painted);
        self.painted[batch.ordinal] = batch.painted;
        Some(batch.command)
    }

    fn open(&mut self, command: usize, painted: PhysicalRect) {
        self.open.push(OpenBatch {
            command,
            ordinal: self.painted.len(),
            painted,
        });
        self.painted.push(painted);
    }
}

/// Dest-pixel bounds a primitive can paint, used only to decide whether folding
/// it into an earlier batch would cross something. It must never under-cover
/// what the GPU paints, so callers pass the outset their paint adds (shadow,
/// outline, stroke half-width) before the transform.
fn painted_bounds(
    local: LogicalRect,
    outset: f32,
    affine: [f32; 6],
    persp: [f32; 2],
    scale: f32,
    scissor: PhysicalRect,
) -> PhysicalRect {
    let padded = LogicalRect::from_xywh(
        local.x - outset,
        local.y - outset,
        local.width + outset * 2.0,
        local.height + outset * 2.0,
    );
    physical_bounds(
        transformed_aabb_projective(padded, affine, persp),
        scale,
        scissor,
    )
}

/// AABB of a polyline in its own local space. `None` when it has no finite
/// point, which is also when the mesh pipeline refuses it.
fn polyline_bounds(points: &[[f32; 2]]) -> Option<LogicalRect> {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for point in points {
        if !point[0].is_finite() || !point[1].is_finite() {
            return None;
        }
        min = [min[0].min(point[0]), min[1].min(point[1])];
        max = [max[0].max(point[0]), max[1].max(point[1])];
    }
    (min[0] <= max[0])
        .then(|| LogicalRect::from_xywh(min[0], min[1], max[0] - min[0], max[1] - min[1]))
}

/// [`painted_bounds`] for a quad: its box grown by the largest outer shadow and
/// the outline. The antialiased edge needs no margin of its own — the box is
/// rounded outwards to whole pixels when it is put in dest space. A filtered
/// surface has no rectangular reach, so it takes the whole dest and can then
/// neither join a batch nor be jumped over.
#[allow(clippy::too_many_arguments)]
fn quad_painted_bounds(
    local: LogicalRect,
    shadow: Option<&nana_ui_runtime::ComponentElevation>,
    surface: &nana_ui_scene::QuadSurfacePaint,
    affine: [f32; 6],
    persp: [f32; 2],
    scale: f32,
    scissor: PhysicalRect,
    dest_physical: [u32; 2],
) -> PhysicalRect {
    if surface.filter.is_some() || surface.backdrop_filter.is_some() {
        return PhysicalRect {
            x: 0,
            y: 0,
            width: dest_physical[0],
            height: dest_physical[1],
        };
    }
    let outset = shadow
        .into_iter()
        .chain(surface.extra_shadows.iter())
        .filter(|shadow| !shadow.inset)
        .map(|shadow| {
            shadow.offset_x.abs().max(shadow.offset_y.abs())
                + shadow.blur_radius * 3.0
                + shadow.spread_radius.max(0.0)
        })
        .fold(surface.outline_width.max(0.0) + 1.0, f32::max);
    painted_bounds(local, outset, affine, persp, scale, scissor)
}

/// Extend an open icon batch when the new slot shares its atlas, scissor and
/// vertex adjacency.
fn push_icon(
    commands: &mut Vec<DrawCommand>,
    batching: &mut Batching,
    icons: &IconPipeline,
    prepared: PreparedIcon,
    scissor: PhysicalRect,
    painted: PhysicalRect,
) {
    batching.note_glyph();
    let slot = prepared.index;
    if let Some(target) = batching.target(commands, painted, |command| {
        matches!(command, DrawCommand::Icon { first, count, scissor: open }
            if *open == scissor && icons.can_extend_run(*first, *count, slot))
    }) {
        let DrawCommand::Icon { count, .. } = &mut commands[target] else {
            unreachable!("an icon batch names an icon command")
        };
        *count += 1;
        return;
    }
    batching.open(commands.len(), painted);
    commands.push(DrawCommand::Icon {
        first: slot,
        count: 1,
        scissor,
    });
}

/// Fold a prepared text into an open text run.
fn push_text_run(
    commands: &mut Vec<DrawCommand>,
    batching: &mut Batching,
    text: &mut TextPipeline,
    prepared: PreparedText,
    scissor: PhysicalRect,
    painted: PhysicalRect,
) {
    batching.note_glyph();
    if let Some(target) = batching.target(commands, painted, |command| {
        matches!(command, DrawCommand::Text { prepared: previous, scissor: open }
            if *open == scissor && text.can_merge_runs(previous, &prepared))
    }) {
        let DrawCommand::Text {
            prepared: previous, ..
        } = &commands[target]
        else {
            unreachable!("a text batch names a text command")
        };
        text.merge_runs(previous, &prepared);
        return;
    }
    batching.open(commands.len(), painted);
    commands.push(DrawCommand::Text { prepared, scissor });
}

fn push_quad(
    commands: &mut Vec<DrawCommand>,
    batching: &mut Batching,
    index: u32,
    scissor: PhysicalRect,
    painted: PhysicalRect,
) {
    batching.note_geometry();
    if let Some(target) = batching.target(commands, painted, |command| {
        matches!(command, DrawCommand::Quads { range, scissor: open }
            if *open == scissor && range.end == index)
    }) {
        let DrawCommand::Quads { range, .. } = &mut commands[target] else {
            unreachable!("a quad batch names a quad command")
        };
        range.end = index + 1;
        return;
    }
    batching.open(commands.len(), painted);
    commands.push(DrawCommand::Quads {
        range: index..index + 1,
        scissor,
    });
}

fn push_mesh_draw(
    commands: &mut Vec<DrawCommand>,
    batching: &mut Batching,
    range: MeshRange,
    scissor: PhysicalRect,
    painted: PhysicalRect,
) {
    batching.note_geometry();
    if let Some(target) = batching.target(commands, painted, |command| {
        matches!(command, DrawCommand::Mesh { range: previous, scissor: open }
            if *open == scissor
                && previous.first_instance + previous.instance_count == range.first_instance)
    }) {
        let DrawCommand::Mesh {
            range: previous, ..
        } = &mut commands[target]
        else {
            unreachable!("a mesh batch names a mesh command")
        };
        previous.instance_count += range.instance_count;
        return;
    }
    batching.open(commands.len(), painted);
    commands.push(DrawCommand::Mesh { range, scissor });
}

fn push_path_draw(
    commands: &mut Vec<DrawCommand>,
    batching: &mut Batching,
    range: PathRange,
    scissor: PhysicalRect,
    painted: PhysicalRect,
) {
    batching.note_geometry();
    if let Some(target) = batching.target(commands, painted, |command| {
        matches!(command, DrawCommand::Path { range: previous, scissor: open }
            if *open == scissor
                && previous.first_index + previous.index_count == range.first_index)
    }) {
        let DrawCommand::Path {
            range: previous, ..
        } = &mut commands[target]
        else {
            unreachable!("a path batch names a path command")
        };
        previous.index_count += range.index_count;
        return;
    }
    batching.open(commands.len(), painted);
    commands.push(DrawCommand::Path { range, scissor });
}

fn text_shadow_draw_offsets(shadow: nana_ui_core::TextShadowSpec) -> Vec<(f32, f32, f32)> {
    let mut out = vec![(0.0, 0.0, 1.0)];
    let blur = shadow.blur_radius.max(0.0);
    if blur > 0.5 {
        let step = blur * 0.35;
        out.push((step, 0.0, 0.55));
        out.push((-step, 0.0, 0.55));
        out.push((0.0, step, 0.55));
        out.push((0.0, -step, 0.55));
    }
    out
}

struct EncodeOrdered<'a> {
    quads: &'a QuadPipeline,
    meshes: &'a MeshPipeline,
    icons: &'a IconPipeline,
    text: &'a TextPipeline,
    host_textures: &'a HostTexturePipeline,
    backdrop: &'a mut BackdropPipeline,
    gpu: &'a GpuContext,
    format: GpuTextureFormat,
    gpu_work: &'a GpuWorkSink,
    motion: &'a wgpu::BindGroup,
    /// What each group can change, by slot ([`group_extents`]).
    group_extents: &'a [GroupExtent],
}

/// Which dest pixels a group can change when it composites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupExtent {
    /// It drew nothing.
    Empty,
    Rect(PhysicalRect),
    /// Not known to be less than the whole target.
    All,
}

impl GroupExtent {
    fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::All, _) | (_, Self::All) => Self::All,
            (Self::Empty, other) | (other, Self::Empty) => other,
            (Self::Rect(a), Self::Rect(b)) => Self::Rect(union_physical(a, b)),
        }
    }
}

/// Each group's extent, by slot: the union of what its commands paint, and
/// the whole target once anything in it has no known bounds or its filter
/// can move pixels past what was drawn.
fn group_extents(
    commands: &[DrawCommand],
    painted: &[PhysicalRect],
    slots: &[GroupSlot],
) -> Vec<GroupExtent> {
    let mut extents = vec![GroupExtent::All; slots.len()];
    if slots.is_empty() {
        return extents;
    }
    let mut stack: Vec<(u32, GroupExtent)> = Vec::new();
    let mut batched = painted.iter();
    for command in commands {
        let drawn = match command {
            DrawCommand::Quads { .. }
            | DrawCommand::Mesh { .. }
            | DrawCommand::Path { .. }
            | DrawCommand::Text { .. }
            | DrawCommand::Icon { .. } => batched
                .next()
                .map_or(GroupExtent::All, |rect| GroupExtent::Rect(*rect)),
            // A mask only takes away from the layer it is applied to.
            DrawCommand::PathMask { .. } => GroupExtent::Empty,
            // A custom renderer may open its own pass, which nothing
            // scissors to the bounds it was given.
            DrawCommand::Custom { .. }
            | DrawCommand::HostTexture(_)
            | DrawCommand::Backdrop { .. } => GroupExtent::All,
            DrawCommand::PushGroup { slot, .. } => {
                stack.push((*slot, GroupExtent::Empty));
                continue;
            }
            DrawCommand::PopGroup => {
                let Some((slot, extent)) = stack.pop() else {
                    continue;
                };
                let extent = match slots.get(slot as usize) {
                    Some(group) if !group.spreads() => extent,
                    _ => GroupExtent::All,
                };
                if let Some(held) = extents.get_mut(slot as usize) {
                    *held = extent;
                }
                extent
            }
        };
        if let Some((_, extent)) = stack.last_mut() {
            *extent = extent.union(drawn);
        }
    }
    extents
}

struct GroupFrame {
    layer: usize,
    slot: u32,
}

fn encode_ordered(
    pipelines: &mut EncodeOrdered<'_>,
    encoder: &mut wgpu::CommandEncoder,
    dest: &DestTarget,
    dest_physical: [u32; 2],
    initial_load: wgpu::LoadOp<wgpu::Color>,
    commands: &[DrawCommand],
    dest_passes: &mut DestPassCounts,
) {
    const SAMPLE_COUNT: u32 = 1;
    let mut index = 0;
    let mut dest_load = initial_load;
    let mut stack: Vec<GroupFrame> = Vec::new();
    let mut layer_ready = Vec::new();
    while index < commands.len() {
        match &commands[index] {
            DrawCommand::PushGroup { layer, slot } => {
                if *layer >= layer_ready.len() {
                    layer_ready.resize(*layer + 1, false);
                }
                layer_ready[*layer] = false;
                stack.push(GroupFrame {
                    layer: *layer,
                    slot: *slot,
                });
                index += 1;
            }
            DrawCommand::PopGroup => {
                let Some(frame) = stack.pop() else {
                    index += 1;
                    continue;
                };
                if !layer_ready.get(frame.layer).copied().unwrap_or(false) {
                    // Nothing drew into the layer, so it was never cleared:
                    // it still holds whatever last used that depth. An empty
                    // group composites to nothing, filters and blends alike.
                    index += 1;
                    continue;
                }
                // Where the group can change its parent: a blend that reads
                // the backdrop copies and composites only there.
                let whole = PhysicalRect {
                    x: 0,
                    y: 0,
                    width: dest_physical[0],
                    height: dest_physical[1],
                };
                let mut area = None;
                if dest.reads_backdrop(frame.slot) {
                    let region = match pipelines.group_extents.get(frame.slot as usize) {
                        Some(GroupExtent::Empty) => {
                            // Nothing drawn: every blend of a transparent
                            // source leaves the backdrop as it was.
                            index += 1;
                            continue;
                        }
                        Some(GroupExtent::Rect(rect)) => intersect_physical(*rect, whole),
                        Some(GroupExtent::All) | None => whole,
                    };
                    if region.width == 0 || region.height == 0 {
                        index += 1;
                        continue;
                    }
                    area = Some(region);
                    // The blend reads what it lands on: bring the parent up
                    // to date — a first use still has to clear it — and copy
                    // it before compositing over it.
                    let parent = stack.last().map(|parent| parent.layer);
                    match parent {
                        Some(layer) => {
                            if let wgpu::LoadOp::Clear(_) =
                                group_layer_load(&mut layer_ready, layer)
                            {
                                let mut pass = dest.begin_group_pass(
                                    encoder,
                                    layer,
                                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    dest_passes,
                                );
                                restore_dest_viewport(&mut pass, dest_physical);
                            }
                        }
                        None => ensure_backdrop_target_ready(
                            dest,
                            encoder,
                            dest_physical,
                            &mut dest_load,
                            dest_passes,
                        ),
                    }
                    dest.copy_backdrop(encoder, parent, region);
                }
                let mut pass = match stack.last() {
                    Some(parent) => {
                        let load = group_layer_load(&mut layer_ready, parent.layer);
                        dest.begin_group_pass(encoder, parent.layer, load, dest_passes)
                    }
                    None => {
                        let pass = dest.begin_color_pass(encoder, dest_load, dest_passes);
                        dest_load = wgpu::LoadOp::Load;
                        pass
                    }
                };
                restore_dest_viewport(&mut pass, dest_physical);
                if let Some(area) = area {
                    pass.set_scissor_rect(area.x, area.y, area.width, area.height);
                }
                dest.composite_group(&mut pass, frame.layer, frame.slot, Some(pipelines.gpu_work));
                drop(pass);
                index += 1;
            }
            _ => {
                if let DrawCommand::Backdrop {
                    index: backdrop_index,
                } = &commands[index]
                {
                    let Some(request) = pipelines.backdrop.request(*backdrop_index).copied() else {
                        index += 1;
                        continue;
                    };
                    ensure_backdrop_target_ready(
                        dest,
                        encoder,
                        dest_physical,
                        &mut dest_load,
                        dest_passes,
                    );
                    pipelines.backdrop.encode(
                        __framework::device(pipelines.gpu),
                        encoder,
                        dest.color_view(),
                        dest_physical,
                        dest.color_view(),
                        &request,
                        pipelines.quads.paint_buffer(),
                        Some(pipelines.gpu_work),
                        &mut dest_passes.backdrop,
                    );
                    dest_load = wgpu::LoadOp::Load;
                    index += 1;
                    continue;
                }
                let mut pass = match stack.last() {
                    Some(frame) => {
                        let load = group_layer_load(&mut layer_ready, frame.layer);
                        dest.begin_group_pass(encoder, frame.layer, load, dest_passes)
                    }
                    None => {
                        let pass = dest.begin_color_pass(encoder, dest_load, dest_passes);
                        dest_load = wgpu::LoadOp::Load;
                        pass
                    }
                };
                restore_dest_viewport(&mut pass, dest_physical);
                while index < commands.len() {
                    match &commands[index] {
                        DrawCommand::PushGroup { .. } | DrawCommand::PopGroup => break,
                        DrawCommand::Backdrop { .. } => break,
                        DrawCommand::Quads { range, scissor } => {
                            pipelines.quads.draw(
                                &mut pass,
                                range.clone(),
                                *scissor,
                                SAMPLE_COUNT,
                                pipelines.motion,
                                Some(pipelines.gpu_work),
                            );
                        }
                        DrawCommand::Mesh { range, scissor } => {
                            pipelines.meshes.draw(
                                &mut pass,
                                range,
                                *scissor,
                                SAMPLE_COUNT,
                                Some(pipelines.gpu_work),
                            );
                        }
                        DrawCommand::Path { range, scissor } => {
                            pipelines.meshes.draw_path(
                                &mut pass,
                                range,
                                *scissor,
                                SAMPLE_COUNT,
                                Some(pipelines.gpu_work),
                            );
                        }
                        DrawCommand::PathMask {
                            range,
                            scissor,
                            mode,
                        } => {
                            pipelines.meshes.draw_mask(
                                &mut pass,
                                range,
                                *scissor,
                                *mode,
                                Some(pipelines.gpu_work),
                            );
                        }
                        DrawCommand::Text { prepared, scissor } => {
                            pipelines.text.draw(
                                &mut pass,
                                prepared,
                                *scissor,
                                Some(pipelines.gpu_work),
                            );
                        }
                        DrawCommand::Icon {
                            first,
                            count,
                            scissor,
                        } => {
                            pipelines.icons.draw(
                                &mut pass,
                                *first,
                                *count,
                                *scissor,
                                Some(pipelines.gpu_work),
                            );
                        }
                        DrawCommand::HostTexture(prepared) => {
                            pipelines.host_textures.draw(
                                prepared,
                                &mut pass,
                                dest_physical,
                                Some(pipelines.gpu_work),
                            );
                            restore_dest_viewport(&mut pass, dest_physical);
                        }
                        DrawCommand::Custom {
                            node,
                            renderer,
                            bounds,
                            clip,
                        } if bounds.width > 0 && bounds.height > 0 => {
                            if !node.custom.dedicated_pass {
                                let encoded = draw_custom_run(
                                    commands,
                                    index,
                                    renderer,
                                    &mut ScenePass::new(&mut pass, dest_physical),
                                    pipelines.gpu,
                                    pipelines.format,
                                    pipelines.gpu_work,
                                );
                                if encoded > 0 {
                                    restore_dest_viewport(&mut pass, dest_physical);
                                    index += encoded;
                                    continue;
                                }
                                if renderer.draw_in_pass(
                                    node,
                                    &mut ScenePass::new(&mut pass, dest_physical),
                                    SceneGpuPassContext {
                                        gpu: pipelines.gpu,
                                        target_format: pipelines.format,
                                        bounds: *bounds,
                                        clip: *clip,
                                        dest_size: dest_physical,
                                        gpu_work: Some(pipelines.gpu_work),
                                    },
                                ) {
                                    restore_dest_viewport(&mut pass, dest_physical);
                                    index += 1;
                                    continue;
                                }
                            }
                            break;
                        }
                        DrawCommand::Custom { .. } => {}
                    }
                    index += 1;
                }
                drop(pass);
                if let Some(DrawCommand::Custom {
                    node,
                    renderer,
                    bounds,
                    clip,
                }) = commands.get(index)
                {
                    if bounds.width > 0 && bounds.height > 0 {
                        let target = match stack.last() {
                            Some(frame) => dest.group_view(frame.layer),
                            None => dest.color_view(),
                        };
                        renderer.render(
                            node,
                            SceneGpuRenderContext::new(
                                pipelines.gpu,
                                pipelines.format,
                                *bounds,
                                *clip,
                                dest_physical,
                                Some(pipelines.gpu_work),
                                encoder,
                                target,
                            ),
                        );
                    }
                    index += 1;
                }
            }
        }
    }
    // Every command may have been an empty group, skipped: the dest still
    // owes its clear.
    ensure_backdrop_target_ready(dest, encoder, dest_physical, &mut dest_load, dest_passes);
}

/// Offer the renderer the run of `Custom` commands starting at `start` that
/// share its renderer instance and want no dedicated pass.
///
/// The run is a contiguous slice of the display list, so any other command
/// between two custom nodes ends it and document order is untouched. Returns
/// how many commands the renderer encoded; `0` means it declined and the caller
/// falls back to the single-node path.
#[allow(clippy::too_many_arguments)]
fn draw_custom_run(
    commands: &[DrawCommand],
    start: usize,
    renderer: &Arc<dyn SceneGpuRenderer>,
    pass: &mut ScenePass<'_, '_>,
    gpu: &GpuContext,
    format: GpuTextureFormat,
    gpu_work: &GpuWorkSink,
) -> usize {
    let capacity = renderer.batch_capacity();
    if capacity <= 1 {
        return 0;
    }
    let mut run: Vec<SceneGpuBatchNode<'_>> = Vec::new();
    for command in &commands[start..] {
        let DrawCommand::Custom {
            node,
            renderer: candidate,
            bounds,
            clip,
        } = command
        else {
            break;
        };
        if !Arc::ptr_eq(renderer, candidate)
            || node.custom.dedicated_pass
            || bounds.width == 0
            || bounds.height == 0
            || run.len() == capacity
        {
            break;
        }
        run.push(SceneGpuBatchNode {
            node,
            bounds: *bounds,
            clip: *clip,
        });
    }
    if run.len() < 2 {
        return 0;
    }
    let encoded = renderer.draw_batch_in_pass(
        &run,
        pass,
        SceneGpuBatchPassContext {
            gpu,
            target_format: format,
            dest_size: pass.dest_size(),
            gpu_work: Some(gpu_work),
        },
    );
    encoded.min(run.len())
}

fn group_layer_load(ready: &mut Vec<bool>, layer: usize) -> wgpu::LoadOp<wgpu::Color> {
    if layer >= ready.len() {
        ready.resize(layer + 1, false);
    }
    if ready[layer] {
        wgpu::LoadOp::Load
    } else {
        ready[layer] = true;
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
    }
}

fn restore_dest_viewport(pass: &mut wgpu::RenderPass<'_>, dest_physical: [u32; 2]) {
    pass.set_viewport(
        0.0,
        0.0,
        dest_physical[0].max(1) as f32,
        dest_physical[1].max(1) as f32,
        0.0,
        1.0,
    );
}

fn ensure_backdrop_target_ready(
    dest: &DestTarget,
    encoder: &mut wgpu::CommandEncoder,
    dest_physical: [u32; 2],
    dest_load: &mut wgpu::LoadOp<wgpu::Color>,
    dest_passes: &mut DestPassCounts,
) {
    if matches!(*dest_load, wgpu::LoadOp::Clear(_)) {
        let mut pass = dest.begin_color_pass(encoder, *dest_load, dest_passes);
        restore_dest_viewport(&mut pass, dest_physical);
        drop(pass);
        *dest_load = wgpu::LoadOp::Load;
    }
}

#[cfg(test)]
mod tests;

/// Whether `rect` (layout space), under `affine` into logical target space,
/// reaches the `size` physical-pixel target.
fn rect_meets_target(
    rect: nana_ui_scene::SceneRect,
    affine: [f32; 6],
    scale: f32,
    size: [u32; 2],
) -> bool {
    let corners = [
        [rect.x, rect.y],
        [rect.x + rect.width, rect.y],
        [rect.x, rect.y + rect.height],
        [rect.x + rect.width, rect.y + rect.height],
    ]
    .map(|[x, y]| clip::transform_point(affine, x, y));
    let (mut min, mut max) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for corner in corners {
        for axis in 0..2 {
            min[axis] = min[axis].min(corner[axis] * scale);
            max[axis] = max[axis].max(corner[axis] * scale);
        }
    }
    // One pixel of slack for the clip's anti-aliased edge; NaN fails closed
    // to drawing.
    !(max[0] < -1.0
        || max[1] < -1.0
        || min[0] > size[0] as f32 + 1.0
        || min[1] > size[1] as f32 + 1.0)
}

/// Painter tests record into encoders and targets of the shared test device
/// directly; the frame contract has tests of its own.
#[cfg(test)]
impl SceneWgpuPainter {
    pub(crate) fn for_test(format: wgpu::TextureFormat) -> Self {
        Self::new(
            &crate::test_gpu::context(),
            __framework::format_from_wgpu(format),
        )
    }

    pub(crate) fn paint_encoder(
        &mut self,
        scene: &UiScene,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        viewport: ScenePaintViewport,
        host_textures: Option<&HostTextureRegistry>,
        gpu_renderers: Option<&SceneGpuRendererRegistry>,
    ) -> Result<(), ScenePaintError> {
        self.paint_recorded(
            scene,
            encoder,
            target,
            viewport,
            host_textures,
            gpu_renderers,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_target_encoder(
        &mut self,
        id: RenderTargetId,
        scene: &UiScene,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        viewport: ScenePaintViewport,
        host_textures: Option<&HostTextureRegistry>,
        gpu_renderers: Option<&SceneGpuRendererRegistry>,
    ) -> Result<(), ScenePaintError> {
        let mut state = self.targets.remove(&id).unwrap_or_default();
        if state.image_revision != self.image_revision {
            state.painted = None;
            QuadPipeline::invalidate_target_image_bindings(&mut state.quads);
            HostTexturePipeline::invalidate_target_image_bindings(&mut state.host_textures);
        }
        self.swap_target_state(&mut state);
        let result = self.paint_recorded(
            scene,
            encoder,
            target,
            viewport,
            host_textures,
            gpu_renderers,
            None,
        );
        self.swap_target_state(&mut state);
        state.image_revision = self.image_revision;
        self.targets.insert(id, state);
        result
    }
}

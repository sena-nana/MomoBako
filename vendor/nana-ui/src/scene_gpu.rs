use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use nana_gpu::{FrameContext, GpuContext, GpuSubmission, GpuTextureFormat};
use nana_ui_runtime::CustomRenderNode;
use nana_ui_scene::{PrimitiveId, ScenePrimitiveKind, UiScene};

use crate::gpu_work::GpuWorkSink;
use crate::{LogicalRect, PhysicalRect};

#[derive(Debug, Clone)]
pub struct SceneGpuNode {
    pub id: PrimitiveId,
    pub custom: CustomRenderNode,
    pub opacity: f32,
}

/// Preparation of one node, before any pass is open.
pub struct SceneGpuPrepareContext<'a> {
    pub gpu: &'a GpuContext,
    /// Format of the destination the node is drawn into.
    pub target_format: GpuTextureFormat,
    pub bounds: LogicalRect,
    pub scale_factor: f32,
    /// Destination size in physical pixels. A change to it invalidates the
    /// painter's prepared batch, so preparation always sees the size its
    /// commands will be encoded against.
    pub dest_size: [u32; 2],
    pub gpu_work: Option<&'a GpuWorkSink>,
}

/// A node that wants a pass of its own ([`SceneGpuRenderer::render`]).
///
/// The destination is the painter's current dest or group target, not the
/// window surface. [`Self::with_pass`] opens a pass on it that loads and
/// keeps what is already drawn, with the viewport on the whole destination
/// and the scissor on the node's clip.
pub struct SceneGpuRenderContext<'a> {
    pub gpu: &'a GpuContext,
    pub target_format: GpuTextureFormat,
    pub bounds: PhysicalRect,
    pub clip: PhysicalRect,
    /// Size of the destination in physical pixels. A dedicated pass covers the
    /// same destination as the main pass, so viewport and scissor math match.
    pub dest_size: [u32; 2],
    pub gpu_work: Option<&'a GpuWorkSink>,
    encoder: &'a mut wgpu::CommandEncoder,
    target: &'a wgpu::TextureView,
}

impl<'a> SceneGpuRenderContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        gpu: &'a GpuContext,
        target_format: GpuTextureFormat,
        bounds: PhysicalRect,
        clip: PhysicalRect,
        dest_size: [u32; 2],
        gpu_work: Option<&'a GpuWorkSink>,
        encoder: &'a mut wgpu::CommandEncoder,
        target: &'a wgpu::TextureView,
    ) -> Self {
        Self {
            gpu,
            target_format,
            bounds,
            clip,
            dest_size,
            gpu_work,
            encoder,
            target,
        }
    }

    /// Open a pass on the destination for `draw`, closed when it returns.
    pub fn with_pass<R>(
        &mut self,
        label: &'static str,
        draw: impl FnOnce(&mut ScenePass<'_, '_>) -> R,
    ) -> R {
        let mut pass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: self.target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let mut pass = ScenePass::new(&mut pass, self.dest_size);
        pass.restore_viewport();
        pass.set_scissor(self.clip);
        draw(&mut pass)
    }

    /// The frame's encoder, for work [`Self::with_pass`] cannot express.
    /// Record into it; never finish or submit it.
    #[cfg(feature = "wgpu-interop")]
    pub fn wgpu_encoder(&mut self) -> &mut wgpu::CommandEncoder {
        self.encoder
    }

    /// The destination view [`Self::with_pass`] draws into.
    #[cfg(feature = "wgpu-interop")]
    pub fn wgpu_target(&self) -> &wgpu::TextureView {
        self.target
    }
}

/// The painter's open pass on the current destination.
///
/// Renderers that join it must leave the viewport as they found it; the
/// scissor is theirs to set. Recording draws needs the backend pass
/// ([`Self::wgpu`], feature `wgpu-interop`) until Nana has a shader ABI of its
/// own.
pub struct ScenePass<'p, 'e> {
    raw: &'p mut wgpu::RenderPass<'e>,
    dest_size: [u32; 2],
}

impl<'p, 'e> ScenePass<'p, 'e> {
    pub(crate) fn new(raw: &'p mut wgpu::RenderPass<'e>, dest_size: [u32; 2]) -> Self {
        Self { raw, dest_size }
    }

    /// Size of the destination in physical pixels.
    pub fn dest_size(&self) -> [u32; 2] {
        self.dest_size
    }

    /// Scissor to `clip`, clamped to the destination. An empty intersection
    /// scissors everything away.
    pub fn set_scissor(&mut self, clip: PhysicalRect) {
        let [width, height] = self.dest_size;
        let x = clip.x.min(width);
        let y = clip.y.min(height);
        let right = clip.x.saturating_add(clip.width).min(width);
        let bottom = clip.y.saturating_add(clip.height).min(height);
        self.raw
            .set_scissor_rect(x, y, right.saturating_sub(x), bottom.saturating_sub(y));
    }

    /// Viewport on `rect` of the destination, full depth range.
    pub fn set_viewport(&mut self, rect: PhysicalRect) {
        self.raw.set_viewport(
            rect.x as f32,
            rect.y as f32,
            rect.width as f32,
            rect.height as f32,
            0.0,
            1.0,
        );
    }

    /// Viewport back on the whole destination, as the painter expects it.
    pub fn restore_viewport(&mut self) {
        self.raw.set_viewport(
            0.0,
            0.0,
            self.dest_size[0].max(1) as f32,
            self.dest_size[1].max(1) as f32,
            0.0,
            1.0,
        );
    }

    /// The backend pass.
    #[cfg(feature = "wgpu-interop")]
    pub fn wgpu(&mut self) -> &mut wgpu::RenderPass<'e> {
        self.raw
    }

    pub(crate) fn raw(&mut self) -> &mut wgpu::RenderPass<'e> {
        self.raw
    }
}

/// In-pass encode for a [`SceneGpuRenderer`] that can share the Scene dest.
///
/// `dest_size` is the current dest viewport in physical pixels so implementations
/// can restore it after changing scissor or viewport.
pub struct SceneGpuPassContext<'a> {
    pub gpu: &'a GpuContext,
    pub target_format: GpuTextureFormat,
    pub bounds: PhysicalRect,
    pub clip: PhysicalRect,
    pub dest_size: [u32; 2],
    pub gpu_work: Option<&'a GpuWorkSink>,
}

/// One node of a contiguous, document-ordered run handed to
/// [`SceneGpuRenderer::draw_batch_in_pass`].
pub struct SceneGpuBatchNode<'a> {
    pub node: &'a SceneGpuNode,
    /// Destination rect in physical pixels, inside the current dest or group
    /// target.
    pub bounds: PhysicalRect,
    /// Physical scissor, already intersected with every ancestor clip. Items in
    /// one run may carry different clips.
    pub clip: PhysicalRect,
}

/// In-pass encode context for a run. `dest_size` is the current dest viewport in
/// physical pixels so implementations can restore it.
pub struct SceneGpuBatchPassContext<'a> {
    pub gpu: &'a GpuContext,
    pub target_format: GpuTextureFormat,
    pub dest_size: [u32; 2],
    pub gpu_work: Option<&'a GpuWorkSink>,
}

/// Advanced in-pass Scene encode. Prefer [`crate::HostTexture`] /
/// [`crate::GpuTextureView`] for first-time hosts.
///
/// Implementations receive NanaUI's [`GpuContext`] during prepare and the
/// current frame's destination during render. Caches built on the device key
/// on [`GpuContext::generation`]: a replaced device is a new generation. They
/// must not create a second GPU context or submit the frame themselves.
pub trait SceneGpuRenderer: fmt::Debug + Send + Sync + 'static {
    /// Opt into reusing prepared UI commands. Change this version whenever
    /// `prepare` must run again. The default is dynamic; rendering still runs
    /// on every requested frame even when preparation is reused.
    fn preparation_version(&self, _node: &CustomRenderNode) -> Option<u64> {
        None
    }

    fn prepare(&self, node: &SceneGpuNode, context: SceneGpuPrepareContext<'_>);

    fn render(&self, node: &SceneGpuNode, context: SceneGpuRenderContext<'_>);

    /// Draw into the caller's current pass. Return `true` if this node was encoded.
    ///
    /// The default returns `false`, so the painter ends the main pass and calls
    /// [`Self::render`]. Prefer joining when the renderer can use the dest
    /// sample count and format.
    fn draw_in_pass(
        &self,
        _node: &SceneGpuNode,
        _pass: &mut ScenePass<'_, '_>,
        _context: SceneGpuPassContext<'_>,
    ) -> bool {
        false
    }

    /// Longest run this renderer wants in one [`Self::draw_batch_in_pass`].
    /// `1` (the default) keeps the one-node-per-call path.
    fn batch_capacity(&self) -> usize {
        1
    }

    /// Encode a contiguous, document-ordered run into the caller's pass.
    ///
    /// Returns how many **leading** nodes were encoded. `0` makes the painter
    /// fall back to [`Self::draw_in_pass`] for `nodes[0]`, so an implementation
    /// may encode a prefix — the leading items that share one scissor, say —
    /// and let the painter offer the rest.
    ///
    /// This is not a frame-end batch. The run is a slice of the display list:
    /// any quad, glyph, icon, host texture, backdrop or group boundary between
    /// two nodes ends it, so a node can never be drawn across ordinary UI.
    /// Document order is exactly what it would be without batching; only the
    /// number of draws changes.
    ///
    /// Implementations restore the viewport to `context.dest_size` just as
    /// [`Self::draw_in_pass`] must, and record one draw batch and one draw call
    /// per draw they issue.
    fn draw_batch_in_pass(
        &self,
        _nodes: &[SceneGpuBatchNode<'_>],
        _pass: &mut ScenePass<'_, '_>,
        _context: SceneGpuBatchPassContext<'_>,
    ) -> usize {
        0
    }
}

/// One preparation pass of a [`SceneResourceProducer`], recorded into the
/// host's frame.
pub struct SceneResourceEncodeContext<'a> {
    pub gpu: &'a GpuContext,
    frame: &'a mut FrameContext,
}

impl SceneResourceEncodeContext<'_> {
    /// The host frame this pass records into. Never submit it: the host does,
    /// together with the UI paint.
    pub fn frame(&mut self) -> &mut FrameContext {
        self.frame
    }
}

/// Advanced graph-scheduled offscreen on the HostTexture path.
/// Prefer `prepare_window_frame`. Visible frames encode on the Surface encoder
/// and submit with UI paint; hidden ticks encode without a Surface and submit
/// immediately. [`Self::submitted`] means this encode was queued, not that a
/// UI frame sampling it has presented.
pub trait SceneResourceProducer: fmt::Debug + Send + Sync + 'static {
    /// Encode one preparation pass. Returning an error drops this pass without
    /// submission; implementations must not retain a pending submission token
    /// until all fallible encoding work has succeeded.
    fn encode(
        &self,
        node: &CustomRenderNode,
        context: SceneResourceEncodeContext<'_>,
    ) -> Result<(), String>;

    fn submitted(&self, _node: &CustomRenderNode, _submission: &GpuSubmission) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneResourceProduceError {
    pub resource: Arc<str>,
    pub message: String,
}

impl fmt::Display for SceneResourceProduceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "scene resource producer `{}` failed: {}",
            self.resource, self.message
        )
    }
}

impl std::error::Error for SceneResourceProduceError {}

#[derive(Debug, Clone, Default)]
pub struct SceneResourceProducerRegistry {
    producers: HashMap<Arc<str>, Arc<dyn SceneResourceProducer>>,
}

impl SceneResourceProducerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        resource: impl Into<Arc<str>>,
        producer: Arc<dyn SceneResourceProducer>,
    ) -> Option<Arc<dyn SceneResourceProducer>> {
        self.producers.insert(resource.into(), producer)
    }

    pub fn get(&self, resource: &str) -> Option<Arc<dyn SceneResourceProducer>> {
        self.producers.get(resource).cloned()
    }

    /// Encode preparation into the host frame. No submission happens here.
    /// Drop the frame if this returns an error.
    pub fn encode_scene(
        &self,
        scene: &UiScene,
        frame: &mut FrameContext,
    ) -> Result<PreparedSceneResources, SceneResourceProduceError> {
        let gpu = frame.gpu().clone();
        let plan = scene
            .frame_plan()
            .map_err(|error| SceneResourceProduceError {
                resource: Arc::from("render-graph"),
                message: error.to_string(),
            })?;
        let mut prepared = PreparedSceneResources::default();
        for id in plan.preparations.iter() {
            let Some(primitive) = scene.primitive(*id) else {
                continue;
            };
            let ScenePrimitiveKind::Custom { node, .. } = &primitive.kind else {
                continue;
            };
            let Some(producer) = self.producers.get(&node.resource) else {
                continue;
            };
            producer
                .encode(
                    node,
                    SceneResourceEncodeContext {
                        gpu: &gpu,
                        frame: &mut *frame,
                    },
                )
                .map_err(|message| SceneResourceProduceError {
                    resource: node.resource.clone(),
                    message,
                })?;
            prepared.nodes.push((node.clone(), Arc::clone(producer)));
        }
        Ok(prepared)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SceneGpuRendererRegistry {
    renderers: HashMap<Arc<str>, Arc<dyn SceneGpuRenderer>>,
}

impl SceneGpuRendererRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        name: impl Into<Arc<str>>,
        renderer: Arc<dyn SceneGpuRenderer>,
    ) -> Option<Arc<dyn SceneGpuRenderer>> {
        self.renderers.insert(name.into(), renderer)
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn SceneGpuRenderer>> {
        self.renderers.get(name).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.renderers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct NoopRenderer;

    impl SceneGpuRenderer for NoopRenderer {
        fn prepare(&self, _node: &SceneGpuNode, _context: SceneGpuPrepareContext<'_>) {}

        fn render(&self, _node: &SceneGpuNode, _context: SceneGpuRenderContext<'_>) {}
    }

    #[test]
    fn registry_replaces_renderer_by_stable_name() {
        let mut registry = SceneGpuRendererRegistry::new();
        assert!(registry.insert("live2d", Arc::new(NoopRenderer)).is_none());
        assert!(registry.get("live2d").is_some());
        assert!(registry.insert("live2d", Arc::new(NoopRenderer)).is_some());
    }
}

/// Submission callbacks for successfully encoded external resources. Dropping
/// this value without `submitted` does not publish successful production.
#[derive(Default)]
pub struct PreparedSceneResources {
    nodes: Vec<(CustomRenderNode, Arc<dyn SceneResourceProducer>)>,
}

impl PreparedSceneResources {
    /// The host queued this encode. Hidden ticks call this without presenting.
    pub fn submitted(self, submission: &GpuSubmission) {
        for (node, producer) in self.nodes {
            producer.submitted(&node, submission);
        }
    }
}

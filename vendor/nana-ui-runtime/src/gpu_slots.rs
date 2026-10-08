//! Backend-neutral GPU content slots.
//!
//! [`GpuTextureView`] is the product default (`"nana.host-texture"`).
//! [`GpuView`] encodes in-pass when there is no intermediate texture.
//! Device, Queue, Surface, and WGPU objects stay host-owned.

use std::sync::Arc;

use nana_ui_core::LengthSpec;

#[cfg(test)]
use crate::LayoutBox;
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, CustomRenderNode, InteractionState,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, UiWorld,
    component_registry::{RegisterableComponent, SemanticSpec},
};

/// Scene HostTexture registry renderer (`resource` is the host slot string).
///
/// Shared by [`GpuTextureView`], [`crate::Video`], [`crate::Thumbnail`], and
/// [`crate::ImageViewer`] HostTexture content. Not a second painter.
pub const HOST_TEXTURE_RENDERER: &str = "nana.host-texture";
/// Scene renderer key for [`GpuView`].
///
/// [`GpuView::project`] attaches [`GpuView::custom_render`]. Hosts that
/// return `None` from scene GPU renderers install a `"gpu-view"` painter;
/// an explicit empty registry still leaves the node unpaintable.
pub const GPU_VIEW_RENDERER: &str = "gpu-view";
/// Alias of [`HOST_TEXTURE_RENDERER`]. Not a distinct Scene painter; the
/// string is `"nana.host-texture"`, never `"gpu-texture-view"`.
pub const GPU_TEXTURE_VIEW_RENDERER: &str = HOST_TEXTURE_RENDERER;

/// Packs identity and content counters for [`CustomRenderNode::revision`].
///
/// `generation` occupies the high 32 bits and changes when the host replaces
/// the view. `version` occupies the low 32 bits and changes on content
/// invalidation. The halves are independent.
pub const fn pack_gpu_revision(generation: u64, version: u64) -> u64 {
    ((generation & 0xFFFF_FFFF) << 32) | (version & 0xFFFF_FFFF)
}

/// Splits a packed [`CustomRenderNode::revision`] into `(generation, version)`.
pub const fn unpack_gpu_revision(revision: u64) -> (u64, u64) {
    (revision >> 32, revision & 0xFFFF_FFFF)
}

/// Selects how a [`GpuView`] should contribute commands to the current frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum GpuViewMode {
    /// Reuse the host painter's current render pass.
    #[default]
    Inline,
    /// Open a dedicated pass on the same encoder and target.
    Standalone,
}

/// Retained palette for the host painter. Runtime does not shade these colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuViewPalette {
    pub background: [f32; 4],
    pub accent: [f32; 4],
}

impl Default for GpuViewPalette {
    fn default() -> Self {
        Self {
            background: [0.0, 0.0, 0.0, 1.0],
            accent: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// Layout of [`CustomRenderNode::params`] for [`GPU_VIEW_RENDERER`].
///
/// Runtime writes these slots and never reads them back; the `"gpu-view"`
/// painter is the only consumer. Any other renderer defines its own layout.
pub mod gpu_view_params {
    /// `background` RGBA occupies `0..4`.
    pub const BACKGROUND: usize = 0;
    /// `accent` RGBA occupies `4..8`.
    pub const ACCENT: usize = 4;
    /// Host-painter seed.
    pub const SEED: usize = 8;
    /// Total slot count written by [`super::GpuView::custom_render`].
    pub const LEN: usize = 9;
}

/// In-pass GPU content identified by a stable host `slot_id`.
///
/// Prefer [`GpuTextureView`] when the host already has a sampleable texture.
/// Paint requires a host-registered Scene GPU renderer for [`GPU_VIEW_RENDERER`].
/// [`Self::slot_id`] is **not** a [`HOST_TEXTURE_RENDERER`] registry key; the
/// Scene `resource` is the decimal form of this integer.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuView {
    pub slot_id: u64,
    pub mode: GpuViewMode,
    pub generation: u64,
    pub version: u64,
    pub palette: GpuViewPalette,
    /// Host-painter seed. Not mixed into revision; bump [`Self::version`] to
    /// request a redraw.
    pub seed: f32,
    pub style: NodeStyle,
}

impl GpuView {
    pub fn new(slot_id: u64) -> Self {
        Self {
            slot_id,
            mode: GpuViewMode::Inline,
            generation: 0,
            version: 0,
            palette: GpuViewPalette::default(),
            seed: 0.0,
            style: NodeStyle::default(),
        }
    }

    pub const fn mode(mut self, mode: GpuViewMode) -> Self {
        self.mode = mode;
        self
    }

    pub const fn palette(mut self, palette: GpuViewPalette) -> Self {
        self.palette = palette;
        self
    }

    pub fn seed(mut self, seed: f32) -> Self {
        self.seed = finite_seed(seed);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    /// Marks the current view as containing new host-rendered content.
    pub fn invalidate_content(&mut self) -> u64 {
        self.version = self.version.saturating_add(1);
        self.version
    }

    /// Records that the host replaced the underlying view. Does not touch
    /// [`Self::version`].
    pub fn replace_view(&mut self, generation: u64) -> u64 {
        self.generation = generation;
        self.generation
    }

    pub const fn revision(&self) -> u64 {
        pack_gpu_revision(self.generation, self.version)
    }

    /// Backend-neutral custom node for hosts that register a Scene GPU
    /// renderer for [`GPU_VIEW_RENDERER`]. [`ComponentView::project`] attaches
    /// this node, so the Scene painter fails until that renderer is registered.
    ///
    /// `resource` is the decimal [`Self::slot_id`]. [`Self::palette`] and
    /// [`Self::seed`] travel in `params` under [`gpu_view_params`], and
    /// [`GpuViewMode::Standalone`] becomes `dedicated_pass`.
    pub fn custom_render(&self) -> CustomRenderNode {
        CustomRenderNode::new(
            GPU_VIEW_RENDERER,
            resource_key(self.slot_id),
            self.revision(),
        )
        .with_params(
            self.palette
                .background
                .into_iter()
                .chain(self.palette.accent)
                .chain([self.seed]),
        )
        .with_dedicated_pass(self.mode == GpuViewMode::Standalone)
    }

    fn effective_style(&self) -> NodeStyle {
        fill_layout(&self.style)
    }
}

/// Displays a host-owned texture inside a layout region.
///
/// Product default. `resource` is the host texture registry slot (the same
/// string registered for `"nana.host-texture"`). Decimal IDs such as `"42"`
/// are valid if the host registers that key. `generation` changes only when the
/// sampled view is replaced. `version` changes for every content invalidation.
/// Presentation fields stay on this view so CustomRenderNode can keep a
/// backend-neutral id/revision contract. Pointer events default to off;
/// [`Self::with_pointer_events`] opts a specific instance into hit-testing.
///
/// Not a page loader. Runtime [`crate::BrowserView`] is the separate native
/// browser exception: it anchors host-owned content to the retained tree and
/// is not a `GpuTextureView` alias. See `docs/reference/window.md` and `docs/reference/gpu.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuTextureView {
    pub resource: Arc<str>,
    pub generation: u64,
    pub version: u64,
    pub opacity: f32,
    pub corner_radius: f32,
    pub fit: nana_ui_core::ContentFit,
    pub style: NodeStyle,
    pub pointer_events: bool,
    /// 在纹理下方绘制 alpha 棋盘底，凸显透明区域。
    pub checkerboard: bool,
    /// 纹理缩放系数（1.0 为适配大小，>1 放大居中）。
    pub zoom: f32,
    /// 采样方式。默认单层双线性，宿主按 `painted_extent` 准备纹理尺寸；
    /// [`nana_ui_core::ImageSampling::Mipmap`] 对宿主提供的 mip 链做三线性采样。
    pub sampling: nana_ui_core::ImageSampling,
}

impl GpuTextureView {
    /// Maps a point in the view's logical coordinate space to the sampled texture.
    /// Uses the same content fit and centered zoom as host-texture rendering;
    /// letterbox bars and points outside the view have no texture coordinate.
    pub fn texture_uv_at(
        &self,
        bounds: nana_ui_core::LogicalRect,
        texture_size: [u32; 2],
        point: nana_ui_core::LogicalPoint,
    ) -> Option<[f32; 2]> {
        if texture_size.contains(&0)
            || !point.x.is_finite()
            || !point.y.is_finite()
            || bounds.width <= 0.0
            || bounds.height <= 0.0
            || point.x < bounds.x
            || point.y < bounds.y
            || point.x >= bounds.x + bounds.width
            || point.y >= bounds.y + bounds.height
        {
            return None;
        }
        let fitted = bounds.fitted(texture_size[0] as f32, texture_size[1] as f32, self.fit);
        let uv = [
            (point.x - fitted.x) / fitted.width,
            (point.y - fitted.y) / fitted.height,
        ];
        if uv.iter().any(|v| !v.is_finite() || !(0.0..1.0).contains(v)) {
            return None;
        }
        let zoom = if self.zoom.is_finite() {
            self.zoom.max(1.0)
        } else {
            1.0
        };
        Some(uv.map(|v| (v - 0.5) / zoom + 0.5))
    }

    pub fn new(resource: impl Into<Arc<str>>) -> Self {
        Self {
            resource: resource.into(),
            generation: 0,
            version: 0,
            opacity: 1.0,
            corner_radius: 0.0,
            fit: nana_ui_core::ContentFit::Fill,
            style: NodeStyle::default(),
            pointer_events: false,
            checkerboard: false,
            zoom: 1.0,
            sampling: nana_ui_core::ImageSampling::Resample,
        }
    }

    /// 在纹理下方绘制 alpha 棋盘底。
    pub const fn checkerboard(mut self, checkerboard: bool) -> Self {
        self.checkerboard = checkerboard;
        self
    }

    /// 选择采样方式。纹理带 mip 链时用 [`nana_ui_core::ImageSampling::Mipmap`]
    /// 做三线性采样；默认只采样第 0 层。
    pub const fn sampling(mut self, sampling: nana_ui_core::ImageSampling) -> Self {
        self.sampling = sampling;
        self
    }

    /// 设置纹理缩放系数（1.0 为适配大小，非有限值按 1.0 处理）。
    pub fn zoom(mut self, zoom: f32) -> Self {
        self.zoom = if zoom.is_finite() && zoom >= 1.0 {
            zoom
        } else {
            1.0
        };
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = finite_opacity(opacity);
        self
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = finite_radius(radius);
        self
    }

    /// Per-corner rounded clip (TL, TR, BR, BL); overrides
    /// [`Self::with_corner_radius`]. The texture is clipped by the same
    /// shape as the node's own surface, e.g. rounded top corners only.
    pub fn with_corner_radii(mut self, radii: [f32; 4]) -> Self {
        Arc::make_mut(&mut self.style.layout).paint.border_radii =
            Some(radii.map(|radius| nana_ui_core::LengthSpec::Px(finite_radius(radius))));
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    /// Opts this instance into hit-testing without changing the product default.
    pub const fn with_pointer_events(mut self, pointer_events: bool) -> Self {
        self.pointer_events = pointer_events;
        self
    }

    /// Selects how the host texture maps into the layout box. Default is fill.
    pub const fn fit(mut self, fit: nana_ui_core::ContentFit) -> Self {
        self.fit = fit;
        self
    }

    /// Letterbox the registered host texture inside the layout box.
    pub const fn contain(self) -> Self {
        self.fit(nana_ui_core::ContentFit::Contain)
    }

    /// Marks the current view as containing new host-rendered content.
    pub fn invalidate_content(&mut self) -> u64 {
        self.version = self.version.saturating_add(1);
        self.version
    }

    /// Records that the host replaced the sampled view. Does not touch
    /// [`Self::version`].
    pub fn replace_view(&mut self, generation: u64) -> u64 {
        self.generation = generation;
        self.generation
    }

    pub const fn revision(&self) -> u64 {
        pack_gpu_revision(self.generation, self.version)
    }

    /// Host-texture Scene node. Empty identities are omitted so the world
    /// does not reject a blank `resource`.
    pub fn custom_render(&self) -> Option<CustomRenderNode> {
        if self.resource.trim().is_empty() {
            return None;
        }
        Some(
            CustomRenderNode::new(
                GPU_TEXTURE_VIEW_RENDERER,
                Arc::clone(&self.resource),
                self.revision(),
            )
            .with_fit(self.fit)
            .with_checkerboard(self.checkerboard)
            .with_zoom(self.zoom)
            .with_sampling(self.sampling),
        )
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = fill_layout(&self.style);
        let layout = Arc::make_mut(&mut style.layout);
        layout.opacity = Some(finite_opacity(self.opacity));
        layout.border_radius = Some(finite_radius(self.corner_radius));
        style
    }
}

impl ComponentView for GpuView {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: GPU_VIEW_RENDERER.into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let custom = Some(self.custom_render());
        if world.custom_render(id) != custom.as_ref() {
            mutations.set_custom_render(id, custom);
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: true,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Image,
                ..AccessibilityState::default()
            },
        );
    }
}

impl ComponentView for GpuTextureView {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: GPU_TEXTURE_VIEW_RENDERER.into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let custom = self.custom_render();
        if world.custom_render(id) != custom.as_ref() {
            mutations.set_custom_render(id, custom);
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: self.pointer_events,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Image,
                ..AccessibilityState::default()
            },
        );
    }
}

fn resource_key(id: u64) -> Arc<str> {
    Arc::from(id.to_string())
}

pub(crate) fn fill_layout(style: &NodeStyle) -> NodeStyle {
    let mut style = style.clone();
    let layout = Arc::make_mut(&mut style.layout);
    if layout.width.is_none() {
        layout.width = Some(LengthSpec::Fill);
    }
    if layout.height.is_none() {
        layout.height = Some(LengthSpec::Fill);
    }
    style
}

impl RegisterableComponent for GpuTextureView {
    const TYPE_ID: &'static str = "nana.gpu";
    const TAGS: &'static [&'static str] = &["gpu"];
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let slot = spec
            .attr("data-nana-gpu")
            .or_else(|| spec.attr("source"))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| spec.display_label());
        let mut view = GpuTextureView::new(slot.trim());
        view.style.layout = Arc::clone(spec.layout);
        if let Some(opacity) = spec.layout.opacity {
            view.opacity = finite_opacity(opacity);
        }
        if let Some(radius) = spec.layout.border_radius {
            view.corner_radius = finite_radius(radius);
        }
        if let Some(fit) = spec.attr("fit") {
            view.fit = parse_content_fit(fit);
        }
        if spec.attr("pointer-events").is_some_and(|value| {
            let value = value.trim();
            value.is_empty()
                || value.eq_ignore_ascii_case("auto")
                || value.eq_ignore_ascii_case("true")
        }) {
            view.pointer_events = true;
        }
        if let Some(generation) = spec.attr("generation").and_then(|raw| raw.parse().ok()) {
            view.replace_view(generation);
        }
        if let Some(version) = spec.attr("version").and_then(|raw| raw.parse().ok()) {
            view.version = version;
        }
        view
    }
}

impl RegisterableComponent for GpuView {
    const TYPE_ID: &'static str = "nana.gpu-view";
    const TAGS: &'static [&'static str] = &["gpu-view"];
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let slot = spec
            .attr("slot")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(spec.value)
            .trim()
            .parse()
            .unwrap_or(0);
        let mut view = GpuView::new(slot);
        view.style.layout = Arc::clone(spec.layout);
        if spec
            .attr("mode")
            .is_some_and(|value| value.eq_ignore_ascii_case("standalone"))
        {
            view.mode = GpuViewMode::Standalone;
        }
        if spec.number.is_finite() && spec.number != 0.0 {
            view.seed = finite_seed(spec.number);
        }
        if let Some(seed) = spec.attr("seed").and_then(|raw| raw.parse().ok()) {
            view.seed = finite_seed(seed);
        }
        view
    }
}

pub(crate) fn parse_content_fit(raw: &str) -> nana_ui_core::ContentFit {
    match raw.trim().to_ascii_lowercase().as_str() {
        "contain" => nana_ui_core::ContentFit::Contain,
        _ => nana_ui_core::ContentFit::Fill,
    }
}

pub(crate) fn finite_opacity(opacity: f32) -> f32 {
    if opacity.is_finite() {
        opacity.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

pub(crate) fn finite_radius(radius: f32) -> f32 {
    if radius.is_finite() {
        radius.max(0.0)
    } else {
        0.0
    }
}

fn finite_seed(seed: f32) -> f32 {
    if seed.is_finite() { seed } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentId, UiWorld};

    fn layout(x: f32, y: f32, width: f32, height: f32) -> LayoutBox {
        LayoutBox {
            x,
            y,
            width,
            height,
        }
    }

    fn mount<C: ComponentView>(component: C) -> (UiWorld, StableNodeId) {
        let mut world = UiWorld::new();
        let id = StableNodeId::new(1).unwrap();
        let document = DocumentId::new(1).unwrap();
        let mut queue = MutationQueue::new();
        queue.create(id, document, component.node_kind());
        component.project(id, &world, &mut queue);
        world.commit(queue).unwrap();
        (world, id)
    }

    #[test]
    fn texture_uv_mapping_matches_fit_zoom_and_rejects_bars_and_nonfinite_points() {
        use nana_ui_core::{LogicalPoint, LogicalRect};
        let bounds = LogicalRect::new(10.0, 20.0, 200.0, 200.0);
        let view = GpuTextureView::new("preview").contain();
        assert_eq!(
            view.texture_uv_at(bounds, [400, 200], LogicalPoint::new(110.0, 120.0)),
            Some([0.5, 0.5])
        );
        assert_eq!(
            view.texture_uv_at(bounds, [400, 200], LogicalPoint::new(60.0, 95.0)),
            Some([0.25, 0.25])
        );
        assert!(
            view.texture_uv_at(bounds, [400, 200], LogicalPoint::new(110.0, 30.0))
                .is_none()
        );
        assert_eq!(
            view.clone()
                .zoom(2.0)
                .texture_uv_at(bounds, [400, 200], LogicalPoint::new(60.0, 95.0)),
            Some([0.375, 0.375])
        );
        assert!(
            view.texture_uv_at(bounds, [0, 200], LogicalPoint::new(110.0, 120.0))
                .is_none()
        );
        assert!(
            view.texture_uv_at(bounds, [400, 200], LogicalPoint::new(f32::NAN, 120.0))
                .is_none()
        );
        assert!(
            view.texture_uv_at(bounds, [400, 200], LogicalPoint::new(210.0, 120.0))
                .is_none()
        );
        let fill = GpuTextureView::new("preview");
        assert_eq!(
            fill.texture_uv_at(bounds, [400, 200], LogicalPoint::new(60.0, 70.0)),
            Some([0.25, 0.25])
        );
    }

    #[test]
    fn generation_and_version_bumps_are_independent() {
        let mut texture = GpuTextureView::new("preview");
        let mut view = GpuView::new(11).mode(GpuViewMode::Standalone);
        assert_eq!(texture.generation, 0);
        assert_eq!(texture.version, 0);
        assert_eq!(view.generation, 0);
        assert_eq!(view.version, 0);

        assert_eq!(texture.invalidate_content(), 1);
        assert_eq!(view.invalidate_content(), 1);
        assert_eq!(texture.generation, 0);
        assert_eq!(view.generation, 0);
        assert_eq!(texture.version, 1);
        assert_eq!(view.version, 1);

        assert_eq!(texture.replace_view(4), 4);
        assert_eq!(view.replace_view(4), 4);
        assert_eq!(texture.generation, 4);
        assert_eq!(view.generation, 4);
        assert_eq!(texture.version, 1);
        assert_eq!(view.version, 1);

        assert_eq!(texture.invalidate_content(), 2);
        assert_eq!(texture.generation, 4);
        assert_eq!(texture.version, 2);
    }

    #[test]
    fn pack_gpu_revision_keeps_generation_and_version_independent() {
        assert_eq!(pack_gpu_revision(0, 0), 0);
        assert_eq!(pack_gpu_revision(1, 0), 1 << 32);
        assert_eq!(pack_gpu_revision(0, 1), 1);
        assert_eq!(unpack_gpu_revision(pack_gpu_revision(7, 9)), (7, 9));
        assert_eq!(
            pack_gpu_revision(3, 2).wrapping_add(1),
            pack_gpu_revision(3, 3)
        );
        assert_eq!(
            pack_gpu_revision(3, 2).wrapping_add(1 << 32),
            pack_gpu_revision(4, 2)
        );
        assert_eq!(pack_gpu_revision(1 << 32, 1 << 32), 0);
        assert_eq!(
            unpack_gpu_revision(pack_gpu_revision(u64::MAX, u64::MAX)),
            (0xFFFF_FFFF, 0xFFFF_FFFF)
        );
    }

    #[test]
    fn texture_view_uses_host_texture_renderer_and_registry_resource() {
        let mut texture = GpuTextureView::new("preview-slot")
            .with_opacity(0.5)
            .with_corner_radius(8.0);
        texture.replace_view(3);
        texture.invalidate_content();
        texture.invalidate_content();

        let node = texture.custom_render().expect("non-empty host slot");
        assert_eq!(node.renderer.as_ref(), "nana.host-texture");
        assert_eq!(node.renderer.as_ref(), GPU_TEXTURE_VIEW_RENDERER);
        assert_eq!(node.renderer.as_ref(), crate::HOST_TEXTURE_RENDERER);
        assert_ne!(node.renderer.as_ref(), "gpu-texture-view");
        assert_eq!(node.resource.as_ref(), "preview-slot");
        assert_eq!(node.revision, pack_gpu_revision(3, 2));
        assert_eq!(unpack_gpu_revision(node.revision), (3, 2));
        assert_eq!(node.revision, (3 << 32) | 2);
        assert_eq!(node.fit, nana_ui_core::ContentFit::Fill);
        assert_eq!(
            GpuTextureView::new("preview-slot")
                .contain()
                .custom_render()
                .unwrap()
                .fit,
            nana_ui_core::ContentFit::Contain
        );

        let decimal = GpuTextureView::new("42").custom_render().unwrap();
        assert_eq!(decimal.renderer.as_ref(), "nana.host-texture");
        assert_eq!(decimal.resource.as_ref(), "42");
        assert_eq!(decimal.revision, pack_gpu_revision(0, 0));

        assert!(GpuTextureView::new("").custom_render().is_none());
        assert!(GpuTextureView::new("   ").custom_render().is_none());
        let (world, id) = mount(GpuTextureView::new(""));
        assert!(world.custom_render(id).is_none());
    }

    #[test]
    fn custom_render_node_encodes_packed_revision() {
        let mut view = GpuView::new(9)
            .mode(GpuViewMode::Standalone)
            .palette(GpuViewPalette {
                background: [0.1, 0.2, 0.3, 1.0],
                accent: [0.9, 0.8, 0.7, 1.0],
            })
            .seed(0.34);
        view.replace_view(5);
        view.invalidate_content();
        let node = view.custom_render();
        assert_eq!(node.renderer.as_ref(), GPU_VIEW_RENDERER);
        assert_eq!(node.renderer.as_ref(), "gpu-view");
        assert_eq!(node.resource.as_ref(), "9");
        assert_eq!(node.revision, pack_gpu_revision(5, 1));
        assert_eq!(unpack_gpu_revision(node.revision), (5, 1));
    }

    #[test]
    fn palette_and_seed_reach_the_custom_render_params() {
        let view = GpuView::new(3)
            .palette(GpuViewPalette {
                background: [0.1, 0.2, 0.3, 1.0],
                accent: [0.9, 0.8, 0.7, 0.6],
            })
            .seed(0.34);
        let node = view.custom_render();
        let params = node.params.as_ref().expect("gpu-view encodes params");
        assert_eq!(params.len(), gpu_view_params::LEN);
        assert_eq!(
            &params[gpu_view_params::BACKGROUND..gpu_view_params::BACKGROUND + 4],
            &[0.1, 0.2, 0.3, 1.0]
        );
        assert_eq!(
            &params[gpu_view_params::ACCENT..gpu_view_params::ACCENT + 4],
            &[0.9, 0.8, 0.7, 0.6]
        );
        assert_eq!(node.param(gpu_view_params::SEED), Some(0.34));

        // A directly assigned non-finite seed must not reach a uniform.
        let mut unsanitized = GpuView::new(3);
        unsanitized.seed = f32::NAN;
        assert_eq!(
            unsanitized.custom_render().param(gpu_view_params::SEED),
            Some(0.0)
        );
    }

    #[test]
    fn standalone_mode_requests_a_dedicated_pass() {
        assert!(!GpuView::new(1).custom_render().dedicated_pass);
        assert!(
            !GpuView::new(1)
                .mode(GpuViewMode::Inline)
                .custom_render()
                .dedicated_pass
        );
        assert!(
            GpuView::new(1)
                .mode(GpuViewMode::Standalone)
                .custom_render()
                .dedicated_pass
        );

        let (world, id) = mount(GpuView::new(4).mode(GpuViewMode::Standalone));
        assert!(
            world
                .custom_render(id)
                .expect("projected node")
                .dedicated_pass
        );
    }

    #[test]
    fn palette_changes_reproject_the_custom_render_node() {
        let view = GpuView::new(5);
        let (world, id) = mount(view.clone());
        let mut idle = MutationQueue::new();
        view.project(id, &world, &mut idle);
        assert!(idle.is_empty(), "unchanged palette must not remutate");

        let recolored = GpuView::new(5).palette(GpuViewPalette {
            background: [1.0, 0.0, 0.0, 1.0],
            accent: [0.0, 1.0, 0.0, 1.0],
        });
        let mut changed = MutationQueue::new();
        recolored.project(id, &world, &mut changed);
        assert!(
            !changed.is_empty(),
            "a palette change must reach the retained node"
        );
    }

    #[test]
    fn gpu_view_mode_is_retained_across_identity_updates() {
        let mut view = GpuView::new(7).mode(GpuViewMode::Standalone);
        assert_eq!(view.mode, GpuViewMode::Standalone);
        view.invalidate_content();
        view.replace_view(2);
        view.seed = 1.25;
        view.palette.accent = [0.2, 0.4, 0.6, 1.0];
        assert_eq!(view.mode, GpuViewMode::Standalone);
        assert_eq!(view.slot_id, 7);
        assert_eq!(view.generation, 2);
        assert_eq!(view.version, 1);

        let (world, id) = mount(view.clone());
        assert_eq!(world.custom_render(id), Some(&view.custom_render()));
        assert_eq!(view.custom_render().revision, pack_gpu_revision(2, 1));
        assert_eq!(view.custom_render().renderer.as_ref(), GPU_VIEW_RENDERER);
    }

    #[test]
    fn gpu_view_project_attaches_custom_render() {
        let view = GpuView::new(1);
        let (world, id) = mount(view.clone());
        let node = world
            .custom_render(id)
            .expect("GpuView project attaches custom_render");
        assert_eq!(node.renderer.as_ref(), "gpu-view");
        assert_eq!(node.resource.as_ref(), "1");
        assert_eq!(node.revision, pack_gpu_revision(0, 0));
        assert_eq!(*node, view.custom_render());
    }

    #[test]
    fn component_view_projects_host_texture_node_and_gpu_view_layout() {
        let texture = GpuTextureView::new("preview")
            .with_opacity(0.25)
            .with_corner_radius(6.0);
        let (world, id) = mount(texture.clone());
        assert!(matches!(
            world.node(id).unwrap().kind,
            NodeKind::Element { tag } if tag == GPU_TEXTURE_VIEW_RENDERER
        ));
        assert_eq!(world.custom_render(id), texture.custom_render().as_ref());
        assert_eq!(
            world.custom_render(id).map(|node| node.renderer.as_ref()),
            Some("nana.host-texture")
        );
        let style = world.node_style(id).unwrap();
        assert_eq!(style.layout.width, Some(LengthSpec::Fill));
        assert_eq!(style.layout.height, Some(LengthSpec::Fill));
        assert_eq!(style.layout.opacity, Some(0.25));
        assert_eq!(style.layout.border_radius, Some(6.0));
        assert_eq!(
            world.interaction(id),
            Some(InteractionState {
                pointer_events: false,
                focusable: false,
            })
        );

        let view = GpuView::new(8).mode(GpuViewMode::Inline);
        let (world, id) = mount(view.clone());
        assert!(matches!(
            world.node(id).unwrap().kind,
            NodeKind::Element { tag } if tag == GPU_VIEW_RENDERER
        ));
        assert_eq!(world.custom_render(id), Some(&view.custom_render()));
        assert_eq!(
            world.custom_render(id).map(|node| node.renderer.as_ref()),
            Some(GPU_VIEW_RENDERER)
        );
        assert_eq!(
            world.custom_render(id).map(|node| node.resource.as_ref()),
            Some("8")
        );
        assert_eq!(
            world.node_style(id).unwrap().layout.width,
            Some(LengthSpec::Fill)
        );
        assert_eq!(
            world.interaction(id),
            Some(InteractionState {
                pointer_events: true,
                focusable: false,
            })
        );
        let mut idle = MutationQueue::new();
        view.project(id, &world, &mut idle);
        assert!(idle.is_empty());
    }

    #[test]
    fn overlay_button_hits_above_hittable_host_texture_not_because_gpu_ignores_pointers() {
        let mut world = UiWorld::new();
        let document = DocumentId::new(1).unwrap();
        let root = StableNodeId::new(1).unwrap();
        let passthrough = StableNodeId::new(2).unwrap();
        let gpu = StableNodeId::new(3).unwrap();
        let button = StableNodeId::new(4).unwrap();

        let pass_view = GpuTextureView::new("live2d.bg");
        let gpu_view = GpuTextureView::new("live2d.model").with_pointer_events(true);
        let chrome = crate::Button::new("Start");
        assert!(
            !pass_view.pointer_events,
            "GpuTextureView stays pointer-transparent by default"
        );
        assert!(gpu_view.pointer_events);

        let mut queue = MutationQueue::new();
        queue.create(root, document, NodeKind::Element { tag: "root".into() });
        queue.create(passthrough, document, pass_view.node_kind());
        queue.create(gpu, document, gpu_view.node_kind());
        queue.create(button, document, chrome.node_kind());
        pass_view.project(passthrough, &world, &mut queue);
        gpu_view.project(gpu, &world, &mut queue);
        chrome.project(button, &world, &mut queue);
        queue.set_interaction(
            root,
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
        );
        queue.insert(root, passthrough, None);
        queue.insert(root, gpu, None);
        queue.insert(root, button, None);
        queue.write_layout(root, layout(0.0, 0.0, 200.0, 100.0));
        queue.write_layout(passthrough, layout(0.0, 0.0, 100.0, 100.0));
        queue.write_layout(gpu, layout(100.0, 0.0, 100.0, 100.0));
        queue.write_layout(button, layout(60.0, 40.0, 80.0, 32.0));
        world.commit(queue).unwrap();
        world.rebuild_hit_test(document);

        assert_eq!(
            world.interaction(passthrough),
            Some(InteractionState {
                pointer_events: false,
                focusable: false,
            })
        );
        assert_eq!(
            world.interaction(gpu),
            Some(InteractionState {
                pointer_events: true,
                focusable: false,
            })
        );

        assert_eq!(world.hit_test(document, 120.0, 50.0), Some(button));
        assert_eq!(world.hit_test(document, 150.0, 10.0), Some(gpu));
        assert_eq!(world.hit_test(document, 80.0, 50.0), Some(button));
        assert_eq!(world.hit_test(document, 20.0, 10.0), None);
        let overlap_candidates = world.hit_test_candidates(document, 120.0, 50.0);
        assert_eq!(overlap_candidates.first().copied(), Some(button));
        assert!(
            overlap_candidates.contains(&gpu),
            "hittable GPU slot remains under the overlay: {overlap_candidates:?}"
        );
    }

    #[test]
    fn gpu_nodes_bind_from_semantic_slots() {
        let type_id = crate::ComponentTypeId::new("nana.gpu").unwrap();
        let layout = Arc::new(nana_ui_core::LayoutStyle::default());
        let mut spec = SemanticSpec::from_parts(&type_id, &layout);
        let attrs = [("data-nana-gpu", "preview"), ("fit", "contain")];
        spec.attrs = &attrs;
        let texture = GpuTextureView::from_semantic(&spec);
        assert_eq!(texture.resource.as_ref(), "preview");
        assert_eq!(texture.fit, nana_ui_core::ContentFit::Contain);

        let view_id = crate::ComponentTypeId::new("nana.gpu-view").unwrap();
        let mut view_spec = SemanticSpec::from_parts(&view_id, &layout);
        let view_attrs = [("slot", "9"), ("mode", "standalone")];
        view_spec.attrs = &view_attrs;
        let view = GpuView::from_semantic(&view_spec);
        assert_eq!(view.slot_id, 9);
        assert_eq!(view.mode, GpuViewMode::Standalone);
    }
}

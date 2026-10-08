//! Compact list-row image slot. NanaUI never stores pixels or codecs.
//!
//! Hosts declare a HostTexture slot and an optional aspect. The box is
//! [`ControlSize`] height × aspect (default 1:1). Empty, loading, ready, and
//! unavailable share that geometry. Ready samples `"nana.host-texture"` with
//! configurable fit (Contain by default); empty identities omit the Scene node.

use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, ContentFit, ControlHeight, ControlSize, ImageSampling, LengthSpec, OverflowSpec,
    PointerEventsSpec, PositionSpec, RadiusTier, SemanticColorRole, SquareSize, ThemeMetrics,
    space,
};

use crate::gpu_slots::pack_gpu_revision;
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, CustomRenderNode, HOST_TEXTURE_RENDERER,
    InteractionState, MutationQueue, NodeKind, NodeStyle, StableNodeId, Stack, StandardVisual,
    TextContent, UiWorld,
};

/// Width ÷ height when the host does not declare an aspect.
pub const DEFAULT_ASPECT: f32 = 1.0;

/// Loading glyph: twice the compact [`crate::Spinner`] (14 → 28). Scene clamps to the box.
const SPINNER_SIZE: f32 = nana_ui_core::UI_METRICS.icon_button_size;

/// Presentation of a [`Thumbnail`] box. All four states keep the same size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThumbnailState {
    #[default]
    Empty,
    Loading,
    Ready,
    Unavailable,
}

/// Compact image control for [`crate::ListItem`] leading slots.
///
/// Pointer events stay off so the parent row owns hit-testing. Overlay children
/// (see [`Self::badge`]) are clipped to this box's radius.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumbnail {
    pub resource: Arc<str>,
    /// Host texture generation and in-place content version. Together they form
    /// the Scene revision, so a slot filled after mount is actually re-sampled.
    pub generation: u64,
    pub version: u64,
    pub state: ThumbnailState,
    pub size: ControlSize,
    pub aspect: f32,
    pub fit: ContentFit,
    /// Level 0 by default (the host sizes the slot from `painted_extent`);
    /// [`ImageSampling::Mipmap`] samples the host's mip chain trilinearly.
    pub sampling: ImageSampling,
    pub label: Arc<str>,
    pub style: NodeStyle,
}

impl Thumbnail {
    /// Ready when `resource` is non-empty after trim; otherwise empty.
    pub fn new(resource: impl Into<Arc<str>>) -> Self {
        let resource = resource.into();
        let state = if resource.trim().is_empty() {
            ThumbnailState::Empty
        } else {
            ThumbnailState::Ready
        };
        Self {
            resource,
            generation: 0,
            version: 0,
            state,
            size: ControlSize::Small,
            aspect: DEFAULT_ASPECT,
            fit: ContentFit::Contain,
            sampling: ImageSampling::Resample,
            label: Arc::from(""),
            style: NodeStyle::default(),
        }
    }

    pub fn empty() -> Self {
        Self::new("").state(ThumbnailState::Empty)
    }

    pub fn loading() -> Self {
        Self::new("").state(ThumbnailState::Loading)
    }

    pub fn unavailable() -> Self {
        Self::new("").state(ThumbnailState::Unavailable)
    }

    pub const fn state(mut self, state: ThumbnailState) -> Self {
        self.state = state;
        self
    }

    pub fn aspect(mut self, aspect: f32) -> Self {
        self.aspect = sanitize_aspect(aspect);
        self
    }

    pub fn fit(mut self, fit: ContentFit) -> Self {
        self.fit = fit;
        self
    }

    /// How the slot's texture is sampled; see [`ImageSampling`].
    pub const fn sampling(mut self, sampling: ImageSampling) -> Self {
        self.sampling = sampling;
        self
    }

    pub const fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = label.into();
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    /// Corner overlay chrome for duration, viewer counts, and similar cover
    /// metadata. Absolute, not hittable, clipped by this thumbnail's radius.
    pub fn badge() -> Stack {
        Stack::row(0.0)
            .surface(SemanticColorRole::Background)
            .radius(nana_ui_core::RadiusTier::Sm)
            .with_layout(|layout| {
                layout.position = PositionSpec::Absolute;
                layout.offset_right = Some(LengthSpec::Px(space::SM));
                layout.offset_bottom = Some(LengthSpec::Px(space::SM));
                layout.z_index = Some(2);
                layout.padding_left = Some(LengthSpec::Px(space::SM));
                layout.padding_right = Some(LengthSpec::Px(space::SM));
                layout.padding_top = Some(LengthSpec::Px(space::XXS));
                layout.padding_bottom = Some(LengthSpec::Px(space::XXS));
                layout.align_items = AlignSpec::Center;
                layout.pointer_events = Some(PointerEventsSpec::None);
            })
    }

    /// Records that the host refreshed the slot's pixels in place.
    ///
    /// The packed [`Self::revision`] is the Scene's conflict key for a
    /// host-texture slot: every view of one slot in a frame must report the
    /// same revision, or `UiScene::frame_graph` rejects the frame with
    /// `ConflictingExternalResource` and nothing paints. A control that
    /// reported a fixed revision could therefore never share a slot with a
    /// view the host had re-bound.
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

    /// Logical box: control height × host aspect (default 1:1).
    pub fn box_extent(&self, metrics: ThemeMetrics) -> (f32, f32) {
        let height = self.size.height_in(metrics);
        (height * sanitize_aspect(self.aspect), height)
    }

    /// Host-texture Scene node. Only Ready with a non-empty slot attaches one.
    pub fn custom_render(&self) -> Option<CustomRenderNode> {
        if self.state != ThumbnailState::Ready || self.resource.trim().is_empty() {
            return None;
        }
        Some(
            CustomRenderNode::new(
                HOST_TEXTURE_RENDERER,
                Arc::clone(&self.resource),
                self.revision(),
            )
            .with_fit(self.fit)
            .with_sampling(self.sampling),
        )
    }

    fn effective_style(&self, _world: &UiWorld) -> NodeStyle {
        let mut style = self.style.clone();
        style.background = match self.state {
            ThumbnailState::Ready => None,
            _ => Some(SemanticColorRole::Subtle),
        };
        style.border = None;
        style.foreground = Some(SemanticColorRole::Muted);
        let unset_width = style.layout.width.is_none();
        let unset_height = style.layout.height.is_none();
        // A host that names a tier or a length keeps it; only an unrounded
        // thumbnail takes the compact default.
        let unset_radius = style.layout.border_radius.is_none() && style.radius.is_none();
        let aspect = sanitize_aspect(self.aspect);
        {
            let layout = Arc::make_mut(&mut style.layout);
            if unset_width {
                layout.aspect_ratio = Some(aspect);
            }
            layout.flex_grow.get_or_insert(0.0);
            layout.flex_shrink.get_or_insert(0.0);
            layout.border_width = Some(0.0);
            layout.overflow_x = OverflowSpec::Hidden;
            layout.overflow_y = OverflowSpec::Hidden;
            if !layout.position.establishes_containing_block() {
                layout.position = PositionSpec::Relative;
            }
        }
        if unset_radius {
            style.radius = Some(RadiusTier::Xs);
        }
        if unset_height {
            style.control_height = Some(ControlHeight::Exact(self.size));
        }
        if unset_width && aspect == DEFAULT_ASPECT {
            style.square = Some(SquareSize::Control(self.size));
        }
        style
    }
}

impl Default for Thumbnail {
    fn default() -> Self {
        Self::empty()
    }
}

impl ComponentView for Thumbnail {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "thumbnail".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.text(id) != Some("") {
            mutations.set_text(
                id,
                TextContent {
                    value: String::new().into(),
                },
            );
        }
        let custom = self.custom_render();
        if world.custom_render(id) != custom.as_ref() {
            mutations.set_custom_render(id, custom);
        }
        let visual = (self.state == ThumbnailState::Loading).then(|| StandardVisual::Spinner {
            label: Arc::from(""),
            size: SPINNER_SIZE.min(self.box_extent(world.theme_metrics()).1),
            phase: 0.0,
        });
        if world.standard_visual(id) != visual {
            mutations.set_standard_visual(id, visual);
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(world),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Image,
                label: (!self.label.is_empty()).then(|| Arc::clone(&self.label)),
                busy: self.state == ThumbnailState::Loading,
                invalid: self.state == ThumbnailState::Unavailable,
                ..AccessibilityState::default()
            },
        );
    }
}

fn sanitize_aspect(aspect: f32) -> f32 {
    if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        DEFAULT_ASPECT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, DocumentId, LayoutViewport, ListItem, ListItemSlots};
    use nana_ui_core::UI_METRICS;

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn mount(component: Thumbnail) -> (crate::UiWorld, StableNodeId) {
        let mut world = crate::UiWorld::new();
        let id = StableNodeId::new(1).unwrap();
        let mut queue = MutationQueue::new();
        queue.create(id, document(), component.node_kind());
        component.project(id, &world, &mut queue);
        world.commit(queue).unwrap();
        (world, id)
    }

    #[test]
    fn box_is_control_height_by_host_aspect() {
        let square = ControlSize::Small.height_in(UI_METRICS);
        assert_eq!(Thumbnail::empty().box_extent(UI_METRICS), (square, square));
        let wide = Thumbnail::empty().aspect(16.0 / 9.0).box_extent(UI_METRICS);
        assert_eq!(wide.1, square);
        assert!((wide.0 - square * 16.0 / 9.0).abs() < f32::EPSILON);
        for invalid in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                Thumbnail::empty().aspect(invalid).box_extent(UI_METRICS),
                (square, square)
            );
        }
        let mut context = AppContext::new();
        let expected = (square, square);
        for thumb in [
            Thumbnail::empty(),
            Thumbnail::loading(),
            Thumbnail::new("stage.thumb"),
            Thumbnail::unavailable(),
        ] {
            let entity = context.create_component(document(), thumb).unwrap();
            context
                .layout_document(document(), LayoutViewport::new(240.0, 80.0))
                .unwrap();
            let bounds = context.world().layout_box(entity.stable_id()).unwrap();
            assert_eq!((bounds.width, bounds.height), expected);
        }
        let wide = context
            .create_component(document(), Thumbnail::empty().aspect(16.0 / 9.0))
            .unwrap();
        context
            .layout_document(document(), LayoutViewport::new(240.0, 80.0))
            .unwrap();
        let bounds = context.world().layout_box(wide.stable_id()).unwrap();
        assert_eq!(bounds.height, square);
        assert!((bounds.width - square * 16.0 / 9.0).abs() < 0.01);
    }

    #[test]
    fn ready_samples_host_texture_contain_otherwise_omits_node() {
        let node = Thumbnail::new("stage.thumb")
            .custom_render()
            .expect("ready slot");
        assert_eq!(node.renderer.as_ref(), HOST_TEXTURE_RENDERER);
        assert_eq!(node.resource.as_ref(), "stage.thumb");
        assert_eq!(node.fit, ContentFit::Contain);
        assert!(Thumbnail::new("").custom_render().is_none());
        assert!(
            Thumbnail::new("stage.thumb")
                .state(ThumbnailState::Loading)
                .custom_render()
                .is_none()
        );
        let (world, id) = mount(Thumbnail::new("stage.thumb"));
        assert_eq!(
            world.custom_render(id).map(|node| node.fit),
            Some(ContentFit::Contain)
        );
        let (world, id) = mount(Thumbnail::loading());
        assert!(world.custom_render(id).is_none());
        assert!(matches!(
            world.standard_visual(id),
            Some(StandardVisual::Spinner { .. })
        ));
        assert!(world.accessibility(id).unwrap().busy);
        match world.standard_visual(id) {
            Some(StandardVisual::Spinner { size, label, .. }) => {
                assert_eq!(size, SPINNER_SIZE);
                assert!(label.is_empty());
            }
            other => panic!("loading thumbnail must project a spinner, got {other:?}"),
        }
        let (world, id) = mount(Thumbnail::unavailable());
        assert!(world.accessibility(id).unwrap().invalid);
        assert_eq!(
            world.interaction(id),
            Some(InteractionState {
                pointer_events: false,
                focusable: false,
            })
        );
    }

    #[test]
    fn responsive_cover_keeps_geometry_across_resource_states() {
        let mut context = AppContext::new();
        let root = context
            .create_component(
                document(),
                crate::Stack::column(0.0)
                    .width(LengthSpec::Fill)
                    .height(LengthSpec::Px(135.0)),
            )
            .unwrap();
        let mut style = NodeStyle::default();
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Fill);
        layout.border_radius = Some(10.0);
        let thumbnail = context
            .create_detached_component(
                document(),
                Thumbnail::loading().fit(ContentFit::Cover).style(style),
            )
            .unwrap();
        context.append_child(root, thumbnail).unwrap();
        for width in [240.0, 360.0] {
            for state in [
                ThumbnailState::Empty,
                ThumbnailState::Loading,
                ThumbnailState::Ready,
                ThumbnailState::Unavailable,
            ] {
                context
                    .update_component(thumbnail, |view, _| {
                        view.state = state;
                        view.resource = "cover".into();
                    })
                    .unwrap();
                context
                    .layout_document(document(), LayoutViewport::new(width, 200.0))
                    .unwrap();
                let bounds = context.world().layout_box(thumbnail.stable_id()).unwrap();
                assert_eq!((bounds.width, bounds.height), (width, 135.0));
                assert_eq!(
                    context
                        .world()
                        .custom_render(thumbnail.stable_id())
                        .map(|node| node.fit),
                    (state == ThumbnailState::Ready).then_some(ContentFit::Cover)
                );
            }
        }
    }

    #[test]
    fn list_item_leading_keeps_the_box() {
        let mut context = AppContext::new();
        let item = context
            .create_component(document(), ListItem::new("角色"))
            .unwrap();
        let leading = context
            .create_component(document(), Thumbnail::empty())
            .unwrap();
        context.append_child(item, leading).unwrap();
        context
            .set_list_item_slots(
                item,
                ListItemSlots {
                    leading: Some(leading.stable_id()),
                    content: None,
                    trailing: None,
                },
            )
            .unwrap();
        context
            .layout_document(document(), LayoutViewport::new(240.0, 80.0))
            .unwrap();
        let thumb = context.world().layout_box(leading.stable_id()).unwrap();
        assert_eq!(thumb.width, ControlSize::Small.height_in(UI_METRICS));
        assert_eq!(thumb.height, ControlSize::Small.height_in(UI_METRICS));
        assert_eq!(
            context.world().node(item.stable_id()).unwrap().children,
            vec![leading.stable_id()]
        );
    }

    /// The revision must move with the binding: it is the Scene's conflict key
    /// for a slot, so a fixed one makes this control unable to share a slot
    /// with any re-bound view. Pixel evidence for that is in
    /// `nana-ui-devtools/tests/host_texture_paint.rs`.
    /// A frame that rounds its picture with the frame's own tier must get
    /// that tier, or the picture's corners stand outside the frame's.
    #[test]
    fn host_radius_tier_is_kept() {
        let (world, id) = mount(Thumbnail::empty());
        assert_eq!(world.node_style(id).unwrap().radius, Some(RadiusTier::Xs));
        let mut style = NodeStyle::default();
        style.radius = Some(RadiusTier::Md);
        let (world, id) = mount(Thumbnail::new("stage.thumb").style(style));
        assert_eq!(world.node_style(id).unwrap().radius, Some(RadiusTier::Md));
        let mut style = NodeStyle::default();
        Arc::make_mut(&mut style.layout).border_radius = Some(6.0);
        let (world, id) = mount(Thumbnail::empty().style(style));
        let node = world.node_style(id).unwrap();
        assert_eq!(node.radius, None);
        assert_eq!(node.layout.border_radius, Some(6.0));
    }

    #[test]
    fn late_host_texture_moves_the_scene_revision() {
        let mut control = Thumbnail::new("cover:a");
        let first = control.custom_render().expect("ready slot").revision;

        control.replace_view(1);
        let replaced = control.custom_render().expect("ready slot").revision;
        assert_ne!(first, replaced, "a replaced view is a new revision");

        control.invalidate_content();
        let refreshed = control.custom_render().expect("ready slot").revision;
        assert_ne!(replaced, refreshed, "in-place pixels are a new revision");
        assert_eq!(refreshed, pack_gpu_revision(1, 1));
    }

    #[test]
    fn badge_is_absolute_and_not_hittable_inside_the_cover() {
        let mut context = AppContext::new();
        let thumbnail = context
            .create_component(
                document(),
                Thumbnail::empty().style({
                    let mut style = NodeStyle::default();
                    let layout = Arc::make_mut(&mut style.layout);
                    layout.width = Some(LengthSpec::Px(160.0));
                    layout.height = Some(LengthSpec::Px(90.0));
                    style
                }),
            )
            .unwrap();
        let badge = context
            .create_detached_component(document(), Thumbnail::badge())
            .unwrap();
        context.append_child(thumbnail, badge).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(240.0, 120.0))
            .unwrap();
        let cover = context.world().layout_box(thumbnail.stable_id()).unwrap();
        let badge_box = context.world().layout_box(badge.stable_id()).unwrap();
        let cover_style = context.world().node_style(thumbnail.stable_id()).unwrap();
        assert_eq!(cover_style.layout.position, PositionSpec::Relative);
        assert_eq!(
            context.world().interaction(badge.stable_id()),
            Some(InteractionState {
                pointer_events: false,
                focusable: false,
            })
        );
        assert!(
            badge_box.x + badge_box.width <= cover.x + cover.width + 0.5,
            "badge={badge_box:?} cover={cover:?}"
        );
        assert!(
            badge_box.y + badge_box.height <= cover.y + cover.height + 0.5,
            "badge={badge_box:?} cover={cover:?}"
        );
    }
}

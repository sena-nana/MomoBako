//! Circular cover-fit host-texture slot. Distinct from [`crate::Thumbnail`]:
//! Cover mapping, host-owned pixel size, circular clip. NanaUI never stores
//! pixels or codecs. Empty or cleared slots use the Subtle placeholder; do not
//! paint initials.

use std::sync::Arc;

use nana_ui_core::{ContentFit, ImageSampling, LengthSpec, OverflowSpec, SemanticColorRole};

use crate::gpu_slots::{HOST_TEXTURE_RENDERER, pack_gpu_revision};
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, CustomRenderNode, InteractionState,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, UiWorld,
};

/// Default edge length when the host does not declare a size.
pub const DEFAULT_SIZE: f32 = nana_ui_core::ControlSize::Medium.height_in(nana_ui_core::UI_METRICS);

/// Circular image control (`nana.avatar`).
#[derive(Debug, Clone, PartialEq)]
pub struct Avatar {
    pub resource: Arc<str>,
    /// Host texture generation and in-place content version. Together they form
    /// the Scene revision, so a slot filled after mount is actually re-sampled.
    pub generation: u64,
    pub version: u64,
    pub size: f32,
    pub label: Arc<str>,
    pub pointer_events: bool,
    /// Level 0 by default (the host sizes the slot from `painted_extent`);
    /// [`ImageSampling::Mipmap`] samples the host's mip chain trilinearly.
    pub sampling: ImageSampling,
    pub style: NodeStyle,
}

impl Avatar {
    pub fn new(resource: impl Into<Arc<str>>) -> Self {
        Self {
            resource: resource.into(),
            generation: 0,
            version: 0,
            size: DEFAULT_SIZE,
            label: Arc::from(""),
            pointer_events: false,
            sampling: ImageSampling::Resample,
            style: NodeStyle::default(),
        }
    }

    pub fn empty() -> Self {
        Self::new("")
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = sanitize_size(size);
        self
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = label.into();
        self
    }

    pub fn pointer_events(mut self, pointer_events: bool) -> Self {
        self.pointer_events = pointer_events;
        self
    }

    /// How the slot's texture is sampled; see [`ImageSampling`].
    pub const fn sampling(mut self, sampling: ImageSampling) -> Self {
        self.sampling = sampling;
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
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

    pub fn custom_render(&self) -> Option<CustomRenderNode> {
        if self.resource.trim().is_empty() {
            return None;
        }
        Some(
            CustomRenderNode::new(
                HOST_TEXTURE_RENDERER,
                Arc::clone(&self.resource),
                self.revision(),
            )
            .with_fit(ContentFit::Cover)
            .with_sampling(self.sampling),
        )
    }

    fn effective_style(&self) -> NodeStyle {
        let size = sanitize_size(self.size);
        let mut style = self.style.clone();
        if self.resource.trim().is_empty() {
            style.background = Some(SemanticColorRole::Subtle);
        } else {
            style.background = None;
        }
        style.border = None;
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(size));
        layout.height = Some(LengthSpec::Px(size));
        layout.min_width = Some(LengthSpec::Px(size));
        layout.min_height = Some(LengthSpec::Px(size));
        layout.max_width = Some(LengthSpec::Px(size));
        layout.max_height = Some(LengthSpec::Px(size));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        layout.border_width = Some(0.0);
        layout.border_radius = Some(size * 0.5);
        layout.overflow_x = OverflowSpec::Hidden;
        layout.overflow_y = OverflowSpec::Hidden;
        style
    }
}

impl Default for Avatar {
    fn default() -> Self {
        Self::empty()
    }
}

impl ComponentView for Avatar {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "avatar".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.text(id) != Some("") {
            mutations.set_text(
                id,
                crate::TextContent {
                    value: String::new().into(),
                },
            );
        }
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
                label: (!self.label.is_empty()).then(|| Arc::clone(&self.label)),
                ..AccessibilityState::default()
            },
        );
    }
}

fn sanitize_size(size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        size
    } else {
        DEFAULT_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, DocumentId, LayoutViewport};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    #[test]
    fn empty_is_circular_placeholder() {
        let mut context = AppContext::new();
        let avatar = context
            .create_component(document(), Avatar::empty().size(40.0).label("用户"))
            .unwrap();
        context
            .layout_document(document(), LayoutViewport::new(120.0, 80.0))
            .unwrap();
        let style = context.world().node_style(avatar.stable_id()).unwrap();
        assert_eq!(style.layout.border_radius, Some(20.0));
        assert_eq!(style.background, Some(SemanticColorRole::Subtle));
        assert!(context.world().custom_render(avatar.stable_id()).is_none());
        let box_ = context.world().layout_box(avatar.stable_id()).unwrap();
        assert!((box_.width - 40.0).abs() < 0.5);
        assert!((box_.height - 40.0).abs() < 0.5);
        assert_eq!(context.world().text(avatar.stable_id()), Some(""));
        let a11y = context.world().accessibility(avatar.stable_id()).unwrap();
        assert_eq!(a11y.role, AccessibilityRole::Image);
        assert_eq!(a11y.label.as_deref(), Some("用户"));
        assert!(
            !context
                .world()
                .interaction(avatar.stable_id())
                .unwrap()
                .focusable
        );
        assert!(
            !context
                .world()
                .interaction(avatar.stable_id())
                .unwrap()
                .pointer_events
        );
    }

    #[test]
    fn unlabeled_empty_is_an_unnamed_image_placeholder() {
        let mut context = AppContext::new();
        let avatar = context
            .create_component(document(), Avatar::empty())
            .unwrap();
        let a11y = context.world().accessibility(avatar.stable_id()).unwrap();
        assert_eq!(a11y.role, AccessibilityRole::Image);
        assert!(a11y.label.is_none());
        assert_eq!(context.world().text(avatar.stable_id()), Some(""));
        assert_eq!(
            context
                .world()
                .node_style(avatar.stable_id())
                .unwrap()
                .background,
            Some(SemanticColorRole::Subtle)
        );
    }

    #[test]
    fn ready_samples_host_texture_cover() {
        let avatar = Avatar::new("user.avatar");
        let render = avatar.custom_render().unwrap();
        assert_eq!(render.renderer.as_ref(), HOST_TEXTURE_RENDERER);
        assert_eq!(render.resource.as_ref(), "user.avatar");
        assert_eq!(render.fit, ContentFit::Cover);
    }

    #[test]
    fn invalid_size_falls_back_to_default() {
        for invalid in [0.0, -8.0, f32::NAN, f32::INFINITY] {
            assert_eq!(sanitize_size(invalid), DEFAULT_SIZE);
        }
    }

    /// The revision must move with the binding: it is the Scene's conflict key
    /// for a slot, so a fixed one makes this control unable to share a slot
    /// with any re-bound view. Pixel evidence for that is in
    /// `nana-ui-devtools/tests/host_texture_paint.rs`.
    #[test]
    fn late_host_texture_moves_the_scene_revision() {
        let mut control = Avatar::new("cover:a");
        let first = control.custom_render().expect("ready slot").revision;

        control.replace_view(1);
        let replaced = control.custom_render().expect("ready slot").revision;
        assert_ne!(first, replaced, "a replaced view is a new revision");

        control.invalidate_content();
        let refreshed = control.custom_render().expect("ready slot").revision;
        assert_ne!(replaced, refreshed, "in-place pixels are a new revision");
        assert_eq!(refreshed, pack_gpu_revision(1, 1));
    }
}

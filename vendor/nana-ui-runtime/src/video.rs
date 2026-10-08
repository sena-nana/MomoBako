//! Retained video playback surface.
//!
//! `<video>` samples a host-owned texture slot (`"nana.host-texture"`,
//! resource `"video:{id}"`) exactly like [`crate::GpuTextureView`]. The
//! framework owns the presentation contract and the declarative playback
//! attributes; the frame source (decoder, clock, transport) stays with the
//! host, which pushes frames and dispatches `play` / `pause` / `ended`.

use std::sync::Arc;

use nana_ui_core::ContentFit;

use crate::component_registry::{RegisterableComponent, SemanticSpec};
use crate::gpu_slots::{fill_layout, finite_opacity, finite_radius, parse_content_fit};
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, CustomRenderNode, InteractionState,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, UiWorld,
};

/// Builds the video host-texture slot for a numeric surface id. The Vue side
/// (`tree::video_host_texture_slot`) and the frame bridge must agree on it.
fn video_slot(id: u64) -> Option<String> {
    (id > 0).then(|| format!("video:{id}"))
}

/// Displays a host-fed video surface inside a layout region.
///
/// `resource` is the host texture registry slot the frame bridge registers.
/// `autoplay` / `loop_` / `muted` mirror the declarative HTML attributes; the
/// live playback truth stays with the host push side. Pointer events default
/// to off.
#[derive(Debug, Clone, PartialEq)]
pub struct Video {
    pub resource: Arc<str>,
    pub opacity: f32,
    pub corner_radius: f32,
    pub fit: ContentFit,
    pub style: NodeStyle,
    pub pointer_events: bool,
    pub autoplay: bool,
    pub loop_: bool,
    pub muted: bool,
}

impl Video {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn new(resource: impl Into<Arc<str>>) -> Self {
        Self {
            resource: resource.into(),
            opacity: 1.0,
            corner_radius: 0.0,
            fit: ContentFit::Fill,
            style: NodeStyle::default(),
            pointer_events: false,
            autoplay: false,
            loop_: false,
            muted: false,
        }
    }

    /// Host-texture Scene node. Empty identities are omitted so the world
    /// does not reject a blank `resource`. The live revision is stamped from
    /// the host texture registry at the frame boundary, so the node carries 0.
    pub fn custom_render(&self) -> Option<CustomRenderNode> {
        if self.resource.trim().is_empty() {
            return None;
        }
        Some(
            CustomRenderNode::new(crate::HOST_TEXTURE_RENDERER, Arc::clone(&self.resource), 0)
                .with_fit(self.fit),
        )
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = fill_layout(&self.style);
        let layout = Arc::make_mut(&mut style.layout);
        layout.opacity = Some(finite_opacity(self.opacity));
        layout.border_radius = Some(finite_radius(self.corner_radius));
        if self.custom_render().is_some() {
            layout.paint.content_image = None;
            layout.paint.skipped_replaced = None;
        }
        style
    }
}

impl ComponentView for Video {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "video".into(),
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

impl RegisterableComponent for Video {
    const TYPE_ID: &'static str = "nana.video";
    const TAGS: &'static [&'static str] = &["video"];
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let slot = spec
            .attr("data-nana-video")
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .and_then(video_slot)
            .unwrap_or_default();
        let mut video = Video::new(slot);
        video.style.layout = Arc::clone(spec.layout);
        if let Some(opacity) = spec.layout.opacity {
            video.opacity = finite_opacity(opacity);
        }
        if let Some(radius) = spec.layout.border_radius {
            video.corner_radius = finite_radius(radius);
        }
        if let Some(fit) = spec.attr("fit") {
            video.fit = parse_content_fit(fit);
        }
        if spec.attr("pointer-events").is_some_and(|value| {
            let value = value.trim();
            value.is_empty()
                || value.eq_ignore_ascii_case("auto")
                || value.eq_ignore_ascii_case("true")
        }) {
            video.pointer_events = true;
        }
        video.autoplay = is_attr_enabled(spec.attr("autoplay"));
        video.loop_ = is_attr_enabled(spec.attr("loop"));
        video.muted = is_attr_enabled(spec.attr("muted"));
        video
    }
}

/// HTML-style boolean attribute: bare presence or `true` enables.
fn is_attr_enabled(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        let value = value.trim();
        value.is_empty() || value.eq_ignore_ascii_case("true")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_registry::ComponentTypeId;
    use nana_ui_core::LayoutStyle;

    fn spec<'a>(
        type_id: &'a ComponentTypeId,
        attrs: &'a [(&'a str, &'a str)],
        layout: &'a Arc<LayoutStyle>,
    ) -> SemanticSpec<'a> {
        let mut spec = SemanticSpec::from_parts(type_id, layout);
        spec.attrs = attrs;
        spec
    }

    #[test]
    fn video_slot_rejects_zero_and_builds_prefix() {
        assert_eq!(video_slot(7).as_deref(), Some("video:7"));
        assert_eq!(video_slot(0), None);
    }

    #[test]
    fn custom_render_uses_host_texture_renderer_and_fit() {
        let mut video = Video::new("video:7");
        video.fit = ContentFit::Contain;
        let custom = video.custom_render().expect("slot present");
        assert_eq!(custom.renderer.as_ref(), crate::HOST_TEXTURE_RENDERER);
        assert_eq!(custom.resource.as_ref(), "video:7");
        assert_eq!(custom.revision, 0);
        assert_eq!(custom.fit, ContentFit::Contain);
    }

    #[test]
    fn empty_resource_omits_custom_render() {
        assert!(Video::new("").custom_render().is_none());
    }

    #[test]
    fn from_semantic_reads_slot_and_declarative_attrs() {
        let type_id = ComponentTypeId::new("nana.video").expect("valid id");
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [
            ("data-nana-video", "7"),
            ("fit", "contain"),
            ("autoplay", "true"),
            ("loop", ""),
            ("muted", "false"),
            ("pointer-events", "auto"),
        ];
        let video = Video::from_semantic(&spec(&type_id, &attrs, &layout));
        assert_eq!(video.resource.as_ref(), "video:7");
        assert_eq!(video.fit, ContentFit::Contain);
        assert!(video.autoplay);
        assert!(video.loop_);
        assert!(!video.muted);
        assert!(video.pointer_events);
    }

    #[test]
    fn from_semantic_without_slot_stays_blank() {
        let type_id = ComponentTypeId::new("nana.video").expect("valid id");
        let layout = Arc::new(LayoutStyle::default());
        let video = Video::from_semantic(&spec(&type_id, &[], &layout));
        assert_eq!(video.resource.as_ref(), "");
        assert!(video.custom_render().is_none());
        assert!(!video.autoplay);
    }

    #[test]
    fn from_semantic_rejects_non_positive_slot_ids() {
        let type_id = ComponentTypeId::new("nana.video").expect("valid id");
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [("data-nana-video", "0")];
        let video = Video::from_semantic(&spec(&type_id, &attrs, &layout));
        assert_eq!(video.resource.as_ref(), "");
    }

    fn mount(video: Video) -> (crate::UiWorld, StableNodeId) {
        let mut world = crate::UiWorld::new();
        let id = StableNodeId::new(1).unwrap();
        let document = crate::DocumentId::new(1).unwrap();
        let mut queue = MutationQueue::new();
        queue.create(id, document, video.node_kind());
        video.project(id, &world, &mut queue);
        world.commit(queue).unwrap();
        (world, id)
    }

    #[test]
    fn slotted_video_drops_poster_content_image() {
        let mut video = Video::new("video:7");
        let mut layout = LayoutStyle::default();
        layout.paint.content_image = Some(nana_ui_core::BackgroundImage::url("frame.png"));
        video.style.layout = Arc::new(layout);
        let (world, id) = mount(video);
        assert_eq!(
            world.custom_render(id).map(|node| node.resource.as_ref()),
            Some("video:7")
        );
        assert!(
            world
                .layout_style(id)
                .expect("projected")
                .paint
                .content_image
                .is_none(),
            "HostTexture video must not also paint poster"
        );
    }

    #[test]
    fn poster_only_video_keeps_content_image() {
        let mut video = Video::new("");
        let mut layout = LayoutStyle::default();
        layout.paint.content_image = Some(nana_ui_core::BackgroundImage::url("frame.png"));
        video.style.layout = Arc::new(layout);
        let (world, id) = mount(video);
        assert!(world.custom_render(id).is_none());
        match world
            .layout_style(id)
            .expect("projected")
            .paint
            .content_image
            .as_ref()
        {
            Some(nana_ui_core::BackgroundImage::Url { url, .. }) => {
                assert_eq!(url.as_str(), "frame.png")
            }
            other => panic!("expected poster content_image, got {other:?}"),
        }
    }
}

//! Layout and input identity for host-composited native content.
use crate::component_registry::{RegisterableComponent, SemanticSpec};
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, CustomRenderNode, InteractionState,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, UiWorld,
};
use nana_ui_core::{LengthSpec, OverflowSpec, SemanticColorRole};
use std::sync::Arc;

pub const NATIVE_CONTENT_RENDERER: &str = "nana.native-content";

/// The host binds `resource` and `generation` to a native surface. Scene owns
/// visibility and clipping; platform handles never enter the retained tree.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeContent {
    pub resource: Arc<str>,
    pub generation: u64,
    pub attached: bool,
    pub style: NodeStyle,
}

impl NativeContent {
    pub fn new(resource: impl Into<Arc<str>>) -> Self {
        let mut style = NodeStyle::default().surface(SemanticColorRole::Surface);
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Fill);
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.overflow_x = OverflowSpec::Hidden;
        layout.overflow_y = OverflowSpec::Hidden;
        Self {
            resource: resource.into(),
            generation: 0,
            attached: true,
            style,
        }
    }

    pub fn generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }
    pub fn attached(mut self, attached: bool) -> Self {
        self.attached = attached;
        self
    }
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
}

impl ComponentView for NativeContent {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "native-content".into(),
        }
    }
    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let custom = Some(
            CustomRenderNode::new(
                NATIVE_CONTENT_RENDERER,
                Arc::clone(&self.resource),
                self.generation,
            )
            .with_params([if self.attached { 1.0 } else { 0.0 }]),
        );
        if world.custom_render(id) != custom.as_ref() {
            mutations.set_custom_render(id, custom);
        }
        project_common(
            id,
            world,
            mutations,
            &self.style,
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                ..Default::default()
            },
        );
    }
}

impl RegisterableComponent for NativeContent {
    const TYPE_ID: &'static str = NATIVE_CONTENT_RENDERER;
    const TAGS: &'static [&'static str] = &["native-content"];
    fn from_semantic(_spec: &SemanticSpec<'_>) -> Self {
        Self::new("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_content_projects_generation_without_changing_identity() {
        let mut context = crate::AppContext::new();
        let document = crate::DocumentId::new(1).unwrap();
        let view = context
            .create_component(document, NativeContent::new("task/tab"))
            .unwrap();
        assert!(context.focus_node(document, view.stable_id()).unwrap());
        context
            .update_component(view, |view, _| view.generation = 2)
            .unwrap();
        let custom = context.world().custom_render(view.stable_id()).unwrap();
        assert_eq!(custom.resource.as_ref(), "task/tab");
        assert_eq!(custom.revision, 2);
        assert_eq!(custom.renderer.as_ref(), NATIVE_CONTENT_RENDERER);
    }
}

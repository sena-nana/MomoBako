//! Retained layout anchor for host-managed native browsing content.
use std::sync::Arc;

use crate::component_registry::{RegisterableComponent, SemanticSpec};
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, MutationQueue,
    NodeKind, NodeStyle, StableNodeId, UiWorld,
};

#[derive(Debug, Clone, PartialEq)]
pub struct BrowserView {
    pub browser_id: String,
    pub style: NodeStyle,
}

impl BrowserView {
    pub fn new(browser_id: impl Into<String>) -> Self {
        Self {
            browser_id: browser_id.into(),
            style: crate::Stack::fill_column(0.0)
                .surface(nana_ui_core::SemanticColorRole::Background)
                .node_style(),
        }
    }
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
}

impl ComponentView for BrowserView {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "nana.browser-view".into(),
        }
    }
    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let mut style = self.style.clone();
        style
            .background
            .get_or_insert(nana_ui_core::SemanticColorRole::Background);
        project_common(
            id,
            world,
            mutations,
            &style,
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
            AccessibilityState {
                role: AccessibilityRole::Document,
                label: Some(Arc::from("网页")),
                ..Default::default()
            },
        );
    }
}

impl RegisterableComponent for BrowserView {
    const TYPE_ID: &'static str = "nana.browser-view";
    const TAGS: &'static [&'static str] = &["browser-view"];
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut view = Self::new(spec.attr("browser-id").unwrap_or("browser"));
        view.style.layout = Arc::clone(spec.layout);
        view
    }
}

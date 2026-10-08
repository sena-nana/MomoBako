//! Backend-neutral overview minimap for a graph canvas.
//!
//! Applications own the canvas, the viewport, and persistence. This component
//! projects [`GraphModel::bounds`] uniformly into its own box, paints node
//! rectangles plus the canvas' visible-world indicator, and emits
//! [`GraphMinimapEvent::ViewportRequested`] for click and drag navigation. It
//! never mutates a graph canvas; the application applies the requested
//! viewport through `GraphCanvas::set_viewport`.

use std::sync::Arc;

use nana_ui_core::{
    GraphModel, GraphPoint, GraphRect, GraphSize, GraphViewport, OverflowSpec, SemanticColorRole,
};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, LayoutBox,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, StandardVisual, UiWorld,
};

const DEFAULT_LABEL: &str = "Graph minimap";

#[derive(Debug, Clone, PartialEq)]
pub enum GraphMinimapEvent {
    /// The viewport that centers the graph canvas on the point the user
    /// navigated to. Apply it with `GraphCanvas::set_viewport`.
    ViewportRequested(GraphViewport),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphMinimapDrag {
    pub pointer_id: u64,
    /// Viewport captured on press, re-requested when the drag is cancelled.
    pub initial: GraphViewport,
}

/// Uniform map projection between model bounds and the minimap box.
///
/// The map is letterboxed and centered so nodes keep their aspect ratio;
/// paint and navigation must share this one projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GraphMinimapProjection {
    bounds_origin: GraphPoint,
    origin: GraphPoint,
    scale: f32,
}

impl GraphMinimapProjection {
    pub(crate) fn new(box_size: GraphSize, bounds: GraphRect) -> Option<Self> {
        if !box_size.is_valid() || !bounds.size.is_valid() {
            return None;
        }
        let scale = (box_size.width / bounds.size.width).min(box_size.height / bounds.size.height);
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        Some(Self {
            bounds_origin: bounds.origin,
            origin: GraphPoint::new(
                (box_size.width - bounds.size.width * scale) * 0.5,
                (box_size.height - bounds.size.height * scale) * 0.5,
            ),
            scale,
        })
    }

    /// Widget-local point to world coordinates.
    fn to_world(self, local: GraphPoint) -> GraphPoint {
        GraphPoint::new(
            self.bounds_origin.x + (local.x - self.origin.x) / self.scale,
            self.bounds_origin.y + (local.y - self.origin.y) / self.scale,
        )
    }

    /// World point to widget-local coordinates.
    fn to_local(self, world: GraphPoint) -> GraphPoint {
        GraphPoint::new(
            self.origin.x + (world.x - self.bounds_origin.x) * self.scale,
            self.origin.y + (world.y - self.bounds_origin.y) * self.scale,
        )
    }

    pub(crate) fn local_rect(self, world: GraphRect) -> GraphRect {
        GraphRect::new(
            self.to_local(world.origin),
            GraphSize::new(
                world.size.width * self.scale,
                world.size.height * self.scale,
            ),
        )
    }
}

/// Controlled overview minimap. The mirrored viewport and canvas size live on
/// the view; navigation decisions are emitted as typed events.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphMinimap {
    pub model: GraphModel,
    pub viewport: GraphViewport,
    /// Visible size of the graph canvas this minimap mirrors. Drives the
    /// indicator rectangle and navigation offsets.
    pub canvas_size: GraphSize,
    /// Optional palette role overriding the default node fill.
    pub node_fill: Option<SemanticColorRole>,
    pub disabled: bool,
    pub label: Option<Arc<str>>,
    pub style: NodeStyle,
    pub dragging: Option<GraphMinimapDrag>,
}

impl GraphMinimap {
    pub fn new(model: GraphModel) -> Self {
        Self {
            model,
            viewport: GraphViewport::default(),
            canvas_size: GraphSize::default(),
            node_fill: None,
            disabled: false,
            label: None,
            style: minimap_style(),
            dragging: None,
        }
    }

    pub fn viewport(mut self, viewport: GraphViewport) -> Self {
        self.viewport = viewport;
        self
    }

    pub fn canvas_size(mut self, size: GraphSize) -> Self {
        if size.is_valid() {
            self.canvas_size = size;
        }
        self
    }

    pub fn node_fill(mut self, role: SemanticColorRole) -> Self {
        self.node_fill = Some(role);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn set_model(&mut self, model: GraphModel) {
        if self.model != model {
            self.model = model;
        }
    }

    pub fn set_viewport(&mut self, viewport: GraphViewport) {
        if self.viewport != viewport {
            self.viewport = viewport;
        }
    }

    /// World-space rectangle of the canvas region visible through the
    /// viewport. `None` without a mapped model or a known canvas size.
    fn visible_world_rect(&self) -> Option<GraphRect> {
        self.model.bounds()?;
        if !self.canvas_size.is_valid() {
            return None;
        }
        let size = GraphSize::new(
            self.canvas_size.width / self.viewport.zoom,
            self.canvas_size.height / self.viewport.zoom,
        );
        size.is_valid()
            .then(|| GraphRect::new(self.viewport.view_to_world(GraphPoint::ZERO), size))
    }

    /// Viewport that centers the canvas on `local` (widget coordinates inside
    /// `box_size`). Clamped to the mapped model bounds.
    pub fn requested_viewport(
        &self,
        box_size: GraphSize,
        local: GraphPoint,
    ) -> Option<GraphViewport> {
        let bounds = self.model.bounds()?;
        let projection = GraphMinimapProjection::new(box_size, bounds)?;
        if !local.is_finite() {
            return None;
        }
        let clamped = GraphPoint::new(
            local.x.clamp(0.0, box_size.width),
            local.y.clamp(0.0, box_size.height),
        );
        let world = projection.to_world(clamped);
        Some(GraphViewport::new(
            GraphPoint::new(
                self.canvas_size.width * 0.5 - world.x * self.viewport.zoom,
                self.canvas_size.height * 0.5 - world.y * self.viewport.zoom,
            ),
            self.viewport.zoom,
        ))
    }

    fn navigate(&mut self, box_size: GraphSize, local: GraphPoint) -> Option<GraphMinimapEvent> {
        let next = self.requested_viewport(box_size, local)?;
        self.viewport = next;
        Some(GraphMinimapEvent::ViewportRequested(next))
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.overflow_x = OverflowSpec::Hidden;
        layout.overflow_y = OverflowSpec::Hidden;
        if style.background.is_none() {
            style.background = Some(SemanticColorRole::Subtle);
        }
        if style.border.is_none() {
            style.border = Some(SemanticColorRole::Border);
        }
        if layout.border_width.is_none() {
            layout.border_width = Some(nana_ui_core::HAIRLINE);
        }
        let unset_radius = layout.border_radius.is_none();
        if self.disabled {
            style.foreground = Some(SemanticColorRole::Faint);
        }
        if unset_radius {
            style.radius = Some(nana_ui_core::RadiusTier::Sm);
        }
        style
    }
}

impl ComponentView for GraphMinimap {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "graph-minimap".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let visual = StandardVisual::GraphMinimap {
            bounds: self.model.bounds().unwrap_or_default(),
            nodes: self
                .model
                .nodes()
                .iter()
                .map(|node| node.bounds())
                .collect(),
            indicator: self.visible_world_rect(),
            node_fill: self.node_fill,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: !self.disabled,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(
                    self.label
                        .clone()
                        .unwrap_or_else(|| Arc::from(DEFAULT_LABEL)),
                ),
                disabled: self.disabled,
                ..AccessibilityState::default()
            },
        );
    }
}

impl crate::AppContext {
    pub fn is_graph_minimap(&self, id: StableNodeId) -> bool {
        self.read(crate::Entity::<GraphMinimap>::from_stable_id(id), |_| ())
            .is_ok()
    }

    pub fn begin_graph_minimap_pointer(
        &mut self,
        pointer_id: u64,
        target: StableNodeId,
        x: f32,
        y: f32,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(entity) = self.graph_minimap_entity(target) else {
            return Ok(false);
        };
        if self.read(entity, |minimap| {
            minimap.disabled || minimap.model.bounds().is_none()
        })? {
            return Ok(false);
        }
        let Some(bounds) = self.world().layout_box(target) else {
            return Ok(false);
        };
        if !point_in_bounds(bounds, x, y) {
            return Ok(false);
        }
        self.update_component(entity, |minimap, cx| {
            let initial = minimap.viewport;
            cx.mutations().capture_pointer(pointer_id, target);
            if let Some(event) = minimap.navigate(
                GraphSize::new(bounds.width, bounds.height),
                local_point(bounds, x, y),
            ) {
                cx.emit(event);
            }
            minimap.dragging = Some(GraphMinimapDrag {
                pointer_id,
                initial,
            });
            true
        })
    }

    pub fn update_graph_minimap_pointer(
        &mut self,
        document: crate::DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(target) = self.world().pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        let Some(entity) = self.graph_minimap_entity(target) else {
            return Ok(false);
        };
        let Some(bounds) = self.world().layout_box(target) else {
            return Ok(false);
        };
        self.update_component(entity, |minimap, cx| {
            if minimap
                .dragging
                .is_none_or(|drag| drag.pointer_id != pointer_id)
            {
                return false;
            }
            if let Some(event) = minimap.navigate(
                GraphSize::new(bounds.width, bounds.height),
                local_point(bounds, x, y),
            ) {
                cx.emit(event);
            }
            true
        })
    }

    pub fn end_graph_minimap_pointer(
        &mut self,
        document: crate::DocumentId,
        pointer_id: u64,
        cancel: bool,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(target) = self.world().pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        let Some(entity) = self.graph_minimap_entity(target) else {
            return Ok(false);
        };
        self.update_component(entity, |minimap, cx| {
            let drag = minimap
                .dragging
                .filter(|drag| drag.pointer_id == pointer_id);
            minimap.dragging = None;
            cx.mutations().release_pointer(pointer_id, target);
            let Some(drag) = drag else {
                return false;
            };
            if cancel {
                minimap.set_viewport(drag.initial);
                cx.emit(GraphMinimapEvent::ViewportRequested(drag.initial));
            }
            true
        })
    }

    fn graph_minimap_entity(&self, id: StableNodeId) -> Option<crate::Entity<GraphMinimap>> {
        self.is_graph_minimap(id)
            .then(|| crate::Entity::from_stable_id(id))
    }
}

fn minimap_style() -> NodeStyle {
    NodeStyle {
        layout: Arc::new(nana_ui_core::LayoutStyle {
            overflow_x: OverflowSpec::Hidden,
            overflow_y: OverflowSpec::Hidden,
            border_width: Some(nana_ui_core::HAIRLINE),
            ..nana_ui_core::LayoutStyle::default()
        }),
        radius: Some(nana_ui_core::RadiusTier::Sm),
        background: Some(SemanticColorRole::Subtle),
        border: Some(SemanticColorRole::Border),
        ..NodeStyle::default()
    }
}

fn local_point(bounds: LayoutBox, x: f32, y: f32) -> GraphPoint {
    GraphPoint::new(x - bounds.x, y - bounds.y)
}

fn point_in_bounds(bounds: LayoutBox, x: f32, y: f32) -> bool {
    x >= bounds.x && y >= bounds.y && x <= bounds.x + bounds.width && y <= bounds.y + bounds.height
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, DocumentId, MutationQueue};
    use std::sync::{Arc, Mutex};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn sample_graph() -> GraphModel {
        let source = nana_ui_core::GraphNode::new(
            "source",
            "Source",
            GraphPoint::new(10.0, 20.0),
            GraphSize::new(120.0, 80.0),
        );
        let target = nana_ui_core::GraphNode::new(
            "target",
            "Target",
            GraphPoint::new(250.0, 20.0),
            GraphSize::new(120.0, 80.0),
        );
        GraphModel::new(vec![source, target], Vec::new()).expect("valid graph")
    }

    fn layout(context: &mut AppContext, id: crate::StableNodeId, width: f32, height: f32) {
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            id,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
        );
        context.commit_mutations(mutations).unwrap();
    }

    fn collect_events(
        context: &mut AppContext,
        minimap: crate::Entity<GraphMinimap>,
    ) -> Arc<Mutex<Vec<GraphMinimapEvent>>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        context
            .on(minimap, move |_minimap, event: &GraphMinimapEvent, _cx| {
                observed.lock().unwrap().push(event.clone());
            })
            .unwrap();
        events
    }

    /// Box 180×135 over bounds (10, 20, 360×80) maps uniformly at 0.5 with a
    /// vertical letterbox origin of 47.5.
    fn navigable_minimap() -> GraphMinimap {
        GraphMinimap::new(sample_graph()).canvas_size(GraphSize::new(400.0, 300.0))
    }

    #[test]
    fn projects_nodes_indicator_and_letterboxed_geometry() {
        let mut context = AppContext::new();
        let minimap = context
            .create_component(
                document(),
                GraphMinimap::new(sample_graph())
                    .canvas_size(GraphSize::new(200.0, 50.0))
                    .viewport(GraphViewport::new(GraphPoint::new(-100.0, -50.0), 1.0)),
            )
            .unwrap();
        layout(&mut context, minimap.stable_id(), 180.0, 135.0);
        assert!(matches!(
            context.world().standard_visual(minimap.stable_id()),
            Some(StandardVisual::GraphMinimap {
                bounds,
                ref nodes,
                indicator: Some(indicator),
                node_fill: None,
            }) if bounds
                == GraphRect::new(GraphPoint::new(10.0, 20.0), GraphSize::new(360.0, 80.0))
                && nodes.as_ref()
                    == [
                        GraphRect::new(GraphPoint::new(10.0, 20.0), GraphSize::new(120.0, 80.0)),
                        GraphRect::new(GraphPoint::new(250.0, 20.0), GraphSize::new(120.0, 80.0)),
                    ]
                .as_slice()
                && indicator == GraphRect::new(GraphPoint::new(100.0, 50.0), GraphSize::new(200.0, 50.0))
        ));

        let work = context.take_system_work();
        context.resolve_styles(&work.style).unwrap();
        context.resolve_styles(&[minimap.stable_id()]).unwrap();
        let Some(crate::ComponentGeometry::GraphMinimap {
            nodes,
            indicator,
            indicator_border,
            ..
        }) = context.world().component_geometry(minimap.stable_id())
        else {
            panic!("minimap geometry");
        };
        assert_eq!(
            nodes,
            vec![
                LayoutBox {
                    x: 0.0,
                    y: 47.5,
                    width: 60.0,
                    height: 40.0,
                },
                LayoutBox {
                    x: 120.0,
                    y: 47.5,
                    width: 60.0,
                    height: 40.0,
                },
            ]
        );
        assert_eq!(
            indicator,
            Some(LayoutBox {
                x: 45.0,
                y: 62.5,
                width: 100.0,
                height: 25.0,
            })
        );
        assert_ne!(indicator_border, [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn indicator_hides_without_a_known_canvas_size() {
        let mut context = AppContext::new();
        let minimap = context
            .create_component(document(), GraphMinimap::new(sample_graph()))
            .unwrap();
        layout(&mut context, minimap.stable_id(), 180.0, 135.0);
        assert!(matches!(
            context.world().standard_visual(minimap.stable_id()),
            Some(StandardVisual::GraphMinimap {
                indicator: None,
                ..
            })
        ));
    }

    #[test]
    fn click_requests_viewport_centered_on_the_mapped_point() {
        let mut context = AppContext::new();
        let minimap = context
            .create_component(document(), navigable_minimap())
            .unwrap();
        layout(&mut context, minimap.stable_id(), 180.0, 135.0);
        let events = collect_events(&mut context, minimap);

        assert!(
            context
                .begin_graph_minimap_pointer(1, minimap.stable_id(), 90.0, 60.0)
                .unwrap()
        );
        let requested = GraphViewport::new(GraphPoint::new(10.0, 105.0), 1.0);
        assert_eq!(
            *events.lock().unwrap(),
            [GraphMinimapEvent::ViewportRequested(requested)]
        );
        assert_eq!(context.read(minimap, |m| m.viewport).unwrap(), requested);
        assert!(
            context
                .end_graph_minimap_pointer(document(), 1, false)
                .unwrap()
        );
        assert!(context.world().pointer_capture(document(), 1).is_none());
    }

    #[test]
    fn drag_follows_the_pointer_until_release() {
        let mut context = AppContext::new();
        let minimap = context
            .create_component(document(), navigable_minimap())
            .unwrap();
        layout(&mut context, minimap.stable_id(), 180.0, 135.0);
        let events = collect_events(&mut context, minimap);

        assert!(
            context
                .begin_graph_minimap_pointer(7, minimap.stable_id(), 60.0, 87.5)
                .unwrap()
        );
        assert!(
            context
                .update_graph_minimap_pointer(document(), 7, 100.0, 107.5)
                .unwrap()
        );
        assert!(
            context
                .end_graph_minimap_pointer(document(), 7, false)
                .unwrap()
        );
        assert_eq!(
            *events.lock().unwrap(),
            [
                GraphMinimapEvent::ViewportRequested(GraphViewport::new(
                    GraphPoint::new(70.0, 50.0),
                    1.0
                )),
                GraphMinimapEvent::ViewportRequested(GraphViewport::new(
                    GraphPoint::new(-10.0, 10.0),
                    1.0
                )),
            ]
        );
        assert!(context.world().pointer_capture(document(), 7).is_none());
        assert_eq!(
            context.read(minimap, |m| m.dragging).unwrap(),
            None,
            "release must end the drag"
        );
    }

    #[test]
    fn cancel_restores_the_viewport_captured_on_press() {
        let mut context = AppContext::new();
        let minimap = context
            .create_component(
                document(),
                navigable_minimap().viewport(GraphViewport::new(GraphPoint::new(-40.0, 12.0), 1.5)),
            )
            .unwrap();
        layout(&mut context, minimap.stable_id(), 180.0, 135.0);
        let events = collect_events(&mut context, minimap);

        assert!(
            context
                .begin_graph_minimap_pointer(3, minimap.stable_id(), 90.0, 87.5)
                .unwrap()
        );
        assert!(
            context
                .end_graph_minimap_pointer(document(), 3, true)
                .unwrap()
        );
        assert_eq!(
            events.lock().unwrap().last(),
            Some(&GraphMinimapEvent::ViewportRequested(GraphViewport::new(
                GraphPoint::new(-40.0, 12.0),
                1.5
            )))
        );
        assert_eq!(
            context.read(minimap, |m| m.viewport).unwrap(),
            GraphViewport::new(GraphPoint::new(-40.0, 12.0), 1.5)
        );
    }

    #[test]
    fn empty_model_projects_nothing_and_ignores_pointer() {
        let mut context = AppContext::new();
        let minimap = context
            .create_component(
                document(),
                GraphMinimap::new(GraphModel::empty()).canvas_size(GraphSize::new(400.0, 300.0)),
            )
            .unwrap();
        layout(&mut context, minimap.stable_id(), 180.0, 135.0);
        let events = collect_events(&mut context, minimap);

        assert!(matches!(
            context.world().standard_visual(minimap.stable_id()),
            Some(StandardVisual::GraphMinimap {
                ref nodes,
                indicator: None,
                ..
            }) if nodes.is_empty()
        ));
        assert!(
            !context
                .begin_graph_minimap_pointer(1, minimap.stable_id(), 90.0, 60.0)
                .unwrap()
        );
        assert!(
            !context
                .update_graph_minimap_pointer(document(), 1, 90.0, 60.0)
                .unwrap()
        );
        assert!(
            !context
                .end_graph_minimap_pointer(document(), 1, false)
                .unwrap()
        );
        assert!(events.lock().unwrap().is_empty());
        assert!(context.world().pointer_capture(document(), 1).is_none());
    }

    #[test]
    fn disabled_minimap_ignores_pointer() {
        let mut context = AppContext::new();
        let minimap = context
            .create_component(document(), navigable_minimap().disabled(true))
            .unwrap();
        layout(&mut context, minimap.stable_id(), 180.0, 135.0);
        let events = collect_events(&mut context, minimap);

        assert!(
            !context
                .begin_graph_minimap_pointer(1, minimap.stable_id(), 90.0, 60.0)
                .unwrap()
        );
        assert!(events.lock().unwrap().is_empty());
        assert!(context.world().pointer_capture(document(), 1).is_none());
    }
}

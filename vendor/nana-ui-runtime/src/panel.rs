//! Nonmodal surfaces use ordinary Card rendering and OverlayHost presence.
use std::sync::Arc;

use crate::{
    AccessibilityRole, AccessibilityState, Card, ComponentView, InteractionState, LayoutBox,
    LengthSpec, MutationQueue, NodeKind, NodeStyle, StableNodeId, UiWorld,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelEdge {
    Left,
    #[default]
    Right,
}

/// Logical pixels reserved by the application for window chrome and tools.
/// Shared by every surface the application pins into that reserved space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PanelInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl PanelInsets {
    /// Clamp reserved chrome into `viewport` in start/end order, including when
    /// chrome consumes the entire viewport. Non-finite values read as zero.
    pub(crate) fn clamp_into(self, viewport: LayoutBox) -> Self {
        let vw = positive(viewport.width);
        let vh = positive(viewport.height);
        let left = positive(self.left).min(vw);
        let top = positive(self.top).min(vh);
        Self {
            top,
            right: positive(self.right).min(vw - left),
            bottom: positive(self.bottom).min(vh - top),
            left,
        }
    }
}

pub(crate) fn positive(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

pub(crate) fn origin(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// A named nonmodal region. Its children provide the visible title, actions and
/// body, usually a ScrollView. The surface alone receives pointer input; the
/// stage around it and the document's ordinary Tab order remain available.
///
/// Attach directly to an OverlayHost and use activate_overlay/dismiss_overlay.
/// Closing and exit completion use OverlayClosing and OverlayChanged, just like
/// dialogs. Business navigation and pinning remain application-owned.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub label: Arc<str>,
    /// 外框类型，与 [`Card`] 相同。[`CardKind::Flat`](nana_ui_core::CardKind)
    /// 不画底色和边框，适合交给 painter 自绘或做透明外框。
    pub kind: nana_ui_core::CardKind,
    pub close_on_escape: bool,
    pub focus_on_open: bool,
    pub style: NodeStyle,
}

impl Panel {
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        let mut style = crate::Stack::fill_column(8.0).node_style();
        Arc::make_mut(&mut style.layout).z_index = Some(100);
        Self {
            label: label.into(),
            kind: nana_ui_core::CardKind::Surface,
            close_on_escape: true,
            focus_on_open: true,
            style,
        }
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn kind(mut self, kind: nana_ui_core::CardKind) -> Self {
        self.kind = kind;
        self
    }

    /// 自绘外框，见 [`crate::Painter`]。
    pub fn painter(mut self, painter: impl Into<crate::NodePainter>) -> Self {
        self.style.painter = Some(painter.into());
        self
    }

    pub fn close_on_escape(mut self, enabled: bool) -> Self {
        self.close_on_escape = enabled;
        self
    }

    /// Keep focus where it is when restoring a pinned/passive panel.
    pub fn focus_on_open(mut self, enabled: bool) -> Self {
        self.focus_on_open = enabled;
        self
    }

    /// Constrain the surface to the available viewport. Insets are clamped in
    /// start/end order, including when chrome consumes the entire viewport.
    /// This updates only geometry; authored padding and visual style survive.
    pub fn viewport(
        mut self,
        viewport: LayoutBox,
        edge: PanelEdge,
        width: f32,
        insets: PanelInsets,
    ) -> Self {
        let bounds = Self::bounds(viewport, edge, width, insets);
        let layout = Arc::make_mut(&mut self.style.layout);
        layout.position = crate::PositionSpec::Absolute;
        layout.offset_left = Some(LengthSpec::Px(bounds.x));
        layout.offset_top = Some(LengthSpec::Px(bounds.y));
        layout.offset_right = None;
        layout.offset_bottom = None;
        layout.width = Some(LengthSpec::Px(bounds.width));
        layout.height = Some(LengthSpec::Px(bounds.height));
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        self
    }

    pub fn bounds(
        viewport: LayoutBox,
        edge: PanelEdge,
        width: f32,
        insets: PanelInsets,
    ) -> LayoutBox {
        let vw = positive(viewport.width);
        let vh = positive(viewport.height);
        let insets = insets.clamp_into(viewport);
        let width = positive(width).min(vw - insets.left - insets.right);
        LayoutBox {
            x: origin(viewport.x)
                + match edge {
                    PanelEdge::Left => insets.left,
                    PanelEdge::Right => vw - insets.right - width,
                },
            y: origin(viewport.y) + insets.top,
            width,
            height: vh - insets.top - insets.bottom,
        }
    }
}

impl ComponentView for Panel {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "panel".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        // Share Card's semantic surface, spacing and radius instead of adding
        // another painter or copying its appearance tokens.
        Card::new()
            .kind(self.kind)
            .style(self.style.clone())
            .project(id, world, mutations);
        mutations.set_interaction(
            id,
            InteractionState {
                pointer_events: true,
                focusable: false,
            },
        );
        mutations.set_accessibility(
            id,
            AccessibilityState {
                role: AccessibilityRole::Region,
                label: Some(self.label.clone()),
                ..AccessibilityState::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppContext, Button, Dialog, DocumentId, Entity, OverlayHost, OverlayKey,
        OverlayPointerPhase,
    };
    use std::time::Duration;

    fn fixture() -> (
        AppContext,
        DocumentId,
        Entity<Button>,
        Entity<OverlayHost>,
        Entity<Panel>,
        Entity<Button>,
    ) {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let entry = cx.create_component(doc, Button::new("Open")).unwrap();
        let host = cx.create_component(doc, OverlayHost::new()).unwrap();
        let panel = cx.create_component(doc, Panel::new("Tools")).unwrap();
        let action = cx.create_component(doc, Button::new("Apply")).unwrap();
        cx.append_child(host, panel).unwrap();
        cx.append_child(panel, action).unwrap();
        let mut layout = MutationQueue::new();
        layout.write_layout(
            entry.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 40.0,
            },
        );
        layout.write_layout(
            panel.stable_id(),
            LayoutBox {
                x: 100.0,
                y: 0.0,
                width: 100.0,
                height: 200.0,
            },
        );
        cx.commit_mutations(layout).unwrap();
        cx.focus_node(doc, entry.stable_id()).unwrap();
        cx.activate_overlay(host, panel).unwrap();
        tick(&mut cx, 200);
        (cx, doc, entry, host, panel, action)
    }

    fn tick(cx: &mut AppContext, ms: u64) {
        cx.advance_animations(Duration::from_millis(ms));
        let work = cx.take_system_work();
        cx.resolve_styles(&work.style).unwrap();
    }

    #[test]
    fn panel_leaves_stage_pointer_and_tab_available() {
        let (mut cx, doc, entry, host, panel, action) = fixture();
        assert_eq!(cx.world().focused(doc), Some(action.stable_id()));
        assert!(!cx.has_blocking_runtime_overlay(doc));
        assert!(
            !cx.world()
                .interaction(host.stable_id())
                .unwrap()
                .pointer_events
        );
        assert!(!cx.world().accessibility(panel.stable_id()).unwrap().modal);
        cx.rebuild_hit_test(doc);
        let down = cx
            .route_overlay_pointer(doc, 1, OverlayPointerPhase::PrimaryDown, 10.0, 10.0)
            .unwrap();
        assert_eq!(down.target, Some(entry.stable_id()));
        assert!(!down.prevent_default);
        assert!(!down.dismissed);
        assert!(
            !cx.route_overlay_key(doc, OverlayKey::Tab { reverse: false })
                .unwrap()
        );
        cx.navigate_sequential_focus(doc, false).unwrap();
        assert_eq!(cx.world().focused(doc), Some(entry.stable_id()));
    }

    #[test]
    fn closing_restores_focus_and_reopening_survives_old_exit() {
        let (mut cx, doc, entry, host, panel, action) = fixture();
        cx.dismiss_overlay(host).unwrap();
        assert_eq!(cx.world().focused(doc), Some(entry.stable_id()));
        assert!(cx.active_runtime_overlay(doc).is_none());
        assert_eq!(
            cx.world().overlay_host(host.stable_id()).unwrap().active,
            Some(panel.stable_id())
        );
        cx.activate_overlay(host, panel).unwrap();
        tick(&mut cx, 600);
        assert_eq!(
            cx.world().overlay_host(host.stable_id()).unwrap().active,
            Some(panel.stable_id())
        );
        assert_eq!(cx.world().focused(doc), Some(action.stable_id()));
        cx.dismiss_overlay(host).unwrap();
        tick(&mut cx, 1000);
        assert_eq!(
            cx.world().overlay_host(host.stable_id()).unwrap().active,
            None
        );
    }

    #[test]
    fn dialog_above_panel_traps_focus_and_escape_closes_one_layer() {
        let (mut cx, doc, _, panel_host, panel, action) = fixture();
        let host = cx.create_component(doc, OverlayHost::new()).unwrap();
        let dialog = cx.create_component(doc, Dialog::new("Confirm")).unwrap();
        cx.append_child(host, dialog).unwrap();
        cx.activate_overlay(host, dialog).unwrap();
        assert!(cx.has_blocking_runtime_overlay(doc));
        assert!(
            cx.route_overlay_key(doc, OverlayKey::Tab { reverse: false })
                .unwrap()
        );
        assert!(cx.route_overlay_key(doc, OverlayKey::Escape).unwrap());
        assert_eq!(cx.world().focused(doc), Some(action.stable_id()));
        assert_eq!(
            cx.active_runtime_overlay(doc).unwrap().root,
            panel.stable_id()
        );
        assert!(!cx.has_blocking_runtime_overlay(doc));
        assert!(cx.route_overlay_key(doc, OverlayKey::Escape).unwrap());
        assert!(cx.active_runtime_overlay(doc).is_none());
        tick(&mut cx, 600);
        assert_eq!(
            cx.world()
                .overlay_host(panel_host.stable_id())
                .unwrap()
                .active,
            None
        );
    }

    #[test]
    fn panel_respects_application_navigation_and_focus_outside() {
        let (mut cx, doc, entry, host, panel, _) = fixture();
        cx.update_component(panel, |panel, _| panel.close_on_escape = false)
            .unwrap();
        assert!(!cx.route_overlay_key(doc, OverlayKey::Escape).unwrap());
        cx.focus_node(doc, entry.stable_id()).unwrap();
        cx.dismiss_overlay(host).unwrap();
        assert_eq!(cx.world().focused(doc), Some(entry.stable_id()));
        cx.update_component(panel, |panel, _| panel.focus_on_open = false)
            .unwrap();
        cx.activate_overlay(host, panel).unwrap();
        assert_eq!(cx.world().focused(doc), Some(entry.stable_id()));
    }

    #[test]
    fn hidden_return_target_is_not_restored() {
        let (mut cx, doc, entry, host, _, _) = fixture();
        cx.update_component(entry, |button, _| {
            Arc::make_mut(&mut button.style.layout).hidden = true
        })
        .unwrap();
        cx.dismiss_overlay(host).unwrap();
        assert_eq!(cx.world().focused(doc), None);
    }

    #[test]
    fn inactive_panels_keep_state_but_do_not_paint_or_receive_input() {
        let (mut cx, doc, _, host, first, _) = fixture();
        let second = cx.create_component(doc, Panel::new("Other tools")).unwrap();
        let action = cx
            .create_component(doc, Button::new("Other action"))
            .unwrap();
        cx.append_child(host, second).unwrap();
        cx.append_child(second, action).unwrap();
        let mut layout = MutationQueue::new();
        layout.write_layout(
            second.stable_id(),
            LayoutBox {
                x: 220.0,
                y: 0.0,
                width: 100.0,
                height: 200.0,
            },
        );
        cx.commit_mutations(layout).unwrap();
        tick(&mut cx, 300);
        assert!(
            !cx.world()
                .extract_document(doc)
                .iter()
                .any(|node| node.id == second.stable_id())
        );
        assert!(!cx.focus_first_in(doc, second.stable_id()).unwrap());
        cx.activate_overlay(host, second).unwrap();
        tick(&mut cx, 500);
        let painted = cx.world().extract_document(doc);
        assert!(painted.iter().any(|node| node.id == second.stable_id()));
        assert!(!painted.iter().any(|node| node.id == first.stable_id()));
        assert!(!cx.focus_first_in(doc, first.stable_id()).unwrap());
        cx.dismiss_overlay(host).unwrap();
        tick(&mut cx, 800);
        assert!(cx.world().is_mounted(second.stable_id()));
        assert!(
            !cx.world()
                .extract_document(doc)
                .iter()
                .any(|node| node.id == second.stable_id())
        );
        cx.rebuild_hit_test(doc);
        assert!(
            !cx.world()
                .hit_test_candidates(doc, 230.0, 10.0)
                .contains(&second.stable_id())
        );
    }

    #[test]
    fn transferring_panel_preserves_content_focus_and_presence() {
        let (mut cx, doc, entry, source, panel, action) = fixture();
        let pinned = cx.create_component(doc, OverlayHost::new()).unwrap();
        assert!(cx.transfer_panel(panel, source, pinned).unwrap());
        assert_eq!(cx.world().focused(doc), Some(action.stable_id()));
        assert_eq!(
            cx.world().node(panel.stable_id()).unwrap().parent,
            Some(pinned.stable_id())
        );
        assert_eq!(
            cx.world().overlay_host(source.stable_id()).unwrap().active,
            None
        );
        tick(&mut cx, 500);
        assert_eq!(
            cx.active_runtime_overlay(doc).unwrap().root,
            panel.stable_id()
        );
        assert!(cx.transfer_panel(panel, pinned, source).unwrap());
        cx.dismiss_overlay(source).unwrap();
        assert_eq!(cx.world().focused(doc), Some(entry.stable_id()));
    }

    #[test]
    fn focus_first_uses_reachable_enabled_candidates() {
        let (mut cx, doc, _, _, panel, action) = fixture();
        let disabled = cx
            .create_component(doc, Button::new("Disabled").disabled(true))
            .unwrap();
        let hidden = cx.create_component(doc, Button::new("Hidden")).unwrap();
        let mut mutations = MutationQueue::new();
        mutations.insert(
            panel.stable_id(),
            disabled.stable_id(),
            Some(action.stable_id()),
        );
        mutations.insert(
            panel.stable_id(),
            hidden.stable_id(),
            Some(action.stable_id()),
        );
        cx.commit_mutations(mutations).unwrap();
        cx.update_component(hidden, |button, _| {
            Arc::make_mut(&mut button.style.layout).hidden = true
        })
        .unwrap();
        cx.clear_focus(doc).unwrap();
        assert!(cx.focus_first_in(doc, panel.stable_id()).unwrap());
        assert_eq!(cx.world().focused(doc), Some(action.stable_id()));
    }

    #[test]
    fn viewport_keeps_panels_inside_reserved_chrome_including_tiny_windows() {
        let viewport = LayoutBox {
            x: 10.0,
            y: 20.0,
            width: 800.0,
            height: 600.0,
        };
        let insets = PanelInsets {
            top: 40.0,
            right: 16.0,
            bottom: 72.0,
            left: 16.0,
        };
        assert_eq!(
            Panel::bounds(viewport, PanelEdge::Right, 400.0, insets),
            LayoutBox {
                x: 394.0,
                y: 60.0,
                width: 400.0,
                height: 488.0
            }
        );
        for width in [0.0, 20.0, 100.0, 800.0] {
            for height in [0.0, 20.0, 600.0] {
                let view = LayoutBox {
                    width,
                    height,
                    ..viewport
                };
                for edge in [PanelEdge::Left, PanelEdge::Right] {
                    let panel = Panel::bounds(view, edge, 400.0, insets);
                    assert!(panel.x >= view.x && panel.y >= view.y);
                    assert!(panel.width >= 0.0 && panel.height >= 0.0);
                    assert!(panel.x + panel.width <= view.x + width);
                    assert!(panel.y + panel.height <= view.y + height);
                }
            }
        }
    }

    #[test]
    fn a_flat_panel_has_no_surface_of_its_own() {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let surface = cx.create_component(doc, Panel::new("Tools")).unwrap();
        let flat = cx
            .create_component(doc, Panel::new("Tools").kind(nana_ui_core::CardKind::Flat))
            .unwrap();
        let background = |id| cx.world().node_style(id).and_then(|style| style.background);
        assert_eq!(
            background(surface.stable_id()),
            Some(nana_ui_core::SemanticColorRole::Surface)
        );
        assert_eq!(background(flat.stable_id()), None);
    }
}

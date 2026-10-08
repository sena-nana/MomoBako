//! Pinned pane section: header and tabs above a flexible body, one content line.
//!
//! Inspector-style panes pin card surfaces and a chrome control row (a
//! [`Tabs`](crate::Tabs) strip, for one) over a scrolling body. The section
//! owns the pane gutter and, through the tabs slot shell, the horizontal
//! content inset, so tab chrome lands on the same vertical line as the cards'
//! own `panel_padding_x` content inset. Host content keeps projecting its own
//! nodes; the slot shells are the single style writer for pane geometry, and
//! [`PaneSection::tabs_hidden`] collapses the strip without leaving a gap.

use std::collections::HashSet;
use std::sync::Arc;

#[cfg(test)]
use nana_ui_core::UI_METRICS;
use nana_ui_core::{AlignSpec, FlexDirection, LengthSpec, OverflowSpec};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, AppContext, ComponentView, DocumentId, Entity,
    FrameworkError, InteractionState, MutationQueue, NodeKind, NodeStyle, StableNodeId,
    TextContent, UiWorld,
};

/// Inset between pane content and the region edge. Card surfaces in the pane
/// keep [`UI_METRICS::panel_padding_x`] as their own content inset; this
/// gutter frames the card boxes around them.
const PANE_GUTTER: f32 = nana_ui_core::space::XL;

/// Vertical distance between the pinned rows and the body.
const PANE_GAP: f32 = nana_ui_core::space::SM;

/// Header and tabs above a flexible body, sharing the cards' content line.
///
/// `header` and `body` host card surfaces that own their own geometry: a
/// [`Card`](crate::Card) carries `panel_padding_x` as its content inset and
/// the shell adds none. The `tabs` row hosts chrome without a card surface, so
/// its shell applies that inset itself — that is what keeps tab labels and
/// selection pills on the card content line instead of the card box line.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneSection {
    pub header: Option<StableNodeId>,
    pub tabs: Option<StableNodeId>,
    pub body: Option<StableNodeId>,
    /// A tab strip collapses to nothing while it has no options; the host
    /// flips this instead of hiding the strip so the shell — which carries
    /// the row inset — leaves no gap behind.
    pub tabs_hidden: bool,
    pub(crate) header_slot: Option<StableNodeId>,
    pub(crate) tabs_slot: Option<StableNodeId>,
    pub(crate) body_slot: Option<StableNodeId>,
    pub gap: f32,
    pub style: NodeStyle,
}

impl PaneSection {
    pub fn new() -> Self {
        Self {
            header: None,
            tabs: None,
            body: None,
            tabs_hidden: false,
            header_slot: None,
            tabs_slot: None,
            body_slot: None,
            gap: PANE_GAP,
            style: NodeStyle::default(),
        }
    }

    pub fn header(mut self, header: StableNodeId) -> Self {
        self.header = Some(header);
        self
    }

    pub fn tabs(mut self, tabs: StableNodeId) -> Self {
        self.tabs = Some(tabs);
        self
    }

    pub fn body(mut self, body: StableNodeId) -> Self {
        self.body = Some(body);
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn root_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(layout.direction.unwrap_or(FlexDirection::Column));
        layout.align_items = AlignSpec::Stretch;
        layout.width = Some(layout.width.unwrap_or(LengthSpec::Fill));
        layout.height = Some(layout.height.unwrap_or(LengthSpec::Fill));
        layout.gap = Some(layout.gap.unwrap_or(LengthSpec::Px(self.gap)));
        if layout.padding.is_none()
            && layout.padding_top.is_none()
            && layout.padding_right.is_none()
            && layout.padding_bottom.is_none()
            && layout.padding_left.is_none()
        {
            layout.padding_top = Some(LengthSpec::Px(PANE_GUTTER));
            layout.padding_right = Some(LengthSpec::Px(PANE_GUTTER));
            layout.padding_bottom = Some(LengthSpec::Px(PANE_GUTTER));
            layout.padding_left = Some(LengthSpec::Px(PANE_GUTTER));
        }
        if !layout.overflow_x.clips() {
            layout.overflow_x = OverflowSpec::Hidden;
        }
        if !layout.overflow_y.clips() {
            layout.overflow_y = OverflowSpec::Hidden;
        }
        style
    }

    fn project_root(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.text(id) != Some("") {
            mutations.set_text(
                id,
                TextContent {
                    value: String::new().into(),
                },
            );
        }
        if world.standard_visual(id).is_some() {
            mutations.set_standard_visual(id, None);
        }
        project_common(
            id,
            world,
            mutations,
            &self.root_style(),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                ..AccessibilityState::default()
            },
        );
    }

    /// Pane geometry lives on the section-owned slot shell, so no other
    /// component's projection can move a host row off the content line.
    fn project_slot(
        &self,
        slot: Option<StableNodeId>,
        kind: SlotKind,
        hidden: bool,
        world: &UiWorld,
        mutations: &mut MutationQueue,
    ) {
        let Some(id) = slot else {
            return;
        };
        if world.node(id).is_none() {
            return;
        }
        let style = slot_style(kind, hidden);
        if world.node_style(id) != Some(&style) {
            mutations.set_style(id, style);
        }
    }
}

impl Default for PaneSection {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotKind {
    Header,
    Tabs,
    Body,
}

fn slot_style(kind: SlotKind, hidden: bool) -> NodeStyle {
    let mut style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.overflow_x = OverflowSpec::Hidden;
        layout.overflow_y = OverflowSpec::Hidden;
        match kind {
            SlotKind::Header => {
                layout.width = Some(LengthSpec::Fill);
                layout.flex_shrink = Some(0.0);
            }
            SlotKind::Tabs => {
                layout.width = Some(LengthSpec::Fill);
                layout.flex_shrink = Some(0.0);
            }
            SlotKind::Body => {
                layout.width = Some(LengthSpec::Fill);
                layout.height = Some(LengthSpec::Fill);
                layout.flex_grow = Some(1.0);
                layout.flex_shrink = Some(1.0);
                layout.min_width = Some(LengthSpec::Px(0.0));
                layout.min_height = Some(LengthSpec::Px(0.0));
            }
        }
        layout.hidden = hidden;
    }
    if matches!(kind, SlotKind::Tabs) {
        style.surface_padding = Some(nana_ui_core::SurfacePadding::PanelX);
    }
    style
}

/// Slot shell owned by the section. The section writes pane geometry onto it;
/// nothing else touches style, so the section stays the single style writer.
#[derive(Debug, Clone, PartialEq)]
struct PaneSectionSlot;

impl ComponentView for PaneSectionSlot {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "pane-section-slot".into(),
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
        if world.standard_visual(id).is_some() {
            mutations.set_standard_visual(id, None);
        }
        project_common(
            id,
            world,
            mutations,
            &NodeStyle::default(),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                ..AccessibilityState::default()
            },
        );
    }
}

fn document_of(context: &AppContext, id: StableNodeId) -> Result<DocumentId, FrameworkError> {
    context
        .world()
        .node(id)
        .map(|node| node.document)
        .ok_or(FrameworkError::MissingView(id))
}

fn ensure_pane_slot(
    context: &mut AppContext,
    stored: Option<StableNodeId>,
    document: DocumentId,
) -> Result<StableNodeId, FrameworkError> {
    if let Some(id) = stored.filter(|id| context.world().contains(*id)) {
        return Ok(id);
    }
    Ok(context
        .create_detached_component(document, PaneSectionSlot)?
        .stable_id())
}

fn reconcile_ids(
    context: &mut AppContext,
    parent: StableNodeId,
    ordered: &[StableNodeId],
) -> Result<bool, FrameworkError> {
    let ordered = ordered
        .iter()
        .copied()
        .filter(|id| *id != parent && context.world().contains(*id))
        .collect::<Vec<_>>();
    let current = context
        .world()
        .node(parent)
        .ok_or(FrameworkError::MissingView(parent))?
        .children
        .clone();
    if current.as_slice() == ordered.as_slice() {
        return Ok(false);
    }
    let keep = ordered.iter().copied().collect::<HashSet<_>>();
    let mut mutations = MutationQueue::new();
    for child in &current {
        if !keep.contains(child) {
            mutations.park_subtree(*child);
        }
    }
    for child in ordered {
        mutations.insert(parent, child, None);
    }
    context.commit_mutations(mutations)?;
    Ok(true)
}

impl ComponentView for PaneSection {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        slot_assembler: Some(crate::AppContext::assemble_pane_section),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "pane-section".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        self.project_root(id, world, mutations);
        self.project_slot(self.header_slot, SlotKind::Header, false, world, mutations);
        self.project_slot(
            self.tabs_slot,
            SlotKind::Tabs,
            self.tabs_hidden,
            world,
            mutations,
        );
        self.project_slot(self.body_slot, SlotKind::Body, false, world, mutations);
    }
}

impl AppContext {
    /// Create the slot shells, reparent host rows into them, and re-project.
    ///
    /// Shell identities are reused. Host content is reparented, not recreated,
    /// and stays untouched by section projection so its own component views
    /// cannot fight pane geometry.
    pub fn assemble_pane_section(
        &mut self,
        section: Entity<PaneSection>,
    ) -> Result<bool, FrameworkError> {
        let parent = section.stable_id();
        let document = document_of(self, parent)?;
        let (header, tabs, body, header_slot, tabs_slot, body_slot) =
            self.read(section, |pane| {
                (
                    pane.header,
                    pane.tabs,
                    pane.body,
                    pane.header_slot,
                    pane.tabs_slot,
                    pane.body_slot,
                )
            })?;
        let header = header.filter(|id| self.world().contains(*id));
        let tabs = tabs.filter(|id| self.world().contains(*id));
        let body = body.filter(|id| self.world().contains(*id));
        let header_slot = ensure_pane_slot(self, header_slot, document)?;
        let tabs_slot = ensure_pane_slot(self, tabs_slot, document)?;
        let body_slot = ensure_pane_slot(self, body_slot, document)?;
        self.update_component(section, |section, _| {
            section.header = header;
            section.tabs = tabs;
            section.body = body;
            section.header_slot = Some(header_slot);
            section.tabs_slot = Some(tabs_slot);
            section.body_slot = Some(body_slot);
        })?;
        let mut children = Vec::new();
        if header.is_some() {
            children.push(header_slot);
        }
        if tabs.is_some() {
            children.push(tabs_slot);
        }
        if body.is_some() {
            children.push(body_slot);
        }
        let mut changed = reconcile_ids(self, parent, &children)?;
        for (slot, content) in [(header_slot, header), (tabs_slot, tabs), (body_slot, body)] {
            if let Some(content) = content {
                changed |= reconcile_ids(self, slot, &[content])?;
            }
        }
        self.reproject_component(section)?;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn host(context: &mut AppContext, tag: &str) -> StableNodeId {
        context
            .create_view(document(), NodeKind::Element { tag: tag.into() }, ())
            .unwrap()
            .stable_id()
    }

    fn mount(context: &mut AppContext, section: PaneSection) -> Entity<PaneSection> {
        context.create_component(document(), section).unwrap()
    }

    #[test]
    fn assemble_stacks_slots_and_insets_tabs_to_the_content_line() {
        let mut context = AppContext::new();
        let header = host(&mut context, "pane-header-card");
        let tabs = host(&mut context, "tabs");
        let body = host(&mut context, "pane-body");
        let section = mount(
            &mut context,
            PaneSection::new().header(header).tabs(tabs).body(body),
        );
        context.assemble_pane_section(section).unwrap();
        let (header_slot, tabs_slot, body_slot) = context
            .read(section, |pane| {
                (
                    pane.header_slot.unwrap(),
                    pane.tabs_slot.unwrap(),
                    pane.body_slot.unwrap(),
                )
            })
            .unwrap();

        let root_children = context
            .world()
            .node(section.stable_id())
            .unwrap()
            .children
            .clone();
        assert_eq!(root_children, vec![header_slot, tabs_slot, body_slot]);
        assert_eq!(
            context.world().node(header_slot).unwrap().children,
            vec![header]
        );
        assert_eq!(
            context.world().node(tabs_slot).unwrap().children,
            vec![tabs]
        );
        assert_eq!(
            context.world().node(body_slot).unwrap().children,
            vec![body]
        );

        let root_layout = &context
            .world()
            .node_style(section.stable_id())
            .unwrap()
            .layout;
        assert_eq!(root_layout.padding_left, Some(LengthSpec::Px(PANE_GUTTER)));
        assert_eq!(root_layout.gap, Some(LengthSpec::Px(PANE_GAP)));

        // The tabs shell is what puts tab chrome on the card content line.
        let tabs_layout = &context.world().extract_nodes(&[tabs_slot])[0]
            .source_style
            .layout;
        assert_eq!(
            tabs_layout.padding_left,
            Some(LengthSpec::Px(UI_METRICS.panel_padding_x))
        );
        assert_eq!(
            tabs_layout.padding_right,
            Some(LengthSpec::Px(UI_METRICS.panel_padding_x))
        );
        assert_eq!(tabs_layout.flex_shrink, Some(0.0));

        // Card surfaces keep their own content inset; the header shell adds none.
        let header_layout = &context.world().node_style(header_slot).unwrap().layout;
        assert_eq!(header_layout.padding_left, None);
        assert_eq!(header_layout.flex_shrink, Some(0.0));

        let body_layout = &context.world().node_style(body_slot).unwrap().layout;
        assert_eq!(body_layout.flex_grow, Some(1.0));
        assert_eq!(body_layout.flex_shrink, Some(1.0));

        // Host content keeps its own style; only the shells carry pane geometry.
        assert_eq!(
            context
                .world()
                .node_style(tabs)
                .cloned()
                .unwrap_or_default()
                .layout
                .padding_left,
            None
        );
    }

    #[test]
    fn absent_slots_leave_no_shell() {
        let mut context = AppContext::new();
        let body = host(&mut context, "pane-body");
        let section = mount(&mut context, PaneSection::new().body(body));
        context.assemble_pane_section(section).unwrap();
        let body_slot = context
            .read(section, |pane| pane.body_slot.unwrap())
            .unwrap();

        let root_children = context
            .world()
            .node(section.stable_id())
            .unwrap()
            .children
            .clone();
        assert_eq!(root_children, vec![body_slot]);
        assert_eq!(
            context.world().node(body_slot).unwrap().children,
            vec![body]
        );
    }

    #[test]
    fn tabs_hidden_collapses_its_shell_without_touching_the_strip() {
        let mut context = AppContext::new();
        let tabs = host(&mut context, "tabs");
        let body = host(&mut context, "pane-body");
        let section = mount(&mut context, PaneSection::new().tabs(tabs).body(body));
        context.assemble_pane_section(section).unwrap();
        let tabs_slot = context
            .read(section, |pane| pane.tabs_slot.unwrap())
            .unwrap();
        assert!(!context.world().node_style(tabs_slot).unwrap().layout.hidden);

        context
            .update_component(section, |pane, _| pane.tabs_hidden = true)
            .unwrap();
        assert!(context.world().node_style(tabs_slot).unwrap().layout.hidden);

        context
            .update_component(section, |pane, _| pane.tabs_hidden = false)
            .unwrap();
        assert!(!context.world().node_style(tabs_slot).unwrap().layout.hidden);
        // The strip keeps its own style; the shell is the only hidden node.
        assert!(
            !context
                .world()
                .node_style(tabs)
                .cloned()
                .unwrap_or_default()
                .layout
                .hidden
        );
    }
}

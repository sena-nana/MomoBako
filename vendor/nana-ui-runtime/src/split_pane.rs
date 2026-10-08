use std::collections::HashSet;
use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, FlexDirection, JustifySpec, LengthSpec, OverflowSpec, SemanticColorRole, SplitAxis,
    SplitPaneModel, SplitPaneMutation,
};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, AppContext, ComponentView, DocumentId, Entity,
    FrameworkError, InteractionState, MutationQueue, NodeKind, NodeStyle, StableNodeId,
    TextContent, UiWorld,
};

pub(crate) const HANDLE_SIZE: f32 = nana_ui_core::space::MD;
pub(crate) const INDICATOR_SIZE: f32 = nana_ui_core::space::XXS;
pub(crate) const DEFAULT_SIZE: f32 = 240.0;
pub(crate) const DEFAULT_MIN: f32 = 120.0;
pub(crate) const DEFAULT_MAX: f32 = 800.0;

/// Two children and an 8px resize handle. Size comes from [`SplitPaneModel`].
///
/// Host applies [`SplitPaneMutation`]; this view reflects the model. Application
/// content stays in the `first` / `second` slots. [`assemble_split_pane`] wraps
/// each slot in a split-owned shell so pane geometry has a single writer: host
/// content (a [`ScrollView`](crate::ScrollView), for one) keeps projecting its
/// own node without fighting the split over pane sizing. Assign `handle` to a
/// host-created node; its optional first child is the 2px indicator. Host paint
/// set through [`SplitPane::surface`] survives re-projection; geometry stays
/// model-driven.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitPane {
    pub first: Option<StableNodeId>,
    pub second: Option<StableNodeId>,
    pub handle: Option<StableNodeId>,
    pub first_slot: Option<StableNodeId>,
    pub second_slot: Option<StableNodeId>,
    pub model: SplitPaneModel,
    pub style: NodeStyle,
}

impl SplitPane {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    /// A split with no content yet; a view gives it with `.first(..)` and
    /// `.second(..)`.
    pub fn new(model: &SplitPaneModel) -> Self {
        Self {
            first: None,
            second: None,
            handle: None,
            first_slot: None,
            second_slot: None,
            model: model.clone(),
            style: NodeStyle::default(),
        }
    }

    pub fn from_model(model: &SplitPaneModel, first: StableNodeId, second: StableNodeId) -> Self {
        Self {
            first: Some(first),
            second: Some(second),
            handle: None,
            first_slot: None,
            second_slot: None,
            model: model.clone(),
            style: NodeStyle::default(),
        }
    }

    pub fn handle(mut self, handle: StableNodeId) -> Self {
        self.handle = Some(handle);
        self
    }

    /// 语义背景色。装配重投影时保留，宿主借此承载工作区区域底色。
    pub fn surface(mut self, role: SemanticColorRole) -> Self {
        self.style.background = Some(role);
        self
    }

    pub fn apply(&mut self, mutation: SplitPaneMutation) -> bool {
        self.model.update(mutation)
    }

    fn first_is_sized(&self) -> bool {
        !self.model.from_end()
    }

    fn handle_color(&self) -> SemanticColorRole {
        if self.model.is_active() {
            SemanticColorRole::BorderStrong
        } else {
            SemanticColorRole::Border
        }
    }

    fn handle_focusable(&self) -> bool {
        true
    }

    fn root_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(split_direction(self.model.axis()));
        layout.align_items = AlignSpec::Stretch;
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Fill);
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

    /// Pane geometry lives on the split-owned slot shell, so no other
    /// component's projection can move the divider.
    fn project_pane(
        &self,
        slot: Option<StableNodeId>,
        sized: bool,
        world: &UiWorld,
        mutations: &mut MutationQueue,
    ) {
        let Some(id) = slot else {
            return;
        };
        if world.node(id).is_none() {
            return;
        }
        let horizontal = self.model.axis() == SplitAxis::Horizontal;
        let size = self.model.size();
        let (min_size, max_size) = self.model.limits();
        let mut style = NodeStyle::default();
        let layout = Arc::make_mut(&mut style.layout);
        layout.overflow_x = OverflowSpec::Hidden;
        layout.overflow_y = OverflowSpec::Hidden;
        layout.flex_grow = Some(if sized { 0.0 } else { 1.0 });
        layout.flex_shrink = Some(if sized { 0.0 } else { 1.0 });
        if horizontal {
            layout.width = Some(if sized {
                LengthSpec::Px(size)
            } else {
                LengthSpec::Fill
            });
            layout.height = Some(LengthSpec::Fill);
            if sized {
                layout.min_width = Some(LengthSpec::Px(min_size));
                layout.max_width = Some(LengthSpec::Px(max_size));
            } else {
                layout.min_width = Some(LengthSpec::Px(0.0));
                layout.max_width = None;
            }
        } else {
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(if sized {
                LengthSpec::Px(size)
            } else {
                LengthSpec::Fill
            });
            if sized {
                layout.min_height = Some(LengthSpec::Px(min_size));
                layout.max_height = Some(LengthSpec::Px(max_size));
            } else {
                layout.min_height = Some(LengthSpec::Px(0.0));
                layout.max_height = None;
            }
        }
        if world.node_style(id) != Some(&style) {
            mutations.set_style(id, style);
        }
    }

    fn project_handle(&self, world: &UiWorld, mutations: &mut MutationQueue) {
        let Some(id) = self.handle else {
            return;
        };
        if world.node(id).is_none() {
            return;
        }
        let horizontal = self.model.axis() == SplitAxis::Horizontal;
        let indicator = world
            .node(id)
            .and_then(|node| node.children.first().copied());
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
        let color = self.handle_color();
        let mut style = NodeStyle::default();
        style.background = indicator.is_none().then_some(color);
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(if horizontal {
            FlexDirection::Row
        } else {
            FlexDirection::Column
        });
        layout.align_items = AlignSpec::Center;
        layout.justify_content = JustifySpec::Center;
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        if horizontal {
            layout.width = Some(LengthSpec::Px(HANDLE_SIZE));
            layout.min_width = Some(LengthSpec::Px(HANDLE_SIZE));
            layout.max_width = Some(LengthSpec::Px(HANDLE_SIZE));
            layout.height = Some(LengthSpec::Fill);
        } else {
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(LengthSpec::Px(HANDLE_SIZE));
            layout.min_height = Some(LengthSpec::Px(HANDLE_SIZE));
            layout.max_height = Some(LengthSpec::Px(HANDLE_SIZE));
        }
        project_common(
            id,
            world,
            mutations,
            &style,
            InteractionState {
                pointer_events: true,
                focusable: self.handle_focusable(),
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::from("Resize")),
                ..AccessibilityState::default()
            },
        );
        if let Some(indicator) = indicator {
            self.project_indicator(indicator, horizontal, color, world, mutations);
        }
    }

    fn project_indicator(
        &self,
        id: StableNodeId,
        horizontal: bool,
        color: SemanticColorRole,
        world: &UiWorld,
        mutations: &mut MutationQueue,
    ) {
        if world.node(id).is_none() {
            return;
        }
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
        let mut style = NodeStyle::default();
        style.background = Some(color);
        let layout = Arc::make_mut(&mut style.layout);
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        if horizontal {
            layout.width = Some(LengthSpec::Px(INDICATOR_SIZE));
            layout.min_width = Some(LengthSpec::Px(INDICATOR_SIZE));
            layout.max_width = Some(LengthSpec::Px(INDICATOR_SIZE));
            layout.height = Some(LengthSpec::Fill);
        } else {
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(LengthSpec::Px(INDICATOR_SIZE));
            layout.min_height = Some(LengthSpec::Px(INDICATOR_SIZE));
            layout.max_height = Some(LengthSpec::Px(INDICATOR_SIZE));
        }
        project_common(
            id,
            world,
            mutations,
            &style,
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

fn point_near_box(bounds: crate::LayoutBox, x: f32, y: f32, slop: f32) -> bool {
    x >= bounds.x - slop
        && y >= bounds.y - slop
        && x <= bounds.x + bounds.width + slop
        && y <= bounds.y + bounds.height + slop
}

pub(crate) fn split_direction(axis: SplitAxis) -> FlexDirection {
    match axis {
        SplitAxis::Horizontal => FlexDirection::Row,
        SplitAxis::Vertical => FlexDirection::Column,
    }
}

impl ComponentView for SplitPane {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        slot_assembler: Some(crate::AppContext::assemble_split_pane),
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
            tag: "split-pane".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        self.project_root(id, world, mutations);
        self.project_pane(self.first_slot, self.first_is_sized(), world, mutations);
        self.project_handle(world, mutations);
        self.project_pane(self.second_slot, !self.first_is_sized(), world, mutations);
    }
}

impl AppContext {
    /// Mount the 8px resize handle between split-owned slot shells, then
    /// re-project.
    ///
    /// Created handle and slot identities are reused. Host first/second content
    /// is reparented into the slots, not recreated, and stays untouched by
    /// split projection so its own component views cannot fight pane sizing.
    pub fn assemble_split_pane(&mut self, pane: Entity<SplitPane>) -> Result<bool, FrameworkError> {
        let parent = pane.stable_id();
        let document = document_of(self, parent)?;
        let (first, second, handle, first_slot, second_slot) = self.read(pane, |pane| {
            (
                pane.first,
                pane.second,
                pane.handle,
                pane.first_slot,
                pane.second_slot,
            )
        })?;
        let first = first.filter(|id| self.world().contains(*id));
        let second = second.filter(|id| self.world().contains(*id));
        let first_slot = ensure_split_slot(self, first_slot, document)?;
        let second_slot = ensure_split_slot(self, second_slot, document)?;
        let handle = match recover_handle(
            self,
            parent,
            &[first, second, Some(first_slot), Some(second_slot)],
            handle,
        ) {
            Some(id) => id,
            None => create_split_handle(self, document)?,
        };
        self.update_component(pane, |pane, _| {
            pane.first = first;
            pane.second = second;
            pane.handle = Some(handle);
            pane.first_slot = Some(first_slot);
            pane.second_slot = Some(second_slot);
        })?;
        let mut children = Vec::new();
        if first.is_some() {
            children.push(first_slot);
        }
        children.push(handle);
        if second.is_some() {
            children.push(second_slot);
        }
        let mut changed = reconcile_ids(self, parent, &children)?;
        if let Some(first) = first {
            changed |= reconcile_ids(self, first_slot, &[first])?;
        }
        if let Some(second) = second {
            changed |= reconcile_ids(self, second_slot, &[second])?;
        }
        self.reproject_component(pane)?;
        Ok(changed)
    }

    pub fn is_split_pane(&self, id: StableNodeId) -> bool {
        self.read(crate::Entity::<SplitPane>::from_stable_id(id), |_| ())
            .is_ok()
    }

    pub fn is_split_handle(&self, id: StableNodeId) -> bool {
        self.split_for_handle(id).is_some()
    }

    pub fn split_handle_axis(&self, id: StableNodeId) -> Option<SplitAxis> {
        let entity = self.split_for_handle(id)?;
        self.read(entity, |pane| pane.model.axis()).ok()
    }

    /// Handle under the pointer, including a few pixels of slop around the 8px bar.
    pub fn split_handle_near(
        &self,
        document: crate::DocumentId,
        x: f32,
        y: f32,
    ) -> Option<StableNodeId> {
        self.split_handle_near_hit(document, x, y, self.pointer_target(document, x, y))
    }

    /// [`Self::split_handle_near`] for a caller that already hit-tested the
    /// point: `hit` is the topmost node there.
    pub fn split_handle_near_hit(
        &self,
        document: crate::DocumentId,
        x: f32,
        y: f32,
        hit: Option<StableNodeId>,
    ) -> Option<StableNodeId> {
        const SLOP: f32 = nana_ui_core::space::SM;
        if let Some(target) = hit {
            if self.is_split_handle(target) {
                return Some(target);
            }
            let mut ancestor = self.world().parent_id(target);
            while let Some(node) = ancestor {
                if self.is_split_handle(node) {
                    return Some(node);
                }
                if let Some(handle) = self.handle_of_split(node)
                    && let Some(bounds) = self.world().layout_box(handle)
                    && point_near_box(bounds, x, y, SLOP)
                {
                    return Some(handle);
                }
                ancestor = self.world().parent_id(node);
            }
        }
        // Every pointer move that is not inside a split reaches here, so walk
        // the pane index rather than the document: a tree with no split panes
        // answers without touching a node, and one with splits costs pane count
        // instead of world size.
        self.split_panes_in(document).find_map(|pane| {
            let handle = self.handle_of_split(pane)?;
            self.world()
                .layout_box(handle)
                .filter(|bounds| point_near_box(*bounds, x, y, SLOP))
                .map(|_| handle)
        })
    }

    /// Live `SplitPane` nodes in `document`, in no particular order.
    fn split_panes_in(
        &self,
        document: crate::DocumentId,
    ) -> impl Iterator<Item = StableNodeId> + '_ {
        self.world()
            .nodes_of_component(document, crate::component_descriptors::SPLIT_PANE.type_id)
    }

    fn handle_of_split(&self, id: StableNodeId) -> Option<StableNodeId> {
        let entity = self.split_pane_entity(id)?;
        self.read(entity, |pane| pane.handle).ok().flatten()
    }

    pub fn begin_split_resize(
        &mut self,
        document: crate::DocumentId,
        pointer_id: u64,
        target: StableNodeId,
        x: f32,
        y: f32,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(entity) = self.split_for_handle(target) else {
            return Ok(false);
        };
        self.update_component(entity, |pane, cx| {
            cx.mutations().request_focus(document, Some(target));
            cx.mutations().capture_pointer(pointer_id, target);
            pane.apply(SplitPaneMutation::Focus);
            pane.apply(SplitPaneMutation::ResizeStart);
            pane.apply(SplitPaneMutation::ResizeMove { x, y });
            true
        })
    }

    pub fn update_split_resize(
        &mut self,
        document: crate::DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(target) = self.world().pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        let Some(entity) = self.split_for_handle(target) else {
            return Ok(false);
        };
        self.update_component(entity, |pane, _| {
            pane.apply(SplitPaneMutation::ResizeMove { x, y })
        })
    }

    pub fn end_split_resize(
        &mut self,
        document: crate::DocumentId,
        pointer_id: u64,
        cancel: bool,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(target) = self.world().pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        let Some(entity) = self.split_for_handle(target) else {
            return Ok(false);
        };
        self.update_component(entity, |pane, cx| {
            if cancel {
                pane.apply(SplitPaneMutation::Reset);
            } else {
                pane.apply(SplitPaneMutation::ResizeEnd);
            }
            cx.mutations().release_pointer(pointer_id, target);
            true
        })
    }

    /// Throttled pointermove entry: probe the handle near the pointer and sync
    /// hover highlights at most once per frame interval.
    ///
    /// A move outside every handle slop makes [`Self::split_handle_near`] walk
    /// the whole document, and moves arrive faster than frames, so the probe is
    /// gated here. The highlight lags at most one frame. Cancel/leave paths
    /// call [`Self::sync_split_handle_hover`] directly — releasing a highlight
    /// must not be throttled.
    pub fn sync_split_handle_hover_near(
        &mut self,
        document: crate::DocumentId,
        x: f32,
        y: f32,
        now: std::time::Duration,
    ) -> Result<bool, crate::FrameworkError> {
        self.sync_split_handle_hover_near_hit(
            document,
            x,
            y,
            self.pointer_target(document, x, y),
            now,
        )
    }

    /// [`Self::sync_split_handle_hover_near`] with the point already
    /// hit-tested.
    pub fn sync_split_handle_hover_near_hit(
        &mut self,
        document: crate::DocumentId,
        x: f32,
        y: f32,
        hit: Option<StableNodeId>,
        now: std::time::Duration,
    ) -> Result<bool, crate::FrameworkError> {
        if !self.begin_split_hover_probe(document, now) {
            return Ok(false);
        }
        let near = self.split_handle_near_hit(document, x, y, hit);
        self.sync_split_handle_hover(document, near)
    }

    /// Reflect the slop-zone hover onto the split handle highlight.
    ///
    /// `None` — or a target that is not a split handle — releases every hovered
    /// split in the document: the pointermove chain has no exact handle target
    /// to key a leave off once the pointer drifts out of the handle's slop.
    pub fn sync_split_handle_hover(
        &mut self,
        document: crate::DocumentId,
        target: Option<StableNodeId>,
    ) -> Result<bool, crate::FrameworkError> {
        let active = target.filter(|id| self.is_split_handle(*id));
        // Same index, same reason as `split_handle_near`: this runs on every
        // pointer move that clears the slop zone, and the panes it has to
        // release are panes, not arbitrary nodes.
        let hovered: Vec<crate::Entity<SplitPane>> = self
            .split_panes_in(document)
            .filter_map(|id| self.split_pane_entity(id))
            .filter(|entity| {
                self.read(*entity, |pane| pane.model.hovered())
                    .unwrap_or(false)
            })
            .collect();
        let mut changed = false;
        if let Some(handle) = active
            && let Some(entity) = self.split_for_handle(handle)
        {
            changed |= self
                .update_component(entity, |pane, _| pane.apply(SplitPaneMutation::Hover(true)))?;
        }
        for entity in hovered {
            let keep_hovered = active.is_some_and(|handle| {
                self.read(entity, |pane| pane.handle == Some(handle))
                    .unwrap_or(false)
            });
            if keep_hovered {
                continue;
            }
            changed |= self.update_component(entity, |pane, _| {
                pane.apply(SplitPaneMutation::Hover(false))
            })?;
        }
        Ok(changed)
    }

    pub fn adjust_focused_split(
        &mut self,
        document: crate::DocumentId,
        direction: f32,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(focused) = self.world().focused(document) else {
            return Ok(false);
        };
        let Some(entity) = self
            .split_for_handle(focused)
            .or_else(|| self.split_pane_entity(focused))
        else {
            return Ok(false);
        };
        self.update_component(entity, |pane, _| {
            pane.apply(SplitPaneMutation::Adjust(direction))
        })
    }

    fn split_for_handle(&self, id: StableNodeId) -> Option<crate::Entity<SplitPane>> {
        let parent = self.world().parent_id(id)?;
        let entity = self.split_pane_entity(parent)?;
        self.read(entity, |pane| pane.handle == Some(id))
            .ok()
            .filter(|matches| *matches)
            .map(|_| entity)
    }

    fn split_pane_entity(&self, id: StableNodeId) -> Option<Entity<SplitPane>> {
        self.is_split_pane(id).then(|| Entity::from_stable_id(id))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct SplitHandle;

impl ComponentView for SplitHandle {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "split-handle".into(),
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
        project_common(
            id,
            world,
            mutations,
            &NodeStyle::default(),
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::from("Resize")),
                ..AccessibilityState::default()
            },
        );
    }
}

#[derive(Debug, Clone, PartialEq)]
struct SplitHandleMark;

impl ComponentView for SplitHandleMark {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "split-handle-mark".into(),
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

/// Split-owned shell around one host pane. [`SplitPane::project_pane`] writes
/// all pane geometry here; this view is projected once at creation and never
/// touches style again, so the split stays the single style writer.
#[derive(Debug, Clone, PartialEq)]
struct SplitPaneSlot;

impl ComponentView for SplitPaneSlot {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "split-pane-slot".into(),
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

fn recover_handle(
    context: &AppContext,
    parent: StableNodeId,
    owned: &[Option<StableNodeId>],
    stored: Option<StableNodeId>,
) -> Option<StableNodeId> {
    if let Some(id) = stored.filter(|id| context.world().contains(*id)) {
        return Some(id);
    }
    let extras = context
        .world()
        .node(parent)
        .map(|node| {
            node.children
                .into_iter()
                .filter(|id| !owned.contains(&Some(*id)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    extras
        .iter()
        .copied()
        .find(|id| is_handle_node(context, *id))
        .or_else(|| extras.first().copied())
}

fn ensure_split_slot(
    context: &mut AppContext,
    stored: Option<StableNodeId>,
    document: DocumentId,
) -> Result<StableNodeId, FrameworkError> {
    if let Some(id) = stored.filter(|id| context.world().contains(*id)) {
        return Ok(id);
    }
    Ok(context
        .create_detached_component(document, SplitPaneSlot)?
        .stable_id())
}

fn is_handle_node(context: &AppContext, id: StableNodeId) -> bool {
    matches!(
        context.world().node(id).map(|node| node.kind),
        Some(NodeKind::Element { tag }) if tag == "split-handle" || tag.contains("handle")
    ) || context
        .world()
        .accessibility(id)
        .is_some_and(|state| state.label.as_deref() == Some("Resize"))
}

fn create_split_handle(
    context: &mut AppContext,
    document: DocumentId,
) -> Result<StableNodeId, FrameworkError> {
    let handle = context
        .create_detached_component(document, SplitHandle)?
        .stable_id();
    let indicator = context
        .create_detached_component(document, SplitHandleMark)?
        .stable_id();
    reconcile_ids(context, handle, &[indicator])?;
    Ok(handle)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, DocumentId, MutationQueue};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn slot(context: &mut AppContext, tag: &str) -> StableNodeId {
        context
            .create_view(document(), NodeKind::Element { tag: tag.into() }, ())
            .unwrap()
            .stable_id()
    }

    fn mount(
        context: &mut AppContext,
        model: &SplitPaneModel,
        first: StableNodeId,
        second: StableNodeId,
        handle: StableNodeId,
    ) -> crate::Entity<SplitPane> {
        context
            .create_component(
                document(),
                SplitPane::from_model(model, first, second).handle(handle),
            )
            .unwrap()
    }

    enum SlotSide {
        First,
        Second,
    }

    fn pane_slot(
        context: &AppContext,
        split: crate::Entity<SplitPane>,
        side: SlotSide,
    ) -> StableNodeId {
        context
            .read(split, |pane| match side {
                SlotSide::First => pane.first_slot,
                SlotSide::Second => pane.second_slot,
            })
            .unwrap()
            .expect("assembled split owns a slot shell")
    }

    #[test]
    fn from_model_horizontal_size_goes_to_first() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let handle = slot(&mut context, "handle");
        let mut model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        model.update(SplitPaneMutation::SetSize(400.0));
        assert_eq!(model.size(), 260.0);
        let split = mount(&mut context, &model, first, second, handle);
        context.assemble_split_pane(split).unwrap();
        let first_slot = pane_slot(&context, split, SlotSide::First);
        let second_slot = pane_slot(&context, split, SlotSide::Second);

        assert_eq!(
            context.world().node(split.stable_id()).unwrap().kind,
            NodeKind::Element {
                tag: "split-pane".into(),
            }
        );
        assert_eq!(
            context
                .world()
                .accessibility(split.stable_id())
                .unwrap()
                .role,
            AccessibilityRole::Generic
        );
        let first_layout = &context.world().node_style(first_slot).unwrap().layout;
        assert_eq!(first_layout.width, Some(LengthSpec::Px(260.0)));
        assert_eq!(first_layout.min_width, Some(LengthSpec::Px(140.0)));
        assert_eq!(first_layout.max_width, Some(LengthSpec::Px(260.0)));
        assert_eq!(first_layout.height, Some(LengthSpec::Fill));
        assert_eq!(first_layout.flex_grow, Some(0.0));
        let second_layout = &context.world().node_style(second_slot).unwrap().layout;
        assert_eq!(second_layout.width, Some(LengthSpec::Fill));
        assert_eq!(second_layout.flex_grow, Some(1.0));
        assert_eq!(
            context
                .world()
                .node_style(split.stable_id())
                .unwrap()
                .layout
                .direction,
            Some(FlexDirection::Row)
        );
        // Host content keeps its own style; the split only sizes the shells.
        assert_eq!(
            context
                .world()
                .node_style(first)
                .cloned()
                .unwrap_or_default()
                .layout
                .width,
            None
        );
    }

    #[test]
    fn from_end_assigns_size_to_second() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let handle = slot(&mut context, "handle");
        let model =
            SplitPaneModel::new(SplitAxis::Horizontal, 180.0, 120.0, 240.0).with_from_end(true);
        let split = mount(&mut context, &model, first, second, handle);
        context.assemble_split_pane(split).unwrap();
        let second_slot = pane_slot(&context, split, SlotSide::Second);

        let first_slot_layout = &context
            .world()
            .node_style(pane_slot(&context, split, SlotSide::First))
            .unwrap()
            .layout;
        assert_eq!(first_slot_layout.width, Some(LengthSpec::Fill));
        assert_eq!(first_slot_layout.flex_grow, Some(1.0));
        let second_layout = &context.world().node_style(second_slot).unwrap().layout;
        assert_eq!(second_layout.width, Some(LengthSpec::Px(180.0)));
        assert_eq!(second_layout.min_width, Some(LengthSpec::Px(120.0)));
        assert_eq!(second_layout.max_width, Some(LengthSpec::Px(240.0)));
        assert_eq!(second_layout.flex_grow, Some(0.0));
    }

    #[test]
    fn assemble_reprojection_keeps_host_surface() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let handle = slot(&mut context, "handle");
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let split = context
            .create_component(
                document(),
                SplitPane::from_model(&model, first, second)
                    .handle(handle)
                    .surface(SemanticColorRole::Background),
            )
            .unwrap();

        context.assemble_split_pane(split).unwrap();

        let style = context.world().node_style(split.stable_id()).unwrap();
        assert_eq!(style.background, Some(SemanticColorRole::Background));
        assert_eq!(style.layout.direction, Some(FlexDirection::Row));
        assert_eq!(style.layout.width, Some(LengthSpec::Fill));
    }

    #[test]
    fn handle_stays_a_pointer_target_when_not_focused() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let handle = slot(&mut context, "handle");
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let split = mount(&mut context, &model, first, second, handle);

        assert!(!model.focused());
        assert_eq!(
            context.world().interaction(handle),
            Some(InteractionState {
                pointer_events: true,
                focusable: true,
            })
        );
        assert_eq!(
            context
                .world()
                .accessibility(handle)
                .unwrap()
                .label
                .as_deref(),
            Some("Resize")
        );
        let handle_layout = &context.world().node_style(handle).unwrap().layout;
        assert_eq!(handle_layout.width, Some(LengthSpec::Px(HANDLE_SIZE)));
        assert_eq!(handle_layout.height, Some(LengthSpec::Fill));

        context
            .update_component(split, |pane, _| {
                pane.apply(SplitPaneMutation::Focus);
            })
            .unwrap();
        assert_eq!(
            context.world().interaction(handle),
            Some(InteractionState {
                pointer_events: true,
                focusable: true,
            })
        );
    }

    #[test]
    fn handle_active_style_uses_border_strong() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let handle = slot(&mut context, "handle");
        let model = SplitPaneModel::new(SplitAxis::Vertical, 160.0, 80.0, 280.0);
        let split = mount(&mut context, &model, first, second, handle);

        assert_eq!(
            context.world().node_style(handle).unwrap().background,
            Some(SemanticColorRole::Border)
        );
        let handle_layout = &context.world().node_style(handle).unwrap().layout;
        assert_eq!(handle_layout.height, Some(LengthSpec::Px(HANDLE_SIZE)));
        assert_eq!(
            context
                .world()
                .node_style(split.stable_id())
                .unwrap()
                .layout
                .direction,
            Some(FlexDirection::Column)
        );

        context
            .update_component(split, |pane, _| {
                pane.apply(SplitPaneMutation::Hover(true));
            })
            .unwrap();
        assert_eq!(
            context.world().node_style(handle).unwrap().background,
            Some(SemanticColorRole::BorderStrong)
        );
        assert_eq!(
            context.world().interaction(handle),
            Some(InteractionState {
                pointer_events: true,
                focusable: true,
            })
        );
    }

    #[test]
    fn pointer_drag_resizes_the_first_pane() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let handle = slot(&mut context, "handle");
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let split = mount(&mut context, &model, first, second, handle);
        context
            .commit_mutations({
                let mut mutations = MutationQueue::new();
                mutations.insert(split.stable_id(), handle, None);
                mutations.write_layout(
                    split.stable_id(),
                    crate::LayoutBox {
                        x: 0.0,
                        y: 0.0,
                        width: 400.0,
                        height: 200.0,
                    },
                );
                mutations.write_layout(
                    handle,
                    crate::LayoutBox {
                        x: 200.0,
                        y: 0.0,
                        width: HANDLE_SIZE,
                        height: 200.0,
                    },
                );
                mutations
            })
            .unwrap();

        assert!(
            context
                .begin_split_resize(document(), 1, handle, 204.0, 20.0)
                .unwrap()
        );
        assert!(
            context
                .update_split_resize(document(), 1, 240.0, 20.0)
                .unwrap()
        );
        assert!(context.end_split_resize(document(), 1, false).unwrap());
        assert_eq!(
            context.read(split, |pane| pane.model.size()).unwrap(),
            236.0
        );
        assert!(context.world().pointer_capture(document(), 1).is_none());
    }

    #[test]
    fn nearby_pointer_still_resolves_the_resize_handle() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let handle = slot(&mut context, "handle");
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let split = mount(&mut context, &model, first, second, handle);
        context
            .commit_mutations({
                let mut mutations = MutationQueue::new();
                mutations.insert(split.stable_id(), handle, None);
                mutations.write_layout(
                    handle,
                    crate::LayoutBox {
                        x: 200.0,
                        y: 0.0,
                        width: HANDLE_SIZE,
                        height: 200.0,
                    },
                );
                mutations
            })
            .unwrap();
        context.rebuild_hit_test(document());
        assert_eq!(
            context.split_handle_near(document(), 204.0, 20.0),
            Some(handle)
        );
        assert_eq!(
            context.split_handle_near(document(), 196.0, 20.0),
            Some(handle)
        );
        assert_eq!(context.split_handle_near(document(), 40.0, 20.0), None);
    }

    #[test]
    fn idle_project_does_not_dirty() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let handle = slot(&mut context, "handle");
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let split = mount(&mut context, &model, first, second, handle);
        let _ = context.take_system_work();
        context.reproject_component(split).unwrap();
        assert!(context.take_system_work().is_empty());
    }

    fn mount_without_handle(
        context: &mut AppContext,
        model: &SplitPaneModel,
        first: StableNodeId,
        second: StableNodeId,
    ) -> crate::Entity<SplitPane> {
        context
            .create_component(document(), SplitPane::from_model(model, first, second))
            .unwrap()
    }

    #[test]
    fn assemble_creates_handle_when_none_was_set() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let split = mount_without_handle(&mut context, &model, first, second);
        assert!(context.read(split, |pane| pane.handle).unwrap().is_none());

        assert!(context.assemble_split_pane(split).unwrap());
        let handle = context
            .read(split, |pane| pane.handle)
            .unwrap()
            .expect("assemble creates a handle");
        let first_slot = pane_slot(&context, split, SlotSide::First);
        let second_slot = pane_slot(&context, split, SlotSide::Second);
        assert_eq!(
            context.world().node(split.stable_id()).unwrap().children,
            vec![first_slot, handle, second_slot]
        );
        assert_eq!(
            context.world().node(first_slot).unwrap().children,
            vec![first]
        );
        assert_eq!(
            context.world().node(second_slot).unwrap().children,
            vec![second]
        );
        assert_eq!(
            context.world().node(handle).unwrap().kind,
            NodeKind::Element {
                tag: "split-handle".into(),
            }
        );
        let handle_layout = &context.world().node_style(handle).unwrap().layout;
        assert_eq!(handle_layout.width, Some(LengthSpec::Px(HANDLE_SIZE)));
        assert_eq!(handle_layout.height, Some(LengthSpec::Fill));
    }

    #[test]
    fn assemble_is_idempotent() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let split = mount_without_handle(&mut context, &model, first, second);
        context.assemble_split_pane(split).unwrap();
        let handle = context
            .read(split, |pane| pane.handle)
            .unwrap()
            .expect("handle");
        let first_slot = pane_slot(&context, split, SlotSide::First);
        let second_slot = pane_slot(&context, split, SlotSide::Second);

        assert!(!context.assemble_split_pane(split).unwrap());
        assert_eq!(
            context.read(split, |pane| pane.handle).unwrap(),
            Some(handle)
        );
        assert_eq!(
            context.read(split, |pane| pane.first_slot).unwrap(),
            Some(first_slot)
        );
        assert_eq!(
            context.read(split, |pane| pane.second_slot).unwrap(),
            Some(second_slot)
        );
        assert_eq!(
            context.world().node(split.stable_id()).unwrap().children,
            vec![first_slot, handle, second_slot]
        );
    }

    #[test]
    fn assemble_keeps_first_second_host_slots() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let model = SplitPaneModel::new(SplitAxis::Vertical, 160.0, 80.0, 280.0);
        let split = mount_without_handle(&mut context, &model, first, second);
        context.assemble_split_pane(split).unwrap();
        let first_slot = pane_slot(&context, split, SlotSide::First);
        let second_slot = pane_slot(&context, split, SlotSide::Second);

        let (bound_first, bound_second) = context
            .read(split, |pane| (pane.first, pane.second))
            .unwrap();
        assert_eq!(bound_first, Some(first));
        assert_eq!(bound_second, Some(second));
        assert_eq!(
            context.world().node(first).unwrap().kind,
            NodeKind::Element {
                tag: "first".into(),
            }
        );
        assert_eq!(
            context.world().node(second).unwrap().kind,
            NodeKind::Element {
                tag: "second".into(),
            }
        );
        // Sizing lands on the split-owned shells; host content is untouched.
        let first_layout = &context.world().node_style(first_slot).unwrap().layout;
        assert_eq!(first_layout.height, Some(LengthSpec::Px(160.0)));
        assert_eq!(first_layout.flex_grow, Some(0.0));
        let second_layout = &context.world().node_style(second_slot).unwrap().layout;
        assert_eq!(second_layout.height, Some(LengthSpec::Fill));
        assert_eq!(second_layout.flex_grow, Some(1.0));
        assert_eq!(
            context
                .world()
                .node_style(first)
                .cloned()
                .unwrap_or_default()
                .layout
                .height,
            None
        );
    }

    #[test]
    fn scroll_view_reprojection_leaves_pane_sizing_to_the_split() {
        let mut context = AppContext::new();
        let scroll = context
            .create_detached_component(
                document(),
                crate::view_components::ScrollView::new(
                    crate::view_components::ScrollAxes::Vertical,
                )
                .style(crate::view_components::Stack::fill_column(0.0).node_style()),
            )
            .unwrap()
            .stable_id();
        let second = slot(&mut context, "second");
        let model = SplitPaneModel::new(SplitAxis::Vertical, 180.0, 72.0, 420.0);
        let split = mount_without_handle(&mut context, &model, scroll, second);
        context.assemble_split_pane(split).unwrap();
        let first_slot = pane_slot(&context, split, SlotSide::First);

        // Hover plumbing re-projects the scroll view through its own component.
        context
            .update_component(
                crate::Entity::<crate::view_components::ScrollView>::from_stable_id(scroll),
                |scroll_view, _| scroll_view.hovered = true,
            )
            .unwrap();

        // The shell keeps the model-driven pane size; the scroll view keeps
        // projecting itself without deciding the divider position.
        let slot_layout = &context.world().node_style(first_slot).unwrap().layout;
        assert_eq!(slot_layout.height, Some(LengthSpec::Px(180.0)));
        assert_eq!(slot_layout.flex_grow, Some(0.0));
        let scroll_layout = &context.world().node_style(scroll).unwrap().layout;
        assert_eq!(scroll_layout.flex_grow, Some(1.0));
        assert_eq!(scroll_layout.height, Some(LengthSpec::Fill));
        let handle = context
            .read(split, |pane| pane.handle)
            .unwrap()
            .expect("handle");
        let second_slot = pane_slot(&context, split, SlotSide::Second);
        assert_eq!(
            context.world().node(split.stable_id()).unwrap().children,
            vec![first_slot, handle, second_slot]
        );
    }

    #[test]
    fn assemble_marks_handle_as_split_handle() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let split = mount_without_handle(&mut context, &model, first, second);
        context.assemble_split_pane(split).unwrap();
        let handle = context
            .read(split, |pane| pane.handle)
            .unwrap()
            .expect("handle");
        assert!(context.is_split_handle(handle));
        assert_eq!(
            context.split_handle_axis(handle),
            Some(SplitAxis::Horizontal)
        );
    }

    #[test]
    fn leaving_the_handle_slop_releases_the_hover_highlight() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let split = mount_without_handle(&mut context, &model, first, second);
        context.assemble_split_pane(split).unwrap();
        let handle = context
            .read(split, |pane| pane.handle)
            .unwrap()
            .expect("handle");

        assert!(
            context
                .sync_split_handle_hover(document(), Some(handle))
                .unwrap()
        );
        assert!(context.read(split, |pane| pane.model.hovered()).unwrap());

        assert!(context.sync_split_handle_hover(document(), None).unwrap());
        assert!(!context.read(split, |pane| pane.model.hovered()).unwrap());
    }

    #[test]
    fn split_hover_probe_is_throttled_to_one_per_interval() {
        let mut context = AppContext::new();
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let split = mount_without_handle(&mut context, &model, first, second);
        context.assemble_split_pane(split).unwrap();
        let handle = context
            .read(split, |pane| pane.handle)
            .unwrap()
            .expect("handle");
        context
            .commit_mutations({
                let mut mutations = MutationQueue::new();
                mutations.write_layout(
                    handle,
                    crate::LayoutBox {
                        x: 200.0,
                        y: 0.0,
                        width: HANDLE_SIZE,
                        height: 200.0,
                    },
                );
                mutations
            })
            .unwrap();
        context.rebuild_hit_test(document());

        let t0 = std::time::Duration::from_millis(1_000);
        assert!(
            context
                .sync_split_handle_hover_near(document(), 204.0, 20.0, t0)
                .unwrap()
        );
        assert!(context.read(split, |pane| pane.model.hovered()).unwrap());

        // Moves within the frame interval skip the probe and keep the
        // highlight even though the pointer has left the slop.
        assert!(
            !context
                .sync_split_handle_hover_near(
                    document(),
                    600.0,
                    20.0,
                    t0 + std::time::Duration::from_millis(4)
                )
                .unwrap()
        );
        assert!(context.read(split, |pane| pane.model.hovered()).unwrap());

        // After the interval the probe runs and releases the highlight.
        assert!(
            context
                .sync_split_handle_hover_near(
                    document(),
                    600.0,
                    20.0,
                    t0 + std::time::Duration::from_millis(16)
                )
                .unwrap()
        );
        assert!(!context.read(split, |pane| pane.model.hovered()).unwrap());
    }

    #[test]
    fn hovering_a_second_split_releases_the_first() {
        let mut context = AppContext::new();
        let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        let first = slot(&mut context, "first");
        let second = slot(&mut context, "second");
        let third = slot(&mut context, "third");
        let fourth = slot(&mut context, "fourth");
        let split_a = mount_without_handle(&mut context, &model, first, second);
        let split_b = mount_without_handle(&mut context, &model, third, fourth);
        context.assemble_split_pane(split_a).unwrap();
        context.assemble_split_pane(split_b).unwrap();
        let handle_a = context
            .read(split_a, |pane| pane.handle)
            .unwrap()
            .expect("handle");
        let handle_b = context
            .read(split_b, |pane| pane.handle)
            .unwrap()
            .expect("handle");

        context
            .sync_split_handle_hover(document(), Some(handle_a))
            .unwrap();
        assert!(context.read(split_a, |pane| pane.model.hovered()).unwrap());

        context
            .sync_split_handle_hover(document(), Some(handle_b))
            .unwrap();
        assert!(!context.read(split_a, |pane| pane.model.hovered()).unwrap());
        assert!(context.read(split_b, |pane| pane.model.hovered()).unwrap());
    }
}

//! Backend-neutral dock chrome. Application pane bodies stay host-mounted slots.

mod commands;
pub use commands::{DockCommand, DockCommandOutcome, DockItemLimits};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use nana_ui_core::{
    AlignSpec, ControlSize, FlexDirection, JustifySpec, LengthSpec, OverflowSpec, PositionSpec,
    SemanticColorRole, StoreError, ViewStateStore,
};

use crate::tabs::{TabOption, Tabs};
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, AppContext, ComponentView, DocumentId, Entity,
    FrameworkError, InteractionState, InteractionStyle, LayoutBox, MutationQueue, NodeKind,
    NodeStyle, SemanticPaint, StableNodeId, TextContent, TextVerticalAlignment, UiWorld,
};

pub(crate) const DOCK_TITLE_BAR_HEIGHT: f32 =
    ControlSize::Small.height_in(nana_ui_core::UI_METRICS);
/// Splitter hit-target thickness. Host adapters must not invent a second divider size.
pub const DOCK_DIVIDER_HIT_SIZE: f32 = nana_ui_core::space::MD;
/// Inclusive lower clamp for a split's first-child share.
pub const MIN_SPLIT_RATIO: f32 = 0.05;
/// Inclusive upper clamp for a split's first-child share.
pub const MAX_SPLIT_RATIO: f32 = 0.95;
/// One keyboard/nudge step; matches [`MIN_SPLIT_RATIO`] so product and host adapters share a step.
pub const DOCK_SPLIT_KEYBOARD_STEP: f32 = MIN_SPLIT_RATIO;

const HANDLE_INDICATOR: f32 = nana_ui_core::space::XXS;
const TITLE_PADDING_X: f32 = nana_ui_core::space::SM;
const TITLE_SIZE: f32 = nana_ui_core::type_scale::HINT;
const TITLE_WEIGHT: u16 = nana_ui_core::type_scale::SEMIBOLD;
const TAB_OVERLAY_THICKNESS: f32 = nana_ui_core::space::XS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockDropZone {
    Left,
    Right,
    Top,
    Bottom,
    Tab,
}

/// Recursive dock tree. `ratio` is the first child's share after clamp.
#[derive(Debug, Clone, PartialEq)]
pub enum DockNode {
    Item {
        id: Arc<str>,
        content: Option<StableNodeId>,
    },
    Tabs {
        tabs: Vec<Arc<str>>,
        active: Arc<str>,
        contents: Vec<(Arc<str>, Option<StableNodeId>)>,
    },
    Split {
        axis: DockAxis,
        ratio: f32,
        first: Box<DockNode>,
        second: Box<DockNode>,
    },
}

impl DockNode {
    pub fn item(id: impl Into<Arc<str>>, content: Option<StableNodeId>) -> Self {
        Self::Item {
            id: id.into(),
            content,
        }
    }

    pub fn tabs(
        tabs: impl IntoIterator<Item = impl Into<Arc<str>>>,
        active: impl Into<Arc<str>>,
        contents: impl IntoIterator<Item = (impl Into<Arc<str>>, Option<StableNodeId>)>,
    ) -> Self {
        let tabs = tabs.into_iter().map(Into::into).collect::<Vec<_>>();
        let active = active.into();
        Self::Tabs {
            active: effective_active(&tabs, &active),
            tabs,
            contents: contents
                .into_iter()
                .map(|(id, content)| (id.into(), content))
                .collect(),
        }
    }

    pub fn split(axis: DockAxis, ratio: f32, first: DockNode, second: DockNode) -> Self {
        Self::Split {
            axis,
            ratio: clamp_ratio(ratio),
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    /// Make `content` the content of item `id`, wherever it is; whether
    /// that changed anything.
    pub fn bind_content(&mut self, id: &str, content: StableNodeId) -> bool {
        match self {
            Self::Item {
                id: item,
                content: slot,
            } if item.as_ref() == id => slot.replace(content) != Some(content),
            Self::Item { .. } => false,
            Self::Tabs { tabs, contents, .. } => {
                if let Some((_, slot)) = contents.iter_mut().find(|(tab, _)| tab.as_ref() == id) {
                    return slot.replace(content) != Some(content);
                }
                let Some(tab) = tabs.iter().find(|tab| tab.as_ref() == id) else {
                    return false;
                };
                contents.push((Arc::clone(tab), Some(content)));
                true
            }
            Self::Split { first, second, .. } => {
                let first = first.bind_content(id, content);
                second.bind_content(id, content) || first
            }
        }
    }

    pub fn flatten(&self) -> Vec<Arc<str>> {
        let mut ids = Vec::new();
        collect_ids(self, &mut ids);
        ids
    }

    pub fn contains(&self, id: &str) -> bool {
        match self {
            Self::Item { id: item, .. } => item.as_ref() == id,
            Self::Tabs { tabs, .. } => tabs.iter().any(|tab| tab.as_ref() == id),
            Self::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }

    pub fn clamp_ratios(&mut self) {
        match self {
            Self::Item { .. } => {}
            Self::Tabs { tabs, active, .. } => {
                *active = effective_active(tabs, active);
            }
            Self::Split {
                ratio,
                first,
                second,
                ..
            } => {
                *ratio = clamp_ratio(*ratio);
                first.clamp_ratios();
                second.clamp_ratios();
            }
        }
    }

    /// Move `dragged_id` before or after `target_id` when they share a Tabs strip.
    pub fn reorder_tab(&mut self, dragged_id: &str, target_id: &str, before: bool) -> bool {
        reorder_dock_tab(self, dragged_id, target_id, before)
    }

    /// First-child share at `path` (`[]` is this node when it is a split).
    pub fn split_ratio_at(&self, path: &[usize]) -> Option<f32> {
        if path.is_empty() {
            return match self {
                Self::Split { ratio, .. } => Some(*ratio),
                _ => None,
            };
        }
        let Self::Split { first, second, .. } = self else {
            return None;
        };
        match path[0] {
            0 => first.split_ratio_at(&path[1..]),
            1 => second.split_ratio_at(&path[1..]),
            _ => None,
        }
    }

    /// Write a clamped first-child share at `path`. Returns whether the stored ratio changed.
    pub fn set_split_ratio_at(&mut self, path: &[usize], ratio: f32) -> bool {
        if path.is_empty() {
            let Self::Split { ratio: current, .. } = self else {
                return false;
            };
            let ratio = clamp_ratio(ratio);
            if (*current - ratio).abs() <= f32::EPSILON {
                return false;
            }
            *current = ratio;
            return true;
        }
        let Self::Split { first, second, .. } = self else {
            return false;
        };
        match path[0] {
            0 => first.set_split_ratio_at(&path[1..], ratio),
            1 => second.set_split_ratio_at(&path[1..], ratio),
            _ => false,
        }
    }

    #[cfg(test)]
    fn split_ratio(&self) -> Option<f32> {
        self.split_ratio_at(&[])
    }
}

/// Host window identity for the primary dock surface.
pub const MAIN_SURFACE_ID: &str = "0";

const DEFAULT_FLOATING_X: f32 = 120.0;
const DEFAULT_FLOATING_Y: f32 = 120.0;
const DEFAULT_FLOATING_WIDTH: f32 = 360.0;
const DEFAULT_FLOATING_HEIGHT: f32 = 280.0;

/// One floating dock window. Hosts open a window and assemble a [`Dock`] on it.
#[derive(Debug, Clone, PartialEq)]
pub struct DockFloatingSurface {
    pub id: Arc<str>,
    pub root: DockNode,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl DockFloatingSurface {
    pub fn window_key(&self) -> u64 {
        dock_surface_window_key(&self.id)
    }
}

/// Deterministic window identity for a dock surface id.
///
/// Decimal strings map to themselves so [`MAIN_SURFACE_ID`] is the primary
/// window. Other ids use a stable non-zero FNV-1a key.
pub fn dock_surface_window_key(id: &str) -> u64 {
    if let Ok(parsed) = id.parse::<u64>() {
        return parsed;
    }
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    if hash == 0 { 1 } else { hash }
}

/// One surface the host should mount as an `Entity<Dock>` on that window's document.
#[derive(Debug, Clone, PartialEq)]
pub struct DockSurfaceSpec {
    pub id: Arc<str>,
    pub root: DockNode,
    pub bounds: Option<(f32, f32, f32, f32)>,
}

/// Host-applied floating window effects. Window commands stay in `nana-ui`.
#[derive(Debug, Clone, PartialEq)]
pub enum DockWorkspaceEvent {
    OpenFloating(DockFloatingSurface),
    CloseFloating(Arc<str>),
    MoveFloating {
        id: Arc<str>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    FocusFloating(Arc<str>),
}

/// Product dock authority: main tree, floating surfaces, hide set, primary id.
///
/// Split ratios are mutated with [`Self::set_split_ratio`] or live pointer
/// resize (`dock_split_ratio_from_pointer`). [`Dock`] is one in-tree surface
/// projection of this workspace; `nana_ui::dock::DockController` is a host
/// adapter, not a second live dock.
///
/// Hosts should create one `Entity<Dock>` per [`DockSurfaceSpec`] on the
/// `RuntimeDocument` that owns that window, then call [`AppContext::assemble_dock`].
#[derive(Debug, Clone, PartialEq)]
pub struct DockWorkspace {
    pub main: DockNode,
    pub floating: Vec<DockFloatingSurface>,
    /// Items omitted from [`Self::surfaces`] until [`Self::show`].
    pub hidden: Vec<Arc<str>>,
    /// Center item that cannot hide, matching Gallery `gallery.primary`.
    pub primary: Option<Arc<str>>,
    pub locked: bool,
    pub monitors: HashMap<Arc<str>, String>,
    pub item_limits: HashMap<Arc<str>, DockItemLimits>,
    next_surface: u64,
}

impl DockWorkspace {
    pub fn new(mut main: DockNode) -> Self {
        main.clamp_ratios();
        Self {
            main,
            floating: Vec::new(),
            hidden: Vec::new(),
            primary: None,
            locked: false,
            monitors: HashMap::new(),
            item_limits: HashMap::new(),
            next_surface: 1,
        }
    }

    pub fn primary(mut self, id: impl Into<Arc<str>>) -> Self {
        self.primary = Some(id.into());
        self
    }

    pub fn surfaces(&self) -> Vec<DockSurfaceSpec> {
        let mut surfaces = Vec::with_capacity(self.floating.len() + 1);
        surfaces.push(DockSurfaceSpec {
            id: Arc::from(MAIN_SURFACE_ID),
            root: filter_hidden(&self.main, &self.hidden).unwrap_or_else(|| self.main.clone()),
            bounds: None,
        });
        surfaces.extend(self.floating.iter().filter_map(|surface| {
            Some(DockSurfaceSpec {
                id: Arc::clone(&surface.id),
                root: filter_hidden(&surface.root, &self.hidden)?,
                bounds: Some((surface.x, surface.y, surface.width, surface.height)),
            })
        }));
        surfaces
    }

    pub fn hide(&mut self, id: impl AsRef<str>) -> bool {
        if self.locked
            || !self
                .item_limits
                .get(id.as_ref())
                .copied()
                .unwrap_or_default()
                .closeable
        {
            return false;
        }
        hide_id(
            &self.workspace_ids(),
            &mut self.hidden,
            self.primary.as_deref(),
            id.as_ref(),
        )
    }

    pub fn show(&mut self, id: impl AsRef<str>) -> bool {
        show_id(&mut self.hidden, id.as_ref())
    }

    pub fn is_visible(&self, id: &str) -> bool {
        !is_hidden(&self.hidden, id)
            && (self.main.contains(id)
                || self
                    .floating
                    .iter()
                    .any(|surface| surface.root.contains(id)))
    }

    fn workspace_ids(&self) -> Vec<Arc<str>> {
        let mut ids = self.main.flatten();
        for surface in &self.floating {
            ids.extend(surface.root.flatten());
        }
        ids
    }

    pub fn float_item(&mut self, id: impl AsRef<str>) -> Option<DockWorkspaceEvent> {
        self.float_item_at(
            id,
            DEFAULT_FLOATING_X,
            DEFAULT_FLOATING_Y,
            DEFAULT_FLOATING_WIDTH,
            DEFAULT_FLOATING_HEIGHT,
        )
    }

    pub fn float_item_at(
        &mut self,
        id: impl AsRef<str>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Option<DockWorkspaceEvent> {
        if self.locked
            || !self
                .item_limits
                .get(id.as_ref())
                .copied()
                .unwrap_or_default()
                .floatable
        {
            return None;
        }
        if !can_hide_id(
            &self.main.flatten(),
            &self.hidden,
            self.primary.as_deref(),
            id.as_ref(),
        ) {
            return None;
        }
        let taken = extract_item(&mut self.main, id.as_ref())?;
        self.hidden.retain(|hidden| hidden.as_ref() != id.as_ref());
        let surface = DockFloatingSurface {
            id: self.allocate_surface_id(),
            root: taken,
            x: finite(x, DEFAULT_FLOATING_X),
            y: finite(y, DEFAULT_FLOATING_Y),
            width: finite(width, DEFAULT_FLOATING_WIDTH),
            height: finite(height, DEFAULT_FLOATING_HEIGHT),
        };
        self.floating.push(surface.clone());
        Some(DockWorkspaceEvent::OpenFloating(surface))
    }

    pub fn apply(&mut self, event: DockWorkspaceEvent) {
        match event {
            DockWorkspaceEvent::OpenFloating(surface) => {
                if !self
                    .floating
                    .iter()
                    .any(|existing| existing.id == surface.id)
                {
                    self.floating.push(surface);
                }
            }
            DockWorkspaceEvent::CloseFloating(id) => {
                self.floating.retain(|surface| surface.id != id);
            }
            DockWorkspaceEvent::MoveFloating {
                id,
                x,
                y,
                width,
                height,
            } => {
                if let Some(surface) = self.floating.iter_mut().find(|surface| surface.id == id) {
                    surface.x = x;
                    surface.y = y;
                    surface.width = width;
                    surface.height = height;
                }
            }
            DockWorkspaceEvent::FocusFloating(_) => {}
        }
    }

    fn allocate_surface_id(&mut self) -> Arc<str> {
        let id = Arc::<str>::from(self.next_surface.to_string());
        self.next_surface = self.next_surface.saturating_add(1);
        id
    }

    /// Mutable tree for `surface` (`MAIN_SURFACE_ID` is `main`).
    pub fn surface_root_mut(&mut self, surface: &str) -> Option<&mut DockNode> {
        if surface == MAIN_SURFACE_ID {
            Some(&mut self.main)
        } else {
            self.floating
                .iter_mut()
                .find(|item| item.id.as_ref() == surface)
                .map(|item| &mut item.root)
        }
    }

    /// Product split-ratio mutation. Host adapters must not apply a second formula.
    pub fn set_split_ratio(&mut self, surface: &str, path: &[usize], ratio: f32) -> bool {
        if self.locked {
            return false;
        }
        self.surface_root_mut(surface)
            .is_some_and(|root| root.set_split_ratio_at(path, ratio))
    }

    /// Persist the product tree using historical `DockLayout` JSON field names.
    ///
    /// Host slot contents and [`Self::primary`] are not stored. `locked` and
    /// per-surface `monitor` are retained host metadata and round-trip unchanged.
    pub fn layout_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&DockWorkspacePersist::from(self))
    }

    /// Restore a product tree from historical `DockLayout` JSON.
    ///
    /// Host slot contents and [`Self::primary`] are not in the JSON. Use
    /// [`Self::restore_layout_json`] to keep the current primary.
    pub fn from_layout_json(value: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str::<DockWorkspacePersist>(value).map(Self::from)
    }

    /// Replace the tree from JSON while keeping [`Self::primary`].
    pub fn restore_layout_json(&mut self, value: &str) -> Result<(), serde_json::Error> {
        let mut restored = Self::from_layout_json(value)?;
        restored.primary = self.primary.clone();
        restored.item_limits = self.item_limits.clone();
        *self = restored;
        Ok(())
    }

    /// Write this workspace into the canonical view-state namespace.
    pub fn save_to_store(&self, store: &ViewStateStore, key: &str) -> Result<(), StoreError> {
        let json = self
            .layout_json()
            .map_err(|error| StoreError::new(error.to_string()))?;
        store.save("dock", key, 1, json)
    }

    /// Restore from the canonical view-state namespace. Returns `false` when absent.
    pub fn restore_from_store(
        &mut self,
        store: &ViewStateStore,
        key: &str,
    ) -> Result<bool, StoreError> {
        let Some(restored) =
            store.restore("dock", key, 1, |raw| Self::from_layout_json(raw).ok())?
        else {
            return Ok(false);
        };
        self.restore_layout_json(
            &restored
                .layout_json()
                .map_err(|e| StoreError::new(e.to_string()))?,
        )
        .map_err(|e| StoreError::new(e.to_string()))?;
        Ok(true)
    }
}

const DOCK_WORKSPACE_LAYOUT_VERSION: u8 = 1;

/// JSON projection of [`DockWorkspace`].
///
/// Field names match historical host-adapter `DockLayout` JSON (`version`,
/// tagged `kind` nodes, numeric `surface`, `bounds`, `monitor`, `hidden`,
/// `locked`). Live slot contents and [`DockWorkspace::primary`] are not
/// persisted. `locked` and `monitor` are persist extras for the host adapter;
/// [`DockWorkspace::from_layout_json`] retains them in workspace host metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockWorkspacePersist {
    pub version: u8,
    pub main: DockNodePersist,
    #[serde(default)]
    pub floating: Vec<DockFloatingPersist>,
    #[serde(default)]
    pub hidden: Vec<String>,
    #[serde(default)]
    pub locked: bool,
}

/// Persisted recursive dock tree. Same tagged `kind` as historical `DockLayout`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DockNodePersist {
    Item {
        id: String,
    },
    Tabs {
        tabs: Vec<String>,
        active: String,
    },
    Split {
        axis: DockAxis,
        ratio: f32,
        first: Box<DockNodePersist>,
        second: Box<DockNodePersist>,
    },
}

/// Persisted floating surface. `surface` is the historical numeric identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockFloatingPersist {
    pub surface: u64,
    pub root: DockNodePersist,
    pub bounds: DockBoundsPersist,
    #[serde(default)]
    pub monitor: Option<String>,
}

/// Persisted floating bounds. Same fields as historical `DockLayout` JSON.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DockBoundsPersist {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl DockWorkspacePersist {
    pub fn from_workspace(workspace: &DockWorkspace) -> Self {
        Self::from(workspace)
    }

    pub fn into_workspace(self) -> DockWorkspace {
        DockWorkspace::from(self)
    }
}

impl From<&DockWorkspace> for DockWorkspacePersist {
    fn from(workspace: &DockWorkspace) -> Self {
        Self {
            version: DOCK_WORKSPACE_LAYOUT_VERSION,
            main: DockNodePersist::from(&workspace.main),
            floating: workspace
                .floating
                .iter()
                .map(|surface| {
                    let mut persist = DockFloatingPersist::from(surface);
                    persist.monitor = workspace.monitors.get(&surface.id).cloned();
                    persist
                })
                .collect(),
            hidden: workspace.hidden.iter().map(|id| id.to_string()).collect(),
            locked: workspace.locked,
        }
    }
}

impl From<DockWorkspacePersist> for DockWorkspace {
    fn from(persist: DockWorkspacePersist) -> Self {
        let mut next_surface = 1_u64;
        let monitors = persist
            .floating
            .iter()
            .filter_map(|item| {
                item.monitor
                    .clone()
                    .map(|monitor| (Arc::from(item.surface.to_string()), monitor))
            })
            .collect();
        let floating = persist
            .floating
            .into_iter()
            .map(|item| {
                next_surface = next_surface.max(item.surface.saturating_add(1));
                let mut root = DockNode::from(item.root);
                root.clamp_ratios();
                DockFloatingSurface {
                    id: Arc::from(item.surface.to_string()),
                    root,
                    x: item.bounds.x,
                    y: item.bounds.y,
                    width: item.bounds.width,
                    height: item.bounds.height,
                }
            })
            .collect();
        let mut workspace = DockWorkspace::new(DockNode::from(persist.main));
        workspace.locked = persist.locked;
        workspace.monitors = monitors;
        workspace.floating = floating;
        workspace.hidden = persist.hidden.into_iter().map(Arc::from).collect();
        workspace.next_surface = next_surface;
        workspace
    }
}

impl From<&DockNode> for DockNodePersist {
    fn from(node: &DockNode) -> Self {
        match node {
            DockNode::Item { id, .. } => Self::Item { id: id.to_string() },
            DockNode::Tabs { tabs, active, .. } => Self::Tabs {
                tabs: tabs.iter().map(|id| id.to_string()).collect(),
                active: active.to_string(),
            },
            DockNode::Split {
                axis,
                ratio,
                first,
                second,
            } => Self::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(Self::from(first.as_ref())),
                second: Box::new(Self::from(second.as_ref())),
            },
        }
    }
}

impl From<DockNodePersist> for DockNode {
    fn from(node: DockNodePersist) -> Self {
        match node {
            DockNodePersist::Item { id } => Self::item(id, None),
            DockNodePersist::Tabs { tabs, active } => {
                let contents = tabs.iter().map(|id| (id.clone(), None)).collect::<Vec<_>>();
                Self::tabs(tabs, active, contents)
            }
            DockNodePersist::Split {
                axis,
                ratio,
                first,
                second,
            } => Self::split(axis, ratio, Self::from(*first), Self::from(*second)),
        }
    }
}

impl From<&DockFloatingSurface> for DockFloatingPersist {
    fn from(surface: &DockFloatingSurface) -> Self {
        Self {
            surface: dock_surface_window_key(&surface.id),
            root: DockNodePersist::from(&surface.root),
            bounds: DockBoundsPersist {
                x: surface.x,
                y: surface.y,
                width: surface.width,
                height: surface.height,
            },
            monitor: None,
        }
    }
}

fn collect_ids(node: &DockNode, output: &mut Vec<Arc<str>>) {
    match node {
        DockNode::Item { id, .. } => output.push(Arc::clone(id)),
        DockNode::Tabs { tabs, .. } => output.extend(tabs.iter().cloned()),
        DockNode::Split { first, second, .. } => {
            collect_ids(first, output);
            collect_ids(second, output);
        }
    }
}

fn collect_contents(node: &DockNode, output: &mut Vec<(Arc<str>, Option<StableNodeId>)>) {
    match node {
        DockNode::Item { id, content } => output.push((Arc::clone(id), *content)),
        DockNode::Tabs { contents, .. } => output.extend(contents.iter().cloned()),
        DockNode::Split { first, second, .. } => {
            collect_contents(first, output);
            collect_contents(second, output);
        }
    }
}

fn host_content_ids(node: &DockNode) -> HashSet<StableNodeId> {
    let mut ids = HashSet::new();
    let mut slots = Vec::new();
    collect_contents(node, &mut slots);
    for (_, content) in slots {
        if let Some(id) = content {
            ids.insert(id);
        }
    }
    ids
}

fn effective_active(tabs: &[Arc<str>], active: &Arc<str>) -> Arc<str> {
    if tabs.iter().any(|tab| tab == active) {
        Arc::clone(active)
    } else {
        tabs.first().cloned().unwrap_or_else(|| Arc::clone(active))
    }
}

pub fn clamp_ratio(ratio: f32) -> f32 {
    finite(ratio, 0.5).clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)
}

/// Length remaining for both children after the divider. Never negative.
pub fn dock_split_available(extent: f32) -> f32 {
    (extent - DOCK_DIVIDER_HIT_SIZE).max(0.0)
}

/// First-child and second-child lengths for a split of `extent` at `ratio`.
pub fn dock_split_child_lengths(ratio: f32, extent: f32) -> (f32, f32) {
    let available = dock_split_available(extent);
    let first = available * clamp_ratio(ratio);
    (first, (available - first).max(0.0))
}

/// Pointer resize: initial ratio plus absolute scalar delta over available length.
///
/// `available` is the split extent minus [`DOCK_DIVIDER_HIT_SIZE`]. This is the
/// only live split-ratio formula; `nana_ui::dock::DockController` must call it.
pub fn dock_split_ratio_from_pointer(
    start_ratio: f32,
    start: f32,
    position: f32,
    available: f32,
) -> f32 {
    clamp_ratio(start_ratio + (position - start) / available.max(1.0))
}

/// Keyboard/nudge: [`DOCK_SPLIT_KEYBOARD_STEP`] per unit `steps`.
pub fn dock_nudge_split_ratio(ratio: f32, steps: f32) -> f32 {
    if !steps.is_finite() || steps == 0.0 {
        return clamp_ratio(ratio);
    }
    clamp_ratio(ratio + steps * DOCK_SPLIT_KEYBOARD_STEP)
}

fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn is_hidden(hidden: &[Arc<str>], id: &str) -> bool {
    hidden.iter().any(|item| item.as_ref() == id)
}

fn can_hide_id(ids: &[Arc<str>], hidden: &[Arc<str>], primary: Option<&str>, id: &str) -> bool {
    if primary == Some(id) || is_hidden(hidden, id) || !ids.iter().any(|item| item.as_ref() == id) {
        return false;
    }
    ids.iter()
        .filter(|item| !is_hidden(hidden, item.as_ref()))
        .count()
        > 1
}

fn hide_id(ids: &[Arc<str>], hidden: &mut Vec<Arc<str>>, primary: Option<&str>, id: &str) -> bool {
    if !can_hide_id(ids, hidden, primary, id) {
        return false;
    }
    hidden.push(Arc::from(id));
    true
}

fn show_id(hidden: &mut Vec<Arc<str>>, id: &str) -> bool {
    let before = hidden.len();
    hidden.retain(|item| item.as_ref() != id);
    hidden.len() != before
}

fn filter_hidden(node: &DockNode, hidden: &[Arc<str>]) -> Option<DockNode> {
    match node {
        DockNode::Item { id, content } => {
            (!is_hidden(hidden, id)).then(|| DockNode::item(Arc::clone(id), *content))
        }
        DockNode::Tabs {
            tabs,
            active,
            contents,
        } => {
            let visible = tabs
                .iter()
                .filter(|id| !is_hidden(hidden, id.as_ref()))
                .cloned()
                .collect::<Vec<_>>();
            match visible.as_slice() {
                [] => None,
                [_] => {
                    let only = visible.into_iter().next().expect("one visible tab");
                    let content = contents
                        .iter()
                        .find(|(id, _)| id == &only)
                        .and_then(|(_, content)| *content);
                    Some(DockNode::item(only, content))
                }
                _ => {
                    let pairs = visible
                        .iter()
                        .map(|id| {
                            let content = contents
                                .iter()
                                .find(|(tab, _)| tab == id)
                                .and_then(|(_, content)| *content);
                            (Arc::clone(id), content)
                        })
                        .collect::<Vec<_>>();
                    Some(DockNode::tabs(visible, Arc::clone(active), pairs))
                }
            }
        }
        DockNode::Split {
            axis,
            ratio,
            first,
            second,
        } => match (filter_hidden(first, hidden), filter_hidden(second, hidden)) {
            (Some(first), Some(second)) => Some(DockNode::split(*axis, *ratio, first, second)),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        },
    }
}

fn insert_dock_item(root: &mut DockNode, target: &str, item: DockNode, zone: DockDropZone) -> bool {
    match zone {
        DockDropZone::Tab => insert_tab(root, target, item),
        zone => insert_split(root, target, item, zone),
    }
}

fn insert_tab(root: &mut DockNode, target: &str, item: DockNode) -> bool {
    let DockNode::Item { id, content } = item.clone() else {
        return false;
    };
    match root {
        DockNode::Item {
            id: current,
            content: current_content,
        } if current.as_ref() == target => {
            let current = Arc::clone(current);
            let current_content = *current_content;
            *root = DockNode::tabs(
                [Arc::clone(&current), Arc::clone(&id)],
                Arc::clone(&id),
                [(current, current_content), (id, content)],
            );
            true
        }
        DockNode::Tabs {
            tabs,
            active,
            contents,
        } if tabs.iter().any(|tab| tab.as_ref() == target) => {
            if !tabs.iter().any(|tab| tab == &id) {
                tabs.push(Arc::clone(&id));
                contents.push((Arc::clone(&id), content));
            }
            *active = id;
            true
        }
        DockNode::Split { first, second, .. } => {
            insert_tab(first, target, item.clone()) || insert_tab(second, target, item)
        }
        _ => false,
    }
}

fn reorder_dock_tab(root: &mut DockNode, dragged_id: &str, target_id: &str, before: bool) -> bool {
    match root {
        DockNode::Tabs { tabs, contents, .. } => {
            if !reorder_sibling_ids(tabs, dragged_id, target_id, before) {
                return false;
            }
            contents
                .sort_by_key(|(id, _)| tabs.iter().position(|tab| tab == id).unwrap_or(usize::MAX));
            true
        }
        DockNode::Split { first, second, .. } => {
            reorder_dock_tab(first, dragged_id, target_id, before)
                || reorder_dock_tab(second, dragged_id, target_id, before)
        }
        DockNode::Item { .. } => false,
    }
}

fn reorder_sibling_ids(
    ids: &mut Vec<Arc<str>>,
    dragged_id: &str,
    target_id: &str,
    before: bool,
) -> bool {
    if dragged_id == target_id {
        return false;
    }
    let Some(from) = ids.iter().position(|id| id.as_ref() == dragged_id) else {
        return false;
    };
    if !ids.iter().any(|id| id.as_ref() == target_id) {
        return false;
    }
    let item = ids.remove(from);
    let target = ids
        .iter()
        .position(|id| id.as_ref() == target_id)
        .expect("target remains after removing dragged sibling");
    let insert_at = if before { target } else { target + 1 };
    let changed = from != insert_at;
    ids.insert(insert_at, item);
    changed
}

fn insert_split(root: &mut DockNode, target: &str, item: DockNode, zone: DockDropZone) -> bool {
    if root.contains(target) && matches!(root, DockNode::Item { .. } | DockNode::Tabs { .. }) {
        let previous = root.clone();
        let (axis, first, second) = match zone {
            DockDropZone::Left => (DockAxis::Horizontal, item, previous),
            DockDropZone::Right => (DockAxis::Horizontal, previous, item),
            DockDropZone::Top => (DockAxis::Vertical, item, previous),
            DockDropZone::Bottom => (DockAxis::Vertical, previous, item),
            DockDropZone::Tab => return insert_tab(root, target, item),
        };
        *root = DockNode::split(axis, 0.5, first, second);
        return true;
    }
    match root {
        DockNode::Split { first, second, .. } => {
            insert_split(first, target, item.clone(), zone)
                || insert_split(second, target, item, zone)
        }
        _ => false,
    }
}

fn point_near_box(bounds: LayoutBox, x: f32, y: f32, slop: f32) -> bool {
    x >= bounds.x - slop
        && y >= bounds.y - slop
        && x <= bounds.x + bounds.width + slop
        && y <= bounds.y + bounds.height + slop
}

fn chrome_has_handle(chrome: &DockChrome, handle: StableNodeId) -> bool {
    match chrome {
        DockChrome::Split {
            handle: current,
            first,
            second,
            ..
        } => {
            *current == handle
                || chrome_has_handle(first, handle)
                || chrome_has_handle(second, handle)
        }
        DockChrome::Item { .. } | DockChrome::Tabs { .. } => false,
    }
}

/// The first node `pick` answers for, depth first, splits before their panes.
fn chrome_find(
    chrome: &DockChrome,
    pick: &mut impl FnMut(&DockChrome) -> Option<StableNodeId>,
) -> Option<StableNodeId> {
    if let Some(found) = pick(chrome) {
        return Some(found);
    }
    match chrome {
        DockChrome::Split { first, second, .. } => {
            chrome_find(first, pick).or_else(|| chrome_find(second, pick))
        }
        DockChrome::Item { .. } | DockChrome::Tabs { .. } => None,
    }
}

fn chrome_has_strip(chrome: &DockChrome, strip: StableNodeId) -> bool {
    match chrome {
        DockChrome::Tabs { strip: current, .. } => *current == strip,
        DockChrome::Split { first, second, .. } => {
            chrome_has_strip(first, strip) || chrome_has_strip(second, strip)
        }
        DockChrome::Item { .. } => false,
    }
}

fn chrome_has_title(chrome: &DockChrome, title: StableNodeId) -> bool {
    match chrome {
        DockChrome::Item { title: current, .. } => *current == title,
        DockChrome::Split { first, second, .. } => {
            chrome_has_title(first, title) || chrome_has_title(second, title)
        }
        DockChrome::Tabs { .. } => false,
    }
}

fn split_info_for_handle(
    chrome: &DockChrome,
    node: &DockNode,
    handle: StableNodeId,
) -> Option<(StableNodeId, DockAxis, f32, Vec<Arc<str>>)> {
    match (chrome, node) {
        (
            DockChrome::Split {
                handle: current,
                frame,
                first,
                second,
                ..
            },
            DockNode::Split {
                axis,
                ratio,
                first: first_node,
                second: second_node,
            },
        ) => {
            if *current == handle {
                return Some((*frame, *axis, *ratio, first_node.flatten()));
            }
            split_info_for_handle(first, first_node, handle)
                .or_else(|| split_info_for_handle(second, second_node, handle))
        }
        _ => None,
    }
}

fn set_split_ratio_for_first_ids(node: &mut DockNode, first_ids: &[Arc<str>], ratio: f32) -> bool {
    match node {
        DockNode::Split {
            first,
            second,
            ratio: current,
            ..
        } => {
            if set_split_ratio_for_first_ids(first, first_ids, ratio)
                || set_split_ratio_for_first_ids(second, first_ids, ratio)
            {
                return true;
            }
            let first_flat = first.flatten();
            let owns_first = !first_ids.is_empty()
                && first_ids
                    .iter()
                    .all(|id| first_flat.iter().any(|item| item == id))
                && first_ids.iter().all(|id| !second.contains(id.as_ref()));
            if !owns_first {
                return false;
            }
            let next = clamp_ratio(ratio);
            if (*current - next).abs() <= f32::EPSILON {
                return false;
            }
            *current = next;
            true
        }
        _ => false,
    }
}

fn title_item_id(chrome: &DockChrome, node: &DockNode, title: StableNodeId) -> Option<Arc<str>> {
    match (chrome, node) {
        (DockChrome::Item { title: current, .. }, DockNode::Item { id, .. })
            if *current == title =>
        {
            Some(Arc::clone(id))
        }
        (
            DockChrome::Split { first, second, .. },
            DockNode::Split {
                first: first_node,
                second: second_node,
                ..
            },
        ) => title_item_id(first, first_node, title)
            .or_else(|| title_item_id(second, second_node, title)),
        _ => None,
    }
}

fn strip_containing_item(chrome: &DockChrome, node: &DockNode, id: &str) -> Option<StableNodeId> {
    match (chrome, node) {
        (DockChrome::Tabs { strip, .. }, DockNode::Tabs { tabs, .. })
            if tabs.iter().any(|tab| tab.as_ref() == id) =>
        {
            Some(*strip)
        }
        (
            DockChrome::Split { first, second, .. },
            DockNode::Split {
                first: first_node,
                second: second_node,
                ..
            },
        ) => strip_containing_item(first, first_node, id)
            .or_else(|| strip_containing_item(second, second_node, id)),
        _ => None,
    }
}

fn strip_for_tabs(chrome: &DockChrome, node: &DockNode, strip: StableNodeId) -> Option<Arc<str>> {
    match (chrome, node) {
        (DockChrome::Tabs { strip: current, .. }, DockNode::Tabs { active, tabs, .. })
            if *current == strip =>
        {
            Some(effective_active(tabs, active))
        }
        (
            DockChrome::Split { first, second, .. },
            DockNode::Split {
                first: first_node,
                second: second_node,
                ..
            },
        ) => strip_for_tabs(first, first_node, strip)
            .or_else(|| strip_for_tabs(second, second_node, strip)),
        _ => None,
    }
}

fn collect_drop_leaves(
    chrome: &DockChrome,
    node: &DockNode,
    output: &mut Vec<(Arc<str>, StableNodeId)>,
) {
    match (chrome, node) {
        (DockChrome::Item { frame, .. }, DockNode::Item { id, .. }) => {
            output.push((Arc::clone(id), *frame));
        }
        (DockChrome::Tabs { frame, .. }, DockNode::Tabs { tabs, .. }) => {
            if let Some(id) = tabs.first() {
                output.push((Arc::clone(id), *frame));
            }
        }
        (
            DockChrome::Split { first, second, .. },
            DockNode::Split {
                first: first_node,
                second: second_node,
                ..
            },
        ) => {
            collect_drop_leaves(first, first_node, output);
            collect_drop_leaves(second, second_node, output);
        }
        _ => {}
    }
}

fn drop_zone_at(bounds: LayoutBox, x: f32, y: f32) -> Option<DockDropZone> {
    if bounds.width <= 0.0 || bounds.height <= 0.0 || !bounds.contains(x, y) {
        return None;
    }
    let local_x = (x - bounds.x) / bounds.width;
    let local_y = (y - bounds.y) / bounds.height;
    Some(if local_x <= 0.25 {
        DockDropZone::Left
    } else if local_x >= 0.75 {
        DockDropZone::Right
    } else if local_y <= 0.25 {
        DockDropZone::Top
    } else if local_y >= 0.75 {
        DockDropZone::Bottom
    } else {
        DockDropZone::Tab
    })
}

#[derive(Debug, Clone, PartialEq)]
enum DockChrome {
    Item {
        frame: StableNodeId,
        title: StableNodeId,
        overlay: Option<StableNodeId>,
    },
    Tabs {
        frame: StableNodeId,
        strip: StableNodeId,
        overlay: Option<StableNodeId>,
    },
    Split {
        frame: StableNodeId,
        handle: StableNodeId,
        indicator: StableNodeId,
        first: Box<DockChrome>,
        second: Box<DockChrome>,
    },
}

impl DockChrome {
    fn frame(&self) -> StableNodeId {
        match self {
            Self::Item { frame, .. } | Self::Tabs { frame, .. } | Self::Split { frame, .. } => {
                *frame
            }
        }
    }

    #[cfg(test)]
    fn split_children(&self) -> Option<(StableNodeId, StableNodeId, StableNodeId)> {
        match self {
            Self::Split {
                handle,
                first,
                second,
                ..
            } => Some((first.frame(), *handle, second.frame())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DockSplitResize {
    handle: StableNodeId,
    frame: StableNodeId,
    axis: DockAxis,
    start: Option<f32>,
    start_ratio: f32,
    first_ids: Vec<Arc<str>>,
}

#[derive(Debug, Clone, PartialEq)]
struct DockItemDrag {
    id: Arc<str>,
    reorder: Option<(Arc<str>, bool)>,
}

/// Recursive split / tabs / item surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Dock {
    pub root: DockNode,
    pub drop_target: Option<(Arc<str>, DockDropZone)>,
    pub locked: bool,
    pub titles: Vec<(Arc<str>, Arc<str>)>,
    pub style: NodeStyle,
    /// Items omitted from assemble / flatten until [`Self::show`].
    pub hidden: Vec<Arc<str>>,
    /// Center item that cannot hide, matching Gallery `gallery.primary`.
    pub primary: Option<Arc<str>>,
    chrome: Option<DockChrome>,
    split_resize: Option<DockSplitResize>,
    item_drag: Option<DockItemDrag>,
}

impl Dock {
    pub fn new(mut root: DockNode) -> Self {
        root.clamp_ratios();
        Self {
            root,
            drop_target: None,
            locked: false,
            titles: Vec::new(),
            style: NodeStyle::default(),
            hidden: Vec::new(),
            primary: None,
            chrome: None,
            split_resize: None,
            item_drag: None,
        }
    }

    pub fn primary(mut self, id: impl Into<Arc<str>>) -> Self {
        self.primary = Some(id.into());
        self
    }

    pub fn drop_target(mut self, id: impl Into<Arc<str>>, zone: DockDropZone) -> Self {
        self.drop_target = Some((id.into(), zone));
        self
    }

    pub fn locked(mut self, locked: bool) -> Self {
        self.locked = locked;
        self
    }

    pub fn title(mut self, id: impl Into<Arc<str>>, title: impl Into<Arc<str>>) -> Self {
        let id = id.into();
        let title = title.into();
        if let Some(existing) = self.titles.iter_mut().find(|(key, _)| key == &id) {
            existing.1 = title;
        } else {
            self.titles.push((id, title));
        }
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn flatten(&self) -> Vec<Arc<str>> {
        self.visible_root()
            .map(|node| node.flatten())
            .unwrap_or_default()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.root.contains(id)
    }

    pub fn visible_root(&self) -> Option<DockNode> {
        filter_hidden(&self.root, &self.hidden)
    }

    pub fn hide(&mut self, id: impl AsRef<str>) -> bool {
        let changed = hide_id(
            &self.root.flatten(),
            &mut self.hidden,
            self.primary.as_deref(),
            id.as_ref(),
        );
        if changed {
            self.clear_drop_if_missing();
        }
        changed
    }

    pub fn show(&mut self, id: impl AsRef<str>) -> bool {
        show_id(&mut self.hidden, id.as_ref())
    }

    pub fn is_visible(&self, id: &str) -> bool {
        self.contains(id) && !is_hidden(&self.hidden, id)
    }

    /// Move a tab before or after a sibling in the same strip. Locked docks refuse.
    pub fn reorder_tab(
        &mut self,
        dragged_id: impl AsRef<str>,
        target_id: impl AsRef<str>,
        before: bool,
    ) -> bool {
        if self.locked {
            return false;
        }
        reorder_dock_tab(
            &mut self.root,
            dragged_id.as_ref(),
            target_id.as_ref(),
            before,
        )
    }

    /// Move `id` onto `target`'s `zone`. Fails when extract or insert cannot apply.
    pub fn retarget(
        &mut self,
        id: impl AsRef<str>,
        target: impl AsRef<str>,
        zone: DockDropZone,
    ) -> bool {
        let id = id.as_ref();
        let target = target.as_ref();
        if id == target || self.primary.as_deref() == Some(id) || !self.can_take(id) {
            return false;
        }
        let before = self.root.clone();
        let Some(taken) = extract_item(&mut self.root, id) else {
            return false;
        };
        self.hidden.retain(|hidden| hidden.as_ref() != id);
        if !insert_dock_item(&mut self.root, target, taken, zone) {
            self.root = before;
            return false;
        }
        self.clear_drop_if_missing();
        true
    }

    fn can_take(&self, id: &str) -> bool {
        can_hide_id(
            &self.root.flatten(),
            &self.hidden,
            self.primary.as_deref(),
            id,
        )
    }

    fn clear_drop_if_missing(&mut self) {
        if let Some((target, _)) = &self.drop_target
            && !self.is_visible(target.as_ref())
        {
            self.drop_target = None;
        }
    }

    /// Remove `id` from this surface. Fails when the id is missing or last.
    pub fn float_item(&mut self, id: impl AsRef<str>) -> Option<DockFloatingSurface> {
        self.float_item_at(
            id,
            DEFAULT_FLOATING_X,
            DEFAULT_FLOATING_Y,
            DEFAULT_FLOATING_WIDTH,
            DEFAULT_FLOATING_HEIGHT,
        )
    }

    pub fn float_item_at(
        &mut self,
        id: impl AsRef<str>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Option<DockFloatingSurface> {
        let id = id.as_ref();
        if !self.can_take(id) {
            return None;
        }
        let taken = extract_item(&mut self.root, id)?;
        self.hidden.retain(|hidden| hidden.as_ref() != id);
        self.clear_drop_if_missing();
        Some(DockFloatingSurface {
            id: Arc::from(id),
            root: taken,
            x: finite(x, DEFAULT_FLOATING_X),
            y: finite(y, DEFAULT_FLOATING_Y),
            width: finite(width, DEFAULT_FLOATING_WIDTH),
            height: finite(height, DEFAULT_FLOATING_HEIGHT),
        })
    }

    fn title_for(&self, id: &str) -> Arc<str> {
        self.titles
            .iter()
            .find(|(key, _)| key.as_ref() == id)
            .map(|(_, title)| Arc::clone(title))
            .unwrap_or_else(|| Arc::from(id))
    }

    fn drop_for(&self, id: &str) -> Option<DockDropZone> {
        self.drop_target
            .as_ref()
            .filter(|(target, _)| target.as_ref() == id)
            .map(|(_, zone)| *zone)
    }

    fn node_drop(&self, node: &DockNode) -> Option<DockDropZone> {
        match node {
            DockNode::Item { id, .. } => self.drop_for(id),
            DockNode::Tabs { tabs, .. } => tabs.iter().find_map(|id| self.drop_for(id)),
            DockNode::Split { .. } => None,
        }
    }
}

impl ComponentView for Dock {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        hooks: Some(|hooks| hooks.dock = Some(&DOCK_HOOKS)),
        slot_assembler: Some(crate::AppContext::assemble_dock_panels),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element { tag: "dock".into() }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let mut root = self.visible_root().unwrap_or_else(|| self.root.clone());
        root.clamp_ratios();
        if let Some(chrome) = &self.chrome {
            project_chrome(self, chrome, &root, world, mutations);
        } else {
            project_common(
                id,
                world,
                mutations,
                &dock_root_style(&root, &self.style),
                InteractionState {
                    pointer_events: false,
                    focusable: false,
                },
                dock_root_accessibility(&root),
            );
            project_content_slots(&root, None, id, world, mutations);
        }
    }
}

fn dock_root_style(root: &DockNode, base: &NodeStyle) -> NodeStyle {
    let mut style = fill_surface(base.clone());
    let layout = Arc::make_mut(&mut style.layout);
    match root {
        DockNode::Split { axis, .. } => {
            layout.direction = Some(split_direction(*axis));
            layout.align_items = AlignSpec::Stretch;
        }
        DockNode::Item { .. } | DockNode::Tabs { .. } => {
            layout.direction = Some(FlexDirection::Column);
            layout.align_items = AlignSpec::Stretch;
            layout.position = PositionSpec::Relative;
        }
    }
    style
}

fn dock_root_accessibility(root: &DockNode) -> AccessibilityState {
    let label = match root {
        DockNode::Item { id, .. } => Some(Arc::clone(id)),
        DockNode::Tabs { active, .. } => Some(Arc::clone(active)),
        DockNode::Split { .. } => None,
    };
    AccessibilityState {
        role: AccessibilityRole::Generic,
        label,
        ..AccessibilityState::default()
    }
}

fn fill_surface(mut style: NodeStyle) -> NodeStyle {
    style.background = Some(SemanticColorRole::Surface);
    style.foreground = Some(SemanticColorRole::Text);
    let layout = Arc::make_mut(&mut style.layout);
    layout.width = Some(layout.width.unwrap_or(LengthSpec::Fill));
    layout.height = Some(layout.height.unwrap_or(LengthSpec::Fill));
    layout.min_width = Some(layout.min_width.unwrap_or(LengthSpec::Px(0.0)));
    layout.min_height = Some(layout.min_height.unwrap_or(LengthSpec::Px(0.0)));
    layout.overflow_x = OverflowSpec::Hidden;
    layout.overflow_y = OverflowSpec::Hidden;
    style
}

fn split_direction(axis: DockAxis) -> FlexDirection {
    match axis {
        DockAxis::Horizontal => FlexDirection::Row,
        DockAxis::Vertical => FlexDirection::Column,
    }
}

fn project_chrome(
    dock: &Dock,
    chrome: &DockChrome,
    node: &DockNode,
    world: &UiWorld,
    mutations: &mut MutationQueue,
) {
    match (chrome, node) {
        (
            DockChrome::Item {
                frame,
                title,
                overlay,
            },
            DockNode::Item { id, content },
        ) => {
            let label = dock.title_for(id);
            DockTitle {
                label: Arc::clone(&label),
                locked: dock.locked,
            }
            .project(*title, world, mutations);
            DockItemFrame { id: Arc::clone(id) }.project(*frame, world, mutations);
            project_content_slots(node, *content, *frame, world, mutations);
            project_overlay(*overlay, dock.node_drop(node), world, mutations);
        }
        (
            DockChrome::Tabs {
                frame,
                strip,
                overlay,
            },
            DockNode::Tabs {
                tabs,
                active,
                contents,
            },
        ) => {
            DockTabsFrame.project(*frame, world, mutations);
            if let Some(style) = world.node_style(*strip).cloned() {
                let mut tabs_view = Tabs::new(effective_active(tabs, active)).fill(true);
                tabs_view.style = style;
                tabs_view.project(*strip, world, mutations);
            }
            project_tab_bodies(tabs, active, contents, *frame, world, mutations);
            project_overlay(*overlay, dock.node_drop(node), world, mutations);
        }
        (
            DockChrome::Split {
                frame,
                handle,
                indicator,
                first,
                second,
            },
            DockNode::Split {
                axis,
                ratio,
                first: first_node,
                second: second_node,
            },
        ) => {
            let ratio = clamp_ratio(*ratio);
            DockSplitFrame { axis: *axis }.project(*frame, world, mutations);
            DockHandle {
                axis: *axis,
                locked: dock.locked,
            }
            .project(*handle, world, mutations);
            DockHandleMark { axis: *axis }.project(*indicator, world, mutations);
            project_chrome(dock, first, first_node, world, mutations);
            project_chrome(dock, second, second_node, world, mutations);
            apply_split_grow(first.frame(), ratio, world, mutations);
            apply_split_grow(second.frame(), 1.0 - ratio, world, mutations);
        }
        _ => project_content_slots(node, None, chrome.frame(), world, mutations),
    }
}

fn project_content_slots(
    node: &DockNode,
    single: Option<StableNodeId>,
    parent: StableNodeId,
    world: &UiWorld,
    mutations: &mut MutationQueue,
) {
    match node {
        DockNode::Item { content, .. } => {
            if let Some(content) = single.or(*content) {
                apply_body_slot(content, false, parent, world, mutations);
            }
        }
        DockNode::Tabs {
            tabs,
            active,
            contents,
        } => project_tab_bodies(tabs, active, contents, parent, world, mutations),
        DockNode::Split { first, second, .. } => {
            let mut slots = Vec::new();
            collect_contents(first, &mut slots);
            collect_contents(second, &mut slots);
            for (_, content) in slots {
                if let Some(content) = content {
                    apply_body_slot(content, false, parent, world, mutations);
                }
            }
        }
    }
}

fn project_tab_bodies(
    tabs: &[Arc<str>],
    active: &Arc<str>,
    contents: &[(Arc<str>, Option<StableNodeId>)],
    parent: StableNodeId,
    world: &UiWorld,
    mutations: &mut MutationQueue,
) {
    let active = effective_active(tabs, active);
    for (id, content) in contents {
        if let Some(content) = *content {
            apply_body_slot(
                content,
                id.as_ref() != active.as_ref(),
                parent,
                world,
                mutations,
            );
        }
    }
}

fn apply_body_slot(
    id: StableNodeId,
    hidden: bool,
    parent: StableNodeId,
    world: &UiWorld,
    mutations: &mut MutationQueue,
) {
    if !world.contains(id) {
        return;
    }
    if world
        .node(id)
        .is_some_and(|node| node.parent != Some(parent))
    {
        mutations.insert(parent, id, None);
    }
    let mut style = world.node_style(id).cloned().unwrap_or_default();
    let layout = Arc::make_mut(&mut style.layout);
    let mut changed = layout.hidden != hidden;
    layout.hidden = hidden;
    if layout.flex_grow.is_none() {
        layout.flex_grow = Some(1.0);
        changed = true;
    }
    if layout.flex_shrink.is_none() {
        layout.flex_shrink = Some(1.0);
        changed = true;
    }
    if layout.width.is_none() {
        layout.width = Some(LengthSpec::Fill);
        changed = true;
    }
    if layout.height.is_none() {
        layout.height = Some(LengthSpec::Fill);
        changed = true;
    }
    if layout.min_width.is_none() {
        layout.min_width = Some(LengthSpec::Px(0.0));
        changed = true;
    }
    if layout.min_height.is_none() {
        layout.min_height = Some(LengthSpec::Px(0.0));
        changed = true;
    }
    if changed {
        mutations.set_style(id, style);
    }
}

fn apply_split_grow(id: StableNodeId, grow: f32, world: &UiWorld, mutations: &mut MutationQueue) {
    let Some(current) = world.node_style(id) else {
        return;
    };
    if current.layout.flex_grow == Some(grow) {
        return;
    }
    let mut style = current.clone();
    Arc::make_mut(&mut style.layout).flex_grow = Some(grow);
    mutations.set_style(id, style);
}

fn project_overlay(
    overlay: Option<StableNodeId>,
    zone: Option<DockDropZone>,
    world: &UiWorld,
    mutations: &mut MutationQueue,
) {
    let Some(id) = overlay else {
        return;
    };
    if !world.contains(id) {
        return;
    }
    if let Some(zone) = zone {
        DockDropOverlay {
            zone,
            visible: true,
        }
        .project(id, world, mutations);
    } else if let Some(current) = world.node_style(id)
        && !current.layout.hidden
    {
        let mut style = current.clone();
        Arc::make_mut(&mut style.layout).hidden = true;
        mutations.set_style(id, style);
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DockItemFrame {
    id: Arc<str>,
}

impl ComponentView for DockItemFrame {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "dock-item".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &item_frame_style(
                world
                    .node_style(id)
                    .and_then(|style| style.layout.flex_grow),
            ),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::clone(&self.id)),
                ..AccessibilityState::default()
            },
        );
    }
}

fn item_frame_style(grow: Option<f32>) -> NodeStyle {
    let mut style = fill_surface(NodeStyle::default());
    let layout = Arc::make_mut(&mut style.layout);
    layout.direction = Some(FlexDirection::Column);
    layout.align_items = AlignSpec::Stretch;
    layout.position = PositionSpec::Relative;
    layout.flex_grow = grow;
    layout.flex_shrink = Some(1.0);
    style
}

#[derive(Debug, Clone, PartialEq)]
struct DockTitle {
    label: Arc<str>,
    locked: bool,
}

impl ComponentView for DockTitle {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "dock-title".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.text(id) != Some(self.label.as_ref()) {
            mutations.set_text(
                id,
                TextContent {
                    value: self.label.to_string().into(),
                },
            );
        }
        project_common(
            id,
            world,
            mutations,
            &title_style(),
            InteractionState {
                pointer_events: !self.locked,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::clone(&self.label)),
                ..AccessibilityState::default()
            },
        );
    }
}

fn title_style() -> NodeStyle {
    let mut style = NodeStyle::default();
    style.background = Some(SemanticColorRole::Surface);
    style.foreground = Some(SemanticColorRole::Muted);
    style.text_vertical_alignment = TextVerticalAlignment::Center;
    let layout = Arc::make_mut(&mut style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.height = Some(LengthSpec::Px(DOCK_TITLE_BAR_HEIGHT));
    layout.min_height = Some(LengthSpec::Px(DOCK_TITLE_BAR_HEIGHT));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.padding_left = Some(LengthSpec::Px(TITLE_PADDING_X));
    layout.padding_right = Some(LengthSpec::Px(TITLE_PADDING_X));
    layout.font_size = Some(TITLE_SIZE);
    layout.font_weight = Some(TITLE_WEIGHT);
    layout.white_space_nowrap = true;
    layout.text_overflow_ellipsis = true;
    style
}

#[derive(Debug, Clone, PartialEq)]
struct DockTabsFrame;

impl ComponentView for DockTabsFrame {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "dock-tabs".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let mut style = fill_surface(NodeStyle::default());
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(FlexDirection::Column);
        layout.align_items = AlignSpec::Stretch;
        layout.position = PositionSpec::Relative;
        layout.flex_grow = world
            .node_style(id)
            .and_then(|style| style.layout.flex_grow);
        layout.flex_shrink = Some(1.0);
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

#[derive(Debug, Clone, PartialEq)]
struct DockSplitFrame {
    axis: DockAxis,
}

impl ComponentView for DockSplitFrame {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "dock-split".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &split_frame_style(
                self.axis,
                world
                    .node_style(id)
                    .and_then(|style| style.layout.flex_grow),
            ),
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

fn split_frame_style(axis: DockAxis, grow: Option<f32>) -> NodeStyle {
    let mut style = fill_surface(NodeStyle::default());
    let layout = Arc::make_mut(&mut style.layout);
    layout.direction = Some(split_direction(axis));
    layout.align_items = AlignSpec::Stretch;
    layout.flex_grow = grow;
    layout.flex_shrink = Some(1.0);
    style
}

#[derive(Debug, Clone, PartialEq)]
struct DockHandle {
    axis: DockAxis,
    locked: bool,
}

impl ComponentView for DockHandle {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "dock-handle".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &handle_style(self.axis),
            handle_interaction(self.locked),
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::from("Resize")),
                ..AccessibilityState::default()
            },
        );
    }
}

fn handle_style(axis: DockAxis) -> NodeStyle {
    let mut style = NodeStyle::default();
    style.interaction = InteractionStyle {
        focused: SemanticPaint::FOCUS_SURFACE,
        hovered: SemanticPaint {
            background: Some(SemanticColorRole::Hover),
            ..SemanticPaint::default()
        },
        ..InteractionStyle::default()
    };
    let layout = Arc::make_mut(&mut style.layout);
    layout.direction = Some(split_direction(axis));
    layout.align_items = AlignSpec::Center;
    layout.justify_content = JustifySpec::Center;
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    match axis {
        DockAxis::Horizontal => {
            layout.width = Some(LengthSpec::Px(DOCK_DIVIDER_HIT_SIZE));
            layout.min_width = Some(LengthSpec::Px(DOCK_DIVIDER_HIT_SIZE));
            layout.height = Some(LengthSpec::Fill);
        }
        DockAxis::Vertical => {
            layout.height = Some(LengthSpec::Px(DOCK_DIVIDER_HIT_SIZE));
            layout.min_height = Some(LengthSpec::Px(DOCK_DIVIDER_HIT_SIZE));
            layout.width = Some(LengthSpec::Fill);
        }
    }
    style
}

fn handle_interaction(locked: bool) -> InteractionState {
    InteractionState {
        pointer_events: !locked,
        focusable: !locked,
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DockHandleMark {
    axis: DockAxis,
}

impl ComponentView for DockHandleMark {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "dock-handle-mark".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &handle_mark_style(self.axis),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState::default(),
        );
    }
}

fn handle_mark_style(axis: DockAxis) -> NodeStyle {
    let mut style = NodeStyle::default();
    style.background = Some(SemanticColorRole::BorderStrong);
    let layout = Arc::make_mut(&mut style.layout);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    match axis {
        DockAxis::Horizontal => {
            layout.width = Some(LengthSpec::Px(HANDLE_INDICATOR));
            layout.height = Some(LengthSpec::Fill);
        }
        DockAxis::Vertical => {
            layout.height = Some(LengthSpec::Px(HANDLE_INDICATOR));
            layout.width = Some(LengthSpec::Fill);
        }
    }
    style
}

#[derive(Debug, Clone, PartialEq)]
struct DockDropOverlay {
    zone: DockDropZone,
    visible: bool,
}

impl ComponentView for DockDropOverlay {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "dock-drop-overlay".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &overlay_style(self.zone, self.visible),
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

fn overlay_style(zone: DockDropZone, visible: bool) -> NodeStyle {
    let mut style = NodeStyle::default();
    style.background = Some(SemanticColorRole::AccentSoft);
    let layout = Arc::make_mut(&mut style.layout);
    layout.position = PositionSpec::Absolute;
    layout.z_index = Some(1);
    layout.hidden = !visible;
    match zone {
        DockDropZone::Left => {
            layout.offset_left = Some(LengthSpec::Px(0.0));
            layout.offset_top = Some(LengthSpec::Px(0.0));
            layout.offset_bottom = Some(LengthSpec::Px(0.0));
            layout.width = Some(LengthSpec::Percent(50.0));
            layout.height = Some(LengthSpec::Fill);
        }
        DockDropZone::Right => {
            layout.offset_right = Some(LengthSpec::Px(0.0));
            layout.offset_top = Some(LengthSpec::Px(0.0));
            layout.offset_bottom = Some(LengthSpec::Px(0.0));
            layout.width = Some(LengthSpec::Percent(50.0));
            layout.height = Some(LengthSpec::Fill);
        }
        DockDropZone::Top => {
            layout.offset_left = Some(LengthSpec::Px(0.0));
            layout.offset_right = Some(LengthSpec::Px(0.0));
            layout.offset_top = Some(LengthSpec::Px(0.0));
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(LengthSpec::Percent(50.0));
        }
        DockDropZone::Bottom => {
            layout.offset_left = Some(LengthSpec::Px(0.0));
            layout.offset_right = Some(LengthSpec::Px(0.0));
            layout.offset_bottom = Some(LengthSpec::Px(0.0));
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(LengthSpec::Percent(50.0));
        }
        DockDropZone::Tab => {
            layout.offset_left = Some(LengthSpec::Px(0.0));
            layout.offset_right = Some(LengthSpec::Px(0.0));
            layout.offset_top = Some(LengthSpec::Px(0.0));
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(LengthSpec::Px(TAB_OVERLAY_THICKNESS));
        }
    }
    style
}

/// Flat bordered inspector / dock content surface.
#[derive(Debug, Clone, PartialEq)]
pub struct DockPanel {
    pub content: Option<StableNodeId>,
    pub padding: f32,
    pub style: NodeStyle,
}

impl DockPanel {
    pub fn new() -> Self {
        Self {
            content: None,
            padding: 0.0,
            style: NodeStyle::default(),
        }
    }

    pub fn content(mut self, content: StableNodeId) -> Self {
        self.content = Some(content);
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding.max(0.0);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        style.background = Some(SemanticColorRole::Surface);
        style.foreground = Some(SemanticColorRole::Text);
        style.border = Some(SemanticColorRole::BorderSoft);
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(layout.height.unwrap_or(LengthSpec::Shrink));
        layout.border_width = Some(nana_ui_core::HAIRLINE);
        layout.border_radius = Some(0.0);
        let pad = LengthSpec::Px(self.padding);
        layout.padding_left = Some(pad);
        layout.padding_right = Some(pad);
        layout.padding_top = Some(pad);
        layout.padding_bottom = Some(pad);
        style
    }
}

impl Default for DockPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentView for DockPanel {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "dock-panel".into(),
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
            &self.effective_style(),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                ..AccessibilityState::default()
            },
        );
        if let Some(content) = self.content {
            apply_body_slot(content, false, id, world, mutations);
        }
    }
}

fn extract_item(root: &mut DockNode, id: &str) -> Option<DockNode> {
    let (remaining, taken) = take_item_node(root.clone(), id);
    let (Some(remaining), Some(taken)) = (remaining, taken) else {
        return None;
    };
    *root = remaining;
    Some(taken)
}

fn take_item_node(node: DockNode, id: &str) -> (Option<DockNode>, Option<DockNode>) {
    match node {
        DockNode::Item { id: item, content } => {
            if item.as_ref() == id {
                (None, Some(DockNode::Item { id: item, content }))
            } else {
                (Some(DockNode::Item { id: item, content }), None)
            }
        }
        DockNode::Tabs {
            tabs,
            active,
            contents,
        } => {
            if !tabs.iter().any(|tab| tab.as_ref() == id) {
                return (
                    Some(DockNode::Tabs {
                        tabs,
                        active,
                        contents,
                    }),
                    None,
                );
            }
            let taken_content = contents
                .iter()
                .find(|(tab, _)| tab.as_ref() == id)
                .and_then(|(_, content)| *content);
            let taken = DockNode::item(Arc::<str>::from(id), taken_content);
            let tabs = tabs
                .into_iter()
                .filter(|tab| tab.as_ref() != id)
                .collect::<Vec<_>>();
            let contents = contents
                .into_iter()
                .filter(|(tab, _)| tab.as_ref() != id)
                .collect::<Vec<_>>();
            let remaining = match tabs.as_slice() {
                [] => None,
                [_] => {
                    let only = tabs.into_iter().next().expect("one remaining tab");
                    let content = contents
                        .into_iter()
                        .find(|(tab, _)| tab == &only)
                        .and_then(|(_, content)| content);
                    Some(DockNode::item(only, content))
                }
                _ => Some(DockNode::Tabs {
                    active: effective_active(&tabs, &active),
                    tabs,
                    contents,
                }),
            };
            (remaining, Some(taken))
        }
        DockNode::Split {
            axis,
            ratio,
            first,
            second,
        } => match take_item_node(*first, id) {
            (remaining_first, Some(taken)) => {
                let remaining = match remaining_first {
                    Some(first) => Some(DockNode::split(axis, ratio, first, *second)),
                    None => Some(*second),
                };
                (remaining, Some(taken))
            }
            (Some(first), None) => match take_item_node(*second, id) {
                (remaining_second, Some(taken)) => {
                    let remaining = match remaining_second {
                        Some(second) => Some(DockNode::split(axis, ratio, first, second)),
                        None => Some(first),
                    };
                    (remaining, Some(taken))
                }
                (Some(second), None) => (Some(DockNode::split(axis, ratio, first, second)), None),
                (None, None) => (Some(first), None),
            },
            (None, None) => take_item_node(*second, id),
        },
    }
}

impl AppContext {
    /// Mount retained chrome for `dock`. Host content slots are reparented, not recreated.
    ///
    /// One [`Dock`] is one window surface. Floating surfaces are additional
    /// `Entity<Dock>` values assembled on the document that owns that window.
    /// The dock's assembler in a view (`TypeBehavior::slot_assembler`):
    /// the child a view keyed with an item's id is that item's content
    /// (`widget(Dock::new(layout)).children(files().key("files"))`), then
    /// [`Self::assemble_dock`]. A child whose key names no item is parked.
    pub fn assemble_dock_panels(&mut self, dock: Entity<Dock>) -> Result<bool, FrameworkError> {
        let mut root = self.read(dock, |dock| dock.root.clone())?;
        let mut bound = false;
        for id in root.flatten() {
            if let Some(content) = self
                .assembled_child(dock.stable_id(), &id)
                .filter(|content| self.world().contains(*content))
            {
                bound |= root.bind_content(&id, content);
            }
        }
        if bound {
            self.update_component(dock, |dock, _| dock.root = root)?;
        }
        Ok(self.assemble_dock(dock)? || bound)
    }

    pub fn assemble_dock(&mut self, dock: Entity<Dock>) -> Result<bool, FrameworkError> {
        let document = document_of(self, dock.stable_id())?;
        let snapshot = self.read(dock, |dock| {
            (
                dock.root.clone(),
                dock.drop_target.clone(),
                dock.locked,
                dock.titles.clone(),
                dock.chrome.clone(),
                dock.hidden.clone(),
            )
        })?;
        let (mut root, drop_target, locked, titles, old, hidden) = snapshot;
        root.clamp_ratios();
        let visible = filter_hidden(&root, &hidden).unwrap_or_else(|| root.clone());
        let hosts = host_content_ids(&root);
        if let Some(old) = old.as_ref() {
            park_hosts(self, old, &hosts)?;
        }
        let chrome = assemble_branch(
            self,
            document,
            dock.stable_id(),
            &visible,
            old,
            &drop_target,
            locked,
            &titles,
            &hosts,
            true,
        )?;
        let children = branch_children(&chrome, &visible);
        self.update_component(dock, |dock, _| {
            dock.root = root.clone();
            dock.chrome = Some(chrome.clone());
        })?;
        reconcile_children(self, dock, &children)
    }

    /// Float `id` off `dock` and rebuild that surface. The host opens the window.
    pub fn float_dock_item(
        &mut self,
        dock: Entity<Dock>,
        id: impl AsRef<str>,
    ) -> Result<Option<DockWorkspaceEvent>, FrameworkError> {
        let id = id.as_ref();
        let event = self.update_component(dock, |dock, _| {
            dock.float_item(id).map(DockWorkspaceEvent::OpenFloating)
        })?;
        if event.is_some() {
            self.assemble_dock(dock)?;
        }
        Ok(event)
    }

    pub fn is_dock(&self, id: StableNodeId) -> bool {
        self.read(Entity::<Dock>::from_stable_id(id), |_| ())
            .is_ok()
    }

    pub fn is_dock_handle(&self, id: StableNodeId) -> bool {
        self.dock_handle_id(id).is_some()
    }

    pub fn is_dock_title(&self, id: StableNodeId) -> bool {
        self.dock_title_id(id).is_some()
    }

    pub fn is_dock_tab_strip(&self, id: StableNodeId) -> bool {
        self.dock_tab_strip_id(id).is_some()
    }

    /// Handle under the pointer, including a few pixels of slop around the 8px bar.
    pub fn dock_handle_near(&self, document: DocumentId, x: f32, y: f32) -> Option<StableNodeId> {
        self.dock_handle_near_hit(document, x, y, self.pointer_target(document, x, y))
    }

    /// [`Self::dock_handle_near`] with the point already hit-tested.
    pub fn dock_handle_near_hit(
        &self,
        document: DocumentId,
        x: f32,
        y: f32,
        hit: Option<StableNodeId>,
    ) -> Option<StableNodeId> {
        const SLOP: f32 = nana_ui_core::space::SM;
        if let Some(target) = hit {
            if let Some(handle) = self.unlocked_dock_handle(target) {
                return Some(handle);
            }
            if let Some(parent) = self.world().parent_id(target)
                && let Some(handle) = self.unlocked_dock_handle(parent)
            {
                return Some(handle);
            }
        }
        // Every pointer move off a handle reaches here, so ask the docks for
        // their handles instead of walking the document.
        self.unlocked_dock_chrome_find(document, |chrome| {
            chrome_find(chrome, &mut |node| match node {
                DockChrome::Split { handle, .. } => self
                    .world()
                    .layout_box(*handle)
                    .is_some_and(|bounds| point_near_box(bounds, x, y, SLOP))
                    .then_some(*handle),
                _ => None,
            })
        })
    }

    /// The first answer `find` gives for the chrome of an unlocked dock in
    /// `document`, docks in no particular order.
    fn unlocked_dock_chrome_find(
        &self,
        document: DocumentId,
        find: impl Fn(&DockChrome) -> Option<StableNodeId>,
    ) -> Option<StableNodeId> {
        self.world()
            .nodes_of_component(document, crate::component_descriptors::DOCK.type_id)
            .find_map(|dock| {
                self.read(Entity::<Dock>::from_stable_id(dock), |dock| {
                    (!dock.locked)
                        .then(|| dock.chrome.as_ref().and_then(&find))
                        .flatten()
                })
                .ok()
                .flatten()
            })
    }

    /// Tab strip under the pointer, including option children of assembled chrome.
    pub fn dock_tab_strip_near(
        &self,
        document: DocumentId,
        x: f32,
        y: f32,
    ) -> Option<StableNodeId> {
        self.dock_tab_strip_near_hit(document, x, y, self.pointer_target(document, x, y))
    }

    /// [`Self::dock_tab_strip_near`] with the point already hit-tested.
    pub fn dock_tab_strip_near_hit(
        &self,
        document: DocumentId,
        x: f32,
        y: f32,
        hit: Option<StableNodeId>,
    ) -> Option<StableNodeId> {
        if let Some(target) = hit {
            if let Some(strip) = self.unlocked_dock_tab_strip(target) {
                return Some(strip);
            }
            let mut current = Some(target);
            while let Some(id) = current {
                if let Some(strip) = self.unlocked_dock_tab_strip(id) {
                    return Some(strip);
                }
                current = self.world().parent_id(id);
            }
        }
        self.unlocked_dock_chrome_find(document, |chrome| {
            chrome_find(chrome, &mut |node| match node {
                DockChrome::Tabs { strip, .. } => self
                    .world()
                    .layout_box(*strip)
                    .is_some_and(|bounds| bounds.contains(x, y))
                    .then_some(*strip),
                _ => None,
            })
        })
    }

    pub fn begin_dock_split_resize(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        target: StableNodeId,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(handle) = self.dock_handle_id(target) else {
            return Ok(false);
        };
        let Some(dock) = self.dock_entity_of(handle) else {
            return Ok(false);
        };
        if self.read(dock, |dock| dock.locked)? {
            return Ok(false);
        }
        let Some((frame, axis, ratio, first_ids)) = self.read(dock, |dock| {
            let chrome = dock.chrome.as_ref()?;
            let visible = dock.visible_root()?;
            split_info_for_handle(chrome, &visible, handle)
        })?
        else {
            return Ok(false);
        };
        let start = match axis {
            DockAxis::Horizontal => x,
            DockAxis::Vertical => y,
        };
        self.update_component(dock, |dock, cx| {
            dock.item_drag = None;
            dock.split_resize = Some(DockSplitResize {
                handle,
                frame,
                axis,
                start: Some(start),
                start_ratio: ratio,
                first_ids,
            });
            cx.mutations().request_focus(document, Some(handle));
            cx.mutations().capture_pointer(pointer_id, handle);
            true
        })
    }

    pub fn update_dock_split_resize(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        let Some(handle) = self.dock_handle_id(target) else {
            return Ok(false);
        };
        let Some(dock) = self.dock_entity_of(handle) else {
            return Ok(false);
        };
        let Some((frame, axis, start, start_ratio, first_ids)) = self.read(dock, |dock| {
            dock.split_resize.as_ref().map(|session| {
                (
                    session.frame,
                    session.axis,
                    session.start,
                    session.start_ratio,
                    session.first_ids.clone(),
                )
            })
        })?
        else {
            return Ok(false);
        };
        if self.read(dock, |dock| dock.locked)? {
            return Ok(false);
        }
        let Some(bounds) = self.world().layout_box(frame) else {
            return Ok(false);
        };
        let extent = match axis {
            DockAxis::Horizontal => bounds.width,
            DockAxis::Vertical => bounds.height,
        } - DOCK_DIVIDER_HIT_SIZE;
        let position = match axis {
            DockAxis::Horizontal => x,
            DockAxis::Vertical => y,
        };
        if !position.is_finite() {
            return Ok(false);
        }
        let start = start.unwrap_or(position);
        let ratio = dock_split_ratio_from_pointer(start_ratio, start, position, extent);
        self.update_component(dock, |dock, _| {
            set_split_ratio_for_first_ids(&mut dock.root, &first_ids, ratio)
        })
    }

    pub fn end_dock_split_resize(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        cancel: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        let Some(handle) = self.dock_handle_id(target) else {
            return Ok(false);
        };
        let Some(dock) = self.dock_entity_of(handle) else {
            return Ok(false);
        };
        self.update_component(dock, |dock, cx| {
            let Some(session) = dock.split_resize.take() else {
                return false;
            };
            if cancel {
                set_split_ratio_for_first_ids(
                    &mut dock.root,
                    &session.first_ids,
                    session.start_ratio,
                );
            }
            cx.mutations().release_pointer(pointer_id, handle);
            true
        })
    }

    /// Nudge the focused dock split handle by one keyboard step. Locked docks refuse.
    pub fn adjust_focused_dock_split(
        &mut self,
        document: DocumentId,
        direction: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.world().focused(document) else {
            return Ok(false);
        };
        let Some(handle) = self.dock_handle_id(focused) else {
            return Ok(false);
        };
        let Some(dock) = self.dock_entity_of(handle) else {
            return Ok(false);
        };
        if self.read(dock, |dock| dock.locked)? {
            return Ok(false);
        }
        let Some((_, _, ratio, first_ids)) = self.read(dock, |dock| {
            let chrome = dock.chrome.as_ref()?;
            let visible = dock.visible_root()?;
            split_info_for_handle(chrome, &visible, handle)
        })?
        else {
            return Ok(false);
        };
        if !direction.is_finite() || direction == 0.0 {
            return Ok(false);
        }
        let next = dock_nudge_split_ratio(ratio, direction);
        let changed = self.update_component(dock, |dock, _| {
            set_split_ratio_for_first_ids(&mut dock.root, &first_ids, next)
        })?;
        if changed {
            self.assemble_dock(dock)?;
        }
        Ok(changed)
    }

    pub fn begin_dock_item_drag(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        target: StableNodeId,
        _x: f32,
        _y: f32,
    ) -> Result<bool, FrameworkError> {
        if self
            .world()
            .node(target)
            .is_none_or(|node| node.document != document)
        {
            return Ok(false);
        }
        let Some(dock) = self.dock_entity_of(target) else {
            return Ok(false);
        };
        if self.read(dock, |dock| dock.locked)? {
            return Ok(false);
        }
        let Some(id) = self.dock_item_for_target(dock, target)? else {
            return Ok(false);
        };
        if self.read(dock, |dock| dock.primary.as_deref() == Some(id.as_ref()))? {
            return Ok(false);
        }
        self.update_component(dock, |dock, cx| {
            activate_runtime_dock_tab(&mut dock.root, id.as_ref());
            dock.split_resize = None;
            dock.drop_target = None;
            dock.item_drag = Some(DockItemDrag { id, reorder: None });
            cx.mutations().capture_pointer(pointer_id, target);
            true
        })
    }

    pub fn update_dock_item_drag(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        let Some(dock) = self.dock_entity_of(target) else {
            return Ok(false);
        };
        let Some(dragged) = self.read(dock, |dock| {
            dock.item_drag.as_ref().map(|drag| Arc::clone(&drag.id))
        })?
        else {
            return Ok(false);
        };
        if self.read(dock, |dock| dock.locked)? {
            return Ok(false);
        }
        let next_reorder = self.dock_reorder_at(dock, dragged.as_ref(), x, y)?;
        let next_zone = if next_reorder.is_some() {
            None
        } else {
            self.dock_drop_target_at(dock, dragged.as_ref(), x, y)?
        };
        let changed = self.update_component(dock, |dock, _| {
            let Some(drag) = dock.item_drag.as_mut() else {
                return false;
            };
            let changed = dock.drop_target != next_zone || drag.reorder != next_reorder;
            dock.drop_target = next_zone;
            drag.reorder = next_reorder;
            changed
        })?;
        if changed {
            self.assemble_dock(dock)?;
        }
        Ok(true)
    }

    pub fn end_dock_item_drag(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
        cancel: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        let Some(dock) = self.dock_entity_of(target) else {
            return Ok(false);
        };
        let Some((dragged, reorder)) = self.read(dock, |dock| {
            dock.item_drag
                .as_ref()
                .map(|drag| (Arc::clone(&drag.id), drag.reorder.clone()))
        })?
        else {
            return Ok(false);
        };
        let drop_target = self.read(dock, |dock| dock.drop_target.clone())?;
        let locked = self.read(dock, |dock| dock.locked)?;
        let outside = self
            .world()
            .layout_box(dock.stable_id())
            .is_none_or(|bounds| !bounds.contains(x, y));
        self.update_component(dock, |dock, cx| {
            dock.item_drag = None;
            dock.drop_target = None;
            cx.mutations().release_pointer(pointer_id, target);
            if cancel || locked {
                return;
            }
            if let Some((target_id, before)) = reorder {
                reorder_dock_tab(&mut dock.root, dragged.as_ref(), target_id.as_ref(), before);
            } else if let Some((target_id, zone)) = drop_target {
                dock.retarget(dragged.as_ref(), target_id.as_ref(), zone);
            } else if outside {
                let _ = dock.float_item_at(
                    dragged.as_ref(),
                    x,
                    y,
                    DEFAULT_FLOATING_WIDTH,
                    DEFAULT_FLOATING_HEIGHT,
                );
            }
        })?;
        self.assemble_dock(dock)?;
        Ok(true)
    }

    pub fn is_dock_item_source(&self, id: StableNodeId) -> bool {
        self.dock_title_id(id).is_some()
            || self
                .dock_entity_of(id)
                .and_then(|dock| self.dock_item_for_target(dock, id).ok().flatten())
                .is_some()
    }

    fn dock_entity_of(&self, id: StableNodeId) -> Option<Entity<Dock>> {
        let mut current = Some(id);
        while let Some(id) = current {
            if self.is_dock(id) {
                return Some(Entity::from_stable_id(id));
            }
            current = self.world().parent_id(id);
        }
        None
    }

    fn dock_is_locked(&self, id: StableNodeId) -> bool {
        self.dock_entity_of(id)
            .and_then(|dock| self.read(dock, |dock| dock.locked).ok())
            .unwrap_or(false)
    }

    fn unlocked_dock_handle(&self, id: StableNodeId) -> Option<StableNodeId> {
        let handle = self.dock_handle_id(id)?;
        (!self.dock_is_locked(handle)).then_some(handle)
    }

    fn unlocked_dock_tab_strip(&self, id: StableNodeId) -> Option<StableNodeId> {
        let strip = self.dock_tab_strip_id(id)?;
        (!self.dock_is_locked(strip)).then_some(strip)
    }

    fn dock_handle_id(&self, id: StableNodeId) -> Option<StableNodeId> {
        if self
            .read(Entity::<DockHandle>::from_stable_id(id), |_| ())
            .is_ok()
        {
            let dock = self.dock_entity_of(id)?;
            return self
                .read(dock, |dock| {
                    dock.chrome
                        .as_ref()
                        .is_some_and(|chrome| chrome_has_handle(chrome, id))
                })
                .ok()
                .filter(|matches| *matches)
                .map(|_| id);
        }
        let parent = self.world().parent_id(id)?;
        if self
            .read(Entity::<DockHandle>::from_stable_id(parent), |_| ())
            .is_ok()
        {
            return self.dock_handle_id(parent);
        }
        None
    }

    fn dock_title_id(&self, id: StableNodeId) -> Option<StableNodeId> {
        if self
            .read(Entity::<DockTitle>::from_stable_id(id), |_| ())
            .is_err()
        {
            return None;
        }
        let dock = self.dock_entity_of(id)?;
        self.read(dock, |dock| {
            dock.chrome
                .as_ref()
                .is_some_and(|chrome| chrome_has_title(chrome, id))
        })
        .ok()
        .filter(|matches| *matches)
        .map(|_| id)
    }

    fn dock_tab_strip_id(&self, id: StableNodeId) -> Option<StableNodeId> {
        if self
            .read(Entity::<Tabs>::from_stable_id(id), |_| ())
            .is_ok()
        {
            let dock = self.dock_entity_of(id)?;
            return self
                .read(dock, |dock| {
                    dock.chrome
                        .as_ref()
                        .is_some_and(|chrome| chrome_has_strip(chrome, id))
                })
                .ok()
                .filter(|matches| *matches)
                .map(|_| id);
        }
        None
    }

    fn dock_item_for_target(
        &self,
        dock: Entity<Dock>,
        target: StableNodeId,
    ) -> Result<Option<Arc<str>>, FrameworkError> {
        let chrome = self.read(dock, |dock| dock.chrome.clone())?;
        let visible = self.read(dock, Dock::visible_root)?;
        let (Some(chrome), Some(visible)) = (chrome, visible) else {
            return Ok(None);
        };
        if let Some(id) = title_item_id(&chrome, &visible, target) {
            return Ok(Some(id));
        }
        let mut current = Some(target);
        while let Some(id) = current {
            if chrome_has_strip(&chrome, id) {
                if let Some(value) = self
                    .read(Entity::<Tabs>::from_stable_id(id), |tabs| {
                        tabs.option_nodes()
                            .iter()
                            .find(|(_, option)| *option == target)
                            .map(|(value, _)| Arc::clone(value))
                            .or_else(|| tabs.selected.clone())
                    })
                    .ok()
                    .flatten()
                {
                    return Ok(Some(value));
                }
                return Ok(strip_for_tabs(&chrome, &visible, id));
            }
            current = self.world().parent_id(id);
        }
        Ok(None)
    }

    fn dock_reorder_at(
        &self,
        dock: Entity<Dock>,
        dragged: &str,
        x: f32,
        y: f32,
    ) -> Result<Option<(Arc<str>, bool)>, FrameworkError> {
        let chrome = self.read(dock, |dock| dock.chrome.clone())?;
        let visible = self.read(dock, Dock::visible_root)?;
        let (Some(chrome), Some(visible)) = (chrome, visible) else {
            return Ok(None);
        };
        let Some(strip) = strip_containing_item(&chrome, &visible, dragged) else {
            return Ok(None);
        };
        let options = self
            .read(Entity::<Tabs>::from_stable_id(strip), |tabs| {
                tabs.option_nodes().to_vec()
            })
            .unwrap_or_default();
        for (id, option) in options {
            let Some(bounds) = self.world().layout_box(option) else {
                continue;
            };
            if !bounds.contains(x, y) {
                continue;
            }
            let before = x < bounds.x + bounds.width * 0.5;
            return Ok(Some((id, before)));
        }
        Ok(None)
    }

    fn dock_drop_target_at(
        &self,
        dock: Entity<Dock>,
        dragged: &str,
        x: f32,
        y: f32,
    ) -> Result<Option<(Arc<str>, DockDropZone)>, FrameworkError> {
        let chrome = self.read(dock, |dock| dock.chrome.clone())?;
        let visible = self.read(dock, Dock::visible_root)?;
        let (Some(chrome), Some(visible)) = (chrome, visible) else {
            return Ok(None);
        };
        let mut leaves = Vec::new();
        collect_drop_leaves(&chrome, &visible, &mut leaves);
        for (id, frame) in leaves {
            if id.as_ref() == dragged {
                continue;
            }
            let Some(bounds) = self.world().layout_box(frame) else {
                continue;
            };
            if let Some(zone) = drop_zone_at(bounds, x, y) {
                return Ok(Some((id, zone)));
            }
        }
        Ok(None)
    }
}

fn activate_runtime_dock_tab(node: &mut DockNode, id: &str) -> bool {
    match node {
        DockNode::Item { id: item, .. } => item.as_ref() == id,
        DockNode::Tabs { tabs, active, .. } => {
            if tabs.iter().any(|tab| tab.as_ref() == id) {
                *active = Arc::from(id);
                true
            } else {
                false
            }
        }
        DockNode::Split { first, second, .. } => {
            activate_runtime_dock_tab(first, id) || activate_runtime_dock_tab(second, id)
        }
    }
}

fn document_of(context: &AppContext, id: StableNodeId) -> Result<DocumentId, FrameworkError> {
    context
        .world()
        .node(id)
        .map(|node| node.document)
        .ok_or(FrameworkError::MissingView(id))
}

fn assemble_branch(
    context: &mut AppContext,
    document: DocumentId,
    root_id: StableNodeId,
    node: &DockNode,
    old: Option<DockChrome>,
    drop_target: &Option<(Arc<str>, DockDropZone)>,
    locked: bool,
    titles: &[(Arc<str>, Arc<str>)],
    hosts: &HashSet<StableNodeId>,
    at_root: bool,
) -> Result<DockChrome, FrameworkError> {
    match node {
        DockNode::Item { id, content } => assemble_item(
            context,
            document,
            root_id,
            id,
            *content,
            old,
            drop_zone_for(drop_target, std::slice::from_ref(id)),
            locked,
            titles,
            hosts,
            at_root,
        ),
        DockNode::Tabs {
            tabs,
            active,
            contents,
        } => assemble_tabs(
            context,
            document,
            root_id,
            tabs,
            active,
            contents,
            old,
            drop_zone_for(drop_target, tabs),
            locked,
            titles,
            hosts,
            at_root,
        ),
        DockNode::Split {
            axis,
            ratio,
            first,
            second,
        } => assemble_split(
            context,
            document,
            root_id,
            *axis,
            *ratio,
            first,
            second,
            old,
            drop_target,
            locked,
            titles,
            hosts,
            at_root,
        ),
    }
}

fn drop_zone_for(
    drop_target: &Option<(Arc<str>, DockDropZone)>,
    ids: &[Arc<str>],
) -> Option<DockDropZone> {
    drop_target
        .as_ref()
        .filter(|(id, _)| ids.iter().any(|item| item == id))
        .map(|(_, zone)| *zone)
}

fn title_lookup(titles: &[(Arc<str>, Arc<str>)], id: &str) -> Arc<str> {
    titles
        .iter()
        .find(|(key, _)| key.as_ref() == id)
        .map(|(_, title)| Arc::clone(title))
        .unwrap_or_else(|| Arc::from(id))
}

fn assemble_item(
    context: &mut AppContext,
    document: DocumentId,
    root_id: StableNodeId,
    id: &Arc<str>,
    content: Option<StableNodeId>,
    old: Option<DockChrome>,
    zone: Option<DockDropZone>,
    locked: bool,
    titles: &[(Arc<str>, Arc<str>)],
    hosts: &HashSet<StableNodeId>,
    at_root: bool,
) -> Result<DockChrome, FrameworkError> {
    let reused = match old {
        Some(DockChrome::Item {
            frame,
            title,
            overlay,
        }) => Some((frame, title, overlay)),
        Some(other) => {
            dispose_chrome(context, other, root_id, hosts)?;
            None
        }
        None => None,
    };
    let frame = if let Some((frame, _, _)) = reused {
        if frame != root_id {
            context.update_component(
                Entity::<DockItemFrame>::from_stable_id(frame),
                |frame, _| {
                    frame.id = Arc::clone(id);
                },
            )?;
        }
        frame
    } else if at_root {
        root_id
    } else {
        context
            .create_detached_component(document, DockItemFrame { id: Arc::clone(id) })?
            .stable_id()
    };
    let label = title_lookup(titles, id);
    let title = if let Some((_, title, _)) = reused {
        let entity = Entity::<DockTitle>::from_stable_id(title);
        context.update_component(entity, |title, _| {
            title.label = Arc::clone(&label);
            title.locked = locked;
        })?;
        title
    } else {
        context
            .create_detached_component(document, DockTitle { label, locked })?
            .stable_id()
    };
    let overlay = ensure_overlay(
        context,
        document,
        reused.and_then(|(_, _, overlay)| overlay),
        zone,
    )?;
    let mut ordered = vec![title];
    if let Some(content) = content {
        ordered.push(content);
    }
    if let Some(overlay) = overlay {
        ordered.push(overlay);
    }
    reconcile_ids(context, frame, &ordered)?;
    Ok(DockChrome::Item {
        frame,
        title,
        overlay,
    })
}

fn assemble_tabs(
    context: &mut AppContext,
    document: DocumentId,
    root_id: StableNodeId,
    tabs: &[Arc<str>],
    active: &Arc<str>,
    contents: &[(Arc<str>, Option<StableNodeId>)],
    old: Option<DockChrome>,
    zone: Option<DockDropZone>,
    locked: bool,
    titles: &[(Arc<str>, Arc<str>)],
    hosts: &HashSet<StableNodeId>,
    at_root: bool,
) -> Result<DockChrome, FrameworkError> {
    let reused = match old {
        Some(DockChrome::Tabs {
            frame,
            strip,
            overlay,
        }) => Some((frame, strip, overlay)),
        Some(other) => {
            dispose_chrome(context, other, root_id, hosts)?;
            None
        }
        None => None,
    };
    let frame = if let Some((frame, _, _)) = reused {
        frame
    } else if at_root {
        root_id
    } else {
        context
            .create_detached_component(document, DockTabsFrame)?
            .stable_id()
    };
    if frame != root_id && reused.is_some() {
        context.reproject_component(Entity::<DockTabsFrame>::from_stable_id(frame))?;
    }
    let active = effective_active(tabs, active);
    let options = tabs
        .iter()
        .map(|id| TabOption::new(Arc::clone(id), title_lookup(titles, id)).draggable(!locked))
        .collect::<Vec<_>>();
    let strip = if let Some((_, strip, _)) = reused {
        context.update_component(Entity::<Tabs>::from_stable_id(strip), |strip, _| {
            strip.options = options.clone();
            strip.selected = Some(Arc::clone(&active));
            strip.focus = Some(Arc::clone(&active));
            strip.fill = true;
            strip.size = ControlSize::Small;
        })?;
        strip
    } else {
        context
            .create_detached_component(
                document,
                Tabs::new(Arc::clone(&active))
                    .options(options)
                    .fill(true)
                    .size(ControlSize::Small),
            )?
            .stable_id()
    };
    let overlay = ensure_overlay(
        context,
        document,
        reused.and_then(|(_, _, overlay)| overlay),
        zone,
    )?;
    let mut ordered = vec![strip];
    for (_, content) in contents {
        if let Some(content) = *content {
            ordered.push(content);
        }
    }
    if let Some(overlay) = overlay {
        ordered.push(overlay);
    }
    reconcile_ids(context, frame, &ordered)?;
    Ok(DockChrome::Tabs {
        frame,
        strip,
        overlay,
    })
}

fn assemble_split(
    context: &mut AppContext,
    document: DocumentId,
    root_id: StableNodeId,
    axis: DockAxis,
    ratio: f32,
    first: &DockNode,
    second: &DockNode,
    old: Option<DockChrome>,
    drop_target: &Option<(Arc<str>, DockDropZone)>,
    locked: bool,
    titles: &[(Arc<str>, Arc<str>)],
    hosts: &HashSet<StableNodeId>,
    at_root: bool,
) -> Result<DockChrome, FrameworkError> {
    let reused = match old {
        Some(DockChrome::Split {
            frame,
            handle,
            indicator,
            first,
            second,
        }) => Some((frame, handle, indicator, first, second)),
        Some(other) => {
            dispose_chrome(context, other, root_id, hosts)?;
            None
        }
        None => None,
    };
    let frame = if let Some((frame, _, _, _, _)) = reused {
        if frame != root_id {
            context.update_component(
                Entity::<DockSplitFrame>::from_stable_id(frame),
                |split, _| {
                    split.axis = axis;
                },
            )?;
        }
        frame
    } else if at_root {
        root_id
    } else {
        context
            .create_detached_component(document, DockSplitFrame { axis })?
            .stable_id()
    };
    let handle = if let Some((_, handle, _, _, _)) = reused {
        context.update_component(Entity::<DockHandle>::from_stable_id(handle), |handle, _| {
            handle.axis = axis;
            handle.locked = locked;
        })?;
        handle
    } else {
        context
            .create_detached_component(document, DockHandle { axis, locked })?
            .stable_id()
    };
    let indicator = if let Some((_, _, indicator, _, _)) = reused {
        context.update_component(
            Entity::<DockHandleMark>::from_stable_id(indicator),
            |mark, _| {
                mark.axis = axis;
            },
        )?;
        indicator
    } else {
        context
            .create_detached_component(document, DockHandleMark { axis })?
            .stable_id()
    };
    reconcile_ids(context, handle, &[indicator])?;
    let first_chrome = assemble_branch(
        context,
        document,
        root_id,
        first,
        reused.as_ref().map(|chrome| (*chrome.3).clone()),
        drop_target,
        locked,
        titles,
        hosts,
        false,
    )?;
    let second_chrome = assemble_branch(
        context,
        document,
        root_id,
        second,
        reused.as_ref().map(|chrome| (*chrome.4).clone()),
        drop_target,
        locked,
        titles,
        hosts,
        false,
    )?;
    apply_grow_now(context, first_chrome.frame(), clamp_ratio(ratio))?;
    apply_grow_now(context, second_chrome.frame(), 1.0 - clamp_ratio(ratio))?;
    reconcile_ids(
        context,
        frame,
        &[first_chrome.frame(), handle, second_chrome.frame()],
    )?;
    Ok(DockChrome::Split {
        frame,
        handle,
        indicator,
        first: Box::new(first_chrome),
        second: Box::new(second_chrome),
    })
}

fn apply_grow_now(
    context: &mut AppContext,
    id: StableNodeId,
    grow: f32,
) -> Result<(), FrameworkError> {
    let Some(current) = context.world().node_style(id).cloned() else {
        return Ok(());
    };
    if current.layout.flex_grow == Some(grow) {
        return Ok(());
    }
    let mut style = current;
    Arc::make_mut(&mut style.layout).flex_grow = Some(grow);
    let mut mutations = MutationQueue::new();
    mutations.set_style(id, style);
    context.commit_mutations(mutations)?;
    Ok(())
}

fn ensure_overlay(
    context: &mut AppContext,
    document: DocumentId,
    existing: Option<StableNodeId>,
    zone: Option<DockDropZone>,
) -> Result<Option<StableNodeId>, FrameworkError> {
    match (existing, zone) {
        (Some(id), Some(zone)) => {
            context.update_component(
                Entity::<DockDropOverlay>::from_stable_id(id),
                |overlay, _| {
                    overlay.zone = zone;
                    overlay.visible = true;
                },
            )?;
            Ok(Some(id))
        }
        (Some(id), None) => {
            context.update_component(
                Entity::<DockDropOverlay>::from_stable_id(id),
                |overlay, _| {
                    overlay.visible = false;
                },
            )?;
            Ok(Some(id))
        }
        (None, Some(zone)) => Ok(Some(
            context
                .create_detached_component(
                    document,
                    DockDropOverlay {
                        zone,
                        visible: true,
                    },
                )?
                .stable_id(),
        )),
        (None, None) => Ok(None),
    }
}

fn branch_children(chrome: &DockChrome, node: &DockNode) -> Vec<StableNodeId> {
    match (chrome, node) {
        (DockChrome::Item { title, overlay, .. }, DockNode::Item { content, .. }) => {
            let mut children = vec![*title];
            if let Some(content) = content {
                children.push(*content);
            }
            if let Some(overlay) = overlay {
                children.push(*overlay);
            }
            children
        }
        (DockChrome::Tabs { strip, overlay, .. }, DockNode::Tabs { contents, .. }) => {
            let mut children = vec![*strip];
            for (_, content) in contents {
                if let Some(content) = content {
                    children.push(*content);
                }
            }
            if let Some(overlay) = overlay {
                children.push(*overlay);
            }
            children
        }
        (
            DockChrome::Split {
                handle,
                first,
                second,
                ..
            },
            _,
        ) => {
            vec![first.frame(), *handle, second.frame()]
        }
        _ => Vec::new(),
    }
}

fn park_hosts(
    context: &mut AppContext,
    chrome: &DockChrome,
    hosts: &HashSet<StableNodeId>,
) -> Result<(), FrameworkError> {
    let mut mutations = MutationQueue::new();
    collect_host_parks(context, chrome.frame(), hosts, &mut mutations);
    if !mutations.is_empty() {
        context.commit_mutations(mutations)?;
    }
    Ok(())
}

fn collect_host_parks(
    context: &AppContext,
    id: StableNodeId,
    hosts: &HashSet<StableNodeId>,
    mutations: &mut MutationQueue,
) {
    let Some(node) = context.world().node(id) else {
        return;
    };
    for child in node.children {
        if hosts.contains(&child) {
            mutations.park_subtree(child);
        } else {
            collect_host_parks(context, child, hosts, mutations);
        }
    }
}

fn dispose_chrome(
    context: &mut AppContext,
    chrome: DockChrome,
    root_id: StableNodeId,
    hosts: &HashSet<StableNodeId>,
) -> Result<(), FrameworkError> {
    park_hosts(context, &chrome, hosts)?;
    match chrome {
        DockChrome::Item {
            frame,
            title,
            overlay,
        } => {
            remove_if_present::<DockTitle>(context, title)?;
            if let Some(overlay) = overlay {
                remove_if_present::<DockDropOverlay>(context, overlay)?;
            }
            if frame != root_id {
                remove_if_present::<DockItemFrame>(context, frame)?;
            }
        }
        DockChrome::Tabs {
            frame,
            strip,
            overlay,
        } => {
            remove_if_present::<Tabs>(context, strip)?;
            if let Some(overlay) = overlay {
                remove_if_present::<DockDropOverlay>(context, overlay)?;
            }
            if frame != root_id {
                remove_if_present::<DockTabsFrame>(context, frame)?;
            }
        }
        DockChrome::Split {
            frame,
            handle,
            indicator,
            first,
            second,
        } => {
            dispose_chrome(context, *first, root_id, hosts)?;
            dispose_chrome(context, *second, root_id, hosts)?;
            remove_if_present::<DockHandleMark>(context, indicator)?;
            remove_if_present::<DockHandle>(context, handle)?;
            if frame != root_id {
                remove_if_present::<DockSplitFrame>(context, frame)?;
            }
        }
    }
    Ok(())
}

fn remove_if_present<C: ComponentView>(
    context: &mut AppContext,
    id: StableNodeId,
) -> Result<(), FrameworkError> {
    if context.world().contains(id) {
        let _ = context.remove_view(Entity::<C>::from_stable_id(id))?;
    }
    Ok(())
}

fn reconcile_children<C: ComponentView>(
    context: &mut AppContext,
    parent: Entity<C>,
    ordered: &[StableNodeId],
) -> Result<bool, FrameworkError> {
    reconcile_ids(context, parent.stable_id(), ordered)
}

fn reconcile_ids(
    context: &mut AppContext,
    parent: StableNodeId,
    ordered: &[StableNodeId],
) -> Result<bool, FrameworkError> {
    context.reconcile_children(parent, ordered)
}

/// The router reaches dock handles and drags only once a dock exists.
pub(crate) static DOCK_HOOKS: crate::framework::DockHooks = crate::framework::DockHooks {
    handle_near_hit: AppContext::dock_handle_near_hit,
    tab_strip_near_hit: AppContext::dock_tab_strip_near_hit,
    is_handle: AppContext::is_dock_handle,
    is_item_source: AppContext::is_dock_item_source,
    begin_split: AppContext::begin_dock_split_resize,
    update_split: AppContext::update_dock_split_resize,
    end_split: AppContext::end_dock_split_resize,
    begin_item: AppContext::begin_dock_item_drag,
    update_item: AppContext::update_dock_item_drag,
    end_item: AppContext::end_dock_item_drag,
    adjust_focused_split: AppContext::adjust_focused_dock_split,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentId, LayoutBox, MutationQueue, Text};

    #[test]
    fn dock_workspace_layout_json_uses_historical_layout_fields() {
        let mut workspace = DockWorkspace::new(DockNode::split(
            DockAxis::Horizontal,
            0.25,
            DockNode::tabs(
                ["scenes", "sources"],
                "scenes",
                [("scenes", None), ("sources", None)],
            ),
            DockNode::item("editor", Some(StableNodeId::new(7).expect("content id"))),
        ));
        workspace.hidden.push(Arc::from("controls"));
        workspace.floating.push(DockFloatingSurface {
            id: Arc::from("1"),
            root: DockNode::item("mixer", None),
            x: 40.0,
            y: 50.0,
            width: 360.0,
            height: 280.0,
        });
        let json = workspace.layout_json().expect("workspace serializes");
        assert!(json.contains("\"version\":1"));
        assert!(json.contains("\"kind\":\"split\""));
        assert!(json.contains("\"axis\":\"horizontal\""));
        assert!(json.contains("\"kind\":\"tabs\""));
        assert!(json.contains("\"surface\":1"));
        assert!(json.contains("\"bounds\""));
        assert!(json.contains("\"hidden\":[\"controls\"]"));
        assert!(!json.contains("content"));
        assert!(!json.contains("primary"));
        assert!(!json.contains("next_surface"));

        let restored = DockWorkspace::from_layout_json(&json).expect("workspace restores");
        assert!(restored.main.contains("editor"));
        assert!(restored.main.contains("scenes"));
        match &restored.main {
            DockNode::Split { first, .. } => match first.as_ref() {
                DockNode::Tabs { contents, .. } => {
                    assert!(contents.iter().all(|(_, content)| content.is_none()));
                }
                other => panic!("expected tabs, got {other:?}"),
            },
            other => panic!("expected split, got {other:?}"),
        }
        match &restored.main {
            DockNode::Split { second, .. } => match second.as_ref() {
                DockNode::Item { content, .. } => assert_eq!(*content, None),
                other => panic!("expected item, got {other:?}"),
            },
            other => panic!("expected split, got {other:?}"),
        }
        assert_eq!(
            restored
                .hidden
                .iter()
                .map(|id| id.as_ref())
                .collect::<Vec<_>>(),
            ["controls"]
        );
        assert_eq!(restored.floating.len(), 1);
        assert_eq!(restored.floating[0].id.as_ref(), "1");
        assert_eq!(restored.floating[0].x, 40.0);
        assert_eq!(restored.floating[0].width, 360.0);
        let again = restored.layout_json().expect("second serialize");
        assert_eq!(json, again);
    }

    #[test]
    fn dock_workspace_roundtrip_through_store() {
        let store = ViewStateStore::new(
            nana_ui_core::memory_store(),
            nana_ui_core::RestorationPath::root(),
        );
        let mut workspace = DockWorkspace::new(DockNode::split(
            DockAxis::Horizontal,
            0.25,
            DockNode::tabs(
                ["scenes", "sources"],
                "scenes",
                [("scenes", None), ("sources", None)],
            ),
            DockNode::item("editor", None),
        ));
        workspace.hidden.push(Arc::from("controls"));
        workspace
            .save_to_store(&store, "gallery")
            .expect("dock saves");
        let mut restored = DockWorkspace::new(DockNode::item("placeholder", None));
        assert!(
            restored
                .restore_from_store(&store, "gallery")
                .expect("dock restores")
        );
        assert_eq!(
            restored.layout_json().expect("restored serializes"),
            workspace.layout_json().expect("original serializes")
        );
        assert!(
            !restored
                .restore_from_store(&store, "missing")
                .expect("missing key")
        );
    }

    #[test]
    fn dock_workspace_layout_json_parses_historical_adapter_document() {
        let json = r#"{"version":1,"main":{"kind":"tabs","tabs":["a","b"],"active":"b"},"floating":[{"surface":2,"root":{"kind":"item","id":"c"},"bounds":{"x":1.0,"y":2.0,"width":3.0,"height":4.0},"monitor":"m"}],"hidden":["d"],"locked":true}"#;
        let mut workspace = DockWorkspace::new(DockNode::item("keep", None)).primary("keep");
        workspace
            .restore_layout_json(json)
            .expect("historical json restores");
        assert_eq!(workspace.primary.as_deref(), Some("keep"));
        match &workspace.main {
            DockNode::Tabs { tabs, active, .. } => {
                assert_eq!(
                    tabs.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
                    ["a", "b"]
                );
                assert_eq!(active.as_ref(), "b");
            }
            other => panic!("expected tabs, got {other:?}"),
        }
        assert_eq!(workspace.floating[0].id.as_ref(), "2");
        assert_eq!(workspace.floating[0].y, 2.0);
        assert_eq!(workspace.hidden[0].as_ref(), "d");
        let encoded = workspace.layout_json().expect("product re-encode");
        assert!(encoded.contains("\"locked\":true"));
        assert!(encoded.contains("\"monitor\":\"m\""));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
            serde_json::from_str::<serde_json::Value>(json).unwrap()
        );
    }

    #[test]
    fn dock_workspace_from_layout_json_clamps_split_ratio() {
        let json = r#"{"version":1,"main":{"kind":"split","axis":"vertical","ratio":4.0,"first":{"kind":"item","id":"top"},"second":{"kind":"item","id":"bottom"}}}"#;
        let workspace = DockWorkspace::from_layout_json(json).expect("loads");
        assert_eq!(workspace.main.split_ratio_at(&[]), Some(MAX_SPLIT_RATIO));
    }

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn body(context: &mut AppContext, label: &str) -> StableNodeId {
        context
            .create_component(document(), Text::new(label))
            .unwrap()
            .stable_id()
    }

    fn descendants(context: &AppContext, root: StableNodeId) -> Vec<StableNodeId> {
        let mut ids = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            ids.push(id);
            if let Some(node) = context.world().node(id) {
                stack.extend(node.children.iter().rev().copied());
            }
        }
        ids
    }

    fn find_tag(context: &AppContext, root: StableNodeId, tag: &str) -> Vec<StableNodeId> {
        descendants(context, root)
            .into_iter()
            .filter(|id| {
                context.world().node(*id).is_some_and(|node| {
                    matches!(
                        &node.kind,
                        NodeKind::Element { tag: current } if current == tag
                    )
                })
            })
            .collect()
    }

    fn visible_overlay(context: &AppContext, root: StableNodeId) -> Option<StableNodeId> {
        find_tag(context, root, "dock-drop-overlay")
            .into_iter()
            .find(|id| {
                context
                    .world()
                    .node_style(*id)
                    .is_some_and(|style| !style.layout.hidden)
            })
    }

    #[test]
    fn dock_split_ratio_clamps_and_preserves_child_order() {
        let low = DockNode::split(
            DockAxis::Horizontal,
            -2.0,
            DockNode::item("left", None),
            DockNode::item("right", None),
        );
        assert_eq!(low.split_ratio(), Some(MIN_SPLIT_RATIO));
        assert_eq!(
            low.flatten()
                .iter()
                .map(|id| id.as_ref())
                .collect::<Vec<_>>(),
            ["left", "right"]
        );

        let high = DockNode::split(
            DockAxis::Vertical,
            4.0,
            DockNode::item("top", None),
            DockNode::item("bottom", None),
        );
        assert_eq!(high.split_ratio(), Some(MAX_SPLIT_RATIO));

        let nan = DockNode::split(
            DockAxis::Horizontal,
            f32::NAN,
            DockNode::item("a", None),
            DockNode::item("b", None),
        );
        assert_eq!(nan.split_ratio(), Some(0.5));

        let mut context = AppContext::new();
        let first = body(&mut context, "first");
        let second = body(&mut context, "second");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::split(
                    DockAxis::Horizontal,
                    1.4,
                    DockNode::item("inspector", Some(first)),
                    DockNode::item("console", Some(second)),
                )),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let chrome = context
            .read(dock, |dock| dock.chrome.clone())
            .unwrap()
            .unwrap();
        let (first_frame, handle, second_frame) = chrome.split_children().unwrap();
        assert_eq!(
            context.world().node(dock.stable_id()).unwrap().children,
            [first_frame, handle, second_frame]
        );
        assert_eq!(
            context
                .world()
                .node_style(first_frame)
                .unwrap()
                .layout
                .flex_grow,
            Some(MAX_SPLIT_RATIO)
        );
        assert_eq!(
            context
                .world()
                .node_style(second_frame)
                .unwrap()
                .layout
                .flex_grow,
            Some(1.0 - MAX_SPLIT_RATIO)
        );
        assert_eq!(
            context
                .world()
                .node_style(dock.stable_id())
                .unwrap()
                .layout
                .direction,
            Some(FlexDirection::Row)
        );
    }

    #[test]
    fn dock_tabs_hide_inactive_body() {
        let mut context = AppContext::new();
        let code = body(&mut context, "code");
        let preview = body(&mut context, "preview");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::tabs(
                    ["code", "preview"],
                    "code",
                    [("code", Some(code)), ("preview", Some(preview))],
                )),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();
        assert!(!context.world().node_style(code).unwrap().layout.hidden);
        assert!(context.world().node_style(preview).unwrap().layout.hidden);
        assert_eq!(
            context.world().node(code).unwrap().parent,
            Some(dock.stable_id())
        );
        assert_eq!(
            context.world().node(preview).unwrap().parent,
            Some(dock.stable_id())
        );

        context
            .update_component(dock, |dock, _| {
                if let DockNode::Tabs { active, .. } = &mut dock.root {
                    *active = Arc::from("preview");
                }
            })
            .unwrap();
        assert!(context.world().node_style(code).unwrap().layout.hidden);
        assert!(!context.world().node_style(preview).unwrap().layout.hidden);
        assert!(context.world().contains(code));
        assert!(context.world().contains(preview));
    }

    #[test]
    fn dock_item_projects_content_slot() {
        let mut context = AppContext::new();
        let content = body(&mut context, "inspector-body");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::item("inspector", Some(content)))
                    .title("inspector", "Inspector"),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let title = find_tag(&context, dock.stable_id(), "dock-title")[0];
        assert_eq!(context.world().text(title), Some("Inspector"));
        assert_eq!(
            context.world().node(content).unwrap().parent,
            Some(dock.stable_id())
        );
        assert_eq!(
            context.world().node(dock.stable_id()).unwrap().children[0],
            title
        );
        assert!(
            context
                .world()
                .node(dock.stable_id())
                .unwrap()
                .children
                .contains(&content)
        );
        let style = context.world().node_style(dock.stable_id()).unwrap();
        assert_eq!(style.background, Some(SemanticColorRole::Surface));
        assert_eq!(style.layout.height, Some(LengthSpec::Fill));
        assert_eq!(style.layout.overflow_y, OverflowSpec::Hidden);
        assert_eq!(
            context
                .world()
                .accessibility(dock.stable_id())
                .unwrap()
                .label
                .as_deref(),
            Some("inspector")
        );
    }

    #[test]
    fn dock_drop_overlay_present_only_when_drop_target_set() {
        let mut context = AppContext::new();
        let content = body(&mut context, "pane");
        let dock = context
            .create_component(document(), Dock::new(DockNode::item("pane", Some(content))))
            .unwrap();
        context.assemble_dock(dock).unwrap();
        assert!(visible_overlay(&context, dock.stable_id()).is_none());

        context
            .update_component(dock, |dock, _| {
                dock.drop_target = Some((Arc::from("pane"), DockDropZone::Left));
            })
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let overlay = visible_overlay(&context, dock.stable_id()).expect("overlay");
        let style = context.world().node_style(overlay).unwrap();
        assert_eq!(style.background, Some(SemanticColorRole::AccentSoft));
        assert_eq!(style.layout.width, Some(LengthSpec::Percent(50.0)));
        assert_eq!(style.layout.position, PositionSpec::Absolute);
        assert_eq!(
            context.world().node(overlay).unwrap().parent,
            Some(dock.stable_id())
        );
        assert!(
            context
                .world()
                .node(dock.stable_id())
                .unwrap()
                .children
                .contains(&content)
        );

        context
            .update_component(dock, |dock, _| {
                dock.drop_target = None;
            })
            .unwrap();
        assert!(visible_overlay(&context, dock.stable_id()).is_none());
    }

    #[test]
    fn dock_locked_omits_handle_pointer() {
        let mut context = AppContext::new();
        let first = body(&mut context, "a");
        let second = body(&mut context, "b");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::split(
                    DockAxis::Vertical,
                    0.4,
                    DockNode::item("a", Some(first)),
                    DockNode::item("b", Some(second)),
                )),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let handle = find_tag(&context, dock.stable_id(), "dock-handle")[0];
        assert_eq!(
            context.world().interaction(handle),
            Some(InteractionState {
                pointer_events: true,
                focusable: true,
            })
        );

        context
            .update_component(dock, |dock, _| {
                dock.locked = true;
            })
            .unwrap();
        assert_eq!(
            context.world().interaction(handle),
            Some(InteractionState {
                pointer_events: false,
                focusable: false,
            })
        );
        let mark = find_tag(&context, dock.stable_id(), "dock-handle-mark")[0];
        assert_eq!(
            context.world().node_style(mark).unwrap().background,
            Some(SemanticColorRole::BorderStrong)
        );
    }

    #[test]
    fn dock_panel_border_and_child_slot() {
        let mut context = AppContext::new();
        let child = body(&mut context, "fields");
        let panel = context
            .create_component(document(), DockPanel::new().padding(8.0).content(child))
            .unwrap();
        let style = context.world().node_style(panel.stable_id()).unwrap();
        assert_eq!(style.background, Some(SemanticColorRole::Surface));
        assert_eq!(style.foreground, Some(SemanticColorRole::Text));
        assert_eq!(style.border, Some(SemanticColorRole::BorderSoft));
        assert_eq!(style.layout.border_width, Some(nana_ui_core::HAIRLINE));
        assert_eq!(style.layout.border_radius, Some(0.0));
        assert_eq!(style.layout.width, Some(LengthSpec::Fill));
        assert_eq!(style.layout.height, Some(LengthSpec::Shrink));
        assert_eq!(style.layout.padding_left, Some(LengthSpec::Px(8.0)));
        assert_eq!(
            context.world().node(panel.stable_id()).unwrap().kind,
            NodeKind::Element {
                tag: "dock-panel".into(),
            }
        );
        assert_eq!(
            context.world().node(child).unwrap().parent,
            Some(panel.stable_id())
        );
    }

    #[test]
    fn dock_idle_project_does_not_dirty() {
        let mut context = AppContext::new();
        let content = body(&mut context, "body");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::item("pane", Some(content)))
                    .drop_target("pane", DockDropZone::Tab),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let panel_child = body(&mut context, "panel-body");
        let panel = context
            .create_component(document(), DockPanel::new().content(panel_child))
            .unwrap();
        let _ = context.take_system_work();
        context.reproject_component(dock).unwrap();
        context.reproject_component(panel).unwrap();
        assert!(context.take_system_work().is_empty());
    }

    #[test]
    fn float_item_removes_branch_from_main_and_returns_spec() {
        let mut dock = Dock::new(DockNode::split(
            DockAxis::Horizontal,
            0.4,
            DockNode::item("inspector", None),
            DockNode::item("console", None),
        ));

        let surface = dock.float_item("console").expect("float console");
        assert_eq!(surface.id.as_ref(), "console");
        assert_eq!(surface.root, DockNode::item("console", None));
        assert_eq!(
            (surface.x, surface.y, surface.width, surface.height),
            (
                DEFAULT_FLOATING_X,
                DEFAULT_FLOATING_Y,
                DEFAULT_FLOATING_WIDTH,
                DEFAULT_FLOATING_HEIGHT
            )
        );
        assert!(!dock.contains("console"));
        assert_eq!(dock.flatten(), vec![Arc::from("inspector")]);
        assert!(dock.float_item("missing").is_none());
        assert!(dock.float_item("inspector").is_none());
        assert!(dock.contains("inspector"));
    }

    #[test]
    fn float_item_from_tabs_collapses_remaining_item() {
        let mut dock = Dock::new(DockNode::tabs(
            ["code", "preview"],
            "preview",
            [("code", None), ("preview", None)],
        ));
        let surface = dock.float_item("preview").expect("float preview");
        assert_eq!(surface.root, DockNode::item("preview", None));
        assert_eq!(dock.root, DockNode::item("code", None));
    }

    #[test]
    fn reassembled_dock_omits_floated_id() {
        let mut context = AppContext::new();
        let inspector = body(&mut context, "inspector-body");
        let console = body(&mut context, "console-body");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::split(
                    DockAxis::Horizontal,
                    0.4,
                    DockNode::item("inspector", Some(inspector)),
                    DockNode::item("console", Some(console)),
                )),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();

        let event = context
            .float_dock_item(dock, "console")
            .unwrap()
            .expect("floated");
        let DockWorkspaceEvent::OpenFloating(surface) = event else {
            panic!("open floating");
        };
        assert_eq!(surface.id.as_ref(), "console");
        assert_eq!(surface.root, DockNode::item("console", Some(console)));
        assert_eq!(
            context.read(dock, |dock| dock.flatten()).unwrap(),
            vec![Arc::from("inspector")]
        );

        context.assemble_dock(dock).unwrap();
        assert_eq!(
            context.read(dock, |dock| dock.flatten()).unwrap(),
            vec![Arc::from("inspector")]
        );
        assert!(!context.read(dock, |dock| dock.contains("console")).unwrap());
        assert_eq!(
            context.world().node(inspector).unwrap().parent,
            Some(dock.stable_id())
        );
        assert!(!descendants(&context, dock.stable_id()).contains(&console));
    }

    #[test]
    fn workspace_float_item_tracks_a_new_surface() {
        let mut workspace = DockWorkspace::new(DockNode::split(
            DockAxis::Vertical,
            0.5,
            DockNode::item("editor", None),
            DockNode::item("terminal", None),
        ));
        let event = workspace.float_item("terminal").expect("float terminal");
        let DockWorkspaceEvent::OpenFloating(surface) = &event else {
            panic!("open floating");
        };
        assert_eq!(surface.id.as_ref(), "1");
        assert_eq!(surface.window_key(), 1);
        assert!(!workspace.main.contains("terminal"));
        assert_eq!(workspace.floating.len(), 1);
        assert_eq!(workspace.surfaces().len(), 2);
        assert_eq!(workspace.surfaces()[0].id.as_ref(), MAIN_SURFACE_ID);
        assert_eq!(workspace.surfaces()[0].bounds, None);
        assert_eq!(
            workspace.surfaces()[1].bounds,
            Some((
                DEFAULT_FLOATING_X,
                DEFAULT_FLOATING_Y,
                DEFAULT_FLOATING_WIDTH,
                DEFAULT_FLOATING_HEIGHT
            ))
        );

        workspace.apply(DockWorkspaceEvent::MoveFloating {
            id: Arc::from("1"),
            x: 80.0,
            y: 90.0,
            width: 360.0,
            height: 280.0,
        });
        assert_eq!(workspace.floating[0].x, 80.0);
        workspace.apply(DockWorkspaceEvent::CloseFloating(Arc::from("1")));
        assert!(workspace.floating.is_empty());
        assert_eq!(dock_surface_window_key(MAIN_SURFACE_ID), 0);
        assert_ne!(dock_surface_window_key("inspector"), 0);
    }

    #[test]
    fn hide_omits_item_from_assemble_and_last_cannot_hide() {
        let mut context = AppContext::new();
        let inspector = body(&mut context, "inspector-body");
        let console = body(&mut context, "console-body");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::split(
                    DockAxis::Horizontal,
                    0.4,
                    DockNode::item("inspector", Some(inspector)),
                    DockNode::item("console", Some(console)),
                ))
                .primary("inspector"),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();
        assert_eq!(
            context.read(dock, |dock| dock.flatten()).unwrap(),
            vec![Arc::from("inspector"), Arc::from("console")]
        );

        context
            .update_component(dock, |dock, _| {
                assert!(!dock.hide("inspector"));
                assert!(dock.hide("console"));
                assert!(!dock.hide("console"));
                assert!(!dock.hide("inspector"));
            })
            .unwrap();
        assert_eq!(
            context.read(dock, |dock| dock.flatten()).unwrap(),
            vec![Arc::from("inspector")]
        );
        assert!(context.read(dock, |dock| dock.contains("console")).unwrap());
        assert!(
            !context
                .read(dock, |dock| dock.is_visible("console"))
                .unwrap()
        );
        context.assemble_dock(dock).unwrap();
        assert_eq!(find_tag(&context, dock.stable_id(), "dock-title").len(), 1);
        assert!(!descendants(&context, dock.stable_id()).contains(&console));
        assert_eq!(
            context.world().node(inspector).unwrap().parent,
            Some(dock.stable_id())
        );

        context
            .update_component(dock, |dock, _| {
                assert!(dock.show("console"));
                assert!(!dock.show("console"));
            })
            .unwrap();
        context.assemble_dock(dock).unwrap();
        assert_eq!(
            context.read(dock, |dock| dock.flatten()).unwrap(),
            vec![Arc::from("inspector"), Arc::from("console")]
        );
        assert_eq!(find_tag(&context, dock.stable_id(), "dock-title").len(), 2);
        assert!(descendants(&context, dock.stable_id()).contains(&console));

        let mut only = Dock::new(DockNode::item("only", None));
        assert!(!only.hide("only"));
        assert!(only.is_visible("only"));
    }

    #[test]
    fn begin_update_end_split_drag_changes_ratio() {
        let mut context = AppContext::new();
        let first = body(&mut context, "first");
        let second = body(&mut context, "second");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::split(
                    DockAxis::Horizontal,
                    0.4,
                    DockNode::item("inspector", Some(first)),
                    DockNode::item("console", Some(second)),
                )),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let chrome = context
            .read(dock, |dock| dock.chrome.clone())
            .unwrap()
            .unwrap();
        let (first_frame, handle, second_frame) = chrome.split_children().unwrap();
        let mut layout = MutationQueue::new();
        layout.write_layout(
            dock.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 200.0,
            },
        );
        layout.write_layout(
            first_frame,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 156.8,
                height: 200.0,
            },
        );
        layout.write_layout(
            handle,
            LayoutBox {
                x: 156.8,
                y: 0.0,
                width: DOCK_DIVIDER_HIT_SIZE,
                height: 200.0,
            },
        );
        layout.write_layout(
            second_frame,
            LayoutBox {
                x: 164.8,
                y: 0.0,
                width: 235.2,
                height: 200.0,
            },
        );
        context.commit_mutations(layout).unwrap();

        assert!(
            context
                .begin_dock_split_resize(document(), 1, handle, 160.0, 20.0)
                .unwrap()
        );
        assert!(
            context
                .update_dock_split_resize(document(), 1, 200.0, 20.0)
                .unwrap()
        );
        assert!(context.end_dock_split_resize(document(), 1, false).unwrap());
        let ratio = context
            .read(dock, |dock| match &dock.root {
                DockNode::Split { ratio, .. } => *ratio,
                _ => panic!("split"),
            })
            .unwrap();
        assert!((ratio - clamp_ratio(0.4 + 40.0 / (400.0 - DOCK_DIVIDER_HIT_SIZE))).abs() < 0.001);
        assert_eq!(
            ratio,
            dock_split_ratio_from_pointer(0.4, 160.0, 200.0, 400.0 - DOCK_DIVIDER_HIT_SIZE)
        );
        assert!(context.world().pointer_capture(document(), 1).is_none());
    }

    #[test]
    fn split_ratio_helpers_are_the_live_formula() {
        let available = 400.0 - DOCK_DIVIDER_HIT_SIZE;
        assert!(
            (dock_split_ratio_from_pointer(0.4, 160.0, 200.0, available)
                - clamp_ratio(0.4 + 40.0 / available))
            .abs()
                < f32::EPSILON
        );
        assert!((dock_nudge_split_ratio(0.4, 1.0) - clamp_ratio(0.45)).abs() < 1e-6);
        let (first, second) = dock_split_child_lengths(0.4, 400.0);
        assert!((first - available * 0.4).abs() < f32::EPSILON);
        assert!((second - (available - first)).abs() < f32::EPSILON);
        let mut workspace = DockWorkspace::new(DockNode::split(
            DockAxis::Horizontal,
            0.4,
            DockNode::item("inspector", None),
            DockNode::item("console", None),
        ));
        assert!(workspace.set_split_ratio(MAIN_SURFACE_ID, &[], 0.7));
        assert_eq!(workspace.main.split_ratio_at(&[]), Some(clamp_ratio(0.7)));
        assert!(!workspace.set_split_ratio(MAIN_SURFACE_ID, &[], 0.7));
    }

    #[test]
    fn drop_onto_left_and_tab_mutates_tree() {
        let mut context = AppContext::new();
        let first = body(&mut context, "first");
        let second = body(&mut context, "second");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::split(
                    DockAxis::Horizontal,
                    0.4,
                    DockNode::item("inspector", Some(first)),
                    DockNode::item("console", Some(second)),
                )),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let chrome = context
            .read(dock, |dock| dock.chrome.clone())
            .unwrap()
            .unwrap();
        let (first_frame, _handle, second_frame) = chrome.split_children().unwrap();
        let titles = find_tag(&context, dock.stable_id(), "dock-title");
        let inspector_title = titles[0];
        let mut layout = MutationQueue::new();
        layout.write_layout(
            dock.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 200.0,
            },
        );
        layout.write_layout(
            first_frame,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 196.0,
                height: 200.0,
            },
        );
        layout.write_layout(
            second_frame,
            LayoutBox {
                x: 204.0,
                y: 0.0,
                width: 196.0,
                height: 200.0,
            },
        );
        layout.write_layout(
            inspector_title,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 196.0,
                height: DOCK_TITLE_BAR_HEIGHT,
            },
        );
        context.commit_mutations(layout).unwrap();

        assert!(
            context
                .begin_dock_item_drag(document(), 1, inspector_title, 20.0, 10.0)
                .unwrap()
        );
        assert!(
            context
                .update_dock_item_drag(document(), 1, 302.0, 100.0)
                .unwrap()
        );
        assert_eq!(
            context.read(dock, |dock| dock.drop_target.clone()).unwrap(),
            Some((Arc::from("console"), DockDropZone::Tab))
        );
        assert!(
            context
                .end_dock_item_drag(document(), 1, 302.0, 100.0, false)
                .unwrap()
        );
        assert!(matches!(
            context.read(dock, |dock| dock.root.clone()).unwrap(),
            DockNode::Tabs { .. }
        ));
        assert_eq!(
            context.read(dock, |dock| dock.flatten()).unwrap(),
            vec![Arc::from("console"), Arc::from("inspector")]
        );

        context
            .update_component(dock, |dock, _| {
                dock.root = DockNode::split(
                    DockAxis::Horizontal,
                    0.4,
                    DockNode::item("inspector", Some(first)),
                    DockNode::item("console", Some(second)),
                );
            })
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let chrome = context
            .read(dock, |dock| dock.chrome.clone())
            .unwrap()
            .unwrap();
        let (first_frame, _handle, second_frame) = chrome.split_children().unwrap();
        let titles = find_tag(&context, dock.stable_id(), "dock-title");
        let console_title = titles[1];
        let mut layout = MutationQueue::new();
        layout.write_layout(
            dock.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 200.0,
            },
        );
        layout.write_layout(
            first_frame,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 196.0,
                height: 200.0,
            },
        );
        layout.write_layout(
            second_frame,
            LayoutBox {
                x: 204.0,
                y: 0.0,
                width: 196.0,
                height: 200.0,
            },
        );
        layout.write_layout(
            console_title,
            LayoutBox {
                x: 204.0,
                y: 0.0,
                width: 196.0,
                height: DOCK_TITLE_BAR_HEIGHT,
            },
        );
        context.commit_mutations(layout).unwrap();

        assert!(
            context
                .begin_dock_item_drag(document(), 2, console_title, 220.0, 10.0)
                .unwrap()
        );
        assert!(
            context
                .update_dock_item_drag(document(), 2, 20.0, 100.0)
                .unwrap()
        );
        assert_eq!(
            context.read(dock, |dock| dock.drop_target.clone()).unwrap(),
            Some((Arc::from("inspector"), DockDropZone::Left))
        );
        assert!(
            context
                .end_dock_item_drag(document(), 2, 20.0, 100.0, false)
                .unwrap()
        );
        match context.read(dock, |dock| dock.root.clone()).unwrap() {
            DockNode::Split {
                axis,
                first,
                second,
                ..
            } => {
                assert_eq!(axis, DockAxis::Horizontal);
                assert_eq!(first.flatten()[0].as_ref(), "console");
                assert_eq!(second.flatten()[0].as_ref(), "inspector");
            }
            other => panic!("expected split, got {other:?}"),
        }
    }

    #[test]
    fn workspace_hide_show_and_primary() {
        let mut workspace = DockWorkspace::new(DockNode::split(
            DockAxis::Horizontal,
            0.4,
            DockNode::item("gallery.primary", None),
            DockNode::item("assets", None),
        ))
        .primary("gallery.primary");
        assert!(!workspace.hide("gallery.primary"));
        assert!(workspace.hide("assets"));
        assert!(!workspace.is_visible("assets"));
        assert_eq!(
            workspace.surfaces()[0].root.flatten(),
            vec![Arc::from("gallery.primary")]
        );
        assert!(workspace.show("assets"));
        assert!(workspace.is_visible("assets"));
        assert_eq!(workspace.surfaces()[0].root.flatten().len(), 2);
    }

    fn tab_option_nodes(
        context: &AppContext,
        dock: crate::Entity<Dock>,
    ) -> Vec<(Arc<str>, StableNodeId)> {
        let chrome = context
            .read(dock, |dock| dock.chrome.clone())
            .unwrap()
            .unwrap();
        let strip = match chrome {
            DockChrome::Tabs { strip, .. } => strip,
            _ => panic!("expected tabs chrome"),
        };
        context
            .read(Entity::<Tabs>::from_stable_id(strip), |tabs| {
                tabs.option_nodes().to_vec()
            })
            .unwrap()
    }

    fn layout_two_tab_strip(
        context: &mut AppContext,
        dock: crate::Entity<Dock>,
        first: StableNodeId,
        second: StableNodeId,
    ) {
        let mut layout = MutationQueue::new();
        layout.write_layout(
            dock.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 200.0,
            },
        );
        layout.write_layout(
            first,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: DOCK_TITLE_BAR_HEIGHT,
            },
        );
        layout.write_layout(
            second,
            LayoutBox {
                x: 80.0,
                y: 0.0,
                width: 80.0,
                height: DOCK_TITLE_BAR_HEIGHT,
            },
        );
        context.commit_mutations(layout).unwrap();
    }

    fn two_tab_dock(
        context: &mut AppContext,
        locked: bool,
    ) -> (crate::Entity<Dock>, StableNodeId, StableNodeId) {
        let first_body = body(context, "first-body");
        let second_body = body(context, "second-body");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::tabs(
                    ["first", "second"],
                    "first",
                    [("first", Some(first_body)), ("second", Some(second_body))],
                ))
                .locked(locked),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let options = tab_option_nodes(context, dock);
        assert_eq!(options.len(), 2);
        (dock, options[0].1, options[1].1)
    }

    #[test]
    fn reorder_dock_tab_moves_before_or_after_sibling() {
        let mut root = DockNode::tabs(
            ["first", "second"],
            "first",
            [("first", None), ("second", None)],
        );
        assert!(reorder_dock_tab(&mut root, "first", "second", false));
        assert_eq!(
            root.flatten(),
            vec![Arc::from("second"), Arc::from("first")]
        );
        assert!(reorder_dock_tab(&mut root, "first", "second", true));
        assert_eq!(
            root.flatten(),
            vec![Arc::from("first"), Arc::from("second")]
        );
        assert!(!reorder_dock_tab(&mut root, "first", "second", true));
        assert!(!reorder_dock_tab(&mut root, "first", "first", false));
    }

    #[test]
    fn drag_first_tab_onto_second_reorders_by_before_after() {
        let mut context = AppContext::new();
        let (dock, first_tab, second_tab) = two_tab_dock(&mut context, false);
        layout_two_tab_strip(&mut context, dock, first_tab, second_tab);

        assert!(
            context
                .begin_dock_item_drag(document(), 1, first_tab, 20.0, 10.0)
                .unwrap()
        );
        assert!(
            context
                .update_dock_item_drag(document(), 1, 140.0, 10.0)
                .unwrap()
        );
        assert_eq!(
            context
                .read(dock, |dock| {
                    dock.item_drag
                        .as_ref()
                        .and_then(|drag| drag.reorder.clone())
                })
                .unwrap(),
            Some((Arc::from("second"), false))
        );
        assert!(
            context
                .read(dock, |dock| dock.drop_target.clone())
                .unwrap()
                .is_none()
        );
        assert!(
            context
                .end_dock_item_drag(document(), 1, 140.0, 10.0, false)
                .unwrap()
        );
        assert_eq!(
            context.read(dock, |dock| dock.root.flatten()).unwrap(),
            vec![Arc::from("second"), Arc::from("first")]
        );

        context
            .update_component(dock, |dock, _| {
                dock.root = DockNode::tabs(
                    ["first", "second"],
                    "first",
                    [("first", None), ("second", None)],
                );
            })
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let options = tab_option_nodes(&context, dock);
        layout_two_tab_strip(&mut context, dock, options[0].1, options[1].1);
        assert!(
            context
                .begin_dock_item_drag(document(), 2, options[0].1, 20.0, 10.0)
                .unwrap()
        );
        assert!(
            context
                .update_dock_item_drag(document(), 2, 100.0, 10.0)
                .unwrap()
        );
        assert_eq!(
            context
                .read(dock, |dock| {
                    dock.item_drag
                        .as_ref()
                        .and_then(|drag| drag.reorder.clone())
                })
                .unwrap(),
            Some((Arc::from("second"), true))
        );
        assert!(
            context
                .end_dock_item_drag(document(), 2, 100.0, 10.0, false)
                .unwrap()
        );
        assert_eq!(
            context.read(dock, |dock| dock.root.flatten()).unwrap(),
            vec![Arc::from("first"), Arc::from("second")]
        );
    }

    #[test]
    fn dock_tab_reorder_is_noop_when_dropping_on_self() {
        let mut context = AppContext::new();
        let (dock, first_tab, second_tab) = two_tab_dock(&mut context, false);
        layout_two_tab_strip(&mut context, dock, first_tab, second_tab);

        assert!(
            context
                .begin_dock_item_drag(document(), 1, first_tab, 20.0, 10.0)
                .unwrap()
        );
        assert!(
            context
                .update_dock_item_drag(document(), 1, 40.0, 10.0)
                .unwrap()
        );
        assert_eq!(
            context
                .read(dock, |dock| {
                    dock.item_drag
                        .as_ref()
                        .and_then(|drag| drag.reorder.clone())
                })
                .unwrap()
                .map(|(id, _)| id),
            Some(Arc::from("first"))
        );
        assert!(
            context
                .end_dock_item_drag(document(), 1, 40.0, 10.0, false)
                .unwrap()
        );
        assert_eq!(
            context.read(dock, |dock| dock.root.flatten()).unwrap(),
            vec![Arc::from("first"), Arc::from("second")]
        );
        assert!(matches!(
            context.read(dock, |dock| dock.root.clone()).unwrap(),
            DockNode::Tabs { .. }
        ));
    }

    #[test]
    fn locked_dock_does_not_reorder() {
        let mut locked = Dock::new(DockNode::tabs(
            ["first", "second"],
            "first",
            [("first", None), ("second", None)],
        ))
        .locked(true);
        assert!(!locked.reorder_tab("first", "second", false));
        assert_eq!(
            locked.flatten(),
            vec![Arc::from("first"), Arc::from("second")]
        );

        let mut context = AppContext::new();
        let (dock, first_tab, second_tab) = two_tab_dock(&mut context, true);
        layout_two_tab_strip(&mut context, dock, first_tab, second_tab);
        assert!(
            !context
                .begin_dock_item_drag(document(), 1, first_tab, 20.0, 10.0)
                .unwrap()
        );
        assert_eq!(
            context.read(dock, |dock| dock.root.flatten()).unwrap(),
            vec![Arc::from("first"), Arc::from("second")]
        );
    }

    #[test]
    fn focused_dock_handle_adjust_changes_ratio_and_clamps() {
        let mut context = AppContext::new();
        let first = body(&mut context, "first");
        let second = body(&mut context, "second");
        let dock = context
            .create_component(
                document(),
                Dock::new(DockNode::split(
                    DockAxis::Horizontal,
                    0.4,
                    DockNode::item("inspector", Some(first)),
                    DockNode::item("console", Some(second)),
                )),
            )
            .unwrap();
        context.assemble_dock(dock).unwrap();
        let handle = find_tag(&context, dock.stable_id(), "dock-handle")[0];
        assert!(context.focus_node(document(), handle).unwrap());
        assert!(
            !context.focus_node(document(), handle).unwrap(),
            "focus_node reports whether focus changed, not whether the node is focused"
        );

        assert!(context.adjust_focused_dock_split(document(), 1.0).unwrap());
        let ratio = context
            .read(dock, |dock| dock.root.split_ratio())
            .unwrap()
            .unwrap();
        assert!((ratio - (0.4 + DOCK_SPLIT_KEYBOARD_STEP)).abs() < f32::EPSILON);

        context
            .update_component(dock, |dock, _| {
                if let DockNode::Split { ratio, .. } = &mut dock.root {
                    *ratio = 0.94;
                }
            })
            .unwrap();
        context.assemble_dock(dock).unwrap();
        if context.world().focused(document()) != Some(handle) {
            assert!(context.focus_node(document(), handle).unwrap());
        }
        assert!(context.adjust_focused_dock_split(document(), 1.0).unwrap());
        assert_eq!(
            context
                .read(dock, |dock| dock.root.split_ratio())
                .unwrap()
                .unwrap(),
            MAX_SPLIT_RATIO
        );
        assert!(!context.adjust_focused_dock_split(document(), 1.0).unwrap());

        context
            .update_component(dock, |dock, _| {
                if let DockNode::Split { ratio, .. } = &mut dock.root {
                    *ratio = 0.06;
                }
            })
            .unwrap();
        context.assemble_dock(dock).unwrap();
        if context.world().focused(document()) != Some(handle) {
            assert!(context.focus_node(document(), handle).unwrap());
        }
        assert!(context.adjust_focused_dock_split(document(), -1.0).unwrap());
        assert_eq!(
            context
                .read(dock, |dock| dock.root.split_ratio())
                .unwrap()
                .unwrap(),
            MIN_SPLIT_RATIO
        );
        assert!(!context.adjust_focused_dock_split(document(), -1.0).unwrap());

        context
            .update_component(dock, |dock, _| {
                dock.locked = true;
                if let DockNode::Split { ratio, .. } = &mut dock.root {
                    *ratio = 0.4;
                }
            })
            .unwrap();
        context.assemble_dock(dock).unwrap();
        assert!(!context.adjust_focused_dock_split(document(), 1.0).unwrap());
        assert_eq!(
            context
                .read(dock, |dock| dock.root.split_ratio())
                .unwrap()
                .unwrap(),
            0.4
        );
    }

    #[test]
    fn eight_pane_keyboard_split_resize_repeats_after_focus() {
        let mut context = AppContext::new();
        let mut contents = [StableNodeId::new(1).unwrap(); 8];
        for (index, slot) in contents.iter_mut().enumerate() {
            *slot = body(&mut context, &format!("pane {index}"));
        }
        fn item(index: usize, content: StableNodeId) -> DockNode {
            DockNode::item(format!("pane-{index}"), Some(content))
        }
        fn split(axis: DockAxis, first: DockNode, second: DockNode) -> DockNode {
            DockNode::split(axis, 0.5, first, second)
        }
        let root = split(
            DockAxis::Horizontal,
            split(
                DockAxis::Vertical,
                split(
                    DockAxis::Horizontal,
                    item(0, contents[0]),
                    item(1, contents[1]),
                ),
                split(
                    DockAxis::Horizontal,
                    item(2, contents[2]),
                    item(3, contents[3]),
                ),
            ),
            split(
                DockAxis::Vertical,
                split(
                    DockAxis::Horizontal,
                    item(4, contents[4]),
                    item(5, contents[5]),
                ),
                split(
                    DockAxis::Horizontal,
                    item(6, contents[6]),
                    item(7, contents[7]),
                ),
            ),
        );
        let dock = context
            .create_component(document(), Dock::new(root))
            .unwrap();
        context.assemble_dock(dock).unwrap();
        assert_eq!(context.read(dock, |dock| dock.flatten().len()).unwrap(), 8);
        let _ = context.layout_document(document(), crate::LayoutViewport::new(1_280.0, 800.0));

        let handle = context
            .world()
            .document_order(document())
            .into_iter()
            .find(|&id| {
                context.is_dock_handle(id)
                    && context
                        .world()
                        .interaction(id)
                        .is_some_and(|interaction| interaction.focusable)
            })
            .expect("assembled dock must expose a focusable split handle");
        assert!(context.focus_node(document(), handle).unwrap());
        assert!(!context.focus_node(document(), handle).unwrap());

        for iteration in 0usize..8 {
            let direction = if iteration.is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            if context.world().focused(document()) != Some(handle) {
                assert!(context.focus_node(document(), handle).unwrap());
            }
            assert!(
                context
                    .adjust_focused_dock_split(document(), direction)
                    .unwrap()
            );
            let _ = context.layout_document(document(), crate::LayoutViewport::new(1_280.0, 800.0));
        }
    }
}

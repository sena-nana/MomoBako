//! Professional tab strip: selection, before-value reorder, close, and drag lease.
//!
//! Application code owns tab values, order persistence, and pane/window
//! identity. This type reports select/reorder/close/transfer results and keeps
//! retained option order in sync with a successful reorder. Scene paints each
//! option through [`crate::SegmentedOption`] / [`crate::StandardVisual::SelectionOption`]
//! with [`crate::SelectionChrome::Tabs`]. Close stays a request: the
//! application owns removal.

use std::sync::Arc;

use nana_ui_core::{ControlSize, Icon, TabDragRect, TabStripPaint, reorder_changes_position};

use crate::selection::{
    RovingFocusIntent, RovingFocusPolicy, SegmentedOption, SelectionChrome, selection_chrome_style,
};
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, LayoutBox,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, UiWorld,
};

/// Option identity stays application-owned. Disabled tabs stay visible.
#[derive(Debug, Clone, PartialEq)]
pub struct TabOption {
    pub value: Arc<str>,
    pub label: Arc<str>,
    pub icon: Option<Icon>,
    pub disabled: bool,
    pub draggable: bool,
    pub closable: bool,
}

impl TabOption {
    pub fn new(value: impl Into<Arc<str>>, label: impl Into<Arc<str>>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            icon: None,
            disabled: false,
            draggable: true,
            closable: false,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Controls whether this tab can be a drag source or drop-before target.
    /// It remains selectable when dragging is disabled.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.draggable = draggable;
        self
    }

    /// Advertises that the application accepts a close request for this tab.
    /// Runtime does not paint a close control.
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    fn drag_blocked(&self) -> bool {
        self.disabled || !self.draggable
    }
}

/// Events reported by the professional tab strip. The application applies
/// close and cross-strip transfer; reorder updates retained option order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabsEvent {
    Select(Arc<str>),
    Reorder {
        value: Arc<str>,
        before: Option<Arc<str>>,
    },
    Close(Arc<str>),
    Transfer {
        source_strip: Arc<str>,
        value: Arc<str>,
        target_strip: Arc<str>,
        before: Option<Arc<str>>,
    },
}

/// Independent tab strip using SegmentedControl tabs chrome.
#[derive(Debug, Clone, PartialEq)]
pub struct Tabs {
    pub selected: Option<Arc<str>>,
    pub options: Vec<TabOption>,
    pub label: Option<Arc<str>>,
    pub size: ControlSize,
    pub fill: bool,
    pub accepts_external_drop: bool,
    pub strip_id: Option<Arc<str>>,
    pub focus: Option<Arc<str>>,
    pub roving_focus: RovingFocusPolicy,
    pub style: NodeStyle,
    pub(crate) option_nodes: Vec<(Arc<str>, StableNodeId)>,
}

impl Tabs {
    /// The surface its options take: size, the tab chrome and fill.
    pub(crate) fn option_surface(&self) -> (ControlSize, SelectionChrome, bool) {
        (self.size, SelectionChrome::Tabs, self.fill)
    }

    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn new(selected: impl Into<Arc<str>>) -> Self {
        let selected = selected.into();
        Self {
            selected: Some(Arc::clone(&selected)),
            options: Vec::new(),
            label: None,
            size: ControlSize::Small,
            fill: false,
            accepts_external_drop: true,
            strip_id: None,
            focus: Some(selected),
            roving_focus: RovingFocusPolicy::default(),
            style: selection_chrome_style(SelectionChrome::Tabs, ControlSize::Small, false),
            option_nodes: Vec::new(),
        }
    }

    pub fn options(mut self, options: impl IntoIterator<Item = TabOption>) -> Self {
        self.options = options.into_iter().collect();
        self
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self.style = selection_chrome_style(SelectionChrome::Tabs, size, self.fill);
        self
    }

    pub fn fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self.style = selection_chrome_style(SelectionChrome::Tabs, self.size, fill);
        self
    }

    pub fn strip_id(mut self, strip_id: impl Into<Arc<str>>) -> Self {
        self.strip_id = Some(strip_id.into());
        self
    }

    /// Controls whether this strip can receive tabs from another strip.
    /// The strip remains a valid drag source when external drops are disabled.
    pub fn accepts_external_drop(mut self, accepts: bool) -> Self {
        self.accepts_external_drop = accepts;
        self
    }

    pub fn option_values(&self) -> impl Iterator<Item = &Arc<str>> {
        self.options.iter().map(|option| &option.value)
    }

    pub fn option_node(&self, value: &str) -> Option<StableNodeId> {
        self.option_nodes
            .iter()
            .find(|(current, _)| current.as_ref() == value)
            .map(|(_, id)| *id)
    }

    pub fn option_nodes(&self) -> &[(Arc<str>, StableNodeId)] {
        &self.option_nodes
    }

    pub fn select(&mut self, value: &str) -> Option<TabsEvent> {
        let option = self
            .options
            .iter()
            .find(|option| option.value.as_ref() == value)?;
        if option.disabled || self.selected.as_deref() == Some(value) {
            return None;
        }
        let value = Arc::clone(&option.value);
        self.selected = Some(Arc::clone(&value));
        self.focus = Some(Arc::clone(&value));
        Some(TabsEvent::Select(value))
    }

    /// Moves `value` so it sits before `before`. `None` appends it to the end.
    pub fn reorder(&mut self, value: &str, before: Option<&str>) -> Option<TabsEvent> {
        let source = self
            .options
            .iter()
            .position(|option| option.value.as_ref() == value)?;
        if self.options[source].drag_blocked() {
            return None;
        }
        if let Some(before) = before {
            let target = self
                .options
                .iter()
                .position(|option| option.value.as_ref() == before)?;
            if self.options[target].drag_blocked() {
                return None;
            }
            if !reorder_changes_position(self.options.len(), source, Some(target)) {
                return None;
            }
        } else if !reorder_changes_position(self.options.len(), source, None) {
            return None;
        }
        let moved = self.options.remove(source);
        let insert_at = before
            .and_then(|before| {
                self.options
                    .iter()
                    .position(|option| option.value.as_ref() == before)
            })
            .unwrap_or(self.options.len());
        let value = Arc::clone(&moved.value);
        let before = before.map(Arc::from);
        self.options.insert(insert_at, moved);
        Some(TabsEvent::Reorder { value, before })
    }

    /// Reports a close request. The application decides whether the tab closes.
    pub fn request_close(&mut self, value: &str) -> Option<TabsEvent> {
        let option = self
            .options
            .iter()
            .find(|option| option.value.as_ref() == value)?;
        if option.disabled || !option.closable {
            return None;
        }
        Some(TabsEvent::Close(Arc::clone(&option.value)))
    }

    /// Reports a cross-strip transfer. Neither strip is mutated here.
    pub fn transfer_to(
        &self,
        target_strip: &str,
        value: &str,
        before: Option<&str>,
    ) -> Option<TabsEvent> {
        let source_strip = self.strip_id.as_ref()?;
        let option = self
            .options
            .iter()
            .find(|option| option.value.as_ref() == value)?;
        if option.drag_blocked() {
            return None;
        }
        Some(TabsEvent::Transfer {
            source_strip: Arc::clone(source_strip),
            value: Arc::clone(&option.value),
            target_strip: Arc::from(target_strip),
            before: before.map(Arc::from),
        })
    }

    pub fn navigate(&mut self, intent: RovingFocusIntent) -> Option<TabsEvent> {
        let items = self
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| (index, !option.disabled))
            .collect::<Vec<_>>();
        let current = self
            .focus
            .as_ref()
            .or(self.selected.as_ref())
            .and_then(|value| {
                self.options
                    .iter()
                    .position(|option| &option.value == value)
            });
        let target = self.roving_focus.resolve(&items, current, intent)?;
        let value = Arc::clone(&self.options[target].value);
        self.focus = Some(Arc::clone(&value));
        self.select(value.as_ref())
    }

    pub fn strip_paint(
        &self,
        bounds: TabDragRect,
        tab_bounds: Vec<TabDragRect>,
    ) -> TabStripPaint<Arc<str>> {
        TabStripPaint {
            bounds,
            tab_bounds,
            values: self
                .options
                .iter()
                .map(|option| Arc::clone(&option.value))
                .collect(),
            disabled: self.options.iter().map(TabOption::drag_blocked).collect(),
            accepts_external_drop: self.accepts_external_drop,
        }
    }

    /// Uses retained option layout boxes after layout. Returns `None` until
    /// every projected option has a box.
    pub fn strip_paint_from_layout(
        &self,
        world: &UiWorld,
        id: StableNodeId,
    ) -> Option<TabStripPaint<Arc<str>>> {
        if self.option_nodes.len() != self.options.len() {
            return None;
        }
        let bounds = tab_drag_rect(world.layout_box(id)?);
        let mut tab_bounds = Vec::with_capacity(self.option_nodes.len());
        for (_, child) in &self.option_nodes {
            tab_bounds.push(tab_drag_rect(world.layout_box(*child)?));
        }
        Some(self.strip_paint(bounds, tab_bounds))
    }

    pub(crate) fn roving_target(&self) -> Option<StableNodeId> {
        let enabled = self
            .options
            .iter()
            .zip(self.option_nodes.iter())
            .filter(|(option, (value, _))| !option.disabled && option.value == *value)
            .map(|(_, (_, id))| *id)
            .collect::<Vec<_>>();
        self.focus
            .as_ref()
            .or(self.selected.as_ref())
            .and_then(|value| self.option_node(value.as_ref()))
            .filter(|id| enabled.contains(id))
            .or_else(|| enabled.first().copied())
    }
}

pub(crate) fn tab_selection_option(option: &TabOption, tabs: &Tabs) -> SegmentedOption {
    let (size, chrome, fill) = tabs.option_surface();
    let mut child = SegmentedOption::new(Arc::clone(&option.label))
        .disabled(option.disabled)
        .with_selected(tabs.selected.as_ref() == Some(&option.value))
        .surface(size, chrome, fill);
    if let Some(icon) = option.icon {
        child = child.icon(icon);
    }
    child
}

fn tab_drag_rect(bounds: LayoutBox) -> TabDragRect {
    TabDragRect::new(bounds.x, bounds.y, bounds.width, bounds.height)
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            selected: None,
            options: Vec::new(),
            label: None,
            size: ControlSize::Small,
            fill: false,
            accepts_external_drop: true,
            strip_id: None,
            focus: None,
            roving_focus: RovingFocusPolicy::default(),
            style: selection_chrome_style(SelectionChrome::Tabs, ControlSize::Small, false),
            option_nodes: Vec::new(),
        }
    }
}

impl ComponentView for Tabs {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::activate_tabs),
        lifecycle: Some(crate::AppContext::sync_tabs_options),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element { tag: "tabs".into() }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.border_radius = None;
        style.background = None;
        style.border = None;
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
                role: AccessibilityRole::TabList,
                label: self.label.clone(),
                value: self.selected.clone(),
                ..AccessibilityState::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;
    use crate::{Entity, NodeKind, SegmentedOption, StandardVisual};
    use nana_ui_core::{LogicalPoint, TabDragGroup, TabDragSurface};

    fn sample() -> Tabs {
        Tabs::new("preview").label("Editor").options([
            TabOption::new("code", "Code").icon(nana_ui_core::Icon::File),
            TabOption::new("preview", "Preview").closable(true),
            TabOption::new("split", "Split").disabled(true),
            TabOption::new("pinned", "Pinned").draggable(false),
        ])
    }

    fn option_children(context: &AppContext, tabs: Entity<Tabs>) -> Vec<StableNodeId> {
        context
            .world()
            .node(tabs.stable_id())
            .map(|node| node.children.clone())
            .unwrap_or_default()
    }

    #[test]
    fn reorder_uses_before_value_and_appends_when_before_is_none() {
        let mut tabs = sample();
        assert_eq!(
            tabs.reorder("preview", Some("code")),
            Some(TabsEvent::Reorder {
                value: Arc::from("preview"),
                before: Some(Arc::from("code")),
            })
        );
        assert_eq!(
            tabs.option_values()
                .map(|value| value.as_ref())
                .collect::<Vec<_>>(),
            ["preview", "code", "split", "pinned"]
        );

        assert_eq!(
            tabs.reorder("preview", None),
            Some(TabsEvent::Reorder {
                value: Arc::from("preview"),
                before: None,
            })
        );
        assert_eq!(
            tabs.option_values()
                .map(|value| value.as_ref())
                .collect::<Vec<_>>(),
            ["code", "split", "pinned", "preview"]
        );
        assert!(tabs.reorder("preview", None).is_none());
    }

    #[test]
    fn close_request_does_not_remove_the_tab() {
        let mut tabs = sample();
        assert_eq!(
            tabs.request_close("preview"),
            Some(TabsEvent::Close(Arc::from("preview")))
        );
        assert_eq!(tabs.options.len(), 4);
        assert!(tabs.request_close("code").is_none());
        assert!(tabs.request_close("split").is_none());
    }

    #[test]
    fn non_draggable_tabs_stay_selectable_and_skip_reorder() {
        let mut tabs = sample();
        assert_eq!(
            tabs.select("pinned"),
            Some(TabsEvent::Select(Arc::from("pinned")))
        );
        assert!(tabs.reorder("pinned", Some("code")).is_none());
        assert!(tabs.reorder("preview", Some("pinned")).is_none());
        assert_eq!(
            tabs.option_values()
                .map(|value| value.as_ref())
                .collect::<Vec<_>>(),
            ["code", "preview", "split", "pinned"]
        );
    }

    #[test]
    fn disabled_tabs_skip_select_reorder_and_close() {
        let mut tabs = sample();
        tabs.options[2].closable = true;
        assert!(tabs.select("split").is_none());
        assert!(tabs.reorder("split", Some("code")).is_none());
        assert!(tabs.reorder("preview", Some("split")).is_none());
        assert!(tabs.request_close("split").is_none());
        assert_eq!(tabs.selected.as_deref(), Some("preview"));
    }

    #[test]
    fn framework_activate_reorder_and_close_update_retained_state() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let tabs = context
            .create_component(document, sample().strip_id("editor"))
            .unwrap();
        let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = std::sync::Arc::clone(&observed);
        context
            .on(tabs, move |_tabs, event: &TabsEvent, _cx| {
                log.lock().expect("tabs events").push(event.clone());
            })
            .unwrap();

        assert!(context.select_tabs_value(tabs, "code").unwrap());
        assert_eq!(
            context
                .read(tabs, |tabs| tabs.selected.clone())
                .unwrap()
                .as_deref(),
            Some("code")
        );
        assert!(context.reorder_tabs(tabs, "code", None).unwrap());
        assert_eq!(
            context
                .read(tabs, |tabs| tabs
                    .option_values()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>())
                .unwrap(),
            ["preview", "split", "pinned", "code"]
        );
        assert!(context.close_tab(tabs, "preview").unwrap());
        assert_eq!(context.read(tabs, |tabs| tabs.options.len()).unwrap(), 4);
        assert!(!context.select_tabs_value(tabs, "split").unwrap());
        assert!(
            !context
                .reorder_tabs(tabs, "pinned", Some("preview"))
                .unwrap()
        );
        assert!(!context.close_tab(tabs, "code").unwrap());

        let events = observed.lock().expect("tabs events");
        assert_eq!(
            events.as_slice(),
            [
                TabsEvent::Select(Arc::from("code")),
                TabsEvent::Reorder {
                    value: Arc::from("code"),
                    before: None,
                },
                TabsEvent::Close(Arc::from("preview")),
            ]
        );
        assert_eq!(
            context
                .world()
                .accessibility(tabs.stable_id())
                .map(|state| state.role),
            Some(AccessibilityRole::TabList)
        );
    }

    #[test]
    fn cross_surface_lease_resolves_one_source_target_and_before() {
        let source = Tabs::new("preview")
            .strip_id("source-pane")
            .accepts_external_drop(false)
            .options([
                TabOption::new("locked", "Locked").draggable(false),
                TabOption::new("code", "Code"),
                TabOption::new("preview", "Preview"),
            ]);
        let target = Tabs::new("other").strip_id("target-pane").options([
            TabOption::new("locked", "Locked").draggable(false),
            TabOption::new("other", "Other"),
            TabOption::new("notes", "Notes"),
        ]);
        let group = TabDragGroup::new();
        let source_surface =
            TabDragSurface::new("source-window").with_physical_geometry(100, 100, 2.0);
        let target_surface =
            TabDragSurface::new("target-window").with_physical_geometry(500, 120, 1.5);
        let source_lease = group.lease(source_surface.clone(), "source-pane");
        let target_lease = group.lease(target_surface.clone(), "target-pane");
        let tab_bounds = (0..3)
            .map(|index| TabDragRect::new(index as f32 * 80.0, 0.0, 76.0, 28.0))
            .collect::<Vec<_>>();
        group.register(
            &source_surface,
            &source_lease.strip_id,
            source_lease.generation,
            source.strip_paint(TabDragRect::new(0.0, 0.0, 236.0, 28.0), tab_bounds.clone()),
        );
        group.register(
            &target_surface,
            &target_lease.strip_id,
            target_lease.generation,
            target.strip_paint(TabDragRect::new(0.0, 0.0, 236.0, 28.0), tab_bounds),
        );
        group.sync_active(
            &source_surface,
            &source_lease.strip_id,
            source_lease.generation,
            2,
            LogicalPoint::new(198.0, 14.0),
            true,
        );

        assert!(group.relay_pointer(&target_surface, LogicalPoint::new(1.0, 14.0)));
        assert_eq!(
            group.finish_relay(
                &target_surface,
                &target_lease.strip_id,
                LogicalPoint::new(1.0, 14.0)
            ),
            Some((
                "source-pane".to_owned(),
                Arc::from("preview"),
                "target-pane".to_owned(),
                Some(Arc::from("other")),
            ))
        );
        assert!(group.take_completed(&source_lease.strip_id, source_lease.generation));
        assert!(!group.take_completed(&source_lease.strip_id, source_lease.generation));
        assert_eq!(
            source.transfer_to("target-pane", "preview", Some("other")),
            Some(TabsEvent::Transfer {
                source_strip: Arc::from("source-pane"),
                value: Arc::from("preview"),
                target_strip: Arc::from("target-pane"),
                before: Some(Arc::from("other")),
            })
        );
        assert!(source.transfer_to("target-pane", "locked", None).is_none());
    }

    #[test]
    fn projecting_tabs_creates_one_option_child_per_tab() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let tabs = context.create_component(document, sample()).unwrap();
        let children = option_children(&context, tabs);
        assert_eq!(children.len(), 4);
        let nodes = context
            .read(tabs, |tabs| tabs.option_nodes().to_vec())
            .unwrap();
        assert_eq!(
            nodes
                .iter()
                .map(|(value, _)| value.as_ref())
                .collect::<Vec<_>>(),
            ["code", "preview", "split", "pinned"]
        );
        assert_eq!(
            nodes.iter().map(|(_, id)| *id).collect::<Vec<_>>(),
            children
        );
        for child in children {
            assert!(
                context
                    .world()
                    .node(child)
                    .is_some_and(|node| node.kind == NodeKind::Element { tag: "tab".into() })
            );
        }
    }

    #[test]
    fn projected_options_publish_selection_option_state() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let tabs = context.create_component(document, sample()).unwrap();
        let children = option_children(&context, tabs);
        let code = Entity::<SegmentedOption>::from_stable_id(children[0]);
        let preview = Entity::<SegmentedOption>::from_stable_id(children[1]);
        let split = Entity::<SegmentedOption>::from_stable_id(children[2]);

        assert!(!context.read(code, SegmentedOption::selected).unwrap());
        assert!(!context.read(code, SegmentedOption::disabled_value).unwrap());
        assert_eq!(
            context.read(code, |option| option.icon).unwrap(),
            Some(nana_ui_core::Icon::File)
        );
        assert!(context.read(preview, SegmentedOption::selected).unwrap());
        assert!(
            !context
                .read(preview, SegmentedOption::disabled_value)
                .unwrap()
        );
        assert!(
            context
                .read(split, SegmentedOption::disabled_value)
                .unwrap()
        );
        assert!(!context.read(split, SegmentedOption::selected).unwrap());

        let Some(StandardVisual::SelectionOption {
            icon,
            selected: false,
            disabled: false,
            show_focus_ring: false,
            ..
        }) = context.world().standard_visual(children[0])
        else {
            panic!("code option visual");
        };
        assert_eq!(icon, Some(nana_ui_core::Icon::File));
        assert!(matches!(
            context.world().standard_visual(children[1]),
            Some(StandardVisual::SelectionOption {
                selected: true,
                disabled: false,
                show_focus_ring: false,
                ..
            })
        ));
        assert!(matches!(
            context.world().standard_visual(children[2]),
            Some(StandardVisual::SelectionOption {
                selected: false,
                disabled: true,
                show_focus_ring: false,
                ..
            })
        ));
    }

    #[test]
    fn reorder_updates_retained_child_order() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let tabs = context.create_component(document, sample()).unwrap();
        let before = option_children(&context, tabs);
        assert!(context.reorder_tabs(tabs, "preview", Some("code")).unwrap());
        let after = option_children(&context, tabs);
        assert_eq!(after, [before[1], before[0], before[2], before[3]]);
        assert_eq!(
            context
                .read(tabs, |tabs| tabs
                    .option_nodes()
                    .iter()
                    .map(|(value, _)| value.to_string())
                    .collect::<Vec<_>>())
                .unwrap(),
            ["preview", "code", "split", "pinned"]
        );
    }

    #[test]
    fn close_request_does_not_remove_children() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let tabs = context.create_component(document, sample()).unwrap();
        let before = option_children(&context, tabs);
        assert!(context.close_tab(tabs, "preview").unwrap());
        assert_eq!(option_children(&context, tabs), before);
        assert_eq!(context.read(tabs, |tabs| tabs.options.len()).unwrap(), 4);
    }

    #[test]
    fn activating_a_painted_option_selects_through_tabs() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let tabs = context.create_component(document, sample()).unwrap();
        let children = option_children(&context, tabs);
        let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = std::sync::Arc::clone(&observed);
        context
            .on(tabs, move |_tabs, event: &TabsEvent, _cx| {
                log.lock().expect("tabs events").push(event.clone());
            })
            .unwrap();

        assert!(context.activate_node(children[0]).unwrap());
        assert_eq!(
            context
                .read(tabs, |tabs| tabs.selected.clone())
                .unwrap()
                .as_deref(),
            Some("code")
        );
        assert!(
            context
                .read(
                    Entity::<SegmentedOption>::from_stable_id(children[0]),
                    SegmentedOption::selected
                )
                .unwrap()
        );
        assert!(
            !context
                .read(
                    Entity::<SegmentedOption>::from_stable_id(children[1]),
                    SegmentedOption::selected
                )
                .unwrap()
        );
        assert!(!context.activate_node(children[2]).unwrap());
        assert_eq!(
            observed.lock().expect("tabs events").as_slice(),
            [TabsEvent::Select(Arc::from("code"))]
        );
    }

    #[test]
    fn strip_paint_uses_projected_option_layout_boxes() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let tabs = context.create_component(document, sample()).unwrap();
        let children = option_children(&context, tabs);
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            tabs.stable_id(),
            LayoutBox {
                x: 10.0,
                y: 4.0,
                width: 320.0,
                height: 28.0,
            },
        );
        for (index, child) in children.iter().enumerate() {
            mutations.write_layout(
                *child,
                LayoutBox {
                    x: 10.0 + index as f32 * 80.0,
                    y: 4.0,
                    width: 76.0,
                    height: 28.0,
                },
            );
        }
        context.commit_mutations(mutations).unwrap();
        let paint = context.tabs_strip_paint(tabs).unwrap().unwrap();
        assert_eq!(paint.bounds, TabDragRect::new(10.0, 4.0, 320.0, 28.0));
        assert_eq!(
            paint.tab_bounds,
            (0..4)
                .map(|index| TabDragRect::new(10.0 + index as f32 * 80.0, 4.0, 76.0, 28.0))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            paint
                .values
                .iter()
                .map(|value| value.as_ref())
                .collect::<Vec<_>>(),
            ["code", "preview", "split", "pinned"]
        );
    }
}

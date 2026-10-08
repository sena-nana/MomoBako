//! Flattened disclosure tree. Application owns node identities and expansion.

use std::sync::Arc;

use nana_ui_core::{
    ControlSize, Icon, LengthSpec, TreeNavigation, TreeNode, TreeViewEvent, tree_navigation_event,
};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentTextRegion, ComponentView, InteractionState,
    LayoutBox, MutationQueue, NodeKind, NodeStyle, StableNodeId, StandardVisual, UiWorld,
};

const ROW_GAP: f32 = nana_ui_core::space::XXS;
use crate::popover::TREE_DEPTH_STEP as DEPTH_STEP;
const DISCLOSURE_SIZE: f32 = nana_ui_core::type_scale::LINE;
const ICON_SIZE: f32 = nana_ui_core::type_scale::META;

/// Visible-row tree. Hidden collapsed children are not projected.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeView {
    pub nodes: Vec<TreeNode<Arc<str>>>,
    pub size: ControlSize,
    pub style: NodeStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRowData {
    pub id: Arc<str>,
    pub label: Arc<str>,
    pub icon: Option<Icon>,
    pub depth: u8,
    pub branch: bool,
    pub expanded: bool,
    pub selected: bool,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TreeRowGeometry {
    pub id: Arc<str>,
    pub bounds: LayoutBox,
    pub disclosure: Option<LayoutBox>,
    pub icon: Option<(Icon, LayoutBox, [f32; 4])>,
    pub label: ComponentTextRegion,
    pub selected: bool,
    pub disabled: bool,
    pub expanded: bool,
    pub background: Option<[f32; 4]>,
}

impl TreeView {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn new(nodes: impl IntoIterator<Item = TreeNode<Arc<str>>>) -> Self {
        Self {
            nodes: nodes.into_iter().collect(),
            size: ControlSize::Small,
            style: NodeStyle::default(),
        }
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn visible_rows(&self) -> Vec<TreeRowData> {
        let mut rows = Vec::new();
        collect_rows(&self.nodes, 0, &mut rows);
        rows
    }

    pub fn selected_id(&self) -> Option<&Arc<str>> {
        find_selected(&self.nodes)
    }

    pub fn apply_event(&mut self, event: TreeViewEvent<Arc<str>>) -> bool {
        match event {
            TreeViewEvent::Toggle(id) => toggle_node(&mut self.nodes, &id),
            TreeViewEvent::Select(id) => select_node(&mut self.nodes, &id),
        }
    }

    pub fn navigate(&mut self, navigation: TreeNavigation) -> Option<TreeViewEvent<Arc<str>>> {
        let event = tree_navigation_event(&self.nodes, self.selected_id(), navigation)?;
        self.apply_event(event.clone());
        Some(event)
    }

    fn intrinsic_height(&self, metrics: nana_ui_core::ThemeMetrics) -> f32 {
        let count = self.visible_rows().len().max(1) as f32;
        count * self.size.height_in(metrics) + (count - 1.0) * ROW_GAP
    }
}

impl ComponentView for TreeView {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activate_at: Some(crate::AppContext::activate_tree_at),
        hooks: Some(|hooks| hooks.tree = Some(&TREE_HOOKS)),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element { tag: "tree".into() }
    }

    fn wants_metrics_reproject() -> bool {
        true
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let rows = self.visible_rows();
        let visual = StandardVisual::TreeView {
            rows: rows.into(),
            size: self.size,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        if layout.width.is_none() {
            layout.width = Some(LengthSpec::Fill);
        }
        // Boxes are border-box, so declared padding has to be added on top of
        // the row stack; otherwise it eats the last row. A caller that sets its
        // own height (a tree inside a fixed viewport) keeps it.
        let padding = layout.resolved_padding();
        let content = self.intrinsic_height(world.theme_metrics()) + padding.top + padding.bottom;
        if layout.height.is_none() {
            layout.height = Some(LengthSpec::Px(content));
        }
        if layout.min_height.is_none() {
            layout.min_height = Some(LengthSpec::Px(content));
        }
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
                role: AccessibilityRole::List,
                label: self.selected_id().cloned(),
                disabled: false,
                ..AccessibilityState::default()
            },
        );
    }
}

fn collect_rows(nodes: &[TreeNode<Arc<str>>], depth: u8, rows: &mut Vec<TreeRowData>) {
    for node in nodes {
        rows.push(TreeRowData {
            id: Arc::clone(&node.id),
            label: Arc::from(node.label.as_str()),
            icon: node.icon,
            depth,
            branch: node.branch,
            expanded: node.expanded,
            selected: node.selected,
            disabled: node.disabled,
        });
        if node.branch && node.expanded {
            collect_rows(&node.children, depth.saturating_add(1), rows);
        }
    }
}

fn find_selected(nodes: &[TreeNode<Arc<str>>]) -> Option<&Arc<str>> {
    for node in nodes {
        if node.selected {
            return Some(&node.id);
        }
        if let Some(id) = find_selected(&node.children) {
            return Some(id);
        }
    }
    None
}

fn toggle_node(nodes: &mut [TreeNode<Arc<str>>], id: &Arc<str>) -> bool {
    for node in nodes {
        if &node.id == id {
            if !node.branch {
                return false;
            }
            node.expanded = !node.expanded;
            return true;
        }
        if toggle_node(&mut node.children, id) {
            return true;
        }
    }
    false
}

fn select_node(nodes: &mut [TreeNode<Arc<str>>], id: &Arc<str>) -> bool {
    let mut changed = false;
    for node in nodes {
        let selected = &node.id == id && !node.disabled;
        if node.selected != selected {
            node.selected = selected;
            changed = true;
        }
        changed |= select_node(&mut node.children, id);
    }
    changed
}

pub(crate) fn tree_view_geometry(
    bounds: LayoutBox,
    rows: &[TreeRowData],
    size: ControlSize,
    palette: &nana_ui_core::SemanticPalette,
    metrics: nana_ui_core::ThemeMetrics,
) -> crate::ComponentGeometry {
    let row_height = size.height_in(metrics);
    let geometry = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let y = bounds.y + index as f32 * (row_height + ROW_GAP);
            let row_bounds = LayoutBox {
                x: bounds.x,
                y,
                width: bounds.width,
                height: row_height,
            };
            let indent = f32::from(row.depth) * DEPTH_STEP;
            let mut cursor = bounds.x + indent + 4.0;
            let disclosure = row.branch.then(|| {
                let box_ = LayoutBox {
                    x: cursor,
                    y: y + (row_height - DISCLOSURE_SIZE) / 2.0,
                    width: DISCLOSURE_SIZE,
                    height: DISCLOSURE_SIZE,
                };
                cursor += DISCLOSURE_SIZE;
                box_
            });
            let icon = row.icon.map(|icon| {
                let box_ = LayoutBox {
                    x: cursor,
                    y: y + (row_height - ICON_SIZE) / 2.0,
                    width: ICON_SIZE,
                    height: ICON_SIZE,
                };
                cursor += ICON_SIZE + 4.0;
                (
                    icon,
                    box_,
                    if row.disabled {
                        palette.faint.as_rgba_array()
                    } else {
                        palette.muted.as_rgba_array()
                    },
                )
            });
            TreeRowGeometry {
                id: Arc::clone(&row.id),
                bounds: row_bounds,
                disclosure,
                icon,
                label: ComponentTextRegion {
                    bounds: LayoutBox {
                        x: cursor,
                        y,
                        width: (bounds.x + bounds.width - cursor - 4.0).max(0.0),
                        height: row_height,
                    },
                    content: Arc::clone(&row.label).into(),
                    color: Some(if row.disabled {
                        palette.faint.as_rgba_array()
                    } else {
                        palette.text.as_rgba_array()
                    }),
                    font_size: size.text_size(),
                    font_weight: row.selected.then_some(600),
                },
                selected: row.selected,
                disabled: row.disabled,
                expanded: row.expanded,
                background: row.selected.then_some(palette.selected.as_rgba_array()),
            }
        })
        .collect();
    crate::ComponentGeometry::TreeView { rows: geometry }
}

pub(crate) fn tree_row_at(rows: &[TreeRowGeometry], x: f32, y: f32) -> Option<usize> {
    rows.iter().position(|row| row.bounds.contains(x, y))
}

pub(crate) fn tree_disclosure_at(rows: &[TreeRowGeometry], x: f32, y: f32) -> Option<usize> {
    rows.iter().position(|row| {
        row.disclosure
            .is_some_and(|disclosure| disclosure.contains(x, y))
    })
}

/// The router reaches this picker's keyboard only once one exists.
pub(crate) static TREE_HOOKS: crate::framework::NavigateHooks<crate::TreeNavigation> =
    crate::framework::NavigateHooks {
        navigate: crate::AppContext::navigate_focused_tree,
    };

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn sample() -> TreeView {
        TreeView::new([
            TreeNode::branch(
                Arc::from("src"),
                "src",
                true,
                [
                    TreeNode::leaf(Arc::from("lib"), "lib.rs"),
                    TreeNode::leaf(Arc::from("main"), "main.rs"),
                ],
            )
            .selected(true),
            TreeNode::leaf(Arc::from("readme"), "README.md"),
        ])
    }

    #[test]
    fn visible_rows_follow_expansion() {
        let mut tree = sample();
        assert_eq!(tree.visible_rows().len(), 4);
        assert!(tree.apply_event(TreeViewEvent::Toggle(Arc::from("src"))));
        assert_eq!(tree.visible_rows().len(), 2);
        let event = tree.navigate(TreeNavigation::Next).expect("next");
        assert_eq!(event, TreeViewEvent::Select(Arc::from("readme")));
        assert_eq!(tree.selected_id().map(Arc::as_ref), Some("readme"));
    }

    #[test]
    fn tree_projects_a_single_retained_surface() {
        let mut context = AppContext::new();
        let tree = context.create_component(document(), sample()).unwrap();
        let id = tree.stable_id();
        assert!(matches!(
            context.world().standard_visual(id),
            Some(StandardVisual::TreeView { ref rows, .. }) if rows.len() == 4
        ));
    }
}

#![cfg(feature = "accesskit-tree")]

use accesskit::{Action, NodeId, Role};
use nana_ui::AccessTreeProjector;
use nana_ui_core::{LayoutStyle, VisibilitySpec};
use nana_ui_runtime::{
    AccessibilityRole, AccessibilityState, DocumentId, LayoutBox, MutationQueue, NodeKind,
    NodeStyle, StableNodeId, UiWorld,
};
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn hidden_runtime_parent_projects_a_connected_neutral_accesskit_container() {
    let document = DocumentId::new(1).unwrap();
    let id = |value| StableNodeId::new(value).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for value in 1..=3 {
        queue.create(
            id(value),
            document,
            if value == 1 {
                NodeKind::Document
            } else {
                NodeKind::Element { tag: "div".into() }
            },
        );
        if value > 1 {
            queue.insert(id(value - 1), id(value), None);
        }
        queue.write_layout(
            id(value),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );
        let mut layout = LayoutStyle::default();
        layout.paint.visibility = Some(if value == 2 {
            VisibilitySpec::Hidden
        } else {
            VisibilitySpec::Visible
        });
        queue.set_style(
            id(value),
            NodeStyle {
                layout: Arc::new(layout),
                ..Default::default()
            },
        );
    }
    queue.set_accessibility(
        id(2),
        AccessibilityState {
            role: AccessibilityRole::Dialog,
            label: Some("Container label".into()),
            value: Some("Container value".into()),
            ..Default::default()
        },
    );
    queue.set_accessibility(
        id(3),
        AccessibilityState {
            role: AccessibilityRole::Button,
            label: Some("Visible action".into()),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let mut projector = AccessTreeProjector::new(world.project_accessibility(document), true, 1.0);
    for visibility in [
        VisibilitySpec::Hidden,
        VisibilitySpec::Visible,
        VisibilitySpec::Hidden,
    ] {
        let mut layout = LayoutStyle::default();
        layout.paint.visibility = Some(visibility);
        let mut queue = MutationQueue::new();
        queue.set_style(
            id(2),
            NodeStyle {
                layout: Arc::new(layout),
                ..Default::default()
            },
        );
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        projector.apply_delta(world.project_accessibility_delta(&work));
        let update = projector.full_update();
        let nodes = update
            .nodes
            .iter()
            .map(|(id, node)| (*id, node))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(nodes[&NodeId(1)].children(), &[NodeId(2)]);
        assert_eq!(nodes[&NodeId(2)].children(), &[NodeId(3)]);
        assert_eq!(nodes[&NodeId(3)].role(), Role::Button);
        assert!(nodes[&NodeId(3)].supports_action(Action::Click));
        if visibility == VisibilitySpec::Hidden {
            let container = nodes[&NodeId(2)];
            assert_eq!(container.role(), Role::GenericContainer);
            assert!(container.label().is_none() && container.value().is_none());
            assert!(!container.supports_action(Action::Click));
            assert!(!container.supports_action(Action::Focus));
        } else {
            assert_eq!(nodes[&NodeId(2)].role(), Role::Dialog);
            assert_eq!(nodes[&NodeId(2)].label(), Some("Container label"));
        }
        let fresh = AccessTreeProjector::new(world.project_accessibility(document), true, 1.0)
            .full_update();
        assert_eq!(update.nodes, fresh.nodes);
    }
    let mut remove = MutationQueue::new();
    remove.despawn_subtree(id(2));
    world.commit(remove).unwrap();
    let work = world.take_system_work();
    projector.apply_delta(world.project_accessibility_delta(&work));
    let update = projector.full_update();
    assert_eq!(update.nodes.len(), 1);
    assert_eq!(update.nodes[0].0, NodeId(1));
    assert!(update.nodes[0].1.children().is_empty());
}

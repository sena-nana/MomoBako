//! Reverse references for overlay validation and teardown.
use super::*;

impl UiWorld {
    pub(super) fn write_overlay_host(
        &mut self,
        host: StableNodeId,
        state: Option<OverlayHostState>,
    ) {
        let previous = self.nodes.overlay_host(host).copied();
        if previous == state {
            return;
        }
        for target in previous
            .into_iter()
            .flat_map(|state| [state.active, state.restore_focus])
            .flatten()
        {
            if let Some(hosts) = self.overlay_dependents.get_mut(&target) {
                hosts.remove(&host);
                if hosts.is_empty() {
                    self.overlay_dependents.remove(&target);
                }
            }
        }
        let document = self.document_of(host).expect("overlay host exists");
        if state.is_some() {
            self.overlay_hosts_by_document
                .entry(document)
                .or_default()
                .insert(host);
        } else if let Some(hosts) = self.overlay_hosts_by_document.get_mut(&document) {
            hosts.remove(&host);
            if hosts.is_empty() {
                self.overlay_hosts_by_document.remove(&document);
            }
        }
        self.nodes.set_overlay_host(host, state);
        if state.is_some() {
            self.overlay_host_nodes.insert(host);
        } else {
            self.overlay_host_nodes.remove(&host);
        }
        for target in state
            .into_iter()
            .flat_map(|state| [state.active, state.restore_focus])
            .flatten()
        {
            self.overlay_dependents
                .entry(target)
                .or_default()
                .insert(host);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::mutation::ValidationPlan;
    use super::*;

    fn id(value: u64) -> StableNodeId {
        StableNodeId::new(value).unwrap()
    }
    fn menu() -> AccessibilityState {
        AccessibilityState {
            role: AccessibilityRole::Menu,
            ..Default::default()
        }
    }
    fn fixture(hosts: u64) -> UiWorld {
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        for index in 0..hosts {
            let document = DocumentId::new(index + 1).unwrap();
            let host = id(index * 3 + 1);
            let surface = id(index * 3 + 2);
            let restore = id(index * 3 + 3);
            queue.create(host, document, NodeKind::Document);
            queue.create(surface, document, NodeKind::Text);
            queue.create(restore, document, NodeKind::Text);
            queue.insert(host, surface, None);
            queue.set_accessibility(surface, menu());
            queue.set_overlay_host(
                host,
                OverlayHostState {
                    active: Some(surface),
                    restore_focus: Some(restore),
                },
            );
        }
        world.commit(queue).unwrap();
        world
    }

    #[test]
    fn non_structural_updates_do_not_validate_a_thousand_unrelated_hosts() {
        let mut world = fixture(1000);
        let mut queue = MutationQueue::new();
        queue.set_scroll_offset(id(1), ScrollOffset { x: 0.0, y: 1.0 });
        queue.set_text(
            id(1),
            TextContent {
                value: "updated".into(),
            },
        );
        queue.set_style(id(1), NodeStyle::default());
        queue.write_layout(
            id(1),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        let mut plan = ValidationPlan::new(&world);
        plan.validate(queue.as_slice()).unwrap();
        assert_eq!(plan.scanned, 0);
        assert!(plan.nodes.is_empty());
        drop(plan);
        world.commit(queue).unwrap();
        let mut queue = MutationQueue::new();
        queue.set_accessibility(id(2), menu());
        let mut plan = ValidationPlan::new(&world);
        plan.validate(queue.as_slice()).unwrap();
        assert!(plan.scanned <= 2);
        drop(plan);
        let before = world.generation();
        let mut queue = MutationQueue::new();
        queue.set_accessibility(id(2), AccessibilityState::default());
        assert_eq!(
            world.commit(queue),
            Err(UiWorldError::InvalidOverlayHost(id(1)))
        );
        assert_eq!(world.generation(), before);
        let mut queue = MutationQueue::new();
        queue.detach(id(2));
        assert_eq!(
            world.commit(queue),
            Err(UiWorldError::InvalidOverlayHost(id(1)))
        );
        assert_eq!(world.parent_id(id(2)), Some(id(1)));
    }

    #[test]
    fn modal_focus_and_staged_order_stay_within_the_document() {
        let mut world = fixture(1000);
        let document = DocumentId::new(1).unwrap();
        let mut queue = MutationQueue::new();
        queue.set_accessibility(
            id(2),
            AccessibilityState {
                modal: true,
                ..menu()
            },
        );
        world.commit(queue).unwrap();
        let mut plan = ValidationPlan::new(&world);
        assert!(plan.active_modal_allows_focus(document, id(2)).unwrap());
        assert!(!plan.active_modal_allows_focus(document, id(3)).unwrap());
        assert!(plan.scanned <= 8);
        assert!(
            plan.nodes
                .keys()
                .all(|node| [id(1), id(2), id(3)].contains(node))
        );
        drop(plan);

        // Detached content leaves both staged and published document order.
        let mut queue = MutationQueue::new();
        queue.set_overlay_host(id(1), OverlayHostState::default());
        queue.detach(id(2));
        let mut plan = ValidationPlan::new(&world);
        plan.validate(queue.as_slice()).unwrap();
        assert_eq!(
            plan.planned_document_order(document).unwrap(),
            vec![id(1), id(3)]
        );
        assert!(!plan.focus_target_visible(id(2)).unwrap());
        drop(plan);
        world.commit(queue).unwrap();
        assert_eq!(world.document_order(document), vec![id(1), id(3)]);
        let mut queue = MutationQueue::new();
        queue.insert(id(1), id(2), None);
        let mut plan = ValidationPlan::new(&world);
        plan.validate(queue.as_slice()).unwrap();
        assert_eq!(
            plan.planned_document_order(document).unwrap(),
            vec![id(1), id(2), id(3)]
        );
        assert!(!plan.focus_target_visible(id(2)).unwrap());
        drop(plan);
        world.commit(queue).unwrap();
        assert_eq!(world.document_order(document), vec![id(1), id(2), id(3)]);
    }

    #[test]
    fn staged_references_validate_new_targets_and_release_replaced_targets() {
        let mut world = fixture(1);
        let mut queue = MutationQueue::new();
        queue.create(id(4), DocumentId::new(1).unwrap(), NodeKind::Text);
        queue.insert(id(1), id(4), None);
        queue.set_accessibility(id(4), menu());
        queue.set_overlay_host(
            id(1),
            OverlayHostState {
                active: Some(id(4)),
                restore_focus: Some(id(3)),
            },
        );
        queue.set_accessibility(id(4), AccessibilityState::default());
        assert_eq!(
            world.commit(queue),
            Err(UiWorldError::InvalidOverlayHost(id(1)))
        );
        assert!(!world.contains(id(4)));
        let mut queue = MutationQueue::new();
        queue.set_overlay_host(id(1), OverlayHostState::default());
        queue.set_accessibility(id(2), AccessibilityState::default());
        world.commit(queue).unwrap();
        assert!(world.overlay_dependents.is_empty());
        assert_eq!(world.overlay_host(id(1)), Some(OverlayHostState::default()));
    }

    #[test]
    fn overlay_teardown_releases_reverse_references_without_touching_other_documents() {
        let mut world = fixture(1000);
        let untouched = world.overlay_host(id(4));
        for index in 0..128 {
            let surface = id(10_000 + index);
            let mut queue = MutationQueue::new();
            queue.create(surface, DocumentId::new(1).unwrap(), NodeKind::Text);
            queue.insert(id(1), surface, None);
            queue.set_accessibility(surface, menu());
            queue.set_overlay_host(
                id(1),
                OverlayHostState {
                    active: Some(surface),
                    restore_focus: Some(id(3)),
                },
            );
            world.commit(queue).unwrap();
            assert!(world.overlay_dependents.contains_key(&surface));
            let mut queue = MutationQueue::new();
            queue.despawn_subtree(surface);
            world.commit(queue).unwrap();
            assert!(!world.overlay_dependents.contains_key(&surface));
            assert!(!world.overlay_dependents.contains_key(&id(3)));
            assert_eq!(world.overlay_host(id(4)), untouched);
            assert_eq!(world.overlay_dependents.len(), 1998);
        }
        let mut queue = MutationQueue::new();
        for index in 0..1000 {
            queue.despawn_subtree(id(index * 3 + 1));
        }
        world.commit(queue).unwrap();
        assert!(world.overlay_dependents.is_empty());
        assert!(world.overlay_host_nodes.is_empty());
        assert!(world.overlay_hosts_by_document.is_empty());
        // Other roots in every document survive host teardown.
        for index in 0..1000 {
            assert_eq!(
                world.document_order(DocumentId::new(index + 1).unwrap()),
                vec![id(index * 3 + 3)]
            );
        }
    }
}

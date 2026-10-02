//! Scene host accessibility coordination.

use super::*;

/// One unpresented batch can stay incremental. Multiple batches may contain
/// intermediate parent removal/reparenting, so publish the final tree instead
/// of discarding a batch or guessing how to combine their tombstones.
#[derive(Default)]
pub(super) enum PendingAccessibility {
    #[default]
    None,
    Delta(nana_ui_runtime::AccessibilityDelta),
    Snapshot,
}

impl PendingAccessibility {
    pub(super) fn stage(&mut self, delta: nana_ui_runtime::AccessibilityDelta) {
        if delta.updated.is_empty() && delta.removed.is_empty() {
            return;
        }
        *self = match self {
            Self::None => Self::Delta(delta),
            Self::Delta(_) | Self::Snapshot => Self::Snapshot,
        };
    }

    #[cfg(not(target_os = "android"))]
    fn take_for_publication(&mut self) -> (Option<AccessibilityUpdate>, bool) {
        match std::mem::take(self) {
            Self::None => (None, false),
            Self::Delta(delta) => (Some(AccessibilityUpdate::Delta(delta)), false),
            Self::Snapshot => (None, true),
        }
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod pending_tests {
    use super::*;
    use nana_ui_runtime::{
        AccessibilityDelta, AccessibilityState, DocumentId, LayoutBox, MutationQueue, NodeKind,
        UiWorld,
    };

    fn commit(world: &mut UiWorld, queue: MutationQueue) -> AccessibilityDelta {
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        world.project_accessibility_delta(&work)
    }

    #[test]
    fn first_publication_rebuilds_the_base_missing_from_a_partial_delta() {
        let document = DocumentId::new(1).unwrap();
        let id = |value| StableNodeId::new(value).unwrap();
        let mut world = UiWorld::new();
        let mut initial = MutationQueue::new();
        for value in [1, 2] {
            initial.create(id(value), document, NodeKind::Element { tag: "div".into() });
            initial.write_layout(
                id(value),
                LayoutBox {
                    width: 100.0,
                    height: 20.0,
                    ..Default::default()
                },
            );
        }
        initial.insert(id(1), id(2), None);
        commit(&mut world, initial); // Application already consumed initial work.
        let mut edit = MutationQueue::new();
        edit.set_accessibility(
            id(2),
            AccessibilityState {
                label: Some("Latest".into()),
                ..Default::default()
            },
        );
        let delta = AccessibilityUpdate::Delta(commit(&mut world, edit));
        let expected = world.project_accessibility(document);
        assert_eq!(expected.len(), 2);
        for from_program in [false, true] {
            let snapshots = std::cell::Cell::new(0);
            let update = next_accessibility_update(
                (!from_program).then(|| delta.clone()),
                from_program.then(|| delta.clone()),
                false,
                None,
                Some(world.generation()),
                || {
                    snapshots.set(snapshots.get() + 1);
                    world.project_accessibility(document)
                },
            );
            assert_eq!(
                update,
                Some(AccessibilityUpdate::Full {
                    generation: Some(world.generation()),
                    nodes: expected.clone()
                })
            );
            assert_eq!(snapshots.get(), 1);
        }
    }

    #[test]
    fn first_publication_accepts_an_explicit_complete_source_without_reprojection() {
        let full = AccessibilityUpdate::Full {
            generation: Some(3),
            nodes: Vec::new(),
        };
        assert_eq!(
            next_accessibility_update(None, Some(full.clone()), false, None, Some(3), || panic!(
                "explicit full source already contains the base tree"
            )),
            Some(full)
        );
    }

    #[test]
    fn retries_publish_all_committed_changes_from_one_final_snapshot() {
        let document = DocumentId::new(1).unwrap();
        let id = |value| StableNodeId::new(value).unwrap();
        let create = |queue: &mut MutationQueue, value| {
            queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
            queue.write_layout(
                id(value),
                LayoutBox {
                    width: 100.0,
                    height: 20.0,
                    ..Default::default()
                },
            );
            if value != 1 {
                queue.insert(id(1), id(value), None);
            }
        };
        let mut world = UiWorld::new();
        let mut initial = MutationQueue::new();
        create(&mut initial, 1);
        create(&mut initial, 2);
        commit(&mut world, initial);
        let published_generation = world.generation();

        let mut pending = PendingAccessibility::default();
        let mut added = MutationQueue::new();
        create(&mut added, 3);
        pending.stage(commit(&mut world, added));
        // Surface acquisition/encoding did not complete, so nothing is drained.
        let mut edited = MutationQueue::new();
        edited.set_accessibility(
            id(2),
            AccessibilityState {
                label: Some("Latest".into()),
                ..Default::default()
            },
        );
        pending.stage(commit(&mut world, edited));
        pending.stage(AccessibilityDelta {
            generation: world.generation(),
            updated: vec![],
            removed: vec![],
        });

        let (queued, resnapshot) = pending.take_for_publication();
        let snapshots = std::cell::Cell::new(0);
        let Some(AccessibilityUpdate::Full { generation, nodes }) = next_accessibility_update(
            queued,
            None,
            resnapshot,
            Some(published_generation),
            Some(world.generation()),
            || {
                snapshots.set(snapshots.get() + 1);
                world.project_accessibility(document)
            },
        ) else {
            panic!("both unpresented transactions must reach the native tree")
        };
        assert_eq!(snapshots.get(), 1);
        assert_eq!(generation, Some(world.generation()));
        assert!(nodes.iter().any(|node| node.id == id(3)));
        assert_eq!(
            nodes
                .iter()
                .find(|node| node.id == id(2))
                .unwrap()
                .label
                .as_deref(),
            Some("Latest")
        );
        assert_eq!(nodes, world.project_accessibility(document));
        assert_eq!(pending.take_for_publication(), (None, false));
    }

    #[test]
    fn idle_retries_preserve_a_single_delta_and_targets_remain_independent() {
        let delta = |generation| AccessibilityDelta {
            generation,
            updated: vec![],
            removed: vec![StableNodeId::new(generation).unwrap()],
        };
        let mut first = PendingAccessibility::default();
        let mut second = PendingAccessibility::default();
        first.stage(delta(1));
        second.stage(delta(1));
        for generation in 2..=100 {
            first.stage(delta(generation));
            second.stage(AccessibilityDelta {
                generation: 1,
                updated: vec![],
                removed: vec![],
            });
        }
        assert_eq!(first.take_for_publication(), (None, true));
        assert_eq!(
            second.take_for_publication(),
            (Some(AccessibilityUpdate::Delta(delta(1))), false)
        );
        first.stage(delta(101));
        assert_eq!(
            first.take_for_publication(),
            (Some(AccessibilityUpdate::Delta(delta(101))), false)
        );
    }
}

impl<Program: RuntimeProgram> WindowManager<Program> {
    #[cfg(not(target_os = "android"))]
    pub(super) fn take_accessibility_actions(
        &self,
        id: WindowId,
    ) -> Vec<nana_ui_runtime::AccessibilityActionRequest> {
        self.window_contexts
            .get(&id)
            .and_then(|host| host.accessibility.as_ref())
            .map_or_else(Vec::new, HostedAccessibility::take_actions)
    }
    #[cfg(not(target_os = "android"))]
    pub(super) fn synchronize_accessibility(&mut self, id: WindowId) {
        let scale_factor = self.scale_factor(id);
        let has_adapter = self
            .window_contexts
            .get(&id)
            .is_some_and(|host| host.accessibility.is_some());
        if !has_adapter {
            return;
        }
        let scale_factor_changed = self
            .window_contexts
            .get(&id)
            .and_then(|host| host.accessibility.as_ref())
            .is_some_and(|accessibility| accessibility.scale_factor_changed(scale_factor));
        let projector_generation = self
            .window_contexts
            .get(&id)
            .and_then(|host| host.accessibility.as_ref())
            .and_then(HostedAccessibility::retained_generation);
        let Some(pending) = self.accessibility_pending_mut(id) else {
            return;
        };
        let (pending, resnapshot) = pending.take_for_publication();
        let program = self.program.take_accessibility_update(id);
        let world_generation = accessibility_world_generation(&mut self.program, id);
        let Some(update) = next_accessibility_update(
            pending,
            program,
            scale_factor_changed || resnapshot,
            projector_generation,
            world_generation,
            || accessibility_snapshot(&mut self.program, id),
        ) else {
            return;
        };
        if let Some(accessibility) = self
            .window_contexts
            .get_mut(&id)
            .and_then(|host| host.accessibility.as_mut())
        {
            accessibility.synchronize(update, scale_factor);
        }
    }
    pub(super) fn accessibility_pending_mut(
        &mut self,
        id: WindowId,
    ) -> Option<&mut PendingAccessibility> {
        self.window_contexts
            .get_mut(&id)
            .map(|host| &mut host.accessibility_pending)
    }
    #[cfg(not(target_os = "android"))]
    pub(super) fn accessibility_mut(&mut self, id: WindowId) -> Option<&mut HostedAccessibility> {
        self.window_contexts
            .get_mut(&id)
            .and_then(|host| host.accessibility.as_mut())
    }
}

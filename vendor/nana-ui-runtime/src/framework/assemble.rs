use std::any::TypeId;
use std::collections::HashSet;

use crate::{ComponentView, DocumentId, Entity, MutationQueue, StableNodeId, View};

use super::{AppContext, FrameworkError};

#[derive(Clone, Copy)]
pub(crate) struct AssembledChild {
    pub id: StableNodeId,
    pub type_id: TypeId,
}

/// One entry of a parent's key table, taken out to be put back
/// ([`AppContext::assembled_key`], [`AppContext::rekey_assembled`]).
#[derive(Clone)]
pub(crate) struct AssembledKey {
    pub(super) key: String,
    pub(super) child: AssembledChild,
}

/// Identity-stable child builder for one retained parent.
pub struct AssemblyScope<'a> {
    context: &'a mut AppContext,
    parent: StableNodeId,
    document: DocumentId,
    seen: Vec<String>,
}

impl AppContext {
    /// Publish the complete order of existing children without destroying omitted
    /// subtrees. Omitted children are parked and may be inserted again later.
    ///
    /// Unlike [`Self::mount`], this does not own keyed component construction or
    /// replace component values. Legal reparenting and parked parents are allowed.
    /// Invalid nodes, duplicate children, cycles and cross-document moves fail
    /// before any tree or component lifecycle change. An unchanged order is a no-op.
    pub fn reconcile_children(
        &mut self,
        parent: StableNodeId,
        ordered: &[StableNodeId],
    ) -> Result<bool, FrameworkError> {
        let current = self
            .world
            .node(parent)
            .ok_or(FrameworkError::MissingView(parent))?
            .children;
        if current.as_slice() == ordered {
            return Ok(false);
        }
        let mut seen = crate::NodeSet::with_capacity_and_hasher(ordered.len(), Default::default());
        for &child in ordered {
            if !self.world.contains(child) {
                return Err(FrameworkError::MissingView(child));
            }
            if child == parent || !seen.insert(child) {
                return Err(FrameworkError::InvalidComponentHierarchy { parent, child });
            }
        }
        let mut mutations = MutationQueue::new();
        if !reconcile_child_order(parent, ordered, &self.world, &mut mutations) {
            return Ok(false);
        }
        // commit_mutations validates the entire queue before changing either
        // the world or retained component lifecycles.
        self.commit_mutations(mutations)?;
        Ok(true)
    }

    /// Last `mount` key for `parent`; omitted keys disappear after remount.
    pub fn assembled_child(&self, parent: StableNodeId, key: &str) -> Option<StableNodeId> {
        self.assembled.get(&parent)?.get(key).map(|child| child.id)
    }

    /// Reconcile keyed children of `parent` without rebuilding the tree.
    pub fn mount<P: View>(
        &mut self,
        parent: Entity<P>,
        build: impl FnOnce(&mut AssemblyScope<'_>) -> Result<(), FrameworkError>,
    ) -> Result<(), FrameworkError> {
        self.read(parent, |_| ())?;
        let document = self
            .world
            .node(parent.id)
            .ok_or(FrameworkError::MissingView(parent.id))?
            .document;
        let mut scope = AssemblyScope {
            context: self,
            parent: parent.id,
            document,
            seen: Vec::new(),
        };
        build(&mut scope)?;
        scope.finish()
    }
}

impl AssemblyScope<'_> {
    pub fn child<C: ComponentView>(
        &mut self,
        key: impl Into<String>,
        component: C,
    ) -> Result<Entity<C>, FrameworkError> {
        self.upsert(key.into(), component)
    }

    pub fn with_child<C: ComponentView>(
        &mut self,
        key: impl Into<String>,
        component: C,
        children: impl FnOnce(&mut AssemblyScope<'_>) -> Result<(), FrameworkError>,
    ) -> Result<Entity<C>, FrameworkError> {
        let entity = self.upsert(key.into(), component)?;
        let document = self.document;
        let mut nested = AssemblyScope {
            context: self.context,
            parent: entity.id,
            document,
            seen: Vec::new(),
        };
        children(&mut nested)?;
        nested.finish()?;
        Ok(entity)
    }

    fn upsert<C: ComponentView>(
        &mut self,
        key: String,
        component: C,
    ) -> Result<Entity<C>, FrameworkError> {
        if !super::valid_assembly_key(&key) {
            return Err(FrameworkError::InvalidInput);
        }
        if self.seen.iter().any(|seen| seen == &key) {
            return Err(FrameworkError::DuplicateAssemblyKey {
                parent: self.parent,
                key,
            });
        }
        self.seen.push(key.clone());
        let type_id = TypeId::of::<C>();
        if let Some(existing) = self
            .context
            .assembled
            .get(&self.parent)
            .and_then(|slots| slots.get(&key))
            .copied()
        {
            if existing.type_id == type_id {
                let entity = Entity::from_stable_id(existing.id);
                self.context
                    .update_component(entity, |view: &mut C, _| view.reconcile(component))?;
                return Ok(entity);
            }
            self.context.despawn_node(existing.id)?;
        }
        let entity = self
            .context
            .create_detached_component(self.document, component)?;
        self.context.attach_child(self.parent, entity.id)?;
        let mut slots = self
            .context
            .assembled
            .get(&self.parent)
            .cloned()
            .unwrap_or_default();
        slots.insert(
            key,
            AssembledChild {
                id: entity.id,
                type_id,
            },
        );
        self.context.store_assembled(self.parent, slots);
        Ok(entity)
    }

    fn finish(self) -> Result<(), FrameworkError> {
        let unused: Vec<_> = self
            .context
            .assembled
            .get(&self.parent)
            .into_iter()
            .flatten()
            .filter(|(key, _)| !self.seen.iter().any(|seen| seen == *key))
            .map(|(_, child)| child.id)
            .collect();
        for id in unused {
            self.context.despawn_node(id)?;
        }
        if let Some(mut slots) = self.context.assembled.get(&self.parent).cloned() {
            slots.retain(|key, _| self.seen.iter().any(|seen| seen == key));
            self.context.store_assembled(self.parent, slots);
        }
        // A child placed under another parent keeps its identity here but is
        // not pulled back.
        let desired: Vec<_> = self
            .seen
            .iter()
            .filter_map(|key| {
                self.context
                    .assembled
                    .get(&self.parent)?
                    .get(key)
                    .map(|child| child.id)
            })
            .filter(|&id| !self.context.is_placed_elsewhere(id))
            .collect();
        let current = self
            .context
            .world
            .node(self.parent)
            .map(|node| node.children.clone())
            .unwrap_or_default();
        let assembled: HashSet<_> = desired.iter().copied().collect();
        let mut ordered: Vec<_> = current
            .iter()
            .copied()
            .filter(|id| !assembled.contains(id))
            .collect();
        ordered.extend(desired);
        if current.as_slice() == ordered.as_slice() {
            return Ok(());
        }
        let mut mutations = MutationQueue::new();
        for child in ordered {
            mutations.insert(self.parent, child, None);
        }
        self.context.commit_mutations(mutations)?;
        Ok(())
    }
}

/// Shared projection path for framework components already building one batch.
/// Input validity is checked by that batch's normal Runtime transaction.
///
/// Moves as few children as the new order allows: the longest run of kept
/// children already in increasing order stays put, and every other child is
/// inserted before its successor. Reversing costs n moves, inserting or
/// removing one child costs one.
/// Insert the children of `ordered` that are not already in order in
/// `current` (the longest increasing run stays put), each before its
/// successor. Children of `current` missing from `ordered` are left alone.
pub(crate) fn move_into_order(
    parent: StableNodeId,
    current: &[StableNodeId],
    ordered: &[StableNodeId],
    mutations: &mut MutationQueue,
) {
    // A common head and tail stay put: one row inserted or removed anywhere
    // leaves only the rows between to compare.
    let head = current
        .iter()
        .zip(ordered)
        .take_while(|(a, b)| a == b)
        .count();
    let tail = current[head..]
        .iter()
        .rev()
        .zip(ordered[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (current, ordered_middle) = (
        &current[head..current.len() - tail],
        &ordered[head..ordered.len() - tail],
    );
    let mut next = ordered.get(ordered.len() - tail).copied();
    let position: crate::NodeMap<usize> = current
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect();
    let positions: Vec<Option<usize>> = ordered_middle
        .iter()
        .map(|id| position.get(id).copied())
        .collect();
    let stays = longest_increasing(&positions);
    // Walking backwards, each moved child goes before the one that follows
    // it, which is already where it belongs.
    for (index, &child) in ordered_middle.iter().enumerate().rev() {
        if !stays[index] {
            mutations.insert(parent, child, next);
        }
        next = Some(child);
    }
}

pub(crate) fn reconcile_child_order(
    parent: StableNodeId,
    ordered: &[StableNodeId],
    world: &crate::UiWorld,
    mutations: &mut MutationQueue,
) -> bool {
    let current = world
        .node(parent)
        .map(|node| node.children)
        .unwrap_or_default();
    if current.as_slice() == ordered {
        return false;
    }
    // Extract retained descendants before parking their former ancestors. A
    // live-to-live reparent must never retire focus, IME or pointer ownership.
    move_into_order(parent, &current, ordered, mutations);
    let keep = ordered.iter().copied().collect::<HashSet<_>>();
    for child in &current {
        if !keep.contains(child) {
            mutations.park_subtree(*child);
        }
    }
    true
}

/// Marks one longest strictly increasing subsequence of the known positions
/// (patience sorting, O(n log n)).
fn longest_increasing(positions: &[Option<usize>]) -> Vec<bool> {
    // tails[k]: index in `positions` ending the best subsequence of length k+1.
    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; positions.len()];
    for (index, position) in positions.iter().enumerate() {
        let Some(position) = *position else {
            continue;
        };
        let length = tails.partition_point(|&tail| {
            positions[tail].expect("tails hold known positions") < position
        });
        if length > 0 {
            previous[index] = Some(tails[length - 1]);
        }
        if length == tails.len() {
            tails.push(index);
        } else {
            tails[length] = index;
        }
    }
    let mut stays = vec![false; positions.len()];
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        stays[index] = true;
        cursor = previous[index];
    }
    stays
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, DocumentId, Stack};

    fn children(context: &AppContext, parent: StableNodeId) -> Vec<StableNodeId> {
        context.world().node(parent).unwrap().children.to_vec()
    }

    /// Reorder `parent` to `ordered`, returning how many inserts it took.
    fn reorder(context: &mut AppContext, parent: StableNodeId, ordered: &[StableNodeId]) -> usize {
        let mut mutations = MutationQueue::new();
        reconcile_child_order(parent, ordered, context.world(), &mut mutations);
        let moves = mutations
            .as_slice()
            .iter()
            .filter(|mutation| matches!(mutation, crate::UiMutation::Insert { .. }))
            .count();
        context.commit_mutations(mutations).unwrap();
        assert_eq!(children(context, parent), ordered);
        moves
    }

    #[test]
    fn a_common_head_and_tail_stay_put() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let parent = context
            .create_component(document, Stack::column(0.0))
            .unwrap()
            .stable_id();
        let mut all = Vec::new();
        for _ in 0..8 {
            let child = context
                .create_component(document, Stack::column(0.0))
                .unwrap()
                .stable_id();
            all.push(child);
        }
        let mut mutations = MutationQueue::new();
        for child in &all {
            mutations.insert(parent, *child, None);
        }
        context.commit_mutations(mutations).unwrap();
        // Swap the middle two: head and tail of three each stay.
        let mut swapped = all.clone();
        swapped.swap(3, 4);
        assert_eq!(reorder(&mut context, parent, &swapped), 1);
        // A new first child moves nothing else.
        let fresh = context
            .create_component(document, Stack::column(0.0))
            .unwrap()
            .stable_id();
        let mut with_fresh = vec![fresh];
        with_fresh.extend(&swapped);
        assert_eq!(reorder(&mut context, parent, &with_fresh), 1);
    }

    #[test]
    fn reordering_moves_only_children_outside_the_longest_kept_run() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let parent = context
            .create_component(document, Stack::column(0.0))
            .unwrap()
            .stable_id();
        let other = context
            .create_component(document, Stack::column(0.0))
            .unwrap()
            .stable_id();
        let mut all = Vec::new();
        for _ in 0..40 {
            let child = context
                .create_component(document, Stack::column(0.0))
                .unwrap()
                .stable_id();
            context
                .append_child(
                    crate::Entity::<Stack>::from_stable_id(parent),
                    crate::Entity::<Stack>::from_stable_id(child),
                )
                .unwrap();
            all.push(child);
        }
        let outsider = context
            .create_component(document, Stack::column(0.0))
            .unwrap()
            .stable_id();
        context
            .append_child(
                crate::Entity::<Stack>::from_stable_id(other),
                crate::Entity::<Stack>::from_stable_id(outsider),
            )
            .unwrap();

        let mut order = all.clone();
        // Insert at the front (taken from another parent): one move.
        order.insert(0, outsider);
        assert_eq!(reorder(&mut context, parent, &order), 1);
        // Remove from the middle: no move, one park.
        order.remove(20);
        assert_eq!(reorder(&mut context, parent, &order), 0);
        // Move one child from the end to the front: one move.
        let last = order.pop().unwrap();
        order.insert(0, last);
        assert_eq!(reorder(&mut context, parent, &order), 1);
        // Swap two: two moves at most.
        order.swap(3, 30);
        assert!(reorder(&mut context, parent, &order) <= 2);
        // Reverse: everything but one child moves.
        order.reverse();
        assert_eq!(reorder(&mut context, parent, &order), order.len() - 1);
        // Pseudo-random shuffles land exactly, with moves bounded by n - LIS.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..20 {
            for index in (1..order.len()).rev() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                order.swap(index, (seed % (index as u64 + 1)) as usize);
            }
            let before = children(&context, parent);
            let positions: Vec<Option<usize>> = order
                .iter()
                .map(|id| before.iter().position(|b| b == id))
                .collect();
            let kept = longest_increasing(&positions)
                .iter()
                .filter(|s| **s)
                .count();
            assert_eq!(reorder(&mut context, parent, &order), order.len() - kept);
        }
    }

    #[test]
    fn the_longest_increasing_run_skips_unknown_positions() {
        let positions = [Some(3), None, Some(0), Some(1), Some(4), Some(2), None];
        let marks = longest_increasing(&positions);
        let run: Vec<usize> = positions
            .iter()
            .zip(&marks)
            .filter(|(_, stays)| **stays)
            .map(|(position, _)| position.expect("marked known"))
            .collect();
        assert_eq!(run.len(), 3);
        assert!(run.windows(2).all(|pair| pair[0] < pair[1]));
    }
}

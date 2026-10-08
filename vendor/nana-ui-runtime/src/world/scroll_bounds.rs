//! Layout-only content extents. Scroll offsets and paint effects do not change
//! the scrollable layout range. Child maxima support shrinking as well as growth.
use super::*;

/// The union of descendant layout boxes, in the scroll container's own
/// coordinate space. Empty is inverted (`left > right`), so it merges as the
/// identity and reads as "nothing overflows" on every edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Extent {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Extent {
    pub(crate) const EMPTY: Self = Self {
        left: f32::INFINITY,
        top: f32::INFINITY,
        right: f32::NEG_INFINITY,
        bottom: f32::NEG_INFINITY,
    };
    fn of(layout: crate::LayoutBox) -> Self {
        Self {
            left: layout.x,
            top: layout.y,
            right: layout.x + layout.width,
            bottom: layout.y + layout.height,
        }
    }
    fn merge(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

#[derive(Default)]
struct MaxTree {
    values: Vec<Extent>,
    leaf: usize,
}

impl MaxTree {
    /// Rebuild over `values` in place, reusing the buffer when the leaf
    /// count is unchanged: O(n), where n single-slot updates are O(n log n).
    fn refill(&mut self, values: impl ExactSizeIterator<Item = Extent>) -> usize {
        let len = values.len();
        if len == 0 {
            *self = Self::default();
            return 0;
        }
        let leaf = len.next_power_of_two();
        if self.leaf != leaf {
            *self = Self {
                values: vec![Extent::EMPTY; leaf * 2],
                leaf,
            };
        }
        let tree = &mut self.values;
        for (index, value) in values.enumerate() {
            tree[leaf + index] = value;
        }
        tree[leaf + len..].fill(Extent::EMPTY);
        for index in (1..leaf).rev() {
            tree[index] = tree[index * 2].merge(tree[index * 2 + 1]);
        }
        tree.len()
    }
    fn total(&self) -> Extent {
        self.values.get(1).copied().unwrap_or(Extent::EMPTY)
    }
    fn set(&mut self, index: usize, value: Extent) -> usize {
        let mut at = self.leaf + index;
        self.values[at] = value;
        let mut work = 1;
        while at > 1 {
            at /= 2;
            self.values[at] = self.values[at * 2].merge(self.values[at * 2 + 1]);
            work += 1;
        }
        work
    }
}

/// The children of an entry whose extent moved. A relayout usually moves
/// one child per parent (a row's label), so one is kept inline and a list is
/// only allocated past it.
#[derive(Default)]
enum Changed {
    #[default]
    None,
    One(StableNodeId),
    Many(Vec<StableNodeId>),
}

impl Changed {
    fn push(&mut self, id: StableNodeId) {
        *self = match std::mem::take(self) {
            Self::None => Self::One(id),
            Self::One(first) => Self::Many(vec![first, id]),
            Self::Many(mut ids) => {
                ids.push(id);
                Self::Many(ids)
            }
        };
    }
    fn as_slice(&self) -> &[StableNodeId] {
        match self {
            Self::None => &[],
            Self::One(id) => std::slice::from_ref(id),
            Self::Many(ids) => ids,
        }
    }
    fn remove(&mut self, id: StableNodeId) {
        match self {
            Self::One(only) if *only == id => *self = Self::None,
            Self::Many(ids) => ids.retain(|child| *child != id),
            _ => {}
        }
    }
}

struct Entry {
    children: Arc<Vec<StableNodeId>>,
    slots: NodeMap<usize>,
    maxima: MaxTree,
    extent: Extent,
    /// This node's own box, or something under it, moved since `extent`.
    dirty: bool,
    /// Children whose extent moved, each listed once: a child is pushed
    /// only as it turns dirty, and a dirty child's parent is dirty too.
    changed: Changed,
    /// The children list changed: re-read all of them.
    rebuild: bool,
}

impl Entry {
    fn fresh(
        children: Arc<Vec<StableNodeId>>,
        bounds: Vec<Extent>,
        own: impl FnOnce(Extent) -> Extent,
    ) -> Self {
        let mut maxima = MaxTree::default();
        maxima.refill(bounds.into_iter());
        Self {
            slots: children
                .iter()
                .copied()
                .enumerate()
                .map(|(index, id)| (id, index))
                .collect(),
            extent: own(maxima.total()),
            maxima,
            children,
            dirty: false,
            changed: Changed::default(),
            rebuild: false,
        }
    }
}

#[derive(Default)]
pub(super) struct ContentBoundsIndex {
    // Keyed by node id through `IdHasher`. Dirtiness lives on the entries, so
    // marking a written box is one lookup of it and one of its parent.
    entries: NodeMap<Entry>,
    /// Boxes written by the commit being applied, marked dirty in one batch
    /// when it ends (or before any query reads the index).
    deferred: Vec<StableNodeId>,
    #[cfg(any(test, feature = "benchmark"))]
    refreshed: usize,
    #[cfg(any(test, feature = "benchmark"))]
    range_updates: usize,
}

impl ContentBoundsIndex {
    /// A child moved in or out of `new_parent` / its current parent. This
    /// reads the parent before the move, so it is applied immediately.
    pub fn topology(
        &mut self,
        world: &UiWorld,
        child: StableNodeId,
        new_parent: Option<StableNodeId>,
    ) {
        if self.entries.is_empty() {
            return;
        }
        self.invalidate(world, child);
        for parent in world.parent_id(child).into_iter().chain(new_parent) {
            if !self.entries.contains_key(&parent) {
                continue;
            }
            self.invalidate(world, parent);
            let entry = self.entries.get_mut(&parent).expect("checked above");
            entry.rebuild = true;
            // A topology rebuild reads all current children. Keeping old dirty
            // edges here could retain IDs moved away and deleted before query.
            entry.changed = Changed::None;
        }
    }

    /// Mark `id` and its indexed ancestors dirty now.
    pub fn invalidate(&mut self, world: &UiWorld, id: StableNodeId) {
        let mut cursor = Some(id);
        while let Some(id) = cursor {
            // A missing entry is built from current authority on first query.
            // Insert already invalidates the nearest indexed parent topology.
            let Some(entry) = self.entries.get_mut(&id) else {
                break;
            };
            if std::mem::replace(&mut entry.dirty, true) {
                break;
            }
            cursor = world.parent_id(id);
            if let Some(parent) = cursor.and_then(|parent| self.entries.get_mut(&parent)) {
                parent.changed.push(id);
            }
        }
    }

    /// `id`'s box was written: mark it with the rest of the commit's writes.
    pub fn defer(&mut self, id: StableNodeId) {
        if !self.entries.is_empty() {
            self.deferred.push(id);
        }
    }

    /// Mark every deferred write dirty. The walk above a node stops at the
    /// first ancestor already dirty, so siblings share it after the first.
    pub fn flush(&mut self, world: &UiWorld) {
        let mut deferred = std::mem::take(&mut self.deferred);
        for id in deferred.drain(..) {
            self.invalidate(world, id);
        }
        // Keep the allocation for the next commit.
        self.deferred = deferred;
    }

    pub fn remove(&mut self, id: StableNodeId, parent: Option<StableNodeId>) {
        self.entries.remove(&id);
        if let Some(parent) = parent.and_then(|parent| self.entries.get_mut(&parent)) {
            parent.changed.remove(id);
        }
        if self.entries.is_empty() {
            self.deferred.clear();
        }
    }

    /// Whether `id`'s extent may have moved since it was last read: dirty,
    /// or never indexed. Call after [`Self::flush`].
    pub fn stale(&self, id: StableNodeId) -> bool {
        self.entries.get(&id).is_none_or(|entry| entry.dirty)
    }

    pub fn content(&mut self, world: &UiWorld, id: StableNodeId) -> Extent {
        self.flush(world);
        self.refresh(world, id);
        self.entries
            .get(&id)
            .map_or(Extent::EMPTY, |entry| entry.maxima.total())
    }

    /// No pending work anywhere in the index.
    #[cfg(test)]
    fn settled(&self) -> bool {
        self.deferred.is_empty()
            && self
                .entries
                .values()
                .all(|entry| !entry.dirty && !entry.rebuild && entry.changed.as_slice().is_empty())
    }

    fn refresh(&mut self, world: &UiWorld, id: StableNodeId) -> Extent {
        let (changed, rebuild) = match self.entries.get_mut(&id) {
            Some(entry) if !entry.dirty => return entry.extent,
            Some(entry) => {
                entry.dirty = false;
                (
                    std::mem::take(&mut entry.changed),
                    std::mem::take(&mut entry.rebuild),
                )
            }
            None => (Changed::None, false),
        };
        let Some(record) = world.nodes.get(id) else {
            return Extent::EMPTY;
        };
        #[cfg(any(test, feature = "benchmark"))]
        {
            self.refreshed += 1;
        }
        let omitted = record.style.layout.omits_box();
        let layout = record.layout;
        let own = |maxima: Extent| {
            if omitted {
                Extent::EMPTY
            } else {
                Extent::of(layout).merge(maxima)
            }
        };
        let incremental = match (rebuild, self.entries.get_mut(&id)) {
            (false, Some(entry)) if Arc::ptr_eq(&entry.children, &record.hierarchy.children) => {
                if changed.as_slice().is_empty() {
                    // Only this node's own box moved (every leaf a relayout
                    // writes).
                    entry.extent = own(entry.maxima.total());
                    return entry.extent;
                }
                true
            }
            _ => false,
        };
        if !incremental {
            // New, or its children list changed: build it from every child.
            let children = Arc::clone(&record.hierarchy.children);
            let bounds = children
                .iter()
                .map(|child| self.refresh(world, *child))
                .collect::<Vec<_>>();
            let entry = Entry::fresh(children, bounds, own);
            let extent = entry.extent;
            self.entries.insert(id, entry);
            return extent;
        }
        // The entry stays where it is: children are refreshed first, then
        // their extents land in the tree in place — a relayout refreshes every
        // row of a list, so taking each entry out and back adds up.
        let children = &record.hierarchy.children;
        let changed = changed.as_slice();
        let work = if changed.len() > 1 && changed.len().saturating_mul(4) >= children.len() {
            // A relayout moved a large share of the children (a list shifted
            // below an edited row): refill the tree once, O(n), instead of
            // O(log n) per moved child. The tree is lent out while the
            // children refresh straight into it.
            let entry = self.entries.get_mut(&id).expect("incremental entry");
            let mut maxima = std::mem::take(&mut entry.maxima);
            let work = maxima.refill(children.iter().map(|child| self.refresh(world, *child)));
            self.entries.get_mut(&id).expect("incremental entry").maxima = maxima;
            work
        } else {
            let mut work = 0;
            for child in changed {
                let extent = self.refresh(world, *child);
                let entry = self.entries.get_mut(&id).expect("incremental entry");
                if let Some(&slot) = entry.slots.get(child) {
                    work += entry.maxima.set(slot, extent);
                }
            }
            work
        };
        let entry = self.entries.get_mut(&id).expect("incremental entry");
        entry.extent = own(entry.maxima.total());
        let extent = entry.extent;
        #[cfg(any(test, feature = "benchmark"))]
        {
            self.range_updates += work;
        }
        #[cfg(not(any(test, feature = "benchmark")))]
        let _ = work;
        extent
    }
}

impl UiWorld {
    /// Diagnostic access to the canonical content index, with per-query work.
    #[cfg(feature = "benchmark")]
    pub fn benchmark_scroll_content_extent(
        &self,
        id: StableNodeId,
    ) -> ((f32, f32), (usize, usize)) {
        let mut index = self.scroll_content_bounds.borrow_mut();
        index.refreshed = 0;
        index.range_updates = 0;
        let extent = index.content(self, id);
        (
            (extent.right, extent.bottom),
            (index.refreshed, index.range_updates),
        )
    }

    /// The union of `id`'s descendant boxes. Scroll offsets do not move it.
    pub(crate) fn scroll_content_extent(&self, id: StableNodeId) -> Extent {
        self.scroll_content_bounds.borrow_mut().content(self, id)
    }

    pub(super) fn invalidate_scroll_content(&self, id: StableNodeId) {
        self.scroll_content_bounds.borrow_mut().invalidate(self, id);
    }

    /// `id`'s box was written; the commit marks all of them when it ends.
    pub(super) fn defer_scroll_content(&self, id: StableNodeId) {
        self.scroll_content_bounds.borrow_mut().defer(id);
    }

    pub(super) fn flush_scroll_content(&self) {
        self.scroll_content_bounds.borrow_mut().flush(self);
    }

    pub(super) fn invalidate_scroll_topology(
        &self,
        child: StableNodeId,
        parent: Option<StableNodeId>,
    ) {
        self.scroll_content_bounds
            .borrow_mut()
            .topology(self, child, parent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MutationQueue, NodeKind, NodeStyle};

    fn id(value: u64) -> StableNodeId {
        StableNodeId::new(value).unwrap()
    }
    fn rectangle(y: f32) -> crate::LayoutBox {
        crate::LayoutBox {
            x: 0.0,
            y,
            width: 10.0,
            height: 10.0,
        }
    }
    fn fixture(count: u64) -> UiWorld {
        let document = crate::DocumentId::new(1).unwrap();
        let mut world = UiWorld::new();
        let mut mutations = MutationQueue::new();
        mutations.create(id(1), document, NodeKind::Document);
        for value in 2..count + 2 {
            mutations.create(id(value), document, NodeKind::Text);
            mutations.insert(id(1), id(value), None);
            mutations.write_layout(id(value), rectangle((value - 2) as f32 * 10.0));
        }
        world.commit(mutations).unwrap();
        world
    }

    #[test]
    fn ten_thousand_siblings_shrink_updates_one_ancestor_path() {
        let mut world = fixture(10_000);
        assert_eq!(far(&world, id(1)), (10.0, 100_000.0));
        {
            let mut index = world.scroll_content_bounds.borrow_mut();
            index.refreshed = 0;
            index.range_updates = 0;
        }
        let mut mutations = MutationQueue::new();
        mutations.write_layout(id(10_001), rectangle(0.0));
        world.commit(mutations).unwrap();
        assert_eq!(far(&world, id(1)), (10.0, 99_990.0));
        {
            let index = world.scroll_content_bounds.borrow();
            assert_eq!(index.refreshed, 2);
            assert!(index.range_updates <= 16);
        }
        let mut mutations = MutationQueue::new();
        mutations.set_scroll_offset(id(1), crate::ScrollOffset { x: 0.0, y: 5.0 });
        world.commit(mutations).unwrap();
        assert_eq!(far(&world, id(1)), (10.0, 99_990.0));
        assert_eq!(world.scroll_content_bounds.borrow().refreshed, 2);
    }

    fn brute(world: &UiWorld, root: StableNodeId) -> Extent {
        let mut extent = Extent::EMPTY;
        let mut stack = world.node(root).unwrap().children;
        while let Some(id) = stack.pop() {
            if world.node_style(id).unwrap().layout.omits_box() {
                continue;
            }
            let node = world.node(id).unwrap();
            let layout = world.layout_box(id).unwrap();
            extent = extent.merge(Extent::of(layout));
            stack.extend(node.children);
        }
        extent
    }

    fn far(world: &UiWorld, id: StableNodeId) -> (f32, f32) {
        let extent = world.scroll_content_extent(id);
        (extent.right, extent.bottom)
    }

    /// A relayout that shifts most of a list refills its tree once, and the
    /// result is the traversal's — shrinking, growing and past the leaf count.
    #[test]
    fn shifting_most_siblings_refills_the_tree_and_matches_traversal() {
        let mut world = fixture(1_000);
        assert_eq!(far(&world, id(1)), (10.0, 10_000.0));
        for (shift, count) in [(-5.0, 1_000), (7.0, 900), (-3.0, 400)] {
            {
                let mut index = world.scroll_content_bounds.borrow_mut();
                index.refreshed = 0;
                index.range_updates = 0;
            }
            let mut mutations = MutationQueue::new();
            for value in 2..count + 2 {
                mutations.write_layout(
                    id(value),
                    crate::LayoutBox {
                        x: shift,
                        ..rectangle((value - 2) as f32 * 10.0 + shift)
                    },
                );
            }
            world.commit(mutations).unwrap();
            assert_eq!(world.scroll_content_extent(id(1)), brute(&world, id(1)));
            // One refill of the 1,024-leaf tree, not 1,000 root-to-leaf paths.
            assert_eq!(world.scroll_content_bounds.borrow().range_updates, 2_048);
        }
    }

    #[test]
    fn extent_tracks_content_overflowing_left_and_up() {
        let mut world = fixture(2);
        assert_eq!(
            world.scroll_content_extent(id(1)),
            Extent {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 20.0,
            }
        );
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            id(3),
            crate::LayoutBox {
                x: -40.0,
                y: -25.0,
                width: 10.0,
                height: 10.0,
            },
        );
        world.commit(mutations).unwrap();
        assert_eq!(
            world.scroll_content_extent(id(1)),
            Extent {
                left: -40.0,
                top: -25.0,
                right: 10.0,
                bottom: 10.0,
            }
        );
        assert_eq!(world.scroll_content_extent(id(1)), brute(&world, id(1)));
    }

    #[test]
    fn cached_extents_match_traversal_after_visibility_reparent_and_deletion() {
        let mut world = fixture(4);
        let mut mutations = MutationQueue::new();
        mutations.insert(id(2), id(5), None);
        world.commit(mutations).unwrap();
        for root in [id(1), id(2), id(3)] {
            assert_eq!(world.scroll_content_extent(root), brute(&world, root));
        }
        let mut hidden = NodeStyle::default();
        Arc::make_mut(&mut hidden.layout).display = Some(nana_ui_core::DisplaySpec::None);
        let mut mutations = MutationQueue::new();
        mutations.set_style(id(2), hidden);
        mutations.write_layout(id(5), rectangle(90.0));
        world.commit(mutations).unwrap();
        assert_eq!(world.scroll_content_extent(id(1)), brute(&world, id(1)));
        let mut mutations = MutationQueue::new();
        mutations.set_style(id(2), NodeStyle::default());
        mutations.insert(id(3), id(5), None);
        mutations.write_layout(id(5), rectangle(120.0));
        world.commit(mutations).unwrap();
        for root in [id(1), id(2), id(3)] {
            assert_eq!(world.scroll_content_extent(root), brute(&world, root));
        }
        let mut mutations = MutationQueue::new();
        mutations.park_subtree(id(3));
        world.commit(mutations).unwrap();
        assert_eq!(world.scroll_content_extent(id(1)), brute(&world, id(1)));
        let mut mutations = MutationQueue::new();
        mutations.insert(id(1), id(3), Some(id(2)));
        mutations.despawn_subtree(id(5));
        world.commit(mutations).unwrap();
        assert_eq!(world.scroll_content_extent(id(1)), brute(&world, id(1)));
        assert!(
            !world
                .scroll_content_bounds
                .borrow()
                .entries
                .contains_key(&id(5))
        );
        let mut mutations = MutationQueue::new();
        mutations.despawn_subtree(id(1));
        world.commit(mutations).unwrap();
        let index = world.scroll_content_bounds.borrow();
        assert!(index.entries.is_empty());
        assert!(index.settled());
    }

    #[test]
    fn scrolling_churn_releases_entries_and_does_not_refresh_another_document() {
        let mut world = fixture(0);
        let mut mutations = MutationQueue::new();
        mutations.create(
            id(1000),
            crate::DocumentId::new(2).unwrap(),
            NodeKind::Document,
        );
        world.commit(mutations).unwrap();
        world.scroll_content_extent(id(1));
        world.scroll_content_extent(id(1000));
        for value in 2..258 {
            let mut mutations = MutationQueue::new();
            mutations.create(
                id(value),
                crate::DocumentId::new(1).unwrap(),
                NodeKind::Text,
            );
            mutations.insert(id(1), id(value), None);
            mutations.write_layout(id(value), rectangle(value as f32));
            world.commit(mutations).unwrap();
            assert_eq!(far(&world, id(1)), (10.0, value as f32 + 10.0));
            let before = world.scroll_content_bounds.borrow().refreshed;
            assert_eq!(
                far(&world, id(1000)),
                (f32::NEG_INFINITY, f32::NEG_INFINITY)
            );
            assert_eq!(world.scroll_content_bounds.borrow().refreshed, before);
            let mut mutations = MutationQueue::new();
            mutations.despawn_subtree(id(value));
            world.commit(mutations).unwrap();
            assert_eq!(far(&world, id(1)), (f32::NEG_INFINITY, f32::NEG_INFINITY));
            let index = world.scroll_content_bounds.borrow();
            assert_eq!(index.entries.len(), 2);
            assert!(index.settled());
        }
    }

    #[test]
    fn reparent_then_delete_does_not_leave_dirty_ids_in_an_unqueried_old_parent() {
        let mut world = fixture(0);
        let mut mutations = MutationQueue::new();
        mutations.create(
            id(999),
            crate::DocumentId::new(1).unwrap(),
            NodeKind::Document,
        );
        world.commit(mutations).unwrap();
        world.scroll_content_extent(id(1));
        world.scroll_content_extent(id(999));
        for value in 2..258 {
            let mut mutations = MutationQueue::new();
            mutations.create(
                id(value),
                crate::DocumentId::new(1).unwrap(),
                NodeKind::Text,
            );
            mutations.insert(id(1), id(value), None);
            mutations.insert(id(999), id(value), None);
            mutations.write_layout(id(value), rectangle(50.0));
            world.commit(mutations).unwrap();
            assert_eq!(far(&world, id(999)), (10.0, 60.0));
            let mut mutations = MutationQueue::new();
            mutations.despawn_subtree(id(value));
            world.commit(mutations).unwrap();
            world.scroll_content_extent(id(999));
            let index = world.scroll_content_bounds.borrow();
            assert_eq!(index.entries.len(), 2);
            assert!(
                index
                    .entries
                    .values()
                    .flat_map(|entry| entry.changed.as_slice())
                    .all(|id| world.contains(*id))
            );
        }
        assert_eq!(world.scroll_content_extent(id(1)), brute(&world, id(1)));
        let index = world.scroll_content_bounds.borrow();
        assert!(index.settled());
    }

    #[test]
    fn unqueried_document_does_not_allocate_content_index_work() {
        let mut world = fixture(4);
        world.scroll_content_extent(id(1));
        let before = world.scroll_content_bounds.borrow().refreshed;
        let document = crate::DocumentId::new(2).unwrap();
        let mut mutations = MutationQueue::new();
        mutations.create(id(10), document, NodeKind::Document);
        for value in 11..1011 {
            mutations.create(id(value), document, NodeKind::Text);
            mutations.insert(id(10), id(value), None);
            mutations.write_layout(id(value), rectangle(value as f32));
        }
        world.commit(mutations).unwrap();
        {
            let index = world.scroll_content_bounds.borrow();
            assert_eq!(index.entries.len(), 5);
            assert!(index.settled());
        }
        world.scroll_content_extent(id(1));
        assert_eq!(world.scroll_content_bounds.borrow().refreshed, before);
        assert_eq!(world.scroll_content_extent(id(10)), brute(&world, id(10)));
    }

    #[test]
    fn scalar_geometry_validation_does_not_materialize_a_wide_hierarchy() {
        let mut world = fixture(10_000);
        let mut mutations = MutationQueue::new();
        mutations.write_layout(id(1), rectangle(0.0));
        mutations.set_scroll_offset(id(1), crate::ScrollOffset { x: 0.0, y: 4.0 });
        mutations.set_scroll_metrics(
            id(1),
            Some(crate::ScrollMetrics {
                viewport_width: 10.0,
                viewport_height: 10.0,
                content_width: 10.0,
                content_height: 100_000.0,
                origin_x: 0.0,
                origin_y: 0.0,
            }),
        );
        let mut plan = super::super::mutation::ValidationPlan::new(&world);
        plan.validate(mutations.as_slice()).unwrap();
        assert!(plan.nodes.is_empty());
        drop(plan);
        world.commit(mutations).unwrap();
        assert_eq!(world.scroll_offset(id(1)).unwrap().y, 4.0);
        let before = world.generation();
        let mut mutations = MutationQueue::new();
        mutations.despawn_subtree(id(2));
        mutations.set_scroll_offset(id(2), crate::ScrollOffset { x: 0.0, y: 3.0 });
        assert_eq!(
            world.commit(mutations),
            Err(UiWorldError::MissingNode(id(2)))
        );
        assert!(world.contains(id(2)));
        assert_eq!(world.generation(), before);
        let mut mutations = MutationQueue::new();
        mutations.create(
            id(20_000),
            crate::DocumentId::new(1).unwrap(),
            NodeKind::Text,
        );
        mutations.set_scroll_offset(id(20_000), crate::ScrollOffset { x: 0.0, y: 5.0 });
        world.commit(mutations).unwrap();
        assert_eq!(world.scroll_offset(id(20_000)).unwrap().y, 5.0);
    }
}

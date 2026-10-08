//! Sparse virtual placement, with the same keyed materializer and Runtime authority.

use super::*;
use crate::Stack;
use nana_ui_core::{PositionSpec, VirtualViewport};

#[cfg(test)]
mod tests;

impl AppContext {
    /// Materialize a positioned content list inside a ScrollView. Each mounted
    /// component has one framework-owned placement container at its logical Y.
    /// The List's height is the full data extent; its width remains caller-owned.
    /// Item content, including editable state and children, is never reprojected
    /// just because its index changes. Do not mix this with the legacy unplaced
    /// materializer on the same `items` instance.
    ///
    /// Focus and a previously observed IME owner are retained automatically.
    /// `retained_keys` additionally protects application editing sessions. The
    /// inverse must resolve current data indices; missing or mismatched keys are
    /// released. Selection reads only mounted items, active ancestors and requested
    /// keys; removal uses the framework's existing event-handler cleanup.
    #[allow(clippy::too_many_arguments)]
    pub fn materialize_virtual_list_retained_in<K, C>(
        &mut self,
        list: Entity<List>,
        items: &mut VirtualListItems<K, C>,
        layout: &VirtualListLayout,
        viewport: VirtualViewport,
        retained_keys: &[K],
        key_at: impl FnMut(usize) -> K,
        index_of_key: impl FnMut(&K) -> Option<usize>,
        build: impl FnMut(usize, &K) -> C,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        self.materialize_virtual_list_retained_with(
            list,
            items,
            layout,
            viewport,
            retained_keys,
            key_at,
            index_of_key,
            build,
            |_, _, _, _| Ok(()),
        )
    }

    /// [`Self::materialize_virtual_list_retained_in`] with a mount hook.
    ///
    /// `on_mount` runs once for each row this pass newly created, after the
    /// commit that published it, so it can bind handlers with
    /// [`Self::on`] / [`Self::observe`]. Rows that scrolled back into an
    /// already-mounted window are not reported again, and a row released by
    /// scrolling away loses its handlers with the node.
    #[allow(clippy::too_many_arguments)]
    pub fn materialize_virtual_list_retained_with<K, C>(
        &mut self,
        list: Entity<List>,
        items: &mut VirtualListItems<K, C>,
        layout: &VirtualListLayout,
        viewport: VirtualViewport,
        retained_keys: &[K],
        mut key_at: impl FnMut(usize) -> K,
        mut index_of_key: impl FnMut(&K) -> Option<usize>,
        mut build: impl FnMut(usize, &K) -> C,
        mut on_mount: impl FnMut(&mut Self, Entity<C>, usize, &K) -> Result<(), FrameworkError>,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        let mut content = self.read(list, Clone::clone)?;
        let root = self
            .world
            .node(list.id)
            .ok_or(FrameworkError::MissingView(list.id))?;
        let mounted = items.materializer.mounted();
        let owned = items
            .containers
            .iter()
            .map(|(key, entity)| (entity.id, key))
            .collect::<HashMap<_, _>>();
        if mounted.len() != items.entities.len()
            || mounted.len() != items.containers.len()
            || owned.len() != mounted.len()
            || root.children.len() != owned.len()
            || root.children.iter().any(|id| !owned.contains_key(id))
        {
            return Err(FrameworkError::InvalidVirtualization);
        }
        for key in mounted {
            let container = items
                .containers
                .get(key)
                .ok_or(FrameworkError::InvalidVirtualization)?;
            let entity = items
                .entities
                .get(key)
                .ok_or(FrameworkError::InvalidVirtualization)?;
            if self
                .world
                .node(container.id)
                .is_none_or(|node| node.parent != Some(list.id) || node.children != [entity.id])
                || self
                    .world
                    .node(entity.id)
                    .is_none_or(|node| node.parent != Some(container.id))
                || self.read(*container, |_| ()).is_err()
                || self.read(*entity, |_| ()).is_err()
            {
                return Err(FrameworkError::InvalidVirtualization);
            }
        }

        let focused = self.world.focused(root.document);
        let ime_owner = focused
            .filter(|id| self.world.ime(*id).is_some())
            .or_else(|| items.ime_owner.filter(|id| self.world.ime(*id).is_some()));
        let activity = self.activity_items(list.id, owned.keys().copied(), focused, ime_owner);
        let mut active = retained_keys.to_vec();
        for target in [focused, ime_owner].into_iter().flatten() {
            let mut current = Some(target);
            while let Some(id) = current {
                if id == list.id {
                    break;
                }
                if let Some(key) = owned.get(&id) {
                    active.push((*key).clone());
                    break;
                }
                current = self.world.parent_id(id);
            }
        }
        let retained = active
            .iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .filter_map(|key| {
                let index = index_of_key(key)?;
                (index < layout.len() && key_at(index) == *key).then_some(index)
            })
            .collect::<Vec<_>>();
        let plan = items
            .materializer
            .prepare_retained(layout, viewport, retained.iter().copied(), &mut key_at)
            .map_err(|_| FrameworkError::InvalidVirtualization)?;
        let indices = layout
            .retained_ranges(&plan.window, retained)
            .into_iter()
            .flatten();

        let mut mutations = MutationQueue::new();
        let mut next_entities = items.entities.clone();
        let mut next_containers = items.containers.clone();
        let mut removed = HashSet::new();
        for key in &plan.unmounts {
            let id = items.containers[key].id;
            let mut stack = vec![id];
            while let Some(id) = stack.pop() {
                let node = self
                    .world
                    .node(id)
                    .ok_or(FrameworkError::InvalidVirtualization)?;
                stack.extend(node.children);
                removed.insert(id);
            }
            mutations.despawn_subtree(id);
            next_entities.remove(key);
            next_containers.remove(key);
        }
        let mut staged_items = Vec::new();
        // Items created by this pass, reported to `on_mount` after the commit.
        let mut mounted_now: Vec<(StableNodeId, usize, K)> = Vec::new();
        let mut staged_containers = Vec::new();
        for (index, key) in indices.zip(&plan.order) {
            let top = layout.extent(0..index);
            let height = layout.extent(index..index + 1);
            // A measured row sizes to its content; its extent is read back
            // after layout instead of imposed on it.
            let container = Stack::column(0.0).style(crate::NodeStyle {
                layout: Arc::new(nana_ui_core::LayoutStyle {
                    position: PositionSpec::Absolute,
                    offset_top: Some(LengthSpec::Px(top)),
                    offset_left: Some(LengthSpec::Px(0.0)),
                    width: Some(LengthSpec::Percent(100.0)),
                    height: (!items.measured).then_some(LengthSpec::Px(height)),
                    flex_shrink: Some(0.0),
                    ..Default::default()
                }),
                ..Default::default()
            });
            if let Some(entity) = next_containers.get(key).copied() {
                if self.read(entity, |old| old != &container)? {
                    container.project(entity.id, &self.world, &mut mutations);
                    staged_containers.push((entity.id, container));
                    items.pending_measure |= items.measured;
                }
            } else {
                items.pending_measure |= items.measured;
                let component = build(index, key);
                let container_id = self.allocate_id();
                let item_id = self.allocate_id();
                mounted_now.push((item_id, index, key.clone()));
                mutations.create(container_id, root.document, container.node_kind());
                container.project(container_id, &self.world, &mut mutations);
                mutations.create(item_id, root.document, component.node_kind());
                component.project(item_id, &self.world, &mut mutations);
                mutations.insert(container_id, item_id, None);
                next_containers.insert(key.clone(), Entity::from_stable_id(container_id));
                next_entities.insert(key.clone(), Entity::from_stable_id(item_id));
                staged_containers.push((container_id, container));
                staged_items.push((item_id, component));
            }
        }
        let desired = plan
            .order
            .iter()
            .map(|key| next_containers[key].id)
            .collect::<Vec<_>>();
        if desired != root.children {
            for id in desired {
                mutations.insert(list.id, id, None);
            }
        }
        let content_layout = Arc::make_mut(&mut content.style.layout);
        content_layout.height = Some(LengthSpec::Px(layout.total_extent()));
        content_layout.min_height = content_layout.height;
        content_layout.max_height = content_layout.height;
        content_layout.flex_shrink = Some(0.0);
        if self.read(list, |old| old.style.layout != content.style.layout)? {
            content.project(list.id, &self.world, &mut mutations);
        }

        // Publish external identities and component data only after atomic commit.
        if !mutations.is_empty() {
            self.world.commit(mutations)?;
        }
        if !removed.is_empty() {
            // Everything a despawned row owned goes with it: handlers,
            // views, and the scopes and bindings of a row built as a view.
            self.forget_subtree(&removed);
        }
        self.views.insert(list.id, Box::new(content));
        for (id, component) in staged_items {
            self.install_view(id, component);
        }
        for (id, container) in staged_containers {
            self.install_view(id, container);
        }
        items.entities = next_entities;
        items.containers = next_containers;
        items.ime_owner = ime_owner.or(focused).filter(|id| !removed.contains(id));
        let window = plan.window.clone();
        items
            .materializer
            .commit(plan)
            .map_err(|_| FrameworkError::InvalidVirtualization)?;
        items.publish_list(layout, &window, activity, 0);
        for (id, index, key) in mounted_now {
            on_mount(self, Entity::from_stable_id(id), index, &key)?;
        }
        Ok(window)
    }

    /// Range-gated list sync from the ScrollView offset. Call from a frame hook;
    /// data and retained-key changes are represented by `fingerprint`.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_virtual_list_retained_in<K, C>(
        &mut self,
        scroll: Entity<ScrollView>,
        list: Entity<List>,
        items: &mut VirtualListItems<K, C>,
        layout: &VirtualListLayout,
        overscan: f32,
        fingerprint: u64,
        retained_keys: &[K],
        key_at: impl FnMut(usize) -> K,
        index_of_key: impl FnMut(&K) -> Option<usize>,
        build: impl FnMut(usize, &K) -> C,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        self.sync_virtual_list_retained_with(
            scroll,
            list,
            items,
            layout,
            overscan,
            fingerprint,
            retained_keys,
            key_at,
            index_of_key,
            build,
            |_, _, _, _| Ok(()),
        )
    }

    /// Range-gated list sync with a post-commit mount hook.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_virtual_list_retained_with<K, C>(
        &mut self,
        scroll: Entity<ScrollView>,
        list: Entity<List>,
        items: &mut VirtualListItems<K, C>,
        layout: &VirtualListLayout,
        overscan: f32,
        fingerprint: u64,
        retained_keys: &[K],
        key_at: impl FnMut(usize) -> K,
        index_of_key: impl FnMut(&K) -> Option<usize>,
        build: impl FnMut(usize, &K) -> C,
        on_mount: impl FnMut(&mut Self, Entity<C>, usize, &K) -> Result<(), FrameworkError>,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        self.sync_virtual_list_retained_at(
            scroll,
            list,
            items,
            layout,
            overscan,
            None,
            fingerprint,
            retained_keys,
            key_at,
            index_of_key,
            build,
            on_mount,
        )
    }

    /// [`Self::sync_virtual_list_retained_with`], placing the window at
    /// `at` (the list's own offset) instead of the one the ScrollView holds:
    /// the caller is about to scroll there, and the ScrollView may not reach
    /// it until the next layout gives it the list's new extent.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sync_virtual_list_retained_at<K, C>(
        &mut self,
        scroll: Entity<ScrollView>,
        list: Entity<List>,
        items: &mut VirtualListItems<K, C>,
        layout: &VirtualListLayout,
        overscan: f32,
        at: Option<f32>,
        fingerprint: u64,
        retained_keys: &[K],
        key_at: impl FnMut(usize) -> K,
        index_of_key: impl FnMut(&K) -> Option<usize>,
        build: impl FnMut(usize, &K) -> C,
        on_mount: impl FnMut(&mut Self, Entity<C>, usize, &K) -> Result<(), FrameworkError>,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        let mut viewport = self.virtual_list_viewport(scroll, list, overscan)?;
        if let Some(at) = at {
            viewport.offset[1] = at.max(0.0);
        }
        self.sync_virtual_list_in_viewport(
            list,
            items,
            layout,
            viewport,
            fingerprint,
            retained_keys,
            key_at,
            index_of_key,
            build,
            on_mount,
        )
    }

    /// The range gate and commit of [`Self::sync_virtual_list_retained_with`]
    /// against a viewport the caller already read from the ScrollView.
    #[allow(clippy::too_many_arguments)]
    fn sync_virtual_list_in_viewport<K, C>(
        &mut self,
        list: Entity<List>,
        items: &mut VirtualListItems<K, C>,
        layout: &VirtualListLayout,
        viewport: VirtualViewport,
        fingerprint: u64,
        retained_keys: &[K],
        key_at: impl FnMut(usize) -> K,
        index_of_key: impl FnMut(&K) -> Option<usize>,
        build: impl FnMut(usize, &K) -> C,
        on_mount: impl FnMut(&mut Self, Entity<C>, usize, &K) -> Result<(), FrameworkError>,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        let window = layout.window_for(viewport);
        let activity = self.virtual_list_activity(list, items)?;
        if items.list_range_unchanged(layout, &window, &activity, fingerprint) {
            return Ok(window);
        }
        let result = self.materialize_virtual_list_retained_with(
            list,
            items,
            layout,
            viewport,
            retained_keys,
            key_at,
            index_of_key,
            build,
            on_mount,
        );
        if result.is_ok() {
            items.publish_list(layout, &window, activity, fingerprint);
        }
        result
    }

    /// [`Self::sync_virtual_list_retained_with`] for rows that size to their
    /// content ([`VirtualListItems::measured`]).
    ///
    /// Before placing the window it reads every mounted row's height from the
    /// last layout pass into `layout`, keeping the row at the top of the
    /// viewport where it is: a row above it that measured taller moves the
    /// scroll offset by the difference instead of pushing the content down.
    /// Rows mounted by this call are measured by the next one; while
    /// [`VirtualListItems::pending_measure`] holds, request another frame.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_virtual_list_measured_with<K, C>(
        &mut self,
        scroll: Entity<ScrollView>,
        list: Entity<List>,
        items: &mut VirtualListItems<K, C>,
        layout: &mut VirtualListLayout,
        overscan: f32,
        fingerprint: u64,
        retained_keys: &[K],
        key_at: impl FnMut(usize) -> K,
        index_of_key: impl FnMut(&K) -> Option<usize>,
        build: impl FnMut(usize, &K) -> C,
        on_mount: impl FnMut(&mut Self, Entity<C>, usize, &K) -> Result<(), FrameworkError>,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        self.sync_virtual_list_measured_at(
            scroll,
            list,
            items,
            layout,
            overscan,
            None,
            fingerprint,
            retained_keys,
            key_at,
            index_of_key,
            build,
            on_mount,
        )
        .map(|(window, _)| window)
    }

    /// [`Self::sync_virtual_list_measured_with`] with the window at `at`,
    /// the list's own offset the caller is about to scroll to (see
    /// [`Self::sync_virtual_list_retained_at`]). Measuring keeps the row at
    /// the top of that offset where it is, so it may move: the offset the
    /// window was placed at comes back with it, for the caller to scroll to.
    /// With no `at`, the ScrollView's offset is moved here as before.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sync_virtual_list_measured_at<K, C>(
        &mut self,
        scroll: Entity<ScrollView>,
        list: Entity<List>,
        items: &mut VirtualListItems<K, C>,
        layout: &mut VirtualListLayout,
        overscan: f32,
        at: Option<f32>,
        fingerprint: u64,
        retained_keys: &[K],
        key_at: impl FnMut(usize) -> K,
        mut index_of_key: impl FnMut(&K) -> Option<usize>,
        build: impl FnMut(usize, &K) -> C,
        on_mount: impl FnMut(&mut Self, Entity<C>, usize, &K) -> Result<(), FrameworkError>,
    ) -> Result<(VirtualListWindow, f32), FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        if !items.measured {
            return Err(FrameworkError::InvalidVirtualization);
        }
        let mut viewport = self.virtual_list_viewport(scroll, list, overscan)?;
        if let Some(at) = at {
            viewport.offset[1] = at.max(0.0);
        }
        let before = viewport.offset[1];
        let mut measured = false;
        let mut unmeasured = false;
        for key in items.materializer.mounted() {
            let Some(index) = index_of_key(key) else {
                continue;
            };
            match items
                .containers
                .get(key)
                .and_then(|container| self.world.layout_box(container.id))
            {
                Some(bounds) if bounds.height > 0.0 => {
                    measured |= layout.measure_anchored(index, bounds.height, &mut viewport);
                }
                _ => unmeasured = true,
            }
        }
        if measured {
            // Rows move with the new extents even when the window is the same.
            items.published = None;
        }
        if at.is_none() && viewport.offset[1] != before {
            let current = self.world.scroll_offset(scroll.id).unwrap_or_default();
            let mut mutations = MutationQueue::new();
            mutations.set_scroll_offset(
                scroll.id,
                ScrollOffset {
                    x: current.x,
                    y: current.y + (viewport.offset[1] - before),
                },
            );
            self.world.commit(mutations)?;
            // The world clamps the offset to the extent it knows, so the
            // window follows the offset it actually kept.
            viewport = self.virtual_list_viewport(scroll, list, overscan)?;
        }
        items.pending_measure = unmeasured;
        let offset = viewport.offset[1];
        self.sync_virtual_list_in_viewport(
            list,
            items,
            layout,
            viewport,
            fingerprint,
            retained_keys,
            key_at,
            index_of_key,
            build,
            on_mount,
        )
        .map(|window| (window, offset))
    }

    /// Materialize both axes, frozen prefixes and active cells in one commit.
    /// Table/row/cell positioning is managed by this entry; nested editor content
    /// remains application-owned. Missing keys end retention. Frozen cells use
    /// the Surface token when no explicit background is supplied.
    #[allow(clippy::too_many_arguments)]
    pub fn materialize_virtual_table_retained_in<R, C>(
        &mut self,
        table: Entity<Table>,
        items: &mut VirtualTableItems<R, C>,
        layout: &VirtualTableLayout,
        viewport: VirtualViewport,
        frozen: [usize; 2],
        retained_cells: &[(R, C)],
        mut row_key_at: impl FnMut(usize) -> R,
        mut row_index_of_key: impl FnMut(&R) -> Option<usize>,
        mut column_key_at: impl FnMut(usize) -> C,
        mut column_index_of_key: impl FnMut(&C) -> Option<usize>,
        build_row: impl FnMut(usize, &R) -> TableRow,
        build_cell: impl FnMut(usize, &R, usize, &C) -> TableCell,
    ) -> Result<VirtualTableWindow, FrameworkError>
    where
        R: Clone + Eq + Hash,
        C: Clone + Eq + Hash,
    {
        let viewport = clamp_virtual_table_viewport(layout, viewport);
        self.read(table, |_| ())?;
        let document = self
            .world
            .node(table.id)
            .ok_or(FrameworkError::MissingView(table.id))?
            .document;
        let focused = self.world.focused(document);
        let ime = focused
            .filter(|id| self.world.ime(*id).is_some())
            .or_else(|| items.ime_owner.filter(|id| self.world.ime(*id).is_some()));
        let cell_keys = items
            .cells
            .iter()
            .map(|(key, entity)| (entity.id, key))
            .collect::<HashMap<_, _>>();
        let mut active = retained_cells.iter().cloned().collect::<HashSet<_>>();
        for target in [focused, ime].into_iter().flatten() {
            let mut current = Some(target);
            while let Some(id) = current {
                if id == table.id {
                    break;
                }
                if let Some(key) = cell_keys.get(&id) {
                    active.insert((*key).clone());
                    break;
                }
                current = self.world.parent_id(id);
            }
        }
        let mut rows = Vec::new();
        let mut columns = Vec::new();
        for (row, column) in active {
            if let (Some(r), Some(c)) = (row_index_of_key(&row), column_index_of_key(&column))
                && r < layout.row_count()
                && c < layout.column_count()
                && row_key_at(r) == row
                && column_key_at(c) == column
            {
                rows.push(r);
                columns.push(c);
            }
        }
        let plan = items
            .materializer
            .prepare_retained(
                layout,
                viewport,
                frozen,
                rows,
                columns,
                row_key_at,
                column_key_at,
            )
            .map_err(|_| FrameworkError::InvalidVirtualization)?;
        let activity = self.activity_items(table.id, cell_keys.keys().copied(), focused, ime);
        let window = self.commit_virtual_table(
            table,
            items,
            plan,
            Some((layout, viewport, frozen)),
            build_row,
            build_cell,
        )?;
        items.ime_owner = ime.or(focused).filter(|id| self.world.contains(*id));
        items.publish_table(
            layout,
            &layout.window_with_frozen(viewport, frozen),
            activity,
            0,
        );
        Ok(window)
    }

    /// Range-gated table sync from the ScrollView offset. Frozen transforms are
    /// updated on an unchanged window when the offset changes.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_virtual_table_retained_in<R, C>(
        &mut self,
        scroll: Entity<ScrollView>,
        table: Entity<Table>,
        items: &mut VirtualTableItems<R, C>,
        layout: &VirtualTableLayout,
        overscan: [f32; 2],
        fingerprint: u64,
        frozen: [usize; 2],
        retained_cells: &[(R, C)],
        row_key_at: impl FnMut(usize) -> R,
        row_index_of_key: impl FnMut(&R) -> Option<usize>,
        column_key_at: impl FnMut(usize) -> C,
        column_index_of_key: impl FnMut(&C) -> Option<usize>,
        build_row: impl FnMut(usize, &R) -> TableRow,
        build_cell: impl FnMut(usize, &R, usize, &C) -> TableCell,
    ) -> Result<VirtualTableWindow, FrameworkError>
    where
        R: Clone + Eq + Hash,
        C: Clone + Eq + Hash,
    {
        let viewport = clamp_virtual_table_viewport(
            layout,
            self.virtual_viewport_from_scroll(scroll, overscan)?,
        );
        let pane = layout.window_with_frozen(viewport, frozen);
        let activity = self.virtual_table_activity(table, items)?;
        if items.table_range_unchanged(layout, &pane, &activity, fingerprint) {
            if frozen != [0, 0] {
                self.pin_virtual_table_frozen(items, viewport)?;
            }
            return Ok(VirtualTableWindow {
                rows: pane.rows.body,
                columns: pane.columns.body,
            });
        }
        let result = self.materialize_virtual_table_retained_in(
            table,
            items,
            layout,
            viewport,
            frozen,
            retained_cells,
            row_key_at,
            row_index_of_key,
            column_key_at,
            column_index_of_key,
            build_row,
            build_cell,
        );
        if result.is_ok() {
            let pane = layout.window_with_frozen(viewport, frozen);
            items.publish_table(layout, &pane, activity, fingerprint);
        }
        result
    }

    /// Positioned virtualization over only the tree's expanded row sequence.
    #[allow(clippy::too_many_arguments)]
    pub fn materialize_virtual_tree_retained_in<K, C>(
        &mut self,
        tree: Entity<List>,
        items: &mut VirtualTreeItems<K, C>,
        layout: &VirtualTreeLayout,
        viewport: VirtualViewport,
        retained_keys: &[K],
        key_at: impl FnMut(usize) -> K,
        index_of_key: impl FnMut(&K) -> Option<usize>,
        build: impl FnMut(usize, &K) -> C,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        self.materialize_virtual_list_retained_in(
            tree,
            &mut items.items,
            layout.row_layout(),
            viewport,
            retained_keys,
            key_at,
            index_of_key,
            build,
        )
    }

    /// Range-gated sync over a tree's expanded row sequence.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_virtual_tree_retained_in<K, C>(
        &mut self,
        scroll: Entity<ScrollView>,
        tree: Entity<List>,
        items: &mut VirtualTreeItems<K, C>,
        layout: &VirtualTreeLayout,
        overscan: f32,
        fingerprint: u64,
        retained_keys: &[K],
        key_at: impl FnMut(usize) -> K,
        index_of_key: impl FnMut(&K) -> Option<usize>,
        build: impl FnMut(usize, &K) -> C,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        self.sync_virtual_list_retained_in(
            scroll,
            tree,
            &mut items.items,
            layout.row_layout(),
            overscan,
            fingerprint,
            retained_keys,
            key_at,
            index_of_key,
            build,
        )
    }

    fn virtual_ime_target(
        &self,
        focused: Option<StableNodeId>,
        stored: Option<StableNodeId>,
    ) -> Option<StableNodeId> {
        focused
            .filter(|id| self.world.ime(*id).is_some())
            .or_else(|| stored.filter(|id| self.world.ime(*id).is_some()))
    }

    fn activity_items(
        &self,
        root: StableNodeId,
        owned: impl IntoIterator<Item = StableNodeId>,
        focused: Option<StableNodeId>,
        ime: Option<StableNodeId>,
    ) -> HashSet<StableNodeId> {
        let owned = owned.into_iter().collect::<HashSet<_>>();
        [focused, ime]
            .into_iter()
            .flatten()
            .filter_map(|target| {
                let mut current = Some(target);
                while let Some(id) = current {
                    if id == root {
                        break;
                    }
                    if owned.contains(&id) {
                        return Some(id);
                    }
                    current = self.world.parent_id(id);
                }
                None
            })
            .collect()
    }

    fn virtual_list_activity<K, C>(
        &self,
        list: Entity<List>,
        items: &VirtualListItems<K, C>,
    ) -> Result<HashSet<StableNodeId>, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        let document = self
            .world
            .node(list.id)
            .ok_or(FrameworkError::MissingView(list.id))?
            .document;
        let focused = self.world.focused(document);
        let ime = self.virtual_ime_target(focused, items.ime_owner);
        Ok(self.activity_items(
            list.id,
            items.containers.values().map(|entity| entity.id),
            focused,
            ime,
        ))
    }

    fn virtual_table_activity<R, C>(
        &self,
        table: Entity<Table>,
        items: &VirtualTableItems<R, C>,
    ) -> Result<HashSet<StableNodeId>, FrameworkError>
    where
        R: Clone + Eq + Hash,
        C: Clone + Eq + Hash,
    {
        let document = self
            .world
            .node(table.id)
            .ok_or(FrameworkError::MissingView(table.id))?
            .document;
        let focused = self.world.focused(document);
        let ime = self.virtual_ime_target(focused, items.ime_owner);
        Ok(self.activity_items(
            table.id,
            items.cells.values().map(|entity| entity.id),
            focused,
            ime,
        ))
    }

    /// The part of `scroll`'s viewport that falls on `list`, in the list's
    /// own coordinates. A list need not be the whole scroll content: one below
    /// a page header starts that far down, and only the rows under the
    /// visible part of the viewport are its window. Before the first layout
    /// pass the list is taken to start at the content's top.
    pub(crate) fn virtual_list_viewport(
        &self,
        scroll: Entity<ScrollView>,
        list: Entity<List>,
        overscan: f32,
    ) -> Result<VirtualViewport, FrameworkError> {
        let mut viewport = self.virtual_viewport_from_scroll(scroll, [0.0, overscan])?;
        let start = match (
            self.world.layout_box(list.id),
            self.world.layout_box(scroll.id),
        ) {
            // Layout boxes are unscrolled, so this is where the list sits in
            // the scroll content, padding and preceding siblings included.
            (Some(list), Some(scroll)) => list.y - scroll.y,
            _ => 0.0,
        };
        let top = viewport.offset[1] - start;
        // A list starting below the viewport's top sees only the rest of it.
        viewport.offset[1] = top.max(0.0);
        viewport.extent[1] = (viewport.extent[1] + top.min(0.0)).max(0.0);
        Ok(viewport)
    }

    fn virtual_viewport_from_scroll(
        &self,
        scroll: Entity<ScrollView>,
        overscan: [f32; 2],
    ) -> Result<VirtualViewport, FrameworkError> {
        self.read(scroll, |_| ())?;
        let offset = self.world.scroll_offset(scroll.id).unwrap_or_default();
        let bounds = self.world.layout_box(scroll.id);
        let metrics = self.world.scroll_metrics(scroll.id);
        let width = bounds
            .and_then(|bounds| (bounds.width > 0.0).then_some(bounds.width))
            .or_else(|| metrics.map(|metrics| metrics.viewport_width))
            .unwrap_or(0.0)
            .max(0.0);
        let height = bounds
            .and_then(|bounds| (bounds.height > 0.0).then_some(bounds.height))
            .or_else(|| metrics.map(|metrics| metrics.viewport_height))
            .unwrap_or(0.0)
            .max(0.0);
        // Virtual content is indexed from the scrolling area's left / top
        // edge. That is the offset itself, except on an axis starting at the
        // right / bottom, whose offsets run negative from its origin there.
        let origin = metrics.map_or_else(ScrollOffset::default, ScrollMetrics::min_offset);
        Ok(VirtualViewport {
            offset: [offset.x - origin.x, offset.y - origin.y],
            extent: [width, height],
            overscan,
        })
    }

    fn pin_virtual_table_frozen<R, C>(
        &mut self,
        items: &VirtualTableItems<R, C>,
        viewport: VirtualViewport,
    ) -> Result<(), FrameworkError>
    where
        R: Clone + Eq + Hash,
        C: Clone + Eq + Hash,
    {
        let mut mutations = MutationQueue::new();
        let mut staged_rows = Vec::new();
        let mut staged_cells = Vec::new();
        for entity in items.rows.values().copied() {
            let Some(transform) = self.read(entity, |row| row.style.layout.transform)? else {
                continue;
            };
            if transform.f == viewport.offset[1] {
                continue;
            }
            let mut row = self.read(entity, Clone::clone)?;
            Arc::make_mut(&mut row.style.layout).transform = Some(nana_ui_core::PaintTransform {
                f: viewport.offset[1],
                ..transform
            });
            row.project(entity.id, &self.world, &mut mutations);
            staged_rows.push((entity.id, row));
        }
        for entity in items.cells.values().copied() {
            let Some(transform) = self.read(entity, |cell| cell.style.layout.transform)? else {
                continue;
            };
            if transform.e == viewport.offset[0] {
                continue;
            }
            let mut cell = self.read(entity, Clone::clone)?;
            Arc::make_mut(&mut cell.style.layout).transform = Some(nana_ui_core::PaintTransform {
                e: viewport.offset[0],
                ..transform
            });
            cell.project(entity.id, &self.world, &mut mutations);
            staged_cells.push((entity.id, cell));
        }
        if mutations.is_empty() {
            return Ok(());
        }
        self.world.commit(mutations)?;
        for (id, row) in staged_rows {
            self.install_view(id, row);
        }
        for (id, cell) in staged_cells {
            self.install_view(id, cell);
        }
        Ok(())
    }
}

fn clamp_virtual_table_viewport(
    layout: &VirtualTableLayout,
    mut viewport: VirtualViewport,
) -> VirtualViewport {
    for (axis, total) in [
        layout.column_layout().total_extent(),
        layout.row_layout().total_extent(),
    ]
    .into_iter()
    .enumerate()
    {
        let extent = viewport.extent[axis];
        viewport.extent[axis] = if extent.is_finite() {
            extent.max(0.0)
        } else {
            0.0
        };
        let offset = viewport.offset[axis];
        viewport.offset[axis] = if offset.is_finite() {
            offset
                .max(0.0)
                .min((total - viewport.extent[axis]).max(0.0))
        } else {
            0.0
        };
    }
    viewport
}

pub(super) fn position_table_row(
    row: &mut TableRow,
    layout: &VirtualTableLayout,
    viewport: VirtualViewport,
    frozen: [usize; 2],
    index: usize,
) {
    let style = Arc::make_mut(&mut row.style.layout);
    style.position = PositionSpec::Absolute;
    style.offset_top = Some(LengthSpec::Px(layout.row_layout().extent(0..index)));
    style.offset_left = Some(LengthSpec::Px(0.0));
    style.width = Some(LengthSpec::Px(layout.column_layout().total_extent()));
    style.height = Some(LengthSpec::Px(layout.row_layout().extent(index..index + 1)));
    style.flex_shrink = Some(0.0);
    style.z_index = Some(if index < frozen[1] { 2 } else { 0 });
    style.transform = (index < frozen[1]).then_some(nana_ui_core::PaintTransform {
        f: viewport.offset[1],
        ..Default::default()
    });
}

pub(super) fn position_table_cell(
    cell: &mut TableCell,
    layout: &VirtualTableLayout,
    viewport: VirtualViewport,
    frozen: [usize; 2],
    row: usize,
    column: usize,
) {
    let style = Arc::make_mut(&mut cell.style.layout);
    style.position = PositionSpec::Absolute;
    style.offset_left = Some(LengthSpec::Px(layout.column_layout().extent(0..column)));
    style.offset_top = Some(LengthSpec::Px(0.0));
    style.width = Some(LengthSpec::Px(
        layout.column_layout().extent(column..column + 1),
    ));
    style.height = Some(LengthSpec::Px(layout.row_layout().extent(row..row + 1)));
    style.flex_shrink = Some(0.0);
    style.z_index = Some(if column < frozen[0] { 1 } else { 0 });
    style.transform = (column < frozen[0]).then_some(nana_ui_core::PaintTransform {
        e: viewport.offset[0],
        ..Default::default()
    });
    cell.style
        .background
        .get_or_insert(nana_ui_core::SemanticColorRole::Surface);
}

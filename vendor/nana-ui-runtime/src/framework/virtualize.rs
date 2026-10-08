//! AppContext virtualize operations.

use super::*;

impl AppContext {
    #[allow(clippy::too_many_arguments)]
    pub fn materialize_virtual_table_in<R, C>(
        &mut self,
        table: Entity<Table>,
        items: &mut VirtualTableItems<R, C>,
        layout: &VirtualTableLayout,
        viewport: nana_ui_core::VirtualViewport,
        row_key_at: impl FnMut(usize) -> R,
        column_key_at: impl FnMut(usize) -> C,
        build_row: impl FnMut(usize, &R) -> TableRow,
        build_cell: impl FnMut(usize, &R, usize, &C) -> TableCell,
    ) -> Result<VirtualTableWindow, FrameworkError>
    where
        R: Clone + Eq + Hash,
        C: Clone + Eq + Hash,
    {
        self.materialize_virtual_table(
            table,
            items,
            layout,
            (viewport.offset[0], viewport.offset[1]),
            (viewport.extent[0], viewport.extent[1]),
            (viewport.overscan[0], viewport.overscan[1]),
            row_key_at,
            column_key_at,
            build_row,
            build_cell,
        )
    }

    pub fn materialize_virtual_tree_in<K, C>(
        &mut self,
        tree: Entity<List>,
        items: &mut VirtualTreeItems<K, C>,
        layout: &VirtualTreeLayout,
        viewport: nana_ui_core::VirtualViewport,
        key_at: impl FnMut(usize) -> K,
        build: impl FnMut(usize, &K) -> C,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        self.materialize_virtual_list_in(
            tree,
            &mut items.items,
            layout.row_layout(),
            viewport,
            key_at,
            build,
        )
    }

    /// Materialize a list using the shared viewport contract.
    pub fn materialize_virtual_list_in<K, C>(
        &mut self,
        list: Entity<List>,
        items: &mut VirtualListItems<K, C>,
        layout: &VirtualListLayout,
        viewport: nana_ui_core::VirtualViewport,
        key_at: impl FnMut(usize) -> K,
        build: impl FnMut(usize, &K) -> C,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        self.materialize_virtual_list(
            list,
            items,
            layout,
            viewport.offset[1],
            viewport.extent[1],
            viewport.overscan[1],
            key_at,
            build,
        )
    }

    /// Reconcile a virtual List to one visible keyed window. Creation,
    /// removal, and final child order share one Runtime commit; the external
    /// materializer is published only after that commit succeeds.
    #[allow(clippy::too_many_arguments)]
    pub fn materialize_virtual_list<K, C>(
        &mut self,
        list: Entity<List>,
        items: &mut VirtualListItems<K, C>,
        layout: &VirtualListLayout,
        scroll_offset: f32,
        viewport_extent: f32,
        overscan_extent: f32,
        key_at: impl FnMut(usize) -> K,
        mut build: impl FnMut(usize, &K) -> C,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        self.read(list, |_| ())?;
        let plan = items
            .materializer
            .prepare(
                layout,
                scroll_offset,
                viewport_extent,
                overscan_extent,
                key_at,
            )
            .map_err(|_| FrameworkError::InvalidVirtualization)?;
        let list_node = self
            .world
            .node(list.id)
            .ok_or(FrameworkError::MissingView(list.id))?;
        let mounted = items
            .materializer
            .mounted()
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let owned = items
            .entities
            .values()
            .map(|entity| entity.id)
            .collect::<HashSet<_>>();
        if mounted.len() != items.materializer.mounted().len()
            || mounted.len() != items.entities.len()
            || items.entities.keys().any(|key| !mounted.contains(key))
            || owned.len() != items.entities.len()
            || list_node.children.len() != owned.len()
            || list_node
                .children
                .iter()
                .any(|child| !owned.contains(child))
            || items.entities.values().any(|entity| {
                self.world
                    .node(entity.id)
                    .is_none_or(|node| node.parent != Some(list.id))
                    || self
                        .views
                        .get(&entity.id)
                        .is_none_or(|view| !view.is::<C>())
            })
        {
            return Err(FrameworkError::InvalidVirtualization);
        }
        if plan.mounts.is_empty() && plan.unmounts.is_empty() {
            let desired = plan
                .order
                .iter()
                .map(|key| {
                    items
                        .entities
                        .get(key)
                        .map(|entity| entity.id)
                        .ok_or(FrameworkError::InvalidVirtualization)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if desired == list_node.children {
                let window = plan.window.clone();
                items
                    .materializer
                    .commit(plan)
                    .map_err(|_| FrameworkError::InvalidVirtualization)?;
                return Ok(window);
            }
        }

        let mut removed_nodes = HashSet::new();
        for key in &plan.unmounts {
            let entity = items
                .entities
                .get(key)
                .ok_or(FrameworkError::InvalidVirtualization)?;
            let mut stack = vec![entity.id];
            while let Some(id) = stack.pop() {
                let node = self
                    .world
                    .node(id)
                    .ok_or(FrameworkError::InvalidVirtualization)?;
                stack.extend(node.children);
                removed_nodes.insert(id);
            }
        }

        let mut mutations = MutationQueue::new();
        for key in &plan.unmounts {
            mutations.despawn_subtree(items.entities[key].id);
        }
        let mut staged = Vec::with_capacity(plan.mounts.len());
        for mount in &plan.mounts {
            let component = build(mount.index, &mount.key);
            let id = self.allocate_id();
            mutations.create(id, list_node.document, component.node_kind());
            component.project(id, &self.world, &mut mutations);
            staged.push((mount.key.clone(), Entity::from_stable_id(id), component));
        }

        let mut next_entities = items.entities.clone();
        for key in &plan.unmounts {
            next_entities.remove(key);
        }
        for (key, entity, _) in &staged {
            next_entities.insert(key.clone(), *entity);
        }
        for key in &plan.order {
            let entity = next_entities
                .get(key)
                .ok_or(FrameworkError::InvalidVirtualization)?;
            if self.world.contains(entity.id) && self.world.parent_id(entity.id) != Some(list.id) {
                return Err(FrameworkError::InvalidVirtualization);
            }
            mutations.insert(list.id, entity.id, None);
        }

        self.world.commit(mutations)?;
        self.forget_subtree(&removed_nodes);
        for (_, entity, component) in staged {
            self.install_view(entity.id, component);
        }
        items.entities = next_entities;
        let window = plan.window.clone();
        items
            .materializer
            .commit(plan)
            .map_err(|error| match error {
                VirtualListMaterializationError::DuplicateKey
                | VirtualListMaterializationError::StalePlan => {
                    FrameworkError::InvalidVirtualization
                }
            })?;
        Ok(window)
    }

    /// Reconcile both visible axes of a virtual Table in one Runtime commit.
    /// Rows and cells with overlapping keys retain their stable entities.
    #[allow(clippy::too_many_arguments)]
    pub fn materialize_virtual_table<R, C>(
        &mut self,
        table: Entity<Table>,
        items: &mut VirtualTableItems<R, C>,
        layout: &VirtualTableLayout,
        scroll: (f32, f32),
        viewport: (f32, f32),
        overscan: (f32, f32),
        row_key_at: impl FnMut(usize) -> R,
        column_key_at: impl FnMut(usize) -> C,
        build_row: impl FnMut(usize, &R) -> TableRow,
        build_cell: impl FnMut(usize, &R, usize, &C) -> TableCell,
    ) -> Result<VirtualTableWindow, FrameworkError>
    where
        R: Clone + Eq + Hash,
        C: Clone + Eq + Hash,
    {
        self.read(table, |_| ())?;
        let plan = items
            .materializer
            .prepare(
                layout,
                scroll,
                viewport,
                overscan,
                row_key_at,
                column_key_at,
            )
            .map_err(|_| FrameworkError::InvalidVirtualization)?;
        // `VirtualTableItems` keeps its row/cell maps private and every
        // mutation goes through this reconciler. When both axes are unchanged,
        // the retained tree invariants were established by the previous commit;
        // avoid rebuilding validation HashSets on steady scroll ticks.
        if plan.rows.mounts.is_empty()
            && plan.rows.unmounts.is_empty()
            && plan.columns.mounts.is_empty()
            && plan.columns.unmounts.is_empty()
        {
            let window = plan.window.clone();
            items
                .materializer
                .commit(plan)
                .map_err(|_| FrameworkError::InvalidVirtualization)?;
            return Ok(window);
        }
        self.commit_virtual_table(table, items, plan, None, build_row, build_cell)
    }

    pub(super) fn commit_virtual_table<R, C>(
        &mut self,
        table: Entity<Table>,
        items: &mut VirtualTableItems<R, C>,
        plan: nana_ui_core::VirtualTableMaterialization<R, C>,
        placement: Option<(
            &VirtualTableLayout,
            nana_ui_core::VirtualViewport,
            [usize; 2],
        )>,
        mut build_row: impl FnMut(usize, &R) -> TableRow,
        mut build_cell: impl FnMut(usize, &R, usize, &C) -> TableCell,
    ) -> Result<VirtualTableWindow, FrameworkError>
    where
        R: Clone + Eq + Hash,
        C: Clone + Eq + Hash,
    {
        let table_node = self
            .world
            .node(table.id)
            .ok_or(FrameworkError::MissingView(table.id))?;
        let mounted_rows = items
            .materializer
            .mounted_rows()
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let mounted_columns = items
            .materializer
            .mounted_columns()
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let row_ids = items
            .rows
            .values()
            .map(|entity| entity.id)
            .collect::<HashSet<_>>();
        let cell_ids = items
            .cells
            .values()
            .map(|entity| entity.id)
            .collect::<HashSet<_>>();
        let expected_cell_count = items
            .rows
            .len()
            .checked_mul(mounted_columns.len())
            .ok_or(FrameworkError::InvalidVirtualization)?;
        let invalid_rows = mounted_rows.len() != items.materializer.mounted_rows().len()
            || mounted_rows.len() != items.rows.len()
            || items.rows.keys().any(|key| !mounted_rows.contains(key))
            || row_ids.len() != items.rows.len()
            || table_node.children.len() != row_ids.len()
            || table_node
                .children
                .iter()
                .any(|child| !row_ids.contains(child));
        let invalid_columns = mounted_columns.len() != items.materializer.mounted_columns().len()
            || items.cells.len() != expected_cell_count
            || cell_ids.len() != items.cells.len()
            || items.cells.keys().any(|(row, column)| {
                !mounted_rows.contains(row) || !mounted_columns.contains(column)
            });
        if invalid_rows || invalid_columns {
            return Err(FrameworkError::InvalidVirtualization);
        }
        for (row_key, row_entity) in &items.rows {
            let Some(row_node) = self.world.node(row_entity.id) else {
                return Err(FrameworkError::InvalidVirtualization);
            };
            if row_node.parent != Some(table.id)
                || self
                    .views
                    .get(&row_entity.id)
                    .is_none_or(|view| !view.is::<TableRow>())
                || row_node.children.len() != mounted_columns.len()
            {
                return Err(FrameworkError::InvalidVirtualization);
            }
            for column_key in &mounted_columns {
                let Some(cell) = items.cells.get(&(row_key.clone(), column_key.clone())) else {
                    return Err(FrameworkError::InvalidVirtualization);
                };
                if !row_node.children.contains(&cell.id)
                    || self
                        .world
                        .node(cell.id)
                        .is_none_or(|node| node.parent != Some(row_entity.id))
                    || self
                        .views
                        .get(&cell.id)
                        .is_none_or(|view| !view.is::<TableCell>())
                {
                    return Err(FrameworkError::InvalidVirtualization);
                }
            }
        }

        let desired_rows = plan
            .rows
            .order
            .iter()
            .map(|key| {
                items
                    .rows
                    .get(key)
                    .map(|entity| entity.id)
                    .ok_or(FrameworkError::InvalidVirtualization)
            })
            .collect::<Result<Vec<_>, _>>();
        if placement.is_none()
            && plan.rows.mounts.is_empty()
            && plan.rows.unmounts.is_empty()
            && plan.columns.mounts.is_empty()
            && plan.columns.unmounts.is_empty()
            && desired_rows
                .as_ref()
                .is_ok_and(|rows| *rows == table_node.children)
            && plan.rows.order.iter().all(|row| {
                let Some(row_entity) = items.rows.get(row) else {
                    return false;
                };
                let desired_cells = plan
                    .columns
                    .order
                    .iter()
                    .filter_map(|column| items.cells.get(&(row.clone(), column.clone())))
                    .map(|entity| entity.id)
                    .collect::<Vec<_>>();
                self.world
                    .node(row_entity.id)
                    .is_some_and(|node| node.children == desired_cells)
            })
        {
            let window = plan.window.clone();
            items
                .materializer
                .commit(plan)
                .map_err(|_| FrameworkError::InvalidVirtualization)?;
            return Ok(window);
        }

        let removed_rows = plan.rows.unmounts.iter().cloned().collect::<HashSet<_>>();
        let mut removed_nodes = HashSet::new();
        for row in &plan.rows.unmounts {
            let mut stack = vec![items.rows[row].id];
            while let Some(id) = stack.pop() {
                let node = self
                    .world
                    .node(id)
                    .ok_or(FrameworkError::InvalidVirtualization)?;
                stack.extend(node.children);
                removed_nodes.insert(id);
            }
        }
        for row in items.rows.keys().filter(|row| !removed_rows.contains(*row)) {
            for column in &plan.columns.unmounts {
                let cell = items
                    .cells
                    .get(&(row.clone(), column.clone()))
                    .ok_or(FrameworkError::InvalidVirtualization)?;
                let mut stack = vec![cell.id];
                while let Some(id) = stack.pop() {
                    let node = self
                        .world
                        .node(id)
                        .ok_or(FrameworkError::InvalidVirtualization)?;
                    stack.extend(node.children);
                    removed_nodes.insert(id);
                }
            }
        }

        let mut mutations = MutationQueue::new();
        for row in &plan.rows.unmounts {
            mutations.despawn_subtree(items.rows[row].id);
        }
        for row in items.rows.keys().filter(|row| !removed_rows.contains(*row)) {
            for column in &plan.columns.unmounts {
                mutations.despawn_subtree(items.cells[&(row.clone(), column.clone())].id);
            }
        }

        let row_indices = plan
            .rows
            .indices
            .iter()
            .copied()
            .zip(plan.rows.order.iter().cloned())
            .map(|(index, key)| (key, index))
            .collect::<HashMap<_, _>>();
        let column_indices = plan
            .columns
            .indices
            .iter()
            .copied()
            .zip(plan.columns.order.iter().cloned())
            .map(|(index, key)| (key, index))
            .collect::<HashMap<_, _>>();
        let new_rows = plan
            .rows
            .mounts
            .iter()
            .map(|mount| mount.key.clone())
            .collect::<HashSet<_>>();
        let new_columns = plan
            .columns
            .mounts
            .iter()
            .map(|mount| mount.key.clone())
            .collect::<HashSet<_>>();
        let mut next_rows = items.rows.clone();
        let mut next_cells = items.cells.clone();
        for row in &plan.rows.unmounts {
            next_rows.remove(row);
            next_cells.retain(|(cell_row, _), _| cell_row != row);
        }
        for column in &plan.columns.unmounts {
            next_cells.retain(|(_, cell_column), _| cell_column != column);
        }

        let mut staged_rows = Vec::with_capacity(plan.rows.mounts.len());
        for mount in &plan.rows.mounts {
            let component = build_row(mount.index, &mount.key);
            let id = self.allocate_id();
            mutations.create(id, table_node.document, component.node_kind());
            component.project(id, &self.world, &mut mutations);
            let entity = Entity::from_stable_id(id);
            next_rows.insert(mount.key.clone(), entity);
            staged_rows.push((entity, component));
        }

        let mut staged_cells = Vec::new();
        for row in &plan.rows.order {
            for column in &plan.columns.order {
                if !new_rows.contains(row) && !new_columns.contains(column) {
                    continue;
                }
                let row_index = row_indices[row];
                let column_index = column_indices[column];
                let component = build_cell(row_index, row, column_index, column);
                let id = self.allocate_id();
                mutations.create(id, table_node.document, component.node_kind());
                component.project(id, &self.world, &mut mutations);
                let entity = Entity::from_stable_id(id);
                next_cells.insert((row.clone(), column.clone()), entity);
                staged_cells.push((entity, component));
            }
        }
        let desired_rows = plan
            .rows
            .order
            .iter()
            .map(|row| next_rows[row].id)
            .collect::<Vec<_>>();
        if desired_rows != table_node.children {
            for id in desired_rows {
                mutations.insert(table.id, id, None);
            }
        }
        for row in &plan.rows.order {
            let row_entity = next_rows[row];
            let desired = plan
                .columns
                .order
                .iter()
                .map(|column| next_cells[&(row.clone(), column.clone())].id)
                .collect::<Vec<_>>();
            if self
                .world
                .node(row_entity.id)
                .is_none_or(|node| node.children != desired)
            {
                for id in desired {
                    mutations.insert(row_entity.id, id, None);
                }
            }
        }

        let mut positioned_rows = Vec::new();
        let mut positioned_cells = Vec::new();
        let mut positioned_table = None;
        if let Some((layout, viewport, frozen)) = placement {
            let fresh_rows = staged_rows
                .iter()
                .map(|(entity, value)| (entity.id, value))
                .collect::<HashMap<_, _>>();
            let fresh_cells = staged_cells
                .iter()
                .map(|(entity, value)| (entity.id, value))
                .collect::<HashMap<_, _>>();
            for row in &plan.rows.order {
                let entity = next_rows[row];
                let index = row_indices[row];
                let mut component = match fresh_rows.get(&entity.id) {
                    Some(value) => (*value).clone(),
                    None => self.read(entity, Clone::clone)?,
                };
                let old = component.clone();
                super::virtualize_retained::position_table_row(
                    &mut component,
                    layout,
                    viewport,
                    frozen,
                    index,
                );
                if component != old || new_rows.contains(row) {
                    component.project(entity.id, &self.world, &mut mutations);
                    positioned_rows.push((entity, component));
                }
                for column in &plan.columns.order {
                    let cell = next_cells[&(row.clone(), column.clone())];
                    let column_index = column_indices[column];
                    let mut component = match fresh_cells.get(&cell.id) {
                        Some(value) => (*value).clone(),
                        None => self.read(cell, Clone::clone)?,
                    };
                    let old = component.clone();
                    super::virtualize_retained::position_table_cell(
                        &mut component,
                        layout,
                        viewport,
                        frozen,
                        index,
                        column_index,
                    );
                    if component != old || new_rows.contains(row) || new_columns.contains(column) {
                        component.project(cell.id, &self.world, &mut mutations);
                        positioned_cells.push((cell, component));
                    }
                }
            }
            let mut component = self.read(table, Clone::clone)?;
            let old = component.clone();
            let style = Arc::make_mut(&mut component.style.layout);
            style.width = Some(LengthSpec::Px(layout.column_layout().total_extent()));
            style.height = Some(LengthSpec::Px(layout.row_layout().total_extent()));
            style.min_height = style.height;
            style.flex_shrink = Some(0.0);
            if component != old {
                component.project(table.id, &self.world, &mut mutations);
                positioned_table = Some(component);
            }
        }
        self.world.commit(mutations)?;
        self.forget_subtree(&removed_nodes);
        for (entity, component) in staged_rows {
            self.install_view(entity.id, component);
        }
        for (entity, component) in staged_cells {
            self.install_view(entity.id, component);
        }
        for (entity, component) in positioned_rows {
            self.views.insert(entity.id, Box::new(component));
        }
        for (entity, component) in positioned_cells {
            self.views.insert(entity.id, Box::new(component));
        }
        if let Some(component) = positioned_table {
            self.views.insert(table.id, Box::new(component));
        }
        items.rows = next_rows;
        items.cells = next_cells;
        let window = plan.window.clone();
        items
            .materializer
            .commit(plan)
            .map_err(|_| FrameworkError::InvalidVirtualization)?;
        Ok(window)
    }

    /// Reconcile a virtual Tree to one visible keyed window of flattened
    /// expanded rows. Creation, removal, and final child order share one
    /// Runtime commit; collapsed descendants are never spawned.
    #[allow(clippy::too_many_arguments)]
    pub fn materialize_virtual_tree<K, C>(
        &mut self,
        tree: Entity<List>,
        items: &mut VirtualTreeItems<K, C>,
        layout: &VirtualTreeLayout,
        scroll_offset: f32,
        viewport_extent: f32,
        overscan_extent: f32,
        key_at: impl FnMut(usize) -> K,
        build: impl FnMut(usize, &K) -> C,
    ) -> Result<VirtualListWindow, FrameworkError>
    where
        K: Clone + Eq + Hash,
        C: ComponentView,
    {
        self.materialize_virtual_list(
            tree,
            &mut items.items,
            layout.row_layout(),
            scroll_offset,
            viewport_extent,
            overscan_extent,
            key_at,
            build,
        )
    }
}

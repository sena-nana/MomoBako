//! AppContext choice operations.

use super::*;

impl AppContext {
    pub fn toggle_select(&mut self, entity: Entity<Select>) -> Result<bool, FrameworkError> {
        if self.read(entity, Select::inactive)? {
            return Ok(false);
        }
        self.update_component(entity, |select, _| select.toggle_open())
    }

    pub fn activate_select_at(
        &mut self,
        entity: Entity<Select>,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        if self.read(entity, Select::inactive)? {
            return Ok(false);
        }
        let opened = self.read(entity, |select| select.opened)?;
        if opened {
            if let Some(crate::ComponentGeometry::Select {
                menu: Some(menu), ..
            }) = self.world.component_geometry(entity.id)
                && let Some(index) = crate::select::select_option_at(&menu, x, y)
            {
                return self.update_component(entity, |select, cx| {
                    if let Some(changed) = select.select_index(index) {
                        cx.emit(changed);
                        true
                    } else {
                        false
                    }
                });
            }
            let Some(field) = self.world.layout_box(entity.id) else {
                return Ok(false);
            };
            if field.contains(x, y) {
                return self.toggle_select(entity);
            }
            return self.update_component(entity, |select, _| {
                select.close();
                true
            });
        }
        self.toggle_select(entity)
    }

    pub fn adjust_focused_select(
        &mut self,
        document: DocumentId,
        delta: i32,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().focused(document) else {
            return Ok(false);
        };
        if !self
            .views
            .get(&target)
            .is_some_and(|view| view.is::<Select>())
        {
            return Ok(false);
        }
        let entity = Entity::<Select>::from_stable_id(target);
        if self.read(entity, Select::inactive)? {
            return Ok(false);
        }
        self.update_component(entity, |select, _| {
            if !select.opened {
                select.toggle_open()
            } else {
                select.highlight_delta(delta)
            }
        })
    }

    pub fn adjust_focused_dropdown(
        &mut self,
        document: DocumentId,
        delta: i32,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().focused(document) else {
            return Ok(false);
        };
        if !self
            .views
            .get(&target)
            .is_some_and(|view| view.is::<Dropdown>())
        {
            return Ok(false);
        }
        let entity = Entity::<Dropdown>::from_stable_id(target);
        if self.read(entity, Dropdown::inactive)? {
            return Ok(false);
        }
        self.update_component(entity, |dropdown, cx| {
            if !dropdown.opened {
                if let Some(event) = dropdown.toggle_open() {
                    cx.emit(event);
                    true
                } else {
                    false
                }
            } else {
                dropdown.highlight_delta(delta)
            }
        })
    }

    pub fn commit_focused_dropdown(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().focused(document) else {
            return Ok(false);
        };
        if !self
            .views
            .get(&target)
            .is_some_and(|view| view.is::<Dropdown>())
        {
            return Ok(false);
        }
        self.update_component(
            Entity::<Dropdown>::from_stable_id(target),
            |dropdown, cx| {
                if let Some(event) = dropdown.commit_highlighted() {
                    cx.emit(event);
                    true
                } else {
                    false
                }
            },
        )
    }

    pub fn commit_focused_select(&mut self, document: DocumentId) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().focused(document) else {
            return Ok(false);
        };
        if !self
            .views
            .get(&target)
            .is_some_and(|view| view.is::<Select>())
        {
            return Ok(false);
        }
        self.update_component(Entity::<Select>::from_stable_id(target), |select, cx| {
            if let Some(changed) = select.commit_highlighted() {
                cx.emit(changed);
                true
            } else {
                false
            }
        })
    }

    pub fn toggle_dropdown(&mut self, entity: Entity<Dropdown>) -> Result<bool, FrameworkError> {
        if self.read(entity, Dropdown::inactive)? {
            return Ok(false);
        }
        self.update_component(entity, |dropdown, cx| {
            if let Some(event) = dropdown.toggle_open() {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    pub fn toggle_search_dropdown(
        &mut self,
        entity: Entity<SearchDropdown>,
    ) -> Result<bool, FrameworkError> {
        if self.read(entity, SearchDropdown::inactive)? {
            return Ok(false);
        }
        if self.read(entity, |dropdown| dropdown.opened)? {
            return Ok(false);
        }
        self.update_component(entity, |dropdown, cx| {
            if let Some(event) = dropdown.toggle_open() {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    pub fn activate_search_dropdown_at(
        &mut self,
        entity: Entity<SearchDropdown>,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        if self.read(entity, SearchDropdown::inactive)? {
            return Ok(false);
        }
        let menu = match self.world.component_geometry(entity.id) {
            Some(crate::ComponentGeometry::Select { menu, .. }) => menu,
            _ => None,
        };
        let Some(field) = self.world.layout_box(entity.id) else {
            return Ok(false);
        };
        self.update_component(entity, |dropdown, cx| {
            if let Some(event) = crate::search_dropdown::activate_search_dropdown_at(
                dropdown,
                menu.as_ref(),
                field,
                x,
                y,
            ) {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    pub fn adjust_focused_search_dropdown(
        &mut self,
        document: DocumentId,
        delta: i32,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().focused(document) else {
            return Ok(false);
        };
        if !self
            .views
            .get(&target)
            .is_some_and(|view| view.is::<SearchDropdown>())
        {
            return Ok(false);
        }
        let entity = Entity::<SearchDropdown>::from_stable_id(target);
        if self.read(entity, SearchDropdown::inactive)? {
            return Ok(false);
        }
        self.update_component(entity, |dropdown, cx| {
            if !dropdown.opened {
                if let Some(event) = dropdown.toggle_open() {
                    cx.emit(event);
                    true
                } else {
                    false
                }
            } else {
                dropdown.highlight_delta(delta)
            }
        })
    }

    pub fn commit_focused_search_dropdown(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().focused(document) else {
            return Ok(false);
        };
        if !self
            .views
            .get(&target)
            .is_some_and(|view| view.is::<SearchDropdown>())
        {
            return Ok(false);
        }
        self.update_component(
            Entity::<SearchDropdown>::from_stable_id(target),
            |dropdown, cx| {
                if let Some(event) = dropdown.commit_highlighted() {
                    cx.emit(event);
                    true
                } else {
                    false
                }
            },
        )
    }

    pub fn activate_command_palette_at(
        &mut self,
        entity: Entity<CommandPalette>,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(crate::ComponentGeometry::CommandPalette {
            surface,
            input,
            rows,
            ..
        }) = self.world.component_geometry(entity.id)
        else {
            return Ok(false);
        };
        self.update_component(entity, |palette, cx| {
            if let Some(event) = crate::command_palette::activate_command_palette_at(
                palette,
                surface,
                input.bounds,
                &rows,
                x,
                y,
            ) {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    pub fn navigate_focused_command_palette(
        &mut self,
        document: DocumentId,
        navigation: ActionPickerNavigation,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().focused(document) else {
            return Ok(false);
        };
        if !self
            .views
            .get(&target)
            .is_some_and(|view| view.is::<CommandPalette>())
        {
            return Ok(false);
        }
        self.update_component(
            Entity::<CommandPalette>::from_stable_id(target),
            |palette, cx| {
                if let Some(event) = palette.navigate(navigation) {
                    cx.emit(event);
                    true
                } else {
                    false
                }
            },
        )
    }

    pub fn activate_dropdown_at(
        &mut self,
        entity: Entity<Dropdown>,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        if self.read(entity, Dropdown::inactive)? {
            return Ok(false);
        }
        let menu = match self.world.component_geometry(entity.id) {
            Some(crate::ComponentGeometry::Select { menu, .. }) => menu,
            _ => None,
        };
        let Some(field) = self.world.layout_box(entity.id) else {
            return Ok(false);
        };
        self.update_component(entity, |dropdown, cx| {
            if let Some(event) =
                crate::dropdown::activate_dropdown_at(dropdown, menu.as_ref(), field, x, y)
            {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    pub fn activate_tree_at(
        &mut self,
        entity: Entity<TreeView>,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(crate::ComponentGeometry::TreeView { rows }) =
            self.world.component_geometry(entity.id)
        else {
            return Ok(false);
        };
        if let Some(index) = crate::tree_view::tree_disclosure_at(&rows, x, y) {
            let id = Arc::clone(&rows[index].id);
            return self.update_component(entity, |tree, cx| {
                let event = crate::TreeViewEvent::Toggle(id);
                if tree.apply_event(event.clone()) {
                    cx.emit(event);
                    true
                } else {
                    false
                }
            });
        }
        if let Some(index) = crate::tree_view::tree_row_at(&rows, x, y) {
            if rows[index].disabled {
                return Ok(false);
            }
            let id = Arc::clone(&rows[index].id);
            return self.update_component(entity, |tree, cx| {
                let event = crate::TreeViewEvent::Select(id);
                if tree.apply_event(event.clone()) {
                    cx.emit(event);
                    true
                } else {
                    false
                }
            });
        }
        Ok(false)
    }

    pub fn navigate_focused_tree(
        &mut self,
        document: DocumentId,
        navigation: crate::TreeNavigation,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().focused(document) else {
            return Ok(false);
        };
        if !self
            .views
            .get(&target)
            .is_some_and(|view| view.is::<TreeView>())
        {
            return Ok(false);
        }
        self.update_component(Entity::<TreeView>::from_stable_id(target), |tree, cx| {
            if let Some(event) = tree.navigate(navigation) {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    pub fn activate_node_at(
        &mut self,
        id: StableNodeId,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        if let Some(activate) = self.behavior(id).and_then(|b| b.activate_at) {
            return activate(self, id, x, y);
        }
        self.activate_node(id)
    }

    /// Route a secondary (right) press to the nearest `SecondaryPress` handler
    /// at or above the hit node.
    ///
    /// Returns the node that handled it. The framework opens no menu and picks
    /// no default items; an application with no handler gets `None`.
    pub fn secondary_press_at(
        &mut self,
        document: DocumentId,
        x: f32,
        y: f32,
    ) -> Result<Option<StableNodeId>, FrameworkError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(FrameworkError::InvalidInput);
        }
        let Some(target) = self.world.hit_test(document, x, y) else {
            return Ok(None);
        };
        let press = SecondaryPress { target, x, y };
        let mut current = Some(target);
        while let Some(id) = current {
            if self
                .event_handlers
                .contains_key(&(id, TypeId::of::<SecondaryPress>()))
                && let Some(emit) = self
                    .views
                    .get(&id)
                    .and_then(|view| self.secondary_presses.get(&view.as_ref().type_id()))
                    .cloned()
            {
                emit(self, id, press)?;
                return Ok(Some(id));
            }
            // Reorder-list row bodies are hit-tested at the list shell (row
            // surfaces are pointer-transparent), so a secondary press there
            // never reaches a row handler; resolve the row on the list itself.
            #[cfg(feature = "controls")]
            if self.emit_reorder_row_secondary(id, x, y)? {
                return Ok(Some(id));
            }
            current = self.world.parent_id(id);
        }
        Ok(None)
    }

    /// Close only the focused field's detached options, leaving its value and
    /// search draft intact. Returns false once the options are already closed.
    pub fn dismiss_focused_field_options(
        &mut self,
        document: DocumentId,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world().focused(document) else {
            return Ok(false);
        };
        match self.behavior(target).and_then(|b| b.close_options) {
            Some(close) => close(self, target),
            None => Ok(false),
        }
    }

    pub fn dismiss_detached_menus(
        &mut self,
        keep: Option<StableNodeId>,
    ) -> Result<(), FrameworkError> {
        let ids = self.views.keys().copied().collect::<Vec<_>>();
        for id in ids {
            if Some(id) == keep {
                continue;
            }
            if let Some(close) = self.behavior(id).and_then(|b| b.close_options) {
                close(self, id)?;
            }
        }
        Ok(())
    }

    /// Closes every open popover whose policy `allows` dismissal. A popover
    /// keeps its state when `inside` sits in its own subtree, so pressing one
    /// of its items still activates that item.
    pub(super) fn close_open_popovers(
        &mut self,
        inside: Option<StableNodeId>,
        allows: fn(&Popover) -> bool,
    ) -> Result<bool, FrameworkError> {
        let ids = self.views.keys().copied().collect::<Vec<_>>();
        let mut dismissed = false;
        for id in ids {
            if inside.is_some_and(|node| self.world.is_descendant_or_self(node, id)) {
                continue;
            }
            if let Some(entity) = self.view_entity::<Popover>(id) {
                if self.read(entity, |popover| popover.open && allows(popover))? {
                    self.toggle_popover(entity)?;
                    dismissed = true;
                }
            } else if let Some(entity) = self.view_entity::<ActionMenu>(id)
                && self.read(entity, |menu| menu.popover.open && allows(&menu.popover))?
            {
                self.toggle_action_menu(entity)?;
                dismissed = true;
            }
        }
        Ok(dismissed)
    }

    /// Light dismiss for toggle-driven popovers, mirroring the outside-press
    /// rule the overlay host applies to dialogs and menus. `inside` is the node
    /// under the pointer. Returns whether anything closed; the caller consumes
    /// the press in that case so it cannot also drive the control underneath,
    /// nor re-open the popover through its own trigger.
    pub fn dismiss_popovers_outside(
        &mut self,
        inside: Option<StableNodeId>,
    ) -> Result<bool, FrameworkError> {
        self.close_open_popovers(inside, |popover| popover.close_on_outside)
    }

    /// Escape closes every open popover and hover card that allows it.
    pub fn dismiss_popovers_on_escape(&mut self) -> Result<bool, FrameworkError> {
        let mut dismissed = self.close_open_popovers(None, |popover| popover.close_on_escape)?;
        let targets = self
            .component_lifecycle
            .hover_cards
            .iter()
            .filter(|(_, lifecycle)| lifecycle.open)
            .map(|(&target, _)| target)
            .collect::<Vec<_>>();
        for target in targets {
            let allows_escape = self
                .view_entity::<crate::HoverCard>(target)
                .and_then(|entity| self.read(entity, |card| card.close_on_escape).ok())
                .unwrap_or(false);
            if allows_escape && self.close_hover_card(target)? {
                dismissed = true;
            }
        }
        Ok(dismissed)
    }

    pub fn toggle_popover(&mut self, entity: Entity<Popover>) -> Result<bool, FrameworkError> {
        self.update_component(entity, |popover, cx| {
            popover.open = !popover.open;
            cx.emit(PopoverToggled { open: popover.open });
            if !popover.open {
                cx.emit(PopoverClosed);
            }
            true
        })
    }

    /// A press released on a popover: its trigger toggles it; the open
    /// surface around its items, which takes the pointer above the page so
    /// the press does not fall through, does nothing.
    pub(crate) fn activate_popover_at(
        &mut self,
        entity: Entity<Popover>,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        if self.world.hanging_surface_contains(entity.id, x, y) {
            return Ok(false);
        }
        self.toggle_popover(entity)
    }

    /// [`Self::activate_popover_at`] for an [`ActionMenu`].
    pub(crate) fn activate_action_menu_at(
        &mut self,
        entity: Entity<ActionMenu>,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        if self.world.hanging_surface_contains(entity.id, x, y) {
            return Ok(false);
        }
        self.toggle_action_menu(entity)
    }

    /// A press released on a hover card activates its trigger, not the open
    /// card's surface around its content.
    pub(crate) fn activate_hover_card_at(
        &mut self,
        entity: Entity<HoverCard>,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        if self.world.hanging_surface_contains(entity.id, x, y) {
            return Ok(false);
        }
        self.activate_hover_card(entity)
    }

    /// A popover that closed around the focus (on one of its items) gives it
    /// back to its trigger, which stays on screen, instead of leaving it on a
    /// hidden item. Runs after each write, so it holds however the popover
    /// closed: its trigger, Escape, a press elsewhere, or the application.
    pub(crate) fn return_focus_to_popover_trigger(
        &mut self,
        id: StableNodeId,
        open: bool,
    ) -> Result<(), FrameworkError> {
        if open {
            return Ok(());
        }
        let Some(document) = self.world.document_of(id) else {
            return Ok(());
        };
        let Some(focused) = self.world.focused(document) else {
            return Ok(());
        };
        if focused != id
            && self.world.is_descendant_or_self(focused, id)
            && self
                .world
                .interaction(id)
                .is_some_and(|interaction| interaction.focusable)
        {
            self.focus_node(document, id)?;
        }
        Ok(())
    }

    pub(crate) fn settle_popover(&mut self, entity: Entity<Popover>) -> Result<(), FrameworkError> {
        let open = self.read(entity, |popover| popover.open)?;
        self.return_focus_to_popover_trigger(entity.stable_id(), open)
    }

    pub(crate) fn settle_action_menu(
        &mut self,
        entity: Entity<ActionMenu>,
    ) -> Result<(), FrameworkError> {
        let open = self.read(entity, |menu| menu.popover.open)?;
        self.return_focus_to_popover_trigger(entity.stable_id(), open)
    }

    pub fn toggle_action_menu(
        &mut self,
        entity: Entity<ActionMenu>,
    ) -> Result<bool, FrameworkError> {
        self.update_component(entity, |menu, cx| {
            menu.popover.open = !menu.popover.open;
            cx.emit(PopoverToggled {
                open: menu.popover.open,
            });
            if !menu.popover.open {
                cx.emit(PopoverClosed);
            }
            true
        })
    }

    pub fn activate_context_menu_at(
        &mut self,
        entity: Entity<ContextMenu>,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(geometry) = self.world.component_geometry(entity.id) else {
            return Ok(false);
        };
        let Some(index) = crate::menus::context_menu_option_at(&geometry, x, y) else {
            return Ok(false);
        };
        self.update_component(entity, |menu, cx| {
            if let Some(event) = menu.select_index(index) {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    pub fn cancel_progress(&mut self, entity: Entity<Progress>) -> Result<bool, FrameworkError> {
        self.update_component(entity, |progress, cx| {
            if !progress.cancellable {
                return false;
            }
            cx.emit(ProgressCancelled);
            true
        })
    }

    pub fn dismiss_context_menu(
        &mut self,
        entity: Entity<ContextMenu>,
    ) -> Result<bool, FrameworkError> {
        self.update_component(entity, |menu, cx| {
            if !menu.open {
                return false;
            }
            menu.dismiss();
            cx.emit(ContextMenuEvent::Dismiss);
            true
        })
    }
}

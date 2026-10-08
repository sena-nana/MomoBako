//! AppContext events operations.

use super::*;

/// A node listening for [`SizeChanged`]: the size it was last told, and how
/// to tell it, since the watch outlives the handler's typed call site.
#[derive(Clone, Copy)]
pub(crate) struct SizeWatch {
    sent: Option<(f32, f32)>,
    emit: fn(&mut AppContext, StableNodeId, SizeChanged) -> Result<(), FrameworkError>,
}

fn emit_size<V: View>(
    cx: &mut AppContext,
    id: StableNodeId,
    event: SizeChanged,
) -> Result<(), FrameworkError> {
    cx.update(Entity::<V>::from_stable_id(id), |_, cx| cx.emit(event))
}

impl AppContext {
    pub(super) fn remove_event_handlers_for(&mut self, removed: &HashSet<StableNodeId>) -> usize {
        // An editor's undo journal dies with the editor.
        for id in removed {
            self.text_histories.forget(*id);
            self.key_handlers.remove(id);
            self.clamp_watchers.remove(id);
            self.size_watchers.remove(id);
        }
        let affected = removed
            .iter()
            .filter_map(|id| self.event_dependencies.remove(id))
            .flatten()
            .collect::<HashSet<_>>();
        let visited = affected.len();
        for key in affected {
            let Some(mut handlers) = self.event_handlers.remove(&key) else {
                continue;
            };
            let owners = std::iter::once(key.0)
                .chain(handlers.iter().map(|handler| handler.observer))
                .collect::<HashSet<_>>();
            for owner in owners {
                if let Some(keys) = self.event_dependencies.get_mut(&owner) {
                    keys.remove(&key);
                    if keys.is_empty() {
                        self.event_dependencies.remove(&owner);
                    }
                }
            }
            if removed.contains(&key.0) {
                continue;
            }
            handlers.retain(|handler| !removed.contains(&handler.observer));
            if !handlers.is_empty() {
                for handler in &handlers {
                    self.index_event_handler(key, handler.observer);
                }
                self.event_handlers.insert(key, handlers);
            }
        }
        visited
    }

    /// Drop every handler `observer` registered on `source` with
    /// [`Self::observe`]; the handlers of anyone else stay.
    pub(crate) fn unobserve(&mut self, source: StableNodeId, observer: StableNodeId) {
        let Some(keys) = self.event_dependencies.get(&source) else {
            return;
        };
        let keys = keys
            .iter()
            .copied()
            .filter(|key| key.0 == source)
            .collect::<Vec<_>>();
        for key in keys {
            let Some(handlers) = self.event_handlers.get_mut(&key) else {
                continue;
            };
            handlers.retain(|handler| handler.observer != observer);
            let empty = handlers.is_empty();
            if empty {
                self.event_handlers.remove(&key);
            }
            let owners = if empty {
                vec![source, observer]
            } else {
                vec![observer]
            };
            for owner in owners {
                if let Some(dependencies) = self.event_dependencies.get_mut(&owner) {
                    dependencies.remove(&key);
                    if dependencies.is_empty() {
                        self.event_dependencies.remove(&owner);
                    }
                }
            }
        }
    }

    fn index_event_handler(&mut self, key: (StableNodeId, TypeId), observer: StableNodeId) {
        self.event_dependencies
            .entry(key.0)
            .or_default()
            .insert(key);
        self.event_dependencies
            .entry(observer)
            .or_default()
            .insert(key);
    }

    pub fn on<V, E>(
        &mut self,
        entity: Entity<V>,
        mut handler: impl FnMut(&mut V, &E, &mut ViewContext<'_, V>) + Send + 'static,
    ) -> Result<(), FrameworkError>
    where
        V: View,
        E: Send + 'static,
    {
        self.read(entity, |_| ())?;
        let erased = move |view: &mut dyn Any,
                           event: &dyn Any,
                           mutations: &mut MutationQueue,
                           events: &mut VecDeque<BoxedEvent>,
                           program_messages: &mut Vec<ProgramMessage>,
                           now: Duration| {
            let view = view
                .downcast_mut::<V>()
                .expect("handler is registered for the entity view type");
            let event = event
                .downcast_ref::<E>()
                .expect("handler is indexed by event type");
            let mut cx = ViewContext {
                entity,
                mutations,
                events,
                program_messages,
                now,
                reassemble: false,
            };
            handler(view, event, &mut cx);
            cx.reassemble
        };
        self.index_event_handler((entity.id, TypeId::of::<E>()), entity.id);
        self.watch_clamp::<E>(entity.id);
        self.watch_size::<V, E>(entity.id);
        self.event_handlers
            .entry((entity.id, TypeId::of::<E>()))
            .or_default()
            .push(EventHandler {
                key: None,
                observer: entity.id,
                callback: Box::new(erased),
            });
        Ok(())
    }

    /// A handler for [`TextClamped`] makes its text announce its clamp.
    fn watch_clamp<E: 'static>(&mut self, text: StableNodeId) {
        if TypeId::of::<E>() == TypeId::of::<TextClamped>() {
            self.clamp_watchers.entry(text).or_insert(None);
        }
    }

    /// A handler for [`SizeChanged`] makes its node announce its size.
    fn watch_size<V: View, E: 'static>(&mut self, node: StableNodeId) {
        if TypeId::of::<E>() == TypeId::of::<SizeChanged>() {
            self.size_watchers.entry(node).or_insert(SizeWatch {
                sent: None,
                emit: emit_size::<V>,
            });
        }
    }

    /// Send [`SizeChanged`] to each listening node of `document` whose box
    /// the layout pass that just committed gave a new size.
    pub(super) fn announce_size_changes(
        &mut self,
        document: DocumentId,
    ) -> Result<(), FrameworkError> {
        if self.size_watchers.is_empty() {
            return Ok(());
        }
        let changed = self
            .size_watchers
            .iter()
            .filter_map(|(&id, watch)| {
                if self.world.document_of(id) != Some(document) || !self.world.is_mounted(id) {
                    return None;
                }
                let bounds = self.world.layout_box(id)?;
                let size = (bounds.width, bounds.height);
                (watch.sent != Some(size)).then_some((id, size, watch.emit))
            })
            .collect::<Vec<_>>();
        for (id, (width, height), emit) in changed {
            if let Some(watch) = self.size_watchers.get_mut(&id) {
                watch.sent = Some((width, height));
            }
            emit(self, id, SizeChanged { width, height })?;
        }
        Ok(())
    }

    /// Send [`TextClamped`] to each listening text whose clamp changed with
    /// the shaping that just ran.
    pub(super) fn announce_text_clamps(&mut self) -> Result<(), FrameworkError> {
        if self.clamp_watchers.is_empty() {
            return Ok(());
        }
        let changed = self
            .clamp_watchers
            .iter()
            .filter_map(|(id, sent)| {
                let clamped = self.world.text_layout(*id).map(|(_, layout)| {
                    layout
                        .overflow
                        .contains(nana_text::OverflowFlags::TRUNCATED_LINES)
                })?;
                (*sent != Some(clamped)).then_some((*id, clamped))
            })
            .collect::<Vec<_>>();
        for (id, clamped) in changed {
            self.clamp_watchers.insert(id, Some(clamped));
            self.update(Entity::<crate::Text>::from_stable_id(id), |_, cx| {
                cx.emit(TextClamped { clamped });
            })?;
        }
        Ok(())
    }

    /// Replace a named binding while leaving ordinary `on` subscriptions intact.
    /// Bindings are removed with their entity, just like ordinary handlers.
    pub fn on_keyed<V, E>(
        &mut self,
        entity: Entity<V>,
        key: impl Into<String>,
        handler: impl FnMut(&mut V, &E, &mut ViewContext<'_, V>) + Send + 'static,
    ) -> Result<(), FrameworkError>
    where
        V: View,
        E: Send + 'static,
    {
        let key = key.into();
        if key.is_empty() {
            return Err(FrameworkError::InvalidInput);
        }
        self.on(entity, handler)?;
        let handlers = self
            .event_handlers
            .get_mut(&(entity.id, TypeId::of::<E>()))
            .expect("on inserted the handler");
        let mut replacement = handlers.pop().expect("on appended the handler");
        replacement.key = Some(key.clone());
        if let Some(existing) = handlers
            .iter_mut()
            .find(|entry| entry.key.as_ref() == Some(&key))
        {
            *existing = replacement;
        } else {
            handlers.push(replacement);
        }
        Ok(())
    }

    pub fn observe<S, V, E>(
        &mut self,
        source: Entity<S>,
        observer: Entity<V>,
        mut handler: impl FnMut(&mut V, &E, &mut ViewContext<'_, V>) + Send + 'static,
    ) -> Result<(), FrameworkError>
    where
        S: View,
        V: View,
        E: Send + 'static,
    {
        self.read(source, |_| ())?;
        self.read(observer, |_| ())?;
        let erased = move |view: &mut dyn Any,
                           event: &dyn Any,
                           mutations: &mut MutationQueue,
                           events: &mut VecDeque<BoxedEvent>,
                           program_messages: &mut Vec<ProgramMessage>,
                           now: Duration| {
            let view = view
                .downcast_mut::<V>()
                .expect("observer handler is registered for its view type");
            let event = event
                .downcast_ref::<E>()
                .expect("observer handler is indexed by event type");
            let mut cx = ViewContext {
                entity: observer,
                mutations,
                events,
                program_messages,
                now,
                reassemble: false,
            };
            handler(view, event, &mut cx);
            cx.reassemble
        };
        self.index_event_handler((source.id, TypeId::of::<E>()), observer.id);
        self.watch_clamp::<E>(source.id);
        self.watch_size::<S, E>(source.id);
        self.event_handlers
            .entry((source.id, TypeId::of::<E>()))
            .or_default()
            .push(EventHandler {
                key: None,
                observer: observer.id,
                callback: Box::new(erased),
            });
        Ok(())
    }

    pub fn register_action(
        &mut self,
        id: impl Into<ActionId>,
        when: ContextPredicate,
        handler: impl FnMut(&mut AppContext) -> Result<(), FrameworkError> + Send + 'static,
    ) -> Result<(), FrameworkError> {
        insert_action(&mut self.actions, id, when, handler)
    }

    pub fn dispatch_action(
        &mut self,
        id: &ActionId,
        context: &KeyContext,
    ) -> Result<(), FrameworkError> {
        let mut action = self
            .actions
            .remove(id)
            .ok_or_else(|| FrameworkError::MissingAction(id.clone()))?;
        if !action.when.matches(context) {
            self.actions.insert(id.clone(), action);
            return Err(FrameworkError::ActionUnavailable(id.clone()));
        }
        let result = (action.handler)(self);
        self.actions.insert(id.clone(), action);
        result
    }

    /// Runs the handlers for queued events. Returns every other view a
    /// handler ran on, and whether it asked, through
    /// [`ViewContext::reassemble`], for its assembler to run once the caller
    /// commits.
    ///
    /// Observer handlers change those views in place, so nothing has
    /// projected their new state yet; the caller does that after it commits.
    pub(super) fn deliver_events(
        &mut self,
        id: StableNodeId,
        view: &mut dyn Any,
        mutations: &mut MutationQueue,
        events: &mut VecDeque<BoxedEvent>,
        program_messages: &mut Vec<ProgramMessage>,
    ) -> Result<Vec<super::TouchedObserver>, FrameworkError> {
        let mut touched: Vec<super::TouchedObserver> = Vec::new();
        let mut delivered = 0;
        while let Some((emitter, event_type, event)) = events.pop_front() {
            delivered += 1;
            if delivered > MAX_EVENTS_PER_UPDATE {
                return Err(FrameworkError::EventOverflow(emitter));
            }
            let key = (emitter, event_type);
            let Some(mut handlers) = self.event_handlers.remove(&key) else {
                continue;
            };
            for handler in &mut handlers {
                if handler.observer == id {
                    (handler.callback)(
                        view,
                        event.as_ref(),
                        mutations,
                        events,
                        program_messages,
                        self.component_lifecycle.now,
                    );
                    continue;
                }
                let Some(mut observer) = self.views.remove(&handler.observer) else {
                    continue;
                };
                let asked = (handler.callback)(
                    observer.as_mut(),
                    event.as_ref(),
                    mutations,
                    events,
                    program_messages,
                    self.component_lifecycle.now,
                );
                let observer_type = (*observer).type_id();
                match touched.iter_mut().find(|seen| seen.id == handler.observer) {
                    Some(seen) => seen.reassemble |= asked,
                    None => touched.push(super::TouchedObserver {
                        id: handler.observer,
                        type_id: observer_type,
                        reassemble: asked,
                    }),
                }
                self.views.insert(handler.observer, observer);
            }
            self.event_handlers.insert(key, handlers);
        }
        Ok(touched)
    }
}

#[cfg(test)]
mod dependency_tests {
    use super::*;
    use crate::Text;

    #[test]
    fn removing_observer_visits_only_subscribed_buckets_and_keeps_other_listeners() {
        let mut cx = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let source = cx.create_component(document, Text::new("source")).unwrap();
        let first = cx.create_component(document, Text::new("first")).unwrap();
        let second = cx.create_component(document, Text::new("second")).unwrap();
        for _ in 0..1000 {
            let unrelated = cx
                .create_component(document, Text::new("unrelated"))
                .unwrap();
            cx.on::<_, Activate>(unrelated, |_, _, _| {}).unwrap();
        }
        cx.observe::<_, _, Activate>(source, first, |_, _, _| {})
            .unwrap();
        cx.observe::<_, _, Activate>(source, second, |_, _, _| {})
            .unwrap();
        cx.on_keyed::<_, Activate>(source, "binding", |_, _, _| {})
            .unwrap();
        cx.on_keyed::<_, Activate>(source, "binding", |_, _, _| {})
            .unwrap();
        assert_eq!(cx.remove_event_handlers_for(&HashSet::from([first.id])), 1);
        assert!(!cx.event_dependencies.contains_key(&first.id));
        let key = (source.id, TypeId::of::<Activate>());
        assert_eq!(cx.event_handlers[&key].len(), 2);
        assert!(cx.event_dependencies[&second.id].contains(&key));
        assert_eq!(cx.remove_event_handlers_for(&HashSet::from([source.id])), 1);
        assert!(!cx.event_dependencies.contains_key(&source.id));
        assert!(!cx.event_dependencies.contains_key(&second.id));
        assert_eq!(cx.event_handlers.len(), 1000);
        assert_eq!(cx.event_dependencies.len(), 1000);
        assert_eq!(cx.remove_event_handlers_for(&HashSet::new()), 0);
    }
}

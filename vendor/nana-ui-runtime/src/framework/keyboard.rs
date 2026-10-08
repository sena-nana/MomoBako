use super::*;

pub(super) type KeyHandler = Box<dyn FnMut(&dyn Any, &crate::KeyInput) -> bool + Send>;

impl AppContext {
    /// Set the focused control's keyboard policy before default editing.
    /// Returning true consumes the key. A new policy replaces the old one.
    pub fn on_key<V: View>(
        &mut self,
        entity: Entity<V>,
        mut handler: impl FnMut(&crate::KeyInput) -> bool + Send + 'static,
    ) -> Result<(), FrameworkError> {
        self.on_view_key(entity, move |_, key| handler(key))
    }

    /// Set a keyboard policy that reads the control's current retained value.
    pub fn on_view_key<V: View>(
        &mut self,
        entity: Entity<V>,
        mut handler: impl FnMut(&V, &crate::KeyInput) -> bool + Send + 'static,
    ) -> Result<(), FrameworkError> {
        self.read(entity, |_| ())?;
        self.key_handlers.insert(
            entity.id,
            Box::new(move |view, key| {
                view.downcast_ref::<V>()
                    .is_some_and(|view| handler(view, key))
            }),
        );
        Ok(())
    }

    /// Dispatch to the eligible focused control. IME composition retains its
    /// keys and never invokes an application submit or clipboard policy.
    pub fn dispatch_focused_key(&mut self, document: DocumentId, key: &crate::KeyInput) -> bool {
        let Some(target) = self.world.focused(document) else {
            return false;
        };
        if !self.world.is_mounted(target)
            || self.world.ime(target).is_some()
            || self
                .world
                .accessibility(target)
                .is_some_and(|state| state.disabled)
        {
            return false;
        }
        let Some(view) = self.views.get(&target) else {
            return false;
        };
        self.key_handlers
            .get_mut(&target)
            .is_some_and(|handler| handler(view.as_ref(), key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn focused_policy_reads_current_view_and_respects_ime_disabled_and_park() {
        let mut cx = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let area = cx
            .create_component(document, TextArea::new("first"))
            .unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        cx.on_view_key(area, move |view, key| {
            sink.lock().unwrap().push(view.state.value.clone());
            key.pressed && key.key.as_ref() == "Enter"
        })
        .unwrap();
        cx.focus_node(document, area.id).unwrap();
        let enter = crate::KeyInput::new(true, "Enter", false, false, false, false, false);
        assert!(cx.dispatch_focused_key(document, &enter));
        cx.update_component(area, |view, _| {
            view.state.replace_value("second");
        })
        .unwrap();
        assert!(cx.dispatch_focused_key(document, &enter));
        assert_eq!(*seen.lock().unwrap(), ["first", "second"]);
        cx.set_ime_preedit(document, "ni".into(), None).unwrap();
        assert!(!cx.dispatch_focused_key(document, &enter));
        cx.commit_ime(document, "你").unwrap();
        cx.update_component(area, |view, _| view.disabled = true)
            .unwrap();
        assert!(!cx.dispatch_focused_key(document, &enter));
        cx.update_component(area, |view, _| view.disabled = false)
            .unwrap();
        let mut queue = MutationQueue::new();
        queue.park_subtree(area.id);
        cx.commit_mutations(queue).unwrap();
        assert!(!cx.dispatch_focused_key(document, &enter));
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn registration_replaces_policy_and_removal_forgets_it() {
        let mut cx = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let area = cx.create_component(document, TextArea::new("")).unwrap();
        cx.focus_node(document, area.id).unwrap();
        cx.on_key(area, |_| panic!("replaced callback must not run"))
            .unwrap();
        cx.on_key(area, |_| true).unwrap();
        assert!(cx.dispatch_focused_key(
            document,
            &crate::KeyInput::new(true, "Enter", false, false, false, false, false)
        ));
        cx.remove_view(area).unwrap();
        assert!(!cx.key_handlers.contains_key(&area.id));
    }
}

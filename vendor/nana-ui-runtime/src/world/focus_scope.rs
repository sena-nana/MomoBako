use super::mutation::ValidationPlan;
use super::*;

impl UiWorld {
    /// Remember the most recently focused descendant of this retained view.
    /// Parking preserves its memory; despawning the scope or target clears it.
    pub fn register_focus_scope(&mut self, root: StableNodeId) -> Result<(), UiWorldError> {
        let document = self
            .node(root)
            .ok_or(UiWorldError::MissingNode(root))?
            .document;
        self.input.focus_scopes.entry(root).or_insert(None);
        if let Some(target) = self.focused(document) {
            let mut ancestor = Some(target);
            while let Some(node) = ancestor {
                if node == root {
                    self.input.focus_scopes.insert(root, Some(target));
                    break;
                }
                ancestor = self.parent_id(node);
            }
        }
        Ok(())
    }

    pub fn unregister_focus_scope(&mut self, root: StableNodeId) {
        self.input.focus_scopes.remove(&root);
    }

    pub(super) fn remember_scope_focus(&mut self, target: StableNodeId) {
        if self.input.focus_scopes.is_empty() {
            return;
        }
        let mut ancestor = Some(target);
        while let Some(node) = ancestor {
            if let Some(remembered) = self.input.focus_scopes.get_mut(&node) {
                *remembered = Some(target);
            }
            ancestor = self.parent_id(node);
        }
    }
}

impl ValidationPlan<'_> {
    pub(super) fn remember_scope_focus(
        &mut self,
        target: StableNodeId,
    ) -> Result<(), UiWorldError> {
        if self.source.input.focus_scopes.is_empty() {
            return Ok(());
        }
        let mut ancestor = Some(target);
        while let Some(node) = ancestor {
            if self.source.input.focus_scopes.contains_key(&node) {
                self.scope_focus.insert(node, target);
            }
            ancestor = self.node(node)?.parent;
        }
        Ok(())
    }

    pub(super) fn restorable_scope_focus(
        &mut self,
        root: StableNodeId,
    ) -> Result<Option<(DocumentId, StableNodeId)>, UiWorldError> {
        if !self.exists(root) || !self.source.input.focus_scopes.contains_key(&root) {
            return Ok(None);
        }
        let Some(target) = self
            .scope_focus
            .get(&root)
            .copied()
            .or_else(|| self.source.input.focus_scopes.get(&root).copied().flatten())
        else {
            return Ok(None);
        };
        if !self.exists(target) {
            return Ok(None);
        }
        let document = self.node(root)?.document;
        if self.node(target)?.document != document || !self.has_ancestor(target, root)? {
            return Ok(None);
        }
        let interaction = self
            .interactions
            .get(&target)
            .copied()
            .or_else(|| self.source.interaction(target))
            .unwrap_or_default();
        if !interaction.focusable
            || !self.focus_target_visible(target)?
            || !self.active_modal_allows_focus(document, target)?
        {
            return Ok(None);
        }
        let mut ancestor = Some(target);
        while let Some(node) = ancestor {
            if self
                .accessibility
                .get(&node)
                .or_else(|| self.source.accessibility(node))
                .is_some_and(|state| state.disabled)
            {
                return Ok(None);
            }
            ancestor = self.node(node)?.parent;
        }
        Ok(Some((document, target)))
    }
}

#[cfg(test)]
mod tests {
    use crate::{AppContext, Button, DocumentId, MutationQueue, Stack, TextArea};

    fn focus(context: &mut AppContext, document: DocumentId, target: crate::StableNodeId) {
        assert!(context.focus_node(document, target).unwrap());
    }

    #[test]
    fn scopes_restore_the_last_descendant_after_parking_and_button_focus() {
        let mut context = AppContext::new();
        let document = DocumentId::new(903).unwrap();
        let host = context
            .create_component(document, Stack::fill_column(0.0))
            .unwrap();
        let scopes = [
            context
                .create_detached_component(document, Stack::fill_column(0.0))
                .unwrap(),
            context
                .create_detached_component(document, Stack::fill_column(0.0))
                .unwrap(),
        ];
        let a = context
            .create_detached_component(document, TextArea::new("a"))
            .unwrap();
        let b = context
            .create_detached_component(document, TextArea::new("b"))
            .unwrap();
        let switch = context
            .create_detached_component(document, Button::new("switch"))
            .unwrap();
        context.append_child(host, switch).unwrap();
        for (scope, editor) in scopes.into_iter().zip([a, b]) {
            context.append_child(host, scope).unwrap();
            context.append_child(scope, editor).unwrap();
            context
                .compat_world_mut()
                .register_focus_scope(scope.stable_id())
                .unwrap();
        }
        focus(&mut context, document, a.stable_id());
        focus(&mut context, document, b.stable_id());
        focus(&mut context, document, switch.stable_id());
        let mut changes = MutationQueue::new();
        changes.park_subtree(scopes[1].stable_id());
        changes.restore_focus_within(scopes[0].stable_id());
        context.commit_mutations(changes).unwrap();
        assert_eq!(context.world().focused(document), Some(a.stable_id()));
        let mut changes = MutationQueue::new();
        changes.park_subtree(scopes[0].stable_id());
        changes.insert(host.stable_id(), scopes[1].stable_id(), None);
        changes.restore_focus_within(scopes[1].stable_id());
        context.commit_mutations(changes).unwrap();
        assert_eq!(context.world().focused(document), Some(b.stable_id()));
    }

    #[test]
    fn scopes_ignore_disabled_hidden_reparented_and_deleted_targets() {
        let mut context = AppContext::new();
        let document = DocumentId::new(904).unwrap();
        let scope = context
            .create_component(document, Stack::fill_column(0.0))
            .unwrap();
        let other = context
            .create_component(document, Stack::fill_column(0.0))
            .unwrap();
        let editor = context
            .create_detached_component(document, TextArea::new("draft"))
            .unwrap();
        let switch = context
            .create_detached_component(document, Button::new("switch"))
            .unwrap();
        context.append_child(scope, editor).unwrap();
        context.append_child(other, switch).unwrap();
        context
            .compat_world_mut()
            .register_focus_scope(scope.stable_id())
            .unwrap();
        focus(&mut context, document, editor.stable_id());
        focus(&mut context, document, switch.stable_id());
        context
            .update_component(editor, |editor, _| editor.disabled = true)
            .unwrap();
        let restore = |context: &mut AppContext| {
            let mut changes = MutationQueue::new();
            changes.restore_focus_within(scope.stable_id());
            context.commit_mutations(changes).unwrap();
            assert_eq!(context.world().focused(document), Some(switch.stable_id()));
        };
        restore(&mut context);
        context
            .update_component(editor, |editor, _| editor.disabled = false)
            .unwrap();
        let mut changes = MutationQueue::new();
        changes.park_subtree(scope.stable_id());
        context.commit_mutations(changes).unwrap();
        restore(&mut context);
        context.append_child(other, editor).unwrap();
        restore(&mut context);
        context.remove_view(editor).unwrap();
        restore(&mut context);
        context.remove_view(scope).unwrap();
        restore(&mut context);
    }
}

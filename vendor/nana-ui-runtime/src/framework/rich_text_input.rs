use super::*;
use crate::{NativeMarkdown, RichTextEvent, SelectableRichText};

impl AppContext {
    pub fn begin_rich_text_pointer(
        &mut self,
        document: DocumentId,
        pointer: u64,
        target: StableNodeId,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        if self.world.document_of(target) != Some(document)
            || !self.world.is_mounted(target)
            || self
                .world
                .accessibility(target)
                .is_some_and(|state| state.disabled)
        {
            return Ok(false);
        }
        self.end_rich_text_pointer(document, pointer, x, y, true)?;
        let handled = self.rich_text_pointer(target, x, y, RichPointerPhase::Down)?;
        if handled {
            let mut queue = MutationQueue::new();
            queue.capture_pointer(pointer, target);
            self.commit_mutations(queue)?;
            self.component_lifecycle
                .rich_text_presses
                .insert((document, pointer), target);
        }
        Ok(handled)
    }

    pub fn update_rich_text_pointer(
        &mut self,
        document: DocumentId,
        pointer: u64,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self
            .component_lifecycle
            .rich_text_presses
            .get(&(document, pointer))
            .copied()
        else {
            return Ok(false);
        };
        if self.world.pointer_capture(document, pointer) != Some(target)
            || self
                .world
                .accessibility(target)
                .is_some_and(|state| state.disabled)
        {
            return self.end_rich_text_pointer(document, pointer, x, y, true);
        }
        self.rich_text_pointer(target, x, y, RichPointerPhase::Move)
    }

    pub fn end_rich_text_pointer(
        &mut self,
        document: DocumentId,
        pointer: u64,
        x: f32,
        y: f32,
        cancelled: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self
            .component_lifecycle
            .rich_text_presses
            .remove(&(document, pointer))
        else {
            return Ok(false);
        };
        let cancelled = cancelled
            || self.world.pointer_capture(document, pointer) != Some(target)
            || !self.world.is_mounted(target)
            || !x.is_finite()
            || !y.is_finite()
            || self
                .world
                .accessibility(target)
                .is_some_and(|state| state.disabled);
        if self.world.node(target).is_none() {
            return Ok(false);
        }
        if self.world.pointer_capture(document, pointer) == Some(target) {
            let mut queue = MutationQueue::new();
            queue.release_pointer(pointer, target);
            self.commit_mutations(queue)?;
        }
        self.rich_text_pointer(
            target,
            x,
            y,
            if cancelled {
                RichPointerPhase::Cancel
            } else {
                RichPointerPhase::Up
            },
        )
    }

    fn rich_text_pointer(
        &mut self,
        target: StableNodeId,
        x: f32,
        y: f32,
        phase: RichPointerPhase,
    ) -> Result<bool, FrameworkError> {
        if matches!(phase, RichPointerPhase::Cancel) {
            if let Some(entity) = self.view_entity::<NativeMarkdown>(target) {
                self.update_component(entity, |view, _| view.clear_selection())?;
                return Ok(true);
            }
            if let Some(entity) = self.view_entity::<SelectableRichText>(target) {
                self.update_component(entity, |view, _| view.clear_selection())?;
                return Ok(true);
            }
            return Ok(false);
        }
        let Some(bounds) = self.world.component_content_box(target) else {
            return Ok(false);
        };
        let Some((x, y)) = self.world.pointer_layout_position(target, x, y) else {
            return Ok(false);
        };
        if let Some(entity) = self.view_entity::<NativeMarkdown>(target) {
            let geometry = self.read(entity, |view| {
                self.world.markdown_layout(target, view.blocks(), bounds)
            })?;
            self.update_component(entity, |view, cx| match phase {
                RichPointerPhase::Down => {
                    view.pointer_down_with_geometry(x, y, &geometry);
                }
                RichPointerPhase::Move => {
                    if view.pointer_move_with_geometry(x, y, &geometry) {
                        cx.emit(RichTextEvent::SelectionChanged(view.selection_snapshot()));
                    }
                }
                RichPointerPhase::Up => {
                    if let Some(event) = view.pointer_up_with_geometry(x, y, &geometry) {
                        cx.emit(event);
                    }
                }
                RichPointerPhase::Cancel => {
                    view.clear_selection();
                }
            })?;
            return Ok(true);
        }
        if let Some(entity) = self.view_entity::<SelectableRichText>(target) {
            let geometry = self.read(entity, |view| {
                self.world.selectable_rich_text_layout(target, view, bounds)
            })?;
            self.update_component(entity, |view, cx| match phase {
                RichPointerPhase::Down => {
                    view.pointer_down_with_geometry(x, y, &geometry);
                }
                RichPointerPhase::Move => {
                    if view.pointer_move_with_geometry(x, y, &geometry) {
                        cx.emit(RichTextEvent::SelectionChanged(view.selection_snapshot()));
                    }
                }
                RichPointerPhase::Up => {
                    if let Some(event) = view.pointer_up_with_geometry(x, y, &geometry) {
                        cx.emit(event);
                    }
                }
                RichPointerPhase::Cancel => {
                    view.clear_selection();
                }
            })?;
            return Ok(true);
        }
        Ok(false)
    }
}

#[derive(Clone, Copy)]
enum RichPointerPhase {
    Down,
    Move,
    Up,
    Cancel,
}

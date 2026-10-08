use super::*;
use crate::view_components::TextAreaResizeDrag;

impl AppContext {
    /// Starts a height-only drag on the editor's projected resize grip.
    pub fn begin_text_area_resize(
        &mut self,
        pointer: u64,
        target: StableNodeId,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(entity) = self.view_entity::<TextArea>(target) else {
            return Ok(false);
        };
        let Some(document) = self.world.document_of(target) else {
            return Ok(false);
        };
        let mut restored_height = None;
        if let Some(drag) = self.read(entity, |area| area.resize_drag)?
            && (drag.pointer == pointer
                || self.world.pointer_capture(document, drag.pointer) != Some(target))
        {
            restored_height = Some(drag.start_height);
            self.component_lifecycle
                .text_area_resizes
                .retain(|_, value| *value != target);
            let owns_capture = self.world.pointer_capture(document, drag.pointer) == Some(target);
            self.update_component(entity, |area, cx| {
                if owns_capture {
                    cx.mutations().release_pointer(drag.pointer, target);
                }
                area.resized_height = drag.previous_height;
                area.resize_drag = None;
            })?;
        }
        if !self.read(entity, |area| {
            area.supports_vertical_resize() && !area.disabled && area.resize_drag.is_none()
        })? {
            return Ok(false);
        }
        let Some((x, y)) = self.world.pointer_layout_position(target, x, y) else {
            return Ok(false);
        };
        let Some(crate::ComponentGeometry::TextInput {
            resize_grip: Some(grip),
            ..
        }) = self.world.component_geometry(target)
        else {
            return Ok(false);
        };
        if !grip.contains(x, y) {
            return Ok(false);
        }
        let Some(bounds) = self.world.layout_box(target) else {
            return Ok(false);
        };
        let start_height = self.read(entity, |area| {
            if area.style.layout.box_sizing == nana_ui_core::BoxSizing::ContentBox {
                area.resized_height
                    .or_else(|| match area.style.layout.height {
                        Some(nana_ui_core::LengthSpec::Px(height)) => Some(height),
                        _ => None,
                    })
                    .unwrap_or(bounds.height)
            } else {
                bounds.height
            }
        })?;
        self.update_component(entity, |area, cx| {
            area.resize_drag = Some(TextAreaResizeDrag {
                pointer,
                start_y: y,
                start_height: restored_height.unwrap_or(start_height),
                previous_height: area.resized_height,
            });
            cx.mutations().capture_pointer(pointer, target);
        })?;
        self.component_lifecycle
            .text_area_resizes
            .insert((document, pointer), target);
        Ok(true)
    }

    pub fn update_text_area_resize(
        &mut self,
        document: DocumentId,
        pointer: u64,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self
            .component_lifecycle
            .text_area_resizes
            .get(&(document, pointer))
            .copied()
        else {
            return Ok(false);
        };
        let Some(entity) = self.view_entity::<TextArea>(target) else {
            return Ok(false);
        };
        let Some(drag) = self.read(entity, |area| area.resize_drag)? else {
            return Ok(false);
        };
        if drag.pointer != pointer {
            return Ok(false);
        }
        if self.world.pointer_capture(document, pointer) != Some(target)
            || self.read(entity, |area| {
                area.disabled || !area.supports_vertical_resize()
            })?
        {
            return self.end_text_area_resize(document, pointer, true);
        }
        let Some((_, y)) = self.world.pointer_layout_position(target, x, y) else {
            return Ok(false);
        };
        self.update_component(entity, |area, _| {
            area.resized_height = Some((drag.start_height + y - drag.start_y).max(0.0));
        })?;
        Ok(true)
    }

    pub fn end_text_area_resize(
        &mut self,
        document: DocumentId,
        pointer: u64,
        cancel: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self
            .component_lifecycle
            .text_area_resizes
            .remove(&(document, pointer))
        else {
            return Ok(false);
        };
        let Some(entity) = self.view_entity::<TextArea>(target) else {
            return Ok(false);
        };
        let Some(drag) = self.read(entity, |area| area.resize_drag)? else {
            return Ok(false);
        };
        if drag.pointer != pointer {
            return Ok(false);
        }
        let owns_capture = self.world.pointer_capture(document, pointer) == Some(target);
        let cancel = cancel || !owns_capture;
        self.update_component(entity, |area, cx| {
            if cancel || area.disabled || !area.supports_vertical_resize() {
                area.resized_height = drag.previous_height;
            }
            area.resize_drag = None;
            if owns_capture {
                cx.mutations().release_pointer(pointer, target);
            }
        })?;
        Ok(true)
    }
}

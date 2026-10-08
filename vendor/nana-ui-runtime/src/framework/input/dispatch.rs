//! What one validated event does to the retained world. The router resolves
//! the source, the pointer identity and the time; these handlers apply the
//! event: pointer and wheel through one shared hit, keys and committed text
//! through one key chain, composition into the focused editor.

use crate::framework::hooked;
use std::time::Duration;

use nana_ui_core::TableNavigation;
use nana_ui_input::{
    CompositionInput, HostServices, InputDisposition, InputModifiers, PointerInput, PointerPhase,
    WheelInput, WheelUnit,
};

use crate::{
    AppContext, DocumentId, FrameworkError, OverlayKey, OverlayPointerDecision,
    OverlayPointerPhase, RangeAdjustment, RovingFocusIntent, ScrollOffset, StableNodeId,
    TextCaretIntent, TextDeleteKind, TextLineDirection, TextShaper, XYPadAdjustment,
};
#[cfg(feature = "graph-canvas")]
use crate::{GraphCanvasAdjustment, GraphPointerButton, GraphScrollDelta};

macro_rules! optional_input {
    // A fallible dispatch into an optional component: absent, it handled
    // nothing.
    ($feature:literal, $call:expr) => {
        optional_input!($feature, $call, Ok::<bool, FrameworkError>(false))
    };
    ($feature:literal, $call:expr, $absent:expr) => {{
        #[cfg(feature = $feature)]
        {
            $call
        }
        #[cfg(not(feature = $feature))]
        {
            $absent
        }
    }};
}

/// Logical pixels one wheel line scrolls.
const LINE_SCROLL_EXTENT: f32 = 60.0;
/// Rows PageUp and PageDown move a focused table by.
const TABLE_PAGE_ROWS: usize = 10;

const CONSUMED: InputDisposition = InputDisposition {
    handled: true,
    prevent_default: true,
};

/// A key press or release, or committed text, as the key chain reads it.
/// Committed text is a press with an empty `key`, so it reaches every editor
/// the way the key that typed it would, without being mistaken for one.
#[derive(Debug, Clone, Copy)]
pub(super) struct KeyStroke<'a> {
    pub(super) pressed: bool,
    pub(super) key: &'a str,
    pub(super) text: Option<&'a str>,
    pub(super) repeat: bool,
    pub(super) modifiers: InputModifiers,
}

impl<'a> KeyStroke<'a> {
    pub(super) fn text(text: &'a str) -> Self {
        Self {
            pressed: true,
            key: "",
            text: Some(text),
            repeat: false,
            modifiers: InputModifiers::default(),
        }
    }

    fn is_text(&self) -> bool {
        self.key.is_empty()
    }
}

impl AppContext {
    /// One pointer sample, its id already the source-local one. `landed`
    /// receives where it landed: the capture owner, else the topmost
    /// reachable node. It is resolved once and everything the event does
    /// reads it, so an uncaptured sample costs one hit query and a captured
    /// one none.
    pub(super) fn dispatch_pointer(
        &mut self,
        document: DocumentId,
        pointer: &PointerInput,
        now: Duration,
        mut text_shaper: Option<&mut dyn TextShaper>,
        landed: &mut Option<StableNodeId>,
    ) -> Result<InputDisposition, FrameworkError> {
        // Before anything routes, so the focus this press causes is recorded
        // against the device that caused it.
        self.world
            .note_input_modality(document, crate::InputModality::Pointer);
        let PointerInput {
            phase,
            x,
            y,
            button,
            is_primary,
            activation_click,
            modifiers,
            ..
        } = pointer;
        let pointer_id = &pointer.pointer_id.0;
        let overlay_phase = match phase {
            PointerPhase::Move => OverlayPointerPhase::Move,
            PointerPhase::Down if *is_primary && *button == 0 => OverlayPointerPhase::PrimaryDown,
            PointerPhase::Up if *is_primary && *button == 0 => OverlayPointerPhase::PrimaryUp,
            PointerPhase::Cancel => OverlayPointerPhase::Cancel,
            PointerPhase::Down | PointerPhase::Up => OverlayPointerPhase::Move,
        };
        let overlay = if matches!(
            phase,
            PointerPhase::Move | PointerPhase::Up | PointerPhase::Cancel
        ) {
            self.world()
                .pointer_capture(document, *pointer_id)
                .map_or_else(
                    || self.route_overlay_pointer(document, *pointer_id, overlay_phase, *x, *y),
                    |target| {
                        Ok(OverlayPointerDecision {
                            target: Some(target),
                            prevent_default: false,
                            dismissed: false,
                        })
                    },
                )?
        } else {
            self.route_overlay_pointer(document, *pointer_id, overlay_phase, *x, *y)?
        };
        let target = overlay.target;
        *landed = target;
        self.set_pointer_location(document, *pointer_id, Some((*x, *y)));
        self.set_pointer_hover_at(document, *pointer_id, target, now)?;
        self.update_text_diagnostic_hover_hit(document, *x, *y, target)?;
        let terminal_phase = match phase {
            PointerPhase::Down if *is_primary && *button == 0 => Some(0),
            PointerPhase::Move => Some(1),
            PointerPhase::Up if *is_primary && *button == 0 => Some(2),
            PointerPhase::Cancel => Some(3),
            _ => None,
        };
        if !overlay.prevent_default
            && let Some(phase) = terminal_phase
            && self.terminal_pointer(document, target, *pointer_id, phase, *x, *y)?
        {
            return Ok(CONSUMED);
        }

        #[cfg(feature = "graph-canvas")]
        let graph_button = match *button {
            1 => GraphPointerButton::Middle,
            _ => GraphPointerButton::Primary,
        };
        let component_handled = match phase {
            PointerPhase::Move => {
                if self.update_text_area_resize(document, *pointer_id, *x, *y)? {
                    return Ok(CONSUMED);
                }
                if optional_input!(
                    "rich-text",
                    self.update_rich_text_pointer(document, *pointer_id, *x, *y)
                )? {
                    return Ok(CONSUMED);
                }
                #[cfg(feature = "image-viewer")]
                if let Some(viewer) = self
                    .world()
                    .pointer_capture(document, *pointer_id)
                    .and_then(|target| self.view_entity::<crate::ImageViewer>(target))
                    && self.image_viewer_pointer_move(viewer, *pointer_id, *x, *y)?
                {
                    return Ok(CONSUMED);
                }
                if let Some(shaper) = reborrow_text_shaper(&mut text_shaper)
                    && self.text_editor_pointer_drag(document, *pointer_id, *x, *y, shaper)?
                {
                    true
                } else if let Some(shaper) = reborrow_text_shaper(&mut text_shaper)
                    && self.document_text_pointer_drag(document, *pointer_id, *x, *y, shaper)?
                {
                    true
                } else {
                    self.update_scrollbar_drag(document, *pointer_id, *x, *y)?
                        || self.update_range_drag(document, *pointer_id, *x)?
                        || self.update_xy_pad_drag(
                            document,
                            *pointer_id,
                            *x,
                            *y,
                            modifiers.shift,
                        )?
                        || optional_input!(
                            "graph-canvas",
                            self.update_graph_canvas_pointer(document, *pointer_id, *x, *y,)
                        )?
                        || optional_input!(
                            "graph-canvas",
                            self.update_graph_minimap_pointer(document, *pointer_id, *x, *y,)
                        )?
                        || optional_input!(
                            "controls",
                            self.update_reorder_list_pointer(document, *pointer_id, *x, *y,)
                        )?
                        || self.update_split_resize(document, *pointer_id, *x, *y)?
                        || hooked!(self, dock, |h| (h.update_split)(
                            self,
                            document,
                            *pointer_id,
                            *x,
                            *y
                        ))?
                        || hooked!(self, workspace, |h| (h.update_resize)(
                            self,
                            document,
                            *pointer_id,
                            *x,
                            *y,
                            now
                        ))?
                        || hooked!(self, dock, |h| (h.update_item)(
                            self,
                            document,
                            *pointer_id,
                            *x,
                            *y
                        ))?
                        || target
                            .map(|_target| {
                                optional_input!(
                                    "graph-canvas",
                                    self.hover_graph_canvas(_target, *x, *y)
                                )
                            })
                            .transpose()?
                            .unwrap_or(false)
                        || target
                            .map(|_target| {
                                optional_input!(
                                    "calendar",
                                    self.hover_calendar_heatmap(_target, *x, *y)
                                )
                            })
                            .transpose()?
                            .unwrap_or(false)
                        || optional_input!("calendar", self.clear_calendar_heatmap_hover(document))?
                        || self.sync_split_handle_hover_near_hit(document, *x, *y, target, now)?
                        || target.is_some()
                }
            }
            PointerPhase::Down if *button == 2 => {
                // A secondary press outside an open popover dismisses
                // it and goes no further, matching the primary press.
                if self.dismiss_popovers_outside(target)? {
                    return Ok(CONSUMED);
                }
                self.dismiss_detached_menus(target)?;
                self.secondary_press_at(document, *x, *y)?.is_some()
            }
            PointerPhase::Down if (*is_primary && *button == 0) || *button == 1 => {
                if self.dismiss_popovers_outside(target)? {
                    // Consume the press that dismissed the popover.
                    // Activation needs a press recorded here to match
                    // on release, so skipping it also stops this click
                    // from reaching the control underneath.
                    return Ok(CONSUMED);
                }
                self.dismiss_detached_menus(target)?;
                // Scrollbars overlay content, so they claim the press
                // before the node underneath sees it.
                if *button == 0
                    && !activation_click
                    && let Some(target) = target
                    && self.begin_text_area_resize(*pointer_id, target, *x, *y)?
                {
                    return Ok(CONSUMED);
                }
                if *button == 0
                    && let Some((view, axis)) = self.scrollbar_target_near_hit(*x, *y, target)
                    && self.begin_scrollbar_drag(*pointer_id, view, axis, *x, *y)?
                {
                    return Ok(CONSUMED);
                }
                // A press a blocking overlay swallowed reaches nothing
                // underneath, not even a handle within slop of it.
                let reachable = !(overlay.prevent_default && target.is_none());
                let [split_handle, dock_handle, workspace_handle] = if reachable {
                    self.reachable_handle_near(document, *x, *y, target)
                } else {
                    [None; 3]
                };
                let dock_source = target
                    .filter(|id| hooked!(self, dock, |h| (h.is_item_source)(self, *id)))
                    .or_else(|| {
                        reachable
                            .then(|| {
                                hooked!(self, dock, |h| (h.tab_strip_near_hit)(
                                    self, document, *x, *y, target
                                ))
                            })
                            .flatten()
                    });
                let hit = dock_handle.or(split_handle).or(workspace_handle).or(target);
                let focus_target = hit.and_then(|id| nearest_focusable(self, id));
                if !hit.is_some_and(|id| self.preserves_hover_card_editor_focus(id)) {
                    if let Some(focus) = focus_target {
                        self.focus_node(document, focus)?;
                    } else {
                        self.clear_focus(document)?;
                    }
                }
                if *button == 0
                    && !activation_click
                    && let Some(shaper) = reborrow_text_shaper(&mut text_shaper)
                {
                    let editor = if let Some(focus) = focus_target {
                        self.text_editor_pointer_press(
                            document,
                            focus,
                            *pointer_id,
                            *x,
                            *y,
                            modifiers.shift,
                            modifiers.alt,
                            now,
                            shaper,
                        )?
                    } else {
                        false
                    };
                    if editor {
                        self.clear_document_text_selection(document);
                    } else {
                        self.document_text_pointer_press(document, *pointer_id, *x, *y, shaper)?;
                    }
                }
                if let Some(target) = hit {
                    if *button == 0
                        && !activation_click
                        && optional_input!(
                            "rich-text",
                            self.begin_rich_text_pointer(document, *pointer_id, target, *x, *y)
                        )?
                    {
                        return Ok(CONSUMED);
                    }
                    #[cfg(feature = "image-viewer")]
                    if *button == 0
                        && !activation_click
                        && let Some(viewer) = self.view_entity::<crate::ImageViewer>(target)
                        && self
                            .image_viewer_pointer_down(viewer, *pointer_id, *x, *y)?
                            .is_some()
                    {
                        return Ok(CONSUMED);
                    }
                    if optional_input!("graph-canvas", self.is_graph_canvas(target), false) {
                        optional_input!(
                            "graph-canvas",
                            self.begin_graph_canvas_pointer(
                                document,
                                *pointer_id,
                                target,
                                *x,
                                *y,
                                graph_button,
                            )
                        )?;
                    } else if optional_input!("graph-canvas", self.is_graph_minimap(target), false)
                    {
                        optional_input!(
                            "graph-canvas",
                            self.begin_graph_minimap_pointer(*pointer_id, target, *x, *y)
                        )?;
                    } else if *button == 0
                        && optional_input!(
                            "controls",
                            self.begin_reorder_list_pointer(document, *pointer_id, target, *x, *y,)
                        )?
                    {
                    } else if hooked!(self, dock, |h| (h.is_handle)(self, target)) && *button == 0 {
                        hooked!(self, dock, |h| (h.begin_split)(
                            self,
                            document,
                            *pointer_id,
                            target,
                            *x,
                            *y
                        ))?;
                    } else if self.is_split_handle(target) && *button == 0 {
                        self.begin_split_resize(document, *pointer_id, target, *x, *y)?;
                    } else if hooked!(self, workspace, |h| (h.is_resize_handle)(self, target))
                        && *button == 0
                    {
                        hooked!(self, workspace, |h| (h.begin_resize)(
                            self,
                            document,
                            *pointer_id,
                            target,
                            *x,
                            *y,
                            now
                        ))?;
                    } else if *button == 0 {
                        if let Some(source) = dock_source {
                            hooked!(self, dock, |h| (h.begin_item)(
                                self,
                                document,
                                *pointer_id,
                                source,
                                *x,
                                *y
                            ))?;
                        } else {
                            self.press_pointer(document, *pointer_id, target)?;
                            if !*activation_click && self.press_number_stepper(target, *x, *y)? {
                                self.release_pointer(document, *pointer_id);
                            } else if self.is_range_field(target) {
                                self.begin_range_drag(document, *pointer_id, target, *x)?;
                            } else if self.is_xy_pad(target) {
                                self.begin_xy_pad_drag(document, *pointer_id, target, *x, *y)?;
                            }
                        }
                    }
                    true
                } else {
                    false
                }
            }
            PointerPhase::Up if (*is_primary && *button == 0) || *button == 1 => {
                if self.end_text_area_resize(document, *pointer_id, false)? {
                    return Ok(CONSUMED);
                }
                if optional_input!(
                    "rich-text",
                    self.end_rich_text_pointer(document, *pointer_id, *x, *y, false)
                )? {
                    return Ok(CONSUMED);
                }
                #[cfg(feature = "image-viewer")]
                if let Some(viewer) = self
                    .world()
                    .pointer_capture(document, *pointer_id)
                    .and_then(|target| self.view_entity::<crate::ImageViewer>(target))
                    && self.image_viewer_pointer_up(viewer, *pointer_id)?
                {
                    return Ok(CONSUMED);
                }
                // 拖拽移动选中的落点执行先于通用释放清理：active 态
                // 落文本、pending 态回落为点击。
                let mut drop_handled = false;
                if let Some(shaper) = reborrow_text_shaper(&mut text_shaper) {
                    drop_handled =
                        self.text_editor_selection_drop(document, *pointer_id, *x, *y, shaper)?;
                }
                self.text_editor_pointer_release(*pointer_id);
                self.document_text_pointer_release(*pointer_id);
                if drop_handled {
                    self.release_pointer(document, *pointer_id);
                    return Ok(CONSUMED);
                }
                if self.end_scrollbar_drag(document, *pointer_id, false)?
                    || self.end_range_drag(document, *pointer_id, false)?
                    || self.end_xy_pad_drag(document, *pointer_id, false)?
                    || optional_input!(
                        "graph-canvas",
                        self.end_graph_canvas_pointer(document, *pointer_id, *x, *y, false,)
                    )?
                    || optional_input!(
                        "graph-canvas",
                        self.end_graph_minimap_pointer(document, *pointer_id, false)
                    )?
                    || optional_input!(
                        "controls",
                        self.end_reorder_list_pointer(document, *pointer_id, *x, *y, false,)
                    )?
                    || self.end_split_resize(document, *pointer_id, false)?
                    || hooked!(self, dock, |h| (h.end_split)(
                        self,
                        document,
                        *pointer_id,
                        false
                    ))?
                    || hooked!(self, workspace, |h| (h.end_resize)(
                        self,
                        document,
                        *pointer_id,
                        now
                    ))?
                    || hooked!(self, dock, |h| (h.end_item)(
                        self,
                        document,
                        *pointer_id,
                        *x,
                        *y,
                        false
                    ))?
                {
                    self.release_pointer(document, *pointer_id);
                    return Ok(CONSUMED);
                }
                let pressed = self.release_pointer(document, *pointer_id);
                if let Some(pressed) = pressed {
                    if Some(pressed) == target && !*activation_click {
                        self.activate_node_at(pressed, *x, *y)?;
                    }
                    true
                } else {
                    false
                }
            }
            PointerPhase::Cancel => {
                self.end_text_area_resize(document, *pointer_id, true)?;
                optional_input!(
                    "rich-text",
                    self.end_rich_text_pointer(document, *pointer_id, *x, *y, true)
                )?;
                #[cfg(feature = "image-viewer")]
                if let Some(viewer) = self
                    .world()
                    .pointer_capture(document, *pointer_id)
                    .and_then(|target| self.view_entity::<crate::ImageViewer>(target))
                {
                    self.image_viewer_pointer_up(viewer, *pointer_id)?;
                }
                self.text_editor_pointer_release(*pointer_id);
                self.document_text_pointer_release(*pointer_id);
                let scrollbar = self.end_scrollbar_drag(document, *pointer_id, true)?;
                let range = self.end_range_drag(document, *pointer_id, true)?;
                let xy_pad = self.end_xy_pad_drag(document, *pointer_id, true)?;
                let graph = optional_input!(
                    "graph-canvas",
                    self.end_graph_canvas_pointer(document, *pointer_id, *x, *y, true,)
                )?;
                let minimap = optional_input!(
                    "graph-canvas",
                    self.end_graph_minimap_pointer(document, *pointer_id, true)
                )?;
                let reorder = optional_input!(
                    "controls",
                    self.end_reorder_list_pointer(document, *pointer_id, *x, *y, true,)
                )?;
                let split = self.end_split_resize(document, *pointer_id, true)?;
                let dock_split = hooked!(self, dock, |h| (h.end_split)(
                    self,
                    document,
                    *pointer_id,
                    true
                ))?;
                let workspace = hooked!(self, workspace, |h| (h.end_resize)(
                    self,
                    document,
                    *pointer_id,
                    now
                ))?;
                let dock_item = hooked!(self, dock, |h| (h.end_item)(
                    self,
                    document,
                    *pointer_id,
                    *x,
                    *y,
                    true
                ))?;
                let pressed = self.release_pointer(document, *pointer_id).is_some();
                self.set_pointer_hover_at(document, *pointer_id, None, now)?;
                let calendar =
                    optional_input!("calendar", self.clear_calendar_heatmap_hover(document))?;
                let split_hover = self.sync_split_handle_hover(document, None)?;
                scrollbar
                    || range
                    || xy_pad
                    || graph
                    || minimap
                    || reorder
                    || split
                    || dock_split
                    || workspace
                    || dock_item
                    || calendar
                    || split_hover
                    || pressed
            }
            _ => false,
        };
        let handled = overlay.prevent_default || component_handled;
        Ok(InputDisposition {
            handled,
            prevent_default: handled,
        })
    }

    /// One wheel delta. `landed` receives the node under the pointer.
    pub(super) fn dispatch_wheel(
        &mut self,
        document: DocumentId,
        wheel: &WheelInput,
        landed: &mut Option<StableNodeId>,
    ) -> Result<InputDisposition, FrameworkError> {
        let WheelInput {
            x,
            y,
            delta_x,
            delta_y,
            modifiers,
            ..
        } = wheel;
        let line_delta = &(wheel.unit == WheelUnit::Lines);
        let (dx, dy) = if modifiers.shift && !cfg!(target_os = "macos") {
            (*delta_y, *delta_x)
        } else {
            (*delta_x, *delta_y)
        };
        let scale = if *line_delta { LINE_SCROLL_EXTENT } else { 1.0 };
        let delta = ScrollOffset {
            x: -dx * scale,
            y: -dy * scale,
        };
        let overlay =
            self.route_overlay_pointer(document, 0, OverlayPointerPhase::Wheel, *x, *y)?;
        *landed = overlay.target;
        // 锚定浮层（补全弹层 / hover 浮窗）优先：指针落在浮层面板
        #[cfg(feature = "image-viewer")]
        if let Some(viewer) = overlay
            .target
            .and_then(|target| self.view_entity::<crate::ImageViewer>(target))
            && self.image_viewer_wheel(viewer, *x, *y, dy)?
        {
            return Ok(CONSUMED);
        }
        // 上时滚轮滚动浮层自身（按行，方向跟随滚轮），不再落到
        // 编辑器或文档滚动。
        let overlay_rows = if *delta_y > 0.0 {
            1isize
        } else if *delta_y < 0.0 {
            -1
        } else {
            0
        };
        if overlay_rows != 0 && self.scroll_text_overlay_at(document, *x, *y, overlay_rows)? {
            return Ok(CONSUMED);
        }
        #[cfg(feature = "graph-canvas")]
        let graph_delta = if *line_delta {
            GraphScrollDelta::Lines { y: -dy }
        } else {
            GraphScrollDelta::Pixels { y: -dy }
        };
        let graph_target = overlay.target;
        let scrolled = if overlay.prevent_default {
            overlay
                .target
                .map(|target| self.scroll_overlay_from(document, target, delta))
                .transpose()?
                .flatten()
                .is_some()
        } else if graph_target.is_some_and(|_target| {
            optional_input!("graph-canvas", self.is_graph_canvas(_target), false)
        }) {
            optional_input!(
                "graph-canvas",
                self.scroll_graph_canvas(
                    document,
                    graph_target.expect("graph target"),
                    *x,
                    *y,
                    graph_delta,
                )
            )?
        } else {
            self.scroll_from_hit(overlay.target, delta)?.is_some()
        };
        let handled = overlay.prevent_default || scrolled;
        Ok(InputDisposition {
            handled,
            prevent_default: handled,
        })
    }
    /// One key transition or committed text through the key chain: blocking
    /// overlays, the application's key policy, a focused terminal, the
    /// focused editor, focus traversal, then the focused control. Clipboard
    /// shortcuts run at their place in that chain, after anything above them
    /// had its chance at the key.
    pub(super) fn dispatch_keystroke(
        &mut self,
        document: DocumentId,
        stroke: KeyStroke<'_>,
        services: &mut dyn HostServices,
        mut text_shaper: Option<&mut dyn TextShaper>,
    ) -> Result<InputDisposition, FrameworkError> {
        self.world
            .note_input_modality(document, crate::InputModality::Keyboard);
        let KeyStroke {
            pressed,
            key,
            text,
            repeat,
            modifiers,
        } = stroke;
        let keyboard_barrier = self.has_blocking_runtime_overlay(document);
        if pressed && !modifiers.alt && !modifiers.control && !modifiers.meta {
            let overlay_key = match key {
                "Escape" if !repeat => Some(OverlayKey::Escape),
                "Tab" => Some(OverlayKey::Tab {
                    reverse: modifiers.shift,
                }),
                _ => None,
            };
            if matches!(overlay_key, Some(OverlayKey::Escape))
                && !modifiers.shift
                && self.dismiss_focused_field_options(document)?
            {
                return Ok(CONSUMED);
            }
            if let Some(key) = overlay_key
                && self.route_overlay_key(document, key)?
            {
                return Ok(CONSUMED);
            }
            if matches!(overlay_key, Some(OverlayKey::Escape))
                && self.dismiss_popovers_on_escape()?
            {
                return Ok(CONSUMED);
            }
            // overlay 未消费的 Esc：先取消拖拽移动选中，再结束 snippet 会
            // 话，签名帮助在场时消费但不关补全（两段式：宿主随后撤签名），
            // 再关闭补全弹层，最后塌缩多光标到主光标。都只在聚焦多行
            // 编辑器且状态存在时消费事件，否则穿透给宿主（首次按下才生
            // 效，repeat 不消费）。
            if matches!(overlay_key, Some(OverlayKey::Escape))
                && !repeat
                && (self.cancel_focused_text_selection_drag(document)
                    || self.cancel_focused_text_snippet(document)?
                    || self.focused_text_signature_showing(document)
                    || self.dismiss_focused_text_completion(document)?
                    || self.collapse_focused_text_selections(document)?)
            {
                return Ok(CONSUMED);
            }
        }
        // The application's key policy sees keys, not text.
        if !stroke.is_text()
            && !keyboard_barrier
            && self.dispatch_focused_key(
                document,
                &crate::KeyInput::new(
                    pressed,
                    key,
                    modifiers.alt,
                    modifiers.control,
                    modifiers.shift,
                    modifiers.meta,
                    repeat,
                ),
            )
        {
            return Ok(CONSUMED);
        }
        // Past the application's policy, only presses act.
        if !pressed {
            return Ok(InputDisposition {
                handled: false,
                prevent_default: keyboard_barrier,
            });
        }
        if !keyboard_barrier && self.focused_terminal(document).is_some() {
            return self.terminal_keystroke(document, stroke, services);
        }
        // Focused plain text editors own their editing keys (caret moves,
        // selection, deletion, indent, pairing) before any generic routing.
        if self.text_editor_key(
            document,
            key,
            text,
            modifiers,
            reborrow_text_shaper(&mut text_shaper),
        )? {
            return Ok(CONSUMED);
        }
        if key == "Tab"
            && !modifiers.alt
            && !modifiers.control
            && !modifiers.meta
            && self.navigate_sequential_focus(document, modifiers.shift)?
        {
            return Ok(CONSUMED);
        }
        if !modifiers.alt && !modifiers.control && !modifiers.meta && !modifiers.shift {
            let segmented_navigation = match key {
                "ArrowLeft" => Some(RovingFocusIntent::Previous),
                "ArrowRight" => Some(RovingFocusIntent::Next),
                "Home" => Some(RovingFocusIntent::First),
                "End" => Some(RovingFocusIntent::Last),
                _ => None,
            };
            if let Some(intent) = segmented_navigation
                && self.navigate_focused_segmented(document, intent)?
            {
                return Ok(CONSUMED);
            }
            if matches!(key, " " | "Space" | "Enter")
                && let Some(target) = self.world().focused(document)
                && self.is_segmented_option_node(target)
            {
                if !repeat {
                    self.activate_node(target)?;
                }
                return Ok(CONSUMED);
            }
        }
        let handled = if !modifiers.alt
            && (modifiers.control || modifiers.meta)
            && key.eq_ignore_ascii_case("z")
        {
            // Undo/redo is the one editing shortcut that carries Shift, so
            // it is matched before the clipboard arm excludes it.
            if modifiers.shift {
                self.redo_focused_text(document)?
            } else {
                self.undo_focused_text(document)?
            }
        } else if !modifiers.alt && !modifiers.shift {
            let primary = modifiers.control || modifiers.meta;
            if primary && self.dispatch_clipboard_shortcut(document, key, services)? {
                return Ok(CONSUMED);
            }
            if self.focused_control_key(document, key, primary)? {
                return Ok(CONSUMED);
            }
            let navigation = match (key, primary) {
                ("ArrowUp", false) => Some(TableNavigation::PreviousRow),
                ("ArrowDown", false) => Some(TableNavigation::NextRow),
                ("ArrowLeft", false) => Some(TableNavigation::PreviousColumn),
                ("ArrowRight", false) => Some(TableNavigation::NextColumn),
                ("Home", false) => Some(TableNavigation::RowStart),
                ("End", false) => Some(TableNavigation::RowEnd),
                ("Home", true) => Some(TableNavigation::FirstRow),
                ("End", true) => Some(TableNavigation::LastRow),
                ("PageUp", false) => Some(TableNavigation::PageUp),
                ("PageDown", false) => Some(TableNavigation::PageDown),
                _ => None,
            };
            if let Some(navigation) = navigation {
                self.navigate_focused_table(document, navigation, TABLE_PAGE_ROWS)?
            } else if !primary {
                match key {
                    "Backspace" => self.delete_focused_text_backward(document)?,
                    _ => match text {
                        Some(text) => self.replace_focused_text(document, text)?,
                        None => false,
                    },
                }
            } else {
                false
            }
        } else {
            false
        };
        Ok(InputDisposition {
            handled,
            prevent_default: handled || keyboard_barrier,
        })
    }

    /// Keys a focused range, pad, graph, split, number field, palette,
    /// select, dropdown, tree or activatable control acts on. `primary` is
    /// Ctrl or Cmd held; none of these take it.
    fn focused_control_key(
        &mut self,
        document: DocumentId,
        key: &str,
        primary: bool,
    ) -> Result<bool, FrameworkError> {
        if primary {
            return Ok(false);
        }
        let range_adjustment = match key {
            "ArrowLeft" | "ArrowDown" => Some(RangeAdjustment::Decrement),
            "ArrowRight" | "ArrowUp" => Some(RangeAdjustment::Increment),
            "PageDown" => Some(RangeAdjustment::PageDecrement),
            "PageUp" => Some(RangeAdjustment::PageIncrement),
            "Home" => Some(RangeAdjustment::Minimum),
            "End" => Some(RangeAdjustment::Maximum),
            _ => None,
        };
        if let Some(adjustment) = range_adjustment
            && self.adjust_focused_range(document, adjustment)?
        {
            return Ok(true);
        }
        let xy_adjustment = match key {
            "ArrowLeft" => Some(XYPadAdjustment::Left),
            "ArrowRight" => Some(XYPadAdjustment::Right),
            "ArrowUp" => Some(XYPadAdjustment::Up),
            "ArrowDown" => Some(XYPadAdjustment::Down),
            _ => None,
        };
        if let Some(adjustment) = xy_adjustment
            && self.adjust_focused_xy_pad(document, adjustment)?
        {
            return Ok(true);
        }
        #[cfg(feature = "image-viewer")]
        {
            let step = match key {
                "ArrowLeft" => Some(false),
                "ArrowRight" => Some(true),
                _ => None,
            };
            if let Some(forward) = step
                && self.step_focused_image_viewer(document, forward)?
            {
                return Ok(true);
            }
        }
        #[cfg(feature = "graph-canvas")]
        {
            let graph_adjustment = match key {
                "ArrowLeft" => Some(GraphCanvasAdjustment::PanLeft),
                "ArrowRight" => Some(GraphCanvasAdjustment::PanRight),
                "ArrowUp" => Some(GraphCanvasAdjustment::PanUp),
                "ArrowDown" => Some(GraphCanvasAdjustment::PanDown),
                "Home" | "0" => Some(GraphCanvasAdjustment::Fit),
                "+" | "=" => Some(GraphCanvasAdjustment::ZoomIn),
                "-" => Some(GraphCanvasAdjustment::ZoomOut),
                "Escape" => Some(GraphCanvasAdjustment::ClearSelection),
                _ => None,
            };
            if let Some(adjustment) = graph_adjustment
                && self.adjust_focused_graph_canvas(document, adjustment)?
            {
                return Ok(true);
            }
        }
        let split_direction = match key {
            "ArrowLeft" | "ArrowUp" => Some(-1.0),
            "ArrowRight" | "ArrowDown" => Some(1.0),
            _ => None,
        };
        if let Some(direction) = split_direction
            && (self.adjust_focused_split(document, direction)?
                || hooked!(self, dock, |h| (h.adjust_focused_split)(
                    self, document, direction
                ))?)
        {
            return Ok(true);
        }
        // A numeric field's step and commit keys are routed with its other
        // editing keys in `text_editor_key`; Escape is not an editing key and
        // reverts the draft here.
        if key == "Escape" && self.revert_focused_number_input(document)? {
            return Ok(true);
        }
        let palette_nav = match key {
            "ArrowUp" => Some(crate::ActionPickerNavigation::Previous),
            "ArrowDown" => Some(crate::ActionPickerNavigation::Next),
            "Home" => Some(crate::ActionPickerNavigation::First),
            "End" => Some(crate::ActionPickerNavigation::Last),
            "Enter" => Some(crate::ActionPickerNavigation::Confirm),
            "Escape" => Some(crate::ActionPickerNavigation::Dismiss),
            _ => None,
        };
        if let Some(navigation) = palette_nav
            && hooked!(self, command_palette, |h| (h.navigate)(
                self, document, navigation
            ))?
        {
            return Ok(true);
        }
        let select_delta = match key {
            "ArrowUp" => Some(-1),
            "ArrowDown" => Some(1),
            _ => None,
        };
        if let Some(delta) = select_delta
            && (hooked!(self, select, |h| (h.adjust)(self, document, delta))?
                || hooked!(self, dropdown, |h| (h.adjust)(self, document, delta))?
                || hooked!(self, search_dropdown, |h| (h.adjust)(self, document, delta))?)
        {
            return Ok(true);
        }
        if matches!(key, " " | "Space" | "Enter")
            && (hooked!(self, select, |h| (h.commit)(self, document))?
                || hooked!(self, dropdown, |h| (h.commit)(self, document))?)
        {
            return Ok(true);
        }
        if key == "Enter" && hooked!(self, search_dropdown, |h| (h.commit)(self, document))? {
            return Ok(true);
        }
        let tree_nav = match key {
            "ArrowUp" => Some(crate::TreeNavigation::Previous),
            "ArrowDown" => Some(crate::TreeNavigation::Next),
            "Home" => Some(crate::TreeNavigation::First),
            "End" => Some(crate::TreeNavigation::Last),
            "ArrowLeft" => Some(crate::TreeNavigation::Parent),
            "ArrowRight" => Some(crate::TreeNavigation::Child),
            "Enter" => Some(crate::TreeNavigation::Activate),
            " " | "Space" => Some(crate::TreeNavigation::Toggle),
            _ => None,
        };
        if let Some(navigation) = tree_nav
            && hooked!(self, tree, |h| (h.navigate)(self, document, navigation))?
        {
            return Ok(true);
        }
        if matches!(key, " " | "Space" | "Enter")
            && let Some(target) = self.world().focused(document)
            && self.activate_node(target)?
        {
            return Ok(true);
        }
        Ok(false)
    }

    /// A key or text for the focused terminal. Copy and paste go through
    /// the host clipboard; everything else becomes terminal input. The event
    /// is consumed either way, but counts as handled only when it sent
    /// something, so a printable key leaves its text to arrive on its own.
    fn terminal_keystroke(
        &mut self,
        document: DocumentId,
        stroke: KeyStroke<'_>,
        services: &mut dyn HostServices,
    ) -> Result<InputDisposition, FrameworkError> {
        let KeyStroke {
            key,
            text,
            modifiers,
            ..
        } = stroke;
        let clipboard_chord = modifiers.control && modifiers.shift || modifiers.meta;
        let handled = if clipboard_chord && key.eq_ignore_ascii_case("c") {
            let copied = self
                .terminal_selected_text(document)
                .filter(|text| !text.is_empty());
            copied.is_some_and(|text| services.write_clipboard(&text).is_ok())
        } else if clipboard_chord && key.eq_ignore_ascii_case("v") {
            match services.read_clipboard() {
                Ok(Some(text)) if !text.is_empty() => self.paste_terminal(document, &text)?,
                _ => false,
            }
        } else {
            self.terminal_key(
                document,
                key,
                text,
                modifiers.control,
                modifiers.alt,
                modifiers.shift,
            )?
        };
        Ok(InputDisposition {
            handled,
            prevent_default: true,
        })
    }

    /// Ctrl/Cmd + A / C / X / V on the focused editor, or a copy of the
    /// document selection.
    ///
    /// The Runtime owns what is selected and what an edit does; this only
    /// moves text between that selection and the host clipboard. A copy with
    /// nothing selected leaves the clipboard alone rather than clearing it,
    /// a cut deletes only once the clipboard took the text, and a paste of
    /// an empty clipboard is not an edit.
    fn dispatch_clipboard_shortcut(
        &mut self,
        document: DocumentId,
        key: &str,
        services: &mut dyn HostServices,
    ) -> Result<bool, FrameworkError> {
        if key.eq_ignore_ascii_case("a") {
            return self.select_all_focused_text(document);
        }
        if key.eq_ignore_ascii_case("c") {
            let text = self
                .focused_selected_text(document)
                .or_else(|| self.document_selected_text(document));
            return Ok(text
                .is_some_and(|text| !text.is_empty() && services.write_clipboard(&text).is_ok()));
        }
        if key.eq_ignore_ascii_case("x") {
            let Some(text) = self.focused_selected_text(document) else {
                return Ok(false);
            };
            // A read-only field copies but never loses its text.
            if services.write_clipboard(&text).is_err() {
                return Ok(false);
            }
            self.cut_focused_text(document)?;
            return Ok(true);
        }
        if key.eq_ignore_ascii_case("v") {
            return match services.read_clipboard() {
                Ok(Some(text)) if !text.is_empty() => self.paste_focused_text(document, &text),
                _ => Ok(false),
            };
        }
        Ok(false)
    }

    /// Composition into the focused editor or terminal.
    ///
    /// Retained TextInput/TextArea/SearchDropdown/CommandPalette state is the
    /// only editing authority. A focused editable field, or a blocking
    /// overlay, consumes the event so no second IME path can also apply it.
    ///
    /// Multi-cursor restriction: composition is anchored to the primary
    /// cursor only. While preedit is active the editor paints a single caret
    /// and, on commit, only the primary selection's text is replaced; the
    /// additional cursors survive through offset remapping.
    pub(super) fn dispatch_composition(
        &mut self,
        document: DocumentId,
        composition: &CompositionInput,
    ) -> Result<InputDisposition, FrameworkError> {
        let overlay_blocks = self.has_blocking_runtime_overlay(document);
        if !overlay_blocks && self.focused_terminal(document).is_some() {
            match composition {
                CompositionInput::Start => {
                    self.set_terminal_preedit(document, "")?;
                }
                CompositionInput::Update { text, .. } => {
                    self.set_terminal_preedit(document, text)?;
                }
                CompositionInput::Commit(text) => {
                    self.set_terminal_preedit(document, "")?;
                    self.terminal_input(document, text.as_bytes().to_vec())?;
                }
                CompositionInput::Disabled | CompositionInput::End => {
                    self.set_terminal_preedit(document, "")?;
                }
                CompositionInput::Enabled | CompositionInput::DeleteSurrounding { .. } => {}
            }
            return Ok(CONSUMED);
        }
        let owns_ime = self.editable_focused_text_input(document).is_some();
        let handled = match composition {
            CompositionInput::Enabled => false,
            CompositionInput::Start => self.set_ime_preedit(document, String::new(), None)?,
            CompositionInput::Disabled => {
                let leftover = self
                    .world()
                    .focused_text_input(document)
                    .and_then(|(id, _)| self.world().ime(id).map(|ime| ime.text.to_owned()))
                    .filter(|text| !text.is_empty());
                match leftover {
                    Some(text) => self.commit_ime(document, &text)?,
                    None => self.clear_ime(document)?,
                }
            }
            CompositionInput::End => self.clear_ime(document)?,
            CompositionInput::Update { text, selection } => {
                self.set_ime_preedit(document, text.clone(), *selection)?
            }
            CompositionInput::Commit(text) => self.commit_ime(document, text)?,
            CompositionInput::DeleteSurrounding {
                before_bytes,
                after_bytes,
            } => self.delete_ime_surrounding(document, *before_bytes, *after_bytes)?,
        };
        Ok(InputDisposition {
            handled,
            prevent_default: handled || owns_ime || overlay_blocks,
        })
    }
}

impl AppContext {
    /// Keyboard editing for the focused plain text editor.
    ///
    /// Returns `false` when no plain editor is focused or the key is not an
    /// editing key, so composite-surface navigation and generic activation
    /// keep working unchanged.
    fn text_editor_key(
        &mut self,
        document: DocumentId,
        key: &str,
        text: Option<&str>,
        modifiers: InputModifiers,
        mut shaper: Option<&mut dyn TextShaper>,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.focused_text_editor(document) else {
            // An IME composition hides the editor, but its navigation keys
            // still belong to it: the composition owns them, and they must
            // not reach an enclosing table, tree or select and carry focus
            // out of the field.
            return Ok(caret_intent(key, modifiers).is_some()
                && self.focused_text_editor_composing(document));
        };
        // A numeric field steps on plain ArrowUp/ArrowDown and commits its
        // draft on Enter. Shift+ArrowUp/Down select like any single-line
        // field. The arrows are the field's even when nothing moves (a bound,
        // read-only), so they never fall through to an enclosing table or
        // tree and carry focus out of the field. An Enter that commits
        // nothing falls through, so a dialog or form can still confirm.
        // Alt+ArrowUp/Down move the caret like any single-line field's; Enter
        // commits whatever Shift or Alt accompany it.
        if focused.is_numeric() && !modifiers.control && !modifiers.meta {
            match key {
                "ArrowUp" | "ArrowDown" if !modifiers.shift && !modifiers.alt => {
                    let steps = if key == "ArrowUp" { 1 } else { -1 };
                    self.step_focused_number_input(document, steps)?;
                    return Ok(true);
                }
                "Enter" => return self.commit_focused_number_input(document),
                _ => {}
            }
        }
        // Arrow keys in the editor's line space (#59): a vertical editor's
        // Up/Down walk its column and Left/Right cross columns, with every
        // modifier below carried along by translating the key itself.
        let key = self.focused_text_line_space_key(document, key);
        if key == "Tab"
            && focused.multiline
            && !modifiers.control
            && !modifiers.meta
            && !modifiers.alt
            && self.advance_focused_text_snippet(document, modifiers.shift)?
        {
            return Ok(true);
        } // 补全弹层激活时，无修饰的 Up/Down/Enter/Tab 由弹层消费：Up/Down
        // 移动候选选中项（编辑器选区不动），Enter/Tab 接受选中项。其余键
        // 穿透正常编辑（打字触发宿主重喂过滤列表）；任何修饰键组合
        // （Cmd+D、Alt+Up、Shift+Up 等）一律穿透。
        if !modifiers.control
            && !modifiers.meta
            && !modifiers.alt
            && !modifiers.shift
            && matches!(key, "ArrowUp" | "ArrowDown" | "Enter" | "Tab")
            && self.focused_text_completion_active(document)
        {
            if matches!(key, "ArrowUp" | "ArrowDown") {
                self.move_focused_text_completion(document, key == "ArrowDown")?;
            } else {
                self.accept_focused_text_completion(document, None)?;
            }
            // 弹层激活期间整键消费（边界上导航无可做也不移动选区）。
            return Ok(true);
        }
        let control = modifiers.control;
        let meta = modifiers.meta;
        // Alt+Cmd/Ctrl+Up/Down adds cursors above/below the selection(s)
        // (Zed-style multi-cursor). Multiline editors own the gesture even
        // when every target already holds a cursor; single-line fields
        // reject multi-cursor entirely and keep plain movement.
        if modifiers.alt
            && (control || meta)
            && focused.multiline
            && matches!(key, "ArrowUp" | "ArrowDown")
        {
            self.add_focused_text_cursor(
                document,
                key == "ArrowUp",
                reborrow_text_shaper(&mut shaper),
            )?;
            return Ok(true);
        }
        // Alt+Up/Down moves the caret's line block; Alt+Shift+Up/Down
        // duplicates it. Multiline editors own the gesture even at the
        // document edge; single-line fields keep plain caret movement.
        if modifiers.alt
            && !control
            && !meta
            && focused.multiline
            && matches!(key, "ArrowUp" | "ArrowDown")
        {
            let direction = if key == "ArrowUp" {
                TextLineDirection::Up
            } else {
                TextLineDirection::Down
            };
            if modifiers.shift {
                self.duplicate_focused_text_lines(document)?;
            } else {
                self.move_focused_text_lines(document, direction)?;
            }
            return Ok(true);
        }
        if let Some(intent) = caret_intent(key, modifiers) {
            return self.move_focused_text_caret(document, intent, modifiers.shift, shaper);
        }
        let delete = match (key, control || meta, modifiers.alt) {
            ("Backspace", false, false) => Some(TextDeleteKind::Backward),
            ("Backspace", _, true) => Some(TextDeleteKind::WordBackward),
            ("Backspace", true, false) => Some(TextDeleteKind::LineStart),
            ("Delete", false, false) => Some(TextDeleteKind::Forward),
            ("Delete", _, true) => Some(TextDeleteKind::WordForward),
            ("Delete", true, false) => Some(TextDeleteKind::LineEnd),
            _ => None,
        };
        if let Some(kind) = delete {
            return self.delete_focused_text(document, kind);
        }
        if control || meta {
            // Cmd/Ctrl+D selects the next occurrence of the primary
            // selection's word (Zed-style multi-cursor). Multiline only.
            // Cmd/Ctrl+Shift+D is deliberately unbound for now; hosts can
            // call `select_focused_text_occurrence(document, true)` for the
            // reverse direction.
            if key.eq_ignore_ascii_case("d") && focused.multiline && !modifiers.shift {
                return self.select_focused_text_occurrence(document, false);
            }
            // Comment toggle is the only code-editing modified key.
            if key == "/" && focused.code_editing.is_some() {
                return self.code_edit_toggle_comment(document);
            }
            // Cmd/Ctrl+Shift+K deletes the caret line.
            if modifiers.shift && key.eq_ignore_ascii_case("k") {
                return self.delete_focused_text_lines(document);
            }
            // Ctrl/Cmd+J joins the touched selection lines.
            if !modifiers.shift && key.eq_ignore_ascii_case("j") {
                return self.join_focused_text_lines(document);
            }
            // Ctrl/Cmd+Shift+U uppercases, Ctrl/Cmd+U lowercases.
            if key.eq_ignore_ascii_case("u") {
                return self.transform_focused_text_case(document, modifiers.shift);
            }
            return Ok(false);
        }
        if key == "Enter" {
            if focused.multiline {
                return self.insert_focused_text_newline(document);
            }
            // Single-line fields submit without inserting a newline. IME
            // confirmation remains exclusively owned by the composition path.
            self.submit_focused_text_input(document)?;
            return Ok(true);
        }
        if key == "Tab" && !meta {
            // snippet 会话内 Tab 跳位优先于缩进；无会话时 `Ok(false)`，
            // 代码编辑器的缩进行为接手。
            if focused.multiline && self.advance_focused_text_snippet(document, modifiers.shift)? {
                return Ok(true);
            }
            if focused.code_editing.is_some() {
                return self.code_edit_indent(document, modifiers.shift);
            }
            return Ok(false);
        }
        let Some(text) = text else {
            return Ok(false);
        };
        let mut typed = text.chars();
        if let (Some(single), None) = (typed.next(), typed.next())
            && focused.code_editing.is_some()
            && self.code_edit_typed(document, single)?
        {
            return Ok(true);
        }
        self.replace_focused_text(document, text)
    }
}

/// The caret move an editor's navigation key asks for, in line space.
/// `None` for keys that are not caret navigation.
fn caret_intent(key: &str, modifiers: InputModifiers) -> Option<TextCaretIntent> {
    let (control, meta) = (modifiers.control, modifiers.meta);
    let word_modifier = control || modifiers.alt;
    match key {
        "ArrowLeft" => Some(match (meta, word_modifier) {
            (true, _) => TextCaretIntent::LineStart,
            (_, true) => TextCaretIntent::WordLeft,
            (false, false) => TextCaretIntent::Left,
        }),
        "ArrowRight" => Some(match (meta, word_modifier) {
            (true, _) => TextCaretIntent::LineEnd,
            (_, true) => TextCaretIntent::WordRight,
            _ => TextCaretIntent::Right,
        }),
        "ArrowUp" => Some(if meta {
            TextCaretIntent::DocStart
        } else {
            TextCaretIntent::Up
        }),
        "ArrowDown" => Some(if meta {
            TextCaretIntent::DocEnd
        } else {
            TextCaretIntent::Down
        }),
        "Home" => Some(if control || meta {
            TextCaretIntent::DocStart
        } else {
            TextCaretIntent::LineStart
        }),
        "End" => Some(if control || meta {
            TextCaretIntent::DocEnd
        } else {
            TextCaretIntent::LineEnd
        }),
        "PageUp" if !control && !meta && !modifiers.alt => Some(TextCaretIntent::PageUp),
        "PageDown" if !control && !meta && !modifiers.alt => Some(TextCaretIntent::PageDown),
        _ => None,
    }
}

/// The text a key carries, or `None` when it carries none. Which characters
/// may type is the runtime's call: its typing path refuses the control
/// characters command keys carry.
/// Reborrow the per-dispatch shaper so sequential uses never alias.
pub(super) fn reborrow_text_shaper<'s>(
    shaper: &'s mut Option<&mut dyn TextShaper>,
) -> Option<&'s mut dyn TextShaper> {
    match shaper.as_mut() {
        Some(shaper) => Some(&mut **shaper),
        None => None,
    }
}

fn nearest_focusable(context: &AppContext, mut target: StableNodeId) -> Option<StableNodeId> {
    loop {
        if context
            .world()
            .interaction(target)
            .is_some_and(|interaction| interaction.focusable)
        {
            return Some(target);
        }
        target = context.world().parent_id(target)?;
    }
}

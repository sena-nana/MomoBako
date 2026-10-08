use std::sync::Arc;

use nana_ui_core::{ControlSize, LengthSpec, SemanticColorRole};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, InteractionStyle,
    LayoutBox, MutationQueue, NodeKind, NodeStyle, SemanticPaint, StableNodeId, UiWorld,
};

pub use nana_ui_core::{XYPadEvent, XYPadValue};

/// Heights for the two-axis pad surface. Wider than single-line controls.
pub const fn xy_pad_height(size: ControlSize) -> f32 {
    match size {
        ControlSize::Small => 40.0,
        ControlSize::Medium => 48.0,
        ControlSize::Large => 64.0,
    }
}

/// Shift-drag lock: first dominant axis wins (`|dx| >= |dy|` is horizontal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XYPadAxisLock {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XYPadDragState {
    pub pointer_id: u64,
    pub origin_x: f32,
    pub origin_y: f32,
    pub axis_lock: Option<XYPadAxisLock>,
    pub initial: XYPadValue,
}

/// Keyboard steps after the pad is focused. Commits [`XYPadEvent::Change`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XYPadAdjustment {
    Left,
    Right,
    Up,
    Down,
}

/// Backend-neutral two-axis pad. Pointer down/move emits [`XYPadEvent::Input`];
/// release and focused arrow keys emit [`XYPadEvent::Change`].
#[derive(Debug, Clone, PartialEq)]
pub struct XYPad {
    pub value: XYPadValue,
    pub x_min: f32,
    pub x_max: f32,
    pub y_min: f32,
    pub y_max: f32,
    pub step: f32,
    pub size: ControlSize,
    pub disabled: bool,
    pub loading: bool,
    pub invalid: bool,
    pub label: Option<Arc<str>>,
    pub dragging: Option<XYPadDragState>,
    pub style: NodeStyle,
}

impl XYPad {
    pub fn new(value: XYPadValue) -> Self {
        let mut pad = Self {
            value,
            x_min: 0.0,
            x_max: 1.0,
            y_min: 0.0,
            y_max: 1.0,
            step: 0.0,
            size: ControlSize::Medium,
            disabled: false,
            loading: false,
            invalid: false,
            label: None,
            dragging: None,
            style: field_style(ControlSize::Medium),
        };
        pad.value = pad.sanitized(value);
        pad
    }

    pub fn x_range(mut self, min: f32, max: f32) -> Self {
        (self.x_min, self.x_max) = valid_range(min, max);
        self.value = self.sanitized(self.value);
        self
    }

    pub fn y_range(mut self, min: f32, max: f32) -> Self {
        (self.y_min, self.y_max) = valid_range(min, max);
        self.value = self.sanitized(self.value);
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = if step.is_finite() && step > 0.0 {
            step
        } else {
            0.0
        };
        self.value = self.sanitized(self.value);
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        Arc::make_mut(&mut self.style.layout).height = Some(LengthSpec::Px(xy_pad_height(size)));
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn inactive(&self) -> bool {
        self.disabled || self.loading
    }

    pub fn sanitized(&self, value: XYPadValue) -> XYPadValue {
        XYPadValue {
            x: quantize(
                finite_or(value.x, self.x_min),
                self.x_min,
                self.x_max,
                self.step,
            ),
            y: quantize(
                finite_or(value.y, self.y_min),
                self.y_min,
                self.y_max,
                self.step,
            ),
        }
    }

    pub fn value_at(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        locked: Option<(XYPadAxisLock, XYPadValue)>,
    ) -> XYPadValue {
        let nx = if width > 0.0 {
            (x / width).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let ny = if height > 0.0 {
            (y / height).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let mut value = XYPadValue {
            x: quantize(
                self.x_min + nx * (self.x_max - self.x_min),
                self.x_min,
                self.x_max,
                self.step,
            ),
            y: quantize(
                self.y_max - ny * (self.y_max - self.y_min),
                self.y_min,
                self.y_max,
                self.step,
            ),
        };
        if let Some((axis, previous)) = locked {
            match axis {
                XYPadAxisLock::Horizontal => value.y = previous.y,
                XYPadAxisLock::Vertical => value.x = previous.x,
            }
        }
        value
    }

    pub fn value_from_point(
        &self,
        x: f32,
        y: f32,
        bounds: LayoutBox,
        locked: Option<(XYPadAxisLock, XYPadValue)>,
    ) -> XYPadValue {
        self.value_at(
            x - bounds.x,
            y - bounds.y,
            bounds.width,
            bounds.height,
            locked,
        )
    }

    pub fn keyboard_value(&self, value: XYPadValue, adjustment: XYPadAdjustment) -> XYPadValue {
        let delta = if self.step > 0.0 {
            self.step
        } else {
            (self.x_max - self.x_min)
                .max(self.y_max - self.y_min)
                .max(1.0)
                / 100.0
        };
        let mut next = value;
        match adjustment {
            XYPadAdjustment::Left => next.x -= delta,
            XYPadAdjustment::Right => next.x += delta,
            XYPadAdjustment::Up => next.y += delta,
            XYPadAdjustment::Down => next.y -= delta,
        }
        self.sanitized(next)
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        if layout.width.is_none() {
            layout.width = Some(LengthSpec::Fill);
        }
        layout.height = Some(LengthSpec::Px(xy_pad_height(self.size)));
        if layout.border_width.is_none() {
            layout.border_width = Some(nana_ui_core::HAIRLINE);
        }
        if layout.border_radius.is_none() {
            style.radius = Some(nana_ui_core::RadiusTier::Sm);
        }
        if style.background.is_none() {
            style.background = Some(SemanticColorRole::Subtle);
        }
        if style.border.is_none() {
            style.border = Some(SemanticColorRole::Border);
        }
        if style.interaction.hovered.border.is_none() {
            style.interaction.hovered.border = Some(SemanticColorRole::BorderStrong);
        }
        if style.interaction.focused.border.is_none() {
            style.interaction.focused.border = Some(SemanticColorRole::BorderStrong);
        }
        if self.invalid {
            style.border = Some(SemanticColorRole::Danger);
            style.interaction.hovered.border = Some(SemanticColorRole::Danger);
            style.interaction.focused.border = Some(SemanticColorRole::Danger);
        }
        style
    }
}

impl ComponentView for XYPad {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "xy-pad".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let inactive = self.inactive();
        let x_span = (self.x_max - self.x_min).max(f32::EPSILON);
        let y_span = (self.y_max - self.y_min).max(f32::EPSILON);
        let visual = crate::StandardVisual::XYPad {
            value: self.value,
            nx: ((self.value.x - self.x_min) / x_span).clamp(0.0, 1.0),
            ny: (1.0 - (self.value.y - self.y_min) / y_span).clamp(0.0, 1.0),
            size: self.size,
            invalid: self.invalid,
            disabled: inactive,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: !inactive,
                focusable: !inactive,
            },
            AccessibilityState {
                role: AccessibilityRole::Slider,
                label: self.label.clone(),
                value: Some(Arc::from(format!("{}, {}", self.value.x, self.value.y))),
                disabled: inactive,
                busy: self.loading,
                invalid: self.invalid,
                numeric_value: Some(f64::from(self.value.x)),
                numeric_minimum: Some(f64::from(self.x_min)),
                numeric_maximum: Some(f64::from(self.x_max)),
                numeric_step: (self.step > 0.0).then_some(f64::from(self.step)),
                ..AccessibilityState::default()
            },
        );
    }
}

impl crate::AppContext {
    /// Focused arrow keys commit [`XYPadEvent::Change`]. No-op unless an
    /// active pad owns document focus.
    pub fn adjust_focused_xy_pad(
        &mut self,
        document: crate::DocumentId,
        adjustment: XYPadAdjustment,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(target) = self.world().focused(document) else {
            return Ok(false);
        };
        if !self.is_xy_pad(target) {
            return Ok(false);
        }
        let entity = crate::Entity::<XYPad>::from_stable_id(target);
        if self.read(entity, XYPad::inactive)? {
            return Ok(false);
        }
        self.update_component(entity, |pad, cx| {
            let next = pad.keyboard_value(pad.value, adjustment);
            if pad.value != next {
                pad.value = next;
                cx.emit(XYPadEvent::Change(next));
            }
            true
        })
    }
}

fn field_style(size: ControlSize) -> NodeStyle {
    NodeStyle {
        layout: Arc::new(nana_ui_core::LayoutStyle {
            width: Some(LengthSpec::Fill),
            height: Some(LengthSpec::Px(xy_pad_height(size))),
            border_width: Some(nana_ui_core::HAIRLINE),
            ..nana_ui_core::LayoutStyle::default()
        }),
        radius: Some(nana_ui_core::RadiusTier::Sm),
        background: Some(SemanticColorRole::Subtle),
        border: Some(SemanticColorRole::Border),
        interaction: InteractionStyle {
            hovered: SemanticPaint {
                border: Some(SemanticColorRole::BorderStrong),
                ..SemanticPaint::default()
            },
            focused: SemanticPaint::FOCUS_BORDER,
            disabled: SemanticPaint {
                foreground: Some(SemanticColorRole::Faint),
                background: Some(SemanticColorRole::Subtle),
                border: Some(SemanticColorRole::Border),
                ..SemanticPaint::default()
            },
            ..InteractionStyle::default()
        },
        ..NodeStyle::default()
    }
}

fn valid_range(min: f32, max: f32) -> (f32, f32) {
    if min.is_finite() && max.is_finite() && max > min {
        (min, max)
    } else {
        (0.0, 1.0)
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn quantize(value: f32, min: f32, max: f32, step: f32) -> f32 {
    let value = value.clamp(min, max);
    if step <= 0.0 || step.is_nan() {
        return value;
    }
    let stepped = ((value - min) / step).round() * step + min;
    ((stepped * 1_000_000.0).round() / 1_000_000.0).clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, DocumentId, MutationQueue};
    use std::sync::{Arc, Mutex};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn layout(context: &mut AppContext, id: crate::StableNodeId) {
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            id,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        context.commit_mutations(mutations).unwrap();
    }

    fn collect_events(
        context: &mut AppContext,
        pad: crate::Entity<XYPad>,
    ) -> Arc<Mutex<Vec<XYPadEvent>>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        context
            .on(pad, move |_pad, event: &XYPadEvent, _cx| {
                observed.lock().unwrap().push(*event);
            })
            .unwrap();
        events
    }

    #[test]
    fn construct_clamps_ranges_and_quantizes_step() {
        let pad = XYPad::new(XYPadValue::new(4.0, f32::NAN));
        assert_eq!(pad.value, XYPadValue::new(1.0, 0.0));
        assert_eq!(xy_pad_height(ControlSize::Small), 40.0);
        assert_eq!(xy_pad_height(ControlSize::Medium), 48.0);
        assert_eq!(xy_pad_height(ControlSize::Large), 64.0);

        let invalid_range = XYPad::new(XYPadValue::new(0.25, 0.25)).x_range(1.0, 0.0);
        assert_eq!((invalid_range.x_min, invalid_range.x_max), (0.0, 1.0));

        let pad = XYPad::new(XYPadValue::new(0.33, 0.4))
            .x_range(-1.0, 1.0)
            .y_range(0.0, 10.0)
            .step(0.25);
        assert_eq!(pad.value, XYPadValue::new(0.25, 0.5));
        assert_eq!(
            pad.value_at(75.0, 25.0, 100.0, 100.0, None),
            XYPadValue::new(0.5, 7.5)
        );
        assert_eq!(
            pad.value_at(200.0, -10.0, 100.0, 100.0, None),
            XYPadValue::new(1.0, 10.0)
        );
        assert_eq!(
            pad.keyboard_value(XYPadValue::new(0.9, 0.0), XYPadAdjustment::Right),
            XYPadValue::new(1.0, 0.0)
        );
        assert_eq!(
            pad.keyboard_value(XYPadValue::new(0.5, 0.5), XYPadAdjustment::Up),
            XYPadValue::new(0.5, 0.75)
        );
    }

    #[test]
    fn pointer_emits_input_then_change_on_release() {
        let mut context = AppContext::new();
        let pad = context
            .create_component(
                document(),
                XYPad::new(XYPadValue::default())
                    .x_range(-1.0, 1.0)
                    .y_range(0.0, 10.0)
                    .step(0.25),
            )
            .unwrap();
        layout(&mut context, pad.stable_id());
        let events = collect_events(&mut context, pad);

        assert!(
            context
                .begin_xy_pad_drag(document(), 1, pad.stable_id(), 75.0, 25.0)
                .unwrap()
        );
        assert_eq!(
            context.read(pad, |pad| pad.value).unwrap(),
            XYPadValue::new(0.5, 7.5)
        );
        assert!(
            context
                .update_xy_pad_drag(document(), 1, 200.0, -10.0, false)
                .unwrap()
        );
        assert_eq!(
            context.read(pad, |pad| pad.value).unwrap(),
            XYPadValue::new(1.0, 10.0)
        );
        assert!(context.end_xy_pad_drag(document(), 1, false).unwrap());
        assert_eq!(
            *events.lock().unwrap(),
            [
                XYPadEvent::Input(XYPadValue::new(0.5, 7.5)),
                XYPadEvent::Input(XYPadValue::new(1.0, 10.0)),
                XYPadEvent::Change(XYPadValue::new(1.0, 10.0)),
            ]
        );
        assert!(context.world().pointer_capture(document(), 1).is_none());
    }

    #[test]
    fn keyboard_arrows_commit_change_after_focus() {
        let mut context = AppContext::new();
        let pad = context
            .create_component(document(), XYPad::new(XYPadValue::new(0.5, 0.5)).step(0.25))
            .unwrap();
        let events = collect_events(&mut context, pad);
        assert!(context.focus_node(document(), pad.stable_id()).unwrap());
        assert!(
            context
                .adjust_focused_xy_pad(document(), XYPadAdjustment::Up)
                .unwrap()
        );
        assert_eq!(
            context.read(pad, |pad| pad.value).unwrap(),
            XYPadValue::new(0.5, 0.75)
        );
        assert!(
            context
                .adjust_focused_xy_pad(document(), XYPadAdjustment::Left)
                .unwrap()
        );
        assert_eq!(
            *events.lock().unwrap(),
            [
                XYPadEvent::Change(XYPadValue::new(0.5, 0.75)),
                XYPadEvent::Change(XYPadValue::new(0.25, 0.75)),
            ]
        );
    }

    #[test]
    fn disabled_and_loading_ignore_pointer_and_keyboard() {
        for inactive in [
            XYPad::new(XYPadValue::new(0.2, 0.2)).disabled(true),
            XYPad::new(XYPadValue::new(0.2, 0.2)).loading(true),
        ] {
            let mut context = AppContext::new();
            let pad = context.create_component(document(), inactive).unwrap();
            layout(&mut context, pad.stable_id());
            let events = collect_events(&mut context, pad);
            assert!(!context.focus_node(document(), pad.stable_id()).unwrap());
            assert!(
                !context
                    .begin_xy_pad_drag(document(), 1, pad.stable_id(), 80.0, 20.0)
                    .unwrap()
            );
            assert!(
                !context
                    .adjust_focused_xy_pad(document(), XYPadAdjustment::Right)
                    .unwrap()
            );
            assert_eq!(
                context.read(pad, |pad| pad.value).unwrap(),
                XYPadValue::new(0.2, 0.2)
            );
            assert!(events.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn invalid_projects_danger_border_role() {
        let mut context = AppContext::new();
        let pad = context
            .create_component(
                document(),
                XYPad::new(XYPadValue::default()).invalid(true).label("Pan"),
            )
            .unwrap();
        let style = context.world().node_style(pad.stable_id()).unwrap();
        assert_eq!(style.border, Some(SemanticColorRole::Danger));
        assert_eq!(
            style.interaction.hovered.border,
            Some(SemanticColorRole::Danger)
        );
        assert_eq!(
            style.interaction.focused.border,
            Some(SemanticColorRole::Danger)
        );
        assert_ne!(
            style.interaction.focused.border,
            Some(SemanticColorRole::Accent)
        );
        assert_eq!(style.layout.height, Some(LengthSpec::Px(48.0)));
        assert_eq!(style.layout.width, Some(LengthSpec::Fill));
        assert_eq!(style.layout.border_width, Some(1.0));
        let accessibility = context.world().accessibility(pad.stable_id()).unwrap();
        assert!(accessibility.invalid);
        assert_eq!(accessibility.label.as_deref(), Some("Pan"));
        assert_eq!(accessibility.role, AccessibilityRole::Slider);
        assert!(matches!(
            context.world().standard_visual(pad.stable_id()),
            Some(crate::StandardVisual::XYPad { invalid: true, .. })
        ));
    }

    #[test]
    fn shift_locks_the_first_dominant_axis() {
        let mut context = AppContext::new();
        let pad = context
            .create_component(
                document(),
                XYPad::new(XYPadValue::default())
                    .x_range(-1.0, 1.0)
                    .y_range(0.0, 10.0),
            )
            .unwrap();
        layout(&mut context, pad.stable_id());

        assert!(
            context
                .begin_xy_pad_drag(document(), 3, pad.stable_id(), 50.0, 50.0)
                .unwrap()
        );
        assert_eq!(
            context.read(pad, |pad| pad.value).unwrap(),
            XYPadValue::new(0.0, 5.0)
        );
        assert!(
            context
                .update_xy_pad_drag(document(), 3, 80.0, 55.0, true)
                .unwrap()
        );
        let locked = context.read(pad, |pad| pad.value).unwrap();
        assert_eq!(locked, XYPadValue::new(0.6, 5.0));
        assert_eq!(
            context
                .read(pad, |pad| pad.dragging.unwrap().axis_lock)
                .unwrap(),
            Some(XYPadAxisLock::Horizontal)
        );
        assert!(
            context
                .update_xy_pad_drag(document(), 3, 80.0, 10.0, true)
                .unwrap()
        );
        assert_eq!(
            context.read(pad, |pad| pad.value).unwrap(),
            XYPadValue::new(0.6, 5.0)
        );
        assert!(
            context
                .update_xy_pad_drag(document(), 3, 80.0, 10.0, false)
                .unwrap()
        );
        assert_eq!(
            context.read(pad, |pad| pad.value).unwrap(),
            XYPadValue::new(0.6, 9.0)
        );
    }
}

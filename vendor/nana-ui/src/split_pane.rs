use nana_ui_core::LogicalPoint;

pub use nana_ui_core::SplitAxis;

/// Framework-owned interaction emitted by a split pane host.
#[derive(Debug, Clone, PartialEq)]
pub enum SplitPaneAction {
    SetSize(f32),
    Reset,
    ResizeStart,
    ResizeMove(LogicalPoint),
    ResizeEnd,
    Adjust(f32),
    Focus,
    Blur,
    Hover(bool),
}

/// Owns a split pane's constraints, persisted size, and transient interaction.
#[derive(Debug, Clone)]
pub struct SplitPaneController {
    model: nana_ui_core::SplitPaneModel,
}

impl SplitPaneController {
    pub fn new(axis: SplitAxis, default_size: f32, min_size: f32, max_size: f32) -> Self {
        Self {
            model: nana_ui_core::SplitPaneModel::new(axis, default_size, min_size, max_size),
        }
    }

    pub fn axis(&self) -> SplitAxis {
        self.model.axis()
    }

    pub fn size(&self) -> f32 {
        self.model.size()
    }

    pub fn default_size(&self) -> f32 {
        self.model.default_size()
    }

    pub fn limits(&self) -> (f32, f32) {
        self.model.limits()
    }

    pub fn keyboard_step(mut self, step: f32) -> Self {
        self.model = self.model.with_keyboard_step(step);
        self
    }

    /// Sizes the second child from the trailing/bottom edge instead of the first.
    pub fn from_end(mut self, from_end: bool) -> Self {
        self.model = self.model.with_from_end(from_end);
        self
    }

    pub fn is_active(&self) -> bool {
        self.model.is_active()
    }

    pub fn layout_json(&self) -> Result<String, serde_json::Error> {
        self.model.layout_json()
    }

    pub fn restore_layout_json(&mut self, value: &str) -> Result<(), serde_json::Error> {
        self.model.restore_layout_json(value)
    }

    /// Applies one interaction and reports whether observable state changed.
    pub fn update(&mut self, action: SplitPaneAction) -> bool {
        let mutation = match action {
            SplitPaneAction::SetSize(size) => nana_ui_core::SplitPaneMutation::SetSize(size),
            SplitPaneAction::Reset => nana_ui_core::SplitPaneMutation::Reset,
            SplitPaneAction::ResizeStart => nana_ui_core::SplitPaneMutation::ResizeStart,
            SplitPaneAction::ResizeMove(position) => nana_ui_core::SplitPaneMutation::ResizeMove {
                x: position.x,
                y: position.y,
            },
            SplitPaneAction::ResizeEnd => nana_ui_core::SplitPaneMutation::ResizeEnd,
            SplitPaneAction::Adjust(direction) => {
                nana_ui_core::SplitPaneMutation::Adjust(direction)
            }
            SplitPaneAction::Focus => nana_ui_core::SplitPaneMutation::Focus,
            SplitPaneAction::Blur => nana_ui_core::SplitPaneMutation::Blur,
            SplitPaneAction::Hover(hovered) => nana_ui_core::SplitPaneMutation::Hover(hovered),
        };
        self.model.update(mutation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_drag_and_keyboard_adjustments() {
        let mut controller = SplitPaneController::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        controller.update(SplitPaneAction::SetSize(400.0));
        assert_eq!(controller.size(), 260.0);
        controller.update(SplitPaneAction::Adjust(-20.0));
        assert_eq!(controller.size(), 140.0);
    }

    #[test]
    fn reset_restores_default_and_ends_drag() {
        let mut controller = SplitPaneController::new(SplitAxis::Vertical, 120.0, 64.0, 280.0);
        controller.update(SplitPaneAction::ResizeStart);
        controller.update(SplitPaneAction::ResizeMove(LogicalPoint::new(0.0, 10.0)));
        controller.update(SplitPaneAction::ResizeMove(LogicalPoint::new(0.0, 70.0)));
        assert_eq!(controller.size(), 180.0);
        controller.update(SplitPaneAction::Reset);
        assert_eq!(controller.size(), 120.0);
        assert!(!controller.is_active());
    }

    #[test]
    fn drag_reenters_limits_at_the_current_pointer_position() {
        let mut controller = SplitPaneController::new(SplitAxis::Horizontal, 200.0, 140.0, 260.0);
        controller.update(SplitPaneAction::ResizeStart);
        controller.update(SplitPaneAction::ResizeMove(LogicalPoint::new(100.0, 0.0)));
        controller.update(SplitPaneAction::ResizeMove(LogicalPoint::new(500.0, 0.0)));
        assert_eq!(controller.size(), 260.0);
        controller.update(SplitPaneAction::ResizeMove(LogicalPoint::new(130.0, 0.0)));
        assert_eq!(controller.size(), 230.0);

        let mut from_end =
            SplitPaneController::new(SplitAxis::Vertical, 200.0, 140.0, 260.0).from_end(true);
        from_end.update(SplitPaneAction::ResizeStart);
        from_end.update(SplitPaneAction::ResizeMove(LogicalPoint::new(0.0, 100.0)));
        from_end.update(SplitPaneAction::ResizeMove(LogicalPoint::new(0.0, 500.0)));
        assert_eq!(from_end.size(), 140.0);
        from_end.update(SplitPaneAction::ResizeMove(LogicalPoint::new(0.0, 70.0)));
        assert_eq!(from_end.size(), 230.0);
    }

    #[test]
    fn persisted_layout_round_trips_and_revalidates_constraints() {
        let mut controller =
            SplitPaneController::new(SplitAxis::Horizontal, 200.0, 140.0, 420.0).keyboard_step(4.0);
        controller.update(SplitPaneAction::SetSize(318.0));
        let encoded = controller.layout_json().expect("split layout serializes");
        let mut restored = SplitPaneController::new(SplitAxis::Vertical, 100.0, 0.0, 100.0);
        restored
            .restore_layout_json(&encoded)
            .expect("split layout restores");
        assert_eq!(restored.axis(), SplitAxis::Horizontal);
        assert_eq!(restored.size(), 318.0);
        assert_eq!(restored.limits(), (140.0, 420.0));
    }
}

use crate::theme::Color;

pub use nana_ui_core::{ButtonKind, CardKind};

const SEGMENTED_CONTROL_BORDER_WIDTH: f32 = 1.0;
const SEGMENTED_CONTROL_PADDING: f32 = 2.0;
pub const SEGMENTED_CONTROL_INSET: f32 = SEGMENTED_CONTROL_BORDER_WIDTH + SEGMENTED_CONTROL_PADDING;

/// Optional paint overrides for a button surface.
///
/// When set, these replace the corresponding `ButtonKind` theme defaults for
/// the Active (and Disabled-faded) surface. Hover / Pressed still follow kind
/// interaction colors so Ghost toolbar icons keep hover feedback.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ButtonPaintOverride {
    pub background: Option<Color>,
    pub text_color: Option<Color>,
    pub border_radius: Option<f32>,
    pub border_width: Option<f32>,
    pub border_color: Option<Color>,
}

impl ButtonPaintOverride {
    pub fn is_empty(self) -> bool {
        self.background.is_none()
            && self.text_color.is_none()
            && self.border_radius.is_none()
            && self.border_width.is_none()
            && self.border_color.is_none()
    }
}

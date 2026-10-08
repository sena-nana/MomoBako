//! Single-line widths of control-chrome text: key-cap badges, menu hints,
//! palette shortcuts, detail and value columns, legends, gutters.
//!
//! Chrome that sizes a box to its text measures through the same `nana-text`
//! engine that shapes the world's text nodes (Issue #99), so a box and the
//! run painted into it agree. [`estimated_text_width`] is what is left for a
//! world no engine has shaped yet — unit fixtures, the first projection.

use nana_text::{SharedTextEngine, TextEngine as _, TextSource};

use crate::text_node::{nana_text_constraints, nana_text_style, text_kind, text_metrics_of_layout};
use crate::{ComputedStyle, TextHorizontalAlignment, TextShapeConstraints};

/// Rough advance estimate for a world without an engine: ASCII glyphs are
/// narrow, every other script gets a full em.
pub(crate) fn estimated_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| {
            if ch.is_ascii() {
                font_size * 0.62
            } else {
                font_size
            }
        })
        .sum::<f32>()
        .max(font_size)
}

/// Measures chrome text the way the scene will paint it: in the owning node's
/// family, letter spacing and features, at the region's own size and weight.
#[derive(Clone, Copy)]
pub(crate) struct ChromeTextMeasure<'a> {
    engine: Option<&'a SharedTextEngine>,
    base: Option<&'a ComputedStyle>,
}

impl<'a> ChromeTextMeasure<'a> {
    /// No engine: every width is [`estimated_text_width`].
    #[cfg(test)]
    pub(crate) const ESTIMATE: ChromeTextMeasure<'static> = ChromeTextMeasure {
        engine: None,
        base: None,
    };

    pub(crate) fn new(
        engine: Option<&'a SharedTextEngine>,
        base: Option<&'a ComputedStyle>,
    ) -> Self {
        Self { engine, base }
    }

    /// The advance of `text` on one unwrapped line, rounded up to a whole
    /// pixel so a region exactly this wide never ellipsizes its own run on
    /// float noise.
    pub(crate) fn width(&self, text: &str, font_size: f32, font_weight: Option<u16>) -> f32 {
        let Some(engine) = self.engine else {
            return estimated_text_width(text, font_size);
        };
        let mut style = self.base.cloned().unwrap_or_default();
        style.font_size = font_size;
        style.font_weight = font_weight;
        let constraints = TextShapeConstraints::default();
        // Chrome runs across a line even in a vertical editor (#59), the same
        // as `NanaTextEngineShaper::horizontal_offset`.
        let nana_constraints = nana_text::TextConstraints {
            writing_mode: nana_ui_core::WritingModeSpec::HorizontalTb,
            ..nana_text_constraints(&style, &constraints, TextHorizontalAlignment::Start)
        };
        let layout = nana_text::lock_text_engine(engine).layout(
            text_kind(&constraints),
            &TextSource::new(text),
            &nana_text_style(&style),
            &nana_constraints,
            &mut nana_text::TextWorkCounters::default(),
        );
        // A line's width leaves out whitespace hanging at its end, but a
        // region that another run follows — a signature prefix ending in
        // ", " — has to reserve it. The end caret stands past it.
        let end = layout
            .caret_geometry(nana_text::CaretPosition::new(
                text.len(),
                nana_text::Affinity::Downstream,
                0,
            ))
            .map_or(0.0, |caret| caret.x_px);
        let width = text_metrics_of_layout(&layout).width.max(end);
        if width.is_finite() {
            width.max(0.0).ceil()
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_classifies_ascii_and_cjk_glyphs() {
        let size = nana_ui_core::type_scale::HINT;
        let ascii = estimated_text_width("Shortcut", size);
        let cjk = estimated_text_width("复制", size);
        // 8 ASCII glyphs at 0.62em; 2 CJK glyphs at a full em each, not
        // inflated by UTF-8 byte counts.
        assert!(
            (ascii - 8.0 * 0.62 * size).abs() < 0.5,
            "ascii estimate {ascii}"
        );
        assert!((cjk - 2.0 * size).abs() < 0.5, "cjk estimate {cjk}");
        assert_eq!(ChromeTextMeasure::ESTIMATE.width("复制", size, None), cjk);
    }
}

//! rich-text geometry from committed node data.

use super::*;
use unicode_segmentation::UnicodeSegmentation;

pub(in crate::world) fn selectable_text_regions(
    content: LayoutBox,
    text: &Arc<str>,
    style: &ComputedStyle,
    palette: &SemanticPalette,
) -> (crate::ComponentTextRegion, [f32; 4]) {
    let region = crate::ComponentTextRegion {
        bounds: content,
        content: Arc::clone(text).into(),
        color: Some(style.color.unwrap_or_else(|| palette.text.as_rgba_array())),
        font_size: style.font_size,
        font_weight: style.font_weight,
    };
    (region, palette.accent_soft.as_rgba_array())
}

impl UiWorld {
    /// The horizontal `nana-text` layout a `SelectableRichText` showing
    /// `text` was measured with and is painted from. `None` when a host shaper
    /// measured it, it is a vertical column, or it holds other text.
    fn selectable_rich_text_retained(
        &self,
        id: StableNodeId,
        text: &str,
    ) -> Option<&Arc<nana_text::TextLayout>> {
        let (_, layout) = self.text_layout(id)?;
        (!layout.is_vertical() && self.text(id) == Some(text)).then_some(layout)
    }

    /// Hit geometry of a `SelectableRichText` at content box `bounds`, where
    /// its text is painted; the fixed-advance estimate without a retained
    /// layout.
    pub(crate) fn selectable_rich_text_layout(
        &self,
        id: StableNodeId,
        view: &crate::SelectableRichText,
        bounds: LayoutBox,
    ) -> crate::rich_text::RichTextGeometry {
        match self.selectable_rich_text_retained(id, &view.plain_text()) {
            Some(layout) => {
                crate::rich_text::layout_rich_spans_shaped(view.spans(), bounds, layout)
            }
            None => view.layout(bounds),
        }
    }

    /// Highlight of a `SelectableRichText` selection (grapheme offsets): the
    /// selected part of each painted line; the whole content box without a
    /// retained layout.
    pub(in crate::world) fn selectable_rich_text_highlights(
        &self,
        id: StableNodeId,
        content: LayoutBox,
        text: &str,
        selection: Option<(usize, usize)>,
    ) -> Vec<LayoutBox> {
        let Some((anchor, focus)) = selection else {
            return Vec::new();
        };
        let Some(layout) = self.selectable_rich_text_retained(id, text) else {
            return vec![content];
        };
        let byte = |grapheme: usize| {
            text.grapheme_indices(true)
                .nth(grapheme)
                .map_or(text.len(), |(offset, _)| offset)
        };
        layout
            .selection_rects(byte(anchor.min(focus))..byte(anchor.max(focus)))
            .into_iter()
            .map(|rect| crate::rich_text::text_rect_at(content, rect))
            .collect()
    }

    /// Markdown geometry measured by the engine that shaped this world, the
    /// one the painter draws the runs with. A world no engine has shaped (a
    /// host shaper without one) keeps the em estimate.
    pub(crate) fn markdown_layout(
        &self,
        id: StableNodeId,
        blocks: &[crate::MarkdownBlock],
        bounds: LayoutBox,
    ) -> crate::rich_text::MarkdownGeometry {
        let (Some(style), Some(engine)) =
            (self.computed_style(id), self.paint_text_engine.as_ref())
        else {
            return crate::rich_text::layout_markdown(blocks, bounds);
        };
        crate::rich_text::layout_markdown_measured(
            blocks,
            bounds,
            Some(&|text, size, weight, italic| {
                let mut measured = style.clone();
                measured.font_size = size;
                measured.font_weight = Some(weight);
                measured.italic = italic;
                // A markdown run is one unwrapped horizontal line.
                measured.writing_mode = nana_ui_core::WritingModeSpec::HorizontalTb;
                crate::text_engine_shaper::grapheme_advances(engine, text, &measured)
            }),
        )
    }
}

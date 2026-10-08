//! Backend-neutral drawings for Markdown mathematics and diagrams.
use crate::{LayoutBox, MarkdownBlock, MarkdownBlockKind};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum MarkdownDrawingCommand {
    Image {
        bounds: LayoutBox,
        source: Arc<str>,
    },
    Svg {
        bounds: LayoutBox,
        source: Arc<str>,
    },
    Text {
        bounds: LayoutBox,
        text: Arc<str>,
        size: f32,
        weight: u16,
        italic: bool,
        line_through: bool,
        underline: bool,
        code: bool,
    },
    Line {
        points: Vec<[f32; 2]>,
        width: f32,
    },
    Box {
        bounds: LayoutBox,
        radius: f32,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarkdownDrawing {
    pub width: f32,
    pub height: f32,
    pub commands: Vec<MarkdownDrawingCommand>,
}

pub(crate) fn text_advance(grapheme: &str, size: f32) -> f32 {
    if grapheme.is_ascii() {
        grapheme.len() as f32 * size * 0.6
    } else {
        size
    }
}

pub(crate) fn text_style(kind: MarkdownBlockKind) -> (f32, u16) {
    match kind {
        MarkdownBlockKind::Heading(level) => (
            (24.0 - f32::from(level.saturating_sub(1)) * 1.5).max(16.0) * 0.85,
            600,
        ),
        _ => (13.0, 400),
    }
}

impl MarkdownDrawing {
    fn text(&mut self, x: f32, y: f32, text: impl Into<Arc<str>>, size: f32, weight: u16) {
        let text = text.into();
        use unicode_segmentation::UnicodeSegmentation;
        let width = text
            .graphemes(true)
            .map(|grapheme| text_advance(grapheme, size))
            .sum::<f32>();
        self.commands.push(MarkdownDrawingCommand::Text {
            bounds: LayoutBox {
                x,
                y,
                width: width.max(1.0),
                height: size * 1.35,
            },
            text,
            size,
            weight,
            italic: false,
            line_through: false,
            underline: false,
            code: false,
        });
    }
    fn line(&mut self, points: Vec<[f32; 2]>, width: f32) {
        self.commands
            .push(MarkdownDrawingCommand::Line { points, width });
    }
    fn append(&mut self, other: &Self, x: f32, y: f32, scale: f32) {
        for command in &other.commands {
            self.commands.push(match command {
                MarkdownDrawingCommand::Image { bounds, source } => MarkdownDrawingCommand::Image {
                    bounds: transform_box(*bounds, x, y, scale),
                    source: source.clone(),
                },
                MarkdownDrawingCommand::Svg { bounds, source } => MarkdownDrawingCommand::Svg {
                    bounds: transform_box(*bounds, x, y, scale),
                    source: source.clone(),
                },
                MarkdownDrawingCommand::Text {
                    bounds,
                    text,
                    size,
                    weight,
                    italic,
                    line_through,
                    underline,
                    code,
                } => MarkdownDrawingCommand::Text {
                    bounds: transform_box(*bounds, x, y, scale),
                    text: text.clone(),
                    size: size * scale,
                    weight: *weight,
                    italic: *italic,
                    line_through: *line_through,
                    underline: *underline,
                    code: *code,
                },
                MarkdownDrawingCommand::Line { points, width } => MarkdownDrawingCommand::Line {
                    points: points
                        .iter()
                        .map(|p| [x + p[0] * scale, y + p[1] * scale])
                        .collect(),
                    width: width * scale,
                },
                MarkdownDrawingCommand::Box { bounds, radius } => MarkdownDrawingCommand::Box {
                    bounds: transform_box(*bounds, x, y, scale),
                    radius: radius * scale,
                },
            });
        }
    }
    pub(crate) fn placed(&self, bounds: LayoutBox) -> Self {
        let scale = (bounds.width.max(1.0) / self.width.max(1.0)).min(1.0);
        let mut output = Self {
            width: self.width * scale,
            height: self.height * scale,
            ..Self::default()
        };
        output.append(
            self,
            bounds.x + (bounds.width - output.width).max(0.0) * 0.5,
            bounds.y,
            scale,
        );
        output
    }
}
fn transform_box(b: LayoutBox, x: f32, y: f32, scale: f32) -> LayoutBox {
    LayoutBox {
        x: x + b.x * scale,
        y: y + b.y * scale,
        width: b.width * scale,
        height: b.height * scale,
    }
}

const THEME_INK: &str = "#010203";

fn cached(
    key: String,
    render: impl FnOnce() -> Option<MarkdownDrawing>,
) -> Option<MarkdownDrawing> {
    use std::collections::VecDeque;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<VecDeque<(String, Option<MarkdownDrawing>)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Ok(cache) = cache.lock()
        && let Some((_, drawing)) = cache.iter().find(|(entry, _)| entry == &key)
    {
        return drawing.clone();
    }
    let drawing = render();
    if let Ok(mut cache) = cache.lock() {
        cache.push_back((key, drawing.clone()));
        while cache.len() > 200 {
            cache.pop_front();
        }
    }
    drawing
}

pub(crate) fn math(source: &str, size: f32) -> MarkdownDrawing {
    if source.len() > 16_384 {
        return math_fallback(source, size);
    }
    cached(format!("math:{size}:{source}"), || {
        let ast = ratex_parser::parse(source).ok()?;
        let mut options = ratex_layout::LayoutOptions::default();
        options.color = ratex_types::color::Color::new(1.0 / 255.0, 2.0 / 255.0, 3.0 / 255.0, 1.0);
        if size <= 13.0 {
            options.style = ratex_types::math_style::MathStyle::Text;
        }
        let layout = ratex_layout::layout(&ast, &options);
        let list = ratex_layout::to_display_list(&layout);
        let options = ratex_svg::SvgOptions {
            font_size: f64::from(size),
            padding: 2.0,
            stroke_width: 1.0,
            embed_glyphs: true,
            ..Default::default()
        };
        let svg = ratex_svg::render_to_svg_with_color_syntax(
            &list,
            &options,
            ratex_svg::SvgColorSyntax::Rgb,
        );
        let width = (list.width * size as f64 + 4.0) as f32;
        let height = ((list.height + list.depth) * size as f64 + 4.0) as f32;
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return None;
        }
        Some(MarkdownDrawing {
            width,
            height,
            commands: vec![MarkdownDrawingCommand::Svg {
                bounds: LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height,
                },
                source: svg.into(),
            }],
        })
    })
    .unwrap_or_else(|| math_fallback(source, size))
}

fn math_fallback(source: &str, size: f32) -> MarkdownDrawing {
    let mut d = MarkdownDrawing::default();
    d.text(0.0, 0.0, source, size, 400);
    d.width = source.chars().count() as f32 * size * 0.6;
    d.height = size * 1.35;
    d
}

fn svg_dimensions(svg: &str) -> Option<(f32, f32)> {
    let root = svg.split('>').next()?;
    if let Some(rest) = root.split("viewBox=\"").nth(1) {
        let values = rest
            .split('"')
            .next()?
            .split_whitespace()
            .map(str::parse::<f32>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        if values.len() == 4 && values[2] > 0.0 && values[3] > 0.0 {
            return Some((values[2], values[3]));
        }
    }
    let value = |name: &str| -> Option<f32> {
        root.split(&format!("{name}=\""))
            .nth(1)?
            .split('"')
            .next()?
            .trim_end_matches("px")
            .parse()
            .ok()
    };
    Some((value("width")?, value("height")?))
}

pub(crate) fn mermaid(source: &str) -> Option<MarkdownDrawing> {
    if source.len() > 128_000 {
        return None;
    }
    cached(format!("mermaid:{source}"), || {
        let mut theme = mermaid_svg::Theme::neutral();
        theme.fg = THEME_INK.into();
        theme.fg_muted = THEME_INK.into();
        theme.bg = "transparent".into();
        theme.actor_fill = "transparent".into();
        theme.actor_stroke = THEME_INK.into();
        theme.lifeline = THEME_INK.into();
        theme.arrow_stroke = THEME_INK.into();
        theme.flow_node_fill = "transparent".into();
        theme.flow_node_stroke = THEME_INK.into();
        theme.flow_edge_stroke = THEME_INK.into();
        theme.flow_label_bg = "transparent".into();
        let svg = mermaid_svg::render_with(source, &theme).ok()?;
        let (width, height) = svg_dimensions(&svg)?;
        Some(MarkdownDrawing {
            width,
            height,
            commands: vec![MarkdownDrawingCommand::Svg {
                bounds: LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height,
                },
                source: svg.into(),
            }],
        })
    })
}

pub(crate) fn block_drawing(block: &MarkdownBlock) -> Option<MarkdownDrawing> {
    match block {
        MarkdownBlock::DisplayMath(source) => Some(math(source, 19.0)),
        MarkdownBlock::Mermaid(source) => mermaid(source),
        _ => None,
    }
}

pub(crate) fn document(blocks: &[MarkdownBlock], bounds: LayoutBox) -> MarkdownDrawing {
    let geometry = crate::rich_text::layout_markdown(blocks, bounds);
    document_with_geometry(blocks, bounds, &geometry)
}

pub(crate) fn document_with_geometry(
    blocks: &[MarkdownBlock],
    bounds: LayoutBox,
    geometry: &crate::rich_text::MarkdownGeometry,
) -> MarkdownDrawing {
    let mut output = MarkdownDrawing {
        width: bounds.width,
        height: geometry.bounds.height,
        ..Default::default()
    };
    for (block, g) in blocks.iter().zip(&geometry.blocks) {
        if let Some(drawing) = block_drawing(block) {
            output.commands.extend(drawing.placed(g.bounds).commands);
            continue;
        }
        if matches!(block, MarkdownBlock::Rule) {
            output.line(
                vec![
                    [g.bounds.x, g.bounds.y],
                    [g.bounds.x + g.bounds.width, g.bounds.y],
                ],
                1.0,
            );
            continue;
        }
        let (size, weight) = match block {
            MarkdownBlock::Text { kind, .. } => text_style(*kind),
            _ => (13.0, 400),
        };
        let mut start = 0;
        while start < g.graphemes.len() {
            let first = &g.graphemes[start];
            let mut end = start + 1;
            while end < g.graphemes.len()
                && g.graphemes[end].span_index == first.span_index
                && g.graphemes[end].bounds.y == first.bounds.y
            {
                end += 1
            }
            let text = g.graphemes[start..end]
                .iter()
                .map(|v| v.grapheme.as_ref())
                .collect::<String>();
            let span = match block {
                MarkdownBlock::Text { spans, .. } => spans.get(first.span_index),
                MarkdownBlock::Table(table) => table
                    .header
                    .iter()
                    .chain(table.rows.iter().flatten())
                    .flatten()
                    .nth(first.span_index),
                _ => None,
            };
            if let Some(resource) = span
                .and_then(|span| span.image_resource.as_ref())
                .filter(|resource| resource.width > 0 && resource.height > 0)
            {
                let last = &g.graphemes[end - 1];
                output.commands.push(MarkdownDrawingCommand::Image {
                    bounds: LayoutBox {
                        x: first.bounds.x,
                        y: first.bounds.y,
                        width: last.bounds.x + last.bounds.width - first.bounds.x,
                        height: first.bounds.height,
                    },
                    source: resource.source.clone(),
                });
            } else if span.is_some_and(|span| span.inline_math) {
                let drawing = math(&text, 13.0);
                output.append(
                    &drawing,
                    first.bounds.x,
                    first.bounds.y,
                    (g.bounds.width / drawing.width.max(1.0)).min(1.0),
                )
            } else {
                output.text(
                    first.bounds.x,
                    first.bounds.y,
                    text,
                    size,
                    if span.is_some_and(|s| s.strong) {
                        700
                    } else {
                        weight
                    },
                );
                if let Some(MarkdownDrawingCommand::Text {
                    bounds,
                    italic,
                    line_through,
                    underline,
                    code,
                    ..
                }) = output.commands.last_mut()
                {
                    *italic = span.is_some_and(|s| s.emphasis);
                    *line_through = span.is_some_and(|s| s.strikethrough);
                    *underline = span.is_some_and(|s| s.link.is_some());
                    *code =
                        span.is_some_and(|s| s.code) || matches!(block, MarkdownBlock::Code { .. });
                    bounds.width = g.graphemes[start..end]
                        .iter()
                        .map(|item| item.bounds.x + item.bounds.width)
                        .fold(first.bounds.x, f32::max)
                        - first.bounds.x;
                }
            }
            start = end;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, DocumentId, LayoutViewport, NativeMarkdown, Stack};

    #[test]
    fn inline_styles_reach_the_drawing_commands() {
        let markdown = NativeMarkdown::from_source(
            "*italic* ~~strike~~ `code` [link](https://example.test) **bold**",
        );
        let drawing = markdown.drawing(LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 600.0,
            height: 100.0,
        });
        for (needle, flag) in [
            ("italic", 0),
            ("strike", 1),
            ("code", 2),
            ("link", 3),
            ("bold", 4),
        ] {
            assert!(drawing.commands.iter().any(|command| matches!(command, MarkdownDrawingCommand::Text { text, italic, line_through, code, underline, weight, .. } if text.as_ref() == needle && [*italic, *line_through, *code, *underline, *weight == 700][flag])));
        }
    }

    #[test]
    fn chinese_text_and_inline_formula_use_one_advance_contract() {
        let markdown = NativeMarkdown::from_source("行内公式 $E=mc^2$。");
        let bounds = LayoutBox {
            x: 317.0,
            y: 280.0,
            width: 700.0,
            height: 100.0,
        };
        let drawing = markdown.drawing(bounds);
        let MarkdownDrawingCommand::Text {
            bounds: before,
            text,
            ..
        } = &drawing.commands[0]
        else {
            panic!("leading text");
        };
        let MarkdownDrawingCommand::Svg {
            bounds: formula, ..
        } = &drawing.commands[1]
        else {
            panic!("formula");
        };
        let MarkdownDrawingCommand::Text { bounds: after, .. } = &drawing.commands[2] else {
            panic!("trailing text");
        };
        assert_eq!(text.as_ref(), "行内公式 ");
        assert!(
            before.width >= 4.0 * 13.0,
            "CJK text cannot fit in fixed 8px cells: {before:?}"
        );
        assert!(
            (formula.x - before.x - before.width).abs() < 0.001,
            "{before:?} {formula:?}"
        );
        assert!(
            (after.x - formula.x - formula.width).abs() < 0.001,
            "{formula:?} {after:?}"
        );
        let narrow = markdown.layout(LayoutBox {
            width: before.width + formula.width - 1.0,
            ..bounds
        });
        let inline = narrow.blocks[0]
            .graphemes
            .iter()
            .find(|g| g.span_index == 1)
            .unwrap();
        assert!(
            inline.bounds.y > formula.y,
            "wrap must use same visual advance"
        );
    }

    #[test]
    fn shaped_advances_determine_formula_origin_and_drawing_bounds() {
        let markdown = NativeMarkdown::from_source("Wi 中文 $x$ end");
        let bounds = LayoutBox {
            x: 10.0,
            y: 0.0,
            width: 400.0,
            height: 100.0,
        };
        let measure = |value: &str, _: f32, _: u16, _: bool| {
            use unicode_segmentation::UnicodeSegmentation as _;
            value
                .graphemes(true)
                .map(|grapheme| match grapheme {
                    "W" => 12.0,
                    "i" => 3.0,
                    " " => 4.0,
                    "中" | "文" => 13.0,
                    _ => 7.0,
                })
                .collect::<Vec<f32>>()
        };
        let geometry =
            crate::rich_text::layout_markdown_measured(markdown.blocks(), bounds, Some(&measure));
        let drawing = document_with_geometry(markdown.blocks(), bounds, &geometry);
        let MarkdownDrawingCommand::Text { bounds: before, .. } = &drawing.commands[0] else {
            panic!("text");
        };
        let MarkdownDrawingCommand::Svg {
            bounds: formula, ..
        } = &drawing.commands[1]
        else {
            panic!("math");
        };
        assert!((before.width - 49.0).abs() < 0.001);
        assert!((formula.x - 59.0).abs() < 0.001);
        assert_eq!(geometry.blocks[0].graphemes[0].bounds.width, 12.0);
        assert_eq!(geometry.blocks[0].graphemes[1].bounds.width, 3.0);
    }

    #[test]
    fn formulas_have_native_glyph_outlines_and_structural_extents() {
        for source in [
            r"E=mc^2",
            r"\frac{1}{2}",
            r"\sqrt{x_1^2+x_2^2}",
            r"\sum_{i=0}^{n}\frac{1}{i^2}",
            r"\begin{pmatrix}a&b\\c&d\end{pmatrix}",
            r"\int_0^\infty e^{-x}\,dx",
        ] {
            let drawing = math(source, 19.0);
            let Some(MarkdownDrawingCommand::Svg {
                source: svg,
                bounds,
            }) = drawing.commands.first()
            else {
                panic!("formula did not typeset: {source}")
            };
            assert!(
                svg.contains("<path"),
                "formula must use embedded glyph outlines"
            );
            assert!(bounds.width > 4.0 && bounds.height > 10.0);
        }
        assert!(math(r"\frac{1}{2}", 19.0).height > math("x", 19.0).height * 1.5);
        assert!(math("x^2", 19.0).height > math("x", 19.0).height);
    }

    #[test]
    fn all_mermaid_diagram_families_produce_measured_svg() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mermaid");
        let mut count = 0;
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("mmd") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let drawing =
                mermaid(&source).unwrap_or_else(|| panic!("cannot render {}", path.display()));
            assert!(
                drawing.width.is_finite()
                    && drawing.width > 0.0
                    && drawing.height.is_finite()
                    && drawing.height > 0.0
            );
            assert!(matches!(
                drawing.commands.first(),
                Some(MarkdownDrawingCommand::Svg { .. })
            ));
            count += 1;
        }
        assert!(count >= 23);
    }

    #[test]
    fn historical_math_boundaries_tables_and_copy_are_preserved() {
        let markdown = NativeMarkdown::from_source(
            "行内 \\(E=mc^2\\) 与 \\(\\frac{1}{2}\\)。\n\n| 比例 |\n| --- |\n| \\(\\frac{1}{2}\\) |\n\n$$\na^2+b^2=c^2\n$$\n\n```mermaid\ngraph TD\nA-->B\n```",
        );
        let bounds = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 430.0,
            height: 2000.0,
        };
        let drawing = markdown.drawing(bounds);
        assert_eq!(
            drawing
                .commands
                .iter()
                .filter(|command| matches!(command, MarkdownDrawingCommand::Svg { .. }))
                .count(),
            5
        );
        assert!(markdown.pointer_down(-1.0, -1.0, bounds));
        markdown.pointer_up(430.0, drawing.height + 20.0, bounds);
        let copied = markdown.copy_snapshot().unwrap().text;
        assert!(copied.contains(r"\frac{1}{2}") && copied.contains("graph TD\nA-->B"));
        assert_eq!(copied, markdown.plain_text());
    }

    #[test]
    fn formula_and_diagram_height_participate_in_real_layout() {
        let id = DocumentId::new(81).unwrap();
        let context = &mut AppContext::new();
        let root = context.create_component(id, Stack::column(0.0)).unwrap();
        let markdown = NativeMarkdown::from_source(
            "$$\\frac{1}{\\sqrt{x^2+1}}$$\n\n```mermaid\nflowchart TD\nA-->B-->C\n```",
        );
        let entity = context
            .create_detached_component(id, markdown.clone())
            .unwrap();
        context.append_child(root, entity).unwrap();
        context
            .layout_document(id, LayoutViewport::new(430.0, 1200.0))
            .unwrap();
        let bounds = context.world().layout_box(entity.stable_id()).unwrap();
        assert!((bounds.height - markdown.layout(bounds).bounds.height).abs() < 0.5);
        assert!(
            bounds.height > 150.0,
            "diagram may not collapse into one text line"
        );
    }
    #[test]
    fn image_resolution_updates_all_occurrences_and_uses_the_same_geometry_for_paint_and_input() {
        let mut markdown = NativeMarkdown::from_source(
            "[ordinary](image.png) ![picture](image.png)\n\n| image |\n| --- |\n| ![table](image.png) |\n\n![](image.png)",
        );
        let bounds = LayoutBox {
            x: 30.0,
            y: 40.0,
            width: 100.0,
            height: 800.0,
        };
        assert!(!markdown.resolve_image("image.png", "data:image/png;base64,test", 0, 20));
        assert!(markdown.resolve_image("image.png", "data:image/png;base64,test", 200, 80));
        assert!(!markdown.resolve_image("image.png", "data:image/png;base64,test", 200, 80));
        let drawing = markdown.drawing(bounds);
        let images = drawing
            .commands
            .iter()
            .filter_map(|command| match command {
                MarkdownDrawingCommand::Image { bounds, .. } => Some(*bounds),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(images.len(), 3);
        for image in images {
            assert!(image.width <= bounds.width && image.height > 0.0);
            assert!((image.width / image.height - 2.5).abs() < 0.001);
            let (x, y) = (image.x + image.width * 0.5, image.y + image.height * 0.5);
            markdown.clear_selection();
            assert!(markdown.pointer_down(x, y, bounds));
            assert!(
                matches!(markdown.pointer_up(x, y, bounds), Some(crate::RichTextEvent::ImageActivated(image)) if image.source == "image.png")
            );
        }
        let geometry = markdown.layout(bounds);
        let link = &geometry.blocks[0].graphemes[0].bounds;
        let (x, y) = (link.x + link.width * 0.5, link.y + link.height * 0.5);
        markdown.clear_selection();
        markdown.pointer_down(x, y, bounds);
        assert!(
            matches!(markdown.pointer_up(x, y, bounds), Some(crate::RichTextEvent::LinkActivated(link)) if link.as_ref() == "image.png")
        );
    }
}

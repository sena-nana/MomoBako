//! Component geometry derived from committed Runtime data.

#[cfg(feature = "calendar")]
mod calendar;
#[cfg(feature = "calendar")]
pub(super) use calendar::*;

#[cfg(feature = "charts")]
mod charts;
#[cfg(feature = "charts")]
pub(super) use charts::*;

#[cfg(feature = "controls")]
mod controls;
#[cfg(feature = "controls")]
use controls::*;

#[cfg(feature = "rich-text")]
mod rich_text;
#[cfg(feature = "rich-text")]
use rich_text::*;

#[cfg(feature = "graph-canvas")]
mod graph_canvas;
#[cfg(feature = "graph-canvas")]
pub(super) use graph_canvas::*;

#[cfg(feature = "image-viewer")]
mod image_viewer;
#[cfg(feature = "image-viewer")]
use image_viewer::*;

use super::*;

/// Diameter of a status dot, as a fraction of the slot reserved for it.
///
/// `StatusBadge` and `ValidationMessage` both wrote this as
/// `indicator_slot * 10.0 / 24.0`, where nothing in either arm is 24: the
/// denominator was the icon box the ratio was originally measured against, and
/// it survived the move to a spacing-step slot. Two copies of a phantom
/// denominator is how the two badges drift apart.
const STATUS_DOT_FRACTION: f32 = 10.0 / 24.0;

/// Block extent of a progress bar.
const PROGRESS_GIRTH: f32 = nana_ui_core::space::SM;

/// Extent of the glyph an empty state leads with. Composed, the way §1.5
/// composed the other off-ladder sizes: one page-tight step plus the smallest.
const EMPTY_STATE_ICON: f32 = nana_ui_core::space::PAGE_TIGHT + nana_ui_core::space::XXS;

pub(super) fn key_capture_geometry(
    content: LayoutBox,
    recording: bool,
    style_model: StyleModelRef,
    measure: crate::text_width::ChromeTextMeasure<'_>,
) -> crate::ComponentGeometry {
    let palette = style_model;
    let label: Arc<str> = if recording {
        Arc::from("Recording")
    } else {
        Arc::from("Idle")
    };
    crate::ComponentGeometry::KeyCaptureLayer {
        badge: key_badge_region(content, &label, !recording, style_model, measure),
        background: Some(if recording {
            palette.color(SemanticColorRole::AccentSoft).as_rgba_array()
        } else {
            palette.color(SemanticColorRole::Subtle).as_rgba_array()
        }),
    }
}

pub(super) fn keymap_geometry(
    content: LayoutBox,
    style_model: StyleModelRef,
    measure: crate::text_width::ChromeTextMeasure<'_>,
) -> crate::ComponentGeometry {
    crate::ComponentGeometry::KeymapLayer {
        badge: key_badge_region(content, "Keymap", false, style_model, measure),
    }
}

pub(super) fn key_badge_region(
    origin: LayoutBox,
    label: &str,
    muted: bool,
    palette: StyleModelRef,
    measure: crate::text_width::ChromeTextMeasure<'_>,
) -> crate::ComponentTextRegion {
    let metrics = palette.metrics;
    let height = nana_ui_core::ControlSize::Small.height_in(metrics);
    const PAD: f32 = nana_ui_core::space::MD;
    // Four of the widest spacing step: the narrowest badge still has to read
    // as a key cap rather than as a letter with a box round it.
    const MIN_WIDTH: f32 = nana_ui_core::space::XXXL * 4.0;
    let font_size = nana_ui_core::type_scale::META;
    const FONT_WEIGHT: u16 = 600;
    crate::ComponentTextRegion {
        bounds: LayoutBox {
            x: origin.x,
            y: origin.y,
            width: (measure.width(label, font_size, Some(FONT_WEIGHT)) + PAD * 2.0).max(MIN_WIDTH),
            height: height.min(origin.height.max(height)),
        },
        content: crate::TextValue::from(label),
        color: Some(if muted {
            palette.color(SemanticColorRole::Muted).as_rgba_array()
        } else {
            palette.color(SemanticColorRole::Text).as_rgba_array()
        }),
        font_size,
        font_weight: Some(FONT_WEIGHT),
    }
}

#[cfg(feature = "charts")]
pub(super) fn area_under_polyline(points: &[[f32; 2]], baseline: f32) -> Vec<LayoutBox> {
    const STRIP: f32 = nana_ui_core::space::XXS;
    let mut strips = Vec::new();
    for pair in points.windows(2) {
        let [x0, y0] = pair[0];
        let [x1, y1] = pair[1];
        let span = x1 - x0;
        if !span.is_finite() || span.abs() < f32::EPSILON {
            continue;
        }
        let left = x0.min(x1);
        let right = x0.max(x1);
        let mut x = left;
        while x < right {
            let width = STRIP.min(right - x);
            let mid = x + width * 0.5;
            let t = (mid - x0) / span;
            let y = y0 + (y1 - y0) * t;
            let top = y.min(baseline);
            let height = (baseline - top).max(0.0);
            if height > 0.0 {
                strips.push(LayoutBox {
                    x,
                    y: top,
                    width,
                    height,
                });
            }
            x += STRIP;
        }
    }
    strips
}

impl UiWorld {
    pub(super) fn derive_component_geometry(
        &self,
        id: StableNodeId,
        visual: &StandardVisual,
        style: &ComputedStyle,
    ) -> Option<crate::ComponentGeometry> {
        let (bounds, source) = {
            let node = self.nodes.get(id)?;
            (node.layout, node.style.clone())
        };
        let padding = self.used_layout_padding(id);
        let border = source.layout.resolved_border_width();
        let content = self.component_content_box(id)?;
        // Both branches now answer "what colour is this node's text, in the
        // state it is in": the primary from the resolved style, the secondary
        // from the `foreground_secondary` role the same state machine picks.
        // The secondary used to be `palette.muted` read straight off the
        // palette, which is how a focused row ended up with a grey detail line
        // on a near-solid `focus_surface` fill.
        let secondary = self.secondary_text_color(id);
        let text_region = |bounds, content: Arc<str>, muted: bool, size: f32, weight| {
            crate::ComponentTextRegion {
                bounds,
                content: content.into(),
                color: Some(if muted {
                    secondary
                } else {
                    style.color.unwrap_or_else(|| {
                        self.style_model
                            .color(SemanticColorRole::Text)
                            .as_rgba_array()
                    })
                }),
                font_size: size,
                font_weight: weight,
            }
        };
        match visual {
            StandardVisual::ModalFrame {
                title,
                description,
                body_text,
                kind,
                slots,
                ..
            } => {
                let presentation = self.nodes.modal_text(id).copied().unwrap_or_default();
                let has_close = slots.close_action.is_some();
                let has_footer = slots.footer.is_some() || !slots.actions.is_empty();
                let chrome = crate::overlay_surfaces::ModalChrome::measure(
                    *kind,
                    presentation.title,
                    presentation.description,
                    has_close,
                    has_footer,
                );
                let body_copy = presentation.body.map_or(0.0, |metrics| metrics.height);
                let body_slot = slots
                    .body
                    .and_then(|id| self.layout_box(id))
                    .map_or(0.0, |region| region.height);
                let body_gap = if body_copy > 0.0 && body_slot > 0.0 {
                    8.0
                } else {
                    0.0
                };
                let intrinsic_height = chrome.chrome_height(body_copy + body_gap + body_slot);
                let surface = crate::overlay_surfaces::modal_surface_bounds(
                    bounds,
                    *kind,
                    Some(intrinsic_height),
                );
                let LayoutBox { x, y, width, .. } = surface;
                let text_width = chrome.text_width(width, *kind, has_close);
                let body = chrome.body_box(surface);
                let text_block = presentation.title.height
                    + presentation.description.map_or(0.0, |metrics| {
                        crate::overlay_surfaces::MODAL_TITLE_DESC_GAP + metrics.height
                    });
                let title_y = match kind {
                    crate::ModalSurfaceKind::Drawer(_) => {
                        y + (chrome.header_height - text_block) / 2.0
                    }
                    _ => y + crate::overlay_surfaces::MODAL_HEADER_PAD_TOP,
                };
                // Was `if background.r > 0.5 { 0.28 } else { 0.45 }` — the
                // modal's lift decided by sniffing how bright the page is. It
                // is an elevation token now, so each theme states it and a
                // mid-grey background no longer picks the wrong branch.
                let overlay_shadow = self.theme.shadow(nana_ui_core::ElevationRole::Overlay);
                Some(crate::ComponentGeometry::ModalFrame {
                    scrim: bounds,
                    surface,
                    body,
                    title: text_region(
                        LayoutBox {
                            x: x + chrome.pad_x,
                            y: title_y,
                            width: text_width,
                            height: presentation.title.height,
                        },
                        Arc::clone(title),
                        false,
                        14.0,
                        Some(600),
                    ),
                    description: description.as_ref().map(|description| {
                        text_region(
                            LayoutBox {
                                x: x + chrome.pad_x,
                                y: title_y
                                    + presentation.title.height
                                    + crate::overlay_surfaces::MODAL_TITLE_DESC_GAP,
                                width: text_width,
                                height: presentation.description.unwrap_or_default().height,
                            },
                            Arc::clone(description),
                            true,
                            12.0,
                            None,
                        )
                    }),
                    body_text: body_text.as_ref().map(|message| {
                        text_region(
                            LayoutBox {
                                x: body.x,
                                y: body.y,
                                width: body.width,
                                height: presentation.body.unwrap_or_default().height,
                            },
                            Arc::clone(message),
                            false,
                            crate::overlay_surfaces::MODAL_BODY_TEXT_SIZE,
                            None,
                        )
                    }),
                    background: self
                        .style_model
                        .color(SemanticColorRole::Surface)
                        .as_rgba_array(),
                    border: [0.0; 4],
                    elevation: crate::ComponentElevation::from_shadow(overlay_shadow),
                })
            }
            StandardVisual::Button {
                label,
                loading,
                icon,
                trailing_icon,
                icon_size,
                icon_gap,
                ..
            } => {
                // One centred group: [leading glyph] label [trailing glyph].
                // The label gives up width first; the glyphs keep theirs.
                let glyph = icon_size.min(content.height).min(content.width).max(0.0);
                let has_leading = *loading || icon.is_some();
                let extent = if has_leading { glyph } else { 0.0 };
                let trailing_extent = if trailing_icon.is_some() { glyph } else { 0.0 };
                let gap = if has_leading && (!label.is_empty() || trailing_icon.is_some()) {
                    icon_gap.min((content.width - extent).max(0.0))
                } else {
                    0.0
                };
                let trailing_gap = if trailing_icon.is_some() && !label.is_empty() {
                    *icon_gap
                } else {
                    0.0
                };
                let reserved = extent + gap + trailing_gap + trailing_extent;
                let label_width = self.text_metrics(id).map_or(0.0, |metrics| {
                    metrics.width.min((content.width - reserved).max(0.0))
                });
                let group_width = (label_width + reserved).min(content.width);
                let group_x = content.x + (content.width - group_width) / 2.0;
                let glyph_box = |x: f32, size: f32| LayoutBox {
                    x,
                    y: content.y + (content.height - size) / 2.0,
                    width: size,
                    height: size,
                };
                let leading = glyph_box(group_x, extent);
                let spinner = (*loading).then_some(leading);
                let label_x = group_x + extent + gap;
                let trailing = glyph_box(group_x + group_width - trailing_extent, trailing_extent);
                Some(crate::ComponentGeometry::Button {
                    icon: if *loading {
                        None
                    } else {
                        icon.map(|icon| (icon, leading))
                    },
                    trailing_icon: trailing_icon.map(|icon| (icon, trailing)),
                    label: text_region(
                        LayoutBox {
                            x: label_x,
                            y: content.y,
                            width: (group_x + group_width
                                - trailing_extent
                                - trailing_gap
                                - label_x)
                                .max(0.0),
                            height: content.height,
                        },
                        Arc::clone(label),
                        false,
                        style.font_size,
                        style.font_weight,
                    ),
                    spinner,
                    background: style.background,
                    border: style.border_color,
                    border_width: if style.border_color.is_some() {
                        source.layout.resolved_border_width()
                    } else {
                        0.0
                    },
                    focus_ring: None,
                })
            }
            StandardVisual::TextInput {
                size,
                invalid,
                steppers,
                ..
            } => {
                let presentation = self.nodes.text_input_presentation(id)?;
                // Fold gutters and sticky headers share the displayed logical-line
                // index. Counting every prefix separately makes many folds quadratic.
                let display_newlines: Vec<usize> = match visual {
                    StandardVisual::TextInput { folds, .. } if !folds.is_empty() => presentation
                        .display_value
                        .bytes()
                        .enumerate()
                        .filter_map(|(offset, byte)| (byte == b'\n').then_some(offset))
                        .collect(),
                    _ => Vec::new(),
                };
                let display_line_of =
                    |offset: usize| display_newlines.partition_point(|newline| *newline < offset);
                let accessibility = self.nodes.get(id).map(|n| &n.accessibility);
                let disabled = accessibility.is_some_and(|state| state.disabled);
                // A read-only field refuses every step too, so its spinner
                // halves are inert: drawn and hit as they behave.
                let inert = disabled || accessibility.is_some_and(|state| !state.editable);
                let steppers = steppers
                    .then(|| {
                        let band = (size.height_in(self.style_model.metrics) / 2.0)
                            .min(content.height / 2.0);
                        let width = size.indicator_size();
                        if band <= 0.0 || width <= 0.0 || content.width <= width {
                            return None;
                        }
                        let x = content.x + content.width - width;
                        let numeric = accessibility.map(|state| {
                            (
                                state.numeric_value.unwrap_or_default(),
                                state.numeric_minimum,
                                state.numeric_maximum,
                            )
                        });
                        let can_increment = !inert
                            && numeric.is_none_or(|(value, _, maximum)| {
                                maximum.is_none_or(|maximum| value < maximum)
                            });
                        let can_decrement = !inert
                            && numeric.is_none_or(|(value, minimum, _)| {
                                minimum.is_none_or(|minimum| value > minimum)
                            });
                        let active = self
                            .style_model
                            .color(SemanticColorRole::Muted)
                            .as_rgba_array();
                        let inert = self
                            .style_model
                            .color(SemanticColorRole::Faint)
                            .as_rgba_array();
                        Some(crate::NumberSteppers {
                            increment: LayoutBox {
                                x,
                                y: content.y + (content.height - band * 2.0) / 2.0,
                                width,
                                height: band,
                            },
                            decrement: LayoutBox {
                                x,
                                y: content.y + (content.height - band * 2.0) / 2.0 + band,
                                width,
                                height: band,
                            },
                            increment_color: if can_increment { active } else { inert },
                            decrement_color: if can_decrement { active } else { inert },
                            increment_enabled: can_increment,
                            decrement_enabled: can_decrement,
                            glyph_size: (width - 2.0).max(1.0),
                        })
                    })
                    .flatten();
                // Where the text sits in the field — the content box less the
                // steppers, the scroll, a single-line field's centring and its
                // inline-start edge — is the editor frame's, the same one a
                // pointer hit is resolved through.
                let frame = self.editor_frame(id)?;
                let content = frame.content;
                let focused = self.input.focused.get(&self.record(id).document) == Some(&id);
                let multiline = accessibility.is_some_and(|state| state.multiline);
                let line_height = frame.line;
                // Horizontal text space: `x` along the line, `y` across. A
                // single-line field has no block scroll of its own; its
                // centring is where its line top lands.
                let scroll_x = frame.inline_scroll;
                let scroll_y = if multiline { frame.block_scroll } else { 0.0 };
                let line_y = content.y - frame.block_scroll;
                let field_x = |offset: f32| content.x + offset - scroll_x;
                let (selection, preedit) = text_input_decorations(
                    presentation,
                    multiline,
                    content,
                    line_y,
                    line_height,
                    scroll_x,
                    scroll_y,
                );
                let caret = focused.then(|| {
                    caret_rule(
                        field_x(presentation.caret_x),
                        line_y + presentation.caret_y,
                        line_height,
                    )
                });
                // 附加多光标：与主光标同形，用主光标色的半透明变体区分；
                // 只随焦点出现（多行编辑器才有附加光标）。
                let caret_color = style.color.unwrap_or_else(|| {
                    self.style_model
                        .color(SemanticColorRole::Text)
                        .as_rgba_array()
                });
                let additional_caret_color = {
                    let mut color = caret_color;
                    color[3] *= 0.55;
                    color
                };
                let additional_carets = if focused {
                    presentation
                        .additional_carets
                        .iter()
                        .map(|(x, y)| caret_rule(field_x(*x), line_y + *y, line_height))
                        .collect()
                } else {
                    Vec::new()
                };
                let marker_color = |severity| match severity {
                    crate::TextDiagnosticSeverity::Error => self
                        .style_model
                        .color(SemanticColorRole::Danger)
                        .as_rgba_array(),
                    crate::TextDiagnosticSeverity::Warning => self
                        .style_model
                        .color(SemanticColorRole::Warning)
                        .as_rgba_array(),
                    crate::TextDiagnosticSeverity::Information
                    | crate::TextDiagnosticSeverity::Hint => self
                        .style_model
                        .color(SemanticColorRole::Muted)
                        .as_rgba_array(),
                };
                let diagnostic_markers = presentation
                    .diagnostic_marks
                    .iter()
                    .map(|mark| {
                        (
                            LayoutBox {
                                x: field_x(mark.rect.x),
                                y: content.y + mark.rect.y - scroll_y,
                                width: mark.rect.width,
                                height: mark.rect.height,
                            },
                            marker_color(mark.severity),
                        )
                    })
                    .collect();
                let diagnostic_labels = presentation
                    .diagnostic_labels
                    .iter()
                    .map(|label| crate::ComponentTextRegion {
                        bounds: LayoutBox {
                            x: field_x(label.rect.x),
                            y: content.y + label.rect.y - scroll_y,
                            width: label.rect.width,
                            height: label.rect.height,
                        },
                        content: crate::TextValue::from(label.text.as_str()),
                        color: Some(marker_color(label.severity)),
                        font_size: size.text_size(),
                        font_weight: None,
                    })
                    .collect();
                // 查找匹配：普通匹配用 accent 软色令牌，当前匹配用同一 accent
                // 色相加深（本地强调系数，与诊断条带的 2px 常量同级的局部约定）。
                let match_color = |current: bool| -> [f32; 4] {
                    if current {
                        let accent = self
                            .style_model
                            .color(SemanticColorRole::Accent)
                            .as_rgba_array();
                        [accent[0], accent[1], accent[2], 0.45]
                    } else {
                        self.style_model
                            .color(SemanticColorRole::AccentSoftHover)
                            .as_rgba_array()
                    }
                };
                let map_chip_box = |rect: LayoutBox| LayoutBox {
                    x: field_x(rect.x),
                    y: content.y + rect.y - scroll_y,
                    width: rect.width,
                    height: rect.height,
                };
                let atom_chips = presentation
                    .atom_chips
                    .iter()
                    .map(|chip| {
                        let mut mapped = chip.clone();
                        mapped.bounds = map_chip_box(chip.bounds);
                        mapped.close = map_chip_box(chip.close);
                        mapped.icon_bounds = map_chip_box(chip.icon_bounds);
                        mapped.label.bounds = map_chip_box(chip.label.bounds);
                        if mapped.label.color.is_none() {
                            mapped.label.color = Some(
                                self.style_model
                                    .color(SemanticColorRole::Text)
                                    .as_rgba_array(),
                            );
                        }
                        mapped.background = self
                            .style_model
                            .color(SemanticColorRole::Subtle)
                            .as_rgba_array();
                        mapped.border = self
                            .style_model
                            .color(SemanticColorRole::Border)
                            .as_rgba_array();
                        mapped
                    })
                    .collect();
                let match_markers = presentation
                    .match_marks
                    .iter()
                    .map(|mark| TextMatchMarker {
                        rect: LayoutBox {
                            x: field_x(mark.rect.x),
                            y: content.y + mark.rect.y - scroll_y,
                            width: mark.rect.width,
                            height: mark.rect.height,
                        },
                        color: match_color(mark.current),
                        current: mark.current,
                    })
                    .collect();
                // 颜色装饰 swatch：随滚动平移并按内容区纵向裁剪——只服务
                // 可见行，视口外的 swatch 不产生图元。颜色按宿主给定值直传
                // （半透明由绘制层常规 alpha 合成）。
                let swatch_markers = presentation
                    .swatch_marks
                    .iter()
                    .filter_map(|mark| {
                        let y = content.y + mark.rect.y - scroll_y;
                        (y + mark.rect.height >= content.y && y <= content.y + content.height)
                            .then_some((
                                LayoutBox {
                                    x: field_x(mark.rect.x),
                                    y,
                                    width: mark.rect.width,
                                    height: mark.rect.height,
                                },
                                mark.color,
                            ))
                    })
                    .collect();
                // 当前行条：聚焦多行且选区收起时，光标所在视觉行画低对比
                // 背景条（与选区层互斥，同用 slot 1 绘制层级）。
                let caret_line = if multiline && focused && presentation.selection.is_none() {
                    Some((
                        LayoutBox {
                            x: content.x,
                            y: line_y + presentation.caret_y - scroll_y,
                            width: content.width,
                            height: line_height,
                        },
                        self.style_model
                            .color(SemanticColorRole::Hover)
                            .as_rgba_array(),
                    ))
                } else {
                    None
                };
                // 括号匹配描边框：跟随聚焦光标；两端共用 accent 色。
                let bracket_markers = if focused {
                    presentation
                        .bracket_marks
                        .iter()
                        .map(|rect| {
                            (
                                LayoutBox {
                                    x: field_x(rect.x),
                                    y: content.y + rect.y - scroll_y,
                                    width: rect.width,
                                    height: rect.height,
                                },
                                self.style_model
                                    .color(SemanticColorRole::Accent)
                                    .as_rgba_array(),
                            )
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                // 出现高亮：淡底色填充（accent_soft，弱于查找匹配的
                // accent_soft_hover / accent@0.45），跟随聚焦光标。
                let occurrence_markers = if focused {
                    presentation
                        .occurrence_marks
                        .iter()
                        .map(|rect| {
                            (
                                LayoutBox {
                                    x: field_x(rect.x),
                                    y: content.y + rect.y - scroll_y,
                                    width: rect.width,
                                    height: rect.height,
                                },
                                self.style_model
                                    .color(SemanticColorRole::AccentSoft)
                                    .as_rgba_array(),
                            )
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                // 空白字符标记：静态结构标记（不随焦点变化），按视口裁剪
                // ——标记只服务可见行，视口外的空白不产生图元。
                let whitespace_color = self
                    .style_model
                    .color(SemanticColorRole::Faint)
                    .as_rgba_array();
                let whitespace_marks = presentation
                    .whitespace_marks
                    .iter()
                    .filter_map(|mark| {
                        let y = content.y + mark.rect.y - scroll_y;
                        (y + mark.rect.height >= content.y && y <= content.y + content.height)
                            .then_some((
                                LayoutBox {
                                    x: field_x(mark.rect.x),
                                    y,
                                    width: mark.rect.width,
                                    height: mark.rect.height,
                                },
                                mark.kind,
                            ))
                    })
                    .collect();
                // wrap guide：全高竖线（贯穿内容区，随横向滚动平移）。
                // 与缩进参考线（slot 10，仅行内缩进深度）区分：列位置由
                // 选项给定且贯穿全高。
                let wrap_guides = presentation
                    .wrap_guides
                    .iter()
                    .map(|&x| {
                        (
                            LayoutBox {
                                x: field_x(x),
                                y: content.y,
                                width: 1.0,
                                height: content.height,
                            },
                            self.style_model
                                .color(SemanticColorRole::Faint)
                                .as_rgba_array(),
                        )
                    })
                    .collect();
                // 缩进参考线：静态结构标记，不随焦点变化。
                let indent_guides = presentation
                    .indent_guides
                    .iter()
                    .map(|rect| {
                        (
                            LayoutBox {
                                x: field_x(rect.x),
                                y: content.y + rect.y - scroll_y,
                                width: rect.width,
                                height: rect.height,
                            },
                            self.style_model
                                .color(SemanticColorRole::Border)
                                .as_rgba_array(),
                        )
                    })
                    .collect();
                let line_labels = if multiline {
                    presentation
                        .line_tops
                        .iter()
                        .enumerate()
                        .map(|(index, top)| crate::LineLabel {
                            y: content.y + top - scroll_y,
                            height: line_height,
                            // 折叠隐藏行后由 presentation 携带原始行号；
                            // 无折叠时行号就是显示索引 + 1。
                            number: presentation
                                .line_numbers
                                .get(index)
                                .copied()
                                .unwrap_or(index as u32 + 1),
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                // git gutter 竖条：gutter 最左侧 2px（宿主预留 padding-left
                // 的区域），随滚动平移并按内容区纵向裁剪。颜色取语义令牌：
                // 新增 = success、修改 = warning、删除 = danger（调色板的
                // 绿/黄/红三档，与诊断条带同源的语义配色，对应 git 惯例）。
                let git_color = |kind| match kind {
                    crate::TextGitMarkKind::Added => self
                        .style_model
                        .color(SemanticColorRole::Success)
                        .as_rgba_array(),
                    crate::TextGitMarkKind::Modified => self
                        .style_model
                        .color(SemanticColorRole::Warning)
                        .as_rgba_array(),
                    crate::TextGitMarkKind::Deleted => self
                        .style_model
                        .color(SemanticColorRole::Danger)
                        .as_rgba_array(),
                };
                let mut git_geometry = crate::TextGitGutterGeometry {
                    added_color: git_color(crate::TextGitMarkKind::Added),
                    modified_color: git_color(crate::TextGitMarkKind::Modified),
                    deleted_color: git_color(crate::TextGitMarkKind::Deleted),
                    ..crate::TextGitGutterGeometry::default()
                };
                for mark in &presentation.git_marks {
                    let y = content.y + mark.y - scroll_y;
                    // 视口外（上/下越界）的标记不产生图元。
                    if y + mark.height < content.y || y > content.y + content.height {
                        continue;
                    }
                    let rect = LayoutBox {
                        x: bounds.x + border,
                        y,
                        width: 2.0,
                        height: mark.height,
                    };
                    match mark.kind {
                        crate::TextGitMarkKind::Added => git_geometry.added.push(rect),
                        crate::TextGitMarkKind::Modified => git_geometry.modified.push(rect),
                        crate::TextGitMarkKind::Deleted => git_geometry.deleted.push(rect),
                    }
                }
                // 折叠几何：宿主喂入的每个折叠区间在起始行 gutter 画一个
                // 可点击箭头（折叠态右箭头/展开态下箭头）；折叠态区间在
                // 起始行行尾还有摘要标记命中框。gutter 空间不足（padding
                // 左侧小于 18px）时不画箭头，摘要标记仍可点击。
                let mut fold_geometry = crate::TextFoldGeometry::default();
                if multiline
                    && let Some(input) = self.nodes.text_input(id)
                    && let Some(entry) = self.nodes.text_fold_view(id)
                {
                    let offered: Arc<[crate::TextCodeFold]> = match visual {
                        StandardVisual::TextInput { folds, .. } => Arc::clone(folds),
                        _ => Arc::from([]),
                    };
                    if !offered.is_empty() {
                        let view = self.text_display_view(id);
                        let display_of = |offset: usize| match &view {
                            Some(view) => view
                                .display_of(offset)
                                .min(presentation.display_value.len()),
                            None => offset.min(presentation.display_value.len()),
                        };
                        let gutter_width = if padding.left >= 18.0 {
                            (padding.left - 4.0).min(14.0)
                        } else {
                            0.0
                        };
                        for fold in offered.iter() {
                            let collapsed = entry.collapsed.contains(fold);
                            let fold_start = fold.start.min(input.value.len());
                            let display_offset = display_of(fold_start);
                            // 嵌套折叠：起始行被父折叠隐藏时（显示映射钳到
                            // 别处）不画箭头，避免在父折叠起始行叠加幽灵箭头。
                            if !collapsed && display_offset != fold_start {
                                continue;
                            }
                            let display_line = display_line_of(display_offset);
                            if let Some(&top) = presentation.line_tops.get(display_line)
                                && gutter_width > 0.0
                            {
                                let extent = gutter_width.min(line_height);
                                fold_geometry.gutters.push(crate::TextFoldGutter {
                                    bounds: LayoutBox {
                                        x: bounds.x + border + 2.0,
                                        y: content.y + top - scroll_y
                                            + (line_height - extent).max(0.0) / 2.0,
                                        width: extent,
                                        height: extent,
                                    },
                                    fold: *fold,
                                    collapsed,
                                    color: self
                                        .style_model
                                        .color(SemanticColorRole::Faint)
                                        .as_rgba_array(),
                                });
                            }
                        }
                        for mark in &presentation.fold_marks {
                            fold_geometry
                                .markers
                                .push(crate::components::TextFoldMarker {
                                    bounds: LayoutBox {
                                        x: field_x(mark.rect.x),
                                        y: content.y + mark.rect.y - scroll_y,
                                        width: mark.rect.width + 2.0,
                                        height: mark.rect.height,
                                    },
                                    fold: mark.fold,
                                });
                        }
                    }
                }
                // sticky scroll：滚动视口顶部落在宿主喂入的折叠区间内部时，
                // 在内容区顶部钉住显示该区间头行（首视觉行，软换行只钉第一
                // 行）。嵌套区间钉最内层（头行顶最大、最靠近视口顶的候选）；
                // 头行仍自然可见（head_top >= scroll_y）或区间末行已完全滚
                // 过视口顶（end_top + line_height <= scroll_y）时不钉——头行
                // 滚回自然位置钉住行瞬时消失。折叠语义不改变派生：折叠态区
                // 间头照常可钉，头行落在折叠隐藏区间内部的跳过（没有可见头
                // 行可钉）。派生每帧
                // 线性扫区间表（区间数小，不加缓存）；钉住行不做内容偏移，
                // 视口下缘被覆盖的内容照常滚动（非 VSCode 推开式布局）。
                let sticky_line = if multiline
                    && let StandardVisual::TextInput {
                        folds: offered,
                        editor_options:
                            crate::TextEditorRenderOptions {
                                sticky_scroll: true,
                                ..
                            },
                        ..
                    } = visual
                    && let Some(input) = self.nodes.text_input(id)
                {
                    let view = self.text_display_view(id);
                    let display_of = |offset: usize| match &view {
                        Some(view) => view
                            .display_of(offset)
                            .min(presentation.display_value.len()),
                        None => offset.min(presentation.display_value.len()),
                    };
                    let value = presentation.display_value.as_str();
                    let mut candidate: Option<(f32, usize)> = None;
                    for fold in offered.iter() {
                        let head = fold.start.min(input.value.len());
                        // 头行落在某个折叠态区间的隐藏范围内（值空间被钳制
                        // 映射到替代文本）：无可见头行可钉。不能用显示偏移
                        // 不等判定——头行在其前折叠区间之后时显示偏移同样
                        // 平移，但头行本身可见。
                        let head_hidden = view.as_ref().is_some_and(|view| {
                            view.spans
                                .iter()
                                .any(|span| head > span.value_start && head < span.value_end)
                        });
                        if head_hidden {
                            continue;
                        }
                        let head_display = display_of(head);
                        let end_display = display_of(fold.end.min(input.value.len()));
                        let (Some(&head_top), Some(&end_top)) = (
                            presentation.line_tops.get(display_line_of(head_display)),
                            presentation.line_tops.get(display_line_of(end_display)),
                        ) else {
                            continue;
                        };
                        // 头行未滚出视口顶，或区间末行已完全滚过视口顶（按
                        // 末行底缘计：末行仍跨视口顶时视口顶行仍在区间内）：
                        // 不钉。
                        if head_top >= scroll_y || end_top + line_height <= scroll_y {
                            continue;
                        }
                        if candidate.is_none_or(|(top, _)| head_top > top) {
                            candidate = Some((head_top, head_display));
                        }
                    }
                    candidate.map(|(_, head_display)| {
                        let head_line_start = value[..head_display]
                            .rfind('\n')
                            .map_or(0, |index| index + 1);
                        let head_line_end = value[head_line_start..]
                            .find('\n')
                            .map_or(value.len(), |index| head_line_start + index);
                        crate::TextStickyLineGeometry {
                            panel: LayoutBox {
                                x: content.x,
                                y: content.y,
                                width: content.width,
                                height: line_height,
                            },
                            divider: LayoutBox {
                                x: content.x,
                                y: content.y + line_height - 1.0,
                                width: content.width,
                                height: 1.0,
                            },
                            text: crate::ComponentTextRegion {
                                bounds: LayoutBox {
                                    x: content.x,
                                    y: content.y,
                                    width: content.width,
                                    height: line_height,
                                },
                                content: crate::TextValue::from(
                                    &value[head_line_start..head_line_end],
                                ),
                                color: Some(caret_color),
                                font_size: size.text_size(),
                                font_weight: style.font_weight,
                            },
                            background: style.background.unwrap_or_else(|| {
                                self.style_model
                                    .color(SemanticColorRole::Surface)
                                    .as_rgba_array()
                            }),
                            divider_color: self
                                .style_model
                                .color(SemanticColorRole::Border)
                                .as_rgba_array(),
                        }
                    })
                } else {
                    None
                };
                // minimap 竖条：内容区右缘 64px 覆盖条 + 1px 分隔线 + 行条
                // 与视口指示器。文本行宽计算不变（极长行会被条遮挡——声明
                // 取舍）；折叠只影响主视图，行条按原始逻辑行全长计算。禁用
                // 态与窄内容不产生条。
                let minimap = if multiline
                    && !disabled
                    && content.width > crate::components::TEXT_MINIMAP_STRIP_WIDTH
                    && !presentation.minimap_line_lengths.is_empty()
                {
                    Some(text_minimap_geometry(
                        &presentation.minimap_line_lengths,
                        content,
                        scroll_y,
                        line_height,
                        &self.style_model.palette,
                    ))
                } else {
                    None
                };
                // 拖拽移动选中文本的落点指示线：框架侧拖拽态写入的文本
                // 空间细竖线（2px），随滚动平移；纯视觉指示，无命中框。
                let drop_indicator = self.text_drop_indicator(id).map(|rect| {
                    (
                        LayoutBox {
                            x: field_x(rect.x),
                            y: content.y + rect.y - scroll_y,
                            width: rect.width,
                            height: rect.height,
                        },
                        self.style_model
                            .color(SemanticColorRole::Accent)
                            .as_rgba_array(),
                    )
                });
                let geometry = crate::ComponentGeometry::TextInput {
                    resize_grip: matches!(visual, StandardVisual::TextInput { editor_options, .. } if editor_options.resize_vertical).then_some(LayoutBox {
                        x: bounds.x + (bounds.width - 14.0).max(0.0), y: bounds.y + (bounds.height - 14.0).max(0.0),
                        width: bounds.width.min(14.0), height: bounds.height.min(14.0),
                    }),
                    diagnostic_markers,
                    diagnostic_labels,
                    match_markers,
                    swatch_markers,
                    swatch_border_color: self.style_model.color(SemanticColorRole::Border).as_rgba_array(),
                    atom_chips,
                    caret_line,
                    bracket_markers,
                    occurrence_markers,
                    drop_indicator,
                    whitespace_marks,
                    whitespace_color,
                    wrap_guides,
                    indent_guides,
                    line_labels,
                    folds: fold_geometry,
                    git_marks: git_geometry,
                    line_labels_color: self.style_model.color(SemanticColorRole::Faint).as_rgba_array(),
                    line_labels_font_size: size.caption_size(),
                    text: crate::ComponentTextRegion {
                        bounds: frame.text_bounds(
                            presentation.content_size.width,
                            presentation.content_size.height,
                            multiline,
                        ),
                        content: presentation.display_value.clone(),
                        color: Some(if presentation.placeholder {
                            text_input_placeholder_color(
                                &source.layout,
                                self.style_model.color(SemanticColorRole::Faint).as_rgba_array(),
                            )
                        } else {
                            style
                                .color
                                .unwrap_or_else(|| self.style_model.color(SemanticColorRole::Text).as_rgba_array())
                        }),
                        font_size: size.text_size(),
                        font_weight: style.font_weight,
                    },
                    multiline,
                    scroll: multiline.then(|| {
                        frame.scroll_x(
                            presentation.content_size.width,
                            presentation.content_size.height,
                            multiline,
                        )
                    }),
                    selection,
                    caret,
                    additional_carets,
                    preedit,
                    completion_popup: {
                        // 补全弹层：聚焦多行编辑器 + 未关闭的非空候选会话；
                        // 锚定主光标行，与其他编辑器覆盖层共用一套定位翻
                        // 转策略（见 `anchored_overlay_panel`）。
                        if multiline && focused {
                            self.nodes
                                .text_completion_view(id)
                                .filter(|state| !state.dismissed)
                                .zip(presentation.overlay_metrics.completion.as_ref())
                                .and_then(|(state, metrics)| {
                                    completion_popup_geometry(
                                        state,
                                        metrics,
                                        OverlayAnchor {
                                            x: field_x(presentation.caret_x),
                                            line_top: line_y + presentation.caret_y,
                                            line_height,
                                        },
                                        bounds,
                                        size.text_size(),
                                        &self.style_model.palette,
                                    )
                                })
                        } else {
                            None
                        }
                    },
                    hover_popup: {
                        // hover 浮窗：宿主喂入即显示（不要求焦点），纯展示。
                        presentation
                            .overlay_metrics
                            .hover_anchor
                            .zip(self.nodes.text_hover_view(id))
                            .and_then(|((hover_x, hover_y), state)| {
                                hover_popup_geometry(
                                    state,
                                    OverlayAnchor {
                                        x: field_x(hover_x),
                                        line_top: line_y + hover_y,
                                        line_height,
                                    },
                                    bounds,
                                    size.text_size(),
                                    &self.style_model.palette,
                                )
                            })
                    },
                    signature_popup: {
                        // 签名帮助：宿主喂入即显示，锚定 caret；占位符与
                        // IME 组合期不弹出（与 hover 同一打字态门）。
                        if multiline && !presentation.placeholder && presentation.preedit.is_none()
                        {
                            self.nodes.text_signature(id).and_then(|help| {
                                signature_popup_geometry(
                                    help,
                                    OverlayAnchor {
                                        x: field_x(presentation.caret_x),
                                        line_top: line_y + presentation.caret_y,
                                        line_height,
                                    },
                                    bounds,
                                    size.text_size(),
                                    &self.style_model.palette,
                                    self.chrome_text_measure(id),
                                )
                            })
                        } else {
                            None
                        }
                    },
                    minimap,
                    sticky_line,
                    background: style.background,
                    border: style.border_color,
                    border_width: {
                        let width = if style.border_color.is_some() {
                            source.layout.resolved_border_width()
                        } else {
                            0.0
                        };
                        if focused && *invalid {
                            width.max(2.0)
                        } else {
                            width
                        }
                    },
                    focus_ring: None,
                    selection_color: self.style_model.color(SemanticColorRole::AccentSoft).as_rgba_array(),
                    caret_color,
                    additional_caret_color,
                    preedit_color: self.style_model.color(SemanticColorRole::Accent).as_rgba_array(),
                    steppers,
                };
                Some(if frame.writing.is_vertical() {
                    vertical_text_input_geometry(geometry, &frame, presentation, multiline, focused)
                } else {
                    geometry
                })
            }
            StandardVisual::Switch {
                thumb_progress,
                label,
                hint,
                checked,
                control_position,
                size,
                invalid,
                ..
            } => {
                let track = self.style_model.metrics.switch;
                let control = LayoutBox {
                    x: match control_position {
                        SwitchControlPosition::Start => content.x,
                        SwitchControlPosition::End => {
                            content.x + (content.width - track.track_width).max(0.0)
                        }
                    },
                    y: content.y + (content.height - track.track_height) / 2.0,
                    width: track.track_width.min(content.width),
                    height: track.track_height.min(content.height),
                };
                let text_x = if *control_position == SwitchControlPosition::Start {
                    control.x + control.width + track.label_gap
                } else {
                    content.x
                };
                let text_right = if *control_position == SwitchControlPosition::End {
                    control.x - track.label_gap
                } else {
                    content.x + content.width
                };
                let text_width = (text_right - text_x).max(0.0);
                let (label_bounds, hint_bounds) = if hint.is_some() {
                    (
                        LayoutBox {
                            x: text_x,
                            y: content.y,
                            width: text_width,
                            height: 18.0_f32.min(content.height),
                        },
                        Some(LayoutBox {
                            x: text_x,
                            y: content.y + 20.0,
                            width: text_width,
                            height: (content.height - 20.0).max(0.0),
                        }),
                    )
                } else {
                    (
                        LayoutBox {
                            x: text_x,
                            y: content.y,
                            width: text_width,
                            height: content.height,
                        },
                        None,
                    )
                };
                let palette = self.style_model;
                let hovered = self
                    .input
                    .pointer_hover
                    .values()
                    .any(|target| *target == id);
                let pressed = self
                    .input
                    .pointer_press
                    .values()
                    .any(|target| *target == id);
                let disabled = self.record(id).accessibility.disabled;
                let mix = |foreground: [f32; 4], background: [f32; 4], amount: f32| {
                    let amount = amount.clamp(0.0, 1.0);
                    std::array::from_fn(|channel| {
                        foreground[channel] * amount + background[channel] * (1.0 - amount)
                    })
                };
                let fade = |mut color: [f32; 4]| {
                    if disabled {
                        color[3] *= 0.55;
                    }
                    color
                };
                let track_background = if *checked {
                    if pressed {
                        palette
                            .color(SemanticColorRole::AccentStrong)
                            .as_rgba_array()
                    } else {
                        palette.color(SemanticColorRole::Accent).as_rgba_array()
                    }
                } else {
                    mix(
                        palette.color(SemanticColorRole::Hover).as_rgba_array(),
                        palette.color(SemanticColorRole::Background).as_rgba_array(),
                        0.78,
                    )
                };
                let track_border = if *invalid {
                    palette.color(SemanticColorRole::Danger).as_rgba_array()
                } else if *checked {
                    if hovered || pressed {
                        palette
                            .color(SemanticColorRole::AccentStrong)
                            .as_rgba_array()
                    } else {
                        palette.color(SemanticColorRole::Accent).as_rgba_array()
                    }
                } else if hovered || pressed {
                    mix(
                        palette.color(SemanticColorRole::Accent).as_rgba_array(),
                        palette
                            .color(SemanticColorRole::BorderStrong)
                            .as_rgba_array(),
                        if pressed { 0.70 } else { 0.42 },
                    )
                } else {
                    palette
                        .color(SemanticColorRole::BorderStrong)
                        .as_rgba_array()
                };
                let thumb_background = if *checked {
                    palette.color(SemanticColorRole::AccentText).as_rgba_array()
                } else {
                    mix(
                        palette.color(SemanticColorRole::Faint).as_rgba_array(),
                        palette.color(SemanticColorRole::Background).as_rgba_array(),
                        0.70,
                    )
                };
                Some(crate::ComponentGeometry::Switch {
                    thumb_progress: *thumb_progress,
                    label: text_region(
                        label_bounds,
                        Arc::clone(label),
                        false,
                        size.text_size(),
                        Some(500),
                    ),
                    hint: hint.as_ref().zip(hint_bounds).map(|(hint, bounds)| {
                        text_region(
                            bounds,
                            Arc::clone(hint),
                            true,
                            (size.text_size() - 1.0).max(10.0),
                            None,
                        )
                    }),
                    control,
                    track_background: fade(track_background),
                    track_border: fade(track_border),
                    thumb_background: fade(thumb_background),
                })
            }
            StandardVisual::Scrollbar {
                axes,
                visibility,
                revealed,
                dragging,
            } => {
                if !revealed || matches!(visibility, nana_ui_core::ScrollbarVisibility::Hidden) {
                    return None;
                }
                let metrics = self.scroll_metrics(id)?;
                let offset = self.scroll_offset(id).unwrap_or_default();
                let skin = source.layout.paint.scrollbar;
                // The installed theme, not the constant: this is the one
                // scrollbar metric read the runtime makes, so a density change
                // has to arrive here or it never reaches a scrollbar.
                let base = self.style_model.metrics.scrollbar;
                let chrome = skin.map(|skin| skin.metrics(base)).unwrap_or(base);
                let palette = self.style_model;
                let track_background = skin.and_then(|skin| skin.track_color).or_else(|| {
                    matches!(visibility, nana_ui_core::ScrollbarVisibility::Always)
                        .then(|| palette.color(SemanticColorRole::Subtle).as_rgba_array())
                });
                let scrolls = |axis: nana_ui_core::ScrollbarAxis| match axis {
                    nana_ui_core::ScrollbarAxis::Horizontal => {
                        axes.horizontal() && metrics.content_width > metrics.viewport_width
                    }
                    nana_ui_core::ScrollbarAxis::Vertical => {
                        axes.vertical() && metrics.content_height > metrics.viewport_height
                    }
                };
                let bar = |axis: nana_ui_core::ScrollbarAxis| {
                    if !scrolls(axis) {
                        return None;
                    }
                    let horizontal = matches!(axis, nana_ui_core::ScrollbarAxis::Horizontal);
                    // Give up the far corner only when the other axis also has
                    // a bar, so the two never overlap.
                    let corner = if scrolls(match axis {
                        nana_ui_core::ScrollbarAxis::Horizontal => {
                            nana_ui_core::ScrollbarAxis::Vertical
                        }
                        nana_ui_core::ScrollbarAxis::Vertical => {
                            nana_ui_core::ScrollbarAxis::Horizontal
                        }
                    }) {
                        chrome.thickness
                    } else {
                        0.0
                    };
                    let along = if horizontal {
                        (bounds.width - corner).max(0.0)
                    } else {
                        (bounds.height - corner).max(0.0)
                    };
                    let track = nana_ui_core::scrollbar_track(
                        if horizontal {
                            metrics.viewport_width
                        } else {
                            metrics.viewport_height
                        },
                        if horizontal {
                            metrics.content_width
                        } else {
                            metrics.content_height
                        },
                        if horizontal { offset.x } else { offset.y },
                        if horizontal {
                            metrics.origin_x
                        } else {
                            metrics.origin_y
                        },
                        if horizontal { bounds.x } else { bounds.y },
                        along,
                        chrome,
                    )?;
                    let active = *dragging == Some(axis);
                    let thumb_inset = ((chrome.thickness - chrome.thumb_thickness) / 2.0).max(0.0);
                    let (track_box, thumb_box) = if horizontal {
                        let track_y = bounds.y + bounds.height - chrome.thickness;
                        (
                            LayoutBox {
                                x: track.origin,
                                y: track_y,
                                width: track.length,
                                height: chrome.thickness,
                            },
                            LayoutBox {
                                x: track.thumb_origin,
                                y: track_y + thumb_inset,
                                width: track.thumb_length,
                                height: chrome.thumb_thickness,
                            },
                        )
                    } else {
                        let track_x = bounds.x + bounds.width - chrome.thickness;
                        (
                            LayoutBox {
                                x: track_x,
                                y: track.origin,
                                width: chrome.thickness,
                                height: track.length,
                            },
                            LayoutBox {
                                x: track_x + thumb_inset,
                                y: track.thumb_origin,
                                width: chrome.thumb_thickness,
                                height: track.thumb_length,
                            },
                        )
                    };
                    Some(crate::ScrollbarBar {
                        track: track_box,
                        thumb: thumb_box,
                        track_background,
                        thumb_background: skin.and_then(|skin| skin.thumb_color).unwrap_or_else(
                            || {
                                if active {
                                    palette.color(SemanticColorRole::Muted).as_rgba_array()
                                } else {
                                    palette
                                        .color(SemanticColorRole::BorderStrong)
                                        .as_rgba_array()
                                }
                            },
                        ),
                        thumb_radius: chrome.thumb_radius(),
                        min_offset: track.min_offset,
                        max_offset: track.max_offset,
                    })
                };
                let horizontal = bar(nana_ui_core::ScrollbarAxis::Horizontal);
                let vertical = bar(nana_ui_core::ScrollbarAxis::Vertical);
                (horizontal.is_some() || vertical.is_some()).then_some(
                    crate::ComponentGeometry::Scrollbar {
                        horizontal,
                        vertical,
                    },
                )
            }
            StandardVisual::Range {
                label,
                value,
                unit,
                size,
                ..
            } => {
                // The spacing ladder, not a second one: these were 6 / 8 / 10
                // as bare numbers sitting next to `SpacingTokens`' own SM / MD
                // / LG, which are the same three values.
                let gap = match size {
                    nana_ui_core::ControlSize::Small => nana_ui_core::space::SM,
                    nana_ui_core::ControlSize::Medium => nana_ui_core::space::MD,
                    nana_ui_core::ControlSize::Large => nana_ui_core::space::LG,
                };
                let label_width = label
                    .as_ref()
                    .map_or(0.0, |_| 84.0_f32.min(content.width * 0.28));
                let show_value = !value.is_empty();
                let unit_width = if show_value {
                    unit.as_ref().map_or(0.0, |_| 32.0_f32.min(content.width))
                } else {
                    0.0
                };
                let value_width = if show_value {
                    60.0_f32.min((content.width - unit_width).max(0.0))
                } else {
                    0.0
                };
                let trailing_width = value_width + unit_width;
                let track_x = content.x + label_width + if label.is_some() { gap } else { 0.0 };
                let track_right = content.x + content.width
                    - trailing_width
                    - if trailing_width > 0.0 { gap } else { 0.0 };
                let thumb = size.icon_size();
                let track = LayoutBox {
                    x: track_x + thumb / 2.0,
                    y: content.y + (content.height - thumb) / 2.0,
                    width: (track_right - track_x - thumb).max(0.0),
                    height: thumb.min(content.height),
                };
                Some(crate::ComponentGeometry::Range {
                    label: label.as_ref().map(|label| {
                        text_region(
                            LayoutBox {
                                x: content.x,
                                y: content.y,
                                width: label_width,
                                height: content.height,
                            },
                            Arc::clone(label),
                            false,
                            size.text_size(),
                            Some(500),
                        )
                    }),
                    value: text_region(
                        LayoutBox {
                            x: content.x + content.width - value_width - unit_width,
                            y: content.y,
                            width: value_width,
                            height: content.height,
                        },
                        Arc::clone(value),
                        false,
                        size.text_size(),
                        Some(500),
                    ),
                    unit: unit.as_ref().map(|unit| {
                        text_region(
                            LayoutBox {
                                x: content.x + content.width - unit_width,
                                y: content.y,
                                width: unit_width,
                                height: content.height,
                            },
                            Arc::clone(unit),
                            true,
                            (size.text_size() - 1.0).max(10.0),
                            None,
                        )
                    }),
                    track,
                })
            }
            StandardVisual::Card {
                title,
                kind,
                loading,
                ..
            } => {
                let shaped_title_width = self
                    .text_metrics(id)
                    .map_or(0.0, |metrics| metrics.width.min(content.width));
                let title_width = (content.width
                    - if *loading {
                        crate::view_components::CARD_LOADING_RESERVE
                    } else {
                        0.0
                    })
                .max(0.0);
                let title_y = bounds.y
                    + border
                    + (padding.top - crate::view_components::CARD_TITLE_BAND).max(0.0);
                Some(crate::ComponentGeometry::Card {
                    title: title.as_ref().map(|title| {
                        text_region(
                            LayoutBox {
                                x: bounds.x + border + padding.left,
                                y: title_y,
                                width: title_width,
                                height: crate::view_components::CARD_TITLE_LINE,
                            },
                            Arc::clone(title),
                            false,
                            crate::view_components::CARD_TITLE_SIZE,
                            Some(crate::view_components::CARD_TITLE_WEIGHT),
                        )
                    }),
                    content,
                    elevation: (*kind == nana_ui_core::CardKind::Raised).then_some(
                        crate::ComponentElevation::from_shadow(
                            self.theme.shadow(nana_ui_core::ElevationRole::Surface),
                        ),
                    ),
                    spinner: (*loading).then_some(LayoutBox {
                        x: (bounds.x + border + padding.left + shaped_title_width + 8.0)
                            .min(content.x + content.width - 14.0),
                        y: title_y + 2.0,
                        width: 14.0,
                        height: 14.0,
                    }),
                })
            }
            StandardVisual::ListItem {
                leading,
                content: content_slot,
                trailing,
                detail,
            } => {
                // 隐藏的槽位子节点已退出 flex 流，其盒几何是陈旧的，不得
                // 参与行文本区间的兜底计算。
                let slot_box = |id: StableNodeId| -> Option<LayoutBox> {
                    if self
                        .node_style(id)
                        .is_some_and(|style| style.layout.omits_box())
                    {
                        return None;
                    }
                    self.layout_box(id)
                };
                let leading = leading.and_then(slot_box);
                let trailing = trailing.and_then(slot_box);
                let fallback_x = leading.map_or(content.x, |leading| {
                    leading.x
                        + leading.width
                        + source.layout.main_gap_against(
                            nana_ui_core::FlexDirection::Row,
                            nana_ui_core::ParentBox::from_viewport(content.width, content.height),
                        )
                });
                let fallback_right = trailing.map_or(content.x + content.width, |trailing| {
                    trailing.x
                        - source.layout.main_gap_against(
                            nana_ui_core::FlexDirection::Row,
                            nana_ui_core::ParentBox::from_viewport(content.width, content.height),
                        )
                });
                let mut label_rect = content_slot.and_then(slot_box).unwrap_or(LayoutBox {
                    x: fallback_x,
                    y: content.y,
                    width: (fallback_right - fallback_x).max(0.0),
                    height: content.height,
                });
                // 单行 detail：小字号 muted 文本右对齐；label 测宽避让，
                // 放不下时 detail 占剩余宽度、超出交给省略号（与 LabeledValue
                // 的值侧同款规则）。
                let detail_region =
                    detail
                        .clone()
                        .filter(|detail| !detail.is_empty())
                        .map(|detail| {
                            let label_size = style.font_size;
                            let detail_size = (label_size - 1.0).max(10.0);
                            let gap = nana_ui_core::space::MD;
                            let measure = self.chrome_text_measure(id);
                            let label_natural = measure.width(
                                self.text(id).unwrap_or_default(),
                                label_size,
                                style.font_weight,
                            );
                            let detail_natural = measure.width(&detail, detail_size, None);
                            let min_detail_visible = nana_ui_core::space::XXXL;
                            let detail_width =
                                if label_natural + gap + detail_natural <= label_rect.width {
                                    detail_natural
                                } else {
                                    (label_rect.width - label_natural - gap)
                                        .max(min_detail_visible)
                                        .min(label_rect.width)
                                };
                            let detail_x =
                                (label_rect.x + label_rect.width - detail_width).max(label_rect.x);
                            let detail_height =
                                (detail_size * 1.2).min(label_rect.height.max(detail_size));
                            let detail_y =
                                label_rect.y + (label_rect.height - detail_height).max(0.0) / 2.0;
                            label_rect.width = ((detail_x - gap) - label_rect.x).max(0.0);
                            crate::ComponentTextRegion {
                                bounds: LayoutBox {
                                    x: detail_x,
                                    y: detail_y,
                                    width: detail_width,
                                    height: detail_height,
                                },
                                content: detail.into(),
                                color: Some(secondary),
                                font_size: detail_size,
                                font_weight: None,
                            }
                        });
                Some(crate::ComponentGeometry::ListItem {
                    leading,
                    content: Some(label_rect),
                    trailing,
                    detail: detail_region,
                })
            }
            StandardVisual::StatusBadge {
                label,
                tone,
                compact,
            } => {
                let (horizontal, indicator_slot, gap, text_size) = if *compact {
                    (
                        nana_ui_core::space::SM,
                        nana_ui_core::space::SM,
                        nana_ui_core::space::XS,
                        nana_ui_core::type_scale::HINT,
                    )
                } else {
                    (
                        nana_ui_core::space::MD,
                        nana_ui_core::space::MD,
                        nana_ui_core::space::SM,
                        nana_ui_core::type_scale::META,
                    )
                };
                let diameter = indicator_slot * STATUS_DOT_FRACTION;
                let foreground = self
                    .style_model
                    .color(self.theme.recipes().status().role(*tone))
                    .as_rgba_array();
                let mut background = foreground;
                background[3] *= 0.12;
                Some(crate::ComponentGeometry::StatusBadge {
                    indicator: LayoutBox {
                        x: bounds.x + horizontal + (indicator_slot - diameter) / 2.0,
                        y: bounds.y + (bounds.height - diameter) / 2.0,
                        width: diameter,
                        height: diameter,
                    },
                    label: crate::ComponentTextRegion {
                        bounds: LayoutBox {
                            x: bounds.x + horizontal + indicator_slot + gap,
                            y: bounds.y,
                            width: (bounds.width - horizontal * 2.0 - indicator_slot - gap)
                                .max(0.0),
                            height: bounds.height,
                        },
                        content: Arc::clone(label).into(),
                        color: Some(foreground),
                        font_size: text_size,
                        font_weight: Some(nana_ui_core::type_scale::MEDIUM),
                    },
                    background,
                    foreground,
                })
            }
            StandardVisual::ValidationMessage {
                message,
                intent,
                compact,
            } => {
                let (indicator_slot, gap, text_size) = if *compact {
                    (
                        nana_ui_core::space::XL,
                        nana_ui_core::space::XS,
                        nana_ui_core::type_scale::HINT,
                    )
                } else {
                    (
                        nana_ui_core::space::XXL,
                        nana_ui_core::space::SM,
                        nana_ui_core::type_scale::META,
                    )
                };
                let diameter = indicator_slot * STATUS_DOT_FRACTION;
                let foreground = self
                    .style_model
                    .color(match intent {
                        nana_ui_core::ValidationIntent::Warning => SemanticColorRole::Warning,
                        nana_ui_core::ValidationIntent::Danger => SemanticColorRole::Danger,
                    })
                    .as_rgba_array();
                Some(crate::ComponentGeometry::ValidationMessage {
                    indicator: LayoutBox {
                        x: bounds.x + (indicator_slot - diameter) / 2.0,
                        y: bounds.y + (bounds.height - diameter) / 2.0,
                        width: diameter,
                        height: diameter,
                    },
                    label: crate::ComponentTextRegion {
                        bounds: LayoutBox {
                            x: bounds.x + indicator_slot + gap,
                            y: bounds.y,
                            width: (bounds.width - indicator_slot - gap).max(0.0),
                            height: bounds.height,
                        },
                        content: Arc::clone(message).into(),
                        color: Some(foreground),
                        font_size: text_size,
                        font_weight: None,
                    },
                    foreground,
                })
            }
            StandardVisual::EmptyState {
                title,
                message,
                icon,
                compact,
                action,
            } => {
                let (horizontal, vertical, title_size, message_size, spacing) = if *compact {
                    (
                        nana_ui_core::space::SM,
                        nana_ui_core::space::MD,
                        nana_ui_core::type_scale::META,
                        nana_ui_core::type_scale::HINT,
                        nana_ui_core::space::XXS,
                    )
                } else {
                    (
                        nana_ui_core::space::XXXL,
                        nana_ui_core::space::PAGE,
                        nana_ui_core::type_scale::BODY,
                        nana_ui_core::type_scale::META,
                        nana_ui_core::space::SM,
                    )
                };
                let width = (bounds.width - horizontal * 2.0).max(0.0);
                let presentation = self.nodes.empty_state_text(id).copied().unwrap_or_default();
                let text_bounds = |metrics: TextMetrics, y: f32| {
                    let shaped_width = metrics.width.clamp(0.0, width);
                    crate::LayoutBox {
                        x: bounds.x
                            + horizontal
                            + if *compact {
                                0.0
                            } else {
                                (width - shaped_width) / 2.0
                            },
                        y,
                        width: shaped_width,
                        height: metrics.height,
                    }
                };
                let mut y = bounds.y + vertical;
                let icon = icon.map(|icon| {
                    let icon_width = EMPTY_STATE_ICON.min(width);
                    let icon_bounds = LayoutBox {
                        x: if *compact {
                            bounds.x + horizontal
                        } else {
                            bounds.x + horizontal + (width - icon_width) / 2.0
                        },
                        y,
                        width: icon_width,
                        height: EMPTY_STATE_ICON,
                    };
                    y += EMPTY_STATE_ICON + spacing;
                    (
                        icon,
                        icon_bounds,
                        self.style_model
                            .color(SemanticColorRole::Faint)
                            .as_rgba_array(),
                    )
                });
                let title_region = crate::ComponentTextRegion {
                    bounds: text_bounds(presentation.title, y),
                    content: Arc::clone(title).into(),
                    color: Some(if *compact {
                        secondary
                    } else {
                        self.style_model
                            .color(SemanticColorRole::Text)
                            .as_rgba_array()
                    }),
                    font_size: title_size,
                    font_weight: Some(600),
                };
                y += presentation.title.height;
                let message = message.as_ref().map(|message| {
                    y += spacing;
                    crate::ComponentTextRegion {
                        bounds: text_bounds(presentation.message.unwrap_or_default(), y),
                        content: Arc::clone(message).into(),
                        color: Some(secondary),
                        font_size: message_size,
                        font_weight: None,
                    }
                });
                Some(crate::ComponentGeometry::EmptyState {
                    root_clip: bounds,
                    content_clip: LayoutBox {
                        x: bounds.x + horizontal,
                        y: bounds.y + vertical,
                        width,
                        height: (bounds.height - vertical * 2.0).max(0.0),
                    },
                    icon,
                    title: title_region,
                    message,
                    action: action.and_then(|action| self.layout_box(action)),
                })
            }
            StandardVisual::LabeledValue {
                label,
                value,
                value_role,
                value_weight,
                compact,
                action,
            } => {
                let gap = if *compact {
                    nana_ui_core::space::XS
                } else {
                    nana_ui_core::space::MD
                };
                let right = action
                    .and_then(|action| self.layout_box(action))
                    .map_or(bounds.x + bounds.width, |action| {
                        (action.x - gap).max(bounds.x)
                    });
                let available = (right - bounds.x).max(0.0);
                let label_size = nana_ui_core::type_scale::HINT;
                let value_size = nana_ui_core::type_scale::META;
                let label_height = (label_size * 1.2).min(bounds.height.max(label_size));
                let value_height = (value_size * 1.2).min(bounds.height.max(value_size));
                // 属性名按自身文本测宽保底,不再压缩到字号常数;两侧都放得下时各取自然宽度。
                let measure = self.chrome_text_measure(id);
                let label_natural = if label.is_empty() {
                    0.0
                } else {
                    measure.width(label, label_size, None)
                };
                let value_natural = measure.width(value, value_size, Some(*value_weight));
                let min_value_visible = nana_ui_core::space::XXXL;
                // 放不下时值侧占满剩余宽度,超出部分由文本图元的省略号收尾;
                // 仅属性名自身就放不下(剩余为负)才回退最小可见宽度。
                let value_width = if label_natural + gap + value_natural <= available {
                    value_natural
                } else {
                    (available - label_natural - gap)
                        .max(min_value_visible)
                        .min(available)
                };
                let value_x = (right - value_width).max(bounds.x);
                let label_width = ((value_x - gap - bounds.x).max(0.0)).min(label_natural);
                let center_y = |height: f32| bounds.y + (bounds.height - height).max(0.0) / 2.0;
                Some(crate::ComponentGeometry::LabeledValue {
                    label: crate::ComponentTextRegion {
                        bounds: LayoutBox {
                            x: bounds.x,
                            y: center_y(label_height),
                            width: label_width,
                            height: label_height,
                        },
                        content: Arc::clone(label).into(),
                        color: Some(
                            self.style_model
                                .color(SemanticColorRole::Faint)
                                .as_rgba_array(),
                        ),
                        font_size: label_size,
                        font_weight: None,
                    },
                    value: crate::ComponentTextRegion {
                        bounds: LayoutBox {
                            x: value_x,
                            y: center_y(value_height),
                            width: value_width,
                            height: value_height,
                        },
                        content: Arc::clone(value).into(),
                        color: Some(self.style_model.color(*value_role).as_rgba_array()),
                        font_size: value_size,
                        font_weight: Some(*value_weight),
                    },
                    action: action.and_then(|action| self.layout_box(action)),
                })
            }
            StandardVisual::SelectionOption {
                label,
                icon,
                size,
                show_focus_ring,
                indicator,
                selected,
                disabled,
            } => {
                let icon_extent = size.icon_size().min(content.height);
                let base_padding = if *indicator {
                    size.radio_lead()
                } else {
                    size.padding_x_in(self.style_model.metrics) + nana_ui_core::space::XXS
                };
                let ring = indicator.then(|| {
                    let extent = size.indicator_size().min(bounds.height);
                    let ring = LayoutBox {
                        x: bounds.x + nana_ui_core::RADIO_ROW_INSET,
                        y: bounds.y + (bounds.height - extent) / 2.0,
                        width: extent,
                        height: extent,
                    };
                    let (ring_color, dot_color) = if *disabled {
                        let faint = self
                            .style_model
                            .color(SemanticColorRole::Faint)
                            .as_rgba_array();
                        (faint, faint)
                    } else if *selected {
                        let accent = self
                            .style_model
                            .color(SemanticColorRole::Accent)
                            .as_rgba_array();
                        (accent, accent)
                    } else {
                        (
                            self.style_model
                                .color(SemanticColorRole::BorderStrong)
                                .as_rgba_array(),
                            self.style_model
                                .color(SemanticColorRole::Accent)
                                .as_rgba_array(),
                        )
                    };
                    let dot_extent = extent / 2.5;
                    crate::RadioIndicator {
                        ring,
                        ring_color,
                        dot: selected.then_some((
                            LayoutBox {
                                x: ring.x + (extent - dot_extent) / 2.0,
                                y: ring.y + (extent - dot_extent) / 2.0,
                                width: dot_extent,
                                height: dot_extent,
                            },
                            dot_color,
                        )),
                    }
                });
                let icon_bounds = icon.map(|icon| {
                    (
                        icon,
                        LayoutBox {
                            x: bounds.x + base_padding,
                            y: icon_y_on_text_glyph_center(
                                content.y,
                                content.height,
                                style.font_size,
                                style.line_height,
                                matches!(
                                    source.text_vertical_alignment,
                                    TextVerticalAlignment::Center
                                ),
                                icon_extent,
                            ),
                            width: icon_extent,
                            height: icon_extent,
                        },
                        style.color.unwrap_or_else(|| {
                            self.style_model
                                .color(SemanticColorRole::Muted)
                                .as_rgba_array()
                        }),
                    )
                });
                // The label is the content box: `SegmentedOption::project`
                // already spent the icon and its gap in the leading padding, so
                // re-deriving the label origin from the icon only invents a
                // second gap that layout never reserved.
                Some(crate::ComponentGeometry::SelectionOption {
                    icon: icon_bounds,
                    label: text_region(
                        LayoutBox {
                            x: content.x,
                            y: content.y,
                            width: content.width,
                            height: content.height,
                        },
                        Arc::clone(label),
                        false,
                        size.text_size(),
                        Some(500),
                    ),
                    // The radio ring is the one indicator that predates the
                    // focus tokens, and it follows the same rule: a clicked
                    // control is focused and does not announce it.
                    focus_ring: (*show_focus_ring
                        && self.focus_visible(self.record(id).document) == Some(id))
                    .then(|| {
                        self.style_model
                            .color(SemanticColorRole::FocusBorder)
                            .as_rgba_array()
                    }),
                    indicator: ring,
                })
            }
            StandardVisual::Progress {
                value_ratio,
                label,
                cancellable,
            } => progress_geometry(
                bounds,
                style,
                *value_ratio,
                PROGRESS_GIRTH,
                // A pill, derived — not the literal `3.0` it used to be. The
                // two were equal and free to drift, and only `LevelMeter`
                // right below said so in code.
                PROGRESS_GIRTH / 2.0,
                label.as_ref(),
                *cancellable,
                self.style_model
                    .color(SemanticColorRole::Text)
                    .as_rgba_array(),
            ),
            StandardVisual::LevelMeter {
                value_ratio, girth, ..
            } => {
                let girth = if girth.is_finite() && *girth > 0.0 {
                    *girth
                } else {
                    nana_ui_core::space::XS
                };
                progress_geometry(
                    bounds,
                    style,
                    *value_ratio,
                    girth,
                    girth / 2.0,
                    None,
                    false,
                    [0.0; 4],
                )
            }
            StandardVisual::FormField {
                label,
                hint,
                error,
                size,
                control,
            } => form_field_geometry(
                bounds,
                *size,
                label,
                hint.as_ref(),
                error.as_ref(),
                *control,
                &|id| self.layout_box(id),
                self.style_model,
            ),
            StandardVisual::Toast {
                title,
                description,
                dismissible,
                ..
            } => {
                // Same constants the layout side reserves; the two must not
                // drift, and the dismiss reserve follows the themeable compact
                // control height rather than a literal.
                let pad_x = crate::toast::PAD_X;
                let pad_y = crate::toast::PAD_Y;
                let indicator = crate::toast::INDICATOR_SIZE;
                let gap = crate::toast::INDICATOR_GAP;
                let dismiss = if *dismissible {
                    nana_ui_core::ControlSize::Small.height_in(self.style_model.metrics)
                } else {
                    0.0
                };
                let copy_x = bounds.x + pad_x + indicator + gap;
                let copy_right =
                    bounds.x + bounds.width - pad_x - if *dismissible { dismiss } else { 0.0 };
                let copy_width = (copy_right - copy_x).max(0.0);
                let title_height = nana_ui_core::type_scale::META;
                let desc_height = nana_ui_core::type_scale::HINT;
                let has_desc = description.is_some();
                let title_y = if has_desc {
                    bounds.y + pad_y
                } else {
                    bounds.y + (bounds.height - title_height) / 2.0
                };
                Some(crate::ComponentGeometry::Toast {
                    indicator: LayoutBox {
                        x: bounds.x + pad_x,
                        y: bounds.y + (bounds.height - indicator) / 2.0,
                        width: indicator,
                        height: indicator,
                    },
                    title: crate::ComponentTextRegion {
                        bounds: LayoutBox {
                            x: copy_x,
                            y: title_y,
                            width: copy_width,
                            height: title_height,
                        },
                        content: Arc::clone(title).into(),
                        color: Some(
                            self.style_model
                                .color(SemanticColorRole::Text)
                                .as_rgba_array(),
                        ),
                        font_size: nana_ui_core::type_scale::META,
                        font_weight: Some(nana_ui_core::type_scale::SEMIBOLD),
                    },
                    description: description.as_ref().map(|description| {
                        crate::ComponentTextRegion {
                            bounds: LayoutBox {
                                x: copy_x,
                                y: title_y + title_height + nana_ui_core::space::XXS,
                                width: copy_width,
                                height: desc_height,
                            },
                            content: Arc::clone(description).into(),
                            color: Some(secondary),
                            font_size: nana_ui_core::type_scale::HINT,
                            font_weight: None,
                        }
                    }),
                    dismiss: dismissible.then(|| LayoutBox {
                        x: bounds.x + bounds.width - pad_x - dismiss,
                        y: bounds.y + (bounds.height - dismiss) / 2.0,
                        width: dismiss,
                        height: dismiss,
                    }),
                })
            }
            StandardVisual::XYPad { nx, ny, .. } => {
                let pad = bounds;
                let thumb = nana_ui_core::space::MD;
                let nx = nx.clamp(0.0, 1.0);
                let ny = ny.clamp(0.0, 1.0);
                Some(crate::ComponentGeometry::XYPad {
                    pad,
                    thumb: LayoutBox {
                        x: pad.x + nx * pad.width - thumb / 2.0,
                        y: pad.y + ny * pad.height - thumb / 2.0,
                        width: thumb,
                        height: thumb,
                    },
                    h_axis: LayoutBox {
                        x: pad.x,
                        y: pad.y + pad.height / 2.0 - nana_ui_core::HAIRLINE / 2.0,
                        width: pad.width,
                        height: nana_ui_core::HAIRLINE,
                    },
                    v_axis: LayoutBox {
                        x: pad.x + pad.width / 2.0 - nana_ui_core::HAIRLINE / 2.0,
                        y: pad.y,
                        width: nana_ui_core::HAIRLINE,
                        height: pad.height,
                    },
                    background: style.background,
                    border: style.border_color,
                    border_width: if style.border_color.is_some() {
                        source.layout.resolved_border_width()
                    } else {
                        0.0
                    },
                    thumb_color: self
                        .style_model
                        .color(SemanticColorRole::Accent)
                        .as_rgba_array(),
                    axis_color: self
                        .style_model
                        .color(SemanticColorRole::Border)
                        .as_rgba_array(),
                })
            }
            StandardVisual::Select {
                label,
                placeholder,
                size,
                opened,
                options,
                highlighted,
                checkable,
                ..
            } => Some(crate::select::select_geometry(
                bounds,
                label,
                *placeholder,
                *size,
                *opened,
                options,
                *highlighted,
                style,
                &source,
                &self.style_model.palette,
                self.style_model.metrics,
                self.document_viewport_of(id),
                *checkable,
                padding,
            )),
            StandardVisual::MenuSurface {
                kind: crate::MenuSurfaceKind::ContextMenu,
                query,
                rows,
                highlighted,
                ..
            } if query.is_some() || !rows.is_empty() => Some(crate::menus::context_menu_geometry(
                bounds,
                query.as_ref(),
                rows,
                *highlighted,
                &self.style_model.palette,
                self.style_model.metrics,
            )),
            StandardVisual::MenuSurface {
                trigger,
                trigger_icon,
                trigger_image,
                overlay,
                ..
            } => {
                let trigger_content = overlay.and_then(|overlay| overlay.trigger_content);
                let has_trigger = trigger.is_some()
                    || trigger_icon.is_some()
                    || trigger_image.is_some()
                    || trigger_content.is_some();
                let surface = if has_trigger {
                    crate::popover::overlay_surface_from_items(self, id, overlay.as_ref())
                } else {
                    bounds
                };
                Some(crate::popover::menu_surface_geometry(
                    bounds,
                    trigger.as_ref(),
                    *trigger_icon,
                    trigger_image.as_ref(),
                    trigger_content.is_some(),
                    style,
                    &self.style_model.palette,
                    self.style_model.metrics,
                    surface,
                ))
            }
            StandardVisual::ActionMenuItem {
                label,
                hint,
                icon,
                danger,
                disabled,
                size,
                ..
            } => Some(crate::menus::action_menu_item_geometry(
                bounds,
                label,
                hint.as_ref(),
                *icon,
                *danger,
                *disabled,
                *size,
                style,
                &self.style_model.palette,
                self.style_model.metrics,
                self.chrome_text_measure(id),
            )),
            StandardVisual::TreeView { rows, size } => Some(crate::tree_view::tree_view_geometry(
                bounds,
                rows,
                *size,
                &self.style_model.palette,
                self.style_model.metrics,
            )),
            StandardVisual::CommandPalette {
                title,
                query,
                placeholder,
                empty,
                rows,
            } => Some(crate::command_palette::command_palette_geometry(
                bounds,
                title,
                query,
                placeholder,
                empty.as_ref(),
                rows,
                &self.style_model.palette,
                self.style_model.metrics,
                self.chrome_text_measure(id),
            )),
            StandardVisual::QrCode { modules, width } => {
                let (module_size, (ox, oy)) = crate::qr_code::module_geometry(bounds, *width);
                let quiet = crate::qr_code::QUIET_ZONE_MODULES as f32;
                let dark = modules
                    .iter()
                    .enumerate()
                    .filter(|(_, dark)| **dark)
                    .map(|(index, _)| {
                        let x = index % *width;
                        let y = index / *width;
                        LayoutBox {
                            x: bounds.x + ox + (x as f32 + quiet) * module_size,
                            y: bounds.y + oy + (y as f32 + quiet) * module_size,
                            width: module_size,
                            height: module_size,
                        }
                    })
                    .collect();
                Some(crate::ComponentGeometry::QrCode {
                    field: LayoutBox {
                        x: bounds.x + ox,
                        y: bounds.y + oy,
                        width: module_size * (*width as f32 + quiet * 2.0),
                        height: module_size * (*width as f32 + quiet * 2.0),
                    },
                    module_size,
                    dark,
                })
            }
            #[cfg(feature = "calendar")]
            StandardVisual::CalendarHeatmap {
                cells,
                month_labels,
                day_labels,
                cell_size,
                max_level,
                active,
                active_title,
                ..
            } => Some(calendar_heatmap_geometry(
                bounds,
                cells,
                month_labels,
                day_labels,
                *cell_size,
                *max_level,
                *active,
                active_title.as_deref(),
                self.style_model.theme_mode,
                &self.style_model.palette,
                self.chrome_text_measure(id),
            )),
            #[cfg(feature = "charts")]
            StandardVisual::DonutChart {
                slices,
                cutout,
                separator,
                active,
            } => {
                let mut chart = crate::DonutChart::new(slices.iter().copied()).cutout(*cutout);
                chart.separator = *separator;
                chart.active = *active;
                let (width, regions) = chart.ring_regions(bounds);
                Some(crate::ComponentGeometry::DonutChart {
                    width,
                    regions: regions
                        .into_iter()
                        .map(|(circle, polygon, role)| {
                            (
                                circle,
                                polygon,
                                self.style_model.color(role).as_rgba_array(),
                            )
                        })
                        .collect(),
                })
            }
            #[cfg(feature = "charts")]
            StandardVisual::StackedTimeSeriesChart {
                title,
                values,
                layers,
                labels,
                active,
            } => Some(stacked_time_series_geometry(
                bounds,
                title,
                values,
                layers,
                labels,
                *active,
                &self.style_model.palette,
                self.style_model.opacity,
                self.chrome_text_measure(id),
            )),
            #[cfg(feature = "charts")]
            StandardVisual::TimeSeriesChart { values } => Some(time_series_geometry(
                bounds,
                values,
                self.style_model.theme_mode,
            )),
            #[cfg(feature = "charts")]
            StandardVisual::TimestampSeriesChart {
                samples,
                unit,
                time_labels,
            } => Some(timestamp_series_geometry(
                bounds,
                samples,
                unit.as_deref(),
                time_labels.as_ref(),
                self.style_model.theme_mode,
            )),
            #[cfg(feature = "controls")]
            StandardVisual::ReorderList {
                rows,
                size,
                spacing,
                insert,
            } => {
                // Live row children own their label painting; a stale retained
                // `rows` (projected before the children were attached) must not
                // paint a second copy underneath them.
                let rows = if self
                    .nodes
                    .get(id)
                    .is_some_and(|node| !node.hierarchy.children.is_empty())
                {
                    Arc::<[crate::ReorderRowPaint]>::from([])
                } else {
                    rows.clone()
                };
                Some(reorder_list_geometry(
                    bounds,
                    &rows,
                    *size,
                    *spacing,
                    *insert,
                    &self.style_model.palette,
                    self.style_model.metrics,
                ))
            }
            #[cfg(feature = "rich-text")]
            StandardVisual::NativeMarkdown {
                text,
                blocks,
                selection,
            } => {
                let selection_range = selection;
                let (text, selection_color) =
                    selectable_text_regions(content, text, style, &self.style_model.palette);
                let geometry = self.markdown_layout(id, blocks, content);
                Some(crate::ComponentGeometry::NativeMarkdown {
                    drawing: crate::markdown_drawing::document_with_geometry(
                        blocks, content, &geometry,
                    ),
                    text,
                    selection: match selection_range {
                        Some((start, end)) => geometry
                            .blocks
                            .into_iter()
                            .flat_map(|block| block.graphemes)
                            .filter(|g| g.index >= *start.min(end) && g.index < *start.max(end))
                            .map(|g| g.bounds)
                            .collect(),
                        _ => Vec::new(),
                    },
                    selection_color,
                })
            }
            #[cfg(feature = "rich-text")]
            StandardVisual::SelectableRichText { text, selection } => {
                let selection = self.selectable_rich_text_highlights(id, content, text, *selection);
                let (text, selection_color) =
                    selectable_text_regions(content, text, style, &self.style_model.palette);
                Some(crate::ComponentGeometry::SelectableRichText {
                    text,
                    selection,
                    selection_color,
                })
            }
            #[cfg(feature = "graph-canvas")]
            StandardVisual::GraphCanvas {
                nodes,
                ports,
                edges,
                connecting,
                grid_spacing,
                viewport_offset_x,
                viewport_offset_y,
                viewport_zoom,
            } => Some(graph_canvas_geometry(
                bounds,
                nodes,
                ports,
                edges,
                connecting.as_ref(),
                *grid_spacing,
                *viewport_offset_x,
                *viewport_offset_y,
                *viewport_zoom,
                &self.style_model.palette,
            )),
            #[cfg(feature = "graph-canvas")]
            StandardVisual::GraphMinimap {
                bounds: model_bounds,
                nodes,
                indicator,
                node_fill,
            } => Some(graph_minimap_geometry(
                bounds,
                *model_bounds,
                nodes,
                indicator.as_ref(),
                *node_fill,
                &self.style_model.palette,
                self.style_model.opacity,
            )),
            #[cfg(feature = "image-viewer")]
            StandardVisual::ImageViewer {
                intrinsic_size,
                name,
                metadata,
                zoom,
                offset_x,
                offset_y,
            } => Some(image_viewer_geometry(
                bounds,
                *intrinsic_size,
                name.as_ref(),
                metadata.as_ref(),
                *zoom,
                *offset_x,
                *offset_y,
                &self.style_model.palette,
                self.theme.effects().media_scrim,
                self.style_model.metrics,
            )),
            StandardVisual::KeyCaptureLayer { recording } => Some(key_capture_geometry(
                content,
                *recording,
                self.style_model,
                self.chrome_text_measure(id),
            )),
            StandardVisual::KeymapLayer => Some(keymap_geometry(
                content,
                self.style_model,
                self.chrome_text_measure(id),
            )),
            _ => None,
        }
    }
}

/// A vertical editor's component geometry (#59): the value, caret, selection
/// and preedit turned from text space onto the page through `frame`, and the
/// code-editor chrome dropped.
///
/// Gutters, guides, minimaps, inline diagnostics and anchored popups are laid
/// out against horizontal lines — a line-number column, a label after the
/// line's end, a popup under the caret's line — and there is no honest column
/// version of them to fall back to, so a vertical editor has none rather than
/// ones drawn across its columns.
fn vertical_text_input_geometry(
    geometry: crate::ComponentGeometry,
    frame: &EditorFrame,
    presentation: &TextInputPresentation,
    multiline: bool,
    focused: bool,
) -> crate::ComponentGeometry {
    let crate::ComponentGeometry::TextInput {
        resize_grip,
        mut text,
        additional_caret_color,
        swatch_border_color,
        whitespace_color,
        line_labels_color,
        line_labels_font_size,
        background,
        border,
        border_width,
        focus_ring,
        selection_color,
        caret_color,
        preedit_color,
        steppers,
        ..
    } = geometry
    else {
        return geometry;
    };
    let line = presentation.line_height.max(1.0);
    let caret_at = |(inline, block): (f32, f32)| frame.field_rect(caret_rule(inline, block, line));
    // Along the columns is the page's height, across them its width.
    let (inline_extent, block_extent) = (
        presentation.content_size.height,
        presentation.content_size.width,
    );
    text.bounds = frame.text_bounds(inline_extent, block_extent, multiline);
    // The preedit underline runs beside the column, on its block-start side:
    // the right of a `vertical-rl` column.
    let preedit = presentation
        .preedit_lines
        .iter()
        .map(|rect| frame.field_rect(preedit_rule(*rect, false)))
        .collect();
    crate::ComponentGeometry::TextInput {
        resize_grip,
        text,
        multiline,
        scroll: multiline.then(|| frame.scroll_x(inline_extent, block_extent, multiline)),
        selection: presentation
            .selection_lines
            .iter()
            .map(|rect| frame.field_rect(*rect))
            .collect(),
        caret: focused.then(|| caret_at((presentation.caret_x, presentation.caret_y))),
        additional_carets: if focused {
            presentation
                .additional_carets
                .iter()
                .copied()
                .map(caret_at)
                .collect()
        } else {
            Vec::new()
        },
        additional_caret_color,
        preedit,
        diagnostic_markers: Vec::new(),
        diagnostic_labels: Vec::new(),
        match_markers: Vec::new(),
        swatch_markers: Vec::new(),
        swatch_border_color,
        atom_chips: Vec::new(),
        caret_line: None,
        bracket_markers: Vec::new(),
        occurrence_markers: Vec::new(),
        drop_indicator: None,
        whitespace_marks: Vec::new(),
        whitespace_color,
        wrap_guides: Vec::new(),
        indent_guides: Vec::new(),
        line_labels: Vec::new(),
        folds: crate::components::TextFoldGeometry::default(),
        git_marks: crate::components::TextGitGutterGeometry::default(),
        line_labels_color,
        line_labels_font_size,
        completion_popup: None,
        hover_popup: None,
        signature_popup: None,
        minimap: None,
        sticky_line: None,
        background,
        border,
        border_width,
        focus_ring,
        selection_color,
        caret_color,
        preedit_color,
        steppers,
    }
}

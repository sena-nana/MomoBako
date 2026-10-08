//! Text shaping, editor presentation and display mappings.

use super::*;
use std::collections::HashMap;

use crate::text_node::TextBackendEpoch;
use nana_text::TextEngine as _;

use crate::components::{
    TextDiagnosticHit, TextDiagnosticLabel, TextDiagnosticMark, TextDiagnosticSeverity,
    TextDiagnosticSpan,
};

pub(super) struct CountingShaper<'a, S: TextShaper> {
    pub(super) inner: &'a mut S,
    pub(super) cache: &'a mut crate::text_layout_cache::TextLayoutCache,
    pub(super) glyphs: &'a mut crate::GlyphCache,
    pub(super) runs: usize,
    pub(super) wrap_layouts: usize,
    /// Cache keys built, and the text bytes each one copied and hashed.
    pub(super) keys: usize,
    pub(super) key_bytes: usize,
    /// The host's font generation, read once for the pass's keys.
    font_generation: u64,
}

impl<'a, S: TextShaper> CountingShaper<'a, S> {
    pub(super) fn new(
        inner: &'a mut S,
        cache: &'a mut crate::text_layout_cache::TextLayoutCache,
        glyphs: &'a mut crate::GlyphCache,
    ) -> Self {
        let font_generation = inner.font_generation();
        Self {
            inner,
            cache,
            glyphs,
            runs: 0,
            wrap_layouts: 0,
            keys: 0,
            key_bytes: 0,
            font_generation,
        }
    }
}

impl<S: TextShaper> TextShaper for CountingShaper<'_, S> {
    fn font_generation(&self) -> u64 {
        self.inner.font_generation()
    }

    fn retains_measurement(&self, id: StableNodeId) -> bool {
        self.inner.retains_measurement(id)
    }

    fn text_engine(&self) -> Option<nana_text::SharedTextEngine> {
        self.inner.text_engine()
    }

    fn take_text_work(&mut self) -> nana_text::TextWorkCounters {
        self.inner.take_text_work()
    }

    fn with_text_probes<R>(
        &mut self,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
        consume: impl FnOnce(&mut dyn TextShaper) -> R,
    ) -> R {
        let Self {
            inner,
            cache,
            glyphs,
            runs,
            wrap_layouts,
            keys,
            key_bytes,
            font_generation,
        } = self;
        inner.with_text_probes(text, style, constraints, |prepared| {
            let mut adapter = PreparedCountingShaper {
                inner: prepared,
                cache,
                glyphs,
                runs,
                wrap_layouts,
                keys,
                key_bytes,
                font_generation: *font_generation,
            };
            consume(&mut adapter)
        })
    }

    fn horizontal_offset(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        offset: usize,
        style: &ComputedStyle,
    ) -> f32 {
        self.inner.horizontal_offset(id, text, offset, style)
    }

    fn text_position(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        offset: usize,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> (f32, f32, f32) {
        self.inner
            .text_position(id, text, offset, style, constraints)
    }

    fn text_caret_position(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        offset: usize,
        affinity: crate::TextAffinity,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> (f32, f32, f32) {
        self.inner
            .text_caret_position(id, text, offset, affinity, style, constraints)
    }

    fn text_hit_at_point(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        x: f32,
        y: f32,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> Option<crate::TextHit> {
        self.inner
            .text_hit_at_point(id, text, x, y, style, constraints)
    }

    fn text_caret_visual_step(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        offset: usize,
        affinity: crate::TextAffinity,
        rightwards: bool,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> Option<crate::TextHit> {
        self.inner.text_caret_visual_step(
            id,
            text,
            offset,
            affinity,
            rightwards,
            style,
            constraints,
        )
    }

    fn text_highlights(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        selection: (usize, usize),
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> Vec<LayoutBox> {
        self.inner
            .text_highlights(id, text, selection, style, constraints)
    }

    fn shape(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> TextMetrics {
        // A node the host measures from a retained layout: keying the cache
        // would copy and hash the whole text to find what the host answers
        // with one read. Editors are exactly that, and their text is the
        // longest in the document. The measurement still counts as a run the
        // cache did not answer -- only the key and the lookup are skipped.
        if self.inner.retains_measurement(id) {
            self.runs = self.runs.saturating_add(1);
            if constraints.wrap {
                self.wrap_layouts = self.wrap_layouts.saturating_add(1);
            }
            return inner_shape_cached(self.inner, id, text, style, constraints, self.glyphs);
        }
        let key = layout_cache_key(text, style, constraints, self.font_generation);
        self.keys += 1;
        self.key_bytes += text.value.len();
        if let Some(metrics) = layout_cache_lookup(self.cache, &key) {
            return metrics;
        }
        self.runs = self.runs.saturating_add(1);
        if constraints.wrap {
            self.wrap_layouts = self.wrap_layouts.saturating_add(1);
        }
        let metrics = inner_shape_cached(self.inner, id, text, style, constraints, self.glyphs);
        // An invalid measurement fails the pass; caching it would fail every
        // retry with the same answer the host has since stopped giving.
        if validate_text_metrics(id, metrics).is_ok() {
            self.cache.insert(key, metrics);
        }
        metrics
    }

    fn shape_cached(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
        _glyphs: &mut crate::GlyphCache,
    ) -> TextMetrics {
        self.shape(id, text, style, constraints)
    }
}

// Keep the host's prepared geometry batch while retaining Runtime measurement
// accounting for any shapes requested by editor decorations inside the batch.
struct PreparedCountingShaper<'a> {
    inner: &'a mut dyn TextShaper,
    cache: &'a mut crate::text_layout_cache::TextLayoutCache,
    glyphs: &'a mut crate::GlyphCache,
    runs: &'a mut usize,
    wrap_layouts: &'a mut usize,
    keys: &'a mut usize,
    key_bytes: &'a mut usize,
    font_generation: u64,
}
impl TextShaper for PreparedCountingShaper<'_> {
    fn font_generation(&self) -> u64 {
        self.inner.font_generation()
    }

    fn retains_measurement(&self, id: StableNodeId) -> bool {
        self.inner.retains_measurement(id)
    }

    fn text_engine(&self) -> Option<nana_text::SharedTextEngine> {
        self.inner.text_engine()
    }

    fn take_text_work(&mut self) -> nana_text::TextWorkCounters {
        self.inner.take_text_work()
    }

    fn shape(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> TextMetrics {
        // See [`CountingShaper::shape`]: no cache in front of a retained
        // measurement, and the run is still counted.
        if self.inner.retains_measurement(id) {
            *self.runs = self.runs.saturating_add(1);
            if constraints.wrap {
                *self.wrap_layouts = self.wrap_layouts.saturating_add(1);
            }
            return inner_shape_cached(self.inner, id, text, style, constraints, self.glyphs);
        }
        let key = layout_cache_key(text, style, constraints, self.font_generation);
        *self.keys += 1;
        *self.key_bytes += text.value.len();
        if let Some(metrics) = layout_cache_lookup(self.cache, &key) {
            return metrics;
        }
        *self.runs = self.runs.saturating_add(1);
        if constraints.wrap {
            *self.wrap_layouts = self.wrap_layouts.saturating_add(1);
        }
        let metrics = inner_shape_cached(self.inner, id, text, style, constraints, self.glyphs);
        // An invalid measurement fails the pass; caching it would fail every
        // retry with the same answer the host has since stopped giving.
        if validate_text_metrics(id, metrics).is_ok() {
            self.cache.insert(key, metrics);
        }
        metrics
    }
    fn shape_cached(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
        _glyphs: &mut crate::GlyphCache,
    ) -> TextMetrics {
        self.shape(id, text, style, constraints)
    }
    fn horizontal_offset(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        offset: usize,
        style: &ComputedStyle,
    ) -> f32 {
        self.inner.horizontal_offset(id, text, offset, style)
    }
    fn text_position(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        offset: usize,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> (f32, f32, f32) {
        self.inner
            .text_position(id, text, offset, style, constraints)
    }

    fn text_caret_position(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        offset: usize,
        affinity: crate::TextAffinity,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> (f32, f32, f32) {
        self.inner
            .text_caret_position(id, text, offset, affinity, style, constraints)
    }
    fn text_hit_at_point(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        x: f32,
        y: f32,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> Option<crate::TextHit> {
        self.inner
            .text_hit_at_point(id, text, x, y, style, constraints)
    }

    fn text_caret_visual_step(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        offset: usize,
        affinity: crate::TextAffinity,
        rightwards: bool,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> Option<crate::TextHit> {
        self.inner.text_caret_visual_step(
            id,
            text,
            offset,
            affinity,
            rightwards,
            style,
            constraints,
        )
    }

    fn text_highlights(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        selection: (usize, usize),
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> Vec<LayoutBox> {
        self.inner
            .text_highlights(id, text, selection, style, constraints)
    }
}

fn layout_cache_key(
    text: &TextContent,
    style: &ComputedStyle,
    constraints: crate::TextShapeConstraints,
    font_generation: u64,
) -> crate::text_layout_cache::TextLayoutKey {
    #[cfg(any(test, feature = "benchmark"))]
    {
        crate::text_shape_stats::note_key_build();
        crate::text_shape_stats::timed_key(|| {
            crate::text_layout_cache::TextLayoutKey::new(text, style, constraints, font_generation)
        })
    }
    #[cfg(not(any(test, feature = "benchmark")))]
    {
        crate::text_layout_cache::TextLayoutKey::new(text, style, constraints, font_generation)
    }
}

fn layout_cache_lookup(
    cache: &mut crate::text_layout_cache::TextLayoutCache,
    key: &crate::text_layout_cache::TextLayoutKey,
) -> Option<TextMetrics> {
    #[cfg(any(test, feature = "benchmark"))]
    {
        crate::text_shape_stats::note_lookup();
        crate::text_shape_stats::timed_lookup(|| cache.lookup(key))
    }
    #[cfg(not(any(test, feature = "benchmark")))]
    {
        cache.lookup(key)
    }
}

fn inner_shape_cached(
    shaper: &mut (impl TextShaper + ?Sized),
    id: StableNodeId,
    text: &TextContent,
    style: &ComputedStyle,
    constraints: crate::TextShapeConstraints,
    glyphs: &mut crate::GlyphCache,
) -> TextMetrics {
    #[cfg(any(test, feature = "benchmark"))]
    {
        crate::text_shape_stats::timed_inner_shape(|| {
            shaper.shape_cached(id, text, style, constraints, glyphs)
        })
    }
    #[cfg(not(any(test, feature = "benchmark")))]
    {
        shaper.shape_cached(id, text, style, constraints, glyphs)
    }
}

fn clone_shaped_text(
    world: &UiWorld,
    id: StableNodeId,
    presentation: Option<&TextInputPresentationSource>,
) -> TextContent {
    let clone = || {
        presentation.as_ref().map_or_else(
            || world.record(id).text.clone(),
            |source| source.text.clone(),
        )
    };
    #[cfg(any(test, feature = "benchmark"))]
    let text = crate::text_shape_stats::timed_clone(clone);
    #[cfg(not(any(test, feature = "benchmark")))]
    let text = clone();
    world.record_string_clone(text.value.len());
    #[cfg(any(test, feature = "benchmark"))]
    crate::text_shape_stats::note_clone(text.value.len());
    text
}

pub(super) fn shape_empty_state_text(
    id: StableNodeId,
    visual: &StandardVisual,
    inherited: &ComputedStyle,
    max_width: Option<f32>,
    shaper: &mut impl TextShaper,
) -> EmptyStateTextPresentation {
    let StandardVisual::EmptyState {
        title,
        message,
        compact,
        ..
    } = visual
    else {
        return EmptyStateTextPresentation::default();
    };
    let mut title_style = inherited.clone();
    title_style.font_size = if *compact {
        nana_ui_core::type_scale::META
    } else {
        nana_ui_core::type_scale::BODY
    };
    title_style.font_weight = Some(nana_ui_core::type_scale::SEMIBOLD);
    title_style.line_height = None;
    let mut message_style = inherited.clone();
    message_style.font_size = if *compact {
        nana_ui_core::type_scale::HINT
    } else {
        nana_ui_core::type_scale::META
    };
    message_style.font_weight = None;
    message_style.line_height = None;
    let constraints = crate::TextShapeConstraints {
        max_width,
        wrap: max_width.is_some(),
        shaping: crate::TextShaping::Auto,
        ..crate::TextShapeConstraints::default()
    };
    EmptyStateTextPresentation {
        title: shaper.shape(
            id,
            &TextContent {
                value: title.to_string().into(),
            },
            &title_style,
            constraints,
        ),
        message: message.as_ref().map(|message| {
            shaper.shape(
                id,
                &TextContent {
                    value: message.to_string().into(),
                },
                &message_style,
                constraints,
            )
        }),
    }
}

pub(super) fn shape_modal_text(
    id: StableNodeId,
    visual: &StandardVisual,
    inherited: &ComputedStyle,
    max_width: Option<f32>,
    shaper: &mut impl TextShaper,
) -> ModalTextPresentation {
    let StandardVisual::ModalFrame {
        title,
        description,
        body_text,
        ..
    } = visual
    else {
        return ModalTextPresentation::default();
    };
    let constraints = crate::TextShapeConstraints {
        max_width,
        wrap: max_width.is_some(),
        shaping: crate::TextShaping::Auto,
        ..Default::default()
    };
    let mut title_style = inherited.clone();
    title_style.font_size = nana_ui_core::type_scale::SECTION;
    title_style.font_weight = Some(nana_ui_core::type_scale::SEMIBOLD);
    title_style.line_height = None;
    let mut description_style = inherited.clone();
    description_style.font_size = nana_ui_core::type_scale::META;
    description_style.font_weight = None;
    description_style.line_height = None;
    let mut body_style = inherited.clone();
    body_style.font_size = crate::overlay_surfaces::MODAL_BODY_TEXT_SIZE;
    body_style.font_weight = None;
    body_style.line_height = None;
    ModalTextPresentation {
        title: shaper.shape(
            id,
            &TextContent {
                value: title.to_string().into(),
            },
            &title_style,
            constraints,
        ),
        description: description.as_ref().map(|value| {
            shaper.shape(
                id,
                &TextContent {
                    value: value.to_string().into(),
                },
                &description_style,
                constraints,
            )
        }),
        body: body_text.as_ref().map(|value| {
            shaper.shape(
                id,
                &TextContent {
                    value: value.to_string().into(),
                },
                &body_style,
                constraints,
            )
        }),
    }
}

pub(super) fn progress_geometry(
    bounds: LayoutBox,
    style: &ComputedStyle,
    value_ratio: f32,
    girth: f32,
    corner_radius: f32,
    label: Option<&Arc<str>>,
    cancellable: bool,
    default_label_color: [f32; 4],
) -> Option<crate::ComponentGeometry> {
    let ratio = value_ratio.clamp(0.0, 1.0);
    let girth = if girth.is_finite() && girth > 0.0 {
        girth
    } else {
        6.0
    };
    let cancel_size = 24.0_f32.min(bounds.height).min(bounds.width);
    let heading = if label.is_some() || cancellable {
        12.0_f32.max(if cancellable { cancel_size } else { 0.0 })
    } else {
        0.0
    };
    let cancel = cancellable.then(|| LayoutBox {
        x: bounds.x + (bounds.width - cancel_size).max(0.0),
        y: bounds.y + (heading - cancel_size).max(0.0) / 2.0,
        width: cancel_size,
        height: cancel_size,
    });
    let label_width = cancel
        .map(|cancel| (cancel.x - bounds.x - nana_ui_core::space::MD).max(0.0))
        .unwrap_or(bounds.width);
    let label_region = label.map(|label| crate::ComponentTextRegion {
        bounds: LayoutBox {
            x: bounds.x,
            y: bounds.y + (heading - nana_ui_core::type_scale::META).max(0.0) / 2.0,
            width: label_width,
            height: nana_ui_core::type_scale::META.min(bounds.height),
        },
        content: Arc::clone(label).into(),
        color: Some(style.color.unwrap_or(default_label_color)),
        font_size: nana_ui_core::type_scale::META,
        font_weight: Some(nana_ui_core::type_scale::MEDIUM),
    });
    let track = if heading > 0.0 {
        let track_y = bounds.y + heading + nana_ui_core::space::SM;
        LayoutBox {
            x: bounds.x,
            y: track_y,
            width: bounds.width,
            height: girth.min((bounds.y + bounds.height - track_y).max(0.0)),
        }
    } else {
        LayoutBox {
            x: bounds.x,
            y: bounds.y + (bounds.height - girth).max(0.0) / 2.0,
            width: bounds.width,
            height: girth.min(bounds.height),
        }
    };
    Some(crate::ComponentGeometry::Progress {
        fill: LayoutBox {
            width: track.width * ratio,
            ..track
        },
        track,
        label: label_region,
        cancel,
        corner_radius: corner_radius.max(0.0),
    })
}

pub(super) fn form_field_geometry(
    bounds: LayoutBox,
    size: ControlSize,
    label: &Arc<str>,
    hint: Option<&Arc<str>>,
    error: Option<&Arc<str>>,
    control: Option<crate::StableNodeId>,
    layout_box: &dyn Fn(crate::StableNodeId) -> Option<LayoutBox>,
    model: nana_ui_core::StyleModelRef,
) -> Option<crate::ComponentGeometry> {
    let (label_size, _gap, label_role, label_weight) =
        crate::form_surfaces::form_field_density(size);
    let label_height = crate::form_surfaces::form_field_label_line(size);
    let support = error.or(hint);
    let support_role = if error.is_some() {
        SemanticColorRole::Danger
    } else {
        SemanticColorRole::Muted
    };
    let support_height = nana_ui_core::type_scale::META.min(bounds.height);
    let support_y = (bounds.y + bounds.height - support_height).max(bounds.y);
    let (indicator, support_x) = if error.is_some() {
        let slot = nana_ui_core::space::XL;
        let diameter = slot * 10.0 / 24.0;
        (
            Some((
                LayoutBox {
                    x: bounds.x + (slot - diameter) / 2.0,
                    y: support_y + (support_height - diameter) / 2.0,
                    width: diameter,
                    height: diameter,
                },
                model.color(support_role).as_rgba_array(),
            )),
            bounds.x + slot + nana_ui_core::space::XS,
        )
    } else {
        (None, bounds.x)
    };
    Some(crate::ComponentGeometry::FormField {
        label: crate::ComponentTextRegion {
            bounds: LayoutBox {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: label_height.min(bounds.height),
            },
            content: Arc::clone(label).into(),
            color: Some(model.color(label_role).as_rgba_array()),
            font_size: label_size,
            font_weight: Some(label_weight),
        },
        support: support.map(|message| crate::ComponentTextRegion {
            bounds: LayoutBox {
                x: support_x,
                y: support_y,
                width: (bounds.x + bounds.width - support_x).max(0.0),
                height: support_height,
            },
            content: Arc::clone(message).into(),
            color: Some(model.color(support_role).as_rgba_array()),
            font_size: nana_ui_core::type_scale::HINT,
            font_weight: None,
        }),
        indicator,
        control: control.and_then(layout_box),
    })
}

pub(super) fn text_input_placeholder_color(layout: &LayoutStyle, faint: [f32; 4]) -> [f32; 4] {
    let mut color = layout.placeholder_color.unwrap_or(faint);
    if let Some(opacity) = layout.placeholder_opacity {
        color[3] = (color[3] * opacity).clamp(0.0, 1.0);
    }
    color
}

/// 折叠摘要标记前缀：折叠起始行行尾显示 ` …N`（N 为隐藏行数）。
pub(super) const TEXT_FOLD_MARK_PREFIX: &str = " …";

/// 一个显示视图片段的形态：折叠替换（隐藏值区间并显示摘要）或纯插入
/// （inlay，值域空区间 + 锚点处插入装饰文本）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TextDisplaySpanKind {
    /// 折叠替换：隐藏 `[value_start, value_end)` 并显示 ` …N` 摘要。
    Fold {
        fold: crate::TextCodeFold,
        hidden_lines: u32,
    },
    /// 纯插入（inlay）：`value_start == value_end` 为锚点，锚点处插入
    /// `label` 文本。插入区间内部无 caret 边界（`value_of` 钳到锚点）。
    Inlay,
}

/// 显示视图的一个映射片段。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextDisplaySpan {
    /// 值空间区间 `[value_start, value_end)`；插入型为空区间（锚点）。
    pub value_start: usize,
    pub value_end: usize,
    /// 显示空间中替代/插入文本的起始偏移。
    pub display_start: usize,
    /// 替代/插入文本的字节长度。
    pub display_len: usize,
    /// 片段形态（折叠替换或纯插入）。
    pub kind: TextDisplaySpanKind,
}

/// 折叠/插入后的显示视图：`value` 是折叠区间替换为 ` …N` 摘要、inlay
/// 锚点处插入装饰文本后的显示文本；`spans` 按值空间顺序列出每个映射
/// 片段（折叠替换与纯插入两类）。几何、点击命中、光标移动都以显示
/// 视图为准；编辑命令仍按原始值语义处理（折叠与 inlay 都不改值）。
///
/// Cloning is O(1): the view is built once per change of what it is built
/// from (Issue #182) and handed to every consumer — presentation, geometry,
/// extraction, caret motion — rather than rebuilt by each. `value` carries a
/// stamp, so a host that laid it out recognises it without comparing.
#[derive(Debug, Clone)]
pub(crate) struct TextDisplayView {
    pub value: crate::TextValue,
    pub spans: Arc<[TextDisplaySpan]>,
}

impl TextDisplayView {
    /// 值空间偏移 → 显示空间偏移。折叠：落在隐藏区间内部时钳制到该折叠
    /// 的替代文本起点（折叠起始行行尾）。inlay：锚点映射到插入文本
    /// 起点（插入文本渲染在锚点字符之前），锚点之后的偏移平移插入长度。
    pub fn display_of(&self, offset: usize) -> usize {
        let mut delta = 0isize;
        for span in self.spans.iter() {
            if offset <= span.value_start {
                break;
            }
            if offset >= span.value_end {
                delta += span.display_len as isize - (span.value_end - span.value_start) as isize;
            } else {
                return span.display_start;
            }
        }
        ((offset as isize + delta).max(0)) as usize
    }

    /// 显示空间偏移 → 值空间偏移。折叠：落在替代文本内部时钳制到折叠
    /// 起始行的行尾。inlay：落在插入文本内部（含右边界）时钳到锚点
    /// （插入区间内部无 caret 边界，点击穿透吸附锚点处的缓冲字符）。
    pub fn value_of(&self, display: usize) -> usize {
        let mut delta = 0isize;
        for span in self.spans.iter() {
            let display_end = span.display_start + span.display_len;
            if display <= span.display_start {
                break;
            }
            if display >= display_end {
                delta += span.display_len as isize - (span.value_end - span.value_start) as isize;
            } else {
                return span.value_start;
            }
        }
        ((display as isize - delta).max(0)) as usize
    }

    /// 值空间偏移是否严格落在该片段的隐藏区间内部（折叠起始行行尾不算）。
    /// 插入型片段没有隐藏区间，恒为 `false`。
    pub fn span_hides(&self, span: &TextDisplaySpan, offset: usize) -> bool {
        offset > span.value_start && offset < span.value_end
    }

    /// 右移落在这里时跨进的覆盖区间：严格落在折叠摘要 / inlay 插入文本
    /// 内部（区间内部没有 caret 边界，按 [`Self::value_of`] 会被钳回区间
    /// 起点的值偏移），或 `stepping` 时恰在 inlay 插入文本末端——单字素
    /// 的标签一步就走到末端，同样映射回锚点，和起点相同。只有逐步向右的
    /// 意图（Right / WordRight）算这一种：End 停在行尾 inlay 之后映射回
    /// 锚点，本来就该停在那里。一次查找供调用方的全部判断使用。
    pub fn crossed_cover(&self, display: usize, stepping: bool) -> Option<&TextDisplaySpan> {
        self.spans.iter().find(|span| {
            span.crossed_at(display)
                && (stepping || display < span.display_start + span.display_len)
        })
    }

    /// 右向移动语义的显示→值映射：`span` 是目标跨进的覆盖区间（见
    /// [`Self::crossed_cover`]）。不钳回区间起点（那样逐字符右移会被钳成
    /// 空操作），而是跨过整个覆盖区间：折叠摘要 → 折叠后首字符；inlay →
    /// `bare`（调用方的移动意图在值文本上从原光标走出的一步：Right 一个
    /// 字素簇、WordRight 到词尾），与裸文本移动一致（按显示文本步进会把
    /// 标签文字算进词里）；这一步跨行落进折叠隐藏区间时（行尾锚点上的
    /// WordRight）再跨到折叠末端。点击命中与垂直移动保持钳制语义，不走
    /// 本映射。
    pub fn value_of_forward(&self, span: &TextDisplaySpan, bare: impl FnOnce() -> usize) -> usize {
        match &span.kind {
            // 折叠：区间末端即折叠后首字符的显示位。
            TextDisplaySpanKind::Fold { .. } => {
                self.value_of(span.display_start + span.display_len)
            }
            // 同锚点的插入（多条标签背靠背）映射到同一锚点，一步一并跨过。
            TextDisplaySpanKind::Inlay => {
                let next = bare();
                self.spans
                    .iter()
                    .find(|fold| fold.fold().is_some() && self.span_hides(fold, next))
                    .map_or(next, |fold| fold.value_end)
            }
        }
    }
}

impl TextDisplaySpan {
    /// 显示偏移严格在区间内部，或（仅 inlay）恰在插入文本末端。
    fn crossed_at(&self, display: usize) -> bool {
        let end = self.display_start + self.display_len;
        display > self.display_start
            && (display < end
                || (display == end && matches!(self.kind, TextDisplaySpanKind::Inlay)))
    }

    /// 折叠形态的区间数据；插入型（inlay）片段返回 `None`。
    pub(crate) fn fold(&self) -> Option<crate::TextCodeFold> {
        match &self.kind {
            TextDisplaySpanKind::Fold { fold, .. } => Some(*fold),
            TextDisplaySpanKind::Inlay => None,
        }
    }
}

/// 世界校验后的 inlay 集合：锚点必须是 `value` 的 char boundary 且不
/// 越界、文本非空且不含 `'\n'`（行数换算按 `\n` 计数，换行会破坏行高
/// 与行号）；按 `(offset, label)` 排序去重。非法条目钳除（照抄
/// [`crate::UiMutation::SetTextInputFoldCollapsed`] 的规范化风格，不做
/// 整体拒绝）——inlay 是宿主随语义快照防抖重喂的视图装饰，钳除让
/// 部分陈旧的喂入不至于拖垮整批。
pub(super) fn normalize_text_inlays(
    value: &str,
    inlays: &[crate::TextInlay],
) -> Vec<crate::TextInlay> {
    let mut normalized: Vec<crate::TextInlay> = inlays
        .iter()
        .filter(|inlay| {
            inlay.offset <= value.len()
                && value.is_char_boundary(inlay.offset)
                && !inlay.label.is_empty()
                && !inlay.label.contains('\n')
        })
        .cloned()
        .collect();
    normalized.sort_by(|a, b| (&a.offset, &a.label).cmp(&(&b.offset, &b.label)));
    normalized.dedup();
    normalized
}

/// 由折叠态区间与 inlay 集合构建显示视图；两者都为空时返回 `None`
/// （零分配短路）。
///
/// 嵌套折叠：子折叠的隐藏区间与前一个已接受区间重叠（即完全落在父折叠
/// 的隐藏范围内）时跳过——父折叠已经把这些行隐藏。inlay 锚点落在折叠
/// 隐藏区间内（锚点字符被替换）时丢弃，照抄诊断/匹配 span 的隐藏丢弃
/// 先例，不强制展开；同锚点的多条 inlay 按标签序依次插入。
pub(super) fn build_text_display_view(
    value: &str,
    collapsed: &[crate::TextCodeFold],
    inlays: &[crate::TextInlay],
) -> Option<TextDisplayView> {
    if collapsed.is_empty() && inlays.is_empty() {
        return None;
    }
    // 折叠与插入统一按值空间位置归并：折叠取隐藏起点（替换区间的键），
    // inlay 取锚点。同键时折叠在前——锚点等于隐藏起点的 inlay 其锚点
    // 字符已被摘要替换，随后按「锚点已被消费」丢弃。
    enum Segment<'a> {
        Fold(crate::TextCodeFold),
        Inlay(&'a crate::TextInlay),
    }
    let mut segments: Vec<(usize, Segment<'_>)> =
        Vec::with_capacity(collapsed.len() + inlays.len());
    segments.extend(
        collapsed
            .iter()
            .map(|fold| (fold.hidden_start_in(value), Segment::Fold(*fold))),
    );
    segments.extend(
        inlays
            .iter()
            .map(|inlay| (inlay.offset, Segment::Inlay(inlay))),
    );
    segments.sort_by_key(|(position, segment)| (*position, segment_rank(segment)));

    fn segment_rank(segment: &Segment<'_>) -> u8 {
        match segment {
            Segment::Fold(_) => 0,
            Segment::Inlay(_) => 1,
        }
    }

    let mut display = String::with_capacity(value.len());
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    for (_, segment) in &segments {
        match segment {
            Segment::Fold(fold) => {
                let fold = *fold;
                if fold.start >= fold.end || fold.end > value.len() {
                    continue;
                }
                let hidden_start = fold.hidden_start_in(value);
                if hidden_start >= fold.end || hidden_start < cursor {
                    // 单行区间没有可隐藏的行；与前一个折叠重叠的子折叠
                    // 不重复隐藏。
                    continue;
                }
                display.push_str(&value[cursor..hidden_start]);
                let display_start = display.len();
                let hidden_lines = value[hidden_start..fold.end].matches('\n').count();
                display.push_str(TEXT_FOLD_MARK_PREFIX);
                display.push_str(&hidden_lines.to_string());
                spans.push(TextDisplaySpan {
                    value_start: hidden_start,
                    value_end: fold.end,
                    display_start,
                    display_len: display.len() - display_start,
                    kind: TextDisplaySpanKind::Fold {
                        fold,
                        hidden_lines: hidden_lines as u32,
                    },
                });
                cursor = fold.end;
            }
            Segment::Inlay(inlay) => {
                // 锚点越界/非边界（防御：正常路径已在世界校验钳除）或
                // 已被前面的折叠消费（锚点字符被摘要替换）时丢弃。
                if inlay.offset > value.len()
                    || !value.is_char_boundary(inlay.offset)
                    || inlay.offset < cursor
                {
                    continue;
                }
                display.push_str(&value[cursor..inlay.offset]);
                let display_start = display.len();
                display.push_str(&inlay.label);
                spans.push(TextDisplaySpan {
                    value_start: inlay.offset,
                    value_end: inlay.offset,
                    display_start,
                    display_len: inlay.label.len(),
                    kind: TextDisplaySpanKind::Inlay,
                });
                cursor = inlay.offset;
            }
        }
    }
    if spans.is_empty() {
        return None;
    }
    display.push_str(&value[cursor..]);
    Some(TextDisplayView {
        value: crate::TextValue::stamped(display),
        spans: spans.into(),
    })
}

/// 值空间 span 端点对 `[start, end)` 经显示视图重投到显示空间的可见片段：
/// - 起点落在隐藏区间内部的片段从该折叠之后起（钳到摘要之后，摘要文本
///   不着色）；
/// - 跨折叠区间的片段在区间边界切分为可见前后两段；
/// - 完全落入隐藏区间的片段丢弃；
/// - 片段起点与 inlay 插入点重合时前进到插入文本之后（插入文本由
///   inlay 自己的着色 span 整段覆盖，值空间 span 不越过）。
///
/// 输出片段按折叠顺序排列，均落在可见文本上（不覆盖任何摘要文本）。
pub(super) fn remap_span_to_display(
    span: (usize, usize),
    view: &TextDisplayView,
) -> Vec<(usize, usize)> {
    /// 显示起点越过紧邻其后的 inlay 插入区间（同锚点连续多条时链式
    /// 前进；spans 按显示顺序排列）。
    fn skip_inlay_prefix(view: &TextDisplayView, mut start: usize) -> usize {
        for span in view.spans.iter() {
            if matches!(span.kind, TextDisplaySpanKind::Inlay) && span.display_start == start {
                start = span.display_start + span.display_len;
            }
        }
        start
    }
    let mut pieces = Vec::new();
    let mut cursor = span.0;
    for region in view.spans.iter() {
        if span.1 <= region.value_start {
            break;
        }
        if cursor >= region.value_end {
            continue;
        }
        if cursor < region.value_start {
            let start = skip_inlay_prefix(view, view.display_of(cursor));
            let end = view.display_of(region.value_start);
            if start < end {
                pieces.push((start, end));
            }
        }
        cursor = region.value_end;
        if cursor >= span.1 {
            return pieces;
        }
    }
    if cursor < span.1 {
        let start = skip_inlay_prefix(view, view.display_of(cursor));
        let end = view.display_of(span.1);
        if start < end {
            pieces.push((start, end));
        }
    }
    pieces
}

/// 最小变更区间：`(old 中被替换的 start, old 中被替换的 end, 长度差)`。
/// 与 [`crate::text_editing`] 的 transform diff 同构：按公共前后缀夹取。
pub(super) fn value_edit_span(old: &str, new: &str) -> (usize, usize, isize) {
    let prefix = old
        .chars()
        .zip(new.chars())
        .take_while(|(current, candidate)| current == candidate)
        .map(|(character, _)| character.len_utf8())
        .sum::<usize>();
    let suffix = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(current, candidate)| current == candidate)
        .map(|(_, character)| character.len_utf8())
        .sum::<usize>();
    let suffix = suffix.min(old.len() - prefix).min(new.len() - prefix);
    (
        prefix,
        old.len() - suffix,
        new.len() as isize - old.len() as isize,
    )
}

/// 值被编辑后的折叠态重映射（确定性策略）：
/// 1. 折叠区间与被编辑区间相交 → 受影响折叠自动展开；
/// 2. 完全在被编辑区间之后的折叠按长度差整体平移；
/// 3. 其余保持不动。平移后再按新值校验可折叠性，失效的展开。
pub(super) fn remap_collapsed_after_edit(
    collapsed: &[crate::TextCodeFold],
    new_value: &str,
    changed_start: usize,
    changed_end: usize,
    delta: isize,
) -> Vec<crate::TextCodeFold> {
    let mut next = Vec::with_capacity(collapsed.len());
    for &fold in collapsed {
        if fold.end > changed_start && fold.start < changed_end {
            continue;
        }
        let shift = if fold.start >= changed_end { delta } else { 0 };
        let start = (fold.start as isize + shift).max(0) as usize;
        let end = (fold.end as isize + shift).max(0) as usize;
        let fold = crate::TextCodeFold::new(start.min(new_value.len()), end.min(new_value.len()));
        if fold.collapsible_in(new_value) {
            next.push(fold);
        }
    }
    next.sort_by_key(|fold| (fold.start, fold.end));
    next.dedup();
    next
}

/// 值被编辑后的 snippet 跳位重映射：跳位落在被编辑区间内 → 会话失效
/// （`None`）；否则按长度差平移并钳制到新值的字符边界。
pub(super) fn remap_snippet_session(
    session: &crate::components::TextSnippetSession,
    new_value: &str,
    changed_start: usize,
    changed_end: usize,
    delta: isize,
) -> Option<crate::components::TextSnippetSession> {
    let mut stops = Vec::with_capacity(session.stops.len());
    for (index, &stop) in session.stops.iter().enumerate() {
        if stop > changed_start && stop < changed_end {
            return None;
        }
        let placeholder_start = session
            .selection_ends
            .get(index)
            .is_some_and(|end| *end > stop);
        let mapped = if stop >= changed_end && !(placeholder_start && stop == changed_start) {
            (stop as isize + delta).max(0) as usize
        } else {
            stop
        };
        let mapped = mapped.min(new_value.len());
        if !new_value.is_char_boundary(mapped) {
            return None;
        }
        stops.push(mapped);
    }
    let mut selection_ends = Vec::with_capacity(session.selection_ends.len());
    for &end in &session.selection_ends {
        if end > changed_start && end < changed_end {
            return None;
        }
        let mapped = if end >= changed_end {
            end.checked_add_signed(delta)?
        } else {
            end
        };
        if !new_value.is_char_boundary(mapped) {
            return None;
        }
        selection_ends.push(mapped);
    }
    Some(crate::components::TextSnippetSession {
        stops,
        selection_ends,
        index: session.index,
        exit_on_last: session.exit_on_last,
        placeholders: if session.placeholders.is_empty() {
            Vec::new()
        } else {
            return None;
        },
    })
}

/// 宿主重喂折叠区间后的折叠态保留策略（确定性）：
/// 1. 与新区间完全一致的条目保留；
/// 2. 其余条目尝试整体位移匹配：上一次喂入与本次喂入数量相等、逐位
///    配对长度相等且 start 差唯一非零（典型场景：折叠区上方的编辑使
///    所有区间平移同一偏移）时，把条目按该位移平移，命中新区间的保留；
/// 3. 其余失效条目自动展开。
pub(super) fn reconcile_collapsed_folds(
    previous_offered: &[crate::TextCodeFold],
    offered: &[crate::TextCodeFold],
    collapsed: &[crate::TextCodeFold],
) -> Vec<crate::TextCodeFold> {
    if collapsed.is_empty() {
        return Vec::new();
    }
    let mut next: Vec<crate::TextCodeFold> = collapsed
        .iter()
        .filter(|fold| offered.contains(fold))
        .copied()
        .collect();
    let shift = (previous_offered.len() == offered.len())
        .then(|| {
            let first = offered.first()?.start as isize - previous_offered.first()?.start as isize;
            (first != 0
                && previous_offered
                    .iter()
                    .zip(offered.iter())
                    .all(|(previous, current)| {
                        current.start as isize - previous.start as isize == first
                            && current.end - current.start == previous.end - previous.start
                    }))
            .then_some(first)
        })
        .flatten();
    if let Some(shift) = shift {
        for &fold in collapsed {
            if next.contains(&fold) {
                continue;
            }
            let shifted = crate::TextCodeFold::new(
                (fold.start as isize + shift).max(0) as usize,
                (fold.end as isize + shift).max(0) as usize,
            );
            if offered.contains(&shifted) && !next.contains(&shifted) {
                next.push(shifted);
            }
        }
    }
    next.sort_by_key(|fold| (fold.start, fold.end));
    next.dedup();
    next
}

#[derive(Debug, Clone)]
pub(super) struct TextInputPresentationSource {
    pub(super) text: TextContent,
    pub(super) placeholder: bool,
    pub(super) selection: Option<(usize, usize)>,
    pub(super) caret: usize,
    /// `caret` 落在软换行 / BiDi 边界时画在哪一侧。占位符与组字期的 caret
    /// 由本模块自己派生（不是用户落的点），一律 downstream。
    pub(super) caret_affinity: crate::TextAffinity,
    /// 附加多光标的显示空间 `(start, end)` 区间与 caret 的 affinity（收起时
    /// 光标也在 `caret` 表）。
    pub(super) additional: Vec<(usize, usize, crate::TextAffinity)>,
    pub(super) preedit: Option<(usize, usize)>,
    pub(super) multiline: bool,
    /// 代码编辑器扩展：诊断标记 / 查找匹配高亮 / 行号栏（占位符态跳过行号）。
    pub(super) diagnostics: Arc<[TextDiagnosticSpan]>,
    pub(super) matches: Arc<[TextMatchSpan]>,
    /// 颜色装饰 span（宿主喂入；仅多行态派生几何）。
    pub(super) color_swatches: Arc<[TextColorSwatchSpan]>,
    pub(super) atoms: Arc<[crate::TextAtomSpan]>,
    pub(super) line_numbers: bool,
    pub(super) indent_guides: Option<Arc<str>>,
    /// git gutter 标记：宿主行号已校验并映射为显示行索引（0 基）；行号
    /// 无效或所在行被折叠隐藏的标记在构建时剔除。
    pub(super) git_marks: Arc<[(u32, TextGitMarkKind)]>,
    /// 内部派生渲染选项（出现高亮、相对行号、空白显示、wrap guide）。
    /// 占位符/IME 组合态置默认（全部关闭）。
    pub(super) editor: TextEditorRenderOptions,
    /// 节点是否持有文档焦点。出现高亮按聚焦派生（不聚焦零分配跳过，
    /// 绘制层再按焦点门控一次）。
    pub(super) focused: bool,
    /// 折叠显示视图（存在折叠态区间时 Some；`text` 等偏移已在显示空间）。
    pub(super) fold: Option<TextDisplayView>,
    /// 补全候选（宿主过滤后的非空列表；占位符/组合态不弹出）。
    pub(super) completions: Option<Arc<[crate::TextCompletion]>>,
    /// hover 文档（宿主喂入时 Some）。
    pub(super) hover: Option<crate::TextHover>,
    /// minimap 行条长度表（原始文档每逻辑行的非空白字符数，含折叠隐藏
    /// 行）。仅开启选项的多行态收集，其余为空向量（零分配短路）。
    pub(super) minimap_line_lengths: Vec<u32>,
    /// 括号配对着色 span 表 `(start, end, depth)`（显示空间；未配对括号
    /// depth 为 [`TEXT_BRACKET_UNMATCHED_DEPTH`]）。仅多行且开启选项的
    /// 非占位/非组合态收集，随文本版本 memo。
    pub(super) bracket_color_spans: Arc<[(usize, usize, usize)]>,
}

/// 从 [`StandardVisual::TextInput`] 提取的代码编辑器扩展。
#[derive(Debug, Clone, Default)]
pub(super) struct TextInputEditorExtras {
    pub(super) diagnostics: Arc<[TextDiagnosticSpan]>,
    pub(super) matches: Arc<[TextMatchSpan]>,
    pub(super) color_swatches: Arc<[TextColorSwatchSpan]>,
    pub(super) atoms: Arc<[crate::TextAtomSpan]>,
    pub(super) line_numbers: bool,
    pub(super) indent_guides: Option<Arc<str>>,
    pub(super) git_marks: Arc<[TextGitMark]>,
    pub(super) editor: TextEditorRenderOptions,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_text_input_presentation_source(
    state: crate::TextInputView<'_>,
    ime: Option<crate::ImeView<'_>>,
    placeholder: &str,
    secure: bool,
    multiline: bool,
    extras: TextInputEditorExtras,
    focused: bool,
    fold: Option<TextDisplayView>,
    completions: Option<Arc<[crate::TextCompletion]>>,
    hover: Option<crate::TextHover>,
    composed_display: Option<crate::TextValue>,
) -> TextInputPresentationSource {
    use unicode_segmentation::UnicodeSegmentation;

    // 浮层是打字态的编辑辅助：占位符与 IME 组合期间一律不弹出。
    let (completions, hover) = if (state.value.is_empty() && !placeholder.is_empty())
        || ime.is_some_and(|ime| !ime.text.is_empty())
    {
        (None, None)
    } else {
        (completions, hover)
    };

    // minimap 行长收集不在源构造内进行：占位符与 IME 组合态的编辑器选项
    // 归默认（不显示 minimap），收集结果只会被丢弃；仅多行且开启选项时
    // 由 [`UiWorld::text_input_presentation_source`] 在早退判定之后收集。

    let mask = |value: &str| {
        if secure {
            "•".repeat(value.graphemes(true).count())
        } else {
            value.to_owned()
        }
    };
    let display_offset = |value: &str, offset: usize| {
        if secure {
            value[..offset].graphemes(true).count() * "•".len()
        } else {
            offset
        }
    };
    if state.value.is_empty() && ime.is_none() && !placeholder.is_empty() {
        return TextInputPresentationSource {
            text: TextContent {
                value: placeholder.to_owned().into(),
            },
            placeholder: true,
            selection: None,
            caret: 0,
            caret_affinity: crate::TextAffinity::Downstream,
            additional: Vec::new(),
            preedit: None,
            multiline,
            diagnostics: extras.diagnostics,
            matches: extras.matches,
            // 占位文本不是文档内容，颜色装饰 span 不派生几何。
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            // 占位符态没有真实文档内容，git 标记随行号栏一并跳过（避免在
            // 占位文本旁渲染指向不存在行的标记）。
            git_marks: Arc::from([]),
            editor: TextEditorRenderOptions::default(),
            focused: false,
            fold: None,
            completions: None,
            hover: None,
            minimap_line_lengths: Vec::new(),
            bracket_color_spans: Arc::from([]),
        };
    }

    // 折叠视图：secure 掩码与折叠互斥（折叠是代码编辑器特性）；诊断/
    // 匹配 span 完全落在隐藏区间内时随行隐藏（丢弃，不强制展开）。
    // What is drawn when nothing is composing: the committed text itself —
    // a shared copy naming its bytes, not a copy of them — unless masked.
    let (fold_view, base_value): (Option<TextDisplayView>, crate::TextValue) = match fold {
        Some(view) if !secure => {
            let value = view.value.clone();
            (Some(view), value)
        }
        _ if secure => (None, mask(state.value).into()),
        _ => (None, state.value_shared()),
    };
    let map_offset = |offset: usize| -> usize {
        match &fold_view {
            Some(view) => view.display_of(offset),
            None => offset,
        }
    };
    let selection = if state.selection.is_valid_for(state.value) {
        state.selection
    } else {
        crate::TextSelection::caret(state.value.len())
    };
    // An IME attached with nothing composed (the empty preedit between a
    // platform's keystrokes) draws the committed text: it is not a
    // composition, and treating it as one dropped line numbers and cursors.
    if let Some(ime) = ime.filter(|ime| !ime.text.is_empty()) {
        // The committed range the preedit stands in for, as the session keeps
        // it (the primary selection when the composition started, moved by
        // any edit since); the selection for a state that is not a session's.
        let replaced = state
            .session()
            .composition()
            .map(|composition| composition.replaced.clone())
            .filter(|replaced| replaced.end <= state.value.len())
            .unwrap_or_else(|| selection.ordered());
        // Unmasked and unfolded, the display is the session's own display
        // text (`composed_display`, kept by the world). Folds and masks are
        // Runtime overlays: the preedit is spliced into them here — 折叠态
        // 组合拼接在显示视图上进行；安全输入先切片后掩码，显示偏移按字形重算。
        let (composed, preedit_start): (crate::TextValue, usize) = if let Some(view) = &fold_view {
            let start = view.display_of(replaced.start).min(base_value.len());
            let end = view.display_of(replaced.end).min(base_value.len());
            let composed = format!("{}{}{}", &base_value[..start], ime.text, &base_value[end..]);
            (composed.into(), start)
        } else if secure {
            let prefix = mask(&state.value[..replaced.start]);
            let composed = format!("{prefix}{}{}", ime.text, mask(&state.value[replaced.end..]));
            (composed.into(), prefix.len())
        } else {
            let composed = composed_display.unwrap_or_else(|| {
                let mut composed =
                    String::with_capacity(state.value.len() - replaced.len() + ime.text.len());
                composed.push_str(&state.value[..replaced.start]);
                composed.push_str(ime.text);
                composed.push_str(&state.value[replaced.end..]);
                composed.into()
            });
            (composed, replaced.start)
        };
        let preedit_end = preedit_start + ime.text.len();
        let ime_focus = ime
            .selection
            .map(|(_, focus)| focus)
            .filter(|focus| *focus <= ime.text.len() && ime.text.is_char_boundary(*focus))
            .unwrap_or(ime.text.len());
        // 多光标限制：组合输入只挂在主光标上，组合期隐藏附加光标。
        return TextInputPresentationSource {
            text: TextContent {
                value: composed.clone(),
            },
            placeholder: false,
            selection: None,
            caret: preedit_start + ime_focus,
            caret_affinity: crate::TextAffinity::Downstream,
            additional: Vec::new(),
            preedit: Some((preedit_start, preedit_end)),
            multiline,
            diagnostics: extras.diagnostics,
            matches: extras.matches,
            // 组合文本改变了字节布局，宿主偏移失效；组合期不派生 swatch。
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            // 组合期标记按原值行号继续锚定（与诊断一致，宿主拥有生命周期）。
            git_marks: map_git_marks(state.value, &composed, extras.git_marks, fold_view.as_ref()),
            editor: TextEditorRenderOptions::default(),
            focused,
            fold: fold_view,
            completions: None,
            hover: None,
            minimap_line_lengths: Vec::new(),
            bracket_color_spans: Arc::from([]),
        };
    }

    let anchor = map_offset(display_offset(state.value, selection.anchor));
    let focus = map_offset(display_offset(state.value, selection.focus));
    // 附加光标：校验 + 显示空间映射；单光标快速路径下向量为空、零分配。
    let additional = state
        .additional_selections
        .iter()
        .filter(|selection| selection.is_valid_for(state.value))
        .map(|selection| {
            let start = map_offset(display_offset(state.value, selection.anchor));
            let end = map_offset(display_offset(state.value, selection.focus));
            (start.min(end), start.max(end), selection.affinity)
        })
        .collect();
    // 诊断/匹配 span 端点映射到显示空间；完全被隐藏的 span 丢弃。
    let map_span = |span_offset: usize, length: usize| -> Option<(usize, usize)> {
        let start = span_offset.min(state.value.len());
        let end = span_offset
            .saturating_add(length.max(1))
            .min(state.value.len());
        if end <= start {
            return None;
        }
        if let Some(view) = &fold_view
            && view
                .spans
                .iter()
                .any(|span| start >= span.value_start && end <= span.value_end)
        {
            return None;
        }
        Some((map_offset(start), map_offset(end)))
    };
    let diagnostics = extras
        .diagnostics
        .iter()
        .filter_map(|span| {
            map_span(span.offset, span.length).map(|(start, end)| TextDiagnosticSpan {
                offset: start,
                length: end.saturating_sub(start).max(1),
                severity: span.severity,
                message: span.message.clone(),
            })
        })
        .collect::<Vec<_>>();
    let matches = extras
        .matches
        .iter()
        .filter_map(|span| map_span(span.offset, span.length).map(|_| span.clone()))
        .collect::<Vec<_>>();
    let color_swatches = extras
        .color_swatches
        .iter()
        .filter_map(|span| map_span(span.offset, span.length).map(|_| span.clone()))
        .collect::<Vec<_>>();
    let atoms = extras
        .atoms
        .iter()
        .filter_map(|span| {
            map_span(span.start, span.end.saturating_sub(span.start)).map(|(start, end)| {
                let mut next = span.clone();
                next.start = start;
                next.end = end;
                next
            })
        })
        .collect::<Vec<_>>();
    // git gutter 标记：宿主行号校验 + 折叠隐藏行剔除后映射为显示行索引。
    let git_marks = map_git_marks(
        state.value,
        &base_value,
        extras.git_marks,
        fold_view.as_ref(),
    );
    TextInputPresentationSource {
        text: TextContent { value: base_value },
        placeholder: false,
        selection: (anchor != focus).then_some((anchor.min(focus), anchor.max(focus))),
        caret: focus,
        caret_affinity: selection.affinity,
        additional,
        preedit: None,
        multiline,
        diagnostics: Arc::from(diagnostics),
        matches: Arc::from(matches),
        color_swatches: Arc::from(color_swatches),
        atoms: Arc::from(atoms),
        git_marks,
        line_numbers: extras.line_numbers,
        indent_guides: extras.indent_guides,
        editor: extras.editor,
        focused,
        fold: fold_view,
        completions,
        hover,
        minimap_line_lengths: Vec::new(),
        bracket_color_spans: Arc::from([]),
    }
}

/// git gutter 标记的行号映射：宿主行号（1 基）→ 显示行索引（0 基）。
/// `value` 是行号语义所属的原始值（行起点按它定位），`display` 是实际
/// 排版的显示值（行索引按它计数）。行号 0、超过文档逻辑行数（尾随换行
/// 不产生幻影行，与行号栏语义一致）或所在行被折叠隐藏的标记静默跳过。
/// 空列表原样返回（零分配零遍历）。
pub(super) fn map_git_marks(
    value: &str,
    display: &str,
    marks: Arc<[TextGitMark]>,
    view: Option<&TextDisplayView>,
) -> Arc<[(u32, TextGitMarkKind)]> {
    if marks.is_empty() {
        return Arc::from([]);
    }
    // 单趟构建行起点表：每标记按表定位行起点、按表计数显示行索引，不再
    // 逐标记从字节 0 回放（O(文档·标记数) → O(文档 + 标记·log 行)）。
    let value_line_starts = line_starts(value);
    let display_line_starts = line_starts(display);
    // 行数语义与行号栏一致：尾随换行不产生幻影行。
    let line_count = if value.ends_with('\n') {
        value_line_starts.len() - 1
    } else {
        value_line_starts.len()
    };
    marks
        .iter()
        .filter_map(|mark| {
            let line_index = usize::try_from(mark.line).ok()?.checked_sub(1)?;
            if line_index >= line_count {
                return None;
            }
            let line_start = value_line_starts[line_index];
            if view.is_some_and(|view| {
                view.spans
                    .iter()
                    .any(|span| view.span_hides(span, line_start))
            }) {
                return None;
            }
            let display_start = match view {
                Some(view) => view.display_of(line_start),
                None => line_start,
            }
            .min(display.len());
            // 显示行索引 = 显示起点前的换行数 = 非零行起点中 ≤ 起点的个数。
            let display_line =
                display_line_starts[1..].partition_point(|&start| start <= display_start);
            Some((display_line as u32, mark.kind))
        })
        .collect()
}

/// 文档行起点表：首项 0，其后每项为对应换行符后的第一个字节偏移。
pub(super) fn line_starts(value: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(value.match_indices('\n').map(|(index, _)| index + 1))
        .collect()
}

/// 括号配对着色的深度色阶：按嵌套深度循环取 5 个主题语义色
/// （accent 蓝 / success 绿 / warning 黄 / danger 红 / muted 中性灰），
/// 与调色板同源保证明暗主题都和谐；未配对括号用 faint 淡化前景（与
/// 语法高亮对 punctuation 的弱化一致）。
pub(super) fn bracket_depth_color(
    palette: &nana_ui_core::SemanticPalette,
    depth: usize,
) -> [f32; 4] {
    if depth == crate::components::TEXT_BRACKET_UNMATCHED_DEPTH {
        return palette.faint.as_rgba_array();
    }
    match depth % 5 {
        0 => palette.accent,
        1 => palette.success,
        2 => palette.warning,
        3 => palette.danger,
        _ => palette.muted,
    }
    .as_rgba_array()
}

/// 把括号配对着色 span 合并进语法高亮 span：括号字符的覆盖色优先，
/// 与括号重叠的语法 span 在括号边界处切分。两侧输入各自不重叠且有序；
/// 输出按起点有序、互不重叠（场景文本渲染按游标推进消费 span）。
pub(super) fn merge_bracket_glyph_spans(
    mut spans: Vec<ExtractedTextSpan>,
    brackets: &[(usize, usize, usize)],
    bracket_color: impl Fn(usize) -> [f32; 4],
) -> Vec<ExtractedTextSpan> {
    if brackets.is_empty() {
        return spans;
    }
    spans.sort_unstable_by_key(|span| (span.start, span.end));
    let mut merged: Vec<ExtractedTextSpan> = Vec::with_capacity(spans.len() + brackets.len());
    let mut bracket_index = 0usize;
    for span in spans.drain(..) {
        while bracket_index < brackets.len() && brackets[bracket_index].1 <= span.start {
            let &(start, end, depth) = &brackets[bracket_index];
            if start < end {
                merged.push(ExtractedTextSpan {
                    start,
                    end,
                    color: bracket_color(depth),
                });
            }
            bracket_index += 1;
        }
        let mut cursor = span.start;
        while bracket_index < brackets.len() && brackets[bracket_index].0 < span.end {
            let &(start, end, depth) = &brackets[bracket_index];
            if start > cursor {
                merged.push(ExtractedTextSpan {
                    start: cursor,
                    end: start,
                    color: span.color,
                });
            }
            merged.push(ExtractedTextSpan {
                start: start.max(cursor),
                end,
                color: bracket_color(depth),
            });
            cursor = end.max(cursor);
            bracket_index += 1;
        }
        if cursor < span.end {
            merged.push(ExtractedTextSpan {
                start: cursor,
                end: span.end,
                color: span.color,
            });
        }
    }
    while bracket_index < brackets.len() {
        let &(start, end, depth) = &brackets[bracket_index];
        if start < end {
            merged.push(ExtractedTextSpan {
                start,
                end,
                color: bracket_color(depth),
            });
        }
        bracket_index += 1;
    }
    merged
}

/// 把行内 inlay 的着色区间合并进显示 span 集：inlay 区间优先（与
/// inlay 重叠的基础层被切分丢弃），inlay 区间以给定颜色整段重发。两侧
/// 输入各自不重叠且有序；输出按起点有序、互不重叠（场景文本渲染按
/// 游标推进消费 span）。
pub(super) fn merge_inlay_glyph_spans(
    mut spans: Vec<ExtractedTextSpan>,
    inlays: &[(usize, usize)],
    color: [f32; 4],
) -> Vec<ExtractedTextSpan> {
    if inlays.is_empty() {
        return spans;
    }
    spans.sort_unstable_by_key(|span| (span.start, span.end));
    let mut merged: Vec<ExtractedTextSpan> = Vec::with_capacity(spans.len() + inlays.len());
    let mut inlay_index = 0usize;
    for span in spans.drain(..) {
        while inlay_index < inlays.len() && inlays[inlay_index].1 <= span.start {
            let &(start, end) = &inlays[inlay_index];
            if start < end {
                merged.push(ExtractedTextSpan { start, end, color });
            }
            inlay_index += 1;
        }
        let mut cursor = span.start;
        while inlay_index < inlays.len() && inlays[inlay_index].0 < span.end {
            let &(start, end) = &inlays[inlay_index];
            if start > cursor {
                merged.push(ExtractedTextSpan {
                    start: cursor,
                    end: start,
                    color: span.color,
                });
            }
            // inlay 区间以 inlay 色整段胜出（与前置/尾 flush 同款补发，
            // 重叠路径不吞段）。
            if start < end {
                merged.push(ExtractedTextSpan { start, end, color });
            }
            cursor = end.max(cursor);
            inlay_index += 1;
        }
        if cursor < span.end {
            merged.push(ExtractedTextSpan {
                start: cursor,
                end: span.end,
                color: span.color,
            });
        }
    }
    while inlay_index < inlays.len() {
        let &(start, end) = &inlays[inlay_index];
        if start < end {
            merged.push(ExtractedTextSpan { start, end, color });
        }
        inlay_index += 1;
    }
    merged
}

/// minimap 行长收集：每个逻辑行的非空白字符数（O(文档) 单趟扫描）。
/// 行数与滚动换算的 `matches('\n') + 1` 语义一致（尾随换行是可滚动到
/// 的空逻辑行）；空白行计 0（绘制层不产生行条）。
pub(super) fn collect_non_whitespace_line_lengths(value: &str) -> Vec<u32> {
    value
        .split('\n')
        .map(|line| {
            line.chars()
                .filter(|character| !character.is_whitespace())
                .count() as u32
        })
        .collect()
}

const DIAGNOSTIC_UNDERLINE: f32 = 2.0;
const DIAGNOSTIC_LABEL_GAP: f32 = 8.0;
const DIAGNOSTIC_LABEL_MAX_CHARS: usize = 80;

fn truncate_diagnostic_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let first = trimmed.lines().next().unwrap_or(trimmed);
    let mut chars = first.chars();
    let taken: String = chars.by_ref().take(DIAGNOSTIC_LABEL_MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{taken}…")
    } else {
        taken
    }
}

fn line_end_offset(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index)
}

fn derive_diagnostic_decorations(
    id: StableNodeId,
    source: &TextInputPresentationSource,
    style: &ComputedStyle,
    presentation_constraints: crate::TextShapeConstraints,
    line_height: f32,
    shaper: &mut (impl TextShaper + ?Sized),
) -> (
    Vec<TextDiagnosticMark>,
    Vec<TextDiagnosticLabel>,
    Vec<TextDiagnosticHit>,
) {
    let mut marks = Vec::new();
    let mut hits = Vec::new();
    struct LineWinner {
        rank: u8,
        severity: TextDiagnosticSeverity,
        message: String,
        offset: usize,
    }
    let mut winners: HashMap<usize, LineWinner> = HashMap::new();
    let font_size = style.font_size.max(1.0);
    for span in source.diagnostics.iter() {
        let start = clamp_boundary(&source.text.value, span.offset);
        let end = clamp_boundary(&source.text.value, span.offset + span.length.max(1));
        if end <= start {
            continue;
        }
        let rects = shaper.text_highlights(
            id,
            &source.text,
            (start, end),
            style,
            presentation_constraints,
        );
        if span.severity.draws_underline() {
            for rect in &rects {
                marks.push(TextDiagnosticMark {
                    rect: LayoutBox {
                        x: rect.x,
                        y: rect.y + rect.height - DIAGNOSTIC_UNDERLINE,
                        width: rect.width.max(1.0),
                        height: DIAGNOSTIC_UNDERLINE,
                    },
                    severity: span.severity,
                });
            }
        }
        let message = truncate_diagnostic_message(&span.message);
        if !message.is_empty() {
            for rect in &rects {
                hits.push(TextDiagnosticHit {
                    rect: *rect,
                    offset: start,
                    message: span.message.clone(),
                });
            }
            let line = source.text.value[..start]
                .bytes()
                .filter(|&byte| byte == b'\n')
                .count();
            let rank = span.severity.rank();
            let replace = winners.get(&line).is_none_or(|winner| rank > winner.rank);
            if replace {
                winners.insert(
                    line,
                    LineWinner {
                        rank,
                        severity: span.severity,
                        message,
                        offset: start,
                    },
                );
            }
        }
    }
    let mut labels = Vec::new();
    let mut lines: Vec<_> = winners.into_iter().collect();
    lines.sort_by_key(|(line, _)| *line);
    let engine = shaper.text_engine();
    let measure = crate::text_width::ChromeTextMeasure::new(engine.as_ref(), Some(style));
    for (_, winner) in lines {
        let end = line_end_offset(&source.text.value, winner.offset);
        let (x, y, height) =
            shaper.text_position(id, &source.text, end, style, presentation_constraints);
        let width = measure.width(&winner.message, font_size, None).max(1.0);
        let rect = LayoutBox {
            x: x + DIAGNOSTIC_LABEL_GAP,
            y,
            width,
            height: height.max(line_height),
        };
        hits.push(TextDiagnosticHit {
            rect,
            offset: winner.offset,
            message: winner.message.clone(),
        });
        labels.push(TextDiagnosticLabel {
            rect,
            text: winner.message,
            severity: winner.severity,
        });
    }
    (marks, labels, hits)
}

/// The constraints an editor's presentation geometry is built with.
///
/// Editing geometry must remain available outside a clipped viewport so the
/// Runtime can scroll the caret into view, so the viewport's height and any
/// clamping never reach it — except as the line budget of a vertical
/// multiline editor, which wraps down its height as a horizontal one wraps
/// across its width. Single-line fields keep their unwrapped
/// presentation even if their authored style omits nowrap.
///
/// Every caret, selection and hit probe of an editor has to be asked under
/// these, not under the node's layout constraints: a probe under different
/// constraints is asking about geometry the editor is not drawn from, and a
/// host that retains its geometry would lay the whole text out again for each
/// of the two.
pub(super) fn text_input_presentation_constraints(
    constraints: crate::TextShapeConstraints,
    multiline: bool,
    vertical: bool,
) -> crate::TextShapeConstraints {
    crate::TextShapeConstraints {
        max_width: if multiline {
            constraints.max_width
        } else {
            None
        },
        // The height a vertical multiline editor wraps down (#59). Anything
        // else it would only clip or truncate by.
        max_height: if multiline && vertical {
            constraints.max_height
        } else {
            None
        },
        wrap: multiline && constraints.wrap,
        ellipsis: false,
        max_lines: None,
        shaping: constraints.shaping,
        preserve_lines: constraints.preserve_lines,
        wrap_break: constraints.wrap_break,
    }
}

pub(super) fn shape_text_input_presentation(
    id: StableNodeId,
    source: TextInputPresentationSource,
    style: &ComputedStyle,
    constraints: crate::TextShapeConstraints,
    previous_overlays: &crate::components::TextOverlayMetrics,
    shaper: &mut impl TextShaper,
) -> TextInputPresentation {
    let presentation_constraints = text_input_presentation_constraints(
        constraints,
        source.multiline,
        style.writing_mode.is_vertical(),
    );
    shaper.with_text_probes(&source.text, style, presentation_constraints, |shaper| {
        shape_text_input_probes(
            id,
            &source,
            style,
            presentation_constraints,
            previous_overlays,
            shaper,
        )
    })
}

fn shape_text_input_probes(
    id: StableNodeId,
    source: &TextInputPresentationSource,
    style: &ComputedStyle,
    presentation_constraints: crate::TextShapeConstraints,
    previous_overlays: &crate::components::TextOverlayMetrics,
    shaper: &mut dyn TextShaper,
) -> TextInputPresentation {
    let (caret_x, caret_y, line_height) = shaper.text_caret_position(
        id,
        &source.text,
        source.caret,
        source.caret_affinity,
        style,
        presentation_constraints,
    );
    // 选区条带：主选区在前，附加光标选区紧随（多光标选区集互不重叠，
    // 条带天然不重叠，可安全合入同一批次）。
    let mut selection_lines = source.selection.map_or_else(Vec::new, |selection| {
        shaper.text_highlights(id, &source.text, selection, style, presentation_constraints)
    });
    for &(start, end, _) in &source.additional {
        selection_lines.extend(shaper.text_highlights(
            id,
            &source.text,
            (start, end),
            style,
            presentation_constraints,
        ));
    }
    let preedit_lines = source.preedit.map_or_else(Vec::new, |preedit| {
        shaper.text_highlights(id, &source.text, preedit, style, presentation_constraints)
    });

    // 编辑器扩展：诊断下划线 / 行尾文案 / 悬停命中 / 行号 y 表（仅多行态）。
    // 边界钳制复用 [`crate::text_editing::clamp_boundary`]。
    let (diagnostic_marks, diagnostic_labels, diagnostic_hits) = if source.multiline {
        derive_diagnostic_decorations(
            id,
            source,
            style,
            presentation_constraints,
            line_height,
            shaper,
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    // 查找匹配高亮：与选区一致的整行高条带（非诊断式下划线）。
    let match_marks = if source.multiline {
        let mut marks = Vec::new();
        for span in source.matches.iter() {
            let start = clamp_boundary(&source.text.value, span.offset);
            let end = clamp_boundary(&source.text.value, span.offset + span.length.max(1));
            if end <= start {
                continue;
            }
            for rect in shaper.text_highlights(
                id,
                &source.text,
                (start, end),
                style,
                presentation_constraints,
            ) {
                marks.push(TextMatchMark {
                    rect,
                    current: span.current,
                });
            }
        }
        marks
    } else {
        Vec::new()
    };
    // 颜色装饰 swatch：每个 span 取末显示行，在行内 span 末端画一个行高
    // 65% 的覆盖方块（垂直居中）。纯装饰：不改变布局测量，也无命中框；
    // 覆盖式绘制（半透明合成到字形之上）避免引入任何水平布局位移。
    let swatch_marks = if source.multiline {
        let mut marks = Vec::new();
        for span in source.color_swatches.iter() {
            let start = clamp_boundary(&source.text.value, span.offset);
            let end = clamp_boundary(&source.text.value, span.offset + span.length.max(1));
            if end <= start {
                continue;
            }
            if let Some(rect) = shaper
                .text_highlights(
                    id,
                    &source.text,
                    (start, end),
                    style,
                    presentation_constraints,
                )
                .last()
            {
                // span 末行可能因软换行只剩很小的尾段：方块尺寸仍按整行高
                // 缩放，右缘钳在 span 末行条带右缘（不越过行尾）；尾段比
                // 方块窄时向左扩展，不压到 span 之后的文本。
                let extent = (line_height * 0.65).clamp(6.0, 18.0);
                marks.push(TextSwatchMark {
                    rect: LayoutBox {
                        x: rect.x + rect.width - extent,
                        y: rect.y + (rect.height - extent).max(0.0) * 0.5,
                        width: extent,
                        height: extent,
                    },
                    color: span.color,
                });
            }
        }
        marks
    } else {
        Vec::new()
    };
    let atom_chips = if source.multiline {
        source
            .atoms
            .iter()
            .filter_map(|atom| {
                let start = clamp_boundary(&source.text.value, atom.start);
                let end = clamp_boundary(&source.text.value, atom.end);
                if end <= start {
                    return None;
                }
                let rect = shaper
                    .text_highlights(
                        id,
                        &source.text,
                        (start, end),
                        style,
                        presentation_constraints,
                    )
                    .into_iter()
                    .next()?;
                Some(layout_atom_chip(rect, atom))
            })
            .collect()
    } else {
        Vec::new()
    };
    // 括号匹配：光标相邻括号与其配对端各一个字符框（描边绘制）。
    let bracket_marks = if source.multiline {
        let value = source.text.value.as_str();
        crate::text_editing::matching_bracket_pair(value, source.caret)
            .map(|(open, close)| {
                [open, close]
                    .into_iter()
                    .map(|offset| {
                        let (x, y, height) = shaper.text_position(
                            id,
                            &source.text,
                            offset,
                            style,
                            presentation_constraints,
                        );
                        let (end_x, _, _) = shaper.text_position(
                            id,
                            &source.text,
                            offset + 1,
                            style,
                            presentation_constraints,
                        );
                        LayoutBox {
                            x,
                            y,
                            width: (end_x - x).max(1.0),
                            height,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    // 出现高亮：聚焦且主光标处有词（[A-Za-z0-9_]，全词匹配）或主选区为
    // 非空单行选区时，扫描全文档找出其余出现（大小写敏感；选中文本按
    // 子串匹配）。主光标所在出现不画（选区/当前行条已覆盖）；附加光标
    // 存在时仍按主光标派生。上限 [`crate::text_editing::
    // OCCURRENCE_HIGHLIGHT_LIMIT`] 处防病态文档；未聚焦、无词、多行选区
    // 或 IME 组合期零分配跳过。
    let occurrence_marks = if source.multiline
        && source.focused
        && source.editor.occurrence_highlight
        && source.preedit.is_none()
    {
        let value = source.text.value.as_str();
        match crate::text_editing::occurrence_query_at(value, source.selection, source.caret) {
            Some((query, whole_word)) => {
                let (ranges, _) = crate::text_editing::find_matches_capped(
                    value,
                    &query,
                    crate::text_editing::TextSearchOptions {
                        case_sensitive: true,
                        whole_word,
                    },
                    crate::text_editing::OCCURRENCE_HIGHLIGHT_LIMIT,
                );
                let selection_range = source.selection.map(|(start, end)| start..end);
                ranges
                    .into_iter()
                    .filter(|found| {
                        Some(found) != selection_range.as_ref()
                            && !(found.start <= source.caret && source.caret <= found.end)
                    })
                    .flat_map(|found| {
                        shaper.text_highlights(
                            id,
                            &source.text,
                            (found.start, found.end),
                            style,
                            presentation_constraints,
                        )
                    })
                    .collect()
            }
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    // 空白字符显示：空格与 Tab 各给一个文本空间字符单元标记（绘制层画
    // 中点/箭头）。每个连续空白 run 做两次端点探测，run 内按线性插值定
    // 位：等宽字体（代码编辑场景）下精确，比例字体下为近似。端点探测
    // 走 shaper 整段布局缓存，同一布局输入只在首次探针时布局一次。
    // 行首缩进与行尾空白一并可见；未开启选项零分配跳过。
    let whitespace_marks = if source.multiline && source.editor.show_whitespace {
        let value = source.text.value.as_str();
        let mut marks = Vec::new();
        let mut offset = 0usize;
        while offset < value.len() {
            if !matches!(value.as_bytes()[offset], b' ' | b'\t') {
                offset += 1;
                continue;
            }
            let run_start = offset;
            while offset < value.len() && matches!(value.as_bytes()[offset], b' ' | b'\t') {
                offset += 1;
            }
            let (start_x, y, height) =
                shaper.text_position(id, &source.text, run_start, style, presentation_constraints);
            let (end_x, _, _) =
                shaper.text_position(id, &source.text, offset, style, presentation_constraints);
            let run = &value[run_start..offset];
            let cell = ((end_x - start_x) / run.len() as f32).max(1.0);
            for (index, &byte) in run.as_bytes().iter().enumerate() {
                marks.push(TextWhitespaceMark {
                    rect: LayoutBox {
                        x: start_x + cell * index as f32,
                        y,
                        width: cell,
                        height,
                    },
                    kind: if byte == b'\t' {
                        TextWhitespaceKind::Tab
                    } else {
                        TextWhitespaceKind::Space
                    },
                });
            }
        }
        marks
    } else {
        Vec::new()
    };
    // wrap guide 列参考线：列宽按 '0' 字形宽度估算（等宽字体假设）；文
    // 档最宽行不足该列时不画。'0' 与整段宽度度量都落在 shaper 的整段布
    // 展缓存上：同一布局输入每趟至多一次真实布局，其后探针为缓存命中
    // （'0' 是单字符键，文档键与光标/高亮探针共享）。
    let wrap_guides = if source.multiline && !source.editor.wrap_guides.is_empty() {
        let unit = TextContent {
            value: "0".to_owned().into(),
        };
        let char_width = shaper.horizontal_offset(id, &unit, 1, style).max(1.0);
        let text_width = shaper
            .shape(id, &source.text, style, presentation_constraints)
            .width;
        source
            .editor
            .wrap_guides
            .iter()
            .filter_map(|&column| {
                let x = column as f32 * char_width;
                (column > 0 && x < text_width).then_some(x)
            })
            .collect()
    } else {
        Vec::new()
    };
    // 缩进参考线：每个逻辑行的前导空白内按缩进单位宽度画竖线。
    let indent_guides = if source.multiline {
        source
            .indent_guides
            .as_deref()
            .map(|unit| {
                let unit_content = TextContent {
                    value: unit.to_owned().into(),
                };
                let unit_width = shaper
                    .horizontal_offset(id, &unit_content, unit.len(), style)
                    .max(1.0);
                let value = source.text.value.as_str();
                let mut guides = Vec::new();
                let mut cursor = 0usize;
                loop {
                    let line_end = value[cursor..]
                        .find('\n')
                        .map_or(value.len(), |index| cursor + index);
                    let content_start =
                        crate::text_editing::line_content_start(value, cursor).min(line_end);
                    if content_start > cursor {
                        let (content_x, line_y, height) = shaper.text_position(
                            id,
                            &source.text,
                            content_start,
                            style,
                            presentation_constraints,
                        );
                        let levels = ((content_x / unit_width) + f32::EPSILON).floor().max(0.0);
                        for level in 0..levels as usize {
                            guides.push(LayoutBox {
                                x: level as f32 * unit_width,
                                y: line_y,
                                width: 1.0,
                                height,
                            });
                        }
                    }
                    if line_end >= value.len() {
                        break;
                    }
                    cursor = line_end + 1;
                }
                guides
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    // 行顶表：行号栏、git gutter 标记或 sticky scroll 需要时计算（一次/
    // 行的 text_position 探针；三者都没有时零成本短路）。git 标记与
    // sticky 钉住派生复用同一张表按显示行索引定位，软换行自然取逻辑行行首。
    let (line_tops, line_numbers) = if source.multiline
        && (source.line_numbers || !source.git_marks.is_empty() || source.editor.sticky_scroll)
    {
        let value = source.text.value.as_str();
        let mut starts: Vec<usize> = vec![0];
        for (index, byte) in value.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(index + 1);
            }
        }
        if value.ends_with('\n') {
            starts.pop();
        }
        let tops: Vec<f32> = starts
            .iter()
            .map(|&start| {
                shaper
                    .text_position(id, &source.text, start, style, presentation_constraints)
                    .1
            })
            .collect();
        // 折叠隐藏行后，显示行索引不再等于原始逻辑行号：把每个折叠片段
        // 之前的隐藏行数累计回行号（无折叠时返回空表，几何层按索引 + 1）。
        // inlay 片段不隐藏行（文本禁 '\n'），不参与计数。
        let mut numbers = match &source.fold {
            Some(view) => {
                let span_lines: Vec<(usize, u32)> = view
                    .spans
                    .iter()
                    .filter_map(|span| match &span.kind {
                        TextDisplaySpanKind::Fold { hidden_lines, .. } => Some((
                            view.value[..span.display_start].matches('\n').count(),
                            *hidden_lines,
                        )),
                        TextDisplaySpanKind::Inlay => None,
                    })
                    .collect();
                starts
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        let mut number = index as u32;
                        for &(span_line, hidden) in &span_lines {
                            if span_line < index {
                                number += hidden;
                            }
                        }
                        number + 1
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        // 相对行号（Zed 惯例，见 zed-industries/zed#62311：光标行显示绝
        // 对行号，其余行显示与光标所在行的距离；"光标行显示 1" 是被报告
        // 的 bug 而非预期）。多光标按主光标；距离按显示行计（所见即所得，
        // 折叠摘要行也参与计数）。
        if source.editor.relative_line_numbers {
            let caret_line = value[..clamp_boundary(value, source.caret)]
                .matches('\n')
                .count();
            numbers = (0..tops.len())
                .map(|index| {
                    if index == caret_line {
                        numbers.get(index).copied().unwrap_or(index as u32 + 1)
                    } else {
                        (index.abs_diff(caret_line)).min(u32::MAX as usize) as u32
                    }
                })
                .collect();
        }
        (tops, numbers)
    } else {
        (Vec::new(), Vec::new())
    };
    // 折叠摘要标记：折叠起始行行尾 ` …N` 的文本框（文本空间），供几何层
    // 生成点击命中区域。inlay 片段没有交互语义，不产生标记。
    let fold_marks = match &source.fold {
        Some(view) if source.multiline => view
            .spans
            .iter()
            .filter_map(|span| {
                let fold = match &span.kind {
                    TextDisplaySpanKind::Fold { fold, .. } => *fold,
                    TextDisplaySpanKind::Inlay => return None,
                };
                let (x, y, height) = shaper.text_position(
                    id,
                    &source.text,
                    span.display_start,
                    style,
                    presentation_constraints,
                );
                let (end_x, _, _) = shaper.text_position(
                    id,
                    &source.text,
                    span.display_start + span.display_len,
                    style,
                    presentation_constraints,
                );
                Some(crate::components::TextFoldMark {
                    rect: LayoutBox {
                        x,
                        y,
                        width: (end_x - x).max(1.0),
                        height,
                    },
                    fold,
                })
            })
            .collect(),
        _ => Vec::new(),
    };

    // git gutter 标记条带：显示行行顶的 2px 竖条素材（文本空间；x 与
    // 颜色由几何层按 gutter 与语义令牌解析）。越界行索引（源阶段已过滤，
    // 双重保险）静默跳过；单行态或空列表零分配短路。
    let git_marks = if source.multiline && !source.git_marks.is_empty() {
        source
            .git_marks
            .iter()
            .filter_map(|&(line, kind)| {
                let top = *line_tops.get(line as usize)?;
                Some(TextGitGutterMark {
                    y: top,
                    height: line_height,
                    kind,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    // 锚定浮层度量：补全行宽按 items 指针相等短路（列表未变零测量、
    // 零分配）；hover 锚点跟随文档偏移，每次 shape 一探（缓存字形度量）。
    let overlay_metrics = TextOverlayMetrics {
        completion: completion_popup_metrics(id, source, previous_overlays, style, shaper),
        hover_anchor: source.hover.as_ref().map(|doc| {
            let (x, y, _) = shaper.text_position(
                id,
                &source.text,
                doc.offset,
                style,
                presentation_constraints,
            );
            (x, y)
        }),
    };

    let mut content_size = shaper.shape(id, &source.text, style, presentation_constraints);
    if source.multiline {
        // Custom shapers may expose intrinsic single-line metrics while their
        // position probes describe the full editor. Include its final visual row.
        let (_, last_y, last_height) = shaper.text_position(
            id,
            &source.text,
            source.text.value.len(),
            style,
            presentation_constraints,
        );
        // Text space is line space: in a vertical editor (#59) `y` runs
        // across the columns, which is the page's width.
        if style.writing_mode.is_vertical() {
            content_size.width = content_size.width.max(last_y + last_height);
        } else {
            content_size.height = content_size.height.max(last_y + last_height);
        }
    }
    TextInputPresentation {
        content_size,
        display_value: source.text.value.clone(),
        placeholder: source.placeholder,
        // Single-line fields draw these x ranges; multiline editors draw the
        // line rects above and only ask whether a range exists. Probing with
        // the presentation constraints reads the layout this batch already
        // holds instead of laying the text out again unwrapped.
        selection: source.selection.map(|(start, end)| {
            (
                shaper
                    .text_position(id, &source.text, start, style, presentation_constraints)
                    .0,
                shaper
                    .text_position(id, &source.text, end, style, presentation_constraints)
                    .0,
            )
        }),
        selection_lines: if source.multiline {
            selection_lines
        } else {
            Vec::new()
        },
        caret_x,
        caret_y: if source.multiline { caret_y } else { 0.0 },
        line_height,
        preedit: source.preedit.map(|(start, end)| {
            (
                shaper
                    .text_position(id, &source.text, start, style, presentation_constraints)
                    .0,
                shaper
                    .text_position(id, &source.text, end, style, presentation_constraints)
                    .0,
            )
        }),
        preedit_lines: if source.multiline {
            preedit_lines
        } else {
            Vec::new()
        },
        // 附加光标：收起态才画 caret（range 选区由条带表达）。
        additional_carets: if source.multiline {
            source
                .additional
                .iter()
                .filter(|(start, end, _)| start == end)
                .map(|&(offset, _, affinity)| {
                    let (x, y, _) = shaper.text_caret_position(
                        id,
                        &source.text,
                        offset,
                        affinity,
                        style,
                        presentation_constraints,
                    );
                    (x, y)
                })
                .collect()
        } else {
            Vec::new()
        },
        diagnostic_marks,
        diagnostic_labels,
        diagnostic_hits,
        match_marks,
        swatch_marks,
        atom_chips,
        bracket_marks,
        bracket_color_spans: source.bracket_color_spans.clone(),
        occurrence_marks,
        whitespace_marks,
        wrap_guides,
        indent_guides,
        line_tops,
        line_numbers,
        fold_marks,
        git_marks,
        overlay_metrics,
        minimap_line_lengths: source.minimap_line_lengths.clone(),
    }
}

const ATOM_CHIP_PAD_X: f32 = 4.0;
const ATOM_CHIP_PAD_Y: f32 = 2.0;
const ATOM_CHIP_GAP: f32 = 5.0;
const ATOM_CHIP_ICON: f32 = 13.0;
const ATOM_CHIP_CLOSE: f32 = 16.0;
const ATOM_CHIP_LABEL: f32 = 12.0;

fn layout_atom_chip(rect: LayoutBox, atom: &crate::TextAtomSpan) -> crate::TextAtomChip {
    let min_width = ATOM_CHIP_PAD_X * 2.0 + ATOM_CHIP_ICON + ATOM_CHIP_GAP + ATOM_CHIP_CLOSE;
    let height = rect.height.max(ATOM_CHIP_CLOSE + ATOM_CHIP_PAD_Y * 2.0);
    let bounds = LayoutBox {
        x: rect.x,
        y: rect.y + (rect.height - height).max(0.0) * 0.5,
        width: rect.width.max(min_width),
        height,
    };
    let icon_bounds = LayoutBox {
        x: bounds.x + ATOM_CHIP_PAD_X,
        y: bounds.y + (bounds.height - ATOM_CHIP_ICON) * 0.5,
        width: ATOM_CHIP_ICON,
        height: ATOM_CHIP_ICON,
    };
    let close = LayoutBox {
        x: bounds.x + bounds.width - ATOM_CHIP_PAD_X - ATOM_CHIP_CLOSE,
        y: bounds.y + (bounds.height - ATOM_CHIP_CLOSE) * 0.5,
        width: ATOM_CHIP_CLOSE,
        height: ATOM_CHIP_CLOSE,
    };
    let label_x = icon_bounds.x + ATOM_CHIP_ICON + ATOM_CHIP_GAP;
    let label_width = (close.x - ATOM_CHIP_GAP - label_x).max(0.0);
    crate::TextAtomChip {
        bounds,
        close,
        icon: atom.icon,
        icon_bounds,
        label: crate::ComponentTextRegion {
            bounds: LayoutBox {
                x: label_x,
                y: bounds.y,
                width: label_width,
                height: bounds.height,
            },
            content: Arc::clone(&atom.label).into(),
            color: None,
            font_size: ATOM_CHIP_LABEL,
            font_weight: Some(650),
        },
        token: Arc::clone(&atom.token),
        start: atom.start,
        end: atom.end,
        background: [0.0; 4],
        border: [0.0; 4],
    }
}

/// 锚定浮层的共享输入：锚点行在节点空间的位置（x 为锚点字形左缘）。
#[derive(Debug, Clone, Copy)]
pub(super) struct OverlayAnchor {
    pub(super) x: f32,
    pub(super) line_top: f32,
    pub(super) line_height: f32,
}

/// minimap 竖条几何：内容区右缘 64px 面板、1px 分隔线、按非空白行长
/// 定宽的 2px 行条（文档超出条高容纳量时按整数步长抽稀）与跟随滚动的
/// 半透明视口指示器。绘制与指针导航共用同一投影（行换算见
/// [`TextMinimapGeometry::line_at`]）。
pub(crate) fn text_minimap_geometry(
    lengths: &[u32],
    content: LayoutBox,
    scroll_y: f32,
    line_height: f32,
    palette: &SemanticPalette,
) -> crate::TextMinimapGeometry {
    let line_count = lengths.len();
    let panel = LayoutBox {
        x: (content.x + content.width - crate::components::TEXT_MINIMAP_STRIP_WIDTH).max(content.x),
        y: content.y,
        width: content
            .width
            .min(crate::components::TEXT_MINIMAP_STRIP_WIDTH),
        height: content.height,
    };
    let separator = LayoutBox {
        x: panel.x - 1.0,
        y: panel.y,
        width: 1.0,
        height: panel.height,
    };
    let capacity =
        ((panel.height / crate::components::TEXT_MINIMAP_BAR_PITCH).floor() as usize).max(1);
    let stride = if line_count > capacity {
        line_count.div_ceil(capacity)
    } else {
        1
    };
    let max_length = lengths.iter().copied().max().unwrap_or(0).max(1) as f32;
    let bars = lengths
        .iter()
        .enumerate()
        .filter_map(|(index, &length)| {
            // 空白行不产生条；抽稀步长取整，槽位按 index / stride 落点。
            if length == 0 || (stride > 1 && index % stride != 0) {
                return None;
            }
            Some(LayoutBox {
                x: panel.x,
                y: panel.y + (index / stride) as f32 * crate::components::TEXT_MINIMAP_BAR_PITCH,
                width: (length as f32 / max_length * panel.width).max(1.0),
                height: crate::components::TEXT_MINIMAP_BAR_PITCH,
            })
        })
        .collect();
    // 视口指示器：视口行范围按同一投影换算（连续映射，随滚动平滑移动）；
    // 底缘钳到文档末行，文档在视口内放得下时不画。
    let line_height = line_height.max(1.0);
    let total_height = line_count as f32 * line_height;
    let indicator = if total_height > content.height + f32::EPSILON {
        let pitch = crate::components::TEXT_MINIMAP_BAR_PITCH / stride.max(1) as f32;
        let first_line = (scroll_y / line_height).max(0.0);
        let visible_lines = (content.height / line_height).ceil().max(1.0);
        let y = panel.y + first_line * pitch;
        let bottom = panel.y
            + (first_line + visible_lines)
                .min(line_count as f32)
                .max(first_line + 1.0)
                * pitch;
        let height = (bottom - y).clamp(2.0, panel.height);
        let y = y.clamp(panel.y, (panel.y + panel.height - height).max(panel.y));
        Some(LayoutBox {
            x: panel.x,
            y,
            width: panel.width,
            height,
        })
    } else {
        None
    };
    let accent = palette.accent.as_rgba_array();
    crate::TextMinimapGeometry {
        panel,
        separator,
        bars,
        indicator,
        panel_color: palette.subtle.as_rgba_array(),
        bar_color: palette.faint.as_rgba_array(),
        indicator_color: [accent[0], accent[1], accent[2], accent[3] * 0.2],
        stride,
        line_count,
    }
}

/// 锚定浮层的共享定位（补全弹层与 hover 浮窗共用，避免两套定位代码）：
/// 优先放在锚点行下方，视口底部放不下且上方放得下时翻转到行上方，
/// 最后整体钳进视口。返回面板矩形（节点空间）。
pub(super) fn anchored_overlay_panel(
    anchor: OverlayAnchor,
    width: f32,
    height: f32,
    viewport: LayoutBox,
    gap: f32,
) -> LayoutBox {
    const VIEWPORT_PAD: f32 = 2.0;
    let viewport_bottom = viewport.y + viewport.height;
    let min_y = viewport.y + VIEWPORT_PAD;
    let max_y = (viewport_bottom - VIEWPORT_PAD - height).max(min_y);
    let mut y = anchor.line_top + anchor.line_height + gap;
    if y > max_y {
        let flipped = anchor.line_top - gap - height;
        if flipped >= min_y {
            y = flipped;
        }
    }
    let y = y.clamp(min_y, max_y);
    let min_x = viewport.x + VIEWPORT_PAD;
    let max_x = (viewport.x + viewport.width - VIEWPORT_PAD - width).max(min_x);
    let x = anchor.x.clamp(min_x, max_x);
    LayoutBox {
        x,
        y,
        width,
        height,
    }
}

/// 补全弹层几何：面板 + 可见行（label 主文本、detail 次要说明、kind 右
/// 对齐标注、doc 文档行）。宽度自适应最长行（label > detail > kind 依次
/// 让位，上限 [`crate::components::TEXT_COMPLETION_MAX_CONTENT_WIDTH`]；
/// doc 行不参与宽度自适应，按内容宽截断），高度最多
/// [`crate::components::TEXT_COMPLETION_VISIBLE_ROWS`] 行（带文档行的
/// 候选占两行高）。
pub(super) fn completion_popup_geometry(
    state: &crate::store::TextCompletionViewState,
    metrics: &crate::components::TextCompletionPopupMetrics,
    anchor: OverlayAnchor,
    viewport: LayoutBox,
    font_size: f32,
    palette: &SemanticPalette,
) -> Option<crate::TextCompletionPopup> {
    const GAP: f32 = 12.0;
    const V_PAD: f32 = 4.0;
    const ROW_GAP_ABOVE_BELOW: f32 = 4.0;
    let items = &state.items;
    if items.is_empty() {
        return None;
    }
    let len = items.len();
    let first_row = state.scroll.min(len.saturating_sub(1));
    let visible =
        &items[first_row..(first_row + crate::components::TEXT_COMPLETION_VISIBLE_ROWS).min(len)];
    let row_height = anchor.line_height.max(1.0);
    let label_w = metrics.label_width;
    let mut content = label_w;
    let show_detail = metrics.detail_width > 0.0
        && content + GAP + metrics.detail_width
            <= crate::components::TEXT_COMPLETION_MAX_CONTENT_WIDTH;
    if show_detail {
        content += GAP + metrics.detail_width;
    }
    let show_kind = metrics.kind_width > 0.0
        && content + GAP + metrics.kind_width
            <= crate::components::TEXT_COMPLETION_MAX_CONTENT_WIDTH;
    if show_kind {
        content += GAP + metrics.kind_width;
    }
    let content = content.min(crate::components::TEXT_COMPLETION_MAX_CONTENT_WIDTH);
    let label_w = label_w.min(content);
    let visible_rows = visible.len();
    let doc_rows = visible.iter().filter(|item| !item.doc.is_empty()).count();
    let panel = anchored_overlay_panel(
        anchor,
        (content + crate::components::TEXT_COMPLETION_PANEL_PAD * 2.0).max(0.0),
        (visible_rows + doc_rows) as f32 * row_height + V_PAD * 2.0,
        viewport,
        ROW_GAP_ABOVE_BELOW,
    );
    let rows = visible
        .iter()
        .scan(panel.y + V_PAD, |y, item| {
            let label_line_y = *y;
            let has_doc = !item.doc.is_empty();
            *y += row_height * (1.0 + u8::from(has_doc) as f32);
            let label_rect_w = label_w.min(content);
            let detail_x =
                panel.x + crate::components::TEXT_COMPLETION_PANEL_PAD + label_rect_w + GAP;
            let detail_rect = show_detail
                .then_some(())
                .filter(|_| !item.detail.is_empty())
                .map(|_| crate::ComponentTextRegion {
                    bounds: LayoutBox {
                        x: detail_x,
                        y: label_line_y,
                        width: metrics.detail_width,
                        height: row_height,
                    },
                    content: crate::TextValue::from(item.detail.as_str()),
                    color: Some(palette.muted.as_rgba_array()),
                    font_size,
                    font_weight: None,
                });
            let kind_rect = show_kind
                .then_some(())
                .filter(|_| !item.kind_label.is_empty())
                .map(|_| crate::ComponentTextRegion {
                    bounds: LayoutBox {
                        x: panel.x + panel.width
                            - crate::components::TEXT_COMPLETION_PANEL_PAD
                            - metrics.kind_width,
                        y: label_line_y,
                        width: metrics.kind_width,
                        height: row_height,
                    },
                    content: crate::TextValue::from(item.kind_label.as_str()),
                    color: Some(palette.faint.as_rgba_array()),
                    font_size,
                    font_weight: None,
                });
            let doc_rect = has_doc.then_some(crate::ComponentTextRegion {
                bounds: LayoutBox {
                    x: panel.x + crate::components::TEXT_COMPLETION_PANEL_PAD,
                    y: label_line_y + row_height,
                    width: content,
                    height: row_height,
                },
                content: crate::TextValue::from(item.doc.as_str()),
                color: Some(palette.muted.as_rgba_array()),
                font_size,
                font_weight: None,
            });
            Some(crate::TextCompletionRow {
                bounds: LayoutBox {
                    x: panel.x,
                    y: label_line_y,
                    width: panel.width,
                    height: *y - label_line_y,
                },
                label: crate::ComponentTextRegion {
                    bounds: LayoutBox {
                        x: panel.x + crate::components::TEXT_COMPLETION_PANEL_PAD,
                        y: label_line_y,
                        width: label_rect_w,
                        height: row_height,
                    },
                    content: crate::TextValue::from(item.label.as_str()),
                    color: Some(palette.text.as_rgba_array()),
                    font_size,
                    font_weight: None,
                },
                detail: detail_rect,
                kind: kind_rect,
                doc: doc_rect,
            })
        })
        .collect();
    Some(crate::TextCompletionPopup {
        panel,
        selected: state.selected,
        first_row,
        rows,
        background: palette.surface.as_rgba_array(),
        border: palette.border_strong.as_rgba_array(),
        selected_background: palette.hover.as_rgba_array(),
        label_color: palette.text.as_rgba_array(),
        detail_color: palette.muted.as_rgba_array(),
        kind_color: palette.faint.as_rgba_array(),
    })
}

/// hover 浮窗几何：面板 + 标题行（强调）+ 正文逻辑行切片。宽度取
/// 视口与上限的较小值；正文超出 [`crate::components::TEXT_HOVER_MAX_BODY_ROWS`] 行
/// 时滚轮滚动（切片由框架命令写回的滚动位置决定）。
pub(super) fn hover_popup_geometry(
    state: &crate::store::TextHoverViewState,
    anchor: OverlayAnchor,
    viewport: LayoutBox,
    font_size: f32,
    palette: &SemanticPalette,
) -> Option<crate::TextHoverPopup> {
    const MAX_WIDTH: f32 = 420.0;
    const H_PAD: f32 = 10.0;
    const V_PAD: f32 = 6.0;
    const TITLE_BODY_GAP: f32 = 4.0;
    const VIEWPORT_GAP: f32 = 4.0;
    let line_height = anchor.line_height.max(1.0);
    let body_lines: Vec<&str> = state.doc.body.lines().collect();
    let scroll = state.scroll.min(body_lines.len().saturating_sub(1));
    let visible = &body_lines
        [scroll..(scroll + crate::components::TEXT_HOVER_MAX_BODY_ROWS).min(body_lines.len())];
    let width = MAX_WIDTH.min(viewport.width.max(1.0));
    let title_height = line_height;
    let panel = anchored_overlay_panel(
        anchor,
        width,
        V_PAD * 2.0 + title_height + TITLE_BODY_GAP + visible.len() as f32 * line_height,
        viewport,
        VIEWPORT_GAP,
    );
    let content_width = (width - H_PAD * 2.0).max(0.0);
    let title = crate::ComponentTextRegion {
        bounds: LayoutBox {
            x: panel.x + H_PAD,
            y: panel.y + V_PAD,
            width: content_width,
            height: title_height,
        },
        content: crate::TextValue::from(state.doc.title.as_str()),
        color: Some(palette.text.as_rgba_array()),
        font_size,
        font_weight: Some(600),
    };
    let body_rows = visible
        .iter()
        .enumerate()
        .map(|(index, line)| crate::ComponentTextRegion {
            bounds: LayoutBox {
                x: panel.x + H_PAD,
                y: panel.y + V_PAD + title_height + TITLE_BODY_GAP + index as f32 * line_height,
                width: content_width,
                height: line_height,
            },
            content: crate::TextValue::from(*line),
            color: Some(palette.muted.as_rgba_array()),
            font_size,
            font_weight: None,
        })
        .collect();
    Some(crate::TextHoverPopup {
        panel,
        title,
        body_rows,
        background: palette.surface.as_rgba_array(),
        border: palette.border_strong.as_rgba_array(),
        title_color: palette.text.as_rgba_array(),
        body_color: palette.muted.as_rgba_array(),
    })
}

/// 签名帮助浮窗：签名行拆成前缀 / 活动参数 / 后缀，文档行取活动参数说明。
pub(super) fn signature_popup_geometry(
    help: &crate::TextSignatureHelp,
    anchor: OverlayAnchor,
    viewport: LayoutBox,
    font_size: f32,
    palette: &SemanticPalette,
    measure: crate::text_width::ChromeTextMeasure<'_>,
) -> Option<crate::TextSignaturePopup> {
    const H_PAD: f32 = 10.0;
    const V_PAD: f32 = 6.0;
    const GAP: f32 = 4.0;
    const MAX_WIDTH: f32 = 420.0;
    const ACTIVE_WEIGHT: u16 = 600;
    let line_height = anchor.line_height.max(1.0);
    let measure = |value: &str, weight| measure.width(value, font_size, weight).max(1.0);
    let names: Vec<&str> = help.params.iter().map(|(name, _)| name.as_str()).collect();
    let active = help.active_index.min(names.len().saturating_sub(1));
    let prefix = if names.is_empty() {
        format!("{}(", help.title)
    } else {
        format!(
            "{}({}{}",
            help.title,
            names[..active].join(", "),
            if active > 0 { ", " } else { "" }
        )
    };
    let active_name = names.get(active).copied().unwrap_or("");
    let suffix = if names.is_empty() {
        ")".to_owned()
    } else {
        format!(
            "{})",
            names[active + 1..]
                .iter()
                .map(|name| format!(", {name}"))
                .collect::<String>()
        )
    };
    let doc = help
        .params
        .get(active)
        .map(|(_, doc)| doc.as_str())
        .filter(|doc| !doc.is_empty())
        .or_else(|| {
            let doc = help.fn_doc.trim();
            (!doc.is_empty()).then_some(doc)
        })
        .map(|doc| doc.lines().next().unwrap_or(doc).to_owned());
    let prefix_w = measure(&prefix, None);
    let active_w = if active_name.is_empty() {
        0.0
    } else {
        measure(active_name, Some(ACTIVE_WEIGHT))
    };
    let suffix_w = measure(&suffix, None);
    let content_w = (prefix_w + active_w + suffix_w).clamp(1.0, MAX_WIDTH - H_PAD * 2.0);
    let panel = anchored_overlay_panel(
        anchor,
        (content_w + H_PAD * 2.0)
            .min(MAX_WIDTH)
            .min(viewport.width.max(1.0)),
        V_PAD * 2.0
            + line_height
            + if doc.is_some() {
                GAP + line_height
            } else {
                0.0
            },
        viewport,
        4.0,
    );
    let content_width = (panel.width - H_PAD * 2.0).max(0.0);
    let y = panel.y + V_PAD;
    let mut x = panel.x + H_PAD;
    let region =
        |x: f32, y: f32, width: f32, content: &str, color: [f32; 4], weight: Option<u16>| {
            crate::ComponentTextRegion {
                bounds: LayoutBox {
                    x,
                    y,
                    width,
                    height: line_height,
                },
                content: crate::TextValue::from(content),
                color: Some(color),
                font_size,
                font_weight: weight,
            }
        };
    let prefix_width = prefix_w.min(content_width);
    let prefix_region = region(
        x,
        y,
        prefix_width,
        &prefix,
        palette.text.as_rgba_array(),
        None,
    );
    x += prefix_width;
    let remaining = (panel.x + H_PAD + content_width - x).max(0.0);
    let active_region = (!active_name.is_empty()).then(|| {
        region(
            x,
            y,
            active_w.min(remaining),
            active_name,
            palette.accent.as_rgba_array(),
            Some(ACTIVE_WEIGHT),
        )
    });
    if let Some(active) = &active_region {
        x += active.bounds.width;
    }
    let suffix_remaining = (panel.x + H_PAD + content_width - x).max(0.0);
    Some(crate::TextSignaturePopup {
        panel,
        prefix: prefix_region,
        active: active_region,
        suffix: region(
            x,
            y,
            suffix_w.min(suffix_remaining),
            &suffix,
            palette.text.as_rgba_array(),
            None,
        ),
        doc: doc.map(|doc| {
            region(
                panel.x + H_PAD,
                y + line_height + GAP,
                content_width,
                &doc,
                palette.muted.as_rgba_array(),
                None,
            )
        }),
        background: palette.surface.as_rgba_array(),
        border: palette.border_strong.as_rgba_array(),
        active_background: palette.hover.as_rgba_array(),
    })
}

/// 补全弹层行宽度量。`items` 指针与上一次度量一致时整段复用（打字重
/// 喂之外的每次 shape 不再逐行测量）；测量只发生在宿主喂入新列表之后。
pub(super) fn completion_popup_metrics(
    id: StableNodeId,
    source: &TextInputPresentationSource,
    previous: &crate::components::TextOverlayMetrics,
    style: &ComputedStyle,
    shaper: &mut (impl TextShaper + ?Sized),
) -> Option<crate::components::TextCompletionPopupMetrics> {
    let items = source.completions.as_ref()?;
    if let Some(previous) = previous
        .completion
        .as_ref()
        .filter(|previous| Arc::ptr_eq(&previous.items, items))
    {
        return Some(previous.clone());
    }
    let mut width_of = |value: &str| -> f32 {
        shaper.horizontal_offset(
            id,
            &TextContent {
                value: value.to_owned().into(),
            },
            value.len(),
            style,
        )
    };
    let metrics = crate::components::TextCompletionPopupMetrics {
        items: Arc::clone(items),
        label_width: items
            .iter()
            .map(|item| width_of(&item.label))
            .fold(0.0_f32, f32::max),
        detail_width: items
            .iter()
            .map(|item| width_of(&item.detail))
            .fold(0.0_f32, f32::max),
        kind_width: items
            .iter()
            .map(|item| width_of(&item.kind_label))
            .fold(0.0_f32, f32::max),
    };
    Some(metrics)
}

/// An editor's caret: a one-pixel rule standing `line_height` tall at `(x, y)`.
///
/// Horizontal and vertical editors both draw it from here, in text space, so
/// the two cannot drift apart.
pub(super) fn caret_rule(x: f32, y: f32, line_height: f32) -> LayoutBox {
    LayoutBox {
        x,
        y,
        width: 1.0,
        height: line_height,
    }
}

/// The underline under a composing run, in text space: `run` is the run's
/// line box, the rule spans its inline extent (at least a pixel) and sits at
/// its block end — below a horizontal line — or, for a vertical column, at its
/// block start, where CJK sets its sidelines.
pub(super) fn preedit_rule(run: LayoutBox, at_block_end: bool) -> LayoutBox {
    let rule = LayoutBox {
        width: run.width.max(1.0),
        height: 2.0,
        ..run
    };
    let y = if at_block_end {
        run.y + run.height - rule.height
    } else {
        run.y
    };
    LayoutBox { y, ..rule }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn text_input_decorations(
    presentation: &TextInputPresentation,
    multiline: bool,
    content: LayoutBox,
    line_y: f32,
    line_height: f32,
    scroll_x: f32,
    scroll_y: f32,
) -> (Vec<LayoutBox>, Vec<LayoutBox>) {
    let field_x = |offset: f32| content.x + offset - scroll_x;
    if multiline {
        let selection = presentation
            .selection_lines
            .iter()
            .map(|selection| LayoutBox {
                x: field_x(selection.x),
                y: content.y + selection.y - scroll_y,
                width: selection.width,
                height: selection.height,
            })
            .collect();
        let preedit = presentation
            .preedit_lines
            .iter()
            .map(|preedit| {
                preedit_rule(
                    LayoutBox {
                        x: field_x(preedit.x),
                        y: content.y + preedit.y - scroll_y,
                        ..*preedit
                    },
                    true,
                )
            })
            .collect();
        (selection, preedit)
    } else {
        let selection = presentation
            .selection
            .map(|(start, end)| LayoutBox {
                x: field_x(start),
                y: line_y,
                width: (end - start).max(0.0),
                height: line_height,
            })
            .into_iter()
            .collect();
        let preedit = presentation
            .preedit
            .map(|(start, end)| {
                preedit_rule(
                    LayoutBox {
                        x: field_x(start),
                        y: line_y,
                        width: end - start,
                        height: line_height,
                    },
                    true,
                )
            })
            .into_iter()
            .collect();
        (selection, preedit)
    }
}

impl UiWorld {
    /// Shape against the last published content box when it exists so wrap
    /// height can stop or propagate LAYOUT. Unmeasured nodes stay unconstrained.
    pub(crate) fn text_shape_constraints(&self, id: StableNodeId) -> crate::TextShapeConstraints {
        self.text_shape_constraints_for(id, self.text_input_kind(id))
    }

    /// Whether an authored newline is a line break for this node.
    ///
    /// A multiline editor's value keeps its line breaks whatever
    /// `white-space` says: they are the text being edited, not authored markup
    /// whitespace. Everywhere else CSS decides.
    ///
    /// One function because two consumers answer it: the constraints this node
    /// is *measured* with, and the scene the renderer *paints* from. A second
    /// spelling would measure one line and paint two.
    fn preserves_lines(style: &NodeStyle, text_input_multiline: bool) -> bool {
        style.layout.white_space.preserve_newlines() || text_input_multiline
    }

    /// [`Self::preserves_lines`] for one node, as the scene carries it.
    pub(crate) fn text_preserves_lines(&self, id: StableNodeId) -> bool {
        Self::preserves_lines(
            &self.record(id).style,
            self.text_input_kind(id).unwrap_or(false),
        )
    }

    /// Whether the node is an editor, and whether it is multiline: the only
    /// two things [`Self::text_shape_constraints_for`] needs to know about its
    /// presentation.
    ///
    /// Derived from the node rather than from a built presentation source: a
    /// caret move asks for the constraints it should probe with, and building
    /// a whole presentation (display text, decorations and all) to learn two
    /// booleans is O(document) per key press.
    pub(super) fn text_input_kind(&self, id: StableNodeId) -> Option<bool> {
        if !matches!(
            self.nodes.visual(id),
            Some(StandardVisual::TextInput { .. })
        ) || self.nodes.text_input(id).is_none()
        {
            return None;
        }
        Some(
            self.nodes
                .get(id)
                .is_some_and(|node| node.accessibility.multiline),
        )
    }

    /// Whether the editor draws bullets instead of its value.
    pub(crate) fn text_input_is_secure(&self, id: StableNodeId) -> bool {
        matches!(
            self.nodes.visual(id),
            Some(StandardVisual::TextInput { secure: true, .. })
        )
    }

    /// [`Self::text_shape_constraints`] for a caller that already knows
    /// whether the node is an editor and whether it is multiline (the built
    /// presentation source's `multiline`).
    pub(super) fn text_shape_constraints_for(
        &self,
        id: StableNodeId,
        editor_multiline: Option<bool>,
    ) -> crate::TextShapeConstraints {
        let source = &self.record(id).style;
        let layout = self.record(id).layout;
        let text_input_multiline = editor_multiline.unwrap_or(false);
        let is_text_input = editor_multiline.is_some();
        let wrap = if is_text_input {
            text_input_multiline && source.layout.text_wraps()
        } else {
            source.layout.text_wraps()
        };
        let preserve_lines = Self::preserves_lines(source, text_input_multiline);
        let wrap_break = source.layout.text_wrap_break();
        let ellipsis = !is_text_input && source.layout.uses_text_ellipsis();
        let max_lines = (!is_text_input)
            .then(|| source.layout.resolved_line_clamp())
            .flatten();
        let measured = layout.width > 0.0 || layout.height > 0.0;
        let vertical = self
            .nodes
            .get(id)
            .is_some_and(|node| node.resolved.0.writing_mode.is_vertical());
        if !measured {
            return crate::TextShapeConstraints {
                wrap,
                ellipsis,
                max_lines,
                shaping: self.text_shaping(id),
                preserve_lines,
                wrap_break,
                ..crate::TextShapeConstraints::default()
            };
        }
        let padding = self.used_layout_padding(id);
        let border = source.layout.resolved_border_edges();
        let leading_visual = text_leading_inset(self.nodes.visual(id));
        crate::TextShapeConstraints {
            max_width: if is_text_input && !text_input_multiline {
                None
            } else {
                Some(
                    (layout.width
                        - padding.left
                        - padding.right
                        - border.left
                        - border.right
                        - leading_visual)
                        .max(0.0),
                )
            },
            // A vertical paragraph's lines run down the box, so its height is
            // the line budget the way a horizontal one's width is: always
            // given once the box is measured. A vertical editor wraps down its
            // height when it wraps at all — a multiline one, as a horizontal
            // editor wraps across its width (#59).
            max_height: ((vertical && (!is_text_input || text_input_multiline))
                || (!is_text_input
                    && (source
                        .layout
                        .height
                        .is_some_and(nana_ui_core::LengthSpec::is_definite_declared)
                        || source
                            .layout
                            .max_height
                            .is_some_and(nana_ui_core::LengthSpec::is_definite_declared))))
            .then(|| {
                (layout.height - padding.top - padding.bottom - border.top - border.bottom).max(0.0)
            }),
            wrap,
            ellipsis,
            max_lines,
            shaping: self.text_shaping(id),
            preserve_lines,
            wrap_break,
        }
    }
}

/// A single line that ellipsizes against its measured box: what it paints is
/// cut to that box, but the width it asks of layout is its whole line. Were it
/// to report the cut width, a shrink-to-fit parent would keep the old box
/// forever — a label changed to something longer could only ever shrink.
fn cut_to_its_box(constraints: &crate::TextShapeConstraints) -> bool {
    constraints.ellipsis
        && !constraints.wrap
        && constraints.max_lines.is_none()
        && !constraints.preserve_lines
        && constraints.max_width.is_some()
}

/// Text that wraps to its measured box: its lines may be narrower than the
/// text is on its own.
fn wraps_to_its_box(constraints: &crate::TextShapeConstraints) -> bool {
    constraints.wrap && constraints.max_width.is_some()
}

/// One plain text node's resolution: [`UiWorld::resolve_plain_text`].
pub(super) struct PlainText {
    metrics: TextMetrics,
    /// Its unwrapped width, when it wrapped narrower than that.
    natural: Option<f32>,
    /// The `nana-text` layout it retains, with an engine.
    layout: Option<Arc<nana_text::TextLayout>>,
}

/// The same constraints without the box width: the line as long as it runs.
fn unbounded(constraints: crate::TextShapeConstraints) -> crate::TextShapeConstraints {
    crate::TextShapeConstraints {
        max_width: None,
        ..constraints
    }
}

/// True when two authored styles give a plain text node the same
/// constraints for the same box: everything [`UiWorld::text_shape_constraints`]
/// reads from [`NodeStyle`]. Alignment only matters to a retained layout and is
/// handled where the style is set. Paint, transform and
/// opacity live on the same `LayoutStyle` and are deliberately not compared;
/// fonts are classified when the computed style resolves.
pub(super) fn same_text_constraint_inputs(previous: &NodeStyle, next: &NodeStyle) -> bool {
    if Arc::ptr_eq(&previous.layout, &next.layout) {
        return true;
    }
    let (a, b) = (previous.layout.as_ref(), next.layout.as_ref());
    let definite = |length: Option<nana_ui_core::LengthSpec>| {
        length.is_some_and(nana_ui_core::LengthSpec::is_definite_declared)
    };
    a.text_wraps() == b.text_wraps()
        && a.white_space.preserve_newlines() == b.white_space.preserve_newlines()
        && a.text_wrap_break() == b.text_wrap_break()
        && a.uses_text_ellipsis() == b.uses_text_ellipsis()
        && a.resolved_line_clamp() == b.resolved_line_clamp()
        && definite(a.height) == definite(b.height)
        && definite(a.max_height) == definite(b.max_height)
        && a.resolved_border_edges() == b.resolved_border_edges()
        && a.padding == b.padding
        && a.padding_top == b.padding_top
        && a.padding_right == b.padding_right
        && a.padding_bottom == b.padding_bottom
        && a.padding_left == b.padding_left
        && a.font_size.map(f32::to_bits) == b.font_size.map(f32::to_bits)
}

/// Width a leading indicator takes from the text box of a checkbox or switch.
fn text_leading_inset(visual: Option<&StandardVisual>) -> f32 {
    match visual {
        Some(StandardVisual::Checkbox { size, .. }) => size.indicator_size() + size.indicator_gap(),
        Some(StandardVisual::Switch { .. }) => 38.0,
        _ => 0.0,
    }
}

/// What plain text resolution reads from a node's visual: which text path the
/// node takes, and the inset a leading indicator takes from its box. Two
/// visuals with the same key cannot change a plain text node's constraints.
pub(crate) fn text_visual_key(visual: Option<&StandardVisual>) -> (u8, u32) {
    let path = match visual {
        Some(StandardVisual::TextInput { .. }) => 1,
        Some(StandardVisual::EmptyState { .. }) => 2,
        Some(StandardVisual::ModalFrame { .. }) => 3,
        // Every other visual leaves plain text on the plain path.
        None | Some(_) => 0,
    };
    (path, text_leading_inset(visual).to_bits())
}

impl UiWorld {
    pub(super) fn text_shaping(&self, id: StableNodeId) -> crate::TextShaping {
        if self.nodes.text_input(id).is_some() {
            crate::TextShaping::Advanced
        } else {
            crate::TextShaping::Auto
        }
    }
}

impl UiWorld {
    /// minimap 行长的单条缓存：值未变（纯光标/选区同步）时复用上一次
    /// O(文档) 单趟扫描结果，避免每趟 shape 全文档重扫。
    pub(super) fn minimap_line_lengths_cached(&self, value: &crate::TextValue) -> Vec<u32> {
        let mut cache = self.minimap_line_lengths_cache.borrow_mut();
        // The same text is recognised by its stamp: a caret move compares
        // nothing, and the cache holds a shared copy, not a second one.
        if let Some((cached_value, cached_lengths)) = cache.as_ref()
            && cached_value == value
        {
            return cached_lengths.clone();
        }
        let lengths = collect_non_whitespace_line_lengths(value);
        *cache = Some((value.clone(), lengths.clone()));
        lengths
    }
}

impl UiWorld {
    /// 括号配对着色的单条缓存。着色只是括号字符序列的函数，所以有两级复用：
    /// 值未变（纯光标/选区同步）直接复用上一次的表；**改动区间里前后都没有
    /// 括号字符**时（打字、删字的绝大多数情况）也不重扫——括号序列没变，
    /// 只需把改动点之后的偏移整体平移。剩下的情况（真的敲了括号）才走
    /// O(文档) 单趟栈扫描。
    ///
    /// 平移是 O(括号数)；重扫是 O(文档)，在 310 KB 文档上是 ~0.37 ms，
    /// 以前每次编辑都要付一次。
    pub(super) fn bracket_color_spans_cached(
        &self,
        value: &crate::TextValue,
    ) -> Arc<[(usize, usize, usize)]> {
        let mut cache = self.bracket_color_spans_cache.borrow_mut();
        if let Some((cached_value, cached_spans)) = cache.as_mut() {
            if cached_value.same_identity(value) {
                return Arc::clone(cached_spans);
            }
            match crate::text_editing::changed_byte_range(cached_value, value) {
                None => return Arc::clone(cached_spans),
                Some((start, previous_end, next_end))
                    if !crate::text_editing::contains_bracket(
                        &cached_value[start..previous_end],
                    ) && !crate::text_editing::contains_bracket(&value[start..next_end]) =>
                {
                    let spans = shifted_bracket_spans(cached_spans, previous_end, next_end);
                    // The cache now describes this text: the next edit diffs
                    // against what the spans describe.
                    *cached_value = value.clone();
                    *cached_spans = Arc::clone(&spans);
                    return spans;
                }
                Some(_) => {}
            }
        }
        #[cfg(any(test, feature = "benchmark"))]
        crate::text_shape_stats::note_bracket_rescan();
        let (pairs, unmatched) = crate::text_editing::bracket_pair_colorization(value);
        let mut spans = Vec::with_capacity(pairs.len() + unmatched.len());
        spans.extend(pairs);
        spans.extend(unmatched.into_iter().map(|offset| {
            (
                offset,
                offset + 1,
                crate::components::TEXT_BRACKET_UNMATCHED_DEPTH,
            )
        }));
        spans.sort_unstable_by_key(|&(start, _, _)| start);
        let spans: Arc<[(usize, usize, usize)]> = spans.into();
        *cache = Some((value.clone(), Arc::clone(&spans)));
        spans
    }
}

/// `spans` with every offset at or after `previous_end` moved to where the
/// edit put it. Spans before the edit are untouched, and no span can lie
/// inside it: the caller only takes this path when the changed bytes hold no
/// bracket.
fn shifted_bracket_spans(
    spans: &[(usize, usize, usize)],
    previous_end: usize,
    next_end: usize,
) -> Arc<[(usize, usize, usize)]> {
    if previous_end == next_end {
        return Arc::from(spans);
    }
    let delta = next_end as isize - previous_end as isize;
    let shift = |offset: usize| {
        if offset >= previous_end {
            (offset as isize + delta) as usize
        } else {
            offset
        }
    };
    spans
        .iter()
        .map(|&(start, end, depth)| (shift(start), shift(end), depth))
        .collect()
}

impl UiWorld {
    pub(super) fn text_input_presentation_source(
        &self,
        id: StableNodeId,
    ) -> Option<TextInputPresentationSource> {
        let StandardVisual::TextInput {
            placeholder,
            secure,
            ..
        } = self.nodes.visual(id)?
        else {
            return None;
        };
        let state = self.nodes.text_input(id)?;
        let ime = self.nodes.ime(id);
        let multiline = self
            .nodes
            .get(id)
            .is_some_and(|node| node.accessibility.multiline);
        let extras = match self.nodes.visual(id) {
            Some(StandardVisual::TextInput {
                diagnostics,
                matches,
                color_swatches,
                atoms,
                line_numbers,
                indent_guides,
                git_marks,
                editor_options,
                ..
            }) => TextInputEditorExtras {
                diagnostics: Arc::clone(diagnostics),
                matches: Arc::clone(matches),
                color_swatches: Arc::clone(color_swatches),
                atoms: Arc::clone(atoms),
                line_numbers: *line_numbers,
                indent_guides: indent_guides.clone(),
                git_marks: Arc::clone(git_marks),
                editor: editor_options.clone(),
            },
            _ => TextInputEditorExtras::default(),
        };
        // 焦点随 source 下发：出现高亮只在聚焦编辑器上派生，未聚焦的
        // 多行编辑器零分配跳过整条扫描路径。
        let focused = self.input.focused.get(&self.record(id).document) == Some(&id);
        // 折叠/inlay 显示视图：仅多行态且有折叠态区间或行内提示时构建
        // （两者都为空集合时零成本短路，不分配显示串）。IME 组合期
        // inlay 退场由 text_display_view 内部处理。
        let fold = if multiline {
            self.text_display_view(id)
        } else {
            None
        };
        // 锚定浮层输入：补全候选与 hover 文档相互独立（仅多行编辑器；
        // 单行字段没有浮层）。hover 文档按值克隆进 presentation source：
        // source 是所有权结构，被 shape 约束、几何派生与测试多处消费，
        // 引用化会把生命周期串进所有构造点；克隆仅在宿主喂入 hover 期间
        // 发生，成本与浮窗文档自身同阶。
        let (completions, hover) = if multiline {
            (
                self.nodes
                    .text_completion_view(id)
                    .map(|state| state.items.clone()),
                self.nodes.text_hover_view(id).map(|h| h.doc.clone()),
            )
        } else {
            (None, None)
        };
        let mut source = build_text_input_presentation_source(
            state,
            ime,
            placeholder,
            *secure,
            multiline,
            extras,
            focused,
            fold,
            completions,
            hover,
            self.composed_display(id),
        );
        // minimap 行长：编辑器选项归默认（占位符/IME 组合态）或多行关闭
        // 时零扫描短路；开启时按原始值收集并走值等值缓存。
        if source.multiline && source.editor.minimap {
            source.minimap_line_lengths = self.minimap_line_lengths_cached(&state.value_shared());
        }
        // 括号配对着色：占位符与 IME 组合态没有真实可着色文档（组合期
        // 偏移漂移），保持空表；其余多行态按显示值收集并走值等值缓存。
        if source.multiline
            && source.editor.bracket_pair_colors
            && !source.placeholder
            && source.preedit.is_none()
        {
            source.bracket_color_spans = self.bracket_color_spans_cached(&source.text.value);
        }
        Some(source)
    }
}

impl UiWorld {
    /// Publishes the shaped title/message block and reports whether anything
    /// changed, so a pass that only re-confirms the current block stays idle.
    pub(super) fn apply_empty_state_text_presentation(
        &mut self,
        id: StableNodeId,
        presentation: EmptyStateTextPresentation,
    ) -> bool {
        let mut changed = false;
        if self.nodes.empty_state_text(id) != Some(&presentation) {
            self.nodes.set_empty_state_text(id, Some(presentation));
            changed = true;
        }
        let Some(StandardVisual::EmptyState {
            icon,
            compact,
            action,
            ..
        }) = self.nodes.visual(id)
        else {
            return changed;
        };
        let spacing = if *compact { 2.0 } else { 6.0 };
        let vertical = if *compact { 8.0 } else { 24.0 };
        let mut height = presentation.title.height;
        if icon.is_some() {
            height += 22.0 + spacing;
        }
        if let Some(message) = presentation.message {
            height += spacing + message.height;
        }
        if action.is_some() {
            height += spacing + 4.0;
        }
        let padding_top = nana_ui_core::LengthSpec::Px(vertical + height);
        let mut style = self.record(id).style.clone();
        if style.layout.padding_top != Some(padding_top) {
            Arc::make_mut(&mut style.layout).padding_top = Some(padding_top);
            self.write_node_style(id, style);
            self.mark(id, DirtyMask::LAYOUT | DirtyMask::RENDER);
            if let Some(parent) = self.parent_id(id) {
                self.mark_ancestors(parent, DirtyMask::LAYOUT | DirtyMask::RENDER);
            }
            changed = true;
        }
        changed
    }
}

impl UiWorld {
    pub(super) fn shape_text_for_layout_impl(
        &mut self,
        ids: Vec<StableNodeId>,
        host: &mut impl TextShaper,
    ) -> Result<bool, UiWorldError> {
        let backend = PlainTextBackend::of(host);
        self.paint_text_engine = backend.engine.clone();
        self.observe_text_backend(backend.epoch);
        let mut work = nana_text::TextWorkCounters::default();
        // Same production adapter as [`Self::shape_text`].
        let mut cache = std::mem::take(&mut self.text_layout_cache);
        let mut glyphs = std::mem::take(&mut self.glyph_cache);
        let mut shaper = CountingShaper::new(host, &mut cache, &mut glyphs);
        let mut shaped = Vec::new();
        let mut empty_shaped = Vec::new();
        let mut modal_shaped = Vec::new();
        let mut resolved = Vec::new();
        #[cfg(any(test, feature = "benchmark"))]
        crate::text_shape_stats::note_scope(ids.len());
        // Run the loop as one fallible unit so an invalid metric still hands
        // the caches back to the world instead of dropping them.
        let outcome = (|| -> Result<(), UiWorldError> {
            for id in ids {
                // The whole per-node decision for text that is already resolved:
                // a few integers on the record, before any text is read.
                if self.plain_text_is_current(id, backend.epoch, &mut work) {
                    continue;
                }
                if matches!(
                    self.nodes.visual(id),
                    Some(StandardVisual::EmptyState { .. } | StandardVisual::ModalFrame { .. })
                ) {
                    // Its text is the visual's own now; a layout it held as
                    // plain text is not drawn any more.
                    self.nodes.release_text_layout(id);
                }
                let computed = self.record(id).resolved.0.as_ref();
                let visual = self.nodes.visual(id);
                if let Some(visual @ StandardVisual::EmptyState { compact, .. }) = visual {
                    if computed.visible {
                        let layout = self.record(id).layout;
                        let horizontal = if *compact { 6.0 } else { 16.0 };
                        let width = (layout.width - horizontal * 2.0).max(0.0);
                        let runs = shaper.runs;
                        let intrinsic =
                            shape_empty_state_text(id, visual, computed, Some(width), &mut shaper);
                        work.record_text_pass(1, usize::from(shaper.runs > runs));
                        validate_text_metrics(id, intrinsic.title)?;
                        if let Some(message) = intrinsic.message {
                            validate_text_metrics(id, message)?;
                        }
                        // The shaped block is republished even when the metrics
                        // are unchanged: it lives in `NodeStyle`, which
                        // `EmptyState::project` rewrites from its own static
                        // style, so an unrelated re-projection can drop it.
                        empty_shaped.push((id, intrinsic));
                    }
                    continue;
                }
                if let Some(visual @ StandardVisual::ModalFrame { kind, slots, .. }) = visual {
                    if computed.visible {
                        let root = self.record(id).layout;
                        let surface =
                            crate::overlay_surfaces::modal_surface_bounds(root, *kind, None);
                        let chrome = crate::overlay_surfaces::ModalChrome::measure(
                            *kind,
                            crate::TextMetrics::default(),
                            None,
                            slots.close_action.is_some(),
                            slots.footer.is_some() || !slots.actions.is_empty(),
                        );
                        let wrap_width =
                            chrome.text_width(surface.width, *kind, slots.close_action.is_some());
                        let runs = shaper.runs;
                        let intrinsic =
                            shape_modal_text(id, visual, computed, Some(wrap_width), &mut shaper);
                        work.record_text_pass(1, usize::from(shaper.runs > runs));
                        validate_text_metrics(id, intrinsic.title)?;
                        if let Some(description) = intrinsic.description {
                            validate_text_metrics(id, description)?;
                        }
                        if let Some(body) = intrinsic.body {
                            validate_text_metrics(id, body)?;
                        }
                        if self.nodes.modal_text(id) != Some(&intrinsic) {
                            modal_shaped.push((id, intrinsic));
                        }
                    }
                    continue;
                }
                if !computed.visible {
                    continue;
                }
                let presentation = matches!(visual, Some(StandardVisual::TextInput { .. }))
                    .then(|| self.text_input_presentation_source(id))
                    .flatten();
                let empty = presentation.as_ref().map_or_else(
                    || self.record(id).text.value.is_empty(),
                    |source| source.text.value.is_empty(),
                );
                if presentation.is_some() {
                    // An editor's text is not plain text: nothing may keep
                    // drawing a layout of what it used to say.
                    self.nodes.release_text_layout(id);
                }
                if empty {
                    // A box with no text of its own resolves to nothing, and
                    // stays resolved until text or a text-bearing visual
                    // arrives: a scope of containers costs one read of the text
                    // table each, not a visual lookup and a style read every
                    // pass. An empty Text node is not such a box: its line box
                    // is measured by the scheduled pass, which this pass must
                    // not pre-empt with a stamp that carries no metrics.
                    if presentation.is_none()
                        && !matches!(self.record(id).kind.as_ref(), NodeKind::Text)
                    {
                        resolved.push(PlainResolution::nothing(id));
                    }
                    continue;
                }
                #[cfg(any(test, feature = "benchmark"))]
                crate::text_shape_stats::note_nonempty();
                let constraints =
                    self.text_shape_constraints_for(id, presentation.as_ref().map(|s| s.multiline));
                let Some(presentation) = presentation else {
                    let PlainText {
                        metrics,
                        natural,
                        layout,
                    } = self.resolve_plain_text(
                        id,
                        constraints,
                        backend.engine.as_ref(),
                        &mut shaper,
                        &mut work,
                    );
                    validate_text_metrics(id, metrics)?;
                    resolved.push(PlainResolution::text(id, constraints, layout));
                    if self.record(id).text_metrics != metrics
                        || self.text_natural_width(id) != natural
                    {
                        shaped.push((id, metrics, natural, None));
                    }
                    continue;
                };
                let style = Arc::clone(&self.record(id).resolved.0);
                let computed = style.as_ref();
                let text = clone_shaped_text(self, id, Some(&presentation));
                work.text_source_clones += 1;
                let runs = shaper.runs;
                let metrics = shaper.shape(id, &text, computed, constraints);
                work.record_text_pass(1, usize::from(shaper.runs > runs));
                validate_text_metrics(id, metrics)?;
                let previous_overlays = self
                    .nodes
                    .text_input_presentation(id)
                    .map(|stored| stored.overlay_metrics.clone())
                    .unwrap_or_default();
                let presentation = shape_text_input_presentation(
                    id,
                    presentation,
                    computed,
                    constraints,
                    &previous_overlays,
                    &mut shaper,
                );
                if self.record(id).text_metrics != metrics
                    || self.nodes.text_input_presentation(id) != Some(&presentation)
                {
                    shaped.push((id, metrics, None, Some(presentation)));
                }
            }
            Ok(())
        })();
        let runs = shaper.runs;
        let wrap_layouts = shaper.wrap_layouts;
        let host_keys = shaper.keys;
        work.text_source_clones += shaper.keys;
        work.text_bytes_hashed += shaper.key_bytes;
        if outcome.is_err() {
            // Nothing of a failed pass is reported, or left to be reported by
            // the next one.
            let _ = cache.take_counters();
            let _ = glyphs.take_counters();
            let _ = host.take_text_work();
            self.text_layout_cache = cache;
            self.glyph_cache = glyphs;
            return outcome.map(|()| false);
        }
        self.apply_plain_resolutions(resolved, backend.epoch, &mut work);
        let mut changed = !shaped.is_empty() || !modal_shaped.is_empty();
        for (id, metrics, natural, presentation) in shaped {
            self.record_mut(id).text_metrics = metrics;
            self.nodes.set_text_natural_width(id, natural);
            if let Some(presentation) = presentation {
                self.nodes
                    .set_text_input_presentation(id, Some(presentation));
            }
        }
        for (id, presentation) in empty_shaped {
            changed |= self.apply_empty_state_text_presentation(id, presentation);
        }
        for (id, presentation) in modal_shaped {
            self.nodes.set_modal_text(id, Some(presentation));
            self.mark(id, DirtyMask::LAYOUT | DirtyMask::RENDER);
        }
        let (hits, misses, evictions) = cache.take_counters();
        if host_keys > 0 {
            // The host path's layout cache is the Runtime `TextLayoutCache`.
            work.record_layout_cache(hits, misses);
            work.layout_cache_lookups += hits + misses;
        }
        let glyph_stats = glyphs.take_counters();
        self.text_layout_cache = cache;
        self.glyph_cache = glyphs;
        self.refresh_document_text_highlights(host);
        // Taken last: document highlights measure through the host too.
        work.accumulate(host.take_text_work());
        self.finish_text_pass(
            work,
            runs,
            hits,
            misses,
            evictions,
            wrap_layouts,
            glyph_stats,
        );
        Ok(changed)
    }

    /// True, and counted as a revision skip, when `id` is plain text already
    /// resolved by `backend` at its current revisions. Reads one small entry of
    /// the text side table and no text.
    fn plain_text_is_current(
        &self,
        id: StableNodeId,
        backend: TextBackendEpoch,
        work: &mut nana_text::TextWorkCounters,
    ) -> bool {
        let Some(text) = self.nodes.text_node(id) else {
            return false;
        };
        if !text.is_current(backend) {
            return false;
        }
        let stamp = text.stamp().expect("a current node is stamped");
        // A resolved box without text is not a text node; only text nodes count.
        if stamp.text_node {
            work.text_nodes_considered += 1;
            work.text_nodes_revision_skipped += 1;
            #[cfg(any(test, feature = "benchmark"))]
            {
                crate::text_shape_stats::note_nonempty();
                crate::text_shape_stats::note_skipped_unchanged();
            }
        }
        #[cfg(debug_assertions)]
        if let Some(constraints) = stamp.constraints {
            // A revision that should have moved and did not would leave this
            // node on stale constraints. Catch the missing invalidation where
            // it is cheap to explain rather than as a wrong wrap on screen.
            let (recorded, alignment) = constraints;
            if !text.layout.is_null() {
                debug_assert_eq!(
                    alignment,
                    self.record(id).style.text_horizontal_alignment,
                    "text node {id:?} retains a layout at a stale alignment"
                );
            }
            debug_assert_eq!(
                recorded,
                self.text_shape_constraints(id),
                "text node {id:?} is current by revision but its constraints changed: \
                 a CONSTRAINT invalidation is missing"
            );
        }
        true
    }

    /// Resolves one plain (non-editor) text node at its current revisions; the
    /// caller stamps it once the metrics validate. With an engine a node with
    /// text retains the `nana-text` layout its metrics were read from; without
    /// one the host shaper measures it. Text that wrapped to its box also
    /// reports its unwrapped width when that is wider (see
    /// [`UiWorld::text_natural_width`]).
    fn resolve_plain_text<S: TextShaper>(
        &mut self,
        id: StableNodeId,
        constraints: crate::TextShapeConstraints,
        engine: Option<&nana_text::SharedTextEngine>,
        shaper: &mut CountingShaper<'_, S>,
        work: &mut nana_text::TextWorkCounters,
    ) -> PlainText {
        let style = Arc::clone(&self.record(id).resolved.0);
        let text_bytes = self.record(id).text.value.len();
        // A box without text that a scheduled pass still measures is not a
        // text node: its measurement is counted, but not as a node.
        let is_text_node =
            text_bytes > 0 || matches!(self.record(id).kind.as_ref(), NodeKind::Text);
        let mut node_work = nana_text::TextWorkCounters::default();
        let resolved = match engine {
            Some(engine) => {
                let alignment = self.record(id).style.text_horizontal_alignment;
                let (source, copied) = self.nodes.text_source(id).expect("a resolved node exists");
                // Locked per resolution, never across the pass: the same pass
                // measures component text through the host's `shape`, which
                // may lay out through this very engine.
                let kind = crate::text_node::text_kind(&constraints);
                let layout = nana_text::lock_text_engine(engine).layout(
                    kind,
                    source,
                    &crate::text_node::nana_text_style(&style),
                    &crate::text_node::nana_text_constraints(&style, &constraints, alignment),
                    &mut node_work,
                );
                let mut metrics = crate::text_node::text_metrics_of_layout(&layout);
                if cut_to_its_box(&constraints)
                    && !layout.is_vertical()
                    && layout
                        .overflow
                        .contains(nana_text::OverflowFlags::ELLIPSIZED)
                {
                    let natural = nana_text::lock_text_engine(engine).layout(
                        kind,
                        source,
                        &crate::text_node::nana_text_style(&style),
                        &crate::text_node::nana_text_constraints(
                            &style,
                            &unbounded(constraints),
                            alignment,
                        ),
                        &mut node_work,
                    );
                    metrics.width = crate::text_node::text_metrics_of_layout(&natural).width;
                }
                // Wrapped lines are as wide as the box they wrapped to; the
                // text's own width is its lines unwrapped. Layout needs that
                // to widen a box that shrinks to its content once the box's
                // limit grows, rather than keep the width it wrapped to.
                let natural = (wraps_to_its_box(&constraints)
                    && !layout.is_vertical()
                    && layout
                        .lines
                        .iter()
                        .any(|line| line.break_cause == nana_text::LineBreakCause::Wrap))
                .then(|| {
                    let natural = nana_text::lock_text_engine(engine).layout(
                        kind,
                        source,
                        &crate::text_node::nana_text_style(&style),
                        &crate::text_node::nana_text_constraints(
                            &style,
                            &unbounded(constraints),
                            alignment,
                        ),
                        &mut node_work,
                    );
                    crate::text_node::text_metrics_of_layout(&natural).width
                })
                .filter(|natural| *natural > metrics.width);
                if copied {
                    node_work.text_source_clones += 1;
                    self.record_string_clone(text_bytes);
                }
                // An empty Text node still has a line box to measure, but
                // nothing to draw: it retains no layout.
                PlainText {
                    metrics,
                    natural,
                    layout: (text_bytes > 0).then_some(layout),
                }
            }
            None => {
                let runs = shaper.runs;
                let mut metrics = shaper.shape(id, &self.record(id).text, &style, constraints);
                if cut_to_its_box(&constraints)
                    && constraints
                        .max_width
                        .is_some_and(|max| metrics.width >= max - 0.5)
                {
                    let natural =
                        shaper.shape(id, &self.record(id).text, &style, unbounded(constraints));
                    metrics.width = metrics.width.max(natural.width);
                }
                // The host does not say where it broke its lines, only how
                // tall they came out. One line that fit is the whole text;
                // more than one may have wrapped, so ask for them unwrapped
                // (cached by its constraints, like the first).
                let lines_tall = crate::components::resolved_text_line_height(&style) * 1.5;
                let natural = (wraps_to_its_box(&constraints) && metrics.height > lines_tall)
                    .then(|| {
                        shaper
                            .shape(id, &self.record(id).text, &style, unbounded(constraints))
                            .width
                    })
                    .filter(|natural| *natural > metrics.width);
                node_work.record_text_pass(1, usize::from(shaper.runs > runs));
                // No layout: one from an engine this host no longer offers is
                // not what the host measures now.
                PlainText {
                    metrics,
                    natural,
                    layout: None,
                }
            }
        };
        if !is_text_node {
            node_work.text_nodes_considered = 0;
            node_work.text_nodes_shaped = 0;
        }
        work.accumulate(node_work);
        resolved
    }

    /// Applies a successful pass's plain text resolutions: retains or releases
    /// each node's layout, then stamps it at the revisions it was resolved at.
    /// Nothing is applied for a pass that failed, so a retry resolves the same
    /// nodes again rather than skipping them with metrics never written.
    fn apply_plain_resolutions(
        &mut self,
        resolutions: Vec<PlainResolution>,
        backend: TextBackendEpoch,
        work: &mut nana_text::TextWorkCounters,
    ) {
        for resolution in resolutions {
            match resolution.layout {
                Some(layout) => {
                    if self.nodes.retain_text_layout(resolution.id, layout) {
                        work.text_layouts_reused += 1;
                    }
                }
                None => self.nodes.release_text_layout(resolution.id),
            }
            self.nodes
                .mark_text_resolved(resolution.id, backend, resolution.constraints);
        }
    }

    /// Folds one pass's counters into the frame.
    #[allow(clippy::too_many_arguments)]
    fn finish_text_pass(
        &mut self,
        work: nana_text::TextWorkCounters,
        runs: usize,
        hits: usize,
        misses: usize,
        evictions: usize,
        wrap_layouts: usize,
        glyph_stats: Option<(usize, usize)>,
    ) {
        // `WorkCounters` keeps its host-shaper meaning: `TextShaper::shape`
        // runs and `TextLayoutCache` hits and misses. Engine work is reported
        // on the text work counters, where its caches are named.
        self.bump_last_counters(|counters| {
            counters.record_text_shape(runs, hits, misses, wrap_layouts);
            counters.record_cache_eviction(evictions);
            if let Some((glyph_hits, glyph_misses)) = glyph_stats {
                counters.record_glyph_cache(glyph_hits, glyph_misses);
            }
        });
        record_text_diagnostics(&work);
        self.record_text_work(work);
    }

    /// Notes the backend plain text resolves against. A different font set
    /// than the last pass saw makes every resolved text node stale; nodes this
    /// pass does not visit are scheduled so none keeps metrics from the old
    /// fonts.
    fn observe_text_backend(&mut self, epoch: TextBackendEpoch) {
        let previous = self.text_backend.replace(epoch);
        if previous != Some(epoch) {
            self.text_backend_changed = true;
        }
        if previous.is_none_or(|previous| previous == epoch) {
            return;
        }
        // Glyph advances and cached metrics are keyed by character, text and
        // style, not by who measured them: another shaper at the same font
        // generation must not be answered with the last one's numbers.
        self.glyph_cache.clear();
        self.text_layout_cache.clear();
        let stale = self.nodes.text_bearing_nodes().collect::<Vec<_>>();
        for id in stale {
            self.nodes
                .invalidate_text(id, crate::text_node::TextDirty::FONT);
            self.mark(id, DirtyMask::TEXT | DirtyMask::LAYOUT | DirtyMask::RENDER);
        }
        // A painter that measured text re-records against the new backend;
        // one that did not is untouched.
        let painted = self
            .paint_recordings
            .get_mut()
            .iter()
            .filter(|(_, held)| held.measured_text.is_some())
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in painted {
            self.mark_repaint(id);
        }
    }
}

impl UiWorld {
    /// Text-bearing nodes style resolution turned visible since the last
    /// scheduled pass and not already in `ids`, for this pass to re-resolve.
    fn take_shown_text(&mut self, ids: &[StableNodeId]) -> Vec<StableNodeId> {
        if self.text_shown.is_empty() {
            return Vec::new();
        }
        let scheduled: HashSet<StableNodeId> = ids.iter().copied().collect();
        let mut shown = std::mem::take(&mut self.text_shown);
        shown.sort_unstable();
        shown.dedup();
        shown.retain(|id| {
            // Editor, EmptyState and ModalFrame text is re-measured whenever a
            // layout scope reaches it and is never stamped; showing it again
            // is not a reason to measure it here as well.
            !scheduled.contains(id)
                && !matches!(
                    self.nodes.visual(*id),
                    Some(
                        StandardVisual::TextInput { .. }
                            | StandardVisual::EmptyState { .. }
                            | StandardVisual::ModalFrame { .. }
                    )
                )
                && self.nodes.get(*id).is_some_and(|record| {
                    matches!(record.kind.as_ref(), NodeKind::Text) || !record.text.value.is_empty()
                })
        });
        shown
    }

    /// Schedules every text the host's current font set may measure
    /// differently than the one this world last resolved against. A frame
    /// driver calls this before draining work, so registering a font makes
    /// a static document settle on the new fonts instead of waiting for
    /// unrelated work to reach a text pass.
    pub fn observe_text_shaper(&mut self, host: &(impl TextShaper + ?Sized)) {
        let backend = PlainTextBackend::of(host);
        self.paint_text_engine = backend.engine.clone();
        self.observe_text_backend(backend.epoch);
    }

    /// Text revisions of `id` (Issue #95): which classes of change its text
    /// has seen. Paint and compositor changes never move `content`, `shape`
    /// or `constraint`.
    pub fn text_revisions(&self, id: StableNodeId) -> Option<crate::TextRevisions> {
        self.nodes.text_node(id).map(|text| text.revisions)
    }

    /// The `nana-text` layout plain text node `id` retains, with its handle.
    /// `None` when the node's text resolved through the host shaper, is empty,
    /// or is an editor's.
    pub fn text_layout(
        &self,
        id: StableNodeId,
    ) -> Option<(nana_text::TextLayoutId, &Arc<nana_text::TextLayout>)> {
        let handle = self.nodes.text_node(id)?.layout;
        self.nodes
            .text_layouts()
            .get(handle)
            .map(|layout| (handle, layout))
    }

    /// Layouts retained across every plain text node of this world.
    pub fn retained_text_layouts(&self) -> usize {
        self.nodes.text_layouts().len()
    }
}

/// One plain text node resolved by a pass, held until the pass succeeds.
pub(super) struct PlainResolution {
    id: StableNodeId,
    /// The constraints a node with text was resolved at. `None` for a box with
    /// no text, whose constraints nothing reads.
    constraints: Option<crate::TextShapeConstraints>,
    /// The layout to retain; `None` releases whatever the node held.
    layout: Option<Arc<nana_text::TextLayout>>,
}

impl PlainResolution {
    fn text(
        id: StableNodeId,
        constraints: crate::TextShapeConstraints,
        layout: Option<Arc<nana_text::TextLayout>>,
    ) -> Self {
        Self {
            id,
            constraints: Some(constraints),
            layout,
        }
    }

    fn nothing(id: StableNodeId) -> Self {
        Self {
            id,
            constraints: None,
            layout: None,
        }
    }
}

/// How plain text resolves for one pass, read once from the host.
pub(super) struct PlainTextBackend {
    pub epoch: TextBackendEpoch,
    pub engine: Option<nana_text::SharedTextEngine>,
}

impl PlainTextBackend {
    pub(super) fn of<H: TextShaper + ?Sized>(host: &H) -> Self {
        match host.text_engine() {
            Some(engine) => {
                let epoch = nana_text::lock_text_engine(&engine).epoch();
                Self {
                    epoch: TextBackendEpoch::Engine(epoch),
                    engine: Some(engine),
                }
            }
            None => Self {
                epoch: TextBackendEpoch::Host {
                    shaper: {
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::hash::DefaultHasher::new();
                        std::any::type_name::<H>().hash(&mut hasher);
                        hasher.finish()
                    },
                    font_generation: host.font_generation(),
                },
                engine: None,
            },
        }
    }
}

impl UiWorld {
    /// Nodes currently carrying a LAYOUT-dirty bit that has not been drained
    /// by [`Self::take_system_work`] — e.g. marked by a shaping pass between
    /// drains. Sorted for determinism.
    pub fn pending_layout_dirty(&self) -> Vec<StableNodeId> {
        let mut ids = self
            .dirty_entities
            .iter()
            .copied()
            .filter(|id| {
                self.nodes
                    .get(*id)
                    .is_some_and(|node| node.dirty.has(DirtyMask::LAYOUT))
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }
}

impl UiWorld {
    /// [`Self::shape_text_for_layout`] restricted to `ids` (typically the
    /// relayout scope plus nodes whose published box changed). Nodes outside
    /// the scope keep their previous shape, which already matches their
    /// unchanged constraints.
    pub fn shape_text_for_layout_scoped(
        &mut self,
        ids: &[StableNodeId],
        shaper: &mut impl TextShaper,
    ) -> Result<bool, UiWorldError> {
        let mut scope = ids.to_vec();
        scope.sort_unstable();
        scope.dedup();
        scope.retain(|id| self.nodes.contains(*id));
        self.shape_text_for_layout_impl(scope, shaper)
    }
}

impl UiWorld {
    /// Re-shape visible text against its resolved content box after the first
    /// layout pass. This closes wrapping/ellipsis height measurement without
    /// moving layout ownership into the renderer adapter.
    pub fn shape_text_for_layout(
        &mut self,
        document: DocumentId,
        shaper: &mut impl TextShaper,
    ) -> Result<bool, UiWorldError> {
        self.shape_text_for_layout_impl(self.document_order(document), shaper)
    }
}

impl UiWorld {
    /// Shape only explicitly scheduled text. The runtime owns invalidation and
    /// storage while the renderer adapter supplies its real shaping backend.
    pub fn shape_text(
        &mut self,
        ids: &[StableNodeId],
        host: &mut impl TextShaper,
    ) -> Result<(), UiWorldError> {
        self.resolve_presentations(ids)?;
        let shown = self.take_shown_text(ids);
        let backend = PlainTextBackend::of(host);
        self.paint_text_engine = backend.engine.clone();
        self.observe_text_backend(backend.epoch);
        let mut work = nana_text::TextWorkCounters::default();
        // Production adapter: every host shaper (MeasureTextShaper, NanaTextShaper,
        // tests) is wrapped once so lookup/insert hit the same UiWorld caches.
        let mut cache = std::mem::take(&mut self.text_layout_cache);
        let mut glyphs = std::mem::take(&mut self.glyph_cache);
        let mut shaper = CountingShaper::new(host, &mut cache, &mut glyphs);
        if !ids.is_empty() {
            self.record_hot_path_allocation(
                1,
                ids.len().saturating_mul(size_of::<(
                    StableNodeId,
                    TextMetrics,
                    Option<TextInputPresentation>,
                )>()),
            );
        }
        let mut shaped = Vec::with_capacity(ids.len());
        let mut empty_shaped = Vec::new();
        let mut modal_shaped = Vec::new();
        let mut resolved = Vec::new();
        #[cfg(any(test, feature = "benchmark"))]
        crate::text_shape_stats::note_scope(ids.len());
        let outcome = (|| -> Result<(), UiWorldError> {
            for &id in ids.iter().chain(&shown) {
                if !self.contains(id) {
                    return Err(UiWorldError::MissingNode(id));
                }
                if self.plain_text_is_current(id, backend.epoch, &mut work) {
                    continue;
                }
                if matches!(
                    self.nodes.visual(id),
                    Some(StandardVisual::EmptyState { .. } | StandardVisual::ModalFrame { .. })
                ) {
                    // Its text is the visual's own now; a layout it held as
                    // plain text is not drawn any more.
                    self.nodes.release_text_layout(id);
                }
                let visual = self.nodes.visual(id);
                let presentation = matches!(visual, Some(StandardVisual::TextInput { .. }))
                    .then(|| self.text_input_presentation_source(id))
                    .flatten();
                let style = Arc::clone(&self.record(id).resolved.0);
                #[cfg(any(test, feature = "benchmark"))]
                if presentation.as_ref().map_or_else(
                    || !self.record(id).text.value.is_empty(),
                    |source| !source.text.value.is_empty(),
                ) {
                    crate::text_shape_stats::note_nonempty();
                }
                // EmptyState and ModalFrame own intrinsic text of their own and
                // re-measure it every pass, so they are never stamped.
                let mut stampable = true;
                if let Some(visual @ StandardVisual::EmptyState { .. }) = visual {
                    stampable = false;
                    let runs = shaper.runs;
                    let intrinsic = shape_empty_state_text(id, visual, &style, None, &mut shaper);
                    work.record_text_pass(1, usize::from(shaper.runs > runs));
                    validate_text_metrics(id, intrinsic.title)?;
                    if let Some(message) = intrinsic.message {
                        validate_text_metrics(id, message)?;
                    }
                    empty_shaped.push((id, intrinsic));
                }
                if let Some(visual @ StandardVisual::ModalFrame { .. }) = visual {
                    stampable = false;
                    let runs = shaper.runs;
                    let intrinsic = shape_modal_text(id, visual, &style, None, &mut shaper);
                    work.record_text_pass(1, usize::from(shaper.runs > runs));
                    validate_text_metrics(id, intrinsic.title)?;
                    if let Some(description) = intrinsic.description {
                        validate_text_metrics(id, description)?;
                    }
                    if let Some(body) = intrinsic.body {
                        validate_text_metrics(id, body)?;
                    }
                    modal_shaped.push((id, intrinsic));
                }
                let constraints =
                    self.text_shape_constraints_for(id, presentation.as_ref().map(|s| s.multiline));
                let Some(presentation) = presentation else {
                    let PlainText {
                        metrics,
                        natural,
                        layout,
                    } = self.resolve_plain_text(
                        id,
                        constraints,
                        backend.engine.as_ref(),
                        &mut shaper,
                        &mut work,
                    );
                    validate_text_metrics(id, metrics)?;
                    if stampable {
                        resolved.push(PlainResolution::text(id, constraints, layout));
                    }
                    shaped.push((id, metrics, natural, None));
                    continue;
                };
                self.nodes.release_text_layout(id);
                let text = clone_shaped_text(self, id, Some(&presentation));
                work.text_source_clones += 1;
                let runs = shaper.runs;
                let metrics = shaper.shape(id, &text, &style, constraints);
                work.record_text_pass(1, usize::from(shaper.runs > runs));
                validate_text_metrics(id, metrics)?;
                let previous_overlays = self
                    .nodes
                    .text_input_presentation(id)
                    .map(|stored| stored.overlay_metrics.clone())
                    .unwrap_or_default();
                let presentation = shape_text_input_presentation(
                    id,
                    presentation,
                    &style,
                    constraints,
                    &previous_overlays,
                    &mut shaper,
                );
                shaped.push((id, metrics, None, Some(presentation)));
            }
            Ok(())
        })();
        let runs = shaper.runs;
        let wrap_layouts = shaper.wrap_layouts;
        let host_keys = shaper.keys;
        work.text_source_clones += shaper.keys;
        work.text_bytes_hashed += shaper.key_bytes;
        if outcome.is_err() {
            let _ = cache.take_counters();
            let _ = glyphs.take_counters();
            let _ = host.take_text_work();
            // Shown text this pass did not get to resolve stays owed.
            self.text_shown.extend(shown);
            self.text_layout_cache = cache;
            self.glyph_cache = glyphs;
            return outcome;
        }
        self.apply_plain_resolutions(resolved, backend.epoch, &mut work);
        for (id, metrics, natural, presentation) in shaped {
            let previous = self.record(id).text_metrics;
            let previous_natural = self.text_natural_width(id);
            self.record_mut(id).text_metrics = metrics;
            self.nodes.set_text_natural_width(id, natural);
            if let Some(presentation) = presentation {
                // minimap 视口钉住随光标移动失效：reveal 恢复权威。上一趟
                // 与本趟 shape 的光标位置不同即视为移动。
                if self.nodes.text_viewport_pin(id).is_some() {
                    let moved = match self.nodes.text_input_presentation(id) {
                        Some(previous_presentation) => {
                            (previous_presentation.caret_x, previous_presentation.caret_y)
                                != (presentation.caret_x, presentation.caret_y)
                        }
                        None => true,
                    };
                    if moved {
                        self.nodes.set_text_viewport_pin(id, None);
                    }
                }
                self.nodes
                    .set_text_input_presentation(id, Some(presentation));
            }
            if text_intrinsic_changed(previous, metrics) || previous_natural != natural {
                self.propagate_layout_from_node(id);
            }
        }
        for (id, presentation) in empty_shaped {
            self.apply_empty_state_text_presentation(id, presentation);
        }
        for (id, presentation) in modal_shaped {
            self.nodes.set_modal_text(id, Some(presentation));
        }
        let (hits, misses, evictions) = cache.take_counters();
        if host_keys > 0 {
            // The host path's layout cache is the Runtime `TextLayoutCache`.
            work.record_layout_cache(hits, misses);
            work.layout_cache_lookups += hits + misses;
        }
        let glyph_stats = glyphs.take_counters();
        self.text_layout_cache = cache;
        self.glyph_cache = glyphs;
        self.refresh_document_text_highlights(host);
        // Taken last: document highlights measure through the host too.
        work.accumulate(host.take_text_work());
        self.finish_text_pass(
            work,
            runs,
            hits,
            misses,
            evictions,
            wrap_layouts,
            glyph_stats,
        );
        Ok(())
    }
}

impl UiWorld {
    /// Content box and scroll offset used to map pointer coordinates onto
    /// text-input byte offsets, mirroring the paint-side `field_x`/`line_y`.
    pub fn text_input_pointer_context(
        &self,
        id: StableNodeId,
    ) -> Option<(LayoutBox, ScrollOffset)> {
        let node = self.nodes.get(id)?;
        if !matches!(
            self.nodes.visual(id),
            Some(StandardVisual::TextInput { .. })
        ) {
            return None;
        }
        let padding = self.used_layout_padding(id);
        let border = node.style.layout.resolved_border_width();
        let content = LayoutBox {
            x: node.layout.x + border + padding.left,
            y: node.layout.y + border + padding.top,
            width: (node.layout.width - border * 2.0 - padding.left - padding.right).max(0.0),
            height: (node.layout.height - border * 2.0 - padding.top - padding.bottom).max(0.0),
        };
        Some((content, self.record(id).scroll_offset))
    }

    /// Content box and scroll offset used to map pointer coordinates onto
    /// document-selected `TextContent`, matching Scene text origin (padding +
    /// border) rather than the border box.
    pub(crate) fn document_text_pointer_context(
        &self,
        id: StableNodeId,
    ) -> Option<(LayoutBox, ScrollOffset)> {
        Some((
            self.component_content_box(id)?,
            self.record(id).scroll_offset,
        ))
    }

    pub(crate) fn document_text_highlight_lines(
        &self,
        node: StableNodeId,
        start: usize,
        end: usize,
        shaper: &mut dyn TextShaper,
    ) -> Vec<LayoutBox> {
        if start >= end {
            return Vec::new();
        }
        let Some(text) = self.text(node).map(str::to_owned) else {
            return Vec::new();
        };
        if end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Vec::new();
        }
        if let Some((layout, box_width)) = self.vertical_document_text(node) {
            return layout
                .selection_rects(start..end)
                .into_iter()
                .map(|rect| {
                    let rect = layout.page_rect(rect, box_width);
                    LayoutBox {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    }
                })
                .collect();
        }
        let style = self.computed_style(node).cloned().unwrap_or_default();
        shaper.text_highlights(
            node,
            &TextContent { value: text.into() },
            (start, end),
            &style,
            self.text_shape_constraints(node),
        )
    }

    /// The layout a vertical, document-selectable text node was measured
    /// with, and the width of the box it is painted in (#59).
    ///
    /// Static selection is otherwise answered by the shaper's editor geometry,
    /// which lays text out as an *editor* would — and editors stay horizontal.
    /// Asked of a column, it would hit-test and highlight a horizontal line
    /// that is not on screen. The retained layout is the one the painter
    /// draws, so its line space, turned onto the page by the same rule the
    /// painter uses, is where the glyphs are.
    pub(crate) fn vertical_document_text(
        &self,
        node: StableNodeId,
    ) -> Option<(Arc<nana_text::TextLayout>, f32)> {
        let (_, layout) = self.text_layout(node)?;
        if !layout.is_vertical() {
            return None;
        }
        let content = self.component_content_box(node)?;
        Some((Arc::clone(layout), content.width))
    }

    pub(crate) fn refresh_document_text_highlights(&mut self, shaper: &mut dyn TextShaper) {
        self.drop_invalid_document_text_selections();
        let pending: Vec<(DocumentId, crate::DocumentTextSelection)> = self
            .document_text_selections
            .iter()
            .map(|(&document, selection)| (document, selection.clone()))
            .collect();
        for (document, selection) in pending {
            let lines = self.document_text_highlight_lines(
                selection.node,
                selection.start,
                selection.end,
                shaper,
            );
            self.set_document_text_selection(
                document,
                Some(crate::DocumentTextSelection {
                    node: selection.node,
                    start: selection.start,
                    end: selection.end,
                    lines,
                }),
            );
        }
    }
}

impl UiWorld {
    /// Shape inputs for text-input geometry queries: resolved style and the
    /// constraints the last layout pass shaped with.
    pub fn text_input_shape_context(
        &self,
        id: StableNodeId,
    ) -> Option<(ComputedStyle, crate::TextShapeConstraints)> {
        if !matches!(
            self.nodes.visual(id),
            Some(StandardVisual::TextInput { .. })
        ) {
            return None;
        }
        let multiline = self.text_input_kind(id)?;
        let node = self.nodes.get(id)?;
        let style = node.resolved.0.as_ref().clone();
        let vertical = style.writing_mode.is_vertical();
        Some((
            style,
            // The geometry the editor is drawn from, so a probe reads the same
            // layout the caret is painted in -- and the host does not hold two
            // of them for one node.
            text_input_presentation_constraints(
                self.text_shape_constraints(id),
                multiline,
                vertical,
            ),
        ))
    }
}

impl UiWorld {
    /// 值被编辑后重映射折叠态与 snippet 会话。
    pub(super) fn reconcile_text_view_state(
        &mut self,
        id: StableNodeId,
        old_value: &str,
        new_value: &str,
    ) {
        let (changed_start, changed_end, delta) = value_edit_span(old_value, new_value);
        if let Some(entry) = self.nodes.text_fold_view(id).cloned() {
            let next = remap_collapsed_after_edit(
                &entry.collapsed,
                new_value,
                changed_start,
                changed_end,
                delta,
            );
            if next != entry.collapsed {
                self.nodes.set_text_fold_view(
                    id,
                    Some(crate::store::TextFoldViewState {
                        offered: entry.offered,
                        collapsed: next,
                    }),
                );
                self.mark(id, DirtyMask::TEXT | DirtyMask::RENDER);
            }
        }
        if let Some(session) = self.nodes.text_snippet_session(id).cloned() {
            match remap_snippet_session(&session, new_value, changed_start, changed_end, delta) {
                Some(next) if next != session => {
                    self.nodes.set_text_snippet_session(id, Some(next));
                }
                Some(_) => {}
                None => self.nodes.set_text_snippet_session(id, None),
            }
        }
    }
}

impl UiWorld {
    /// 宿主重喂折叠区间后对账折叠态（`offered` 为 `None` 表示宿主不再
    /// 喂入折叠，整个视图状态随之移除）。
    pub(super) fn reconcile_text_fold_offered(
        &mut self,
        id: StableNodeId,
        offered: Option<Arc<[crate::TextCodeFold]>>,
    ) {
        let Some(offered) = offered else {
            if self.nodes.text_fold_view(id).is_some() {
                self.nodes.set_text_fold_view(id, None);
                self.mark(id, DirtyMask::TEXT | DirtyMask::RENDER);
            }
            return;
        };
        let next = match self.nodes.text_fold_view(id) {
            Some(entry) => {
                let collapsed =
                    reconcile_collapsed_folds(&entry.offered, &offered, &entry.collapsed);
                if collapsed == entry.collapsed && entry.offered == offered {
                    return;
                }
                collapsed
            }
            None => Vec::new(),
        };
        self.nodes.set_text_fold_view(
            id,
            Some(crate::store::TextFoldViewState {
                offered,
                collapsed: next,
            }),
        );
        self.mark(id, DirtyMask::TEXT | DirtyMask::RENDER);
    }
}

impl UiWorld {
    /// 滚轮落点命中的 hover 浮窗面板所属节点。命中测试驱动：hover 显示
    /// 不要求焦点，任意文档内编辑器的浮窗面板都可能被滚动。重叠时取最小
    /// 节点 id 保证稳定结果（浮层各自锚定自己的编辑器，正常不重叠）。
    pub(crate) fn text_hover_panel_at(
        &self,
        document: DocumentId,
        x: f32,
        y: f32,
    ) -> Option<StableNodeId> {
        let mut hits: Vec<StableNodeId> = self
            .nodes
            .text_hover_ids()
            .filter(|&id| {
                self.nodes
                    .get(id)
                    .is_some_and(|node| node.document == document)
            })
            .filter(|&id| self.text_hover_panel_hit(id, x, y))
            .collect();
        hits.sort_unstable();
        hits.first().copied()
    }
}

impl UiWorld {
    /// 文本空间坐标命中诊断 span 或行尾文案时，返回用于 hover 的文档。
    pub fn text_diagnostic_hit(
        &self,
        id: StableNodeId,
        local_x: f32,
        local_y: f32,
    ) -> Option<crate::TextHover> {
        let presentation = self.text_input_presentation(id)?;
        presentation
            .diagnostic_hits
            .iter()
            .find(|hit| hit.rect.contains(local_x, local_y))
            .map(|hit| crate::TextHover::new(hit.offset, hit.message.clone(), String::new()))
    }

    pub fn diagnostic_hover_ids(&self) -> Vec<StableNodeId> {
        self.nodes.diagnostic_hover_ids()
    }

    /// 指针是否落在 hover 浮窗面板上（含内边距）。坐标为节点空间。
    pub fn text_hover_panel_hit(&self, id: StableNodeId, x: f32, y: f32) -> bool {
        let Some(crate::ComponentGeometry::TextInput { hover_popup, .. }) =
            self.component_geometry(id)
        else {
            return false;
        };
        hover_popup
            .as_ref()
            .is_some_and(|popup| popup.panel.contains(x, y))
    }
}

impl UiWorld {
    /// 指针是否落在补全弹层面板上（含内边距）。坐标为节点空间；滚轮
    /// 路由用它把弹层内滚动与编辑器滚动分开。
    pub fn text_completion_panel_hit(&self, id: StableNodeId, x: f32, y: f32) -> bool {
        let Some(crate::ComponentGeometry::TextInput {
            completion_popup, ..
        }) = self.component_geometry(id)
        else {
            return false;
        };
        completion_popup
            .as_ref()
            .is_some_and(|popup| popup.panel.contains(x, y))
    }
}

impl UiWorld {
    /// 命中补全弹层的候选行，返回该候选的绝对下标。弹层绘制在折叠
    /// 之上，调用方（框架指针路径）应先于折叠命中查询。坐标为节点空间。
    pub fn text_completion_hit(&self, id: StableNodeId, x: f32, y: f32) -> Option<usize> {
        match self.component_geometry(id)? {
            crate::ComponentGeometry::TextInput {
                completion_popup, ..
            } => {
                let popup = completion_popup.as_ref()?;
                let row = popup
                    .rows
                    .iter()
                    .position(|row| row.bounds.contains(x, y))?;
                Some(popup.first_row + row)
            }
            _ => None,
        }
    }
}

impl UiWorld {
    /// Hit the close control of an atom chip. Coordinates are node space.
    pub fn text_atom_close_hit(&self, id: StableNodeId, x: f32, y: f32) -> Option<Arc<str>> {
        match self.component_geometry(id)? {
            crate::ComponentGeometry::TextInput { atom_chips, .. } => atom_chips
                .iter()
                .find(|chip| chip.close.contains(x, y))
                .map(|chip| Arc::clone(&chip.token)),
            _ => None,
        }
    }

    /// 命中折叠交互区域（gutter 箭头优先，其次折叠起始行的摘要标记），
    /// 返回对应折叠区间。坐标为节点空间。
    pub fn text_fold_hit(&self, id: StableNodeId, x: f32, y: f32) -> Option<crate::TextCodeFold> {
        match self.component_geometry(id)? {
            crate::ComponentGeometry::TextInput { folds, .. } => folds
                .gutters
                .iter()
                .find(|gutter| gutter.bounds.contains(x, y))
                .map(|gutter| gutter.fold)
                .or_else(|| {
                    folds
                        .markers
                        .iter()
                        .find(|marker| marker.bounds.contains(x, y))
                        .map(|marker| marker.fold)
                }),
            _ => None,
        }
    }
}

impl UiWorld {
    /// hover 浮窗正文的滚动行数（宿主查询入口）；未喂入 hover 时为 0。
    pub fn text_hover_scroll(&self, id: StableNodeId) -> usize {
        self.nodes
            .text_hover_view(id)
            .map(|state| state.scroll)
            .unwrap_or(0)
    }
}

impl UiWorld {
    /// 补全会话只读快照（宿主查询入口）。无会话时为 `None`。
    pub fn text_completion_snapshot(
        &self,
        id: StableNodeId,
    ) -> Option<crate::TextCompletionSnapshot> {
        self.nodes
            .text_completion_view(id)
            .map(|state| crate::TextCompletionSnapshot {
                count: state.items.len(),
                selected: state.selected,
                scroll: state.scroll,
                dismissed: state.dismissed,
            })
    }
}

impl UiWorld {
    /// 当前喂入的 hover 文档（供组件投影做喂入去重）。
    pub(crate) fn text_hover_doc(&self, id: StableNodeId) -> Option<&crate::TextHover> {
        self.nodes.text_hover_view(id).map(|state| &state.doc)
    }

    /// 当前喂入的签名帮助（供组件投影做喂入去重）。
    pub(crate) fn text_signature_help(
        &self,
        id: StableNodeId,
    ) -> Option<&crate::TextSignatureHelp> {
        self.nodes.text_signature(id)
    }
}

impl UiWorld {
    /// hover 浮窗状态快照。
    pub(crate) fn text_hover_view(
        &self,
        id: StableNodeId,
    ) -> Option<crate::store::TextHoverViewState> {
        self.nodes.text_hover_view(id).cloned()
    }
}

impl UiWorld {
    /// 当前喂入的补全候选（供组件投影做喂入去重）。
    pub(crate) fn text_completion_items(
        &self,
        id: StableNodeId,
    ) -> Option<&Arc<[crate::TextCompletion]>> {
        self.nodes
            .text_completion_view(id)
            .map(|state| &state.items)
    }
}

impl UiWorld {
    /// 补全弹层会话快照（供框架命令读取-修改-写回与几何层读取）。
    pub(crate) fn text_completion_view(
        &self,
        id: StableNodeId,
    ) -> Option<crate::store::TextCompletionViewState> {
        self.nodes.text_completion_view(id).cloned()
    }
}

impl UiWorld {
    /// snippet 会话快照。
    pub(crate) fn text_snippet_session(
        &self,
        id: StableNodeId,
    ) -> Option<crate::components::TextSnippetSession> {
        self.nodes.text_snippet_session(id).cloned()
    }
}

impl UiWorld {
    /// 折叠视图状态快照（供框架命令读取-修改-写回）。
    pub(crate) fn text_fold_view_state(
        &self,
        id: StableNodeId,
    ) -> Option<crate::store::TextFoldViewState> {
        self.nodes.text_fold_view(id).cloned()
    }
}

impl UiWorld {
    /// 折叠/inlay 后的显示视图；没有折叠态区间与行内提示时 `None`
    /// （零分配短路）。
    /// The fold / inlay display view, built once per change of the text, the
    /// folds, the inlay feed or composing, and shared (O(1) clones) by every
    /// caller after that.
    pub(crate) fn text_display_view(&self, id: StableNodeId) -> Option<TextDisplayView> {
        let editor = self.nodes.editor(id)?;
        let inlays = self.nodes.text_inlays(id);
        let entry = self.nodes.text_fold_view(id);
        if inlays.is_none() && entry.is_none() {
            return None;
        }
        // IME 组合期 inlay 整体退场（同 swatch 先例：组合拼接改变显示
        // 字节布局，值空间锚点漂移）；折叠照常参与组合拼接。一个挂着
        // 却没有组字的 IME（空 preedit）不算组合期。
        let composing = editor.session.is_composing();
        let fed = match (composing, inlays) {
            (false, Some(fed)) => Some(Arc::clone(fed)),
            _ => None,
        };
        let inlays: &[crate::TextInlay] = fed.as_deref().unwrap_or(&[]);
        let collapsed = entry
            .as_ref()
            .map(|entry| entry.collapsed.as_slice())
            .unwrap_or(&[]);
        let key = crate::store::DisplayViewKey {
            text: editor.session.text().stamp(),
            collapsed: collapsed.to_vec(),
            inlays: fed.clone(),
            composing,
        };
        if let Some((cached, view)) = &editor.display.borrow().view
            && *cached == key
        {
            return view.clone();
        }
        let view = build_text_display_view(editor.session.as_str(), collapsed, inlays);
        editor.display.borrow_mut().view = Some((key, view.clone()));
        view
    }

    /// The committed text with the preedit in place of what it replaces, as
    /// the session defines it, built once per text or composition change.
    fn composed_display(&self, id: StableNodeId) -> Option<crate::TextValue> {
        let editor = self.nodes.editor(id)?;
        let session = &editor.session;
        session.composition()?;
        let key = (session.text().stamp(), session.revisions().composition);
        if let Some((cached, text)) = &editor.display.borrow().composed
            && *cached == key
        {
            return Some(text.clone());
        }
        let text = crate::TextValue::stamped(session.display_text().into_owned());
        editor.display.borrow_mut().composed = Some((key, text.clone()));
        Some(text)
    }

    /// 当前喂入的行内提示集（供组件投影做喂入去重）。
    pub(crate) fn text_inlay_items(&self, id: StableNodeId) -> Option<&Arc<[crate::TextInlay]>> {
        self.nodes.text_inlays(id)
    }
}

impl UiWorld {
    /// 当前折叠态的区间（值空间，按 `start` 排序）。没有折叠视图的节点
    /// 返回空表。宿主测试与状态面板的查询入口。
    pub fn text_fold_collapsed(&self, id: StableNodeId) -> Vec<crate::TextCodeFold> {
        self.nodes
            .text_fold_view(id)
            .map(|entry| entry.collapsed.clone())
            .unwrap_or_default()
    }
}

impl UiWorld {
    /// 当前落点指示线（只读，提取层翻译为节点空间图元）。
    pub(crate) fn text_drop_indicator(&self, id: StableNodeId) -> Option<LayoutBox> {
        self.nodes.text_drop_indicator(id).copied()
    }
}

impl UiWorld {
    /// 拖拽移动选中文本的落点指示线（框架侧拖拽状态机写入；文本空间
    /// 矩形）。`None` 清除指示线。
    pub(crate) fn set_text_drop_indicator(&mut self, id: StableNodeId, rect: Option<LayoutBox>) {
        self.nodes.set_text_drop_indicator(id, rect);
    }
}

impl UiWorld {
    /// 写入/清除 minimap 视口钉住。
    pub(crate) fn set_text_viewport_pin(
        &mut self,
        id: StableNodeId,
        pin: Option<crate::store::TextViewportPin>,
    ) {
        self.nodes.set_text_viewport_pin(id, pin);
    }
}

impl UiWorld {
    /// minimap 视口钉住快照（框架导航路径读取-写入）。
    pub(crate) fn text_viewport_pin(
        &self,
        id: StableNodeId,
    ) -> Option<crate::store::TextViewportPin> {
        self.nodes.text_viewport_pin(id).copied()
    }
}

impl UiWorld {
    /// A multiline editor's scrolling area: its text box over the value it
    /// shaped. The value runs from the block-start edge, which `vertical-rl`
    /// puts on the right, so there the later columns overflow to the left and
    /// the offset runs negative from 0, like any container's (#59).
    pub(crate) fn text_scroll_metrics(&self, id: StableNodeId) -> Option<ScrollMetrics> {
        if !self.nodes.get(id)?.accessibility.multiline {
            return None;
        }
        let presentation = self.nodes.text_input_presentation(id)?;
        let text_box = self.text_input_text_box(id)?;
        if text_box.width <= 0.0 || text_box.height <= 0.0 {
            return None;
        }
        let writing = self.computed_style(id)?.writing_context();
        let far_x = writing.is_vertical() && writing.block_reversed();
        let (width, height) = (
            presentation.content_size.width,
            presentation.content_size.height,
        );
        let left = if far_x {
            text_box.x + text_box.width - width
        } else {
            text_box.x
        };
        Some(ScrollMetrics::scrolling_area(
            text_box,
            [left, text_box.y, left + width, text_box.y + height],
            [far_x, false],
        ))
    }

    /// minimap 导航换算：条内点击点 → 目标滚动偏移（点击行在视口居中，
    /// 钳到文档范围；横向偏移保持不变）。不在条内或编辑器无 minimap 时
    /// `None`。只读查询：调用方（框架指针路径）负责写回。
    pub fn text_minimap_scroll_target(
        &self,
        id: StableNodeId,
        x: f32,
        y: f32,
    ) -> Option<ScrollOffset> {
        let Some(crate::ComponentGeometry::TextInput {
            minimap: Some(minimap),
            ..
        }) = self.component_geometry(id)
        else {
            return None;
        };
        if !minimap.panel.contains(x, y) {
            return None;
        }
        let line = minimap.line_at(y)?;
        let presentation = self.nodes.text_input_presentation(id)?;
        let line_height = presentation.line_height.max(1.0);
        let node = self.nodes.get(id)?;
        let padding = self.used_layout_padding(id);
        let border = node.style.layout.resolved_border_width();
        let content_height =
            (node.layout.height - border * 2.0 - padding.top - padding.bottom).max(0.0);
        let total_height = presentation.content_size.height;
        let max_scroll = (total_height - content_height).max(0.0);
        let line_top = presentation
            .line_tops
            .get(line)
            .copied()
            .unwrap_or(line as f32 * line_height);
        let centered = line_top + line_height * 0.5 - content_height * 0.5;
        Some(ScrollOffset {
            x: self.record(id).scroll_offset.x,
            y: centered.clamp(0.0, max_scroll),
        })
    }
}

impl UiWorld {
    /// 计算使多行文本输入内 `offset` 所在逻辑行进入可视区所需的滚动偏移。
    /// 只读查询：不改世界状态；宿主将返回值写回组件的 `scroll_offset`。
    /// 使用排版后的逻辑行起点；当前光标使用精确的软折行位置。
    ///
    /// 折叠感知：存在折叠态区间时按显示视图计算行号与总高；被折叠隐藏
    /// 的偏移钳制到折叠起始行。查找导航到折叠内匹配时的自动展开由框架
    /// 命令负责（reveal 的展开语义），本查询只做几何换算。
    pub fn text_input_reveal_scroll(
        &self,
        id: StableNodeId,
        offset: usize,
    ) -> Option<ScrollOffset> {
        let state = self.nodes.text_input(id)?;
        if !self.nodes.get(id)?.accessibility.multiline {
            return None;
        }
        if !matches!(
            self.nodes.visual(id),
            Some(StandardVisual::TextInput { .. })
        ) {
            return None;
        }
        let presentation = self.nodes.text_input_presentation(id)?;
        let line_height = presentation.line_height.max(1.0);
        let offset = offset.min(state.value.len());
        // 折叠态：显示视图内的行号才是渲染行号；隐藏偏移钳到折叠起始行。
        let (display_value, display_offset) = match self.text_display_view(id) {
            Some(view) => {
                let display_offset = view.display_of(offset).min(view.value.len());
                (view.value, display_offset)
            }
            None => (state.value_shared(), offset),
        };
        let line_index = display_value[..display_offset]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count() as f32;
        let reveal_y = if offset == state.selection.focus {
            presentation.caret_y
        } else {
            presentation
                .line_tops
                .get(line_index as usize)
                .copied()
                .unwrap_or(line_index * line_height)
        };
        let node = self.nodes.get(id)?;
        let padding = self.used_layout_padding(id);
        let border = node.style.layout.resolved_border_width();
        let content_height =
            (node.layout.height - border * 2.0 - padding.top - padding.bottom).max(0.0);
        let total_height = presentation.content_size.height;
        let max_scroll = (total_height - content_height).max(0.0);
        let mut scroll_y = self.record(id).scroll_offset.y;
        if reveal_y < scroll_y {
            scroll_y = reveal_y;
        } else if reveal_y + line_height > scroll_y + content_height {
            scroll_y = reveal_y + line_height - content_height;
        }
        Some(ScrollOffset {
            x: self.record(id).scroll_offset.x,
            y: scroll_y.clamp(0.0, max_scroll),
        })
    }
}

#[cfg(test)]
mod counting_probe_tests {
    use super::*;

    #[derive(Default)]
    struct GeometryShaper {
        batches: usize,
        shapes: usize,
    }
    impl TextShaper for GeometryShaper {
        fn shape(
            &mut self,
            _: StableNodeId,
            _: &TextContent,
            _: &ComputedStyle,
            _: crate::TextShapeConstraints,
        ) -> TextMetrics {
            self.shapes += 1;
            TextMetrics {
                width: 123.0,
                height: 72.0,
                ascent: None,
            }
        }
        fn with_text_probes<R>(
            &mut self,
            _: &TextContent,
            _: &ComputedStyle,
            _: crate::TextShapeConstraints,
            consume: impl FnOnce(&mut dyn TextShaper) -> R,
        ) -> R {
            self.batches += 1;
            consume(self)
        }
        fn horizontal_offset(
            &mut self,
            _: StableNodeId,
            _: &TextContent,
            _: usize,
            _: &ComputedStyle,
        ) -> f32 {
            31.0
        }
        fn text_position(
            &mut self,
            _: StableNodeId,
            _: &TextContent,
            _: usize,
            _: &ComputedStyle,
            _: crate::TextShapeConstraints,
        ) -> (f32, f32, f32) {
            (31.0, 48.0, 24.0)
        }
        fn text_highlights(
            &mut self,
            _: StableNodeId,
            _: &TextContent,
            _: (usize, usize),
            _: &ComputedStyle,
            _: crate::TextShapeConstraints,
        ) -> Vec<LayoutBox> {
            vec![LayoutBox {
                x: 31.0,
                y: 48.0,
                width: 17.0,
                height: 24.0,
            }]
        }
    }

    #[test]
    fn counting_adapter_preserves_host_geometry_batch_and_measurement_cache() {
        let mut host = GeometryShaper::default();
        let mut cache = crate::text_layout_cache::TextLayoutCache::default();
        let mut glyphs = crate::GlyphCache::default();
        let id = StableNodeId(1);
        let text = TextContent {
            value: "first\nsecond".into(),
        };
        let style = ComputedStyle::default();
        let constraints = crate::TextShapeConstraints {
            wrap: true,
            ..Default::default()
        };
        let mut adapter = CountingShaper::new(&mut host, &mut cache, &mut glyphs);
        assert_eq!(
            adapter.text_position(id, &text, 8, &style, constraints),
            (31.0, 48.0, 24.0)
        );
        adapter.with_text_probes(&text, &style, constraints, |prepared| {
            assert_eq!(
                prepared.text_position(id, &text, 8, &style, constraints),
                (31.0, 48.0, 24.0)
            );
            assert_eq!(prepared.horizontal_offset(id, &text, 8, &style), 31.0);
            assert_eq!(
                prepared.text_highlights(id, &text, (6, 8), &style, constraints)[0].y,
                48.0
            );
            assert_eq!(prepared.shape(id, &text, &style, constraints).height, 72.0);
            assert_eq!(prepared.shape(id, &text, &style, constraints).height, 72.0);
        });
        assert_eq!(adapter.runs, 1);
        assert_eq!(adapter.wrap_layouts, 1);
        assert_eq!(host.shapes, 1);
        assert_eq!(host.batches, 1);
    }
}

/// How an editor's text space lands on the page, in every writing mode.
///
/// An editor's geometry — caret, selection, preedit, hit-testing — is in the
/// line space `nana-text` lays out in: `x` along a line from line-left, `y`
/// across the lines from the block-start one. For horizontal text that is
/// the page, offset by the content box and the scroll; for vertical text (#59)
/// `x` runs down a column and `y` across the columns. This is the one place
/// that turns it onto the page and back, so the component geometry a frame
/// draws and the point a pointer hits cannot disagree about where a glyph is.
/// The axes themselves are [`nana_ui_core::WritingContext`]'s, the same
/// line-relative map the painter and `nana-text` use.
///
/// Scrolling is drawn in line space too: `inline_scroll` along the lines,
/// `block_scroll` across them, turned from the physical offset every scroll
/// container records. Where a single-line field's line sits is a
/// scroll as well — centred across the box (a negative block scroll), and
/// against the inline-start edge: the left, or the right of an RTL field, or
/// the bottom of a vertical RTL one (a negative inline scroll).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EditorFrame {
    pub(crate) content: LayoutBox,
    pub(crate) writing: nana_ui_core::WritingContext,
    /// The line box across the line: a caret's length, and a single-line
    /// field's thickness.
    pub(crate) line: f32,
    pub(crate) inline_scroll: f32,
    pub(crate) block_scroll: f32,
}

impl EditorFrame {
    /// The scroll as a physical offset, the inverse of how `editor_frame`
    /// reads the recorded one: the block scroll of `vertical-rl` runs left.
    pub(crate) fn scroll_offset(&self) -> ScrollOffset {
        if !self.writing.is_vertical() {
            return ScrollOffset {
                x: self.inline_scroll,
                y: self.block_scroll,
            };
        }
        ScrollOffset {
            x: self.writing.block_to_page_x(self.block_scroll, 0.0),
            y: self.inline_scroll,
        }
    }

    /// A text-space rectangle (`x`/`width` along the line, `y`/`height`
    /// across it) on the page.
    pub(crate) fn field_rect(&self, rect: LayoutBox) -> LayoutBox {
        let (x, y, width, height) = self.writing.line_rect_to_page(
            (
                rect.x - self.inline_scroll,
                rect.y - self.block_scroll,
                rect.width,
                rect.height,
            ),
            self.content.width,
        );
        LayoutBox {
            x: self.content.x + x,
            y: self.content.y + y,
            width,
            height,
        }
    }

    /// A page point in text space. The inverse of [`Self::field_rect`].
    pub(crate) fn text_point(&self, x: f32, y: f32) -> (f32, f32) {
        let (inline, block) = self.writing.page_point_to_line(
            x - self.content.x,
            y - self.content.y,
            self.content.width,
        );
        (inline + self.inline_scroll, block + self.block_scroll)
    }

    /// The box the painter lays the editor's value out in, given the value's
    /// extent along its lines and across them.
    ///
    /// For a wrapping editor its length along the lines is the content box's
    /// — the line budget the editor geometry wrapped and aligned at, so the
    /// painter wraps and aligns the same. A single-line field's geometry has
    /// no line budget and starts its line at line-left, so the box is exactly
    /// as long as the line: the painter's own `start` alignment has no slack
    /// to move the glyphs off the carets, and where the line sits in the field
    /// is the frame's scroll.
    pub(crate) fn text_bounds(
        &self,
        inline_extent: f32,
        block_extent: f32,
        multiline: bool,
    ) -> LayoutBox {
        let (content_inline, content_block) = self.content_extents();
        self.field_rect(LayoutBox {
            x: 0.0,
            y: 0.0,
            width: if multiline {
                content_inline
            } else {
                inline_extent
            },
            height: if multiline {
                block_extent.max(content_block)
            } else {
                self.line
            },
        })
    }

    /// The scroll along the page's x axis — the inline scroll of horizontal
    /// text, the block scroll of vertical text — and where
    /// [`Self::text_bounds`] starts without it.
    pub(crate) fn scroll_x(
        &self,
        inline_extent: f32,
        block_extent: f32,
        multiline: bool,
    ) -> crate::TextInputScroll {
        let mut unscrolled = *self;
        if self.writing.is_vertical() {
            unscrolled.block_scroll = 0.0;
        } else {
            unscrolled.inline_scroll = 0.0;
        }
        crate::TextInputScroll {
            offset_x: -self.scroll_offset().x,
            text_x: unscrolled
                .text_bounds(inline_extent, block_extent, multiline)
                .x,
        }
    }

    /// The content box's extent along the lines and across them.
    fn content_extents(&self) -> (f32, f32) {
        if self.writing.is_vertical() {
            (self.content.height, self.content.width)
        } else {
            (self.content.width, self.content.height)
        }
    }
}

impl UiWorld {
    /// How far PageUp/PageDown move an editor: its viewport across its
    /// lines — the content box's height, or its width for a vertical editor,
    /// whose lines stack across the page (#59).
    pub fn text_input_page_extent(&self, id: StableNodeId) -> f32 {
        let vertical = self
            .computed_style(id)
            .is_some_and(|style| style.writing_mode.is_vertical());
        self.text_input_pointer_context(id)
            .map_or(0.0, |(content, _)| {
                if vertical {
                    content.width
                } else {
                    content.height
                }
            })
    }

    /// The box an editor's text is drawn in: its content box, less the
    /// number steppers a numeric field keeps at its right edge. Component
    /// geometry and pointer hits both read it, so the two agree on where the
    /// text is.
    pub(crate) fn text_input_text_box(&self, id: StableNodeId) -> Option<LayoutBox> {
        let (content, _) = self.text_input_pointer_context(id)?;
        let Some(StandardVisual::TextInput {
            size,
            steppers: true,
            ..
        }) = self.nodes.visual(id)
        else {
            return Some(content);
        };
        let band = (size.height_in(self.style_model.metrics) / 2.0).min(content.height / 2.0);
        let width = size.indicator_size();
        if band <= 0.0 || width <= 0.0 || content.width <= width {
            return Some(content);
        }
        Some(LayoutBox {
            width: (content.width - width - 4.0).max(0.0),
            ..content
        })
    }

    /// The frame an editor's text space is drawn and hit in.
    ///
    /// Scrolled in line space, turned from the recorded physical offset: a
    /// focused multiline editor reveals its caret
    /// from the offset it was left at (unless a minimap navigation pinned the
    /// viewport), a single-line field follows its caret along the line.
    pub(crate) fn editor_frame(&self, id: StableNodeId) -> Option<EditorFrame> {
        let writing = self.computed_style(id)?.writing_context();
        let Some(StandardVisual::TextInput { size, .. }) = self.nodes.visual(id) else {
            return None;
        };
        let content = self.text_input_text_box(id)?;
        let requested = self.record(id).scroll_offset;
        let vertical = writing.is_vertical();
        // The recorded offset is physical, like every scroll container's. As
        // a displacement it turns into line space mirrored about 0: the block
        // scroll runs from the block-start edge, which `vertical-rl` puts on
        // the right, where the physical offset is negative.
        let requested_line = writing.page_point_to_line(requested.x, requested.y, 0.0);
        let Some(presentation) = self.nodes.text_input_presentation(id) else {
            // Not shaped yet: nothing is drawn, so there is no line to anchor
            // or centre. The recorded offset is all there is, and a pointer
            // still resolves through it.
            let (inline_scroll, block_scroll) = requested_line;
            return Some(EditorFrame {
                content,
                writing,
                line: size.line_height().max(1.0),
                inline_scroll,
                block_scroll,
            });
        };
        let multiline = self
            .nodes
            .get(id)
            .is_some_and(|node| node.accessibility.multiline);
        let focused = self.input.focused.get(&self.record(id).document) == Some(&id);
        let (content_inline, content_block) = if vertical {
            (content.height, content.width)
        } else {
            (content.width, content.height)
        };
        let (inline_extent, block_extent) = if vertical {
            (
                presentation.content_size.height,
                presentation.content_size.width,
            )
        } else {
            (
                presentation.content_size.width,
                presentation.content_size.height,
            )
        };
        // The caret's line box: the editor's own for a wrapping editor, the
        // control's for a single-line field. Never taller than the box.
        let line = if multiline {
            presentation.line_height
        } else {
            size.line_height()
        }
        .max(1.0)
        .min(content_block.max(1.0));
        let (caret_inline, caret_block) = (presentation.caret_x, presentation.caret_y);
        let max_inline = (inline_extent - content_inline).max(0.0);
        let max_block = (block_extent - content_block).max(0.0);
        let (inline_scroll, block_scroll) = if multiline {
            let (mut inline, mut block) = requested_line;
            inline = inline.min(max_inline);
            block = block.min(max_block);
            // A minimap navigation pins the viewport: the caret yields to it
            // until the host rewrites the offset or the caret moves.
            let pinned = self.text_viewport_pin(id) == Some(requested);
            if focused && !pinned {
                if caret_inline < inline {
                    inline = caret_inline;
                } else if caret_inline + 1.0 > inline + content_inline {
                    inline = caret_inline + 1.0 - content_inline;
                }
                if caret_block < block {
                    block = caret_block;
                } else if caret_block + line > block + content_block {
                    block = caret_block + line - content_block;
                }
            }
            (inline.clamp(0.0, max_inline), block.clamp(0.0, max_block))
        } else {
            // The line sits against the field's inline-start edge — the left,
            // the right of an RTL field, the top, or the bottom of a vertical
            // RTL one (CSS Writing Modes §2.1) — and scrolls only as far as it
            // takes to show the caret. Line space runs from line-left, so on a
            // reversed axis the start is the line's far end: a line shorter than
            // the field is a negative scroll, and a longer one shows its far end
            // until the caret heads off the near one. Both are the LTR rule
            // mirrored, so neither keeps any state beyond the caret.
            let inline = if !writing.inline_reversed() {
                (caret_inline - content_inline + 1.0).clamp(0.0, max_inline)
            } else if inline_extent <= content_inline {
                -(content_inline - inline_extent)
            } else {
                caret_inline.clamp(0.0, max_inline)
            };
            (inline, -(content_block - line) * 0.5)
        };
        Some(EditorFrame {
            content,
            writing,
            line,
            inline_scroll,
            block_scroll,
        })
    }
}

/// Aggregate one text pass into the process diagnostics (Issue #227). Pure
/// counter adds; nothing when diagnostics are off.
fn record_text_diagnostics(work: &nana_text::TextWorkCounters) {
    use nana_diagnostics::framework::text;
    if !nana_diagnostics::metrics_enabled() {
        return;
    }
    let pairs = [
        (&text::SHAPE_HITS, work.shape_cache_hits),
        (&text::SHAPE_MISSES, work.shape_cache_misses),
        (&text::LAYOUT_HITS, work.layout_cache_hits),
        (&text::LAYOUT_MISSES, work.layout_cache_misses),
        (&text::GLYPHS_RESOLVED, work.glyphs_resolved),
    ];
    for (metric, value) in pairs {
        if let Some(value) = value.filter(|v| *v > 0) {
            metric.record(value as u64);
        }
    }
}

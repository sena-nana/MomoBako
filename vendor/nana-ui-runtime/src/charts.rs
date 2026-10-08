//! Retained charts. Applications own values and localized labels.

use std::sync::Arc;

use nana_ui_core::{SemanticColor, ThemeMode};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, LayoutBox, LengthSpec,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, StandardVisual, UiWorld,
};

mod donut;
mod stacked;
pub use donut::{DonutChart, DonutSlice};
pub use stacked::TimeSeriesLayer;

const DEFAULT_LABEL: &str = "Time series";

/// Backend-neutral time-series geometry. Scene paint of the grid/area/line is not here.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeSeriesChart {
    pub values: Vec<f64>,
    pub layers: Vec<TimeSeriesLayer>,
    pub axis_labels: Vec<Arc<str>>,
    pub tooltip_details: Vec<Arc<str>>,
    pub active: Option<usize>,
    /// Unix milliseconds and optional samples. None and non-finite samples leave gaps.
    pub samples: Option<Vec<(i64, Option<f64>)>>,
    pub unit: Option<Arc<str>>,
    pub time_labels: Option<(Arc<str>, Arc<str>)>,
    pub label: Option<Arc<str>>,
    pub style: NodeStyle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeSeriesPaint {
    pub grid: SemanticColor,
    pub area: SemanticColor,
    pub line: SemanticColor,
}

impl TimeSeriesChart {
    pub const INTRINSIC_HEIGHT: f32 = 148.0;
    pub const INSET_X: f32 = nana_ui_core::space::MD;
    pub const INSET_Y: f32 = nana_ui_core::space::LG;
    pub const GRID_LINE_COUNT: usize = 4;
    pub const LINE_WIDTH: f32 = nana_ui_core::space::XXS;

    pub fn new(values: impl IntoIterator<Item = f64>) -> Self {
        Self {
            values: values.into_iter().map(sanitize_value).collect(),
            layers: Vec::new(),
            axis_labels: Vec::new(),
            tooltip_details: Vec::new(),
            active: None,
            samples: None,
            unit: None,
            time_labels: None,
            label: None,
            style: NodeStyle::default(),
        }
    }

    /// Creates a time-proportional series, ordered by Unix milliseconds.
    /// Missing/non-finite values interrupt the line; repeated timestamps retain input order.
    pub fn from_samples(samples: impl IntoIterator<Item = (i64, Option<f64>)>) -> Self {
        let mut samples: Vec<_> = samples
            .into_iter()
            .map(|(time, value)| {
                (
                    time,
                    value
                        .filter(|value| value.is_finite())
                        .map(|value| value.max(0.0)),
                )
            })
            .collect();
        samples.sort_by_key(|sample| sample.0);
        Self {
            samples: Some(samples),
            ..Self::new([])
        }
    }

    pub fn unit(mut self, unit: impl Into<Arc<str>>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Localized endpoint labels; formatting/timezone remains the consumer's responsibility.
    pub fn time_labels(mut self, start: impl Into<Arc<str>>, end: impl Into<Arc<str>>) -> Self {
        self.time_labels = Some((start.into(), end.into()));
        self
    }

    /// Independent contiguous runs. Unlike `points`, this preserves missing-data gaps.
    pub fn segments(&self, bounds: LayoutBox) -> Vec<Vec<(f32, f32)>> {
        let Some(samples) = &self.samples else {
            let points = self.points(bounds);
            return if points.is_empty() {
                Vec::new()
            } else {
                vec![points]
            };
        };
        let Some(first) = samples.first() else {
            return Vec::new();
        };
        let span = (samples.last().unwrap().0 as i128 - first.0 as i128).max(1) as f64;
        let maximum = samples
            .iter()
            .filter_map(|sample| sample.1)
            .fold(1.0_f64, f64::max);
        let width = (bounds.width - Self::INSET_X * 2.0).max(1.0);
        let height = (bounds.height - Self::INSET_Y * 2.0).max(1.0);
        let mut runs = Vec::new();
        let mut run = Vec::new();
        for (time, value) in samples {
            if let Some(value) = value.filter(|value| value.is_finite()) {
                let elapsed = (*time as i128 - first.0 as i128) as f64;
                run.push((
                    Self::INSET_X + width * (elapsed / span) as f32,
                    Self::INSET_Y + height * (1.0 - (value / maximum).clamp(0.0, 1.0) as f32),
                ));
            } else if !run.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
        }
        if !run.is_empty() {
            runs.push(run);
        }
        runs
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        let label = label.into();
        self.label = Some(if label.is_empty() {
            Arc::from(DEFAULT_LABEL)
        } else {
            label
        });
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    /// Local points using inset (`INSET_X=8`, `INSET_Y=10`).
    /// Use `segments` when drawing timestamp samples to retain missing-data gaps.
    pub fn points(&self, bounds: LayoutBox) -> Vec<(f32, f32)> {
        if self.samples.is_some() {
            return self.segments(bounds).into_iter().flatten().collect();
        }
        let values: Vec<f64> = self.values.iter().copied().map(sanitize_value).collect();
        if values.is_empty() {
            return Vec::new();
        }
        let width = (bounds.width - Self::INSET_X * 2.0).max(1.0);
        let height = (bounds.height - Self::INSET_Y * 2.0).max(1.0);
        let maximum = values.iter().copied().fold(0.0_f64, f64::max).max(1.0);
        let denominator = values.len().saturating_sub(1).max(1) as f32;
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let x = Self::INSET_X + width * index as f32 / denominator;
                let normalized = (*value / maximum).clamp(0.0, 1.0) as f32;
                (x, Self::INSET_Y + height * (1.0 - normalized))
            })
            .collect()
    }

    /// Four horizontal grid-line Y coordinates (`0..=3`).
    pub fn grid_ys(bounds: LayoutBox) -> [f32; 4] {
        let span = (bounds.height - Self::INSET_Y * 2.0).max(1.0);
        core::array::from_fn(|division| Self::INSET_Y + span * division as f32 / 3.0)
    }

    fn resolved_label(&self) -> Arc<str> {
        self.label
            .clone()
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| Arc::from(DEFAULT_LABEL))
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.width.get_or_insert(LengthSpec::Fill);
        layout
            .height
            .get_or_insert(LengthSpec::Px(Self::INTRINSIC_HEIGHT));
        layout
            .min_height
            .get_or_insert(LengthSpec::Px(Self::INTRINSIC_HEIGHT));
        style
    }
}

/// Sparkline colors: grid `border_soft` at 0.55, area accent at 0.16, line `accent_strong`.
pub fn time_series_paint(mode: ThemeMode) -> TimeSeriesPaint {
    let palette = mode.palette();
    TimeSeriesPaint {
        grid: SemanticColor {
            a: 0.55,
            ..palette.border_soft
        },
        area: SemanticColor {
            a: 0.16,
            ..palette.accent
        },
        line: palette.accent_strong,
    }
}

fn sanitize_value(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
fn inert() -> InteractionState {
    InteractionState {
        pointer_events: false,
        focusable: false,
    }
}

impl ComponentView for TimeSeriesChart {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "time-series-chart".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let visual = if let Some(samples) = &self.samples {
            StandardVisual::TimestampSeriesChart {
                samples: samples.clone().into(),
                unit: self.unit.clone(),
                time_labels: self.time_labels.clone(),
            }
        } else if !self.layers.is_empty() {
            StandardVisual::StackedTimeSeriesChart {
                title: self.resolved_label(),
                values: self.values.clone().into(),
                layers: self.layers.clone().into(),
                labels: self.axis_labels.clone().into(),
                active: self.active,
            }
        } else {
            StandardVisual::TimeSeriesChart {
                values: self.values.clone().into(),
            }
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
                pointer_events: self.samples.is_none() && !self.layers.is_empty(),
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Image,
                label: Some(self.resolved_label()),
                ..AccessibilityState::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::AppContext;
    use crate::{DocumentId, NodeKind, StandardVisual};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn bounds(width: f32, height: f32) -> LayoutBox {
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }

    #[test]
    fn timestamps_control_spacing_and_missing_values_split_runs() {
        let chart = TimeSeriesChart::from_samples([
            (10_000, Some(10.0)),
            (0, Some(0.0)),
            (1_000, Some(5.0)),
            (2_000, None),
            (9_000, Some(8.0)),
        ]);
        let segments = chart.segments(bounds(116.0, 120.0));
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], vec![(8.0, 110.0), (18.0, 60.0)]);
        assert_eq!(segments[1].len(), 2);
        assert_eq!(segments[1][0].0, 98.0);
        assert_eq!(segments[1][1], (108.0, 10.0));
    }

    #[test]
    fn missing_samples_are_not_zero_and_extreme_timestamps_do_not_overflow() {
        let chart = TimeSeriesChart::from_samples([
            (i64::MIN, Some(3.0)),
            (0, Some(f64::NAN)),
            (i64::MAX, Some(3.0)),
        ]);
        let segments = chart.segments(bounds(116.0, 120.0));
        assert_eq!(segments, vec![vec![(8.0, 10.0)], vec![(108.0, 10.0)]]);
        assert!(
            TimeSeriesChart::from_samples([(0, None), (1, None)])
                .segments(bounds(116.0, 120.0))
                .is_empty()
        );
    }

    #[test]
    fn timestamp_chart_projects_samples_and_localized_axis_metadata() {
        let mut context = AppContext::new();
        let chart = context
            .create_component(
                document(),
                TimeSeriesChart::from_samples([(0, Some(1.0)), (1000, None), (5000, Some(2.0))])
                    .unit("people")
                    .time_labels("10:00", "10:05"),
            )
            .unwrap();
        let Some(StandardVisual::TimestampSeriesChart {
            samples,
            unit,
            time_labels,
        }) = context.world().standard_visual(chart.stable_id())
        else {
            panic!("timestamp visual");
        };
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[1].1, None);
        assert_eq!(unit.as_deref(), Some("people"));
        assert!(time_labels.is_some());
    }

    #[test]
    fn empty_series_has_no_points() {
        let chart = TimeSeriesChart::new([]);
        assert!(chart.values.is_empty());
        assert!(chart.points(bounds(108.0, 120.0)).is_empty());
        assert_eq!(TimeSeriesChart::grid_ys(bounds(108.0, 120.0)).len(), 4);
    }

    #[test]
    fn single_value_sits_on_the_left_inset() {
        let chart = TimeSeriesChart::new([10.0]);
        let points = chart.points(bounds(108.0, 120.0));
        assert_eq!(points.len(), 1);
        assert_eq!(points[0], (8.0, 10.0));
    }

    #[test]
    fn multiple_values_span_and_scale_to_the_largest() {
        let chart = TimeSeriesChart::new([0.0, 5.0, 10.0]);
        let points = chart.points(bounds(108.0, 120.0));
        assert_eq!(points.len(), 3);
        assert_eq!(points[0], (8.0, 110.0));
        assert_eq!(points[1], (54.0, 60.0));
        assert_eq!(points[2], (100.0, 10.0));
    }

    #[test]
    fn non_finite_and_negative_values_become_zero() {
        let chart = TimeSeriesChart::new([f64::NAN, -2.0, 4.0, f64::INFINITY]);
        assert_eq!(chart.values, vec![0.0, 0.0, 4.0, 0.0]);
        let points = chart.points(bounds(108.0, 120.0));
        assert_eq!(points.len(), 4);
        assert_eq!(points[2].1, 10.0);
        assert!(points[0].1 > points[2].1);
        assert_eq!(points[0].1, points[1].1);
        assert_eq!(points[0].1, points[3].1);
    }

    #[test]
    fn higher_values_have_smaller_y() {
        let chart = TimeSeriesChart::new([1.0, 3.0, 2.0]);
        let points = chart.points(bounds(108.0, 120.0));
        assert_eq!(points.len(), 3);
        assert!(points[1].1 < points[2].1);
        assert!(points[2].1 < points[0].1);
    }

    #[test]
    fn chart_projects_a_fill_width_inert_leaf() {
        let mut context = AppContext::new();
        let chart = context
            .create_component(document(), TimeSeriesChart::new([1.0, 2.0, 3.0]))
            .unwrap();
        let id = chart.stable_id();
        assert!(matches!(
            context.world().node(id).unwrap().kind,
            NodeKind::Element { tag } if tag == "time-series-chart"
        ));
        assert!(matches!(
            context.world().standard_visual(id),
            Some(StandardVisual::TimeSeriesChart { .. })
        ));
        let style = context.world().node_style(id).unwrap();
        assert_eq!(style.layout.width, Some(LengthSpec::Fill));
        assert_eq!(
            style.layout.height,
            Some(LengthSpec::Px(TimeSeriesChart::INTRINSIC_HEIGHT))
        );
        assert_eq!(context.world().interaction(id), Some(inert()));
        let accessibility = context.world().accessibility(id).unwrap();
        assert_eq!(accessibility.role, AccessibilityRole::Image);
        assert_eq!(accessibility.label.as_deref(), Some(DEFAULT_LABEL));
    }

    #[test]
    fn chart_commits_time_series_standard_visual() {
        let mut context = AppContext::new();
        let view = TimeSeriesChart::new([1.0, 2.0, 3.0]);
        let expected = StandardVisual::TimeSeriesChart {
            values: view.values.clone().into(),
        };
        let chart = context.create_component(document(), view).unwrap();
        assert_eq!(
            context.world().standard_visual(chart.stable_id()),
            Some(expected)
        );
    }
}

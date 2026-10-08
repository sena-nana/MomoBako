use super::*;
use nana_ui_core::SemanticColorRole;

#[derive(Debug, Clone, PartialEq)]
pub struct TimeSeriesLayer {
    pub label: Arc<str>,
    pub values: Vec<f64>,
    pub color: SemanticColorRole,
}
impl TimeSeriesLayer {
    pub fn new(
        label: impl Into<Arc<str>>,
        values: impl IntoIterator<Item = f64>,
        color: SemanticColorRole,
    ) -> Self {
        Self {
            label: label.into(),
            values: values.into_iter().map(sanitize_value).collect(),
            color,
        }
    }
}
impl TimeSeriesChart {
    pub fn stacked(mut self, layers: impl IntoIterator<Item = TimeSeriesLayer>) -> Self {
        self.layers = layers.into_iter().collect();
        self
    }
    pub fn axis_labels(mut self, labels: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        self.axis_labels = labels.into_iter().map(Into::into).collect();
        self
    }
    pub fn tooltip_details(
        mut self,
        details: impl IntoIterator<Item = impl Into<Arc<str>>>,
    ) -> Self {
        self.tooltip_details = details.into_iter().map(Into::into).collect();
        self
    }
    pub fn datum_at(&self, bounds: LayoutBox, x: f32, y: f32) -> Option<usize> {
        let plot = Self::stacked_plot(bounds);
        if plot.width <= 0.0
            || plot.height <= 0.0
            || self.samples.is_some()
            || self.layers.is_empty()
            || self.values.is_empty()
            || !x.is_finite()
            || !y.is_finite()
            || x < plot.x
            || x > plot.x + plot.width
            || y < plot.y
            || y > plot.y + plot.height
        {
            return None;
        }
        Some(
            (((x - plot.x) / plot.width * self.values.len() as f32) as usize)
                .min(self.values.len() - 1),
        )
    }
    pub fn tooltip(&self, index: usize) -> Option<String> {
        let total = self.values.get(index)?;
        let mut rows = vec![
            self.axis_labels
                .get(index)
                .map(|s| s.to_string())
                .unwrap_or_else(|| (index + 1).to_string()),
        ];
        rows.extend(self.layers.iter().map(|layer| {
            format!(
                "{}: {:.0}",
                layer.label,
                layer.values.get(index).copied().unwrap_or(0.0)
            )
        }));
        rows.push(format!(
            "{}: {:.0}",
            self.label.as_deref().unwrap_or("Total"),
            total
        ));
        if let Some(detail) = self.tooltip_details.get(index) {
            rows.push(detail.to_string());
        }
        Some(rows.join("\n"))
    }
    pub(crate) fn stacked_plot(bounds: LayoutBox) -> LayoutBox {
        let width = bounds.width.max(0.0);
        let height = bounds.height.max(0.0);
        let left = (nana_ui_core::space::PAGE * 2.0).min(width);
        let top = nana_ui_core::space::XL.min(height);
        let right = nana_ui_core::space::XL;
        let bottom = nana_ui_core::space::PAGE * 2.0 + nana_ui_core::space::SM;
        LayoutBox {
            x: bounds.x + left,
            y: bounds.y + top,
            width: (width - left - right).max(0.0),
            height: (height - top - bottom).max(0.0),
        }
    }
}
impl DonutChart {
    pub fn tooltip(&self, index: usize) -> Option<String> {
        let slice = self.slices.get(index)?;
        let max = self
            .slices
            .iter()
            .map(|s| sanitize_value(s.value))
            .fold(0.0_f64, f64::max);
        if max <= 0.0 || sanitize_value(slice.value) <= 0.0 {
            return None;
        }
        let total: f64 = self
            .slices
            .iter()
            .map(|s| sanitize_value(s.value) / max)
            .sum();
        Some(format!(
            "{}: {:.0} ({:.0}%)",
            self.labels
                .get(index)
                .map(|s| s.as_ref())
                .unwrap_or("Value"),
            slice.value,
            sanitize_value(slice.value) / max / total * 100.0
        ))
    }
}

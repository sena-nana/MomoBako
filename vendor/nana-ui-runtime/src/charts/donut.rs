use super::*;
/// A semantic segment of a donut chart. Invalid and negative values contribute no area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DonutSlice {
    pub value: f64,
    pub color: nana_ui_core::SemanticColorRole,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DonutChart {
    pub slices: Vec<DonutSlice>,
    pub labels: Vec<Arc<str>>,
    pub active: Option<usize>,
    pub cutout: f32,
    pub separator: f32,
    pub label: Arc<str>,
    pub style: NodeStyle,
}

impl DonutChart {
    pub fn new(slices: impl IntoIterator<Item = DonutSlice>) -> Self {
        Self {
            slices: slices.into_iter().collect(),
            labels: Vec::new(),
            active: None,
            cutout: 0.62,
            separator: 2.0,
            label: Arc::from("Donut chart"),
            style: NodeStyle::default(),
        }
    }
    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = label.into();
        self
    }
    pub fn labels(mut self, labels: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        self.labels = labels.into_iter().map(Into::into).collect();
        self
    }
    pub fn cutout(mut self, cutout: f32) -> Self {
        self.cutout = sanitized_cutout(cutout);
        self
    }
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    /// Absolute center-line arcs and ring width, with logical-pixel separators.
    #[allow(clippy::type_complexity)]
    pub fn arcs(
        &self,
        bounds: LayoutBox,
    ) -> (f32, Vec<(Vec<[f32; 2]>, nana_ui_core::SemanticColorRole)>) {
        let sectors = self.sectors(bounds);
        let width = sectors.first().map_or(0.0, |sector| sector.width);
        let arcs = sectors
            .into_iter()
            .map(|sector| {
                let steps = ((sector.end - sector.start) * f64::from(sector.radius) / 1.5)
                    .ceil()
                    .clamp(2.0, 8192.0) as usize;
                let points = (0..=steps)
                    .map(|step| {
                        let a =
                            sector.start + (sector.end - sector.start) * step as f64 / steps as f64;
                        [
                            sector.center[0] + sector.radius * a.cos() as f32,
                            sector.center[1] + sector.radius * a.sin() as f32,
                        ]
                    })
                    .collect();
                (points, self.slices[sector.index].color)
            })
            .collect();
        (width, arcs)
    }

    /// One circular border quad per slice, cut to it by one wedge polygon.
    /// Unlike flat-capped polylines this has no interior joins or alpha
    /// overdraw, and unlike wedges cut edge to edge, no seam: a clip edge
    /// ramps over a pixel, and two ramps laid over each other leave a quarter
    /// of the colour out. A slice that closes the ring has no polygon.
    #[allow(clippy::type_complexity)]
    pub(crate) fn ring_regions(
        &self,
        bounds: LayoutBox,
    ) -> (
        f32,
        Vec<(LayoutBox, Vec<[f32; 2]>, nana_ui_core::SemanticColorRole)>,
    ) {
        let sectors = self.sectors(bounds);
        let width = sectors.first().map_or(0.0, |sector| sector.width);
        let mut regions = Vec::new();
        for sector in sectors {
            let outer = sector.radius + sector.width / 2.0;
            let circle = LayoutBox {
                x: sector.center[0] - outer,
                y: sector.center[1] - outer,
                width: outer * 2.0,
                height: outer * 2.0,
            };
            let sweep = sector.end - sector.start;
            // The centre, then points twice the ring's radius out at most a
            // quarter turn apart, whose chords clear the ring: at most six
            // points, within the eight a quad's polygon holds.
            let polygon = if sweep >= std::f64::consts::TAU - 1e-9 {
                Vec::new()
            } else {
                let parts = (sweep / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
                std::iter::once([outer, outer])
                    .chain((0..=parts).map(|part| {
                        let angle = sector.start + sweep * part as f64 / parts as f64;
                        [
                            outer + outer * 2.0 * angle.cos() as f32,
                            outer + outer * 2.0 * angle.sin() as f32,
                        ]
                    }))
                    .collect()
            };
            regions.push((circle, polygon, self.slices[sector.index].color));
        }
        (width, regions)
    }

    pub fn slice_at(&self, bounds: LayoutBox, x: f32, y: f32) -> Option<usize> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        self.sectors(bounds).into_iter().rev().find_map(|sector| {
            let dx = x - sector.center[0];
            let dy = y - sector.center[1];
            let distance = dx.hypot(dy);
            let angle = (f64::from(dy.atan2(dx)) + std::f64::consts::FRAC_PI_2)
                .rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::FRAC_PI_2;
            (distance >= sector.radius - sector.width / 2.0
                && distance <= sector.radius + sector.width / 2.0
                && angle >= sector.start
                && angle <= sector.end)
                .then_some(sector.index)
        })
    }

    fn sectors(&self, bounds: LayoutBox) -> Vec<DonutSector> {
        if ![bounds.x, bounds.y, bounds.width, bounds.height]
            .into_iter()
            .all(f32::is_finite)
        {
            return Vec::new();
        }
        let outer = bounds.width.min(bounds.height).max(0.0) / 2.0;
        let width = outer * (1.0 - sanitized_cutout(self.cutout));
        let radius = outer - width / 2.0;
        let maximum = self
            .slices
            .iter()
            .map(|s| sanitize_value(s.value))
            .fold(0.0, f64::max);
        if maximum <= 0.0 || radius <= 0.0 {
            return Vec::new();
        }
        let total: f64 = self
            .slices
            .iter()
            .map(|s| sanitize_value(s.value) / maximum)
            .sum();
        let count = self
            .slices
            .iter()
            .filter(|s| sanitize_value(s.value) > 0.0)
            .count();
        let separator = if self.separator.is_finite() {
            self.separator.max(0.0)
        } else {
            2.0
        };
        let gap = if count > 1 {
            f64::from(separator / radius)
        } else {
            0.0
        };
        let mut angle = -std::f64::consts::FRAC_PI_2;
        let mut sectors = Vec::new();
        for (index, slice) in self.slices.iter().enumerate() {
            let sweep = sanitize_value(slice.value) / maximum / total * std::f64::consts::TAU;
            let start = angle + gap.min(sweep) / 2.0;
            let end = angle + sweep - gap.min(sweep) / 2.0;
            if end > start {
                let offset = if self.active == Some(index) { 3.0 } else { 0.0 };
                let mid = angle + sweep / 2.0;
                sectors.push(DonutSector {
                    index,
                    start,
                    end,
                    width,
                    radius,
                    center: [
                        bounds.x + bounds.width / 2.0 + offset * mid.cos() as f32,
                        bounds.y + bounds.height / 2.0 + offset * mid.sin() as f32,
                    ],
                });
            }
            angle += sweep;
        }
        sectors
    }
}
impl ComponentView for DonutChart {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "donut-chart".into(),
        }
    }
    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let visual = StandardVisual::DonutChart {
            slices: self.slices.clone().into(),
            cutout: self.cutout,
            separator: self.separator,
            active: self.active,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.width.get_or_insert(LengthSpec::Px(66.0));
        layout.height.get_or_insert(LengthSpec::Px(66.0));
        project_common(
            id,
            world,
            mutations,
            &style,
            InteractionState {
                pointer_events: true,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Image,
                label: Some(self.label.clone()),
                ..AccessibilityState::default()
            },
        );
    }
}

pub(super) fn sanitized_cutout(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 0.99)
    } else {
        0.62
    }
}

struct DonutSector {
    index: usize,
    start: f64,
    end: f64,
    center: [f32; 2],
    radius: f32,
    width: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui_core::SemanticColorRole;
    #[test]
    fn arcs_and_hit_regions_share_gaps_holes_and_invalid_value_handling() {
        let mut chart = DonutChart::new([
            DonutSlice {
                value: f64::NAN,
                color: SemanticColorRole::Danger,
            },
            DonutSlice {
                value: f64::MAX,
                color: SemanticColorRole::Accent,
            },
            DonutSlice {
                value: f64::MAX,
                color: SemanticColorRole::Success,
            },
        ]);
        let bounds = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let (width, arcs) = chart.arcs(bounds);
        assert!((width - 19.0).abs() < 0.001);
        assert_eq!(arcs.len(), 2);
        assert!(
            arcs.iter()
                .flat_map(|arc| &arc.0)
                .flatten()
                .all(|v| v.is_finite())
        );
        assert_eq!(chart.slice_at(bounds, 50.0, 50.0), None);
        assert_eq!(chart.slice_at(bounds, 50.0, 0.0), None);
        assert_eq!(chart.slice_at(bounds, 95.0, 50.0), Some(1));
        chart.active = Some(1);
        assert_eq!(chart.slice_at(bounds, 102.0, 50.0), Some(1));
        assert_eq!(chart.slice_at(bounds, 0.0, 50.0), Some(2));
        chart.slices.clear();
        assert!(chart.arcs(bounds).1.is_empty());
        assert!(chart.tooltip(0).is_none());
    }

    #[test]
    fn each_slice_is_one_region_cut_by_one_polygon_and_a_whole_ring_by_none() {
        let bounds = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let slice = |value| DonutSlice {
            value,
            color: SemanticColorRole::Accent,
        };
        // 350°, 10°: the first wedge needs four chords, six points.
        let chart = DonutChart::new([slice(35.0), slice(1.0)]);
        let (_, regions) = chart.ring_regions(bounds);
        let points: Vec<_> = regions.iter().map(|region| region.1.len()).collect();
        assert_eq!(points, [6, 3]);
        let (_, regions) = DonutChart::new([slice(1.0)]).ring_regions(bounds);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].1.is_empty(), "{:?}", regions[0].1);
    }
}

//! Week-column calendar heatmap. Application owns dates, values, titles, and meaning.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use nana_ui_core::{SemanticColor, ThemeMode};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, LayoutBox, LengthSpec,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, StandardVisual, UiWorld,
};

const DAYS_PER_WEEK: usize = 7;
const DEFAULT_LABEL: &str = "Calendar heatmap";

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarHeatmapDatum<T = ()> {
    pub date: String,
    pub value: f32,
    pub data: Option<T>,
}

impl<T> CalendarHeatmapDatum<T> {
    pub fn new(date: impl Into<String>, value: f32) -> Self {
        Self {
            date: date.into(),
            value,
            data: None,
        }
    }

    pub fn data(mut self, data: T) -> Self {
        self.data = Some(data);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarHeatmapCell<T = ()> {
    pub date: String,
    pub value: f32,
    pub data: Option<T>,
    pub level: u8,
    pub week_start: String,
    pub title: String,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarHeatmapActiveCell<T = ()> {
    pub date: String,
    pub value: f32,
    pub data: Option<T>,
    pub level: u8,
    pub title: String,
    pub x: f32,
    pub y: f32,
}

impl<T: Clone> From<&CalendarHeatmapCell<T>> for CalendarHeatmapActiveCell<T> {
    fn from(cell: &CalendarHeatmapCell<T>) -> Self {
        Self {
            date: cell.date.clone(),
            value: cell.value,
            data: cell.data.clone(),
            level: cell.level,
            title: cell.title.clone(),
            x: cell.x,
            y: cell.y,
        }
    }
}

/// Theme-free cell paint. Scene maps `level` through [`calendar_cell_fill`].
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarHeatmapCellPaint {
    pub x: f32,
    pub y: f32,
    pub level: u8,
}

/// Theme-free axis label paint in widget-local coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarHeatmapLabelPaint {
    pub text: Arc<str>,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarHeatmapMonthLabel {
    pub key: String,
    pub label: String,
    pub x: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarHeatmapDayLabel {
    pub day: u8,
    pub label: String,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarHeatmapModel<T = ()> {
    pub cells: Vec<CalendarHeatmapCell<T>>,
    pub month_labels: Vec<CalendarHeatmapMonthLabel>,
    pub day_labels: Vec<CalendarHeatmapDayLabel>,
    pub width: f32,
    pub height: f32,
    pub cell_size: f32,
    pub cell_gap: f32,
    pub cell_radius: f32,
    pub label_width: f32,
    pub month_label_height: f32,
}

impl<T> CalendarHeatmapModel<T> {
    pub fn cell_index_at(&self, x: f32, y: f32) -> Option<usize> {
        let relative_x = x - self.label_width;
        let relative_y = y - self.month_label_height;
        if !relative_x.is_finite()
            || !relative_y.is_finite()
            || relative_x < 0.0
            || relative_y < 0.0
        {
            return None;
        }
        let step = self.cell_size + self.cell_gap;
        if step <= 0.0 {
            return None;
        }
        let week = (relative_x / step).floor() as usize;
        let day = (relative_y / step).floor() as usize;
        if day >= DAYS_PER_WEEK {
            return None;
        }
        let local_x = relative_x - week as f32 * step;
        let local_y = relative_y - day as f32 * step;
        (local_x <= self.cell_size && local_y <= self.cell_size)
            .then_some(week * DAYS_PER_WEEK + day)
            .filter(|index| *index < self.cells.len())
    }

    pub fn week_count(&self) -> usize {
        self.cells.len() / DAYS_PER_WEEK
    }

    pub fn max_level(&self) -> u8 {
        self.cells
            .iter()
            .map(|cell| cell.level)
            .max()
            .unwrap_or(4)
            .max(1)
    }
}

impl<T: Clone> CalendarHeatmapModel<T> {
    /// Local-widget hit. Points in the weekday/month gutters or in a cell gap miss.
    pub fn cell_at(&self, x: f32, y: f32) -> Option<CalendarHeatmapActiveCell<T>> {
        self.cell_index_at(x, y)
            .and_then(|index| self.cells.get(index).map(Into::into))
    }

    pub fn cell_at_in(
        &self,
        bounds: LayoutBox,
        x: f32,
        y: f32,
    ) -> Option<CalendarHeatmapActiveCell<T>> {
        self.cell_at(x - bounds.x, y - bounds.y)
    }
}

pub type CalendarLevelResolver<T> =
    Arc<dyn Fn(&CalendarHeatmapDatum<T>, (f32, f32)) -> u8 + Send + Sync>;
pub type CalendarTitleFormatter<T> = Arc<dyn Fn(&CalendarHeatmapDatum<T>) -> String + Send + Sync>;
pub type CalendarMonthFormatter = Arc<dyn Fn(i32, u8) -> String + Send + Sync>;

pub enum CalendarLevelStrategy<T> {
    Relative {
        levels: u8,
    },
    Thresholds(Vec<f32>),
    Custom {
        levels: u8,
        resolve: CalendarLevelResolver<T>,
    },
}

impl<T> Clone for CalendarLevelStrategy<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Relative { levels } => Self::Relative { levels: *levels },
            Self::Thresholds(thresholds) => Self::Thresholds(thresholds.clone()),
            Self::Custom { levels, resolve } => Self::Custom {
                levels: *levels,
                resolve: Arc::clone(resolve),
            },
        }
    }
}

impl<T> fmt::Debug for CalendarLevelStrategy<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Relative { levels } => formatter
                .debug_struct("Relative")
                .field("levels", levels)
                .finish(),
            Self::Thresholds(thresholds) => formatter
                .debug_tuple("Thresholds")
                .field(thresholds)
                .finish(),
            Self::Custom { levels, .. } => formatter
                .debug_struct("Custom")
                .field("levels", levels)
                .finish_non_exhaustive(),
        }
    }
}

/// Closures compare by identity: the same resolver is equal, an equivalent
/// one built anew is not. A component comparing unequal only costs the
/// no-op fast path in `AppContext::update_component`.
impl<T> PartialEq for CalendarLevelStrategy<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Relative { levels: a }, Self::Relative { levels: b }) => a == b,
            (Self::Thresholds(a), Self::Thresholds(b)) => a == b,
            (
                Self::Custom {
                    levels: a,
                    resolve: resolve_a,
                },
                Self::Custom {
                    levels: b,
                    resolve: resolve_b,
                },
            ) => a == b && Arc::ptr_eq(resolve_a, resolve_b),
            _ => false,
        }
    }
}

impl<T> Default for CalendarLevelStrategy<T> {
    fn default() -> Self {
        Self::Relative { levels: 5 }
    }
}

pub struct CalendarHeatmapOptions<T> {
    pub cell_size: f32,
    pub cell_gap: f32,
    pub cell_radius: f32,
    pub label_width: f32,
    pub month_label_height: f32,
    pub week_starts_on: u8,
    pub weekday_labels: Vec<(u8, String)>,
    pub level_strategy: CalendarLevelStrategy<T>,
    pub month_formatter: CalendarMonthFormatter,
    pub title_formatter: CalendarTitleFormatter<T>,
}

impl<T> Clone for CalendarHeatmapOptions<T> {
    fn clone(&self) -> Self {
        Self {
            cell_size: self.cell_size,
            cell_gap: self.cell_gap,
            cell_radius: self.cell_radius,
            label_width: self.label_width,
            month_label_height: self.month_label_height,
            week_starts_on: self.week_starts_on,
            weekday_labels: self.weekday_labels.clone(),
            level_strategy: self.level_strategy.clone(),
            month_formatter: Arc::clone(&self.month_formatter),
            title_formatter: Arc::clone(&self.title_formatter),
        }
    }
}

impl<T> fmt::Debug for CalendarHeatmapOptions<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CalendarHeatmapOptions")
            .field("cell_size", &self.cell_size)
            .field("cell_gap", &self.cell_gap)
            .field("cell_radius", &self.cell_radius)
            .field("label_width", &self.label_width)
            .field("month_label_height", &self.month_label_height)
            .field("week_starts_on", &self.week_starts_on)
            .field("weekday_labels", &self.weekday_labels)
            .field("level_strategy", &self.level_strategy)
            .finish_non_exhaustive()
    }
}

impl<T> PartialEq for CalendarHeatmapOptions<T> {
    fn eq(&self, other: &Self) -> bool {
        self.cell_size == other.cell_size
            && self.cell_gap == other.cell_gap
            && self.cell_radius == other.cell_radius
            && self.label_width == other.label_width
            && self.month_label_height == other.month_label_height
            && self.week_starts_on == other.week_starts_on
            && self.weekday_labels == other.weekday_labels
            && self.level_strategy == other.level_strategy
            && Arc::ptr_eq(&self.month_formatter, &other.month_formatter)
            && Arc::ptr_eq(&self.title_formatter, &other.title_formatter)
    }
}

impl<T> Default for CalendarHeatmapOptions<T> {
    fn default() -> Self {
        Self {
            cell_size: CalendarHeatmap::<()>::CELL_SIZE,
            cell_gap: CalendarHeatmap::<()>::CELL_GAP,
            cell_radius: CalendarHeatmap::<()>::CELL_RADIUS,
            label_width: CalendarHeatmap::<()>::LABEL_WIDTH,
            month_label_height: CalendarHeatmap::<()>::MONTH_LABEL_HEIGHT,
            week_starts_on: 1,
            weekday_labels: vec![
                (1, "周一".to_owned()),
                (3, "周三".to_owned()),
                (5, "周五".to_owned()),
            ],
            level_strategy: CalendarLevelStrategy::default(),
            month_formatter: Arc::new(|_year, month| format!("{month}月")),
            title_formatter: Arc::new(|datum| format!("{}: {}", datum.date, datum.value)),
        }
    }
}

impl<T> CalendarHeatmapOptions<T> {
    pub fn cell_metrics(mut self, size: f32, gap: f32, radius: f32) -> Self {
        self.cell_size = finite_positive(size, CalendarHeatmap::<()>::CELL_SIZE);
        self.cell_gap = finite_non_negative(gap, CalendarHeatmap::<()>::CELL_GAP);
        self.cell_radius = finite_non_negative(radius, CalendarHeatmap::<()>::CELL_RADIUS)
            .min(self.cell_size / 2.0);
        self
    }

    pub fn week_starts_on(mut self, day: i32) -> Self {
        self.week_starts_on = day.rem_euclid(7) as u8;
        self
    }

    pub fn weekday_labels(
        mut self,
        labels: impl IntoIterator<Item = (u8, impl Into<String>)>,
    ) -> Self {
        self.weekday_labels = labels
            .into_iter()
            .map(|(day, label)| (day % 7, label.into()))
            .collect();
        self
    }

    pub fn level_strategy(mut self, strategy: CalendarLevelStrategy<T>) -> Self {
        self.level_strategy = strategy;
        self
    }

    pub fn month_formatter(
        mut self,
        formatter: impl Fn(i32, u8) -> String + Send + Sync + 'static,
    ) -> Self {
        self.month_formatter = Arc::new(formatter);
        self
    }

    pub fn title_formatter(
        mut self,
        formatter: impl Fn(&CalendarHeatmapDatum<T>) -> String + Send + Sync + 'static,
    ) -> Self {
        self.title_formatter = Arc::new(formatter);
        self
    }
}

pub fn build_calendar_heatmap_model<T: Clone>(
    data: &[CalendarHeatmapDatum<T>],
    mut options: CalendarHeatmapOptions<T>,
) -> CalendarHeatmapModel<T> {
    options.cell_size = finite_positive(options.cell_size, CalendarHeatmap::<()>::CELL_SIZE);
    options.cell_gap = finite_non_negative(options.cell_gap, CalendarHeatmap::<()>::CELL_GAP);
    options.cell_radius =
        finite_non_negative(options.cell_radius, CalendarHeatmap::<()>::CELL_RADIUS)
            .min(options.cell_size / 2.0);
    options.label_width =
        finite_non_negative(options.label_width, CalendarHeatmap::<()>::LABEL_WIDTH);
    options.month_label_height = finite_non_negative(
        options.month_label_height,
        CalendarHeatmap::<()>::MONTH_LABEL_HEIGHT,
    );

    let mut dated: Vec<_> = data
        .iter()
        .filter_map(|datum| parse_date(&datum.date).map(|day| (day, datum)))
        .collect();
    dated.sort_by_key(|(day, _)| *day);
    let Some((first, _)) = dated.first() else {
        return empty_model(&options);
    };
    let last = dated.last().map_or(*first, |(day, _)| *day);
    let by_day: BTreeMap<_, _> = dated.iter().map(|(day, datum)| (*day, *datum)).collect();
    let values: Vec<_> = dated
        .iter()
        .map(|(_, datum)| datum.value)
        .filter(|value| value.is_finite())
        .collect();
    let range = values
        .iter()
        .copied()
        .fold(None, |range, value| {
            Some(match range {
                None => (value, value),
                Some((min, max)) => (f32::min(min, value), f32::max(max, value)),
            })
        })
        .unwrap_or((0.0, 0.0));
    let start = start_of_week(*first, options.week_starts_on);
    let end = start_of_week(last, options.week_starts_on) + 6;
    let week_count = ((end - start + 1) / 7) as usize;
    let mut cells = Vec::with_capacity(week_count * DAYS_PER_WEEK);
    for week in 0..week_count {
        let week_start = start + week as i64 * 7;
        for offset in 0..DAYS_PER_WEEK {
            let day = week_start + offset as i64;
            let source = by_day.get(&day).copied();
            let (year, month, date) = civil_from_days(day);
            let date_key = format!("{year:04}-{month:02}-{date:02}");
            let fallback = CalendarHeatmapDatum::new(date_key.clone(), 0.0);
            let datum = source.unwrap_or(&fallback);
            cells.push(CalendarHeatmapCell {
                date: datum.date.clone(),
                value: datum.value,
                data: datum.data.clone(),
                level: resolve_level(datum, &options.level_strategy, range),
                week_start: date_key_from_days(week_start),
                title: (options.title_formatter)(datum),
                x: options.label_width + week as f32 * (options.cell_size + options.cell_gap),
                y: options.month_label_height
                    + offset as f32 * (options.cell_size + options.cell_gap),
            });
        }
    }
    let width = options.label_width
        + (week_count as f32 * (options.cell_size + options.cell_gap) - options.cell_gap).max(0.0)
        + 2.0;
    let height = options.month_label_height
        + DAYS_PER_WEEK as f32 * options.cell_size
        + (DAYS_PER_WEEK - 1) as f32 * options.cell_gap
        + 2.0;
    let month_labels = build_month_labels(&cells, *first, last, &options);
    let day_labels = build_day_labels(&options);
    CalendarHeatmapModel {
        cells,
        month_labels,
        day_labels,
        width,
        height,
        cell_size: options.cell_size,
        cell_gap: options.cell_gap,
        cell_radius: options.cell_radius,
        label_width: options.label_width,
        month_label_height: options.month_label_height,
    }
}

/// Heatmap fill: level 0 is `subtle`, otherwise accent mixed into subtle.
pub fn calendar_cell_fill(mode: ThemeMode, level: u8, max_level: u8) -> SemanticColor {
    let palette = mode.palette();
    if level == 0 {
        return palette.subtle;
    }
    let amount = f32::from(level) / f32::from(max_level.max(1));
    mix_color(palette.accent, palette.subtle, 0.25 + amount * 0.75)
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalendarHeatmapEvent<T = ()> {
    CellEnter(CalendarHeatmapActiveCell<T>),
    CellMove(CalendarHeatmapActiveCell<T>),
    CellLeave,
}

/// Retained heatmap leaf. Geometry and level mapping are the Runtime authority.
#[derive(Clone, PartialEq)]
pub struct CalendarHeatmap<T = ()> {
    pub data: Vec<CalendarHeatmapDatum<T>>,
    pub options: CalendarHeatmapOptions<T>,
    pub active: Option<usize>,
    pub label: Option<Arc<str>>,
    pub style: NodeStyle,
}

impl<T> Default for CalendarHeatmap<T> {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            options: CalendarHeatmapOptions::default(),
            active: None,
            label: None,
            style: NodeStyle::default(),
        }
    }
}

impl<T> CalendarHeatmap<T> {
    pub const CELL_SIZE: f32 = 11.0;
    pub const CELL_GAP: f32 = 3.0;
    pub const CELL_RADIUS: f32 = nana_ui_core::space::XXS;
    pub const LABEL_WIDTH: f32 = 42.0;
    pub const MONTH_LABEL_HEIGHT: f32 = nana_ui_core::type_scale::SECTION;

    pub fn new(data: impl IntoIterator<Item = CalendarHeatmapDatum<T>>) -> Self {
        Self {
            data: data.into_iter().collect(),
            ..Self::default()
        }
    }

    pub fn options(mut self, options: CalendarHeatmapOptions<T>) -> Self {
        self.options = options;
        self
    }

    pub fn week_starts_on(mut self, day: i32) -> Self {
        self.options = self.options.week_starts_on(day);
        self
    }

    pub fn level_strategy(mut self, strategy: CalendarLevelStrategy<T>) -> Self {
        self.options = self.options.level_strategy(strategy);
        self
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
}

impl<T: Clone> CalendarHeatmap<T> {
    pub fn model(&self) -> CalendarHeatmapModel<T> {
        build_calendar_heatmap_model(&self.data, self.options.clone())
    }

    pub fn cell_at(&self, x: f32, y: f32) -> Option<CalendarHeatmapActiveCell<T>> {
        self.model().cell_at(x, y)
    }

    pub fn cell_at_in(
        &self,
        bounds: LayoutBox,
        x: f32,
        y: f32,
    ) -> Option<CalendarHeatmapActiveCell<T>> {
        self.model().cell_at_in(bounds, x, y)
    }

    pub fn active_cell(&self) -> Option<CalendarHeatmapActiveCell<T>> {
        let model = self.model();
        self.active
            .and_then(|index| model.cells.get(index).map(Into::into))
    }

    /// Pointer/touch hit in local widget coordinates. `None` is a leave.
    pub fn set_pointer(&mut self, point: Option<(f32, f32)>) -> Option<CalendarHeatmapEvent<T>> {
        let model = self.model();
        let next = point.and_then(|(x, y)| model.cell_index_at(x, y));
        let event = match (self.active, next) {
            (Some(previous), Some(next_index)) if previous == next_index => model
                .cells
                .get(next_index)
                .map(|cell| CalendarHeatmapEvent::CellMove(cell.into())),
            (_, Some(next_index)) => model
                .cells
                .get(next_index)
                .map(|cell| CalendarHeatmapEvent::CellEnter(cell.into())),
            (Some(_), None) => Some(CalendarHeatmapEvent::CellLeave),
            (None, None) => None,
        };
        self.active = next;
        event
    }

    pub fn paint_cells(&self) -> Arc<[CalendarHeatmapCellPaint]> {
        self.model()
            .cells
            .iter()
            .map(|cell| CalendarHeatmapCellPaint {
                x: cell.x,
                y: cell.y,
                level: cell.level,
            })
            .collect()
    }

    pub fn paint_month_labels(&self) -> Arc<[CalendarHeatmapLabelPaint]> {
        self.model()
            .month_labels
            .iter()
            .map(|label| CalendarHeatmapLabelPaint {
                text: Arc::from(label.label.as_str()),
                x: label.x,
                y: 0.0,
            })
            .collect()
    }

    pub fn paint_day_labels(&self) -> Arc<[CalendarHeatmapLabelPaint]> {
        self.model()
            .day_labels
            .iter()
            .map(|label| CalendarHeatmapLabelPaint {
                text: Arc::from(label.label.as_str()),
                x: label.x,
                y: label.y,
            })
            .collect()
    }

    pub fn max_level(&self) -> u8 {
        self.model()
            .cells
            .iter()
            .map(|cell| cell.level)
            .max()
            .unwrap_or(0)
            .max(1)
    }

    fn resolved_label(&self, model: &CalendarHeatmapModel<T>) -> Arc<str> {
        if let Some(index) = self.active
            && let Some(cell) = model.cells.get(index)
        {
            return Arc::from(cell.title.as_str());
        }
        self.label
            .clone()
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| Arc::from(DEFAULT_LABEL))
    }

    fn effective_style(&self, model: &CalendarHeatmapModel<T>) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(model.width));
        layout.height = Some(LengthSpec::Px(model.height));
        layout.min_width = Some(LengthSpec::Px(model.width));
        layout.min_height = Some(LengthSpec::Px(model.height));
        style
    }
}

impl<T> ComponentView for CalendarHeatmap<T>
where
    T: Clone + PartialEq + Send + 'static,
{
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "calendar-heatmap".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let model = self.model();
        let visual = StandardVisual::CalendarHeatmap {
            cells: self.paint_cells(),
            month_labels: self.paint_month_labels(),
            day_labels: self.paint_day_labels(),
            cell_size: model.cell_size,
            cell_radius: model.cell_radius,
            max_level: self.max_level(),
            active: self.active,
            active_title: self
                .active
                .and_then(|index| model.cells.get(index))
                .map(|cell| Arc::from(cell.title.as_str())),
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(&model),
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
            AccessibilityState {
                role: AccessibilityRole::Image,
                label: Some(self.resolved_label(&model)),
                numeric_value: self
                    .active
                    .and_then(|index| model.cells.get(index))
                    .map(|cell| f64::from(cell.value)),
                ..AccessibilityState::default()
            },
        );
    }
}

impl crate::AppContext {
    pub fn is_calendar_heatmap(&self, id: crate::StableNodeId) -> bool {
        self.read(crate::Entity::<CalendarHeatmap>::from_stable_id(id), |_| ())
            .is_ok()
    }

    pub fn hover_calendar_heatmap(
        &mut self,
        target: crate::StableNodeId,
        x: f32,
        y: f32,
    ) -> Result<bool, crate::FrameworkError> {
        if !self.is_calendar_heatmap(target) {
            return Ok(false);
        }
        let entity = crate::Entity::<CalendarHeatmap>::from_stable_id(target);
        let Some(bounds) = self.world().layout_box(target) else {
            return Ok(false);
        };
        let local = bounds
            .contains(x, y)
            .then_some((x - bounds.x, y - bounds.y));
        self.update_component(entity, |calendar, cx| {
            if let Some(event) = calendar.set_pointer(local) {
                cx.emit(event);
            }
            true
        })?;
        Ok(true)
    }

    pub fn clear_calendar_heatmap_hover(
        &mut self,
        document: crate::DocumentId,
    ) -> Result<bool, crate::FrameworkError> {
        // The pointer chain calls this on every move that leaves a heatmap, so
        // it walks the component index rather than the document: a tree with no
        // calendar in it answers without touching a node.
        let ids = self
            .world()
            .nodes_of_component(
                document,
                crate::component_descriptors::CALENDAR_HEATMAP.type_id,
            )
            .filter(|id| self.is_calendar_heatmap(*id))
            .collect::<Vec<_>>();
        let mut changed = false;
        for id in ids {
            let entity = crate::Entity::<CalendarHeatmap>::from_stable_id(id);
            if !self.read(entity, |calendar| calendar.active.is_some())? {
                continue;
            }
            changed |= self.update_component(entity, |calendar, cx| {
                if let Some(event) = calendar.set_pointer(None) {
                    cx.emit(event);
                }
                true
            })?;
        }
        Ok(changed)
    }
}

fn empty_model<T>(options: &CalendarHeatmapOptions<T>) -> CalendarHeatmapModel<T> {
    CalendarHeatmapModel {
        cells: Vec::new(),
        month_labels: Vec::new(),
        day_labels: build_day_labels(options),
        width: options.label_width + 2.0,
        height: options.month_label_height
            + DAYS_PER_WEEK as f32 * options.cell_size
            + (DAYS_PER_WEEK - 1) as f32 * options.cell_gap
            + 2.0,
        cell_size: options.cell_size,
        cell_gap: options.cell_gap,
        cell_radius: options.cell_radius,
        label_width: options.label_width,
        month_label_height: options.month_label_height,
    }
}

fn build_day_labels<T>(options: &CalendarHeatmapOptions<T>) -> Vec<CalendarHeatmapDayLabel> {
    options
        .weekday_labels
        .iter()
        .map(|(day, label)| {
            let offset = (i32::from(*day) - i32::from(options.week_starts_on)).rem_euclid(7) as f32;
            CalendarHeatmapDayLabel {
                day: *day,
                label: label.clone(),
                x: 0.0,
                y: options.month_label_height
                    + offset * (options.cell_size + options.cell_gap)
                    + options.cell_size
                    - 1.0,
            }
        })
        .collect()
}

fn build_month_labels<T>(
    cells: &[CalendarHeatmapCell<T>],
    start: i64,
    end: i64,
    options: &CalendarHeatmapOptions<T>,
) -> Vec<CalendarHeatmapMonthLabel> {
    let mut labels = Vec::new();
    let mut last_month = None;
    for (week, cells) in cells.chunks(DAYS_PER_WEEK).enumerate() {
        let month = cells
            .iter()
            .filter_map(|cell| parse_date(&cell.date))
            .filter(|day| *day >= start && *day <= end)
            .map(civil_from_days)
            .find(|(_, month, _)| Some(*month) != last_month);
        let Some((year, month, _)) = month else {
            continue;
        };
        last_month = Some(month);
        labels.push(CalendarHeatmapMonthLabel {
            key: cells
                .first()
                .map_or_else(|| week.to_string(), |cell| cell.week_start.clone()),
            label: (options.month_formatter)(year, month),
            x: options.label_width
                + week as f32 * (options.cell_size + options.cell_gap)
                + options.cell_size / 2.0,
        });
    }
    labels
}

fn resolve_level<T>(
    datum: &CalendarHeatmapDatum<T>,
    strategy: &CalendarLevelStrategy<T>,
    range: (f32, f32),
) -> u8 {
    match strategy {
        CalendarLevelStrategy::Thresholds(thresholds) => thresholds
            .iter()
            .filter(|threshold| datum.value >= **threshold)
            .count()
            .min(u8::MAX as usize) as u8,
        CalendarLevelStrategy::Custom { levels, resolve } => {
            resolve(datum, range).min((*levels).max(2).saturating_sub(1))
        }
        CalendarLevelStrategy::Relative { levels } => {
            let upper = (*levels).max(2).saturating_sub(1);
            if datum.value <= 0.0 || range.1 <= 0.0 {
                0
            } else {
                (((datum.value / range.1) * f32::from(upper)).ceil() as u8).clamp(1, upper)
            }
        }
    }
}

fn mix_color(foreground: SemanticColor, background: SemanticColor, ratio: f32) -> SemanticColor {
    let ratio = ratio.clamp(0.0, 1.0);
    SemanticColor {
        r: foreground.r * ratio + background.r * (1.0 - ratio),
        g: foreground.g * ratio + background.g * (1.0 - ratio),
        b: foreground.b * ratio + background.b * (1.0 - ratio),
        a: foreground.a * ratio + background.a * (1.0 - ratio),
    }
}

fn finite_positive(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

fn finite_non_negative(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        fallback
    }
}

fn parse_date(value: &str) -> Option<i64> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    let max_day = days_in_month(year, month);
    if day == 0 || day > max_day {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

fn start_of_week(day: i64, week_starts_on: u8) -> i64 {
    let weekday = (day + 4).rem_euclid(7);
    day - (weekday - i64::from(week_starts_on)).rem_euclid(7)
}

fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(day: i64) -> (i32, u8, u8) {
    let day = day + 719_468;
    let era = day.div_euclid(146_097);
    let day_of_era = day - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year as i32, month as u8, day as u8)
}

fn date_key_from_days(day: i64) -> String {
    let (year, month, day) = civil_from_days(day);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::{
        CalendarHeatmap, CalendarHeatmapDatum, CalendarHeatmapEvent, CalendarHeatmapOptions,
        CalendarLevelStrategy, build_calendar_heatmap_model, civil_from_days, days_from_civil,
    };
    use crate::framework::AppContext;
    use crate::{DocumentId, LayoutBox, LengthSpec, MutationQueue, NodeKind, StandardVisual};
    use std::sync::Arc;

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn june_sample() -> Vec<CalendarHeatmapDatum> {
        vec![
            CalendarHeatmapDatum::new("2026-06-01", 2.0),
            CalendarHeatmapDatum::new("2026-06-03", 8.0),
        ]
    }

    #[test]
    fn week_grouping_fills_complete_monday_weeks() {
        let model = build_calendar_heatmap_model(&june_sample(), CalendarHeatmapOptions::default());
        assert_eq!(model.week_count(), 1);
        assert_eq!(model.cells.len(), 7);
        assert_eq!(model.cells[0].date, "2026-06-01");
        assert_eq!(model.cells[0].week_start, "2026-06-01");
        assert_eq!(model.cells[2].date, "2026-06-03");
        assert_eq!(model.month_labels.len(), 1);
        assert_eq!(model.month_labels[0].label, "6月");

        let sunday = build_calendar_heatmap_model(
            &june_sample(),
            CalendarHeatmapOptions::default().week_starts_on(0),
        );
        assert_eq!(sunday.week_count(), 1);
        assert_eq!(sunday.cells[0].date, "2026-05-31");
        assert!(model.cells[0].y < sunday.cells[1].y);

        let spanned = build_calendar_heatmap_model(
            &[
                CalendarHeatmapDatum::<()>::new("2026-06-01", 1.0),
                CalendarHeatmapDatum::<()>::new("2026-06-08", 1.0),
            ],
            CalendarHeatmapOptions::default(),
        );
        assert_eq!(spanned.week_count(), 2);
        assert_eq!(spanned.cells.len(), 14);
        assert_eq!(spanned.cells[7].week_start, "2026-06-08");
    }

    #[test]
    fn level_buckets_follow_relative_thresholds_and_custom() {
        let relative =
            build_calendar_heatmap_model(&june_sample(), CalendarHeatmapOptions::default());
        let june_first = relative
            .cells
            .iter()
            .find(|cell| cell.date == "2026-06-01")
            .expect("June 1");
        let june_third = relative
            .cells
            .iter()
            .find(|cell| cell.date == "2026-06-03")
            .expect("June 3");
        let empty_day = relative
            .cells
            .iter()
            .find(|cell| cell.date == "2026-06-07")
            .expect("padded Sunday at week end");
        assert_eq!(june_first.level, 1);
        assert_eq!(june_third.level, 4);
        assert_eq!(empty_day.level, 0);
        assert_eq!(empty_day.value, 0.0);

        let zeroed = build_calendar_heatmap_model(
            &[
                CalendarHeatmapDatum::<()>::new("2026-06-01", 0.0),
                CalendarHeatmapDatum::<()>::new("2026-06-02", -3.0),
            ],
            CalendarHeatmapOptions::default(),
        );
        assert!(zeroed.cells.iter().all(|cell| cell.level == 0));

        let thresholds = build_calendar_heatmap_model(
            &june_sample(),
            CalendarHeatmapOptions::default()
                .level_strategy(CalendarLevelStrategy::Thresholds(vec![2.0, 4.0, 8.0])),
        );
        assert_eq!(
            thresholds
                .cells
                .iter()
                .find(|cell| cell.date == "2026-06-01")
                .map(|cell| cell.level),
            Some(1)
        );
        assert_eq!(
            thresholds
                .cells
                .iter()
                .find(|cell| cell.date == "2026-06-03")
                .map(|cell| cell.level),
            Some(3)
        );

        let custom = build_calendar_heatmap_model(
            &june_sample(),
            CalendarHeatmapOptions::default().level_strategy(CalendarLevelStrategy::Custom {
                levels: 5,
                resolve: Arc::new(|datum, _range| if datum.value >= 8.0 { 9 } else { 1 }),
            }),
        );
        assert_eq!(
            custom
                .cells
                .iter()
                .find(|cell| cell.date == "2026-06-03")
                .map(|cell| cell.level),
            Some(4)
        );
    }

    #[test]
    fn hit_testing_accepts_paint_bounds_and_rejects_gaps() {
        let model = build_calendar_heatmap_model(&june_sample(), CalendarHeatmapOptions::default());
        let cell = model
            .cells
            .iter()
            .find(|cell| cell.date == "2026-06-03")
            .expect("June 3 cell");
        assert_eq!(
            model
                .cell_at(cell.x + 1.0, cell.y + 1.0)
                .expect("painted cell")
                .date,
            "2026-06-03"
        );
        assert_eq!(
            model
                .cell_at(cell.x + model.cell_size, cell.y + model.cell_size)
                .expect("inclusive edge")
                .date,
            "2026-06-03"
        );
        assert!(
            model
                .cell_at(cell.x + model.cell_size + 1.0, cell.y + 1.0)
                .is_none()
        );
        assert!(model.cell_at(0.0, cell.y + 1.0).is_none());
        assert!(model.cell_at(cell.x + 1.0, 0.0).is_none());
        assert!(model.cell_at(f32::NAN, cell.y).is_none());

        let bounds = LayoutBox {
            x: 10.0,
            y: 20.0,
            width: model.width,
            height: model.height,
        };
        assert_eq!(
            model
                .cell_at_in(bounds, bounds.x + cell.x + 1.0, bounds.y + cell.y + 1.0)
                .map(|hit| hit.date),
            Some("2026-06-03".to_owned())
        );

        let mut heatmap = CalendarHeatmap::new(june_sample());
        let enter = heatmap
            .set_pointer(Some((cell.x + 1.0, cell.y + 1.0)))
            .expect("enter");
        assert!(matches!(enter, CalendarHeatmapEvent::CellEnter(hit) if hit.date == "2026-06-03"));
        assert!(matches!(
            heatmap.set_pointer(Some((cell.x + 2.0, cell.y + 2.0))),
            Some(CalendarHeatmapEvent::CellMove(_))
        ));
        assert_eq!(
            heatmap.set_pointer(None),
            Some(CalendarHeatmapEvent::CellLeave)
        );
        assert!(heatmap.active_cell().is_none());
    }

    #[test]
    fn empty_and_invalid_data_keep_weekday_axis() {
        let empty = build_calendar_heatmap_model::<()>(&[], CalendarHeatmapOptions::default());
        assert!(empty.cells.is_empty());
        assert_eq!(empty.week_count(), 0);
        assert_eq!(empty.day_labels.len(), 3);
        assert!(empty.month_labels.is_empty());
        assert_eq!(empty.width, 44.0);
        assert!(
            empty
                .cell_at(empty.label_width + 1.0, empty.month_label_height + 1.0)
                .is_none()
        );

        let invalid = build_calendar_heatmap_model(
            &[
                CalendarHeatmapDatum::<()>::new("not-a-date", 4.0),
                CalendarHeatmapDatum::<()>::new("2026-02-30", 4.0),
                CalendarHeatmapDatum::<()>::new("2026-13-01", 4.0),
            ],
            CalendarHeatmapOptions::default(),
        );
        assert!(invalid.cells.is_empty());
        assert_eq!(invalid.day_labels.len(), 3);
    }

    #[test]
    fn date_conversion_round_trips_leap_days() {
        let days = days_from_civil(2024, 2, 29);
        assert_eq!(civil_from_days(days), (2024, 2, 29));
    }

    #[test]
    fn heatmap_projects_a_sized_focusable_leaf() {
        let mut context = AppContext::new();
        let heatmap = context
            .create_component(document(), CalendarHeatmap::new(june_sample()))
            .unwrap();
        let id = heatmap.stable_id();
        assert!(matches!(
            context.world().node(id).unwrap().kind,
            NodeKind::Element { tag } if tag == "calendar-heatmap"
        ));
        assert!(matches!(
            context.world().standard_visual(id),
            Some(StandardVisual::CalendarHeatmap { .. })
        ));
        let style = context.world().node_style(id).unwrap();
        assert_eq!(style.layout.width, Some(LengthSpec::Px(55.0)));
        assert_eq!(style.layout.height, Some(LengthSpec::Px(111.0)));
        let interaction = context.world().interaction(id).unwrap();
        assert!(interaction.pointer_events);
        assert!(interaction.focusable);
        let accessibility = context.world().accessibility(id).unwrap();
        assert_eq!(accessibility.role, crate::AccessibilityRole::Image);
        assert_eq!(accessibility.label.as_deref(), Some("Calendar heatmap"));
    }

    #[test]
    fn heatmap_commits_calendar_heatmap_standard_visual() {
        let mut context = AppContext::new();
        let view = CalendarHeatmap::new(june_sample());
        let model = view.model();
        let expected = StandardVisual::CalendarHeatmap {
            cells: view.paint_cells(),
            month_labels: view.paint_month_labels(),
            day_labels: view.paint_day_labels(),
            cell_size: model.cell_size,
            cell_radius: model.cell_radius,
            max_level: view.max_level(),
            active: view.active,
            active_title: None,
        };
        let heatmap = context.create_component(document(), view).unwrap();
        assert_eq!(
            context.world().standard_visual(heatmap.stable_id()),
            Some(expected)
        );
    }

    #[test]
    fn hover_sets_active_cell_ring_and_title_tooltip() {
        let mut context = AppContext::new();
        let heatmap = context
            .create_component(document(), CalendarHeatmap::new(june_sample()))
            .unwrap();
        let model = context.read(heatmap, CalendarHeatmap::model).unwrap();
        let cell = model
            .cells
            .iter()
            .find(|cell| cell.date == "2026-06-03")
            .expect("June 3");
        context
            .commit_mutations({
                let mut mutations = MutationQueue::new();
                mutations.write_layout(
                    heatmap.stable_id(),
                    LayoutBox {
                        x: 0.0,
                        y: 0.0,
                        width: model.width,
                        height: model.height,
                    },
                );
                mutations
            })
            .unwrap();

        assert!(
            context
                .hover_calendar_heatmap(heatmap.stable_id(), cell.x + 1.0, cell.y + 1.0)
                .unwrap()
        );
        assert_eq!(
            context.read(heatmap, |calendar| calendar.active).unwrap(),
            Some(
                model
                    .cells
                    .iter()
                    .position(|item| item.date == "2026-06-03")
                    .expect("index")
            )
        );
        let Some(StandardVisual::CalendarHeatmap {
            active_title,
            active,
            ..
        }) = context.world().standard_visual(heatmap.stable_id())
        else {
            panic!("hovered calendar must keep a heatmap visual");
        };
        assert!(active.is_some());
        assert_eq!(active_title.as_deref(), Some("2026-06-03: 8"));
        let crate::ComponentGeometry::CalendarHeatmap { hover, .. } = context
            .world()
            .component_geometry(heatmap.stable_id())
            .expect("hovered geometry")
        else {
            panic!("expected calendar geometry");
        };
        let hover = hover.expect("hovered cell paints hover chrome");
        assert_eq!(hover.title.content.as_ref(), "2026-06-03: 8");
        assert!(hover.tooltip.width < 176.0);
        assert!(hover.tooltip.width > nana_ui_core::TooltipConfig::PADDING_X * 2.0);

        assert!(context.clear_calendar_heatmap_hover(document()).unwrap());
        assert!(
            context
                .read(heatmap, |calendar| calendar.active)
                .unwrap()
                .is_none()
        );
    }
}

//! Month grid the user picks a day from.
//!
//! Assembled from existing controls — a header row of icon buttons and a text
//! label, then a grid of day buttons — rather than a painted calendar, so it
//! inherits control sizing, focus, hover and accessibility unchanged. Place it
//! wherever you want it: inline in a form, or inside a `Popover` the
//! application owns.
//!
//! It reports the day the user chose. Formatting the month heading, storing the
//! value and deciding what a date means stay the application's; supply the
//! heading through [`DatePicker::month_label`].

use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, ButtonKind, CivilDate, ControlSize, FlexDirection, Icon, LengthSpec, MonthGrid,
    SemanticColorRole, WeekStart, space,
};

use crate::view_components::{Activate, Button, IconButton, Text, project_common};
use crate::{
    AccessibilityRole, AccessibilityState, AppContext, ComponentView, Entity, FrameworkError,
    InteractionState, MutationQueue, NodeKind, NodeStyle, StableNodeId, Stack, UiWorld,
};

/// The user picked a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateChanged {
    pub date: CivilDate,
}

/// The visible month changed, so the application can refresh the heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateCursorMoved {
    pub year: i32,
    pub month: u8,
}

/// Child nodes [`AppContext::assemble_date_picker`] owns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatePickerSlots {
    pub previous: Option<StableNodeId>,
    pub heading: Option<StableNodeId>,
    pub next: Option<StableNodeId>,
    pub header: Option<StableNodeId>,
    pub grid: Option<StableNodeId>,
    pub days: Vec<StableNodeId>,
}

/// Calendar month grid (`nana.date-picker`).
#[derive(Debug, Clone, PartialEq)]
pub struct DatePicker {
    /// Selected day, if any.
    pub value: Option<CivilDate>,
    /// Month on screen. Paging moves this without changing `value`.
    pub cursor: CivilDate,
    pub week_start: WeekStart,
    /// Inclusive selectable range. Days outside it render disabled.
    pub minimum: Option<CivilDate>,
    pub maximum: Option<CivilDate>,
    pub disabled: bool,
    pub size: ControlSize,
    /// Heading text. The application formats it, because month names are
    /// locale- and calendar-specific and the framework ships no locale data.
    pub month_label: Arc<str>,
    pub style: NodeStyle,
    pub(crate) slots: DatePickerSlots,
}

impl DatePicker {
    /// Grid showing the month `cursor` falls in.
    pub fn new(cursor: CivilDate) -> Self {
        Self {
            value: None,
            cursor,
            week_start: WeekStart::default(),
            minimum: None,
            maximum: None,
            disabled: false,
            size: ControlSize::Small,
            month_label: Arc::from(""),
            style: grid_style(),
            slots: DatePickerSlots::default(),
        }
    }

    /// Grid showing the selected day's month.
    pub fn selected(value: CivilDate) -> Self {
        Self {
            value: Some(value),
            ..Self::new(value)
        }
    }

    pub fn value(mut self, value: Option<CivilDate>) -> Self {
        self.value = value;
        self
    }

    pub fn week_start(mut self, week_start: WeekStart) -> Self {
        self.week_start = week_start;
        self
    }

    pub fn range(mut self, minimum: Option<CivilDate>, maximum: Option<CivilDate>) -> Self {
        self.minimum = minimum;
        self.maximum = maximum;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// Heading for the visible month, e.g. `2026 年 9 月`.
    pub fn month_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.month_label = label.into();
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn slots(&self) -> &DatePickerSlots {
        &self.slots
    }

    /// Whether `date` is inside the selectable range.
    pub fn selectable(&self, date: CivilDate) -> bool {
        !self.disabled
            && self.minimum.is_none_or(|minimum| date >= minimum)
            && self.maximum.is_none_or(|maximum| date <= maximum)
    }

    /// The month on screen.
    pub fn grid(&self) -> MonthGrid {
        MonthGrid::new(self.cursor.year(), self.cursor.month(), self.week_start)
            .expect("cursor is a valid date, so its month has a grid")
    }

    /// Pages the visible month, leaving the selection alone.
    pub fn shift_month(&mut self, months: i32) {
        self.cursor = self.cursor.shift_months(months);
    }
}

fn grid_style() -> NodeStyle {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.direction = Some(FlexDirection::Column);
    layout.gap = Some(LengthSpec::Px(space::XS));
    layout.width = Some(LengthSpec::Shrink);
    style
}

impl ComponentView for DatePicker {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        slot_assembler: Some(crate::AppContext::assemble_date_picker),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "date-picker".into(),
        }
    }

    fn reconcile(&mut self, mut next: Self) {
        // Child identities are runtime-owned; the visible month is the user's
        // paging unless the application moved the selection.
        next.slots = self.slots.clone();
        if self.value == next.value {
            next.cursor = self.cursor;
        }
        *self = next;
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &self.style,
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Table,
                label: Some(Arc::clone(&self.month_label)),
                disabled: self.disabled,
                ..AccessibilityState::default()
            },
        );
    }
}

impl AppContext {
    /// Builds (or refreshes) the header and day buttons of a [`DatePicker`].
    ///
    /// Idempotent: day buttons are reused across months, only their labels,
    /// state and handlers' target date change. Returns whether it created the
    /// children.
    pub fn assemble_date_picker(
        &mut self,
        picker: Entity<DatePicker>,
    ) -> Result<bool, FrameworkError> {
        let document = self
            .world()
            .node(picker.stable_id())
            .ok_or(FrameworkError::MissingView(picker.stable_id()))?
            .document;
        let snapshot = self.read(picker, Clone::clone)?;
        let grid = snapshot.grid();
        let created = snapshot.slots.days.is_empty();

        let mut slots = snapshot.slots.clone();
        if created {
            let header = self.create_detached_component(
                document,
                Stack::fill_row(space::XS).align(AlignSpec::Center),
            )?;
            let previous = self.create_detached_component(
                document,
                IconButton::new(Icon::ArrowLeft, "上一月")
                    .kind(ButtonKind::Text)
                    .size(snapshot.size),
            )?;
            let heading = self.create_detached_component(document, Text::new(""))?;
            let next = self.create_detached_component(
                document,
                IconButton::new(Icon::ArrowRight, "下一月")
                    .kind(ButtonKind::Text)
                    .size(snapshot.size),
            )?;
            self.append_child(header, previous)?;
            self.append_child(header, heading)?;
            self.append_child(header, next)?;
            self.append_child(picker, header)?;

            self.observe(previous, picker, |picker, _: &Activate, cx| {
                if picker.disabled {
                    return;
                }
                picker.shift_month(-1);
                cx.emit(DateCursorMoved {
                    year: picker.cursor.year(),
                    month: picker.cursor.month(),
                });
            })?;
            self.observe(next, picker, |picker, _: &Activate, cx| {
                if picker.disabled {
                    return;
                }
                picker.shift_month(1);
                cx.emit(DateCursorMoved {
                    year: picker.cursor.year(),
                    month: picker.cursor.month(),
                });
            })?;

            let body = self.create_detached_component(document, Stack::column(space::XXS))?;
            self.append_child(picker, body)?;
            let mut days = Vec::with_capacity(42);
            for week in &grid.weeks {
                let row = self.create_detached_component(document, Stack::row(space::XXS))?;
                self.append_child(body, row)?;
                for _ in week {
                    let day = self.create_detached_component(
                        document,
                        Button::new("").kind(ButtonKind::Text).size(snapshot.size),
                    )?;
                    self.append_child(row, day)?;
                    days.push(day.stable_id());
                }
            }
            slots = DatePickerSlots {
                previous: Some(previous.stable_id()),
                heading: Some(heading.stable_id()),
                next: Some(next.stable_id()),
                header: Some(header.stable_id()),
                grid: Some(body.stable_id()),
                days,
            };
            self.update_component(picker, |picker, _| picker.slots = slots.clone())?;
        }

        if let Some(heading) = slots.heading {
            let label = snapshot.month_label.to_string();
            self.update_component(Entity::<Text>::from_stable_id(heading), |text, _| {
                text.value = label
            })?;
        }

        // Refresh every cell for the month now on screen.
        for (cell, id) in grid.cells().zip(slots.days.iter().copied()) {
            let date = cell.date;
            let selected = snapshot.value == Some(date);
            let selectable = snapshot.selectable(date) && cell.in_month;
            let day = Entity::<Button>::from_stable_id(id);
            self.update_component(day, |button, _| {
                button.label = date.day().to_string();
                button.disabled = !selectable;
                button.kind = if selected {
                    ButtonKind::Selected
                } else if cell.in_month {
                    ButtonKind::Text
                } else {
                    ButtonKind::Ghost
                };
                let layout = Arc::make_mut(&mut button.style.layout);
                layout.width = Some(LengthSpec::Px(28.0));
                if !cell.in_month {
                    button.style.foreground = Some(SemanticColorRole::Faint);
                }
            })?;
            if created {
                self.observe(day, picker, move |picker, _: &Activate, cx| {
                    if !picker.selectable(date) {
                        return;
                    }
                    picker.value = Some(date);
                    picker.cursor = date;
                    cx.emit(DateChanged { date });
                })?;
            }
        }
        Ok(created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use std::sync::{Arc as StdArc, Mutex};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn date(year: i32, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap()
    }

    #[test]
    fn assembling_builds_six_weeks_of_days_and_reuses_them_across_months() {
        let mut cx = AppContext::new();
        let picker = cx
            .create_component(
                document(),
                DatePicker::selected(date(2026, 9, 9)).month_label("2026 年 9 月"),
            )
            .unwrap();

        assert!(cx.assemble_date_picker(picker).unwrap());
        let days = cx
            .read(picker, |picker| picker.slots().days.clone())
            .unwrap();
        assert_eq!(days.len(), 42, "always six whole weeks");

        // The selected day carries the selected chrome; a trailing day is
        // disabled because it is outside the month.
        let labels = |cx: &AppContext, days: &[StableNodeId]| {
            days.iter()
                .map(|id| {
                    cx.read(Entity::<Button>::from_stable_id(*id), |button| {
                        button.label.clone()
                    })
                    .unwrap()
                })
                .collect::<Vec<_>>()
        };
        let before = labels(&cx, &days);
        assert_eq!(
            before[1], "1",
            "2026-09-01 is the second cell of a Monday grid"
        );

        // Paging refreshes the same buttons rather than rebuilding the grid.
        cx.update_component(picker, |picker, _| picker.shift_month(1))
            .unwrap();
        assert!(!cx.assemble_date_picker(picker).unwrap());
        let after_ids = cx
            .read(picker, |picker| picker.slots().days.clone())
            .unwrap();
        assert_eq!(after_ids, days, "day buttons are reused");
        assert_ne!(labels(&cx, &days), before, "their labels follow the month");
    }

    #[test]
    fn only_selectable_days_report_a_change() {
        let mut cx = AppContext::new();
        let picker = cx
            .create_component(
                document(),
                DatePicker::new(date(2026, 9, 1))
                    .range(Some(date(2026, 9, 10)), Some(date(2026, 9, 20))),
            )
            .unwrap();
        cx.assemble_date_picker(picker).unwrap();
        let days = cx
            .read(picker, |picker| picker.slots().days.clone())
            .unwrap();

        let seen = StdArc::new(Mutex::new(Vec::new()));
        let out = StdArc::clone(&seen);
        cx.on(picker, move |_picker, event: &DateChanged, _| {
            out.lock().unwrap().push(event.date)
        })
        .unwrap();

        let grid = MonthGrid::new(2026, 9, WeekStart::Monday).unwrap();
        let cell_of = |day: u8| {
            grid.cells()
                .position(|cell| cell.in_month && cell.date.day() == day)
                .expect("day is in the grid")
        };

        // Below the minimum: activation is refused.
        cx.activate_node(days[cell_of(5)]).unwrap();
        assert!(seen.lock().unwrap().is_empty());

        // Inside the range: the picker commits it itself.
        cx.activate_node(days[cell_of(15)]).unwrap();
        assert_eq!(&*seen.lock().unwrap(), &[date(2026, 9, 15)]);
        assert_eq!(
            cx.read(picker, |picker| picker.value).unwrap(),
            Some(date(2026, 9, 15))
        );

        // Above the maximum: refused.
        cx.activate_node(days[cell_of(25)]).unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn paging_moves_the_visible_month_without_touching_the_selection() {
        let mut cx = AppContext::new();
        let picker = cx
            .create_component(document(), DatePicker::selected(date(2026, 9, 9)))
            .unwrap();
        cx.assemble_date_picker(picker).unwrap();
        let previous = cx
            .read(picker, |picker| picker.slots().previous)
            .unwrap()
            .unwrap();

        let moved = StdArc::new(Mutex::new(Vec::new()));
        let out = StdArc::clone(&moved);
        cx.on(picker, move |_picker, event: &DateCursorMoved, _| {
            out.lock().unwrap().push((event.year, event.month))
        })
        .unwrap();

        cx.activate_node(previous).unwrap();
        assert_eq!(&*moved.lock().unwrap(), &[(2026, 8)]);
        assert_eq!(
            cx.read(picker, |picker| picker.value).unwrap(),
            Some(date(2026, 9, 9)),
            "paging is not selecting"
        );
    }
}

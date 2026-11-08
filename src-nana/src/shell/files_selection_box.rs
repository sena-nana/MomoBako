//! 文件列表的框选矩形，对应 Vue `.files-list__selection-box` 和 `selectionBoxStyle`。
//!
//! 在列表空白处按下，任一轴拖过 3px（`didDrag`）以后画出圆角选框；松手选框消失，选择留下。
//! 选框盖在列表视口上，左上是指针范围相对列表视口的位置，并且不小于 0。指针事件穿透，卡片仍能被框住。
//! 拖动进行中只同步热字段，选框的位置和显隐走热信号，不重挂列表。

use nana_ui::runtime::view::{AnyView, FieldWrite, IntoView, Signal, StyledComponent, widget};
use nana_ui::runtime::{
    LayoutBox, LengthSpec, PositionSpec, RadiusTier, SemanticColorRole, SemanticPaint, Stack,
};
use nana_ui_core::{OverflowSpec, PointerEventsSpec, SemanticColorMix};

use super::ShellViewModel;
use super::input::LiveGesture;

/// 选框在列表视口里的位置。`visible` 为假时不画。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SelectionBox {
    pub visible: bool,
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for SelectionBox {
    fn default() -> Self {
        Self::hidden()
    }
}

impl SelectionBox {
    /// 没有在拖选框时的样子：不占位置。
    pub(crate) fn hidden() -> Self {
        Self {
            visible: false,
            left: 0.0,
            top: 0.0,
            width: 0.0,
            height: 0.0,
        }
    }

    /// 按 Vue `selectionBoxStyle` 把指针范围换成列表视口里的矩形。
    ///
    /// `origin_x` / `origin_y` 是列表视口的左上角。左上夹到不小于 0，宽高是指针两端的距离。
    /// 坐标不是有限数时返回 `None`，调用方记日志并不画。
    pub(crate) fn from_drag(
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        origin_x: f32,
        origin_y: f32,
    ) -> Option<Self> {
        if ![x0, y0, x1, y1, origin_x, origin_y]
            .iter()
            .all(|value| value.is_finite())
        {
            return None;
        }
        Some(Self {
            visible: true,
            left: (x0.min(x1) - origin_x).max(0.0),
            top: (y0.min(y1) - origin_y).max(0.0),
            width: (x1 - x0).abs(),
            height: (y1 - y0).abs(),
        })
    }
}

/// 按当前手势刷新选框。不是框选、还没拖过 3px，或没有列表区域时藏起来。
pub(crate) fn track(model: &mut ShellViewModel, list: Option<LayoutBox>) {
    let Some(gesture) = model.input.live_gesture.clone() else {
        model.input.selection_box = SelectionBox::hidden();
        return;
    };
    model.input.selection_box = placed(&gesture, list);
}

/// 松手或手势取消时收起选框。选择本身由框选归约留下。
pub(crate) fn clear(model: &mut ShellViewModel) {
    model.input.selection_box = SelectionBox::hidden();
}

/// 手势对应的选框。条目拖动和还没越过 3px 的按下都不画。
fn placed(gesture: &LiveGesture, list: Option<LayoutBox>) -> SelectionBox {
    if gesture.row.is_some() || !gesture.armed {
        return SelectionBox::hidden();
    }
    let Some(list) = list else {
        eprintln!("Nana 框选已经拖动，但没有列表区域，选框画不出来");
        return SelectionBox::hidden();
    };
    match SelectionBox::from_drag(
        gesture.origin_x,
        gesture.origin_y,
        gesture.x,
        gesture.y,
        list.x,
        list.y,
    ) {
        Some(rect) => rect,
        None => {
            eprintln!(
                "Nana 框选坐标无效，选框不画：({}, {}) -> ({}, {})，列表 ({}, {})",
                gesture.origin_x, gesture.origin_y, gesture.x, gesture.y, list.x, list.y
            );
            SelectionBox::hidden()
        }
    }
}

/// 把滚动列表包进定位框，选框叠在视口上。列表仍占满剩余高度。
pub(crate) fn overlay(list: AnyView, selection: Signal<SelectionBox>) -> AnyView {
    let frame = Stack::column(0.0)
        .width(LengthSpec::Fill)
        .min_width(LengthSpec::Px(0.0))
        .min_height(LengthSpec::Px(0.0))
        .grow(1.0)
        .with_layout(|layout| {
            layout.flex_shrink = Some(1.0);
            layout.flex_basis = Some(LengthSpec::Px(0.0));
            layout.position = PositionSpec::Relative;
            layout.overflow_x = OverflowSpec::Hidden;
            layout.overflow_y = OverflowSpec::Hidden;
        });
    widget(frame)
        .children((list, rectangle(selection)))
        .key("file-list-frame")
        .into_any()
}

/// `.files-list__selection-box`：1px 强调色 82% 混白的描边，18% 强调色的底，圆角 lg，在卡片之上。
fn rectangle(selection: Signal<SelectionBox>) -> AnyView {
    let initial = selection.get_untracked();
    let mut frame = Stack::column(0.0)
        .radius(RadiusTier::Lg)
        .with_layout(|layout| {
            layout.position = PositionSpec::Absolute;
            layout.z_index = Some(3);
            layout.pointer_events = Some(PointerEventsSpec::None);
            layout.border_width = Some(1.0);
            write_box(layout, initial);
        });
    let mut style = frame.node_style();
    let paint = box_paint();
    style.interaction.base = paint;
    style.interaction.hovered = paint;
    style.interaction.pressed = paint;
    style.interaction.selected = paint;
    style.interaction.selected_hovered = paint;
    style.interaction.selected_pressed = paint;
    frame = frame.style(style);
    widget(frame)
        .prop::<SelectionBox, SelectionBoxField>(selection)
        .key("file-selection-box")
        .into_any()
}

/// 描边是强调色 82% 混白（`OnMedia` 在两个主题里都是白），底是强调色 18% 透明。
fn box_paint() -> SemanticPaint {
    SemanticPaint {
        background_mix: Some(SemanticColorMix::alpha(SemanticColorRole::Accent, 0.18)),
        border_mix: Some(SemanticColorMix::new(
            SemanticColorRole::Accent,
            SemanticColorRole::OnMedia,
            0.82,
        )),
        ..SemanticPaint::default()
    }
}

/// 把选框写进布局：藏起来时缩成 0，避免透明块留在列表上。
fn write_box(layout: &mut nana_ui_core::LayoutStyle, box_rect: SelectionBox) {
    let shown = box_rect.visible;
    layout.offset_left = Some(LengthSpec::Px(if shown { box_rect.left } else { 0.0 }));
    layout.offset_top = Some(LengthSpec::Px(if shown { box_rect.top } else { 0.0 }));
    layout.width = Some(LengthSpec::Px(if shown { box_rect.width } else { 0.0 }));
    layout.height = Some(LengthSpec::Px(if shown { box_rect.height } else { 0.0 }));
    layout.opacity = Some(if shown { 1.0 } else { 0.0 });
}

/// 选框的位置和显隐。拖动中每帧写这一处，列表不重挂。
pub(crate) struct SelectionBoxField;

impl FieldWrite<Stack, SelectionBox> for SelectionBoxField {
    const FIELD: &'static str = "Stack.style.layout.selection-box";

    fn write(target: &mut Stack, value: SelectionBox) {
        write_box(
            std::sync::Arc::make_mut(&mut target.node_style_mut().layout),
            value,
        );
    }

    fn differs(target: &Stack, value: &SelectionBox) -> bool {
        let layout = &target.node_style().layout;
        let shown = value.visible;
        layout.offset_left != Some(LengthSpec::Px(if shown { value.left } else { 0.0 }))
            || layout.offset_top != Some(LengthSpec::Px(if shown { value.top } else { 0.0 }))
            || layout.width != Some(LengthSpec::Px(if shown { value.width } else { 0.0 }))
            || layout.height != Some(LengthSpec::Px(if shown { value.height } else { 0.0 }))
            || layout.opacity != Some(if shown { 1.0 } else { 0.0 })
    }
}

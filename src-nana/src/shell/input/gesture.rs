//! 文件列表上还没松手的拖动：条目拖动和框选。
//!
//! 卡片和列表空白处挂 NanaUI 的 `PointerDragged`，越过框架的拖动容差以后每个采样发一条
//! [`DragSample`]。归约只把采样记进 [`LiveGesture`]：按下点、位移、是不是已经松手。
//! 窗口坐标、指针底下的文件夹和框住的条目要读文档布局，由准备帧（`pointer_gesture.rs`）结算。

use nana_ui::runtime::{PointerDragPhase, StableNodeId};

/// 一次 `PointerDragged` 采样，原样带进归约。
#[derive(Clone, Debug, PartialEq)]
pub struct DragSample {
    /// 按在哪张卡片上；`None` 是按在列表空白处，拖动就是框选。
    pub row: Option<String>,
    /// 收到这次拖动的节点：卡片或列表框。`x` / `y` 是它自己的布局坐标。
    pub anchor: Option<StableNodeId>,
    pub phase: PointerDragPhase,
    pub x: f32,
    pub y: f32,
    /// 从按下点算起的位移，按窗口逻辑像素。
    pub dx: f32,
    pub dy: f32,
    /// 按着 Ctrl 或 Meta：框选追加到已有选择。
    pub append: bool,
}

/// 实况树上还没松手的拖动。`row` 为空时是框选。
#[derive(Clone, Debug)]
pub struct LiveGesture {
    pub row: Option<String>,
    /// 收到拖动的节点和它自己坐标里的按下点，准备帧用它换出窗口里的按下点。
    pub(crate) anchor: Option<StableNodeId>,
    pub(crate) press: (f32, f32),
    /// 窗口里的按下点。准备帧第一次结算时写上。
    pub(crate) origin: Option<(f32, f32)>,
    pub(crate) dx: f32,
    pub(crate) dy: f32,
    /// 条目拖动已经越过起步阈值、会话已经开始。框选从框架判定拖动起就算。
    pub armed: bool,
    /// 按下时 Ctrl 或 Meta 是否按着。
    pub append: bool,
    /// 已经松手（`Some(false)`）或被取消（`Some(true)`），等准备帧收尾。
    pub(crate) finished: Option<bool>,
}

impl LiveGesture {
    /// 窗口里的当前指针位置：按下点加位移。按下点还没结算时没有。
    pub(crate) fn point(&self) -> Option<(f32, f32)> {
        self.origin.map(|(x, y)| (x + self.dx, y + self.dy))
    }

    /// 从按下点算起走了多远。
    pub(crate) fn travel(&self) -> f32 {
        f64::from(self.dx).hypot(f64::from(self.dy)) as f32
    }
}

/// 列表空白处的一次按下，用来认出「点空白」：框架只把越过容差的按下报成拖动，没越过的点按不发事件。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlankPress {
    pub x: f32,
    pub y: f32,
    /// 按下以后离按下点最远的距离。
    pub travel: f32,
    /// 按下时 Ctrl 或 Meta 是否按着。
    pub append: bool,
}

/// 把一次拖动采样记进手势。`Started` 开新手势；之后的采样只更新位移和收尾标记。
/// 没有手势时收到的后续采样（手势已经被收尾、或开头那条丢了）记日志后丢掉。
pub(crate) fn record(gesture: &mut Option<LiveGesture>, sample: DragSample) {
    if sample.phase == PointerDragPhase::Started {
        *gesture = Some(LiveGesture {
            row: sample.row,
            anchor: sample.anchor,
            press: (sample.x, sample.y),
            origin: None,
            dx: 0.0,
            dy: 0.0,
            armed: false,
            append: sample.append,
            finished: None,
        });
        return;
    }
    let Some(live) = gesture.as_mut() else {
        if sample.phase == PointerDragPhase::Moved {
            eprintln!("Nana 拖动采样没有对应的手势，丢掉：{:?}", sample.row);
        }
        return;
    };
    if live.row != sample.row || live.finished.is_some() {
        eprintln!(
            "Nana 拖动采样和当前手势对不上，丢掉：{:?} / {:?}",
            sample.row, live.row
        );
        return;
    }
    if sample.dx.is_finite() && sample.dy.is_finite() {
        live.dx = sample.dx;
        live.dy = sample.dy;
    } else {
        eprintln!("Nana 拖动位移无效：{}, {}", sample.dx, sample.dy);
    }
    match sample.phase {
        PointerDragPhase::Ended => live.finished = Some(false),
        PointerDragPhase::Cancelled => live.finished = Some(true),
        PointerDragPhase::Started | PointerDragPhase::Moved => {}
    }
}

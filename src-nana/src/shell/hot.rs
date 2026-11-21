//! 每帧会变的显示值：进度条填充、底部入口的悬停、播放进度和框选矩形，以及标题栏随状态变的字段。
//!
//! 投影是从 ViewModel 算出的纯值（实现 `PartialEq`），`ShellView` 同步时只在值变了才写进信号。
//! 槽位里的现有视图函数挂载期间经 [`prop`] 拿到这些信号，绑到节点自己的字段上：填充过渡、悬停和播放推进
//! 只改这几个字段，不重挂内容，焦点、选区、悬停和输入法预编辑都留在原节点上。
//! 不经 `ShellView` 建树时 [`prop`] 按投影值写成常量，画面和绑定时一样。
//!
//! 绑定只放在槽位根节点以下：组合控件给槽位根节点打的布局补丁只在它自己投影时写，根节点要是带
//! 绑定，逐帧重投影会把补丁冲掉。

use std::cell::Cell;

use nana_ui::runtime::view::{PropSource, Signal, signal};

use super::ShellViewModel;

/// 这一帧热信号的值。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HotFrame {
    /// 启动进度条这一刻的百分比（`progress_fill.rs`）。
    pub startup_percent: f32,
    /// 任务弹层各行进度条这一刻的百分比。
    pub task_fills: TaskFills,
    /// 指针停在侧栏底部入口那一排上。
    pub footer_hover: bool,
    /// 播放条拖动条的位置，单位毫秒。
    pub position: f64,
    /// 文件列表框选矩形。
    pub selection_box: super::files_selection_box::SelectionBox,
}

impl HotFrame {
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let session = &model.player.session;
        Self {
            startup_percent: model.fills.startup(),
            task_fills: TaskFills(model.fills.rows()),
            footer_hover: model.input.footer_hover,
            position: super::player_view::bar::position_ms(
                session.current_time_ms,
                session.duration_ms.unwrap_or(0),
            ),
            selection_box: model.input.selection_box,
        }
    }
}

/// 任务弹层各行进度条这一刻的填充百分比，按行键（宽度过渡见
/// `ProgressFills::set_rows`）。弹层关着时为空。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TaskFills(Vec<(String, f32)>);

impl TaskFills {
    /// 某一行这一刻的填充百分比；这一行没有填充时为 `None`。
    pub(crate) fn get(&self, key: &str) -> Option<f32> {
        self.0
            .iter()
            .find(|(row, _)| row == key)
            .map(|(_, fill)| *fill)
    }
}

/// 标题栏随状态变的字段。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TitleBarView {
    pub collapsed: bool,
    pub filter_open: bool,
    pub filter_count: usize,
    pub query: String,
}

impl TitleBarView {
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        Self {
            collapsed: model.workspace.sidebar_collapsed,
            filter_open: model.inspect.filter_bar_open,
            filter_count: model.inspect.active_filter_count(),
            query: model.inspect.query.clone(),
        }
    }
}

/// 热投影的信号。句柄是 `Copy` 的 id，值在骨架的挂载作用域里，骨架卸载时一起回收。
#[derive(Clone, Copy)]
pub(crate) struct HotSignals {
    pub startup: Signal<f32>,
    pub task_fills: Signal<TaskFills>,
    pub footer_hover: Signal<bool>,
    pub position: Signal<f64>,
    /// 文件列表框选矩形。拖动中每帧更新，列表不重挂。
    pub selection_box: Signal<super::files_selection_box::SelectionBox>,
}

impl HotSignals {
    /// 在当前作用域里建信号，初值是这一刻的投影。只在骨架的挂载闭包里调用，否则信号会留到线程结束。
    pub(crate) fn new(frame: HotFrame) -> Self {
        Self {
            startup: signal(frame.startup_percent),
            task_fills: signal(frame.task_fills),
            footer_hover: signal(frame.footer_hover),
            position: signal(frame.position),
            selection_box: signal(frame.selection_box),
        }
    }

    /// 骨架还在：信号所在的作用域没有被回收。
    pub(crate) fn alive(&self) -> bool {
        self.startup.defined_at().is_some()
    }

    /// 写入这一帧的投影，只写变了的。信号可能已经随文档回收，回收了就不写。
    pub(crate) fn write(&self, frame: HotFrame) {
        self.startup.try_set_if_changed(frame.startup_percent);
        self.task_fills.try_set_if_changed(frame.task_fills);
        self.footer_hover.try_set_if_changed(frame.footer_hover);
        self.position.try_set_if_changed(frame.position);
        self.selection_box.try_set_if_changed(frame.selection_box);
    }
}

/// 筛选开关的样子：打开时的说明文字，以及打开或有条件时的强调底色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FilterLook {
    pub open: bool,
    pub active: bool,
}

/// 标题栏字段的信号。搜索框的草稿不在这里，见 [`ModelField`]。
#[derive(Clone, Copy)]
pub(crate) struct TitleSignals {
    pub collapsed: Signal<bool>,
    pub filter: Signal<FilterLook>,
    pub count: Signal<usize>,
}

impl TitleSignals {
    /// 在骨架的挂载闭包里建信号。
    pub(crate) fn new(view: &TitleBarView) -> Self {
        Self {
            collapsed: signal(view.collapsed),
            filter: signal(filter_look(view)),
            count: signal(view.filter_count),
        }
    }

    /// 写入标题栏投影，只写变了的。
    pub(crate) fn write(&self, view: &TitleBarView) {
        self.collapsed.try_set_if_changed(view.collapsed);
        self.filter.try_set_if_changed(filter_look(view));
        self.count.try_set_if_changed(view.filter_count);
    }
}

fn filter_look(view: &TitleBarView) -> FilterLook {
    FilterLook {
        open: view.filter_open,
        active: view.filter_open || view.filter_count > 0,
    }
}

/// 受控输入框的草稿：信号驱动输入框，`projected` 记着上次从 ViewModel 写进去的值。
///
/// ViewModel 的值没变时不回写：用户刚打的字已经由 `.model` 写进了信号，对应的消息可能还排在
/// 后台消息后面没归约。这时把 ViewModel 里较旧的草稿写回信号，下一次刷新就会把刚打的字回滚。
///
/// `projected` 也放在信号里（只在同步时不追踪地读写，不驱动绑定），整个结构是 `Copy` 的句柄，
/// 能放进各块常驻的信号结构，也能在列表行里建。
#[derive(Clone, Copy)]
pub(crate) struct ModelField {
    signal: Signal<String>,
    projected: Signal<String>,
}

impl ModelField {
    /// 在当前作用域里建信号，初值是 ViewModel 当前的值。
    pub(crate) fn new(value: &str) -> Self {
        Self {
            signal: signal(value.to_owned()),
            projected: signal(value.to_owned()),
        }
    }

    pub(crate) fn signal(&self) -> Signal<String> {
        self.signal
    }

    /// ViewModel 的值相对上次投影变了才写进信号。返回是否写了；信号已随作用域回收时不写。
    pub(crate) fn sync(&self, value: &str) -> bool {
        if self.projected.defined_at().is_none()
            || self
                .projected
                .with_untracked(|projected| projected == value)
        {
            return false;
        }
        self.projected
            .update(|projected| value.clone_into(projected));
        self.signal.try_set_if_changed(value.to_owned())
    }
}

thread_local! {
    /// 正在挂载的宿主可用的热信号。只在 [`with_signals`] 期间有值。
    static CURRENT: Cell<Option<HotSignals>> = const { Cell::new(None) };
}

/// 在 `build` 期间把热信号交给旧视图函数；结束（包括 panic 展开）时恢复外层的值。
pub(crate) fn with_signals<R>(signals: HotSignals, build: impl FnOnce() -> R) -> R {
    struct Restore(Option<HotSignals>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(CURRENT.with(|cell| cell.replace(Some(signals))));
    build()
}

/// 挂载期间有热信号时取出其中一个信号的句柄，交给之后才建的行（`each` 的新行）去绑；不经 `ShellView` 建树时
/// 为 `None`，调用方按投影值写常量。
pub(crate) fn current<T>(pick: impl FnOnce(&HotSignals) -> Signal<T>) -> Option<Signal<T>> {
    CURRENT.with(Cell::get).map(|signals| pick(&signals))
}

/// 热字段的属性：挂载期间有热信号就直接绑定信号，否则写成 `fallback` 常量。
///
/// 只在挂载闭包里同步建的视图拿得到信号；`each_virtual` 的行这类之后才建的视图拿到的是常量。
pub(crate) fn prop<T: Clone + 'static>(
    pick: impl FnOnce(&HotSignals) -> Signal<T>,
    fallback: T,
) -> PropSource<T> {
    match CURRENT.with(Cell::get) {
        Some(signals) => PropSource::Signal(pick(&signals)),
        None => PropSource::Const(fallback),
    }
}

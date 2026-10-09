//! 每帧会变的显示值：动效时钟、播放进度，以及标题栏随状态变的字段。
//!
//! 投影是从 ViewModel 算出的纯值（实现 `PartialEq`），`ShellView` 同步时只在值变了才写进信号。
//! 槽位里的现有视图函数挂载期间经 [`prop`] 拿到这些信号，绑到节点自己的字段上：动效帧和播放推进
//! 只改这几个字段，不重挂内容，焦点、选区、悬停和输入法预编辑都留在原节点上。
//! 不经 `ShellView` 建树时 [`prop`] 按投影值写成常量，画面和绑定时一样。
//!
//! 绑定只放在槽位根节点以下：组合控件给槽位根节点打的布局补丁只在它自己投影时写，根节点要是带
//! 绑定，逐帧重投影会把补丁冲掉。

use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;

use nana_ui::runtime::view::{signal, FieldWrite, PropSource, Signal, StyledComponent};
use nana_ui::runtime::{IconButton, IconGlyph, LengthSpec, Stack, Workspace};
use nana_ui::{RegionId, WorkspaceMutation};
use nana_ui_core::PaintTransform;

use super::motion::{shift_scale, spin_transform, MotionState};
use super::ShellViewModel;

/// 弹层外框的透明度和变换：淡入上移。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayerPaint {
    pub opacity: f32,
    pub transform: PaintTransform,
}

impl LayerPaint {
    /// 弹层：透明度和 4px 上移。
    pub(crate) fn panel(motion: &MotionState) -> Self {
        let frame = motion.panel_frame();
        Self { opacity: frame.opacity, transform: shift_scale(frame.shift, 1.0) }
    }
}

/// 动效时钟这一刻在界面上的样子。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MotionFrame {
    pub panel: LayerPaint,
    /// 启动进度条填充的百分比。
    pub startup_percent: f32,
    /// 忙指示旋转角，单位度。
    pub spinner_degrees: f32,
    /// 侧栏底部入口平时的透明度。
    pub footer_opacity: f32,
    /// 侧栏呈现宽度，折叠动画中逐帧变化。`ShellView` 直接写进工作区，不经信号，见 [`set_resources_width`]。
    pub sidebar_width: f32,
}

impl MotionFrame {
    /// 按 ViewModel 的动效时钟取这一帧。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let motion = &model.motion;
        Self {
            panel: LayerPaint::panel(motion),
            startup_percent: motion.startup_percent(),
            spinner_degrees: motion.spinner_degrees(),
            footer_opacity: motion.footer_opacity(),
            sidebar_width: motion.sidebar_presented_width(),
        }
    }
}

/// 播放条随播放时钟变的部分。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaybackView {
    /// 进度轨填充比例。
    pub ratio: f32,
    /// 「当前 / 总长」。
    pub time: String,
    /// 拖动条的位置，单位毫秒。
    pub position: f64,
}

impl PlaybackView {
    /// 按播放会话的当前时间和时长取值，算法和播放条挂载时相同。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let session = &model.player.session;
        let (ratio, time, position) = super::player_view::bar::clock_parts(session.current_time_ms, session.duration_ms.unwrap_or(0));
        Self { ratio, time, position }
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
    pub panel: Signal<LayerPaint>,
    pub startup: Signal<f32>,
    pub spinner: Signal<f32>,
    pub footer: Signal<f32>,
    pub progress: Signal<f32>,
    pub time: Signal<String>,
    pub position: Signal<f64>,
}

impl HotSignals {
    /// 在当前作用域里建信号，初值是这一刻的投影。只在骨架的挂载闭包里调用，否则信号会留到线程结束。
    pub(crate) fn new(motion: MotionFrame, playback: &PlaybackView) -> Self {
        Self {
            panel: signal(motion.panel),
            startup: signal(motion.startup_percent),
            spinner: signal(motion.spinner_degrees),
            footer: signal(motion.footer_opacity),
            progress: signal(playback.ratio),
            time: signal(playback.time.clone()),
            position: signal(playback.position),
        }
    }

    /// 骨架还在：信号所在的作用域没有被回收。
    pub(crate) fn alive(&self) -> bool {
        self.panel.defined_at().is_some()
    }

    /// 写入这一帧的投影，只写变了的。信号可能已经随文档回收，回收了就不写。
    pub(crate) fn write(&self, motion: MotionFrame, playback: &PlaybackView) {
        self.panel.try_set_if_changed(motion.panel);
        self.startup.try_set_if_changed(motion.startup_percent);
        self.spinner.try_set_if_changed(motion.spinner_degrees);
        self.footer.try_set_if_changed(motion.footer_opacity);
        self.progress.try_set_if_changed(playback.ratio);
        self.time.try_set_if_changed(playback.time.clone());
        self.position.try_set_if_changed(playback.position);
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
    FilterLook { open: view.filter_open, active: view.filter_open || view.filter_count > 0 }
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
        Self { signal: signal(value.to_owned()), projected: signal(value.to_owned()) }
    }

    pub(crate) fn signal(&self) -> Signal<String> {
        self.signal
    }

    /// ViewModel 的值相对上次投影变了才写进信号。返回是否写了；信号已随作用域回收时不写。
    pub(crate) fn sync(&self, value: &str) -> bool {
        if self.projected.defined_at().is_none() || self.projected.with_untracked(|projected| projected == value) {
            return false;
        }
        self.projected.update(|projected| value.clone_into(projected));
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

/// 热字段的属性：挂载期间有热信号就直接绑定信号，否则写成 `fallback` 常量。
///
/// 只在挂载闭包里同步建的视图拿得到信号；`each_virtual` 的行这类之后才建的视图拿到的是常量。
pub(crate) fn prop<T: Clone + 'static>(pick: impl FnOnce(&HotSignals) -> Signal<T>, fallback: T) -> PropSource<T> {
    match CURRENT.with(Cell::get) {
        Some(signals) => PropSource::Signal(pick(&signals)),
        None => PropSource::Const(fallback),
    }
}

/// `Stack` 的透明度和变换，弹层外框用。
pub(crate) struct LayerPaintField;

impl FieldWrite<Stack, LayerPaint> for LayerPaintField {
    const FIELD: &'static str = "Stack.style.layout.opacity+transform";

    fn write(target: &mut Stack, value: LayerPaint) {
        let layout = Arc::make_mut(&mut <Stack as StyledComponent>::node_style_mut(target).layout);
        layout.opacity = Some(value.opacity);
        layout.transform = Some(value.transform);
    }

    fn differs(target: &Stack, value: &LayerPaint) -> bool {
        let layout = &<Stack as StyledComponent>::node_style(target).layout;
        layout.opacity != Some(value.opacity) || layout.transform != Some(value.transform)
    }
}

/// `Stack` 的宽度百分比，启动进度条的填充用。超出 0..100 的值按两端算。
pub(crate) struct FillPercentField;

impl FieldWrite<Stack, f32> for FillPercentField {
    const FIELD: &'static str = "Stack.style.layout.width%";

    fn write(target: &mut Stack, percent: f32) {
        Arc::make_mut(&mut <Stack as StyledComponent>::node_style_mut(target).layout).width = Some(fill_percent(percent));
    }

    fn differs(target: &Stack, percent: &f32) -> bool {
        <Stack as StyledComponent>::node_style(target).layout.width != Some(fill_percent(*percent))
    }
}

fn fill_percent(percent: f32) -> LengthSpec {
    LengthSpec::Percent(percent.clamp(0.0, 100.0))
}

/// 图标的旋转角，忙指示用。
pub(crate) struct SpinField;

impl FieldWrite<IconGlyph, f32> for SpinField {
    const FIELD: &'static str = "IconGlyph.style.layout.transform";

    fn write(target: &mut IconGlyph, degrees: f32) {
        Arc::make_mut(&mut target.style.layout).transform = Some(spin_transform(degrees));
    }

    fn differs(target: &IconGlyph, degrees: &f32) -> bool {
        target.style.layout.transform != Some(spin_transform(*degrees))
    }
}

/// 图标按钮平时的透明度，侧栏底部入口用。
pub(crate) struct OpacityField;

impl FieldWrite<IconButton, f32> for OpacityField {
    const FIELD: &'static str = "IconButton.style.layout.opacity";

    fn write(target: &mut IconButton, opacity: f32) {
        Arc::make_mut(&mut target.style.layout).opacity = Some(opacity);
    }

    fn differs(target: &IconButton, opacity: &f32) -> bool {
        target.style.layout.opacity != Some(*opacity)
    }
}

/// 进度轨自绘的填充比例，播放条用。
pub(crate) struct TrackRatioField;

impl FieldWrite<Stack, f32> for TrackRatioField {
    const FIELD: &'static str = "Stack.style.painter(ProgressTrack)";

    fn write(target: &mut Stack, ratio: f32) {
        <Stack as StyledComponent>::node_style_mut(target).painter = Some(track(ratio));
    }

    fn differs(target: &Stack, ratio: &f32) -> bool {
        <Stack as StyledComponent>::node_style(target).painter != Some(track(*ratio))
    }
}

fn track(ratio: f32) -> nana_ui::runtime::NodePainter {
    super::player_view::paint::ProgressTrack { ratio }.into()
}

/// 按侧栏呈现宽度设工作区资源区尺寸，返回尺寸有没有变。侧栏宽度少一条发丝间隙，和挂载时的区域
/// 尺寸同一个口径，按区域上下限夹取；写的是区域尺寸，拖动中的指针状态不受影响。
///
/// 不做成绑定：工作区是 AppShell 的 body，它自己重投影会冲掉 AppShell 给 body 打的布局补丁，
/// 调用方写完要让 AppShell 重新投影一次。
pub(crate) fn set_resources_width(target: &mut Workspace, width: f32) -> bool {
    let Some(region) = target.layout.region(&RegionId::Resources) else {
        eprintln!("Nana 工作区没有资源区，侧栏宽度写不进去");
        return false;
    };
    let wanted = resources_size(width).clamp(region.min_size_value(), region.max_size_value());
    if region.size_value() == Some(wanted) {
        return false;
    }
    target.apply(WorkspaceMutation::SetRegionSize(RegionId::Resources, wanted), Duration::ZERO)
}

/// 侧栏呈现宽度换成资源区尺寸。
pub(crate) fn resources_size(width: f32) -> f32 {
    (width - super::render::WORKBENCH_GAP_PX).max(0.0)
}

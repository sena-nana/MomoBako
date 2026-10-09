//! Vue 样式表里真正绘制的过渡。
//!
//! 时钟留在 `ShellViewModel` 上。下一次归约会拆掉控件树，但进行中的轨道还在，
//! 下一帧按已经走过的时间取样，不会从 0 重新开始。减少动效时全部立刻停在终点。

use nana_ui::runtime::view::{widget, AnyView, IntoView};

/// 对话框遮罩。`shell.css` `.modal-enter-active` 的 `opacity 0.16s ease`。
pub const MODAL_OVERLAY_MS: u64 = 160;
/// 对话框卡片。`transform 0.18s cubic-bezier(0.2, 0.8, 0.2, 1)`。
pub const MODAL_CARD_MS: u64 = 180;
/// 面板透明度。`.panel-enter-active` 的 `opacity 0.14s ease`。
pub const PANEL_OPACITY_MS: u64 = 140;
/// 面板上移。同一条规则里的 `transform 0.16s ease`，距离 4px。
pub const PANEL_RISE_MS: u64 = 160;
/// 启动条和操作条宽度。`transition: width 0.18s ease`。
pub const PROGRESS_WIDTH_MS: u64 = 180;
/// 不确定进度呼吸。`progress-pulse 1.15s ease-in-out`。
pub const PULSE_MS: u64 = 1150;
/// 媒体下载扫光。`media-preview-progress-sweep 1.05s ease-in-out`。
pub const SWEEP_MS: u64 = 1050;
/// 忙指示旋转。`.spin` 的 `spin 0.8s linear`。
pub const SPINNER_MS: u64 = 800;
/// 侧栏分组工具。`.sb-section__tools` 的 `opacity 0.12s ease`。
pub const SIDEBAR_TOOL_FADE_MS: u64 = 120;
/// 底栏按钮。`.workspace-footer__btn` 的 `opacity 0.35s ease`。
pub const FOOTER_FADE_MS: u64 = 350;
/// 侧栏折叠。Lilia 工作区 `grid-template-columns 0.24s cubic-bezier(0.2, 0.8, 0.2, 1)`。
pub const SIDEBAR_COLLAPSE_MS: u64 = 240;
/// 文件夹拖放悬停后展开并打开的等待。
pub const FOLDER_HOVER_MS: u64 = 450;
/// 虚拟列表缩略图空闲预取。
pub const PREFETCH_IDLE_MS: u64 = 420;

const MODAL_CARD_SHIFT: f32 = -8.0;
const MODAL_CARD_SCALE: f32 = 0.98;
const PANEL_SHIFT: f32 = -4.0;
const FOOTER_REST: f32 = 0.44;
const PULSE_LOW: f32 = 0.42;

/// 一条还没走完的过渡。`from`/`to` 是该轨道自己的标量。
#[derive(Clone, Debug)]
struct Track {
    started_ms: u64,
    from: f32,
    to: f32,
    duration_ms: u64,
    easing: EasingKind,
    looping: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EasingKind {
    /// CSS `ease`：`cubic-bezier(0.25, 0.1, 0.25, 1)`。
    Ease,
    /// CSS `ease-in-out`。
    EaseInOut,
    /// `cubic-bezier(0.2, 0.8, 0.2, 1)`，对话框卡片和侧栏宽度用它。
    Emphasis,
    Linear,
}

/// 壳层动效时钟。
#[derive(Clone, Debug)]
pub struct MotionState {
    now_ms: u64,
    reduced: bool,
    modal: Option<Track>,
    panel: Option<Track>,
    startup: Option<Track>,
    operation: Option<Track>,
    pulse: Option<Track>,
    sweep: Option<Track>,
    spinner: Option<Track>,
    tools: Option<Track>,
    footer: Option<Track>,
    sidebar: Option<Track>,
    modal_wants_open: bool,
    panel_wants_open: bool,
    tools_hover: bool,
    footer_hover: bool,
    spinner_on: bool,
    pulse_on: bool,
    sweep_on: bool,
    sidebar_collapsed: bool,
    sidebar_expanded_width: f32,
}

impl Default for MotionState {
    fn default() -> Self {
        Self {
            now_ms: 0,
            reduced: false,
            modal: None,
            panel: None,
            startup: None,
            operation: None,
            pulse: None,
            sweep: None,
            spinner: None,
            tools: None,
            footer: None,
            sidebar: None,
            modal_wants_open: false,
            panel_wants_open: false,
            tools_hover: false,
            footer_hover: false,
            spinner_on: false,
            pulse_on: false,
            sweep_on: false,
            sidebar_collapsed: false,
            sidebar_expanded_width: crate::theme_map::SIDEBAR_DEFAULT_PX,
        }
    }
}

/// 对话框当前画面。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModalFrame {
    pub overlay_opacity: f32,
    pub card_shift: f32,
    pub card_scale: f32,
    pub visible: bool,
}

/// 面板当前画面。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelFrame {
    pub opacity: f32,
    pub shift: f32,
    pub visible: bool,
}

impl MotionState {
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }

    pub fn reduced(&self) -> bool {
        self.reduced
    }

    /// 把时钟拨到绝对毫秒。进行中的轨道保持原来的起点。
    pub fn advance(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        self.drop_finished();
    }

    pub fn set_reduced(&mut self, reduced: bool) {
        self.reduced = reduced;
        if reduced {
            self.snap_all();
        }
    }

    /// 还有轨道需要下一帧。
    pub fn active(&self) -> bool {
        if self.reduced {
            return false;
        }
        [&self.modal, &self.panel, &self.startup, &self.operation, &self.pulse, &self.sweep, &self.spinner, &self.tools, &self.footer, &self.sidebar]
            .into_iter()
            .any(|track| track.as_ref().is_some_and(|track| !track.finished(self.now_ms)))
    }

    /// 开关有变化时返回 true，调用方据此把界面标成待重建。
    pub fn set_modal_open(&mut self, open: bool) -> bool {
        if open == self.modal_wants_open && self.modal.is_some() {
            return false;
        }
        let changed = open != self.modal_wants_open;
        self.modal_wants_open = open;
        self.modal = Some(if self.reduced {
            settled_modal(open)
        } else if open {
            track(self.now_ms, 0.0, 1.0, MODAL_CARD_MS, EasingKind::Emphasis, false)
        } else {
            track(self.now_ms, 1.0, 0.0, MODAL_CARD_MS, EasingKind::Emphasis, false)
        });
        changed
    }

    /// 开关有变化时返回 true，调用方据此把界面标成待重建。
    pub fn set_panel_open(&mut self, open: bool) -> bool {
        if open == self.panel_wants_open && self.panel.is_some() {
            return false;
        }
        let changed = open != self.panel_wants_open;
        self.panel_wants_open = open;
        self.panel = Some(if self.reduced {
            settled_scalar(open)
        } else if open {
            track(self.now_ms, 0.0, 1.0, PANEL_RISE_MS, EasingKind::Ease, false)
        } else {
            track(self.now_ms, 1.0, 0.0, PANEL_RISE_MS, EasingKind::Ease, false)
        });
        changed
    }

    pub fn set_startup_percent(&mut self, percent: f32) {
        let percent = percent.clamp(0.0, 100.0);
        let from = self.startup.as_ref().map(|track| track.value(self.now_ms)).unwrap_or(0.0);
        if (from - percent).abs() < 0.01 && self.startup.as_ref().is_some_and(|track| (track.to - percent).abs() < 0.01) {
            return;
        }
        self.startup = Some(if self.reduced {
            track(self.now_ms, percent, percent, 0, EasingKind::Linear, false)
        } else {
            track(self.now_ms, from, percent, PROGRESS_WIDTH_MS, EasingKind::Ease, false)
        });
    }

    pub fn set_operation_percent(&mut self, percent: Option<f32>) {
        let Some(percent) = percent else {
            self.operation = None;
            return;
        };
        let percent = percent.clamp(0.0, 100.0);
        let from = self.operation.as_ref().map(|track| track.value(self.now_ms)).unwrap_or(0.0);
        if self.operation.as_ref().is_some_and(|track| (track.to - percent).abs() < 0.01) {
            return;
        }
        self.operation = Some(if self.reduced {
            track(self.now_ms, percent, percent, 0, EasingKind::Linear, false)
        } else {
            track(self.now_ms, from, percent, PROGRESS_WIDTH_MS, EasingKind::Ease, false)
        });
    }

    pub fn set_spinner(&mut self, on: bool) {
        if on == self.spinner_on && (on || self.spinner.is_none()) {
            return;
        }
        self.spinner_on = on;
        self.spinner = on.then(|| track(self.now_ms, 0.0, 1.0, SPINNER_MS, EasingKind::Linear, !self.reduced));
    }

    pub fn set_pulse(&mut self, on: bool) {
        self.pulse_on = on;
        self.pulse = on.then(|| track(self.now_ms, 0.0, 1.0, PULSE_MS, EasingKind::EaseInOut, !self.reduced));
    }

    pub fn set_sweep(&mut self, on: bool) {
        self.sweep_on = on;
        self.sweep = on.then(|| track(self.now_ms, 0.0, 1.0, SWEEP_MS, EasingKind::EaseInOut, !self.reduced));
    }

    pub fn set_tools_hover(&mut self, hover: bool) {
        if hover == self.tools_hover && (self.tools.is_some() || !hover) {
            return;
        }
        let from = self.tools.as_ref().map(|track| track.value(self.now_ms)).unwrap_or(0.0);
        self.tools_hover = hover;
        let to = if hover { 1.0 } else { 0.0 };
        self.tools = Some(if self.reduced {
            track(self.now_ms, to, to, 0, EasingKind::Linear, false)
        } else {
            track(self.now_ms, from, to, SIDEBAR_TOOL_FADE_MS, EasingKind::Ease, false)
        });
    }

    pub fn set_footer_hover(&mut self, hover: bool) {
        if hover == self.footer_hover && (self.footer.is_some() || !hover) {
            return;
        }
        let from = self.footer.as_ref().map(|track| track.value(self.now_ms)).unwrap_or(FOOTER_REST);
        self.footer_hover = hover;
        let to = if hover { 1.0 } else { FOOTER_REST };
        self.footer = Some(if self.reduced {
            track(self.now_ms, to, to, 0, EasingKind::Linear, false)
        } else {
            track(self.now_ms, from, to, FOOTER_FADE_MS, EasingKind::Ease, false)
        });
    }

    /// 折叠把呈现宽度收到 0，展开回到上次松开时的宽度。存储值不在这里改。
    pub fn set_sidebar_collapsed(&mut self, collapsed: bool, expanded_width: f32) {
        self.sidebar_expanded_width = expanded_width;
        let to = if collapsed { 0.0 } else { expanded_width };
        if collapsed == self.sidebar_collapsed {
            if !collapsed && self.sidebar.as_ref().is_none_or(|track| track.finished(self.now_ms) && (track.to - to).abs() > 0.5) {
                self.sidebar = Some(track_done(to));
            }
            return;
        }
        let from = self.sidebar_presented_width();
        self.sidebar_collapsed = collapsed;
        self.sidebar = Some(if self.reduced {
            track(self.now_ms, to, to, 0, EasingKind::Linear, false)
        } else {
            track(self.now_ms, from, to, SIDEBAR_COLLAPSE_MS, EasingKind::Emphasis, false)
        });
    }

    pub fn modal_frame(&self) -> ModalFrame {
        let progress = self.modal.as_ref().map(|track| track.value(self.now_ms)).unwrap_or(0.0);
        let overlay = if self.reduced {
            if self.modal_wants_open { 1.0 } else { 0.0 }
        } else {
            scalar_at(self.modal.as_ref(), self.now_ms, MODAL_OVERLAY_MS)
        };
        ModalFrame {
            overlay_opacity: overlay,
            card_shift: MODAL_CARD_SHIFT * (1.0 - progress),
            card_scale: MODAL_CARD_SCALE + (1.0 - MODAL_CARD_SCALE) * progress,
            visible: self.modal_wants_open || self.modal.as_ref().is_some_and(|track| !track.finished(self.now_ms) && track.value(self.now_ms) > 0.01),
        }
    }

    pub fn panel_frame(&self) -> PanelFrame {
        let progress = self.panel.as_ref().map(|track| track.value(self.now_ms)).unwrap_or(0.0);
        let opacity = if self.reduced {
            if self.panel_wants_open { 1.0 } else { 0.0 }
        } else {
            scalar_at(self.panel.as_ref(), self.now_ms, PANEL_OPACITY_MS)
        };
        PanelFrame {
            opacity,
            shift: PANEL_SHIFT * (1.0 - progress),
            visible: self.panel_wants_open || progress > 0.01,
        }
    }

    pub fn startup_percent(&self) -> f32 {
        self.startup.as_ref().map(|track| track.value(self.now_ms)).unwrap_or(0.0)
    }

    pub fn operation_percent(&self) -> Option<f32> {
        self.operation.as_ref().map(|track| track.value(self.now_ms))
    }

    pub fn spinner_on(&self) -> bool {
        self.spinner_on && !self.reduced
    }

    /// 悬停已经开始，或淡出还没走完。分组工具在这期间留在树上。
    pub fn tools_revealed(&self) -> bool {
        self.tools_hover || self.tools.as_ref().is_some_and(|track| track.value(self.now_ms) > 0.01)
    }

    pub fn sweep_on(&self) -> bool {
        self.sweep_on && !self.reduced
    }

    pub fn spinner_degrees(&self) -> f32 {
        if !self.spinner_on || self.reduced {
            return 0.0;
        }
        let elapsed = self.spinner.as_ref().map(|track| self.now_ms.saturating_sub(track.started_ms)).unwrap_or(0);
        (elapsed % SPINNER_MS) as f32 / SPINNER_MS as f32 * 360.0
    }

    pub fn pulse_opacity(&self) -> f32 {
        if !self.pulse_on || self.reduced {
            return 1.0;
        }
        let progress = self.pulse.as_ref().map(|track| track.loop_progress(self.now_ms)).unwrap_or(0.0);
        if progress < 0.5 {
            lerp(PULSE_LOW, 1.0, ease_in_out(progress / 0.5))
        } else {
            lerp(1.0, PULSE_LOW, ease_in_out((progress - 0.5) / 0.5))
        }
    }

    /// 扫光平移，单位是轨道宽度的百分比。关键帧从 -120 到 250。
    pub fn sweep_percent(&self) -> f32 {
        if !self.sweep_on || self.reduced {
            return 0.0;
        }
        let progress = self.sweep.as_ref().map(|track| ease_in_out(track.loop_progress(self.now_ms))).unwrap_or(0.0);
        lerp(-120.0, 250.0, progress)
    }

    pub fn tools_opacity(&self) -> f32 {
        if self.reduced {
            return if self.tools_hover { 1.0 } else { 0.0 };
        }
        self.tools.as_ref().map(|track| track.value(self.now_ms)).unwrap_or(0.0)
    }

    pub fn footer_opacity(&self) -> f32 {
        if self.reduced {
            return if self.footer_hover { 1.0 } else { FOOTER_REST };
        }
        self.footer.as_ref().map(|track| track.value(self.now_ms)).unwrap_or(FOOTER_REST)
    }

    pub fn sidebar_presented_width(&self) -> f32 {
        self.sidebar.as_ref().map(|track| track.value(self.now_ms)).unwrap_or(if self.sidebar_collapsed {
            0.0
        } else {
            self.sidebar_expanded_width
        })
    }

    fn snap_all(&mut self) {
        if let Some(track) = &mut self.modal {
            *track = settled_modal(self.modal_wants_open);
        }
        if let Some(track) = &mut self.panel {
            *track = settled_scalar(self.panel_wants_open);
        }
        if let Some(track) = &mut self.startup {
            *track = track_done(track.to);
        }
        if let Some(track) = &mut self.operation {
            *track = track_done(track.to);
        }
        self.spinner = None;
        self.pulse = None;
        self.sweep = None;
        if let Some(track) = &mut self.tools {
            *track = track_done(if self.tools_hover { 1.0 } else { 0.0 });
        }
        if let Some(track) = &mut self.footer {
            *track = track_done(if self.footer_hover { 1.0 } else { FOOTER_REST });
        }
        if let Some(track) = &mut self.sidebar {
            *track = track_done(if self.sidebar_collapsed { 0.0 } else { self.sidebar_expanded_width });
        }
    }

    fn drop_finished(&mut self) {
        if self.modal.as_ref().is_some_and(|track| track.finished(self.now_ms) && !self.modal_wants_open) {
            self.modal = None;
        }
        if self.panel.as_ref().is_some_and(|track| track.finished(self.now_ms) && !self.panel_wants_open) {
            self.panel = None;
        }
    }
}

fn settled_modal(open: bool) -> Track {
    track_done(if open { 1.0 } else { 0.0 })
}

fn settled_scalar(open: bool) -> Track {
    track_done(if open { 1.0 } else { 0.0 })
}

fn track_done(value: f32) -> Track {
    Track { started_ms: 0, from: value, to: value, duration_ms: 0, easing: EasingKind::Linear, looping: false }
}

fn track(started_ms: u64, from: f32, to: f32, duration_ms: u64, easing: EasingKind, looping: bool) -> Track {
    Track { started_ms, from, to, duration_ms, easing, looping }
}

fn scalar_at(track: Option<&Track>, now_ms: u64, duration_ms: u64) -> f32 {
    let Some(track) = track else {
        return 0.0;
    };
    let elapsed = now_ms.saturating_sub(track.started_ms);
    let span = duration_ms.max(1);
    let linear = (elapsed as f32 / span as f32).clamp(0.0, 1.0);
    let eased = if track.to < track.from { 1.0 - apply_easing(EasingKind::Ease, 1.0 - linear) } else { apply_easing(EasingKind::Ease, linear) };
    eased.clamp(0.0, 1.0)
}

impl Track {
    fn value(&self, now_ms: u64) -> f32 {
        if self.duration_ms == 0 {
            return self.to;
        }
        let linear = if self.looping {
            self.loop_progress(now_ms)
        } else {
            let elapsed = now_ms.saturating_sub(self.started_ms);
            (elapsed as f32 / self.duration_ms as f32).clamp(0.0, 1.0)
        };
        lerp(self.from, self.to, apply_easing(self.easing, linear))
    }

    fn loop_progress(&self, now_ms: u64) -> f32 {
        if self.duration_ms == 0 {
            return 1.0;
        }
        (now_ms.saturating_sub(self.started_ms) % self.duration_ms) as f32 / self.duration_ms as f32
    }

    fn finished(&self, now_ms: u64) -> bool {
        self.looping || now_ms.saturating_sub(self.started_ms) >= self.duration_ms
    }
}

/// 对话框遮罩透明度和卡片的上移、缩放。
pub fn paint_modal(view: impl IntoView, motion: &MotionState) -> AnyView {
    let frame = motion.modal_frame();
    paint_layer(view, frame.overlay_opacity, shift_scale(frame.card_shift, frame.card_scale))
}

/// 弹层透明度和 4px 上移。
pub fn paint_panel(view: impl IntoView, motion: &MotionState) -> AnyView {
    let frame = motion.panel_frame();
    paint_layer(view, frame.opacity, shift_scale(frame.shift, 1.0))
}

fn paint_layer(view: impl IntoView, opacity: f32, transform: nana_ui_core::PaintTransform) -> AnyView {
    widget(nana_ui::runtime::Stack::column(0.0).with_layout(|layout| {
        layout.opacity = Some(opacity);
        layout.transform = Some(transform);
    }))
    .children((view.into_any(),))
    .into_any()
}

/// 把平移动效写成绘制矩阵。`shift_y` 向下为正，缩放绕控件中心。
pub fn shift_scale(shift_y: f32, scale: f32) -> nana_ui_core::PaintTransform {
    nana_ui_core::PaintTransform { a: scale, b: 0.0, c: 0.0, d: scale, e: 0.0, f: shift_y }
}

/// 旋转绘制矩阵，角度为度。
pub fn spin_transform(degrees: f32) -> nana_ui_core::PaintTransform {
    let (sin, cos) = degrees.to_radians().sin_cos();
    nana_ui_core::PaintTransform { a: cos, b: sin, c: -sin, d: cos, e: 0.0, f: 0.0 }
}

fn lerp(from: f32, to: f32, progress: f32) -> f32 {
    from + (to - from) * progress
}

fn apply_easing(kind: EasingKind, linear: f32) -> f32 {
    match kind {
        EasingKind::Linear => linear,
        EasingKind::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, linear),
        EasingKind::EaseInOut => ease_in_out(linear),
        EasingKind::Emphasis => cubic_bezier(0.2, 0.8, 0.2, 1.0, linear),
    }
}

fn ease_in_out(linear: f32) -> f32 {
    cubic_bezier(0.42, 0.0, 0.58, 1.0, linear)
}

/// 解 `cubic-bezier(x1, y1, x2, y2)` 在时间 `t` 上的 y。
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t == 0.0 || t == 1.0 {
        return t;
    }
    let mut guess = t;
    for _ in 0..8 {
        let x = bezier_component(x1, x2, guess) - t;
        let slope = bezier_slope(x1, x2, guess);
        if slope.abs() < 1.0e-4 {
            break;
        }
        guess = (guess - x / slope).clamp(0.0, 1.0);
    }
    bezier_component(y1, y2, guess)
}

fn bezier_component(a: f32, b: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
}

fn bezier_slope(a: f32, b: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * a + 6.0 * u * t * (b - a) + 3.0 * t * t * (1.0 - b)
}

/// 分隔条一帧。拖动中只夹取；松手，或没有指针捕获的键盘/双击变化，才写入。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SidebarResizeFrame {
    pub width: f32,
    pub dragging: bool,
    pub persist: bool,
    pub dirty: bool,
}

/// `extent` 是工作区资源区的尺寸。资源区比侧栏宽度少一条发丝间隙，这里加回来，
/// 存下的仍是 Vue `momobako.sidebarWidth` 的口径。
pub fn note_sidebar_resize(
    was_dragging: bool,
    dragging: bool,
    extent: f32,
    shell_width: f32,
    dirty: bool,
) -> SidebarResizeFrame {
    let width = super::workspace::clamp_sidebar_width(extent + super::render::WORKBENCH_GAP_PX);
    let changed = (width - shell_width).abs() > 0.5;
    if dragging {
        return SidebarResizeFrame { width, dragging: true, persist: false, dirty: dirty || changed };
    }
    if was_dragging {
        return SidebarResizeFrame { width, dragging: false, persist: dirty || changed, dirty: false };
    }
    SidebarResizeFrame { width, dragging: false, persist: changed, dirty: false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{ShellMessage, ShellViewModel};

    #[test]
    fn modal_matches_vue_endpoints_and_survives_the_next_reduce() {
        let mut model = writable_shell();
        model.reduce(ShellMessage::Files(super::super::files::FilesMessage::OpenDialog(
            super::super::files::FileDialog::CreateDirectory,
        )));
        let opened = model.motion.modal_frame();
        assert_eq!(opened.overlay_opacity, 0.0);
        assert_eq!(opened.card_shift, MODAL_CARD_SHIFT);
        assert_eq!(opened.card_scale, MODAL_CARD_SCALE);
        model.motion.advance(MODAL_OVERLAY_MS);
        let mid = model.motion.modal_frame().overlay_opacity;
        assert_eq!(mid, 1.0);
        model.reduce(ShellMessage::Refresh);
        assert_eq!(model.motion.modal_frame().overlay_opacity, mid);
        model.motion.advance(MODAL_CARD_MS);
        let frame = model.motion.modal_frame();
        assert_eq!(frame.overlay_opacity, 1.0);
        assert_eq!(frame.card_shift, 0.0);
        assert_eq!(frame.card_scale, 1.0);
        assert!(frame.visible);
    }

    fn writable_shell() -> ShellViewModel {
        let mut model = ShellViewModel::default();
        model.repository_id = Some("repo".into());
        model.workspace.active_repo_id = Some("repo".into());
        model.workspace.repositories.push(super::super::workspace::WorkspaceRepository {
            repo_id: "repo".into(),
            name: "库".into(),
            path: "C:/repo".into(),
            status: "ready".into(),
            backend_plugin_id: "filesystem".into(),
            capabilities: vec!["write".into()],
            cache_required: false,
            cache_status: String::new(),
        });
        model
    }

    #[test]
    fn reduced_motion_snaps_every_painted_track() {
        let mut motion = MotionState::default();
        motion.set_modal_open(true);
        motion.set_panel_open(true);
        motion.set_startup_percent(40.0);
        motion.set_operation_percent(Some(32.0));
        motion.set_spinner(true);
        motion.set_pulse(true);
        motion.set_sweep(true);
        motion.set_tools_hover(true);
        motion.set_footer_hover(true);
        motion.set_sidebar_collapsed(true, 276.0);
        motion.advance(40);
        motion.set_reduced(true);
        assert_eq!(motion.modal_frame().overlay_opacity, 1.0);
        assert_eq!(motion.modal_frame().card_scale, 1.0);
        assert_eq!(motion.panel_frame().opacity, 1.0);
        assert_eq!(motion.panel_frame().shift, 0.0);
        assert_eq!(motion.startup_percent(), 40.0);
        assert_eq!(motion.operation_percent(), Some(32.0));
        assert_eq!(motion.spinner_degrees(), 0.0);
        assert_eq!(motion.pulse_opacity(), 1.0);
        assert_eq!(motion.sweep_percent(), 0.0);
        assert_eq!(motion.tools_opacity(), 1.0);
        assert_eq!(motion.footer_opacity(), 1.0);
        assert_eq!(motion.sidebar_presented_width(), 0.0);
        assert!(!motion.active());
    }

    #[test]
    fn panel_progress_spinner_and_sidebar_use_the_vue_durations() {
        let mut motion = MotionState::default();
        motion.set_panel_open(true);
        assert_eq!(motion.panel_frame().opacity, 0.0);
        assert_eq!(motion.panel_frame().shift, PANEL_SHIFT);
        motion.advance(PANEL_OPACITY_MS);
        assert_eq!(motion.panel_frame().opacity, 1.0);
        motion.advance(PANEL_RISE_MS);
        assert_eq!(motion.panel_frame().shift, 0.0);
        motion.set_startup_percent(50.0);
        assert_eq!(motion.startup_percent(), 0.0);
        motion.advance(motion.now_ms() + PROGRESS_WIDTH_MS);
        assert_eq!(motion.startup_percent(), 50.0);
        motion.set_operation_percent(Some(32.0));
        assert_eq!(motion.operation_percent(), Some(0.0));
        motion.advance(motion.now_ms() + PROGRESS_WIDTH_MS);
        assert_eq!(motion.operation_percent(), Some(32.0));
        motion.set_spinner(true);
        let started = motion.now_ms();
        assert_eq!(motion.spinner_degrees(), 0.0);
        motion.advance(started + SPINNER_MS / 2);
        assert_eq!(motion.spinner_degrees(), 180.0);
        motion.set_tools_hover(true);
        assert_eq!(motion.tools_opacity(), 0.0);
        motion.advance(motion.now_ms() + SIDEBAR_TOOL_FADE_MS);
        assert_eq!(motion.tools_opacity(), 1.0);
        motion.set_footer_hover(true);
        assert_eq!(motion.footer_opacity(), FOOTER_REST);
        motion.advance(motion.now_ms() + FOOTER_FADE_MS);
        assert_eq!(motion.footer_opacity(), 1.0);
        motion.set_sidebar_collapsed(true, 276.0);
        assert_eq!(motion.sidebar_presented_width(), 276.0);
        motion.advance(motion.now_ms() + SIDEBAR_COLLAPSE_MS);
        assert_eq!(motion.sidebar_presented_width(), 0.0);
        motion.set_pulse(true);
        motion.set_sweep(true);
        assert_eq!(motion.pulse_opacity(), PULSE_LOW);
        assert_eq!(motion.sweep_percent(), -120.0);
        let looped = motion.now_ms();
        motion.advance(looped + SWEEP_MS);
        assert_eq!(motion.sweep_percent(), -120.0);
        motion.advance(looped + PULSE_MS);
        assert_eq!(motion.pulse_opacity(), PULSE_LOW);
    }

    #[test]
    fn sidebar_resize_persists_on_release_and_on_a_settled_change() {
        // 资源区比侧栏宽度少一条发丝间隙，读回时加上。
        let gap = super::super::render::WORKBENCH_GAP_PX;
        let dragging = note_sidebar_resize(false, true, 420.0 - gap, 276.0, false);
        assert!(!dragging.persist);
        assert!(dragging.dirty);
        assert_eq!(dragging.width, 420.0);
        let released = note_sidebar_resize(true, false, 420.0 - gap, 420.0, true);
        assert!(released.persist);
        assert!(!released.dirty);
        let settled = note_sidebar_resize(false, false, 420.0 - gap, 420.0, false);
        assert!(!settled.persist, "静止时资源区加间隙正好等于侧栏宽度，不再写入");
        let keyed = note_sidebar_resize(false, false, 428.0 - gap, 420.0, false);
        assert!(keyed.persist);
        assert_eq!(keyed.width, 428.0);
        let clamped = note_sidebar_resize(false, false, 1000.0, 276.0, false);
        assert_eq!(clamped.width, crate::theme_map::SIDEBAR_MAX_PX);
        let reset = note_sidebar_resize(false, false, 276.0 - gap, 480.0, false);
        assert!(reset.persist);
        assert_eq!(reset.width, 276.0);
    }
}

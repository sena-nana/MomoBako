//! 启动页和任务弹层进度条的填充：值变了按 `transition: width 0.18s ease` 从这一刻的值走到新值。
//!
//! 别的动效都交给 NanaUI：转圈和不确定进度由 `Spinner`、`Progress` 自己画，侧栏折叠是工作区自己的区域过渡，
//! 底部入口的淡入淡出是样式表的 `transition`。进度条不行：NanaUI 的 `Progress` 收到新值直接画到新值，
//! 没有值的过渡；`El::animate` 和样式表的 `transition` 只作用在节点自己的透明度、变换、像素宽高和颜色上，
//! 进度条的填充画在 `Progress` 节点里面，不是单独的节点，按比例的宽度也不是像素宽度。所以填充的值留在这里，
//! 按准备帧的真实时间走，经热信号（`hot.rs`）绑到进度条的值上；只有填充在走时要帧。
//!
//! 减少动态效果跟 NanaUI 文档上下文的设置（`AppContext::reduced_motion`，默认跟系统）：准备帧每帧读一次，
//! 打开时所有填充直接停在新值。

/// 填充宽度过渡的时长：`transition: width 0.18s ease`。
pub const PROGRESS_WIDTH_MS: u64 = 180;

/// 一段还没走完的填充。走完以后 `value` 一直是 `to`。
#[derive(Clone, Copy, Debug)]
struct Fill {
    started_ms: u64,
    from: f32,
    to: f32,
}

impl Fill {
    /// 直接停在 `value`，不过渡。
    fn settled(value: f32) -> Self {
        Self {
            started_ms: 0,
            from: value,
            to: value,
        }
    }

    fn finished(&self, now_ms: u64) -> bool {
        self.from == self.to || now_ms.saturating_sub(self.started_ms) >= PROGRESS_WIDTH_MS
    }

    fn value(&self, now_ms: u64) -> f32 {
        if self.finished(now_ms) {
            return self.to;
        }
        let linear = now_ms.saturating_sub(self.started_ms) as f32 / PROGRESS_WIDTH_MS as f32;
        self.from + (self.to - self.from) * ease(linear)
    }
}

/// 进度条填充的时钟和各条的值，百分比 0–100。
#[derive(Clone, Debug, Default)]
pub struct ProgressFills {
    now_ms: u64,
    reduced: bool,
    startup: Option<Fill>,
    /// 任务弹层每一行的填充，按行键，见 [`ProgressFills::set_rows`]。
    rows: Vec<(String, Fill)>,
}

impl ProgressFills {
    /// 还有填充在走，下一帧要接着画。
    pub fn active(&self) -> bool {
        let now = self.now_ms;
        self.startup.is_some_and(|fill| !fill.finished(now))
            || self.rows.iter().any(|(_, fill)| !fill.finished(now))
    }

    /// 时钟走 `step_ms`。没有填充在走时不动：新的过渡从这一刻起步。
    pub fn tick(&mut self, step_ms: u64) {
        if self.active() {
            self.now_ms = self.now_ms.saturating_add(step_ms);
        }
    }

    /// 跟上减少动态效果的设置。打开时走到一半的填充直接停在新值。
    pub fn set_reduced(&mut self, reduced: bool) {
        self.reduced = reduced;
        if reduced {
            if let Some(fill) = &mut self.startup {
                *fill = Fill::settled(fill.to);
            }
            for (_, fill) in &mut self.rows {
                *fill = Fill::settled(fill.to);
            }
        }
    }

    /// 启动页进度条的目标百分比。
    pub fn set_startup(&mut self, percent: f32) {
        let percent = percent.clamp(0.0, 100.0);
        let fill = self.startup.unwrap_or_else(|| Fill::settled(0.0));
        if (fill.to - percent).abs() < 0.01 {
            return;
        }
        self.startup = Some(self.toward(fill, percent));
    }

    /// 任务弹层各行进度条的目标百分比（行键、百分比），弹层关着时传空。新出现的行直接停在目标（CSS 过渡
    /// 不在挂载时播），目标变了从这一刻的值走到新目标，没有了的行一起拿掉。
    pub fn set_rows(&mut self, targets: &[(String, f32)]) {
        let next = targets
            .iter()
            .map(|(key, target)| {
                let target = target.clamp(0.0, 100.0);
                let existing = self
                    .rows
                    .iter()
                    .find(|(existing, _)| existing == key)
                    .map(|(_, fill)| *fill);
                let fill = match existing {
                    Some(fill) if (fill.to - target).abs() < 0.01 => fill,
                    Some(fill) => self.toward(fill, target),
                    None => Fill::settled(target),
                };
                (key.clone(), fill)
            })
            .collect();
        self.rows = next;
    }

    /// 启动页进度条这一刻的百分比。
    pub fn startup(&self) -> f32 {
        self.startup.map_or(0.0, |fill| fill.value(self.now_ms))
    }

    /// 任务弹层各行这一刻的百分比，按行键。
    pub fn rows(&self) -> Vec<(String, f32)> {
        self.rows
            .iter()
            .map(|(key, fill)| (key.clone(), fill.value(self.now_ms)))
            .collect()
    }

    /// 从 `fill` 这一刻的值走到 `to`；减少动态效果时直接停在 `to`。
    fn toward(&self, fill: Fill, to: f32) -> Fill {
        if self.reduced {
            return Fill::settled(to);
        }
        Fill {
            started_ms: self.now_ms,
            from: fill.value(self.now_ms),
            to,
        }
    }
}

/// CSS `ease`：`cubic-bezier(0.25, 0.1, 0.25, 1)` 在时间 `t` 上的进度。
fn ease(t: f32) -> f32 {
    const X1: f32 = 0.25;
    const Y1: f32 = 0.1;
    const X2: f32 = 0.25;
    const Y2: f32 = 1.0;
    let t = t.clamp(0.0, 1.0);
    if t == 0.0 || t == 1.0 {
        return t;
    }
    let component = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    };
    let slope = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * a + 6.0 * u * s * (b - a) + 3.0 * s * s * (1.0 - b)
    };
    let mut guess = t;
    for _ in 0..8 {
        let error = component(X1, X2, guess) - t;
        let rate = slope(X1, X2, guess);
        if rate.abs() < 1.0e-4 {
            break;
        }
        guess = (guess - error / rate).clamp(0.0, 1.0);
    }
    component(Y1, Y2, guess)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_move_for_180ms_and_new_rows_land_at_once() {
        let mut fills = ProgressFills::default();
        fills.set_startup(50.0);
        assert_eq!(fills.startup(), 0.0, "启动条从 0 起步");
        assert!(fills.active());
        fills.tick(PROGRESS_WIDTH_MS / 2);
        let halfway = fills.startup();
        assert!(halfway > 25.0 && halfway < 50.0, "ease 前快后慢：{halfway}");
        fills.tick(PROGRESS_WIDTH_MS / 2);
        assert_eq!(fills.startup(), 50.0);
        assert!(!fills.active(), "走完不再要帧");

        fills.set_rows(&[("task".into(), 12.0)]);
        assert_eq!(
            fills.rows(),
            vec![("task".to_string(), 12.0)],
            "新行直接停在目标"
        );
        assert!(!fills.active());
        fills.set_rows(&[("task".into(), 32.0)]);
        assert_eq!(
            fills.rows(),
            vec![("task".to_string(), 12.0)],
            "从这一刻的值起步"
        );
        fills.tick(PROGRESS_WIDTH_MS);
        assert_eq!(fills.rows(), vec![("task".to_string(), 32.0)]);
        fills.set_rows(&[]);
        assert!(fills.rows().is_empty(), "弹层关上，行一起拿掉");
    }

    #[test]
    fn reduced_motion_lands_every_fill_at_once() {
        let mut fills = ProgressFills::default();
        fills.set_startup(40.0);
        fills.set_rows(&[("task".into(), 10.0)]);
        fills.set_rows(&[("task".into(), 32.0)]);
        fills.tick(40);
        fills.set_reduced(true);
        assert_eq!(fills.startup(), 40.0);
        assert_eq!(fills.rows(), vec![("task".to_string(), 32.0)]);
        assert!(!fills.active());
        fills.set_startup(80.0);
        assert_eq!(fills.startup(), 80.0, "减少动态效果时直接到新值");
        assert!(!fills.active());
        fills.set_reduced(false);
        fills.set_startup(90.0);
        assert!(fills.active(), "关掉以后接着过渡");
    }
}

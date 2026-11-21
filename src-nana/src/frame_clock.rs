//! 宿主时钟：计时器的时间线和动效的帧步长。
//!
//! 时间按第一次读钟以来的整毫秒算，每次都从原点重新换算，毫秒以下的零头不会越攒越多。时钟分两条用：
//! - 计时器（搜索和元数据自动保存的延时、缩略图和下一页的空闲预取、仓库操作行的爬升和收起、外部拖放停在
//!   侧栏文件夹上的展开）不要帧。宿主每个回调开头（`update`、`input_event`、`wake`、`prepare`）都用
//!   [`FrameClock::sync_timers`] 取上次同步以来流逝的时间交给它们，所以归约里武装的计时器从武装那一刻起算。
//!   最早的到期时刻经 [`FrameClock::deadline`] 换成 `Instant` 交给 NanaUI 的 `next_wakeup`，到点由 `wake`
//!   触发；只有计时器在等时界面不画帧。
//! - 动效（进度条填充、播放时钟和图片停留）要连续帧，步长是这一帧和上一帧之间的时间。上一帧没要连续帧
//!   （界面闲着）时这一帧是起点，步长为 0：刚按下的播放和刚开始的过渡不把闲着的间隔算进去。步长不封顶，
//!   声音由系统设备按真实时间放，进度跟着声音走。

use std::time::{Duration, Instant};

use nana_ui::FrameDemand;

/// 一次同步交出去的时间。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStep {
    /// 计时器走的毫秒数：上次同步以来真实流逝的时间。
    pub timer_ms: u64,
    /// 动效和播放时钟走的毫秒数：和上一帧之间的时间，上一帧没要连续帧时为 0。
    pub motion_ms: u64,
}

impl FrameStep {
    /// 两样都走 `ms` 毫秒。单测和离屏场景没有真实时钟，按它推进。
    pub const fn millis(ms: u64) -> Self {
        Self {
            timer_ms: ms,
            motion_ms: ms,
        }
    }
}

/// 产品窗口的宿主时钟。
#[derive(Debug, Default)]
pub struct FrameClock {
    /// 第一次读钟的时刻，毫秒数从这里算。
    origin: Option<Instant>,
    /// 计时器上次同步到的毫秒数。
    timers_ms: u64,
    /// 上一帧的毫秒数。
    frame_ms: u64,
    /// 上一帧要了连续帧：两帧之间有动效在走。
    running: bool,
}

impl FrameClock {
    fn millis(&mut self, now: Instant) -> u64 {
        let origin = *self.origin.get_or_insert(now);
        u64::try_from(now.saturating_duration_since(origin).as_millis()).unwrap_or(u64::MAX)
    }

    /// 计时器从上次同步以来走过的毫秒数，`now` 是这次回调的时刻。比上次还早的时刻按 0 算。
    pub fn sync_timers(&mut self, now: Instant) -> u64 {
        let at = self.millis(now);
        let step = at.saturating_sub(self.timers_ms);
        self.timers_ms = self.timers_ms.max(at);
        step
    }

    /// 准备一帧时的步长，`now` 是这一帧开始准备的时刻：计时器照常同步，动效按上一帧有没有要连续帧取步长。
    pub fn step(&mut self, now: Instant) -> FrameStep {
        let timer_ms = self.sync_timers(now);
        let at = self.timers_ms;
        let motion_ms = if self.running {
            at.saturating_sub(self.frame_ms)
        } else {
            0
        };
        self.frame_ms = at;
        FrameStep {
            timer_ms,
            motion_ms,
        }
    }

    /// 记下这一帧准备完以后要的帧：要连续帧时下一帧的间隔算进动效，按需重绘时不算。
    pub fn note_demand(&mut self, demand: FrameDemand) {
        self.running = matches!(demand, FrameDemand::Continuous(_));
    }

    /// 计时器时间线上，从上次同步再过 `due_in_ms` 毫秒的时刻。还没读过钟时从现在算。
    pub fn deadline(&self, due_in_ms: u64) -> Instant {
        let Some(origin) = self.origin else {
            return Instant::now() + Duration::from_millis(due_in_ms);
        };
        let at = Duration::from_millis(self.timers_ms.saturating_add(due_in_ms));
        origin.checked_add(at).unwrap_or(origin)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::time::{Duration, Instant};

    use nana_ui::FrameDemand;

    use super::{FrameClock, FrameStep};

    fn continuous() -> FrameDemand {
        FrameDemand::Continuous(NonZeroU32::new(30).expect("30"))
    }

    /// 闲着以后的第一帧从 0 起算动效；计时器不看帧，把两次同步之间的时间照实交出去。
    #[test]
    fn idle_frames_restart_motion_but_timers_keep_real_time() {
        let start = Instant::now();
        let mut clock = FrameClock::default();
        assert_eq!(clock.step(start), FrameStep::default(), "第一帧没有上一帧");
        clock.note_demand(FrameDemand::OnDemand);
        assert_eq!(
            clock.step(start + Duration::from_secs(5)),
            FrameStep {
                timer_ms: 5_000,
                motion_ms: 0
            },
            "闲着时过去的 5 秒不算进动效"
        );
        clock.note_demand(continuous());
        assert_eq!(
            clock.step(start + Duration::from_millis(5_033)),
            FrameStep::millis(33)
        );
    }

    /// 回调之间的同步把时间交给计时器，下一帧只拿剩下的；动效仍按两帧之间算。
    #[test]
    fn syncs_between_frames_split_timer_time_but_not_motion() {
        let start = Instant::now();
        let mut clock = FrameClock::default();
        clock.step(start);
        clock.note_demand(continuous());
        assert_eq!(clock.sync_timers(start + Duration::from_millis(10)), 10);
        assert_eq!(
            clock.step(start + Duration::from_millis(33)),
            FrameStep {
                timer_ms: 23,
                motion_ms: 33
            }
        );
        assert_eq!(clock.sync_timers(start), 0, "倒回去的时刻按 0 算");
    }

    /// 毫秒数每次从原点换算：30 帧的帧距 33.333 ms，30 帧以后正好 1000 ms。
    #[test]
    fn sub_millisecond_remainders_do_not_drift() {
        let start = Instant::now();
        let mut clock = FrameClock::default();
        clock.step(start);
        clock.note_demand(continuous());
        let mut total = 0;
        for frame in 1..=30u64 {
            let at = start + Duration::from_micros(frame * 1_000_000 / 30);
            total += clock.step(at).motion_ms;
            clock.note_demand(continuous());
        }
        assert_eq!(total, 1_000);
    }

    /// 到期时刻从上次同步起算；到点叫醒时同步出的毫秒数正好够到期。
    #[test]
    fn deadlines_wake_exactly_when_the_timer_is_due() {
        let start = Instant::now();
        let mut clock = FrameClock::default();
        clock.sync_timers(start);
        clock.sync_timers(start + Duration::from_micros(1_400));
        let due = clock.deadline(250);
        assert_eq!(due, start + Duration::from_millis(251));
        assert_eq!(clock.sync_timers(due), 250);
    }
}

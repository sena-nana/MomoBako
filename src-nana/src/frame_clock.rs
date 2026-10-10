//! 准备帧的时钟：把两帧之间真实流逝的时间交给壳层的计时器、动效和播放时钟。
//!
//! 宿主按需要要帧（`FrameDemand::Continuous(30)`），实际帧距随负载、输入和别处的重绘变，30 帧、
//! 120 帧都会有，所以不能按每帧固定的毫秒数累加。这里记下上一帧的时刻，算出这一帧的步长：
//! - 上一帧没有要连续帧（界面闲着）时，这一帧是计时的起点，步长为 0。闲着的这段时间里没有计时器
//!   在走，刚在归约里武装的计时器（搜索延时、仓库操作爬升）从这一帧起算，不被空闲间隔提前触发。
//! - 界面计时器和动效的步长不超过 [`MAX_UI_STEP_MS`]：线程卡住、窗口挂起之后恢复时，动效、进度爬升和
//!   延时接着原来的位置走，不一下子跳到头（Vue 的 `setInterval` 卡住以后也只补一拍）。
//! - 播放时钟用不封顶的真实间隔：声音由系统设备按真实时间放，进度条跟着声音走；幻灯片一帧最多
//!   切一项（到头以后剩下的时间不带到下一项），不会连跳几张。
//!
//! 毫秒以下的零头留到下一帧，长时间累加不漂移。

use std::time::Instant;

use nana_ui::FrameDemand;

/// 界面计时器和动效一帧最多走的毫秒数。正常帧距在 8–40 ms，偶尔的卡顿照实算，几百毫秒以上的
/// 停顿按这个数算。
pub const MAX_UI_STEP_MS: u64 = 250;

/// 一帧的步长。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStep {
    /// 界面计时器和动效走的毫秒数，不超过 [`MAX_UI_STEP_MS`]。
    pub ui_ms: u64,
    /// 播放时钟走的毫秒数：真实间隔，不封顶。
    pub media_ms: u64,
}

impl FrameStep {
    /// 两样都走 `ms` 毫秒。离屏验收和测试没有真实帧时钟，按它推进。
    pub const fn millis(ms: u64) -> Self {
        Self { ui_ms: ms, media_ms: ms }
    }
}

/// 产品窗口的帧时钟，`prepare` 开头取步长，末尾记下这一帧要不要连续帧。
#[derive(Debug, Default)]
pub struct FrameClock {
    last: Option<Instant>,
    /// 上一帧要了连续帧：两帧之间有计时器或动效在走。
    running: bool,
    /// 还没凑满一毫秒的零头，单位微秒。
    carry_us: u64,
}

impl FrameClock {
    /// 这一帧的步长，`now` 是这一帧开始准备的时刻。
    ///
    /// 算法：上一帧闲着或这是第一帧时返回 0，并丢掉零头；否则把两帧间隔（微秒）加上零头，整毫秒
    /// 部分是播放时钟的步长，余下的留到下一帧；界面步长再封顶 [`MAX_UI_STEP_MS`]。
    pub fn step(&mut self, now: Instant) -> FrameStep {
        let previous = self.last.replace(now);
        let Some(previous) = previous.filter(|_| self.running) else {
            self.carry_us = 0;
            return FrameStep::default();
        };
        let elapsed_us = u64::try_from(now.saturating_duration_since(previous).as_micros()).unwrap_or(u64::MAX);
        let total_us = elapsed_us.saturating_add(self.carry_us);
        let media_ms = total_us / 1000;
        self.carry_us = total_us % 1000;
        FrameStep { ui_ms: media_ms.min(MAX_UI_STEP_MS), media_ms }
    }

    /// 记下这一帧准备完以后要的帧：要连续帧时下一帧的间隔算进计时，按需重绘时不算。
    pub fn note_demand(&mut self, demand: FrameDemand) {
        self.running = matches!(demand, FrameDemand::Continuous(_));
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::time::{Duration, Instant};

    use nana_ui::FrameDemand;

    use super::{FrameClock, FrameStep, MAX_UI_STEP_MS};

    fn continuous() -> FrameDemand {
        FrameDemand::Continuous(NonZeroU32::new(30).expect("30"))
    }

    /// 第一帧和闲着以后的第一帧从 0 起算：空闲的间隔不算进计时。
    #[test]
    fn idle_frames_start_the_clock_from_zero() {
        let start = Instant::now();
        let mut clock = FrameClock::default();
        assert_eq!(clock.step(start), FrameStep::default(), "第一帧没有上一帧");
        clock.note_demand(FrameDemand::OnDemand);
        assert_eq!(clock.step(start + Duration::from_secs(5)), FrameStep::default(), "闲着时过去的 5 秒不算");
        clock.note_demand(continuous());
        assert_eq!(clock.step(start + Duration::from_millis(5_033)), FrameStep::millis(33));
    }

    /// 毫秒以下的零头留到下一帧：30 帧的帧距 33.333 ms，30 帧以后正好 1000 ms。
    #[test]
    fn sub_millisecond_remainders_carry_over() {
        let start = Instant::now();
        let mut clock = FrameClock::default();
        clock.step(start);
        clock.note_demand(continuous());
        let mut total = 0;
        for frame in 1..=30u64 {
            let at = start + Duration::from_micros(frame * 1_000_000 / 30);
            total += clock.step(at).ui_ms;
            clock.note_demand(continuous());
        }
        assert_eq!(total, 1_000);
    }

    /// 大间隔：界面步长封顶，播放时钟照真实间隔走。
    #[test]
    fn long_gaps_cap_the_ui_step_but_not_the_media_clock() {
        let start = Instant::now();
        let mut clock = FrameClock::default();
        clock.step(start);
        clock.note_demand(continuous());
        let step = clock.step(start + Duration::from_secs(3));
        assert_eq!(step, FrameStep { ui_ms: MAX_UI_STEP_MS, media_ms: 3_000 });
        clock.note_demand(continuous());
        assert_eq!(clock.step(start + Duration::from_millis(3_016)), FrameStep::millis(16), "卡顿以后照常走");
    }
}

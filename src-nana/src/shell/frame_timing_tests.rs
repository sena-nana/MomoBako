//! 计时器不要帧：只靠到点叫醒，和 30 帧、120 帧一直画，真实时间相同时走到同样的状态（`frame_clock.rs`）。
//!
//! 界面先闲 5 秒，再从用户操作开始：在标题栏输入搜索词（250ms 的延时）、点「刷新文件夹树」（仓库操作行每
//! 220ms 往上爬）。归约之前宿主把流逝的时间交给计时器（`update` 开头那一步），所以闲着的 5 秒不算进延时。
//! 之后分别按产品的准备帧（`poll_timers` 加 `prepare_motion`，步长由宿主时钟算）以 30 帧、120 帧走，
//! 或者只在 `next_wakeup` 给的时刻叫醒（`wake` 那一步），在几个时刻比对状态。

use std::time::{Duration, Instant};

use nana_ui::{ApplicationWindow, FrameDemand};

use crate::frame_clock::{FrameClock, FrameStep};
use crate::window_host::{next_timer_in_ms, pass_timer_time, prepare_motion, run_timers};

use super::{
    InspectEffect, InspectMessage, ShellMessage, ShellPage, ShellViewModel, SidebarMessage,
};

/// 用户操作之前闲着的时间。
const IDLE_MS: u64 = 5_000;

/// 某一刻的计时状态。
#[derive(Debug, PartialEq)]
struct Sample {
    at_ms: u64,
    operation: Option<f64>,
    searched: bool,
}

/// 宿主怎么推进时间。
#[derive(Clone, Copy)]
enum Drive {
    /// 每秒这么多帧，一直画。
    Frames(u64),
    /// 不画帧，只在最早到期的时刻叫醒。
    Wakeups,
}

/// 一次运行：各时刻的状态和叫醒的次数。
struct Run {
    samples: Vec<Sample>,
    wakes: usize,
}

fn searched(model: &mut ShellViewModel) -> bool {
    model
        .inspect
        .take_effects()
        .iter()
        .any(|effect| matches!(effect, InspectEffect::Search { .. }))
}

/// 闲 `IDLE_MS` 以后做两个操作，再按 `drive` 走到最后一个时刻（从操作起算的毫秒），每个时刻取一次状态。
/// 帧的时刻按整微秒算，各帧率都有帧正好落在这些时刻上。
fn run(drive: Drive, checkpoints_ms: &[u64]) -> Run {
    let start = Instant::now();
    let at = |ms: u64| start + Duration::from_millis(IDLE_MS + ms);
    let mut clock = FrameClock::default();
    clock.sync_timers(start);
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    pass_timer_time(&mut model, clock.sync_timers(at(0)));
    model.reduce(ShellMessage::Inspect(InspectMessage::SetQuery(
        "cover".into(),
    )));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    let _ = model.inspect.take_effects();
    let mut window = ApplicationWindow::new();
    let mut found = false;
    let mut samples = Vec::new();
    let mut wakes = 0;
    let sample = |model: &ShellViewModel, at_ms, searched| Sample {
        at_ms,
        operation: model.operation.shown().map(|progress| progress.value),
        searched,
    };
    match drive {
        Drive::Frames(fps) => {
            let last = checkpoints_ms.last().copied().unwrap_or(0);
            for frame in 0..=last * fps / 1000 {
                let at_us = frame * 1_000_000 / fps;
                let step = clock.step(at(0) + Duration::from_micros(at_us));
                super::poll_timers(&mut model, step);
                prepare_motion(&mut model, None, &mut window, step);
                assert_eq!(
                    window.demand,
                    FrameDemand::OnDemand,
                    "只有计时器在等时不要帧"
                );
                clock.note_demand(window.demand);
                found |= searched(&mut model);
                if at_us % 1000 == 0 && checkpoints_ms.contains(&(at_us / 1000)) {
                    samples.push(sample(&model, at_us / 1000, found));
                }
            }
        }
        Drive::Wakeups => {
            for &checkpoint in checkpoints_ms {
                while let Some(due) = next_timer_in_ms(&model).map(|ms| clock.deadline(ms)) {
                    if due > at(checkpoint) {
                        break;
                    }
                    wakes += 1;
                    let timer_ms = clock.sync_timers(due);
                    super::poll_timers(
                        &mut model,
                        FrameStep {
                            timer_ms,
                            motion_ms: 0,
                        },
                    );
                    run_timers(&mut model, timer_ms);
                    found |= searched(&mut model);
                }
                samples.push(sample(&model, checkpoint, found));
            }
        }
    }
    Run { samples, wakes }
}

/// 同样走 1 秒：只靠叫醒和 30 帧、120 帧一直画，在 200ms、300ms 和 1000ms 的仓库操作进度和搜索是否已跑完全一样，
/// 而且和真实时间对得上；闲着的 5 秒不算进搜索延时和爬升间隔。叫醒只发生在到期的时刻：250ms 的搜索一次，
/// 220、440、660、880ms 的爬升各一次。
#[test]
fn wakeups_and_frames_reach_the_same_state_in_the_same_real_time() {
    let checkpoints = [200, 300, 1_000];
    let woken = run(Drive::Wakeups, &checkpoints);
    assert_eq!(woken.samples, run(Drive::Frames(30), &checkpoints).samples);
    assert_eq!(woken.samples, run(Drive::Frames(120), &checkpoints).samples);
    assert_eq!(woken.wakes, 5);
    let [at_200, at_300, at_1000] = &woken.samples[..] else {
        panic!("每个时刻都该有状态：{:?}", woken.samples);
    };
    assert!(!at_200.searched, "250ms 的搜索延时到 200ms 还没到");
    assert_eq!(at_200.operation, Some(12.0), "第一次爬升在 220ms");
    assert!(at_300.searched);
    assert_eq!(at_300.operation, Some(17.0));
    assert_eq!(
        at_1000.operation,
        Some(32.0),
        "220、440、660、880ms 各爬一级：12 → 17 → 22 → 27 → 32"
    );
}

/// 空闲预取到点叫醒一次就清掉：之后不再报到期时刻，宿主不会反复叫醒；也不标脏，叫醒以后不画帧。
#[test]
fn a_prefetch_wakeup_fires_once_without_touching_the_surface() {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    assert_eq!(next_timer_in_ms(&model), None, "闲着时没有要等的计时器");
    let now = model.files.tick_prefetch(0);
    model.files.schedule_thumbnail_prefetch(true, now);
    assert_eq!(next_timer_in_ms(&model), Some(420));
    let revision = model.revision;
    run_timers(&mut model, 419);
    assert_eq!(next_timer_in_ms(&model), Some(1));
    run_timers(&mut model, 1);
    assert_eq!(next_timer_in_ms(&model), None);
    assert_eq!(model.revision, revision);
}

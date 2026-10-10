//! 帧率不同、真实时间相同时，界面计时器和动效走到同样的状态（`frame_clock.rs`）。
//!
//! 从用户操作开始：收起侧栏（240ms 的宽度动效）、在标题栏输入搜索词（250ms 的延时）、点「刷新文件夹树」
//! （仓库操作行每 220ms 往上爬）。再按产品的准备帧（`poll_timers` 加 `prepare_motion`，步长由
//! 帧时钟按帧的时刻算）分别以 30 帧和 120 帧走同样的真实时间，在几个时刻比对状态。

use std::time::{Duration, Instant};

use nana_ui::ApplicationWindow;

use crate::frame_clock::FrameClock;

use super::{InspectEffect, InspectMessage, ShellMessage, ShellPage, ShellViewModel, SidebarMessage};

/// 某一刻的计时状态。
#[derive(Debug, PartialEq)]
struct Sample {
    at_ms: u64,
    sidebar_width: f32,
    operation: Option<f64>,
    searched: bool,
}

/// 按 `fps` 走到最后一个时刻，在每个时刻（毫秒）取一次状态。帧的时刻按整微秒算，各帧率都有帧正好落在
/// 这些时刻上。
fn run(fps: u64, checkpoints_ms: &[u64]) -> Vec<Sample> {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.reduce(ShellMessage::ToggleSidebar);
    model.reduce(ShellMessage::Inspect(InspectMessage::SetQuery("cover".into())));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    let _ = model.inspect.take_effects();
    let mut window = ApplicationWindow::new();
    let mut clock = FrameClock::default();
    let start = Instant::now();
    let last = checkpoints_ms.last().copied().unwrap_or(0);
    let mut searched = false;
    let mut samples = Vec::new();
    for frame in 0..=last * fps / 1000 {
        let at_us = frame * 1_000_000 / fps;
        let step = clock.step(start + Duration::from_micros(at_us));
        super::poll_timers(&mut model, step);
        crate::window_host::prepare_motion(&mut model, None, &mut window, step);
        clock.note_demand(window.demand);
        searched |= model.inspect.take_effects().iter().any(|effect| matches!(effect, InspectEffect::Search { .. }));
        if at_us % 1000 == 0 && checkpoints_ms.contains(&(at_us / 1000)) {
            samples.push(Sample {
                at_ms: at_us / 1000,
                sidebar_width: model.motion.sidebar_presented_width(),
                operation: model.operation.shown().map(|progress| progress.value),
                searched,
            });
        }
    }
    samples
}

/// 同样走 1 秒：30 帧和 120 帧在 200ms、300ms 和 1000ms 的侧栏宽度、仓库操作进度和搜索是否已跑完全一样，
/// 而且和真实时间对得上（按每帧 16ms 算时，30 帧的 1 秒只走了 480ms）。
#[test]
fn thirty_and_one_hundred_twenty_fps_reach_the_same_state_in_the_same_real_time() {
    let checkpoints = [200, 300, 1_000];
    let slow = run(30, &checkpoints);
    let fast = run(120, &checkpoints);
    assert_eq!(slow, fast);
    let [at_200, at_300, at_1000] = &slow[..] else {
        panic!("每个时刻都该有一帧：{slow:?}");
    };
    assert!(at_200.sidebar_width > 0.0, "240ms 的收起动效到 200ms 还没走完");
    assert!(!at_200.searched, "250ms 的搜索延时到 200ms 还没到");
    assert_eq!(at_200.operation, Some(12.0), "第一次爬升在 220ms");
    assert_eq!(at_300.sidebar_width, 0.0);
    assert!(at_300.searched);
    assert_eq!(at_300.operation, Some(17.0));
    assert_eq!(at_1000.operation, Some(32.0), "220、440、660、880ms 各爬一级：12 → 17 → 22 → 27 → 32");
}

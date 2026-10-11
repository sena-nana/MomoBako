//! 服务任务的派发：panic、排不进去和服务没起来都按失败交回；所有派发点都经过这里。
//!
//! 假的宿主照 NanaUI 任务线程的样子跑任务：直接把任务跑完，不接 panic。兜底没接住的 panic 会冲出来，测试就失败。

use std::cell::RefCell;
use std::path::Path;
use std::task::{Context, Poll, Waker};

use nana_ui::runtime::Task;

use super::{settle, spawn, spawn_service, TaskHost};
use crate::shell::ShellMessage;

/// 假的宿主：`accept` 为假时排不进任务；排进来的任务当场跑完，结果和送回归约的消息都记在 `delivered` 里。
pub(crate) struct InlineHost {
    accept: bool,
    pub(crate) submitted: RefCell<usize>,
    pub(crate) delivered: RefCell<Vec<ShellMessage>>,
}

impl InlineHost {
    pub(crate) fn new() -> Self {
        Self { accept: true, submitted: RefCell::new(0), delivered: RefCell::new(Vec::new()) }
    }

    fn rejecting() -> Self {
        Self { accept: false, ..Self::new() }
    }
}

impl TaskHost for InlineHost {
    fn submit(&self, task: Task<ShellMessage>) -> Result<(), String> {
        if !self.accept {
            return Err("runtime host task queue is full".into());
        }
        *self.submitted.borrow_mut() += 1;
        let mut future = task.into_future();
        match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(message) => self.delivered.borrow_mut().push(message),
            Poll::Pending => panic!("服务任务都是一次跑完的同步工作"),
        }
        Ok(())
    }

    fn deliver(&self, message: ShellMessage) {
        self.delivered.borrow_mut().push(message);
    }
}

/// 照产品派发一个任务（经 [`spawn`]），交回送回归约的那一条消息。给别的测试模拟某个派发点的任务 panic 用。
pub(crate) fn run_guarded(
    what: &str,
    failed: impl Fn(String) -> ShellMessage + Clone + Send + 'static,
    work: impl FnOnce() -> ShellMessage + Send + 'static,
) -> ShellMessage {
    let host = InlineHost::new();
    spawn(&host, what, failed, work);
    let mut delivered = host.delivered.into_inner();
    assert_eq!(delivered.len(), 1, "一个任务只交回一条消息");
    delivered.remove(0)
}

fn listing_error(message: &ShellMessage) -> Option<&str> {
    match message {
        ShellMessage::FileBrowserLoaded(Err(error)) => Some(error),
        _ => None,
    }
}

fn listing_failed(error: String) -> ShellMessage {
    ShellMessage::FileBrowserLoaded(Err(error))
}

/// 任务 panic：在任务里接住，按失败交回 panic 的原因，任务线程不会因此退出。
#[test]
fn a_panicking_task_hands_back_its_failure() {
    let message = run_guarded("文件浏览", listing_failed, || panic!("解码器越界"));
    assert_eq!(listing_error(&message), Some("文件浏览任务异常退出：解码器越界"));
    let formatted = run_guarded("文件浏览", listing_failed, || panic!("第 {} 页", 2));
    assert_eq!(listing_error(&formatted), Some("文件浏览任务异常退出：第 2 页"));
}

/// 任务排不进去：当场把失败送回归约。
#[test]
fn a_task_that_cannot_be_queued_hands_back_its_failure() {
    let host = InlineHost::rejecting();
    spawn(&host, "文件浏览", listing_failed, || panic!("排不进去的任务不会跑"));
    let delivered = host.delivered.into_inner();
    assert_eq!(delivered.iter().filter_map(listing_error).collect::<Vec<_>>(), ["文件浏览任务提交失败：runtime host task queue is full"]);
}

/// 服务没起来：不派发，当场把失败送回归约。
#[test]
fn missing_services_hand_back_their_failure_without_queueing() {
    let host = InlineHost::new();
    let prepared = RefCell::new(false);
    spawn_service(None::<&()>, &host, "文件浏览", listing_failed, |_| {
        *prepared.borrow_mut() = true;
        || listing_failed("不该跑".into())
    });
    assert!(!*prepared.borrow());
    assert_eq!(*host.submitted.borrow(), 0);
    assert_eq!(host.delivered.borrow().iter().filter_map(listing_error).collect::<Vec<_>>(), ["领域服务未启动"]);

    spawn_service(Some(&7_u32), &host, "文件浏览", listing_failed, |value| {
        let value = *value;
        move || listing_failed(format!("拿到 {value}"))
    });
    assert_eq!(*host.submitted.borrow(), 1);
    assert_eq!(host.delivered.borrow().last().and_then(listing_error), Some("拿到 7"));
}

#[test]
fn panic_reasons_are_written_out() {
    let result: Result<u32, String> = settle("设置页数据", || panic!("插件宿主没有响应"), Err);
    assert_eq!(result, Err("设置页数据任务异常退出：插件宿主没有响应".into()));
    let unknown: Result<u32, String> = settle("播放器类型", || std::panic::panic_any(7_u8), Err);
    assert_eq!(unknown, Err("播放器类型任务异常退出：未知原因".into()));
}

#[test]
fn a_finished_task_keeps_its_own_result() {
    let result: Result<u32, String> = settle("设置页数据", || Ok(3), |_| unreachable!("没有 panic 不交回失败"));
    assert_eq!(result, Ok(3));
}

/// 所有派发点都经过 [`spawn`]：扫描 `src` 下的源码，直接排任务的 `.run_task(` 只能出现在这个模块里（就是宿主能力的
/// 那一处）。以后新加的派发点绕过兜底时这里会失败。
#[test]
fn every_task_goes_through_the_guard() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut guarded = 0;
    visit(&root, &mut |path, text| {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        let count = text.lines().filter(|line| line.contains(".run_task(")).count();
        match name {
            "task_guard.rs" => guarded += count,
            "task_guard_tests.rs" => {}
            _ if count > 0 => offenders.push(format!("{}：{count} 处", path.display())),
            _ => {}
        }
    });
    assert!(offenders.is_empty(), "这些文件直接排任务，没经过 task_guard::spawn：{offenders:#?}");
    assert_eq!(guarded, 1, "task_guard.rs 里只有宿主能力那一处排任务");
}

fn visit(folder: &Path, check: &mut impl FnMut(&Path, &str)) {
    for entry in std::fs::read_dir(folder).expect("读源码目录").flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, check);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let text = std::fs::read_to_string(&path).expect("读源码");
            check(&path, &text);
        }
    }
}

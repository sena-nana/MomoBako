//! 服务任务的派发和 panic 兜底。
//!
//! NanaUI 的 `RuntimeProgramContext::run_task` 把任务排进宿主的四个工作线程，线程里直接 `block_on` 任务、不接
//! panic（NanaUI `scene_host::spawn_task_workers`）：任务一旦 panic，那个工作线程就退出了，结果消息永远不会回来，
//! 等着结果的状态（首屏启动、读取中、处理中）会一直停在原地。所以所有服务任务都经 [`spawn`] / [`spawn_service`]
//! 派发：每个派发点只交代做什么、失败时交回什么，panic、任务排不进去和服务没起来都按失败交回去，状态机照常收尾。
//! 测试 `every_task_goes_through_the_guard` 扫描源码，保证没有派发点绕过这里直接排任务。

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::shell::ShellMessage;

/// 服务没起来时交回的原因。
pub(crate) const NO_SERVICES: &str = "领域服务未启动";

/// 派发服务任务要用到的宿主能力。产品里是 NanaUI 的 `RuntimeProgramContext`，测试里换成假的。
pub(crate) trait TaskHost {
    /// 把任务排进宿主的任务线程；排不进去（队列满、宿主停了）时交回原因。
    fn submit(&self, task: Task<ShellMessage>) -> Result<(), String>;
    /// 把一条消息送回归约：和任务的结果一样经事件循环到达，排在正在归约的这条消息之后。
    fn deliver(&self, message: ShellMessage);
}

impl TaskHost for RuntimeProgramContext<ShellMessage> {
    fn submit(&self, task: Task<ShellMessage>) -> Result<(), String> {
        self.run_task(task).map_err(|error| error.to_string())
    }

    fn deliver(&self, message: ShellMessage) {
        self.dispatch(message);
    }
}

/// 派发一个服务任务：`work` 在任务线程里做事、交回结果消息。`work` panic 时在任务里接住、记日志，交回
/// `failed(原因)`；任务排不进去时当场把 `failed(原因)` 送回归约。`what` 是这件事的名字，写进原因和日志。
pub(crate) fn spawn<W, F>(host: &impl TaskHost, what: &str, failed: F, work: W)
where
    W: FnOnce() -> ShellMessage + Send + 'static,
    F: Fn(String) -> ShellMessage + Clone + Send + 'static,
{
    let label = what.to_string();
    let on_panic = failed.clone();
    let task = Task::new(async move { settle(&label, work, on_panic) });
    if let Err(error) = host.submit(task) {
        let reason = format!("{what}任务提交失败：{error}");
        eprintln!("Nana {reason}");
        host.deliver(failed(reason));
    }
}

/// 派发一个要用领域服务的任务：`prepare` 在当前线程里从服务取出要用的句柄、交回在任务线程里跑的 `work`。服务没
/// 起来时不派发，当场把 `failed("领域服务未启动")` 送回归约。其余同 [`spawn`]。
pub(crate) fn spawn_service<S, P, W, F>(services: Option<&S>, host: &impl TaskHost, what: &str, failed: F, prepare: P)
where
    P: FnOnce(&S) -> W,
    W: FnOnce() -> ShellMessage + Send + 'static,
    F: Fn(String) -> ShellMessage + Clone + Send + 'static,
{
    let Some(services) = services else {
        eprintln!("Nana {what}需要领域服务，当前服务未启动");
        host.deliver(failed(NO_SERVICES.into()));
        return;
    };
    spawn(host, what, failed, prepare(services));
}

/// 在任务线程里跑 `work`。它 panic 时记下 `what` 和 panic 的原因（标准错误和系统日志），改为交回 `failed(原因)`。
///
/// `work` 拿着的服务句柄 panic 以后不再使用，随闭包一起丢掉，所以按 `AssertUnwindSafe` 接住。
pub(crate) fn settle<T>(what: &str, work: impl FnOnce() -> T, failed: impl FnOnce(String) -> T) -> T {
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(value) => value,
        Err(payload) => {
            let reason = format!("{what}任务异常退出：{}", panic_reason(payload.as_ref()));
            eprintln!("Nana {reason}");
            momobako_backend::app_log!("error", "nana.task", "taskPanicked", reason, serde_json::json!({ "task": what }));
            failed(reason)
        }
    }
}

/// panic 携带的文字：`panic!` 的格式化消息或字面量，别的类型写「未知原因」。
fn panic_reason(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    "未知原因".into()
}

#[cfg(test)]
#[path = "task_guard_tests.rs"]
pub(crate) mod tests;

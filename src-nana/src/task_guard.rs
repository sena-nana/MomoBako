//! 服务任务的 panic 兜底。
//!
//! NanaUI 的 `RuntimeProgramContext::run_task` 把任务排进宿主的四个工作线程，线程里直接 `block_on` 任务、不接
//! panic（NanaUI `scene_host::spawn_task_workers`）：任务一旦 panic，那个工作线程就退出了，结果消息永远不会回来。
//! 等着结果才往下走的状态机会一直停在原地，比如首屏启动第 4 步等设置包和播放器类型。派发层在任务里先接住
//! panic、记日志，再按调用方给的失败消息交回去，状态机照常收尾。

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// 在任务线程里跑 `work`。它 panic 时记下 `what` 和 panic 的原因，改为交回 `failed(原因)`。
///
/// `work` 拿着的服务句柄 panic 以后不再使用，随闭包一起丢掉，所以按 `AssertUnwindSafe` 接住。
pub(crate) fn settle<T>(what: &str, work: impl FnOnce() -> T, failed: impl FnOnce(String) -> T) -> T {
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(value) => value,
        Err(payload) => {
            let reason = format!("{what}任务异常退出：{}", panic_reason(payload.as_ref()));
            eprintln!("Nana {reason}");
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
mod tests {
    use super::settle;

    #[test]
    fn a_panicking_task_hands_back_the_failure_with_its_reason() {
        let result: Result<u32, String> = settle("设置页数据", || panic!("插件宿主没有响应"), Err);
        assert_eq!(result, Err("设置页数据任务异常退出：插件宿主没有响应".into()));
        let formatted: Result<u32, String> = settle("播放器类型", || panic!("第 {} 次读取", 2), Err);
        assert_eq!(formatted, Err("播放器类型任务异常退出：第 2 次读取".into()));
        let unknown: Result<u32, String> = settle("播放器类型", || std::panic::panic_any(7_u8), Err);
        assert_eq!(unknown, Err("播放器类型任务异常退出：未知原因".into()));
    }

    #[test]
    fn a_finished_task_keeps_its_own_result() {
        let result: Result<u32, String> = settle("设置页数据", || Ok(3), |_| unreachable!("没有 panic 不交回失败"));
        assert_eq!(result, Ok(3));
    }
}

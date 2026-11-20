//! 长任务的提交、观察和取消。宿主提交的是类型化协议（`services::mutsuki_runner::TaskProtocol`），
//! 拿回带类型的结果和进度事件。

use crate::services::mutsuki_runner::{MomoTaskRuntime, TaskOutput, TaskProtocol};
use std::sync::Arc;

#[derive(Clone)]
pub struct TaskService {
    runtime: Arc<MomoTaskRuntime>,
}

impl TaskService {
    pub fn new(runtime: Arc<MomoTaskRuntime>) -> Self {
        Self { runtime }
    }

    /// 提交一个内建长任务，阻塞到它结束，交回带类型的结果和它报过的进度事件。失败时交回领域的原话。
    /// 在宿主的任务线程里调用。
    pub fn run<P: TaskProtocol>(
        &self,
        request: P::Request,
    ) -> Result<TaskOutput<P::Response>, String> {
        self.runtime.run::<P>(request)
    }

    /// 返回当前可取消任务的稳定 ID 列表。
    pub fn active_task_ids(&self) -> Vec<String> {
        self.runtime.active_task_ids()
    }

    /// 返回任务中心可展示的阶段、进度与终态快照。
    pub fn progress_snapshots(&self) -> Vec<crate::services::repository::TaskProgressSnapshot> {
        self.runtime.progress_snapshots()
    }

    /// 请求取消一个正在排队或运行中的任务。
    pub fn cancel(&self, task_id: &str) -> bool {
        self.runtime.cancel(task_id)
    }
}

//! MomoBako 长任务的 Tauri 命令编排边界。

use crate::services::mutsuki_runner::MomoTaskRuntime;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct MutsukiTaskViewModel {
    runtime: Arc<MomoTaskRuntime>,
}

impl MutsukiTaskViewModel {
    pub fn new(runtime: Arc<MomoTaskRuntime>) -> Self {
        Self { runtime }
    }

    /// 将命令请求提交给 Momo 自有双 lane runtime，并保留任务产生的进度事件。
    pub async fn execute<Request>(
        &self,
        protocol_id: &'static str,
        request: Request,
    ) -> Result<(Value, Vec<Value>), String>
    where
        Request: Serialize + Send + 'static,
    {
        self.runtime.execute(protocol_id, request).await
    }

    /// 返回任务中心所需的活动和近期终态计数。
    pub fn activity_snapshot(&self) -> (usize, usize) {
        self.runtime.activity_snapshot()
    }

    /// 返回当前可取消任务的稳定 ID 列表。
    pub fn active_task_ids(&self) -> Vec<String> {
        self.runtime.active_task_ids()
    }

    /// 请求取消一个正在排队或运行中的任务。
    pub fn cancel(&self, task_id: &str) -> bool {
        self.runtime.cancel(task_id)
    }
}

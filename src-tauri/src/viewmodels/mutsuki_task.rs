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
}

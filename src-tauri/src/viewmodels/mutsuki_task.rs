//! Mutsuki 长任务的 Tauri 命令编排边界。

use crate::services::mutsuki_host;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Default)]
pub struct MutsukiTaskViewModel;

impl MutsukiTaskViewModel {
    /// 将命令请求提交给 Mutsuki，并保留任务产生的进度事件。
    pub async fn execute<Request>(
        &self,
        protocol_id: &'static str,
        request: Request,
    ) -> Result<(Value, Vec<Value>), String>
    where
        Request: Serialize + Send + 'static,
    {
        mutsuki_host::call_long_task(protocol_id, request).await
    }
}

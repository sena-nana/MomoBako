//! 资源库长任务的只读取消边界。

/// 资源库领域层只依赖该接口，不感知 Mutsuki wire DTO 或任务注册表。
pub(crate) trait CancellationCheck: Send + Sync {
    /// 返回调用方是否已经请求取消。
    fn is_cancelled(&self) -> bool;

    /// 在副作用边界前快速终止当前操作。
    fn checkpoint(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err("repository operation cancelled".to_string())
        } else {
            Ok(())
        }
    }
}

/// 非长任务调用使用的永不取消检查器。
pub(crate) struct NeverCancelled;

impl CancellationCheck for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
}

//! 宿主无关的应用事件边界，供原生 UI 与迁移期 Tauri 适配器共同订阅。

use crate::services::repository::{RepositoryStructureUpdatedEvent, SystemLogRecord};
use std::sync::{mpsc, Arc};

/// 应用服务产生的事件；宿主负责将其送入自己的 UI 事件循环。
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum HostEvent {
    LogRecorded(SystemLogRecord),
    RepositoryStructureUpdated(RepositoryStructureUpdatedEvent),
}

impl HostEvent {
    /// 保留已有前端事件名称，避免迁移期监听协议变化。
    pub fn name(&self) -> &'static str {
        match self {
            Self::LogRecorded(_) => "system://log-recorded",
            Self::RepositoryStructureUpdated(_) => "repository://structure-updated",
        }
    }

    /// 返回原事件负载，不添加 enum 标签或额外 envelope。
    pub fn payload(&self) -> Result<serde_json::Value, String> {
        match self {
            Self::LogRecorded(record) => serde_json::to_value(record),
            Self::RepositoryStructureUpdated(event) => serde_json::to_value(event),
        }
        .map_err(|error| error.to_string())
    }
}

/// 跨线程事件接收边界，不依赖窗口句柄或 UI 框架。
pub trait HostEventSink: Send + Sync {
    fn emit(&self, event: HostEvent) -> Result<(), String>;
}

impl<F> HostEventSink for F
where
    F: Fn(HostEvent) -> Result<(), String> + Send + Sync,
{
    fn emit(&self, event: HostEvent) -> Result<(), String> {
        self(event)
    }
}

/// 创建 FIFO 通道；原生宿主在自身事件循环中消费 receiver。
#[allow(dead_code)]
pub fn host_event_channel() -> (Arc<dyn HostEventSink>, mpsc::Receiver<HostEvent>) {
    let (sender, receiver) = mpsc::channel();
    let sink = move |event| sender.send(event).map_err(|error| error.to_string());
    (Arc::new(sink), receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure_event(repo_id: &str) -> HostEvent {
        HostEvent::RepositoryStructureUpdated(RepositoryStructureUpdatedEvent {
            repo_id: repo_id.to_string(),
            reason: "watcher".to_string(),
            indexed_at: None,
        })
    }

    #[test]
    fn channel_preserves_event_order_and_existing_payload() {
        let (sink, receiver) = host_event_channel();
        sink.emit(structure_event("first")).unwrap();
        sink.emit(structure_event("second")).unwrap();
        let first = receiver.recv().unwrap();
        assert_eq!(first.name(), "repository://structure-updated");
        assert_eq!(
            first.payload().unwrap(),
            serde_json::json!({
                "repoId": "first",
                "reason": "watcher",
                "indexedAt": null,
            })
        );
        assert_eq!(
            receiver.recv().unwrap().payload().unwrap()["repoId"],
            "second"
        );
    }

    #[test]
    fn disconnected_channel_reports_delivery_failure() {
        let (sink, receiver) = host_event_channel();
        drop(receiver);
        assert!(sink.emit(structure_event("repo")).is_err());
    }
}

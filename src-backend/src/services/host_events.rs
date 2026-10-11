//! 宿主无关的应用事件边界：领域服务把事件交给宿主，宿主送进自己的 UI 事件循环。

use crate::services::repository::{RepositoryStructureUpdatedEvent, SystemLogRecord};
use std::sync::{mpsc, Arc};

/// 应用服务产生的事件；宿主负责将其送入自己的 UI 事件循环。
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum HostEvent {
    LogRecorded(SystemLogRecord),
    RepositoryStructureUpdated(RepositoryStructureUpdatedEvent),
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

    /// 取结构更新事件的资源库 id；别的事件交回 None。
    fn structure_repo_id(event: HostEvent) -> Option<String> {
        match event {
            HostEvent::RepositoryStructureUpdated(event) => Some(event.repo_id),
            HostEvent::LogRecorded(_) => None,
        }
    }

    #[test]
    fn channel_preserves_event_order() {
        let (sink, receiver) = host_event_channel();
        sink.emit(structure_event("first")).unwrap();
        sink.emit(structure_event("second")).unwrap();
        assert_eq!(structure_repo_id(receiver.recv().unwrap()).as_deref(), Some("first"));
        assert_eq!(structure_repo_id(receiver.recv().unwrap()).as_deref(), Some("second"));
    }

    #[test]
    fn disconnected_channel_reports_delivery_failure() {
        let (sink, receiver) = host_event_channel();
        drop(receiver);
        assert!(sink.emit(structure_event("repo")).is_err());
    }
}

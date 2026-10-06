//! MomoBako 宿主无关领域服务库。
//!
//! models、services 与 ViewModel 住在这个 crate。Tauri 窗口壳和 Nana 宿主都依赖这里，
//! 不把窗口、WebView 或 Tauri command 编进领域代码。

pub mod models;
pub mod services;
pub mod viewmodels;

#[cfg(test)]
mod tests;

pub use services::host_events::{host_event_channel, HostEvent, HostEventSink};
pub use services::repository::eagle_import::source_adapter::{
    build_eagle_source_snapshot, EagleSourceDiscoveredFile, EagleSourceEntry, EagleSourceEntryKind,
    EagleSourceSnapshot,
};
pub use services::runtime::RepositoryRuntime;

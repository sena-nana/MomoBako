//! MomoBako 宿主无关领域服务库。
//!
//! 迁移期间 Tauri 和 Nana 宿主共享这里导出的同一组 models、services 与
//! ViewModel 源码。该 crate 不依赖窗口、WebView、`AppHandle` 或 Tauri command。

#[path = "../../src-tauri/src/models/mod.rs"]
pub mod models;
#[path = "../../src-tauri/src/services/mod.rs"]
pub mod services;
#[path = "../../src-tauri/src/viewmodels/mod.rs"]
pub mod viewmodels;

pub use services::host_events::{host_event_channel, HostEvent, HostEventSink};
pub use services::runtime::RepositoryRuntime;

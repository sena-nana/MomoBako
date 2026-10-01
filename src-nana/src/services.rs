//! Nana 宿主使用的共享应用服务组合。
//!
//! 每个 ViewModel 都由同一个 `RepositoryRuntime` 构造，保证读写锁、事件 sink、
//! 预览服务和任务 runner 只有一份所有权；页面层只持有这个组合，不直接接触 Tauri。

use std::sync::Arc;

use crate::backend::RepositoryRuntime;
use crate::backend::services::mutsuki_runner::MomoTaskRuntime;
use crate::backend::viewmodels::{
    FileBrowserViewModel, MutsukiTaskViewModel, PluginViewModel, RepositoryInteractionViewModel,
    RepositoryManagementViewModel, RepositoryQueryViewModel, SystemViewModel,
};

/// Nana Runtime 的领域服务依赖集合。
pub struct NativeServices {
    /// 领域服务依赖 Tokio；Nana 自身的 pollster 任务线程不提供 reactor。
    pub executor: Arc<tokio::runtime::Runtime>,
    pub runtime: RepositoryRuntime,
    pub repository_query: RepositoryQueryViewModel,
    pub file_browser: FileBrowserViewModel,
    pub repository_interaction: RepositoryInteractionViewModel,
    pub repository_management: RepositoryManagementViewModel,
    pub plugin: PluginViewModel,
    pub system: SystemViewModel,
    pub tasks: MutsukiTaskViewModel,
}

impl NativeServices {
    /// 启动一次领域 Runtime，并将所有页面 ViewModel 绑定到同一实例。
    pub fn start() -> Result<Self, String> {
        let executor = Arc::new(tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("领域异步执行器启动失败：{error}"))?);
        let runtime = RepositoryRuntime::start()?;
        let task_runtime = Arc::new(MomoTaskRuntime::new(runtime.clone()));
        Ok(Self {
            executor,
            repository_query: RepositoryQueryViewModel::new(runtime.clone()),
            file_browser: FileBrowserViewModel::new(runtime.clone()),
            repository_interaction: RepositoryInteractionViewModel::new(runtime.clone()),
            repository_management: RepositoryManagementViewModel::new(runtime.clone()),
            plugin: PluginViewModel::new(runtime.clone()),
            system: SystemViewModel::new(runtime.clone()),
            tasks: MutsukiTaskViewModel::new(task_runtime),
            runtime,
        })
    }
}

impl Drop for NativeServices {
    fn drop(&mut self) {
        self.runtime.shutdown_helpers();
    }
}

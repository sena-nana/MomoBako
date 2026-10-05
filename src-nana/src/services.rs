//! Nana 宿主使用的共享应用服务组合。
//!
//! 每个 ViewModel 都由同一个 `RepositoryRuntime` 构造，保证读写锁、事件 sink、
//! 预览服务和任务 runner 只有一份所有权；页面层只持有这个组合，不直接接触 Tauri。

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::backend::RepositoryRuntime;
use crate::backend::services::mutsuki_runner::MomoTaskRuntime;
use crate::backend::viewmodels::{
    FileBrowserViewModel, MutsukiTaskViewModel, PluginViewModel, RepositoryInteractionViewModel,
    RepositoryManagementViewModel, RepositoryQueryViewModel, SystemViewModel,
};
use crate::settings::{self, ApplicationSettings, SettingsStore};

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
    pub settings: SettingsStore,
    host_events: Mutex<Option<mpsc::Receiver<crate::backend::services::host_events::HostEvent>>>,
}

impl NativeServices {
    /// 启动一次领域 Runtime，并将所有页面 ViewModel 绑定到同一实例。
    pub fn start() -> Result<Self, String> {
        let executor = Arc::new(tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("领域异步执行器启动失败：{error}"))?);
        let runtime = RepositoryRuntime::start()?;
        let (sink, receiver) = crate::backend::services::host_events::host_event_channel();
        if let Err(error) = runtime.set_event_sink(Arc::clone(&sink)) {
            eprintln!("Nana 资源库结构事件绑定失败：{error}");
        }
        match crate::backend::services::logging::init_app_logger(runtime.service_root()) {
            Ok(logger) => {
                if let Err(error) = logger.set_event_sink(sink) {
                    eprintln!("Nana 日志事件绑定失败：{error}");
                }
            }
            Err(error) => eprintln!("Nana 应用日志器启动失败：{error}"),
        }
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
            settings: SettingsStore::new(settings::default_path()),
            runtime,
            host_events: Mutex::new(Some(receiver)),
        })
    }

    /// 在独立线程里把宿主事件交给界面。通道只消费一次。
    pub fn pump_host_events<F>(&self, dispatch: F)
    where
        F: Fn(crate::backend::services::host_events::HostEvent) + Send + 'static,
    {
        let Some(receiver) = self.take_host_events() else {
            return;
        };
        if let Err(error) = std::thread::Builder::new().name("nana-host-events".into()).spawn(move || {
            while let Ok(event) = receiver.recv() {
                dispatch(event);
            }
            eprintln!("Nana 宿主事件通道已关闭");
        }) {
            eprintln!("Nana 宿主事件线程启动失败：{error}");
        }
    }

    /// 取出宿主事件接收端。通道只交给一个消费线程。
    pub fn take_host_events(&self) -> Option<mpsc::Receiver<crate::backend::services::host_events::HostEvent>> {
        match self.host_events.lock() {
            Ok(mut receiver) => receiver.take(),
            Err(error) => {
                eprintln!("Nana 宿主事件通道锁失效：{error}");
                None
            }
        }
    }

    pub fn load_settings(&self) -> Result<(ApplicationSettings, Option<String>), String> {
        self.settings.load_or_recover()
    }

    pub fn save_settings(&self, settings: &ApplicationSettings) -> Result<ApplicationSettings, String> {
        settings.validate()?;
        self.settings.save(settings)?;
        Ok(settings.clone())
    }
}

impl Drop for NativeServices {
    fn drop(&mut self) {
        self.runtime.shutdown_helpers();
    }
}

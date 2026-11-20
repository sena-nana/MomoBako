//! 应用服务层：宿主（Nana）在进程内调用的同步接口。每个服务包一层 `RepositoryRuntime` 的读写边界
//! （读直接做，写在写锁后面，改资源库集合时重新同步文件监听），插件配置改动后切换插件 generation。

pub mod plugin;
pub mod repository;
pub mod system;
pub mod task;

pub use plugin::PluginService;
pub use repository::{
    FileBrowserService, RepositoryInteractionService, RepositoryManagementService,
    RepositoryQueryService,
};
pub use system::SystemService;
pub use task::TaskService;

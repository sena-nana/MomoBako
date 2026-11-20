//! 资源库的应用服务：查询、文件浏览、交互（播放集、智能文件夹、动作、访问记录）和管理。

mod browser;
mod interaction;
mod management;
mod query;

pub use browser::FileBrowserService;
pub use interaction::RepositoryInteractionService;
pub use management::RepositoryManagementService;
pub use query::RepositoryQueryService;

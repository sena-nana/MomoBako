//! MomoBako 的 NanaUI 原生宿主骨架。
//!
//! 该 crate 只负责窗口、Runtime 文档和 Nana 控件树；资源库领域服务在
//! `src-tauri` 的宿主无关服务层收口后由应用状态注入。离屏测试复用同一棵
//! `RuntimeDocument`，不创建第二套 UI 树或 GPU 设备。

use nana_ui::runtime::{DocumentId, FrameworkError, Task};
use nana_ui::{
    ApplicationIdentity, ApplicationState, ApplicationWindow, DiagnosticsConfig, NanaApplication,
    RuntimeApplication, RuntimeProgramContext, RuntimeProgramUpdate, WindowDescriptor,
};

pub mod shell;
use shell::{ShellMessage, ShellPage, ShellViewModel, mount_shell};

pub mod host_api;
pub mod services;

/// Nana 宿主直接使用共享领域服务 crate，迁移期 Tauri 仍保留同一服务源码的
/// 适配入口；此 re-export 让后续 ViewModel 接线不需要再穿过 command 层。
pub use momobako_backend as backend;
pub mod plugin_api;

/// Nana 宿主应用状态，持有共享领域 Runtime 和原生壳层 ViewModel。
pub struct MomoBakoApplication {
    pub services: Option<services::NativeServices>,
    shell: ShellViewModel,
    repositories_load_scheduled: bool,
}

impl Default for MomoBakoApplication {
    fn default() -> Self {
        Self {
            services: None,
            shell: ShellViewModel::default(),
            repositories_load_scheduled: false,
        }
    }
}

impl ApplicationState for MomoBakoApplication {
    type Message = ShellMessage;
    type Error = FrameworkError;

    fn initialize(_: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        let services = match services::NativeServices::start() {
            Ok(services) => Some(services),
            Err(error) => {
                eprintln!("Nana 领域 Runtime 启动失败：{error}");
                None
            }
        };
        let shell = if services.is_some() {
            ShellViewModel::default()
        } else {
            let mut shell = ShellViewModel::for_page(ShellPage::Error);
            shell.detail = "领域服务启动失败，请检查服务目录和端口配置".into();
            shell
        };
        Ok(Self {
            services,
            shell,
            repositories_load_scheduled: false,
        })
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        mount_shell(&mut window.document, &self.shell)?;
        if !self.repositories_load_scheduled {
            if let Some(services) = self.services.as_ref() {
                let query = services.repository_query.clone();
                let executor = services.executor.clone();
                if let Err(error) = context.run_task(Task::new(async move {
                    ShellMessage::RepositoriesLoaded(executor.block_on(query.list_repositories()))
                })) {
                    eprintln!("Nana 资源库加载任务提交失败：{error}");
                }
                self.repositories_load_scheduled = true;
            }
        }
        Ok(())
    }

    fn update(
        &mut self,
        message: ShellMessage,
        windows: &mut std::collections::HashMap<nana_ui_platform::WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        let Some((id, window)) = windows.iter_mut().next() else {
            return RuntimeProgramUpdate::default();
        };
        if let ShellMessage::RepositoriesLoaded(Ok(repositories)) = &message
            && let Some(repository) = repositories.first()
            && let Some(services) = self.services.as_ref()
        {
            let query = services.repository_query.clone();
            let executor = services.executor.clone();
            let repo_id = repository.repo_id.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::RepositorySnapshotLoaded(
                    executor.block_on(query.get_repository_snapshot(repo_id)),
                )
            })) {
                eprintln!("Nana 资源库快照任务提交失败：{error}");
            }
        }
        if let ShellMessage::RepositorySnapshotLoaded(Ok(snapshot)) = &message
            && let Some(services) = self.services.as_ref()
        {
            let browser = services.file_browser.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::FileBrowserRequest {
                repo_id: snapshot.repository.repo_id.clone(),
                directory_path: None,
                include_tree: Some(false),
                special_location: None,
                offset: Some(0),
                limit: Some(200),
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::FileBrowserLoaded(executor.block_on(browser.get_file_browser(request)))
            })) {
                eprintln!("Nana 文件浏览任务提交失败：{error}");
            }
        }
        if let ShellMessage::SelectFile {
            asset_id: Some(asset_id),
            ..
        } = &message
            && let Some(repository_id) = self.shell.repository_id.clone()
            && let Some(services) = self.services.as_ref()
        {
            let query = services.repository_query.clone();
            let executor = services.executor.clone();
            let asset_id = asset_id.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::AssetDetailLoaded(
                    executor.block_on(query.get_asset_detail(repository_id, asset_id)),
                )
            })) {
                eprintln!("Nana 文件元数据任务提交失败：{error}");
            }
        }
        if matches!(&message, ShellMessage::PrimaryAction)
            && self.shell.page == ShellPage::SelectedFile
            && let (Some(repository_id), Some(path), Some(services)) = (
                self.shell.repository_id.clone(),
                self.shell.selected_path.clone(),
                self.services.as_ref(),
            )
        {
            let query = services.repository_query.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::FileReadRequest { repo_id: repository_id, path };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PreviewSourceLoaded(
                    executor.block_on(query.prepare_preview_file_source(request)),
                )
            })) {
                eprintln!("Nana 预览源任务提交失败：{error}");
            }
        }
        self.shell.reduce(message);
        if let Err(error) = mount_shell(&mut window.document, &self.shell) {
            eprintln!("Nana 壳层重建失败：{error}");
            self.shell.page = ShellPage::Error;
            self.shell.detail = "页面更新失败，请查看系统日志".into();
            return RuntimeProgramUpdate::redraw(*id);
        }
        RuntimeProgramUpdate::redraw(*id)
    }
}

/// 启动原生 NanaUI 窗口。
pub fn run() -> Result<(), nana_ui::HostedRunError> {
    let identity = ApplicationIdentity::new(
        "com.momobako.desktop",
        "MomoBako",
        env!("CARGO_PKG_VERSION"),
    );
    NanaApplication::builder(identity)
        .diagnostics(DiagnosticsConfig::default())
        .run::<RuntimeApplication<MomoBakoApplication>>(
            WindowDescriptor::new("MomoBako").initial_size(1200.0, 800.0),
        )
}

/// 为离屏验收创建与生产宿主相同的 Runtime 文档。
pub fn acceptance_document() -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    acceptance_document_for(ShellPage::Loading)
}

/// 为离屏验收创建指定页面状态的生产 Runtime 文档。
pub fn acceptance_document_for(
    page: ShellPage,
) -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    let document_id = DocumentId::new(1).expect("document id 1 is valid");
    let mut document = nana_ui::runtime::RuntimeDocument::new(document_id);
    mount_shell(&mut document, &ShellViewModel::for_page(page))?;
    Ok(document)
}

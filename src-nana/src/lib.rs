//! MomoBako 的 NanaUI 原生宿主骨架。
//!
//! 该 crate 只负责窗口、Runtime 文档和 Nana 控件树；资源库领域服务在
//! `src-tauri` 的宿主无关服务层收口后由应用状态注入。离屏测试复用同一棵
//! `RuntimeDocument`，不创建第二套 UI 树或 GPU 设备。

use nana_ui::runtime::{DocumentId, FrameworkError};
use nana_ui::{
    ApplicationIdentity, ApplicationState, ApplicationWindow, DiagnosticsConfig, NanaApplication,
    RuntimeApplication, RuntimeProgramContext, RuntimeProgramUpdate, WindowDescriptor,
};

pub mod shell;
use shell::{ShellPage, ShellViewModel, mount_shell};

pub mod host_api;
pub mod plugin_api;

/// Nana 宿主的最小应用状态；后续阶段将把 repository/plugin/task ViewModel 注入这里。
#[derive(Default)]
pub struct MomoBakoApplication;

impl ApplicationState for MomoBakoApplication {
    type Message = ();
    type Error = FrameworkError;

    fn initialize(_: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        Ok(Self)
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        mount_shell(&mut window.document, &ShellViewModel::default())
    }

    fn update(
        &mut self,
        _id: (),
        _windows: &mut std::collections::HashMap<nana_ui_platform::WindowId, ApplicationWindow>,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        RuntimeProgramUpdate::default()
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

//! 离屏验收用的 Runtime 文档：和产品窗口走同一个 `mount_shell`，只给测试和验收场景用（`acceptance` feature）。

use nana_ui::runtime::{DocumentId, FrameworkError};

use crate::shell::{ShellPage, ShellViewModel, mount_shell};

/// 为离屏验收创建与生产宿主相同的 Runtime 文档。
pub fn acceptance_document() -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    acceptance_document_for(ShellPage::Loading)
}

/// 为离屏验收创建指定页面状态的生产 Runtime 文档。
pub fn acceptance_document_for(
    page: ShellPage,
) -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    acceptance_document_for_model(ShellViewModel::for_page(page))
}

/// 按离屏视口宽度挂载验收文档。不超过窄屏断点的视口走窄屏排布。
pub fn acceptance_document_at_width(
    mut model: ShellViewModel,
    width: f32,
) -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    model.set_viewport_width(width);
    acceptance_document_for_model(model)
}

/// 为离屏验收挂载指定 ViewModel；仍然复用生产壳层挂载函数和同一棵 Runtime 树。
pub fn acceptance_document_for_model(
    model: ShellViewModel,
) -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    let document_id = DocumentId::new(1).expect("document id 1 is valid");
    let mut document = nana_ui::runtime::RuntimeDocument::new(document_id);
    mount_shell(&mut document, &model)?;
    Ok(document)
}

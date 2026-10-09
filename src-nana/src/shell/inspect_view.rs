//! 检视面：搜索结果和文件预览页。元数据编辑在 `inspect_metadata_view`，搜索和筛选在 `inspect_search_view`。
//!
//! 预览页的外框在 `inspect_preview_frame`，预览框里的内容在 `inspect_preview_body`，
//! 音频舞台在 `inspect_audio_stage`，PDF 页纸在 `inspect_preview_page`。播放、暂停、跳转和音量
//! 只在底部播放条上，预览页里不另放一套传输控件。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{LengthSpec, Stack};

use super::ShellViewModel;

#[path = "inspect_preview_frame.rs"]
mod frame;
#[path = "inspect_preview_body.rs"]
mod body;
#[path = "inspect_audio_stage.rs"]
mod audio_stage;
#[path = "inspect_preview_page.rs"]
mod page;
#[path = "preview_paint.rs"]
mod preview_paint;

/// 检视面：搜索面板时只有搜索，已选文件时是文件预览页。
pub(super) fn inspect_surface(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    // 搜索面板独占主体，和 Vue 的 `SearchPanel` 一样不和预览叠在一起。筛选栏由壳层放置。
    if model.workspace.panel == super::workspace::WorkspacePanel::Search {
        return super::inspect_search_view::search_panel(model);
    }
    let mut rows = Vec::new();
    if inspect.has_target() {
        rows.push(frame::preview_page(model, body::preview_body(model)));
    }
    widget(
        Stack::fill_column(8.0)
            .min_height(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0)
            .with_layout(|layout| {
                layout.flex_basis = Some(LengthSpec::Px(0.0));
            }),
    )
    .children(rows)
    .key("inspect-surface")
    .into_any()
}

//! 检视面：搜索结果和文件预览页。元数据编辑在 `inspect_metadata_view`，搜索和筛选在 `inspect_search_view`。
//!
//! 预览页的外框在 `inspect_preview_frame`，预览框里的内容在 `inspect_preview_body`，
//! 音频舞台在 `inspect_audio_stage`，PDF 页纸在 `inspect_preview_page`。播放、暂停、跳转和音量
//! 只在底部播放条上，预览页里不另放一套传输控件。
//!
//! 文件预览页常驻在文件路由里（[`preview_page`]），读 [`PreviewSignals`]；搜索路由仍由旧视图函数
//! [`inspect_surface`] 整块建出。

use nana_ui::runtime::view::{widget, AnyView, IntoView, NodeRef};
use nana_ui::runtime::{LengthSpec, Stack};

use super::inspect_metadata_view::MetadataSignals;

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

pub(crate) use frame::PreviewSignals;

/// 文件预览页：预览框架铺满文件列，播放条由框架贴在页底（Vue `files-preview-page` 里的
/// `WorkspacePlayerBar`）。`bar` 是播放条的占位节点。
pub(super) fn preview_page(signals: PreviewSignals, metadata: MetadataSignals, bar: NodeRef) -> AnyView {
    surface(vec![frame::preview_page(signals, metadata, bar)])
}

/// 检视面的外框：占满剩余高度，行距 8。
fn surface(rows: Vec<AnyView>) -> AnyView {
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

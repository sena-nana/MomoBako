//! 文件预览页。预览页的外框在 `inspect_preview_frame`，预览框里的内容在 `inspect_preview_body`，
//! 音频舞台在 `inspect_audio_stage`，PDF 页纸在 `inspect_preview_page`；元数据编辑在
//! `inspect_metadata_view`，搜索结果和筛选在 `inspect_search_view`（搜索路由见 `route_search.rs`）。
//! 播放、暂停、跳转和音量只在底部播放条上，预览页里不另放一套传输控件。
//!
//! 文件预览页常驻在文件路由里（[`preview_page`]），读 [`PreviewSignals`]。宿主按预览框这一帧画不画
//! 纹理槽决定上不上传预览画面（[`wants_preview_texture`]）。

use nana_ui::runtime::view::{AnyView, IntoView, NodeRef, widget};
use nana_ui::runtime::{LengthSpec, Stack};

use super::inspect_metadata_view::MetadataSignals;

mod frame;
#[cfg(test)]
pub(crate) use frame::preview_open_target;
mod audio_stage;
mod body;
mod page;
mod preview_paint;

pub(crate) use audio_stage::{LyricFollow, follow_active_lyric};
pub(crate) use body::wants_preview_texture;
pub(crate) use frame::{PreviewSignals, seed_detail_card_editor, seed_preview_facts};

/// 文件预览页：预览框架铺满文件列，播放条由框架贴在页底。`bar` 是播放条的占位节点。
pub(super) fn preview_page(
    signals: PreviewSignals,
    metadata: MetadataSignals,
    bar: NodeRef,
) -> AnyView {
    surface(vec![frame::preview_page(signals, metadata, bar)])
}

/// 预览页的外框：占满剩余高度，行距 `Md`。
fn surface(rows: Vec<AnyView>) -> AnyView {
    widget(
        Stack::fill_column(0.0)
            .gap_theme(nana_ui_core::SpacingStep::Md)
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

//! `SelectableRichText` 的语义色 overlay。
//!
//! 片段仍拼成一段文本、一个绘制对象。只有 `color` 有值的片段按 UTF-8 字节
//! 写成 `TextSpan`，相邻同角色合并，交给已有的 `HighlightRequest::with_overlay`。
//! 这个 presenter 名字不会注册；未注册时基础层为空，overlay 单独进入抽取和 GPU。

use std::sync::Arc;

use crate::{HighlightRequest, RichSpan, TextSpan};

/// 不注册的 presenter 名。避开 `"highlight"`，这样不会把 syntect 接进富文本。
pub(crate) const RICH_SPAN_COLOR_PRESENTER: &str = "rich-span-color";

/// 把带色片段收成一条 overlay 请求。没有带色片段时返回 `None`，节点保持正文色。
pub(crate) fn color_request(spans: &[RichSpan]) -> Option<HighlightRequest> {
    let mut overlay: Vec<TextSpan> = Vec::new();
    let mut offset = 0usize;
    for span in spans {
        let end = offset.saturating_add(span.text.len());
        if let Some(color) = span.color.filter(|_| offset < end) {
            match overlay.last_mut() {
                Some(last) if last.end == offset && last.color == color => last.end = end,
                _ => overlay.push(TextSpan {
                    start: offset,
                    end,
                    color,
                }),
            }
        }
        offset = end;
    }
    if overlay.is_empty() {
        None
    } else {
        Some(
            HighlightRequest::new(RICH_SPAN_COLOR_PRESENTER, "").with_overlay(Arc::from(overlay)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui_core::SemanticColorRole;

    use crate::{
        ComponentView, DocumentId, MutationQueue, NodeKind, StableNodeId, StandardVisual, UiWorld,
    };

    fn id() -> StableNodeId {
        StableNodeId::new(1).unwrap()
    }

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    /// 一段 Keyword、一段无色，再接一段相邻 Keyword：投影后只有一条文本，
    /// 字节偏移正确，相邻同色合并。没有 presenter 时 overlay 仍进入抽取。
    #[test]
    fn colored_spans_project_one_text_and_merge_adjacent_roles() {
        let view = crate::SelectableRichText::new([
            RichSpan::plain("fn").role(SemanticColorRole::Keyword),
            RichSpan::plain("你").role(SemanticColorRole::Keyword),
            RichSpan::plain(" x"),
            RichSpan::plain("pub").role(SemanticColorRole::Keyword),
        ]);
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        queue.create(id(), document(), NodeKind::Text);
        world.commit(queue).unwrap();
        let mut mutations = MutationQueue::new();
        view.project(id(), &world, &mut mutations);
        world.commit(mutations).unwrap();

        let plain = "fn你 xpub";
        assert_eq!(world.text(id()), Some(plain));
        assert_eq!(world.document_order(document()).len(), 1);
        assert_eq!(
            world.standard_visual(id()),
            Some(StandardVisual::SelectableRichText {
                text: Arc::from(plain),
                selection: None,
            })
        );
        assert!(!world.has_presenter(RICH_SPAN_COLOR_PRESENTER));
        assert!(!world.has_presenter(crate::HIGHLIGHT_PRESENTER));
        let overlay = world
            .highlight_request(id())
            .expect("overlay request")
            .overlay
            .clone();
        assert_eq!(
            world.highlight_request(id()).unwrap().presenter.as_ref(),
            RICH_SPAN_COLOR_PRESENTER
        );
        assert_eq!(
            overlay.as_deref(),
            Some(
                [
                    TextSpan {
                        start: 0,
                        end: "fn你".len(),
                        color: SemanticColorRole::Keyword,
                    },
                    TextSpan {
                        start: "fn你 x".len(),
                        end: plain.len(),
                        color: SemanticColorRole::Keyword,
                    },
                ]
                .as_slice()
            )
        );

        world.resolve_presentations(&[id()]).unwrap();
        let presentation = world.text_presentation(id()).expect("presentation");
        assert_eq!(presentation.spans, overlay.as_deref().unwrap());
        let extracted = world.extract_nodes(&[id()]);
        assert_eq!(extracted.len(), 1);
        assert_eq!(
            extracted[0]
                .text_spans
                .iter()
                .map(|span| (span.start, span.end))
                .collect::<Vec<_>>(),
            vec![(0, "fn你".len()), ("fn你 x".len(), plain.len())]
        );
    }
}

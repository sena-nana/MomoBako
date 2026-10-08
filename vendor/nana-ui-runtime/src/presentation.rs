//! Registered text presentation.
//!
//! Intent ([`HighlightRequest`]) and derived spans ([`TextPresentation`]) are
//! retained node fields. Algorithms live on [`UiWorld`] as named
//! [`TextPresenter`] values so Vue flush and `AppContext` share one registry.
//! Presenters color committed text only; IME preedit stays solid.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui_core::SemanticColorRole;

/// Built-in presenter name for syntax highlighting.
pub const HIGHLIGHT_PRESENTER: &str = "highlight";

/// One committed-text range painted with a theme role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextSpan {
    pub start: usize,
    pub end: usize,
    pub color: SemanticColorRole,
}

/// Application intent: which presenter should color this node's committed text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HighlightRequest {
    pub presenter: Arc<str>,
    pub language: Arc<str>,
    /// 宿主喂入的语义 overlay 段（值空间，与基础层同文本；框架只渲染，
    /// 不解释内容）。合并时 overlay 段优先，基础层与 overlay 重叠处丢弃
    /// 基础层；`None` 与空表等价于无 overlay（纯基础层）。内容哈希纳入
    /// `presentation_source` 缓存键。
    pub overlay: Option<Arc<[TextSpan]>>,
}

impl HighlightRequest {
    pub fn new(presenter: impl Into<Arc<str>>, language: impl Into<Arc<str>>) -> Self {
        Self {
            presenter: presenter.into(),
            language: language.into(),
            overlay: None,
        }
    }

    /// Request the built-in `"highlight"` presenter.
    pub fn highlight(language: impl Into<Arc<str>>) -> Self {
        Self::new(HIGHLIGHT_PRESENTER, language)
    }

    /// 附带语义 overlay 段（值空间）。撤除 overlay 时重建不带 overlay 的
    /// 请求（缓存键随内容变化，下一次 resolve 回到纯基础层）。
    pub fn with_overlay(mut self, overlay: Arc<[TextSpan]>) -> Self {
        self.overlay = Some(overlay);
        self
    }
}

/// Derived committed-text spans. Recomputed only when the source hash changes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextPresentation {
    pub spans: Vec<TextSpan>,
    pub source: u64,
}

/// World-level algorithm that turns committed text into semantic spans.
pub trait TextPresenter: Send + 'static {
    fn name(&self) -> &'static str;
    fn present(&self, text: &str, request: &HighlightRequest) -> Vec<TextSpan>;
}

pub(crate) fn presentation_source(text: &crate::TextValue, request: &HighlightRequest) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    request.presenter.hash(&mut hasher);
    request.language.hash(&mut hasher);
    // overlay 内容参与缓存键：宿主换喂语义段（含撤除）即失效重算。
    request.overlay.hash(&mut hasher);
    // An editor's text names its bytes by stamp: a caret move keys the same
    // source without hashing the document (Issue #182). Unstamped text —
    // a label — is hashed as before. Equal bytes under a new stamp only miss
    // the cache; they never hit a wrong entry.
    match text.stamp() {
        Some(stamp) => {
            0u8.hash(&mut hasher);
            stamp.hash(&mut hasher);
        }
        None => {
            1u8.hash(&mut hasher);
            text.as_str().hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// 合并语义 overlay 段：overlay 优先，基础层与 overlay 重叠处被切分丢弃。
/// 合并发生在 presenter 结果之后、`sanitize_spans` 之前；输出交由 sanitize
/// 排序、校验与相邻同色合并。
pub(crate) fn merge_overlay_spans(base: Vec<TextSpan>, overlay: &[TextSpan]) -> Vec<TextSpan> {
    if overlay.is_empty() {
        return base;
    }
    let mut regions: Vec<(usize, usize)> = overlay
        .iter()
        .map(|span| (span.start, span.end))
        .filter(|(start, end)| start < end)
        .collect();
    regions.sort_unstable();
    let mut merged = Vec::with_capacity(base.len() + overlay.len());
    for span in base {
        let mut cursor = span.start;
        for &(start, end) in &regions {
            if end <= cursor || start >= span.end {
                continue;
            }
            if start > cursor {
                merged.push(TextSpan {
                    start: cursor,
                    end: start.min(span.end),
                    color: span.color,
                });
            }
            cursor = cursor.max(end).min(span.end);
            if cursor >= span.end {
                break;
            }
        }
        if cursor < span.end {
            merged.push(TextSpan {
                start: cursor,
                end: span.end,
                color: span.color,
            });
        }
    }
    merged.extend_from_slice(overlay);
    merged
}

pub(crate) fn sanitize_spans(text: &str, spans: Vec<TextSpan>) -> Vec<TextSpan> {
    let mut cleaned = spans
        .into_iter()
        .filter(|span| {
            span.start < span.end
                && span.end <= text.len()
                && text.is_char_boundary(span.start)
                && text.is_char_boundary(span.end)
        })
        .collect::<Vec<_>>();
    cleaned.sort_by_key(|span| (span.start, span.end));
    let mut merged: Vec<TextSpan> = Vec::new();
    let mut cursor = 0usize;
    for mut span in cleaned {
        if span.start < cursor {
            span.start = cursor;
        }
        if span.start >= span.end {
            continue;
        }
        if let Some(last) = merged.last_mut()
            && last.end == span.start
            && last.color == span.color
        {
            last.end = span.end;
            cursor = span.end;
            continue;
        }
        cursor = span.end;
        merged.push(span);
    }
    if merged.len() == 1
        && merged[0].color == SemanticColorRole::Text
        && merged[0].start == 0
        && merged[0].end == text.len()
    {
        return Vec::new();
    }
    merged
}

/// First registered presenter: syntect scope names mapped onto theme roles.
#[cfg(feature = "syntax-highlighting")]
#[derive(Debug, Default)]
pub struct SyntectHighlighter;

#[cfg(feature = "syntax-highlighting")]
impl TextPresenter for SyntectHighlighter {
    fn name(&self) -> &'static str {
        HIGHLIGHT_PRESENTER
    }

    fn present(&self, text: &str, request: &HighlightRequest) -> Vec<TextSpan> {
        syntect_present(text, request.language.as_ref())
    }
}

/// Installs [`SyntectHighlighter`] as the `"highlight"` presenter.
#[cfg(feature = "syntax-highlighting")]
pub struct HighlightPresentation;

#[cfg(feature = "syntax-highlighting")]
impl crate::framework::UiExtension for HighlightPresentation {
    fn name(&self) -> &'static str {
        "nana.highlight"
    }

    fn install(
        &self,
        registrar: &mut crate::framework::ExtensionRegistrar,
    ) -> Result<(), crate::framework::FrameworkError> {
        registrar.register_presenter(Box::new(CachedSyntectHighlighter::default()))
    }
}

#[cfg(feature = "syntax-highlighting")]
#[derive(Default)]
struct CachedSyntectHighlighter {
    cache: std::cell::RefCell<Vec<SyntaxDocument>>,
}

#[cfg(feature = "syntax-highlighting")]
#[derive(Clone)]
struct SyntaxLine {
    text: String,
    spans: Vec<TextSpan>,
    parse: two_face::re_exports::syntect::parsing::ParseState,
    stack: two_face::re_exports::syntect::parsing::ScopeStack,
}

#[cfg(feature = "syntax-highlighting")]
struct SyntaxDocument {
    language: String,
    lines: Vec<SyntaxLine>,
    bytes: usize,
}

#[cfg(feature = "syntax-highlighting")]
impl TextPresenter for CachedSyntectHighlighter {
    fn name(&self) -> &'static str {
        HIGHLIGHT_PRESENTER
    }
    fn present(&self, text: &str, request: &HighlightRequest) -> Vec<TextSpan> {
        self.present_incremental(text, &request.language).0
    }
}

#[cfg(feature = "syntax-highlighting")]
impl CachedSyntectHighlighter {
    fn present_incremental(&self, text: &str, language: &str) -> (Vec<TextSpan>, usize) {
        use two_face::re_exports::syntect::parsing::{ParseState, ScopeStack};
        // Keep a small document working set. Large one-off previews still receive
        // complete highlighting without retaining their parse snapshots.
        const MAX_BYTES: usize = 2 * 1024 * 1024;
        const MAX_LINES: usize = 16_384;
        let syntax = syntax_for(language);
        if syntax.name == "Plain Text" {
            return (Vec::new(), 0);
        }
        let lines: Vec<_> = text.split_inclusive('\n').collect();
        if text.len() > MAX_BYTES / 2 || lines.len() > MAX_LINES / 2 {
            return (syntect_present(text, language), lines.len());
        }
        let mut cache = self.cache.borrow_mut();
        let candidate = cache
            .iter()
            .enumerate()
            .filter(|(_, document)| document.language == language)
            .max_by_key(|(_, document)| {
                document
                    .lines
                    .iter()
                    .zip(&lines)
                    .take_while(|(old, new)| old.text == **new)
                    .count()
            })
            .map(|(index, _)| index);
        let old = candidate.map(|index| cache.remove(index));
        let old_lines = old
            .as_ref()
            .map(|document| document.lines.as_slice())
            .unwrap_or(&[]);
        let prefix = old_lines
            .iter()
            .zip(&lines)
            .take_while(|(old, new)| old.text == **new)
            .count();
        let suffix = old_lines[prefix..]
            .iter()
            .rev()
            .zip(lines[prefix..].iter().rev())
            .take_while(|(old, new)| old.text == **new)
            .count();
        let mut result = old_lines[..prefix].to_vec();
        let (mut parse, mut stack) = result
            .last()
            .map(|line| (line.parse.clone(), line.stack.clone()))
            .unwrap_or_else(|| (ParseState::new(syntax), ScopeStack::new()));
        let mut parsed = 0;
        let mut index = prefix;
        while index < lines.len() {
            if index >= lines.len() - suffix {
                let old_index = old_lines.len() - (lines.len() - index);
                let old_start = if old_index == 0 {
                    (ParseState::new(syntax), ScopeStack::new())
                } else {
                    (
                        old_lines[old_index - 1].parse.clone(),
                        old_lines[old_index - 1].stack.clone(),
                    )
                };
                if parse == old_start.0 && stack == old_start.1 {
                    result.extend_from_slice(&old_lines[old_index..]);
                    break;
                }
            }
            let line = lines[index];
            let operations = parse.parse_line(line, syntax_set()).unwrap_or_default();
            let mut spans = Vec::new();
            let mut last = 0;
            for (offset, operation) in operations {
                if offset > last {
                    push_scope_span(&mut spans, 0, last, offset, &stack);
                }
                let _ = stack.apply(&operation);
                last = offset;
            }
            if last < line.len() {
                push_scope_span(&mut spans, 0, last, line.len(), &stack);
            }
            result.push(SyntaxLine {
                text: line.to_owned(),
                spans,
                parse: parse.clone(),
                stack: stack.clone(),
            });
            parsed += 1;
            index += 1;
        }
        let mut offset = 0;
        let mut spans = Vec::new();
        for line in &result {
            spans.extend(line.spans.iter().map(|span| TextSpan {
                start: offset + span.start,
                end: offset + span.end,
                color: span.color,
            }));
            offset += line.text.len();
        }
        cache.push(SyntaxDocument {
            language: language.to_owned(),
            lines: result,
            bytes: text.len(),
        });
        while cache.len() > 4
            || cache.iter().map(|document| document.bytes).sum::<usize>() > MAX_BYTES
            || cache
                .iter()
                .map(|document| document.lines.len())
                .sum::<usize>()
                > MAX_LINES
        {
            cache.remove(0);
        }
        (sanitize_spans(text, spans), parsed)
    }
}

#[cfg(feature = "syntax-highlighting")]
fn syntax_set() -> &'static two_face::re_exports::syntect::parsing::SyntaxSet {
    static SYNTAXES: std::sync::LazyLock<two_face::re_exports::syntect::parsing::SyntaxSet> =
        std::sync::LazyLock::new(two_face::syntax::extra_newlines);
    &SYNTAXES
}

#[cfg(feature = "syntax-highlighting")]
fn syntax_for(language: &str) -> &'static two_face::re_exports::syntect::parsing::SyntaxReference {
    let syntaxes = syntax_set();
    syntaxes
        .find_syntax_by_token(language)
        .or_else(|| syntaxes.find_syntax_by_extension(language))
        .or_else(|| syntaxes.find_syntax_by_name(language))
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
}

#[cfg(feature = "syntax-highlighting")]
fn syntect_present(text: &str, language: &str) -> Vec<TextSpan> {
    use two_face::re_exports::syntect::parsing::{ParseState, ScopeStack};
    let syntax = syntax_for(language);
    if syntax.name == "Plain Text" {
        return Vec::new();
    }

    let mut parse = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut spans = Vec::new();
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let ops = parse.parse_line(line, syntax_set()).unwrap_or_default();
        let mut last = 0usize;
        for (index, op) in ops {
            if index > last {
                push_scope_span(&mut spans, offset, last, index, &stack);
            }
            let _ = stack.apply(&op);
            last = index;
        }
        if last < line.len() {
            push_scope_span(&mut spans, offset, last, line.len(), &stack);
        }
        offset += line.len();
    }
    sanitize_spans(text, spans)
}

#[cfg(feature = "syntax-highlighting")]
fn push_scope_span(
    spans: &mut Vec<TextSpan>,
    base: usize,
    start: usize,
    end: usize,
    stack: &two_face::re_exports::syntect::parsing::ScopeStack,
) {
    if start >= end {
        return;
    }
    let color = role_for_stack(stack);
    if color == SemanticColorRole::Text {
        return;
    }
    spans.push(TextSpan {
        start: base + start,
        end: base + end,
        color,
    });
}

#[cfg(feature = "syntax-highlighting")]
fn role_for_stack(stack: &two_face::re_exports::syntect::parsing::ScopeStack) -> SemanticColorRole {
    for scope in stack.as_slice().iter().rev() {
        let name = scope.build_string();
        if name.contains("invalid") {
            return SemanticColorRole::Danger;
        }
        if name.contains("comment") {
            return SemanticColorRole::Muted;
        }
        if name.contains("string") {
            return SemanticColorRole::Success;
        }
        if name.contains("constant.numeric")
            || name.contains("constant.character")
            || name.contains("constant.language")
        {
            return SemanticColorRole::Warning;
        }
        if name.contains("storage") || name.contains("keyword.declaration") {
            return SemanticColorRole::AccentStrong;
        }
        if name.contains("keyword") {
            return SemanticColorRole::Accent;
        }
        if name.contains("entity.name.function") {
            return SemanticColorRole::Accent;
        }
        if name.contains("entity.name.type")
            || name.contains("support.type")
            || name.contains("entity.name.class")
        {
            return SemanticColorRole::AccentOnSoft;
        }
        if name.contains("punctuation") {
            return SemanticColorRole::Faint;
        }
    }
    SemanticColorRole::Text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ComputedStyle, DocumentId, MutationQueue, NodeKind, StableNodeId, TextContent,
        TextInputState, TextMetrics, TextShapeConstraints, TextShaper, UiWorld,
    };

    struct KeywordPresenter;

    impl TextPresenter for KeywordPresenter {
        fn name(&self) -> &'static str {
            HIGHLIGHT_PRESENTER
        }

        fn present(&self, text: &str, _request: &HighlightRequest) -> Vec<TextSpan> {
            text.match_indices("fn")
                .map(|(start, token)| TextSpan {
                    start,
                    end: start + token.len(),
                    color: SemanticColorRole::Accent,
                })
                .collect()
        }
    }

    struct ZeroShaper;

    impl TextShaper for ZeroShaper {
        fn shape(
            &mut self,
            _id: StableNodeId,
            _text: &TextContent,
            style: &ComputedStyle,
            _constraints: TextShapeConstraints,
        ) -> TextMetrics {
            TextMetrics {
                width: style.font_size,
                height: style.font_size,
                ascent: None,
            }
        }
    }

    fn id(value: u64) -> StableNodeId {
        StableNodeId::new(value).unwrap()
    }

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    #[test]
    fn sanitize_drops_invalid_and_merges_adjacent_roles() {
        let spans = sanitize_spans(
            "fn main",
            vec![
                TextSpan {
                    start: 0,
                    end: 2,
                    color: SemanticColorRole::Accent,
                },
                TextSpan {
                    start: 2,
                    end: 3,
                    color: SemanticColorRole::Accent,
                },
                TextSpan {
                    start: 4,
                    end: 99,
                    color: SemanticColorRole::Danger,
                },
                TextSpan {
                    start: 3,
                    end: 2,
                    color: SemanticColorRole::Muted,
                },
            ],
        );
        assert_eq!(
            spans,
            vec![TextSpan {
                start: 0,
                end: 3,
                color: SemanticColorRole::Accent,
            }]
        );
    }

    #[test]
    fn registered_presenter_colors_committed_text() {
        let mut world = UiWorld::new();
        world
            .register_presenter(Box::new(KeywordPresenter))
            .unwrap();
        let mut queue = MutationQueue::new();
        queue.create(id(1), document(), NodeKind::Text);
        queue.set_text(
            id(1),
            TextContent {
                value: "fn main".into(),
            },
        );
        queue.set_highlight_request(id(1), Some(HighlightRequest::highlight("rs")));
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        assert!(work.text.contains(&id(1)));
        world.resolve_presentations(&work.text).unwrap();
        let presentation = world.text_presentation(id(1)).unwrap();
        assert_eq!(
            presentation.spans,
            vec![TextSpan {
                start: 0,
                end: 2,
                color: SemanticColorRole::Accent,
            }]
        );
        let extracted = world.extract_nodes(&[id(1)]);
        assert_eq!(extracted[0].text_spans.len(), 1);
        assert_eq!(extracted[0].text_spans[0].start, 0);
        assert_eq!(extracted[0].text_spans[0].end, 2);
    }

    #[test]
    fn missing_presenter_leaves_solid_text() {
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        queue.create(id(1), document(), NodeKind::Text);
        queue.set_text(
            id(1),
            TextContent {
                value: "fn main".into(),
            },
        );
        queue.set_highlight_request(id(1), Some(HighlightRequest::highlight("rs")));
        world.commit(queue).unwrap();
        world.resolve_presentations(&[id(1)]).unwrap();
        assert!(world.text_presentation(id(1)).unwrap().spans.is_empty());
        assert!(world.extract_nodes(&[id(1)])[0].text_spans.is_empty());
    }

    #[test]
    fn text_edit_rebuilds_spans_and_shape_text_runs_presenters() {
        let mut world = UiWorld::new();
        world
            .register_presenter(Box::new(KeywordPresenter))
            .unwrap();
        let mut queue = MutationQueue::new();
        queue.create(id(1), document(), NodeKind::Text);
        queue.set_text(
            id(1),
            TextContent {
                value: "let x".into(),
            },
        );
        queue.set_highlight_request(id(1), Some(HighlightRequest::highlight("rs")));
        world.commit(queue).unwrap();
        world.shape_text(&[id(1)], &mut ZeroShaper).unwrap();
        assert!(world.text_presentation(id(1)).unwrap().spans.is_empty());

        let mut queue = MutationQueue::new();
        queue.set_text(
            id(1),
            TextContent {
                value: "fn x".into(),
            },
        );
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.shape_text(&work.text, &mut ZeroShaper).unwrap();
        assert_eq!(world.text_presentation(id(1)).unwrap().spans.len(), 1);
    }

    #[test]
    fn theme_change_recolors_extracted_spans_without_rerunning() {
        let mut world = UiWorld::new();
        world
            .register_presenter(Box::new(KeywordPresenter))
            .unwrap();
        let mut queue = MutationQueue::new();
        queue.create(id(1), document(), NodeKind::Text);
        queue.set_text(id(1), TextContent { value: "fn".into() });
        queue.set_highlight_request(id(1), Some(HighlightRequest::highlight("rs")));
        world.commit(queue).unwrap();
        world.resolve_presentations(&[id(1)]).unwrap();
        let source = world.text_presentation(id(1)).unwrap().source;
        let dark = world.extract_nodes(&[id(1)])[0].text_spans[0].color;

        let mut queue = MutationQueue::new();
        queue.set_theme(nana_ui_core::ThemeMode::Light);
        world.commit(queue).unwrap();
        let light = world.extract_nodes(&[id(1)])[0].text_spans[0].color;
        assert_ne!(dark, light);
        assert_eq!(world.text_presentation(id(1)).unwrap().source, source);
    }

    #[test]
    fn ime_preedit_suppresses_extracted_spans() {
        let mut world = UiWorld::new();
        world
            .register_presenter(Box::new(KeywordPresenter))
            .unwrap();
        let mut queue = MutationQueue::new();
        queue.create(
            id(1),
            document(),
            NodeKind::Element {
                tag: "input".into(),
            },
        );
        queue.set_interaction(
            id(1),
            crate::InteractionState {
                pointer_events: true,
                focusable: true,
            },
        );
        queue.set_text_input(id(1), Some(TextInputState::new("fn main")));
        queue.set_highlight_request(id(1), Some(HighlightRequest::highlight("rs")));
        queue.request_focus(document(), Some(id(1)));
        queue.set_ime(
            id(1),
            Some(crate::ImeComposition {
                text: "x".into(),
                selection: None,
            }),
        );
        world.commit(queue).unwrap();
        world.resolve_presentations(&[id(1)]).unwrap();
        assert_eq!(world.text_presentation(id(1)).unwrap().spans.len(), 1);
        assert!(world.extract_nodes(&[id(1)])[0].text_spans.is_empty());
    }

    #[test]
    fn late_presenter_registration_dirties_matching_nodes() {
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        queue.create(id(1), document(), NodeKind::Text);
        queue.set_text(id(1), TextContent { value: "fn".into() });
        queue.set_highlight_request(id(1), Some(HighlightRequest::highlight("rs")));
        world.commit(queue).unwrap();
        let _ = world.take_system_work();
        world
            .register_presenter(Box::new(KeywordPresenter))
            .unwrap();
        let work = world.take_system_work();
        assert!(work.text.contains(&id(1)));
        world.resolve_presentations(&work.text).unwrap();
        assert_eq!(world.text_presentation(id(1)).unwrap().spans.len(), 1);
    }

    #[cfg(feature = "syntax-highlighting")]
    #[test]
    fn syntect_maps_rust_keywords_to_accent() {
        let spans = SyntectHighlighter.present("fn main() {}", &HighlightRequest::highlight("rs"));
        assert!(
            spans.iter().any(|span| {
                matches!(
                    span.color,
                    SemanticColorRole::Accent | SemanticColorRole::AccentStrong
                ) && &"fn main() {}"[span.start..span.end] == "fn"
            }),
            "expected rust `fn` to map to Accent or AccentStrong, got {spans:?}"
        );
    }

    fn span(start: usize, end: usize, color: SemanticColorRole) -> TextSpan {
        TextSpan { start, end, color }
    }

    /// overlay 段优先：基础层与 overlay 重叠处被切分丢弃，overlay 原样保留
    /// 在合并结果里（排序/校验交给 sanitize）。
    #[test]
    fn merge_overlay_spans_prioritizes_overlay_and_cuts_base() {
        let merged = merge_overlay_spans(
            vec![
                span(0, 10, SemanticColorRole::Accent),
                span(20, 30, SemanticColorRole::Muted),
            ],
            &[span(5, 8, SemanticColorRole::Danger)],
        );
        assert_eq!(
            merged,
            vec![
                span(0, 5, SemanticColorRole::Accent),
                span(8, 10, SemanticColorRole::Accent),
                span(20, 30, SemanticColorRole::Muted),
                span(5, 8, SemanticColorRole::Danger),
            ]
        );
    }

    /// overlay 内容参与 `presentation_source` 缓存键：换喂/撤除即失效，
    /// 内容相同（不同 Arc 分配）保持同键。
    #[test]
    fn presentation_source_hash_includes_overlay_content() {
        let text = &crate::TextValue::from("fn main");
        let plain = presentation_source(text, &HighlightRequest::highlight("rs"));
        let with_overlay = presentation_source(
            text,
            &HighlightRequest::highlight("rs").with_overlay(Arc::from([span(
                0,
                2,
                SemanticColorRole::Danger,
            )])),
        );
        assert_ne!(plain, with_overlay, "overlay 变更必须失效缓存");
        let same_content = presentation_source(
            text,
            &HighlightRequest::highlight("rs").with_overlay(Arc::from([span(
                0,
                2,
                SemanticColorRole::Danger,
            )])),
        );
        assert_eq!(with_overlay, same_content, "同内容 overlay 不无谓失效");
    }

    /// An editor's text keys the source by stamp: the same text under its
    /// stamp keys the same, and other bytes under another stamp do not.
    #[test]
    fn presentation_source_keys_stamped_text_by_its_stamp() {
        let request = HighlightRequest::highlight("rs");
        let text = crate::TextValue::stamped("fn main");
        assert_eq!(
            presentation_source(&text, &request),
            presentation_source(&text.clone(), &request)
        );
        let mut edited = text.clone();
        edited.replace_range(0..2, "pub fn");
        assert_ne!(
            presentation_source(&text, &request),
            presentation_source(&edited, &request)
        );
    }

    /// overlay 全覆盖时基础层整段丢弃；撤 overlay（重建不带 overlay 的
    /// 请求）后缓存键失效、回落纯基础层。无效偏移的 overlay 段被
    /// sanitize 丢弃，不进 presentation。
    #[test]
    fn overlay_wins_over_base_and_revocation_falls_back_to_syntect() {
        let mut world = UiWorld::new();
        world
            .register_presenter(Box::new(KeywordPresenter))
            .unwrap();
        let mut queue = MutationQueue::new();
        queue.create(id(1), document(), NodeKind::Text);
        queue.set_text(
            id(1),
            TextContent {
                value: "fn main".into(),
            },
        );
        queue.set_highlight_request(
            id(1),
            Some(HighlightRequest::highlight("rs").with_overlay(Arc::from([
                span(0, 7, SemanticColorRole::Danger),
                span(9, 99, SemanticColorRole::Success),
            ]))),
        );
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.resolve_presentations(&work.text).unwrap();
        // 基础层 "fn"(0,2 Accent) 全部落在 overlay 内被丢；越界段被
        // sanitize 丢弃，只剩 Danger 全覆盖段。
        assert_eq!(
            world.text_presentation(id(1)).unwrap().spans,
            vec![span(0, 7, SemanticColorRole::Danger)]
        );

        // 撤 overlay：同 presenter/language、无 overlay 的请求重建 →
        // 缓存键变化 → 重算回基础层。
        let mut queue = MutationQueue::new();
        queue.set_highlight_request(id(1), Some(HighlightRequest::highlight("rs")));
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.resolve_presentations(&work.text).unwrap();
        assert_eq!(
            world.text_presentation(id(1)).unwrap().spans,
            vec![span(0, 2, SemanticColorRole::Accent)]
        );
    }
}

#[cfg(all(test, feature = "syntax-highlighting"))]
mod incremental_syntax_tests {
    use super::*;

    #[test]
    fn incremental_highlighting_matches_fresh_parse_after_edits_and_undo() {
        let presenter = CachedSyntectHighlighter::default();
        let original = "fn main() {\n    let α = 1;\n    /* first\n       second */\n    let text = \"hello\";\n}\n";
        let variants = [
            original.to_owned(),
            original.replace("α = 1", "α = 12"),
            format!("// inserted\n{original}"),
            original.replace("/* first", "// first"),
            original.replace("second */", "second"),
            original.replace("\n", "\r\n"),
            original.replace("    let α = 1;\n", ""),
            original.to_owned(),
        ];
        for value in variants {
            let (actual, _) = presenter.present_incremental(&value, "rs");
            assert_eq!(actual, syntect_present(&value, "rs"), "{value}");
        }
        for language in ["wgsl", "js", "rs", "plain-unknown"] {
            let (actual, _) = presenter.present_incremental(original, language);
            assert_eq!(actual, syntect_present(original, language));
        }
    }

    #[test]
    fn end_edit_reuses_prefix_and_middle_edit_reuses_converged_suffix() {
        let presenter = CachedSyntectHighlighter::default();
        let source = (0..750)
            .map(|index| format!("fn sample_{index}() {{ let color = 0.5; }}\n"))
            .collect::<String>();
        let (_, initial) = presenter.present_incremental(&source, "rs");
        assert_eq!(initial, 750);
        let edited = format!("{source}// input");
        let (actual, parsed) = presenter.present_incremental(&edited, "rs");
        assert_eq!(actual, syntect_present(&edited, "rs"));
        assert_eq!(parsed, 1);
        let middle = edited.replacen("sample_375", "renamed_375", 1);
        let (actual, parsed) = presenter.present_incremental(&middle, "rs");
        assert_eq!(actual, syntect_present(&middle, "rs"));
        assert_eq!(parsed, 1);
    }
}

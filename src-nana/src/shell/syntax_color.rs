//! 纯文本预览的语义色。
//!
//! 仍用 `SelectableRichText`，不打开 syntax-highlighting，也不把 syntect 接进来。
//! 关键字用 `Keyword`（可保留 strong），字符串用 `Success`，注释用 `Muted`。
//! 这些角色和 Nana `role_for_stack` 对字符串、注释的映射一致。txt、log、csv 和未知扩展名保持无色。
//! 超过 768KiB 直接失败，不截断后假装成功。

use std::sync::Arc;

use nana_ui::runtime::{RichSpan, SemanticColorRole};

const TEXT_BYTE_LIMIT: usize = 768 * 1024;

const PLAIN: &[&str] = &["txt", "text", "log", "csv", "tsv"];

/// 从路径取出扩展名。没有点时用整段，方便 `.gitignore` 这类名字。
pub(super) fn extension_of(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((file, ext)) if !file.is_empty() && !ext.is_empty() => ext.to_ascii_lowercase(),
        _ => name.trim_start_matches('.').to_ascii_lowercase(),
    }
}

/// 按扩展名切成富文本片段。单色扩展名只有一段普通文本。
pub(super) fn paint(extension: &str, source: &str) -> Result<Vec<RichSpan>, String> {
    if source.len() > TEXT_BYTE_LIMIT {
        let message = format!("文本超过 {TEXT_BYTE_LIMIT} 字节");
        eprintln!("Nana 文本着色失败：{message}");
        return Err(message);
    }
    if PLAIN.iter().any(|item| *item == extension) || language(extension).is_none() {
        return Ok(vec![RichSpan::plain(source)]);
    }
    Ok(highlight(extension, source))
}

fn highlight(extension: &str, source: &str) -> Vec<RichSpan> {
    let lang = language(extension).unwrap_or(Language::plain());
    let mut spans = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut index = 0;
    let mut plain = String::new();
    let flush_plain = |plain: &mut String, spans: &mut Vec<RichSpan>| {
        if !plain.is_empty() {
            spans.push(RichSpan::plain(std::mem::take(plain)));
        }
    };
    while index < chars.len() {
        if let Some(end) = block_comment(&chars, index, &lang) {
            flush_plain(&mut plain, &mut spans);
            spans.push(comment(&chars[index..end]));
            index = end;
            continue;
        }
        if let Some(end) = line_comment(&chars, index, &lang) {
            flush_plain(&mut plain, &mut spans);
            spans.push(comment(&chars[index..end]));
            index = end;
            continue;
        }
        if let Some(end) = quoted(&chars, index) {
            flush_plain(&mut plain, &mut spans);
            spans.push(string_span(&chars[index..end]));
            index = end;
            continue;
        }
        if is_ident(chars[index]) {
            let start = index;
            index += 1;
            while index < chars.len() && is_ident(chars[index]) {
                index += 1;
            }
            let word: String = chars[start..index].iter().collect();
            if lang.keywords.iter().any(|keyword| *keyword == word) {
                flush_plain(&mut plain, &mut spans);
                spans.push(keyword_span(&word));
            } else {
                plain.push_str(&word);
            }
            continue;
        }
        plain.push(chars[index]);
        index += 1;
    }
    flush_plain(&mut plain, &mut spans);
    if spans.is_empty() {
        spans.push(RichSpan::plain(""));
    }
    spans
}

fn comment(chars: &[char]) -> RichSpan {
    let mut span = RichSpan::plain(Arc::<str>::from(chars.iter().collect::<String>()))
        .role(SemanticColorRole::Muted);
    span.emphasis = true;
    span
}

fn string_span(chars: &[char]) -> RichSpan {
    let mut span = RichSpan::plain(Arc::<str>::from(chars.iter().collect::<String>()))
        .role(SemanticColorRole::Success);
    span.code = true;
    span
}

fn keyword_span(word: &str) -> RichSpan {
    let mut span = RichSpan::plain(word).role(SemanticColorRole::Keyword);
    span.strong = true;
    span
}

fn block_comment(chars: &[char], index: usize, lang: &Language) -> Option<usize> {
    if lang.html && starts_with(chars, index, "<!--") {
        return Some(find_end(chars, index + 4, "-->").unwrap_or(chars.len()));
    }
    if lang.block && starts_with(chars, index, "/*") {
        return Some(find_end(chars, index + 2, "*/").unwrap_or(chars.len()));
    }
    None
}

fn line_comment(chars: &[char], index: usize, lang: &Language) -> Option<usize> {
    let slash = lang.slash && starts_with(chars, index, "//");
    let hash = lang.hash && chars.get(index) == Some(&'#');
    if !slash && !hash {
        return None;
    }
    let mut end = index;
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
    }
    Some(end)
}

fn quoted(chars: &[char], index: usize) -> Option<usize> {
    let quote = chars[index];
    if quote != '"' && quote != '\'' && quote != '`' {
        return None;
    }
    let mut end = index + 1;
    while end < chars.len() {
        if chars[end] == '\\' {
            end = (end + 2).min(chars.len());
            continue;
        }
        if chars[end] == quote {
            return Some(end + 1);
        }
        if chars[end] == '\n' && quote != '`' {
            return Some(end);
        }
        end += 1;
    }
    Some(chars.len())
}

fn starts_with(chars: &[char], index: usize, needle: &str) -> bool {
    let needle: Vec<char> = needle.chars().collect();
    chars.get(index..index + needle.len()).is_some_and(|slice| slice == needle.as_slice())
}

fn find_end(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    chars[from..].windows(needle.len()).position(|window| window == needle.as_slice()).map(|pos| from + pos + needle.len())
}

fn is_ident(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
}

struct Language {
    keywords: &'static [&'static str],
    slash: bool,
    hash: bool,
    block: bool,
    html: bool,
}

impl Language {
    fn plain() -> Self {
        Self { keywords: &[], slash: false, hash: false, block: false, html: false }
    }
}

fn language(extension: &str) -> Option<Language> {
    let keywords = match extension {
        "js" | "jsx" | "ts" | "tsx" | "vue" => JS,
        "json" | "jsonl" => JSON,
        "rs" => RUST,
        "py" => PYTHON,
        "go" => GO,
        "java" | "c" | "h" | "cpp" | "hpp" | "cs" => C_FAMILY,
        "php" => PHP,
        "rb" => RUBY,
        "css" | "scss" | "sass" | "less" => CSS,
        "sh" | "bash" | "zsh" | "ps1" => SHELL,
        "html" | "xml" => &[][..],
        "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "env" | "gitignore" | "gitattributes" | "bat" | "cmd" => &[][..],
        _ => return None,
    };
    Some(Language {
        keywords,
        slash: matches!(extension, "js" | "jsx" | "ts" | "tsx" | "vue" | "rs" | "go" | "java" | "c" | "h" | "cpp" | "hpp" | "cs" | "php" | "css" | "scss" | "sass" | "less" | "json" | "jsonl"),
        hash: matches!(extension, "py" | "rb" | "sh" | "bash" | "zsh" | "ps1" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "env" | "gitignore" | "gitattributes"),
        block: matches!(extension, "js" | "jsx" | "ts" | "tsx" | "vue" | "rs" | "go" | "java" | "c" | "h" | "cpp" | "hpp" | "cs" | "php" | "css" | "scss" | "sass" | "less"),
        html: matches!(extension, "html" | "xml" | "vue"),
    })
}

const JS: &[&str] = &[
    "const", "let", "var", "function", "return", "if", "else", "for", "while", "class", "import", "export", "from", "async",
    "await", "new", "this", "typeof", "interface", "type", "extends", "implements",
];
const JSON: &[&str] = &["true", "false", "null"];
const RUST: &[&str] = &[
    "fn", "let", "mut", "pub", "struct", "enum", "impl", "use", "mod", "return", "if", "else", "match", "for", "while", "loop",
    "self", "Self", "crate", "super", "async", "await", "const", "trait", "where",
];
const PYTHON: &[&str] = &["def", "class", "return", "if", "elif", "else", "for", "while", "import", "from", "as", "with", "try", "except", "lambda", "None", "True", "False"];
const GO: &[&str] = &["func", "package", "import", "return", "if", "else", "for", "var", "const", "type", "struct", "interface", "go", "defer"];
const C_FAMILY: &[&str] = &["if", "else", "for", "while", "return", "class", "struct", "public", "private", "void", "int", "bool", "const", "new", "namespace", "using"];
const PHP: &[&str] = &["function", "return", "if", "else", "foreach", "class", "public", "echo", "namespace", "use"];
const RUBY: &[&str] = &["def", "end", "class", "module", "if", "else", "do", "return", "require"];
const CSS: &[&str] = &["important"];
const SHELL: &[&str] = &["if", "then", "else", "fi", "for", "in", "do", "done", "echo", "function"];

#[cfg(test)]
mod tests {
    use super::paint;
    use nana_ui::runtime::SemanticColorRole;

    #[test]
    fn code_extensions_mark_keywords_strings_and_comments() {
        let spans = paint("js", "const a = \"hi\"; // note").expect("js");
        assert!(spans.iter().any(|span| {
            span.strong && span.color == Some(SemanticColorRole::Keyword) && span.text.as_ref() == "const"
        }));
        assert!(spans.iter().any(|span| {
            span.code && span.color == Some(SemanticColorRole::Success) && span.text.contains("hi")
        }));
        assert!(spans.iter().any(|span| {
            span.emphasis && span.color == Some(SemanticColorRole::Muted) && span.text.contains("note")
        }));
        let rust = paint("rs", "fn main() { /* block */ }").expect("rs");
        assert!(rust.iter().any(|span| {
            span.strong && span.color == Some(SemanticColorRole::Keyword) && span.text.as_ref() == "fn"
        }));
        assert!(rust.iter().any(|span| {
            span.emphasis && span.color == Some(SemanticColorRole::Muted) && span.text.contains("block")
        }));
        let python = paint("py", "def run():\n    return \"ok\" # done").expect("py");
        assert!(python.iter().any(|span| {
            span.strong && span.color == Some(SemanticColorRole::Keyword) && span.text.as_ref() == "def"
        }));
        assert!(python.iter().any(|span| {
            span.code && span.color == Some(SemanticColorRole::Success) && span.text.contains("ok")
        }));
        let json = paint("json", "{\"ok\": true}").expect("json");
        assert!(json.iter().any(|span| {
            span.code && span.color == Some(SemanticColorRole::Success) && span.text.contains("ok")
        }));
        assert!(json.iter().any(|span| {
            span.strong && span.color == Some(SemanticColorRole::Keyword) && span.text.as_ref() == "true"
        }));
    }

    #[test]
    fn plain_extensions_stay_one_color_and_oversize_fails() {
        for extension in ["txt", "log", "csv", "bin"] {
            let spans = paint(extension, "const \"hi\" // note").expect(extension);
            assert_eq!(spans.len(), 1, "{extension}");
            assert!(!spans[0].strong && !spans[0].code && !spans[0].emphasis);
            assert!(spans[0].color.is_none(), "{extension}");
        }
        let big = "a".repeat(768 * 1024 + 1);
        let error = paint("js", &big).expect_err("oversize");
        assert!(error.contains("786432"), "{error}");
        assert!(!error.contains("const"));
    }
}

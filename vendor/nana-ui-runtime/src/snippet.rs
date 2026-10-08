//! TextMate/LSP snippet parsing and linked placeholder coordinates.
use std::{collections::BTreeMap, ops::Range};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnippetPlaceholder {
    pub ranges: Vec<Range<usize>>,
    pub choices: Vec<String>,
    pub transforms: Vec<(Range<usize>, SnippetTransform)>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetTransform {
    pub regex: String,
    pub format: String,
    pub options: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnippetExpansion {
    pub text: String,
    pub placeholders: Vec<SnippetPlaceholder>,
}
#[derive(Clone, Debug)]
enum Part {
    Text(String),
    Stop(u32, Vec<Part>, Vec<String>),
    Transform(u32, SnippetTransform),
    Variable(String, Vec<Part>, Option<SnippetTransform>),
}

pub fn expand_text_snippet(body: &str) -> Option<SnippetExpansion> {
    expand_text_snippet_with_variables(body, &BTreeMap::new())
}
/// Expand an LSP snippet with host-owned document/clipboard variables.
pub fn expand_text_snippet_with_variables(
    body: &str,
    variables: &BTreeMap<String, String>,
) -> Option<SnippetExpansion> {
    fn transform(s: &str, at: &mut usize) -> Option<SnippetTransform> {
        fn section(s: &str, at: &mut usize, format: bool) -> Option<String> {
            let mut text = String::new();
            let mut braces = 0;
            loop {
                let c = s[*at..].chars().next()?;
                *at += c.len_utf8();
                match c {
                    '\\' => {
                        let next = s[*at..].chars().next()?;
                        *at += next.len_utf8();
                        if next != '/' {
                            text.push(c);
                        }
                        text.push(next);
                    }
                    '{' if format => {
                        braces += 1;
                        text.push(c);
                    }
                    '}' if format && braces > 0 => {
                        braces -= 1;
                        text.push(c);
                    }
                    '/' if braces == 0 => return Some(text),
                    _ => text.push(c),
                }
            }
        }
        *at += 1;
        let regex = section(s, at, false)?;
        let format = section(s, at, true)?;
        let start = *at;
        while s.as_bytes().get(*at).is_some_and(|c| *c != b'}') {
            *at += 1;
        }
        let options = s.get(start..*at)?.to_owned();
        if !s[*at..].starts_with('}') {
            return None;
        }
        *at += 1;
        Some(SnippetTransform {
            regex,
            format,
            options,
        })
    }
    fn parse(s: &str, at: &mut usize, nested: bool, depth: usize) -> Option<Vec<Part>> {
        if depth > 64 {
            return None;
        }
        let mut parts = Vec::new();
        let mut text = String::new();
        while *at < s.len() {
            let ch = s[*at..].chars().next()?;
            *at += ch.len_utf8();
            if ch == '}' && nested {
                if !text.is_empty() {
                    parts.push(Part::Text(text));
                }
                return Some(parts);
            }
            if ch == '\\' {
                if let Some(next) = s[*at..].chars().next()
                    && matches!(next, '$' | '}' | '\\')
                {
                    *at += next.len_utf8();
                    text.push(next);
                } else {
                    text.push(ch);
                }
                continue;
            }
            if ch != '$' {
                text.push(ch);
                continue;
            }
            let braced = s[*at..].starts_with('{');
            if braced {
                *at += 1;
            }
            let start = *at;
            let numeric = s.as_bytes().get(*at).is_some_and(u8::is_ascii_digit);
            while s.as_bytes().get(*at).is_some_and(|c| {
                if numeric {
                    c.is_ascii_digit()
                } else {
                    c.is_ascii_alphanumeric() || *c == b'_'
                }
            }) {
                *at += 1;
            }
            if start == *at {
                text.push('$');
                if braced {
                    text.push('{');
                }
                continue;
            }
            let name = s[start..*at].to_owned();
            if !text.is_empty() {
                parts.push(Part::Text(std::mem::take(&mut text)));
            }
            let mut default = Vec::new();
            let mut choices = Vec::new();
            let mut trans = None;
            if braced {
                match s.as_bytes().get(*at).copied()? {
                    b'}' => *at += 1,
                    b':' => {
                        *at += 1;
                        default = parse(s, at, true, depth + 1)?;
                    }
                    b'/' => trans = Some(transform(s, at)?),
                    b'|' if numeric => {
                        *at += 1;
                        let mut choice = String::new();
                        loop {
                            let c = s[*at..].chars().next()?;
                            *at += c.len_utf8();
                            match c {
                                '\\' => {
                                    let n = s[*at..].chars().next()?;
                                    if matches!(n, ',' | '|' | '\\') {
                                        *at += n.len_utf8();
                                        choice.push(n);
                                    } else {
                                        choice.push(c);
                                    }
                                }
                                ',' => choices.push(std::mem::take(&mut choice)),
                                '|' => {
                                    if !s[*at..].starts_with('}') {
                                        return None;
                                    }
                                    *at += 1;
                                    choices.push(choice);
                                    break;
                                }
                                _ => choice.push(c),
                            }
                        }
                    }
                    _ => return None,
                }
            }
            if numeric {
                let id = name.parse().ok()?;
                parts.push(if let Some(t) = trans {
                    Part::Transform(id, t)
                } else {
                    Part::Stop(id, default, choices)
                });
            } else {
                parts.push(Part::Variable(name, default, trans));
            }
        }
        if nested {
            return None;
        }
        if !text.is_empty() {
            parts.push(Part::Text(text));
        }
        Some(parts)
    }
    fn definitions(parts: &[Part], out: &mut BTreeMap<u32, (Vec<Part>, Vec<String>)>) {
        for p in parts {
            match p {
                Part::Stop(id, default, choices) => {
                    if !default.is_empty() || !choices.is_empty() {
                        out.entry(*id)
                            .or_insert_with(|| (default.clone(), choices.clone()));
                    }
                    definitions(default, out);
                }
                Part::Variable(_, default, _) => definitions(default, out),
                _ => {}
            }
        }
    }
    fn render(
        parts: &[Part],
        defs: &BTreeMap<u32, (Vec<Part>, Vec<String>)>,
        vars: &BTreeMap<String, String>,
        output: &mut SnippetExpansion,
        groups: &mut BTreeMap<u32, SnippetPlaceholder>,
        stack: &mut Vec<u32>,
    ) -> Option<()> {
        for p in parts {
            match p {
                Part::Text(t) => output.text.push_str(t),
                Part::Variable(name, default, trans) => {
                    let start = output.text.len();
                    if let Some(value) = vars.get(name) {
                        output.text.push_str(
                            &trans
                                .as_ref()
                                .map(|t| t.apply(value))
                                .unwrap_or_else(|| value.clone()),
                        );
                    } else if let Some(t) = trans {
                        output.text.push_str(&t.apply(""));
                    } else if !default.is_empty() {
                        render(default, defs, vars, output, groups, stack)?;
                    } else if !known_variable(name) {
                        output.text.push_str(name);
                        let id = 1_000_000 + groups.len() as u32;
                        groups
                            .entry(id)
                            .or_default()
                            .ranges
                            .push(start..output.text.len());
                    }
                }
                Part::Stop(id, _, _) | Part::Transform(id, _) => {
                    if stack.contains(id) {
                        return None;
                    }
                    let start = output.text.len();
                    stack.push(*id);
                    if let Some((default, choices)) = defs.get(id) {
                        if let Some(choice) = choices.first() {
                            output.text.push_str(choice);
                        } else {
                            if matches!(p, Part::Transform(_, _)) {
                                let mut scratch = SnippetExpansion::default();
                                render(
                                    default,
                                    defs,
                                    vars,
                                    &mut scratch,
                                    &mut BTreeMap::new(),
                                    stack,
                                )?;
                                output.text.push_str(&scratch.text);
                            } else {
                                render(default, defs, vars, output, groups, stack)?;
                            }
                        }
                    }
                    stack.pop();
                    if let Part::Transform(_, t) = p {
                        let value = t.apply(&output.text[start..]);
                        output.text.truncate(start);
                        output.text.push_str(&value);
                        groups
                            .entry(*id)
                            .or_default()
                            .transforms
                            .push((start..output.text.len(), t.clone()));
                    } else {
                        let group = groups.entry(*id).or_default();
                        group.ranges.push(start..output.text.len());
                        if let Some((_, choices)) = defs.get(id) {
                            group.choices = choices.clone();
                        }
                    }
                }
            }
        }
        Some(())
    }
    let parts = parse(body, &mut 0, false, 0)?;
    let mut defs = BTreeMap::new();
    definitions(&parts, &mut defs);
    let mut output = SnippetExpansion::default();
    let mut groups = BTreeMap::new();
    render(
        &parts,
        &defs,
        variables,
        &mut output,
        &mut groups,
        &mut Vec::new(),
    )?;
    let last = groups
        .remove(&0)
        .filter(|g| !g.ranges.is_empty())
        .unwrap_or_else(|| SnippetPlaceholder {
            #[allow(clippy::single_range_in_vec_init)]
            ranges: vec![output.text.len()..output.text.len()],
            ..Default::default()
        });
    output.placeholders = groups
        .into_values()
        .filter(|g| !g.ranges.is_empty())
        .collect();
    output.placeholders.push(last);
    Some(output)
}
fn known_variable(name: &str) -> bool {
    matches!(
        name,
        "TM_SELECTED_TEXT"
            | "TM_CURRENT_LINE"
            | "TM_CURRENT_WORD"
            | "TM_LINE_INDEX"
            | "TM_LINE_NUMBER"
            | "TM_FILENAME"
            | "TM_FILENAME_BASE"
            | "TM_DIRECTORY"
            | "TM_FILEPATH"
            | "RELATIVE_FILEPATH"
            | "CLIPBOARD"
            | "WORKSPACE_NAME"
            | "WORKSPACE_FOLDER"
            | "CURSOR_INDEX"
            | "CURSOR_NUMBER"
            | "BLOCK_COMMENT_START"
            | "BLOCK_COMMENT_END"
            | "LINE_COMMENT"
    ) || name.starts_with("CURRENT_")
}
impl SnippetTransform {
    fn apply(&self, value: &str) -> String {
        let flags = self
            .options
            .chars()
            .filter(|c| matches!(c, 'i' | 'm' | 's'))
            .collect::<String>();
        let pattern = if flags.is_empty() {
            self.regex.clone()
        } else {
            format!("(?{flags}){}", self.regex)
        };
        let Ok(regex) = fancy_regex::RegexBuilder::new(&pattern)
            .backtrack_limit(100_000)
            .build()
        else {
            return value.into();
        };
        let mut result = String::new();
        let mut end = 0;
        for captures in regex.captures_iter(value) {
            let Ok(c) = captures else {
                return value.into();
            };
            let Some(m) = c.get(0) else {
                continue;
            };
            result.push_str(&value[end..m.start()]);
            result.push_str(&format_captures(&self.format, &c));
            end = m.end();
            if !self.options.contains('g') {
                break;
            }
        }
        result.push_str(&value[end..]);
        result
    }
}
fn format_captures(format: &str, c: &fancy_regex::Captures<'_>) -> String {
    let mut out = String::new();
    let mut at = 0;
    while at < format.len() {
        let ch = format[at..].chars().next().unwrap();
        at += ch.len_utf8();
        if ch == '\\' {
            if let Some(next) = format[at..].chars().next() {
                at += next.len_utf8();
                out.push(next);
            }
            continue;
        }
        if ch != '$' {
            out.push(ch);
            continue;
        }
        let braced = format[at..].starts_with('{');
        if braced {
            at += 1;
        }
        let start = at;
        while format.as_bytes().get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        let Ok(index) = format[start..at].parse::<usize>() else {
            out.push('$');
            continue;
        };
        let value = c.get(index).map(|m| m.as_str()).unwrap_or("");
        if !braced {
            out.push_str(value);
            continue;
        }
        let start = at;
        while format.as_bytes().get(at).is_some_and(|b| *b != b'}') {
            at += 1;
        }
        let op = &format[start..at];
        if at < format.len() {
            at += 1;
        }
        if let Some(case) = op.strip_prefix(":/") {
            let words = value
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            let capital = |s: &str| {
                let mut c = s.chars();
                c.next()
                    .map(|first| first.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            };
            out.push_str(&match case {
                "upcase" => value.to_uppercase(),
                "downcase" => value.to_lowercase(),
                "capitalize" => capital(value),
                "pascalcase" => words.iter().map(|w| capital(w)).collect(),
                "camelcase" => words
                    .iter()
                    .enumerate()
                    .map(|(i, w)| if i == 0 { w.to_lowercase() } else { capital(w) })
                    .collect(),
                "snakecase" => words.join("_").to_lowercase(),
                "kebabcase" => words.join("-").to_lowercase(),
                _ => value.into(),
            });
        } else if let Some(yes) = op.strip_prefix(":+") {
            if !value.is_empty() {
                out.push_str(yes);
            }
        } else if let Some(branch) = op.strip_prefix(":?") {
            let (yes, no) = branch.split_once(':').unwrap_or((branch, ""));
            out.push_str(if value.is_empty() { no } else { yes });
        } else if let Some(no) = op.strip_prefix(":-").or_else(|| op.strip_prefix(':')) {
            out.push_str(if value.is_empty() { no } else { value });
        } else {
            out.push_str(value);
        }
    }
    out
}
impl crate::components::TextSnippetSession {
    pub(crate) fn linked_edit(
        &self,
        old: &str,
        new: &str,
        selection: crate::TextSelection,
    ) -> Option<(String, crate::TextSelection, Self)> {
        let active = self.index.checked_sub(1)?;
        let primary = self.placeholders.get(active)?.ranges.first()?.clone();
        let mut start = old
            .bytes()
            .zip(new.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
            start -= 1;
        }
        let mut old_end = old.len();
        let mut new_end = new.len();
        while old_end > start
            && new_end > start
            && old.as_bytes()[old_end - 1] == new.as_bytes()[new_end - 1]
        {
            old_end -= 1;
            new_end -= 1;
        }
        while !old.is_char_boundary(old_end) || !new.is_char_boundary(new_end) {
            old_end += 1;
            new_end += 1;
        }
        if start < primary.start || old_end > primary.end {
            return None;
        }
        let delta = new.len() as isize - old.len() as isize;
        let next_end = primary.end.checked_add_signed(delta)?;
        let inserted = new.get(primary.start..next_end)?.to_owned();
        let mut edits = vec![(start..old_end, new.get(start..new_end)?.to_owned())];
        for r in self.placeholders[active].ranges.iter().skip(1) {
            if r.start < primary.end && r.end > primary.start {
                return None;
            }
            edits.push((r.clone(), inserted.clone()));
        }
        for (range, transform) in &self.placeholders[active].transforms {
            edits.push((range.clone(), transform.apply(&inserted)));
        }
        edits.sort_by_key(|(r, _)| (r.start, r.end));
        if edits.windows(2).any(|p| p[0].0.end > p[1].0.start) {
            return None;
        }
        let shift: isize = edits
            .iter()
            .filter(|(r, _)| r.end <= primary.start && r.start != start)
            .map(|(r, t)| t.len() as isize - r.len() as isize)
            .sum();
        let selection = crate::TextSelection::new(
            selection.anchor.checked_add_signed(shift)?,
            selection.focus.checked_add_signed(shift)?,
        );
        let mut output = old.to_owned();
        let mut session = self.clone();
        for (range, text) in edits.into_iter().rev() {
            output.replace_range(range.clone(), &text);
            let d = text.len() as isize - range.len() as isize;
            for (group_index, group) in session.placeholders.iter_mut().enumerate() {
                let map_range = |r: &Range<usize>| {
                    if group_index == active && r.start <= range.start && r.end >= range.end {
                        return Some(r.start..r.end.checked_add_signed(d)?);
                    }
                    if r.end < range.start || (r.end == range.start && r.start < r.end) {
                        return Some(r.clone());
                    }
                    if r.start > range.end || (r.start == range.end && r.start != range.start) {
                        return Some(r.start.checked_add_signed(d)?..r.end.checked_add_signed(d)?);
                    }
                    if r.start <= range.start && r.end >= range.end {
                        return Some(r.start..r.end.checked_add_signed(d)?);
                    }
                    None
                };
                group.ranges = group.ranges.iter().filter_map(map_range).collect();
                group.transforms = group
                    .transforms
                    .iter()
                    .filter_map(|(r, t)| map_range(r).map(|r| (r, t.clone())))
                    .collect();
            }
        }
        let removed_before = session
            .placeholders
            .iter()
            .take(active)
            .filter(|g| g.ranges.is_empty())
            .count();
        session.placeholders.retain(|g| !g.ranges.is_empty());
        session.index -= removed_before;
        session.stops = session
            .placeholders
            .iter()
            .map(|g| g.ranges[0].start)
            .collect();
        session.selection_ends = session
            .placeholders
            .iter()
            .map(|g| g.ranges[0].end)
            .collect();
        Some((output, selection, session))
    }
    pub(crate) fn choice_items(&self) -> Option<std::sync::Arc<[crate::TextCompletion]>> {
        let group = self.placeholders.get(self.index.checked_sub(1)?)?;
        (!group.choices.is_empty()).then(|| {
            group
                .choices
                .iter()
                .map(|text| crate::TextCompletion::new(text, "choice"))
                .collect()
        })
    }
}

pub(crate) fn context_variables(
    source: &str,
    selection: crate::TextSelection,
    host: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut vars = host.clone();
    let caret = selection.focus.min(source.len());
    if !source.is_char_boundary(caret) {
        return vars;
    }
    let start = source[..caret].rfind('\n').map_or(0, |at| at + 1);
    let end = source[caret..]
        .find('\n')
        .map_or(source.len(), |at| caret + at);
    let line = source[..caret].bytes().filter(|b| *b == b'\n').count();
    let mut word_start = caret;
    while word_start > 0 {
        let c = source[..word_start].chars().next_back().unwrap();
        if !c.is_alphanumeric() && c != '_' {
            break;
        }
        word_start -= c.len_utf8();
    }
    let mut word_end = caret;
    while word_end < source.len() {
        let c = source[word_end..].chars().next().unwrap();
        if !c.is_alphanumeric() && c != '_' {
            break;
        }
        word_end += c.len_utf8();
    }
    for (name, value) in [
        (
            "TM_SELECTED_TEXT",
            source.get(selection.ordered()).unwrap_or("").to_owned(),
        ),
        ("TM_CURRENT_LINE", source[start..end].into()),
        ("TM_CURRENT_WORD", source[word_start..word_end].into()),
        ("TM_LINE_INDEX", line.to_string()),
        ("TM_LINE_NUMBER", (line + 1).to_string()),
        ("CURSOR_INDEX", "0".into()),
        ("CURSOR_NUMBER", "1".into()),
    ] {
        vars.insert(name.into(), value);
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snippet_variables_nested_defaults_and_transforms_expand_and_track_edits() {
        let variables = BTreeMap::from([("TM_FILENAME".into(), "hello-world.wgsl".into())]);
        let e=expand_text_snippet_with_variables("${TM_FILENAME/(.*)\\..+$/${1:/pascalcase}/} ${1:foo} ${1/(.*)/${1:/upcase}/} ${UNSET:default} $unknown$0",&variables).unwrap();
        assert_eq!(e.text, "HelloWorld foo FOO default unknown");
        let session = crate::components::TextSnippetSession {
            exit_on_last: false,
            stops: e.placeholders.iter().map(|g| g.ranges[0].start).collect(),
            selection_ends: e.placeholders.iter().map(|g| g.ranges[0].end).collect(),
            placeholders: e.placeholders,
            index: 1,
        };
        let (value, _, next) = session
            .linked_edit(
                &e.text,
                "HelloWorld bar FOO default unknown",
                crate::TextSelection::caret(14),
            )
            .unwrap();
        assert_eq!(value, "HelloWorld bar BAR default unknown");
        assert_eq!(next.placeholders[1].ranges[0], 27..34);
        let zero = expand_text_snippet("${0/x/y/}").unwrap();
        assert_eq!(zero.placeholders[0].ranges, vec![0..0]);
        let transformed = expand_text_snippet("${1:${2:foo}} ${1/(.*)/${1:/upcase}/}").unwrap();
        assert_eq!(transformed.placeholders[1].ranges, vec![0..3]);
        let nested = expand_text_snippet("${1:outer ${2:inner}} $2 $1").unwrap();
        assert_eq!(nested.text, "outer inner inner outer inner");
    }
    #[test]
    fn snippet_mirrors_forward_defaults_and_escaped_choices_use_utf8_ranges() {
        let e = expand_text_snippet("$1 ${1:颜色} ${2|r\\,g,b\\|a|} $2$0").unwrap();
        assert_eq!(e.text, "颜色 颜色 r,g r,g");
        assert_eq!(e.placeholders[0].ranges, vec![0..6, 7..13]);
        assert_eq!(e.placeholders[1].choices, vec!["r,g", "b|a"]);
        let session = crate::components::TextSnippetSession {
            exit_on_last: false,
            stops: e.placeholders.iter().map(|g| g.ranges[0].start).collect(),
            selection_ends: e.placeholders.iter().map(|g| g.ranges[0].end).collect(),
            placeholders: e.placeholders,
            index: 1,
        };
        let (value, selection, next) = session
            .linked_edit(&e.text, "蓝 颜色 r,g r,g", crate::TextSelection::caret(3))
            .unwrap();
        assert_eq!(value, "蓝 蓝 r,g r,g");
        assert_eq!(selection.focus, 3);
        assert_eq!(next.placeholders[1].ranges, vec![8..11, 12..15]);
        let (value, _, next) = next
            .linked_edit(&value, "蓝色 蓝 r,g r,g", crate::TextSelection::caret(6))
            .unwrap();
        assert_eq!(value, "蓝色 蓝色 r,g r,g");
        assert_eq!(next.selection_ends[0], 6);
    }
}

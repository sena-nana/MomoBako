//! PDF 的对象表和页树：按 `/Type /Page` 数页、按页树的顺序排页，每页带上内容流和资源。
//!
//! 这是渲染器接好以前的止血（真正的 PDF 渲染器以后换）：不解析交叉引用表，按文件顺序扫顶层的
//! `N G obj … endobj`，再解开对象流（`/Type /ObjStm`）里的对象，同一个编号后出现的覆盖先出现的（增量更新
//! 追加在文件末尾）。页序从文件尾的 `/Root` 找目录、顺着 `/Pages` 的 `/Kids` 往下走；找不到目录或者页树断了时，
//! 按对象编号收集所有 `/Type /Page`。资源字典沿页树继承。

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::{
    dict_before, direct_length, find_keyword, is_pdf_delim, parse_hex, parse_literal, parse_number, skip_noise, stream_data_start,
};

/// 页树最多走多深、最多收多少页，防止环和恶意文件。
const MAX_TREE_DEPTH: usize = 64;
const MAX_PAGES: usize = 100_000;

/// 解析出来的 PDF 值。只认页树、内容流和资源要用到的几种。
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Value {
    Name(String),
    Num(f64),
    Ref(u32),
    Array(Vec<Value>),
    Dict(Vec<(String, Value)>),
    Str(Vec<u8>),
    Other,
}

impl Value {
    /// 字典里某个键的值；不是字典或没有这个键时为 `None`。
    pub(super) fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(entries) => entries.iter().rev().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    fn name(&self) -> Option<&str> {
        match self {
            Value::Name(name) => Some(name),
            _ => None,
        }
    }
}

/// 一个间接对象：字典（或别的值），是流的话记着流的字典原文和数据在文件里的位置（不复制，大文件不翻倍占内存）。
#[derive(Clone, Debug)]
pub(super) struct Object {
    pub(super) value: Value,
    stream: Option<(Range<usize>, Range<usize>)>,
}

/// 一页：内容流（按顺序拼起来画）和继承以后的资源字典。
#[derive(Clone, Debug)]
pub(super) struct Page {
    pub(super) contents: Vec<Value>,
    pub(super) resources: Option<Value>,
}

/// 文件里的对象表。
pub(super) struct Document<'a> {
    bytes: &'a [u8],
    objects: HashMap<u32, Object>,
    root: Option<u32>,
}

impl<'a> Document<'a> {
    /// 扫一遍文件，建对象表、找目录。
    pub(super) fn parse(bytes: &'a [u8]) -> Self {
        let mut objects = HashMap::new();
        let mut cursor = 0;
        while let Some((number, body_start)) = next_object_header(bytes, cursor) {
            let (object, end) = read_object(bytes, body_start);
            if is_type(&object.value, "ObjStm") {
                for (inner, value) in object_stream_members(bytes, &object) {
                    objects.insert(inner, Object { value, stream: None });
                }
            }
            objects.insert(number, object);
            cursor = end.max(body_start + 1);
        }
        Self { bytes, objects, root: trailer_root(bytes) }
    }

    pub(super) fn object(&self, number: u32) -> Option<&Object> {
        self.objects.get(&number)
    }

    /// 流对象的字典原文和数据；不是流时为 `None`。
    pub(super) fn stream(&self, number: u32) -> Option<(&'a [u8], &'a [u8])> {
        let (dict, data) = self.objects.get(&number)?.stream.clone()?;
        Some((&self.bytes[dict], &self.bytes[data]))
    }

    /// 顺着引用取到真正的值（最多跟 8 层）。
    pub(super) fn resolve<'v>(&'v self, value: &'v Value) -> &'v Value {
        let mut current = value;
        for _ in 0..8 {
            match current {
                Value::Ref(number) => match self.objects.get(number) {
                    Some(object) => current = &object.value,
                    None => return &Value::Other,
                },
                _ => return current,
            }
        }
        current
    }

    /// 按页树顺序列出所有页。页树走不通时按对象编号收集 `/Type /Page`。
    pub(super) fn pages(&self) -> Vec<Page> {
        let mut pages = Vec::new();
        let tree = self.root.and_then(|root| self.objects.get(&root)).and_then(|catalog| catalog.value.get("Pages")).cloned();
        if let Some(Value::Ref(number)) = tree {
            let mut visited = HashSet::new();
            self.walk(number, None, 0, &mut visited, &mut pages);
        }
        if pages.is_empty() {
            let mut numbers: Vec<u32> = self.objects.iter().filter(|(_, object)| is_type(&object.value, "Page")).map(|(number, _)| *number).collect();
            numbers.sort_unstable();
            pages = numbers.into_iter().filter_map(|number| self.page(number, None)).take(MAX_PAGES).collect();
        }
        pages
    }

    /// 走一个页树节点：`/Pages` 往下走 `/Kids`，`/Page` 收进来。资源字典没写时继承上一层的。
    fn walk(&self, number: u32, inherited: Option<&Value>, depth: usize, visited: &mut HashSet<u32>, pages: &mut Vec<Page>) {
        if depth > MAX_TREE_DEPTH || pages.len() >= MAX_PAGES || !visited.insert(number) {
            return;
        }
        let Some(node) = self.objects.get(&number) else {
            return;
        };
        if is_type(&node.value, "Page") {
            pages.extend(self.page(number, inherited));
            return;
        }
        let resources = node.value.get("Resources").or(inherited);
        let Some(Value::Array(kids)) = node.value.get("Kids").map(|kids| self.resolve(kids)) else {
            return;
        };
        for kid in kids {
            if let Value::Ref(kid) = kid {
                self.walk(*kid, resources, depth + 1, visited, pages);
            }
        }
    }

    fn page(&self, number: u32, inherited: Option<&Value>) -> Option<Page> {
        let page = &self.objects.get(&number)?.value;
        let contents = match page.get("Contents").map(|contents| (contents, self.resolve(contents))) {
            Some((_, Value::Array(items))) => items.clone(),
            Some((reference @ Value::Ref(_), _)) => vec![reference.clone()],
            _ => Vec::new(),
        };
        let resources = page.get("Resources").or(inherited).map(|resources| self.resolve(resources).clone());
        Some(Page { contents, resources })
    }
}

/// 字典的 `/Type` 是这个名字（`Page` 不会匹配 `Pages`）。
fn is_type(value: &Value, kind: &str) -> bool {
    value.get("Type").and_then(Value::name) == Some(kind)
}

/// 从 `from` 起找下一个顶层对象头 `N G obj`，交回对象编号和 `obj` 后面的位置。
fn next_object_header(bytes: &[u8], from: usize) -> Option<(u32, usize)> {
    let mut cursor = from;
    while let Some(at) = find_keyword(bytes, b"obj", cursor) {
        cursor = at + 3;
        let Some((generation_start, _)) = number_before(bytes, at) else {
            continue;
        };
        let Some((number_start, number)) = number_before(bytes, generation_start) else {
            continue;
        };
        if number_start > 0 && !is_pdf_delim(bytes[number_start - 1]) {
            continue;
        }
        return Some((number, at + 3));
    }
    None
}

/// `end` 前面（隔着空白）的一个非负整数：交回它的起点和值。
fn number_before(bytes: &[u8], end: usize) -> Option<(usize, u32)> {
    let mut index = end;
    while index > 0 && bytes[index - 1].is_ascii_whitespace() {
        index -= 1;
    }
    if index == end {
        return None;
    }
    let digits_end = index;
    while index > 0 && bytes[index - 1].is_ascii_digit() {
        index -= 1;
    }
    if index == digits_end {
        return None;
    }
    let value = std::str::from_utf8(&bytes[index..digits_end]).ok()?.parse().ok()?;
    Some((index, value))
}

/// 读 `obj` 后面的对象：值，是流的话再带上字典原文和数据。交回对象和 `endobj`（或流尾）后面的位置。
fn read_object(bytes: &[u8], start: usize) -> (Object, usize) {
    let mut index = start;
    let value = parse_value(bytes, &mut index, 0).unwrap_or(Value::Other);
    let mut after = index;
    skip_noise(bytes, &mut after);
    let mut stream = None;
    if bytes[after..].starts_with(b"stream") {
        if let Some(data_start) = stream_data_start(bytes, after + 6) {
            let dict = dict_before(bytes, after);
            // `dict_before` 找不到字典时交回空串，不在文件里，记成空区间。
            let dict_start = if dict.is_empty() { after } else { dict.as_ptr() as usize - bytes.as_ptr() as usize };
            let dict_range = dict_start..dict_start + dict.len();
            let end_keyword = find_keyword(bytes, b"endstream", data_start);
            let data_end = match (direct_length(dict), end_keyword) {
                (Some(length), end) => {
                    let by_length = data_start.saturating_add(length).min(bytes.len());
                    end.map(|position| by_length.min(position)).unwrap_or(by_length)
                }
                (None, Some(position)) => position,
                (None, None) => bytes.len(),
            };
            stream = Some((dict_range, data_start..data_end.max(data_start)));
            index = end_keyword.map(|position| position + b"endstream".len()).unwrap_or(data_end);
        }
    }
    let end = find_keyword(bytes, b"endobj", index).map(|position| position + b"endobj".len()).unwrap_or(index);
    (Object { value, stream }, end)
}

/// 对象流里的对象：先按滤镜解开，头部是 `/N` 对「编号 偏移」，偏移从 `/First` 算起。
fn object_stream_members(bytes: &[u8], object: &Object) -> Vec<(u32, Value)> {
    let Some((dict, data)) = object.stream.clone() else {
        return Vec::new();
    };
    let plain = match super::paint::decode(&bytes[dict], &bytes[data]) {
        Ok(super::paint::Decoded::Text(plain)) => plain,
        Ok(super::paint::Decoded::Jpeg(_)) => return Vec::new(),
        Err(error) => {
            eprintln!("Nana PDF 对象流解不开：{error}");
            return Vec::new();
        }
    };
    let count = match object.value.get("N") {
        Some(Value::Num(count)) if *count >= 0.0 => *count as usize,
        _ => return Vec::new(),
    };
    let first = match object.value.get("First") {
        Some(Value::Num(first)) if *first >= 0.0 => *first as usize,
        _ => return Vec::new(),
    };
    let mut header = 0;
    let mut entries = Vec::new();
    for _ in 0..count.min(MAX_PAGES) {
        let (Some(Value::Num(number)), Some(Value::Num(offset))) =
            (parse_value(&plain, &mut header, 0), parse_value(&plain, &mut header, 0))
        else {
            break;
        };
        entries.push((number as u32, first.saturating_add(offset as usize)));
    }
    entries
        .iter()
        .filter(|(_, offset)| *offset < plain.len())
        .filter_map(|(number, offset)| {
            let mut index = *offset;
            parse_value(&plain, &mut index, 0).map(|value| (*number, value))
        })
        .collect()
}

/// 文件尾最后一个 `/Root N G R`（传统 trailer 和交叉引用流的字典都写在明文里）。
fn trailer_root(bytes: &[u8]) -> Option<u32> {
    let mut found = None;
    let mut cursor = 0;
    while let Some(at) = find_keyword(bytes, b"/Root", cursor) {
        cursor = at + 5;
        let mut index = at + 5;
        if let Some(Value::Ref(number)) = parse_value(bytes, &mut index, 0) {
            found = Some(number);
        }
    }
    found
}

/// 解析一个 PDF 值，`index` 走到值后面。认不出的记成 `Other`，读到文件尾或嵌套太深时为 `None`。
pub(super) fn parse_value(bytes: &[u8], index: &mut usize, depth: usize) -> Option<Value> {
    if depth > 32 {
        return None;
    }
    skip_noise(bytes, index);
    let byte = *bytes.get(*index)?;
    match byte {
        b'/' => {
            *index += 1;
            let start = *index;
            while *index < bytes.len() && !bytes[*index].is_ascii_whitespace() && !is_pdf_delim(bytes[*index]) {
                *index += 1;
            }
            Some(Value::Name(String::from_utf8_lossy(&bytes[start..*index]).into_owned()))
        }
        b'<' if bytes.get(*index + 1) == Some(&b'<') => {
            *index += 2;
            let mut entries = Vec::new();
            loop {
                skip_noise(bytes, index);
                match bytes.get(*index) {
                    None => return Some(Value::Dict(entries)),
                    Some(b'>') if bytes.get(*index + 1) == Some(&b'>') => {
                        *index += 2;
                        return Some(Value::Dict(entries));
                    }
                    Some(b'/') => {
                        let Some(Value::Name(key)) = parse_value(bytes, index, depth + 1) else {
                            return Some(Value::Dict(entries));
                        };
                        let value = parse_value(bytes, index, depth + 1)?;
                        entries.push((key, value));
                    }
                    Some(_) => {
                        // 字典里不是键的东西：跳过一个值，不让整个字典失败。
                        parse_value(bytes, index, depth + 1)?;
                    }
                }
            }
        }
        b'<' => parse_hex(bytes, index).map(Value::Str),
        b'(' => parse_literal(bytes, index).map(Value::Str),
        b'[' => {
            *index += 1;
            let mut items = Vec::new();
            loop {
                skip_noise(bytes, index);
                match bytes.get(*index) {
                    None => return Some(Value::Array(items)),
                    Some(b']') => {
                        *index += 1;
                        return Some(Value::Array(items));
                    }
                    Some(_) => items.push(parse_value(bytes, index, depth + 1)?),
                }
            }
        }
        b'+' | b'-' | b'.' | b'0'..=b'9' => {
            let first = parse_number(bytes, index)?;
            // `N G R` 是引用：后面跟着第二个数和 `R` 时合成一个引用，否则只是一个数。
            let mut look = *index;
            skip_noise(bytes, &mut look);
            if parse_number(bytes, &mut look).is_some() {
                skip_noise(bytes, &mut look);
                let ends = bytes.get(look + 1).is_none_or(|next| next.is_ascii_whitespace() || is_pdf_delim(*next));
                if bytes.get(look) == Some(&b'R') && ends {
                    *index = look + 1;
                    return Some(Value::Ref(first as u32));
                }
            }
            Some(Value::Num(first))
        }
        _ => {
            // 关键字（true、false、null、R、stream 之类）：读到分隔符为止。
            let start = *index;
            while *index < bytes.len() && !bytes[*index].is_ascii_whitespace() && !is_pdf_delim(bytes[*index]) {
                *index += 1;
            }
            if *index == start {
                *index += 1;
            }
            Some(Value::Other)
        }
    }
}

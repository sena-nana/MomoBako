//! PDF 文本提取和页面绘制（渲染器接好以前的止血）。
//!
//! 页数和页序来自页树（`/Type /Page`，见 `native_preview_pdf_objects.rs`），不再把文件里的每个流都当成一页。
//! 每页把内容流（含它调用的表单 XObject）里的文字画进页面；没有文字时画它调用的第一张 DCTDecode 图片。
//! ASCII85、LZW、Flate 按顺序解开。解不开的内容流记成该页失败，不假装成空白成功；失败的页照样能翻过去。

#[path = "native_preview_pdf_objects.rs"]
mod objects;
#[path = "native_preview_pdf_paint.rs"]
mod paint;

use super::super::bridge::{page_caption, NativeLoad, PageFrame};
use objects::{Document, Page, Value};

/// 画出位图的页数上限：每页 480×640 的位图约 1.2 MB。后面的页记成失败、写明原因，页数仍是真实的。
const RENDERED_PAGES: usize = 32;
/// 表单 XObject 最多往里套几层。
const FORM_DEPTH: usize = 8;

/// 抽出第一页文字。整份文件都打不开时返回错误。
#[cfg(test)]
fn read(bytes: &[u8]) -> Result<String, String> {
    load(bytes).map(|item| item.text)
}

/// 按页树分页。每一页要么有画面，要么带明确失败原因；只有不是 PDF、加密了或者一页也找不到时整份失败。
pub(super) fn load(bytes: &[u8]) -> Result<NativeLoad, String> {
    if !has_pdf_header(bytes) {
        eprintln!("Nana PDF 文件头不匹配");
        return Err("不是 PDF 文件".into());
    }
    if find_keyword(bytes, b"/Encrypt", 0).is_some() {
        eprintln!("Nana PDF 已加密，内容流解不开");
        return Err("加密的 PDF 暂不支持预览".into());
    }
    let document = Document::parse(bytes);
    let pages = document.pages();
    if pages.is_empty() {
        eprintln!("Nana PDF 里没有找到页面");
        return Err("PDF 里没有找到页面".into());
    }
    if pages.len() > RENDERED_PAGES {
        eprintln!("Nana PDF 共 {} 页，只画前 {RENDERED_PAGES} 页", pages.len());
    }
    let frames: Vec<PageFrame> = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            if index < RENDERED_PAGES {
                render_page(&document, page)
            } else {
                paint::failed(format!("只预览前 {RENDERED_PAGES} 页"))
            }
        })
        .collect();
    let text = super::limit_text(&page_caption(0, frames.len(), &frames[0]));
    Ok(NativeLoad { text, frames, mesh: None, paged: true })
}

/// 画一页：内容流和它调用的表单里的文字按点阵画；没有文字时画它调用的第一张 JPEG；都没有时这一页失败。
fn render_page(document: &Document, page: &Page) -> PageFrame {
    let content = match stream_text(document, &page.contents) {
        Ok(content) => content,
        Err(error) => {
            eprintln!("Nana PDF 这一页的内容流解不开：{error}");
            return paint::failed(error);
        }
    };
    let mut text = show_text(&content);
    let mut images = Vec::new();
    follow_xobjects(document, &content, page.resources.as_ref(), 0, &mut text, &mut images);
    if !text.trim().is_empty() {
        return paint::paint_text(&text);
    }
    let mut last_error = None;
    for jpeg in images {
        match paint::paint_jpeg(&jpeg) {
            Ok(frame) => return frame,
            Err(error) => last_error = Some(error),
        }
    }
    let message = last_error.unwrap_or_else(|| "这一页没有可绘制的内容".to_string());
    eprintln!("Nana PDF {message}");
    paint::failed(message)
}

/// 把这些内容流按顺序解开、拼起来（中间隔一个换行）。有一个解不开就交回它的原因。
fn stream_text(document: &Document, streams: &[Value]) -> Result<Vec<u8>, String> {
    let mut content = Vec::new();
    for stream in streams {
        let Value::Ref(number) = stream else {
            continue;
        };
        let Some((dict, data)) = document.stream(*number) else {
            continue;
        };
        if let paint::Decoded::Text(plain) = paint::decode(dict, data)? {
            content.extend_from_slice(&plain);
            content.push(b'\n');
        }
    }
    Ok(content)
}

/// 内容里 `Do` 调用的 XObject：表单把它的文字接到 `text` 后面（再看它自己调用的），DCTDecode 的图片收进 `images`。
fn follow_xobjects(document: &Document, content: &[u8], resources: Option<&Value>, depth: usize, text: &mut String, images: &mut Vec<Vec<u8>>) {
    if depth >= FORM_DEPTH {
        return;
    }
    let Some(xobjects) = resources.map(|resources| document.resolve(resources)).and_then(|resources| resources.get("XObject")) else {
        return;
    };
    let xobjects = document.resolve(xobjects).clone();
    for name in invoked_xobjects(content) {
        let Some(Value::Ref(number)) = xobjects.get(&name) else {
            continue;
        };
        let (Some(object), Some((dict, data))) = (document.object(*number), document.stream(*number)) else {
            continue;
        };
        let subtype = match object.value.get("Subtype") {
            Some(Value::Name(subtype)) => subtype.as_str(),
            _ => "",
        };
        match (subtype, paint::decode(dict, data)) {
            ("Image", Ok(paint::Decoded::Jpeg(jpeg))) => images.push(jpeg),
            ("Form", Ok(paint::Decoded::Text(form))) => {
                let inner = show_text(&form);
                if !inner.trim().is_empty() {
                    if !text.is_empty() && !text.ends_with('\n') {
                        text.push('\n');
                    }
                    text.push_str(&inner);
                }
                let own = object.value.get("Resources").or(resources);
                follow_xobjects(document, &form, own, depth + 1, text, images);
            }
            (_, Err(error)) => eprintln!("Nana PDF XObject /{name} 解不开：{error}"),
            _ => {}
        }
    }
}

/// 内容流里 `/名字 Do` 调用的 XObject 名字，按出现顺序。内嵌图片 `BI … ID … EI` 的二进制数据跳过。
fn invoked_xobjects(content: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut index = 0;
    let mut last_name: Option<String> = None;
    while index < content.len() {
        let start = index;
        match objects::parse_value(content, &mut index, 0) {
            Some(Value::Name(name)) => last_name = Some(name),
            Some(Value::Other) => {
                let token = content[start..index].trim_ascii_start();
                if token == b"Do" {
                    names.extend(last_name.take());
                } else if token == b"ID" {
                    index = find_keyword(content, b"EI", index).map(|end| end + 2).unwrap_or(content.len());
                    last_name = None;
                } else {
                    last_name = None;
                }
            }
            Some(_) => last_name = None,
            None => break,
        }
        if index == start {
            index += 1;
        }
    }
    names
}

fn has_pdf_header(bytes: &[u8]) -> bool {
    bytes.windows(5).take(1024).any(|window| window == b"%PDF-")
}

fn stream_data_start(bytes: &[u8], after_keyword: usize) -> Option<usize> {
    match bytes.get(after_keyword) {
        Some(b'\r') if bytes.get(after_keyword + 1) == Some(&b'\n') => Some(after_keyword + 2),
        Some(b'\r' | b'\n') => Some(after_keyword + 1),
        _ => None,
    }
}

fn dict_before(bytes: &[u8], stream_at: usize) -> &[u8] {
    let mut end = stream_at;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end < 2 || bytes[end - 2] != b'>' || bytes[end - 1] != b'>' {
        return b"";
    }
    let mut depth = 0usize;
    let mut index = end;
    while index >= 2 {
        if bytes[index - 2] == b'>' && bytes[index - 1] == b'>' {
            depth += 1;
            index -= 2;
            continue;
        }
        if bytes[index - 2] == b'<' && bytes[index - 1] == b'<' {
            depth -= 1;
            if depth == 0 {
                return &bytes[index - 2..end];
            }
            index -= 2;
            continue;
        }
        index -= 1;
    }
    b""
}

fn direct_length(dict: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = find_keyword(dict, b"/Length", from) {
        let mut index = at + b"/Length".len();
        while index < dict.len() && dict[index].is_ascii_whitespace() {
            index += 1;
        }
        let start = index;
        while index < dict.len() && dict[index].is_ascii_digit() {
            index += 1;
        }
        if start == index {
            from = at + b"/Length".len();
            continue;
        }
        let mut after = index;
        while after < dict.len() && dict[after].is_ascii_whitespace() {
            after += 1;
        }
        if after < dict.len() && dict[after].is_ascii_digit() {
            from = at + b"/Length".len();
            continue;
        }
        return std::str::from_utf8(&dict[start..index]).ok()?.parse().ok();
    }
    None
}

fn find_keyword(bytes: &[u8], keyword: &[u8], from: usize) -> Option<usize> {
    let mut index = from;
    while index + keyword.len() <= bytes.len() {
        if &bytes[index..index + keyword.len()] == keyword
            && (index == 0 || is_pdf_delim(bytes[index - 1]))
            && (index + keyword.len() == bytes.len() || is_pdf_delim(bytes[index + keyword.len()]))
        {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn is_pdf_delim(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

enum Operand {
    Str(Vec<u8>),
    Num(f64),
    Arr(Vec<Operand>),
}

/// 从内容流取出 Tj / TJ 的字面量。数组里过宽的负间距补一个空格。
fn show_text(stream: &[u8]) -> String {
    let mut index = 0;
    let mut stack = Vec::new();
    let mut out = String::new();
    while index < stream.len() {
        skip_noise(stream, &mut index);
        if index >= stream.len() {
            break;
        }
        let byte = stream[index];
        if byte == b'(' {
            let Some(text) = parse_literal(stream, &mut index) else { break };
            stack.push(Operand::Str(text));
            continue;
        }
        if byte == b'<' {
            if stream.get(index + 1) == Some(&b'<') {
                if !skip_dict(stream, &mut index) {
                    break;
                }
                continue;
            }
            let Some(text) = parse_hex(stream, &mut index) else { break };
            stack.push(Operand::Str(text));
            continue;
        }
        if byte == b'[' {
            let Some(items) = parse_array(stream, &mut index) else { break };
            stack.push(Operand::Arr(items));
            continue;
        }
        if byte == b'/' {
            index += 1;
            while index < stream.len() && is_regular(stream[index]) {
                index += 1;
            }
            continue;
        }
        if byte == b'+' || byte == b'-' || byte == b'.' || byte.is_ascii_digit() {
            if parse_number(stream, &mut index).map(|number| stack.push(Operand::Num(number))).is_none() {
                index += 1;
            }
            continue;
        }
        let start = index;
        while index < stream.len() && is_regular(stream[index]) {
            index += 1;
        }
        if start == index {
            index += 1;
            continue;
        }
        apply_operator(&stream[start..index], &stack, &mut out);
        stack.clear();
    }
    out
}

fn apply_operator(operator: &[u8], stack: &[Operand], out: &mut String) {
    match operator {
        b"Tj" | b"TJ" | b"'" | b"\"" => {
            if matches!(operator, b"'" | b"\"") && !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if let Some(item) = stack.iter().rev().find(|item| matches!(item, Operand::Str(_) | Operand::Arr(_))) {
                append_operand(item, out);
            }
        }
        b"T*" => {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
        }
        b"Td" | b"TD" => {
            if let Some(Operand::Num(y)) = stack.last() {
                if *y != 0.0 && !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
        _ => {}
    }
}

fn append_operand(item: &Operand, out: &mut String) {
    match item {
        Operand::Str(bytes) => out.push_str(&pdf_text(bytes)),
        Operand::Arr(items) => {
            for part in items {
                match part {
                    Operand::Str(bytes) => out.push_str(&pdf_text(bytes)),
                    Operand::Num(number) if *number <= -200.0 => out.push(' '),
                    _ => {}
                }
            }
        }
        Operand::Num(_) => {}
    }
}

fn parse_array(data: &[u8], index: &mut usize) -> Option<Vec<Operand>> {
    *index += 1;
    let mut items = Vec::new();
    loop {
        skip_noise(data, index);
        if *index >= data.len() {
            return None;
        }
        let byte = data[*index];
        if byte == b']' {
            *index += 1;
            return Some(items);
        }
        if byte == b'(' {
            items.push(Operand::Str(parse_literal(data, index)?));
            continue;
        }
        if byte == b'<' {
            if data.get(*index + 1) == Some(&b'<') {
                if !skip_dict(data, index) {
                    return None;
                }
                continue;
            }
            items.push(Operand::Str(parse_hex(data, index)?));
            continue;
        }
        if byte == b'[' {
            items.push(Operand::Arr(parse_array(data, index)?));
            continue;
        }
        if byte == b'+' || byte == b'-' || byte == b'.' || byte.is_ascii_digit() {
            items.push(Operand::Num(parse_number(data, index)?));
            continue;
        }
        if byte == b'/' {
            *index += 1;
            while *index < data.len() && is_regular(data[*index]) {
                *index += 1;
            }
            continue;
        }
        *index += 1;
    }
}

fn parse_literal(data: &[u8], index: &mut usize) -> Option<Vec<u8>> {
    *index += 1;
    let mut out = Vec::new();
    let mut depth = 1usize;
    while *index < data.len() {
        let byte = data[*index];
        *index += 1;
        if byte == b'\\' {
            if *index >= data.len() {
                break;
            }
            let escaped = data[*index];
            *index += 1;
            match escaped {
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'b' => out.push(0x08),
                b'f' => out.push(0x0c),
                b'(' | b')' | b'\\' => out.push(escaped),
                b'\n' => {}
                b'\r' => {
                    if data.get(*index) == Some(&b'\n') {
                        *index += 1;
                    }
                }
                b'0'..=b'7' => {
                    let mut value = u16::from(escaped - b'0');
                    for _ in 0..2 {
                        if matches!(data.get(*index), Some(b'0'..=b'7')) {
                            value = (value << 3) + u16::from(data[*index] - b'0');
                            *index += 1;
                        } else {
                            break;
                        }
                    }
                    out.push(value as u8);
                }
                other => out.push(other),
            }
            continue;
        }
        if byte == b'(' {
            depth += 1;
            out.push(byte);
            continue;
        }
        if byte == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(out);
            }
            out.push(byte);
            continue;
        }
        out.push(byte);
    }
    None
}

fn parse_hex(data: &[u8], index: &mut usize) -> Option<Vec<u8>> {
    *index += 1;
    let mut hex = Vec::new();
    while *index < data.len() && data[*index] != b'>' {
        let byte = data[*index];
        *index += 1;
        if byte.is_ascii_hexdigit() {
            hex.push(byte);
        } else if !byte.is_ascii_whitespace() {
            return None;
        }
    }
    if data.get(*index) != Some(&b'>') {
        return None;
    }
    *index += 1;
    if hex.len() % 2 == 1 {
        hex.push(b'0');
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.chunks_exact(2) {
        out.push((hex_value(chunk[0]) << 4) | hex_value(chunk[1]));
    }
    Some(out)
}

fn hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

fn parse_number(data: &[u8], index: &mut usize) -> Option<f64> {
    let start = *index;
    if matches!(data.get(*index), Some(b'+' | b'-')) {
        *index += 1;
    }
    let mut saw_digit = false;
    let mut saw_dot = false;
    while *index < data.len() {
        let byte = data[*index];
        if byte.is_ascii_digit() {
            saw_digit = true;
            *index += 1;
            continue;
        }
        if byte == b'.' && !saw_dot {
            saw_dot = true;
            *index += 1;
            continue;
        }
        break;
    }
    if !saw_digit {
        *index = start;
        return None;
    }
    std::str::from_utf8(&data[start..*index]).ok()?.parse().ok()
}

fn skip_dict(data: &[u8], index: &mut usize) -> bool {
    if data.get(*index) != Some(&b'<') || data.get(*index + 1) != Some(&b'<') {
        return false;
    }
    *index += 2;
    let mut depth = 1usize;
    while *index + 1 < data.len() && depth > 0 {
        if data[*index] == b'<' && data[*index + 1] == b'<' {
            depth += 1;
            *index += 2;
            continue;
        }
        if data[*index] == b'>' && data[*index + 1] == b'>' {
            depth -= 1;
            *index += 2;
            continue;
        }
        *index += 1;
    }
    depth == 0
}

fn skip_noise(data: &[u8], index: &mut usize) {
    loop {
        while *index < data.len() && data[*index].is_ascii_whitespace() {
            *index += 1;
        }
        if data.get(*index) == Some(&b'%') {
            while *index < data.len() && data[*index] != b'\n' && data[*index] != b'\r' {
                *index += 1;
            }
            continue;
        }
        break;
    }
}

fn is_regular(byte: u8) -> bool {
    !byte.is_ascii_whitespace() && !is_pdf_delim(byte)
}

fn pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units = bytes[2..]
            .chunks(2)
            .filter(|chunk| chunk.len() == 2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| bytes.iter().map(|byte| char::from(*byte)).collect())
}

/// 测试用的最小 PDF：目录（1 号）、页树（2 号），每页一个页对象和一个内容流，内容流字典里另加 `extra`
/// （比如滤镜），对象按页的顺序编号。
#[cfg(test)]
pub(super) fn sample(pages: &[(&str, &[u8])]) -> Vec<u8> {
    let kids: Vec<String> = (0..pages.len()).map(|index| format!("{} 0 R", 3 + index * 2)).collect();
    let mut pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_vec();
    pdf.extend_from_slice(format!("2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n", kids.join(" "), pages.len()).as_bytes());
    for (index, (extra, data)) in pages.iter().enumerate() {
        let page = 3 + index * 2;
        let contents = page + 1;
        pdf.extend_from_slice(format!("{page} 0 obj\n<< /Type /Page /Parent 2 0 R /Contents {contents} 0 R >>\nendobj\n").as_bytes());
        pdf.extend_from_slice(format!("{contents} 0 obj\n<< /Length {} {extra} >>\nstream\n", data.len()).as_bytes());
        pdf.extend_from_slice(data);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
    }
    pdf.extend_from_slice(b"trailer\n<< /Root 1 0 R >>\n%%EOF\n");
    pdf
}

#[cfg(test)]
#[path = "native_preview_pdf_tests.rs"]
mod tests;

//! PDF 文本提取和页面绘制。
//!
//! 内容流里的文字画进页面，DCTDecode 的 JPEG 也画进去。
//! ASCII85、LZW、Flate 按顺序解开。解不开的流记成该页失败，不假装成空白成功。

#[path = "native_preview_pdf_paint.rs"]
mod paint;

use super::super::bridge::{page_caption, NativeLoad, PageFrame};

/// 抽出第一页文字。整份文件都画不出来时返回错误。
#[cfg(test)]
fn read(bytes: &[u8]) -> Result<String, String> {
    load(bytes).map(|item| item.text)
}

/// 按内容流分页。每一页要么有画面，要么带明确失败原因。
pub(super) fn load(bytes: &[u8]) -> Result<NativeLoad, String> {
    if !has_pdf_header(bytes) {
        eprintln!("Nana PDF 文件头不匹配");
        return Err("不是 PDF 文件".into());
    }
    let mut pages = collect_pages(bytes);
    if pages.len() > 32 {
        eprintln!("Nana PDF 只保留前 32 页");
        pages.truncate(32);
    }
    if pages.is_empty() {
        eprintln!("Nana PDF 没有内容流");
        return Err("PDF 没有可提取的文字".into());
    }
    let all_failed = pages.iter().all(|page| page.error.is_some() && page.text.trim().is_empty());
    if all_failed {
        let message = pages[0].error.clone().unwrap_or_else(|| "PDF 没有可提取的文字".into());
        eprintln!("Nana PDF 页面失败：{message}");
        return Err(message);
    }
    let text = super::limit_text(&page_caption(0, pages.len(), &pages[0]));
    Ok(NativeLoad { text, frames: pages, mesh: None, paged: true })
}

fn collect_pages(bytes: &[u8]) -> Vec<PageFrame> {
    let mut pages = Vec::new();
    let mut cursor = 0;
    while let Some(at) = find_keyword(bytes, b"stream", cursor) {
        let Some(data_start) = stream_data_start(bytes, at + 6) else {
            cursor = at + 6;
            continue;
        };
        let dict = dict_before(bytes, at);
        let end_kw = find_keyword(bytes, b"endstream", data_start);
        let data_end = match (direct_length(dict), end_kw) {
            (Some(len), end) => {
                let by_len = data_start.saturating_add(len).min(bytes.len());
                end.map(|pos| by_len.min(pos)).unwrap_or(by_len)
            }
            (None, Some(pos)) => pos,
            (None, None) => {
                cursor = at + 6;
                continue;
            }
        };
        if data_end > data_start {
            pages.push(page_from_stream(dict, &bytes[data_start..data_end]));
        }
        let next = end_kw.map(|pos| pos + b"endstream".len()).unwrap_or(data_end);
        cursor = if next > at { next } else { at + 6 };
    }
    pages
}

fn page_from_stream(dict: &[u8], data: &[u8]) -> PageFrame {
    match paint::decode(dict, data) {
        Ok(paint::Decoded::Text(plain)) => {
            let text = show_text(&plain);
            if text.trim().is_empty() {
                let message = "这一页没有可绘制的内容".to_string();
                eprintln!("Nana PDF {message}");
                paint::failed(message)
            } else {
                paint::paint_text(&text)
            }
        }
        Ok(paint::Decoded::Jpeg(jpeg)) => match paint::paint_jpeg(&jpeg) {
            Ok(frame) => frame,
            Err(error) => paint::failed(error),
        },
        Err(error) => paint::failed(error),
    }
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn zlib_bytes(plain: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(plain).expect("压缩");
        encoder.finish().expect("结束")
    }

    fn push_stream(out: &mut Vec<u8>, dict: &str, data: &[u8]) {
        out.extend_from_slice(b"<< ");
        out.extend_from_slice(dict.as_bytes());
        out.extend_from_slice(b" >>\nstream\n");
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream\n");
    }

    /// 页眉 B 必须有左边竖干。和 8 共用点阵时，浅色预览会读成 MOMO8AKO。
    #[test]
    fn b_glyph_has_a_stem_that_eight_does_not() {
        let bee = paint::paint_text("B");
        let eight = paint::paint_text("8");
        let index = ((24 * 480 + 16) * 4) as usize;
        assert_eq!(&bee.rgba[index..index + 3], &[20, 20, 20], "B 左上角应是竖干");
        assert_eq!(&eight.rgba[index..index + 3], &[255, 255, 255], "8 左上角应留白");
    }

    #[test]
    fn uncompressed_tj_still_extracts() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Length 14 >>\nstream\n(MomoBako) Tj\nendstream\nendobj\n%%EOF\n";
        assert!(read(pdf).expect("pdf").contains("MomoBako"));
        let tj = b"%PDF-1.4\n<< /Length 22 >>\nstream\n[(Mo) 20 (moBako)] TJ\nendstream\n%%EOF\n";
        assert!(read(tj).expect("tj").contains("MomoBako"));
    }

    #[test]
    fn flate_stream_extracts_inflated_text() {
        let plain = b"(Inflated) Tj\n";
        let compressed = zlib_bytes(plain);
        let mut pdf = b"%PDF-1.4\n".to_vec();
        push_stream(
            &mut pdf,
            &format!("/Length {} /Filter /FlateDecode /Type /XObject", compressed.len()),
            &compressed,
        );
        assert!(!pdf.windows(plain.len()).any(|window| window == plain), "内容流不该带着明文");
        let text = read(&pdf).expect("flate");
        assert!(text.contains("Inflated"), "{text}");
    }

    #[test]
    fn flate_array_and_corrupt_stream_keep_other_text() {
        let plain = b"(Inflated) Tj\n";
        let compressed = zlib_bytes(plain);
        let mut pdf = b"%PDF-1.4\n".to_vec();
        push_stream(&mut pdf, "/Filter /FlateDecode /Length 4", b"xxxx");
        push_stream(&mut pdf, &format!("/Filter [/FlateDecode] /Length {}", compressed.len()), &compressed);
        let loaded = load(&pdf).expect("mixed");
        assert_eq!(loaded.frames.len(), 2, "{}", loaded.text);
        let failed = loaded.frames[0].error.as_deref().unwrap_or("");
        assert!(failed.contains("FlateDecode"), "{failed}");
        assert!(loaded.frames[1].text.contains("Inflated"), "{}", loaded.frames[1].text);
    }

    #[test]
    fn other_filters_and_bad_deflate_do_not_panic() {
        for name in ["/ASCII85Decode", "/LZWDecode", "/DCTDecode"] {
            let mut pdf = b"%PDF-1.4\n".to_vec();
            push_stream(&mut pdf, &format!("/Filter {name} /Length 8"), b"(No) Tj\n");
            assert!(read(&pdf).is_err(), "{name}");
        }
        let pdf = b"%PDF-1.4\n<< /Filter /FlateDecode >>\nstream\nxxxx\nendstream\n";
        assert!(read(pdf).is_err());
        let truncated = b"%PDF-1.4\n<< /Filter /FlateDecode /Length 2 >>\nstream\n\x78\x9c\nendstream\n";
        assert!(read(truncated).is_err());
    }
}

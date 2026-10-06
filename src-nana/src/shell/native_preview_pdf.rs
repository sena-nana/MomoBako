//! 未压缩文字 PDF 的文本提取。
//!
//! 只认内容流里的 `(text) Tj` 和 `TJ`。带 Filter 的流直接跳过，不画页面。

/// 抽出可见文字。坏文件或没有文字时返回错误。
pub(super) fn read(bytes: &[u8]) -> Result<String, String> {
    if !has_pdf_header(bytes) {
        eprintln!("Nana PDF 文件头不匹配");
        return Err("不是 PDF 文件".into());
    }
    let text = extract(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        eprintln!("Nana PDF 没有可提取的文字");
        return Err("PDF 没有可提取的文字".into());
    }
    Ok(super::limit_text(trimmed))
}

fn has_pdf_header(bytes: &[u8]) -> bool {
    bytes.windows(5).take(1024).any(|window| window == b"%PDF-")
}

fn extract(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut cursor = 0;
    let mut noted_filter = false;
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
        if has_filter(dict) {
            if !noted_filter {
                eprintln!("Nana PDF 跳过带 Filter 的流");
                noted_filter = true;
            }
        } else if data_end > data_start {
            append_piece(&mut out, &show_text(&bytes[data_start..data_end]));
        }
        let next = end_kw.map(|pos| pos + b"endstream".len()).unwrap_or(data_end);
        cursor = if next > at { next } else { at + 6 };
    }
    out
}

fn append_piece(out: &mut String, piece: &str) {
    let piece = piece.trim();
    if piece.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(piece);
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

fn has_filter(dict: &[u8]) -> bool {
    find_keyword(dict, b"/Filter", 0).is_some()
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

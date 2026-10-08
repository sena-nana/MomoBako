//! 把 PDF 内容流解成文字或图片，再画成一页 RGBA。
//!
//! ASCII85、LZW 和 Flate 按滤镜顺序解开。DCTDecode 交给 `image` 画进页面。
//! 解不开的滤镜返回明确错误，不把空白页当成成功。

use super::super::super::bridge::PageFrame;

const PAGE_W: u32 = 480;
const PAGE_H: u32 = 640;
/// 页宽会撑满预览列。5×7 点阵在列宽里只有几个像素，页眉按两倍点阵画才能读到。
const GLYPH_SCALE: i32 = 2;

pub(super) enum Decoded {
    Text(Vec<u8>),
    Jpeg(Vec<u8>),
}

/// 按字典里的滤镜顺序解码。未知滤镜直接失败。
pub(super) fn decode(dict: &[u8], data: &[u8]) -> Result<Decoded, String> {
    let names = filter_names(dict)?;
    if names.is_empty() {
        return Ok(Decoded::Text(data.to_vec()));
    }
    let mut bytes = data.to_vec();
    let mut image = false;
    for name in names {
        if name == "DCTDecode" {
            image = true;
            continue;
        }
        if image {
            let message = format!("{name} 不能排在 DCTDecode 后面");
            eprintln!("Nana PDF 滤镜顺序无效：{message}");
            return Err(message);
        }
        bytes = match name {
            "FlateDecode" => inflate(&bytes)?,
            "ASCII85Decode" => ascii85(&bytes)?,
            "LZWDecode" => lzw(&bytes)?,
            "ASCIIHexDecode" => ascii_hex(&bytes)?,
            other => {
                let message = format!("无法解码 {other}");
                eprintln!("Nana PDF {message}");
                return Err(message);
            }
        };
    }
    if image {
        Ok(Decoded::Jpeg(bytes))
    } else {
        Ok(Decoded::Text(bytes))
    }
}

pub(super) fn paint_text(text: &str) -> PageFrame {
    let mut rgba = white_page();
    draw_border(&mut rgba);
    let mut x = 16i32;
    let mut y = 24i32;
    let advance = 8 * GLYPH_SCALE;
    let line = 14 * GLYPH_SCALE;
    for ch in text.chars() {
        if ch == '\n' || x > PAGE_W as i32 - 24 {
            x = 16;
            y += line;
            if ch == '\n' {
                continue;
            }
        }
        if y > PAGE_H as i32 - 16 {
            break;
        }
        draw_glyph(&mut rgba, x, y, ch);
        x += advance;
    }
    PageFrame { width: PAGE_W, height: PAGE_H, rgba, text: text.to_string(), error: None }
}

pub(super) fn paint_jpeg(jpeg: &[u8]) -> Result<PageFrame, String> {
    let image = image::load_from_memory(jpeg).map_err(|error| {
        eprintln!("Nana PDF DCTDecode 无法解码：{error}");
        format!("DCTDecode 无法解码：{error}")
    })?;
    let rgba_image = image.to_rgba8();
    let (src_w, src_h) = rgba_image.dimensions();
    if src_w == 0 || src_h == 0 {
        eprintln!("Nana PDF DCTDecode 尺寸是空的");
        return Err("DCTDecode 尺寸是空的".into());
    }
    let mut rgba = white_page();
    draw_border(&mut rgba);
    let max_w = PAGE_W - 32;
    let max_h = PAGE_H - 32;
    let scale = (max_w as f32 / src_w as f32).min(max_h as f32 / src_h as f32).min(1.0);
    let dst_w = ((src_w as f32) * scale).round().max(1.0) as u32;
    let dst_h = ((src_h as f32) * scale).round().max(1.0) as u32;
    let origin_x = (PAGE_W - dst_w) / 2;
    let origin_y = (PAGE_H - dst_h) / 2;
    for y in 0..dst_h {
        for x in 0..dst_w {
            let sx = ((x as f32 / dst_w as f32) * src_w as f32) as u32;
            let sy = ((y as f32 / dst_h as f32) * src_h as f32) as u32;
            let pixel = rgba_image.get_pixel(sx.min(src_w - 1), sy.min(src_h - 1));
            put(&mut rgba, origin_x + x, origin_y + y, pixel[0], pixel[1], pixel[2], pixel[3]);
        }
    }
    Ok(PageFrame { width: PAGE_W, height: PAGE_H, rgba, text: String::new(), error: None })
}

pub(super) fn failed(message: String) -> PageFrame {
    PageFrame { width: 0, height: 0, rgba: Vec::new(), text: String::new(), error: Some(message) }
}

fn filter_names(dict: &[u8]) -> Result<Vec<&'static str>, String> {
    const KNOWN: &[(&str, &str)] = &[
        ("ASCIIHexDecode", "ASCIIHexDecode"),
        ("ASCII85Decode", "ASCII85Decode"),
        ("LZWDecode", "LZWDecode"),
        ("FlateDecode", "FlateDecode"),
        ("RunLengthDecode", "RunLengthDecode"),
        ("CCITTFaxDecode", "CCITTFaxDecode"),
        ("JBIG2Decode", "JBIG2Decode"),
        ("DCTDecode", "DCTDecode"),
        ("JPXDecode", "JPXDecode"),
        ("Crypt", "Crypt"),
    ];
    let mut names = Vec::new();
    let mut index = 0usize;
    while index + 1 < dict.len() {
        if dict[index] == b'/' {
            let start = index + 1;
            let mut end = start;
            while end < dict.len() && is_name(dict[end]) {
                end += 1;
            }
            let token = std::str::from_utf8(&dict[start..end]).unwrap_or("");
            if let Some((_, canonical)) = KNOWN.iter().find(|(name, _)| *name == token) {
                names.push(*canonical);
            }
            index = end;
            continue;
        }
        index += 1;
    }
    if find_filter_key(dict) && names.is_empty() {
        eprintln!("Nana PDF 有 Filter 但认不出名字");
        return Err("无法解码未知 Filter".into());
    }
    Ok(names)
}

fn find_filter_key(dict: &[u8]) -> bool {
    dict.windows(7).any(|window| window == b"/Filter")
}

fn is_name(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'#'
}

fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(|error| {
        eprintln!("Nana PDF FlateDecode 解压失败：{error}");
        format!("FlateDecode 解压失败：{error}")
    })?;
    Ok(out)
}

/// Adobe ASCII85。`z` 表示四个 0，`~>` 结束。空白忽略。
fn ascii85(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut filled = 0usize;
    let mut index = 0usize;
    while index < data.len() {
        let byte = data[index];
        index += 1;
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'~' {
            break;
        }
        if byte == b'z' && filled == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(33..=117).contains(&byte) {
            eprintln!("Nana PDF ASCII85 有非法字符");
            return Err("ASCII85Decode 无法解码".into());
        }
        group[filled] = byte;
        filled += 1;
        if filled == 5 {
            push_group(&mut out, &group, 5)?;
            filled = 0;
        }
    }
    if filled > 0 {
        if filled == 1 {
            eprintln!("Nana PDF ASCII85 末组只有一个字符");
            return Err("ASCII85Decode 无法解码".into());
        }
        for slot in &mut group[filled..] {
            *slot = b'u';
        }
        push_group(&mut out, &group, filled)?;
    }
    Ok(out)
}

fn push_group(out: &mut Vec<u8>, group: &[u8; 5], count: usize) -> Result<(), String> {
    let mut value = 0u32;
    for byte in group {
        value = value.checked_mul(85).and_then(|value| value.checked_add(u32::from(byte - 33))).ok_or_else(|| {
            eprintln!("Nana PDF ASCII85 数值溢出");
            "ASCII85Decode 无法解码".to_string()
        })?;
    }
    let bytes = value.to_be_bytes();
    let take = count - 1;
    out.extend_from_slice(&bytes[..take]);
    Ok(())
}

/// PDF LZW。码宽从 9 位起，Clear 是 256，EOD 是 257，提前一位加宽。
fn lzw(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut bits = BitReader::new(data);
    let mut width = 9u32;
    let mut table: Vec<Vec<u8>> = (0..258).map(|code| if code < 256 { vec![code as u8] } else { Vec::new() }).collect();
    let mut previous: Option<Vec<u8>> = None;
    let mut out = Vec::new();
    loop {
        let Some(code) = bits.read(width) else {
            eprintln!("Nana PDF LZW 在结束码之前截断");
            return Err("LZWDecode 无法解码".into());
        };
        if code == 256 {
            table.truncate(258);
            width = 9;
            previous = None;
            continue;
        }
        if code == 257 {
            return Ok(out);
        }
        let entry = if (code as usize) < table.len() {
            table[code as usize].clone()
        } else if previous.is_some() && code as usize == table.len() {
            let mut entry = previous.clone().unwrap_or_default();
            if let Some(first) = entry.first().copied() {
                entry.push(first);
            }
            entry
        } else {
            eprintln!("Nana PDF LZW 码 {code} 不在表里");
            return Err("LZWDecode 无法解码".into());
        };
        if entry.is_empty() && code >= 258 {
            eprintln!("Nana PDF LZW 得到空串");
            return Err("LZWDecode 无法解码".into());
        }
        out.extend_from_slice(&entry);
        if let Some(prev) = &previous {
            let mut next = prev.clone();
            if let Some(first) = entry.first().copied() {
                next.push(first);
            }
            table.push(next);
            let limit = 1u32 << width;
            if table.len() as u32 == limit - 1 && width < 12 {
                width += 1;
            }
        }
        previous = Some(entry);
    }
}

fn ascii_hex(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut hex = Vec::new();
    for byte in data {
        if *byte == b'>' {
            break;
        }
        if byte.is_ascii_hexdigit() {
            hex.push(*byte);
        } else if !byte.is_ascii_whitespace() {
            eprintln!("Nana PDF ASCIIHex 有非法字符");
            return Err("ASCIIHexDecode 无法解码".into());
        }
    }
    if hex.len() % 2 == 1 {
        hex.push(b'0');
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.chunks_exact(2) {
        out.push((hex_value(chunk[0]) << 4) | hex_value(chunk[1]));
    }
    Ok(out)
}

fn hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

struct BitReader<'a> {
    data: &'a [u8],
    index: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, index: 0, bit: 0 }
    }

    fn read(&mut self, width: u32) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..width {
            if self.index >= self.data.len() {
                return None;
            }
            let shift = 7 - self.bit;
            value = (value << 1) | u32::from((self.data[self.index] >> shift) & 1);
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.index += 1;
            }
        }
        Some(value)
    }
}

fn white_page() -> Vec<u8> {
    vec![255u8; (PAGE_W * PAGE_H * 4) as usize]
}

fn draw_border(rgba: &mut [u8]) {
    for x in 0..PAGE_W {
        put(rgba, x, 0, 40, 40, 40, 255);
        put(rgba, x, PAGE_H - 1, 40, 40, 40, 255);
    }
    for y in 0..PAGE_H {
        put(rgba, 0, y, 40, 40, 40, 255);
        put(rgba, PAGE_W - 1, y, 40, 40, 40, 255);
    }
}

fn draw_glyph(rgba: &mut [u8], origin_x: i32, origin_y: i32, ch: char) {
    let rows = glyph_rows(ch);
    for (row, bits) in rows.iter().enumerate() {
        for col in 0..5 {
            if bits & (1 << (4 - col)) != 0 {
                for sy in 0..GLYPH_SCALE {
                    for sx in 0..GLYPH_SCALE {
                        put(
                            rgba,
                            (origin_x + col * GLYPH_SCALE + sx) as u32,
                            (origin_y + row as i32 * GLYPH_SCALE + sy) as u32,
                            20,
                            20,
                            20,
                            255,
                        );
                    }
                }
            }
        }
    }
}

fn glyph_rows(ch: char) -> [u8; 7] {
    match ch {
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        '0' | 'O' | 'o' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        '1' | 'I' | 'l' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        '3' => [0x0E, 0x11, 0x01, 0x06, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' | 'S' | 's' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x0E, 0x10, 0x1E, 0x11, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        // 8 两边都收圆。B 左边是整条竖干，页眉 MomoBako 才不会读成 8。
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        'B' | 'b' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x01, 0x0E],
        'A' | 'a' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'M' | 'm' => [0x11, 0x1B, 0x15, 0x11, 0x11, 0x11, 0x11],
        'k' | 'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        '/' => [0x01, 0x02, 0x04, 0x08, 0x10, 0x00, 0x00],
        _ => [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F],
    }
}

fn put(rgba: &mut [u8], x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
    if x >= PAGE_W || y >= PAGE_H {
        return;
    }
    let index = ((y * PAGE_W + x) * 4) as usize;
    rgba[index] = r;
    rgba[index + 1] = g;
    rgba[index + 2] = b;
    rgba[index + 3] = a;
}

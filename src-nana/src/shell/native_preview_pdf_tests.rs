//! PDF 止血版的测试：页数和页序来自页树，不是流的个数；对象流里的页对象也认；解不开的页单独失败、照样能翻；
//! 扫描页画它的 JPEG；表单里的文字也画；超过上限的页列出来但不画；加密的和没有页的整份失败。

use std::io::Write;

use super::*;

fn zlib_bytes(plain: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(plain).expect("压缩");
    encoder.finish().expect("结束")
}

/// 一张 16×16 的纯色 JPEG。
fn jpeg(color: [u8; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(16, 16, image::Rgb(color)))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .expect("JPEG 编码");
    bytes
}

/// 逐个写对象的 PDF。
struct Builder(Vec<u8>);

impl Builder {
    fn new() -> Self {
        Self(b"%PDF-1.5\n".to_vec())
    }

    fn obj(mut self, number: u32, body: &str) -> Self {
        self.0.extend_from_slice(format!("{number} 0 obj\n{body}\nendobj\n").as_bytes());
        self
    }

    fn stream(mut self, number: u32, dict: &str, data: &[u8]) -> Self {
        self.0.extend_from_slice(format!("{number} 0 obj\n<< /Length {} {dict} >>\nstream\n", data.len()).as_bytes());
        self.0.extend_from_slice(data);
        self.0.extend_from_slice(b"\nendstream\nendobj\n");
        self
    }

    fn finish(mut self, trailer: &str) -> Vec<u8> {
        self.0.extend_from_slice(format!("trailer\n{trailer}\n%%EOF\n").as_bytes());
        self.0
    }
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

/// 页数是页树里的页，字体、ToUnicode 这些不是内容的流不算页；页序跟着 `/Kids`，不跟对象编号。
#[test]
fn pages_come_from_the_page_tree_in_kids_order() {
    let pdf = Builder::new()
        .obj(1, "<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, "<< /Type /Pages /Kids [5 0 R 3 0 R] /Count 2 >>")
        .obj(3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 7 0 R >> >> >>")
        .stream(4, "", b"BT /F1 12 Tf (Second) Tj ET")
        .obj(5, "<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>")
        .stream(6, "", b"BT (First) Tj ET")
        .obj(7, "<< /Type /Font /Subtype /TrueType /FontFile2 8 0 R /ToUnicode 9 0 R >>")
        .stream(8, "/Length1 4", b"\x00\x01\x00\x00")
        .stream(9, "", b"/CIDInit /ProcSet findresource begin endcmap")
        .finish("<< /Size 10 /Root 1 0 R >>");
    let loaded = load(&pdf).expect("PDF 能读");
    assert_eq!(loaded.frames.len(), 2, "两页，不是五个流");
    assert!(loaded.frames[0].text.contains("First"), "{}", loaded.frames[0].text);
    assert!(loaded.frames[1].text.contains("Second"), "{}", loaded.frames[1].text);
    assert!(loaded.text.starts_with("1 / 2"), "{}", loaded.text);
}

/// PDF 1.5 把页树和页对象放进压缩的对象流（`/Type /ObjStm`）里，文件里看不到 `/Type /Page` 明文也要认得。
#[test]
fn pages_inside_an_object_stream_are_found() {
    let members = b"<< /Type /Catalog /Pages 2 0 R >> << /Type /Pages /Kids [3 0 R] /Count 1 >> << /Type /Page /Parent 2 0 R /Contents 4 0 R >>";
    let catalog = "<< /Type /Catalog /Pages 2 0 R >> ".len();
    let tree = "<< /Type /Pages /Kids [3 0 R] /Count 1 >> ".len();
    let header = format!("1 0 2 {catalog} 3 {} ", catalog + tree);
    let mut plain = header.clone().into_bytes();
    plain.extend_from_slice(members);
    let packed = zlib_bytes(&plain);
    let pdf = Builder::new()
        .stream(10, &format!("/Type /ObjStm /N 3 /First {} /Filter /FlateDecode", header.len()), &packed)
        .stream(4, "", b"(Packed) Tj")
        .stream(11, "/Type /XRef /Root 1 0 R /Size 12", b"\x01\x00\x00")
        .finish("<< /Size 12 >>");
    assert!(!pdf.windows(b"/Type /Page".len()).any(|window| window == b"/Type /Page"), "页对象只在压缩的对象流里");
    let loaded = load(&pdf).expect("PDF 能读");
    assert_eq!(loaded.frames.len(), 1);
    assert!(loaded.frames[0].text.contains("Packed"), "{}", loaded.frames[0].text);
}

/// 第一页的内容流解不开：这一页单独失败、写明原因，后面的页照常；整份不失败，失败的页也在页数里，能翻过去。
#[test]
fn a_broken_page_fails_alone_and_the_rest_still_load() {
    let compressed = zlib_bytes(b"(Inflated) Tj\n");
    let pdf = sample(&[("/Filter /FlateDecode", b"xxxx"), ("/Filter [/FlateDecode]", &compressed)]);
    let loaded = load(&pdf).expect("一页坏了整份照样能读");
    assert_eq!(loaded.frames.len(), 2, "{}", loaded.text);
    let failed = loaded.frames[0].error.as_deref().unwrap_or("");
    assert!(failed.contains("FlateDecode"), "{failed}");
    assert!(loaded.text.contains("1 / 2") && loaded.text.contains("FlateDecode"), "{}", loaded.text);
    assert!(loaded.frames[1].text.contains("Inflated"), "{}", loaded.frames[1].text);
}

/// 每一页都画不出来时也列出这些页，每页写自己的原因，不再把整份当成失败、看不到页数。
#[test]
fn every_page_failing_still_lists_the_pages() {
    let pdf = sample(&[("/Filter /JPXDecode", b"xxxx"), ("", b"0 0 m 10 10 l S")]);
    let loaded = load(&pdf).expect("页都在");
    assert_eq!(loaded.frames.len(), 2);
    assert!(loaded.frames[0].error.as_deref().is_some_and(|error| error.contains("无法解码 JPXDecode")));
    assert_eq!(loaded.frames[1].error.as_deref(), Some("这一页没有可绘制的内容"), "只有线条的页");
}

/// 字面量、TJ 数组、ASCII85 都照常抽字；解不开的滤镜和坏的压缩数据不 panic。
#[test]
fn text_operators_and_filters_extract_without_panicking() {
    let plain = read(&sample(&[("", b"(MomoBako) Tj")])).expect("Tj");
    assert!(plain.contains("MomoBako"), "{plain}");
    let array = read(&sample(&[("", b"[(Mo) 20 (moBako)] TJ")])).expect("TJ");
    assert!(array.contains("MomoBako"), "{array}");
    let ascii85 = read(&sample(&[("/Filter /ASCII85Decode", b"-qJ&$+B3(_~>")])).expect("ASCII85");
    assert!(ascii85.contains("Hi"), "{ascii85}");
    for filter in ["/Filter /ASCII85Decode", "/Filter /LZWDecode", "/Filter /DCTDecode", "/Filter /FlateDecode"] {
        let loaded = load(&sample(&[(filter, b"\x78\x9c(No) Tj\xff\xfe")])).expect(filter);
        assert!(loaded.frames[0].error.is_some(), "{filter} 解不开就是这一页失败");
    }
}

/// 只有一张图片的扫描页：画它用 `Do` 调用的 DCTDecode 图片；图片资源写在上一层页树节点上也认（资源继承）。
#[test]
fn a_scanned_page_paints_its_jpeg() {
    let picture = jpeg([30, 120, 220]);
    let pdf = Builder::new()
        .obj(1, "<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources << /XObject << /Im0 5 0 R >> >> >>")
        .obj(3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>")
        .stream(4, "", b"q 595 0 0 842 0 0 cm /Im0 Do Q")
        .stream(5, "/Type /XObject /Subtype /Image /Width 16 /Height 16 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode", &picture)
        .finish("<< /Root 1 0 R >>");
    let loaded = load(&pdf).expect("扫描件能读");
    let frame = &loaded.frames[0];
    assert!(frame.error.is_none(), "{:?}", frame.error);
    let center = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
    let [red, green, blue] = [frame.rgba[center], frame.rgba[center + 1], frame.rgba[center + 2]];
    assert!(blue > 180 && red < 80 && green < 160, "页中间是图片的蓝色：{red} {green} {blue}");
}

/// 文字写在表单 XObject 里的页（不少导出工具这样包一层）：表单里的文字也画出来。
#[test]
fn form_xobjects_contribute_their_text() {
    let pdf = Builder::new()
        .obj(1, "<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
        .obj(3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Fm0 5 0 R >> >> >>")
        .stream(4, "", b"q /Fm0 Do Q")
        .stream(5, "/Type /XObject /Subtype /Form /BBox [0 0 100 100]", b"BT (Inside) Tj ET")
        .finish("<< /Root 1 0 R >>");
    let loaded = load(&pdf).expect("PDF 能读");
    assert!(loaded.frames[0].text.contains("Inside"), "{}", loaded.frames[0].text);
}

/// 超过 32 页的文件页数照实写，前 32 页有画面，后面的页写明只预览前 32 页，照样能翻到。
#[test]
fn pages_after_the_render_limit_are_listed_but_not_drawn() {
    let contents: Vec<Vec<u8>> = (0..40).map(|index| format!("(Page {index}) Tj").into_bytes()).collect();
    let pages: Vec<(&str, &[u8])> = contents.iter().map(|content| ("", content.as_slice())).collect();
    let loaded = load(&sample(&pages)).expect("长文档能读");
    assert_eq!(loaded.frames.len(), 40, "页数照实");
    assert!(loaded.frames[31].error.is_none() && !loaded.frames[31].rgba.is_empty(), "第 32 页有画面");
    assert_eq!(loaded.frames[32].error.as_deref(), Some("只预览前 32 页"));
    assert!(loaded.frames[32].rgba.is_empty(), "后面的页不占位图内存");
}

/// 加密的 PDF 内容流解不开，整份写明原因；没有页树也没有页对象（只有几个流）的文件不当 PDF 页面。
#[test]
fn encrypted_or_pageless_files_fail_as_a_whole() {
    let mut encrypted = sample(&[("", b"(Secret) Tj")]);
    let tail = encrypted.len() - b"trailer\n<< /Root 1 0 R >>\n%%EOF\n".len();
    encrypted.truncate(tail);
    encrypted.extend_from_slice(b"trailer\n<< /Root 1 0 R /Encrypt 9 0 R >>\n%%EOF\n");
    assert_eq!(load(&encrypted).map(|loaded| loaded.text), Err("加密的 PDF 暂不支持预览".to_string()));

    let streams_only = b"%PDF-1.4\n1 0 obj\n<< /Length 14 >>\nstream\n(MomoBako) Tj\nendstream\nendobj\n%%EOF\n";
    assert_eq!(read(streams_only), Err("PDF 里没有找到页面".to_string()));
    assert_eq!(read(b"hello"), Err("不是 PDF 文件".to_string()));
}

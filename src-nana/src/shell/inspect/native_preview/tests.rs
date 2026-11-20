//! 原生预览的测试：zip、7z、PDF、Office、模型摘要、OLE 文本与压缩包内条目预览。

use std::io::Write;

use super::super::PreviewBody;
use super::super::bridge::NativeLoad;
use super::*;

fn sample_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut archive = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            archive.start_file(*name, options).expect("entry");
            if !name.ends_with('/') {
                archive.write_all(bytes).expect("bytes");
            }
        }
        archive.finish().expect("finish");
    }
    cursor.into_inner()
}

#[test]
fn zip_lists_files_and_docx_extracts_text() {
    let zip = sample_zip(&[("dir/", b""), ("a.txt", b"hi")]);
    let listed = read(ARCHIVE_VIEW, &zip).expect("zip");
    assert!(listed.contains("a.txt · 2 B"), "{listed}");
    assert!(listed.contains("dir · 目录"), "{listed}");
    assert!(!listed.contains("dir/"), "{listed}");
    let loaded = super::load(ARCHIVE_VIEW, &zip).expect("zip load");
    let mut state = super::super::InspectState::default();
    state.generation = 1;
    state.target_path = Some("a.zip".into());
    state.body = PreviewBody::Native {
        view_id: ARCHIVE_VIEW.into(),
        label: "压缩包".into(),
        content: String::new(),
    };
    note_loaded(&mut state, "a.zip".into(), 1, Ok(loaded));
    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::EnterArchive("dir".into()),
    );
    let inside = match &state.body {
        PreviewBody::Native { content, .. } => content.clone(),
        other => panic!("进入空目录后预览丢了：{other:?}"),
    };
    assert!(inside.contains("压缩包 / dir"), "{inside}");
    assert!(!inside.contains("a.txt"), "{inside}");

    let xml = "<w:p><w:t>你好 &amp; 桃</w:t></w:p>".as_bytes();
    let docx = sample_zip(&[("word/document.xml", xml)]);
    assert_eq!(read(OFFICE_VIEW, &docx).expect("docx"), "你好 & 桃");
    assert!(read(ARCHIVE_VIEW, b"not-a-zip").is_err());
}

#[test]
fn pdf_extracts_uncompressed_text_and_three_mf_counts_meshes() {
    let pdf = pdf::sample(&[("", b"(MomoBako) Tj")]);
    assert!(read(PDF_VIEW, &pdf).expect("pdf").contains("MomoBako"));
    let tj = pdf::sample(&[("", b"[(Mo) 20 (moBako)] TJ")]);
    assert!(read(PDF_VIEW, &tj).expect("tj").contains("MomoBako"));
    assert!(read(PDF_VIEW, b"not-a-pdf").is_err());
    // 只有流、没有页树的文件不算有页面。
    assert!(
        read(
            PDF_VIEW,
            b"%PDF-1.4\n<< /Filter /FlateDecode >>\nstream\nxxxx\nendstream\n"
        )
        .is_err()
    );

    let xml = br#"<?xml version="1.0"?>
<model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
<resources><object id="1" type="model"><mesh></mesh></object></resources>
<build><item objectid="1"/></build>
</model>"#;
    let three_mf = sample_zip(&[("3D/3dmodel.model", xml)]);
    let summary = read(MODEL_VIEW, &three_mf).expect("3mf");
    assert!(summary.contains("对象 1"), "{summary}");
    assert!(summary.contains("网格 1"), "{summary}");
    assert!(read(MODEL_VIEW, &sample_zip(&[("a.txt", b"hi")])).is_err());
}

#[test]
fn sevenz_lists_files_and_skips_directories() {
    // sevenz-rust2 写出的未加密 7z：目录 dir，文件 dir/a.txt=hi、b.txt=yo。
    const SEVEN_Z: &[u8] = &[
        0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c, 0x00, 0x04, 0x2c, 0x4a, 0xb6, 0xf6, 0x0c, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x99, 0x8e,
        0x1f, 0x0e, 0x01, 0x00, 0x01, 0x68, 0x69, 0x00, 0x01, 0x00, 0x01, 0x79, 0x6f, 0x00, 0x01,
        0x04, 0x06, 0x00, 0x02, 0x09, 0x06, 0x06, 0x0a, 0x01, 0x15, 0x5f, 0xd9, 0x30, 0xd4, 0x31,
        0x67, 0x7b, 0x00, 0x07, 0x0b, 0x02, 0x00, 0x01, 0x21, 0x21, 0x01, 0x16, 0x01, 0x21, 0x21,
        0x01, 0x16, 0x0c, 0x02, 0x02, 0x00, 0x08, 0x0a, 0x01, 0xac, 0x2a, 0x93, 0xd8, 0x89, 0xac,
        0x29, 0x62, 0x00, 0x00, 0x05, 0x03, 0x0e, 0x01, 0x80, 0x11, 0x29, 0x00, 0x64, 0x00, 0x69,
        0x00, 0x72, 0x00, 0x00, 0x00, 0x64, 0x00, 0x69, 0x00, 0x72, 0x00, 0x2f, 0x00, 0x61, 0x00,
        0x2e, 0x00, 0x74, 0x00, 0x78, 0x00, 0x74, 0x00, 0x00, 0x00, 0x62, 0x00, 0x2e, 0x00, 0x74,
        0x00, 0x78, 0x00, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let loaded = super::load(ARCHIVE_VIEW, SEVEN_Z).expect("7z");
    let listed = loaded.text.clone();
    assert!(listed.contains("压缩包 · 2 项"), "{listed}");
    assert!(listed.contains("dir · 目录"), "{listed}");
    assert!(listed.contains("b.txt · 2 B"), "{listed}");
    assert!(!listed.contains("dir/a.txt"), "{listed}");

    let mut state = super::super::InspectState::default();
    state.generation = 1;
    state.target_path = Some("a.7z".into());
    state.body = PreviewBody::Native {
        view_id: ARCHIVE_VIEW.into(),
        label: "压缩包".into(),
        content: String::new(),
    };
    note_loaded(&mut state, "a.7z".into(), 1, Ok(loaded));
    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::EnterArchive("dir".into()),
    );
    let inside = native_text(&state);
    assert!(inside.contains("压缩包 / dir · 1 项"), "{inside}");
    assert!(inside.contains("a.txt · 2 B"), "{inside}");
    assert!(!inside.contains("b.txt"), "{inside}");
    assert!(!inside.contains("dir/a.txt"), "{inside}");

    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::EnterArchive(String::new()),
    );
    let root = native_text(&state);
    assert!(root.contains("b.txt · 2 B"), "{root}");
    assert!(root.contains("dir · 目录"), "{root}");
    assert!(
        !root.lines().any(|line| line.starts_with("a.txt")),
        "{root}"
    );

    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::EnterArchive("b.txt".into()),
    );
    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::EnterArchive("..".into()),
    );
    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::EnterArchive("dir/nope".into()),
    );
    assert_eq!(native_text(&state), root);

    state.archive.as_mut().expect("catalog").layered = false;
    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::EnterArchive("dir".into()),
    );
    assert_eq!(native_text(&state), root, "rar 标记的列表不能进入下一层");
}

fn native_text(state: &super::super::InspectState) -> String {
    match &state.body {
        PreviewBody::Native { content, .. } => content.clone(),
        other => panic!("压缩包预览丢了：{other:?}"),
    }
}

/// 内置视图认得 PDF、3MF 和 zip 以外的压缩包；扩展名不分大小写、前导点去掉；认不出的不给视图。
#[test]
fn builtin_views_cover_pdf_three_mf_and_non_zip_archives() {
    let view = |extension: &str| builtin_view(extension).map(|(view, _)| view);
    assert_eq!(view("PDF"), Some(PDF_VIEW));
    assert_eq!(view(".3mf"), Some(MODEL_VIEW));
    for extension in ["zip", "cbz", "7z", "rar", "cbr"] {
        assert_eq!(view(extension), Some(ARCHIVE_VIEW), "{extension}");
    }
    assert_eq!(view("psd"), None);
}

#[test]
fn obj_gltf_and_stl_summaries_count_structure() {
    let obj = read(MODEL_VIEW, b"o Cube\nv 0 0 0\nv 1 0 0\nf 1 2 1\n").expect("obj");
    assert!(obj.contains("Cube"));
    assert!(obj.contains("顶点 2"));
    assert!(obj.contains("面 1"));

    let gltf = br#"{"meshes":[{}],"nodes":[{},{}],"materials":[],"animations":[]}"#;
    let summary = read(MODEL_VIEW, gltf).expect("gltf");
    assert!(summary.contains("网格 1"));
    assert!(summary.contains("节点 2"));

    let mut stl = vec![0_u8; 84 + 50];
    stl[80..84].copy_from_slice(&1_u32.to_le_bytes());
    assert_eq!(read(MODEL_VIEW, &stl).expect("stl"), "STL 二进制\n三角形 1");
    assert!(read(MODEL_VIEW, b"plain").is_err());
}

#[test]
fn ole_utf16_vrm_binding_fbx_and_blend_headers() {
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    for unit in "桃箱".encode_utf16() {
        ole.extend_from_slice(&unit.to_le_bytes());
    }
    assert_eq!(read(OFFICE_VIEW, &ole).expect("ole"), "桃箱");
    assert!(
        read(
            OFFICE_VIEW,
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]
        )
        .is_err()
    );

    let view = |extension: &str| builtin_view(extension).map(|(view, _)| view);
    for extension in [
        "doc", "xls", "ppt", "dot", "xlt", "pps", "dotm", "xlsb", "xltx", "xltm", "ppsx", "ppsm",
        "potx", "potm", "pot",
    ] {
        assert_eq!(view(extension), Some(OFFICE_VIEW), "{extension}");
    }
    assert!(read(OFFICE_VIEW, &sample_zip(&[("a.txt", b"hi")])).is_err());
    for extension in ["vrm", "fbx", "blend"] {
        assert_eq!(view(extension), Some(MODEL_VIEW), "{extension}");
    }

    assert!(read(MODEL_VIEW, b"glTF").is_err());

    let fbx = b"; FBX 7.4\nModel: \"Cube\"";
    let summary = read(MODEL_VIEW, fbx).expect("fbx");
    assert!(summary.contains("模型 1"), "{summary}");
    assert!(
        !summary.contains("网格") && !summary.contains("顶点"),
        "{summary}"
    );

    let binary = b"Kaydara FBX Binary";
    let summary = read(MODEL_VIEW, binary).expect("binary fbx");
    assert!(summary.contains("二进制 FBX"), "{summary}");
    assert!(
        summary.contains(&format!("{} 字节", binary.len())),
        "{summary}"
    );
    assert!(!summary.contains("网格"), "{summary}");

    let blend = b"BLENDER-v293";
    let summary = read(MODEL_VIEW, blend).expect("blend");
    assert!(summary.contains("293"), "{summary}");
    assert!(
        summary.contains(&format!("{} 字节", blend.len())),
        "{summary}"
    );
    assert!(!summary.contains("网格"), "{summary}");
    assert!(read(MODEL_VIEW, b"BLENDER").is_err());
}

#[test]
fn stale_native_result_keeps_the_current_body() {
    let mut state = super::super::InspectState::default();
    state.generation = 2;
    state.target_path = Some("a.zip".into());
    state.body = PreviewBody::Native {
        view_id: ARCHIVE_VIEW.into(),
        label: "压缩包".into(),
        content: String::new(),
    };
    note_loaded(
        &mut state,
        "a.zip".into(),
        2,
        Ok(NativeLoad::text_only("压缩包 · 1 个文件")),
    );
    assert!(
        matches!(state.body, PreviewBody::Native { ref content, .. } if content.contains("1 个文件"))
    );
    note_loaded(
        &mut state,
        "old.zip".into(),
        2,
        Ok(NativeLoad::text_only("过期")),
    );
    assert!(
        matches!(state.body, PreviewBody::Native { ref content, .. } if !content.contains("过期"))
    );
}

#[test]
fn pdf_pages_turn_and_unreadable_filters_fail() {
    let pdf = pdf::sample(&[("", b"(One) Tj"), ("", b"(Two) Tj")]);
    let loaded = super::load(PDF_VIEW, &pdf).expect("pdf");
    assert!(loaded.text.contains("1 / 2"), "{}", loaded.text);
    assert!(loaded.text.contains("One"), "{}", loaded.text);
    assert_eq!(loaded.frames.len(), 2);
    assert!(loaded.frames[0].rgba.iter().any(|byte| *byte != 255));

    let mut state = super::super::InspectState::default();
    state.generation = 1;
    state.target_path = Some("a.pdf".into());
    state.body = PreviewBody::Native {
        view_id: PDF_VIEW.into(),
        label: "PDF".into(),
        content: String::new(),
    };
    note_loaded(&mut state, "a.pdf".into(), 1, Ok(loaded));
    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::TurnPage(1),
    );
    assert!(
        matches!(state.body, PreviewBody::Native { ref content, .. } if content.contains("2 / 2") && content.contains("Two"))
    );

    let ascii85 = pdf::sample(&[("/Filter /ASCII85Decode", b"-qJ&$+B3(_~>")]);
    let text = read(PDF_VIEW, &ascii85).expect("ascii85");
    assert!(text.contains("Hi"), "{text}");

    // 解不开的滤镜是这一页失败、写明原因，整份照样打开，页数和翻页都在。
    let jpx = pdf::sample(&[("/Filter /JPXDecode", b"xxxx")]);
    let text = read(PDF_VIEW, &jpx).expect("jpx");
    assert!(
        text.contains("1 / 1") && text.contains("无法解码 JPXDecode"),
        "{text}"
    );
}

#[test]
fn obj_mesh_raster_can_orbit_and_fbx_stays_a_header() {
    let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let loaded = super::load(MODEL_VIEW, obj).expect("obj");
    assert!(loaded.mesh.is_some());
    assert!(loaded.text.contains("顶点 3"), "{}", loaded.text);
    let before = loaded.frames[0].rgba.clone();
    assert!(before.iter().any(|byte| *byte == 70), "没有画到三角形");

    let mut state = super::super::InspectState::default();
    state.generation = 1;
    state.target_path = Some("a.obj".into());
    state.body = PreviewBody::Native {
        view_id: MODEL_VIEW.into(),
        label: "模型".into(),
        content: loaded.text.clone(),
    };
    note_loaded(&mut state, "a.obj".into(), 1, Ok(loaded));
    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::Orbit {
            yaw: 1.2,
            zoom: 1.0,
        },
    );
    assert!(state.has_mesh());
    assert_ne!(state.raster_rgba(), Some(before.as_slice()));
    let fbx = read(MODEL_VIEW, b"; FBX 7.4\nModel: \"Cube\"").expect("fbx");
    assert!(fbx.contains("只报告文件头"), "{fbx}");
    assert!(!fbx.contains("网格") && !fbx.contains("顶点"), "{fbx}");
}

/// 点开已经列出的文本和图片，画的是这一条的原文和像素。取不出或不支持的仍停在名单上。
#[test]
fn archive_entry_shows_listed_text_and_pixels() {
    let png = tiny_png();
    let zip = sample_zip(&[
        ("a.txt", "hi".as_bytes()),
        ("b.txt", b"yo"),
        ("readme.md", "# 桃\n".as_bytes()),
        ("pic.png", &png),
        ("page.pdf", b"%PDF"),
        ("inner.zip", b"PK"),
    ]);
    let mut state = opened_archive(&zip, "pack.zip");
    let list = native_text(&state);

    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::OpenArchiveFile("a.txt".into()),
    );
    let preview = take_entry(&mut state, &zip);
    assert_eq!(entry_text(&preview), "hi");
    assert!(!entry_text(&preview).contains("yo"));
    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::ArchiveEntryLoaded {
            path: "pack.zip".into(),
            entry: "a.txt".into(),
            through: Vec::new(),
            generation: state.generation,
            result: Ok(preview),
        },
    );
    assert_eq!(
        entry_text(state.archive_entry.as_ref().expect("打开了文本")),
        "hi"
    );
    assert_eq!(native_text(&state), list, "名单还在，没有改成编出来的正文");

    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::EnterArchive(String::new()),
    );
    assert!(state.archive_entry.is_none(), "面包屑回到名单");
    assert_eq!(native_text(&state), list);

    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::OpenArchiveFile("nope.txt".into()),
    );
    assert!(
        state.take_effects().is_empty(),
        "名单里没有的名字不应去读字节"
    );
    assert!(state.archive_entry.is_none());

    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::OpenArchiveFile("readme.md".into()),
    );
    let preview = take_entry(&mut state, &zip);
    match &preview.kind {
        super::super::bridge::ArchiveEntryKind::Text { markdown, text, .. } => {
            assert!(*markdown);
            assert_eq!(text, "# 桃\n");
        }
        other => panic!("不是原文：{other:?}"),
    }

    state.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::OpenArchiveFile("pic.png".into()),
    );
    let preview = take_entry(&mut state, &zip);
    match &preview.kind {
        super::super::bridge::ArchiveEntryKind::Image {
            width,
            height,
            rgba,
        } => {
            assert_eq!((*width, *height), (1, 1));
            assert_eq!(rgba, &[255, 0, 0, 255]);
        }
        other => panic!("不是像素：{other:?}"),
    }

    let deflated = deflated_zip("note.txt", "原文".as_bytes());
    let preview = archive::preview_entry(&deflated, "note.txt").expect("deflate");
    assert_eq!(entry_text(&preview), "原文");

    // sevenz-rust2 写出的未加密 7z：目录 dir，文件 dir/a.txt=hi、b.txt=yo。
    const SEVEN_Z: &[u8] = &[
        0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c, 0x00, 0x04, 0x2c, 0x4a, 0xb6, 0xf6, 0x0c, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x99, 0x8e,
        0x1f, 0x0e, 0x01, 0x00, 0x01, 0x68, 0x69, 0x00, 0x01, 0x00, 0x01, 0x79, 0x6f, 0x00, 0x01,
        0x04, 0x06, 0x00, 0x02, 0x09, 0x06, 0x06, 0x0a, 0x01, 0x15, 0x5f, 0xd9, 0x30, 0xd4, 0x31,
        0x67, 0x7b, 0x00, 0x07, 0x0b, 0x02, 0x00, 0x01, 0x21, 0x21, 0x01, 0x16, 0x01, 0x21, 0x21,
        0x01, 0x16, 0x0c, 0x02, 0x02, 0x00, 0x08, 0x0a, 0x01, 0xac, 0x2a, 0x93, 0xd8, 0x89, 0xac,
        0x29, 0x62, 0x00, 0x00, 0x05, 0x03, 0x0e, 0x01, 0x80, 0x11, 0x29, 0x00, 0x64, 0x00, 0x69,
        0x00, 0x72, 0x00, 0x00, 0x00, 0x64, 0x00, 0x69, 0x00, 0x72, 0x00, 0x2f, 0x00, 0x61, 0x00,
        0x2e, 0x00, 0x74, 0x00, 0x78, 0x00, 0x74, 0x00, 0x00, 0x00, 0x62, 0x00, 0x2e, 0x00, 0x74,
        0x00, 0x78, 0x00, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let seven = opened_archive(SEVEN_Z, "a.7z");
    assert_eq!(
        entry_text(&archive::preview_entry(SEVEN_Z, "b.txt").expect("7z b")),
        "yo"
    );
    assert_eq!(
        entry_text(&archive::preview_entry(SEVEN_Z, "dir/a.txt").expect("7z a")),
        "hi"
    );
    let mut flat = seven;
    flat.archive.as_mut().expect("catalog").layered = false;
    flat.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::OpenArchiveFile("b.txt".into()),
    );
    assert!(flat.take_effects().is_empty(), "rar 式列表不打开包内文件");

    let mut model = crate::shell::ShellViewModel::for_page(crate::shell::ShellPage::SelectedFile);
    model.inspect = opened_archive(&zip, "pack.zip");
    model.inspect.reduce(
        true,
        Some("repo"),
        super::super::InspectMessage::OpenArchiveFile("a.txt".into()),
    );
    let preview = take_entry(&mut model.inspect, &zip);
    let generation = model.inspect.generation;
    model.reduce(crate::shell::ShellMessage::Inspect(
        super::super::InspectMessage::ArchiveEntryLoaded {
            path: "pack.zip".into(),
            entry: "a.txt".into(),
            through: Vec::new(),
            generation,
            result: Ok(preview),
        },
    ));
    let harness = crate::shell::view_harness::ShellHarness::mount(model);
    assert!(harness.find("hi").is_some(), "纯文本预览画出原文");
    assert!(harness.keyed("inspect-text").is_some());
    assert!(harness.keyed("inspect-archive-entry").is_some());
    assert!(
        harness.keyed("inspect-archive-list").is_some(),
        "打开文件后名单还在"
    );
}

fn opened_archive(bytes: &[u8], path: &str) -> super::super::InspectState {
    let loaded = super::load(ARCHIVE_VIEW, bytes).expect("archive");
    let mut state = super::super::InspectState::default();
    state.generation = 1;
    state.repo_id = Some("repo".into());
    state.target_path = Some(path.into());
    state.body = PreviewBody::Native {
        view_id: ARCHIVE_VIEW.into(),
        label: "压缩包".into(),
        content: String::new(),
    };
    note_loaded(&mut state, path.into(), 1, Ok(loaded));
    state
}

fn take_entry(
    state: &mut super::super::InspectState,
    bytes: &[u8],
) -> super::super::bridge::ArchiveEntryPreview {
    let effect = state.take_effects().pop().expect("要读这一条");
    let super::super::InspectEffect::LoadArchiveEntry { entry, .. } = effect else {
        panic!("不是包内文件：{effect:?}")
    };
    archive::preview_entry(bytes, &entry).unwrap_or_else(|error| panic!("{entry}：{error}"))
}

fn entry_text(preview: &super::super::bridge::ArchiveEntryPreview) -> &str {
    match &preview.kind {
        super::super::bridge::ArchiveEntryKind::Text { text, .. } => text,
        other => panic!("不是文本：{other:?}"),
    }
}

fn deflated_zip(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut archive = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        archive.start_file(name, options).expect("entry");
        archive.write_all(bytes).expect("bytes");
        archive.finish().expect("finish");
    }
    cursor.into_inner()
}

// 内联 `tests` 模块的外部文件相对 `shell/tests/` 解析。
mod entry_preview;

fn tiny_png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    let mut cursor = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .expect("png");
    cursor.into_inner()
}

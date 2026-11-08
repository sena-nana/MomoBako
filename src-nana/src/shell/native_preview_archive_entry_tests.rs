//! 包内 PDF 画自绘页，包内 zip/7z 再列当前层真实名字。
//! Office 只抽文本，模型用现有软件光栅。能在内存里解开的音频走唱片舞台。
//! 未压缩 AVI 用现有纯 Rust 解码画出真实帧。夹具都在内存里，不打开声卡，也不播放。

use std::time::{Duration, Instant};

use super::super::super::super::inspect_library::{
    archive_directory_activation, directory_double_click,
};
use super::super::super::bridge::ArchiveEntryKind;
use super::super::super::{InspectEffect, InspectMessage};
use super::super::{archive, pdf};
use super::{native_text, opened_archive, sample_zip};

/// sevenz-rust2 写出的未加密 7z：目录 dir，文件 dir/a.txt=hi、b.txt=yo。
const SEVEN_Z: &[u8] = &[
    0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c, 0x00, 0x04, 0x2c, 0x4a, 0xb6, 0xf6, 0x0c, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x99, 0x8e, 0x1f, 0x0e,
    0x01, 0x00, 0x01, 0x68, 0x69, 0x00, 0x01, 0x00, 0x01, 0x79, 0x6f, 0x00, 0x01, 0x04, 0x06, 0x00,
    0x02, 0x09, 0x06, 0x06, 0x0a, 0x01, 0x15, 0x5f, 0xd9, 0x30, 0xd4, 0x31, 0x67, 0x7b, 0x00, 0x07,
    0x0b, 0x02, 0x00, 0x01, 0x21, 0x21, 0x01, 0x16, 0x01, 0x21, 0x21, 0x01, 0x16, 0x0c, 0x02, 0x02,
    0x00, 0x08, 0x0a, 0x01, 0xac, 0x2a, 0x93, 0xd8, 0x89, 0xac, 0x29, 0x62, 0x00, 0x00, 0x05, 0x03,
    0x0e, 0x01, 0x80, 0x11, 0x29, 0x00, 0x64, 0x00, 0x69, 0x00, 0x72, 0x00, 0x00, 0x00, 0x64, 0x00,
    0x69, 0x00, 0x72, 0x00, 0x2f, 0x00, 0x61, 0x00, 0x2e, 0x00, 0x74, 0x00, 0x78, 0x00, 0x74, 0x00,
    0x00, 0x00, 0x62, 0x00, 0x2e, 0x00, 0x74, 0x00, 0x78, 0x00, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// 取出的 PDF 画这一份的页；解析失败用现有失败状态。取出的 zip/7z 只列名字，取不出的内层条目停在名单上。
#[test]
fn archive_entry_shows_pdf_pages_and_nested_names() {
    let pdf = pdf::sample(&[("", b"(One) Tj"), ("", b"(Two) Tj")]);
    let bad = b"%PDF-1.4\n".to_vec();
    let inner = sample_zip(&[
        ("note.txt", b"inside"),
        ("sub/c.txt", b"c"),
        ("clip.rar", b"Rar!"),
    ]);
    let zip = sample_zip(&[
        ("page.pdf", &pdf),
        ("bad.pdf", &bad),
        ("inner.zip", &inner),
        ("pack.7z", SEVEN_Z),
        ("broken.zip", b"PK"),
    ]);
    let mut state = opened_archive(&zip, "pack.zip");
    let outer_list = native_text(&state);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("page.pdf".into()),
    );
    let preview = take(&mut state, &zip);
    match &preview.kind {
        ArchiveEntryKind::Pdf {
            frames,
            index,
            error,
        } => {
            assert!(error.is_none());
            assert_eq!(*index, 0);
            assert_eq!(frames.len(), 2);
            assert!(frames[0].text.contains("One"), "{}", frames[0].text);
            assert!(frames[1].text.contains("Two"), "{}", frames[1].text);
            assert!(
                frames[0].rgba.iter().any(|byte| *byte != 255),
                "第一页没有画到字"
            );
        }
        other => panic!("不是这一份 PDF：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(preview));
    assert_eq!(native_text(&state), outer_list, "外层名单还在");
    let mut model = crate::shell::ShellViewModel::for_page(crate::shell::ShellPage::SelectedFile);
    model.inspect = state;
    let mut harness = crate::shell::view_harness::ShellHarness::mount(model);
    assert!(
        harness.keyed("inspect-archive-entry").is_some(),
        "PDF 留在包内这一条"
    );
    assert!(
        harness.keyed("inspect-native-view").is_some(),
        "用现有 PDF 预览"
    );
    assert!(
        harness.keyed("inspect-archive-list").is_some(),
        "打开后名单还在"
    );
    assert!(harness.find("2 页 PDF").is_some(), "页数来自这一份");
    let mut state = std::mem::replace(&mut harness.model.inspect, Default::default());
    drop(harness);
    state.reduce(true, Some("repo"), InspectMessage::TurnPage(1));
    match &state.archive_entry.as_ref().expect("还在这一份 PDF").kind {
        ArchiveEntryKind::Pdf {
            frames,
            index,
            error,
        } => {
            assert!(error.is_none());
            assert_eq!(*index, 1);
            assert!(frames[1].text.contains("Two"), "{}", frames[1].text);
        }
        other => panic!("翻页换成了别的：{other:?}"),
    }
    assert_eq!(native_text(&state), outer_list);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("bad.pdf".into()),
    );
    let failed = take(&mut state, &zip);
    match &failed.kind {
        ArchiveEntryKind::Pdf {
            frames,
            error: Some(error),
            ..
        } => {
            assert!(frames.is_empty(), "解析失败不留另一份的页");
            assert!(error.contains("没有找到页面"), "{error}");
        }
        other => panic!("失败状态丢了：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(failed));
    let mut model = crate::shell::ShellViewModel::for_page(crate::shell::ShellPage::SelectedFile);
    model.inspect = state;
    let mut harness = crate::shell::view_harness::ShellHarness::mount(model);
    assert!(harness.find("无法预览该文档").is_some());
    assert!(harness.find("PDF 里没有找到页面").is_some());
    assert!(harness.find("One").is_none(), "没有换成能画出的那一份");
    let mut state = std::mem::replace(&mut harness.model.inspect, Default::default());
    drop(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    for name in ["nope.txt"] {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(name.into()),
        );
        assert!(state.take_effects().is_empty(), "{name} 应停在名单上");
    }

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("broken.zip".into()),
    );
    let broken = state.take_effects().pop().expect("坏包也要试这一条");
    let InspectEffect::LoadArchiveEntry { entry, through, .. } = broken else {
        panic!("不是包内文件")
    };
    assert_eq!(entry, "broken.zip");
    assert!(through.is_empty());
    let error = archive::preview_chain(&zip, &through, &entry).expect_err("打不开的嵌套包不编名单");
    finish(&mut state, "pack.zip", Err(error));
    assert!(state.archive_entry.is_none());
    assert!(state.archive_nests.is_empty());
    assert_eq!(native_text(&state), outer_list);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("inner.zip".into()),
    );
    let nested = take(&mut state, &zip);
    match &nested.kind {
        ArchiveEntryKind::Nested { catalog } => {
            assert!(catalog.layered);
            assert!(
                catalog
                    .names
                    .iter()
                    .any(|name| name.path == "note.txt" && name.size == 6)
            );
            assert!(
                catalog.names.iter().all(|name| name.path != "inside"),
                "名单里没有条目正文"
            );
        }
        other => panic!("不是内层名单：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(nested));
    assert!(state.archive_entry.is_none(), "列完仍是名单");
    let listed = state.archive_nests.last().expect("内层").text.clone();
    assert!(listed.contains("压缩包 / inner.zip"), "{listed}");
    assert!(listed.contains("note.txt · 6 B"), "{listed}");
    assert!(listed.contains("sub · 目录"), "{listed}");
    assert!(!listed.contains("inside"), "{listed}");
    assert!(!listed.contains("page.pdf"), "{listed}");
    let mut model = crate::shell::ShellViewModel::for_page(crate::shell::ShellPage::SelectedFile);
    model.inspect = state;
    let mut harness = crate::shell::view_harness::ShellHarness::mount(model);
    assert!(harness.keyed("inspect-archive-list").is_some());
    assert!(
        harness.find("note.txt  6 B").is_some(),
        "现有名单画出内层名字"
    );
    assert!(harness.keyed("inspect-archive-entry").is_none());
    let mut state = std::mem::replace(&mut harness.model.inspect, Default::default());
    drop(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive("inner.zip/sub".into()),
    );
    let inside = state.archive_nests.last().expect("还在内层").text.clone();
    assert!(inside.contains("压缩包 / inner.zip/sub"), "{inside}");
    assert!(inside.contains("c.txt · 1 B"), "{inside}");
    assert!(!inside.contains("note.txt"), "{inside}");

    for name in ["inner.zip/clip.rar", "inner.zip/missing.txt"] {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(name.into()),
        );
        assert!(state.take_effects().is_empty(), "{name} 应停在内层名单上");
        assert!(state.archive_entry.is_none(), "{name}");
    }

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive("inner.zip".into()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("inner.zip/note.txt".into()),
    );
    let note = take(&mut state, &zip);
    assert_eq!(super::entry_text(&note), "inside");
    finish(&mut state, "pack.zip", Ok(note));
    assert_eq!(
        super::entry_text(state.archive_entry.as_ref().expect("内层文本")),
        "inside"
    );
    assert_eq!(state.archive_nests.len(), 1, "面包屑还能回到内层名单");

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    assert!(state.archive_nests.is_empty());
    assert!(state.archive_entry.is_none());
    assert_eq!(native_text(&state), outer_list);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("pack.7z".into()),
    );
    let seven = take(&mut state, &zip);
    finish(&mut state, "pack.zip", Ok(seven));
    let listed = state.archive_nests.last().expect("7z").text.clone();
    assert!(listed.contains("b.txt · 2 B"), "{listed}");
    assert!(listed.contains("dir · 目录"), "{listed}");
    assert!(!listed.contains("yo") && !listed.contains("hi"), "{listed}");
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive("pack.7z/dir".into()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("pack.7z/dir/a.txt".into()),
    );
    let file = take(&mut state, &zip);
    assert_eq!(super::entry_text(&file), "hi");
}

/// 取出的 Office 用现有文本抽取；能光栅的模型用现有画面，FBX/BLEND 只留文件头。视频仍停在名单上。
#[test]
fn archive_entry_shows_office_text_and_model_raster() {
    let docx = sample_zip(&[(
        "word/document.xml",
        "<w:p><w:t>你好 &amp; 桃</w:t></w:p>".as_bytes(),
    )]);
    let xlsx = sample_zip(&[("xl/sharedStrings.xml", "<si><t>表格</t></si>".as_bytes())]);
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    for unit in "桃箱".encode_utf16() {
        ole.extend_from_slice(&unit.to_le_bytes());
    }
    let bad_docx = sample_zip(&[("a.txt", b"hi")]);
    let model_xml = r#"<model><resources><object id="1"><mesh>
      <vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
      <triangles><triangle v1="0" v2="1" v3="2"/></triangles>
    </mesh></object></resources></model>"#;
    let three_mf = sample_zip(&[("3D/3dmodel.model", model_xml.as_bytes())]);
    let vrm = glb_triangle();
    let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let stl = b"solid tri\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n";
    let zip = sample_zip(&[
        ("note.docx", &docx),
        ("sheet.xlsx", &xlsx),
        ("old.doc", &ole),
        ("bad.docx", &bad_docx),
        ("tri.obj", obj),
        ("tri.stl", stl),
        ("part.3mf", &three_mf),
        ("avatar.vrm", &vrm),
        ("head.fbx", b"; FBX 7.4\nModel: \"Cube\""),
        ("scene.blend", b"BLENDER-v293"),
        ("bad.obj", b"plain"),
        ("clip.mp4", b"ftyp"),
    ]);
    let mut state = opened_archive(&zip, "pack.zip");
    let outer_list = native_text(&state);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("note.docx".into()),
    );
    let preview = take(&mut state, &zip);
    match &preview.kind {
        ArchiveEntryKind::Office { text, error: None } => assert_eq!(text, "你好 & 桃"),
        other => panic!("不是抽出的文档：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(preview));
    assert_eq!(native_text(&state), outer_list, "外层名单还在");
    let harness = mount(state);
    assert!(harness.keyed("inspect-archive-entry").is_some());
    assert!(
        harness.keyed("inspect-native-view").is_some(),
        "用现有文档预览"
    );
    assert!(harness.find("文档").is_some());
    assert!(harness.find("DOCX").is_some());
    assert!(harness.find("你好 & 桃").is_some());
    assert!(
        harness.keyed("inspect-archive-list").is_some(),
        "打开后名单还在"
    );
    state = unmount(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("sheet.xlsx".into()),
    );
    let preview = take(&mut state, &zip);
    match &preview.kind {
        ArchiveEntryKind::Office { text, error: None } => assert_eq!(text, "表格"),
        other => panic!("不是表格文本：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(preview));

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("old.doc".into()),
    );
    let preview = take(&mut state, &zip);
    match &preview.kind {
        ArchiveEntryKind::Office { text, error: None } => assert_eq!(text, "桃箱"),
        other => panic!("不是 OLE 文本：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(preview));

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("bad.docx".into()),
    );
    let failed = take(&mut state, &zip);
    match &failed.kind {
        ArchiveEntryKind::Office {
            text,
            error: Some(error),
        } => {
            assert!(text.is_empty());
            assert!(error.contains("没有可提取的文本"), "{error}");
        }
        other => panic!("失败状态丢了：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(failed));
    let harness = mount(state);
    assert!(shows(&harness, "无法预览该文档"));
    assert!(shows(&harness, "没有可提取的文本"));
    state = unmount(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("tri.obj".into()),
    );
    let preview = take(&mut state, &zip);
    let before = model_rgba(&preview).to_vec();
    assert!(before.iter().any(|byte| *byte == 70), "没有画到三角形");
    assert!(model_has_mesh(&preview));
    finish(&mut state, "pack.zip", Ok(preview));
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::Orbit {
            yaw: 1.2,
            zoom: 1.0,
        },
    );
    let turned = model_rgba(state.archive_entry.as_ref().expect("还是这一份模型"));
    assert_ne!(turned, before.as_slice(), "旋转没有重画");
    let harness = mount(state);
    assert!(harness.keyed("inspect-model").is_some(), "用现有模型预览");
    assert!(harness.keyed("inspect-orbit").is_some());
    assert!(harness.keyed("inspect-native-page").is_some());
    assert!(harness.find("OBJ").is_some());
    state = unmount(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    for name in ["tri.stl", "part.3mf", "avatar.vrm"] {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(name.into()),
        );
        let preview = take(&mut state, &zip);
        assert!(
            model_rgba(&preview).iter().any(|byte| *byte == 70),
            "{name} 没有画到三角形"
        );
        assert!(model_has_mesh(&preview), "{name}");
        finish(&mut state, "pack.zip", Ok(preview));
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::EnterArchive(String::new()),
        );
    }

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("head.fbx".into()),
    );
    let preview = take(&mut state, &zip);
    match &preview.kind {
        ArchiveEntryKind::Model {
            text,
            mesh: None,
            frame: None,
            error: None,
            ..
        } => {
            assert!(text.contains("只报告文件头"), "{text}");
            assert!(text.contains("模型 1"), "{text}");
        }
        other => panic!("FBX 不该光栅：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(preview));
    let harness = mount(state);
    assert!(shows(&harness, "只报告文件头"));
    assert!(harness.keyed("inspect-model").is_some());
    assert!(
        harness.keyed("inspect-orbit").is_none(),
        "没有网格就不给旋转"
    );
    state = unmount(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("scene.blend".into()),
    );
    let preview = take(&mut state, &zip);
    match &preview.kind {
        ArchiveEntryKind::Model {
            text,
            mesh: None,
            frame: None,
            error: None,
            ..
        } => {
            assert!(text.contains("只报告文件头"), "{text}");
            assert!(text.contains("293"), "{text}");
        }
        other => panic!("BLEND 不该光栅：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(preview));

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("bad.obj".into()),
    );
    let failed = take(&mut state, &zip);
    match &failed.kind {
        ArchiveEntryKind::Model {
            error: Some(error),
            mesh: None,
            frame: None,
            ..
        } => {
            assert!(error.contains("没有顶点或面"), "{error}");
        }
        other => panic!("模型失败状态丢了：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(failed));
    let harness = mount(state);
    assert!(shows(&harness, "无法预览该模型"));
    assert!(shows(&harness, "没有顶点或面"));
    state = unmount(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    assert!(state.archive_entry.is_none());
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("clip.mp4".into()),
    );
    assert!(state.take_effects().is_empty(), "视频应停在名单上");
    assert!(state.archive_entry.is_none());
    assert_eq!(native_text(&state), outer_list);
}

/// 取出的 WAV、mp3、flac、ogg 用现有解码在内存里解开，预览是唱片舞台。不装进播放条，也不打开声卡。
/// 解不开的 wav 用现有失败浮层。视频和没有这条解码的格式仍停在名单上。
#[test]
fn archive_entry_previews_decoded_audio_without_opening_the_sound_device() {
    assert!(
        !crate::shell::player::sound_device_compiled_in(),
        "测试构建不应编进声卡"
    );
    let wav = voice_wav();
    let mp3 = include_bytes!("audio_fixtures/tone.mp3");
    let flac = include_bytes!("audio_fixtures/tone.flac");
    let ogg = include_bytes!("audio_fixtures/tone.ogg");
    let zip = sample_zip(&[
        ("song/voice.wav", &wav),
        ("tone.mp3", mp3),
        ("tone.flac", flac),
        ("tone.ogg", ogg),
        ("broken.wav", b"RIFF"),
        ("clip.mp4", b"ftyp"),
        ("voice.wma", b"wma"),
    ]);
    let mut state = opened_archive(&zip, "pack.zip");
    let outer_list = native_text(&state);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("song/voice.wav".into()),
    );
    let preview = take(&mut state, &zip);
    assert_audio(&preview, &wav);
    finish(&mut state, "pack.zip", Ok(preview));
    assert_eq!(native_text(&state), outer_list, "外层名单还在");
    let harness = mount(state);
    assert!(harness.keyed("inspect-archive-entry").is_some());
    assert!(
        harness.keyed("inspect-audio-stage").is_some(),
        "用现有唱片舞台"
    );
    assert!(harness.keyed("inspect-player-mount").is_some());
    assert!(harness.find("voice").is_some(), "标题是文件名");
    assert!(harness.find("通用音频").is_some());
    assert!(harness.find("暂无歌词").is_some());
    assert!(
        harness.keyed("inspect-archive-list").is_some(),
        "打开后名单还在"
    );
    assert_eq!(
        harness.model.player.session.status, "idle",
        "没有装进播放条"
    );
    assert!(!harness.model.player.wants_playing, "没有开始播放");
    state = unmount(harness);

    for (name, bytes) in [
        ("tone.mp3", mp3.as_slice()),
        ("tone.flac", flac.as_slice()),
        ("tone.ogg", ogg.as_slice()),
    ] {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::EnterArchive(String::new()),
        );
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(name.into()),
        );
        let preview = take(&mut state, &zip);
        assert_audio(&preview, bytes);
        finish(&mut state, "pack.zip", Ok(preview));
        assert!(state.take_effects().is_empty(), "{name} 不应再去读或播放");
        assert!(
            !matches!(state.body, super::super::super::PreviewBody::Media(_)),
            "{name} 不应换成播放会话"
        );
    }

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("broken.wav".into()),
    );
    let failed = take(&mut state, &zip);
    match &failed.kind {
        ArchiveEntryKind::Audio {
            error: Some(error),
            duration_ms,
            ..
        } => {
            assert_eq!(*duration_ms, 0);
            assert!(error.contains("不是 WAV 头"), "{error}");
        }
        other => panic!("失败状态丢了：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(failed));
    let harness = mount(state);
    assert!(shows(&harness, "无法预览该音频"));
    assert!(shows(&harness, "不是 WAV 头"));
    assert!(
        harness.keyed("inspect-audio-stage").is_none(),
        "解不开就不画唱片"
    );
    assert_eq!(harness.model.player.session.status, "idle");
    assert!(!harness.model.player.wants_playing);
    state = unmount(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    for name in ["clip.mp4", "voice.wma"] {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(name.into()),
        );
        assert!(state.take_effects().is_empty(), "{name} 应停在名单上");
        assert!(state.archive_entry.is_none(), "{name}");
    }
    assert!(
        !crate::shell::player::sound_device_compiled_in(),
        "解完仍然没有声卡模块"
    );
    assert_eq!(native_text(&state), outer_list);
}

/// 取出的未压缩 AVI（夹具 `clip.avi`）用现有纯 Rust 解码画出真实帧。PCM 不进播放条，也不打开声卡。
/// 其它视频扩展名仍停在名单上。字节不是未压缩 AVI 的 `.avi` 取出后仍回到名单。
#[test]
fn archive_entry_draws_uncompressed_avi_frames_without_opening_the_sound_device() {
    assert!(
        !crate::shell::player::sound_device_compiled_in(),
        "测试构建不应编进声卡"
    );
    let clip = include_bytes!("preview_fixtures/clip.avi");
    let zip = sample_zip(&[
        ("movie/clip.avi", clip),
        ("clip.mp4", b"ftyp"),
        ("clip.mkv", b"mkv"),
        ("clip.webm", b"webm"),
        ("clip.mov", b"mov"),
        ("clip.m4v", b"m4v"),
        ("fake.avi", b"RIFF"),
    ]);
    let mut state = opened_archive(&zip, "pack.zip");
    let outer_list = native_text(&state);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("movie/clip.avi".into()),
    );
    let preview = take(&mut state, &zip);
    let (first_png, later, second_png) = match &preview.kind {
        ArchiveEntryKind::Video {
            frames,
            position_ms,
        } => {
            assert_eq!(*position_ms, 0, "打开时播放头在片头");
            assert!(frames.len() >= 2, "{}", frames.len());
            let first = &frames[0];
            assert_eq!(
                (first.width, first.height, first.rgba.len()),
                (32, 18, 32 * 18 * 4)
            );
            assert!(
                first
                    .rgba
                    .chunks_exact(4)
                    .all(|pixel| pixel == [0x20, 0x22, 0x26, 0xFF]),
                "第一帧应是文件里的深灰"
            );
            let second = &frames[1];
            assert!(
                second.time_ms > first.time_ms,
                "第二帧应晚于片头，后半才落得上去"
            );
            assert!(
                second
                    .rgba
                    .chunks_exact(4)
                    .all(|pixel| pixel == [0x2A, 0x2D, 0x33, 0xFF]),
                "第二帧应是文件里的另一色"
            );
            (frame_png(first), second.time_ms, frame_png(second))
        }
        other => panic!("不是解开的画面：{other:?}"),
    };
    finish(&mut state, "pack.zip", Ok(preview));
    assert_eq!(native_text(&state), outer_list, "外层名单还在");
    assert!(state.take_effects().is_empty(), "解完不应再去读或播放");
    assert!(
        !matches!(state.body, super::super::super::PreviewBody::Media(_)),
        "不应换成播放会话"
    );
    let mut harness = mount(state);
    assert!(harness.keyed("inspect-archive-entry").is_some());
    assert!(
        harness.keyed("inspect-archive-video").is_some(),
        "片头画已经解开的第一帧"
    );
    assert_eq!(
        painted(&harness, "inspect-archive-video"),
        first_png,
        "片头应是解码出的第一帧，不是手写色块"
    );
    assert!(
        harness.keyed("inspect-video").is_none(),
        "没有宿主纹理时不挂 file-preview"
    );
    assert!(
        !crate::shell::wants_preview_texture(&harness.model),
        "离屏不上传宿主纹理"
    );
    assert!(harness.model.preview_token.is_none(), "离屏不挂纹理记号");
    assert!(
        harness.keyed("inspect-archive-list").is_some(),
        "打开后名单还在"
    );
    assert!(
        harness.keyed("inspect-audio-stage").is_none(),
        "视频不画唱片"
    );
    assert!(harness.keyed("inspect-fallback-icon").is_none());
    assert_eq!(
        harness.model.player.session.status, "idle",
        "没有装进播放条"
    );
    assert!(!harness.model.player.wants_playing, "没有开始播放");
    assert!(
        !matches!(
            harness.model.inspect.body,
            super::super::super::PreviewBody::Media(_)
        ),
        "拨播放头也不换成播放会话"
    );

    // 夹具两帧平分时长，第二帧的时刻就是后半。用预览自己的位置，不拨播放条。
    super::super::archive::place_video(&mut harness.model, later);
    harness.sync();
    harness.flush();
    assert_eq!(
        painted(&harness, "inspect-archive-video"),
        second_png,
        "后半应是解码出的第二帧"
    );
    assert!(
        harness.keyed("inspect-video").is_none(),
        "播放头往前走也不挂 file-preview"
    );
    assert!(
        harness.model.preview_token.is_none(),
        "没有宿主纹理时不把记号挂回去"
    );
    assert_eq!(
        harness.model.player.session.status, "idle",
        "播放条仍是 idle"
    );
    assert!(!crate::shell::wants_preview_texture(&harness.model));

    harness.model.preview_token = Some("video:movie/clip.avi:0:32".into());
    harness.model.preview_pixels = Some(crate::shell::PreviewPixels {
        width: 2,
        height: 2,
        rgba: vec![255; 16],
    });
    harness.sync();
    harness.flush();
    assert!(
        harness.keyed("inspect-video").is_some(),
        "有宿主纹理时和文件预览一样画纹理槽"
    );
    assert!(
        harness.keyed("inspect-archive-video").is_none(),
        "有宿主纹理时不改画解码像素"
    );
    assert!(
        crate::shell::wants_preview_texture(&harness.model),
        "有宿主纹理时仍要上传"
    );
    super::super::archive::place_video(&mut harness.model, later);
    assert!(
        harness
            .model
            .preview_token
            .as_deref()
            .is_some_and(|token| token.starts_with("video:")),
        "已有记号时跟着换到这一帧"
    );
    harness.model.preview_token = None;
    super::super::archive::place_video(&mut harness.model, later);
    harness.sync();
    harness.flush();
    assert!(
        harness.model.preview_token.is_none(),
        "清掉记号后再拨，也不把未注册记号挂回去"
    );
    assert_eq!(
        painted(&harness, "inspect-archive-video"),
        second_png,
        "清掉记号后仍画解码出的第二帧"
    );
    assert!(harness.keyed("inspect-video").is_none());
    state = unmount(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    assert!(state.archive_entry.is_none(), "面包屑回到名单");
    for name in ["clip.mp4", "clip.mkv", "clip.webm", "clip.mov", "clip.m4v"] {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(name.into()),
        );
        assert!(state.take_effects().is_empty(), "{name} 应停在名单上");
        assert!(state.archive_entry.is_none(), "{name}");
    }

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("fake.avi".into()),
    );
    let effect = state.take_effects().pop().expect("扩展名放行，要读字节");
    let InspectEffect::LoadArchiveEntry { entry, through, .. } = effect else {
        panic!("不是包内文件：{effect:?}")
    };
    let error = archive::preview_chain(&zip, &through, &entry).expect_err("不是未压缩 AVI");
    finish(&mut state, "pack.zip", Err(error));
    assert!(state.archive_entry.is_none(), "解不开仍停在名单上");
    assert_eq!(native_text(&state), outer_list);
    assert!(
        !crate::shell::player::sound_device_compiled_in(),
        "解完仍然没有声卡模块"
    );
}
fn assert_audio(preview: &super::super::super::bridge::ArchiveEntryPreview, bytes: &[u8]) {
    let pcm = crate::shell::player::pcm_from_bytes(bytes).expect("对照解码");
    assert!(
        !pcm.pcm.is_empty() && pcm.frame_count > 0,
        "对照解码没有样本"
    );
    match &preview.kind {
        ArchiveEntryKind::Audio {
            duration_ms,
            sample_rate,
            channels,
            error: None,
        } => {
            assert_eq!(
                (*sample_rate, *channels, *duration_ms),
                (pcm.sample_rate, pcm.channels, pcm.duration_ms)
            );
        }
        other => panic!("不是解开的音频：{other:?}"),
    }
}

/// 8 kHz 单声道 16 位，8 个样本，时长 1 毫秒。测试用，不播放。
fn voice_wav() -> Vec<u8> {
    use crate::shell::audio_decode::pcm::{fmt_chunk, wav_file};
    let samples = [0i16, 1000, -1000, 500, 0, 1000, -1000, 500];
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    wav_file(&fmt_chunk(1, 1, 8_000, 16, None), &data)
}

/// 这一帧编成的内容图。和预览框里 `content_image` 用的是同一个编码器。
fn frame_png(frame: &crate::shell::inspect::support::VideoFrame) -> String {
    super::super::super::rgba_png_data_url(frame.width, frame.height, &frame.rgba)
        .expect("帧能编成 PNG")
}

/// 读出预览框里已经画上的内容图。对不上解码帧就说明没有按像素画。
fn painted(harness: &crate::shell::view_harness::ShellHarness, key: &str) -> String {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    let context = harness.document().context();
    let style = context
        .world()
        .node_style(id)
        .unwrap_or_else(|| panic!("{key} 没有样式"));
    match &style.layout.paint.content_image {
        Some(nana_ui_core::BackgroundImage::Url { url, .. }) => url.clone(),
        other => panic!("{key} 不是解码出的画面：{other:?}"),
    }
}

fn shows(harness: &crate::shell::view_harness::ShellHarness, needle: &str) -> bool {
    harness.nodes().into_iter().any(|node| {
        node.label
            .as_deref()
            .is_some_and(|label| label.contains(needle))
    })
}

fn mount(state: super::super::super::InspectState) -> crate::shell::view_harness::ShellHarness {
    let mut model = crate::shell::ShellViewModel::for_page(crate::shell::ShellPage::SelectedFile);
    model.inspect = state;
    crate::shell::view_harness::ShellHarness::mount(model)
}

fn unmount(
    mut harness: crate::shell::view_harness::ShellHarness,
) -> super::super::super::InspectState {
    let state = std::mem::replace(&mut harness.model.inspect, Default::default());
    drop(harness);
    state
}

fn model_rgba(preview: &super::super::super::bridge::ArchiveEntryPreview) -> &[u8] {
    match &preview.kind {
        ArchiveEntryKind::Model {
            frame: Some(frame),
            error: None,
            ..
        } => &frame.rgba,
        other => panic!("不是光栅：{other:?}"),
    }
}

fn model_has_mesh(preview: &super::super::super::bridge::ArchiveEntryPreview) -> bool {
    matches!(&preview.kind, ArchiveEntryKind::Model { mesh: Some(_), .. })
}

/// 一个三角形的 glb。VRM 就是这种文件头，走现有 glTF 光栅。
fn glb_triangle() -> Vec<u8> {
    let mut bin = Vec::new();
    for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let mut json = br#"{"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}],"bufferViews":[{"byteLength":36}],"buffers":[{"byteLength":36}]}"#.to_vec();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    bytes.extend(json);
    bytes.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0x004E_4942u32.to_le_bytes());
    bytes.extend(bin);
    bytes
}

/// 目录单击只选中，间隔严格小于 400 毫秒的第二次 Activate 才进入。文件单击仍读取这一条。
/// 进入只过滤已经列出的名字，不重新打开压缩包。
#[test]
fn archive_directory_click_selects_until_second_activate() {
    let now = Instant::now();
    let mut slot = None;
    assert!(
        !directory_double_click(&mut slot, "bound-dir", now),
        "第一次不是连点"
    );
    assert!(
        directory_double_click(&mut slot, "bound-dir", now + Duration::from_millis(399)),
        "不到 400 毫秒的第二次进入"
    );
    assert!(
        !directory_double_click(
            &mut slot,
            "bound-dir",
            now + Duration::from_millis(399) + Duration::from_millis(400)
        ),
        "间隔满 400 毫秒只选中"
    );
    assert!(
        !directory_double_click(&mut slot, "other-dir", now + Duration::from_millis(799)),
        "换一条目录不是连点"
    );
    assert!(
        matches!(archive_directory_activation("gesture-photos"), InspectMessage::SelectArchive(path) if path == "gesture-photos")
    );
    assert!(
        matches!(archive_directory_activation("gesture-photos"), InspectMessage::EnterArchive(path) if path == "gesture-photos")
    );

    let inner = sample_zip(&[("note.txt", b"inside"), ("sub/c.txt", b"c")]);
    let zip = sample_zip(&[
        ("photos/", b""),
        ("photos/sub/", b""),
        ("photos/b.txt", b"yo"),
        ("other/", b""),
        ("a.txt", b"hi"),
        ("inner.zip", &inner),
    ]);
    let mut state = opened_archive(&zip, "pack.zip");
    let root = native_text(&state);
    let names = catalog_names(&state);
    assert!(
        root.contains("photos · 目录") && root.contains("a.txt · 2 B"),
        "{root}"
    );
    assert!(
        !state.archive.as_ref().expect("catalog").truncated,
        "这份夹具没到 200 条上限"
    );

    let pick = |state: &mut super::super::super::InspectState, path: &str| {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::SelectArchive(path.into()),
        );
    };
    pick(&mut state, "photos");
    assert_eq!(state.archive_selected.as_deref(), Some("photos"));
    assert!(state.archive_directory.is_empty(), "单击目录不进入");
    assert_eq!(native_text(&state), root);
    assert_eq!(catalog_names(&state), names);
    assert!(state.take_effects().is_empty(), "选中不重新打开压缩包");

    pick(&mut state, "other");
    assert_eq!(
        state.archive_selected.as_deref(),
        Some("other"),
        "再点另一个目录只换选中"
    );
    assert_eq!(native_text(&state), root);
    pick(&mut state, "photos");
    pick(&mut state, "");
    assert_eq!(
        state.archive_selected.as_deref(),
        Some("photos"),
        "空路径不能选中"
    );
    for path in ["..", "missing", "a.txt", "photos/nope"] {
        pick(&mut state, path);
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::EnterArchive(path.into()),
        );
        assert_eq!(
            state.archive_selected.as_deref(),
            Some("photos"),
            "{path} 不能换掉选中"
        );
        assert!(state.archive_directory.is_empty(), "{path}");
        assert_eq!(native_text(&state), root, "{path}");
    }
    assert_eq!(catalog_names(&state), names);
    assert!(state.take_effects().is_empty());

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive("photos".into()),
    );
    assert!(state.archive_selected.is_none(), "进入后清掉选中");
    assert_eq!(state.archive_directory, "photos");
    let inside = native_text(&state);
    assert!(inside.contains("压缩包 / photos"), "{inside}");
    assert!(
        inside.contains("sub · 目录") && inside.contains("b.txt · 2 B"),
        "{inside}"
    );
    assert!(
        !inside.contains("a.txt") && !inside.contains("other"),
        "{inside}"
    );
    assert_eq!(catalog_names(&state), names, "进入只过滤已列出的名字");
    assert!(state.take_effects().is_empty());

    pick(&mut state, "photos/sub");
    assert_eq!(state.archive_selected.as_deref(), Some("photos/sub"));
    assert_eq!(state.archive_directory, "photos", "单击子目录仍停在这一层");
    assert_eq!(native_text(&state), inside);
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive("photos/sub".into()),
    );
    assert!(state.archive_selected.is_none());
    assert_eq!(state.archive_directory, "photos/sub");
    let empty = native_text(&state);
    assert!(empty.contains("压缩包 / photos/sub · 0 项"), "{empty}");
    assert!(
        !empty.contains("b.txt") && !empty.contains("c.txt"),
        "{empty}"
    );

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    assert!(state.archive_directory.is_empty());
    assert!(state.archive_selected.is_none());
    assert_eq!(native_text(&state), root, "面包屑回到根，名单还是原来那份");
    assert_eq!(catalog_names(&state), names);

    pick(&mut state, "photos");
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("a.txt".into()),
    );
    let effect = state.take_effects().pop().expect("文件单击仍读取这一条");
    let InspectEffect::LoadArchiveEntry { entry, through, .. } = effect else {
        panic!("不是包内文件：{effect:?}")
    };
    assert_eq!(entry, "a.txt");
    assert!(through.is_empty());
    assert_eq!(
        state.archive_selected.as_deref(),
        Some("a.txt"),
        "点文件后这一行是当前项，目录不再亮着"
    );
    assert!(state.archive_directory.is_empty());
    assert_eq!(native_text(&state), root);
    assert_eq!(catalog_names(&state), names);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("inner.zip".into()),
    );
    let nested = take(&mut state, &zip);
    finish(&mut state, "pack.zip", Ok(nested));
    let listed = state.archive_nests.last().expect("内层").text.clone();
    assert!(
        listed.contains("sub · 目录") && listed.contains("note.txt"),
        "{listed}"
    );
    pick(&mut state, "inner.zip/sub");
    assert_eq!(state.archive_selected.as_deref(), Some("inner.zip/sub"));
    assert!(
        state
            .archive_nests
            .last()
            .expect("内层")
            .directory
            .is_empty(),
        "单击内层目录不进入"
    );
    assert_eq!(state.archive_nests.last().expect("内层").text, listed);
    pick(&mut state, "inner.zip/missing");
    assert_eq!(
        state.archive_selected.as_deref(),
        Some("inner.zip/sub"),
        "内层没有的名字不选中"
    );
    assert_eq!(catalog_names(&state), names);
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive("inner.zip/sub".into()),
    );
    assert!(state.archive_selected.is_none());
    let nested_inside = state.archive_nests.last().expect("还在内层").text.clone();
    assert!(nested_inside.contains("c.txt · 1 B"), "{nested_inside}");
    assert!(!nested_inside.contains("note.txt"), "{nested_inside}");
    assert_eq!(catalog_names(&state), names, "内层也只过滤已经列出的名字");
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    assert!(state.archive_nests.is_empty());
    assert_eq!(native_text(&state), root);

    let mut flat = opened_archive(&zip, "flat.zip");
    flat.archive.as_mut().expect("catalog").layered = false;
    let flat_text = native_text(&flat);
    flat.reduce(
        true,
        Some("repo"),
        InspectMessage::SelectArchive("photos".into()),
    );
    flat.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive("photos".into()),
    );
    assert!(flat.archive_selected.is_none());
    assert!(flat.archive_directory.is_empty());
    assert_eq!(native_text(&flat), flat_text, "扁平列表不选中也不进入目录");
    assert!(flat.take_effects().is_empty());
}

/// 已经读出的条目名。选中和进入都不该改这份名单。
fn catalog_names(state: &super::super::super::InspectState) -> Vec<String> {
    state
        .archive
        .as_ref()
        .expect("catalog")
        .names
        .iter()
        .map(|name| format!("{}:{}", name.path, name.directory))
        .collect()
}

fn take(
    state: &mut super::super::super::InspectState,
    bytes: &[u8],
) -> super::super::super::bridge::ArchiveEntryPreview {
    let effect = state.take_effects().pop().expect("要读这一条");
    let InspectEffect::LoadArchiveEntry { entry, through, .. } = effect else {
        panic!("不是包内文件：{effect:?}")
    };
    archive::preview_chain(bytes, &through, &entry)
        .unwrap_or_else(|error| panic!("{entry}：{error}"))
}

/// 名单还在时，旁边的提示跟 Vue `renderInternalPreview`：空闲写当前目录和「选择内部文件进行预览」，
/// 不能内联的类型不取字节、写文件名和「无法内联预览该文件类型」，正在取写「准备内部文件」，
/// 取出失败写「无法预览内部文件」和原因并留在名单上。点目录后舞台改写目录名和「无法内联预览该文件类型」。
/// 打开成功的那条见并排测试。
#[test]
fn archive_stage_explains_the_idle_list_and_a_failed_internal_file() {
    let zip = sample_zip(&[("photos/", b""), ("a.txt", b"hi"), ("clip.bin", b"nope")]);
    let mut state = opened_archive(&zip, "pack.zip");
    assert_eq!(stage_of(&state), ("根目录", "选择内部文件进行预览", false));
    let idle = mount_archive(&state);
    assert!(
        idle.keyed("inspect-archive-list").is_some(),
        "空闲时名单还在"
    );
    assert!(idle.keyed("inspect-archive-stage").is_some());
    assert!(
        idle.find("选择内部文件进行预览").is_some(),
        "空闲提示要画出来"
    );
    assert!(idle.find("根目录").is_some(), "根目录的提示用「根目录」");

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("clip.bin".into()),
    );
    assert!(state.take_effects().is_empty(), "不能内联的类型不取字节");
    assert_eq!(
        stage_of(&state),
        ("clip.bin", "无法内联预览该文件类型", false)
    );
    let inline = mount_archive(&state);
    assert!(inline.find("无法内联预览该文件类型").is_some());
    assert!(
        inline.keyed("inspect-archive-list").is_some(),
        "不能内联时仍留在名单"
    );

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("a.txt".into()),
    );
    assert_eq!(stage_of(&state), ("准备内部文件", "a.txt", false));
    let preparing = mount_archive(&state);
    assert!(preparing.find("准备内部文件").is_some());
    assert!(
        preparing.keyed("inspect-archive-list").is_some(),
        "还在取字节时名单还在"
    );

    finish(&mut state, "pack.zip", Err("读不出这一条".into()));
    assert!(state.archive_entry.is_none(), "失败不编一条预览");
    assert_eq!(stage_of(&state), ("无法预览内部文件", "读不出这一条", true));
    let failed = mount_archive(&state);
    assert!(failed.find("无法预览内部文件").is_some());
    assert!(failed.find("读不出这一条").is_some());
    assert!(
        failed.keyed("inspect-archive-list").is_some(),
        "失败后仍留在名单"
    );

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SelectArchive("photos".into()),
    );
    assert_eq!(
        stage_of(&state),
        ("photos", "无法内联预览该文件类型", false),
        "点目录后舞台写目录名"
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive("photos".into()),
    );
    assert_eq!(
        stage_of(&state),
        ("photos", "选择内部文件进行预览", false),
        "进入后提示改成这一层的目录"
    );
}

/// 打开内部文件后名单留在左边，舞台在右边画这一条。再点另一条，舞台换成那一条，名单仍在。
/// 点目录的单击不在这里改：仍只选中，第二次 Activate 才进入。
#[test]
fn archive_open_file_keeps_the_list_beside_this_entry() {
    let zip = sample_zip(&[("photos/", b""), ("a.txt", b"alpha"), ("b.txt", b"beta")]);
    let mut state = opened_archive(&zip, "pack.zip");
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("a.txt".into()),
    );
    let preview = take(&mut state, &zip);
    finish(&mut state, "pack.zip", Ok(preview));
    let harness = mount(state);
    assert!(
        harness.keyed("inspect-archive-list").is_some(),
        "打开后名单还在"
    );
    assert!(
        shows(&harness, "b.txt") && shows(&harness, "photos"),
        "其它条目还在名单上"
    );
    assert_eq!(
        crumb_text(&harness, "inspect-archive-crumb-root"),
        "pack.zip",
        "第一段仍是带扩展名的文件名"
    );
    assert!(harness.find("alpha").is_some(), "舞台是这一条");
    assert!(harness.keyed("inspect-text").is_some());
    assert!(
        harness.find("选择内部文件进行预览").is_none(),
        "打开后舞台不再是空闲提示"
    );
    assert_beside(
        &harness,
        "inspect-archive-list-pane",
        "inspect-archive-stage-pane",
    );
    let mut state = unmount(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("b.txt".into()),
    );
    assert_eq!(
        stage_of(&state).0,
        "准备内部文件",
        "点另一条时舞台先换成准备"
    );
    let preparing = mount(state);
    assert!(
        preparing.keyed("inspect-archive-list").is_some(),
        "准备时名单还在"
    );
    assert!(preparing.find("准备内部文件").is_some());
    assert!(preparing.find("alpha").is_none(), "准备下一条时不留上一条");
    let mut state = unmount(preparing);
    let preview = take(&mut state, &zip);
    finish(&mut state, "pack.zip", Ok(preview));
    let harness = mount(state);
    assert!(
        harness.keyed("inspect-archive-list").is_some(),
        "换成另一条后名单还在"
    );
    assert!(harness.find("beta").is_some(), "舞台换成这一条");
    assert!(harness.find("alpha").is_none());
    assert!(shows(&harness, "photos"), "点文件没有进入目录");
    assert_beside(
        &harness,
        "inspect-archive-list-pane",
        "inspect-archive-stage-pane",
    );
}

/// 左边一块和右边一块的布局盒要左右分开，并且在竖直方向上重叠。
fn assert_beside(
    harness: &crate::shell::view_harness::ShellHarness,
    left_key: &str,
    right_key: &str,
) {
    let left = bounds(harness, left_key);
    let right = bounds(harness, right_key);
    assert!(left.0 < right.0, "名单应在舞台左边：{left:?} {right:?}");
    let overlap = (left.1 + left.3).min(right.1 + right.3) - left.1.max(right.1);
    assert!(overlap > 1.0, "名单和舞台应并排：{left:?} {right:?}");
}

fn bounds(harness: &crate::shell::view_harness::ShellHarness, key: &str) -> (f32, f32, f32, f32) {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    let placed = harness
        .document()
        .context()
        .world()
        .layout_box(id)
        .unwrap_or_else(|| panic!("{key} 没有布局"));
    (placed.x, placed.y, placed.width, placed.height)
}

/// 面包屑第一段是压缩包文件名：路径最后一段，扩展名留着。点它仍回到根。没有名字时仍写「压缩包」。
#[test]
fn archive_breadcrumb_root_is_the_archive_filename() {
    let zip = sample_zip(&[("photos/", b""), ("a.txt", b"hi")]);
    let mut state = opened_archive(&zip, "Books/volume.cbz");
    let root = mount_archive(&state);
    assert_eq!(
        crumb_text(&root, "inspect-archive-crumb-root"),
        "volume.cbz",
        "第一段是带扩展名的文件名"
    );

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive("photos".into()),
    );
    let mut inside = mount_archive(&state);
    assert_eq!(
        crumb_text(&inside, "inspect-archive-crumb-root"),
        "volume.cbz",
        "进入下一层后第一段仍是压缩包文件名"
    );
    assert_eq!(
        crumb_text(&inside, "inspect-archive-crumb-0"),
        "photos",
        "后面的目录段还在"
    );
    let crumb = inside.keyed("inspect-archive-crumb-root").expect("第一段");
    let (x, y) = inside.center(crumb);
    let messages = inside.click_messages(x, y);
    inside.settle(messages);
    assert!(
        inside.model.inspect.archive_directory.is_empty(),
        "点第一段回到根"
    );
    assert!(
        inside.keyed("inspect-archive-list").is_some(),
        "回到根以后名单还在"
    );
    assert!(
        inside.keyed("inspect-archive-crumb-0").is_none(),
        "回到根以后只剩第一段"
    );
    assert!(
        inside.keyed("inspect-archive-crumb-root").is_some(),
        "回到根以后第一段还在"
    );
    assert_eq!(
        crumb_text(&inside, "inspect-archive-crumb-root"),
        "volume.cbz"
    );

    state.target_path = Some(String::new());
    let missing = mount_archive(&state);
    assert_eq!(
        crumb_text(&missing, "inspect-archive-crumb-root"),
        "压缩包",
        "路径是空的时第一段仍是「压缩包」"
    );
    assert_eq!(
        crumb_text(&missing, "inspect-archive-crumb-0"),
        "photos",
        "后面的目录段不变"
    );

    state.target_path = Some(r"D:\Books\volume.cbz".into());
    assert_eq!(
        crumb_text(&mount_archive(&state), "inspect-archive-crumb-root"),
        "volume.cbz",
        "反斜杠路径也取最后一段"
    );
    state.target_path = Some("Books/".into());
    assert_eq!(
        crumb_text(&mount_archive(&state), "inspect-archive-crumb-root"),
        "压缩包",
        "路径没有最后一段时仍是「压缩包」"
    );
    state.target_path = Some("   ".into());
    assert_eq!(
        crumb_text(&mount_archive(&state), "inspect-archive-crumb-root"),
        "压缩包",
        "只有空白时仍是「压缩包」"
    );
}

/// 空压缩包和空目录仍写 Vue 工具条的「0 项」，列表里只有「空目录」。根上第一段是带扩展名的文件名。
#[test]
fn archive_empty_directory_shows_vue_zero_count() {
    let empty = opened_archive(&sample_zip(&[]), "empty.zip");
    let root = mount_archive(&empty);
    assert_eq!(
        crumb_text(&root, "inspect-archive-crumb-root"),
        "empty.zip",
        "空包第一段仍是文件名"
    );
    assert!(root.find("0 项").is_some(), "空包也要写出项数");
    assert!(root.find("空目录").is_some());
    assert!(root.find("选择内部文件进行预览").is_some(), "空闲提示还在");
    assert!(
        !node_texts(&root)
            .iter()
            .any(|text| text.contains("这个压缩包里没有")),
        "不另编一句说明"
    );

    let zip = sample_zip(&[("photos/", b""), ("a.txt", b"hi")]);
    let mut state = opened_archive(&zip, "pack.zip");
    let filled = mount_archive(&state);
    assert!(filled.find("2 项").is_some(), "有条目时项数仍在");
    assert!(filled.find("空目录").is_none(), "有条目时不是空目录");

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive("photos".into()),
    );
    let inside = mount_archive(&state);
    assert_eq!(
        crumb_text(&inside, "inspect-archive-crumb-root"),
        "pack.zip"
    );
    assert_eq!(crumb_text(&inside, "inspect-archive-crumb-0"), "photos");
    assert!(inside.find("0 项").is_some(), "空目录也要写出项数");
    assert!(inside.find("空目录").is_some());
    assert!(inside.find("选择内部文件进行预览").is_some());
    assert!(
        inside.find("a.txt").is_none(),
        "空目录不把上一层的文件列出来"
    );
    assert!(
        !node_texts(&inside)
            .iter()
            .any(|text| text.contains("这个目录里没有")),
        "不另编一句说明"
    );
}

/// 名单上能读到的无障碍名和值。
fn node_texts(harness: &crate::shell::view_harness::ShellHarness) -> Vec<String> {
    harness
        .nodes()
        .into_iter()
        .flat_map(|node| {
            [
                node.label.map(|label| label.to_string()),
                node.value.as_ref().map(|value| value.to_string()),
            ]
            .into_iter()
            .flatten()
        })
        .collect()
}

/// 名单行首照 Vue `fileKindLabel`：目录写 DIR，扩展名前四位大写，没有扩展名写 FILE。大小和「目录」仍在行尾。
#[test]
fn archive_row_icons_use_vue_file_kind_label() {
    let zip = sample_zip(&[
        ("photos/", b""),
        ("notes.docx", b"hi"),
        ("README", b"x"),
        ("chapter.markdown", b"y"),
        (".gitignore", b"z"),
    ]);
    let state = opened_archive(&zip, "pack.zip");
    let harness = mount_archive(&state);
    assert_eq!(kind_text(&harness, 0), "DIR", "目录行首是 DIR");
    assert_eq!(kind_text(&harness, 1), "FILE", "只有前导点时没有扩展名");
    assert_eq!(kind_text(&harness, 2), "MARK", "扩展名只取前四位");
    assert_eq!(kind_text(&harness, 3), "DOCX");
    assert_eq!(kind_text(&harness, 4), "FILE", "没有点就写 FILE");
    assert!(row_text(&harness, 0).contains("目录"), "目录说明还在行尾");
    assert!(row_text(&harness, 3).contains("notes.docx"));
    assert!(row_text(&harness, 3).contains("2 B"), "条目大小还在行尾");
}

/// 名单行的无障碍名：文件名，后面是目录说明或大小。
fn row_text(harness: &crate::shell::view_harness::ShellHarness, index: usize) -> String {
    let key = format!("inspect-archive-row-{index}");
    let id = harness
        .keyed(&key)
        .unwrap_or_else(|| panic!("没有名单行 {key}"));
    harness
        .nodes()
        .into_iter()
        .find(|node| node.id == id)
        .and_then(|node| node.label.map(|label| label.to_string()))
        .unwrap_or_default()
}

/// 行首种类上的字。
fn kind_text(harness: &crate::shell::view_harness::ShellHarness, index: usize) -> String {
    let key = format!("inspect-archive-kind-{index}");
    let id = harness
        .keyed(&key)
        .unwrap_or_else(|| panic!("没有行首种类 {key}"));
    harness
        .nodes()
        .into_iter()
        .find(|node| node.id == id)
        .and_then(|node| node.label.map(|label| label.to_string()))
        .unwrap_or_default()
}

/// 面包屑按钮上的字。键落在按钮上，无障碍名就是这一段。
fn crumb_text(harness: &crate::shell::view_harness::ShellHarness, key: &str) -> String {
    let id = harness
        .keyed(key)
        .unwrap_or_else(|| panic!("没有面包屑 {key}"));
    harness
        .nodes()
        .into_iter()
        .find(|node| node.id == id)
        .and_then(|node| node.label.map(|label| label.to_string()))
        .unwrap_or_default()
}

fn stage_of(state: &super::super::super::InspectState) -> (&str, &str, bool) {
    crate::shell::inspect_library::archive_stage(state)
}

fn mount_archive(
    state: &super::super::super::InspectState,
) -> crate::shell::view_harness::ShellHarness {
    let mut model = crate::shell::ShellViewModel::for_page(crate::shell::ShellPage::SelectedFile);
    model.inspect = state.clone();
    crate::shell::view_harness::ShellHarness::mount(model)
}

fn finish(
    state: &mut super::super::super::InspectState,
    path: &str,
    result: Result<super::super::super::bridge::ArchiveEntryPreview, String>,
) {
    let (entry, through) = match state.archive_request.clone() {
        Some(key) => request_parts(&key),
        None => panic!("没有待回来的包内文件"),
    };
    let generation = state.generation;
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::ArchiveEntryLoaded {
            path: path.into(),
            entry,
            through,
            generation,
            result,
        },
    );
}

/// 和 `request_key` 同一套分隔：嵌套路径在前，条目名在最后一个分隔之后。
fn request_parts(key: &str) -> (String, Vec<String>) {
    let mut parts = key.split('\u{1}').map(str::to_string).collect::<Vec<_>>();
    let entry = parts.pop().unwrap_or_default();
    if parts.first().is_some_and(|part| part.is_empty()) {
        parts.remove(0);
    }
    (entry, parts)
}

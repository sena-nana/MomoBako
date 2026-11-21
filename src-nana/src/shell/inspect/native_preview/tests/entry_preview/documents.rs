//! 压缩包内条目预览 PDF 页面、嵌套名称、Office 文本与模型光栅的测试。

use super::*;

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
    assert_eq!(super::super::entry_text(&note), "inside");
    finish(&mut state, "pack.zip", Ok(note));
    assert_eq!(
        super::super::entry_text(state.archive_entry.as_ref().expect("内层文本")),
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
    assert_eq!(super::super::entry_text(&file), "hi");
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
    let mut harness = mount(state);
    let surface = harness.keyed("inspect-model").expect("用现有模型预览");
    assert!(harness.keyed("inspect-orbit").is_some());
    let (x, y) = harness.center(surface);
    let turns = harness
        .drag_messages((x - 60.0, y + 40.0), (x - 20.0, y + 40.0))
        .into_iter()
        .filter_map(|message| match message {
            crate::shell::ShellMessage::Inspect(InspectMessage::Orbit { yaw, .. }) => Some(yaw),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(turns.len() > 1, "拖拽按采样分几次转动：{turns:?}");
    let total: f32 = turns.iter().sum();
    assert!(
        (total - 0.4).abs() < 0.001,
        "向右拖 40px 一共转 0.4：{total}"
    );
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

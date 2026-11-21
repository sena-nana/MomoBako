//! 压缩包目录点击、空闲与失败说明、打开文件布局、面包屑、空目录与行图标的测试。

use super::*;

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

    let pick = |state: &mut super::super::super::super::InspectState, path: &str| {
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

/// 名单还在时，旁边的提示：空闲写当前目录和「选择内部文件进行预览」，
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

/// 空压缩包和空目录的工具条仍写「0 项」，列表里只有「空目录」。根上第一段是带扩展名的文件名。
#[test]
fn archive_empty_directory_shows_a_zero_count() {
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

/// 名单行首：目录写 DIR，扩展名前四位大写，没有扩展名写 FILE。大小和「目录」仍在行尾。
#[test]
fn archive_row_icons_use_the_file_kind_label() {
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

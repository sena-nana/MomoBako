//! 作品队列排序、标题与状态拆分、跟随预览列表，以及保存封面的测试。

use super::*;

/// L7：作品队列按音轨路径的自然序排，`2.mp3` 在 `10.mp3` 前面。
#[test]
fn work_queue_sorts_tracks_naturally() {
    let files = [
        ("work/1.mp3", audio(Some("work"), &[])),
        ("work/10.mp3", audio(Some("work"), &[])),
        ("work/2.mp3", audio(Some("work"), &[])),
    ];
    let model = scene(&files, "work/1.mp3");
    let panels = super::super::preview_panels(&model).expect("有作品队列");
    let (count, items) = panels.queue.expect("作品队列");
    assert_eq!(count, "3 轨");
    assert_eq!(
        items
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>(),
        ["work/2.mp3", "work/10.mp3"]
    );
}

/// 作品队列行是左标题、右状态，不是「标题 状态」一整句。状态是 `listeningStatus` 原文，
/// 没有这个字段时是占位 `unlistened`。正在预览的音轨不在队列里，
/// 作品队列不标当前项，行不画成选中；没有别的音轨时不编造作品。
#[test]
fn work_queue_splits_the_title_from_the_status() {
    let files = [
        (
            "work/01.mp3",
            audio(
                Some("work"),
                &[
                    ("trackTitle", json!("序章")),
                    ("listeningStatus", json!("listening")),
                ],
            ),
        ),
        (
            "work/02.mp3",
            audio(Some("work"), &[("trackTitle", json!("第二轨"))]),
        ),
    ];
    let model = scene(&files, "work/01.mp3");
    let panels = super::super::preview_panels(&model).expect("有作品队列");
    let (count, items) = panels.queue.expect("作品队列");
    assert_eq!(count, "2 轨");
    assert_eq!(items.len(), 1, "正在预览的序章不进作品队列");
    assert_eq!(items[0].path, "work/02.mp3");
    assert_eq!(items[0].title, "第二轨");
    assert_eq!(
        items[0].status, "unlistened",
        "没有 listeningStatus 时用 Vue 的占位，不编中文"
    );

    let other = scene(&files, "work/02.mp3");
    let (_, siblings) = super::super::preview_panels(&other)
        .expect("有作品队列")
        .queue
        .expect("作品队列");
    assert_eq!(siblings[0].title, "序章");
    assert_eq!(
        siblings[0].status, "listening",
        "有字段时原样显示，不翻成「收听中」"
    );

    let alone = scene(
        &[("solo.mp3", audio(None, &[("trackTitle", json!("独奏"))]))],
        "solo.mp3",
    );
    assert!(
        super::super::preview_panels(&alone).is_none_or(|panels| panels.queue.is_none()),
        "没有别的音轨时不编造作品队列"
    );

    let mut harness = crate::shell::view_harness::ShellHarness::mount(model);
    harness.frame();
    let row_key = "inspect-asmr-queue-work\u{2215}02.mp3";
    let row = harness.keyed(row_key).expect("作品队列行");
    let world = harness.document().context().world();
    assert_eq!(world.text(row), Some("第二轨"), "行上主文本只有标题");
    let access = world.accessibility(row).expect("行的无障碍");
    assert_eq!(
        access.label.as_deref(),
        Some("第二轨  unlistened"),
        "状态是行尾补充，读屏在标题后面念"
    );
    assert_ne!(access.selected, Some(true), "作品队列没有当前项，不画成选中");
    let list = harness
        .keyed("inspect-asmr-queue-list")
        .expect("作品队列列表");
    let named = harness.nodes().into_iter().any(|node| {
        node.role == nana_ui::runtime::AccessibilityRole::List
            && node.label.as_deref() == Some("作品队列")
            && world.is_descendant_or_self(node.id, list)
    });
    assert!(named, "队列行放在带名字的列表里");
}

/// 作品队列按预览条目和当前列表取音轨：
/// 当前文件在列表里就用这一行的元数据，不在列表里再用播放集条目。
/// 详情叠层和还没保存的草稿不改分组、标题、状态。没有作品目录时不把别的音轨编进来。
/// 列表行没有的标题不编，收听状态仍用占位 `unlistened`。
#[test]
fn work_queue_follows_the_preview_list_not_the_draft() {
    let files = [
        (
            "work/01.mp3",
            audio(Some("work-a"), &[("trackTitle", json!("当前轨"))]),
        ),
        (
            "work/02.mp3",
            audio(
                Some("work-a"),
                &[
                    ("trackTitle", json!("列表第二轨")),
                    ("listeningStatus", json!("listening")),
                ],
            ),
        ),
        ("work/04.mp3", audio(Some("work-a"), &[])),
        (
            "work/03.mp3",
            audio(Some("work-b"), &[("trackTitle", json!("另一部"))]),
        ),
    ];
    let listed = [
        ("work/02.mp3", "列表第二轨", "listening"),
        ("work/04.mp3", "04.mp3", "unlistened"),
    ];
    let mut model = scene(&files, "work/01.mp3");
    assert_queue(&model, "3 轨", &listed);

    let overlaid = audio(
        Some("work-b"),
        &[
            ("trackTitle", json!("详情音轨")),
            ("series", json!("不该出现")),
        ],
    );
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        "work/01.mp3",
        &overlaid,
        2,
    ))));
    assert!(
        super::super::super::files_view::previewing(&model),
        "详情到达后仍在预览页"
    );
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("workRoot")
            .and_then(Value::as_str),
        Some("work-b"),
        "草稿已换成详情里的作品目录"
    );
    assert_queue(&model, "3 轨", &listed);

    let patched = model.inspect.apply_metadata_patch(
        &asset_id("work/01.mp3"),
        BTreeMap::from([
            ("workRoot".into(), json!("work-b")),
            ("trackTitle".into(), json!("草稿轨")),
            ("listeningStatus".into(), json!("drafted")),
        ]),
    );
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("trackTitle")
            .and_then(Value::as_str),
        Some("草稿轨")
    );
    assert_queue(&model, "3 轨", &listed);

    let bare = [
        ("solo.mp3", audio(None, &[("trackTitle", json!("独奏"))])),
        (
            "work/02.mp3",
            audio(Some("work-a"), &[("trackTitle", json!("不该进来"))]),
        ),
    ];
    let mut alone = scene(&bare, "solo.mp3");
    assert!(queue_of(&alone).is_none(), "列表没有作品目录时不编造队列");
    let forced = audio(Some("work-a"), &[("trackTitle", json!("详情目录"))]);
    alone.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        "solo.mp3", &forced, 2,
    ))));
    assert!(
        queue_of(&alone).is_none(),
        "详情里的作品目录不把别的音轨编进来"
    );
    let patched = alone.inspect.apply_metadata_patch(
        &asset_id("solo.mp3"),
        BTreeMap::from([("workRoot".into(), json!("work-a"))]),
    );
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert!(
        queue_of(&alone).is_none(),
        "草稿里的作品目录不把别的音轨编进来"
    );

    let mut playlist = ShellViewModel::for_page(ShellPage::FileList);
    playlist.files.rows = vec![
        row(
            "album/02.mp3",
            &audio(
                Some("album"),
                &[
                    ("trackTitle", json!("列表轨")),
                    ("listeningStatus", json!("listening")),
                ],
            ),
        ),
        row(
            "album/03.mp3",
            &audio(Some("other"), &[("trackTitle", json!("另一部"))]),
        ),
    ];
    playlist.player.listed = Some(playlist_page(
        "album/01.mp3",
        &audio(Some("album"), &[("trackTitle", json!("播放集轨"))]),
    ));
    playlist.reduce(ShellMessage::Player(PlayerMessage::OpenPreview {
        item_id: Some("item".into()),
    }));
    assert!(
        super::super::super::files_view::previewing(&playlist),
        "播放集打开预览页"
    );
    assert!(
        playlist.inspect.current_asset_id().is_none(),
        "详情还没回来，队列不靠草稿"
    );
    assert_queue(
        &playlist,
        "2 轨",
        &[("album/02.mp3", "列表轨", "listening")],
    );

    let foreign = audio(Some("other"), &[("trackTitle", json!("详情音轨"))]);
    playlist.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        "album/01.mp3",
        &foreign,
        2,
    ))));
    assert_eq!(
        playlist
            .inspect
            .draft_custom()
            .get("workRoot")
            .and_then(Value::as_str),
        Some("other")
    );
    assert_queue(
        &playlist,
        "2 轨",
        &[("album/02.mp3", "列表轨", "listening")],
    );
    let patched = playlist.inspect.apply_metadata_patch(
        &asset_id("album/01.mp3"),
        BTreeMap::from([
            ("workRoot".into(), json!("other")),
            ("trackTitle".into(), json!("草稿轨")),
        ]),
    );
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert_queue(
        &playlist,
        "2 轨",
        &[("album/02.mp3", "列表轨", "listening")],
    );
}

/// 作品信息的「保存封面」和候选上的「封面」按已有的 http(s) 地址下载，写成自定义缩略图。
/// 别的网址、回收站和目录不下载；没写出文件时不编造封面。
#[test]
fn asmr_save_cover_downloads_the_https_cover_and_writes_the_thumbnail() {
    let ftp = scene(
        &[(
            "voice.mp3",
            audio(None, &[("coverUrl", json!("ftp://cdn.example/a.jpg"))]),
        )],
        "voice.mp3",
    );
    assert!(works_cover(&ftp).is_none(), "ftp 不是封面地址");
    let blank = scene(
        &[("voice.mp3", audio(None, &[("cover", json!("  "))]))],
        "voice.mp3",
    );
    assert!(works_cover(&blank).is_none(), "空白封面不画按钮");

    let cover = "HTTP://cdn.example/work.jpg";
    let candidate_cover = "https://cdn.example/candidate.jpg";
    let files = [(
        "voice.mp3",
        audio(
            None,
            &[
                ("cover", json!(cover)),
                (
                    "providerCandidates",
                    json!([{ "source": "dlsite", "fields": { "circle": "社团", "coverUrl": candidate_cover } }]),
                ),
            ],
        ),
    )];
    let mut model = scene(&files, "voice.mp3");
    let offer = works_cover(&model).expect("cover 字段的 http 地址");
    assert_eq!(offer.url, cover);
    assert_eq!(offer.label, "保存封面");
    assert!(!offer.disabled);

    let mut directory = scene(&files, "voice.mp3");
    directory.files.rows[0].kind = "directory".into();
    assert!(works_cover(&directory).expect("目录仍画出按钮").disabled);
    directory.reduce(ShellMessage::Asmr(AsmrMessage::SaveCover(cover.into())));
    assert!(cover_saves(&mut directory).is_empty(), "目录不能保存封面");

    model.reduce(ShellMessage::Asmr(AsmrMessage::SaveCover(
        "https://evil.example/a.jpg".into(),
    )));
    assert!(
        cover_saves(&mut model).is_empty(),
        "不是这个文件或候选的地址不下载"
    );
    model.files.trash = true;
    model.reduce(ShellMessage::Asmr(AsmrMessage::SaveCover(cover.into())));
    assert!(cover_saves(&mut model).is_empty(), "回收站不保存封面");
    model.files.trash = false;

    let repo = model.workspace.active_repo_id.clone().expect("活动仓库");
    model.reduce(ShellMessage::Files(FilesMessage::ThumbnailEnsured {
        repo_id: repo.clone(),
        path: "voice.mp3".into(),
        result: Ok(EnsuredThumbnail {
            thumbnail_path: Some("thumbs/old.jpg".into()),
            custom: false,
            metadata: None,
        }),
    }));
    let _ = model.files.take_effects();

    let mut harness = ShellHarness::mount(model);
    click_revealed(&mut harness, "inspect-asmr-save-cover");
    assert_eq!(
        cover_saves(&mut harness.model),
        vec![(repo.clone(), "voice.mp3".into(), cover.into())]
    );
    assert!(harness.find("保存中").is_some(), "保存期间按钮写保存中");
    assert!(harness.find("保存封面").is_none());
    click_revealed(&mut harness, "inspect-asmr-save-cover");
    if harness.keyed("inspect-asmr-cover-0").is_some() {
        click_revealed(&mut harness, "inspect-asmr-cover-0");
    }
    assert!(
        cover_saves(&mut harness.model).is_empty(),
        "进行中不重复提交"
    );

    harness.apply(ShellMessage::Asmr(AsmrMessage::CoverSaved {
        repo_id: repo.clone(),
        path: "other.mp3".into(),
        result: Ok(EnsuredThumbnail {
            thumbnail_path: Some("thumbs/nope.jpg".into()),
            custom: true,
            metadata: None,
        }),
    }));
    assert_eq!(
        harness.model.files.rows[0].thumbnail_path.as_deref(),
        Some("thumbs/old.jpg")
    );
    harness.flush();
    assert!(
        harness.find("保存中").is_some(),
        "别的路径的结果不结束这次保存"
    );

    harness.apply(ShellMessage::Asmr(AsmrMessage::CoverSaved {
        repo_id: repo.clone(),
        path: "voice.mp3".into(),
        result: Ok(EnsuredThumbnail {
            thumbnail_path: None,
            custom: true,
            metadata: None,
        }),
    }));
    assert_eq!(
        harness.model.files.rows[0].thumbnail_path.as_deref(),
        Some("thumbs/old.jpg"),
        "没有路径就不编造封面"
    );
    harness.flush();
    assert!(harness.find("保存封面").is_some());

    click_revealed(&mut harness, "inspect-asmr-save-cover");
    assert_eq!(cover_saves(&mut harness.model).len(), 1);
    harness.apply(ShellMessage::Asmr(AsmrMessage::CoverSaved {
        repo_id: repo.clone(),
        path: "voice.mp3".into(),
        result: Ok(EnsuredThumbnail {
            thumbnail_path: Some("thumbs/cover.jpg".into()),
            custom: true,
            metadata: None,
        }),
    }));
    assert_eq!(
        harness.model.files.rows[0].thumbnail_path.as_deref(),
        Some("thumbs/cover.jpg")
    );
    assert!(harness.model.files.rows[0].thumbnail_custom);
    assert!(
        matches!(harness.model.files.take_effects().as_slice(), [FilesEffect::DecodeThumbnails { paths }] if paths == &["thumbs/cover.jpg".to_string()])
    );
    harness.flush();
    assert!(harness.find("保存封面").is_some());

    click_revealed(&mut harness, "inspect-asmr-cover-0");
    assert_eq!(
        cover_saves(&mut harness.model),
        vec![(repo.clone(), "voice.mp3".into(), candidate_cover.into())]
    );
    harness.apply(ShellMessage::Asmr(AsmrMessage::CoverSaved {
        repo_id: repo,
        path: "voice.mp3".into(),
        result: Err("拒绝访问".into()),
    }));
    assert_eq!(harness.model.files.error, "拒绝访问");
    assert_eq!(
        harness.model.files.rows[0].thumbnail_path.as_deref(),
        Some("thumbs/cover.jpg")
    );
    harness.flush();
    assert!(harness.find("保存封面").is_some());
}

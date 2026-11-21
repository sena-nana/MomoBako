//! 元数据草稿编辑、保存、旧标签组、过期保存结果、冲突与自动保存的测试。

use super::*;

#[test]
fn metadata_draft_tracks_edits_and_refuses_invalid_saves() {
    let mut state = InspectState::default();
    state.note_detail(&asset(
        "pics/a.png",
        "png",
        3,
        false,
        vec![
            meta("note", json!("from-note")),
            meta("tagGroups", json!(["kept"])),
        ],
    ));
    assert_eq!(state.draft.comment, "from-note");
    assert_eq!(state.draft.tags, ["kept"]);
    state.reduce(true, Some("repo"), InspectMessage::SetComment("hi".into()));
    state.reduce(true, Some("repo"), InspectMessage::SetRating(4));
    state.reduce(true, Some("repo"), InspectMessage::SetRating(4));
    assert_eq!(state.draft.rating, 0);
    state.reduce(true, Some("repo"), InspectMessage::SetRating(4));
    state.reduce(true, Some("repo"), InspectMessage::AddTag("  ".into()));
    state.reduce(true, Some("repo"), InspectMessage::AddTag("a".into()));
    state.reduce(true, Some("repo"), InspectMessage::AddTag("a".into()));
    assert!(state.dirty());
    state.take_effects();
    // 补丁不收空键和编辑器自己的四个输入，别的键按原样写，写完立刻保存（编辑器还没存的改动一起写）。
    let patch = [
        (" ", json!("x")),
        ("rating", json!("no")),
        ("artist", json!("momo")),
    ]
    .map(|(key, value)| (key.to_string(), value));
    state
        .apply_metadata_patch("asset", patch.into_iter().collect())
        .expect("补丁写进当前素材");
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    let InspectEffect::SaveMetadata {
        repo_id,
        asset_id,
        expected_version,
        metadata,
    } = state.take_effects().pop().unwrap()
    else {
        panic!("没有保存请求");
    };
    assert!(state.effects.is_empty(), "保存中再要求保存不会再发一次");
    assert_eq!(repo_id, "repo");
    assert_eq!(asset_id, "asset");
    assert_eq!(expected_version, 3);
    assert_eq!(metadata.get("comment"), Some(&json!("hi")));
    assert_eq!(metadata.get("rating"), Some(&json!(4)));
    assert_eq!(metadata.get("tagGroups"), Some(&json!(["kept", "a"])));
    assert_eq!(metadata.get("artist"), Some(&json!("momo")));
    assert!(
        !metadata.contains_key(" ") && metadata.len() == 4,
        "{metadata:?}"
    );
    assert!(state.saving);
    assert!(
        state
            .apply_metadata_patch("other", Default::default())
            .is_err(),
        "补丁只写正在看的素材"
    );

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::MetadataSaved(Err("磁盘错误".into())),
    );
    assert!(!state.saving);
    assert_eq!(state.draft.comment, "hi");
    assert!(state.error.contains("磁盘错误"));
}

/// 只改注释时，保存只写注释和补丁写进来的字段：没动过的评分、标签组和元数据里原有的布尔、数字、数组
/// 都不回写，免得被改存成字符串，或者把旧格式的标签组改写掉。补丁里的数组、数字按原样写。
#[test]
fn saving_leaves_untouched_fields_alone() {
    let mut state = InspectState::default();
    state.note_detail(&asset(
        "voice/a.mp3",
        "mp3",
        2,
        false,
        vec![
            meta("favorite", json!(false)),
            meta("trackDurationMs", json!(125000)),
            meta("voiceActors", json!(["甲", "乙"])),
            meta("circle", json!("社团")),
        ],
    ));
    state.take_effects();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetComment("夜里听".into()),
    );
    let patch = [
        ("circle", json!("新社团")),
        ("price", json!(1320)),
        ("scenarioTags", json!(["治愈", "耳语"])),
    ];
    state
        .apply_metadata_patch(
            "asset",
            patch
                .map(|(key, value)| (key.to_string(), value))
                .into_iter()
                .collect(),
        )
        .expect("补丁");
    let InspectEffect::SaveMetadata { metadata, .. } = state.take_effects().pop().unwrap() else {
        panic!("没有保存请求");
    };
    assert_eq!(metadata.get("comment"), Some(&json!("夜里听")));
    assert_eq!(
        metadata.get("circle"),
        Some(&json!("新社团")),
        "改过的字段照常保存"
    );
    assert_eq!(metadata.get("price"), Some(&json!(1320)), "数字还是数字");
    assert_eq!(
        metadata.get("scenarioTags"),
        Some(&json!(["治愈", "耳语"])),
        "数组还是数组"
    );
    for key in [
        "rating",
        "tagGroups",
        "link",
        "favorite",
        "trackDurationMs",
        "voiceActors",
    ] {
        assert!(
            !metadata.contains_key(key),
            "没改的 {key} 不回写：{metadata:?}"
        );
    }
}

/// 旧格式的标签组 `{ label, tags }` 展平进编辑器（有 tags 取 tags，没有取 label）；
/// 只改评分时标签组不回写，旧数据原样留着；改了标签才写展平后的字符串数组。
#[test]
fn legacy_tag_groups_show_their_tags_and_survive_unrelated_saves() {
    let legacy = json!([{ "label": "角色", "tags": ["主角", " 配角 "] }, { "label": "场景" }, "单独", { "label": "空", "tags": [] }, 7]);
    let mut state = InspectState::default();
    state.note_detail(&asset(
        "pics/a.png",
        "png",
        1,
        false,
        vec![meta("tagGroups", legacy)],
    ));
    state.take_effects();
    assert_eq!(state.draft.tags, ["主角", "配角", "场景", "单独", "空"]);
    state.reduce(true, Some("repo"), InspectMessage::SetRating(3));
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    let InspectEffect::SaveMetadata { metadata, .. } = state.take_effects().pop().unwrap() else {
        panic!("没有保存请求");
    };
    assert_eq!(
        metadata,
        [("rating".to_string(), json!(3))].into_iter().collect(),
        "只写改了的评分"
    );

    let saved = asset(
        "pics/a.png",
        "png",
        2,
        false,
        vec![
            meta("rating", json!(3)),
            meta("tagGroups", json!([{ "label": "角色", "tags": ["主角"] }])),
        ],
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::MetadataSaved(Ok(("success".into(), saved))),
    );
    state.reduce(true, Some("repo"), InspectMessage::RemoveTag("主角".into()));
    state.reduce(true, Some("repo"), InspectMessage::AddTag("新".into()));
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    let InspectEffect::SaveMetadata { metadata, .. } = state.take_effects().pop().unwrap() else {
        panic!("没有保存请求");
    };
    assert_eq!(metadata.get("tagGroups"), Some(&json!(["新"])));
}

/// 换选前写出去的上一个素材的保存结果回来时，不动当前素材：不换回上一个文件的预览和草稿，冲突也不落到
/// 当前素材上（那样「采用服务器版本」会把上一个文件的元数据写进当前文件），只交给状态区报出来。
#[test]
fn save_results_of_the_previous_file_stay_off_the_current_file() {
    let mut model = writable_shell();
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "pics/a.png",
        "png",
        1,
        false,
        Vec::new(),
    ))));
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "给 a".into(),
    )));
    model.inspect.take_effects();
    model.reduce(ShellMessage::SelectFile {
        path: "pics/b.png".into(),
        asset_id: Some("b".into()),
    });
    assert!(
        matches!(model.inspect.take_effects().as_slice(), [InspectEffect::SaveMetadata { asset_id, .. }] if asset_id == "asset"),
        "换选前写出上一个文件"
    );
    let mut other = asset(
        "pics/b.png",
        "png",
        5,
        false,
        vec![meta("comment", json!("b 的注释"))],
    );
    other.summary.asset_id = "b".into();
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(other)));
    model.inspect.take_effects();
    assert!(
        !model.inspect.can_edit(),
        "上一个文件的保存还在路上，同一时刻只发一次"
    );

    let stale = asset(
        "pics/a.png",
        "png",
        9,
        false,
        vec![meta("comment", json!("别处写的"))],
    );
    model.reduce(ShellMessage::Inspect(InspectMessage::MetadataSaved(Ok((
        "conflict".into(),
        stale,
    )))));
    assert_eq!(
        model.inspect.target_path.as_deref(),
        Some("pics/b.png"),
        "预览不回到上一个文件"
    );
    assert_eq!(model.inspect.draft_comment(), "b 的注释");
    assert!(model.inspect.conflict.is_empty(), "冲突不落到当前文件");
    assert!(model.inspect.can_edit());
    let failure = model
        .status
        .failure()
        .map(|failure| failure.message.clone());
    assert_eq!(
        failure.as_deref(),
        Some("保存元数据失败：「a.png」版本冲突，修改没有写入")
    );
}

/// 保存还在路上时补进草稿的字段，结果回来以后留着并接着保存，不会被这次结果冲掉。
#[test]
fn a_patch_made_while_saving_survives_the_result_and_saves_next() {
    let mut state = InspectState::default();
    state.note_detail(&asset("voice/a.mp3", "mp3", 1, false, Vec::new()));
    state.take_effects();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetComment("先存".into()),
    );
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    state.take_effects();
    state
        .apply_metadata_patch(
            "asset",
            [("circle".to_string(), json!("社团"))]
                .into_iter()
                .collect(),
        )
        .expect("保存中也收补丁");
    assert!(state.take_effects().is_empty(), "保存中不再发第二次");
    let saved = asset(
        "voice/a.mp3",
        "mp3",
        2,
        false,
        vec![meta("comment", json!("先存"))],
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::MetadataSaved(Ok(("success".into(), saved))),
    );
    assert_eq!(
        state.draft.custom.get("circle"),
        Some(&json!("社团")),
        "补丁还在草稿里"
    );
    assert_eq!(state.draft.comment, "先存");
    let InspectEffect::SaveMetadata {
        expected_version,
        metadata,
        ..
    } = state.take_effects().pop().expect("接着保存补丁")
    else {
        panic!("没有保存请求");
    };
    assert_eq!(expected_version, 2);
    assert_eq!(
        metadata,
        [("circle".to_string(), json!("社团"))]
            .into_iter()
            .collect()
    );
}

/// F8：保存成功以后同一素材的文件行换上读回的元数据和索引标签，详情导语不再是旧值；
/// 写了标签组的保存再静默重读仓库摘要，侧栏「未标签」计数、筛选栏和标签菜单的候选跟着变。只改注释不重读。
#[test]
fn saved_metadata_reaches_the_file_row_and_retagging_reloads_the_summary() {
    let mut model = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == "files-selected-metadata")
        .map(|(_, model)| model)
        .expect("files-selected-metadata 场景");
    model.workspace.take_effects();
    let saved_detail = |model: &ShellViewModel, version: i64, comment: &str, tags: &[&str]| {
        let mut detail = asset(
            "cover.png",
            "png",
            version,
            false,
            vec![
                meta("comment", json!(comment)),
                meta("tagGroups", json!(tags)),
            ],
        );
        detail.summary.asset_id = model
            .inspect
            .current_asset_id()
            .expect("cover.png 的素材")
            .to_string();
        detail.summary.repo_id = model.workspace.active_repo_id.clone().expect("活动仓库");
        detail.summary.tags = tags.iter().map(|tag| tag.to_string()).collect();
        detail
    };

    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "新注释".into(),
    )));
    model.reduce(ShellMessage::Inspect(InspectMessage::SaveMetadata));
    model.inspect.take_effects();
    let detail = saved_detail(&model, 2, "新注释", &["封面", "参考"]);
    model.reduce(ShellMessage::Inspect(InspectMessage::MetadataSaved(Ok((
        "success".into(),
        detail,
    )))));
    let row = model
        .files
        .rows
        .iter()
        .find(|row| row.path == "cover.png")
        .expect("cover.png 的行");
    assert_eq!(
        row.metadata.get("comment"),
        Some(&json!("新注释")),
        "文件行换上读回的元数据"
    );
    assert!(
        !model.workspace.effects.iter().any(|effect| matches!(
            effect,
            crate::shell::WorkspaceEffect::LoadSnapshotSilent { .. }
        )),
        "只改注释不重读摘要"
    );

    model.reduce(ShellMessage::Inspect(InspectMessage::AddTag(
        "新标签".into(),
    )));
    model.reduce(ShellMessage::Inspect(InspectMessage::SaveMetadata));
    model.inspect.take_effects();
    let detail = saved_detail(&model, 3, "新注释", &["封面", "参考", "新标签"]);
    model.reduce(ShellMessage::Inspect(InspectMessage::MetadataSaved(Ok((
        "success".into(),
        detail,
    )))));
    let row = model
        .files
        .rows
        .iter()
        .find(|row| row.path == "cover.png")
        .expect("cover.png 的行");
    assert_eq!(row.tags, ["封面", "参考", "新标签"], "索引标签跟着换");
    assert!(
        model.workspace.effects.iter().any(|effect| matches!(effect, crate::shell::WorkspaceEffect::LoadSnapshotSilent { repo_id } if repo_id == "acceptance-repo")),
        "写了标签组，静默重读仓库摘要"
    );
}

/// 回到目录（不再看文件）之前，等着自动保存的改动先写出去，不随预览一起丢掉。
#[test]
fn leaving_the_preview_flushes_the_pending_autosave() {
    let mut state = InspectState::default();
    state.note_detail(&asset("pics/a.png", "png", 1, false, Vec::new()));
    state.take_effects();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetComment("刚打的字".into()),
    );
    assert!(state.timer_due_in().is_some());
    state.clear();
    assert!(
        matches!(state.take_effects().as_slice(), [InspectEffect::SaveMetadata { metadata, .. }] if metadata.get("comment") == Some(&json!("刚打的字")))
    );
    assert!(!state.dirty(), "草稿跟着清掉，不留到下一个文件");
}

#[test]
fn conflict_keeps_the_draft_until_the_server_version_is_adopted() {
    let mut state = InspectState::default();
    state.note_detail(&asset("pics/a.png", "png", 3, false, Vec::new()));
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetComment("local".into()),
    );
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    state.take_effects();
    let server = asset(
        "pics/a.png",
        "png",
        9,
        false,
        vec![meta("comment", json!("server"))],
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::MetadataSaved(Ok(("conflict".into(), server))),
    );
    assert_eq!(state.draft.comment, "local");
    assert_eq!(state.expected_version, 3);
    assert_eq!(state.conflict, "版本冲突，未写入");
    state.reduce(true, Some("repo"), InspectMessage::AdoptConflict);
    assert_eq!(state.draft.comment, "server");
    assert_eq!(state.expected_version, 9);
    assert!(!state.dirty());
    assert!(state.conflict.is_empty());
}

#[test]
fn successful_save_on_the_same_path_does_not_restart_preview() {
    let mut state = InspectState::default();
    state.note_detail(&asset("pics/a.png", "png", 3, false, Vec::new()));
    state.take_effects();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetLink("https://example.test".into()),
    );
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    state.take_effects();
    let saved = asset(
        "pics/a.png",
        "png",
        4,
        false,
        vec![meta("link", json!("https://example.test"))],
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::MetadataSaved(Ok(("success".into(), saved))),
    );
    assert!(!state.dirty());
    assert_eq!(state.expected_version, 4);
    assert!(state.effects.is_empty());
    assert!(matches!(state.body, PreviewBody::Empty));
}

#[test]
fn virtual_clean_and_unrecognized_metadata_results_do_not_save() {
    let mut virtual_asset = InspectState::default();
    virtual_asset.note_detail(&asset("pics/a.png", "png", 1, true, Vec::new()));
    virtual_asset.take_effects();
    virtual_asset.reduce(true, Some("repo"), InspectMessage::SetComment("x".into()));
    virtual_asset.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    assert!(virtual_asset.effects.is_empty());
    assert!(!virtual_asset.saving);

    let mut clean = InspectState::default();
    clean.note_detail(&asset("pics/a.png", "png", 1, false, Vec::new()));
    clean.take_effects();
    clean.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    assert!(clean.effects.is_empty());
    assert!(!clean.saving);

    clean.reduce(true, Some("repo"), InspectMessage::SetComment("x".into()));
    clean.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    clean.take_effects();
    clean.reduce(
        true,
        Some("repo"),
        InspectMessage::MetadataSaved(Ok((
            "weird".into(),
            asset("pics/a.png", "png", 2, false, Vec::new()),
        ))),
    );
    assert!(clean.error.contains("无法识别"));
    assert_eq!(clean.draft.comment, "x");
}

#[test]
fn metadata_autosave_waits_260ms_and_flushes_before_the_next_file() {
    let mut state = InspectState::default();
    state.note_detail(&asset("pics/a.png", "png", 1, false, Vec::new()));
    state.take_effects();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetComment("note".into()),
    );
    assert!(state.effects.is_empty());
    assert!(!state.poll_own(259, true, Some("repo")));
    assert!(state.poll_own(1, true, Some("repo")));
    assert!(matches!(
        state.take_effects().as_slice(),
        [InspectEffect::SaveMetadata { .. }]
    ));

    let mut next = InspectState::default();
    next.note_detail(&asset("pics/b.png", "png", 1, false, Vec::new()));
    next.take_effects();
    next.reduce(
        true,
        Some("repo"),
        InspectMessage::SetComment("later".into()),
    );
    next.begin_selection("pics/c.png");
    assert!(matches!(
        next.take_effects().as_slice(),
        [InspectEffect::SaveMetadata { .. }]
    ));
}

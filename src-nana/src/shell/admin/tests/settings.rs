//! 主题圆角与外部连接、仓库后端与音频播放选择、音频卡片，以及日志筛选、跟随与任务弹层的测试。

use super::*;

#[test]
fn theme_corners_and_external_connection() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::SettingsThemeChanged("sepia".into()));
    assert!(model.admin.take_effects().is_empty());
    model.reduce(ShellMessage::SettingsThemeChanged("dark".into()));
    assert_eq!(model.settings.theme, "dark");
    assert!(matches!(
        model.admin.take_effects().as_slice(),
        [AdminEffect::SaveSettings]
    ));

    send(&mut model, AdminMessage::SetCornerStyle("square".into()));
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::SetCornerStyle("round".into()));
    assert_eq!(model.admin.corner_style, "round");
    send(&mut model, AdminMessage::SetCornerRadius("100".into()));
    assert_eq!(model.admin.corner_radius, 20.0);
    send(&mut model, AdminMessage::SetCornerRadius("nope".into()));
    assert_eq!(model.admin.corner_radius, 20.0);
    assert!(
        model
            .admin
            .take_effects()
            .iter()
            .all(|effect| matches!(effect, AdminEffect::PersistCorners))
    );

    let directory = std::env::temp_dir().join(format!("momobako-corners-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("临时目录");
    let path = directory.join("corners.json");
    support::write_corners(&path, "round", 8.0);
    model.admin.load_corners_file(&path);
    assert_eq!(
        (model.admin.corner_style.as_str(), model.admin.corner_radius),
        ("round", 8.0)
    );
    let _ = std::fs::remove_dir_all(&directory);

    assert_eq!(support::external_status_label(None), "未加载");
    assert_eq!(
        support::mask_token(Some("1234567890abcdef")),
        "1234567890...abcdef"
    );
    send(&mut model, AdminMessage::ExportExternal);
    assert_eq!(model.admin.external_error, "连接 JSON 尚未加载。");
    send(
        &mut model,
        AdminMessage::CopyExternal {
            label: "Base URL".into(),
            value: String::new(),
        },
    );
    assert_eq!(model.admin.external_error, "Base URL 尚未加载。");
    model.admin.external = Some(connection());
    send(
        &mut model,
        AdminMessage::CopyExternal {
            label: "Token".into(),
            value: "1234567890abcdef".into(),
        },
    );
    assert_eq!(model.admin.external_message, "Token 已复制。");
    assert!(model.admin.external_error.is_empty());
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::CopyText(text)) if text == "1234567890abcdef")
    );
    send(&mut model, AdminMessage::ExportExternal);
    match model.admin.take_effects().pop() {
        Some(AdminEffect::RequestSaveDialog { content }) => {
            let parsed: serde_json::Value = serde_json::from_str(&content).expect("连接 JSON");
            assert_eq!(parsed["token"], "1234567890abcdef");
            assert!(parsed.get("connectionFilePath").is_none());
        }
        other => panic!("unexpected {other:?}"),
    }
    send(&mut model, AdminMessage::CompleteExport(None));
    assert!(model.admin.take_effects().is_empty());
    send(
        &mut model,
        AdminMessage::CompleteExport(Some("external-api.json".into())),
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::WriteFile { path, .. }) if path == "external-api.json")
    );
    send(&mut model, AdminMessage::WriteFinished(Ok(())));
    assert_eq!(model.admin.external_message, "external-api.json 已导出。");
    send(
        &mut model,
        AdminMessage::WriteFinished(Err("磁盘已满".into())),
    );
    assert_eq!(model.admin.external_error, "导出失败：磁盘已满");
}

#[test]
fn repository_backends_and_audio_player_choices() {
    let mut model = ShellViewModel::default();
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation,
        result: Ok(vec![
            repository("one", "filesystem", "本地"),
            repository("two", "filesystem", "本地"),
            repository("three", "webdav", "WebDAV"),
        ]),
    });
    assert_eq!(
        support::backend_summary(&model.admin.backends),
        "本地 (2) / WebDAV (1)"
    );
    assert_eq!(support::backend_summary(&[]), "无");

    // 原生候选登记了音频能力时：插件名从清单里找，偏好不可用时回退到官方实现；什么都没有才报缺失。
    let mut official = plugin("momobako.player.audio", "builtin", "preview", "preview");
    official.name = "官方音频播放器".into();
    let plugins = [official];
    let mut preferences = BTreeMap::new();
    let view = support::audio_view(
        &[candidate("momobako.player.audio", "音频顺序播放")],
        &plugins,
        &preferences,
    );
    assert_eq!(
        audio_labels(&view),
        ["官方音频播放器 · momobako.player.audio"]
    );
    assert_eq!(view.selected, "momobako.player.audio");
    assert!(view.selectable && view.notice.is_none());
    preferences.insert(AUDIO_CAPABILITY.to_string(), "missing.player".to_string());
    let view = support::audio_view(
        &[candidate("momobako.player.audio", "音频顺序播放")],
        &plugins,
        &preferences,
    );
    assert_eq!(view.choices[0].label, "missing.player（不可用）");
    assert_eq!(
        view.notice,
        Some((
            "所选播放器当前不可用，已回退到 官方音频播放器。".into(),
            false
        ))
    );
    let view = support::audio_view(&[], &[], &BTreeMap::new());
    assert!(view.choices.is_empty() && !view.selectable);
    assert_eq!(
        view.notice,
        Some((
            "官方音频播放器未启用或缺失，音频播放暂不可用。".into(),
            true
        ))
    );

    model.player.candidates = vec![candidate("momobako.player.audio", "音频顺序播放")];
    send(
        &mut model,
        AdminMessage::SetAudioPlayer(Some(" momobako.player.audio ".into())),
    );
    assert_eq!(
        model
            .player
            .preferences
            .get(AUDIO_CAPABILITY)
            .map(String::as_str),
        Some("momobako.player.audio")
    );
}

/// 生产里的原生候选只有内置解码器，音频能力的实现来自插件清单：装了官方音频播放器就显示它；
/// 没装、停用或只有没选的第三方实现时，音频由内置解码器播放，下拉框如实写出它，不报缺失。
#[test]
fn audio_card_reads_plugin_players_and_falls_back_to_the_builtin_decoder() {
    let builtin = ShellViewModel::default().player.candidates;
    let official: PluginManifest = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../External/Plugins/player-audio/manifest.json"
    )))
    .expect("官方音频播放器清单");

    let view = support::audio_view(&builtin, std::slice::from_ref(&official), &BTreeMap::new());
    assert_eq!(
        audio_labels(&view),
        ["Audio Player · momobako.player.audio"]
    );
    assert_eq!(view.selected, "momobako.player.audio");
    assert!(view.selectable && view.notice.is_none());

    // Opus、M4A、AAC 交给媒体基础，只在 Windows 上有；其它平台只有 symphonia 解的 MP3 / FLAC / Ogg。
    let builtin_label = if cfg!(windows) {
        "内置解码器 · WAV / MP3 / FLAC / Ogg / Opus / M4A / AAC"
    } else {
        "内置解码器 · WAV / MP3 / FLAC / Ogg"
    };
    let none = support::audio_view(&builtin, &[], &BTreeMap::new());
    assert_eq!(audio_labels(&none), [builtin_label]);
    assert_eq!(none.selected, "");
    assert!(
        !none.selectable && none.notice.is_none(),
        "内置解码器能放音频，不报缺失"
    );
    let mut disabled = official.clone();
    disabled.enabled = false;
    assert_eq!(
        support::audio_view(&builtin, &[disabled], &BTreeMap::new()),
        none,
        "停用的插件不算实现"
    );

    let preferences = BTreeMap::from([(
        AUDIO_CAPABILITY.to_string(),
        "momobako.player.audio".to_string(),
    )]);
    let gone = support::audio_view(&builtin, &[], &preferences);
    assert_eq!(audio_labels(&gone), ["momobako.player.audio（不可用）"]);
    assert_eq!(
        gone.notice,
        Some(("所选播放器当前不可用，已回退到 内置解码器。".into(), false))
    );

    let mut third = official.clone();
    third.plugin_id = "user.player.lyric".into();
    third.name = "Lyric Player".into();
    let view = support::audio_view(&builtin, &[third], &BTreeMap::new());
    assert_eq!(
        audio_labels(&view),
        [builtin_label, "Lyric Player · user.player.lyric"]
    );
    assert_eq!(view.selected, "", "没选的第三方实现不接管音频");
    assert!(view.selectable && view.notice.is_none());

    let mut model = ShellViewModel::default();
    model.player.preferences = preferences;
    send(
        &mut model,
        AdminMessage::SetAudioPlayer(Some(String::new())),
    );
    assert!(
        model.player.preferences.get(AUDIO_CAPABILITY).is_none(),
        "选内置解码器等于清掉偏好"
    );
}

#[test]
fn logs_filter_sort_pause_and_context() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage {
        records: vec![
            log_record(
                "b",
                "2020-02-01T00:00:00Z",
                "error",
                "helper",
                "plug",
                "repo",
                "失败",
            ),
            log_record(
                "a",
                "2020-01-01T00:00:00Z",
                "info",
                "host",
                "",
                "",
                "开始 needle",
            ),
        ],
        next_cursor: None,
    })));
    assert!(model.admin.log_would_scroll);
    assert_eq!(
        model
            .admin
            .filtered_logs()
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    send(&mut model, AdminMessage::ToggleLogLevel("error".into()));
    assert_eq!(model.admin.filtered_logs().len(), 1);
    assert_eq!(
        support::active_filter_count(&model.admin.log_levels, &model.admin.log_kinds, "", "", ""),
        1
    );
    send(&mut model, AdminMessage::SetLogSearch("needle".into()));
    assert!(model.admin.filtered_logs().is_empty());
    send(&mut model, AdminMessage::ResetLogFilters);
    assert_eq!(
        support::active_filter_count(&model.admin.log_levels, &model.admin.log_kinds, "", "", ""),
        0
    );
    send(&mut model, AdminMessage::SetLogPaused(true));
    assert!(!model.admin.log_would_scroll);
    send(&mut model, AdminMessage::SetLogSearch("不存在".into()));
    assert!(!model.admin.log_would_scroll);
    assert_eq!(model.admin.logs.len(), 2);
    assert_eq!(
        support::unique_sorted(
            model
                .admin
                .logs
                .iter()
                .filter_map(|record| record.source.plugin_id.clone())
        ),
        ["plug"]
    );
    send(&mut model, AdminMessage::ToggleLogContext("a".into()));
    assert!(model.admin.log_context_open.contains("a"));
    send(&mut model, AdminMessage::ToggleLogContext("a".into()));
    assert!(model.admin.log_context_open.is_empty());
    assert_eq!(support::level_label("warn"), "警告");
    assert_eq!(support::source_kind_label("frontend-plugin"), "前端插件");
    assert_eq!(
        support::location_label(&model.admin.logs[0]),
        "app:main.rs:12"
    );

    for index in 0..502 {
        model.admin.merge_log(log_record(
            &format!("n{index:03}"),
            &format!("2021-01-01T00:{:02}:{:02}Z", index / 60, index % 60),
            "info",
            "host",
            "",
            "",
            "x",
        ));
    }
    assert_eq!(model.admin.logs.len(), 500);
    assert_eq!(model.admin.logs[0].id, "n501");
}

#[test]
fn logs_follow_the_end_while_tracking_and_hold_when_paused() {
    let mut model = ShellViewModel::for_page(super::super::super::ShellPage::Logs);
    assert!(model.admin_logs_follow_end());
    send(&mut model, AdminMessage::SetLogPaused(true));
    assert!(!model.admin_logs_follow_end());
    model.admin.merge_log(log_record(
        "late",
        "2026-10-08T08:00:00Z",
        "info",
        "host",
        "",
        "",
        "新记录",
    ));
    assert!(!model.admin_logs_follow_end());
    send(&mut model, AdminMessage::SetLogPaused(false));
    model.admin.merge_log(log_record(
        "later",
        "2026-10-08T08:01:00Z",
        "info",
        "host",
        "",
        "",
        "又一条",
    ));
    assert!(model.admin_logs_follow_end());
    model.workspace.panel = WorkspacePanel::Files;
    assert!(!model.admin_logs_follow_end());
}

#[test]
fn task_popover_merges_repository_operation_and_closes() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::TaskProgressLoaded(vec![
        TaskProgressSnapshot {
            task_id: "task-1".into(),
            protocol_id: "momobako.sync".into(),
            status: "running".into(),
            phase: Some("scanning".into()),
            label: Some("扫描".into()),
            current: None,
            total: None,
            percent: Some(10.0),
            error: None,
            updated_at: "1970-01-01T00:00:00.020Z".into(),
        },
    ]));
    let operation = model
        .operation
        .start(OperationStart::new("导入", "复制", 8.0));
    let rows = model.task_rows();
    assert_eq!(
        (rows[0].id.as_str(), rows[0].source.as_str()),
        ("workspace-operation", "资源库")
    );
    assert_eq!(
        (
            rows[1].id.as_str(),
            rows[1].source.as_str(),
            rows[1].detail.as_str()
        ),
        ("task-1", "任务", "scanning")
    );
    // 面板里的关闭按钮发 `CloseTaskPopover`；框架开合（点外面、Escape、点触发器）发 `TaskPopoverToggled`。
    send(&mut model, AdminMessage::ToggleTaskPopover);
    assert!(model.admin.popover_open);
    send(&mut model, AdminMessage::CloseTaskPopover);
    assert!(!model.admin.popover_open);
    send(&mut model, AdminMessage::TaskPopoverToggled(true));
    assert!(model.admin.popover_open);
    assert!(
        crate::shell::escape_layer(&model).is_none(),
        "任务弹层的 Escape 由 NanaUI 的 Popover 处理，不进全局 Escape"
    );
    send(&mut model, AdminMessage::TaskPopoverToggled(false));
    assert!(!model.admin.popover_open);
    model.operation.cancel(operation);
    model.task_progress.clear();
    assert!(model.task_rows().is_empty());
}

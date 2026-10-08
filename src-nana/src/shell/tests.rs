//! 壳层 ViewModel 状态转换回归测试。

    use super::{ShellMessage, ShellPage, ShellViewModel};
    use crate::backend::services::repository::TaskProgressSnapshot;

    #[test]
    fn shell_messages_reduce_to_user_visible_states() {
        let mut model = ShellViewModel::for_page(ShellPage::Loading);
        model.reduce(ShellMessage::PrimaryAction);
        assert_eq!(model.page, ShellPage::Loading);

        model.reduce(ShellMessage::Navigate(ShellPage::PluginSettings));
        assert_eq!(model.detail, "正在读取页面数据…");

        model = ShellViewModel::for_page(ShellPage::UnsavedEdit);
        model.reduce(ShellMessage::EditAction);
        assert_eq!(model.page, ShellPage::UnsavedEdit);
        assert!(model.dirty);
        model.reduce(ShellMessage::PrimaryAction);
        assert!(model.dirty);
        assert_eq!(model.detail, "当前没有可打开的预览");
    }

    #[test]
    fn navigation_preserves_repository_and_task_context() {
        let mut model = ShellViewModel::default();
        model.repository_id = Some("repo-real".into());
        model.file_entries = vec!["cover.png".into()];
        model.active_task_ids = vec!["task-real".into()];
        model.reduce(ShellMessage::TaskSnapshotLoaded { active: 1, completed: 2 });
        model.reduce(ShellMessage::Navigate(ShellPage::TaskRunning));
        assert_eq!(model.repository_id.as_deref(), Some("repo-real"));
        assert_eq!(model.file_entries, ["cover.png"]);
        assert_eq!(model.active_task_ids, ["task-real"]);
        assert_eq!(model.detail, "1 个运行中任务 · 2 个近期完成任务");
        model.reduce(ShellMessage::Navigate(ShellPage::Playlists));
        assert_eq!(model.repository_id.as_deref(), Some("repo-real"));
    }

    #[test]
    fn playlist_creation_requires_name_and_player_type() {
        let mut model = ShellViewModel::for_page(ShellPage::Playlists);
        model.reduce(ShellMessage::CreatePlaylist);
        assert_eq!(model.detail, "播放列表名称不能为空");
        model.reduce(ShellMessage::NewPlaylistNameChanged("我的列表".into()));
        model.reduce(ShellMessage::CreatePlaylist);
        assert_eq!(model.detail, "请先选择播放器类型");
    }

    #[test]
    fn playlist_reorder_keeps_item_ids_and_labels_aligned() {
        let mut model = ShellViewModel::for_page(ShellPage::Playlists);
        model.selected_playlist_id = Some("playlist-1".into());
        model.playlist_item_ids = vec!["a".into(), "b".into()];
        model.playlist_item_entries = vec!["A".into(), "B".into()];
        model.reduce(ShellMessage::MovePlaylistItem { item_id: "b".into(), direction: -1 });
        assert_eq!(model.playlist_item_ids, ["b", "a"]);
        assert_eq!(model.playlist_item_entries, ["B", "A"]);
        assert!(model.detail.contains("正在保存播放列表顺序"));
    }

    #[test]
    fn task_progress_updates_running_detail_and_retains_terminal_rows() {
        let mut model = ShellViewModel::for_page(ShellPage::TaskRunning);
        model.reduce(ShellMessage::TaskProgressLoaded(vec![
            TaskProgressSnapshot {
                task_id: "task-1".into(),
                protocol_id: "momobako.sync".into(),
                status: "running".into(),
                phase: Some("scanning".into()),
                label: Some("扫描文件".into()),
                current: Some(4),
                total: Some(10),
                percent: Some(40.0),
                error: None,
                updated_at: "now".into(),
            },
            TaskProgressSnapshot {
                task_id: "task-2".into(),
                protocol_id: "momobako.sync".into(),
                status: "cancelled".into(),
                phase: None,
                label: Some("旧任务".into()),
                current: None,
                total: None,
                percent: None,
                error: Some("用户取消".into()),
                updated_at: "now".into(),
            },
        ]));
        assert_eq!(model.task_progress.len(), 2);
        assert!(model.detail.contains("扫描文件"));
        assert!(model.detail.contains("40%"));
    }

    #[test]
    fn settings_changes_are_validated_before_save_feedback() {
        let mut model = ShellViewModel::for_page(ShellPage::Settings);
        model.reduce(ShellMessage::SettingsCacheLimitChanged("32".into()));
        model.reduce(ShellMessage::SaveSettings);
        assert_eq!(model.settings_cache_limit_draft, "32");
        model.reduce(ShellMessage::SettingsSaved(Err("缩略图缓存上限必须在 64–16384 MB 之间".into())));
        assert_eq!(model.page, ShellPage::SettingsError);
        assert!(model.settings_error.is_some());
        model.reduce(ShellMessage::SettingsCacheLimitChanged("512".into()));
        model.reduce(ShellMessage::SettingsSaved(Ok(crate::settings::ApplicationSettings {
            theme: "dark".into(),
            thumbnail_cache_limit_mb: 512,
            default_playlist_player_type_id: None,
            close_behavior: "confirm".into(),
        })));
        assert_eq!(model.page, ShellPage::Settings);
        assert_eq!(model.settings.thumbnail_cache_limit_mb, 512);
        assert_eq!(model.detail, "应用设置已保存");
    }

    fn playlist_detail(repo_id: &str, playlist_id: &str) -> crate::backend::services::repository::PlaylistDetail {
        use crate::backend::services::repository::{PlaylistDetail, PlaylistSummary};
        PlaylistDetail {
            playlist: PlaylistSummary {
                playlist_id: playlist_id.into(),
                repo_id: repo_id.into(),
                name: "早晨".into(),
                player_type_id: "audio".into(),
                player_plugin_id: "audio".into(),
                player_label: "音频".into(),
                file_class: "audio".into(),
                item_count: 0,
                sort_order: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            items: Vec::new(),
        }
    }

    #[test]
    fn stale_playlist_detail_keeps_the_bound_repository_screen() {
        let mut model = ShellViewModel::default();
        model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(playlist_detail("repo-b", "pl"))));
        assert_eq!(model.page, ShellPage::Playlists);
        assert_eq!(model.selected_playlist_id.as_deref(), Some("pl"));

        let mut model = ShellViewModel::default();
        model.page = ShellPage::FileList;
        model.detail = "保持".into();
        model.sidebar.bind_repository(Some("repo-a"), false);
        model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(playlist_detail("repo-b", "pl"))));
        assert_eq!(model.page, ShellPage::FileList);
        assert_eq!(model.detail, "保持");
        assert!(model.selected_playlist_id.is_none());

        model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(playlist_detail("repo-a", "pl"))));
        assert_eq!(model.page, ShellPage::Playlists);
        assert_eq!(model.selected_playlist_id.as_deref(), Some("pl"));
        assert_eq!(model.detail, "早晨 · 0 个项目");
    }

//! 壳层 ViewModel 状态转换回归测试。

    use super::{ShellMessage, ShellPage, ShellViewModel};
    use crate::backend::services::repository::TaskProgressSnapshot;

    #[test]
    fn shell_messages_reduce_to_user_visible_states() {
        let mut model = ShellViewModel::for_page(ShellPage::Loading);
        model.reduce(ShellMessage::PrimaryAction);
        assert_eq!(model.page, ShellPage::Loading);

        // 「未保存」页的脏状态来自注释草稿：编辑动作只报告草稿还在，主按钮重开预览也不丢草稿。
        model = ShellViewModel::for_page(ShellPage::UnsavedEdit);
        model.reduce(ShellMessage::EditAction);
        assert_eq!(model.page, ShellPage::UnsavedEdit);
        assert_eq!(model.detail, "未保存的修改留在当前草稿");
        assert!(model.close_is_dirty());
        model.reduce(ShellMessage::PrimaryAction);
        assert!(model.close_is_dirty());
        assert!(matches!(model.inspect.take_effects().as_slice(), [super::InspectEffect::LoadNative { path, .. }] if path == "notes/page.pdf"));
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

    /// 设置页只有主题会写设置文件。读回和保存结果整份替换设置，缓存上限和关闭行为照旧留给缓存和关窗流程；
    /// 校验失败只写错误，不切页面。
    #[test]
    fn settings_results_keep_the_fields_without_a_settings_card() {
        let mut model = ShellViewModel::for_page(ShellPage::Settings);
        let loaded = crate::settings::ApplicationSettings {
            theme: "dark".into(),
            thumbnail_cache_limit_mb: 512,
            default_playlist_player_type_id: Some("momobako.playlist.audio-sequence".into()),
            close_behavior: "minimizeToTray".into(),
        };
        model.reduce(ShellMessage::SettingsLoaded(Ok((loaded.clone(), None))));
        assert_eq!(model.settings, loaded);
        model.reduce(ShellMessage::SettingsThemeChanged("light".into()));
        model.reduce(ShellMessage::SettingsSaved(Err("缩略图缓存上限必须在 64–16384 MB 之间".into())));
        assert_eq!(model.page, ShellPage::Settings);
        assert!(model.settings_error.is_some());
        let saved = crate::settings::ApplicationSettings { theme: "light".into(), ..loaded };
        model.reduce(ShellMessage::SettingsSaved(Ok(saved.clone())));
        assert_eq!(model.page, ShellPage::Settings);
        assert_eq!(model.settings, saved);
        assert!(model.settings_error.is_none());
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

    /// 播放集详情只写播放集的状态，不改页面身份：主区走哪条路由由面板决定。
    #[test]
    fn stale_playlist_detail_keeps_the_bound_repository_screen() {
        let mut model = ShellViewModel::default();
        model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(playlist_detail("repo-b", "pl"))));
        assert_eq!(model.page, ShellPage::Loading);
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
        assert_eq!(model.page, ShellPage::FileList);
        assert_eq!(model.selected_playlist_id.as_deref(), Some("pl"));
        assert_eq!(model.detail, "早晨 · 0 个项目");
    }

    /// 壳层视图只发一种 `ShellMessage`，`dispatch_program` 同帧同类型只留最后一条，会吞掉操作。
    /// 所有视图一律用 `dispatch_program_all`。
    #[test]
    fn views_never_use_the_coalescing_dispatch() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut pending = vec![root];
        let mut offenders = Vec::new();
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).expect("读源码目录") {
                let path = entry.expect("读目录项").path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let text = std::fs::read_to_string(&path).expect("读源码");
                    let needle = concat!(".dispatch_program", "(");
                    offenders.extend(
                        text.lines()
                            .enumerate()
                            .filter(|(_, line)| line.contains(needle))
                            .map(|(index, _)| format!("{}:{}", path.display(), index + 1)),
                    );
                }
            }
        }
        assert!(offenders.is_empty(), "改用 dispatch_program_all：{offenders:?}");
    }

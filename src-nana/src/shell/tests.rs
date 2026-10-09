//! 壳层 ViewModel 状态转换回归测试。

    use super::{ShellMessage, ShellPage, ShellViewModel};

    /// 「未保存」页的脏状态来自注释草稿，关窗时按脏处理。
    #[test]
    fn unsaved_edit_page_keeps_the_draft_dirty() {
        let model = ShellViewModel::for_page(ShellPage::UnsavedEdit);
        assert_eq!(model.page, ShellPage::UnsavedEdit);
        assert!(model.close_is_dirty());
    }

    /// 和 Vue `playlistDialogDisabled` 一样，名称为空或没有选类型时提交（含回车）不建、对话框留着。
    #[test]
    fn playlist_creation_requires_name_and_player_type() {
        let mut model = ShellViewModel::for_page(ShellPage::Playlists);
        model.playlist_dialog_open = true;
        model.selected_new_playlist_player_type_id = None;
        model.reduce(ShellMessage::CreatePlaylist);
        assert!(model.playlist_dialog_open, "名称为空不提交");
        model.reduce(ShellMessage::NewPlaylistNameChanged("我的列表".into()));
        model.reduce(ShellMessage::CreatePlaylist);
        assert!(model.playlist_dialog_open, "没有播放类型不提交");
        model.reduce(ShellMessage::SelectPlaylistPlayer("momobako.playlist.audio-sequence".into()));
        model.reduce(ShellMessage::CreatePlaylist);
        assert!(!model.playlist_dialog_open);
    }

    /// 设置页只有主题会写设置文件。读回和保存结果整份替换设置，缓存上限和关闭行为照旧留给缓存和关窗流程；
    /// 校验失败进状态区，不切页面。
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
        assert!(model.status.failure().is_some_and(|failure| failure.message.starts_with("保存应用设置失败")));
        let saved = crate::settings::ApplicationSettings { theme: "light".into(), ..loaded };
        model.reduce(ShellMessage::SettingsSaved(Ok(saved.clone())));
        assert_eq!(model.page, ShellPage::Settings);
        assert_eq!(model.settings, saved);
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
        model.sidebar.bind_repository(Some("repo-a"), false);
        model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(playlist_detail("repo-b", "pl"))));
        assert_eq!(model.page, ShellPage::FileList);
        assert!(model.selected_playlist_id.is_none());

        model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(playlist_detail("repo-a", "pl"))));
        assert_eq!(model.page, ShellPage::FileList);
        assert_eq!(model.selected_playlist_id.as_deref(), Some("pl"));
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

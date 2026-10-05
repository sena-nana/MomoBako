//! MomoBako 的 NanaUI 原生宿主骨架。
//!
//! 该 crate 只负责窗口、Runtime 文档和 Nana 控件树；资源库领域服务在
//! `src-tauri` 的宿主无关服务层收口后由应用状态注入。离屏测试复用同一棵
//! `RuntimeDocument`，不创建第二套 UI 树或 GPU 设备。

use nana_ui::runtime::{DocumentId, FrameworkError, Task};
use nana_ui::{
    ApplicationIdentity, ApplicationState, ApplicationWindow, DiagnosticsConfig, NanaApplication,
    GpuTexture, GpuTextureDescriptor, GpuTextureFormat, GpuTextureUsages, HostTexture,
    HostTextureAlphaMode, GpuTextureRegion, RuntimeApplication, RuntimeProgramContext,
    RuntimeProgramUpdate, WindowDescriptor,
};

pub mod shell;
mod files_dispatch;
mod inspect_dispatch;
mod player_dispatch;
mod admin_dispatch;
mod sidebar_dispatch;
use shell::{
    DeleteMode, PreviewPixels, ShellMessage, ShellPage, ShellViewModel, StartupStatus, WindowAction,
    WorkspaceEffect, display_mode_path, mount_shell, sidebar_prefs_path,
};

pub mod capability;
pub mod host_api;
pub mod services;
pub mod theme_map;

/// Nana 宿主直接使用共享领域服务 crate，迁移期 Tauri 仍保留同一服务源码的
/// 适配入口；此 re-export 让后续 ViewModel 接线不需要再穿过 command 层。
pub use momobako_backend as backend;
pub(crate) use shell::prepare_text as prepare_preview_text;
use backend::services::mutsuki_runner::{PROTOCOL_REPOSITORY_RELOCATE, PROTOCOL_REPOSITORY_SYNC};
use backend::services::repository::{
    FileBrowserRequest, RepositoryDeleteMode, RepositoryDeleteRequest, RepositoryRelocateRequest,
    SyncRequest,
};
pub mod plugin_api;
pub mod settings;
mod thumbnail_host;

use thumbnail_host::{PendingThumb, ThumbnailGpu};

/// Nana 宿主应用状态，持有共享领域 Runtime 和原生壳层 ViewModel。
pub struct MomoBakoApplication {
    pub services: Option<services::NativeServices>,
    shell: ShellViewModel,
    repositories_load_scheduled: bool,
    preview_gpu: Option<NativePreviewGpu>,
    pub(crate) thumbnail_gpu: Vec<ThumbnailGpu>,
    pub(crate) pending_thumbs: Vec<PendingThumb>,
}

struct NativePreviewGpu {
    token: String,
    texture: GpuTexture,
    width: u32,
    height: u32,
}

impl Default for MomoBakoApplication {
    fn default() -> Self {
        Self {
            services: None,
            shell: ShellViewModel::default(),
            repositories_load_scheduled: false,
            preview_gpu: None,
            thumbnail_gpu: Vec::new(),
            pending_thumbs: Vec::new(),
        }
    }
}

impl ApplicationState for MomoBakoApplication {
    type Message = ShellMessage;
    type Error = FrameworkError;

    fn initialize(context: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        let services = match services::NativeServices::start() {
            Ok(services) => {
                let context = context.clone();
                services.pump_host_events(move |event| {
                    context.dispatch(ShellMessage::Host(shell::host_events::HostMessage::from_event(event)));
                });
                Some(services)
            }
            Err(error) => {
                eprintln!("Nana 领域 Runtime 启动失败：{error}");
                None
            }
        };
        let mut shell = if services.is_some() {
            ShellViewModel::default()
        } else {
            let mut shell = ShellViewModel::for_page(ShellPage::Error);
            shell.detail = "领域服务启动失败，请检查服务目录和端口配置".into();
            shell
        };
        shell.workspace.load_prefs_file(&sidebar_prefs_path());
        shell.files.load_display_mode_file(&display_mode_path());
        shell.player.load_default_files();
        shell.admin.load_default_file();
        Ok(Self {
            services,
            shell,
            repositories_load_scheduled: false,
            preview_gpu: None,
            thumbnail_gpu: Vec::new(),
            pending_thumbs: Vec::new(),
        })
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        if self.services.is_some() && !self.repositories_load_scheduled {
            self.shell.workspace.prepare_initial_list();
        }
        mount_shell(&mut window.document, &self.shell)?;
        if !self.repositories_load_scheduled {
            if let Some(services) = self.services.as_ref() {
                let query = services.repository_query.clone();
                let executor = services.executor.clone();
                let generation = self.shell.workspace.list_generation;
                if let Err(error) = context.run_task(Task::new(async move {
                    ShellMessage::WorkspaceListLoaded {
                        generation,
                        result: executor.block_on(query.list_repositories()),
                    }
                })) {
                    eprintln!("Nana 资源库加载任务提交失败：{error}");
                }
                self.repositories_load_scheduled = true;
            }
        }
        Ok(())
    }

    fn prepare(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) {
        let Some(token) = self.shell.preview_token.clone() else {
            self.preview_gpu = None;
            window.textures.remove("file-preview");
            return;
        };
        let Some(pixels) = self.shell.preview_pixels.as_ref() else {
            self.preview_gpu = None;
            window.textures.remove("file-preview");
            return;
        };
        let needs_upload = self.preview_gpu.as_ref().is_none_or(|preview| {
            preview.token != token
                || preview.width != pixels.width
                || preview.height != pixels.height
        });
        if needs_upload {
            let Ok(texture) = context.gpu().create_texture(&GpuTextureDescriptor {
                label: Some("momobako file preview"),
                width: pixels.width,
                height: pixels.height,
                format: GpuTextureFormat::RGBA8_UNORM_SRGB,
                usage: GpuTextureUsages::SAMPLED | GpuTextureUsages::COPY_DST,
            }) else {
                eprintln!("Nana 文件预览纹理创建失败：{}x{}", pixels.width, pixels.height);
                return;
            };
            if let Err(error) = context.gpu().write_texture(
                &texture,
                GpuTextureRegion::full(pixels.width, pixels.height),
                &pixels.rgba,
                pixels.width.saturating_mul(4),
            ) {
                eprintln!("Nana 文件预览纹理上传失败：{error}");
                return;
            }
            self.preview_gpu = Some(NativePreviewGpu {
                token,
                texture,
                width: pixels.width,
                height: pixels.height,
            });
        }
        if let Some(preview) = self.preview_gpu.as_ref() {
            window.textures.register(
                "file-preview",
                HostTexture::new(1, 1, &preview.texture),
                preview.width,
                preview.height,
                HostTextureAlphaMode::Premultiplied,
            );
        }
        self.publish_thumbnails(window, context);
    }

    fn update(
        &mut self,
        mut message: ShellMessage,
        windows: &mut std::collections::HashMap<nana_ui_platform::WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        if let ShellMessage::ThumbnailPixels(frames) = &mut message {
            self.queue_thumbnail_frames(frames);
        }
        let Some((id, window)) = windows.iter_mut().next() else {
            return RuntimeProgramUpdate::default();
        };
        if let Some(window_commands) =
            shell::window_action_commands(&message, *id, context.geometry().maximized)
        {
            return RuntimeProgramUpdate {
                window_commands,
                ..RuntimeProgramUpdate::default()
            };
        }
        if matches!(&message, ShellMessage::Navigate(ShellPage::PluginSettings))
            && let Some(services) = self.services.as_ref()
        {
            let plugin = services.plugin.clone();
            let executor = services.executor.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PluginsLoaded(executor.block_on(plugin.list_plugins()))
            })) {
                eprintln!("Nana 插件列表任务提交失败：{error}");
            }
        }
        if matches!(&message, ShellMessage::Navigate(ShellPage::Logs))
            && let Some(services) = self.services.as_ref()
        {
            let system = services.system.clone();
            let executor = services.executor.clone();
            let query = backend::services::repository::SystemLogQuery {
                limit: Some(100),
                ..Default::default()
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::LogsLoaded(executor.block_on(system.list_system_logs(Some(query))))
            })) {
                eprintln!("Nana 系统日志任务提交失败：{error}");
            }
        }
        if matches!(&message, ShellMessage::Navigate(ShellPage::Playlists))
            && let Some(repository_id) = self.shell.repository_id.clone()
            && let Some(services) = self.services.as_ref()
        {
            let interaction = services.repository_interaction.clone();
            let plugin = services.plugin.clone();
            let executor = services.executor.clone();
            let players_executor = executor.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PlaylistsLoaded(executor.block_on(interaction.list_playlists(repository_id)))
            })) {
                eprintln!("Nana 播放列表任务提交失败：{error}");
            }
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PlaylistPlayersLoaded(
                    players_executor.block_on(plugin.list_playlist_players()),
                )
            })) {
                eprintln!("Nana 播放器类型任务提交失败：{error}");
            }
        }
        if matches!(&message, ShellMessage::Navigate(ShellPage::TaskRunning))
            && let Some(services) = self.services.as_ref()
        {
            let (active, completed) = services.tasks.activity_snapshot();
            self.shell.active_task_ids = services.tasks.active_task_ids();
            self.shell.reduce(ShellMessage::TaskSnapshotLoaded { active, completed });
            self.shell
                .reduce(ShellMessage::TaskProgressLoaded(services.tasks.progress_snapshots()));
        }
        if matches!(&message, ShellMessage::Navigate(ShellPage::Settings))
            && let Some(services) = self.services.as_ref()
        {
            schedule_settings_load(services, context);
        }
        if matches!(&message, ShellMessage::SaveSettings)
            && let Some(services) = self.services.as_ref()
        {
            let settings = self.shell.settings.clone();
            let settings_service = services.settings.clone();
            let executor = services.executor.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::SettingsSaved(executor.block_on(async move {
                    settings.validate()?;
                    settings_service.save(&settings).map(|()| settings)
                }))
            })) {
                eprintln!("Nana 应用设置保存任务提交失败：{error}");
            }
        }
        if let ShellMessage::SelectPlugin(plugin_id) = &message
            && let Some(services) = self.services.as_ref()
        {
            let plugin = services.plugin.clone();
            let executor = services.executor.clone();
            let plugin_id = plugin_id.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PluginConfigLoaded(executor.block_on(plugin.get_plugin_config(plugin_id)))
            })) {
                eprintln!("Nana 插件设置任务提交失败：{error}");
            }
        }
        if let ShellMessage::TogglePlugin { plugin_id, enabled } = &message
            && let Some(services) = self.services.as_ref()
        {
            let plugin = services.plugin.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::PluginEnabledRequest {
                plugin_id: plugin_id.clone(),
                enabled: *enabled,
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PluginsLoaded(
                    executor.block_on(plugin.set_plugin_enabled(request)).map(|response| response.plugins),
                )
            })) {
                eprintln!("Nana 插件启停任务提交失败：{error}");
            }
        }
        if let ShellMessage::DeletePlugin(plugin_id) = &message
            && let Some(services) = self.services.as_ref()
        {
            let plugin = services.plugin.clone();
            let executor = services.executor.clone();
            let plugin_id = plugin_id.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PluginsLoaded(
                    executor.block_on(plugin.delete_plugin(plugin_id)).map(|response| response.plugins),
                )
            })) {
                eprintln!("Nana 插件删除任务提交失败：{error}");
            }
        }
        if let ShellMessage::SelectPlaylist(playlist_id) = &message
            && let Some(repository_id) = self.shell.repository_id.clone()
            && let Some(services) = self.services.as_ref()
        {
            let interaction = services.repository_interaction.clone();
            let executor = services.executor.clone();
            let playlist_id = playlist_id.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PlaylistDetailLoaded(executor.block_on(
                    interaction.get_playlist_detail(repository_id, playlist_id),
                ))
            })) {
                eprintln!("Nana 播放列表详情任务提交失败：{error}");
            }
        }
        if let ShellMessage::DeletePlaylist(playlist_id) = &message
            && let Some(repository_id) = self.shell.repository_id.clone()
            && let Some(services) = self.services.as_ref()
        {
            let interaction = services.repository_interaction.clone();
            let executor = services.executor.clone();
            let playlist_id = playlist_id.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                let result = executor.block_on(async {
                    interaction
                        .delete_playlist(repository_id.clone(), playlist_id)
                        .await?;
                    interaction.list_playlists(repository_id).await
                });
                ShellMessage::PlaylistsLoaded(result)
            })) {
                eprintln!("Nana 播放列表删除任务提交失败：{error}");
            }
        }
        if let ShellMessage::RemovePlaylistItem { playlist_id, item_id } = &message
            && let Some(repository_id) = self.shell.repository_id.clone()
            && let Some(services) = self.services.as_ref()
        {
            let interaction = services.repository_interaction.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::PlaylistItemRemoveRequest {
                repo_id: repository_id,
                playlist_id: playlist_id.clone(),
                playlist_item_id: item_id.clone(),
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PlaylistDetailLoaded(
                    executor.block_on(interaction.remove_playlist_item(request)),
                )
            })) {
                eprintln!("Nana 播放列表项目移除任务提交失败：{error}");
            }
        }
        if let ShellMessage::ReorderPlaylistItems { playlist_id, item_ids } = &message
            && let Some(repository_id) = self.shell.repository_id.clone()
            && let Some(services) = self.services.as_ref()
        {
            let interaction = services.repository_interaction.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::PlaylistItemsOrderRequest {
                repo_id: repository_id,
                playlist_id: playlist_id.clone(),
                item_ids: item_ids.clone(),
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PlaylistDetailLoaded(
                    executor.block_on(interaction.reorder_playlist_items(request)),
                )
            })) {
                eprintln!("Nana 播放列表排序任务提交失败：{error}");
            }
        }
        if let ShellMessage::MovePlaylistItem { item_id, direction } = &message
            && let Some(playlist_id) = self.shell.selected_playlist_id.clone()
        {
            let mut item_ids = self.shell.playlist_item_ids.clone();
            if let Some(index) = item_ids.iter().position(|id| id == item_id) {
                let target = if *direction < 0 { index.checked_sub(1) } else { (index + 1 < item_ids.len()).then_some(index + 1) };
                if let Some(target) = target {
                    item_ids.swap(index, target);
                    self.shell.reduce(ShellMessage::ReorderPlaylistItems { playlist_id: playlist_id.clone(), item_ids: item_ids.clone() });
                    let message = ShellMessage::ReorderPlaylistItems { playlist_id, item_ids };
                    // Continue through the normal service dispatch below.
                    return self.update(message, windows, context);
                }
            }
        }
        if let ShellMessage::AddPlaylistItemsByPaths { playlist_id, paths } = &message
            && let Some(repository_id) = self.shell.repository_id.clone()
            && let Some(services) = self.services.as_ref()
        {
            let interaction = services.repository_interaction.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::PlaylistItemsByPathsAddRequest {
                repo_id: repository_id,
                playlist_id: playlist_id.clone(),
                paths: paths.clone(),
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PlaylistDetailLoaded(
                    executor.block_on(interaction.add_playlist_items_by_paths(request)),
                )
            })) {
                eprintln!("Nana 播放列表添加任务提交失败：{error}");
            }
        }
        if matches!(&message, ShellMessage::SavePlaylistName)
            && let (Some(repository_id), Some(playlist_id), Some(player_type_id), Some(services)) = (
                self.shell.repository_id.clone(),
                self.shell.selected_playlist_id.clone(),
                self.shell.selected_playlist_player_type_id.clone(),
                self.services.as_ref(),
            )
        {
            let interaction = services.repository_interaction.clone();
            let executor = services.executor.clone();
            let name = self.shell.playlist_name_draft.trim().to_string();
            let request = backend::services::repository::PlaylistUpdateRequest {
                repo_id: repository_id,
                playlist_id,
                name: Some(name),
                player_type_id: Some(player_type_id),
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PlaylistsLoaded(
                    executor
                        .block_on(interaction.update_playlist(request))
                        .map(|response| response.playlists),
                )
            })) {
                eprintln!("Nana 播放列表名称保存任务提交失败：{error}");
            }
        }
        if matches!(&message, ShellMessage::CreatePlaylist)
            && let Some(repository_id) = self.shell.repository_id.clone()
            && let Some(player_type_id) = self.shell.selected_new_playlist_player_type_id.clone()
            && !self.shell.new_playlist_name.trim().is_empty()
            && let Some(services) = self.services.as_ref()
        {
            let interaction = services.repository_interaction.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::PlaylistMutationRequest {
                repo_id: repository_id,
                playlist_id: None,
                name: self.shell.new_playlist_name.trim().to_string(),
                player_type_id,
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PlaylistsLoaded(
                    executor
                        .block_on(interaction.create_playlist(request))
                        .map(|response| response.playlists),
                )
            })) {
                eprintln!("Nana 播放列表创建任务提交失败：{error}");
            }
        }
        if matches!(&message, ShellMessage::ClearLogs)
            && let Some(services) = self.services.as_ref()
        {
            let system = services.system.clone();
            let executor = services.executor.clone();
            let query = backend::services::repository::SystemLogQuery {
                limit: Some(100),
                ..Default::default()
            };
            if let Err(error) = context.run_task(Task::new(async move {
                let result = executor.block_on(async {
                    system.clear_system_logs().await?;
                    system.list_system_logs(Some(query)).await
                });
                ShellMessage::LogsLoaded(result)
            })) {
                eprintln!("Nana 系统日志清理任务提交失败：{error}");
            }
        }
        if let ShellMessage::DeletePluginConfig { plugin_id, key } = &message
            && let Some(services) = self.services.as_ref()
        {
            let plugin = services.plugin.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::PluginConfigDeleteRequest {
                plugin_id: plugin_id.clone(),
                key: key.clone(),
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PluginConfigLoaded(
                    executor.block_on(plugin.delete_plugin_config_value(request)),
                )
            })) {
                eprintln!("Nana 插件设置删除任务提交失败：{error}");
            }
        }
        if let ShellMessage::SavePluginConfig { plugin_id, key } = &message
            && let Some(services) = self.services.as_ref()
        {
            let plugin = services.plugin.clone();
            let executor = services.executor.clone();
            let value = self
                .shell
                .plugin_config_drafts
                .get(key)
                .cloned()
                .unwrap_or_default();
            let value = if self.shell.plugin_config_string_values.contains(key) {
                serde_json::Value::String(value)
            } else {
                match serde_json::from_str(&value) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("Nana 插件配置 JSON 解析失败 {key}：{error}");
                        self.shell.detail = format!("配置 {key} 不是有效 JSON：{error}");
                        return RuntimeProgramUpdate::redraw(*id);
                    }
                }
            };
            let request = backend::services::repository::PluginConfigSetRequest {
                plugin_id: plugin_id.clone(),
                key: key.clone(),
                value,
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PluginConfigLoaded(
                    executor.block_on(plugin.set_plugin_config_value(request)),
                )
            })) {
                eprintln!("Nana 插件设置保存任务提交失败：{error}");
            }
        }
        if let ShellMessage::CancelTask(task_id) = &message
            && let Some(services) = self.services.as_ref()
        {
            if !services.tasks.cancel(task_id) {
                eprintln!("Nana 任务取消请求未找到任务：{task_id}");
            }
        }
        let snapshot_repo_id = if let ShellMessage::RepositorySnapshotLoaded(Ok(snapshot)) = &message {
            Some(snapshot.repository.repo_id.clone())
        } else {
            None
        };
        if let ShellMessage::OpenDirectory(path) = &message
            && let Some(repo_id) = self.shell.repository_id.clone()
            && let Some(services) = self.services.as_ref()
        {
            let browser = services.file_browser.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::FileBrowserRequest {
                repo_id, directory_path: Some(path.clone()), include_tree: Some(false),
                special_location: None, offset: Some(0), limit: Some(200),
            };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::FileBrowserLoaded(executor.block_on(browser.get_file_browser(request)))
            })) {
                eprintln!("Nana 目录加载任务提交失败：{error}");
            }
        }
        if let ShellMessage::SelectFile {
            asset_id: Some(asset_id),
            ..
        } = &message
            && let Some(repository_id) = self.shell.repository_id.clone()
            && let Some(services) = self.services.as_ref()
        {
            let query = services.repository_query.clone();
            let executor = services.executor.clone();
            let asset_id = asset_id.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::AssetDetailLoaded(
                    executor.block_on(query.get_asset_detail(repository_id, asset_id)),
                )
            })) {
                eprintln!("Nana 文件元数据任务提交失败：{error}");
            }
        }
        if matches!(&message, ShellMessage::PrimaryAction)
            && self.shell.acceptance_scene
            && self.shell.page == ShellPage::SelectedFile
            && let (Some(repository_id), Some(path), Some(services)) = (
                self.shell.repository_id.clone(),
                self.shell.selected_path.clone(),
                self.services.as_ref(),
            )
        {
            let query = services.repository_query.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::FileReadRequest { repo_id: repository_id, path };
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::PreviewSourceLoaded(
                    executor.block_on(query.prepare_preview_file_source(request)),
                )
            })) {
                eprintln!("Nana 预览源任务提交失败：{error}");
            }
        }
        if let ShellMessage::PreviewSourceLoaded(Ok(source)) = &message
            && let Some(services) = self.services.as_ref()
        {
            let query = services.repository_query.clone();
            let executor = services.executor.clone();
            let source = backend::services::repository::FilePreviewSourceResponse {
                repo_id: source.repo_id.clone(),
                path: source.path.clone(),
                token: source.token.clone(),
                source_url: source.source_url.clone(),
                local_path: source.local_path.clone(),
                media_type: source.media_type.clone(),
                size_bytes: source.size_bytes,
                modified_at: source.modified_at.clone(),
            };
            let request = backend::services::repository::FileReadRequest {
                repo_id: source.repo_id.clone(),
                path: source.path.clone(),
            };
            if let Err(error) = context.run_task(Task::new(async move {
                let pixels = executor.block_on(query.read_file(request)).and_then(|bytes| {
                    if !source.media_type.starts_with("image/") {
                        return Err(format!("原生纹理预览暂不支持 {}", source.media_type));
                    }
                    decode_preview_pixels(&bytes)
                });
                ShellMessage::PreviewPixelsLoaded { source, pixels }
            })) {
                eprintln!("Nana 原生图片预览任务提交失败：{error}");
            }
        }
        self.shell.reduce(message);
        if let Some(repo_id) = snapshot_repo_id
            && self.shell.repository_id.as_deref() == Some(repo_id.as_str())
            && self.shell.workspace.startup.status != StartupStatus::Error
            && (self.shell.workspace.startup.status != StartupStatus::Loading
                || self.shell.workspace.startup.current_step >= 4)
            && let Some(services) = self.services.as_ref()
        {
            schedule_root_browser(services, context, repo_id);
        }
        dispatch_workspace_effects(self, context);
        player_dispatch::dispatch_player_effects(self, context);
        admin_dispatch::dispatch_admin_effects(self, context);
        sidebar_dispatch::dispatch_sidebar_effects(self, context);
        files_dispatch::dispatch_files_effects(self, context);
        inspect_dispatch::dispatch_inspect_effects(self, context);
        let maximized = context.geometry().maximized;
        let window_commands = self.shell.input.take_platform_commands(*id, maximized);
        if let Err(error) = mount_shell(&mut window.document, &self.shell) {
            eprintln!("Nana 壳层重建失败：{error}");
            self.shell.page = ShellPage::Error;
            self.shell.detail = "页面更新失败，请查看系统日志".into();
            return RuntimeProgramUpdate { window_commands, ..RuntimeProgramUpdate::redraw(*id) };
        }
        RuntimeProgramUpdate { window_commands, ..RuntimeProgramUpdate::redraw(*id) }
    }

    fn window_event(
        &mut self,
        event: &nana_ui_platform::WindowEvent,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        match event {
            nana_ui_platform::WindowEvent::CloseRequested { id } => answer_close(self, *id, context),
            nana_ui_platform::WindowEvent::FileDialogCompleted { id, result } => {
                let failed = result.error.as_ref().map(|error| format!("{error:?}"));
                let paths = result.paths.iter().map(|path| path.display().to_string()).collect();
                context.dispatch(ShellMessage::Input(shell::input::InputMessage::FileDialogCompleted {
                    request_id: result.id,
                    paths,
                    failed,
                }));
                RuntimeProgramUpdate::redraw(*id)
            }
            nana_ui_platform::WindowEvent::FileDialogRejected { id, request_id, error } => {
                eprintln!("Nana 文件对话框被拒绝：{error:?}");
                context.dispatch(ShellMessage::Input(shell::input::InputMessage::FileDialogCompleted {
                    request_id: *request_id,
                    paths: Vec::new(),
                    failed: Some(format!("{error:?}")),
                }));
                RuntimeProgramUpdate::redraw(*id)
            }
            _ => RuntimeProgramUpdate::default(),
        }
    }
}

/// 系统关闭请求按关闭设置回答。确认和托盘只重绘，不立刻关闭。
fn answer_close(app: &mut MomoBakoApplication, id: nana_ui_platform::WindowId, context: &RuntimeProgramContext<ShellMessage>) -> RuntimeProgramUpdate {
    let decision = shell::input::decide_close(app.shell.settings.close_behavior.as_str(), app.shell.close_is_dirty());
    match decision {
        shell::input::CloseDecision::CloseNow => RuntimeProgramUpdate {
            window_commands: host_api::WindowCommand::Close
                .to_platform_command(id, context.geometry().maximized)
                .into_iter()
                .collect(),
            ..RuntimeProgramUpdate::redraw(id)
        },
        shell::input::CloseDecision::Ask { .. } | shell::input::CloseDecision::HoldForTray => {
            context.dispatch(ShellMessage::WindowAction(WindowAction::Close));
            RuntimeProgramUpdate::redraw(id)
        }
    }
}

/// 读取系统连接状态和应用设置。设置页和来源缓存问题共用这一次调度。
fn schedule_settings_load(
    services: &services::NativeServices,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    let system = services.system.clone();
    let settings = services.settings.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::SystemStatusLoaded(
            executor.block_on(system.get_external_api_connection_status()),
        )
    })) {
        eprintln!("Nana 系统状态任务提交失败：{error}");
    }
    let settings_executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::SettingsLoaded(
            settings_executor.block_on(async move { settings.load_or_recover() }),
        )
    })) {
        eprintln!("Nana 应用设置加载任务提交失败：{error}");
    }
}

/// 摘要确认属于当前仓库后，读取根目录作为启动第 4 步。
fn schedule_root_browser(
    services: &services::NativeServices,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
) {
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
    let request = FileBrowserRequest {
        repo_id,
        directory_path: None,
        include_tree: Some(false),
        special_location: None,
        offset: Some(0),
        limit: Some(200),
    };
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::FileBrowserLoaded(executor.block_on(browser.get_file_browser(request)))
    })) {
        eprintln!("Nana 文件浏览任务提交失败：{error}");
    }
}

/// 执行工作台归约留下的副作用。服务未启动或任务提交失败时把错误写回状态机，避免步骤停在进行中。
fn dispatch_workspace_effects(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    for effect in app.shell.workspace.take_effects() {
        match effect {
            WorkspaceEffect::PersistSidebar => {
                app.shell.workspace.save_prefs_file(&sidebar_prefs_path());
            }
            WorkspaceEffect::StopPlayback { previous_repo_id } => {
                eprintln!("Nana 仓库切换，停止播放会话：{previous_repo_id}");
                app.shell.reduce(ShellMessage::Player(crate::shell::player::PlayerMessage::Stop {
                    repo_id: Some(previous_repo_id),
                    clear_stored: true,
                }));
            }
            WorkspaceEffect::RefreshRepositories { generation } => {
                let prepared = app.services.as_ref().map(|services| {
                    (services.repository_query.clone(), services.executor.clone())
                });
                let Some((query, executor)) = prepared else {
                    eprintln!("Nana 资源库刷新需要领域服务，当前服务未启动");
                    app.shell.reduce(ShellMessage::WorkspaceListLoaded {
                        generation,
                        result: Err("领域服务未启动".into()),
                    });
                    continue;
                };
                if let Err(error) = context.run_task(Task::new(async move {
                    ShellMessage::WorkspaceListLoaded {
                        generation,
                        result: executor.block_on(query.list_repositories()),
                    }
                })) {
                    eprintln!("Nana 资源库刷新任务提交失败：{error}");
                    app.shell.reduce(ShellMessage::WorkspaceListLoaded {
                        generation,
                        result: Err(format!("资源库列表任务提交失败：{error}")),
                    });
                }
            }
            WorkspaceEffect::SyncRepository { repo_id, generation } => {
                let prepared = app
                    .services
                    .as_ref()
                    .map(|services| (services.tasks.clone(), services.executor.clone()));
                let Some((tasks, executor)) = prepared else {
                    eprintln!("Nana 资源库同步需要领域服务，当前服务未启动");
                    app.shell.reduce(ShellMessage::StartupSyncFinished {
                        generation,
                        result: Err("领域服务未启动".into()),
                    });
                    continue;
                };
                if let Err(error) = context.run_task(Task::new(async move {
                    let result = executor
                        .block_on(tasks.execute(PROTOCOL_REPOSITORY_SYNC, SyncRequest { repo_id }))
                        .map(|_| ());
                    ShellMessage::StartupSyncFinished { generation, result }
                })) {
                    eprintln!("Nana 资源库同步任务提交失败：{error}");
                    app.shell.reduce(ShellMessage::StartupSyncFinished {
                        generation,
                        result: Err(format!("资源库同步任务提交失败：{error}")),
                    });
                }
            }
            WorkspaceEffect::LoadSnapshot { repo_id } => {
                let prepared = app.services.as_ref().map(|services| {
                    (services.repository_query.clone(), services.executor.clone())
                });
                let Some((query, executor)) = prepared else {
                    eprintln!("Nana 资源库摘要需要领域服务，当前服务未启动");
                    app.shell.reduce(ShellMessage::RepositorySnapshotLoaded(Err(
                        "领域服务未启动".into(),
                    )));
                    continue;
                };
                if let Err(error) = context.run_task(Task::new(async move {
                    ShellMessage::RepositorySnapshotLoaded(
                        executor.block_on(query.get_repository_snapshot(repo_id)),
                    )
                })) {
                    eprintln!("Nana 资源库摘要任务提交失败：{error}");
                    app.shell.reduce(ShellMessage::RepositorySnapshotLoaded(Err(format!(
                        "资源库摘要任务提交失败：{error}"
                    ))));
                }
            }
            WorkspaceEffect::RelocateRepository { repo_id, path } => {
                let prepared = app
                    .services
                    .as_ref()
                    .map(|services| (services.tasks.clone(), services.executor.clone()));
                let Some((tasks, executor)) = prepared else {
                    eprintln!("Nana 资源库重定向需要领域服务，当前服务未启动");
                    app.shell
                        .reduce(ShellMessage::MissingRelocateFinished(Err("领域服务未启动".into())));
                    continue;
                };
                if let Err(error) = context.run_task(Task::new(async move {
                    let result = executor
                        .block_on(tasks.execute(
                            PROTOCOL_REPOSITORY_RELOCATE,
                            RepositoryRelocateRequest { repo_id, path },
                        ))
                        .map(|_| ());
                    ShellMessage::MissingRelocateFinished(result)
                })) {
                    eprintln!("Nana 资源库重定向任务提交失败：{error}");
                    app.shell.reduce(ShellMessage::MissingRelocateFinished(Err(format!(
                        "资源库重定向任务提交失败：{error}"
                    ))));
                }
            }
            WorkspaceEffect::DeleteRepository { repo_id, mode } => {
                let prepared = app.services.as_ref().map(|services| {
                    (
                        services.repository_management.clone(),
                        services.executor.clone(),
                    )
                });
                let mode = match mode {
                    DeleteMode::RecordOnly => RepositoryDeleteMode::RecordOnly,
                    DeleteMode::DeleteMetadata => RepositoryDeleteMode::DeleteMetadata,
                    DeleteMode::DeleteFolder => RepositoryDeleteMode::DeleteFolder,
                };
                let Some((management, executor)) = prepared else {
                    eprintln!("Nana 资源库删除需要领域服务，当前服务未启动");
                    app.shell
                        .reduce(ShellMessage::MissingDeleteFinished(Err("领域服务未启动".into())));
                    continue;
                };
                if let Err(error) = context.run_task(Task::new(async move {
                    ShellMessage::MissingDeleteFinished(
                        executor.block_on(
                            management.delete_repository(RepositoryDeleteRequest { repo_id, mode }),
                        ),
                    )
                })) {
                    eprintln!("Nana 资源库删除任务提交失败：{error}");
                    app.shell.reduce(ShellMessage::MissingDeleteFinished(Err(format!(
                        "资源库删除任务提交失败：{error}"
                    ))));
                }
            }
            WorkspaceEffect::OpenSourceSettings => {
                let Some(services) = app.services.as_ref() else {
                    eprintln!("Nana 来源设置需要领域服务，当前服务未启动");
                    app.shell
                        .reduce(ShellMessage::SystemStatusLoaded(Err("领域服务未启动".into())));
                    continue;
                };
                schedule_settings_load(services, context);
            }
            WorkspaceEffect::RefreshRepositoriesSilent => {
                shell::workspace_refresh::dispatch_silent_list(app, context);
            }
            WorkspaceEffect::LoadSnapshotSilent { repo_id } => {
                shell::workspace_refresh::dispatch_silent_snapshot(app, context, repo_id);
            }
        }
    }
}

pub(crate) fn decode_preview_pixels(bytes: &[u8]) -> Result<PreviewPixels, String> {
    let image = image::load_from_memory(bytes).map_err(|error| format!("图片解码失败：{error}"))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(format!("图片尺寸不受支持：{width}x{height}"));
    }
    Ok(PreviewPixels {
        width,
        height,
        rgba: rgba.into_raw(),
    })
}

/// 启动原生 NanaUI 窗口。
pub fn run() -> Result<(), nana_ui::HostedRunError> {
    let identity = ApplicationIdentity::new(
        "com.momobako.desktop",
        "MomoBako",
        env!("CARGO_PKG_VERSION"),
    );
    NanaApplication::builder(identity)
        .diagnostics(DiagnosticsConfig::default())
        .run::<RuntimeApplication<MomoBakoApplication>>(
            WindowDescriptor::new("MomoBako").initial_size(1200.0, 800.0),
        )
}

/// 为离屏验收创建与生产宿主相同的 Runtime 文档。
pub fn acceptance_document() -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    acceptance_document_for(ShellPage::Loading)
}

/// 为离屏验收创建指定页面状态的生产 Runtime 文档。
pub fn acceptance_document_for(
    page: ShellPage,
) -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    acceptance_document_for_model(ShellViewModel::for_page(page))
}

/// 为离屏验收挂载指定 ViewModel；仍然复用生产壳层挂载函数和同一棵 Runtime 树。
pub fn acceptance_document_for_model(
    model: ShellViewModel,
) -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    let document_id = DocumentId::new(1).expect("document id 1 is valid");
    let mut document = nana_ui::runtime::RuntimeDocument::new(document_id);
    mount_shell(&mut document, &model)?;
    Ok(document)
}

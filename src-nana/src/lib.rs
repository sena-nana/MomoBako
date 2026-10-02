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
use shell::{PreviewPixels, ShellMessage, ShellPage, ShellViewModel, WindowAction, mount_shell};

pub mod host_api;
pub mod services;

/// Nana 宿主直接使用共享领域服务 crate，迁移期 Tauri 仍保留同一服务源码的
/// 适配入口；此 re-export 让后续 ViewModel 接线不需要再穿过 command 层。
pub use momobako_backend as backend;
pub mod plugin_api;
pub mod settings;

/// Nana 宿主应用状态，持有共享领域 Runtime 和原生壳层 ViewModel。
pub struct MomoBakoApplication {
    pub services: Option<services::NativeServices>,
    shell: ShellViewModel,
    repositories_load_scheduled: bool,
    preview_gpu: Option<NativePreviewGpu>,
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
        }
    }
}

impl ApplicationState for MomoBakoApplication {
    type Message = ShellMessage;
    type Error = FrameworkError;

    fn initialize(_: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        let services = match services::NativeServices::start() {
            Ok(services) => Some(services),
            Err(error) => {
                eprintln!("Nana 领域 Runtime 启动失败：{error}");
                None
            }
        };
        let shell = if services.is_some() {
            ShellViewModel::default()
        } else {
            let mut shell = ShellViewModel::for_page(ShellPage::Error);
            shell.detail = "领域服务启动失败，请检查服务目录和端口配置".into();
            shell
        };
        Ok(Self {
            services,
            shell,
            repositories_load_scheduled: false,
            preview_gpu: None,
        })
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        mount_shell(&mut window.document, &self.shell)?;
        if !self.repositories_load_scheduled {
            if let Some(services) = self.services.as_ref() {
                let query = services.repository_query.clone();
                let executor = services.executor.clone();
                if let Err(error) = context.run_task(Task::new(async move {
                    ShellMessage::RepositoriesLoaded(executor.block_on(query.list_repositories()))
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
    }

    fn update(
        &mut self,
        message: ShellMessage,
        windows: &mut std::collections::HashMap<nana_ui_platform::WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        let Some((id, window)) = windows.iter_mut().next() else {
            return RuntimeProgramUpdate::default();
        };
        if let ShellMessage::WindowAction(action) = &message {
            let request = match action {
                WindowAction::Minimize => host_api::WindowCommand::Minimize,
                WindowAction::ToggleMaximize => host_api::WindowCommand::ToggleMaximize,
                WindowAction::Close => host_api::WindowCommand::Close,
            };
            return RuntimeProgramUpdate {
                window_commands: request
                    .to_platform_command(*id, context.geometry().maximized)
                    .into_iter()
                    .collect(),
                ..RuntimeProgramUpdate::default()
            };
        }
        if let ShellMessage::RepositoriesLoaded(Ok(repositories)) = &message
            && let Some(repository) = repositories.first()
            && let Some(services) = self.services.as_ref()
        {
            let query = services.repository_query.clone();
            let executor = services.executor.clone();
            let repo_id = repository.repo_id.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::RepositorySnapshotLoaded(
                    executor.block_on(query.get_repository_snapshot(repo_id)),
                )
            })) {
                eprintln!("Nana 资源库快照任务提交失败：{error}");
            }
        }
        if matches!(&message, ShellMessage::Refresh)
            && let Some(services) = self.services.as_ref()
        {
            let query = services.repository_query.clone();
            let executor = services.executor.clone();
            if let Err(error) = context.run_task(Task::new(async move {
                ShellMessage::RepositoriesLoaded(executor.block_on(query.list_repositories()))
            })) {
                eprintln!("Nana 资源库刷新任务提交失败：{error}");
            }
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
                ShellMessage::SettingsLoaded(settings_executor.block_on(async move {
                    settings.load_or_recover()
                }))
            })) {
                eprintln!("Nana 应用设置加载任务提交失败：{error}");
            }
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
        if let ShellMessage::RepositorySnapshotLoaded(Ok(snapshot)) = &message
            && let Some(services) = self.services.as_ref()
        {
            let browser = services.file_browser.clone();
            let executor = services.executor.clone();
            let request = backend::services::repository::FileBrowserRequest {
                repo_id: snapshot.repository.repo_id.clone(),
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
        if let Err(error) = mount_shell(&mut window.document, &self.shell) {
            eprintln!("Nana 壳层重建失败：{error}");
            self.shell.page = ShellPage::Error;
            self.shell.detail = "页面更新失败，请查看系统日志".into();
            return RuntimeProgramUpdate::redraw(*id);
        }
        RuntimeProgramUpdate::redraw(*id)
    }
}

fn decode_preview_pixels(bytes: &[u8]) -> Result<PreviewPixels, String> {
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
    let document_id = DocumentId::new(1).expect("document id 1 is valid");
    let mut document = nana_ui::runtime::RuntimeDocument::new(document_id);
    mount_shell(&mut document, &ShellViewModel::for_page(page))?;
    Ok(document)
}

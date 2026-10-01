//! MomoBako 原生应用壳层和页面 ViewModel。
//!
//! 壳层只描述稳定的导航、状态和主内容层级；仓库、插件和任务服务通过
//! `ShellViewModel` 注入文本状态，避免把领域服务直接耦合到 Nana 控件树。

use nana_ui::runtime::view::{button, text, widget};
use nana_ui::runtime::{Activate, FrameworkError, LengthSpec, List, RuntimeDocument, Stack};
use crate::backend::services::repository::{
    AssetDetail, FileBrowserEntry, FileBrowserSnapshot, FilePreviewSourceResponse, RepositorySnapshot,
    PluginManifest, PlaylistSummary, RepositorySummary, SystemLogPage,
    PluginConfigSnapshot,
};

/// Nana Runtime 传递给应用状态的壳层交互消息。
pub enum ShellMessage {
    Navigate(ShellPage),
    Refresh,
    PrimaryAction,
    EditAction,
    RepositoriesLoaded(Result<Vec<RepositorySummary>, String>),
    RepositorySnapshotLoaded(Result<RepositorySnapshot, String>),
    FileBrowserLoaded(Result<FileBrowserSnapshot, String>),
    SelectFile { path: String, asset_id: Option<String> },
    OpenDirectory(String),
    AssetDetailLoaded(Result<AssetDetail, String>),
    PreviewSourceLoaded(Result<FilePreviewSourceResponse, String>),
    PluginsLoaded(Result<Vec<PluginManifest>, String>),
    SelectPlugin(String),
    PluginConfigLoaded(Result<PluginConfigSnapshot, String>),
    DeletePluginConfig { plugin_id: String, key: String },
    LogsLoaded(Result<SystemLogPage, String>),
    PlaylistsLoaded(Result<Vec<PlaylistSummary>, String>),
    SystemStatusLoaded(Result<crate::backend::services::runtime::ExternalApiConnectionStatus, String>),
    TaskSnapshotLoaded { active: usize, completed: usize },
    CancelTask(String),
    WindowAction(WindowAction),
}

/// 宿主无关的窗口生命周期动作。
#[derive(Clone, Debug)]
pub enum WindowAction {
    Minimize,
    ToggleMaximize,
    Close,
}

/// 主内容页面的可观察状态。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ShellPage {
    /// 服务尚未返回仓库结构。
    #[default]
    Loading,
    /// 当前仓库没有可展示文件。
    EmptyRepository,
    /// 仓库服务返回可向用户解释的错误。
    Error,
    /// 展示文件列表。
    FileList,
    /// 展示当前选中的文件及预览入口。
    SelectedFile,
    /// 当前仓库的播放列表。
    Playlists,
    /// 插件原生设置贡献页。
    PluginSettings,
    /// 任务中心有正在运行的任务。
    TaskRunning,
    /// 当前资源存在同步冲突。
    Conflict,
    /// 编辑器存在尚未保存的内容。
    UnsavedEdit,
    /// 应用设置页。
    Settings,
    /// 系统日志页。
    Logs,
}

impl ShellPage {
    fn title(&self) -> &'static str {
        match self {
            Self::Loading => "正在加载资源库",
            Self::EmptyRepository => "资源库为空",
            Self::Error => "资源库加载失败",
            Self::FileList => "文件列表",
            Self::SelectedFile => "文件预览",
            Self::Playlists => "播放列表",
            Self::PluginSettings => "插件设置",
            Self::TaskRunning => "任务进行中",
            Self::Conflict => "同步冲突",
            Self::UnsavedEdit => "编辑未保存",
            Self::Settings => "应用设置",
            Self::Logs => "系统日志",
        }
    }

    fn status(&self) -> &'static str {
        match self {
            Self::Loading => "正在读取仓库结构…",
            Self::EmptyRepository => "还没有可显示的文件",
            Self::Error => "需要处理仓库错误",
            Self::FileList => "已加载仓库文件",
            Self::SelectedFile => "已选中一个文件",
            Self::Playlists => "正在读取仓库播放列表",
            Self::PluginSettings => "正在编辑官方插件的原生设置",
            Self::TaskRunning => "任务活动",
            Self::Conflict => "本地与远端 Revision 不一致",
            Self::UnsavedEdit => "编辑内容尚未写入仓库",
            Self::Settings => "应用偏好和服务配置",
            Self::Logs => "最近的服务和插件事件",
        }
    }

    fn primary_action(&self) -> &'static str {
        match self {
            Self::Loading => "取消加载",
            Self::EmptyRepository => "打开文件夹",
            Self::Error => "重试加载",
            Self::FileList => "刷新列表",
            Self::SelectedFile => "打开预览",
            Self::Playlists => "刷新列表",
            Self::PluginSettings => "保存插件设置",
            Self::TaskRunning => "查看任务",
            Self::Conflict => "查看冲突",
            Self::UnsavedEdit => "保存更改",
            Self::Settings => "应用设置",
            Self::Logs => "刷新日志",
        }
    }
}

/// 壳层所需的宿主无关页面状态。
#[derive(Clone, Debug)]
pub struct ShellViewModel {
    pub page: ShellPage,
    pub repository_name: String,
    pub repository_id: Option<String>,
    pub selected_path: Option<String>,
    pub detail: String,
    pub dirty: bool,
    pub file_entries: Vec<String>,
    pub browser_entries: Vec<FileBrowserEntry>,
    pub current_directory: String,
    pub preview_url: Option<String>,
    pub plugin_entries: Vec<String>,
    pub plugin_entry_ids: Vec<String>,
    pub log_entries: Vec<String>,
    pub playlist_entries: Vec<String>,
    pub active_tasks: usize,
    pub completed_tasks: usize,
    pub active_task_ids: Vec<String>,
    pub system_status: Option<String>,
    pub selected_plugin_id: Option<String>,
    pub plugin_config_keys: Vec<String>,
}

impl Default for ShellViewModel {
    fn default() -> Self {
        Self {
            page: ShellPage::default(),
            repository_name: "默认资源库".into(),
            repository_id: None,
            selected_path: None,
            detail: "等待资源库服务响应".into(),
            dirty: false,
            file_entries: Vec::new(),
            browser_entries: Vec::new(),
            current_directory: String::new(),
            preview_url: None,
            plugin_entries: Vec::new(),
            plugin_entry_ids: Vec::new(),
            log_entries: Vec::new(),
            playlist_entries: Vec::new(),
            active_tasks: 0,
            completed_tasks: 0,
            active_task_ids: Vec::new(),
            system_status: None,
            selected_plugin_id: None,
            plugin_config_keys: Vec::new(),
        }
    }
}

impl ShellViewModel {
    /// 创建用于验收某一状态的页面模型。
    pub fn for_page(page: ShellPage) -> Self {
        let mut model = Self {
            page,
            ..Self::default()
        };
        match model.page {
            ShellPage::Error => model.detail = "无法读取仓库目录，请检查路径和权限".into(),
            ShellPage::EmptyRepository => model.detail = "可从文件夹或拖放导入资源".into(),
            ShellPage::FileList => model.detail = "12 个文件 · 按名称排序".into(),
            ShellPage::SelectedFile => {
                model.selected_path = Some("assets/cover.png".into());
                model.detail = "PNG 图片 · 1920 × 1080 · 2.4 MB".into();
            }
            ShellPage::Playlists => model.detail = "正在加载播放列表".into(),
            ShellPage::PluginSettings => {
                model.detail = "官方插件 · Nana 原生贡献接口 · 已加载 3 项配置".into();
            }
            ShellPage::TaskRunning => {
                model.detail = "扫描默认资源库 · 1,284 / 3,040 个文件".into();
            }
            ShellPage::Conflict => {
                model.selected_path = Some("assets/cover.png".into());
                model.detail = "远端修改时间较新，需要选择保留本地或远端版本".into();
            }
            ShellPage::UnsavedEdit => {
                model.selected_path = Some("notes/readme.md".into());
                model.dirty = true;
                model.detail = "Markdown · 3 行未保存 · 最后保存于 2 分钟前".into();
            }
            ShellPage::Settings => model.detail = "主题、缩略图缓存和默认播放器".into(),
            ShellPage::Logs => model.detail = "最近 24 小时 · 18 条记录 · 0 个错误".into(),
            ShellPage::Loading => {}
        }
        model
    }

    fn selection_label(&self) -> String {
        self.selected_path
            .as_deref()
            .map(|path| format!("当前文件：{path}"))
            .unwrap_or_else(|| "未选择文件".into())
    }

    fn edit_label(&self) -> &'static str {
        if self.dirty {
            "编辑内容 · 未保存"
        } else {
            "编辑内容"
        }
    }

    fn file_entries_label(&self) -> String {
        if self.file_entries.is_empty() {
            "当前目录暂无已加载条目".into()
        } else {
            format!("条目：{}", self.file_entries.iter().take(8).cloned().collect::<Vec<_>>().join("、"))
        }
    }

    fn plugin_entries_label(&self) -> String {
        if self.plugin_entries.is_empty() {
            "当前没有已安装插件".into()
        } else {
            format!("插件：{}", self.plugin_entries.iter().take(6).cloned().collect::<Vec<_>>().join("、"))
        }
    }

    fn log_entries_label(&self) -> String {
        if self.log_entries.is_empty() {
            "当前没有系统日志".into()
        } else {
            format!("日志：{}", self.log_entries.iter().take(6).cloned().collect::<Vec<_>>().join("、"))
        }
    }

    fn playlist_entries_label(&self) -> String {
        if self.playlist_entries.is_empty() {
            "当前没有播放列表".into()
        } else {
            format!("播放列表：{}", self.playlist_entries.iter().take(6).cloned().collect::<Vec<_>>().join("、"))
        }
    }

    /// 在 ViewModel 边界集中处理导航和页面动作，避免控件闭包直接修改领域状态。
    pub fn reduce(&mut self, message: ShellMessage) {
        match message {
            ShellMessage::Navigate(page) => {
                self.page = page;
                self.detail = match self.page {
                    ShellPage::TaskRunning => format!(
                        "{} 个运行中任务 · {} 个近期完成任务",
                        self.active_tasks, self.completed_tasks
                    ),
                    _ => "正在读取页面数据…".into(),
                };
            }
            ShellMessage::RepositoriesLoaded(Ok(repositories)) => {
                if repositories.is_empty() {
                    self.apply_page(ShellPage::EmptyRepository);
                } else {
                    self.apply_page(ShellPage::FileList);
                    self.repository_name = repositories
                        .first()
                        .map(|repository| repository.name.clone())
                        .unwrap_or_else(|| "默认资源库".into());
                    self.repository_id = repositories.first().map(|repository| repository.repo_id.clone());
                    self.detail = format!("{} 个资源库 · 已加载文件列表", repositories.len());
                }
            }
            ShellMessage::RepositoriesLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取资源库：{error}");
            }
            ShellMessage::RepositorySnapshotLoaded(Ok(snapshot)) => {
                self.page = ShellPage::FileList;
                self.repository_name = snapshot.repository.name;
                self.repository_id = Some(snapshot.repository.repo_id);
                self.detail = format!(
                    "{} 个文件 · {} 个文件夹 · {}",
                    snapshot.overview.file_count,
                    snapshot.overview.folder_count,
                    snapshot.repository.status
                );
            }
            ShellMessage::RepositorySnapshotLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取资源库文件列表：{error}");
            }
            ShellMessage::FileBrowserLoaded(Ok(browser)) => {
                self.page = ShellPage::FileList;
                self.file_entries = browser.entries.iter().map(|entry| entry.name.clone()).collect();
                self.browser_entries = browser.entries;
                self.current_directory = browser.current_path.clone();
                self.detail = format!(
                    "{} 个条目 · 当前目录 {}",
                    browser.total_entries, browser.current_path
                );
            }
            ShellMessage::FileBrowserLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取文件列表：{error}");
            }
            ShellMessage::SelectFile { path, .. } => {
                self.page = ShellPage::SelectedFile;
                self.selected_path = Some(path);
                self.detail = "正在读取文件元数据…".into();
            }
            ShellMessage::OpenDirectory(path) => {
                self.page = ShellPage::FileList;
                self.detail = format!("正在读取目录 {path}…");
                self.selected_path = None;
                self.preview_url = None;
            }
            ShellMessage::AssetDetailLoaded(Ok(detail)) => {
                self.page = ShellPage::SelectedFile;
                self.selected_path = Some(detail.summary.path);
                self.detail = format!(
                    "{} · {} 个元数据字段 · {} 个修订",
                    detail.summary.size_label,
                    detail.metadata.len(),
                    detail.revisions.len()
                );
            }
            ShellMessage::AssetDetailLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取文件元数据：{error}");
            }
            ShellMessage::PreviewSourceLoaded(Ok(source)) => {
                self.page = ShellPage::SelectedFile;
                self.preview_url = source.source_url;
                self.detail = format!(
                    "{} · {} · {} 字节",
                    source.media_type, source.path, source.size_bytes
                );
            }
            ShellMessage::PreviewSourceLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法打开预览源：{error}");
            }
            ShellMessage::PluginsLoaded(Ok(plugins)) => {
                self.page = ShellPage::PluginSettings;
                self.plugin_entries = plugins
                    .iter()
                    .map(|plugin| format!("{} {} · {}", plugin.name, plugin.version, plugin.status))
                    .collect();
                self.plugin_entry_ids = plugins.iter().map(|plugin| plugin.plugin_id.clone()).collect();
                self.detail = format!("{} 个插件 · 原生贡献接口优先", plugins.len());
            }
            ShellMessage::PluginsLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取插件列表：{error}");
            }
            ShellMessage::SelectPlugin(plugin_id) => {
                self.page = ShellPage::PluginSettings;
                self.detail = format!("正在读取插件 {plugin_id} 的原生设置…");
            }
            ShellMessage::PluginConfigLoaded(Ok(config)) => {
                self.page = ShellPage::PluginSettings;
                self.selected_plugin_id = Some(config.plugin_id.clone());
                self.plugin_config_keys = config.values.keys().cloned().collect();
                self.detail = format!("插件 {} · 已加载 {} 项配置", config.plugin_id, config.values.len());
            }
            ShellMessage::PluginConfigLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取插件设置：{error}");
            }
            ShellMessage::DeletePluginConfig { plugin_id, key } => {
                self.detail = format!("正在删除插件 {plugin_id} 的配置 {key}…");
            }
            ShellMessage::LogsLoaded(Ok(page)) => {
                self.page = ShellPage::Logs;
                self.log_entries = page
                    .records
                    .iter()
                    .map(|record| format!("{} · {} · {}", record.level, record.category, record.message))
                    .collect();
                self.detail = format!("最近日志 · {} 条记录", page.records.len());
            }
            ShellMessage::LogsLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取系统日志：{error}");
            }
            ShellMessage::PlaylistsLoaded(Ok(playlists)) => {
                self.page = ShellPage::Playlists;
                self.playlist_entries = playlists
                    .iter()
                    .map(|playlist| format!("{} · {} 项", playlist.name, playlist.item_count))
                    .collect();
                self.detail = format!("{} 个播放列表", playlists.len());
            }
            ShellMessage::PlaylistsLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取播放列表：{error}");
            }
            ShellMessage::TaskSnapshotLoaded { active, completed } => {
                self.page = ShellPage::TaskRunning;
                self.active_tasks = active;
                self.completed_tasks = completed;
                self.detail = format!("{} 个运行中任务 · {} 个近期完成任务", active, completed);
            }
            ShellMessage::SystemStatusLoaded(Ok(status)) => {
                self.page = ShellPage::Settings;
                self.system_status = Some(format!(
                    "{} · {}",
                    if status.ready { "服务已就绪" } else { "服务未就绪" },
                    status.base_url
                ));
                self.detail = self.system_status.clone().unwrap_or_default();
            }
            ShellMessage::SystemStatusLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取系统服务状态：{error}");
            }
            ShellMessage::CancelTask(task_id) => {
                self.detail = format!("已请求取消任务 {task_id}");
            }
            ShellMessage::WindowAction(_) => {}
            ShellMessage::Refresh => {
                self.detail = "刷新服务尚未接通，当前数据未变更".into();
            }
            ShellMessage::PrimaryAction => {
                self.detail = "该操作的领域服务尚未接通，数据未写入".into();
            }
            ShellMessage::EditAction => {
                self.detail = "原生编辑器尚未接通，当前文件未修改".into();
            }
        }
    }

    fn apply_page(&mut self, page: ShellPage) {
        *self = Self::for_page(page);
    }
}

/// 在给定 Runtime 文档中挂载完整的 MomoBako 壳层。
pub fn mount_shell(
    document: &mut RuntimeDocument,
    model: &ShellViewModel,
) -> Result<(), FrameworkError> {
    let document_id = document.document();
    let view_model = model.clone();
    document
        .context_mut()
        .mount_view_root(document_id, move || {
            // 标题栏、导航栏和主工作区分别承担窗口级操作、上下文导航和资源主线。
            let navigation = widget(
                Stack::fill_column(8.0)
                    .width(LengthSpec::Px(220.0))
                    .grow(0.0)
                    .shrink(0.0)
                    .padding_xy(16.0, 18.0),
            )
            .children((
                button("资源库").key("nav-library").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Navigate(ShellPage::FileList));
                }),
                button("播放列表").key("nav-playlists").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Navigate(ShellPage::Playlists));
                }),
                button("插件").key("nav-plugins").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Navigate(ShellPage::PluginSettings));
                }),
                button("设置").key("nav-settings").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Navigate(ShellPage::Settings));
                }),
            ));
            let status_summary = widget(Stack::fill_column(8.0)).children((
                text(view_model.page.title()).key("page-title"),
                text(view_model.page.status()).key("page-status"),
                text(view_model.selection_label()).key("selection"),
                text(view_model.detail.clone()).key("page-detail"),
                text(view_model.file_entries_label()).key("file-entries"),
                text(view_model.plugin_entries_label()).key("plugin-entries"),
                text(view_model.log_entries_label()).key("log-entries"),
                text(view_model.playlist_entries_label()).key("playlist-entries"),
                text(view_model.system_status.clone().unwrap_or_else(|| "尚未读取系统服务状态".into()))
                    .key("system-status"),
            ));
            let task_actions = widget(Stack::fill_column(6.0)).children(
                view_model
                    .active_task_ids
                    .iter()
                    .map(|task_id| {
                        let task_id = task_id.clone();
                        button(format!("取消任务 {task_id}"))
                            .key(format!("cancel-task-{task_id}"))
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program(ShellMessage::CancelTask(task_id.clone()));
                            })
                    })
                    .collect::<Vec<_>>(),
            );
            let file_actions = widget(Stack::fill_column(6.0)).children(
                view_model
                    .browser_entries
                    .iter()
                    .take(8)
                    .map(|entry| {
                        let entry = entry.clone();
                        button(entry.name.clone())
                            .key(format!("file-entry-{}", entry.path))
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program(entry_message(&entry));
                            })
                    })
                    .collect::<Vec<_>>(),
            );
            let plugin_actions = widget(Stack::fill_column(6.0)).children(
                view_model
                    .plugin_entries
                    .iter()
                    .zip(view_model.plugin_entry_ids.iter())
                    .take(8)
                    .map(|(label, plugin_id)| {
                        let plugin_id = plugin_id.clone();
                        button(label.clone())
                            .key(format!("plugin-entry-{plugin_id}"))
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program(ShellMessage::SelectPlugin(plugin_id.clone()));
                            })
                    })
                    .collect::<Vec<_>>(),
            );
            let plugin_config_actions = widget(Stack::fill_column(6.0)).children(
                view_model
                    .selected_plugin_id
                    .as_ref()
                    .into_iter()
                    .flat_map(|plugin_id| {
                        view_model.plugin_config_keys.iter().map(move |key| {
                            let plugin_id = plugin_id.clone();
                            let key = key.clone();
                            button(format!("删除配置 {key}"))
                                .key(format!("delete-plugin-config-{key}"))
                                .on_cx(move |_, _: &Activate, cx| {
                                    cx.dispatch_program(ShellMessage::DeletePluginConfig {
                                        plugin_id: plugin_id.clone(),
                                        key: key.clone(),
                                    });
                                })
                        })
                    })
                    .collect::<Vec<_>>(),
            );
            let content = widget(
                Stack::fill_column(12.0)
                    .padding_xy(24.0, 20.0)
                    .min_width(LengthSpec::Px(0.0)),
            )
            .children((status_summary, task_actions, file_actions, plugin_actions, plugin_config_actions, widget(
                List::new()
                    .label(view_model.page.title())
                    .style(Stack::column(12.0).node_style()),
            )
            .children((
                button(view_model.page.primary_action())
                    .key("primary-action")
                    .on_cx(|_, _: &Activate, cx| cx.dispatch_program(ShellMessage::PrimaryAction)),
                button(view_model.edit_label())
                    .key("edit-action")
                    .on_cx(|_, _: &Activate, cx| cx.dispatch_program(ShellMessage::EditAction)),
            )),));
            let process = widget(
                Stack::fill_column(8.0)
                    .width(LengthSpec::Px(240.0))
                    .grow(0.0)
                    .shrink(0.0)
                    .padding_xy(16.0, 20.0),
            )
            .children((
                text("当前状态").key("process-heading"),
                text(view_model.page.status()).key("process-status"),
            ));
            let title_bar = widget(
                Stack::bar(12.0)
                    .height(LengthSpec::Px(48.0))
                    .padding_xy(20.0, 12.0),
            )
            .children((
                text("MomoBako").key("title"),
                text("资源库工作区").key("subtitle"),
                button("刷新状态")
                    .key("refresh")
                    .on_cx(|_, _: &Activate, cx| cx.dispatch_program(ShellMessage::Refresh)),
                button("最小化")
                    .key("window-minimize")
                    .on_cx(|_, _: &Activate, cx| {
                        cx.dispatch_program(ShellMessage::WindowAction(WindowAction::Minimize));
                    }),
                button("最大化")
                    .key("window-maximize")
                    .on_cx(|_, _: &Activate, cx| {
                        cx.dispatch_program(ShellMessage::WindowAction(WindowAction::ToggleMaximize));
                    }),
                button("关闭")
                    .key("window-close")
                    .on_cx(|_, _: &Activate, cx| {
                        cx.dispatch_program(ShellMessage::WindowAction(WindowAction::Close));
                    }),
            ));
            let body = widget(Stack::fill_row(0.0).min_height(LengthSpec::Px(0.0)))
                .children((navigation, content, process))
                .key("workspace-body");
            widget(Stack::fill_column(0.0).min_width(LengthSpec::Px(0.0)))
                .children((title_bar, body))
        })?;
    Ok(())
}

/// 显示名仅用于标签，服务请求始终使用 DTO 中的完整仓库相对路径。
fn entry_message(entry: &FileBrowserEntry) -> ShellMessage {
    if entry.kind == "directory" {
        ShellMessage::OpenDirectory(entry.path.clone())
    } else {
        ShellMessage::SelectFile { path: entry.path.clone(), asset_id: entry.asset_id.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::{ShellMessage, ShellPage, ShellViewModel};

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
        assert_eq!(model.detail, "该操作的领域服务尚未接通，数据未写入");
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
}

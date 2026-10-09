//! 设置页卡片的投影和常驻信号（插件管理面板另有自己的，见 `admin_plugins_state.rs`）。
//!
//! [`SettingsView`] 按 `Settings.vue` 算好音频、外观、仓库服务、外部素材接入、缓存和 API 设计
//! 各卡片要显示的文字；[`SettingsSignals`] 每张卡片一个信号，同步时只写变了的那张。

use nana_ui::runtime::view::{signal, Signal};

use super::super::ShellViewModel;
use super::support;

/// 设置页卡片要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SettingsView {
    pub audio: AudioCard,
    /// 圆角样式：`smooth` 或 `round`。
    pub corner_style: String,
    pub corner_radius: f64,
    pub repository: RepositoryCard,
    pub external: ExternalCard,
    pub cache: CacheCard,
    pub api: ApiCard,
}

/// 音频播放：下拉框的选项、当前值、能不能选，以及回退或缺失提示。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AudioCard {
    /// `(插件 id, 文字)`。
    pub options: Vec<(String, String)>,
    pub selected: String,
    pub selectable: bool,
    /// 提示文字，错误时为真。
    pub notice: Option<(String, bool)>,
}

/// 仓库服务的两个会变的值。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RepositoryCard {
    pub count: String,
    pub backends: String,
}

/// 外部素材接入。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ExternalCard {
    pub ready: bool,
    pub status: String,
    /// 读到了连接状态：没有时复制 Base URL 和 Token 不能点。
    pub loaded: bool,
    pub file: String,
    pub url: String,
    /// 打了码的 Token。
    pub token: String,
    pub started: String,
    /// 复制和导出用的原值。
    pub base_url: String,
    pub raw_token: String,
    pub json: String,
    pub error: String,
    pub message: String,
}

/// `.kv` 的一行：名称、值，末行没有分割线。没有编号，身份是内容加位置。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct KvRow {
    pub key: String,
    pub name: String,
    pub value: String,
    pub last: bool,
}

/// 缓存：三格容量和最近条目。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CacheCard {
    pub metrics: [usize; 3],
    pub entries: Vec<KvRow>,
}

/// API 设计：传输方式和端点。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ApiCard {
    pub transport: String,
    pub endpoints: Vec<KvRow>,
}

impl SettingsView {
    /// 从 ViewModel 取设置页卡片的投影，取舍和旧视图一致。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let admin = &model.admin;
        let audio = support::audio_view(&model.player.candidates, &admin.plugins, &model.player.preferences);
        let status = admin.external.as_ref();
        let text = |value: Option<&str>| value.map(str::to_string).unwrap_or_else(|| "未加载".into());
        let config = admin.cache.as_ref().map(|cache| &cache.config);
        let entries = admin.cache.as_ref().map(|cache| cache.entries.as_slice()).unwrap_or(&[]);
        let endpoints = admin.api_design.as_ref().map(|api| api.endpoints.as_slice()).unwrap_or(&[]);
        Self {
            audio: AudioCard {
                options: audio.choices.iter().map(|choice| (choice.plugin_id.clone(), choice.label.clone())).collect(),
                selected: audio.selected,
                selectable: audio.selectable,
                notice: audio.notice,
            },
            corner_style: admin.corner_style.clone(),
            corner_radius: admin.corner_radius,
            repository: RepositoryCard {
                count: model.workspace.repositories.len().to_string(),
                backends: support::backend_summary(&admin.backends),
            },
            external: ExternalCard {
                ready: status.is_some_and(|item| item.ready),
                status: support::external_status_label(status.map(|item| item.ready)).to_string(),
                loaded: status.is_some(),
                file: text(status.map(|item| item.connection_file_path.as_str())),
                url: text(status.map(|item| item.base_url.as_str())),
                token: support::mask_token(status.map(|item| item.token.as_str())),
                started: text(status.map(|item| item.started_at.as_str())),
                base_url: status.map(|item| item.base_url.clone()).unwrap_or_default(),
                raw_token: status.map(|item| item.token.clone()).unwrap_or_default(),
                json: status.map(|item| support::connection_json(&item.base_url, &item.token, &item.version, &item.started_at)).unwrap_or_default(),
                error: admin.external_error.clone(),
                message: admin.external_message.clone(),
            },
            cache: CacheCard {
                metrics: [
                    config.map(|item| item.metadata_capacity).unwrap_or(0),
                    config.map(|item| item.thumbnail_capacity).unwrap_or(0),
                    config.map(|item| item.query_capacity).unwrap_or(0),
                ],
                entries: entries
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| KvRow {
                        key: format!("admin-cache-entry-{index}"),
                        name: format!("{} / {}", entry.cache_type, entry.key),
                        value: entry.last_accessed_at.clone(),
                        last: index + 1 == entries.len(),
                    })
                    .collect(),
            },
            api: ApiCard {
                transport: admin.api_design.as_ref().map(|api| api.transport.clone()).unwrap_or_else(|| "本地服务契约未加载".into()),
                endpoints: endpoints
                    .iter()
                    .enumerate()
                    .map(|(index, endpoint)| KvRow {
                        key: format!("admin-api-endpoint-{index}"),
                        name: format!("{} / {} {}", endpoint.group, endpoint.method, endpoint.path),
                        value: endpoint.summary.clone(),
                        last: index + 1 == endpoints.len(),
                    })
                    .collect(),
            },
        }
    }
}

/// 设置页卡片的信号，每张卡片一个。句柄都是 `Copy` 的 id。
#[derive(Clone, Copy)]
pub(crate) struct SettingsSignals {
    pub(crate) audio: Signal<AudioCard>,
    pub(crate) corner_style: Signal<String>,
    pub(crate) corner_radius: Signal<f64>,
    pub(crate) repository: Signal<RepositoryCard>,
    pub(crate) external: Signal<ExternalCard>,
    pub(crate) cache: Signal<CacheCard>,
    pub(crate) api: Signal<ApiCard>,
}

impl SettingsSignals {
    /// 空的信号，进路由之前由同步写入第一份投影。
    pub(crate) fn new() -> Self {
        Self {
            audio: signal(AudioCard::default()),
            corner_style: signal(String::new()),
            corner_radius: signal(0.0),
            repository: signal(RepositoryCard::default()),
            external: signal(ExternalCard::default()),
            cache: signal(CacheCard::default()),
            api: signal(ApiCard::default()),
        }
    }

    /// 写入投影，只写变了的卡片。
    pub(crate) fn write(&self, view: SettingsView) {
        self.audio.try_set_if_changed(view.audio);
        self.corner_style.try_set_if_changed(view.corner_style);
        self.corner_radius.try_set_if_changed(view.corner_radius);
        self.repository.try_set_if_changed(view.repository);
        self.external.try_set_if_changed(view.external);
        self.cache.try_set_if_changed(view.cache);
        self.api.try_set_if_changed(view.api);
    }
}

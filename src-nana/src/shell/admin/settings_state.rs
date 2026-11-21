//! 设置页卡片的投影和常驻信号（插件管理面板另有自己的，见 `shell/admin/plugins_state.rs`）。
//!
//! [`SettingsView`] 算好音频、外观、仓库服务、外部素材接入、缓存和 API 设计
//! 各卡片要显示的文字；[`SettingsSignals`] 每张卡片一个信号，同步时只写变了的那张。

use nana_ui::runtime::view::{Signal, signal};

use super::super::ShellViewModel;
use super::support;
use crate::backend::services::repository::CacheSnapshot;

/// 设置页卡片要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SettingsView {
    pub audio: AudioCard,
    /// 主题设置：`dark`、`light` 或 `system`（跟随系统）。
    pub theme: String,
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

/// 键值表的一行：名称和值。没有编号，身份是内容。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct KvRow {
    pub key: String,
    pub name: String,
    pub value: String,
}

/// 缓存：当前资源库的缩略图文件数、其余缓存文件数和总占用三格，下面逐项列出各缓存目录。
/// 没有打开资源库或还没读回来时三格写「未加载」，不另造数字。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CacheCard {
    pub metrics: [String; 3],
    pub entries: Vec<KvRow>,
}

impl CacheCard {
    fn project(snapshot: Option<&CacheSnapshot>) -> Self {
        let Some(snapshot) = snapshot else {
            return Self {
                metrics: std::array::from_fn(|_| "未加载".into()),
                entries: Vec::new(),
            };
        };
        let thumbnails = snapshot
            .entries
            .iter()
            .filter(|entry| entry.kind == "thumbnails")
            .map(|entry| entry.files)
            .sum::<u64>();
        let files = snapshot
            .entries
            .iter()
            .map(|entry| entry.files)
            .sum::<u64>();
        let bytes = snapshot
            .entries
            .iter()
            .map(|entry| entry.bytes)
            .sum::<u64>();
        Self {
            metrics: [
                thumbnails.to_string(),
                (files - thumbnails).to_string(),
                human_bytes(bytes),
            ],
            entries: snapshot
                .entries
                .iter()
                .map(|entry| KvRow {
                    key: format!("admin-cache-entry-{}", entry.kind.replace('/', "-")),
                    name: entry.label.clone(),
                    value: format!("{} 个文件 · {}", entry.files, human_bytes(entry.bytes)),
                })
                .collect(),
        }
    }
}

/// 字节数按 1024 进位写成 B、KB、MB、GB，KB 以上保留一位小数。
pub(crate) fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
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
        let audio = support::audio_view(
            &model.player.candidates,
            &admin.plugins,
            &model.player.preferences,
        );
        let status = admin.external.as_ref();
        let text =
            |value: Option<&str>| value.map(str::to_string).unwrap_or_else(|| "未加载".into());
        let endpoints = admin
            .api_design
            .as_ref()
            .map(|api| api.endpoints.as_slice())
            .unwrap_or(&[]);
        Self {
            audio: AudioCard {
                options: audio
                    .choices
                    .iter()
                    .map(|choice| (choice.plugin_id.clone(), choice.label.clone()))
                    .collect(),
                selected: audio.selected,
                selectable: audio.selectable,
                notice: audio.notice,
            },
            theme: model.settings.theme.clone(),
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
                json: status
                    .map(|item| {
                        support::connection_json(
                            &item.base_url,
                            &item.token,
                            &item.version,
                            &item.started_at,
                        )
                    })
                    .unwrap_or_default(),
                error: admin.external_error.clone(),
                message: admin.external_message.clone(),
            },
            cache: CacheCard::project(admin.cache.as_ref().filter(|cache| {
                model.workspace.active_repo_id.as_deref() == Some(cache.repo_id.as_str())
            })),
            api: ApiCard {
                transport: admin
                    .api_design
                    .as_ref()
                    .map(|api| api.transport.clone())
                    .unwrap_or_else(|| "本地服务契约未加载".into()),
                endpoints: endpoints
                    .iter()
                    .enumerate()
                    .map(|(index, endpoint)| KvRow {
                        key: format!("admin-api-endpoint-{index}"),
                        name: format!("{} / {} {}", endpoint.group, endpoint.method, endpoint.path),
                        value: endpoint.summary.clone(),
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
    pub(crate) theme: Signal<String>,
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
            theme: signal(String::new()),
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
        self.theme.try_set_if_changed(view.theme);
        self.corner_style.try_set_if_changed(view.corner_style);
        self.corner_radius.try_set_if_changed(view.corner_radius);
        self.repository.try_set_if_changed(view.repository);
        self.external.try_set_if_changed(view.external);
        self.cache.try_set_if_changed(view.cache);
        self.api.try_set_if_changed(view.api);
    }
}

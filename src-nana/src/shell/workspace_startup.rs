//! 首屏启动的步骤和加载日志。
//!
//! 对应 Vue `AppShell.vue` 的启动区块和 `workspace/lifecycle.ts` 的 `emitStartupLog`：四个步骤、
//! 当前步骤的标题和说明、进度百分比，以及「加载日志」里最近 8 条记录。日志文案、级别和
//! `键: 值 · 键: 值` 的明细格式与 Vue 一致；时间是本地时钟的「时:分:秒」。
//!
//! 换仓库（Vue `navigation.ts` 的 `selectRepository` 切到别的仓库）也走这块启动页，见 [`StartupMode`]。

/// 启动步骤总数。
pub const STARTUP_TOTAL_STEPS: u8 = 4;
const STARTUP_LOG_LIMIT: usize = 40;
const STARTUP_VISIBLE_LOGS: usize = 8;

const STARTUP_STEP_HINTS: [(&str, &str); 4] = [
    ("准备资源库", "读取仓库列表或切换目标资源库。"),
    ("同步文件变化", "扫描新增、移动、删除和缓存状态。"),
    ("读取资源索引", "整理摘要、素材索引和默认预览对象。"),
    ("加载首屏内容", "准备目录、播放列表和首屏辅助数据。"),
];

/// 启动流程状态。失败时保留当前步骤，已完成步骤不退回。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StartupStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Error,
}

/// 这一轮加载是首屏启动还是换仓库。
///
/// 换仓库照 Vue `selectRepository`：从「切换资源库」起步，不写首屏启动日志（之前的日志留着，同步日志照常
/// 追加），第 4 步的说明不同，失败停在启动页给「重试」，结束时不重读设置包。重试走完整的首屏启动。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StartupMode {
    #[default]
    Launch,
    Switch,
}

/// 首屏启动第 4 步等的设置数据，照 Vue `ensureRepositoryWorkspace` 在离开启动页之前 `await loadSettingsData()`：
/// 读完首屏目录（空库、缺失仓库是选定以后）先等设置包，插件列表到了再等插件登记的播放器类型（Vue 在
/// `loadSettingsData` 里同步前端插件注册表），都回来（失败也算）才结束启动。换仓库不读设置包，不等。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsWait {
    /// 不等：还没走到这一步，或者是换仓库。
    #[default]
    None,
    /// 等设置包：插件、钩子记录、缓存、API 设计和外部连接。
    Bundle,
    /// 设置包到了，等播放器类型。
    Players,
}

/// 单个启动步骤在步骤条上的状态。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StartupStepState {
    Pending,
    Current,
    Done,
    Error,
}

/// 启动步骤的可见文案。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupStepItem {
    pub number: u8,
    pub label: &'static str,
    pub detail: &'static str,
    pub state: StartupStepState,
}

/// 同步写回的计数（Vue `lastSyncResult`），「首屏启动文件变化同步完成。」的明细用。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SyncCounts {
    pub scanned_files: i64,
    pub created_assets: i64,
    pub updated_assets: i64,
    pub deleted_assets: i64,
    pub created_events: i64,
    pub hardlink_candidates: i64,
}

impl SyncCounts {
    /// 从同步协议的输出（后端 `SyncResult` 的 JSON）里取计数。缺的或不是整数的按 0，并记日志。
    pub fn from_output(output: &serde_json::Value) -> Self {
        let read = |key: &str| {
            output.get(key).and_then(serde_json::Value::as_i64).unwrap_or_else(|| {
                eprintln!("Nana 同步结果里没有 {key}，按 0 记");
                0
            })
        };
        Self {
            scanned_files: read("scannedFiles"),
            created_assets: read("createdAssets"),
            updated_assets: read("updatedAssets"),
            deleted_assets: read("deletedAssets"),
            created_events: read("createdEvents"),
            hardlink_candidates: read("hardlinkCandidates"),
        }
    }
}

/// 读完仓库摘要时记下的数（Vue `activeSnapshot`、`playlists` 和 `activeAssetId`），摘要、首屏目录和流程完成
/// 三条日志的明细用。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexCounts {
    pub asset_count: usize,
    pub playlist_count: usize,
    /// 默认预览对象：之前选中的素材还在就是它，否则是第一个素材。
    pub default_asset_id: Option<String>,
}

/// 一条加载日志：级别、本地时间、消息和明细。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StartupLog {
    pub level: &'static str,
    /// 本地时间「时:分:秒」。
    pub time: String,
    pub message: String,
    /// `键: 值 · 键: 值`。没有明细时为 `None`。
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupState {
    pub status: StartupStatus,
    pub mode: StartupMode,
    pub step_label: String,
    pub step_detail: String,
    pub current_step: u8,
    pub total_steps: u8,
    pub percent: u8,
    pub error: Option<String>,
    pub logs: Vec<StartupLog>,
    /// 这一轮读到的仓库摘要计数；没读到摘要（空库、缺失仓库）时为 `None`。
    pub index: Option<IndexCounts>,
    /// 首屏启动第 4 步还在等的设置数据，见 [`SettingsWait`]。
    pub settings: SettingsWait,
    /// 这一轮开始时加载日志里的活动仓库（Vue `ensureRepositoryWorkspace` 开始时的 `activeRepoId`）。
    pub base_repo_id: Option<String>,
    /// 这一轮已经换上了活动仓库：首屏启动读完摘要、选中缺失仓库或列表为空时，换仓库一开始就换（Vue 在这些时候
    /// 写 `activeRepoId`）。换上以前，加载日志里的活动仓库还是 [`Self::base_repo_id`]。
    pub active_applied: bool,
}

impl Default for StartupState {
    fn default() -> Self {
        Self {
            status: StartupStatus::Idle,
            mode: StartupMode::Launch,
            step_label: "准备加载仓库".into(),
            step_detail: "准备资源库状态，恢复上次打开的工作区。".into(),
            current_step: 0,
            total_steps: STARTUP_TOTAL_STEPS,
            percent: 0,
            error: None,
            logs: Vec::new(),
            index: None,
            settings: SettingsWait::None,
            base_repo_id: None,
            active_applied: false,
        }
    }
}

impl StartupState {
    /// 重新开始一轮启动。已有日志和错误随这轮一起清空。
    pub fn begin(&mut self) {
        self.begin_with(None, None);
    }

    /// 重新开始一轮启动，并记下「首屏启动流程开始。」，明细带当前和上次打开的资源库。`active_repo_id` 也是这一轮
    /// 换上活动仓库以前日志里的活动仓库。
    pub fn begin_with(&mut self, active_repo_id: Option<&str>, remembered_repo_id: Option<&str>) {
        *self = Self {
            status: StartupStatus::Loading,
            base_repo_id: active_repo_id.map(str::to_string),
            ..Self::default()
        };
        let total = STARTUP_TOTAL_STEPS.to_string();
        self.log(
            "info",
            "首屏启动流程开始。",
            &[("totalSteps", Some(total.as_str())), ("activeRepoId", active_repo_id), ("rememberedRepoId", remembered_repo_id)],
        );
    }

    /// 换仓库：回到第 1 步「切换资源库」（Vue `setWorkspaceStartupProgress(1, "切换资源库", …)`）。
    /// 已有的加载日志留着，这一轮不再写首屏启动日志。
    pub fn begin_switch(&mut self) {
        self.mode = StartupMode::Switch;
        self.settings = SettingsWait::None;
        self.active_applied = true;
        self.set_progress(1, "切换资源库", "准备目标资源库，清理上一个资源库的首屏状态。");
    }

    /// 第 4 步「读取首屏目录」的说明：启动和换仓库各照 Vue 的原文。
    pub fn first_screen_detail(&self) -> &'static str {
        match self.mode {
            StartupMode::Launch => "加载根目录、播放列表和首屏关联数据。",
            StartupMode::Switch => "加载根目录文件、侧栏结构和首屏内容。",
        }
    }

    /// 进入指定步骤。百分比按当前步除以总步数，和 Vue 一致。步骤本身不记日志。
    pub fn set_progress(&mut self, step: u8, label: impl Into<String>, detail: impl Into<String>) {
        let step = step.clamp(1, STARTUP_TOTAL_STEPS);
        self.status = StartupStatus::Loading;
        self.step_label = label.into();
        self.step_detail = detail.into();
        self.current_step = step;
        self.total_steps = STARTUP_TOTAL_STEPS;
        self.percent = u8::try_from((u16::from(step) * 100) / u16::from(STARTUP_TOTAL_STEPS)).unwrap_or(100);
        self.error = None;
    }

    /// 停在当前步骤。更早的步骤保持完成，当前步骤变为失败。
    pub fn fail(&mut self, message: impl Into<String>) {
        self.fail_with(message, None, None);
    }

    /// 停在当前步骤，并记下「首屏启动流程失败。」，明细带步骤、错误和相关资源库。
    pub fn fail_with(&mut self, message: impl Into<String>, active_repo_id: Option<&str>, target_repo_id: Option<&str>) {
        let message = message.into();
        self.status = StartupStatus::Error;
        self.step_label = "加载失败".into();
        self.step_detail = "资源库加载流程已停止，保留当前错误供重试。".into();
        self.error = Some(message.clone());
        let step = self.current_step.to_string();
        let label = self.step_label.clone();
        self.log(
            "error",
            "首屏启动流程失败。",
            &[
                ("step", Some(step.as_str())),
                ("stepLabel", Some(label.as_str())),
                ("error", Some(message.as_str())),
                ("activeRepoId", active_repo_id),
                ("targetRepoId", target_repo_id),
            ],
        );
    }

    pub fn finish(&mut self) {
        self.status = StartupStatus::Ready;
        self.step_label = "加载完成".into();
        self.step_detail = "工作区首屏已经准备完成。".into();
        self.current_step = STARTUP_TOTAL_STEPS;
        self.total_steps = STARTUP_TOTAL_STEPS;
        self.percent = 100;
        self.error = None;
    }

    /// 步骤条规则与 `AppShell.vue` 的 `startupStepItems` 相同。
    pub fn step_items(&self) -> [StartupStepItem; 4] {
        std::array::from_fn(|index| {
            let number = u8::try_from(index + 1).unwrap_or(1);
            let (label, detail) = STARTUP_STEP_HINTS[index];
            StartupStepItem {
                number,
                label,
                detail,
                state: self.step_state(number),
            }
        })
    }

    pub fn step_state(&self, step_number: u8) -> StartupStepState {
        let is_current = self.current_step == step_number;
        let is_done = self.current_step > step_number || self.status == StartupStatus::Ready;
        let is_error = is_current && self.status == StartupStatus::Error;
        if is_error {
            StartupStepState::Error
        } else if is_done {
            StartupStepState::Done
        } else if is_current {
            StartupStepState::Current
        } else {
            StartupStepState::Pending
        }
    }

    /// 首屏启动开始等设置数据（Vue 第 4 步 `loadSettingsData` 之前）：换上第 4 步的说明，记「首屏启动开始加载应用
    /// 配置与插件设置。」，明细带第 4 步和活动仓库。设置包和播放器类型都回来再由 [`Self::finish_launch`] 收尾。
    pub fn await_settings(&mut self, repo_id: Option<&str>) {
        self.set_progress(4, "读取首屏目录", "加载应用配置、插件设置和首屏辅助数据。");
        self.settings = SettingsWait::Bundle;
        self.log("info", "首屏启动开始加载应用配置与插件设置。", &[("step", Some("4")), ("repoId", repo_id)]);
    }

    /// 设置数据都回来了（失败也算，和 Vue `loadSettingsData` 吃掉错误一样）：首屏启动结束，记「首屏启动应用配置与
    /// 插件设置加载完成。」和「首屏启动流程完成。」，后者明细带活动仓库、素材数和播放集数（没读到摘要时都是 0）。
    pub fn finish_launch(&mut self, repo_id: Option<&str>) {
        self.settings = SettingsWait::None;
        self.log("info", "首屏启动应用配置与插件设置加载完成。", &[("step", Some("4")), ("repoId", repo_id)]);
        self.finish();
        let index = self.index.clone().unwrap_or_default();
        let (assets, playlists) = (index.asset_count.to_string(), index.playlist_count.to_string());
        self.log(
            "info",
            "首屏启动流程完成。",
            &[("activeRepoId", repo_id), ("assetCount", Some(assets.as_str())), ("playlistCount", Some(playlists.as_str()))],
        );
    }

    /// 最近 8 条日志，新的在前。
    pub fn visible_logs(&self) -> Vec<&StartupLog> {
        self.logs.iter().rev().take(STARTUP_VISIBLE_LOGS).collect()
    }

    /// 启动仍在加载时追加一条同步日志，明细照 Vue `appendStartupSyncLog` 取记录的 `context`。其它状态忽略。
    pub(crate) fn append_sync_log(&mut self, level: &str, message: impl Into<String>, context: &serde_json::Value) {
        if self.status != StartupStatus::Loading {
            return;
        }
        let level = match level {
            "debug" => "debug",
            "warn" => "warn",
            "error" => "error",
            _ => "info",
        };
        self.push(level, message.into(), context_detail(context));
    }

    /// 记一条日志。明细按 Vue `appendStartupLog` 去掉空值，用「 · 」连接。换仓库不写首屏启动日志
    /// （Vue `selectRepository` 不调 `emitStartupLog`）。
    pub(crate) fn log(&mut self, level: &'static str, message: &str, context: &[(&str, Option<&str>)]) {
        if self.mode == StartupMode::Switch {
            return;
        }
        let detail = context
            .iter()
            .filter_map(|(key, value)| value.filter(|value| !value.is_empty()).map(|value| format!("{key}: {value}")))
            .collect::<Vec<_>>()
            .join(" · ");
        self.push(level, message.to_string(), (!detail.is_empty()).then_some(detail));
    }

    fn push(&mut self, level: &'static str, message: String, detail: Option<String>) {
        self.logs.push(StartupLog { level, time: local_clock(), message, detail });
        if self.logs.len() > STARTUP_LOG_LIMIT {
            let extra = self.logs.len() - STARTUP_LOG_LIMIT;
            self.logs.drain(0..extra);
        }
    }
}

/// 同步记录 `context` 的明细，照 Vue `appendStartupLog`：去掉 null 和空串，对象和数组写成紧凑 JSON，其余写成
/// 字面值，`键: 值` 用「 · 」连接。不是对象或没有可写的键时为 `None`。
fn context_detail(context: &serde_json::Value) -> Option<String> {
    let entries = context.as_object()?;
    let detail = entries
        .iter()
        .filter_map(|(key, value)| {
            let text = match value {
                serde_json::Value::Null => return None,
                serde_json::Value::String(text) if text.is_empty() => return None,
                serde_json::Value::String(text) => text.clone(),
                serde_json::Value::Array(_) | serde_json::Value::Object(_) => value.to_string(),
                serde_json::Value::Bool(_) | serde_json::Value::Number(_) => value.to_string(),
            };
            Some(format!("{key}: {text}"))
        })
        .collect::<Vec<_>>()
        .join(" · ");
    (!detail.is_empty()).then_some(detail)
}

/// 本地时间「时:分:秒」，和 Vue `toLocaleTimeString("zh-CN", { hour12: false })` 的写法一致。
pub fn local_clock() -> String {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTime {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            milliseconds: u16,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetLocalTime(time: *mut SystemTime);
        }
        let mut now = SystemTime { year: 0, month: 0, day_of_week: 0, day: 0, hour: 0, minute: 0, second: 0, milliseconds: 0 };
        // SAFETY: GetLocalTime 只写入调用方传入的结构体，结构体布局与 Win32 SYSTEMTIME 一致。
        unsafe { GetLocalTime(&mut now) };
        format!("{:02}:{:02}:{:02}", now.hour, now.minute, now.second)
    }
    #[cfg(not(windows))]
    {
        // 非 Windows 平台拿不到时区，退回 UTC。
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_else(|error| {
                eprintln!("Nana 系统时钟早于 1970 年：{error}");
                0
            });
        let day = seconds % 86_400;
        format!("{:02}:{:02}:{:02}", day / 3600, day % 3600 / 60, day % 60)
    }
}

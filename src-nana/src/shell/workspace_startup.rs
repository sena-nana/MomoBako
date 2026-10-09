//! 首屏启动的步骤和加载日志。
//!
//! 对应 Vue `AppShell.vue` 的启动区块和 `workspace/lifecycle.ts` 的 `emitStartupLog`：四个步骤、
//! 当前步骤的标题和说明、进度百分比，以及「加载日志」里最近 8 条记录。日志文案、级别和
//! `键: 值 · 键: 值` 的明细格式与 Vue 一致；时间是本地时钟的「时:分:秒」。

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
    pub step_label: String,
    pub step_detail: String,
    pub current_step: u8,
    pub total_steps: u8,
    pub percent: u8,
    pub error: Option<String>,
    pub logs: Vec<StartupLog>,
}

impl Default for StartupState {
    fn default() -> Self {
        Self {
            status: StartupStatus::Idle,
            step_label: "准备加载仓库".into(),
            step_detail: "准备资源库状态，恢复上次打开的工作区。".into(),
            current_step: 0,
            total_steps: STARTUP_TOTAL_STEPS,
            percent: 0,
            error: None,
            logs: Vec::new(),
        }
    }
}

impl StartupState {
    /// 重新开始一轮启动。已有日志和错误随这轮一起清空。
    pub fn begin(&mut self) {
        self.begin_with(None, None);
    }

    /// 重新开始一轮启动，并记下「首屏启动流程开始。」，明细带当前和上次打开的资源库。
    pub fn begin_with(&mut self, active_repo_id: Option<&str>, remembered_repo_id: Option<&str>) {
        *self = Self {
            status: StartupStatus::Loading,
            ..Self::default()
        };
        let total = STARTUP_TOTAL_STEPS.to_string();
        self.log(
            "info",
            "首屏启动流程开始。",
            &[("totalSteps", Some(total.as_str())), ("activeRepoId", active_repo_id), ("rememberedRepoId", remembered_repo_id)],
        );
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

    /// 最近 8 条日志，新的在前。
    pub fn visible_logs(&self) -> Vec<&StartupLog> {
        self.logs.iter().rev().take(STARTUP_VISIBLE_LOGS).collect()
    }

    /// 启动仍在加载时追加一条同步日志。其它状态忽略。
    pub(crate) fn append_sync_log(&mut self, level: &str, message: impl Into<String>) {
        if self.status != StartupStatus::Loading {
            return;
        }
        let level = match level {
            "debug" => "debug",
            "warn" => "warn",
            "error" => "error",
            _ => "info",
        };
        self.push(level, message.into(), None);
    }

    /// 记一条日志。明细按 Vue `appendStartupLog` 去掉空值，用「 · 」连接。
    pub(crate) fn log(&mut self, level: &'static str, message: &str, context: &[(&str, Option<&str>)]) {
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

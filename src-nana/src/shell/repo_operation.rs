//! 任务弹层的「仓库操作」行：对应 Vue `composables/workspace/tasks.ts` 的 `operationProgress`。
//!
//! 只有一个槽位，最后开始的操作占着它。开始一次操作拿到编号，之后的更新、完成和取消都带着编号；
//! 编号已经不是槽位里的那一个（被后来的操作顶掉）时什么也不做，和 Vue `operationProgressId` 一样。
//! 等待时每 [`CLIMB_INTERVAL_MS`] 往上爬一次：35 以下加 5、70 以下加 3、其余加 1，不超过
//! [`CLIMB_CEILING`]；完成时停在 100%，过 [`FINISH_LINGER_MS`] 收起；取消立刻收起。
//!
//! 有 `&mut ShellViewModel` 的归约直接调槽位；文件、工作台和侧栏这些只拿得到自己状态的子状态机把请求
//! 排进 [`OperationRequests`]，归约结束时由壳层按顺序交给槽位（[`RepositoryOperation::apply`]）。
//! 爬升和收起的时钟由宿主在准备帧里拨（[`RepositoryOperation::tick`]），和别的计时器一样一帧 16 ms。

use super::tree_sync::now_ms;

/// Vue `setInterval(.., 220)`：等待中的进度隔多久往上爬一次。
pub const CLIMB_INTERVAL_MS: u64 = 220;
/// Vue `finishOperationProgress` 停在 100% 以后、收起之前的停留。
pub const FINISH_LINGER_MS: u64 = 180;
/// Vue `startOperationProgress` 的默认 `ceiling`：等待中最多爬到这里。
pub const CLIMB_CEILING: f64 = 88.0;

/// 仓库操作行显示的内容。
#[derive(Clone, Debug, PartialEq)]
pub struct OperationProgress {
    /// 操作种类，例如「加载资源库」「读取目录」。
    pub label: String,
    /// 当前阶段。
    pub detail: String,
    /// 百分比，0–100。
    pub value: f64,
    /// 不确定进度：不显示百分比，进度条呼吸。
    pub indeterminate: bool,
    /// 上一次变化的墙钟毫秒。任务弹层按它和运行中的任务一起排先后（Vue 每次重算都取 `Date.now()`）。
    pub updated_at_ms: i64,
}

/// 一次操作的编号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationId(u64);

/// 开始一次操作：种类、阶段、起始百分比和是否不确定（Vue `startOperationProgress` 的参数）。
#[derive(Clone, Debug, PartialEq)]
pub struct OperationStart {
    pub label: String,
    pub detail: String,
    pub initial: f64,
    pub indeterminate: bool,
}

impl OperationStart {
    pub fn new(label: impl Into<String>, detail: impl Into<String>, initial: f64) -> Self {
        Self { label: label.into(), detail: detail.into(), initial, indeterminate: false }
    }

    /// 标成不确定进度（Vue `{ indeterminate: true }`）。
    pub fn indeterminate(mut self) -> Self {
        self.indeterminate = true;
        self
    }
}

/// 更新进行中的操作，只改给了的字段（Vue `updateOperationProgress` 的补丁）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OperationPatch {
    pub detail: Option<String>,
    pub value: Option<f64>,
    pub indeterminate: Option<bool>,
}

impl OperationPatch {
    /// 走到下一个阶段：阶段说明和百分比一起换。
    pub fn step(detail: impl Into<String>, value: f64) -> Self {
        Self { detail: Some(detail.into()), value: Some(value), indeterminate: None }
    }

    /// 同时改不确定标记。
    pub fn with_indeterminate(mut self, indeterminate: bool) -> Self {
        self.indeterminate = Some(indeterminate);
        self
    }
}

/// 槽位里正在显示的操作。
#[derive(Clone, Debug)]
struct Running {
    id: u64,
    progress: OperationProgress,
    /// 下一次往上爬的时刻。完成以后不再爬。
    next_climb_ms: Option<u64>,
    /// 完成以后收起的时刻。
    hide_at_ms: Option<u64>,
}

/// 「仓库操作」的槽位。
#[derive(Clone, Debug, Default)]
pub struct RepositoryOperation {
    /// 发过的编号，下一次开始用它加一。
    issued: u64,
    /// 准备帧拨的时钟（毫秒）。
    clock_ms: u64,
    current: Option<Running>,
}

impl RepositoryOperation {
    /// 开始一次操作，顶掉槽位里原来的那个。返回这次操作的编号。
    pub fn start(&mut self, start: OperationStart) -> OperationId {
        self.issued += 1;
        self.current = Some(Running {
            id: self.issued,
            progress: OperationProgress {
                label: start.label,
                detail: start.detail,
                value: clamp_percent(start.initial),
                indeterminate: start.indeterminate,
                updated_at_ms: now_ms(),
            },
            next_climb_ms: Some(self.clock_ms + CLIMB_INTERVAL_MS),
            hide_at_ms: None,
        });
        OperationId(self.issued)
    }

    /// 更新这次操作。编号已经被顶掉时不改，返回是否改了。
    pub fn update(&mut self, id: OperationId, patch: OperationPatch) -> bool {
        let Some(running) = self.running_mut(id) else {
            return false;
        };
        let progress = &mut running.progress;
        if let Some(detail) = patch.detail {
            progress.detail = detail;
        }
        if let Some(value) = patch.value {
            progress.value = clamp_percent(value);
        }
        if let Some(indeterminate) = patch.indeterminate {
            progress.indeterminate = indeterminate;
        }
        progress.updated_at_ms = now_ms();
        true
    }

    /// 完成：停止爬升，停在 100%，[`FINISH_LINGER_MS`] 以后收起。编号已经被顶掉时不改。
    pub fn finish(&mut self, id: OperationId) -> bool {
        let clock = self.clock_ms;
        let Some(running) = self.running_mut(id) else {
            return false;
        };
        running.next_climb_ms = None;
        running.hide_at_ms = Some(clock + FINISH_LINGER_MS);
        running.progress.value = 100.0;
        running.progress.indeterminate = false;
        running.progress.updated_at_ms = now_ms();
        true
    }

    /// 取消：立刻收起。编号已经被顶掉时不改。
    pub fn cancel(&mut self, id: OperationId) -> bool {
        if self.running_mut(id).is_none() {
            return false;
        }
        self.current = None;
        true
    }

    /// 把子状态机排下的请求按顺序落到槽位上：开始记下编号，更新、完成和取消用它。
    pub fn apply(&mut self, requests: &mut OperationRequests) {
        for request in std::mem::take(&mut requests.queue) {
            match request {
                Request::Start(start) => requests.id = Some(self.start(start)),
                Request::Update(patch) => {
                    if let Some(id) = requests.id {
                        self.update(id, patch);
                    }
                }
                Request::Finish => {
                    if let Some(id) = requests.id.take() {
                        self.finish(id);
                    }
                }
                Request::Cancel => {
                    if let Some(id) = requests.id.take() {
                        self.cancel(id);
                    }
                }
            }
        }
    }

    /// 准备帧拨时钟：到点往上爬，完成停留到点收起。返回槽位显示的内容有没有变。
    ///
    /// 算法：每过一个 [`CLIMB_INTERVAL_MS`] 爬一级，增量看爬之前的值（35 以下 5、70 以下 3、其余 1），
    /// 到 [`CLIMB_CEILING`] 停住；已经被更新到上限以上的值不往回压（Vue 的 `Math.min` 会压回 88，
    /// 但它的更新和完成总在同一拍里，看不到这一步）。
    pub fn tick(&mut self, step_ms: u64) -> bool {
        self.clock_ms = self.clock_ms.saturating_add(step_ms);
        let clock = self.clock_ms;
        let Some(running) = self.current.as_mut() else {
            return false;
        };
        if running.hide_at_ms.is_some_and(|hide_at| clock >= hide_at) {
            self.current = None;
            return true;
        }
        let mut changed = false;
        while let Some(due) = running.next_climb_ms.filter(|due| clock >= *due) {
            running.next_climb_ms = Some(due + CLIMB_INTERVAL_MS);
            let value = running.progress.value;
            if value >= CLIMB_CEILING {
                continue;
            }
            let increment = if value < 35.0 {
                5.0
            } else if value < 70.0 {
                3.0
            } else {
                1.0
            };
            running.progress.value = (value + increment).min(CLIMB_CEILING);
            changed = true;
        }
        if changed {
            running.progress.updated_at_ms = now_ms();
        }
        changed
    }

    /// 还要准备帧接着拨：在爬，或者等着收起。到了上限、只剩爬升的操作也算，收起靠完成或取消。
    pub fn pending(&self) -> bool {
        self.current.as_ref().is_some_and(|running| {
            running.hide_at_ms.is_some() || running.next_climb_ms.is_some() && running.progress.value < CLIMB_CEILING
        })
    }

    /// 槽位现在显示的操作。
    pub fn shown(&self) -> Option<&OperationProgress> {
        self.current.as_ref().map(|running| &running.progress)
    }

    /// `id` 还占着槽位。
    pub fn is_current(&self, id: OperationId) -> bool {
        self.current.as_ref().is_some_and(|running| running.id == id.0)
    }

    fn running_mut(&mut self, id: OperationId) -> Option<&mut Running> {
        self.current.as_mut().filter(|running| running.id == id.0)
    }
}

/// 子状态机排给槽位的请求。
#[derive(Clone, Debug, PartialEq)]
enum Request {
    Start(OperationStart),
    Update(OperationPatch),
    Finish,
    Cancel,
}

/// 只拿得到自己状态的子状态机用它记下对仓库操作行的请求，归约结束时由壳层交给槽位。
/// 记着这个子状态机最近一次开始的操作编号：之后的更新、完成和取消都落到这次操作上。
#[derive(Clone, Debug, Default)]
pub struct OperationRequests {
    queue: Vec<Request>,
    id: Option<OperationId>,
}

impl OperationRequests {
    pub fn start(&mut self, start: OperationStart) {
        self.queue.push(Request::Start(start));
    }

    pub fn update(&mut self, patch: OperationPatch) {
        self.queue.push(Request::Update(patch));
    }

    pub fn finish(&mut self) {
        self.queue.push(Request::Finish);
    }

    pub fn cancel(&mut self) {
        self.queue.push(Request::Cancel);
    }

    /// 这个子状态机开始过、还没完成或取消的操作（含还没交给槽位的）。
    pub fn active(&self) -> bool {
        let mut active = self.id.is_some();
        for request in &self.queue {
            match request {
                Request::Start(_) => active = true,
                Request::Finish | Request::Cancel => active = false,
                Request::Update(_) => {}
            }
        }
        active
    }
}

fn clamp_percent(value: f64) -> f64 {
    if value.is_finite() { value.clamp(0.0, 100.0) } else { 0.0 }
}

#[cfg(test)]
#[path = "repo_operation_tests.rs"]
mod tests;

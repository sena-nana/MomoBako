//! 全局状态区：最近一次失败和它的来源，对齐 Vue `WorkspaceSidebarStatus` 读的全局 `error`，
//! 以及没有失败时的忙碌行（Vue `isBusy`）和同步进度。显示在侧栏顶部，见 `sidebar_view::status_line`。
//!
//! **记什么。** 只有一个槽位，后记的失败顶替先记的。记的是没有就近显示的失败：
//! - 各领域在失败处直接 [`StatusState::fail`]：宿主打开、定位、拖出，最小化到托盘，资源库列表和摘要，
//!   文件区以外发起的目录读取，素材详情，元数据保存、撤销、重做，播放集读写，设置和系统日志，
//!   智能文件夹删除，添加资源库时的文件夹选择；
//! - 播放控制和播放集成员这类写在播放器内部的失败，由播放器排进待取的失败，归约结束时取走；
//! - 文件操作（`files.error`）、文件夹树（`sidebar.tree_error`）和智能文件夹（`sidebar.smart_error`）
//!   三个领域字段：新写了一次就记一次，字段清掉时它记下的那一条一起清。文件操作失败本来就写在文件列表里，
//!   列表显示着的时候状态区不重复显示（[`StatusLine::project`]）。
//!
//! 对话框里的错误行、搜索面板、缺失仓库页、空库页、启动页、插件面板、导出对话框、预览和播放条这些已经
//! 就近显示的失败不进这里。
//!
//! **什么时候清。** 照 Vue `error.value = null` 的时机：用户开始一个会写全局错误的操作时清掉上一次失败，
//! 成功不专门清。开始的迹象有两种（[`starts_operation`]、[`Activity`]）：打开、定位、拖出，换仓库、
//! 刷新资源库列表、重试启动、打开智能文件夹这几条消息；以及读目录、文件变更、刷新文件夹树、选中别的文件、
//! 保存元数据、搜索、导出、智能文件夹增改删、插件操作和执行仓库动作这些操作的忙碌标志由假变真。后台的
//! 静默刷新不算。同一次归约里新记的失败不会被这次清掉。
//!
//! 没有失败时依次是忙碌行和同步进度（文件夹树刷新时的扫描、写入、刷新三档）。

use super::input::InputMessage;
use super::sidebar::SidebarMessage;
use super::view_part_primary::RouteKey;
use super::{ShellMessage, ShellViewModel};

/// 失败来源：哪一类操作失败。状态区只显示文案，来源决定就近显示时让不让位，日志和测试据此认出是谁。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureSource {
    /// 用系统程序打开、在文件管理器里定位、把文件拖出窗口。
    Host,
    /// 最小化到托盘。
    Tray,
    /// 资源库列表、摘要，以及添加资源库时的文件夹选择。
    Repository,
    /// 文件区以外发起的目录读取。
    Directory,
    /// 侧栏文件夹树读取。
    FolderTree,
    /// 文件夹树的「刷新」：同步仓库，再重读摘要、硬链接候选和目录树。
    Sync,
    /// 智能文件夹的读取、查询和删除。
    SmartFolder,
    /// 素材详情读取。
    Asset,
    /// 元数据保存、撤销和重做。
    Metadata,
    /// 文件操作：`files.error`，在文件列表里就近显示。
    Files,
    /// 播放集和条目的读写、播放器类型。
    Playlist,
    /// 播放控制和播放集成员。
    Player,
    /// 应用设置和系统服务状态。
    Settings,
    /// 系统日志。
    Logs,
}

/// 记下的一次失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub source: FailureSource,
    /// 状态区显示的文案：说明失败的对象，后面接系统返回的原因。
    pub message: String,
    /// 第几次记的失败，判断是不是这次归约里新记的。
    seq: u64,
}

/// 被盯着的领域错误字段：新写一次就记一次失败，清掉时它记下的那一条一起清。
const WATCHED: [FailureSource; 3] = [FailureSource::Files, FailureSource::FolderTree, FailureSource::SmartFolder];

/// 全局状态区的状态。
#[derive(Clone, Debug, Default)]
pub struct StatusState {
    failure: Option<Failure>,
    /// 一共记过几次失败。
    seq: u64,
    /// [`WATCHED`] 各字段上次看到的值。
    watched: [String; 3],
}

impl StatusState {
    /// 状态区现在的失败。
    pub fn failure(&self) -> Option<&Failure> {
        self.failure.as_ref()
    }

    /// 一共记过几次失败。归约开始时记下，用来分清哪些失败是这次归约里新记的。
    pub(super) fn seq(&self) -> u64 {
        self.seq
    }

    /// 记下一次失败，顶替上一次。空文案不记。
    pub(crate) fn fail(&mut self, source: FailureSource, message: impl Into<String>) {
        let message = message.into();
        if message.trim().is_empty() {
            eprintln!("Nana 状态区收到空的失败文案，来源 {source:?}");
            return;
        }
        eprintln!("Nana 状态区记下失败（{source:?}）：{message}");
        self.seq += 1;
        self.failure = Some(Failure { source, message, seq: self.seq });
    }

    /// 清掉第 `seq` 次及以前记的失败；之后新记的留着。
    fn clear_through(&mut self, seq: u64) {
        if self.failure.as_ref().is_some_and(|failure| failure.seq <= seq) {
            self.failure = None;
        }
    }

    /// `source` 的操作这次成功了：它记下的失败作废。只给没有 Vue 对应、成功就该收起提示的托盘用。
    pub(crate) fn resolve(&mut self, source: FailureSource) {
        self.clear_source(source);
    }

    /// 清掉来自 `source` 的失败。
    fn clear_source(&mut self, source: FailureSource) {
        if self.failure.as_ref().is_some_and(|failure| failure.source == source) {
            self.failure = None;
        }
    }
}

/// 会写全局错误的操作开始时置真的忙碌标志，以及正在看的文件。标志由假变真、换了文件，就是用户开始了
/// 一个操作（Vue 在这些操作开始时 `error.value = null`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Activity {
    busy: [bool; 9],
    /// 正在看的文件：换成别的文件对应 Vue `selectAsset`。
    selection: Option<String>,
}

impl Activity {
    pub(super) fn of(model: &ShellViewModel) -> Self {
        Self {
            busy: [
                // 读目录（非静默）、文件变更、刷新文件夹树（同步仓库）。
                model.files.loading,
                model.files.mutating,
                model.tree_sync.running(),
                // 保存元数据、搜索、导出。
                model.inspect.saving(),
                model.inspect.searching,
                model.files.export.busy,
                // 智能文件夹的增改删。
                model.sidebar.smart_draft.busy,
                // 插件操作、执行仓库动作。结构更新后台重读动作列表不算：Vue 那条路径顺带清掉错误，
                // 用户还没看到的失败会被后台刷新抹掉，这里不照抄。
                model.admin.managing,
                model.admin.actions_running,
            ],
            selection: model.inspect.target_path.clone(),
        }
    }

    /// 和 `before` 比，有操作开始了。
    fn started_since(&self, before: &Activity) -> bool {
        self.busy.iter().zip(before.busy).any(|(now, was)| *now && !was)
            || (self.selection.is_some() && self.selection != before.selection)
    }
}

/// 不靠忙碌标志、由消息本身表示的操作开始：打开、定位、拖出，换仓库、刷新资源库列表、重试启动和打开
/// 智能文件夹。打开、定位和拖出照 Vue 先过守卫（有仓库、有路径、能拖出），守卫拦下的请求不清。
/// 刷新文件夹树看忙碌标志（[`Activity`]），被拦下的点击不清。
pub(super) fn starts_operation(message: &ShellMessage) -> bool {
    let filled = |text: &str| !text.trim().is_empty();
    match message {
        ShellMessage::Input(InputMessage::OpenEntry { has_repo, absolute_path }) => *has_repo && filled(absolute_path),
        ShellMessage::Input(InputMessage::RevealEntry { absolute_path }) => filled(absolute_path),
        ShellMessage::Input(InputMessage::OpenExternalUrl { url }) => filled(url),
        ShellMessage::Input(InputMessage::StartExternalDrag { paths, trash, backend_kind, .. }) => {
            !*trash && backend_kind == "filesystem" && paths.iter().any(|path| filled(path))
        }
        ShellMessage::SelectWorkspaceRepository(_)
        | ShellMessage::Refresh
        | ShellMessage::StartupRetry
        | ShellMessage::Sidebar(SidebarMessage::OpenSmartFolder(_)) => true,
        _ => false,
    }
}

impl ShellViewModel {
    /// 归约结束时整理状态区：这次归约里开始了操作，清掉这次以前记的失败；再收下这次新写的失败。
    pub(super) fn settle_status(&mut self, before: &Activity, seq: u64, started: bool) {
        if started || Activity::of(self).started_since(before) {
            self.status.clear_through(seq);
        }
        self.observe_failures();
    }

    /// 收下归约以外也可能写的失败：播放器排着的失败，以及被盯着的领域错误字段的变化。返回状态区有没有变。
    /// 归约结束时调；宿主把服务结果和请求写回以后、同步视图之前也调一次。
    pub(crate) fn observe_failures(&mut self) -> bool {
        let before = self.status.failure.clone();
        for message in self.player.take_failures() {
            self.status.fail(FailureSource::Player, message);
        }
        let current = [self.files.error.clone(), self.sidebar.tree_error.clone(), self.sidebar.smart_error.clone()];
        for (index, now) in current.into_iter().enumerate() {
            if self.status.watched[index] == now {
                continue;
            }
            if now.is_empty() {
                self.status.clear_source(WATCHED[index]);
            } else {
                self.status.fail(WATCHED[index], now.clone());
            }
            self.status.watched[index] = now;
        }
        self.status.failure != before
    }
}

/// 状态区显示什么。顺序照 Vue：失败 > 忙碌 > 同步进度。同步进度来自文件夹树的「刷新」（`tree_sync.rs`），
/// 启动时的同步进度在启动页上。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StatusLine {
    Hidden,
    /// 红色错误条：最近一次失败的文案。
    Failure(String),
    /// 「正在同步仓库状态」：读资源库列表、读仓库摘要或读素材详情时。
    Busy,
    /// 同步进度：这一步的文案和百分比（扫描、写入、刷新三档）。
    Progress { label: String, percent: u8 },
}

impl StatusLine {
    /// 从 ViewModel 取状态区的投影。最近一次失败正在别处就近显示时让位给忙碌行和同步进度。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        if let Some(failure) = model.status.failure()
            && !shown_in_place(model, failure.source)
        {
            return Self::Failure(failure.message.clone());
        }
        if busy(model) {
            return Self::Busy;
        }
        match model.tree_sync.progress.shown() {
            Some((label, percent)) => Self::Progress { label: label.to_string(), percent },
            None => Self::Hidden,
        }
    }
}

/// 失败已经在面板里就近显示着：文件操作失败写在文件列表的状态框里，列表显示着（文件路由、不在预览页）时
/// 状态区不重复显示。
fn shown_in_place(model: &ShellViewModel, source: FailureSource) -> bool {
    source == FailureSource::Files && RouteKey::of(model) == RouteKey::Files && !super::files_view::previewing(model)
}

/// Vue `isBusy`：在读资源库列表、仓库摘要或素材详情。启动阶段由启动页显示进度，不算在这里。
pub(crate) fn busy(model: &ShellViewModel) -> bool {
    let workspace = &model.workspace;
    workspace.list_loading || workspace.snapshot_loading || model.inspect.detail_loading()
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;

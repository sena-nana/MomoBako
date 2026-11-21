//! 标签菜单、搜索延时和元数据自动保存。
//!
//! 标签菜单铺在元数据的标签区里，开合只是一个标志，位置由卡片排版决定，和窗口大小无关。
//! 搜索等待 250 毫秒，元数据等待 260 毫秒。两个延时不要帧：宿主在每个回调开头把流逝的时间交给时钟，
//! 按最早的到期时刻要 NanaUI 到点叫醒（`frame_clock.rs`）。

use super::super::player::ClockStep;
use super::{InspectMessage, InspectState};

const SEARCH_DELAY_MS: u64 = 250;
const METADATA_DELAY_MS: u64 = 260;

/// 实况搜索和元数据的到期时刻。`clock_ms` 是宿主交过来的计时器时间。
#[derive(Clone, Debug, Default)]
pub(super) struct LiveTimers {
    clock_ms: u64,
    search_due_ms: Option<u64>,
    metadata_due_ms: Option<u64>,
}

pub(super) enum ClockHint {
    None,
    Search,
    Metadata,
    ClearMetadata,
}

/// 这条消息会不会武装定时器。空标签和非法字段在归约里提前返回，不会走到这里。
pub(super) fn clock_hint(message: &InspectMessage) -> ClockHint {
    match message {
        InspectMessage::SetQuery(_) => ClockHint::Search,
        InspectMessage::SetComment(_)
        | InspectMessage::SetLink(_)
        | InspectMessage::SetRating(_)
        | InspectMessage::AddTag(_)
        | InspectMessage::RemoveTag(_) => ClockHint::Metadata,
        InspectMessage::SaveMetadata => ClockHint::ClearMetadata,
        _ => ClockHint::None,
    }
}

impl InspectState {
    pub fn tag_menu_open(&self) -> bool {
        self.tag_menu
    }

    pub fn close_tag_menu(&mut self) {
        self.tag_menu = false;
    }

    /// 打开标签菜单。虚拟素材、保存中或没有素材时不开。
    pub fn open_tag_menu(&mut self) {
        if self.virtual_asset || self.saving || self.asset_id.is_none() {
            eprintln!("Nana 当前不能打开标签菜单");
            return;
        }
        self.tag_menu = true;
    }

    pub(super) fn apply_clock(&mut self, hint: ClockHint) {
        match hint {
            ClockHint::None => {}
            ClockHint::Search => {
                self.timers.search_due_ms =
                    Some(self.timers.clock_ms.saturating_add(SEARCH_DELAY_MS));
            }
            ClockHint::ClearMetadata => self.timers.clear_metadata(),
            ClockHint::Metadata => {
                if self.can_edit() && self.dirty() && self.conflict.is_empty() {
                    self.timers.metadata_due_ms =
                        Some(self.timers.clock_ms.saturating_add(METADATA_DELAY_MS));
                } else {
                    self.timers.clear_metadata();
                }
            }
        }
    }

    /// 换选之前先把已武装的元数据写出去，避免写到下一个素材。
    pub(super) fn flush_pending_metadata(&mut self) {
        let armed = self.timers.metadata_due_ms.take().is_some();
        if armed && self.dirty() && self.can_edit() && self.conflict.is_empty() {
            self.save_metadata();
        }
    }

    /// 时钟走 `step` 毫秒，不触发。宿主在归约之前调，归约里武装的延时从这一刻起算。
    pub(crate) fn pass_time(&mut self, step: u64) {
        self.timers.clock_ms = self.timers.clock_ms.saturating_add(step);
    }

    /// 最早到期的延时离现在还有多少毫秒，没有在等的延时时为 `None`。
    pub(crate) fn timer_due_in(&self) -> Option<u64> {
        let clock = self.timers.clock_ms;
        [self.timers.search_due_ms, self.timers.metadata_due_ms]
            .into_iter()
            .flatten()
            .map(|due| due.saturating_sub(clock))
            .min()
    }

    /// 前进时钟，取走到期的延时。到期后由调用方再归约搜索或保存。
    pub(super) fn advance_timers(&mut self, step: u64) -> (bool, bool) {
        self.pass_time(step);
        if !self.timers.pending() {
            return (false, false);
        }
        let search = self
            .timers
            .search_due_ms
            .is_some_and(|due| self.timers.clock_ms >= due);
        let metadata = self
            .timers
            .metadata_due_ms
            .is_some_and(|due| self.timers.clock_ms >= due);
        if search {
            self.timers.search_due_ms = None;
        }
        if metadata {
            self.timers.metadata_due_ms = None;
        }
        (search, metadata)
    }

    /// 前进并在到期时归约。
    pub(super) fn poll_own(&mut self, step: u64, writable: bool, repo: Option<&str>) -> bool {
        let (search, metadata) = self.advance_timers(step);
        if search {
            self.reduce(writable, repo, InspectMessage::RunSearch);
        }
        if metadata {
            self.reduce(writable, repo, InspectMessage::SaveMetadata);
        }
        search || metadata
    }
}

impl LiveTimers {
    fn pending(&self) -> bool {
        self.search_due_ms.is_some() || self.metadata_due_ms.is_some()
    }

    pub(super) fn clear_metadata(&mut self) {
        self.metadata_due_ms = None;
    }
}

/// 准备帧或计时器叫醒时拨一次，返回计时器或播放时钟有没有动。搜索和自动保存的延时按 `step.timer_ms` 走，
/// 播放时钟（音视频进度、图片停留）按 `step.motion_ms` 走，见 `frame_clock.rs`。
///
/// 搜索、保存这类计时器触发和播放切项会改界面结构，标脏让随后整体同步；只是播放进度往前走时
/// 不标脏，进度和时间由热信号跟上。
pub(crate) fn poll_timers(
    model: &mut super::super::ShellViewModel,
    step: crate::frame_clock::FrameStep,
) -> bool {
    let writable = model
        .workspace
        .active_repository()
        .is_some_and(|repository| {
            super::super::files::repository_is_writable(
                &repository.status,
                &repository.capabilities,
            )
        });
    let repo_id = model.workspace.active_repo_id.clone();
    let fired = model
        .inspect
        .poll_own(step.timer_ms, writable, repo_id.as_deref());
    let playback = super::bridge::advance_playback(model, step.motion_ms);
    if fired || playback == ClockStep::Changed {
        model.mark_surface_dirty();
    }
    fired || playback != ClockStep::Idle
}

#[cfg(test)]
mod tests {
    use super::InspectState;

    /// 有素材才开得了；保存中和虚拟素材不开。
    #[test]
    fn tag_menu_opens_only_for_an_editable_asset() {
        let mut state = InspectState::default();
        state.open_tag_menu();
        assert!(!state.tag_menu_open(), "没有素材不开");
        state.asset_id = Some("asset".into());
        state.saving = true;
        state.open_tag_menu();
        assert!(!state.tag_menu_open(), "保存中不开");
        state.saving = false;
        state.open_tag_menu();
        assert!(state.tag_menu_open());
        state.close_tag_menu();
        assert!(!state.tag_menu_open());
    }
}

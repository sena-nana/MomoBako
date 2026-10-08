//! 标签菜单、搜索延时和元数据自动保存。
//!
//! 菜单坐标夹取和侧栏弹层用同一条规则。搜索等待 250 毫秒，元数据等待 260 毫秒。
//! 时钟只在还有到期项时前进，避免空闲时把窗口钉在连续帧上。

use super::{InspectMessage, InspectState};

const SEARCH_DELAY_MS: u64 = 250;
const METADATA_DELAY_MS: u64 = 260;

/// 实况搜索和元数据的到期时刻。`clock_ms` 只在有到期项时按帧累加。
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
        | InspectMessage::RemoveTag(_)
        | InspectMessage::SetCustom { .. }
        | InspectMessage::RemoveCustom(_) => ClockHint::Metadata,
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

    pub fn open_tag_menu(&mut self, x: f32, y: f32, width: f32, height: f32, viewport_w: f32, viewport_h: f32) {
        if self.virtual_asset || self.saving || self.asset_id.is_none() {
            eprintln!("Nana 当前不能打开标签菜单");
            return;
        }
        let (x, y) = super::super::sidebar::clamp_anchored(x, y, width, height, viewport_w, viewport_h);
        self.tag_menu = true;
        self.tag_menu_x = x;
        self.tag_menu_y = y;
    }

    pub fn dismiss_tag_menu_outside(&mut self, inside: bool) {
        if self.tag_menu && !inside {
            self.tag_menu = false;
        }
    }

    pub(super) fn apply_clock(&mut self, hint: ClockHint) {
        match hint {
            ClockHint::None => {}
            ClockHint::Search => {
                self.timers.search_due_ms = Some(self.timers.clock_ms.saturating_add(SEARCH_DELAY_MS));
            }
            ClockHint::ClearMetadata => self.timers.clear_metadata(),
            ClockHint::Metadata => {
                if self.can_edit() && self.dirty() && self.conflict.is_empty() {
                    self.timers.metadata_due_ms = Some(self.timers.clock_ms.saturating_add(METADATA_DELAY_MS));
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

    /// 前进时钟。到期后由调用方再归约搜索或保存。
    pub(super) fn advance_timers(&mut self, step: u64) -> (bool, bool) {
        if !self.timers.pending() {
            return (false, false);
        }
        self.timers.clock_ms = self.timers.clock_ms.saturating_add(step);
        let search = self.timers.search_due_ms.is_some_and(|due| self.timers.clock_ms >= due);
        let metadata = self.timers.metadata_due_ms.is_some_and(|due| self.timers.clock_ms >= due);
        if search {
            self.timers.search_due_ms = None;
        }
        if metadata {
            self.timers.metadata_due_ms = None;
        }
        (search, metadata)
    }

    /// 单测用：前进并在到期时归约，不经过窗口准备。
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

    pub(crate) fn timers_pending(&self) -> bool {
        self.timers.pending()
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

/// 准备阶段拨一帧。触发搜索或保存时标脏，让随后的挂载画上新结果。
pub(crate) fn poll_timers(model: &mut super::super::ShellViewModel) -> bool {
    let writable = model.workspace.active_repository().is_some_and(|repository| {
        super::super::files::repository_is_writable(&repository.status, &repository.capabilities)
    });
    let repo_id = model.workspace.active_repo_id.clone();
    let fired = model.inspect.poll_own(16, writable, repo_id.as_deref());
    let video = super::bridge::advance_playback(model, 16);
    if fired || video {
        model.surface_dirty = true;
    }
    fired || video
}

#[cfg(test)]
mod tests {
    use super::InspectState;

    #[test]
    fn tag_menu_clamps_like_vue_and_closes_on_an_outside_click() {
        let mut state = InspectState::default();
        state.asset_id = Some("asset".into());
        state.open_tag_menu(-10.0, 900.0, 180.0, 80.0, 400.0, 300.0);
        assert!(state.tag_menu_open());
        assert_eq!(state.tag_menu_x, 4.0);
        assert_eq!(state.tag_menu_y, 216.0);
        state.dismiss_tag_menu_outside(true);
        assert!(state.tag_menu_open());
        state.dismiss_tag_menu_outside(false);
        assert!(!state.tag_menu_open());
    }
}

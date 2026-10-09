//! 播放集路由（常驻）：播放表面，以及不能播放的条目。名称和当前目录不在这一页另开输入。
//!
//! 分支只在进入路由时建一次，之后同步只写 [`PlaylistRouteSignals`]：播放集页的页眉、条目和顺序
//! （`player_playlist_page.rs`），播放表面开不开，以及「不可播放项目」那一行。首页外框上方是常驻筛选栏。
//!
//! 播放条仍是播放器模块的旧视图，在这里登记成岛（见 `view_part_primary::Island`）：占位节点在播放集
//! 面板最后，点开了播放集才有内容；版本按播放条真正读到的值算（`player_view::bar_stamp`）。

use nana_ui::runtime::view::{node_ref, signal, text, widget, AnyView, IntoView, NodeRef, Signal};
use nana_ui::runtime::{LengthSpec, Stack};

use super::inspect_search_view::{resident_filter_bar, FilterBarSignals};
use super::player_view::playlist::{PlaylistPageSignals, PlaylistPageView};
use super::route_home::{home_page, home_scroll};
use super::view_part_primary::Island;
use super::ShellViewModel;

/// 播放集路由的常驻信号和播放条岛的占位节点。
#[derive(Clone, Copy)]
pub(crate) struct PlaylistRouteSignals {
    page: PlaylistPageSignals,
    /// 播放表面：只在播放集面板显示。停在播放集页、面板是别的时只剩外框。
    surface: Signal<bool>,
    /// 不能播放的条目，「文件名: 原因」用「 · 」连起来；空时不显示那一行。
    unplayable: Signal<String>,
    /// 播放条岛的占位节点。
    bar: NodeRef,
}

impl PlaylistRouteSignals {
    /// 在主区块的常驻作用域里建信号，初值是这一刻的投影。
    pub(crate) fn new(model: &ShellViewModel) -> Self {
        Self {
            page: PlaylistPageSignals::new(PlaylistPageView::project(model)),
            surface: signal(model.player_surface_visible()),
            unplayable: signal(model.playlist_item_status.clone()),
            bar: node_ref(),
        }
    }

    /// 写入投影，每组只写变了的。
    pub(crate) fn write(&self, model: &ShellViewModel) {
        self.page.write(PlaylistPageView::project(model));
        self.surface.try_set_if_changed(model.player_surface_visible());
        self.unplayable.try_set_if_changed(model.playlist_item_status.clone());
    }
}

/// 播放集路由的岛：播放集面板底部的播放条。
pub(super) fn islands(signals: PlaylistRouteSignals) -> Vec<Island> {
    vec![Island { slot: signals.bar, build: player_bar, stamp: super::player_view::bar_stamp }]
}

/// 播放集面板底部的播放条：显示着播放表面、点开了播放集才有。
fn player_bar(model: &ShellViewModel) -> Option<AnyView> {
    (model.player_surface_visible() && model.player.listed.is_some()).then(|| super::player_view::hosted_bar(model))
}

/// 播放集路由的分支：首页外框里常驻筛选栏在上，下面是纵向滚动的播放集页和「不可播放项目」。
pub(super) fn view(filter: FilterBarSignals, signals: PlaylistRouteSignals) -> AnyView {
    let unplayable = signals.unplayable;
    // 占位节点排成和原来直接放在面板里的播放条一样：宽度随面板，高度随内容。
    let bar = widget(Stack::column(0.0).grow(0.0).shrink(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .node_ref(signals.bar)
        .key("playlist-player-slot")
        .into_any();
    let status = text(move || unplayable.with(|status| format!("不可播放项目：{status}")))
        .visible(move || unplayable.with(|status| !status.is_empty()))
        .key("playlist-item-status")
        .into_any();
    let page = super::player_view::playlist::view(signals.page, signals.surface, bar);
    let body = super::workbench::page(vec![page, status]);
    home_page(Some(resident_filter_bar(filter)), home_scroll("workspace-page-scroll-HasRepository-Playlist", body))
}

#[cfg(test)]
#[path = "route_playlists_tests.rs"]
mod tests;

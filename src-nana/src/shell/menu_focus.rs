//! 键盘打开的菜单：把焦点放进菜单的首项或末项。菜单关掉以后焦点还给打开它的控件，见 `focus_return.rs`。
//!
//! NanaUI 把菜单按钮（`has_popup`）上的 ArrowDown / ArrowUp、任意焦点上的 `ContextMenu` 键和 Shift+F10
//! 都派发成 `keyboard: true` 的 `SecondaryPress`。处理函数照常发打开菜单的消息，再发
//! [`ShellMessage::FocusMenu`]：归约确认菜单真的开着才记下这次请求，视图整体同步完以后照着聚焦
//! （`menu_focus_view.rs`）。ArrowDown 聚焦首项、ArrowUp 聚焦末项，菜单键和 Shift+F10 由应用挑，取首项；
//! 指针打开的菜单不动焦点。菜单项都在 [`MENU_KEYS`] 方向键焦点组里，上下键在项之间走、首尾相接。

use nana_ui::runtime::{RovingEdge, RovingFocusGroup, SecondaryPress};

use super::files::FileContext;
use super::sidebar::PopoverMode;
use super::view_part_primary::RouteKey;
use super::{ShellMessage, ShellViewModel};

/// 菜单项所在的方向键焦点组：竖排，越过两端回到另一端（WAI-ARIA 菜单）。
pub(crate) const MENU_KEYS: RovingFocusGroup = RovingFocusGroup::vertical().wrap(true);

/// 能从键盘打开、要把焦点放进去的菜单。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuSurface {
    /// 侧栏仓库头打开的切换弹层。
    RepositorySwitcher,
    /// 文件页工具条「导入」的菜单。
    Import,
    /// 文件条目的右键菜单。
    Entry,
    /// 侧栏文件夹行的右键菜单。
    Folder,
}

impl MenuSurface {
    /// 菜单项所在方向键焦点组的键，视图按它找到组节点。
    pub(crate) fn group_key(self) -> &'static str {
        match self {
            Self::RepositorySwitcher => "repository-switcher-popover",
            Self::Import => "file-import-menu",
            Self::Entry => "file-context-menu",
            Self::Folder => "folder-context-menu",
        }
    }

    /// 这个菜单现在开着，条件和各菜单视图显示出来的条件一致。
    pub(crate) fn is_open(self, model: &ShellViewModel) -> bool {
        match self {
            Self::RepositorySwitcher => model.sidebar.popover == PopoverMode::Switcher,
            Self::Import => {
                let ctx = FileContext::from_model(model);
                RouteKey::of(model) == RouteKey::Files
                    && !super::files_view::previewing(model)
                    && !ctx.trash
                    && !ctx.is_virtual()
                    && model.files.import_open
                    && model.files.can_import(&ctx)
            }
            Self::Entry => model.files.entry_menu.is_some(),
            Self::Folder => model.sidebar.folder_menu.is_some(),
        }
    }
}

/// 聚焦菜单的哪一端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuEdge {
    First,
    Last,
}

impl MenuEdge {
    /// 键盘要来的菜单聚焦哪一端：ArrowUp 是末项，ArrowDown、菜单键和 Shift+F10 是首项。指针的右键是 `None`。
    pub(crate) fn of_press(press: &SecondaryPress) -> Option<Self> {
        press.keyboard.then_some(match press.focus {
            Some(RovingEdge::End) => Self::Last,
            Some(RovingEdge::Start) | None => Self::First,
        })
    }

    pub(crate) fn roving(self) -> RovingEdge {
        match self {
            Self::First => RovingEdge::Start,
            Self::Last => RovingEdge::End,
        }
    }
}

/// 一次「把焦点放进菜单」的请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuFocusRequest {
    pub(crate) surface: MenuSurface,
    pub(crate) edge: MenuEdge,
}

/// 处理函数发的消息：菜单已经由前一条消息打开，焦点放进 `surface` 的 `edge` 那一端。
pub(crate) fn focus_menu(surface: MenuSurface, edge: MenuEdge) -> ShellMessage {
    ShellMessage::FocusMenu(MenuFocusRequest { surface, edge })
}

/// 最近一次请求和它的序号。视图记着照做过的序号，每个请求只照做一次。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MenuFocus {
    serial: u64,
    request: Option<MenuFocusRequest>,
}

impl MenuFocus {
    /// 最近一次请求和它的序号。
    pub(crate) fn latest(&self) -> Option<(u64, MenuFocusRequest)> {
        self.request.map(|request| (self.serial, request))
    }
}

/// 归约 [`ShellMessage::FocusMenu`]：菜单开着才记下；没打开（提交中、导入不可用、目标已不在列表里）时丢掉。
pub(crate) fn request(model: &mut ShellViewModel, request: MenuFocusRequest) {
    if !request.surface.is_open(model) {
        eprintln!("Nana 键盘要聚焦的菜单没有打开：{:?}", request.surface);
        return;
    }
    model.menu_focus.serial = model.menu_focus.serial.wrapping_add(1);
    model.menu_focus.request = Some(request);
}

#[cfg(test)]
#[path = "menu_focus_tests.rs"]
mod tests;

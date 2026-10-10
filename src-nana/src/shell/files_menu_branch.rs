//! 文件右键菜单里子菜单的展开状态（ViewModel 一侧）。
//!
//! 照 Vue `ContextMenuHost.vue` 的 `.ctx-menu__slot:hover > .ctx-menu__submenu` 和
//! `.ctx-menu__slot:focus-within > .ctx-menu__submenu`：指针在分组那一行或它的子菜单里，或者键盘焦点
//! 在里面，子菜单就展开；都离开就收起，没有延迟。视图只报告进出（`files_menu.rs`），这里记下指针和
//! 焦点各自所在的分组，算出展开的是哪一个。

/// 子菜单展开的来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchSource {
    /// 指针进出分组那一行和它的子菜单，对应 `:hover`。
    Pointer,
    /// 键盘焦点进出，对应 `:focus-within`。
    Focus,
}

/// 右键菜单里指针和键盘焦点各自所在的分组。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MenuBranch {
    hovered: Option<String>,
    focused: Option<String>,
}

impl MenuBranch {
    /// 展开着的分组：指针所在的优先，没有时是焦点所在的。
    ///
    /// Vue 两处各在一个分组时两个子菜单叠着显示，这里只显示指针那一个。
    pub(crate) fn open(&self) -> Option<&str> {
        self.hovered.as_deref().or(self.focused.as_deref())
    }

    /// 记下指针或焦点进出了 `branch`。
    ///
    /// 离开时只收起记着的那一个：从一个分组移到另一个分组，上一个的离开和下一个的进入谁先到都收得对。
    pub(crate) fn note(&mut self, source: BranchSource, branch: String, inside: bool) {
        let slot = match source {
            BranchSource::Pointer => &mut self.hovered,
            BranchSource::Focus => &mut self.focused,
        };
        if inside {
            *slot = Some(branch);
        } else if slot.as_deref() == Some(branch.as_str()) {
            *slot = None;
        }
    }
}

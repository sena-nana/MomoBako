//! Named slots of the shell composites, as views:
//!
//! ```ignore
//! widget(DesktopShell::from_model(model).title("Gallery"))
//!     .title_trailing(row().gap(6.0).children((search, theme)))
//!     .navigation(sidebar())
//!     .primary(page())
//! ```
//!
//! `AppShell`'s title bar, body and overlay and a `Workspace`'s regions are
//! slots the same way.
//!
//! Each slot is built before the shell and placed by its assembler once the
//! tree commits (`TypeBehavior::slot_assembler`), so a view never calls
//! `assemble_desktop_shell`. A slot's node stays for the view's life: what
//! changes inside it is a `when` / `each` / `dynamic` there, and whether a
//! region shows is the workspace model's.

use nana_ui_core::RegionId;

use super::{El, IntoView};
use crate::{AppShell, AppTitleBar, DesktopShell, Workspace};

impl<K> El<DesktopShell, K> {
    pub fn title_leading(self, view: impl IntoView) -> Self {
        self.slot(view, DesktopShell::title_leading)
    }

    pub fn title_center(self, view: impl IntoView) -> Self {
        self.slot(view, DesktopShell::title_center)
    }

    pub fn title_trailing(self, view: impl IntoView) -> Self {
        self.slot(view, DesktopShell::title_trailing)
    }

    pub fn primary(self, view: impl IntoView) -> Self {
        self.slot(view, DesktopShell::primary)
    }

    pub fn navigation(self, view: impl IntoView) -> Self {
        self.slot(view, DesktopShell::navigation)
    }

    pub fn navigation_footer(self, view: impl IntoView) -> Self {
        self.slot(view, DesktopShell::navigation_footer)
    }

    /// See [`DesktopShell::inspector`].
    pub fn inspector(self, view: impl IntoView) -> Self {
        self.slot(view, DesktopShell::inspector)
    }

    pub fn bottom(self, view: impl IntoView) -> Self {
        self.slot(view, DesktopShell::bottom)
    }

    /// See [`DesktopShell::region`].
    pub fn region(self, id: RegionId, view: impl IntoView) -> Self {
        self.slot(view, move |shell, content| shell.region(id, content))
    }

    /// See [`DesktopShell::overlay`].
    pub fn overlay(self, view: impl IntoView) -> Self {
        self.slot(view, DesktopShell::overlay)
    }

    /// See [`DesktopShell::status`].
    pub fn status(self, view: impl IntoView) -> Self {
        self.slot(view, DesktopShell::status)
    }
}

impl<K> El<AppShell, K> {
    /// The title bar; without one the shell makes its own.
    pub fn title_bar(self, view: impl IntoView) -> Self {
        self.slot(view, AppShell::title_bar)
    }

    /// The content under the title bar, filling the shell.
    pub fn body(self, view: impl IntoView) -> Self {
        self.slot(view, AppShell::body)
    }

    /// The overlay layer above the body.
    pub fn overlay(self, view: impl IntoView) -> Self {
        self.slot(view, AppShell::overlay)
    }
}

impl<K> El<Workspace, K> {
    /// The content of region `id`; the workspace places and sizes it.
    pub fn region(self, id: RegionId, view: impl IntoView) -> Self {
        self.slot(view, move |workspace, content| workspace.slot(id, content))
    }
}

impl<K> El<AppTitleBar, K> {
    pub fn leading(self, view: impl IntoView) -> Self {
        self.slot(view, AppTitleBar::leading)
    }

    pub fn center(self, view: impl IntoView) -> Self {
        self.slot(view, AppTitleBar::center)
    }

    pub fn trailing(self, view: impl IntoView) -> Self {
        self.slot(view, AppTitleBar::trailing)
    }
}

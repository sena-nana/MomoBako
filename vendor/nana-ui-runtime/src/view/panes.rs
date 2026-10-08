//! Named slots of the pane composites, as views:
//!
//! ```ignore
//! widget(SplitPane::new(model)).first(list()).second(detail())
//! widget(PaneSection::new()).header(title()).tabs(tabs()).body(rows())
//! widget(PaneChrome::new())
//!     .tabs(text("main.rs"))
//!     .action(PaneChromeAction::new(PaneChromeActionKind::CloseItem, "关闭").icon(Icon::Close), close())
//!     .body(editor())
//! ```
//!
//! Each composite places its slots in shells of its own when the tree
//! commits (`TypeBehavior::slot_assembler`).

use super::{El, IntoView};
use crate::{PaneChrome, PaneChromeAction, PaneSection, SplitPane};

impl<K> El<SplitPane, K> {
    /// The first pane (left, or top).
    pub fn first(self, view: impl IntoView) -> Self {
        self.slot(view, |mut pane: SplitPane, content| {
            pane.first = Some(content);
            pane
        })
    }

    /// The second pane (right, or bottom).
    pub fn second(self, view: impl IntoView) -> Self {
        self.slot(view, |mut pane: SplitPane, content| {
            pane.second = Some(content);
            pane
        })
    }
}

impl<K> El<PaneSection, K> {
    pub fn header(self, view: impl IntoView) -> Self {
        self.slot(view, PaneSection::header)
    }

    pub fn tabs(self, view: impl IntoView) -> Self {
        self.slot(view, PaneSection::tabs)
    }

    pub fn body(self, view: impl IntoView) -> Self {
        self.slot(view, PaneSection::body)
    }
}

impl<K> El<PaneChrome, K> {
    /// A header row of the application's own, in place of the one the
    /// chrome makes; the chrome styles it but leaves its children alone.
    pub fn header(self, view: impl IntoView) -> Self {
        self.slot(view, PaneChrome::header)
    }

    /// The tabs, first in the header row.
    pub fn tabs(self, view: impl IntoView) -> Self {
        self.slot(view, PaneChrome::tabs)
    }

    /// An action at the end of the header row: `view` is its control, and
    /// the chrome styles it as `action` says (an icon button when it has an
    /// icon). Actions keep the order they are written in.
    pub fn action(self, action: PaneChromeAction, view: impl IntoView) -> Self {
        self.slot(view, move |mut chrome: PaneChrome, target| {
            chrome.actions.push(action.target(target));
            chrome
        })
    }

    /// The content under the header row.
    pub fn body(self, view: impl IntoView) -> Self {
        self.slot(view, PaneChrome::body)
    }
}

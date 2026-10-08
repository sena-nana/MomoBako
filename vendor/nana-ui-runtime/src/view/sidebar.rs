//! Named slots of the sidebar composites, as views:
//!
//! ```ignore
//! widget(SidebarFrame::new())
//!     .body(
//!         widget(SidebarSection::new("资源").count(2).collapsible(true))
//!             .tools(icon_button(Icon::Add, "新建"))
//!             .children(rows.each(|r| r.id, row)),
//!     )
//!     .footer(settings_button())
//! ```
//!
//! A section builds its own chrome (header, title, count, disclosure, body
//! port) and moves its children into the body
//! ([`AppContext::assemble_sidebar_section`](crate::AppContext::assemble_sidebar_section)).

use super::{El, IntoView, widget};
use crate::{SidebarFrame, SidebarSection};

impl<K> El<SidebarSection, K> {
    /// Tools shown in the header while it is hovered, in place of the count.
    pub fn tools(self, view: impl IntoView) -> Self {
        self.slot(view, SidebarSection::tools)
    }
}

impl<K> El<SidebarFrame, K> {
    /// The fixed top, above the body. Give the slots in the order top,
    /// body, footer: they become the frame's children in the order given.
    pub fn top(self, view: impl IntoView) -> Self {
        self.child_slot(view, SidebarFrame::top)
    }

    /// The content that scrolls, inside the frame's vertical scrollport.
    pub fn body(self, view: impl IntoView) -> Self {
        let scroll = widget(SidebarFrame::vertical_body_scroll()).children(view);
        self.child_slot(scroll, SidebarFrame::body)
    }

    /// The fixed footer, below the body.
    pub fn footer(self, view: impl IntoView) -> Self {
        self.child_slot(view, SidebarFrame::footer)
    }
}

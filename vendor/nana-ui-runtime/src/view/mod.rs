//! Declarative views over the retained tree; see `docs/reference/reactive-view.md`.
//!
//! A view is an expression built once. Its dynamic parts are signals or
//! closures, and each binding updates exactly the node field it names:
//!
//! ```ignore
//! fn counter() -> impl IntoView {
//!     let count = signal(0u64);
//!     column().gap(12.0).children((
//!         text!("计数 {count}"),
//!         button("加一").on_activate(move || count.update(|c| *c += 1)),
//!     ))
//! }
//! cx.mount_view(parent, counter)?;
//! ```
//!
//! Nothing re-renders and nothing diffs. A write queues the effects that
//! read the signal; the owning [`crate::AppContext`] flushes them after
//! input and before a frame, merging every changed node into one commit.

mod composites;
mod controls;
mod each_virtual;
mod error;
mod hot;
mod node;
mod panes;
mod prop;
pub(crate) mod reactive;
mod resource;
mod selection;
mod settings;
mod shell;
mod sidebar;
mod store;
mod structural;
mod style;
mod task;
mod teleport;
#[cfg(feature = "reactive-trace")]
pub(crate) mod trace;
mod transition;

pub use crate::VirtualAlignment;
pub use controls::{
    Px, StyledComponent, avatar, button, checkbox, chip, column, divider, empty_state, icon_button,
    list_item, number_input, progress, row, select, slider, spinner, status_badge, switch, text,
    text_area, text_input, texture, thumbnail,
};
pub(crate) use controls::{edit_control, inspect_control};
pub use each_virtual::{EachVirtual, VirtualItem, VirtualListRef, each_virtual, virtual_list_ref};
pub use error::{ErrorBoundary, error_boundary, report_error};
#[doc(hidden)]
pub use hot::{__hot_register, __hot_text};
pub use hot::{HotReloadError, apply_hot_literals};
#[doc(hidden)]
pub use node::ViewSource;
pub use node::{
    AnyView, Children, Detached, El, EntityRef, IntoView, Keyed, Mount, NodeBindingInfo,
    NodeBindings, NodeRef, Refs, SourceLocation, ViewBuilder, WithRefs, detached, entity_ref,
    keyed, node_ref, widget, with_refs,
};
pub use node::{InspectedField, Inspection};
pub(crate) use node::{NodePatch, StructuralBinding, ViewParts, ViewState};
#[doc(hidden)]
pub use prop::Fixed;
pub use prop::{FieldWrite, IntoProp, PropSource};
#[doc(hidden)]
pub use reactive::{__checked, Dep};
pub use reactive::{
    Computed, Const, Effect, ReactiveStats, Readable, Signal, computed, constant, on_cleanup,
    on_mount, provide, reactive_stats, signal, untrack, use_context, watch_effect,
};
pub use resource::{Resource, Suspense, resource, suspense};
pub use selection::{segmented, segmented_option};
pub use settings::settings_row;
pub use store::{
    Item, KeyedList, Store, StoreList, StorePath, Subfield, store, store_with_history,
};
#[doc(hidden)]
pub use store::{Paths, StoreKey};
pub use structural::{Dynamic, Each, EachExt, When, WhenExt, dynamic, each, when};
#[doc(hidden)]
pub use style::ComposedLayout;
pub use style::{Class, InlineStyle, Sheet, StylePatch, StyleSite};
#[doc(hidden)]
pub use style::{SheetRule, SheetTransition};
pub use task::{
    Task, has_woken_tasks, poll_tasks, set_task_wake, spawn_blocking, spawn_local, task_count,
};
pub use teleport::{Teleport, teleport};
#[cfg(feature = "reactive-trace")]
pub use trace::{Cause, WhyUpdated};
pub use transition::{Implicit, Presence, Transition};

/// Text with `format!` interpolation that re-evaluates when a signal it
/// names changes: `text!("{count} items")`.
#[macro_export]
macro_rules! text {
    ($($arg:tt)*) => {
        $crate::view::text(move || ::std::format!($($arg)*))
    };
}

#[cfg(feature = "view-macro")]
pub use crate::css;
pub use crate::text;

#[cfg(feature = "view-macro")]
#[doc(hidden)]
pub use nana_ui_view_macros::view as __view;

/// One CSS declaration block compiled at build time, for [`El::css`]
/// (feature `view-macro`): `.css(css! { padding: 12px; opacity: 0.8 })`.
/// A value Rust cannot tokenize goes in double quotes: `font-size: "1.5em"`.
#[cfg(feature = "view-macro")]
#[macro_export]
macro_rules! css {
    ($($declarations:tt)*) => {
        $crate::view::__css!(crate = $crate; $($declarations)*)
    };
}

#[cfg(feature = "view-macro")]
#[doc(hidden)]
pub use nana_ui_view_macros::css as __css;

/// A stylesheet written as CSS, compiled at build time into a module with
/// one [`Class`] per class (feature `view-macro`):
///
/// ```ignore
/// stylesheet! {
///     mod styles;
///     .card { padding: 12px; transition: opacity 150ms; }
///     .card.done { opacity: 0.5; }
/// }
/// widget(card).class(styles::card).class_when(styles::done, done)
/// ```
///
/// The same CSS, rules and checks as a `view!` template's `<style>`.
#[cfg(feature = "view-macro")]
#[macro_export]
macro_rules! stylesheet {
    ($($sheet:tt)*) => {
        $crate::view::__stylesheet!(crate = $crate; $($sheet)*);
    };
}

#[cfg(feature = "view-macro")]
#[doc(hidden)]
pub use nana_ui_view_macros::stylesheet as __stylesheet;

#[cfg(feature = "view-macro")]
pub use crate::stylesheet;

/// `#[derive(Store)]` (feature `view-macro`): field accessors for a struct
/// kept in a [`Store`], as a `<Name>StoreFields` trait.
#[cfg(feature = "view-macro")]
pub use nana_ui_view_macros::Store;

/// A Vue-shaped template over this module (feature `view-macro`):
///
/// ```ignore
/// view! {
///     <Column gap=12>
///         <Text>"计数 {count}"</Text>
///         <Button @activate={count.update(|c| *c += 1)}>"加一"</Button>
///         <Text v-if={loading}>"加载中"</Text>
///         <Text v-else>"完成"</Text>
///         <Slider min=0 max=1 step=0.05 v-model={volume} />
///         <TodoRow v-for={t in todos} key={t.id} todo={t} />
///     </Column>
/// }
/// ```
///
/// It expands to the function calls it names and adds nothing at run time:
/// a literal is a constant, `{path}` is passed as is (a signal or a value),
/// any other `{expression}` becomes `move || expression`. Unknown tags call
/// the snake-cased function with the attribute values in order and the
/// children last. See `docs/reference/reactive-view.md`.
#[cfg(feature = "view-macro")]
#[macro_export]
macro_rules! view {
    ($($template:tt)*) => {
        $crate::view::__view!(crate = $crate; $($template)*)
    };
}

#[cfg(test)]
mod tests;

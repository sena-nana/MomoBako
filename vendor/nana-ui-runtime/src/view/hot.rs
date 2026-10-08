//! Static text of `.vue` views that a development build can replace while
//! it runs (`nana_ui_sfc::Compiler::hot`, `nana-ui-dev`'s
//! `watch_templates`).
//!
//! A view compiled in hot mode keeps its static text in a table and reads
//! each entry through [`__hot_text`], which tracks one signal per view. The
//! view also registers a shape: the hash of everything it is but that text.
//! [`apply_hot_literals`] swaps the text of every mounted instance when the
//! shape matches, and refuses when it does not, since then the code
//! changed and only a rebuild can apply it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use super::reactive::{self, Signal, signal};

struct HotView {
    shape: u64,
    /// Replacement text, in table order; `None` until the first swap.
    text: Signal<Option<Rc<[String]>>>,
}

thread_local! {
    static VIEWS: RefCell<HashMap<&'static str, HotView>> = RefCell::new(HashMap::new());
}

/// Called by each hot view as it is built.
#[doc(hidden)]
pub fn __hot_register(view: &'static str, shape: u64) {
    VIEWS.with(|views| {
        views.borrow_mut().entry(view).or_insert_with(|| HotView {
            shape,
            // Every instance reads it, so no instance's scope may own it.
            text: reactive::without_scope(|| signal(None)),
        });
    });
}

/// Entry `index` of `view`'s static text: the replacement if one was
/// applied, else the compiled `default`. Tracked.
#[doc(hidden)]
pub fn __hot_text(view: &'static str, index: usize, default: &'static str) -> String {
    let text = VIEWS.with(|views| views.borrow().get(view).map(|view| view.text));
    match text {
        Some(text) => text.with(|text| {
            text.as_ref()
                .and_then(|text| text.get(index).cloned())
                .unwrap_or_else(|| default.to_owned())
        }),
        None => default.to_owned(),
    }
}

/// Why new text could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotReloadError {
    /// No view by that name has been built in this process.
    UnknownView(String),
    /// The view changed beyond its text: rebuild.
    ShapeChanged(String),
}

impl fmt::Display for HotReloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownView(view) => write!(formatter, "no hot view `{view}` was built"),
            Self::ShapeChanged(view) => {
                write!(
                    formatter,
                    "`{view}` changed beyond its text; rebuild to apply it"
                )
            }
        }
    }
}

impl std::error::Error for HotReloadError {}

/// Replace the static text of every mounted `view` with `text` (in the
/// order `nana_ui_sfc::Compiler::hot_views` lists it), if the view's `shape` is
/// the one it was built with. Bindings update at the next flush.
pub fn apply_hot_literals(view: &str, shape: u64, text: Vec<String>) -> Result<(), HotReloadError> {
    let signal = VIEWS.with(|views| {
        let views = views.borrow();
        let hot = views
            .get(view)
            .ok_or_else(|| HotReloadError::UnknownView(view.to_owned()))?;
        if hot.shape != shape {
            return Err(HotReloadError::ShapeChanged(view.to_owned()));
        }
        Ok(hot.text)
    })?;
    signal.set(Some(text.into()));
    Ok(())
}

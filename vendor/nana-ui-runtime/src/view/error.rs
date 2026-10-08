//! Errors shown in place of the views that failed (Leptos `ErrorBoundary`,
//! Vue `onErrorCaptured`).
//!
//! Errors are values: a view that is `Err(e)` builds nothing and reports
//! `e` to the nearest [`error_boundary`] above it, which shows its fallback
//! instead of its content while any error stands. An error stands as long
//! as the scope that reported it: when the branch or row that failed is
//! dropped (its data changed), the error goes and the content is back.
//!
//! ```ignore
//! error_boundary(
//!     |errors| text(format!("出错了：{}", errors.join("；"))),
//!     move || dynamic(move || user.get(), |user| match user {
//!         Some(Ok(user)) => Ok(text(user.name.clone())),
//!         Some(Err(error)) => Err(error.clone()),
//!         None => Ok(text("…")),
//!     }),
//! )
//! ```
//!
//! Panics are bugs and are not caught.

use std::fmt::Display;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::node::{IntoView, ViewBuilder, widget};
use super::reactive::{self, Signal, on_cleanup, provide, signal, use_context};
use super::structural::{build_scoped, dynamic};
use crate::Stack;

static NEXT_ERROR: AtomicU64 = AtomicU64::new(1);

/// The errors standing inside one boundary.
#[derive(Clone, Copy)]
struct Boundary {
    errors: Signal<Vec<(u64, Arc<str>)>>,
}

/// Report `error` to the nearest [`error_boundary`], standing until the
/// current scope is disposed. Without a boundary it is a diagnostic fault
/// and `false` is returned.
#[track_caller]
pub fn report_error(error: impl Display) -> bool {
    let message: Arc<str> = error.to_string().into();
    let Some(boundary) = use_context::<Boundary>() else {
        nana_diagnostics::fault!(
            nana_diagnostics::framework::runtime::VIEW_ERROR_UNHANDLED;
            "view error outside any error boundary: {message}"
        );
        return false;
    };
    let id = NEXT_ERROR.fetch_add(1, Ordering::Relaxed);
    boundary.errors.update(|errors| errors.push((id, message)));
    if reactive::current_scope().is_some() {
        on_cleanup(move || {
            // The boundary may be gone first, with everything under it.
            let _ = boundary
                .errors
                .try_update(|errors| errors.retain(|(standing, _)| *standing != id));
        });
    }
    true
}

impl<V: IntoView, E: Display + 'static> IntoView for Result<V, E> {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        match self {
            Ok(view) => view.build(vb),
            Err(error) => {
                report_error(error);
            }
        }
    }
}

/// `content`, or `fallback(errors)` while a view inside it failed (see the
/// module docs). The content stays built, hidden, so it is back as it was
/// once the errors are gone.
pub fn error_boundary<F, V, C>(
    fallback: impl Fn(Vec<Arc<str>>) -> F + Send + 'static,
    content: C,
) -> ErrorBoundary<C>
where
    F: IntoView,
    V: IntoView,
    C: FnOnce() -> V + 'static,
{
    ErrorBoundary {
        fallback: Box::new(move |errors| fallback(errors).into_any()),
        content,
    }
}

/// See [`error_boundary`].
pub struct ErrorBoundary<C> {
    fallback: Box<dyn Fn(Vec<Arc<str>>) -> super::AnyView + Send>,
    content: C,
}

struct Guarded<C> {
    boundary: Boundary,
    content: C,
}

impl<V: IntoView, C: FnOnce() -> V + 'static> IntoView for Guarded<C> {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        let (boundary, content) = (self.boundary, self.content);
        build_scoped(vb, reactive::current_scope(), move || {
            provide(boundary);
            content().into_any()
        });
    }
}

impl<V: IntoView, C: FnOnce() -> V + 'static> IntoView for ErrorBoundary<C> {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        let errors = signal(Vec::<(u64, Arc<str>)>::new());
        let fallback = self.fallback;
        let messages = move || {
            errors.with(|errors| errors.iter().map(|(_, message)| message.clone()).collect())
        };
        super::column()
            .children((
                widget(Stack::column(0.0))
                    .visible(move || errors.with(Vec::is_empty))
                    .children(Guarded {
                        boundary: Boundary { errors },
                        content: self.content,
                    }),
                dynamic(messages, move |messages: &Vec<Arc<str>>| {
                    (!messages.is_empty()).then(|| fallback(messages.clone()))
                }),
            ))
            .build(vb);
    }
}

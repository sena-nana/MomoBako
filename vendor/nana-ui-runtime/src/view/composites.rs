//! Named slots of modal surfaces, the confirm dialog, the form field and
//! the media transport bar, as views:
//!
//! ```ignore
//! widget(Dialog::new("投币"))
//!     .body(column().children((summary, choices)))
//!     .footer(row().children((cancel, confirm)))
//! ```
//!
//! Each slot is built before its composite and placed by the composite's
//! assembler once the tree commits, so a view never calls
//! `set_modal_slots` or appends into the transport bar's groups. A slot's
//! node stays for the view's life; what changes inside it is a `when` /
//! `each` there.

use super::{El, EntityRef, IntoView};
use crate::{
    ActionMenu, ConfirmDialog, Dialog, Drawer, FormField, MediaTransportBar, ModalInitialFocus,
    ModalSurface, Popover,
};

macro_rules! modal_focus {
    ($($surface:ty),*) => {$(
        impl<K> El<$surface, K> {
            /// Focus `target` when the surface opens: a control in one of its
            /// slots, such as the button given to `.confirm`. Slots are built
            /// before the surface, so their refs are resolved here; a target
            /// that is not built yet leaves the surface's default focus.
            pub fn initial_focus_on<T: crate::View>(self, target: EntityRef<T>) -> Self {
                self.bind(move |modal: &mut $surface| {
                    if let Some(target) = target.get() {
                        modal.set_initial_focus(ModalInitialFocus::Target(target.stable_id()));
                    }
                })
            }
        }
    )*};
}

modal_focus!(Dialog, Drawer, ConfirmDialog);

macro_rules! modal_slots {
    ($($surface:ty),*) => {$(
        impl<K> El<$surface, K> {
            /// The surface's content, under its title.
            pub fn body(self, view: impl IntoView) -> Self {
                self.slot(view, |mut modal: $surface, id| {
                    modal.slots_mut().body = Some(id);
                    modal
                })
            }

            /// The row of actions at the surface's foot.
            pub fn footer(self, view: impl IntoView) -> Self {
                self.slot(view, |mut modal: $surface, id| {
                    modal.slots_mut().footer = Some(id);
                    modal
                })
            }

            /// The affordance that dismisses the surface, beside its title.
            pub fn close_action(self, view: impl IntoView) -> Self {
                self.slot(view, |mut modal: $surface, id| {
                    modal.slots_mut().close_action = Some(id);
                    modal
                })
            }
        }
    )*};
}

modal_slots!(Dialog, Drawer);

impl<K> El<ConfirmDialog, K> {
    /// Content between the message and the actions.
    pub fn body(self, view: impl IntoView) -> Self {
        self.slot(view, ConfirmDialog::body)
    }

    /// The affordance that dismisses the dialog, beside its title.
    pub fn close_action(self, view: impl IntoView) -> Self {
        self.slot(view, ConfirmDialog::close_action)
    }

    /// The dismissing action, in place of the button the dialog makes from
    /// [`ConfirmDialog::cancel_label`]. The dialog leaves its label alone.
    pub fn cancel(self, view: impl IntoView) -> Self {
        self.slot(view, ConfirmDialog::cancel)
    }

    /// A third action between cancel and confirm.
    pub fn secondary(self, view: impl IntoView) -> Self {
        self.slot(view, ConfirmDialog::secondary)
    }

    /// The confirming action, in place of the button the dialog makes from
    /// [`ConfirmDialog::confirm_label`]. The dialog leaves its label alone.
    pub fn confirm(self, view: impl IntoView) -> Self {
        self.slot(view, ConfirmDialog::confirm)
    }
}

impl<K> El<FormField, K> {
    /// The field's control, under its label and over its message.
    pub fn control(self, view: impl IntoView) -> Self {
        self.child_slot(view, FormField::control_child)
    }
}

impl<K> El<MediaTransportBar, K> {
    /// Controls after play: next, skip.
    pub fn leading(self, view: impl IntoView) -> Self {
        self.slot(view, MediaTransportBar::leading_content)
    }

    /// Controls before volume: speed, quality, subtitles.
    pub fn trailing(self, view: impl IntoView) -> Self {
        self.slot(view, MediaTransportBar::trailing_content)
    }

    /// The second row under the seek bar; it collapses while empty.
    pub fn secondary(self, view: impl IntoView) -> Self {
        self.slot(view, MediaTransportBar::secondary_content)
    }

    /// The items of the settings menu: theatre, a window of its own, stop.
    pub fn settings(self, view: impl IntoView) -> Self {
        self.slot(view, MediaTransportBar::settings_content)
    }
}

impl<K> El<Popover, K> {
    /// What the trigger shows — an icon, a label and a count — in place of
    /// the popover's text label, which stays its accessible name. The
    /// popover remains the control: the press, focus, Enter / Space and the
    /// anchor are its, so the content holds nothing pressable of its own.
    /// Its `.children(..)` are the surface's items.
    pub fn trigger(self, view: impl IntoView) -> Self {
        self.child_slot(view, Popover::trigger_content)
    }
}

impl<K> El<ActionMenu, K> {
    /// What the trigger shows, as [`El::<Popover>::trigger`].
    pub fn trigger(self, view: impl IntoView) -> Self {
        self.child_slot(view, |mut menu: ActionMenu, id| {
            menu.popover.trigger_content = Some(id);
            menu
        })
    }
}

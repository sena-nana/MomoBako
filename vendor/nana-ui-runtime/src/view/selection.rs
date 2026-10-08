//! A segmented control as a view: its options are its children, each with
//! a bindable label, disabled and selected state and its own handler.
//!
//! ```ignore
//! widget(SegmentedControl::new().size(ControlSize::Small).fill(true)).children((
//!     segmented_option("面捕").selected(move || mode.get() == Mode::Face).on_select(face),
//!     segmented_option("鼠标").selected(move || mode.get() == Mode::Mouse).on_select(mouse),
//! ))
//! ```
//!
//! The control registers its option children and its selection itself
//! (`TypeBehavior::slot_assembler`), after it is built and whenever a binding
//! changes it or one of its options; a view never calls
//! [`AppContext::set_segmented_options`]. An option the view hides stays a
//! child: hide and disable it together.
//!
//! [`AppContext::set_segmented_options`]: crate::AppContext::set_segmented_options

use std::sync::Arc;

use super::controls::StyledComponent;
use super::node::{El, widget};
use super::prop::{FieldWrite, IntoProp};
use crate::{NodeStyle, SegmentedControl, SegmentedOption, SegmentedOptionChosen};

/// An empty segmented control; give it [`segmented_option`] children.
#[track_caller]
pub fn segmented() -> El<SegmentedControl> {
    widget(SegmentedControl::new())
}

/// One option of a segmented control, labelled `label`.
#[track_caller]
pub fn segmented_option(label: impl IntoProp<Arc<str>>) -> El<SegmentedOption> {
    widget(SegmentedOption::new("")).label(label)
}

impl StyledComponent for SegmentedOption {
    fn node_style(&self) -> &NodeStyle {
        &self.style
    }

    fn node_style_mut(&mut self) -> &mut NodeStyle {
        &mut self.style
    }
}

impl StyledComponent for SegmentedControl {
    fn node_style(&self) -> &NodeStyle {
        &self.style
    }

    fn node_style_mut(&mut self) -> &mut NodeStyle {
        &mut self.style
    }
}

/// Bindable fields of [`SegmentedOption`].
#[allow(non_camel_case_types)]
#[doc(hidden)]
pub mod segmented_option {
    pub struct label;
    pub struct disabled;
    pub struct selected;
}

impl FieldWrite<SegmentedOption, Arc<str>> for segmented_option::label {
    const FIELD: &'static str = "SegmentedOption.label";

    fn write(target: &mut SegmentedOption, value: Arc<str>) {
        target.label = value;
    }

    fn differs(target: &SegmentedOption, value: &Arc<str>) -> bool {
        target.label != *value
    }
}

impl FieldWrite<SegmentedOption, bool> for segmented_option::disabled {
    const FIELD: &'static str = "SegmentedOption.disabled";

    fn write(target: &mut SegmentedOption, value: bool) {
        target.disabled = value;
    }

    fn differs(target: &SegmentedOption, value: &bool) -> bool {
        target.disabled != *value
    }
}

impl FieldWrite<SegmentedOption, bool> for segmented_option::selected {
    const FIELD: &'static str = "SegmentedOption.selected";

    fn write(target: &mut SegmentedOption, value: bool) {
        target.selected = value;
    }

    fn differs(target: &SegmentedOption, value: &bool) -> bool {
        target.selected != *value
    }
}

impl<K> El<SegmentedOption, K> {
    #[track_caller]
    pub fn label(self, value: impl IntoProp<Arc<str>>) -> Self {
        self.prop::<Arc<str>, segmented_option::label>(value)
    }

    #[track_caller]
    pub fn disabled(self, value: impl IntoProp<bool>) -> Self {
        self.prop::<bool, segmented_option::disabled>(value)
    }

    /// Whether this is the control's selection. The control follows it; a
    /// user's choice selects the option at once, and the binding restates
    /// what it says the next time it runs.
    #[track_caller]
    pub fn selected(self, value: impl IntoProp<bool>) -> Self {
        self.prop::<bool, segmented_option::selected>(value)
    }

    /// `handler` runs when the user chooses this option.
    pub fn on_select(self, mut handler: impl FnMut() + Send + 'static) -> Self {
        self.on(move |_: &SegmentedOptionChosen| handler())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::view::signal;
    use crate::{AppContext, DocumentId, Entity, StableNodeId, Stack};

    fn control_state(
        cx: &AppContext,
        id: StableNodeId,
    ) -> (Vec<StableNodeId>, Option<StableNodeId>) {
        cx.read(Entity::<SegmentedControl>::from_stable_id(id), |control| {
            (control.options.clone(), control.selected)
        })
        .unwrap()
    }

    /// Options and selection are data: the control registers its children
    /// and follows their `selected` flags, and each option runs its own
    /// handler when chosen.
    #[test]
    fn a_view_control_registers_its_options_and_follows_the_selection() {
        let document = DocumentId::new(1).unwrap();
        let mut cx = AppContext::new();
        let root = cx.create_component(document, Stack::column(0.0)).unwrap();
        let mode = signal(0usize);
        let chosen = Arc::new(AtomicUsize::new(usize::MAX));
        let option = |index: usize, label: &'static str| {
            let chosen = Arc::clone(&chosen);
            segmented_option(label)
                .selected(move || mode.get() == index)
                .on_select(move || chosen.store(index, Ordering::SeqCst))
        };
        cx.mount_view(root.stable_id(), move || {
            segmented().children((option(0, "面捕"), option(1, "鼠标"), option(2, "手柄")))
        })
        .unwrap();
        cx.flush_reactive().unwrap();

        let control = cx.world().node(root.stable_id()).unwrap().children[0];
        let options = cx.world().node(control).unwrap().children.clone();
        assert_eq!(
            control_state(&cx, control),
            (options.clone(), Some(options[0]))
        );

        mode.set(2);
        cx.flush_reactive().unwrap();
        assert_eq!(control_state(&cx, control).1, Some(options[2]));

        assert!(cx.activate_node(options[1]).unwrap());
        assert_eq!(
            chosen.load(Ordering::SeqCst),
            1,
            "the chosen option's handler runs"
        );
        assert_eq!(control_state(&cx, control).1, Some(options[1]));

        // The application kept its own choice: the next run restates it.
        mode.set(0);
        cx.flush_reactive().unwrap();
        assert_eq!(control_state(&cx, control).1, Some(options[0]));
    }
}

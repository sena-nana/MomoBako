//! Element functions of the built-in controls, and their typed bindable
//! fields, `model` and event methods expanded from the control table in
//! `nana-ui-view-schema`.

use std::any::Any;
use std::sync::Arc;

use super::node::IntoView;
use super::node::{El, widget};
use super::prop::{FieldWrite, IntoProp};
use super::reactive::Signal;
use crate::{
    Activate, AppContext, Avatar, Button, Card, Checkbox, Chip, Divider, EmptyState, Entity,
    GpuTextureView, IconButton, ListItem, NodeStyle, NumberChanged, NumberInput, Progress,
    RangeChanged, RangeField, RangeInput, ScrollView, Select, SelectChanged, SelectOption, Spinner,
    StableNodeId, Stack, StatusBadge, Switch, Text, TextArea, TextChanged, TextInput,
    TextSubmitted, Thumbnail, ToggleChanged,
};
use crate::{
    AppTitleBar, Breadcrumb, Dialog, Drawer, FormField, GpuView, IconGlyph, InteractiveCard,
    LabeledValue, LevelMeter, List, MediaTransportBar, Panel, SettingsCard, SidebarRow, Skeleton,
    StatusBar, Tabs, Toolbar, Tooltip, ValidationMessage, Video,
};
use nana_ui_core::{Icon, RadiusTier, SemanticColorRole, StatusTone};

/// Components whose [`NodeStyle`] the view layer may write (visibility).
pub trait StyledComponent {
    #[doc(hidden)]
    fn node_style(&self) -> &NodeStyle;
    #[doc(hidden)]
    fn node_style_mut(&mut self) -> &mut NodeStyle;
}

impl StyledComponent for Stack {
    fn node_style(&self) -> &NodeStyle {
        self.style_ref()
    }

    fn node_style_mut(&mut self) -> &mut NodeStyle {
        self.style_mut()
    }
}

#[doc(hidden)]
pub struct Visible;

impl<C: StyledComponent> FieldWrite<C, bool> for Visible {
    const FIELD: &'static str = "style.layout.hidden";

    fn write(target: &mut C, visible: bool) {
        let style = target.node_style_mut();
        if style.layout.hidden == visible {
            Arc::make_mut(&mut style.layout).hidden = !visible;
        }
    }

    fn differs(target: &C, visible: &bool) -> bool {
        target.node_style().layout.hidden == *visible
    }
}

/// Semantic roles of [`NodeStyle`], resolved against the installed theme
/// when painted, so they follow a theme switch; a stylesheet's colours are
/// fixed when it is compiled.
macro_rules! style_roles {
    ($($writer:ident: $field:ident: $ty:ty, $doc:literal;)*) => {$(
        #[doc(hidden)]
        pub struct $writer;

        impl<C: StyledComponent> FieldWrite<C, $ty> for $writer {
            const FIELD: &'static str = concat!("style.", stringify!($field));

            fn write(target: &mut C, value: $ty) {
                target.node_style_mut().$field = value;
            }

            fn differs(target: &C, value: &$ty) -> bool {
                target.node_style().$field != *value
            }
        }

        impl<C: StyledComponent + crate::ComponentView, K> El<C, K> {
            #[doc = $doc]
            #[track_caller]
            pub fn $field(self, value: impl IntoProp<$ty>) -> Self {
                self.prop::<$ty, $writer>(value)
            }
        }
    )*};
}

style_roles! {
    Foreground: foreground: Option<SemanticColorRole>, "Text and glyph colour by theme role.";
    Background: background: Option<SemanticColorRole>, "Fill by theme role.";
    Border: border: Option<SemanticColorRole>, "Border colour by theme role; the width is the layout's.";
    Radius: radius: Option<RadiusTier>, "Corner radius by theme step.";
}

impl<C: StyledComponent + crate::ComponentView, K> El<C, K> {
    /// Keep the node but take it out of layout, paint and hit testing
    /// (`v-show`). Use [`super::when`] to drop the subtree instead (`v-if`).
    #[track_caller]
    pub fn visible(self, visible: impl IntoProp<bool>) -> Self {
        self.prop::<bool, Visible>(visible)
    }

    /// [`Self::visible`] declared at `at`: a structural view's container.
    pub(crate) fn visible_at(
        mut self,
        visible: super::prop::PropSource<bool>,
        at: &'static std::panic::Location<'static>,
    ) -> Self {
        let (component, bindings) = self.parts_mut();
        visible.bind_field::<C, Visible>(component, bindings, at);
        self
    }
}

/// Components outside the control table whose style views may still write.
macro_rules! styled {
    ($($component:ident),*) => {$(
        impl StyledComponent for $component {
            fn node_style(&self) -> &NodeStyle {
                &self.style
            }

            fn node_style_mut(&mut self) -> &mut NodeStyle {
                &mut self.style
            }
        }
    )*};
}

styled!(
    Card,
    ScrollView,
    ValidationMessage,
    Skeleton,
    Tooltip,
    Panel,
    List,
    Toolbar,
    Tabs,
    InteractiveCard,
    SettingsCard,
    LabeledValue,
    FormField,
    Breadcrumb,
    StatusBar,
    IconGlyph,
    GpuView,
    Video,
    SidebarRow,
    MediaTransportBar,
    Dialog,
    Drawer,
    LevelMeter,
    AppTitleBar
);

/// Expands the control table of `nana-ui-view-schema`: per control, one
/// [`FieldWrite`] and `El` setter per field, `model` and event methods, and
/// [`StyledComponent`] for `.visible`.
macro_rules! controls {
    ($(
        $tag:ident => $function:ident ($($argument:ident: $kind:ident),*) for $component:ident {
            $($field:ident: $ty:ty = $write:ident),* $(,)?
        }
        $(on { $($on:ident: $on_event:ident),* $(,)? })?
        $(with { $($with:ident: $with_event:ident),* $(,)? })?
        $(model $model:ident: $model_ty:ty => $model_event:ident |$event:ident| $from_event:expr)?
        ;
    )*) => {$(
        #[allow(non_camel_case_types)]
        #[doc(hidden)]
        pub mod $function {
            $(pub struct $field;)*
        }

        $(
            impl FieldWrite<$component, $ty> for $function::$field {
                const FIELD: &'static str = concat!(stringify!($component), ".", stringify!($field));

                fn write(target: &mut $component, value: $ty) {
                    controls!(@write $write target, value, $field)
                }

                fn differs(target: &$component, value: &$ty) -> bool {
                    controls!(@differs $write target, value, $field)
                }
            }
        )*

        impl<K> El<$component, K> {
            $(
                #[track_caller]
                pub fn $field(self, value: impl IntoProp<$ty>) -> Self {
                    self.prop::<$ty, $function::$field>(value)
                }
            )*

            $(
                /// `v-model`: shows the signal and writes each change back.
                #[track_caller]
                pub fn model(self, value: Signal<$model_ty>) -> Self {
                    self.$model(value)
                        .on(move |$event: &$model_event| value.set($from_event))
                }
            )?

            $($(
                /// `@` event of the template.
                pub fn $on(self, mut handler: impl FnMut() + Send + 'static) -> Self {
                    self.on(move |_: &$on_event| handler())
                }
            )*)?

            $($(
                /// `@` event of the template, with its value.
                pub fn $with(self, handler: impl FnMut(&$with_event) + Send + 'static) -> Self {
                    self.on(handler)
                }
            )*)?
        }

        impl StyledComponent for $component {
            fn node_style(&self) -> &NodeStyle {
                &self.style
            }

            fn node_style_mut(&mut self) -> &mut NodeStyle {
                &mut self.style
            }
        }
    )*};
    (@write set $target:ident, $value:ident, $field:ident) => {
        $target.$field = $value
    };
    (@differs set $target:ident, $value:ident, $field:ident) => {
        $target.$field != *$value
    };
    // Only a different value replaces the text, so echoing an edit back
    // through its signal leaves the caret and selection alone.
    (@write text_state $target:ident, $value:ident, $field:ident) => {
        if $target.state.value != $value {
            $target.state.replace_value($value);
        }
    };
    (@differs text_state $target:ident, $value:ident, $field:ident) => {
        $target.state.value != *$value
    };
    (@write assign $target:ident, $value:ident, $field:ident) => {{
        $target.assign($value);
    }};
    (@differs assign $target:ident, $value:ident, $field:ident) => {
        $target.$field() != *$value
    };
}

nana_ui_view_schema::for_each_control!(controls);

/// Vertical stack: `column().gap(8).with(|c| { c.add(…); })`, or
/// `.children((a, b))` for a fixed few.
#[track_caller]
pub fn column() -> El<Stack> {
    widget(Stack::column(0.0))
}

/// Horizontal stack; see [`column`].
#[track_caller]
pub fn row() -> El<Stack> {
    widget(Stack::row(0.0))
}

/// A length in logical pixels, whole or not: `gap(8)`, `gap(7.5)`.
pub trait Px {
    fn px(self) -> f32;
}

macro_rules! px {
    ($($ty:ty),*) => {$(
        impl Px for $ty {
            fn px(self) -> f32 {
                self as f32
            }
        }
    )*};
}

px!(f32, f64, i32, u32, i64, u64, usize, u16, u8);

impl<K> El<Stack, K> {
    /// Space between the children, in logical pixels.
    pub fn gap(self, gap: impl Px) -> Self {
        let gap = crate::LengthSpec::Px(gap.px().max(0.0));
        self.map_component(|stack| stack.with_layout(|layout| layout.gap = Some(gap)))
    }
}

/// Text. For interpolation see [`crate::text!`].
#[track_caller]
pub fn text(value: impl IntoProp<String>) -> El<Text> {
    widget(Text::new("")).value(value)
}

#[track_caller]
pub fn button(label: impl IntoProp<String>) -> El<Button> {
    widget(Button::new("")).label(label)
}

/// A range slider between `minimum` and `maximum`.
#[track_caller]
pub fn slider(minimum: f64, maximum: f64, step: f64) -> El<RangeField> {
    widget(RangeField::new(minimum, minimum, maximum, step))
}

#[track_caller]
pub fn text_input() -> El<TextInput> {
    widget(TextInput::new(""))
}

#[track_caller]
pub fn checkbox(label: impl IntoProp<String>) -> El<Checkbox> {
    widget(Checkbox::new("", false)).label(label)
}

#[track_caller]
pub fn switch(label: impl IntoProp<String>) -> El<Switch> {
    widget(Switch::new("", false)).label(label)
}

/// A multi-line text field.
#[track_caller]
pub fn text_area() -> El<TextArea> {
    widget(TextArea::new(""))
}

#[track_caller]
pub fn number_input() -> El<NumberInput> {
    widget(NumberInput::new(0.0))
}

/// A single-value field; give it `.options(…)`.
#[track_caller]
pub fn select() -> El<Select> {
    widget(Select::new(None::<Arc<str>>))
}

#[track_caller]
pub fn list_item(label: impl IntoProp<String>) -> El<ListItem> {
    widget(ListItem::new("")).label(label)
}

/// Progress towards `max`; bind `.value(…)`.
#[track_caller]
pub fn progress(max: f64) -> El<Progress> {
    widget(Progress::new(0.0, max))
}

#[track_caller]
pub fn spinner() -> El<Spinner> {
    widget(Spinner::new(""))
}

/// A horizontal rule.
#[track_caller]
pub fn divider() -> El<Divider> {
    widget(Divider::horizontal())
}

/// A host-texture image at its aspect ratio; bind `.resource(..)` and move
/// `.generation(..)` when the host fills the slot.
#[track_caller]
pub fn thumbnail() -> El<Thumbnail> {
    widget(Thumbnail::new(""))
}

/// A round host-texture image `size` pixels across.
#[track_caller]
pub fn avatar(size: f32) -> El<Avatar> {
    widget(Avatar::new("")).size(size)
}

/// A host texture filling its box: video frames, previews.
#[track_caller]
pub fn texture() -> El<GpuTextureView> {
    widget(GpuTextureView::new(""))
}

/// A glyph button; `label` is its accessible name and tooltip.
#[track_caller]
pub fn icon_button(icon: Icon, label: impl IntoProp<Arc<str>>) -> El<IconButton> {
    widget(IconButton::new(icon, "")).label(label)
}

#[track_caller]
pub fn chip(label: impl IntoProp<Arc<str>>) -> El<Chip> {
    widget(Chip::new("")).label(label)
}

/// Short status text in its tone; bind `.tone(..)`.
#[track_caller]
pub fn status_badge(label: impl IntoProp<Arc<str>>) -> El<StatusBadge> {
    widget(StatusBadge::new("", StatusTone::Neutral)).label(label)
}

/// A title with an optional message, icon and [`El::action`].
#[track_caller]
pub fn empty_state(title: impl IntoProp<Arc<str>>) -> El<EmptyState> {
    widget(EmptyState::new("")).title(title)
}

impl<K> El<EmptyState, K> {
    /// The one action under the message, such as a retry button.
    pub fn action(self, view: impl IntoView) -> Self {
        self.child_slot(view, |empty, id| empty.action_child(id))
    }
}

/// A field value written as text by devtools and parsed back.
trait FieldText: Sized {
    fn parse(text: &str) -> Result<Self, String>;
}

impl FieldText for String {
    fn parse(text: &str) -> Result<Self, String> {
        Ok(text.to_owned())
    }
}

impl FieldText for Arc<str> {
    fn parse(text: &str) -> Result<Self, String> {
        Ok(Arc::from(text))
    }
}

impl FieldText for Option<Arc<str>> {
    /// Empty text is `None`.
    fn parse(text: &str) -> Result<Self, String> {
        Ok((!text.is_empty()).then(|| Arc::from(text)))
    }
}

impl FieldText for bool {
    fn parse(text: &str) -> Result<Self, String> {
        text.trim()
            .parse()
            .map_err(|_| format!("`{text}` is not `true` or `false`"))
    }
}

impl FieldText for f64 {
    fn parse(text: &str) -> Result<Self, String> {
        text.trim()
            .parse()
            .map_err(|_| format!("`{text}` is not a number"))
    }
}

impl FieldText for f32 {
    fn parse(text: &str) -> Result<Self, String> {
        text.trim()
            .parse()
            .map_err(|_| format!("`{text}` is not a number"))
    }
}

impl FieldText for u64 {
    fn parse(text: &str) -> Result<Self, String> {
        text.trim()
            .parse()
            .map_err(|_| format!("`{text}` is not a whole number"))
    }
}

macro_rules! edited_in_code {
    ($($ty:ty),*) => {$(
        impl FieldText for $ty {
            fn parse(_: &str) -> Result<Self, String> {
                Err(concat!("`", stringify!($ty), "` is edited in code, not as text").into())
            }
        }
    )*};
}

edited_in_code!(Icon, Option<Icon>, StatusTone);

impl FieldText for Vec<SelectOption> {
    fn parse(_: &str) -> Result<Self, String> {
        Err("options are edited in code, not as text".into())
    }
}

/// Every bindable field of a built-in control, as text: the control table's
/// tag and `(field, value)` pairs, `None` for anything else.
pub(crate) fn inspect_control(
    view: &dyn Any,
) -> Option<(&'static str, Vec<(&'static str, String)>)> {
    macro_rules! inspectors {
        ($(
            $tag:ident => $function:ident ($($argument:ident: $kind:ident),*) for $component:ident {
                $($field:ident: $ty:ty = $write:ident),* $(,)?
            }
            $(on { $($on:ident: $on_event:ident),* $(,)? })?
            $(with { $($with:ident: $with_event:ident),* $(,)? })?
            $(model $model:ident: $model_ty:ty => $model_event:ident |$event:ident| $from_event:expr)?
            ;
        )*) => {$(
            if let Some(control) = view.downcast_ref::<$component>() {
                let _ = control;
                return Some((
                    stringify!($tag),
                    vec![$((
                        stringify!($field),
                        format!("{:?}", inspectors!(@read $write control, $field)),
                    )),*],
                ));
            }
        )*};
        (@read set $control:ident, $field:ident) => { &$control.$field };
        (@read text_state $control:ident, $field:ident) => { &$control.state.value };
        (@read assign $control:ident, $field:ident) => { $control.$field() };
    }
    nana_ui_view_schema::for_each_control!(inspectors);
    None
}

/// Write `field` of the built-in control at `node` from `text`, as its
/// binding would. `None` when the node is not a built-in control.
pub(crate) fn edit_control(
    cx: &mut AppContext,
    node: StableNodeId,
    field: &str,
    text: &str,
) -> Option<Result<(), String>> {
    macro_rules! editors {
        ($(
            $tag:ident => $function:ident ($($argument:ident: $kind:ident),*) for $component:ident {
                $($field:ident: $ty:ty = $write:ident),* $(,)?
            }
            $(on { $($on:ident: $on_event:ident),* $(,)? })?
            $(with { $($with:ident: $with_event:ident),* $(,)? })?
            $(model $model:ident: $model_ty:ty => $model_event:ident |$event:ident| $from_event:expr)?
            ;
        )*) => {$(
            if cx.view_is::<$component>(node) {
                return Some(match field {
                    $(stringify!($field) => <$ty as FieldText>::parse(text).and_then(|value| {
                        cx.update_component(Entity::<$component>::from_stable_id(node), |control, _| {
                            <$function::$field as FieldWrite<$component, $ty>>::write(control, value)
                        })
                        .map_err(|error| error.to_string())
                    }),)*
                    other => Err(format!(
                        "`<{}>` has no field `{other}`",
                        stringify!($tag)
                    )),
                });
            }
        )*};
    }
    nana_ui_view_schema::for_each_control!(editors);
    None
}

macro_rules! item_slots {
    ($($component:ty),*) => {$(
        /// A list row's slots, as its own children: give them in the order
        /// leading, content, trailing.
        impl<K> El<$component, K> {
            /// Before the label (an icon, a check).
            pub fn leading(self, view: impl super::IntoView) -> Self {
                self.child_slot(view, |mut item: $component, id| {
                    item.slots.leading = Some(id);
                    item
                })
            }

            /// In place of the label.
            pub fn content(self, view: impl super::IntoView) -> Self {
                self.child_slot(view, |mut item: $component, id| {
                    item.slots.content = Some(id);
                    item
                })
            }

            /// After the label (a count, a shortcut, a control).
            pub fn trailing(self, view: impl super::IntoView) -> Self {
                self.child_slot(view, |mut item: $component, id| {
                    item.slots.trailing = Some(id);
                    item
                })
            }
        }
    )*};
}

item_slots!(ListItem, crate::SidebarRow);

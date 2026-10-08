//! Styles compiled from CSS at build time.
//!
//! The `.vue` compiler (and `css!`) parse and match a view's stylesheet
//! while the application builds, with the CSS engine of `nana-ui-css`. What
//! reaches the running program is data: for each element, the rules that
//! apply to it in cascade order, each as a [`StylePatch`] (the Style Model
//! fields its declarations set) and the conditional classes it needs. No
//! CSS is parsed and no selector is matched at run time, and this crate has
//! no CSS in it.
//!
//! A [`Sheet`] is a stylesheet's class rules in cascade order. An element
//! names its classes with [`El::class`] and [`El::class_when`]; the sheet
//! picks the rules those classes can match, once per distinct set of
//! classes, into a [`StyleSite`]. The site composes the element's base
//! layout with the patches whose classes are active, once per distinct base
//! and active set, and hands out the same shared layout to every instance
//! after that.

use std::panic::Location;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use nana_ui_core::LayoutStyle;
use serde_json::Value;

use super::controls::StyledComponent;
use super::node::{El, NodeBindings};
use super::prop::{FieldWrite, IntoProp, PropSource};
use super::transition::Implicit;
use crate::ComponentView;

/// The Style Model fields one rule's declarations write, as a JSON object
/// from each field's dotted path (`align_items`, `paint.outline.width`) to
/// its value: produced by the build, read once. A field written with the
/// value the default layout has is in it too, so a class can reset what
/// the element was built with.
pub struct StylePatch {
    json: &'static str,
    value: OnceLock<Value>,
}

impl StylePatch {
    #[doc(hidden)]
    pub const fn new(json: &'static str) -> Self {
        Self {
            json,
            value: OnceLock::new(),
        }
    }

    fn value(&self) -> &Value {
        self.value.get_or_init(|| {
            serde_json::from_str(self.json).expect("a compiled style patch is valid JSON")
        })
    }
}

/// One element's compiled rules, in cascade order: each patch applies
/// while every conditional class in its mask is active (bit `i` is the
/// element's `i`-th conditional class).
pub struct StyleSite {
    rules: &'static [(u64, &'static StylePatch)],
    /// `(base, active classes) -> composed`, shared by every instance.
    composed: Mutex<Vec<(Arc<LayoutStyle>, u64, Arc<LayoutStyle>)>>,
}

impl StyleSite {
    #[doc(hidden)]
    pub const fn new(rules: &'static [(u64, &'static StylePatch)]) -> Self {
        Self {
            rules,
            composed: Mutex::new(Vec::new()),
        }
    }

    /// The base layout equal to `base` that this site has composed from
    /// before, or `base` itself. A binding that keeps its base holds this
    /// one: every instance then shares it, and composing finds it by
    /// pointer instead of comparing whole layouts.
    fn shared_base(&self, base: &Arc<LayoutStyle>) -> Arc<LayoutStyle> {
        let composed = self.composed.lock().unwrap_or_else(PoisonError::into_inner);
        composed
            .iter()
            .find(|(known, _, _)| Arc::ptr_eq(known, base) || **known == **base)
            .map_or_else(|| Arc::clone(base), |(known, _, _)| Arc::clone(known))
    }

    /// `base` with the patches that apply under `active`, in order.
    pub fn compose(&self, base: &Arc<LayoutStyle>, active: u64) -> Arc<LayoutStyle> {
        let mut composed = self.composed.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, _, layout)) = composed.iter().find(|(known, mask, _)| {
            *mask == active && (Arc::ptr_eq(known, base) || **known == **base)
        }) {
            return Arc::clone(layout);
        }
        let applying = self
            .rules
            .iter()
            .filter(|(needs, _)| needs & !active == 0)
            .map(|(_, patch)| patch.value());
        let layout = Arc::new(apply(base, applying));
        composed.push((Arc::clone(base), active, Arc::clone(&layout)));
        layout
    }
}

/// A stylesheet compiled at build time (`stylesheet!`, a view's `<style>`):
/// its class rules and transitions, each in cascade order.
pub struct Sheet {
    rules: &'static [SheetRule],
    transitions: &'static [SheetTransition],
    /// `(fixed classes, conditional classes) -> (site, animations)`.
    resolved: Mutex<Vec<Resolved>>,
}

type Resolved = (Vec<u16>, Vec<u16>, &'static StyleSite, &'static [Implicit]);

/// One rule: the classes its selector needs and its patch.
#[doc(hidden)]
pub struct SheetRule {
    pub classes: &'static [u16],
    pub patch: &'static StylePatch,
}

/// One `transition`: the classes its selector needs and what it animates.
#[doc(hidden)]
pub struct SheetTransition {
    pub classes: &'static [u16],
    pub animate: &'static [Implicit],
}

impl Sheet {
    #[doc(hidden)]
    pub const fn new(rules: &'static [SheetRule], transitions: &'static [SheetTransition]) -> Self {
        Self {
            rules,
            transitions,
            resolved: Mutex::new(Vec::new()),
        }
    }

    /// The rules an element with these classes can match, with the bit of
    /// each conditional class they need, and the winning transition among
    /// those its fixed classes match.
    fn resolve(
        &self,
        fixed: &[u16],
        conditional: &[u16],
    ) -> (&'static StyleSite, &'static [Implicit]) {
        let mut resolved = self.resolved.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, _, site, animate)) = resolved
            .iter()
            .find(|(f, c, _, _)| f.as_slice() == fixed && c.as_slice() == conditional)
        {
            return (site, animate);
        }
        let bit = |class: &u16| {
            conditional
                .iter()
                .position(|c| c == class)
                .map_or(0, |at| 1u64 << at)
        };
        let rules: Vec<(u64, &'static StylePatch)> = self
            .rules
            .iter()
            .filter(|rule| {
                rule.classes
                    .iter()
                    .all(|class| fixed.contains(class) || conditional.contains(class))
            })
            .map(|rule| {
                let needs = rule
                    .classes
                    .iter()
                    .filter(|class| !fixed.contains(class))
                    .fold(0, |needs, class| needs | bit(class));
                (needs, rule.patch)
            })
            .collect();
        // One per distinct class set an element is written with: bounded
        // by the program's source, not by how many nodes it builds.
        let site: &'static StyleSite = Box::leak(Box::new(StyleSite::new(Box::leak(
            rules.into_boxed_slice(),
        ))));
        let animate = self
            .transitions
            .iter()
            .rfind(|t| t.classes.iter().all(|class| fixed.contains(class)))
            .map_or(&[][..], |t| t.animate);
        resolved.push((fixed.to_vec(), conditional.to_vec(), site, animate));
        (site, animate)
    }
}

/// One class of a [`Sheet`], as `stylesheet!` names it: `styles::card`.
#[derive(Clone, Copy)]
pub struct Class {
    sheet: &'static Sheet,
    index: u16,
}

impl Class {
    #[doc(hidden)]
    pub const fn new(sheet: &'static Sheet, index: u16) -> Self {
        Self { sheet, index }
    }
}

/// The classes [`El::class`] and [`El::class_when`] gave an element.
pub(crate) struct Classes<C> {
    sheet: &'static Sheet,
    fixed: Vec<u16>,
    conditional: Vec<(u16, PropSource<bool>)>,
    apply: ApplySite<C>,
    at: &'static Location<'static>,
}

type ApplySite<C> = fn(
    &mut C,
    &mut NodeBindings<C>,
    &'static StyleSite,
    Vec<PropSource<bool>>,
    &'static Location<'static>,
);

impl<C> Classes<C> {
    /// Write the element's styles into it, and return its animations.
    pub(crate) fn apply(
        self,
        component: &mut C,
        bindings: &mut NodeBindings<C>,
    ) -> &'static [Implicit] {
        let indices: Vec<u16> = self.conditional.iter().map(|(class, _)| *class).collect();
        let (site, animate) = self.sheet.resolve(&self.fixed, &indices);
        let sources = self
            .conditional
            .into_iter()
            .map(|(_, source)| source)
            .collect();
        (self.apply)(component, bindings, site, sources, self.at);
        animate
    }
}

/// `site` over the component's layout as built; the `i`-th condition
/// switches the site's `i`-th conditional class.
fn apply_site<C: StyledComponent + ComponentView>(
    component: &mut C,
    bindings: &mut NodeBindings<C>,
    site: &'static StyleSite,
    conditions: Vec<PropSource<bool>>,
    at: &'static Location<'static>,
) {
    let base = &component.node_style().layout;
    if conditions.is_empty() {
        let composed = site.compose(base, 0);
        <ComposedLayout as FieldWrite<C, Arc<LayoutStyle>>>::write(component, composed);
        return;
    }
    let base = site.shared_base(base);
    let composed = move || {
        let active = conditions
            .iter()
            .enumerate()
            .fold(0u64, |mask, (bit, class)| match class.get() {
                true => mask | 1 << bit,
                false => mask,
            });
        site.compose(&base, active)
    };
    composed.bind_field::<C, ComposedLayout>(component, bindings, at);
}

fn apply<'a>(base: &LayoutStyle, patches: impl Iterator<Item = &'a Value>) -> LayoutStyle {
    let mut value = serde_json::to_value(base).expect("a layout serializes");
    for patch in patches {
        let Value::Object(fields) = patch else {
            continue;
        };
        for (path, field) in fields {
            *field_mut(&mut value, path) = field.clone();
        }
    }
    serde_json::from_value(value).expect("a patched layout deserializes")
}

/// The field a patch key names: a dotted path through the Style Model's
/// structs (`paint.outline.width`). The field itself, an enum or an
/// `Option` included, is replaced whole: merging a `{"Px": 12}` into a
/// `{"Percent": 50}` would be neither.
fn field_mut<'a>(layout: &'a mut Value, path: &str) -> &'a mut Value {
    path.split('.').fold(layout, |value, key| {
        if !value.is_object() {
            *value = Value::Object(Default::default());
        }
        value
            .as_object_mut()
            .expect("made an object above")
            .entry(key)
            .or_insert(Value::Null)
    })
}

/// Writes a composed layout, keeping what `.visible` decided.
#[doc(hidden)]
pub struct ComposedLayout;

impl<C: StyledComponent> FieldWrite<C, Arc<LayoutStyle>> for ComposedLayout {
    const FIELD: &'static str = "style.layout";

    fn write(target: &mut C, mut layout: Arc<LayoutStyle>) {
        let style = target.node_style_mut();
        if style.layout.hidden != layout.hidden {
            Arc::make_mut(&mut layout).hidden = style.layout.hidden;
        }
        style.layout = layout;
    }

    fn differs(target: &C, layout: &Arc<LayoutStyle>) -> bool {
        let current = &target.node_style().layout;
        !Arc::ptr_eq(current, layout) && (current.hidden != layout.hidden || **current != **layout)
    }
}

impl<C: StyledComponent + ComponentView, K> El<C, K> {
    /// Give the element `class` of a stylesheet:
    ///
    /// ```ignore
    /// stylesheet! {
    ///     mod styles;
    ///     .card { padding: 12px; }
    ///     .card.done { opacity: 0.5; }
    /// }
    /// widget(card).class(styles::card).class_when(styles::done, done)
    /// ```
    ///
    /// Every class of one element comes from one sheet. Its rules apply
    /// over the layout the element was built with, in the sheet's cascade
    /// order; a `transition` on its fixed classes animates what bindings
    /// change.
    #[track_caller]
    pub fn class(self, class: Class) -> Self {
        self.class_at(class, Location::caller())
    }

    /// `class` while `condition` holds (Vue's `:class="{ done: … }"`).
    #[track_caller]
    pub fn class_when(self, class: Class, condition: impl IntoProp<bool>) -> Self {
        self.class_when_at(class, condition.into_source(), Location::caller())
    }

    fn class_at(self, class: Class, at: &'static Location<'static>) -> Self {
        self.with_classes(class, at, |classes, index| classes.fixed.push(index))
    }

    fn class_when_at(
        self,
        class: Class,
        condition: PropSource<bool>,
        at: &'static Location<'static>,
    ) -> Self {
        self.with_classes(class, at, move |classes, index| {
            if classes.conditional.len() < 64 {
                classes.conditional.push((index, condition));
            }
        })
    }

    fn with_classes(
        mut self,
        class: Class,
        at: &'static Location<'static>,
        add: impl FnOnce(&mut Classes<C>, u16),
    ) -> Self {
        let classes = self.classes_mut().get_or_insert_with(|| Classes {
            sheet: class.sheet,
            fixed: Vec::new(),
            conditional: Vec::new(),
            apply: apply_site::<C>,
            at,
        });
        assert!(
            std::ptr::eq(classes.sheet, class.sheet),
            "an element's classes come from one stylesheet"
        );
        add(classes, class.index);
        self
    }

    /// A compiled site over the element's layout as built, now (`css!`).
    fn styles(mut self, site: &'static StyleSite, at: &'static Location<'static>) -> Self {
        let (component, bindings) = self.parts_mut();
        apply_site(component, bindings, site, Vec::new(), at);
        self
    }
}

/// One declaration block compiled at build time (`css! { … }`): its patch
/// and its implicit animations. See [`El::css`].
#[derive(Clone, Copy)]
pub struct InlineStyle {
    site: Option<&'static StyleSite>,
    animate: &'static [super::Implicit],
}

impl InlineStyle {
    #[doc(hidden)]
    pub const fn new(
        site: Option<&'static StyleSite>,
        animate: &'static [super::Implicit],
    ) -> Self {
        Self { site, animate }
    }
}

impl<C: StyledComponent + ComponentView, K> El<C, K> {
    /// A declaration block compiled by `css!`: `column().gap(8.0).children(rows)
    /// .css(css! { padding: 12px; transition: opacity 150ms })`. Call it
    /// before other layout props.
    #[track_caller]
    pub fn css(self, style: InlineStyle) -> Self {
        self.css_at(style, Location::caller())
    }

    fn css_at(self, style: InlineStyle, at: &'static Location<'static>) -> Self {
        let styled = match style.site {
            Some(site) => self.styles(site, at),
            None => self,
        };
        styled.animate(style.animate.iter().copied())
    }
}

/// What [`El::class`], [`El::class_when`] and [`El::css`] give the
/// container of a structural view (`each`, `when`, `dynamic`,
/// `each_virtual`), kept until the view builds its container.
#[derive(Default)]
pub(crate) struct ContainerStyle(Vec<ContainerStyleOp>);

enum ContainerStyleOp {
    Class(Class, &'static Location<'static>),
    ClassWhen(Class, PropSource<bool>, &'static Location<'static>),
    Css(InlineStyle, &'static Location<'static>),
    Visible(PropSource<bool>, &'static Location<'static>),
}

impl ContainerStyle {
    /// `element` with these styles, in the order they were given.
    pub(crate) fn apply<C: StyledComponent + ComponentView>(self, element: El<C>) -> El<C> {
        self.0.into_iter().fold(element, |element, op| match op {
            ContainerStyleOp::Class(class, at) => element.class_at(class, at),
            ContainerStyleOp::ClassWhen(class, condition, at) => {
                element.class_when_at(class, condition, at)
            }
            ContainerStyleOp::Css(style, at) => element.css_at(style, at),
            ContainerStyleOp::Visible(visible, at) => element.visible_at(visible, at),
        })
    }
}

/// `.class`, `.class_when`, `.css` and `.visible` on a structural view: they
/// style the container its rows or branches are built in, as they style an
/// element.
macro_rules! container_styles {
    ($([$($generics:tt)*] $view:ty),* $(,)?) => {$(
        impl<$($generics)*> $view {
            /// Give the container the rows or branches are built in `class`
            /// of a stylesheet, as [`El::class`] gives an element one.
            #[track_caller]
            pub fn class(mut self, class: $crate::view::Class) -> Self {
                self.container_style_mut().push_class(class, ::std::panic::Location::caller());
                self
            }

            /// `class` on the container while `condition` holds.
            #[track_caller]
            pub fn class_when(
                mut self,
                class: $crate::view::Class,
                condition: impl $crate::view::IntoProp<bool>,
            ) -> Self {
                self.container_style_mut().push_class_when(
                    class,
                    condition.into_source(),
                    ::std::panic::Location::caller(),
                );
                self
            }

            /// A `css!` block on the container, as [`El::css`] on an
            /// element.
            #[track_caller]
            pub fn css(mut self, style: $crate::view::InlineStyle) -> Self {
                self.container_style_mut().push_css(style, ::std::panic::Location::caller());
                self
            }

            /// Keep the container, and what is built in it, but take it out
            /// of layout, paint and hit testing while `visible` is false
            /// (`v-show`), as [`El::visible`] does an element's.
            #[track_caller]
            pub fn visible(mut self, visible: impl $crate::view::IntoProp<bool>) -> Self {
                self.container_style_mut().push_visible(
                    visible.into_source(),
                    ::std::panic::Location::caller(),
                );
                self
            }
        }
    )*};
}
pub(crate) use container_styles;

impl ContainerStyle {
    pub(crate) fn push_class(&mut self, class: Class, at: &'static Location<'static>) {
        self.0.push(ContainerStyleOp::Class(class, at));
    }

    pub(crate) fn push_class_when(
        &mut self,
        class: Class,
        condition: PropSource<bool>,
        at: &'static Location<'static>,
    ) {
        self.0
            .push(ContainerStyleOp::ClassWhen(class, condition, at));
    }

    pub(crate) fn push_css(&mut self, style: InlineStyle, at: &'static Location<'static>) {
        self.0.push(ContainerStyleOp::Css(style, at));
    }

    pub(crate) fn push_visible(
        &mut self,
        visible: PropSource<bool>,
        at: &'static Location<'static>,
    ) {
        self.0.push(ContainerStyleOp::Visible(visible, at));
    }
}

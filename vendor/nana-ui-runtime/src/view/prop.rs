//! Props: a constant, a signal, or a closure, bound to one component field.
//!
//! A constant is written into the component before it is created and costs
//! nothing afterwards. A signal or computed is a direct binding: a field
//! writer and the signal's id, no closure. A closure is boxed once.

use std::any::Any;
use std::cell::RefCell;
use std::panic::Location;
use std::sync::Arc;

use std::marker::PhantomData;

use super::node::{DirectBinding, DynBinding, DynProp, NodeBindings};
use super::reactive::{Computed, Const, Signal};

/// Writes one field of `C`. The view layer generates these per bindable
/// field; applications write their own for [`super::El::prop`].
pub trait FieldWrite<C, T> {
    /// `Component.field`, for traces.
    const FIELD: &'static str;
    fn write(target: &mut C, value: T);

    /// Whether writing `value` would change `target`. A binding whose
    /// fields all compare equal costs no copy and no projection; the default
    /// assumes every write changes something.
    fn differs(target: &C, value: &T) -> bool {
        let _ = (target, value);
        true
    }
}

/// A value, signal or closure that can drive a `T` field.
pub trait IntoProp<T: 'static>: Sized {
    #[doc(hidden)]
    fn bind_field<C: 'static, W: FieldWrite<C, T> + 'static>(
        self,
        target: &mut C,
        bindings: &mut NodeBindings<C>,
        site: &'static Location<'static>,
    );

    #[doc(hidden)]
    fn into_source(self) -> PropSource<T>;
}

/// A prop kept for later evaluation (conditions, list sources).
pub enum PropSource<T: 'static> {
    Const(T),
    Signal(Signal<T>),
    Computed(Computed<T>),
    Dyn(Box<dyn Fn() -> T + Send>),
}

impl<T: Clone + 'static> PropSource<T> {
    /// The current value, tracked by the innermost effect.
    #[track_caller]
    pub fn get(&self) -> T {
        match self {
            Self::Const(value) => value.clone(),
            Self::Signal(signal) => signal.get(),
            Self::Computed(computed) => computed.get(),
            Self::Dyn(f) => f(),
        }
    }
}

fn apply_cell<C, T: Clone + 'static, W: FieldWrite<C, T>>(target: &mut C, cell: &dyn Any) {
    let cell = cell
        .downcast_ref::<RefCell<T>>()
        .expect("signal cell holds its own type");
    W::write(target, cell.borrow().clone());
}

fn differs_cell<C, T: 'static, W: FieldWrite<C, T>>(target: &C, cell: &dyn Any) -> bool {
    let cell = cell
        .downcast_ref::<RefCell<T>>()
        .expect("signal cell holds its own type");
    W::differs(target, &cell.borrow())
}

fn bind_direct<C, T: Clone + 'static, W: FieldWrite<C, T>>(
    source: super::reactive::SignalKey,
    bindings: &mut NodeBindings<C>,
    site: &'static Location<'static>,
) {
    bindings.direct.push(DirectBinding {
        source,
        apply: apply_cell::<C, T, W>,
        differs: differs_cell::<C, T, W>,
        field: W::FIELD,
        site,
    });
}

/// A prop kept for later binds as what it was given as: a constant is
/// written, a signal or computed binds directly, a closure is boxed.
impl<T: Clone + Send + 'static> IntoProp<T> for PropSource<T> {
    fn bind_field<C: 'static, W: FieldWrite<C, T> + 'static>(
        self,
        target: &mut C,
        bindings: &mut NodeBindings<C>,
        site: &'static Location<'static>,
    ) {
        match self {
            Self::Const(value) => W::write(target, value),
            Self::Signal(signal) => signal.bind_field::<C, W>(target, bindings, site),
            Self::Computed(computed) => computed.bind_field::<C, W>(target, bindings, site),
            Self::Dyn(f) => f.bind_field::<C, W>(target, bindings, site),
        }
    }

    fn into_source(self) -> PropSource<T> {
        self
    }
}

impl<T: Clone + 'static> IntoProp<T> for Signal<T> {
    fn bind_field<C: 'static, W: FieldWrite<C, T> + 'static>(
        self,
        _: &mut C,
        bindings: &mut NodeBindings<C>,
        site: &'static Location<'static>,
    ) {
        bind_direct::<C, T, W>(self.key, bindings, site);
    }

    fn into_source(self) -> PropSource<T> {
        PropSource::Signal(self)
    }
}

impl<T: Clone + 'static> IntoProp<T> for Computed<T> {
    fn bind_field<C: 'static, W: FieldWrite<C, T> + 'static>(
        self,
        _: &mut C,
        bindings: &mut NodeBindings<C>,
        site: &'static Location<'static>,
    ) {
        bind_direct::<C, T, W>(self.key, bindings, site);
    }

    fn into_source(self) -> PropSource<T> {
        PropSource::Computed(self)
    }
}

impl<T: Clone + 'static> IntoProp<T> for Const<T> {
    fn bind_field<C: 'static, W: FieldWrite<C, T> + 'static>(
        self,
        target: &mut C,
        bindings: &mut NodeBindings<C>,
        site: &'static Location<'static>,
    ) {
        Fixed(self.get()).bind_field::<C, W>(target, bindings, site);
    }

    fn into_source(self) -> PropSource<T> {
        Fixed(self.get()).into_source()
    }
}

/// A value of any type used as a constant prop. The `.vue` compiler wraps
/// expressions it proved read no signal in it, so they are written once
/// instead of becoming a binding.
#[doc(hidden)]
pub struct Fixed<T>(pub T);

impl<T: 'static> IntoProp<T> for Fixed<T> {
    fn bind_field<C: 'static, W: FieldWrite<C, T> + 'static>(
        self,
        target: &mut C,
        _: &mut NodeBindings<C>,
        _: &'static Location<'static>,
    ) {
        W::write(target, self.0);
    }

    fn into_source(self) -> PropSource<T> {
        PropSource::Const(self.0)
    }
}

/// A closure prop: evaluated once per run, the value kept until written.
struct ClosureProp<F, T, W> {
    f: F,
    pending: Option<T>,
    _writer: PhantomData<fn() -> W>,
}

impl<C, T, F, W> DynProp<C> for ClosureProp<F, T, W>
where
    T: Send,
    F: Fn() -> T + Send,
    W: FieldWrite<C, T>,
{
    fn evaluate(&mut self, current: &C) -> bool {
        let value = (self.f)();
        let differs = W::differs(current, &value);
        self.pending = Some(value);
        differs
    }

    fn write(&mut self, target: &mut C) {
        if let Some(value) = self.pending.take() {
            W::write(target, value);
        }
    }

    fn discard(&mut self) {
        self.pending = None;
    }
}

impl<T: Send + 'static, F: Fn() -> T + Send + 'static> IntoProp<T> for F {
    fn bind_field<C: 'static, W: FieldWrite<C, T> + 'static>(
        self,
        _: &mut C,
        bindings: &mut NodeBindings<C>,
        site: &'static Location<'static>,
    ) {
        bindings.dynamic.push(DynBinding {
            prop: Box::new(ClosureProp::<F, T, W> {
                f: self,
                pending: None,
                _writer: PhantomData,
            }),
            field: W::FIELD,
            site,
        });
    }

    fn into_source(self) -> PropSource<T> {
        PropSource::Dyn(Box::new(self))
    }
}

macro_rules! const_props {
    ($($from:ty => $to:ty, |$value:ident| $convert:expr;)*) => {$(
        impl IntoProp<$to> for $from {
            fn bind_field<C: 'static, W: FieldWrite<C, $to> + 'static>(
                self,
                target: &mut C,
                _: &mut NodeBindings<C>,
                _: &'static Location<'static>,
            ) {
                let $value = self;
                W::write(target, $convert);
            }

            fn into_source(self) -> PropSource<$to> {
                let $value = self;
                PropSource::Const($convert)
            }
        }
    )*};
}

const_props! {
    String => String, |v| v;
    &'static str => String, |v| v.to_owned();
    Arc<str> => Arc<str>, |v| v;
    &'static str => Arc<str>, |v| Arc::from(v);
    String => Arc<str>, |v| Arc::from(v);
    bool => bool, |v| v;
    f32 => f32, |v| v;
    f64 => f64, |v| v;
    i32 => i32, |v| v;
    i64 => i64, |v| v;
    u32 => u32, |v| v;
    u64 => u64, |v| v;
    usize => usize, |v| v;
    Option<Arc<str>> => Option<Arc<str>>, |v| v;
    Option<&'static str> => Option<Arc<str>>, |v| v.map(Arc::from);
    &'static str => Option<Arc<str>>, |v| Some(Arc::from(v));
    String => Option<Arc<str>>, |v| Some(Arc::from(v));
    Vec<crate::SelectOption> => Vec<crate::SelectOption>, |v| v;
    crate::StableNodeId => Option<crate::StableNodeId>, |v| Some(v);
    Option<crate::StableNodeId> => Option<crate::StableNodeId>, |v| v;
    nana_ui_core::SemanticColorRole => Option<nana_ui_core::SemanticColorRole>, |v| Some(v);
    Option<nana_ui_core::SemanticColorRole> => Option<nana_ui_core::SemanticColorRole>, |v| v;
    nana_ui_core::RadiusTier => Option<nana_ui_core::RadiusTier>, |v| Some(v);
    Option<nana_ui_core::RadiusTier> => Option<nana_ui_core::RadiusTier>, |v| v;
    nana_ui_core::Icon => nana_ui_core::Icon, |v| v;
    nana_ui_core::Icon => Option<nana_ui_core::Icon>, |v| Some(v);
    Option<nana_ui_core::Icon> => Option<nana_ui_core::Icon>, |v| v;
    nana_ui_core::StatusTone => nana_ui_core::StatusTone, |v| v;
}

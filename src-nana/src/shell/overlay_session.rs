//! 浮层块的会话：一块浮层从挂上到换下，浮层块经它同步投影、激活对话框、交还焦点。
//!
//! 浮层改成常驻以后，每种浮层的信号建在这块浮层自己的挂载作用域里，随浮层一起回收。视图函数
//! 建的时候把会话交给 [`register`]，浮层块挂这块内容时用 [`collect`] 收下：之后每次整体同步调
//! [`OverlaySession::write`] 写投影，挂好后调 [`OverlaySession::placed`]，换下前调
//! [`OverlaySession::retire`]。视图函数的签名因此不变，壳层其余模块不必知道浮层怎么同步。

use std::cell::RefCell;

use nana_ui::runtime::AppContext;

use crate::shell::ShellViewModel;

/// 一块浮层留给浮层块的钩子。三个方法都有空的默认实现，只实现用得着的。
pub(crate) trait OverlaySession {
    /// 每次整体同步都调用，组合输入中也调用：把这一刻的投影写进信号，只写变了的。
    /// 浮层这一刻已经关了时，投影取不到，什么也不写，同一次同步里浮层块会把它换下。
    fn write(&mut self, _model: &ShellViewModel) {}

    /// 这块浮层挂好了，即将放进槽位。对话框据此在下一次刷新时激活。
    fn placed(&mut self) {}

    /// 这块浮层要被换下。对话框先经框架关掉，焦点回到打开它之前的位置。
    fn retire(&mut self, _context: &mut AppContext) {}
}

thread_local! {
    /// 正在挂载的浮层块收下的会话。只在 [`collect`] 期间是 `Some`。
    static COLLECTING: RefCell<Option<Vec<Box<dyn OverlaySession>>>> = const { RefCell::new(None) };
}

/// 浮层视图建的时候登记会话。不在浮层块里建时（单测直接调用视图函数）没人收，直接丢掉。
pub(crate) fn register(session: impl OverlaySession + 'static) {
    COLLECTING.with(|cell| {
        if let Some(sessions) = cell.borrow_mut().as_mut() {
            sessions.push(Box::new(session));
        }
    });
}

/// 在 `build` 期间收下视图函数登记的会话。嵌套调用（或 `build` panic 展开）时恢复外层的收集。
pub(crate) fn collect<R>(build: impl FnOnce() -> R) -> (R, Vec<Box<dyn OverlaySession>>) {
    struct Restore(Option<Vec<Box<dyn OverlaySession>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let outer = self.0.take();
            COLLECTING.with(|cell| *cell.borrow_mut() = outer);
        }
    }
    let outer = COLLECTING.with(|cell| cell.borrow_mut().replace(Vec::new()));
    let restore = Restore(outer);
    let built = build();
    let collected = COLLECTING.with(|cell| cell.borrow_mut().take()).unwrap_or_default();
    drop(restore);
    (built, collected)
}

/// 最常见的会话：一个投影放在一个信号里。同步时按 ViewModel 重算，变了才写；取不到投影
/// （浮层已关）时不写。
pub(crate) struct Projected<P: 'static> {
    signal: nana_ui::runtime::view::Signal<P>,
    project: fn(&ShellViewModel) -> Option<P>,
}

impl<P: Clone + PartialEq + 'static> Projected<P> {
    /// 登记一个投影会话。`signal` 在浮层的挂载作用域里建好，`project` 和建信号时用的是同一个函数。
    pub(crate) fn register(signal: nana_ui::runtime::view::Signal<P>, project: fn(&ShellViewModel) -> Option<P>) {
        register(Self { signal, project });
    }
}

impl<P: Clone + PartialEq + 'static> OverlaySession for Projected<P> {
    fn write(&mut self, model: &ShellViewModel) {
        if let Some(view) = (self.project)(model) {
            self.signal.try_set_if_changed(view);
        }
    }
}

/// 受控输入的草稿：信号驱动输入框，ViewModel 的值相对上次投影变了才写回，见 [`crate::shell::hot::ModelField`]。
pub(crate) struct Draft {
    field: crate::shell::hot::ModelField,
    read: Box<dyn Fn(&ShellViewModel) -> Option<String>>,
}

impl Draft {
    /// 在浮层的挂载作用域里建草稿信号并登记同步；返回给 `.model(..)` 用的信号。
    /// `read` 取不到值（浮层已关）时不写。
    pub(crate) fn register(
        model: &ShellViewModel,
        read: impl Fn(&ShellViewModel) -> Option<String> + 'static,
    ) -> nana_ui::runtime::view::Signal<String> {
        let field = crate::shell::hot::ModelField::new(read(model).as_deref().unwrap_or_default());
        let signal = field.signal();
        register(Self { field, read: Box::new(read) });
        signal
    }
}

impl OverlaySession for Draft {
    fn write(&mut self, model: &ShellViewModel) {
        if let Some(value) = (self.read)(model) {
            self.field.sync(&value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{collect, register, OverlaySession};
    use crate::shell::ShellViewModel;

    struct Count(usize);

    impl OverlaySession for Count {
        fn write(&mut self, _: &ShellViewModel) {
            self.0 += 1;
        }
    }

    /// 只收 `collect` 期间登记的会话；嵌套的收集各收各的，外层不丢。
    #[test]
    fn collect_takes_what_its_build_registers() {
        register(Count(0));
        let (value, outer) = collect(|| {
            register(Count(0));
            let ((), inner) = collect(|| register(Count(0)));
            assert_eq!(inner.len(), 1, "内层只收自己的");
            register(Count(0));
            7
        });
        assert_eq!(value, 7);
        assert_eq!(outer.len(), 2, "外层收下前后两次登记");
        let ((), empty) = collect(|| {});
        assert!(empty.is_empty());
    }
}

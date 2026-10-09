//! 文件路由和检视面共用的绑定小件：整份样式字段、可复制的受控草稿、值绑定的事实行，
//! 以及结构块容器的整列排版。
//!
//! 常驻视图按投影绑定字段。样式里有好几处要一起变（放置态的底和边、展开排法、圆角半径），
//! 这些地方按投影现算整份样式，和旧视图用同一个构造函数，保证画面和整块重挂时一样。

use nana_ui::runtime::view::{css, fields, signal, untrack, widget, AnyView, FieldWrite, InlineStyle, IntoView, Signal, StyledComponent};
use nana_ui::runtime::{Button, IconGlyph, NodeStyle, ValidationMessage};
use nana_ui_core::Icon;

use super::super::hot::ModelField;
use super::style;

/// 整份节点样式。绑定闭包按投影现算样式，值不同才写。
///
/// `layout.hidden` 归 `.visible` 管：同一个节点不要同时绑 `.visible` 和整份样式，
/// 要两样都有时外面包一层。
pub(crate) struct StyleField;

impl<C: StyledComponent> FieldWrite<C, NodeStyle> for StyleField {
    const FIELD: &'static str = "style";

    fn write(target: &mut C, value: NodeStyle) {
        *target.node_style_mut() = value;
    }

    fn differs(target: &C, value: &NodeStyle) -> bool {
        target.node_style() != value
    }
}

/// 按钮文字前的图标。图标按几何指针比较。
pub(crate) struct ButtonIconField;

impl FieldWrite<Button, Option<Icon>> for ButtonIconField {
    const FIELD: &'static str = "Button.icon";

    fn write(target: &mut Button, value: Option<Icon>) {
        target.icon = value;
    }

    fn differs(target: &Button, value: &Option<Icon>) -> bool {
        target.icon.map(Icon::as_ptr) != value.map(Icon::as_ptr)
    }
}

/// 独立图标节点的图标。图标按几何指针比较。
pub(crate) struct GlyphIconField;

impl FieldWrite<IconGlyph, Icon> for GlyphIconField {
    const FIELD: &'static str = "IconGlyph.icon";

    fn write(target: &mut IconGlyph, value: Icon) {
        target.icon = value;
    }

    fn differs(target: &IconGlyph, value: &Icon) -> bool {
        target.icon.as_ptr() != value.as_ptr()
    }
}

/// 校验提示的文字。
pub(crate) struct ValidationTextField;

impl FieldWrite<ValidationMessage, String> for ValidationTextField {
    const FIELD: &'static str = "ValidationMessage.message";

    fn write(target: &mut ValidationMessage, value: String) {
        target.message = value.into();
    }

    fn differs(target: &ValidationMessage, value: &String) -> bool {
        &*target.message != value.as_str()
    }
}

/// 受控输入框的草稿：句柄可复制，能放进常驻信号组。输入框用 `.model(draft.signal())`，
/// 同步时 [`DraftField::sync`] 只在 ViewModel 的值相对上次投影变了才写回，见 [`ModelField`]。
#[derive(Clone, Copy)]
pub(crate) struct DraftField(Signal<ModelField>);

impl DraftField {
    /// 在常驻作用域里建草稿，初值是 ViewModel 当前的值。
    pub(crate) fn new(value: &str) -> Self {
        Self(signal(ModelField::new(value)))
    }

    /// 输入框绑定的信号。
    pub(crate) fn signal(&self) -> Signal<String> {
        self.0.with_untracked(ModelField::signal)
    }

    /// 输入框现在的文字（含还没归约的按键），事件处理器现读用。
    pub(crate) fn current(&self) -> String {
        self.signal().get_untracked()
    }

    /// ViewModel 的值相对上次投影变了才写进输入框。草稿已随文档回收时不写。
    pub(crate) fn sync(&self, value: &str) {
        if self.0.defined_at().is_none() {
            return;
        }
        self.0.update(|field| {
            field.sync(value);
        });
    }
}

/// 结构块容器的排版和 `Stack::fill_column(0)` 一样：占满父级剩余高度，里面的整列照旧铺满。
pub(crate) fn fill_container() -> InlineStyle {
    css! { height: 100%; min-height: 0; flex-grow: 1; flex-shrink: 1; }
}

/// `asset-meta__row`：小标签和值，值按字段绑定。`first` 是组里的第一行，不画上边线；
/// `shown` 为假时这一行不占位。详情卡片和元数据的只读行共用。
pub(crate) fn fact_row(
    label: &str,
    value: impl Fn() -> String + Send + Clone + 'static,
    key: &str,
    first: bool,
    shown: impl Fn() -> bool + Send + 'static,
) -> AnyView {
    widget(style::meta_row(first))
        .visible(shown)
        .children((
            widget(style::row_label(label)).key(format!("{key}-label")),
            widget(style::value(untrack(&value))).prop::<String, fields::text::value>(value).key(format!("{key}-value")),
        ))
        .key(key.to_string())
        .into_any()
}

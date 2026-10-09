//! 统一的对话框框架：侧栏、文件页、导出、插件删除和关闭确认的对话框都从这里建，和消息类型无关。
//!
//! 行为交给 NanaUI：对话框是 `Dialog` / `ConfirmDialog`，挂在自己的 `OverlayHost` 下，用
//! `activate_overlay` 打开，焦点陷阱、焦点归还、无障碍角色和初始焦点都由框架负责。关闭策略是
//! `DialogClosePolicy::requests_only()`：Escape、点外面和关闭位只在对话框上发一次
//! `DialogCloseRequested`，这里把它换成调用方给的关闭消息，处理中（busy）时不发，开合由归约决定。
//!
//! 激活的时机：浮层块把内容挂成脱离树的一块，`ShellView` 之后才把它放进 AppShell 的浮层槽位，
//! 挂载时的 `on_mount` 运行时宿主还不在树里。所以对话框旁边放一个 `when(placed, ..)`：浮层块挂好后
//! 经 [`OverlaySession::placed`] 置真，下一次刷新建出分支，分支的 `on_mount` 运行时宿主已经在树里，
//! 这时激活。换下前经 [`OverlaySession::retire`] 让框架关掉对话框，焦点回到打开前的位置。
//!
//! 外观照 Vue `.modal-card` / `.dialog-card__*`：每个对话框照自己的宽度类给 CSS 宽度，危险对话框用
//! NanaUI 的危险口气，Vue 标题前有图标的放进标题图标槽；遮罩、分隔线、内边距、距顶 12vh、卡片圆角
//! 和开合动效由主题配方决定（`appearance.rs`），差异见 `docs/nana-vue-parity.md`。

use std::sync::Arc;

use nana_ui::icons_tabler::{LOADER_2, X};
use nana_ui::runtime::view::{
    entity_ref, fields, on_mount, signal, when, widget, AnyView, El, EntityRef, FieldWrite, IntoProp, IntoView, Signal,
};
use nana_ui::runtime::{
    Activate, AlignSpec, AppContext, Button, ConfirmDialog, ConfirmIntent, Dialog, DialogCloseRequested, IconButton,
    IconGlyph, LengthSpec, ListItem, NodeStyle, OverlayHost, RadiusTier, SemanticColorRole, Stack, Text,
};
use nana_ui::{ButtonKind, ControlSize, Icon};
use nana_ui_core::{DialogClosePolicy, DialogSize};

use super::session::{register, OverlaySession};
use crate::shell::ShellMessage;

#[path = "overlay_dialog_fields.rs"]
mod form;
pub(crate) use form::{select_field, text_area_field, text_field, two_columns, Choices};

/// 处理中的判断，事件处理器和绑定都要读，所以共享一份。
pub(crate) type BusyFn = Arc<dyn Fn() -> bool + Send + Sync>;
/// 现做一条壳层消息。`ShellMessage` 不能克隆，每次手势现做。
pub(crate) type MessageFn = Arc<dyn Fn() -> ShellMessage + Send + Sync>;

/// `.modal-card { width: min(520px, 92vw) }`：没有自己宽度类的对话框都这么宽。
pub(crate) const MODAL_CARD: DialogSize = DialogSize::capped(520.0, 92.0);

/// 标题前的图标：Vue 头部的图标都是 14px。
const TITLE_ICON_SIZE: f32 = 14.0;

/// 一个对话框的外壳：键、标题、口气、宽度、标题图标、处理中和关闭时发的消息。
pub(crate) struct DialogFrame {
    key: &'static str,
    title: Box<dyn Fn() -> String + Send>,
    danger: bool,
    size: DialogSize,
    icon: Option<Icon>,
    busy: BusyFn,
    close: MessageFn,
    close_button: bool,
}

impl DialogFrame {
    /// `title` 可以读信号，打开期间变了只改标题字段；`close` 是三种关闭手势发的消息。
    pub(crate) fn new(
        key: &'static str,
        title: impl Fn() -> String + Send + 'static,
        close: impl Fn() -> ShellMessage + Send + Sync + 'static,
    ) -> Self {
        Self {
            key,
            title: Box::new(title),
            danger: false,
            size: MODAL_CARD,
            icon: None,
            busy: Arc::new(|| false),
            close: Arc::new(close),
            close_button: false,
        }
    }

    /// 危险口气（Vue `.dialog-card__header--danger`）：标题和标题前的图标用主题的危险色，正文不变。
    pub(crate) fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// 卡片宽度，照 Vue 这个对话框的宽度类写成 CSS 长度，例如 `min(560px, 92vw)` 是
    /// `DialogSize::capped(560.0, 92.0)`。
    pub(crate) fn size(mut self, size: DialogSize) -> Self {
        self.size = size;
        self
    }

    /// 标题前的图标（Vue 标题前的 Lucide 图标换成对应的 Tabler 图标）。
    pub(crate) fn title_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// 处理中：三种关闭手势都不发关闭消息。在手势发生时现读。
    pub(crate) fn busy(mut self, busy: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.busy = Arc::new(busy);
        self
    }

    /// 标题右侧放关闭位（导出对话框）。点它和 Escape 一样只发关闭请求。
    pub(crate) fn close_button(mut self) -> Self {
        self.close_button = true;
        self
    }

    /// 普通对话框：标题下面是 `body`，底栏是 `footer`（一般是 [`footer`] 建的按钮行）。
    pub(crate) fn dialog(self, body: impl IntoView, footer_view: impl IntoView) -> AnyView {
        let Self { key, title, danger, size, icon, busy, close, close_button } = self;
        let host = entity_ref::<OverlayHost>();
        let surface = entity_ref::<Dialog>();
        let placed = signal(false);
        register(Activation { placed, host });
        let dialog = Dialog::new(title()).size(size).danger(danger).close_policy(DialogClosePolicy::requests_only());
        let mut element = widget(dialog)
            .entity_ref(surface)
            .key("dialog-surface")
            .prop::<String, DialogTitle>(title)
            .on_cx({
                let busy = busy.clone();
                move |_, request: &DialogCloseRequested, cx| {
                    if busy() {
                        eprintln!("Nana 对话框处理中，不响应关闭手势：{:?}", request.trigger);
                        return;
                    }
                    cx.dispatch_program_all(close());
                }
            })
            .body(body)
            .footer(footer_view);
        if let Some(icon) = icon {
            element = element.title_icon(title_icon(icon, danger));
        }
        if close_button {
            element = element.close_action(close_affordance(busy));
        }
        frame(key, host, element.into_any(), activator(placed, move |cx| {
            if let (Some(host), Some(surface)) = (host.get(), surface.get()) {
                cx.activate_overlay(host, surface).map(|_| ())
            } else {
                Err(missing())
            }
        }))
    }

    /// 确认框：标题、一句说明、取消和确认两个按钮（调用方建好，按钮自己不发消息，点击由框架
    /// 变成 `ConfirmIntent`）。取消和三种关闭手势发关闭消息，确认发 `on_confirm`，处理中都不发。
    pub(crate) fn confirm(
        self,
        message: impl Fn() -> String + Send + 'static,
        cancel: impl IntoView,
        confirm: impl IntoView,
        on_confirm: impl Fn() -> ShellMessage + Send + Sync + 'static,
    ) -> AnyView {
        let Self { key, title, danger, size, icon, busy, close, close_button: _ } = self;
        let host = entity_ref::<OverlayHost>();
        let surface = entity_ref::<ConfirmDialog>();
        let placed = signal(false);
        register(Activation { placed, host });
        let dialog = ConfirmDialog::new(title(), message()).size(size).danger(danger).close_policy(DialogClosePolicy::requests_only());
        let gesture_close = close.clone();
        let gesture_busy = busy.clone();
        let mut element = widget(dialog)
            .entity_ref(surface)
            .key("dialog-surface")
            .prop::<String, ConfirmTitle>(title)
            .prop::<String, ConfirmMessage>(message)
            .prop::<bool, ConfirmBusy>({
                let busy = busy.clone();
                move || busy()
            })
            .on_cx(move |_, request: &DialogCloseRequested, cx| {
                if gesture_busy() {
                    eprintln!("Nana 确认框处理中，不响应关闭手势：{:?}", request.trigger);
                    return;
                }
                cx.dispatch_program_all(gesture_close());
            })
            .on_cx(move |_, intent: &ConfirmIntent, cx| {
                if busy() {
                    eprintln!("Nana 确认框处理中，不响应按钮：{intent:?}");
                    return;
                }
                match intent {
                    ConfirmIntent::Cancel => cx.dispatch_program_all(close()),
                    ConfirmIntent::Confirm { .. } => cx.dispatch_program_all(on_confirm()),
                    ConfirmIntent::Secondary => {}
                }
            })
            .cancel(cancel)
            .confirm(confirm);
        if let Some(icon) = icon {
            element = element.title_icon(title_icon(icon, danger));
        }
        frame(key, host, element.into_any(), activator(placed, move |cx| {
            if let (Some(host), Some(surface)) = (host.get(), surface.get()) {
                cx.activate_overlay(host, surface).map(|_| ())
            } else {
                Err(missing())
            }
        }))
    }
}

/// 标题前的图标：14px，颜色跟着标题走（Vue 的图标继承头部的文字色，危险口气时是 `--err`）。
fn title_icon(icon: Icon, danger: bool) -> AnyView {
    let role = if danger { SemanticColorRole::Danger } else { SemanticColorRole::Text };
    widget(IconGlyph::new(icon).size(TITLE_ICON_SIZE).role(role)).key("dialog-icon").into_any()
}

/// 浮层槽位里的一块：铺满的外层（AppShell 给它打铺满窗口的补丁，有子节点时挡住下面的点击，
/// 在激活前也不会漏点），里面是对话框的宿主和激活用的结构块。遮罩的颜色、背景模糊和开合动效
/// 归主题（`appearance.rs` 的效果令牌和对话框配方）。外层上不放绑定：AppShell 给它打的布局补丁
/// 只在自己投影时写，绑定重投影会把补丁冲掉。
fn frame(key: &'static str, host: EntityRef<OverlayHost>, surface: AnyView, activator: AnyView) -> AnyView {
    let mut style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Fill);
    }
    widget(Stack::column(0.0))
        .key(key)
        .children((widget(OverlayHost::new().style(style)).entity_ref(host).key("dialog-host").children((surface,)), activator))
        .into_any()
}

/// 浮层块置真 `placed` 以后的那次刷新里建出分支，分支挂上时宿主已经在树里，这时激活。
fn activator(placed: Signal<bool>, activate: impl Fn(&mut AppContext) -> Result<(), nana_ui::runtime::FrameworkError> + Send + Sync + 'static) -> AnyView {
    let activate = Arc::new(activate);
    when(placed, move || {
        let activate = activate.clone();
        on_mount(move |cx| {
            if let Err(error) = activate(cx) {
                eprintln!("Nana 对话框激活失败：{error}");
            }
        });
        widget(Stack::column(0.0)).key("dialog-activated")
    })
    .into_any()
}

fn missing() -> nana_ui::runtime::FrameworkError {
    nana_ui::runtime::FrameworkError::InvalidInput
}

/// 对话框的会话：挂好后置真 `placed`，换下前让框架关掉对话框、交还焦点。
struct Activation {
    placed: Signal<bool>,
    host: EntityRef<OverlayHost>,
}

impl OverlaySession for Activation {
    fn placed(&mut self) {
        self.placed.try_set_if_changed(true);
    }

    fn retire(&mut self, context: &mut AppContext) {
        let Some(host) = self.host.get() else {
            return;
        };
        if let Err(error) = context.dismiss_overlay(host) {
            eprintln!("Nana 换下对话框时没能交还焦点：{error}");
        }
    }
}

/// 标题右侧的关闭位：24 见方、弱色叉号，悬停换底色（`.repository-export-dialog__close`）。
/// 点它由框架变成关闭请求，按钮自己不发消息；处理中显示为禁用。
fn close_affordance(busy: BusyFn) -> AnyView {
    let mut button = IconButton::new(X, "关闭").size(ControlSize::Small);
    button.style.radius = Some(RadiusTier::Sm);
    button.style.foreground = Some(SemanticColorRole::Faint);
    widget(button).key("dialog-close").prop::<bool, fields::icon_button::disabled>(move || busy()).into_any()
}

/// 底栏：左边可放一段说明（处理中），按钮靠右、间距 8。对应 `.dialog-card__actions`。
pub(crate) fn footer(leading: Option<AnyView>, buttons: Vec<AnyView>) -> AnyView {
    let mut children = Vec::with_capacity(buttons.len() + 2);
    children.extend(leading);
    children.push(widget(Stack::spacer()).into_any());
    children.extend(buttons);
    widget(Stack::bar(8.0).align(AlignSpec::Center)).children(children).key("dialog-actions").into_any()
}

/// 底栏按钮：高 32、左右 10、14px；主操作低饱和蓝 600 字重，危险操作低饱和红，普通操作透明
/// （DESIGN.md 5.1）。点击发 `message`；禁用时框架不派发点击。
pub(crate) fn action(
    label: impl IntoProp<String>,
    kind: ButtonKind,
    disabled: impl IntoProp<bool>,
    key: &'static str,
    message: impl Fn() -> ShellMessage + Send + Sync + 'static,
) -> AnyView {
    action_button(label, kind, disabled, key, message).into_any()
}

/// 同 [`action`]，返回元素本身，调用方还能再绑字段（比如提交中的 `loading`）。
pub(crate) fn action_button(
    label: impl IntoProp<String>,
    kind: ButtonKind,
    disabled: impl IntoProp<bool>,
    key: &'static str,
    message: impl Fn() -> ShellMessage + Send + Sync + 'static,
) -> El<Button> {
    button_view(label, kind, disabled, key).on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(message()))
}

/// 确认框里的按钮：样式和 [`action`] 一样，点击由框架变成 `ConfirmIntent`，按钮自己不发消息。
pub(crate) fn intent_button(label: impl IntoProp<String>, kind: ButtonKind, disabled: impl IntoProp<bool>, key: &'static str) -> AnyView {
    button_view(label, kind, disabled, key).into_any()
}

fn button_view(label: impl IntoProp<String>, kind: ButtonKind, disabled: impl IntoProp<bool>, key: &'static str) -> El<Button> {
    let mut button = Button::new("").kind(kind);
    button.style.control_height = None;
    button.style.control_padding_x = None;
    {
        let layout = Arc::make_mut(&mut button.style.layout);
        layout.height = Some(LengthSpec::Px(32.0));
        layout.min_height = Some(LengthSpec::Px(32.0));
        layout.padding_left = Some(LengthSpec::Px(10.0));
        layout.padding_right = Some(LengthSpec::Px(10.0));
        layout.font_size = Some(14.0);
        layout.font_weight = Some(if kind == ButtonKind::Primary { 600 } else { 500 });
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
    }
    widget(button).key(key).prop::<String, fields::button::label>(label).prop::<bool, fields::button::disabled>(disabled)
}

/// 正文里的一段说明：13px、行高 1.5、占满宽度、任意处折行（`.dialog-card__body p`）。
/// `muted` 时用次要文字色（`.folder-dialog__summary`）。
pub(crate) fn paragraph(copy: impl IntoProp<String>, muted: bool, key: &'static str) -> AnyView {
    let color = if muted { SemanticColorRole::Muted } else { SemanticColorRole::Text };
    widget(wrapping_text(13.0, 19.5, color)).key(key).prop::<String, fields::text::value>(copy).into_any()
}

/// 正文里的错误：`--err` 12px，没有错误时不占位。
pub(crate) fn error_line(copy: impl Fn() -> String + Send + 'static, key: &'static str) -> AnyView {
    widget(wrapping_text(12.0, 19.0, SemanticColorRole::Danger))
        .key(key)
        .prop::<String, nana_ui::runtime::view::fields::HiddenWhenEmpty<fields::text::value>>(copy)
        .into_any()
}

/// 底栏左边的「处理中...」：转圈图标加 12px 弱色字，`visible` 为假时不占位。
pub(crate) fn busy_note(visible: impl IntoProp<bool>, key: &'static str) -> AnyView {
    widget(Stack::row(6.0).align(AlignSpec::Center))
        .visible(visible)
        .key(key)
        .children((
            widget(IconGlyph::new(LOADER_2).size(13.0).role(SemanticColorRole::Muted)),
            widget(Text::new("处理中...").font_size(12.0).color(SemanticColorRole::Muted)),
        ))
        .into_any()
}

/// 占满宽度、任意处折行的一段字。
pub(crate) fn wrapping_text(size: f32, line_height: f32, color: SemanticColorRole) -> Text {
    let mut node = Text::new(String::new()).font_size(size).font_weight(400).color(color).line_height(line_height);
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    node
}

/// 行和卡片（`ListItem`）的禁用：连同 Vue `button:disabled` 的 0.45 整体透明度一起换。
/// 禁用的行框架不派发点击。对话框的选项卡片和弹层的菜单行共用。
pub(crate) struct DimmedDisabled;

/// Vue `button:disabled` 的整体透明度。
const DISABLED_OPACITY: f32 = 0.45;

impl FieldWrite<ListItem, bool> for DimmedDisabled {
    const FIELD: &'static str = "ListItem.disabled+opacity";

    fn write(target: &mut ListItem, disabled: bool) {
        target.disabled = disabled;
        Arc::make_mut(&mut target.style.layout).opacity = disabled.then_some(DISABLED_OPACITY);
    }

    fn differs(target: &ListItem, disabled: &bool) -> bool {
        target.disabled != *disabled
    }
}

/// `Dialog` 的标题。
struct DialogTitle;

impl FieldWrite<Dialog, String> for DialogTitle {
    const FIELD: &'static str = "Dialog.title";

    fn write(target: &mut Dialog, value: String) {
        target.title = Arc::from(value);
    }

    fn differs(target: &Dialog, value: &String) -> bool {
        target.title.as_ref() != value.as_str()
    }
}

/// `ConfirmDialog` 的标题。
struct ConfirmTitle;

impl FieldWrite<ConfirmDialog, String> for ConfirmTitle {
    const FIELD: &'static str = "ConfirmDialog.title";

    fn write(target: &mut ConfirmDialog, value: String) {
        target.title = Arc::from(value);
    }

    fn differs(target: &ConfirmDialog, value: &String) -> bool {
        target.title.as_ref() != value.as_str()
    }
}

/// `ConfirmDialog` 的说明。
struct ConfirmMessage;

impl FieldWrite<ConfirmDialog, String> for ConfirmMessage {
    const FIELD: &'static str = "ConfirmDialog.message";

    fn write(target: &mut ConfirmDialog, value: String) {
        target.message = Arc::from(value);
    }

    fn differs(target: &ConfirmDialog, value: &String) -> bool {
        target.message.as_ref() != value.as_str()
    }
}

/// `ConfirmDialog` 的处理中：框架据此不派发按钮点击。
struct ConfirmBusy;

impl FieldWrite<ConfirmDialog, bool> for ConfirmBusy {
    const FIELD: &'static str = "ConfirmDialog.busy";

    fn write(target: &mut ConfirmDialog, value: bool) {
        target.busy = value;
    }

    fn differs(target: &ConfirmDialog, value: &bool) -> bool {
        target.busy != *value
    }
}

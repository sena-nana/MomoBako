//! 仓库弹层：切换列表、添加菜单和后端表单。
//!
//! 弹层是 NanaUI 的 `Popover`，触发器就是侧栏的仓库头（当前仓库名加上下箭头，[`repository_switcher`]）。
//! 开合、点外面和 Escape 收起、收起后焦点回到仓库头、放不下时翻面和收进窗口都交给框架；框架开合时发的
//! `PopoverToggled` 换成打开切换列表或关闭弹层的消息写回 ViewModel，ViewModel 的开合经 `open` 写回弹层，
//! 提交中点外面和 Escape 不收起。面板左边对齐仓库头、在它下面 6px；切换列表和仓库头同宽，添加菜单按
//! 右键菜单的最小宽 180px，表单 320px。外框（底色、描边、圆角）由框架画，各页只留自己的内边距。
//! 切换列表和添加菜单是菜单：项在方向键焦点组里，上下键在项之间走、首尾相接。
//!
//! 面板里只有一格，三页（切换、添加、表单）各是一块浮层块（`view_part_overlay.rs`），挂进这一格；
//! 换页时浮层块按身份换块。同一页打开期间列表、禁用和错误都经信号原地改，列表行按内容做键，变了的
//! 那一行才重建；表单输入框受控。

use std::sync::Arc;

use nana_ui::icons_tabler::{ALERT_CIRCLE, CHECK, PLUS, SELECTOR, TRASH, X};
use nana_ui::runtime::view::{
    AnyView, FieldWrite, IntoProp, IntoView, NodeRef, Signal, callout, computed, each, fields,
    signal, status_badge, widget,
};
use nana_ui::runtime::{
    Activate, AlignSpec, IconButton, LengthSpec, ListItem, Popover, PopoverAlignment,
    PopoverToggled, RadiusTier, SemanticColorRole, Stack, StatusTone,
};
use nana_ui::{ButtonKind, ControlSize, Icon};

use super::super::menu_focus::MENU_KEYS;
use super::super::sidebar::{BackendOption, GapMessage, PopoverMode, backend_options};
use super::super::view_part_overlay::OverlayKey;
use super::super::view_part_overlay::dialog::{DimmedDisabled, action, text_field, wrapping_text};
use super::super::view_part_overlay::session::{Draft, Projected};
use super::super::view_part_sidebar::project::HeadView;
use super::super::{ShellMessage, ShellViewModel, SidebarMessage};
use super::parts::{self, ActiveTone, PopoverOpen};
use super::sidebar_message;
use crate::theme_map::tiers::{self, Edge};
use nana_ui::runtime::view::css;
use nana_ui_core::{SpacingStep, TextWeight};

/// 侧栏和仓库头的左右内边距合起来（侧栏 `Md`、仓库头 `Xxs`，两边各一份）：切换列表比侧栏窄这么多，
/// 和仓库头同宽。
const SWITCHER_INSET: f32 =
    2.0 * (tiers::space_px(SpacingStep::Md) + tiers::space_px(SpacingStep::Xxs));
/// 面板顶边在仓库头下方的间距：`Sm` 档（默认 6px）。
const ANCHOR_GAP: f32 = tiers::space_px(SpacingStep::Sm);
const ADD_MENU_WIDTH: f32 = 180.0;
const FORM_WIDTH: f32 = 320.0;

/// 仓库弹层随 ViewModel 变的部分。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RepositoryPopoverView {
    /// 显示着：ViewModel 开着弹层，而且前面没有排着别的浮层（`OverlayKey` 的先后）。
    pub open: bool,
    /// 提交中：点外面和 Escape 不收起。
    pub submitting: bool,
    /// 面板宽：切换列表和仓库头同宽，添加菜单 180，表单 320。关着时按切换列表算。
    pub width: f32,
}

impl RepositoryPopoverView {
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let width = match model.sidebar.popover {
            PopoverMode::Closed | PopoverMode::Switcher => {
                (model.workspace.sidebar_width - SWITCHER_INSET).max(0.0)
            }
            PopoverMode::AddMenu => ADD_MENU_WIDTH,
            PopoverMode::BackendForm => FORM_WIDTH,
        };
        Self {
            open: OverlayKey::of(model) == Some(OverlayKey::RepositoryPopover),
            submitting: model.sidebar.submitting,
            width,
        }
    }
}

/// 仓库头的无障碍名：「资源库 · 仓库名」。只读「资源库」的话读屏听不到当前是哪个库，
/// 所以把仓库名一起读出来。
fn switcher_label(name: &str) -> String {
    format!("资源库 · {name}")
}

/// 仓库头：NanaUI `Popover` 的触发器，画成侧栏的一行（当前仓库名加上下箭头），点一下或 Enter 打开弹层。
/// 面板里只有一格 `slot`，浮层块把当前这一页挂进去。框架打开时发打开切换列表，收起时发关闭弹层；
/// ViewModel 没跟（提交中）时，`open` 在下一次同步照 ViewModel 写回（`SidebarSignals::write`）。
pub(crate) fn repository_switcher(
    head: Signal<HeadView>,
    popover: Signal<RepositoryPopoverView>,
    slot: NodeRef,
) -> AnyView {
    let name = head.with_untracked(|head| head.name.clone());
    let width = popover.with_untracked(|view| view.width);
    let mut style = parts::row_style(
        28.0,
        Some(SpacingStep::Md),
        Some(SpacingStep::Md),
        Some(SpacingStep::Sm),
        ActiveTone::Accent,
        false,
    );
    crate::theme_map::control_weight(Arc::make_mut(&mut style.layout), TextWeight::Semibold);
    let trigger = widget(
        Stack::fill_row(0.0)
            .gap_step(SpacingStep::Sm)
            .align(AlignSpec::Center),
    )
    .children((
        widget(parts::fill_text(name.clone(), 13.0, TextWeight::Semibold))
            .prop::<String, fields::text::value>(move || head.with(|head| head.name.clone())),
        widget(parts::inherit_icon(SELECTOR, 13.0)),
    ))
    .key("repository-switcher-trigger");
    let anchor = Popover::new()
        .trigger(switcher_label(&name))
        .bare_trigger(true)
        .trigger_style(style)
        .alignment(PopoverAlignment::Start)
        .gap(ANCHOR_GAP)
        .padding(0.0)
        .width(width);
    let slot = widget(Stack::column(0.0).width(LengthSpec::Fill))
        .node_ref(slot)
        .key("repository-popover-slot");
    widget(anchor)
        .prop::<String, SwitcherLabel>(move || head.with(|head| switcher_label(&head.name)))
        .prop::<bool, PopoverOpen>(move || popover.with(|view| view.open))
        .prop::<f32, PanelWidth>(move || popover.with(|view| view.width))
        .prop::<bool, Dismissable>(move || popover.with(|view| !view.submitting))
        .trigger(trigger)
        .children((slot,))
        .key("repository-switcher")
        .on_cx(|_, toggled: &PopoverToggled, cx| {
            cx.dispatch_program_all(sidebar_message(if toggled.open {
                SidebarMessage::OpenRepositorySwitcher
            } else {
                SidebarMessage::CloseRepositoryPopover
            }));
        })
        .into_any()
}

/// 仓库头的无障碍名，换仓库时跟着改。
struct SwitcherLabel;

impl FieldWrite<Popover, String> for SwitcherLabel {
    const FIELD: &'static str = "Popover.trigger";

    fn write(target: &mut Popover, label: String) {
        target.trigger = label.into();
    }

    fn differs(target: &Popover, label: &String) -> bool {
        target.trigger.as_ref() != label.as_str()
    }
}

/// 面板宽，换页时跟着改。框架给面板定了最小宽，这里照它夹。
struct PanelWidth;

impl FieldWrite<Popover, f32> for PanelWidth {
    const FIELD: &'static str = "Popover.width";

    fn write(target: &mut Popover, width: f32) {
        target.width = Popover::new().width(width).width;
    }

    fn differs(target: &Popover, width: &f32) -> bool {
        target.width != Popover::new().width(*width).width
    }
}

/// 点外面和 Escape 收不收起：提交中都不收。
struct Dismissable;

impl FieldWrite<Popover, bool> for Dismissable {
    const FIELD: &'static str = "Popover.close_on_escape+close_on_outside";

    fn write(target: &mut Popover, dismissable: bool) {
        target.close_on_escape = dismissable;
        target.close_on_outside = dismissable;
    }

    fn differs(target: &Popover, dismissable: &bool) -> bool {
        target.close_on_escape != *dismissable || target.close_on_outside != *dismissable
    }
}

/// 打开的仓库弹层当前这一页。关闭时返回 `None`。
pub fn repository_popover(model: &ShellViewModel) -> Option<AnyView> {
    match model.sidebar.popover {
        PopoverMode::Switcher => Some(switcher(model)),
        PopoverMode::AddMenu => Some(add_menu(model)),
        PopoverMode::BackendForm => Some(backend_form(model)),
        PopoverMode::Closed => None,
    }
}

/// 一页：竖排、撑满面板宽，`padding` 是这一页自己的内边距，`gap` 是块与块之间的间距，都取主题间距档。
/// `menu` 为真时（切换列表、添加菜单）里面的项是一个方向键焦点组，上下键在项之间走。
fn page(
    padding: SpacingStep,
    gap: SpacingStep,
    children: Vec<AnyView>,
    label_key: &'static str,
    menu: bool,
) -> AnyView {
    let page = widget(
        Stack::column(0.0)
            .gap_step(gap)
            .padding_step(padding, padding)
            .width(LengthSpec::Fill),
    )
    .children(children)
    .key(label_key);
    if menu {
        page.roving_focus(MENU_KEYS).into_any()
    } else {
        page.into_any()
    }
}

/// 切换列表的一行：行的全部内容就是它的键，变了的行重建。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RepositoryRow {
    pub repo_id: String,
    pub name: String,
    pub active: bool,
    pub missing: bool,
    /// 提交中：整行禁用。
    pub disabled: bool,
}

/// 切换列表要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SwitcherView {
    pub rows: Vec<RepositoryRow>,
    pub submitting: bool,
    /// 有当前资源库时才能删除。
    pub can_delete: bool,
    pub error: String,
}

impl SwitcherView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        if model.sidebar.popover != PopoverMode::Switcher {
            return None;
        }
        let submitting = model.sidebar.submitting;
        let active_id = model.workspace.active_repo_id.as_deref();
        Some(Self {
            rows: model
                .workspace
                .repositories
                .iter()
                .map(|repository| RepositoryRow {
                    repo_id: repository.repo_id.clone(),
                    name: repository.name.clone(),
                    active: active_id == Some(repository.repo_id.as_str()),
                    missing: repository.status == "missing",
                    disabled: submitting,
                })
                .collect(),
            submitting,
            can_delete: active_id.is_some(),
            error: model.sidebar.popover_error.clone(),
        })
    }
}

/// 切换列表：每个资源库一行（勾选列、名称、丢失标记），下面是「添加资源库」和「删除当前资源库」。
fn switcher(model: &ShellViewModel) -> AnyView {
    let Some(initial) = SwitcherView::project(model) else {
        return widget(Stack::column(0.0)).into_any();
    };
    let view = signal(initial);
    Projected::register(view, SwitcherView::project);
    let rows = computed(move || view.with(|view| view.rows.clone()));
    let list = widget(Stack::column(0.0))
        .children((each(rows, Clone::clone, repository_row).css(css! { gap: theme(space-xxs); }),))
        .key("repository-switch-list")
        .into_any();
    let actions = widget(top_divider(
        Stack::column(0.0).gap_step(SpacingStep::Xxs),
        SpacingStep::Sm,
    ))
    .children((
        menu_item(
            Some(PLUS),
            "添加资源库",
            "repository-add",
            false,
            move || view.with(|view| view.submitting),
            || sidebar_message(SidebarMessage::ShowRepositoryAddMenu),
        ),
        menu_item(
            Some(TRASH),
            "删除当前资源库",
            "repository-delete",
            true,
            move || view.with(|view| view.submitting || !view.can_delete),
            || sidebar_message(SidebarMessage::DeleteRepositoryFromSwitcher),
        ),
    ))
    .into_any();
    page(
        SpacingStep::Sm,
        SpacingStep::Sm,
        vec![
            list,
            actions,
            popover_error(move || view.with(|view| view.error.clone())),
        ],
        "repository-switcher-popover",
        true,
    )
}

/// 一个资源库：勾选列、名称和丢失标记。当前库只铺 `--accent-soft` 底，文字仍是正文色。
fn repository_row(row: RepositoryRow) -> AnyView {
    let check = widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(16.0))
            .shrink(0.0)
            .justify(nana_ui::runtime::JustifySpec::Center),
    )
    .children(row.active.then(|| {
        widget(
            nana_ui::runtime::IconGlyph::new(CHECK)
                .size(13.0)
                .role(SemanticColorRole::Accent),
        )
    }));
    let mut name = parts::label_text(
        row.name.clone(),
        13.0,
        TextWeight::Semibold,
        Some(SemanticColorRole::Text),
    )
    .truncating();
    {
        let layout = Arc::make_mut(&mut name.style.layout);
        layout.flex_shrink = Some(1.0);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }
    let missing = row.missing.then(missing_badge);
    let content = widget(
        Stack::fill_row(0.0)
            .gap_step(SpacingStep::Md)
            .align(AlignSpec::Center),
    )
    .children((
        check,
        widget(
            Stack::fill_row(0.0)
                .gap_step(SpacingStep::Sm)
                .align(AlignSpec::Center),
        )
        .children((widget(name), missing)),
    ));
    let mut style = parts::row_style(
        30.0,
        Some(SpacingStep::Md),
        Some(SpacingStep::Md),
        Some(SpacingStep::Md),
        ActiveTone::Accent,
        row.disabled,
    );
    style.interaction.selected.foreground = Some(SemanticColorRole::Text);
    style.interaction.selected_hovered = style.interaction.selected;
    style.interaction.selected_pressed = style.interaction.selected;
    let repo_id = row.repo_id.clone();
    widget(
        ListItem::new(format!("切换资源库 {}", row.name))
            .selected(row.active)
            .disabled(row.disabled)
            .style(style),
    )
    .content(content)
    .key(format!("repository-switch-{}", row.repo_id))
    .on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program_all(sidebar_message(
            SidebarMessage::SelectRepositoryFromSwitcher(repo_id.clone()),
        ));
    })
    .into_any()
}

/// 「丢失」小标记：NanaUI 的状态徽章（`status_badge`），危险色调。
fn missing_badge() -> AnyView {
    status_badge("丢失").tone(StatusTone::Danger).into_any()
}

/// 添加菜单里的一个来源后端。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BackendRow {
    pub plugin_id: String,
    pub label: String,
    /// 不可用或提交中。
    pub disabled: bool,
}

/// 添加菜单要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AddMenuView {
    pub options: Vec<BackendRow>,
    pub error: String,
}

impl AddMenuView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        if model.sidebar.popover != PopoverMode::AddMenu {
            return None;
        }
        let submitting = model.sidebar.submitting;
        Some(Self {
            options: backend_options(&model.admin.plugins)
                .into_iter()
                .map(|option| BackendRow {
                    plugin_id: option.plugin_id,
                    label: option.label,
                    disabled: submitting || !option.enabled,
                })
                .collect(),
            error: model.sidebar.popover_error.clone(),
        })
    }
}

/// 添加菜单：每个来源后端一项，不可用的禁用。外观和右键菜单一样。
fn add_menu(model: &ShellViewModel) -> AnyView {
    let Some(initial) = AddMenuView::project(model) else {
        return widget(Stack::column(0.0)).into_any();
    };
    let view = signal(initial);
    Projected::register(view, AddMenuView::project);
    let options = computed(move || view.with(|view| view.options.clone()));
    let rows = each(options, Clone::clone, |option: BackendRow| {
        let plugin_id = option.plugin_id.clone();
        menu_item(
            None,
            &option.label,
            "repository-backend",
            false,
            option.disabled,
            move || sidebar_message(SidebarMessage::SelectRepositoryBackend(plugin_id.clone())),
        )
    })
    .css(css! { gap: theme(space-xxs); });
    let empty = widget(Stack::column(0.0))
        .visible(move || view.with(|view| view.options.is_empty()))
        .children((parts::empty_hint(
            "没有可用的资源库来源插件。",
            "repository-backend-empty",
        ),));
    page(
        SpacingStep::Xs,
        SpacingStep::Xxs,
        vec![
            widget(Stack::column(0.0)).children((rows,)).into_any(),
            empty.into_any(),
            popover_error(move || view.with(|view| view.error.clone())),
        ],
        "repository-add-menu",
        true,
    )
}

/// 菜单项：28px、左右 `Lg` 档（默认 10px）、13px 字、图标和文字间 `Lg` 档。危险项红字，悬停浅红底。
/// 禁用时整行按主题的禁用不透明度淡化，框架不派发点击。
fn menu_item(
    icon: Option<Icon>,
    label: &str,
    key: &'static str,
    danger: bool,
    disabled: impl IntoProp<bool>,
    message: impl Fn() -> ShellMessage + Send + Sync + 'static,
) -> AnyView {
    let mut style = parts::row_style(
        28.0,
        Some(SpacingStep::Lg),
        Some(SpacingStep::Lg),
        Some(SpacingStep::Lg),
        ActiveTone::Accent,
        false,
    );
    if danger {
        style.foreground = Some(SemanticColorRole::Danger);
        style.interaction.hovered.foreground = Some(SemanticColorRole::Danger);
        style.interaction.hovered.background = Some(SemanticColorRole::DangerSoftHover);
        style.interaction.pressed = style.interaction.hovered;
    }
    let mut parts_row: Vec<AnyView> = Vec::new();
    if let Some(icon) = icon {
        parts_row.push(widget(parts::inherit_icon(icon, 14.0)).into_any());
    }
    parts_row.push(parts::fill_label(
        label.to_string(),
        13.0,
        TextWeight::Medium,
    ));
    widget(ListItem::new(label.to_string()).style(style))
        .content(
            widget(
                Stack::fill_row(0.0)
                    .gap_step(SpacingStep::Lg)
                    .align(AlignSpec::Center),
            )
            .children(parts_row),
        )
        .key(format!("{key}-{label}"))
        .prop::<bool, DimmedDisabled>(disabled)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(message()))
        .into_any()
}

/// 后端表单要显示的东西。各输入框的草稿不在这里，见 [`Draft`]。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BackendFormView {
    pub title: String,
    pub summary: String,
    pub submitting: bool,
    /// 提交中、后端不可用或地址为空：「创建」不可用。
    pub blocked: bool,
    pub error: String,
}

impl BackendFormView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let sidebar = &model.sidebar;
        if sidebar.popover != PopoverMode::BackendForm {
            return None;
        }
        let options = backend_options(&model.admin.plugins);
        let option: Option<&BackendOption> = options
            .iter()
            .find(|option| option.plugin_id == sidebar.backend_plugin_id);
        Some(Self {
            title: option
                .map(|option| option.name.clone())
                .unwrap_or_else(|| "添加资源库".into()),
            summary: option
                .map(|option| option.description.clone())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "填写资源库配置。".into()),
            submitting: sidebar.submitting,
            blocked: sidebar.submitting || sidebar.backend_submit_disabled(option),
            error: sidebar.popover_error.clone(),
        })
    }
}

/// 表单里一个字段的草稿：表单开着时读侧栏里这一项。
fn form_draft(model: &ShellViewModel, read: fn(&ShellViewModel) -> String) -> Signal<String> {
    Draft::register(model, move |model| {
        (model.sidebar.popover == PopoverMode::BackendForm).then(|| read(model))
    })
}

/// 后端表单：名称、服务地址、根目录、用户名、密码，说明和「返回 / 取消 / 创建」。
fn backend_form(model: &ShellViewModel) -> AnyView {
    let Some(initial) = BackendFormView::project(model) else {
        return widget(Stack::column(0.0)).into_any();
    };
    let view = signal(initial);
    Projected::register(view, BackendFormView::project);
    let submitting = move || view.with(|view| view.submitting);
    let mut close = IconButton::new(X, "关闭添加资源库")
        .size(ControlSize::Small)
        .with_tooltip("关闭");
    close.style = parts::icon_button_style(22.0, RadiusTier::Sm, SemanticColorRole::Faint, None);
    // 提交中停用：按主题的禁用不透明度和弹层底色（`--bg-elev`）在 sRGB 里混。
    close.style.interaction.disabled =
        crate::theme_map::disabled_paint(SemanticColorRole::Faint, SemanticColorRole::Surface);
    let header = widget(
        Stack::bar(0.0)
            .gap_step(SpacingStep::Md)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(24.0)),
    )
    .children((
        widget(parts::label_text(
            String::new(),
            13.0,
            TextWeight::Bold,
            Some(SemanticColorRole::Text),
        ))
        .prop::<String, fields::text::value>(move || view.with(|view| view.title.clone())),
        widget(Stack::spacer()),
        widget(close.colors_from_style())
            .key("repository-form-close")
            .prop::<bool, fields::icon_button::disabled>(submitting)
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(sidebar_message(SidebarMessage::CloseRepositoryPopover))
            }),
    ));
    let gap = |message: fn(String) -> GapMessage| {
        move |value: String| sidebar_message(SidebarMessage::Gap(message(value)))
    };
    let field = |label: &'static str,
                 key: &'static str,
                 placeholder: &'static str,
                 read: fn(&ShellViewModel) -> String,
                 secure: bool,
                 message: fn(String) -> GapMessage| {
        text_field(
            label,
            key,
            form_draft(model, read),
            placeholder,
            secure,
            submitting,
            gap(message),
            None,
        )
    };
    let mut note = crate::theme_map::tiers::text_line_px(
        parts::label_text(
            "当前仅完成后端配置入口与服务端抽象。远端适配器尚未实现，请先用于配置演进与契约联调。",
            12.0,
            TextWeight::Regular,
            Some(SemanticColorRole::Warning),
        ),
        17.0,
    );
    Arc::make_mut(&mut note.style.layout).width = Some(LengthSpec::Fill);
    let body: Vec<AnyView> = vec![
        widget(wrapping_text(12.0, 17.0, SemanticColorRole::Muted))
            .prop::<String, fields::text::value>(move || view.with(|view| view.summary.clone()))
            .into_any(),
        field(
            "资源库名称",
            "repository-form-name",
            "可选，默认使用后端名称",
            |model| model.sidebar.backend_name.clone(),
            false,
            GapMessage::SetBackendName,
        ),
        field(
            "服务地址",
            "repository-form-url",
            "https://example.com/dav/",
            |model| model.sidebar.backend_url.clone(),
            false,
            GapMessage::SetBackendUrl,
        ),
        field(
            "根目录",
            "repository-form-root",
            "/assets/anime",
            |model| model.sidebar.backend_root.clone(),
            false,
            GapMessage::SetBackendRoot,
        ),
        field(
            "用户名",
            "repository-form-user",
            "可选",
            |model| model.sidebar.backend_user.clone(),
            false,
            GapMessage::SetBackendUser,
        ),
        field(
            "密码 / Token",
            "repository-form-password",
            "可选",
            |model| model.sidebar.backend_password.clone(),
            true,
            GapMessage::SetBackendPassword,
        ),
        widget(
            Stack::column(0.0)
                .padding_step(SpacingStep::Lg, SpacingStep::Md)
                .radius(RadiusTier::Sm)
                .surface(SemanticColorRole::WarningSoft),
        )
        .children((widget(note),))
        .into_any(),
        popover_error(move || view.with(|view| view.error.clone())),
    ];
    let actions = widget(
        Stack::bar(0.0)
            .gap_step(SpacingStep::Md)
            .align(AlignSpec::Center)
            .with_layout(|layout| {
                tiers::padding(layout, &[Edge::Top], SpacingStep::Xxs);
            }),
    )
    .children((
        widget(Stack::spacer()),
        action(
            "返回",
            ButtonKind::Ghost,
            submitting,
            "repository-form-back",
            || sidebar_message(SidebarMessage::BackToAddMenu),
        ),
        action(
            "取消",
            ButtonKind::Ghost,
            submitting,
            "repository-form-cancel",
            || sidebar_message(SidebarMessage::CloseRepositoryPopover),
        ),
        action(
            move || if submitting() { "创建中" } else { "创建" }.to_string(),
            ButtonKind::Primary,
            move || view.with(|view| view.blocked),
            "repository-form-submit",
            || sidebar_message(SidebarMessage::Gap(GapMessage::SubmitBackend)),
        ),
    ));
    page(
        SpacingStep::Lg,
        SpacingStep::Lg,
        vec![
            header.into_any(),
            widget(Stack::column(0.0).gap_step(SpacingStep::Lg))
                .children(body)
                .into_any(),
            actions.into_any(),
        ],
        "repository-backend-form",
        false,
    )
}

/// 弹层里的错误：NanaUI 的提示条（`callout`），危险色调，没有错误时不占位。
fn popover_error(error: impl Fn() -> String + Send + 'static) -> AnyView {
    let error = computed(error);
    widget(Stack::column(0.0))
        .visible(move || error.with(|error| !error.is_empty()))
        .children((callout(ALERT_CIRCLE, StatusTone::Danger, error),))
        .key("repository-popover-error")
        .into_any()
}

/// 顶边一条 `--border-soft` 发丝分隔线，线下留 `gap` 档内边距。
fn top_divider(column: Stack, gap: SpacingStep) -> Stack {
    let column = column.with_layout(|layout| {
        tiers::padding(layout, &[Edge::Top], gap);
        crate::theme_map::tiers::border_side(layout, Edge::Top, 1);
    });
    let mut style = column.node_style();
    style.border = Some(SemanticColorRole::BorderSoft);
    column.style(style)
}

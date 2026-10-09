//! 仓库切换弹层：切换列表、添加菜单和后端表单。对应 Vue `RepositorySwitcherPopover.vue`。
//!
//! 位置照 `useRepositorySwitcherUi.getPopoverPosition`：左边和仓库头按钮对齐，顶边在按钮下方 6px；
//! 切换列表和按钮同宽，添加菜单按 `.ctx-menu` 的最小宽 180px，表单 320px，左右夹在视口里。
//! 弹层下面铺一层透明的点击层，点弹层外面关闭，和 Vue 的外部按下关闭一致；提交中归约不关。
//!
//! 弹层常驻：三页（切换、添加、表单）各是一块，换页时浮层块按身份换块；同一页打开期间位置、
//! 列表、禁用和错误都经信号原地改，列表行按内容做键，变了的那一行才重建；表单输入框受控。

use std::sync::Arc;

use nana_ui::icons_tabler::{CHECK, PLUS, TRASH, X};
use nana_ui::runtime::view::{computed, each, fields, signal, widget, AnyView, FieldWrite, IntoProp, IntoView, Signal};
use nana_ui::runtime::{
    Activate, AlignSpec, IconButton, LengthSpec, ListItem, RadiusTier, SemanticColorRole, Stack,
};
use nana_ui::{ButtonKind, ControlSize, Icon};

use super::super::hot::{self, LayerPaint, LayerPaintField};
use super::super::sidebar::{backend_options, BackendOption, GapMessage, PopoverMode};
use super::super::view_part_overlay::dialog::{action, text_field, wrapping_text, DimmedDisabled};
use super::super::view_part_overlay::session::{Draft, Projected};
use super::super::{ShellMessage, ShellViewModel, SidebarMessage};
use super::parts::{self, ActiveTone};
use super::sidebar_message;

/// 标题栏高度加侧栏上内边距：仓库头按钮的顶边。
const ANCHOR_TOP: f32 = 36.0 + 10.0;
/// 仓库头按钮高度。
const ANCHOR_HEIGHT: f32 = 28.0;
/// 侧栏左内边距加仓库头左内边距：仓库头按钮的左边。
const ANCHOR_LEFT: f32 = 8.0 + 2.0;
/// 弹层顶边在按钮下方的间距。
const ANCHOR_GAP: f32 = 6.0;
/// 视口边距。
const VIEWPORT_MARGIN: f32 = 8.0;
const ADD_MENU_WIDTH: f32 = 180.0;
const FORM_WIDTH: f32 = 320.0;

/// 弹层外框放在哪：左、上和宽。窗口缩放或侧栏变宽时原地挪。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    pub left: f32,
    pub top: f32,
    pub width: f32,
}

impl Placement {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let width = match model.sidebar.popover {
            PopoverMode::Closed => return None,
            PopoverMode::Switcher => (model.workspace.sidebar_width - 2.0 * ANCHOR_LEFT).max(0.0),
            PopoverMode::AddMenu => ADD_MENU_WIDTH,
            PopoverMode::BackendForm => FORM_WIDTH,
        };
        let max_left = (model.viewport_width - width - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN);
        Some(Self { left: ANCHOR_LEFT.clamp(VIEWPORT_MARGIN, max_left), top: ANCHOR_TOP + ANCHOR_HEIGHT + ANCHOR_GAP, width })
    }
}

/// 打开的仓库弹层。关闭时返回 `None`。
pub fn repository_popover(model: &ShellViewModel) -> Option<AnyView> {
    let placement = signal(Placement::project(model)?);
    Projected::register(placement, Placement::project);
    let panel = match model.sidebar.popover {
        PopoverMode::Switcher => switcher(model),
        PopoverMode::AddMenu => add_menu(model),
        PopoverMode::BackendForm => backend_form(model),
        PopoverMode::Closed => return None,
    };
    // 淡入和上移逐帧走，绑在热信号上，开合动效不重挂弹层。
    let paint = hot::prop(|signals| signals.panel, LayerPaint::panel(&model.motion));
    let positioned = widget(Stack::column(0.0).with_layout(|layout| layout.position = nana_ui_core::PositionSpec::Absolute))
        .prop::<Placement, PlacementField>(placement)
        .prop::<LayerPaint, LayerPaintField>(paint)
        .children((panel,))
        .into_any();
    Some(
        widget(Stack::fill_column(0.0).with_layout(|layout| layout.position = nana_ui_core::PositionSpec::Relative))
            .children((dismiss_layer(), positioned))
            .key("repository-popover-layer")
            .into_any(),
    )
}

/// 弹层外框的位置和宽度。
struct PlacementField;

impl FieldWrite<Stack, Placement> for PlacementField {
    const FIELD: &'static str = "Stack.style.layout.offset+width";

    fn write(target: &mut Stack, value: Placement) {
        let layout = Arc::make_mut(&mut <Stack as nana_ui::runtime::view::StyledComponent>::node_style_mut(target).layout);
        layout.offset_left = Some(LengthSpec::Px(value.left));
        layout.offset_top = Some(LengthSpec::Px(value.top));
        layout.width = Some(LengthSpec::Px(value.width));
    }

    fn differs(target: &Stack, value: &Placement) -> bool {
        let layout = &<Stack as nana_ui::runtime::view::StyledComponent>::node_style(target).layout;
        layout.offset_left != Some(LengthSpec::Px(value.left))
            || layout.offset_top != Some(LengthSpec::Px(value.top))
            || layout.width != Some(LengthSpec::Px(value.width))
    }
}

/// 弹层外的透明点击层。提交中归约不关，和 Vue 提交时不关弹层一致。
fn dismiss_layer() -> AnyView {
    let mut style = nana_ui::runtime::NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.position = nana_ui_core::PositionSpec::Absolute;
        layout.offset_left = Some(LengthSpec::Px(0.0));
        layout.offset_top = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Percent(100.0));
        layout.height = Some(LengthSpec::Percent(100.0));
    }
    widget(ListItem::new("关闭资源库弹层").style(style))
        .content(widget(Stack::row(0.0)))
        .key("repository-popover-dismiss")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(sidebar_message(SidebarMessage::CloseRepositoryPopover)))
        .into_any()
}

/// 弹层外框：`--bg-elev` 底、`--border-strong` 描边、`--radius-md`、投影。
fn surface(padding: f32, gap: f32, children: Vec<AnyView>, label_key: &'static str) -> AnyView {
    widget(
        Stack::column(gap)
            .padding(padding)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderStrong, 1.0)
            .radius(RadiusTier::Md)
            .with_layout(|layout| {
                layout.paint.box_shadows = vec![nana_ui_core::BoxShadowSpec {
                    paint_color: None,
                    offset_x: 0.0,
                    offset_y: 12.0,
                    blur_radius: 32.0,
                    spread_radius: -12.0,
                    color: [0.0, 0.0, 0.0, 0.62],
                    inset: false,
                }];
            }),
    )
    .children(children)
    .key(label_key)
    .into_any()
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
    let list = widget(Stack::column(0.0)).children((each(rows, Clone::clone, repository_row).gap(2.0),)).key("repository-switch-list").into_any();
    let actions = widget(top_divider(Stack::column(2.0), 6.0))
        .children((
            menu_item(Some(PLUS), "添加资源库", "repository-add", false, move || view.with(|view| view.submitting), || {
                sidebar_message(SidebarMessage::ShowRepositoryAddMenu)
            }),
            menu_item(Some(TRASH), "删除当前资源库", "repository-delete", true, move || view.with(|view| view.submitting || !view.can_delete), || {
                sidebar_message(SidebarMessage::DeleteRepositoryFromSwitcher)
            }),
        ))
        .into_any();
    surface(6.0, 6.0, vec![list, actions, popover_error(move || view.with(|view| view.error.clone()))], "repository-switcher-popover")
}

/// 一个资源库：勾选列、名称和丢失标记。当前库只铺 `--accent-soft` 底，文字仍是正文色。
fn repository_row(row: RepositoryRow) -> AnyView {
    let check = widget(Stack::row(0.0).width(LengthSpec::Px(16.0)).shrink(0.0).justify(nana_ui::runtime::JustifySpec::Center))
        .children(row.active.then(|| widget(nana_ui::runtime::IconGlyph::new(CHECK).size(13.0).role(SemanticColorRole::Accent))));
    let mut name = parts::label_text(row.name.clone(), 13.0, 600, Some(SemanticColorRole::Text)).truncating();
    {
        let layout = Arc::make_mut(&mut name.style.layout);
        layout.flex_shrink = Some(1.0);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }
    let missing = row.missing.then(missing_badge);
    let content = widget(Stack::fill_row(8.0).align(AlignSpec::Center))
        .children((check, widget(Stack::fill_row(6.0).align(AlignSpec::Center)).children((widget(name), missing))));
    let mut style = parts::row_style(30.0, 8.0, 8.0, 8.0, ActiveTone::Accent, row.disabled);
    style.interaction.selected.foreground = Some(SemanticColorRole::Text);
    style.interaction.selected_hovered = style.interaction.selected;
    style.interaction.selected_pressed = style.interaction.selected;
    let repo_id = row.repo_id.clone();
    widget(ListItem::new(format!("切换资源库 {}", row.name)).selected(row.active).disabled(row.disabled).style(style))
        .content(content)
        .key(format!("repository-switch-{}", row.repo_id))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::SelectRepositoryFromSwitcher(repo_id.clone())));
        })
        .into_any()
}

/// 「丢失」小标记：`--err-soft` 胶囊、`--err` 11px 字。
fn missing_badge() -> AnyView {
    widget(Stack::row(0.0).padding_xy(5.0, 1.0).shrink(0.0).painter(super::super::shell_tint::SoftFill::err().pill()))
        .children((widget(parts::label_text("丢失", 11.0, 400, Some(SemanticColorRole::Danger)).line_height(15.0)),))
        .into_any()
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
                .map(|option| BackendRow { plugin_id: option.plugin_id, label: option.label, disabled: submitting || !option.enabled })
                .collect(),
            error: model.sidebar.popover_error.clone(),
        })
    }
}

/// 添加菜单：每个来源后端一项，不可用的禁用。对应 `.ctx-menu`。
fn add_menu(model: &ShellViewModel) -> AnyView {
    let Some(initial) = AddMenuView::project(model) else {
        return widget(Stack::column(0.0)).into_any();
    };
    let view = signal(initial);
    Projected::register(view, AddMenuView::project);
    let options = computed(move || view.with(|view| view.options.clone()));
    let rows = each(options, Clone::clone, |option: BackendRow| {
        let plugin_id = option.plugin_id.clone();
        menu_item(None, &option.label, "repository-backend", false, option.disabled, move || {
            sidebar_message(SidebarMessage::SelectRepositoryBackend(plugin_id.clone()))
        })
    })
    .gap(1.0);
    let empty = widget(Stack::column(0.0))
        .visible(move || view.with(|view| view.options.is_empty()))
        .children((parts::empty_hint("没有可用的资源库来源插件。", "repository-backend-empty"),));
    surface(
        4.0,
        1.0,
        vec![widget(Stack::column(0.0)).children((rows,)).into_any(), empty.into_any(), popover_error(move || view.with(|view| view.error.clone()))],
        "repository-add-menu",
    )
}

/// 菜单项：28px、左右 10px、13px 字、图标和文字间 10px。危险项红字，悬停浅红底。
/// 禁用时整行 0.45 透明度，框架不派发点击。
fn menu_item(
    icon: Option<Icon>,
    label: &str,
    key: &'static str,
    danger: bool,
    disabled: impl IntoProp<bool>,
    message: impl Fn() -> ShellMessage + Send + Sync + 'static,
) -> AnyView {
    let mut style = parts::row_style(28.0, 10.0, 10.0, 10.0, ActiveTone::Accent, false);
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
    parts_row.push(parts::fill_label(label.to_string(), 13.0, 500));
    widget(ListItem::new(label.to_string()).style(style))
        .content(widget(Stack::fill_row(10.0).align(AlignSpec::Center)).children(parts_row))
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
        let option: Option<&BackendOption> = options.iter().find(|option| option.plugin_id == sidebar.backend_plugin_id);
        Some(Self {
            title: option.map(|option| option.name.clone()).unwrap_or_else(|| "添加资源库".into()),
            summary: option.map(|option| option.description.clone()).filter(|text| !text.is_empty()).unwrap_or_else(|| "填写资源库配置。".into()),
            submitting: sidebar.submitting,
            blocked: sidebar.submitting || sidebar.backend_submit_disabled(option),
            error: sidebar.popover_error.clone(),
        })
    }
}

/// 表单里一个字段的草稿：表单开着时读侧栏里这一项。
fn form_draft(model: &ShellViewModel, read: fn(&ShellViewModel) -> String) -> Signal<String> {
    Draft::register(model, move |model| (model.sidebar.popover == PopoverMode::BackendForm).then(|| read(model)))
}

/// 后端表单：名称、服务地址、根目录、用户名、密码，说明和「返回 / 取消 / 创建」。
fn backend_form(model: &ShellViewModel) -> AnyView {
    let Some(initial) = BackendFormView::project(model) else {
        return widget(Stack::column(0.0)).into_any();
    };
    let view = signal(initial);
    Projected::register(view, BackendFormView::project);
    let submitting = move || view.with(|view| view.submitting);
    let mut close = IconButton::new(X, "关闭添加资源库").size(ControlSize::Small).with_tooltip("关闭");
    close.style = parts::icon_button_style(22.0, RadiusTier::Sm, SemanticColorRole::Faint, None, false);
    let header = widget(Stack::bar(8.0).align(AlignSpec::Center).min_height(LengthSpec::Px(24.0))).children((
        widget(parts::label_text(String::new(), 13.0, 700, Some(SemanticColorRole::Text)))
            .prop::<String, fields::text::value>(move || view.with(|view| view.title.clone())),
        widget(Stack::spacer()),
        widget(close.colors_from_style())
            .key("repository-form-close")
            .prop::<bool, IconDimmed>(submitting)
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(sidebar_message(SidebarMessage::CloseRepositoryPopover))),
    ));
    let gap = |message: fn(String) -> GapMessage| move |value: String| sidebar_message(SidebarMessage::Gap(message(value)));
    let field = |label: &'static str, key: &'static str, placeholder: &'static str, read: fn(&ShellViewModel) -> String, secure: bool, message: fn(String) -> GapMessage| {
        text_field(label, key, form_draft(model, read), placeholder, secure, submitting, gap(message), None)
    };
    let mut note = parts::label_text("当前仅完成后端配置入口与服务端抽象。远端适配器尚未实现，请先用于配置演进与契约联调。", 12.0, 400, Some(SemanticColorRole::Warning)).line_height(17.0);
    Arc::make_mut(&mut note.style.layout).width = Some(LengthSpec::Fill);
    let body: Vec<AnyView> = vec![
        widget(wrapping_text(12.0, 17.0, SemanticColorRole::Muted))
            .prop::<String, fields::text::value>(move || view.with(|view| view.summary.clone()))
            .into_any(),
        field("资源库名称", "repository-form-name", "可选，默认使用后端名称", |model| model.sidebar.backend_name.clone(), false, GapMessage::SetBackendName),
        field("服务地址", "repository-form-url", "https://example.com/dav/", |model| model.sidebar.backend_url.clone(), false, GapMessage::SetBackendUrl),
        field("根目录", "repository-form-root", "/assets/anime", |model| model.sidebar.backend_root.clone(), false, GapMessage::SetBackendRoot),
        field("用户名", "repository-form-user", "可选", |model| model.sidebar.backend_user.clone(), false, GapMessage::SetBackendUser),
        field("密码 / Token", "repository-form-password", "可选", |model| model.sidebar.backend_password.clone(), true, GapMessage::SetBackendPassword),
        widget(Stack::column(0.0).padding_xy(10.0, 8.0).radius(RadiusTier::Sm).surface(SemanticColorRole::WarningSoft))
            .children((widget(note),))
            .into_any(),
        popover_error(move || view.with(|view| view.error.clone())),
    ];
    let actions = widget(Stack::bar(8.0).align(AlignSpec::Center).with_layout(|layout| {
        layout.padding_top = Some(LengthSpec::Px(2.0));
    }))
    .children((
        widget(Stack::spacer()),
        action("返回", ButtonKind::Ghost, submitting, "repository-form-back", || sidebar_message(SidebarMessage::BackToAddMenu)),
        action("取消", ButtonKind::Ghost, submitting, "repository-form-cancel", || sidebar_message(SidebarMessage::CloseRepositoryPopover)),
        action(
            move || if submitting() { "创建中" } else { "创建" }.to_string(),
            ButtonKind::Primary,
            move || view.with(|view| view.blocked),
            "repository-form-submit",
            || sidebar_message(SidebarMessage::Gap(GapMessage::SubmitBackend)),
        ),
    ));
    surface(
        10.0,
        10.0,
        vec![header.into_any(), widget(Stack::column(10.0)).children(body).into_any(), actions.into_any()],
        "repository-backend-form",
    )
}

/// 图标按钮禁用：连同 0.45 透明度一起换。
struct IconDimmed;

impl FieldWrite<IconButton, bool> for IconDimmed {
    const FIELD: &'static str = "IconButton.disabled+opacity";

    fn write(target: &mut IconButton, disabled: bool) {
        target.disabled = disabled;
        Arc::make_mut(&mut target.style.layout).opacity = disabled.then_some(parts::DISABLED_OPACITY);
    }

    fn differs(target: &IconButton, disabled: &bool) -> bool {
        target.disabled != *disabled
    }
}

/// 弹层里的错误：`--err-soft` 底、`--err` 12px 字，没有错误时不占位。
fn popover_error(error: impl Fn() -> String + Send + Clone + 'static) -> AnyView {
    let shown = error.clone();
    widget(Stack::column(0.0).padding_xy(10.0, 8.0).painter(super::super::shell_tint::SoftFill::err()))
        .visible(move || !shown().is_empty())
        .children((widget(wrapping_text(12.0, 17.0, SemanticColorRole::Danger))
            .key("repository-popover-error")
            .prop::<String, fields::text::value>(error),))
        .into_any()
}

/// 顶边一条 `--border-soft` 分隔线，线下留 `gap` 内边距。
fn top_divider(column: Stack, gap: f32) -> Stack {
    let column = column.with_layout(|layout| {
        layout.padding_top = Some(LengthSpec::Px(gap));
        layout.border_top_width = Some(1.0);
    });
    let mut style = column.node_style();
    style.border = Some(SemanticColorRole::BorderSoft);
    column.style(style)
}

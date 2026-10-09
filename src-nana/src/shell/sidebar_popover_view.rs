//! 仓库切换弹层：切换列表、添加菜单和后端表单。对应 Vue `RepositorySwitcherPopover.vue`。
//!
//! 位置照 `useRepositorySwitcherUi.getPopoverPosition`：左边和仓库头按钮对齐，顶边在按钮下方 6px；
//! 切换列表和按钮同宽，添加菜单按 `.ctx-menu` 的最小宽 180px，表单 320px，左右夹在视口里。
//! 弹层下面铺一层透明的点击层，点弹层外面关闭，和 Vue 的外部按下关闭一致。

use std::sync::Arc;

use nana_ui::icons_tabler::{CHECK, PLUS, TRASH, X};
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, IconButton, LengthSpec, ListItem, RadiusTier, SemanticColorRole, Stack, TextChanged,
    TextInput,
};
use nana_ui::{ButtonKind, ControlSize, Icon};

use super::super::sidebar::{backend_options, BackendOption, GapMessage, PopoverMode};
use super::super::{ShellViewModel, SidebarMessage};
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

/// 打开的仓库弹层。关闭时返回 `None`。
pub fn repository_popover(model: &ShellViewModel) -> Option<AnyView> {
    let mode = model.sidebar.popover;
    if mode == PopoverMode::Closed {
        return None;
    }
    let options = backend_options(&model.admin.plugins);
    let anchor_width = (model.workspace.sidebar_width - 2.0 * ANCHOR_LEFT).max(0.0);
    let (panel, width) = match mode {
        PopoverMode::Switcher => (switcher(model), anchor_width),
        PopoverMode::AddMenu => (add_menu(model, &options), ADD_MENU_WIDTH),
        PopoverMode::BackendForm => (backend_form(model, &options), FORM_WIDTH),
        PopoverMode::Closed => return None,
    };
    let max_left = (model.viewport_width - width - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN);
    let left = ANCHOR_LEFT.clamp(VIEWPORT_MARGIN, max_left);
    let top = ANCHOR_TOP + ANCHOR_HEIGHT + ANCHOR_GAP;
    let frame = model.motion.panel_frame();
    let positioned = widget(Stack::column(0.0).width(LengthSpec::Px(width)).with_layout(move |layout| {
        layout.position = nana_ui_core::PositionSpec::Absolute;
        layout.offset_left = Some(LengthSpec::Px(left));
        layout.offset_top = Some(LengthSpec::Px(top));
        layout.opacity = Some(frame.opacity);
        layout.transform = Some(super::super::motion::shift_scale(frame.shift, 1.0));
    }))
    .children((panel,))
    .into_any();
    Some(
        widget(Stack::fill_column(0.0).with_layout(|layout| layout.position = nana_ui_core::PositionSpec::Relative))
            .children((dismiss_layer(model.sidebar.submitting), positioned))
            .key("repository-popover-layer")
            .into_any(),
    )
}

/// 弹层外的透明点击层。提交中不响应，和 Vue 提交时不关弹层一致。
fn dismiss_layer(submitting: bool) -> AnyView {
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
        .on_cx(move |_, _: &Activate, cx| {
            if !submitting {
                cx.dispatch_program_all(sidebar_message(SidebarMessage::CloseRepositoryPopover));
            }
        })
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

/// 切换列表：每个资源库一行（勾选列、名称、丢失标记），下面是「添加资源库」和「删除当前资源库」。
fn switcher(model: &ShellViewModel) -> AnyView {
    let submitting = model.sidebar.submitting;
    let active_id = model.workspace.active_repo_id.as_deref();
    let rows = model
        .workspace
        .repositories
        .iter()
        .map(|repository| {
            let repo_id = repository.repo_id.clone();
            let active = active_id == Some(repository.repo_id.as_str());
            let check = widget(Stack::row(0.0).width(LengthSpec::Px(16.0)).shrink(0.0).justify(nana_ui::runtime::JustifySpec::Center))
                .children(active.then(|| widget(nana_ui::runtime::IconGlyph::new(CHECK).size(13.0).role(SemanticColorRole::Accent))));
            let mut name = parts::label_text(repository.name.clone(), 13.0, 600, Some(SemanticColorRole::Text)).truncating();
            {
                let layout = Arc::make_mut(&mut name.style.layout);
                layout.flex_shrink = Some(1.0);
                layout.min_width = Some(LengthSpec::Px(0.0));
            }
            let missing = (repository.status == "missing").then(missing_badge);
            let content = widget(Stack::fill_row(8.0).align(AlignSpec::Center))
                .children((check, widget(Stack::fill_row(6.0).align(AlignSpec::Center)).children((widget(name), missing))));
            let mut style = parts::row_style(30.0, 8.0, 8.0, 8.0, ActiveTone::Accent, submitting);
            // 当前库只铺 `--accent-soft` 底，文字仍是正文色。
            style.interaction.selected.foreground = Some(SemanticColorRole::Text);
            style.interaction.selected_hovered = style.interaction.selected;
            style.interaction.selected_pressed = style.interaction.selected;
            widget(ListItem::new(format!("切换资源库 {}", repository.name)).selected(active).disabled(submitting).style(style))
                .content(content)
                .key(format!("repository-switch-{repo_id}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(sidebar_message(SidebarMessage::SelectRepositoryFromSwitcher(repo_id.clone())));
                })
                .into_any()
        })
        .collect::<Vec<_>>();
    let list = widget(Stack::column(2.0)).children(rows).key("repository-switch-list").into_any();
    let actions = widget(top_divider(Stack::column(2.0), 6.0))
    .children((
        menu_item(Some(PLUS), "添加资源库", "repository-add", false, submitting, |cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::ShowRepositoryAddMenu));
        }),
        menu_item(Some(TRASH), "删除当前资源库", "repository-delete", true, submitting || active_id.is_none(), |cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::DeleteRepositoryFromSwitcher));
        }),
    ))
    .into_any();
    let mut children = vec![list, actions];
    if let Some(error) = popover_error(model) {
        children.push(error);
    }
    surface(6.0, 6.0, children, "repository-switcher-popover")
}

/// 「丢失」小标记：`--err-soft` 胶囊、`--err` 11px 字。
fn missing_badge() -> AnyView {
    widget(Stack::row(0.0).padding_xy(5.0, 1.0).shrink(0.0).painter(super::super::shell_tint::SoftFill::err().pill()))
        .children((widget(parts::label_text("丢失", 11.0, 400, Some(SemanticColorRole::Danger)).line_height(15.0)),))
        .into_any()
}

/// 添加菜单：每个来源后端一项，不可用的禁用。对应 `.ctx-menu`。
fn add_menu(model: &ShellViewModel, options: &[BackendOption]) -> AnyView {
    let submitting = model.sidebar.submitting;
    let mut children = options
        .iter()
        .map(|option| {
            let plugin_id = option.plugin_id.clone();
            menu_item(None, &option.label, "repository-backend", false, submitting || !option.enabled, move |cx| {
                cx.dispatch_program_all(sidebar_message(SidebarMessage::SelectRepositoryBackend(plugin_id.clone())));
            })
        })
        .collect::<Vec<_>>();
    if options.is_empty() {
        children.push(parts::empty_hint("没有可用的资源库来源插件。", "repository-backend-empty"));
    }
    if let Some(error) = popover_error(model) {
        children.push(error);
    }
    surface(4.0, 1.0, children, "repository-add-menu")
}

/// 菜单项：28px、左右 10px、13px 字、图标和文字间 10px。危险项红字，悬停浅红底。
fn menu_item(
    icon: Option<Icon>,
    label: &str,
    key: &'static str,
    danger: bool,
    disabled: bool,
    on_activate: impl Fn(&mut nana_ui::runtime::ViewContext<ListItem>) + Send + 'static,
) -> AnyView {
    let mut style = parts::row_style(28.0, 10.0, 10.0, 10.0, ActiveTone::Accent, disabled);
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
    widget(ListItem::new(label.to_string()).disabled(disabled).style(style))
        .content(widget(Stack::fill_row(10.0).align(AlignSpec::Center)).children(parts_row))
        .key(format!("{key}-{label}"))
        .on_cx(move |_, _: &Activate, cx| {
            if !disabled {
                on_activate(cx);
            }
        })
        .into_any()
}

/// 后端表单：名称、服务地址、根目录、用户名、密码，说明和「返回 / 取消 / 创建」。
fn backend_form(model: &ShellViewModel, options: &[BackendOption]) -> AnyView {
    let sidebar = &model.sidebar;
    let submitting = sidebar.submitting;
    let option = options.iter().find(|option| option.plugin_id == sidebar.backend_plugin_id);
    let title = option.map(|option| option.name.clone()).unwrap_or_else(|| "添加资源库".into());
    let summary = option.map(|option| option.description.clone()).filter(|text| !text.is_empty()).unwrap_or_else(|| "填写资源库配置。".into());
    let mut close = IconButton::new(X, "关闭添加资源库").size(ControlSize::Small).disabled(submitting).with_tooltip("关闭");
    close.style = parts::icon_button_style(22.0, RadiusTier::Sm, SemanticColorRole::Faint, None, submitting);
    let header = widget(Stack::bar(8.0).align(AlignSpec::Center).min_height(LengthSpec::Px(24.0))).children((
        widget(parts::label_text(title, 13.0, 700, Some(SemanticColorRole::Text))),
        widget(Stack::spacer()),
        widget(close.colors_from_style()).key("repository-form-close").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::CloseRepositoryPopover));
        }),
    ));
    let mut summary_text = parts::label_text(summary, 12.0, 400, Some(SemanticColorRole::Muted)).line_height(17.0);
    Arc::make_mut(&mut summary_text.style.layout).width = Some(LengthSpec::Fill);
    let field = |label: &'static str, placeholder: &'static str, value: &str, secure: bool, message: fn(String) -> GapMessage| {
        let mut input = TextInput::new(value.to_string()).label(label).placeholder(placeholder).disabled(submitting);
        if secure {
            input = input.secure(true);
        }
        widget(Stack::column(6.0)).children((
            widget(parts::label_text(label, 12.0, 600, Some(SemanticColorRole::Muted))),
            widget(input).on_cx(move |_, event: &TextChanged, cx| {
                cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(message(event.value.to_string()))));
            }),
        ))
    };
    let mut note = parts::label_text("当前仅完成后端配置入口与服务端抽象。远端适配器尚未实现，请先用于配置演进与契约联调。", 12.0, 400, Some(SemanticColorRole::Warning)).line_height(17.0);
    Arc::make_mut(&mut note.style.layout).width = Some(LengthSpec::Fill);
    let mut body: Vec<AnyView> = vec![
        widget(summary_text).into_any(),
        field("资源库名称", "可选，默认使用后端名称", &sidebar.backend_name, false, GapMessage::SetBackendName).into_any(),
        field("服务地址", "https://example.com/dav/", &sidebar.backend_url, false, GapMessage::SetBackendUrl).into_any(),
        field("根目录", "/assets/anime", &sidebar.backend_root, false, GapMessage::SetBackendRoot).into_any(),
        field("用户名", "可选", &sidebar.backend_user, false, GapMessage::SetBackendUser).into_any(),
        field("密码 / Token", "可选", &sidebar.backend_password, true, GapMessage::SetBackendPassword).into_any(),
        widget(Stack::column(0.0).padding_xy(10.0, 8.0).radius(RadiusTier::Sm).surface(SemanticColorRole::WarningSoft))
            .children((widget(note),))
            .into_any(),
    ];
    if let Some(error) = popover_error(model) {
        body.push(error);
    }
    let blocked = submitting || sidebar.backend_submit_disabled(option);
    let actions = widget(Stack::bar(8.0).align(AlignSpec::Center).with_layout(|layout| {
        layout.padding_top = Some(LengthSpec::Px(2.0));
    }))
    .children((
        widget(Stack::spacer()),
        widget(Button::new("返回").kind(ButtonKind::Ghost).disabled(submitting)).key("repository-form-back").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::BackToAddMenu));
        }),
        widget(Button::new("取消").kind(ButtonKind::Ghost).disabled(submitting)).key("repository-form-cancel").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::CloseRepositoryPopover));
        }),
        widget(parts::primary_button(if submitting { "创建中" } else { "创建" }).disabled(blocked))
            .key("repository-form-submit")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(GapMessage::SubmitBackend)))),
    ));
    surface(
        10.0,
        10.0,
        vec![header.into_any(), widget(Stack::column(10.0)).children(body).into_any(), actions.into_any()],
        "repository-backend-form",
    )
}

/// 弹层里的错误：`--err-soft` 底、`--err` 12px 字。
fn popover_error(model: &ShellViewModel) -> Option<AnyView> {
    if model.sidebar.popover_error.is_empty() {
        return None;
    }
    let mut copy = parts::label_text(model.sidebar.popover_error.clone(), 12.0, 400, Some(SemanticColorRole::Danger)).line_height(17.0);
    {
        let layout = Arc::make_mut(&mut copy.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    Some(
        widget(Stack::column(0.0).padding_xy(10.0, 8.0).painter(super::super::shell_tint::SoftFill::err()))
            .children((widget(copy).key("repository-popover-error"),))
            .into_any(),
    )
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

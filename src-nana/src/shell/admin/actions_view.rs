//! 仓库动作面板。
//!
//! 页头是「动作」眉题、「仓库动作」标题和动作数（NanaUI 标签）；下面是加载中的转圈或没有动作的
//! 空状态（NanaUI `EmptyState`），或者左边动作列表（220–300 宽，每个动作一行 NanaUI `ListItem`）、
//! 右边选中动作的详情和步骤；最后是错误提示条（`callout`）。
//!
//! 面板只建一次，只读 [`ActionsSignals`]：动作行按动作 id 做键，选中态和文字是绑定；详情的文字、
//! 执行按钮的图标和禁用是绑定，步骤列表内容变了整块换。执行按钮点下去时现读选中的动作。

use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, each, fields, signal, widget};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, List, ListItem, Stack};
use nana_ui_core::{ButtonKind, RadiusTier, SemanticColorRole as Role, TextWeight, TypeRole};

use super::super::{ShellMessage, ShellViewModel};
use super::AdminMessage;
use super::bind::ActionDisabled;
use super::icons;
use super::marks;
use super::style::{self, action, column, label, pad, wrapping};
use super::support::{action_can_run, action_status_label};

/// 面板主体显示哪一块。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ActionsState {
    #[default]
    Loading,
    Empty,
    List,
}

/// 列表里的一个动作。整行是它的身份：名称或状态变了这一行重建。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ActionRow {
    pub id: String,
    pub name: String,
    /// 「来源 · N 步 · 状态」。
    pub meta: String,
    /// 状态不是就绪时画警示图标。
    pub warn: bool,
}

/// 动作的一步。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StepView {
    pub label: String,
    /// 「类型 · 状态」。
    pub meta: String,
    pub reason: Option<String>,
}

/// 选中动作的详情。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ActionDetail {
    pub id: String,
    pub name: String,
    /// 「状态 · 最近运行 …」。
    pub status: String,
    pub can_run: bool,
    pub running: bool,
    pub unsupported: Option<String>,
    pub steps: Vec<StepView>,
}

/// 动作面板要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActionsView {
    pub count: String,
    pub state: ActionsState,
    pub error: String,
    pub rows: Vec<ActionRow>,
    /// 选中的动作；选中的 id 对不上时是第一个。
    pub detail: Option<ActionDetail>,
}

impl ActionsView {
    /// 从 ViewModel 取动作面板的投影，取舍和旧视图一致。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let admin = &model.admin;
        let state = if admin.actions_loading {
            ActionsState::Loading
        } else if admin.actions.is_empty() {
            ActionsState::Empty
        } else {
            ActionsState::List
        };
        let active = admin
            .actions
            .iter()
            .find(|action| Some(action.action_id.as_str()) == admin.active_action_id.as_deref())
            .or(admin.actions.first());
        let mut seen = std::collections::HashSet::new();
        Self {
            count: format!("{} 项", admin.actions.len()),
            state,
            error: admin.actions_error.clone(),
            rows: admin
                .actions
                .iter()
                // 同一个 id 只留第一个：节点键按 id 取，重复了挂不上。
                .filter(|action| seen.insert(action.action_id.as_str()))
                .map(|item| ActionRow {
                    id: item.action_id.clone(),
                    name: item.name.clone(),
                    meta: format!(
                        "{} · {} 步 · {}",
                        item.source,
                        item.steps.len(),
                        action_status_label(&item.status, item.enabled)
                    ),
                    warn: item.status != "ready",
                })
                .collect(),
            detail: active.map(|current| {
                let last = current
                    .last_run
                    .as_ref()
                    .map(|run| run.status.clone())
                    .unwrap_or_else(|| "无".into());
                ActionDetail {
                    id: current.action_id.clone(),
                    name: current.name.clone(),
                    status: format!(
                        "{} · 最近运行 {last}",
                        action_status_label(&current.status, current.enabled)
                    ),
                    can_run: action_can_run(
                        &current.status,
                        current.enabled,
                        model.files.selected_paths().len(),
                        admin.actions_running,
                    ),
                    running: admin.actions_running,
                    unsupported: current.unsupported_reason.clone(),
                    steps: current
                        .steps
                        .iter()
                        .map(|step| StepView {
                            label: step.label.clone(),
                            meta: format!("{} · {}", step.step_kind, step.status),
                            reason: step.unsupported_reason.clone(),
                        })
                        .collect(),
                }
            }),
        }
    }
}

/// 动作面板的信号。
#[derive(Clone, Copy)]
pub(crate) struct ActionsSignals {
    count: Signal<String>,
    state: Signal<ActionsState>,
    error: Signal<String>,
    rows: Signal<Vec<ActionRow>>,
    detail: Signal<Option<ActionDetail>>,
}

impl ActionsSignals {
    /// 空的信号，进路由之前由同步写入第一份投影。
    pub(crate) fn new() -> Self {
        Self {
            count: signal(String::new()),
            state: signal(ActionsState::default()),
            error: signal(String::new()),
            rows: signal(Vec::new()),
            detail: signal(None),
        }
    }

    /// 写入投影，只写变了的信号。
    pub(crate) fn write(&self, view: ActionsView) {
        self.count.try_set_if_changed(view.count);
        self.state.try_set_if_changed(view.state);
        self.error.try_set_if_changed(view.error);
        self.rows.try_set_if_changed(view.rows);
        self.detail.try_set_if_changed(view.detail);
    }
}

/// 仓库动作面板。
pub(crate) fn actions_panel(signals: ActionsSignals) -> AnyView {
    let (count, state, error) = (signals.count, signals.state, signals.error);
    let header = widget(style::spread(12.0, AlignSpec::Center))
        .children((
            widget(column(0.0)).children((
                widget(style::eyebrow_text("动作")).key("admin-actions-eyebrow"),
                widget(style::label_lh(
                    "仓库动作",
                    TypeRole::Title,
                    TextWeight::Bold,
                    Role::Text,
                    1.25,
                ))
                .key("admin-actions-title"),
            )),
            marks::bound_tag(count, "admin-actions-count"),
        ))
        .into_any();
    let body = (
        header,
        marks::loading("正在加载动作", "admin-actions-loading")
            .visible(move || state.with(|state| *state == ActionsState::Loading)),
        marks::empty(
            || "当前仓库没有导入动作。".to_string(),
            String::new,
            "admin-actions-empty",
        )
        .compact(true)
        .visible(move || state.with(|state| *state == ActionsState::Empty)),
        action_body(signals),
        marks::notice(error, true, "admin-actions-error")
            .visible(move || error.with(|error| !error.is_empty())),
    );
    widget(pad(column(12.0), 18.0, 18.0, 18.0, 18.0))
        .children(body)
        .key("admin-actions")
        .into_any()
}

/// 左边动作列表（220–300 宽），右边选中动作的详情和步骤。有动作时才占布局。
fn action_body(signals: ActionsSignals) -> AnyView {
    let detail = signals.detail;
    let state = signals.state;
    let items = each(signals.rows, ActionRow::clone, move |item: ActionRow| {
        action_row(item, detail)
    });
    let list = widget(
        column(0.0)
            .surface(Role::Surface)
            .border(Role::BorderSoft, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((widget(List::new().label("仓库动作")).children((items,)),))
    .key("admin-action-list")
    .into_any();
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid.columns = Some(vec![
            style::capped(220.0, 300.0),
            nana_ui_core::GridTrack::MinMax {
                min_px: 0.0,
                fr: 1.0,
                max_px: None,
            },
        ]);
        layout.gap = Some(LengthSpec::Px(12.0));
        layout.width = Some(LengthSpec::Fill);
        layout.align_items = AlignSpec::Start;
    });
    widget(grid)
        .visible(move || state.with(|state| *state == ActionsState::List))
        .children((list, action_detail(detail)))
        .into_any()
}

/// 列表里的一个动作：NanaUI 的 `ListItem`，名称下面一行弱色的「状态 · 步骤数」，有不支持的步骤时
/// 行尾一枚盾牌；选中态绑在详情的动作上，点下去选中它。
fn action_row(item: ActionRow, detail: Signal<Option<ActionDetail>>) -> AnyView {
    let id = item.id.clone();
    let selected_id = item.id.clone();
    let content = widget(
        Stack::column(3.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children((
        widget(label(
            item.name.clone(),
            TypeRole::Body,
            TextWeight::Semibold,
            Role::Text,
        )),
        widget(wrapping(label(
            item.meta.clone(),
            TypeRole::Meta,
            TextWeight::Regular,
            Role::Muted,
        ))),
    ));
    let row = widget(ListItem::new(item.name.clone()).auto_height(true))
        .selected(move || {
            detail.with(|detail| {
                detail
                    .as_ref()
                    .is_some_and(|detail| detail.id == selected_id)
            })
        })
        .content(content);
    let row = if item.warn {
        row.trailing(widget(style::glyph(icons::SHIELD_ALERT, 14.0, Role::Text)))
    } else {
        row
    };
    row.key(format!("admin-action-{}", style::key_part(&item.id)))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SelectAction(id.clone())))
        })
        .into_any()
}

/// 详情里的一个值。没有选中的动作时读默认值。
fn detail_value<R: Default + 'static>(
    detail: Signal<Option<ActionDetail>>,
    pick: fn(&ActionDetail) -> R,
) -> impl Fn() -> R + Send + 'static {
    move || detail.with(|detail| detail.as_ref().map(pick).unwrap_or_default())
}

/// 选中动作的详情卡片：名称、状态、「执行」、不支持的原因和步骤。
fn action_detail(detail: Signal<Option<ActionDetail>>) -> AnyView {
    let running = detail_value(detail, |detail| detail.running);
    let head = widget(style::spread(12.0, AlignSpec::Center)).children((
        widget(column(4.0)).children((
            style::bound(
                style::label_lh(
                    String::new(),
                    TypeRole::Title,
                    TextWeight::Bold,
                    Role::Text,
                    1.25,
                ),
                detail_value(detail, |detail| detail.name.clone()),
            ),
            style::bound(
                label(
                    String::new(),
                    TypeRole::Meta,
                    TextWeight::Regular,
                    Role::Muted,
                ),
                detail_value(detail, |detail| detail.status.clone()),
            )
            .key("admin-action-detail"),
        )),
        widget(action(
            "执行",
            Some(icons::PLAY),
            ButtonKind::Primary,
            false,
        ))
        .prop::<bool, fields::button::loading>(running)
        .prop::<bool, ActionDisabled>(detail_value(detail, |detail| !detail.can_run))
        .key("admin-action-run")
        .on_cx(move |_, _: &Activate, cx| {
            // 选中的动作随点选变，点下去时现读。
            let action_id =
                detail.with_untracked(|detail| detail.as_ref().map(|detail| detail.id.clone()));
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RunAction(action_id)));
        }),
    ));
    let unsupported = widget(pad(column(0.0), 12.0, 0.0, 0.0, 0.0))
        .visible(detail_value(detail, |detail| detail.unsupported.is_some()))
        .children((marks::notice(
            detail_value(detail, |detail| {
                detail.unsupported.clone().unwrap_or_default()
            }),
            true,
            "admin-action-unsupported",
        ),));
    let steps = dynamic(
        detail_value(detail, |detail| detail.steps.clone()),
        |steps: &Vec<StepView>| {
            widget(pad(column(8.0), 12.0, 0.0, 0.0, 0.0))
                .children(steps.iter().map(step_view).collect::<Vec<_>>())
                .into_any()
        },
    );
    widget(
        pad(column(0.0), 14.0, 14.0, 14.0, 14.0)
            .surface(Role::Surface)
            .border(Role::BorderSoft, 1.0)
            .radius(RadiusTier::Md),
    )
    .visible(move || detail.with(Option::is_some))
    .children((head, unsupported, steps))
    .key("admin-action-detail-card")
    .into_any()
}

/// 一步：标签和「类型 · 状态」，不支持时右边写原因。
fn step_view(step: &StepView) -> AnyView {
    let mut children = vec![
        widget(column(3.0).shrink(1.0))
            .children((
                widget(label(
                    step.label.clone(),
                    TypeRole::Body,
                    TextWeight::Semibold,
                    Role::Text,
                )),
                widget(label(
                    step.meta.clone(),
                    TypeRole::Meta,
                    TextWeight::Regular,
                    Role::Muted,
                )),
            ))
            .into_any(),
    ];
    if let Some(reason) = step.reason.clone() {
        children.push(
            widget(style::align_end(label(
                reason,
                TypeRole::Meta,
                TextWeight::Regular,
                Role::Danger,
            )))
            .into_any(),
        );
    }
    widget(
        pad(
            style::spread(10.0, AlignSpec::Start),
            10.0,
            10.0,
            10.0,
            10.0,
        )
        .surface(Role::Subtle)
        .border(Role::BorderSoft, 1.0)
        .radius(RadiusTier::Sm),
    )
    .children(children)
    .into_any()
}

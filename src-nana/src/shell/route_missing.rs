//! 缺失仓库路由（常驻）：重定向、刷新和删除资源库。
//!
//! 照启动页的样板改成常驻：[`MissingView`] 是投影，[`MissingSignals`] 建在主区块的常驻作用域里，
//! 视图只建一次。仓库名、说明、路径、按钮的文字和禁用按字段绑定；错误条和重定向输入用 `.visible`，
//! 节点留着、不占布局。路径输入框用 `.model` 受控，ViewModel 的草稿相对上次投影变了才写回信号。
//! 按钮只发意图消息，主按钮在点击时现读「是不是来源缓存问题」。
//!
//! 版式照 `MissingRepositoryState.vue`：警示图标、眉题、仓库名、说明、路径框、错误和三个操作。

use std::sync::Arc;

use nana_ui::icons_tabler::ALERT_TRIANGLE;
use nana_ui::runtime::view::{fields, signal, widget, AnyView, IntoView, Signal};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, IconGlyph, JustifySpec, LengthSpec, RadiusTier, SemanticColorRole, Stack, TextChanged,
    TextInput,
};
use nana_ui::ButtonKind;

use super::shell_tint::SoftFill;
use super::sidebar_view::parts::primary_button;
use super::startup_view::{eyebrow, fill_section, line, margin, wrapping, ROOT_LINE_HEIGHT};
use super::{ShellMessage, ShellViewModel};

/// 缺失仓库页内容最宽 560px（`.missing-repository-page__panel`）。
const MISSING_PANEL_WIDTH: f32 = 560.0;
/// Vue `--font-mono` 的字体栈。
const MONO_FONTS: &str =
    "ui-monospace, \"SF Mono\", \"Cascadia Mono\", \"Cascadia Code\", \"JetBrains Mono\", Menlo, Consolas, \"Liberation Mono\", monospace";
/// 来源资源库缺缓存或要重新认证时的说明。
const CACHE_SUMMARY: &str = "这个来源资源库需要在插件设置中配置本地缓存或重新认证。仓库记录和已有缓存不会被删除。";
/// 本地文件夹找不到时的说明。
const FOLDER_SUMMARY: &str = "MomoBako 找不到这个资源库的本地文件夹。可以重定向到原资源库位置，或移除这条注册记录和本机缓存。";

/// 缺失仓库页要显示的东西。路径草稿不在这里，见 [`MissingSignals`]。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MissingView {
    pub name: String,
    pub path: String,
    /// 来源资源库缺缓存或要重新认证：换说明，主按钮打开来源设置，不显示重定向输入。
    pub cache_issue: bool,
    /// 重定向或删除进行中，按钮都不可用。
    pub busy: bool,
    pub error: String,
    /// 显示重定向输入。
    pub prompt: bool,
    pub primary: &'static str,
    pub delete: &'static str,
}

impl MissingView {
    /// 从 ViewModel 取缺失仓库页的投影。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let workspace = &model.workspace;
        let repository = workspace.active_repository();
        let cache_issue = repository.is_some_and(|item| item.is_source_cache_issue());
        Self {
            name: repository.map(|item| item.name.clone()).unwrap_or_else(|| "资源库不可用".into()),
            path: repository.map(|item| item.path.clone()).unwrap_or_default(),
            cache_issue,
            busy: workspace.missing_busy(),
            error: workspace.missing_error.clone(),
            prompt: workspace.path_prompt && !cache_issue,
            primary: workspace.missing_primary_label(),
            delete: workspace.missing_delete_label(),
        }
    }

    fn summary(&self) -> &'static str {
        if self.cache_issue { CACHE_SUMMARY } else { FOLDER_SUMMARY }
    }
}

/// 缺失仓库页的信号。`draft` 驱动路径输入框，`projected` 记着上次从 ViewModel 写进去的草稿。
#[derive(Clone, Copy)]
pub(crate) struct MissingSignals {
    view: Signal<MissingView>,
    draft: Signal<String>,
    projected: Signal<String>,
}

impl MissingSignals {
    /// 在常驻作用域里建信号，初值是这一刻的投影和草稿。
    pub(crate) fn new(model: &ShellViewModel) -> Self {
        let draft = model.workspace.path_draft.clone();
        Self { view: signal(MissingView::project(model)), draft: signal(draft.clone()), projected: signal(draft) }
    }

    /// 写入投影，只写变了的。路径草稿只在 ViewModel 的值相对上次投影变了时写：刚打的字已经由
    /// `.model` 写进信号，对应的消息可能还排在后台消息后面，这时写回旧草稿会把刚打的字冲掉。
    pub(crate) fn write(&self, model: &ShellViewModel) {
        if self.view.defined_at().is_none() {
            eprintln!("Nana 缺失仓库页的信号已随骨架回收，跳过写入");
            return;
        }
        self.view.try_set_if_changed(MissingView::project(model));
        let draft = &model.workspace.path_draft;
        if self.projected.with_untracked(|projected| projected != draft) {
            self.projected.try_set_if_changed(draft.clone());
            self.draft.try_set_if_changed(draft.clone());
        }
    }
}

/// 缺失仓库页：首页外框里纵向滚动的一节，内容在可见高度里居中。
pub(super) fn view(signals: MissingSignals) -> AnyView {
    let section = widget(fill_section()).children((panel(signals),)).key("missing-repository-page").into_any();
    super::route_home::resident_page(section, "workspace-missing-scroll")
}

/// 给一个节点套上外边距。Vue 用 `margin` 微调的几处间距照搬。
fn with_margin(child: AnyView, top: f32, bottom: f32) -> AnyView {
    widget(margin(top, bottom)).children((child,)).into_any()
}

/// 缺失仓库：图标、眉题、仓库名、说明、路径框、错误、重定向输入和三个操作。
fn panel(signals: MissingSignals) -> AnyView {
    let view = signals.view;
    let initial = view.get_untracked();
    let icon = widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(42.0))
            .height(LengthSpec::Px(42.0))
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .painter(SoftFill::err().radius(RadiusTier::Md)),
    )
    .children((widget(IconGlyph::new(ALERT_TRIANGLE).size(22.0).role(SemanticColorRole::Danger)),))
    .key("missing-icon");
    let name = widget(line(initial.name.clone(), 22.0, 650, SemanticColorRole::Text, 22.0 * ROOT_LINE_HEIGHT))
        .prop::<String, fields::text::value>(move || view.with(|view| view.name.clone()))
        .key("missing-name");
    let mut summary = wrapping(initial.summary(), 13.0, 400, SemanticColorRole::Muted, 13.0 * 1.6);
    Arc::make_mut(&mut summary.style.layout).max_width = Some(LengthSpec::Px(520.0));
    let summary = widget(summary)
        .prop::<String, fields::text::value>(move || view.with(|view| view.summary().to_string()))
        .key("missing-summary");
    widget(Stack::column(0.0).width(LengthSpec::Fill).align(AlignSpec::Start).with_layout(|layout| {
        layout.max_width = Some(LengthSpec::Px(MISSING_PANEL_WIDTH));
    }))
    .children((
        with_margin(icon.into_any(), 0.0, 14.0),
        eyebrow("资源库丢失", "missing-eyebrow"),
        with_margin(name.into_any(), 4.0, 0.0),
        with_margin(summary.into_any(), 12.0, 0.0),
        with_margin(path_box(view), 16.0, 0.0),
        error_block(view),
        editor(signals),
        with_margin(actions(view), 18.0, 0.0),
    ))
    .key("missing-panel")
    .into_any()
}

/// 路径框：`--bg` 底、`--border` 描边、等宽 12px、可在任意位置折行。
fn path_box(view: Signal<MissingView>) -> AnyView {
    let mut node = wrapping(view.with_untracked(|view| view.path.clone()), 12.0, 400, SemanticColorRole::Text, 18.0);
    Arc::make_mut(&mut node.style.layout).font_family = Some(MONO_FONTS.into());
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .padding_xy(12.0, 10.0)
            .surface(SemanticColorRole::Background)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Sm),
    )
    .children((widget(node).prop::<String, fields::text::value>(move || view.with(|view| view.path.clone())).key("missing-path"),))
    .into_any()
}

/// 错误条：重定向、刷新或删除失败时显示。
fn error_block(view: Signal<MissingView>) -> AnyView {
    let copy = wrapping(view.with_untracked(|view| view.error.clone()), 12.0, 400, SemanticColorRole::Danger, 17.4);
    let error = widget(Stack::column(0.0).width(LengthSpec::Fill).padding_xy(10.0, 8.0).painter(SoftFill::err())).children((widget(copy)
        .prop::<String, fields::text::value>(move || view.with(|view| view.error.clone()))
        .key("missing-error"),));
    widget(margin(12.0, 0.0)).visible(move || view.with(|view| !view.error.is_empty())).children((error,)).into_any()
}

/// 重定向输入：路径框和「确认重定向」。选了「重定向」、而且不是来源缓存问题时显示。
fn editor(signals: MissingSignals) -> AnyView {
    let view = signals.view;
    let input = widget(TextInput::new(signals.draft.get_untracked()).label("资源库新位置"))
        .model(signals.draft)
        .key("missing-path-input")
        .on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program_all(ShellMessage::MissingPathChanged(event.value.to_string()));
        });
    let submit = widget(primary_button("确认重定向").disabled(view.with_untracked(|view| view.busy)))
        .prop::<bool, fields::button::disabled>(move || view.with(|view| view.busy))
        .key("missing-submit-path")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::MissingSubmitPath));
    let editor = widget(Stack::column(8.0).width(LengthSpec::Fill)).children((input, widget(Stack::row(0.0)).children((submit,))));
    widget(margin(12.0, 0.0)).visible(move || view.with(|view| view.prompt)).children((editor,)).into_any()
}

/// 重定向（或打开来源设置）、刷新和删除。忙的时候都不可用。
fn actions(view: Signal<MissingView>) -> AnyView {
    let initial = view.get_untracked();
    let busy = move || view.with(|view| view.busy);
    widget(Stack::row(8.0).align(AlignSpec::Center).wrap(true))
        .children((
            widget(primary_button(initial.primary).disabled(initial.busy))
                .prop::<String, fields::button::label>(move || view.with(|view| view.primary.to_string()))
                .prop::<bool, fields::button::disabled>(busy)
                .key("missing-primary")
                .on_cx(move |_, _: &Activate, cx| {
                    let message = if view.with_untracked(|view| view.cache_issue) {
                        ShellMessage::MissingOpenSourceSettings
                    } else {
                        ShellMessage::MissingChoosePath
                    };
                    cx.dispatch_program_all(message);
                }),
            widget(Button::new("刷新").kind(ButtonKind::Ghost).disabled(initial.busy))
                .prop::<bool, fields::button::disabled>(busy)
                .key("missing-refresh")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::MissingRefresh)),
            widget(Button::new(initial.delete).kind(ButtonKind::Danger).disabled(initial.busy))
                .prop::<String, fields::button::label>(move || view.with(|view| view.delete.to_string()))
                .prop::<bool, fields::button::disabled>(busy)
                .key("missing-delete")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::MissingOpenDelete)),
        ))
        .into_any()
}

#[cfg(test)]
#[path = "route_missing_tests.rs"]
mod tests;

//! 插件管理面板。
//!
//! 结构照 `src/components/PluginManagerPanel.vue`：页头（眉题、标题、说明、插件数、刷新、
//! 安装）、提示、筛选框、按分类分组的插件卡片，卡片里可展开插件设置。设置页和拓展页
//! 共用，只换标题文案。删除确认是壳层浮层，见 [`delete_dialog`]。
//!
//! 面板只建一次，只读 [`PluginPanelSignals`]：计数、提示和按钮状态按字段绑定，加载中、空状态和
//! 分组三块按面板状态显隐；分组按分类、卡片按插件 id 做键（卡片在 `admin_plugin_card.rs`）。
//! 筛选框受控，组合输入中不被打断。

use nana_ui::runtime::view::{signal, widget, AnyView, IntoView, Item, Store, StoreList, StorePath};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, TextChanged, TextInput};
use nana_ui::ButtonKind;
use nana_ui_core::SemanticColorRole as Role;

use super::super::hot::ModelField;
use super::super::view_part_overlay::dialog::{intent_button, DialogFrame};
use super::super::view_part_overlay::session::Projected;
use super::super::{ShellMessage, ShellViewModel};
use super::bind::{row_text, ActionDisabled};
use super::icons;
use super::plugins_state::{PanelState, PluginCardView, PluginGroupView, PluginGroupViewStoreFields, PluginPanelSignals};
use super::style::{self, action, column, label, Tone};
use super::AdminMessage;

/// 面板上随入口变化的文案。
pub(crate) struct PanelCopy {
    pub title: &'static str,
    pub eyebrow: &'static str,
    pub subline: &'static str,
    pub search_placeholder: &'static str,
    pub empty_title: &'static str,
    pub empty_description: &'static str,
}

/// 设置页里的「插件管理」。
pub(crate) const SETTINGS_COPY: PanelCopy = PanelCopy {
    title: "插件管理",
    eyebrow: "系统扩展",
    subline: "在这里启用、禁用、删除用户插件，或从压缩包导入新的插件。",
    search_placeholder: "筛选插件、能力或运行时",
    empty_title: "没有匹配的插件",
    empty_description: "试试其他关键词，或从压缩包导入新的插件。",
};

/// 拓展页里的「文件系统与插件」。
pub(crate) const EXTENSIONS_COPY: PanelCopy = PanelCopy {
    title: "文件系统与插件",
    eyebrow: "拓展能力",
    subline: "这里集中展示当前插件和后端能力。",
    search_placeholder: "筛选导入器、脚本或元数据拓展",
    empty_title: "没有匹配的插件",
    empty_description: "试试其他关键词，或从 .momoplug 安装新的插件。",
};

/// 分组在 Store 里的句柄。
type GroupItem = Item<Store<Vec<PluginGroupView>>, String, PluginGroupView>;

/// 整个插件管理面板。
pub(crate) fn manager_panel(signals: PluginPanelSignals, copy: &'static PanelCopy) -> AnyView {
    let head = signals.head;
    let managing = move || head.with(|head| head.managing);
    let header = style::workbench_header(
        copy.eyebrow,
        copy.title,
        copy.subline,
        vec![
            style::stat(move || head.with(|head| head.count.clone()), "admin-plugin-count"),
            widget(action("刷新", Some(icons::REFRESH_CW), Tone::Plain, false))
                .prop::<bool, ActionDisabled>(managing)
                .key("admin-plugin-refresh")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RefreshPlugins)))
                .into_any(),
            widget(action("从 .momoplug 安装", Some(icons::UPLOAD), Tone::Primary, false))
                .prop::<bool, ActionDisabled>(managing)
                .key("admin-plugin-install")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ChooseArchive)))
                .into_any(),
        ],
        "admin-plugin",
    );
    let notice = signals.notice;
    // 操作失败和读取失败用危险色块，操作结果用普通块；一次只显示一块。
    let notice_text = move |error: bool| {
        move || notice.with(|notice| notice.as_ref().filter(|(_, flagged)| *flagged == error).map(|(text, _)| text.clone()).unwrap_or_default())
    };
    let shown = move |error: bool| move || notice.with(|notice| notice.as_ref().is_some_and(|(_, flagged)| *flagged == error));
    let state = signals.state;
    let body = vec![
        header,
        style::state_notice(notice_text(true), true, "admin-plugin-error").visible(shown(true)).into_any(),
        style::state_notice(notice_text(false), false, "admin-plugin-message").visible(shown(false)).into_any(),
        search_field(signals.keyword, copy.search_placeholder),
        widget(label("正在加载插件信息", 14.0, 400, Role::Text))
            .visible(move || state.with(|state| *state == PanelState::Loading))
            .key("admin-plugin-loading")
            .into_any(),
        style::dashed_empty(copy.empty_title, copy.empty_description, "admin-plugin-empty")
            .visible(move || state.with(|state| *state == PanelState::Empty))
            .into_any(),
        groups(signals),
    ];
    style::workbench_panel(body, "admin-plugin-panel")
}

/// `.search-workbench__field`：主背景、1px 边线、lg 圆角，最小高 38，左右 12。输入框受控。
fn search_field(keyword: ModelField, placeholder: &'static str) -> AnyView {
    let value = keyword.signal();
    let input = style::bare_input(TextInput::new(value.get_untracked()).label("筛选插件").placeholder(placeholder));
    widget(style::field_frame())
        .children((widget(input).model(value).key("admin-plugin-keyword").on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetKeyword(event.value.to_string())));
        }),))
        .key("admin-plugin-search")
        .into_any()
}

/// 分组：组间 16，组内标题行最小高 28，卡片间 10。按分类做键，面板显示分组时才占布局。
fn groups(signals: PluginPanelSignals) -> AnyView {
    let state = signals.state;
    signals
        .groups
        .keyed(PluginGroupView::key)
        .each(move |group| group_view(group, signals))
        .gap(16.0)
        .visible(move || state.with(|state| *state == PanelState::Groups))
        .key("admin-plugin-groups")
        .into_any()
}

/// 一个分组：标题行和按插件 id 做键的卡片列。
fn group_view(group: GroupItem, signals: PluginPanelSignals) -> AnyView {
    let category = style::key_part(&group.get_untracked().category);
    let head = widget(style::fixed(style::spread(12.0, AlignSpec::Center), None, None).with_layout(|layout| {
        layout.min_height = Some(LengthSpec::Px(28.0));
    }))
    .children((
        style::bound(style::label_lh(String::new(), 15.0, 700, Role::Text, 1.25), row_text(group, |group| &group.label))
            .key(format!("admin-plugin-group-{category}")),
        style::bound(label(String::new(), 12.0, 400, Role::Muted), row_text(group, |group| &group.count))
            .key(format!("admin-plugin-group-count-{category}")),
    ));
    let cards = group
        .cards()
        .keyed(PluginCardView::key)
        .each(move |card| super::plugin_card::plugin_card(card, signals))
        .gap(10.0);
    widget(column(8.0)).children((head, cards)).key(format!("admin-plugin-section-{category}")).into_any()
}

/// 删除确认要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginDeleteView {
    pub message: String,
    /// 插件操作进行中：按钮禁用、确认写「删除中...」，三种关闭手势都不关。
    pub managing: bool,
}

impl PluginDeleteView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let plugin_id = model.admin.pending_delete.as_ref()?;
        let name = model
            .admin
            .plugins
            .iter()
            .find(|plugin| &plugin.plugin_id == plugin_id)
            .map(|plugin| plugin.name.clone())
            .unwrap_or_else(|| plugin_id.clone());
        Some(Self { message: format!("删除插件“{name}”后将移除其 .momoplug 安装包。"), managing: model.admin.managing })
    }
}

/// 删除确认浮层。Vue 的 `ConfirmDialog`：标题「删除插件」，确认「删除」，忙时「删除中...」。
/// 外壳走统一对话框框架，常驻，忙碌和文案按字段原地改。
pub(crate) fn delete_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(PluginDeleteView::project(model)?);
    Projected::register(view, PluginDeleteView::project);
    let busy = move || view.with(|view| view.managing);
    let cancel = intent_button("取消", ButtonKind::Ghost, busy, "admin-plugin-cancel-delete");
    let confirm = intent_button(
        move || if busy() { "删除中..." } else { "删除" }.to_string(),
        ButtonKind::Danger,
        busy,
        "admin-plugin-confirm-delete",
    );
    Some(
        DialogFrame::new("admin-plugin-delete-dialog", || "删除插件".to_string(), || ShellMessage::Admin(AdminMessage::CancelDelete))
            .busy(busy)
            .confirm(move || view.with(|view| view.message.clone()), cancel, confirm, || ShellMessage::Admin(AdminMessage::ConfirmDelete)),
    )
}

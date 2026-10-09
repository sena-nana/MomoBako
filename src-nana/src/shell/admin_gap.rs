//! 拓展页：左侧插件工具导航和右侧工具页，下面是插件管理面板。
//!
//! 照 `ExtensionsPanel.vue`：有工具页时先画两栏（导航 180–240 宽，页面占余下宽度，
//! 最小高 520），再画「文件系统与插件」。内置的 API Playground、文件导入和 Eagle 导入画原生页面；
//! 其它插件的工具页是插件自己的 Vue 组件，Nana 跑不了，只画页头和说明。
//!
//! 页面只建一次，只读 [`ToolsSignals`]：导航按工具页 id 做键，选中态和文字是绑定；右侧按当前工具页
//! 的种类换内容（原生页面的输入框和按钮在各自的页面里绑定），第三方页面的说明随工具页一起换。

use nana_ui::runtime::view::{css, dynamic, signal, widget, AnyView, IntoView, Signal};
use nana_ui::runtime::{Activate, AlignSpec, InteractionStyle, LengthSpec, NodeStyle, SemanticPaint, Stack};
use nana_ui_core::{GridTrack, RadiusTier, SemanticColorRole as Role};

use super::super::{ShellMessage, ShellViewModel};
use super::api::view::PlaygroundSignals;
use super::plugins_state::PluginPanelSignals;
use super::style::{self, column, label, pad, wrapping};
use super::support::{self, TOOL_API_PLAYGROUND};
use super::tool_native::ImportSignals;
use super::{AdminMessage, ToolPageEntry};

/// 右侧工具页显示哪一种内容。原生页面按页面 id 区分，第三方页面带着它的说明。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ToolPageKind {
    None,
    Playground,
    Native(String),
    Foreign(ToolPageEntry),
}

/// 导航里的一个工具页。整行是它的身份：文字变了这一项重建。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ToolTab {
    pub id: String,
    pub label: String,
    /// 说明，空时用插件名。
    pub description: String,
}

/// 拓展页工具区要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolsView {
    pub tabs: Vec<ToolTab>,
    /// 选中的工具页：选中的 id 对不上时是第一个。
    pub active: Option<String>,
    pub page: ToolPageKind,
}

impl ToolsView {
    /// 从 ViewModel 取工具区的投影，取舍和旧视图一致。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let active = active_page(model);
        // 同一个 id 只留第一个：节点键按 id 取，重复了挂不上。
        let mut seen = std::collections::HashSet::new();
        Self {
            tabs: model
                .admin
                .tool_pages
                .iter()
                .filter(|page| seen.insert(page.id.as_str()))
                .map(|page| ToolTab {
                    id: page.id.clone(),
                    label: page.label.clone(),
                    description: if page.description.trim().is_empty() { page.plugin_name.clone() } else { page.description.clone() },
                })
                .collect(),
            active: active.map(|page| page.id.clone()),
            page: match active {
                Some(page) if page.id == TOOL_API_PLAYGROUND => ToolPageKind::Playground,
                Some(page) if support::is_builtin_tool_page(&page.id) => ToolPageKind::Native(page.id.clone()),
                Some(page) => ToolPageKind::Foreign(page.clone()),
                None => ToolPageKind::None,
            },
        }
    }
}

/// 拓展页的信号：工具区、API Playground 和内置导入页各一份，插件管理面板和设置页共用另一份。
#[derive(Clone, Copy)]
pub(crate) struct ToolsSignals {
    pub(crate) tabs: Signal<Vec<ToolTab>>,
    pub(crate) active: Signal<Option<String>>,
    pub(crate) page: Signal<ToolPageKind>,
    pub(crate) playground: PlaygroundSignals,
    pub(crate) import: ImportSignals,
}

impl ToolsSignals {
    /// 空的信号，进路由之前由同步写入第一份投影。
    pub(crate) fn new() -> Self {
        Self {
            tabs: signal(Vec::new()),
            active: signal(None),
            page: signal(ToolPageKind::None),
            playground: PlaygroundSignals::new(),
            import: ImportSignals::new(),
        }
    }

    /// 写入工具区和当前工具页的投影。不显示的原生页面不算。
    pub(crate) fn write(&self, model: &ShellViewModel) {
        let view = ToolsView::project(model);
        match &view.page {
            ToolPageKind::Playground => self.playground.write(model),
            ToolPageKind::Native(id) => self.import.write(model, id),
            ToolPageKind::None | ToolPageKind::Foreign(_) => {}
        }
        self.tabs.try_set_if_changed(view.tabs);
        self.active.try_set_if_changed(view.active);
        self.page.try_set_if_changed(view.page);
    }
}

/// 整个拓展页：工具区和插件管理，间距 14。没有工具页时工具区不占布局。
pub(crate) fn extensions_page(signals: ToolsSignals, plugins: PluginPanelSignals) -> AnyView {
    let tabs = signals.tabs;
    let body = (
        tools(signals).visible(move || tabs.with(|tabs| !tabs.is_empty())),
        widget(Stack::column(0.0).with_layout(|layout| layout.min_height = Some(LengthSpec::Px(420.0))))
            .children((super::plugins_view::manager_panel(plugins, &super::plugins_view::EXTENSIONS_COPY),)),
    );
    widget(column(14.0)).children(body).key("admin-extensions").into_any()
}

/// 工具区：两栏网格，最小高 520。
fn tools(signals: ToolsSignals) -> nana_ui::runtime::view::El<Stack, (AnyView, AnyView)> {
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(vec![style::capped(180.0, 240.0), GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None }]);
        layout.gap = Some(LengthSpec::Px(12.0));
        layout.width = Some(LengthSpec::Fill);
        layout.min_height = Some(LengthSpec::Px(520.0));
        layout.align_items = AlignSpec::Stretch;
    });
    widget(grid).children((nav(signals), page_frame(signals))).key("admin-tool-workbench")
}

/// 左侧导航：`bg-elev` 底、md 圆角、内边距 14、间距 8。工具页按 id 做键。
fn nav(signals: ToolsSignals) -> AnyView {
    let active = signals.active;
    let tabs = nana_ui::runtime::view::each(signals.tabs, ToolTab::clone, move |tab: ToolTab| tool_tab(tab, active)).gap(8.0);
    widget(pad(column(8.0), 14.0, 14.0, 14.0, 14.0).surface(Role::Surface).radius(RadiusTier::Md))
        .children((widget(style::eyebrow_text("插件工具")).key("admin-tool-nav-eyebrow"), tabs))
        .key("admin-tool-nav")
        .into_any()
}

/// 导航里的一个工具：最小高 58、内边距 10、sm 圆角；13/700 名称和 12 号弱色说明。
/// 选中时 `bg-hover` 底、强调色名称。行的身份是工具页 id，文字随建行时的工具页。
fn tool_tab(page: ToolTab, active: Signal<Option<String>>) -> AnyView {
    let tid = style::key_part(&page.id);
    let id = page.id.clone();
    let selected = move || active.with(|active| active.as_deref() == Some(id.as_str()));
    let is_selected = selected.clone();
    let mut node = NodeStyle {
        radius: Some(RadiusTier::Sm),
        interaction: InteractionStyle {
            hovered: SemanticPaint { background: Some(Role::Hover), ..SemanticPaint::default() },
            ..InteractionStyle::default()
        },
        ..NodeStyle::default()
    };
    {
        let layout = std::sync::Arc::make_mut(&mut node.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_height = Some(LengthSpec::Px(58.0));
        layout.padding_top = Some(LengthSpec::Px(10.0));
        layout.padding_bottom = Some(LengthSpec::Px(10.0));
        layout.padding_left = Some(LengthSpec::Px(10.0));
        layout.padding_right = Some(LengthSpec::Px(10.0));
        layout.direction = Some(nana_ui_core::FlexDirection::Column);
        layout.gap = Some(LengthSpec::Px(4.0));
        layout.align_items = AlignSpec::Start;
    }
    let stack = Stack::column(4.0).style(node).hittable();
    let page_id = page.id.clone();
    widget(stack)
        .background(move || is_selected().then_some(Role::Hover))
        .children((
            widget(wrapping(label(page.label.clone(), 13.0, 700, Role::Text)))
                .foreground(move || Some(if selected() { Role::Accent } else { Role::Text }))
                .key(format!("admin-tool-{tid}")),
            widget(wrapping(style::label_lh(page.description.clone(), 12.0, 400, Role::Muted, 1.35))).key(format!("admin-tool-desc-{tid}")),
        ))
        .key(format!("admin-tool-tab-{tid}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SelectToolPage(page_id.clone())));
        })
        .into_any()
}

/// 当前选中的工具页：选中的 id 对不上时是第一个。
fn active_page(model: &ShellViewModel) -> Option<&ToolPageEntry> {
    model
        .admin
        .tool_pages
        .iter()
        .find(|page| Some(page.id.as_str()) == model.admin.active_tool_page_id.as_deref())
        .or(model.admin.tool_pages.first())
}

/// 右侧页面框：`bg-elev` 底、md 圆角、裁切溢出。里面按工具页的种类换内容，
/// 换内容的容器排版和 `Stack::fill_column(0)` 相同，内容和直接放在框里一样占满。
fn page_frame(signals: ToolsSignals) -> AnyView {
    let body = dynamic(signals.page, move |page: &ToolPageKind| match page {
        ToolPageKind::Playground => super::api::view::playground(signals.playground),
        ToolPageKind::Native(id) => super::tool_native::import_page(id, signals.import),
        ToolPageKind::Foreign(page) => foreign_page(page),
        ToolPageKind::None => widget(column(0.0)).into_any(),
    })
    .css(css! { height: 100%; min-height: 0; flex-grow: 1; flex-shrink: 1; });
    widget(Stack::fill_column(0.0).surface(Role::Surface).radius(RadiusTier::Md).with_layout(|layout| {
        layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
        layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        layout.height = Some(LengthSpec::Fill);
    }))
    .children((body,))
    .key("admin-tool-page")
    .into_any()
}

/// 第三方工具页：Vue 里是插件自己的组件。Nana 不能运行它，画页头并说明原因。
fn foreign_page(page: &ToolPageEntry) -> AnyView {
    let mut header = vec![
        widget(style::eyebrow_text(&page.plugin_name)).key(format!("admin-foreign-tool-eyebrow-{}", style::key_part(&page.id))).into_any(),
        widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0))
            .children((widget(style::label_lh(page.label.clone(), 22.0, 700, Role::Text, 1.25)).key(format!("admin-foreign-tool-title-{}", style::key_part(&page.id))),))
            .into_any(),
    ];
    if !page.description.trim().is_empty() {
        header.push(
            widget(pad(column(0.0), 8.0, 0.0, 0.0, 0.0))
                .children((widget(wrapping(label(page.description.clone(), 13.0, 400, Role::Muted))),))
                .into_any(),
        );
    }
    let notice = style::state_notice("这个工具页由插件的前端组件绘制，Nana 原生界面不能运行它。", false, "admin-foreign-tool-notice").into_any();
    widget(pad(column(14.0), 18.0, 18.0, 18.0, 18.0))
        .children((widget(column(0.0)).children(header), notice))
        .key(format!("admin-foreign-tool-{}", style::key_part(&page.id)))
        .into_any()
}

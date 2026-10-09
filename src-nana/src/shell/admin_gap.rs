//! 拓展页：左侧插件工具导航和右侧工具页，下面是插件管理面板。
//!
//! 照 `ExtensionsPanel.vue`：有工具页时先画两栏（导航 180–240 宽，页面占余下宽度，
//! 最小高 520），再画「文件系统与插件」。内置的 API Playground、文件导入和 Eagle 导入画原生页面；
//! 其它插件的工具页是插件自己的 Vue 组件，Nana 跑不了，只画页头和说明。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, InteractionStyle, LengthSpec, NodeStyle, SemanticPaint, Stack};
use nana_ui_core::{GridTrack, RadiusTier, SemanticColorRole as Role};

use super::super::{ShellMessage, ShellViewModel};
use super::style::{self, column, label, pad, wrapping};
use super::support::{self, TOOL_API_PLAYGROUND};
use super::{AdminMessage, ToolPageEntry};

/// 整个拓展页：工具区和插件管理，间距 14。
pub(crate) fn extensions_page(model: &ShellViewModel) -> AnyView {
    let mut body = Vec::new();
    if !model.admin.tool_pages.is_empty() {
        body.push(tools(model));
    }
    body.push(
        widget(Stack::column(0.0).with_layout(|layout| layout.min_height = Some(LengthSpec::Px(420.0))))
            .children((super::plugins_view::manager_panel(model, &super::plugins_view::EXTENSIONS_COPY),))
            .into_any(),
    );
    widget(column(14.0)).children(body).key("admin-extensions").into_any()
}

/// 工具区：两栏网格，最小高 520。
fn tools(model: &ShellViewModel) -> AnyView {
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(vec![
            style::capped(180.0, 240.0),
            GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None },
        ]);
        layout.gap = Some(LengthSpec::Px(12.0));
        layout.width = Some(LengthSpec::Fill);
        layout.min_height = Some(LengthSpec::Px(520.0));
        layout.align_items = AlignSpec::Stretch;
    });
    widget(grid).children((nav(model), page_frame(model))).key("admin-tool-workbench").into_any()
}

/// 左侧导航：`bg-elev` 底、md 圆角、内边距 14、间距 8。
fn nav(model: &ShellViewModel) -> AnyView {
    let active = active_page(model).map(|page| page.id.clone());
    let mut rows = vec![widget(style::eyebrow_text("插件工具")).key("admin-tool-nav-eyebrow").into_any()];
    let keys = style::unique_keys(model.admin.tool_pages.iter().map(|page| page.id.as_str()));
    for (page, key) in model.admin.tool_pages.iter().zip(&keys) {
        rows.push(tool_tab(page, key, active.as_deref() == Some(page.id.as_str())));
    }
    widget(pad(column(8.0), 14.0, 14.0, 14.0, 14.0).surface(Role::Surface).radius(RadiusTier::Md))
        .children(rows)
        .key("admin-tool-nav")
        .into_any()
}

/// 导航里的一个工具：最小高 58、内边距 10、sm 圆角；13/700 名称和 12 号弱色说明。
/// 选中时 `bg-hover` 底、强调色名称。
fn tool_tab(page: &ToolPageEntry, tid: &str, active: bool) -> AnyView {
    let id = page.id.clone();
    let description = if page.description.trim().is_empty() { page.plugin_name.clone() } else { page.description.clone() };
    let mut node = NodeStyle {
        background: active.then_some(Role::Hover),
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
    widget(stack)
        .children((
            widget(wrapping(label(page.label.clone(), 13.0, 700, if active { Role::Accent } else { Role::Text }))).key(format!("admin-tool-{tid}")),
            widget(wrapping(style::label_lh(description, 12.0, 400, Role::Muted, 1.35))).key(format!("admin-tool-desc-{tid}")),
        ))
        .key(format!("admin-tool-tab-{tid}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SelectToolPage(id.clone())));
        })
        .into_any()
}

fn active_page(model: &ShellViewModel) -> Option<&ToolPageEntry> {
    model
        .admin
        .tool_pages
        .iter()
        .find(|page| Some(page.id.as_str()) == model.admin.active_tool_page_id.as_deref())
        .or(model.admin.tool_pages.first())
}

/// 右侧页面框：`bg-elev` 底、md 圆角、裁切溢出。
fn page_frame(model: &ShellViewModel) -> AnyView {
    let body = match active_page(model) {
        Some(page) if page.id == TOOL_API_PLAYGROUND => super::api::view::playground(model),
        Some(page) if support::is_builtin_tool_page(&page.id) => super::tool_native::import_page(model, page),
        Some(page) => foreign_page(page),
        None => widget(column(0.0)).into_any(),
    };
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
    let notice = style::state_notice("这个工具页由插件的前端组件绘制，Nana 原生界面不能运行它。".into(), false, "admin-foreign-tool-notice");
    widget(pad(column(14.0), 18.0, 18.0, 18.0, 18.0))
        .children((widget(column(0.0)).children(header), notice))
        .key(format!("admin-foreign-tool-{}", style::key_part(&page.id)))
        .into_any()
}

//! 资源筛选栏，对应 Vue `pages/workspace/search/WorkspaceFilterBar.vue`。
//!
//! 头部是「当前资源库筛选」和仓库名，右侧是条件数、清除和关闭。下面按组排：
//! 格式、标签只在有候选时出现，是候选芯片；颜色、形状是候选芯片加输入框和「添加」；
//! 评分是「全部」和 1–5「星+」；库类型快捷方式在有匹配条目时出现；高级区是 12 个输入、
//! 排序方向下拉和「应用」，按 `repeat(auto-fit, minmax(132px, 1fr))` 网格排。
//! 放在页面哪里由壳层决定，这里只画筛选栏本身。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, JustifySpec, LengthSpec, RadiusTier, Select, SelectChanged, SelectOption, SemanticColorRole,
    Stack, TextChanged, TextSubmitted, Toolbar,
};
use nana_ui_core::{DisplaySpec, GridLine, GridRepeatAuto, GridTrack, GridTrackListUnsupported};

use super::super::inspect::{AdvancedField, FilterList, InspectMessage, MetadataInput, SortDirection};
use super::super::inspect_shortcuts::SearchShortcut;
use super::super::{ShellMessage, ShellViewModel};
use super::presenter::{self, RATING_OPTIONS};
use super::widgets::{self, CONTROL_HEIGHT, INPUT_WIDTH};

/// 高级区一个输入：字段、稳定键、无障碍名称、占位文字和是否占两列。
/// 键只用 ASCII，重挂后按键路径找回焦点。
struct AdvancedInput {
    field: AdvancedField,
    key: &'static str,
    name: &'static str,
    placeholder: &'static str,
    wide: bool,
}

/// Vue 高级区输入的顺序和文案。排序方向下拉在「排序字段」之后，「结果数量」之前。
const ADVANCED_INPUTS: [AdvancedInput; 11] = [
    AdvancedInput { field: AdvancedField::ExcludeQuery, key: "exclude-query", name: "排除关键词", placeholder: "排除关键词", wide: false },
    AdvancedInput { field: AdvancedField::ExcludePaths, key: "exclude-paths", name: "排除路径", placeholder: "排除路径", wide: false },
    AdvancedInput { field: AdvancedField::ExcludeTags, key: "exclude-tags", name: "排除标签", placeholder: "排除标签", wide: false },
    AdvancedInput { field: AdvancedField::ExcludeFormats, key: "exclude-formats", name: "排除格式", placeholder: "排除格式", wide: false },
    AdvancedInput { field: AdvancedField::Metadata, key: "metadata", name: "元数据", placeholder: "libraryKind=audio", wide: true },
    AdvancedInput { field: AdvancedField::ExcludeMetadata, key: "exclude-metadata", name: "排除元数据", placeholder: "status=archived", wide: true },
    AdvancedInput { field: AdvancedField::ExcludeNumber, key: "exclude-number", name: "排除数值范围", placeholder: "排除 width=0..640", wide: true },
    AdvancedInput {
        field: AdvancedField::ExcludeDate,
        key: "exclude-date",
        name: "排除日期范围",
        placeholder: "排除 fileCreatedAt=2024-01-01T00:00:00Z..",
        wide: true,
    },
    AdvancedInput { field: AdvancedField::Number, key: "number", name: "数值范围", placeholder: "width=1024..4096", wide: true },
    AdvancedInput { field: AdvancedField::Date, key: "date", name: "日期范围", placeholder: "fileCreatedAt=2024-01-01T00:00:00Z..", wide: true },
    AdvancedInput { field: AdvancedField::SortField, key: "sort-field", name: "排序字段", placeholder: "排序字段", wide: false },
];

/// 筛选栏本身。壳层在有仓库且筛选栏打开时把它放在首页面板上方。
pub(crate) fn filter_bar(model: &ShellViewModel) -> AnyView {
    let options = presenter::filter_options(model);
    let shortcuts = presenter::active_shortcuts(model);
    let filters = &model.inspect.filters;
    let mut groups = Vec::new();
    if !options.formats.is_empty() {
        groups.push(chip_group("格式", "格式筛选", "format", FilterList::Formats, &options.formats, &filters.formats));
    }
    if !options.tags.is_empty() {
        groups.push(chip_group("标签", "文件标签筛选", "tag", FilterList::Tags, &options.tags, &filters.tags));
    }
    groups.push(metadata_group(model, MetadataInput::Colors, &options.colors, &filters.colors));
    groups.push(metadata_group(model, MetadataInput::Shapes, &options.shapes, &filters.shapes));
    groups.push(rating_group(filters.min_rating));
    if !shortcuts.is_empty() {
        groups.push(shortcut_group(&shortcuts));
    }
    groups.push(advanced_group(model));
    widget(
        Stack::column(10.0)
            .padding_xy(14.0, 12.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Lg)
            .grow(0.0)
            .shrink(0.0)
            .with_layout(|layout| {
                layout.margin_bottom = Some(LengthSpec::Px(14.0));
            }),
    )
    .children((head(model), widget(Stack::column(8.0)).children(groups).key("workspace-filter-groups")))
    .key("workspace-filter-bar")
    .into_any()
}

/// 头部：左边眉题和仓库名，右边条件数、清除和关闭。
fn head(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let count = inspect.active_filter_count();
    let repository = model.workspace.active_repository().map(|item| item.name.clone()).unwrap_or_default();
    let mut actions = Vec::new();
    if count > 0 {
        actions.push(widgets::stat(format!("{count} 个条件"), "workspace-filter-count"));
    }
    let clear_disabled = !inspect.filters.has_active_filters() && inspect.query.trim().is_empty();
    actions.push(
        widget(widgets::bar_button("清除", clear_disabled))
            .key("workspace-filter-clear")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(message(InspectMessage::ClearFilters)))
            .into_any(),
    );
    actions.push(
        widget(widgets::close_button("关闭筛选栏"))
            .key("workspace-filter-close")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(message(InspectMessage::CloseFilterBar)))
            .into_any(),
    );
    let title = widget(Stack::column(3.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0))
        .children((
            widget(widgets::eyebrow("当前资源库筛选")).key("workspace-filter-eyebrow"),
            widget(widgets::label(repository, 13.0, 700, SemanticColorRole::Text)).key("workspace-filter-repository"),
        ));
    widget(Stack::bar(12.0).align(AlignSpec::Start).justify(JustifySpec::SpaceBetween))
        .children((
            title,
            widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0).wrap(true).justify(JustifySpec::End))
                .children(actions)
                .key("workspace-filter-actions"),
        ))
        .key("workspace-filter-head")
        .into_any()
}

/// 一组：左边 42px 的组名，右边内容。组名在 26px 的行盒里垂直居中。
fn group(title: &'static str, name: &'static str, key: &'static str, body: AnyView) -> AnyView {
    let caption = widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .width(LengthSpec::Px(42.0))
            .min_width(LengthSpec::Px(42.0))
            .min_height(LengthSpec::Px(CONTROL_HEIGHT))
            .grow(0.0)
            .shrink(0.0),
    )
    .children((widget(widgets::label(title, 12.0, 700, SemanticColorRole::Muted)).key(format!("workspace-filter-{key}-title")),))
    .key(format!("workspace-filter-{key}-caption"));
    let style = Stack::bar(8.0).align(AlignSpec::Start).node_style();
    widget(Toolbar::new().label(name).chrome(false).style(style))
        .children((caption, body))
        .key(format!("workspace-filter-{key}"))
        .into_any()
}

/// 芯片行：6px 间距、可折行，内容区占满组名右侧。
fn options_row(children: Vec<AnyView>, key: String) -> AnyView {
    widget(
        Stack::row(6.0)
            .align(AlignSpec::Center)
            .wrap(true)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0),
    )
    .children(children)
    .key(key)
    .into_any()
}

/// 格式、标签：候选芯片，点一下切换。
fn chip_group(
    title: &'static str,
    name: &'static str,
    key: &'static str,
    list: FilterList,
    options: &[String],
    selected: &[String],
) -> AnyView {
    let chips = options
        .iter()
        .map(|value| toggle_chip(list, key, value, selected.contains(value), None))
        .collect();
    group(title, name, key, options_row(chips, format!("workspace-filter-{key}-options")))
}

fn toggle_chip(list: FilterList, key: &str, value: &str, active: bool, swatch: Option<presenter::SwatchColor>) -> AnyView {
    let toggled = value.to_string();
    widget(widgets::chip(value, active, swatch))
        .key(format!("workspace-filter-{key}-{value}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(message(InspectMessage::ToggleFilter { key: list, value: toggled.clone() }));
        })
        .into_any()
}

/// 颜色、形状：候选芯片（颜色带色块），后面是输入框和「添加」。回车和「添加」都提交。
fn metadata_group(model: &ShellViewModel, input: MetadataInput, options: &[String], selected: &[String]) -> AnyView {
    let (title, name, key, field_name, placeholder) = match input {
        MetadataInput::Colors => ("颜色", "文件颜色筛选", "color", "输入文件颜色", "输入颜色"),
        MetadataInput::Shapes => ("形状", "形状筛选", "shape", "输入形状", "输入形状"),
    };
    let list = input.list();
    let mut children: Vec<AnyView> = options
        .iter()
        .map(|value| {
            let swatch = (input == MetadataInput::Colors).then(|| presenter::swatch_color(value));
            toggle_chip(list, key, value, selected.contains(value), swatch)
        })
        .collect();
    let draft = model.inspect.search_ui.draft.metadata_input(input).to_string();
    let empty = draft.trim().is_empty();
    let field = widget(widgets::pill_input(&draft, placeholder, field_name, Some(INPUT_WIDTH)))
        .key(format!("workspace-filter-{key}-input"))
        .on_cx(move |_, event: &TextChanged, cx| {
            cx.dispatch_program_all(message(InspectMessage::SetMetadataInput { key: input, value: event.value.to_string() }));
        })
        .on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(message(InspectMessage::SubmitMetadataInput(input))));
    let add = widget(widgets::pill_button("添加", empty))
        .key(format!("workspace-filter-{key}-add"))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(message(InspectMessage::SubmitMetadataInput(input))));
    children.push(
        widget(widgets::input_pill(false))
            .children((field, widget(widgets::pill_divider()).key(format!("workspace-filter-{key}-divider")), add))
            .key(format!("workspace-filter-{key}-field"))
            .into_any(),
    );
    group(title, name, key, options_row(children, format!("workspace-filter-{key}-options")))
}

/// 评分：「全部」清掉最低评分，「N 星+」设成 N。
fn rating_group(min_rating: Option<f64>) -> AnyView {
    let mut chips = vec![widget(widgets::chip("全部", min_rating.is_none(), None))
        .key("workspace-filter-rating-all")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(message(InspectMessage::SetMinimumRating(None))))
        .into_any()];
    for rating in RATING_OPTIONS {
        let value = f64::from(rating);
        chips.push(
            widget(widgets::chip(&format!("{rating} 星+"), min_rating == Some(value), None))
                .key(format!("workspace-filter-rating-{rating}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(message(InspectMessage::SetMinimumRating(Some(value))));
                })
                .into_any(),
        );
    }
    group("评分", "评分筛选", "rating", options_row(chips, "workspace-filter-rating-options".into()))
}

/// 库类型：每个快捷方式一个芯片，点下去改写元数据和排序。芯片不显示选中态，和 Vue 一致。
fn shortcut_group(shortcuts: &[SearchShortcut]) -> AnyView {
    let chips = shortcuts
        .iter()
        .map(|shortcut| {
            let metadata = shortcut.metadata.clone();
            let sort_field = shortcut.sort_field.clone();
            let sort_direction = shortcut.sort_direction;
            widget(widgets::chip(&shortcut.label, false, None))
                .key(format!("workspace-filter-shortcut-{}-{}", shortcut.plugin_id, shortcut.id))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(message(InspectMessage::ApplyShortcut {
                        metadata: metadata.clone(),
                        sort_field: sort_field.clone(),
                        sort_direction,
                    }));
                })
                .into_any()
        })
        .collect();
    group("库类型", "库类型筛选", "shortcut", options_row(chips, "workspace-filter-shortcut-options".into()))
}

/// 高级区：自适应列网格，宽输入占两列；回车和「应用」都把草稿写进条件。
fn advanced_group(model: &ShellViewModel) -> AnyView {
    let draft = &model.inspect.search_ui.draft;
    let mut cells: Vec<AnyView> = Vec::new();
    for input in &ADVANCED_INPUTS {
        cells.push(advanced_input(input, draft.advanced(input.field)));
        if input.field == AdvancedField::SortField {
            cells.push(sort_direction_select(draft.sort_direction));
        }
    }
    cells.push(advanced_input(
        &AdvancedInput { field: AdvancedField::Limit, key: "limit", name: "结果数量", placeholder: "数量", wide: false },
        draft.advanced(AdvancedField::Limit),
    ));
    cells.push(
        widget(widgets::bar_button("应用", false))
            .key("workspace-filter-apply")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(message(InspectMessage::ApplyAdvanced)))
            .into_any(),
    );
    let grid = widget(Stack::column(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0).with_layout(|layout| {
        layout.display = Some(DisplaySpec::Grid);
        layout.grid_columns_repeat = Some(GridRepeatAuto {
            kind: GridTrackListUnsupported::RepeatAutoFit,
            tracks: vec![GridTrack::MinMax { min_px: 132.0, fr: 1.0, max_px: None }],
            ..GridRepeatAuto::default()
        });
        layout.row_gap = Some(LengthSpec::Px(6.0));
        layout.column_gap = Some(LengthSpec::Px(6.0));
    }))
    .children(cells)
    .key("workspace-filter-advanced-grid")
    .into_any();
    group("高级", "高级筛选", "advanced", grid)
}

/// 高级区的输入胶囊，输入框填满整个格子。
/// Vue 的内层网格把输入框卡在 120px，占两列的宽输入也只用左边 120px、长占位被截断，
/// 这是给颜色形状输入写的列宽误用到了高级区，这里按设计意图让输入框用满格子。
fn advanced_input(input: &AdvancedInput, value: &str) -> AnyView {
    let field = input.field;
    let pill = widgets::input_pill(true).with_layout(|layout| {
        if input.wide {
            layout.grid_placement.column_start = GridLine::Span(2);
        }
    });
    widget(pill)
        .children((widget(widgets::pill_input(value, input.placeholder, input.name, None))
            .key(format!("workspace-filter-advanced-{}-input", input.key))
            .on_cx(move |_, event: &TextChanged, cx| {
                cx.dispatch_program_all(message(InspectMessage::SetAdvanced { field, value: event.value.to_string() }));
            })
            .on_cx(|_, _: &TextSubmitted, cx| cx.dispatch_program_all(message(InspectMessage::ApplyAdvanced))),))
        .key(format!("workspace-filter-advanced-{}", input.key))
        .into_any()
}

/// 排序方向下拉：升序、降序。选中只改草稿，「应用」后才生效。
fn sort_direction_select(direction: SortDirection) -> AnyView {
    let options = [SortDirection::Asc, SortDirection::Desc]
        .into_iter()
        .map(|item| SelectOption::new(item.value(), item.label()))
        .collect::<Vec<_>>();
    let mut select = Select::new(Some(direction.value())).options(options);
    select.style.background = None;
    select.style.border = None;
    select.style.radius = None;
    select.style.control_height = None;
    select.style.control_padding_x = None;
    select.style.interaction.hovered = Default::default();
    select.style.interaction.focused = Default::default();
    {
        let layout = std::sync::Arc::make_mut(&mut select.style.layout);
        layout.height = Some(LengthSpec::Px(CONTROL_HEIGHT - 2.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.padding_left = Some(LengthSpec::Px(8.0));
        layout.padding_right = Some(LengthSpec::Px(8.0));
        layout.border_width = Some(0.0);
        layout.font_size = Some(12.0);
        layout.font_weight = Some(400);
        layout.flex_grow = Some(1.0);
    }
    widget(widgets::input_pill(true))
        .children((widget(select).key("workspace-filter-sort-direction").on_cx(|_, event: &SelectChanged, cx| {
            cx.dispatch_program_all(message(InspectMessage::SetSortDirection(SortDirection::parse(&event.value))));
        }),))
        .key("workspace-filter-sort-direction-field")
        .into_any()
}

fn message(message: InspectMessage) -> ShellMessage {
    ShellMessage::Inspect(message)
}

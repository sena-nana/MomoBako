//! 资源筛选栏，对应 Vue `pages/workspace/search/WorkspaceFilterBar.vue`。
//!
//! 头部是「当前资源库筛选」和仓库名，右侧是条件数、清除和关闭。下面按组排：
//! 格式、标签只在有候选时出现，是候选芯片；颜色、形状是候选芯片加输入框和「添加」；
//! 评分是「全部」和 1–5「星+」；库类型快捷方式在有匹配条目时出现；高级区是 12 个输入、
//! 排序方向下拉和「应用」，按 `repeat(auto-fit, minmax(132px, 1fr))` 网格排。
//!
//! 视图只建一次（[`resident_filter_bar`]），只读 [`FilterBarSignals`]：仓库名、条件数和按钮状态按字段
//! 绑定，候选芯片按值做键的 `each`，芯片的选中态绑在已选条件上；输入框 `.model` 受控，组合输入中
//! 不被打断。放在页面哪里由壳层决定，这里只画筛选栏本身。

use std::sync::Arc;

use nana_ui::runtime::view::{computed, css, each, fields, widget, AnyView, El, InlineStyle, IntoView, Signal};
use nana_ui::runtime::{
    Activate, AlignSpec, JustifySpec, LengthSpec, RadiusTier, Select, SelectChanged, SelectOption, SemanticColorRole,
    Stack, TextChanged, TextSubmitted, Toolbar,
};
use nana_ui_core::{DisplaySpec, GridLine, GridRepeatAuto, GridTrack, GridTrackListUnsupported};

use super::super::admin::style::key_part;
use super::super::hot::ModelField;
use super::super::inspect::{AdvancedField, FilterList, InspectMessage, MetadataInput, SortDirection};
use super::super::inspect_shortcuts::SearchShortcut;
use super::super::ShellMessage;
use super::filter_state::{FilterBarSignals, FilterSelection};
use super::presenter::{self, RATING_OPTIONS};
use super::widgets::{self, ChipActive, CONTROL_HEIGHT, INPUT_WIDTH};

/// 高级区一个输入：字段、稳定键、无障碍名称、占位文字和是否占两列。
/// 键只用 ASCII，按键路径找得到。
struct AdvancedInput {
    field: AdvancedField,
    key: &'static str,
    name: &'static str,
    placeholder: &'static str,
    wide: bool,
}

/// Vue 高级区输入的顺序和文案。排序方向下拉在「排序字段」之后，「结果数量」之前。
const ADVANCED_INPUTS: [AdvancedInput; 12] = [
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
    AdvancedInput { field: AdvancedField::Limit, key: "limit", name: "结果数量", placeholder: "数量", wide: false },
];

/// 常驻筛选栏：只读 `signals`，不读 ViewModel，别的常驻首页路由直接嵌进首页外框的筛选栏位置。
/// 显隐跟着 `signals.open`（有仓库且筛选栏打开），藏起时节点留着、不占布局。
pub(crate) fn resident_filter_bar(signals: FilterBarSignals) -> AnyView {
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
    .visible(signals.open)
    .children((
        head(signals),
        widget(Stack::column(8.0))
            .children((
                chip_group("格式", "格式筛选", "format", FilterList::Formats, signals.formats, signals),
                chip_group("标签", "文件标签筛选", "tag", FilterList::Tags, signals.tags, signals),
                metadata_group(MetadataInput::Colors, signals),
                metadata_group(MetadataInput::Shapes, signals),
                rating_group(signals.min_rating),
                shortcut_group(signals.shortcuts),
                advanced_group(signals),
            ))
            .key("workspace-filter-groups"),
    ))
    .key("workspace-filter-bar")
    .into_any()
}

/// 头部：左边眉题和仓库名，右边条件数、清除和关闭。
fn head(signals: FilterBarSignals) -> AnyView {
    let head = signals.head;
    let count = widgets::stat(move || head.with(|head| format!("{} 个条件", head.count)), "workspace-filter-count")
        .visible(move || head.with(|head| head.count > 0));
    let clear = widget(widgets::bar_button("清除", false))
        .prop::<bool, fields::button::disabled>(move || head.with(|head| head.clear_disabled))
        .key("workspace-filter-clear")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(message(InspectMessage::ClearFilters)));
    let close = widget(widgets::close_button("关闭筛选栏"))
        .key("workspace-filter-close")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(message(InspectMessage::CloseFilterBar)));
    let title = widget(Stack::column(3.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0))
        .children((
            widget(widgets::eyebrow("当前资源库筛选")).key("workspace-filter-eyebrow"),
            widgets::bound(widgets::label(String::new(), 13.0, 700, SemanticColorRole::Text), move || {
                head.with(|head| head.repository.clone())
            })
            .key("workspace-filter-repository"),
        ));
    widget(Stack::bar(12.0).align(AlignSpec::Start).justify(JustifySpec::SpaceBetween))
        .children((
            title,
            widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0).wrap(true).justify(JustifySpec::End))
                .children((count, clear, close))
                .key("workspace-filter-actions"),
        ))
        .key("workspace-filter-head")
        .into_any()
}

/// 一组：左边 42px 的组名，右边内容。组名在 26px 的行盒里垂直居中。
fn group(title: &'static str, name: &'static str, key: &'static str, body: AnyView) -> El<Toolbar, (AnyView, AnyView)> {
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
    .key(format!("workspace-filter-{key}-caption"))
    .into_any();
    let style = Stack::bar(8.0).align(AlignSpec::Start).node_style();
    widget(Toolbar::new().label(name).chrome(false).style(style)).children((caption, body)).key(format!("workspace-filter-{key}"))
}

/// 芯片行的排版：6px 间距、可折行、占满组名右侧。芯片行是 `each` 的容器，`horizontal(6)` 之外
/// 补上和旧视图 `Stack::row(6).wrap(true).width(Fill).min_width(0).grow(1).shrink(1)` 相同的字段。
fn options_row_style() -> InlineStyle {
    css! { flex-wrap: wrap; width: 100%; min-width: 0; flex-grow: 1; flex-shrink: 1; }
}

/// 格式、标签：候选芯片，点一下切换。没有候选时整组不显示。
fn chip_group(
    title: &'static str,
    name: &'static str,
    key: &'static str,
    list: FilterList,
    options: Signal<Vec<String>>,
    signals: FilterBarSignals,
) -> AnyView {
    let selected = signals.selected;
    let chips = each(options, String::clone, move |value| toggle_chip(list, key, value, selected, None))
        .horizontal(6.0)
        .css(options_row_style())
        .key(format!("workspace-filter-{key}-options"));
    group(title, name, key, chips.into_any()).visible(move || options.with(|options| !options.is_empty())).into_any()
}

/// 一枚可切换的候选芯片。值是行的身份，处理器只带它和所属的列表。
fn toggle_chip(
    list: FilterList,
    key: &'static str,
    value: String,
    selected: Signal<FilterSelection>,
    swatch: Option<presenter::SwatchColor>,
) -> AnyView {
    let current = value.clone();
    let toggled = value.clone();
    widget(widgets::chip(&value, false, swatch))
        .prop::<bool, ChipActive>(move || selected.with(|selected| selected_in(selected, list).contains(&current)))
        .key(format!("workspace-filter-{key}-{}", key_part(&value)))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(message(InspectMessage::ToggleFilter { key: list, value: toggled.clone() }));
        })
        .into_any()
}

/// 某个筛选列表的已选值。
fn selected_in(selected: &FilterSelection, list: FilterList) -> &[String] {
    match list {
        FilterList::Formats => &selected.formats,
        FilterList::Tags => &selected.tags,
        FilterList::Colors => &selected.colors,
        FilterList::Shapes => &selected.shapes,
        FilterList::ExcludeTags | FilterList::ExcludeFormats => &[],
    }
}

/// 颜色、形状芯片行里的一项：候选芯片，或排在最后的输入胶囊。输入胶囊的键不变，
/// 候选怎么增删它都留在原节点上。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum MetadataItem {
    Chip(String),
    Field,
}

/// 颜色、形状：候选芯片（颜色带色块），后面是输入框和「添加」。回车和「添加」都提交。
fn metadata_group(input: MetadataInput, signals: FilterBarSignals) -> AnyView {
    let (title, name, key) = match input {
        MetadataInput::Colors => ("颜色", "文件颜色筛选", "color"),
        MetadataInput::Shapes => ("形状", "形状筛选", "shape"),
    };
    let (options, draft) = match input {
        MetadataInput::Colors => (signals.colors, signals.color),
        MetadataInput::Shapes => (signals.shapes, signals.shape),
    };
    let items = computed(move || {
        let mut items = options.with(|options| options.iter().cloned().map(MetadataItem::Chip).collect::<Vec<_>>());
        items.push(MetadataItem::Field);
        items
    });
    let selected = signals.selected;
    let row = each(items, MetadataItem::clone, move |item| match item {
        MetadataItem::Chip(value) => {
            let swatch = (input == MetadataInput::Colors).then(|| presenter::swatch_color(&value));
            toggle_chip(input.list(), key, value, selected, swatch)
        }
        MetadataItem::Field => metadata_field(input, key, draft),
    })
    .horizontal(6.0)
    .css(options_row_style())
    .key(format!("workspace-filter-{key}-options"));
    group(title, name, key, row.into_any()).into_any()
}

/// 颜色、形状的输入胶囊：输入框受控，「添加」在输入为空时禁用。
fn metadata_field(input: MetadataInput, key: &'static str, draft: ModelField) -> AnyView {
    let (field_name, placeholder) = match input {
        MetadataInput::Colors => ("输入文件颜色", "输入颜色"),
        MetadataInput::Shapes => ("输入形状", "输入形状"),
    };
    let value = draft.signal();
    // 初值照信号建，光标在末尾，和旧视图一样；之后由 `.model` 受控。
    let field = widget(widgets::pill_input(&value.get_untracked(), placeholder, field_name, Some(INPUT_WIDTH)))
        .model(value)
        .key(format!("workspace-filter-{key}-input"))
        .on_cx(move |_, event: &TextChanged, cx| {
            cx.dispatch_program_all(message(InspectMessage::SetMetadataInput { key: input, value: event.value.to_string() }));
        })
        .on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(message(InspectMessage::SubmitMetadataInput(input))));
    let add = widget(widgets::pill_button("添加", false))
        .prop::<bool, fields::button::disabled>(move || value.with(|value| value.trim().is_empty()))
        .key(format!("workspace-filter-{key}-add"))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(message(InspectMessage::SubmitMetadataInput(input))));
    widget(widgets::input_pill(false))
        .children((field, widget(widgets::pill_divider()).key(format!("workspace-filter-{key}-divider")), add))
        .key(format!("workspace-filter-{key}-field"))
        .into_any()
}

/// 评分：「全部」清掉最低评分，「N 星+」设成 N。芯片固定，选中态绑在最低评分上。
fn rating_group(min_rating: Signal<Option<f64>>) -> AnyView {
    let mut chips = vec![widget(widgets::chip("全部", false, None))
        .prop::<bool, ChipActive>(move || min_rating.with(Option::is_none))
        .key("workspace-filter-rating-all")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(message(InspectMessage::SetMinimumRating(None))))
        .into_any()];
    for rating in RATING_OPTIONS {
        let value = f64::from(rating);
        chips.push(
            widget(widgets::chip(&format!("{rating} 星+"), false, None))
                .prop::<bool, ChipActive>(move || min_rating.with(|current| *current == Some(value)))
                .key(format!("workspace-filter-rating-{rating}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(message(InspectMessage::SetMinimumRating(Some(value))));
                })
                .into_any(),
        );
    }
    let row = widget(
        Stack::row(6.0)
            .align(AlignSpec::Center)
            .wrap(true)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0),
    )
    .children(chips)
    .key("workspace-filter-rating-options");
    group("评分", "评分筛选", "rating", row.into_any()).into_any()
}

/// 一个快捷方式芯片的身份：插件、标识和它带的全部条件。条件变了芯片重建，处理器带的值不会过期。
fn shortcut_key(shortcut: &SearchShortcut) -> (String, String, String, String, String, &'static str) {
    (
        shortcut.plugin_id.clone(),
        shortcut.id.clone(),
        shortcut.label.clone(),
        shortcut.metadata.clone(),
        shortcut.sort_field.clone(),
        shortcut.sort_direction.value(),
    )
}

/// 库类型：每个快捷方式一个芯片，点下去改写元数据和排序。芯片不显示选中态，和 Vue 一致。
/// 没有匹配条目时整组不显示。
fn shortcut_group(shortcuts: Signal<Vec<SearchShortcut>>) -> AnyView {
    let chips = each(shortcuts, shortcut_key, |shortcut: SearchShortcut| {
        let metadata = shortcut.metadata.clone();
        let sort_field = shortcut.sort_field.clone();
        let sort_direction = shortcut.sort_direction;
        widget(widgets::chip(&shortcut.label, false, None))
            .key(format!("workspace-filter-shortcut-{}-{}", key_part(&shortcut.plugin_id), key_part(&shortcut.id)))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(message(InspectMessage::ApplyShortcut {
                    metadata: metadata.clone(),
                    sort_field: sort_field.clone(),
                    sort_direction,
                }));
            })
            .into_any()
    })
    .horizontal(6.0)
    .css(options_row_style())
    .key("workspace-filter-shortcut-options");
    group("库类型", "库类型筛选", "shortcut", chips.into_any())
        .visible(move || shortcuts.with(|shortcuts| !shortcuts.is_empty()))
        .into_any()
}

/// 高级区：自适应列网格，宽输入占两列；回车和「应用」都把草稿写进条件。
fn advanced_group(signals: FilterBarSignals) -> AnyView {
    let mut cells: Vec<AnyView> = Vec::new();
    for input in &ADVANCED_INPUTS {
        cells.push(advanced_input(input, signals.advanced(input.field)));
        if input.field == AdvancedField::SortField {
            cells.push(sort_direction_select(signals.sort_direction));
        }
    }
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
    group("高级", "高级筛选", "advanced", grid).into_any()
}

/// 高级区的输入胶囊，输入框填满整个格子，受控于这个字段的草稿。
/// Vue 的内层网格把输入框卡在 120px，占两列的宽输入也只用左边 120px、长占位被截断，
/// 这是给颜色形状输入写的列宽误用到了高级区，这里按设计意图让输入框用满格子。
fn advanced_input(input: &AdvancedInput, draft: ModelField) -> AnyView {
    let field = input.field;
    let pill = widgets::input_pill(true).with_layout(|layout| {
        if input.wide {
            layout.grid_placement.column_start = GridLine::Span(2);
        }
    });
    widget(pill)
        .children((widget(widgets::pill_input(&draft.signal().get_untracked(), input.placeholder, input.name, None))
            .model(draft.signal())
            .key(format!("workspace-filter-advanced-{}-input", input.key))
            .on_cx(move |_, event: &TextChanged, cx| {
                cx.dispatch_program_all(message(InspectMessage::SetAdvanced { field, value: event.value.to_string() }));
            })
            .on_cx(|_, _: &TextSubmitted, cx| cx.dispatch_program_all(message(InspectMessage::ApplyAdvanced))),))
        .key(format!("workspace-filter-advanced-{}", input.key))
        .into_any()
}

/// 排序方向下拉：升序、降序。选中只改草稿，「应用」后才生效。没有可见标题，读屏名称写「排序方向」。
fn sort_direction_select(direction: Signal<SortDirection>) -> AnyView {
    let options = [SortDirection::Asc, SortDirection::Desc]
        .into_iter()
        .map(|item| SelectOption::new(item.value(), item.label()))
        .collect::<Vec<_>>();
    let mut select = Select::new(Some(direction.get_untracked().value())).options(options).label("排序方向");
    select.style.background = None;
    select.style.border = None;
    select.style.radius = None;
    select.style.control_height = None;
    select.style.control_padding_x = None;
    select.style.interaction.hovered = Default::default();
    select.style.interaction.focused = Default::default();
    {
        let layout = Arc::make_mut(&mut select.style.layout);
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
        .children((widget(select)
            .prop::<Option<Arc<str>>, fields::select::value>(move || Some(Arc::from(direction.with(|direction| direction.value()))))
            .key("workspace-filter-sort-direction")
            .on_cx(|_, event: &SelectChanged, cx| {
                cx.dispatch_program_all(message(InspectMessage::SetSortDirection(SortDirection::parse(&event.value))));
            }),))
        .key("workspace-filter-sort-direction-field")
        .into_any()
}

fn message(message: InspectMessage) -> ShellMessage {
    ShellMessage::Inspect(message)
}

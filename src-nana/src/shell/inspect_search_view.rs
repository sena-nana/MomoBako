//! 搜索面板和资源筛选栏。
//!
//! 标题栏输入进入搜索，筛选条件改动后重新查询当前资源库。

use std::sync::Arc;

use nana_ui::runtime::view::{button, icon_button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Chip, EmptyState, Icon,
    LengthSpec, ListItem, RadiusTier, SemanticColorRole, Stack, TextChanged, TextInput,
    ValidationIntent, ValidationMessage,
};
use nana_ui::ControlSize;

use super::inspect::{
    AdvancedField, FilterList, InspectMessage, InspectState, SearchFilters, SortDirection,
};
use super::{ShellMessage, ShellViewModel};

pub(super) fn search_panel(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    if model.workspace.panel != super::workspace::WorkspacePanel::Search {
        return search_controls(inspect, false);
    }
    let mut rows = Vec::new();
    if inspect.filter_bar_open {
        rows.push(filter_bar(model));
    }
    rows.push(search_results(model));
    // 筛选栏按内容高度，结果面板接在下面，不让筛选卡片撑满整页。
    widget(Stack::column(12.0).min_height(LengthSpec::Px(0.0))).children(rows).key("search-workbench").into_any()
}

/// 搜索结果面板。筛选栏在它上面，不塞进结果列表里。
fn search_results(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let scope = if inspect.filters.has_active_filters() {
        let name = model.workspace.active_repository().map(|item| item.name.clone()).unwrap_or_else(|| "当前资源库".into());
        format!("{name}内筛选")
    } else {
        "全局搜索".into()
    };
    let mut body = vec![super::workbench::header(
        &scope,
        "inspect-search-eyebrow",
        "搜索结果",
        "inspect-search-title",
        &search_summary(inspect),
        "inspect-search-summary",
        vec![
            super::workbench::badge(format!("{} 个仓库", model.workspace.repositories.len()), "inspect-search-repos"),
            super::workbench::badge(format!("{} 条结果", inspect.results.len()), "inspect-search-hits"),
        ],
    )];
    if !inspect.search_error.is_empty() {
        body.push(widget(ValidationMessage::new(inspect.search_error.clone(), ValidationIntent::Danger)).key("inspect-search-error").into_any());
    }
    if inspect.searching {
        body.push(widget(super::workbench::meta("正在执行全局搜索")).key("inspect-search-loading").into_any());
    } else if model.workspace.repositories.is_empty() {
        body.push(super::workbench::dashed_empty(
            "还没有可搜索的资源库",
            "先在资源库页面添加一个仓库，再执行跨仓库搜索。",
            "inspect-search-empty",
            "inspect-search-empty-detail",
            false,
            true,
        ));
    } else if inspect.results.is_empty() {
        body.push(super::workbench::dashed_empty(
            "等待搜索条件",
            "输入关键词、标签或评分条件后，这里会展示结果。",
            "inspect-search-empty",
            "inspect-search-empty-detail",
            false,
            true,
        ));
    } else {
        body.extend(inspect.results.iter().map(search_hit));
    }
    super::workbench::panel(body)
}

fn search_hit(row: &super::inspect::SearchRow) -> AnyView {
    let asset_id = row.asset_id.clone();
    let detail = if row.path.is_empty() { row.repo_name.clone() } else { format!("{} / {}", row.repo_name, row.path) };
    let kind = file_kind(&row.filename);
    widget(Stack::row(8.0).align(AlignSpec::Center))
        .key(format!("inspect-hit-{}", row.asset_id))
        .children((
            widget(Chip::new(kind)).key(format!("inspect-hit-kind-{}", row.asset_id)),
            widget(ListItem::new(row.filename.clone()).detail(detail)).on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::OpenHit(asset_id.clone())))
            }),
        ))
        .into_any()
}

/// 结果行的类型标记。没有扩展名时用「文件」，不另造分类。
fn file_kind(filename: &str) -> String {
    match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => ext.to_ascii_uppercase(),
        _ => "文件".into(),
    }
}

fn search_controls(inspect: &InspectState, embedded: bool) -> AnyView {
    let mut rows = Vec::new();
    if !embedded {
        rows.push(text(search_summary(inspect)).key("inspect-search-summary").into_any());
    }
    rows.push(field("搜索", "inspect-query", &inspect.query, false, |value| InspectMessage::SetQuery(value)));
    rows.push(widget(Stack::row(8.0)).children((
        button("搜索").key("inspect-run-search").disabled(inspect.searching).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::RunSearch));
        }),
        button(if inspect.filter_bar_open { "关闭筛选" } else { "筛选" }).key("inspect-filter-toggle").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::ToggleFilterBar));
        }),
    )).into_any());
    if !embedded && !inspect.search_error.is_empty() {
        rows.push(widget(ValidationMessage::new(inspect.search_error.clone(), ValidationIntent::Danger)).key("inspect-search-error").into_any());
    }
    if !embedded && inspect.results.is_empty() && !inspect.searching {
        rows.push(
            widget(EmptyState::new("没有搜索结果").message("换一个关键词，或清空筛选后再查。").compact(true))
                .key("inspect-search-empty")
                .into_any(),
        );
    }
    if !embedded {
        rows.extend(inspect.results.iter().map(search_hit));
    }
    widget(Stack::column(8.0)).children(rows).into_any()
}

pub(super) fn filter_bar(model: &ShellViewModel) -> AnyView {
    let filters = &model.inspect.filters;
    let searching = model.inspect.searching;
    let shortcuts = model.inspect.shortcuts.as_slice();
    let name = model.workspace.active_repository().map(|item| item.name.clone()).unwrap_or_else(|| "当前资源库".into());
    let count = filter_count(filters);
    let mut trailing = Vec::new();
    if count > 0 {
        trailing.push(super::workbench::badge(format!("{count} 个条件"), "inspect-filter-count"));
    }
    trailing.push(
        button("清除").disabled(searching || (count == 0 && model.inspect.query.trim().is_empty())).key("inspect-clear-filters").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::ClearFilters));
        }).into_any(),
    );
    trailing.push(
        icon_button(Icon::Close, "关闭筛选栏").key("inspect-filter-close").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::ToggleFilterBar));
        }).into_any(),
    );
    // 左列用 Shrink，避免按父级 100% 把清除和关闭挤出卡片。
    let mut rows = vec![widget(Stack::bar(8.0).align(AlignSpec::Center))
        .children((
            widget(Stack::column(2.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
                widget(super::workbench::eyebrow("当前资源库筛选")).key("inspect-filter-title"),
                widget(super::workbench::section_title(name)).key("inspect-filter-repo"),
            )),
            widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0)).children(trailing),
        ))
        .into_any()];
    rows.push(chip_row("格式", FilterList::Formats, &filters.formats, searching, "添加格式"));
    rows.push(chip_row("标签", FilterList::Tags, &filters.tags, searching, "添加标签"));
    rows.push(chip_row("颜色", FilterList::Colors, &filters.colors, searching, "输入颜色"));
    rows.push(chip_row("形状", FilterList::Shapes, &filters.shapes, searching, "输入形状"));
    rows.push(rating_group(filters.min_rating));
    if !shortcuts.is_empty() {
        rows.push(labeled_row("库类型", shortcut_row(shortcuts, searching)));
    }
    rows.push(widget(super::workbench::eyebrow("高级")).key("inspect-filter-advanced").into_any());
    let mut advanced = Vec::new();
    for (label, field) in [
        ("排除关键词", AdvancedField::ExcludeQuery),
        ("排除路径", AdvancedField::ExcludePaths),
        ("排除标签", AdvancedField::ExcludeTags),
        ("排除格式", AdvancedField::ExcludeFormats),
        ("元数据", AdvancedField::Metadata),
        ("排除元数据", AdvancedField::ExcludeMetadata),
        ("数值", AdvancedField::Number),
        ("排除数值", AdvancedField::ExcludeNumber),
        ("日期", AdvancedField::Date),
        ("排除日期", AdvancedField::ExcludeDate),
        ("排序字段", AdvancedField::SortField),
        ("条数", AdvancedField::Limit),
    ] {
        advanced.push(advanced_field(label, field, searching));
    }
    advanced.push(
        widget(Stack::row(8.0).wrap(true)).children((
            button("升序").key("inspect-sort-asc").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SetSortDirection(SortDirection::Asc)));
            }),
            button("降序").key("inspect-sort-desc").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SetSortDirection(SortDirection::Desc)));
            }),
            button("应用筛选").key("inspect-apply-filters").disabled(searching).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::ApplyAdvanced));
            }),
            button("加入筛选").key("inspect-submit-filter").disabled(searching).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SubmitFilterInput));
            }),
        )).into_any(),
    );
    rows.push(widget(Stack::row(8.0).wrap(true)).children(advanced).key("inspect-filter-advanced-fields").into_any());
    widget(
        Stack::column(8.0)
            .surface(SemanticColorRole::Surface)
            .radius(nana_ui::runtime::RadiusTier::Lg)
            .padding_xy(16.0, 12.0)
            .grow(0.0)
            .shrink(0.0),
    )
    .children(rows)
    .key("workspace-filter-bar")
    .into_any()
}

fn filter_count(filters: &SearchFilters) -> usize {
    filters.tags.len()
        + filters.formats.len()
        + filters.colors.len()
        + filters.shapes.len()
        + usize::from(filters.min_rating.is_some())
        + usize::from(!filters.metadata_filters.trim().is_empty())
        + usize::from(!filters.sort_field.trim().is_empty())
}

/// 左标签、右芯片，对应 Vue 筛选栏 `42px + 1fr` 的一行。
fn labeled_row(label: &str, body: AnyView) -> AnyView {
    widget(Stack::row(8.0).align(AlignSpec::Center).wrap(true))
        .children((widget(super::workbench::meta(label)).key(format!("inspect-filter-group-{label}")), body))
        .into_any()
}

fn rating_group(min_rating: Option<f64>) -> AnyView {
    let mut buttons = vec![widget(Chip::new("全部").selected(min_rating.is_none())).key("inspect-rating-clear").on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program(inspect_message(InspectMessage::SetMinimumRating(None)));
    }).into_any()];
    for rating in 1..=5 {
        let selected = min_rating == Some(f64::from(rating));
        buttons.push(
            widget(Chip::new(format!("{rating} 星+")).selected(selected))
                .key(format!("inspect-rating-{rating}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(inspect_message(InspectMessage::SetMinimumRating(Some(f64::from(rating)))));
                })
                .into_any(),
        );
    }
    labeled_row("评分", widget(Stack::row(8.0).wrap(true)).children(buttons).into_any())
}

/// 库类型快捷方式。点下去走已有的 `ApplyShortcut`。
fn shortcut_row(shortcuts: &[super::inspect_shortcuts::SearchShortcut], searching: bool) -> AnyView {
    let buttons = shortcuts
        .iter()
        .map(|shortcut| {
            let metadata = shortcut.metadata.clone();
            let sort_field = shortcut.sort_field.clone();
            let direction = shortcut.sort_direction;
            button(shortcut.label.clone())
                .key(format!("inspect-shortcut-{}", shortcut.id))
                .disabled(searching)
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(inspect_message(InspectMessage::ApplyShortcut {
                        metadata: metadata.clone(),
                        sort_field: sort_field.clone(),
                        sort_direction: direction,
                    }));
                })
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(Stack::row(8.0).wrap(true)).children(buttons).key("inspect-shortcuts").into_any()
}

fn chip_row(label: &'static str, key: FilterList, values: &[String], disabled: bool, placeholder: &'static str) -> AnyView {
    let mut chips = Vec::new();
    for value in values {
        let chip_value = value.clone();
        let chip = widget(Chip::new(value.clone()).selected(true).disabled(disabled))
            .key(format!("inspect-filter-chip-{label}-{value}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::ToggleFilter { key, value: chip_value.clone() }));
            })
            .into_any();
        if key == FilterList::Colors {
            chips.push(
                widget(Stack::row(4.0).align(AlignSpec::Center))
                    .key(format!("inspect-filter-color-{value}"))
                    .children((color_swatch(value), chip))
                    .into_any(),
            );
        } else {
            chips.push(chip);
        }
    }
    labeled_row(
        label,
        widget(Stack::row(8.0).align(AlignSpec::Center).wrap(true)).children((
            widget(Stack::row(4.0).wrap(true)).children(chips),
            widget(compact_input(placeholder, disabled))
                .key(format!("inspect-filter-{label}"))
                .on_cx(move |_, event: &TextChanged, cx| {
                    cx.dispatch_program(inspect_message(InspectMessage::SetFilterInput { key, value: event.value.to_string() }));
                }),
        )).into_any(),
    )
}

/// 颜色筛选用色块。能认的名字映射到语义色，不把色名再写进另一段说明。
fn color_swatch(name: &str) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(14.0))
            .height(LengthSpec::Px(14.0))
            .min_width(LengthSpec::Px(14.0))
            .min_height(LengthSpec::Px(14.0))
            .grow(0.0)
            .shrink(0.0)
            .surface(color_role(name))
            .radius(RadiusTier::Sm),
    )
    .key(format!("inspect-filter-swatch-{name}"))
    .into_any()
}

fn color_role(name: &str) -> SemanticColorRole {
    match name.trim().to_ascii_lowercase().as_str() {
        "red" | "红色" => SemanticColorRole::Danger,
        "green" | "绿色" => SemanticColorRole::Success,
        "yellow" | "黄色" | "orange" | "橙色" => SemanticColorRole::Warning,
        "blue" | "蓝色" | "purple" | "紫色" | "pink" | "粉色" => SemanticColorRole::Accent,
        _ => SemanticColorRole::Border,
    }
}

/// 高级条件用短输入横排，对应 Vue 筛选栏的自适应网格。
fn compact_input(placeholder: &'static str, disabled: bool) -> TextInput {
    let mut field = TextInput::new(String::new()).placeholder(placeholder).size(ControlSize::Small).disabled(disabled);
    let layout = Arc::make_mut(&mut field.style.layout);
    layout.width = Some(LengthSpec::Px(148.0));
    layout.min_width = Some(LengthSpec::Px(120.0));
    layout.max_width = Some(LengthSpec::Px(180.0));
    layout.flex_grow = Some(0.0);
    field
}

fn advanced_field(label: &'static str, field: AdvancedField, disabled: bool) -> AnyView {
    widget(compact_input(label, disabled))
        .key(format!("inspect-advanced-{label}"))
        .on_cx(move |_, event: &TextChanged, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::SetAdvanced { field, value: event.value.to_string() }));
        })
        .into_any()
}

fn field(
    label: &'static str,
    key_name: &'static str,
    value: &str,
    disabled: bool,
    map: impl Fn(String) -> InspectMessage + Send + 'static,
) -> AnyView {
    widget(TextInput::new(value.to_string()).label(label).disabled(disabled)).key(key_name).on_cx(move |_, event: &TextChanged, cx| {
        cx.dispatch_program(inspect_message(map(event.value.to_string())));
    }).into_any()
}

fn search_summary(inspect: &InspectState) -> String {
    if inspect.filters.has_active_filters() {
        if inspect.query.trim().is_empty() {
            "按当前资源库筛选结果。".into()
        } else {
            format!("当前资源库筛选: {}", inspect.query)
        }
    } else if inspect.query.trim().is_empty() {
        "输入关键词、标签或评分条件后，这里会展示跨仓库结果。".into()
    } else {
        format!("当前查询: {}", inspect.query)
    }
}

fn inspect_message(message: InspectMessage) -> ShellMessage {
    ShellMessage::Inspect(message)
}

//! 资源筛选栏的投影和常驻信号。
//!
//! 筛选栏由首页外框放在首页面板上方，有仓库的首页路由都可能显示它。[`FilterBarSignals`] 建在
//! 主区块的常驻作用域里（`RouteSignals::filter`），除启动页和设置页以外每次同步都写，所以哪条
//! 常驻首页路由嵌了常驻筛选栏，读到的都是同一份最新的值。筛选栏关着时不算候选和库类型快捷方式，
//! 打开时再算；输入框的草稿走 [`ModelField`]，ViewModel 的草稿变了才写回，组合输入和刚打的字不被冲掉。

use nana_ui::runtime::view::{signal, Signal};

use super::super::hot::ModelField;
use super::super::inspect::{AdvancedField, MetadataInput, SortDirection};
use super::super::inspect_shortcuts::SearchShortcut;
use super::super::{MainRegion, ShellViewModel};
use super::presenter::{self, FilterOptions};

/// 高级区输入框，按 [`AdvancedField`] 的声明顺序排，草稿信号按这个下标取。
pub(crate) const ADVANCED_FIELDS: [AdvancedField; 12] = [
    AdvancedField::ExcludeQuery,
    AdvancedField::ExcludePaths,
    AdvancedField::ExcludeTags,
    AdvancedField::ExcludeFormats,
    AdvancedField::Metadata,
    AdvancedField::ExcludeMetadata,
    AdvancedField::Number,
    AdvancedField::ExcludeNumber,
    AdvancedField::Date,
    AdvancedField::ExcludeDate,
    AdvancedField::SortField,
    AdvancedField::Limit,
];

/// 筛选栏要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FilterBarView {
    /// 有仓库且筛选栏打开。
    pub open: bool,
    pub head: FilterHead,
    /// 四组候选；关着时为空。
    pub options: FilterOptions,
    pub selected: FilterSelection,
    pub min_rating: Option<f64>,
    /// 当前视图里有条目的库类型快捷方式；关着时为空。
    pub shortcuts: Vec<SearchShortcut>,
    /// 输入框的草稿和排序方向。
    pub draft: FilterDrafts,
}

/// 筛选栏各输入框的草稿，对应 ViewModel 的 `search_ui.draft`。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FilterDrafts {
    pub color: String,
    pub shape: String,
    /// 高级区，下标同 [`ADVANCED_FIELDS`]。
    pub advanced: [String; 12],
    pub sort_direction: SortDirection,
}

/// 头部右侧：仓库名、条件数和「清除」能不能点。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FilterHead {
    pub repository: String,
    /// 生效的条件数，0 时不显示「N 个条件」。
    pub count: usize,
    pub clear_disabled: bool,
}

/// 已选的格式、标签、颜色和形状，芯片按它显示选中态。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FilterSelection {
    pub formats: Vec<String>,
    pub tags: Vec<String>,
    pub colors: Vec<String>,
    pub shapes: Vec<String>,
}

impl FilterBarView {
    /// 从 ViewModel 取筛选栏的投影。有仓库且筛选栏打开时显示。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let inspect = &model.inspect;
        let filters = &inspect.filters;
        let open = model.workspace.main_region() == MainRegion::HasRepository && inspect.filter_bar_open;
        let (options, shortcuts) = if open {
            (presenter::filter_options(model), presenter::active_shortcuts(model))
        } else {
            (FilterOptions::default(), Vec::new())
        };
        Self {
            open,
            head: FilterHead {
                repository: model.workspace.active_repository().map(|item| item.name.clone()).unwrap_or_default(),
                count: inspect.active_filter_count(),
                clear_disabled: !filters.has_active_filters() && inspect.query.trim().is_empty(),
            },
            options,
            selected: FilterSelection {
                formats: filters.formats.clone(),
                tags: filters.tags.clone(),
                colors: filters.colors.clone(),
                shapes: filters.shapes.clone(),
            },
            min_rating: filters.min_rating,
            shortcuts,
            draft: FilterDrafts::project(model),
        }
    }
}

impl FilterDrafts {
    fn project(model: &ShellViewModel) -> Self {
        let draft = &model.inspect.search_ui.draft;
        Self {
            color: draft.metadata_input(MetadataInput::Colors).to_string(),
            shape: draft.metadata_input(MetadataInput::Shapes).to_string(),
            advanced: ADVANCED_FIELDS.map(|field| draft.advanced(field).to_string()),
            sort_direction: draft.sort_direction,
        }
    }
}

/// 筛选栏的信号。句柄都是 `Copy` 的 id，值在建它的作用域里。
#[derive(Clone, Copy)]
pub(crate) struct FilterBarSignals {
    pub(crate) open: Signal<bool>,
    pub(crate) head: Signal<FilterHead>,
    pub(crate) formats: Signal<Vec<String>>,
    pub(crate) tags: Signal<Vec<String>>,
    pub(crate) colors: Signal<Vec<String>>,
    pub(crate) shapes: Signal<Vec<String>>,
    pub(crate) selected: Signal<FilterSelection>,
    pub(crate) min_rating: Signal<Option<f64>>,
    pub(crate) shortcuts: Signal<Vec<SearchShortcut>>,
    pub(crate) sort_direction: Signal<SortDirection>,
    pub(crate) color: ModelField,
    pub(crate) shape: ModelField,
    /// 高级区草稿，下标同 [`ADVANCED_FIELDS`]。
    pub(crate) advanced: [ModelField; 12],
}

impl FilterBarSignals {
    /// 空的信号，进路由之前由同步写入第一份投影。
    pub(crate) fn new() -> Self {
        Self::of(FilterBarView {
            open: false,
            head: FilterHead::default(),
            options: FilterOptions::default(),
            selected: FilterSelection::default(),
            min_rating: None,
            shortcuts: Vec::new(),
            draft: FilterDrafts::default(),
        })
    }

    /// 按投影建一份信号。
    fn of(view: FilterBarView) -> Self {
        let draft = &view.draft;
        Self {
            open: signal(view.open),
            head: signal(view.head),
            formats: signal(view.options.formats),
            tags: signal(view.options.tags),
            colors: signal(view.options.colors),
            shapes: signal(view.options.shapes),
            selected: signal(view.selected),
            min_rating: signal(view.min_rating),
            shortcuts: signal(view.shortcuts),
            sort_direction: signal(draft.sort_direction),
            color: ModelField::new(&draft.color),
            shape: ModelField::new(&draft.shape),
            advanced: draft.advanced.each_ref().map(|value| ModelField::new(value)),
        }
    }

    /// 高级区某个输入框的草稿。
    pub(crate) fn advanced(&self, field: AdvancedField) -> ModelField {
        let index = ADVANCED_FIELDS.iter().position(|item| *item == field).unwrap_or_else(|| {
            eprintln!("Nana 筛选栏没有高级字段 {field:?}，改用第一个输入框");
            0
        });
        self.advanced[index]
    }

    /// 写入投影：只写变了的信号，草稿只在 ViewModel 的值相对上次投影变了时写。
    pub(crate) fn write(&self, view: FilterBarView) {
        self.open.try_set_if_changed(view.open);
        self.head.try_set_if_changed(view.head);
        self.formats.try_set_if_changed(view.options.formats);
        self.tags.try_set_if_changed(view.options.tags);
        self.colors.try_set_if_changed(view.options.colors);
        self.shapes.try_set_if_changed(view.options.shapes);
        self.selected.try_set_if_changed(view.selected);
        self.min_rating.try_set_if_changed(view.min_rating);
        self.shortcuts.try_set_if_changed(view.shortcuts);
        self.sort_direction.try_set_if_changed(view.draft.sort_direction);
        self.color.sync(&view.draft.color);
        self.shape.sync(&view.draft.shape);
        for (draft, value) in self.advanced.iter().zip(&view.draft.advanced) {
            draft.sync(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FilterBarView, ADVANCED_FIELDS};
    use crate::shell::acceptance_gap_models;
    use crate::shell::inspect::AdvancedField;

    fn scene(name: &str) -> crate::shell::ShellViewModel {
        acceptance_gap_models().into_iter().find(|(scene, _)| *scene == name).map(|(_, model)| model).expect("场景")
    }

    #[test]
    fn projection_lists_candidates_only_while_open() {
        let mut model = scene("filter-bar-active");
        let view = FilterBarView::project(&model);
        assert!(view.open);
        assert_eq!(view.head.count, 3);
        assert!(!view.head.clear_disabled);
        assert_eq!(view.options.formats, ["pdf", "png"]);
        assert_eq!(view.selected.formats, ["png"]);
        assert_eq!(view.min_rating, Some(3.0));
        assert_eq!(FilterBarView::project(&model), view, "同一状态的投影相等");

        model.inspect.filter_bar_open = false;
        let closed = FilterBarView::project(&model);
        assert!(!closed.open);
        assert!(closed.options.formats.is_empty() && closed.shortcuts.is_empty(), "关着时不算候选");
        assert_eq!(closed.selected, view.selected, "已选条件照常投影");
    }

    #[test]
    fn advanced_fields_follow_the_enum_order() {
        assert_eq!(ADVANCED_FIELDS.len(), 12);
        assert_eq!(ADVANCED_FIELDS[0], AdvancedField::ExcludeQuery);
        assert_eq!(ADVANCED_FIELDS[11], AdvancedField::Limit);
    }
}

//! 搜索请求的组装和筛选输入解析。
//!
//! 对应 Vue `composables/workspace/search.ts` 的 `buildSearchRequest`、`hasSearchCriteria`
//! 和 `filterInputs.ts` 的各个解析函数：逗号、全角逗号和换行都是分隔符，
//! 空键、空值和两端都为空的范围直接丢掉，不向服务发半截条件。

use super::{DateBound, FilterList, NumberBound, SearchFilters, SearchRequestDraft, SortDirection};

/// 按当前查询和筛选组装请求。只有筛选生效时才把搜索收进当前资源库。
pub(crate) fn build_search_request(query: &str, filters: &SearchFilters, active_repo: Option<&str>) -> SearchRequestDraft {
    let mut metadata_filters = Vec::new();
    metadata_filters.extend(filters.colors.iter().cloned().map(|value| ("color".into(), value)));
    metadata_filters.extend(filters.shapes.iter().cloned().map(|value| ("shape".into(), value)));
    metadata_filters.extend(parse_metadata(&filters.metadata_filters));
    let sort_field = filters.sort_field.trim();
    SearchRequestDraft {
        query: query.to_string(),
        repo_id: filters.has_active_filters().then(|| active_repo.map(str::to_string)).flatten(),
        exclude_query: nonempty(&filters.exclude_query),
        tags: normalize_values(&filters.tags),
        formats: normalize_values(&filters.formats),
        metadata_filters,
        exclude_tags: normalize_values(&filters.exclude_tags),
        exclude_formats: normalize_values(&filters.exclude_formats),
        exclude_metadata_filters: parse_metadata(&filters.exclude_metadata_filters),
        exclude_path_prefixes: parse_paths(&filters.exclude_path_prefixes),
        exclude_number_filters: parse_numbers(&filters.exclude_number_filters),
        exclude_date_filters: parse_dates(&filters.exclude_date_filters),
        number_filters: parse_numbers(&filters.number_filters),
        date_filters: parse_dates(&filters.date_filters),
        sort_field: (!sort_field.is_empty()).then(|| sort_field.to_string()),
        sort_direction: (!sort_field.is_empty()).then(|| match filters.sort_direction {
            SortDirection::Asc => "asc".to_string(),
            SortDirection::Desc => "desc".to_string(),
        }),
        limit: filters.limit,
        min_rating: filters.min_rating,
    }
}

/// Vue `toggleFilterValue`：原值不在列表里就加入去空白后的值，否则移除去空白后的值。
/// 去空白后为空时不改列表。
pub(crate) fn toggle_filter(filters: &mut SearchFilters, key: FilterList, value: &str) {
    let normalized = value.trim();
    if normalized.is_empty() {
        return;
    }
    let list = match key {
        FilterList::Tags => &mut filters.tags,
        FilterList::Formats => &mut filters.formats,
        FilterList::Colors => &mut filters.colors,
        FilterList::Shapes => &mut filters.shapes,
        FilterList::ExcludeTags => &mut filters.exclude_tags,
        FilterList::ExcludeFormats => &mut filters.exclude_formats,
    };
    if list.iter().any(|item| item == value) {
        list.retain(|item| item != normalized);
    } else {
        list.push(normalized.to_string());
        *list = normalize_values(list);
    }
}

/// 去空白、去空值、保序去重。
fn normalize_values(values: &[String]) -> Vec<String> {
    let mut unique = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || unique.iter().any(|item| item == value) {
            continue;
        }
        unique.push(value.to_string());
    }
    unique
}

/// Vue `splitListInput`：按逗号、全角逗号和换行拆开，保序去重。
pub(crate) fn split_list(value: &str) -> Vec<String> {
    let mut unique = Vec::new();
    for item in value.split([',', '，', '\n']) {
        let item = item.trim();
        if item.is_empty() || unique.iter().any(|existing| existing == item) {
            continue;
        }
        unique.push(item.to_string());
    }
    unique
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// 结果数量只接受正整数，其余视为未设置。
pub(crate) fn parse_limit(value: &str) -> Option<usize> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    value.parse::<usize>().ok().filter(|limit| *limit > 0)
}

fn parse_metadata(value: &str) -> Vec<(String, String)> {
    value
        .split(['\n', ',', '，'])
        .filter_map(|item| {
            let item = item.trim();
            let index = item.find('=')?;
            let key = item[..index].trim();
            let filter_value = item[index + 1..].trim();
            if key.is_empty() || filter_value.is_empty() {
                None
            } else {
                Some((key.to_string(), filter_value.to_string()))
            }
        })
        .collect()
}

fn parse_numbers(value: &str) -> Vec<NumberBound> {
    value
        .split(['\n', ',', '，'])
        .filter_map(|item| {
            let item = item.trim();
            let (key, range) = item.split_once('=')?;
            let (min_text, max_text) = range.split_once("..").unwrap_or((range, ""));
            let min = bound_number(min_text);
            let max = bound_number(max_text);
            if key.trim().is_empty() || (min.is_none() && max.is_none()) {
                None
            } else {
                Some(NumberBound { key: key.trim().to_string(), min, max })
            }
        })
        .collect()
}

fn bound_number(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    value.parse::<f64>().ok().filter(|number| number.is_finite()).map(|number| number.to_string())
}

fn parse_dates(value: &str) -> Vec<DateBound> {
    value
        .split(['\n', ',', '，'])
        .filter_map(|item| {
            let item = item.trim();
            let (key, range) = item.split_once('=')?;
            let (from, to) = range.split_once("..").unwrap_or((range, ""));
            let from = nonempty(from);
            let to = nonempty(to);
            if key.trim().is_empty() || (from.is_none() && to.is_none()) {
                None
            } else {
                Some(DateBound { key: key.trim().to_string(), from, to })
            }
        })
        .collect()
}

/// 路径前缀统一成正斜杠，去掉两端斜杠。
fn parse_paths(value: &str) -> Vec<String> {
    split_list(value)
        .into_iter()
        .map(|item| item.replace('\\', "/").trim_matches('/').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

impl SearchFilters {
    /// 与 Vue `hasActiveFilters` 相同：排除关键词、排除路径和数值日期排除不单独把搜索收进当前仓库。
    pub fn has_active_filters(&self) -> bool {
        self.active_count() > 0
    }

    /// Vue `activeFilterCount`：列表条件按条数计，文本条件非空计一条。
    /// 排除关键词、排除路径、排除数值和排除日期不计数。
    pub fn active_count(&self) -> usize {
        self.tags.len()
            + self.formats.len()
            + self.colors.len()
            + self.shapes.len()
            + self.exclude_tags.len()
            + self.exclude_formats.len()
            + usize::from(!self.metadata_filters.trim().is_empty())
            + usize::from(!self.exclude_metadata_filters.trim().is_empty())
            + usize::from(!self.number_filters.trim().is_empty())
            + usize::from(!self.date_filters.trim().is_empty())
            + usize::from(!self.sort_field.trim().is_empty())
            + usize::from(self.limit.is_some())
            + usize::from(self.min_rating.is_some())
    }
}

impl SearchRequestDraft {
    /// Vue `hasSearchCriteria`：没有任何条件时不发请求。
    pub fn has_criteria(&self) -> bool {
        !self.query.trim().is_empty()
            || !self.tags.is_empty()
            || !self.formats.is_empty()
            || !self.metadata_filters.is_empty()
            || !self.exclude_tags.is_empty()
            || !self.exclude_formats.is_empty()
            || self.exclude_query.as_ref().is_some_and(|value| !value.trim().is_empty())
            || !self.exclude_path_prefixes.is_empty()
            || !self.exclude_metadata_filters.is_empty()
            || !self.exclude_number_filters.is_empty()
            || !self.exclude_date_filters.is_empty()
            || !self.number_filters.is_empty()
            || !self.date_filters.is_empty()
            || self.sort_field.is_some()
            || self.limit.is_some()
            || self.min_rating.is_some()
    }
}

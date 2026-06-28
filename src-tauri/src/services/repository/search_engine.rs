//! Search, filter, sort, and smart-folder query helpers.

use super::*;

pub(super) fn search_repository_assets(
    connection: &Connection,
    repo: &RepositorySummary,
    query: &str,
    request: &SearchRequest,
) -> Result<Vec<SearchHit>, rusqlite::Error> {
    let assets = load_assets(connection, &repo.repo_id)?;
    let mut results = Vec::new();

    for asset in assets {
        let metadata = load_metadata_map(connection, &asset.asset_id)?;
        if asset.status == "deleted" {
            continue;
        }
        if !search_filter_matches(repo, &asset, &metadata, query, request) {
            continue;
        }

        results.push(SearchHit {
            repo_id: repo.repo_id.clone(),
            repo_name: repo.name.clone(),
            asset_id: asset.asset_id.clone(),
            path: asset.path.clone(),
            filename: asset.filename.clone(),
            status: asset.status.clone(),
            tags: asset.tags.clone(),
            metadata,
            is_virtual: asset.is_virtual,
            provider_id: asset.provider_id.clone(),
            provider_item_id: asset.provider_item_id.clone(),
            source_payload: asset.source_payload.clone(),
            local_absolute_path: asset.local_absolute_path.clone(),
        });
    }

    sort_search_hits(&mut results, request.sort.as_ref());
    if let Some(limit) = request.limit.filter(|value| *value > 0) {
        results.truncate(limit);
    }

    Ok(results)
}

pub(super) fn search_filter_matches(
    repo: &RepositorySummary,
    asset: &AssetSummary,
    metadata: &BTreeMap<String, serde_json::Value>,
    query: &str,
    request: &SearchRequest,
) -> bool {
    let mut include_matches = Vec::new();
    push_query_match(&mut include_matches, repo, asset, metadata, Some(query));
    push_format_match(
        &mut include_matches,
        &asset.extension,
        request.formats.as_ref(),
    );
    push_legacy_tag_match(&mut include_matches, &asset.tags, request.tag.as_deref());
    push_tag_match(&mut include_matches, &asset.tags, request.tags.as_ref());
    push_legacy_metadata_match(
        &mut include_matches,
        metadata,
        request.metadata_key.as_deref(),
        request.metadata_value.as_deref(),
    );
    push_metadata_match(
        &mut include_matches,
        metadata,
        request.metadata_filters.as_ref(),
    );
    push_number_match(
        &mut include_matches,
        metadata,
        request.number_filters.as_ref(),
    );
    push_date_match(
        &mut include_matches,
        metadata,
        request.date_filters.as_ref(),
    );
    push_rating_match(&mut include_matches, metadata, request.min_rating);

    if !combine_include_matches(&include_matches, request.match_mode.as_deref()) {
        return false;
    }

    !matches_excluded_filters(
        repo,
        asset,
        &asset.tags,
        &asset.extension,
        metadata,
        request.exclude_query.as_deref(),
        request.exclude_path_prefixes.as_ref(),
        request.exclude_tags.as_ref(),
        request.exclude_formats.as_ref(),
        request.exclude_metadata_filters.as_ref(),
        request.exclude_number_filters.as_ref(),
        request.exclude_date_filters.as_ref(),
    )
}

pub(super) fn push_query_match(
    include_matches: &mut Vec<bool>,
    repo: &RepositorySummary,
    asset: &AssetSummary,
    metadata: &BTreeMap<String, serde_json::Value>,
    query: Option<&str>,
) {
    let terms = query_terms(query);
    if terms.is_empty() {
        return;
    }
    let haystack = build_search_haystack(repo, asset, metadata);
    include_matches.push(terms.iter().all(|term| haystack.contains(term)));
}

pub(super) fn push_path_prefix_match(
    include_matches: &mut Vec<bool>,
    asset: &AssetSummary,
    path_prefix: Option<&str>,
) {
    let prefixes = query_terms(path_prefix);
    if prefixes.is_empty() {
        return;
    }
    include_matches.push(
        prefixes
            .iter()
            .all(|prefix| asset.path == *prefix || asset.path.starts_with(&format!("{prefix}/"))),
    );
}

pub(super) fn push_format_match(
    include_matches: &mut Vec<bool>,
    extension: &str,
    formats: Option<&Vec<String>>,
) {
    let formats = formats.map_or_else(Vec::new, |values| normalized_filter_values(values));
    if formats.is_empty() {
        return;
    }
    include_matches.push(formats.contains(&extension.to_lowercase()));
}

pub(super) fn push_legacy_tag_match(
    include_matches: &mut Vec<bool>,
    asset_tags: &[String],
    tag: Option<&str>,
) {
    let Some(tag) = tag.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let tag = tag.to_lowercase();
    include_matches.push(
        asset_tags
            .iter()
            .any(|item| item.to_lowercase().contains(&tag)),
    );
}

pub(super) fn push_tag_match(
    include_matches: &mut Vec<bool>,
    asset_tags: &[String],
    tags: Option<&Vec<String>>,
) {
    let tags = tags.map_or_else(Vec::new, |values| normalized_filter_values(values));
    if tags.is_empty() {
        return;
    }
    include_matches.push(tags_match(asset_tags, &tags, Some("and")));
}

pub(super) fn push_legacy_metadata_match(
    include_matches: &mut Vec<bool>,
    metadata: &BTreeMap<String, serde_json::Value>,
    metadata_key: Option<&str>,
    metadata_value: Option<&str>,
) {
    let Some(key) = metadata_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let matched = metadata.get(key).is_some_and(|value| {
        metadata_value
            .map(str::trim)
            .filter(|expected| !expected.is_empty())
            .is_none_or(|expected| {
                json_value_to_search_text(value)
                    .to_lowercase()
                    .contains(&expected.to_lowercase())
            })
    });
    include_matches.push(matched);
}

pub(super) fn push_metadata_match(
    include_matches: &mut Vec<bool>,
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: Option<&Vec<SearchMetadataFilter>>,
) {
    let Some(filters) = filters else {
        return;
    };
    if has_active_metadata_filters(filters) {
        include_matches.push(metadata_filters_match_with_mode(
            metadata,
            filters,
            Some("and"),
        ));
    }
}

pub(super) fn push_number_match(
    include_matches: &mut Vec<bool>,
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: Option<&Vec<SearchNumberFilter>>,
) {
    let Some(filters) = filters else {
        return;
    };
    if has_active_number_filters(filters) {
        include_matches.push(number_filters_match(metadata, filters));
    }
}

pub(super) fn push_date_match(
    include_matches: &mut Vec<bool>,
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: Option<&Vec<SearchDateFilter>>,
) {
    let Some(filters) = filters else {
        return;
    };
    if has_active_date_filters(filters) {
        include_matches.push(date_filters_match(metadata, filters));
    }
}

pub(super) fn push_rating_match(
    include_matches: &mut Vec<bool>,
    metadata: &BTreeMap<String, serde_json::Value>,
    min_rating: Option<f64>,
) {
    let Some(min_rating) = min_rating else {
        return;
    };
    let rating = metadata
        .get("rating")
        .and_then(|value| value.as_f64())
        .unwrap_or_default();
    include_matches.push(rating >= min_rating);
}

pub(super) fn combine_include_matches(matches: &[bool], match_mode: Option<&str>) -> bool {
    if matches.is_empty() {
        true
    } else if is_or_match_mode(match_mode) {
        matches.iter().any(|matched| *matched)
    } else {
        matches.iter().all(|matched| *matched)
    }
}

pub(super) fn matches_excluded_filters(
    repo: &RepositorySummary,
    asset: &AssetSummary,
    asset_tags: &[String],
    extension: &str,
    metadata: &BTreeMap<String, serde_json::Value>,
    exclude_query: Option<&str>,
    exclude_path_prefixes: Option<&Vec<String>>,
    exclude_tags: Option<&Vec<String>>,
    exclude_formats: Option<&Vec<String>>,
    exclude_metadata_filters: Option<&Vec<SearchMetadataFilter>>,
    exclude_number_filters: Option<&Vec<SearchNumberFilter>>,
    exclude_date_filters: Option<&Vec<SearchDateFilter>>,
) -> bool {
    if query_terms(exclude_query)
        .iter()
        .any(|term| build_search_haystack(repo, asset, metadata).contains(term))
    {
        return true;
    }
    if exclude_path_prefixes.is_some_and(|prefixes| {
        prefixes.iter().any(|prefix| {
            normalize_directory_path(prefix).ok().is_some_and(|prefix| {
                !prefix.is_empty()
                    && (asset.path == prefix || asset.path.starts_with(&format!("{prefix}/")))
            })
        })
    }) {
        return true;
    }
    let tags = exclude_tags.map_or_else(Vec::new, |values| normalized_filter_values(values));
    if !tags.is_empty() && tags_match(asset_tags, &tags, Some("or")) {
        return true;
    }

    let formats = exclude_formats.map_or_else(Vec::new, |values| normalized_filter_values(values));
    if !formats.is_empty() && formats.contains(&extension.to_lowercase()) {
        return true;
    }

    exclude_metadata_filters.is_some_and(|filters| {
        has_active_metadata_filters(filters)
            && metadata_filters_match_with_mode(metadata, filters, Some("or"))
    }) || exclude_number_filters.is_some_and(|filters| {
        has_active_number_filters(filters) && number_filters_match(metadata, filters)
    }) || exclude_date_filters.is_some_and(|filters| {
        has_active_date_filters(filters) && date_filters_match(metadata, filters)
    })
}

pub(super) fn normalized_filter_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

pub(super) fn tags_match(
    asset_tags: &[String],
    filters: &[String],
    match_mode: Option<&str>,
) -> bool {
    let matches = |filter: &String| {
        asset_tags.iter().any(|item| {
            let normalized_tag = item.to_lowercase();
            normalized_tag.contains(filter)
        })
    };
    if is_or_match_mode(match_mode) {
        filters.iter().any(matches)
    } else {
        filters.iter().all(matches)
    }
}

pub(super) fn query_terms(value: Option<&str>) -> Vec<String> {
    value
        .map(|value| {
            value
                .lines()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn metadata_filter_groups(
    filters: &[SearchMetadataFilter],
) -> BTreeMap<String, Vec<String>> {
    let mut grouped_filters = BTreeMap::<String, Vec<String>>::new();
    for filter in filters {
        let key = filter.key.trim();
        let value = filter.value.trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        grouped_filters
            .entry(key.to_string())
            .or_default()
            .push(value.to_lowercase());
    }
    grouped_filters
}

pub(super) fn has_active_metadata_filters(filters: &[SearchMetadataFilter]) -> bool {
    filters
        .iter()
        .any(|filter| !filter.key.trim().is_empty() && !filter.value.trim().is_empty())
}

pub(super) fn metadata_filters_match_with_mode(
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: &[SearchMetadataFilter],
    match_mode: Option<&str>,
) -> bool {
    let grouped_filters = metadata_filter_groups(filters);

    let matcher = |(key, expected_values): (String, Vec<String>)| {
        let Some(actual_value) = metadata.get(&key) else {
            return false;
        };
        let actual_text = json_value_to_search_text(actual_value).to_lowercase();
        expected_values
            .iter()
            .any(|expected| actual_text == *expected || actual_text.contains(expected))
    };
    if is_or_match_mode(match_mode) {
        grouped_filters.into_iter().any(matcher)
    } else {
        grouped_filters.into_iter().all(matcher)
    }
}

pub(super) fn number_filters_match(
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: &[SearchNumberFilter],
) -> bool {
    filters.iter().all(|filter| {
        let key = filter.key.trim();
        if key.is_empty() {
            return true;
        }
        let Some(value) = metadata.get(key).and_then(|value| value.as_f64()) else {
            return false;
        };
        if filter.min.is_some_and(|min| value < min) {
            return false;
        }
        if filter.max.is_some_and(|max| value > max) {
            return false;
        }
        true
    })
}

pub(super) fn has_active_number_filters(filters: &[SearchNumberFilter]) -> bool {
    filters.iter().any(|filter| {
        !filter.key.trim().is_empty() && (filter.min.is_some() || filter.max.is_some())
    })
}

pub(super) fn date_filters_match(
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: &[SearchDateFilter],
) -> bool {
    filters.iter().all(|filter| {
        let key = filter.key.trim();
        if key.is_empty() {
            return true;
        }
        let Some(value) = metadata
            .get(key)
            .and_then(|value| value.as_str())
            .and_then(parse_rfc3339_timestamp)
        else {
            return false;
        };
        if filter
            .from
            .as_deref()
            .and_then(parse_rfc3339_timestamp)
            .is_some_and(|from| value < from)
        {
            return false;
        }
        if filter
            .to
            .as_deref()
            .and_then(parse_rfc3339_timestamp)
            .is_some_and(|to| value > to)
        {
            return false;
        }
        true
    })
}

pub(super) fn has_active_date_filters(filters: &[SearchDateFilter]) -> bool {
    filters.iter().any(|filter| {
        !filter.key.trim().is_empty() && (filter.from.is_some() || filter.to.is_some())
    })
}

pub(super) fn parse_rfc3339_timestamp(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

pub(super) fn is_or_match_mode(match_mode: Option<&str>) -> bool {
    matches!(
        match_mode.map(|value| value.trim().to_lowercase()),
        Some(value) if matches!(value.as_str(), "or" | "any" | "some")
    )
}

pub(super) fn sort_search_hits(results: &mut [SearchHit], sort: Option<&SearchSort>) {
    let Some(sort) = sort else {
        return;
    };
    let field = sort.field.trim();
    let normalized_field = field.to_lowercase();
    if normalized_field == "random" {
        sort_by_random_key(
            results,
            |hit| &hit.path,
            sort.direction.trim().eq_ignore_ascii_case("desc"),
        );
        return;
    }
    let descending = sort.direction.trim().eq_ignore_ascii_case("desc");
    results.sort_by(|left, right| {
        let ordering =
            compare_sort_field(
                field,
                &left.metadata,
                &right.metadata,
                || match normalized_field.as_str() {
                    "filename" | "name" => left
                        .filename
                        .to_lowercase()
                        .cmp(&right.filename.to_lowercase()),
                    "path" => left.path.to_lowercase().cmp(&right.path.to_lowercase()),
                    "rating" => metadata_sort_number(&left.metadata, "rating")
                        .partial_cmp(&metadata_sort_number(&right.metadata, "rating"))
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => left.path.to_lowercase().cmp(&right.path.to_lowercase()),
                },
            );
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

pub(super) fn normalize_smart_folder_filter(filter: SmartFolderFilter) -> SmartFolderFilter {
    SmartFolderFilter {
        query: normalize_optional_text(filter.query),
        path_prefix: normalize_optional_path_prefix(filter.path_prefix),
        exclude_query: normalize_optional_text(filter.exclude_query),
        exclude_path_prefixes: normalize_optional_path_values(filter.exclude_path_prefixes),
        tags: normalize_optional_values(filter.tags),
        formats: normalize_optional_values(filter.formats).map(|items| {
            items
                .into_iter()
                .map(|item| item.to_lowercase())
                .collect::<Vec<_>>()
        }),
        colors: normalize_optional_values(filter.colors),
        shapes: normalize_optional_values(filter.shapes),
        metadata_filters: normalize_metadata_filter_values(filter.metadata_filters),
        exclude_tags: normalize_optional_values(filter.exclude_tags),
        exclude_formats: normalize_optional_values(filter.exclude_formats).map(|items| {
            items
                .into_iter()
                .map(|item| item.to_lowercase())
                .collect::<Vec<_>>()
        }),
        exclude_metadata_filters: normalize_metadata_filter_values(filter.exclude_metadata_filters),
        exclude_number_filters: normalize_number_filter_values(filter.exclude_number_filters),
        exclude_date_filters: normalize_date_filter_values(filter.exclude_date_filters),
        number_filters: normalize_number_filter_values(filter.number_filters),
        date_filters: normalize_date_filter_values(filter.date_filters),
        min_rating: filter.min_rating.filter(|value| *value > 0.0),
        match_mode: normalize_match_mode(filter.match_mode),
        sort: normalize_search_sort(filter.sort),
        limit: filter.limit.filter(|value| *value > 0),
    }
}

pub(super) fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

pub(super) fn normalize_optional_values(values: Option<Vec<String>>) -> Option<Vec<String>> {
    let normalized = values
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(super) fn normalize_metadata_filter_values(
    filters: Option<Vec<SearchMetadataFilter>>,
) -> Option<Vec<SearchMetadataFilter>> {
    let normalized = filters
        .unwrap_or_default()
        .into_iter()
        .filter_map(|filter| {
            let key = filter.key.trim();
            let value = filter.value.trim();
            if key.is_empty() || value.is_empty() {
                return None;
            }
            Some(SearchMetadataFilter {
                key: key.to_string(),
                value: value.to_string(),
            })
        })
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(super) fn normalize_number_filter_values(
    filters: Option<Vec<SearchNumberFilter>>,
) -> Option<Vec<SearchNumberFilter>> {
    let normalized = filters
        .unwrap_or_default()
        .into_iter()
        .filter_map(|filter| {
            let key = filter.key.trim().to_string();
            if key.is_empty() || (filter.min.is_none() && filter.max.is_none()) {
                return None;
            }
            Some(SearchNumberFilter {
                key,
                min: filter.min,
                max: filter.max,
            })
        })
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(super) fn normalize_date_filter_values(
    filters: Option<Vec<SearchDateFilter>>,
) -> Option<Vec<SearchDateFilter>> {
    let normalized = filters
        .unwrap_or_default()
        .into_iter()
        .filter_map(|filter| {
            let key = filter.key.trim().to_string();
            let from = normalize_optional_text(filter.from);
            let to = normalize_optional_text(filter.to);
            if key.is_empty() || (from.is_none() && to.is_none()) {
                return None;
            }
            Some(SearchDateFilter { key, from, to })
        })
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(super) fn normalize_match_mode(value: Option<String>) -> Option<String> {
    let value = value?.trim().to_lowercase();
    match value.as_str() {
        "or" | "any" | "some" => Some("or".to_string()),
        "and" | "all" => Some("and".to_string()),
        _ => None,
    }
}

pub(super) fn normalize_search_sort(sort: Option<SearchSort>) -> Option<SearchSort> {
    let sort = sort?;
    let field = sort.field.trim().to_string();
    if field.is_empty() {
        return None;
    }
    let direction = if sort.direction.trim().eq_ignore_ascii_case("desc") {
        "desc"
    } else {
        "asc"
    };
    Some(SearchSort {
        field,
        direction: direction.to_string(),
    })
}

pub(super) fn normalize_optional_path_prefix(value: Option<String>) -> Option<String> {
    value
        .and_then(|path| normalize_directory_path(&path).ok())
        .filter(|path| !path.is_empty())
}

pub(super) fn normalize_optional_path_values(values: Option<Vec<String>>) -> Option<Vec<String>> {
    let normalized = values
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| normalize_directory_path(&path).ok())
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    empty_vec_to_none(normalized)
}

pub(super) fn normalized_optional_id(value: Option<&str>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

pub(super) fn validate_smart_folder_name(name: &str) -> Result<String, String> {
    let value = name.trim();
    if value.is_empty() {
        return Err("smart folder name cannot be empty".to_string());
    }
    if value.contains('/') || value.contains('\\') {
        return Err("smart folder name cannot contain path separators".to_string());
    }
    Ok(value.to_string())
}

pub(super) fn validate_smart_folder_id(id: &str) -> Result<String, String> {
    let value = id.trim();
    if value.is_empty() {
        return Err("smart folder id cannot be empty".to_string());
    }
    Ok(slugify_ascii_component(value))
}

pub(super) fn smart_folder_id_for(repo_id: &str, parent_id: Option<&str>, name: &str) -> String {
    slugify_ascii_component(&format!(
        "smart-{repo_id}-{}-{name}-{}",
        parent_id.unwrap_or("root"),
        now_rfc3339()
    ))
}

pub(super) fn smart_folder_filter_metadata_filters(
    filter: &SmartFolderFilter,
) -> Vec<SearchMetadataFilter> {
    let mut filters = filter.metadata_filters.clone().unwrap_or_default();
    filters.extend(
        filter
            .colors
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|value| SearchMetadataFilter {
                key: "color".to_string(),
                value,
            }),
    );
    filters.extend(
        filter
            .shapes
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|value| SearchMetadataFilter {
                key: "shape".to_string(),
                value,
            }),
    );
    filters
}

pub(super) fn merge_smart_folder_filters(
    parent: SmartFolderFilter,
    child: &SmartFolderFilter,
) -> SmartFolderFilter {
    let mut metadata_filters = parent.metadata_filters.unwrap_or_default();
    metadata_filters.extend(child.metadata_filters.clone().unwrap_or_default());
    let mut exclude_metadata_filters = parent.exclude_metadata_filters.unwrap_or_default();
    exclude_metadata_filters.extend(child.exclude_metadata_filters.clone().unwrap_or_default());
    let mut exclude_number_filters = parent.exclude_number_filters.unwrap_or_default();
    exclude_number_filters.extend(child.exclude_number_filters.clone().unwrap_or_default());
    let mut exclude_date_filters = parent.exclude_date_filters.unwrap_or_default();
    exclude_date_filters.extend(child.exclude_date_filters.clone().unwrap_or_default());
    let mut exclude_path_prefixes = parent.exclude_path_prefixes.unwrap_or_default();
    exclude_path_prefixes.extend(child.exclude_path_prefixes.clone().unwrap_or_default());
    let mut number_filters = parent.number_filters.unwrap_or_default();
    number_filters.extend(child.number_filters.clone().unwrap_or_default());
    let mut date_filters = parent.date_filters.unwrap_or_default();
    date_filters.extend(child.date_filters.clone().unwrap_or_default());
    let mut colors = parent.colors.unwrap_or_default();
    colors.extend(child.colors.clone().unwrap_or_default());
    let mut shapes = parent.shapes.unwrap_or_default();
    shapes.extend(child.shapes.clone().unwrap_or_default());
    SmartFolderFilter {
        query: match (parent.query, child.query.clone()) {
            (Some(left), Some(right)) => Some(format!("{left}\n{right}")),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        },
        path_prefix: merge_path_prefix(parent.path_prefix, child.path_prefix.clone()),
        exclude_query: match (parent.exclude_query, child.exclude_query.clone()) {
            (Some(left), Some(right)) => Some(format!("{left}\n{right}")),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        },
        exclude_path_prefixes: empty_vec_to_none(exclude_path_prefixes),
        tags: merge_optional_lists(parent.tags, child.tags.clone()),
        formats: merge_optional_lists(parent.formats, child.formats.clone()),
        colors: empty_vec_to_none(colors),
        shapes: empty_vec_to_none(shapes),
        metadata_filters: empty_vec_to_none(metadata_filters),
        exclude_tags: merge_optional_lists(parent.exclude_tags, child.exclude_tags.clone()),
        exclude_formats: merge_optional_lists(
            parent.exclude_formats,
            child.exclude_formats.clone(),
        ),
        exclude_metadata_filters: empty_vec_to_none(exclude_metadata_filters),
        exclude_number_filters: empty_vec_to_none(exclude_number_filters),
        exclude_date_filters: empty_vec_to_none(exclude_date_filters),
        number_filters: empty_vec_to_none(number_filters),
        date_filters: empty_vec_to_none(date_filters),
        min_rating: match (parent.min_rating, child.min_rating) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        },
        match_mode: child.match_mode.clone().or(parent.match_mode),
        sort: child.sort.clone().or(parent.sort),
        limit: child.limit.or(parent.limit),
    }
}

pub(super) fn merge_optional_lists(
    parent: Option<Vec<String>>,
    child: Option<Vec<String>>,
) -> Option<Vec<String>> {
    let mut values = parent.unwrap_or_default();
    values.extend(child.unwrap_or_default());
    empty_vec_to_none(values)
}

pub(super) fn empty_vec_to_none<T>(values: Vec<T>) -> Option<Vec<T>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

pub(super) fn merge_path_prefix(parent: Option<String>, child: Option<String>) -> Option<String> {
    match (parent, child) {
        (Some(left), Some(right)) if right.starts_with(&format!("{left}/")) || right == left => {
            Some(right)
        }
        (Some(left), Some(right)) if left.starts_with(&format!("{right}/")) || left == right => {
            Some(left)
        }
        (Some(left), Some(right)) => Some(format!("{left}\n{right}")),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

pub(super) fn load_smart_folder(
    connection: &Connection,
    repo_id: &str,
    smart_folder_id: &str,
) -> Result<Option<SmartFolder>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT smart_folder_id, repo_id, parent_id, name, filter_json,
                   sort_order, created_at, updated_at
            FROM smart_folders
            WHERE repo_id = ?1 AND smart_folder_id = ?2
            "#,
            params![repo_id, smart_folder_id],
            map_smart_folder_row,
        )
        .optional()
}

pub(super) fn load_smart_folders(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<SmartFolder>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT smart_folder_id, repo_id, parent_id, name, filter_json,
               sort_order, created_at, updated_at
        FROM smart_folders
        WHERE repo_id = ?1
        ORDER BY parent_id IS NOT NULL, parent_id, sort_order, name COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map([repo_id], map_smart_folder_row)?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn map_smart_folder_row(
    row: &rusqlite::Row<'_>,
) -> Result<SmartFolder, rusqlite::Error> {
    let filter_json: String = row.get(4)?;
    let filter = serde_json::from_str::<SmartFolderFilter>(&filter_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, Type::Text, Box::new(error))
    })?;
    Ok(SmartFolder {
        smart_folder_id: row.get(0)?,
        repo_id: row.get(1)?,
        parent_id: row.get(2)?,
        name: row.get(3)?,
        filter,
        sort_order: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

pub(super) fn build_smart_folder_tree(folders: Vec<SmartFolder>) -> Vec<SmartFolderTreeNode> {
    fn build(parent_id: Option<&str>, folders: &[SmartFolder]) -> Vec<SmartFolderTreeNode> {
        folders
            .iter()
            .filter(|folder| folder.parent_id.as_deref() == parent_id)
            .map(|folder| SmartFolderTreeNode {
                folder: folder.clone(),
                children: build(Some(&folder.smart_folder_id), folders),
            })
            .collect()
    }
    build(None, &folders)
}

pub(super) fn validate_smart_folder_parent(
    connection: &Connection,
    repo_id: &str,
    parent_id: Option<&str>,
    editing_id: Option<&str>,
) -> Result<(), rusqlite::Error> {
    let Some(parent_id) = normalized_optional_id(parent_id) else {
        return Ok(());
    };
    if editing_id == Some(parent_id.as_str()) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut cursor = Some(parent_id);
    while let Some(current_id) = cursor {
        let parent = load_smart_folder(connection, repo_id, &current_id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
        if editing_id == Some(parent.smart_folder_id.as_str()) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        cursor = parent.parent_id;
    }
    Ok(())
}

pub(super) fn next_smart_folder_sort_order(
    connection: &Connection,
    repo_id: &str,
    parent_id: Option<&str>,
) -> Result<i64, rusqlite::Error> {
    connection.query_row(
        r#"
        SELECT COALESCE(MAX(sort_order), -1) + 1
        FROM smart_folders
        WHERE repo_id = ?1 AND parent_id IS ?2
        "#,
        params![repo_id, normalized_optional_id(parent_id)],
        |row| row.get(0),
    )
}

pub(super) fn inherited_smart_folder_filter(
    folders: &[SmartFolder],
    smart_folder: &SmartFolder,
) -> SmartFolderFilter {
    let mut chain = Vec::<SmartFolder>::new();
    let mut current = Some(smart_folder.clone());
    while let Some(folder) = current {
        current = folder
            .parent_id
            .as_ref()
            .and_then(|parent_id| {
                folders
                    .iter()
                    .find(|item| &item.smart_folder_id == parent_id)
            })
            .cloned();
        chain.push(folder);
    }
    chain.reverse();
    let mut filter = SmartFolderFilter::default();
    for folder in chain {
        filter = merge_smart_folder_filters(filter, &normalize_smart_folder_filter(folder.filter));
    }
    filter
}

pub(super) fn smart_folder_filter_matches(
    repo: &RepositorySummary,
    asset: &AssetSummary,
    metadata: &BTreeMap<String, serde_json::Value>,
    filter: &SmartFolderFilter,
) -> bool {
    if asset.status == "deleted" {
        return false;
    }
    let mut include_matches = Vec::new();
    push_path_prefix_match(&mut include_matches, asset, filter.path_prefix.as_deref());
    push_query_match(
        &mut include_matches,
        repo,
        asset,
        metadata,
        filter.query.as_deref(),
    );
    push_format_match(
        &mut include_matches,
        &asset.extension,
        filter.formats.as_ref(),
    );
    push_tag_match(&mut include_matches, &asset.tags, filter.tags.as_ref());
    let metadata_filters = smart_folder_filter_metadata_filters(filter);
    push_metadata_match(&mut include_matches, metadata, Some(&metadata_filters));
    push_number_match(
        &mut include_matches,
        metadata,
        filter.number_filters.as_ref(),
    );
    push_date_match(&mut include_matches, metadata, filter.date_filters.as_ref());
    push_rating_match(&mut include_matches, metadata, filter.min_rating);

    combine_include_matches(&include_matches, filter.match_mode.as_deref())
        && !matches_excluded_filters(
            repo,
            asset,
            &asset.tags,
            &asset.extension,
            metadata,
            filter.exclude_query.as_deref(),
            filter.exclude_path_prefixes.as_ref(),
            filter.exclude_tags.as_ref(),
            filter.exclude_formats.as_ref(),
            filter.exclude_metadata_filters.as_ref(),
            filter.exclude_number_filters.as_ref(),
            filter.exclude_date_filters.as_ref(),
        )
}

pub(super) fn query_smart_folder_entries(
    connection: &Connection,
    repo: &RepositorySummary,
    filter: &SmartFolderFilter,
    asset_map: &BTreeMap<String, AssetPathRecord>,
) -> Result<Vec<FileBrowserEntry>, rusqlite::Error> {
    let assets = load_assets(connection, &repo.repo_id)?;
    let asset_ids = assets
        .iter()
        .map(|asset| asset.asset_id.clone())
        .collect::<Vec<_>>();
    let alias_paths_by_asset = load_alias_paths_for_assets(connection, &repo.repo_id, &asset_ids)?;
    let mut results = Vec::new();
    for asset in assets {
        let metadata = load_metadata_map(connection, &asset.asset_id)?;
        if !smart_folder_filter_matches(repo, &asset, &metadata, filter) {
            continue;
        }
        let asset_record = asset_map.get(&asset.path);
        results.push(FileBrowserEntry {
            path: asset.path.clone(),
            name: asset.filename.clone(),
            kind: "file".to_string(),
            extension: Some(asset.extension.clone()),
            size_bytes: Some(asset.size_bytes),
            size_label: Some(asset.size_label.clone()),
            modified_at: Some(asset.modified_at.clone()),
            asset_id: Some(asset.asset_id.clone()),
            status: Some(asset.status.clone()),
            thumbnail_path: asset_record
                .and_then(|record| record.thumbnail_path.clone())
                .or(asset.thumbnail_path.clone()),
            thumbnail_custom: false,
            hardlink_group_id: asset_record.and_then(|record| record.hardlink_group_id.clone()),
            hardlink_state: asset_record.and_then(|record| record.hardlink_state.clone()),
            tags: asset.tags.clone(),
            alias_paths: alias_paths_by_asset
                .get(&asset.asset_id)
                .cloned()
                .unwrap_or_default(),
            folder_metadata: None,
            metadata,
            is_virtual: asset.is_virtual,
            provider_id: asset.provider_id.clone(),
            provider_item_id: asset.provider_item_id.clone(),
            source_payload: asset.source_payload.clone(),
            local_absolute_path: asset.local_absolute_path.clone(),
        });
    }
    sort_file_browser_entries(&mut results, filter.sort.as_ref());
    if let Some(limit) = filter.limit.filter(|value| *value > 0) {
        results.truncate(limit);
    }
    Ok(results)
}

pub(super) fn build_search_haystack(
    repo: &RepositorySummary,
    asset: &AssetSummary,
    metadata: &BTreeMap<String, serde_json::Value>,
) -> String {
    let metadata_values = metadata
        .iter()
        .map(|(key, value)| format!("{key} {}", json_value_to_search_text(value)))
        .collect::<Vec<_>>()
        .join(" ");

    [
        repo.name.as_str(),
        asset.filename.as_str(),
        asset.path.as_str(),
        asset.status.as_str(),
        &asset.tags.join(" "),
        metadata_values.as_str(),
    ]
    .join(" ")
    .to_lowercase()
}

pub(super) fn sort_file_browser_entries(
    entries: &mut [FileBrowserEntry],
    sort: Option<&SearchSort>,
) {
    let Some(sort) = sort else {
        entries.sort_by(|left, right| left.path.to_lowercase().cmp(&right.path.to_lowercase()));
        return;
    };
    let field = sort.field.trim();
    let normalized_field = field.to_lowercase();
    if normalized_field == "random" {
        sort_by_random_key(
            entries,
            |entry| &entry.path,
            sort.direction.trim().eq_ignore_ascii_case("desc"),
        );
        return;
    }
    let descending = sort.direction.trim().eq_ignore_ascii_case("desc");
    entries.sort_by(|left, right| {
        let ordering =
            compare_sort_field(
                field,
                &left.metadata,
                &right.metadata,
                || match normalized_field.as_str() {
                    "filename" | "name" => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
                    "size" | "sizebytes" => left.size_bytes.cmp(&right.size_bytes),
                    "modified" | "modifiedat" => left.modified_at.cmp(&right.modified_at),
                    "rating" => metadata_sort_number(&left.metadata, "rating")
                        .partial_cmp(&metadata_sort_number(&right.metadata, "rating"))
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => left.path.to_lowercase().cmp(&right.path.to_lowercase()),
                },
            );
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

pub(super) fn sort_by_random_key<T>(items: &mut [T], key: impl Fn(&T) -> &str, descending: bool) {
    use std::collections::hash_map::DefaultHasher;

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    items.sort_by(|left, right| {
        let mut left_hasher = DefaultHasher::new();
        seed.hash(&mut left_hasher);
        key(left).hash(&mut left_hasher);
        let mut right_hasher = DefaultHasher::new();
        seed.hash(&mut right_hasher);
        key(right).hash(&mut right_hasher);
        let ordering = left_hasher.finish().cmp(&right_hasher.finish());
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

pub(super) fn metadata_sort_number(
    metadata: &BTreeMap<String, serde_json::Value>,
    key: &str,
) -> f64 {
    metadata
        .get(key)
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0)
}

pub(super) fn metadata_sort_field_key(field: &str) -> Option<&str> {
    const PREFIX: &str = "metadata.";
    if field.len() > PREFIX.len() && field[..PREFIX.len()].eq_ignore_ascii_case(PREFIX) {
        Some(&field[PREFIX.len()..])
    } else {
        None
    }
}

pub(super) fn compare_sort_field(
    field: &str,
    left_metadata: &BTreeMap<String, serde_json::Value>,
    right_metadata: &BTreeMap<String, serde_json::Value>,
    fallback: impl FnOnce() -> std::cmp::Ordering,
) -> std::cmp::Ordering {
    if let Some(metadata_key) = metadata_sort_field_key(field) {
        compare_metadata_values(left_metadata, right_metadata, metadata_key)
    } else {
        fallback()
    }
}

pub(super) fn compare_metadata_values(
    left: &BTreeMap<String, serde_json::Value>,
    right: &BTreeMap<String, serde_json::Value>,
    key: &str,
) -> std::cmp::Ordering {
    compare_optional_json_values(left.get(key), right.get(key))
}

pub(super) fn compare_optional_json_values(
    left: Option<&serde_json::Value>,
    right: Option<&serde_json::Value>,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => compare_json_values(left, right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

pub(super) fn compare_json_values(
    left: &serde_json::Value,
    right: &serde_json::Value,
) -> std::cmp::Ordering {
    if let (Some(left), Some(right)) = (json_value_to_f64(left), json_value_to_f64(right)) {
        return left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal);
    }
    if let (Some(left), Some(right)) = (
        json_value_to_timestamp(left),
        json_value_to_timestamp(right),
    ) {
        return left.cmp(&right);
    }
    json_value_to_search_text(left)
        .to_lowercase()
        .cmp(&json_value_to_search_text(right).to_lowercase())
}

pub(super) fn json_value_to_f64(value: &serde_json::Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        value
            .as_str()
            .map(str::trim)
            .and_then(|value| value.parse::<f64>().ok())
    })
}

pub(super) fn json_value_to_timestamp(value: &serde_json::Value) -> Option<OffsetDateTime> {
    value
        .as_str()
        .map(str::trim)
        .and_then(parse_rfc3339_timestamp)
}

pub(super) fn json_value_to_search_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

pub(super) fn infer_value_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::String(_) => "string",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::Bool(_) => "boolean",
        _ => "json",
    }
}

pub(super) fn parse_json_column(value_json: &str) -> Result<serde_json::Value, rusqlite::Error> {
    serde_json::from_str(value_json)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))
}

pub(super) fn parse_json_column_optional(
    value_json: Option<String>,
) -> Result<serde_json::Value, rusqlite::Error> {
    match value_json {
        Some(value) => parse_json_column(&value),
        None => Ok(serde_json::json!({})),
    }
}

pub(super) fn parse_json_column_nullable(
    value_json: Option<String>,
) -> Result<Option<serde_json::Value>, rusqlite::Error> {
    match value_json {
        Some(value) => Ok(Some(parse_json_column(&value)?)),
        None => Ok(None),
    }
}

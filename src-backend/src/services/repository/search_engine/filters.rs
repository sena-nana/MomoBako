//! 资源搜索入口、筛选条件匹配与搜索结果排序。

use super::*;

pub(in super::super) fn search_repository_assets(
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

pub(in super::super) fn search_filter_matches(
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

pub(in super::super) fn push_query_match(
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

pub(in super::super) fn push_path_prefix_match(
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

pub(in super::super) fn push_format_match(
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

pub(in super::super) fn push_legacy_tag_match(
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

pub(in super::super) fn push_tag_match(
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

pub(in super::super) fn push_legacy_metadata_match(
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

pub(in super::super) fn push_metadata_match(
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

pub(in super::super) fn push_number_match(
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

pub(in super::super) fn push_date_match(
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

pub(in super::super) fn push_rating_match(
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

pub(in super::super) fn combine_include_matches(
    matches: &[bool],
    match_mode: Option<&str>,
) -> bool {
    if matches.is_empty() {
        true
    } else if is_or_match_mode(match_mode) {
        matches.iter().any(|matched| *matched)
    } else {
        matches.iter().all(|matched| *matched)
    }
}

pub(in super::super) fn matches_excluded_filters(
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

pub(in super::super) fn normalized_filter_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

pub(in super::super) fn tags_match(
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

pub(in super::super) fn query_terms(value: Option<&str>) -> Vec<String> {
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

pub(in super::super) fn metadata_filter_groups(
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

pub(in super::super) fn has_active_metadata_filters(filters: &[SearchMetadataFilter]) -> bool {
    filters
        .iter()
        .any(|filter| !filter.key.trim().is_empty() && !filter.value.trim().is_empty())
}

pub(in super::super) fn metadata_filters_match_with_mode(
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

pub(in super::super) fn number_filters_match(
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

pub(in super::super) fn has_active_number_filters(filters: &[SearchNumberFilter]) -> bool {
    filters.iter().any(|filter| {
        !filter.key.trim().is_empty() && (filter.min.is_some() || filter.max.is_some())
    })
}

pub(in super::super) fn date_filters_match(
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

pub(in super::super) fn has_active_date_filters(filters: &[SearchDateFilter]) -> bool {
    filters.iter().any(|filter| {
        !filter.key.trim().is_empty() && (filter.from.is_some() || filter.to.is_some())
    })
}

pub(in super::super) fn parse_rfc3339_timestamp(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

pub(in super::super) fn is_or_match_mode(match_mode: Option<&str>) -> bool {
    matches!(
        match_mode.map(|value| value.trim().to_lowercase()),
        Some(value) if matches!(value.as_str(), "or" | "any" | "some")
    )
}

pub(in super::super) fn sort_search_hits(results: &mut [SearchHit], sort: Option<&SearchSort>) {
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

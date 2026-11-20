//! 筛选条件规范化与智能文件夹的读取、合并、继承和查询。

use super::*;

pub(in super::super) fn normalize_smart_folder_filter(
    filter: SmartFolderFilter,
) -> SmartFolderFilter {
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

pub(in super::super) fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

pub(in super::super) fn normalize_optional_values(
    values: Option<Vec<String>>,
) -> Option<Vec<String>> {
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

pub(in super::super) fn normalize_metadata_filter_values(
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

pub(in super::super) fn normalize_number_filter_values(
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

pub(in super::super) fn normalize_date_filter_values(
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

pub(in super::super) fn normalize_match_mode(value: Option<String>) -> Option<String> {
    let value = value?.trim().to_lowercase();
    match value.as_str() {
        "or" | "any" | "some" => Some("or".to_string()),
        "and" | "all" => Some("and".to_string()),
        _ => None,
    }
}

pub(in super::super) fn normalize_search_sort(sort: Option<SearchSort>) -> Option<SearchSort> {
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

pub(in super::super) fn normalize_optional_path_prefix(value: Option<String>) -> Option<String> {
    value
        .and_then(|path| normalize_directory_path(&path).ok())
        .filter(|path| !path.is_empty())
}

pub(in super::super) fn normalize_optional_path_values(
    values: Option<Vec<String>>,
) -> Option<Vec<String>> {
    let normalized = values
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| normalize_directory_path(&path).ok())
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    empty_vec_to_none(normalized)
}

pub(in super::super) fn normalized_optional_id(value: Option<&str>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

pub(in super::super) fn validate_smart_folder_name(name: &str) -> Result<String, String> {
    let value = name.trim();
    if value.is_empty() {
        return Err("smart folder name cannot be empty".to_string());
    }
    if value.contains('/') || value.contains('\\') {
        return Err("smart folder name cannot contain path separators".to_string());
    }
    Ok(value.to_string())
}

pub(in super::super) fn validate_smart_folder_id(id: &str) -> Result<String, String> {
    let value = id.trim();
    if value.is_empty() {
        return Err("smart folder id cannot be empty".to_string());
    }
    Ok(slugify_ascii_component(value))
}

pub(in super::super) fn smart_folder_id_for(
    repo_id: &str,
    parent_id: Option<&str>,
    name: &str,
) -> String {
    slugify_ascii_component(&format!(
        "smart-{repo_id}-{}-{name}-{}",
        parent_id.unwrap_or("root"),
        now_rfc3339()
    ))
}

pub(in super::super) fn smart_folder_filter_metadata_filters(
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

pub(in super::super) fn merge_smart_folder_filters(
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

pub(in super::super) fn merge_optional_lists(
    parent: Option<Vec<String>>,
    child: Option<Vec<String>>,
) -> Option<Vec<String>> {
    let mut values = parent.unwrap_or_default();
    values.extend(child.unwrap_or_default());
    empty_vec_to_none(values)
}

pub(in super::super) fn empty_vec_to_none<T>(values: Vec<T>) -> Option<Vec<T>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

pub(in super::super) fn merge_path_prefix(
    parent: Option<String>,
    child: Option<String>,
) -> Option<String> {
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

pub(in super::super) fn load_smart_folder(
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

pub(in super::super) fn load_smart_folders(
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

pub(in super::super) fn map_smart_folder_row(
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

pub(in super::super) fn build_smart_folder_tree(
    folders: Vec<SmartFolder>,
) -> Vec<SmartFolderTreeNode> {
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

pub(in super::super) fn validate_smart_folder_parent(
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

pub(in super::super) fn next_smart_folder_sort_order(
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

pub(in super::super) fn inherited_smart_folder_filter(
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

pub(in super::super) fn smart_folder_filter_matches(
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

pub(in super::super) fn query_smart_folder_entries(
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

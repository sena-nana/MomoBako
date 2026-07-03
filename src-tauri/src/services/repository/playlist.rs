//! Playlist workflows for repository-scoped playback collections.

use super::*;

pub(super) fn list_playlists(
    state: &RepositoryState,
    repo_id: &str,
) -> Result<Vec<PlaylistSummary>, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    load_playlists(&connection, repo_id).map_err(db_error)
}

pub(super) fn list_playlist_memberships(
    state: &RepositoryState,
    repo_id: &str,
) -> Result<PlaylistMembershipIndex, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    Ok(PlaylistMembershipIndex {
        memberships: load_playlist_memberships(&connection, repo_id).map_err(db_error)?,
    })
}

pub(super) fn create_playlist(
    state: &RepositoryState,
    request: PlaylistMutationRequest,
) -> Result<PlaylistMutationResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let registry = backend_plugin_registry(&state.root);
    let player = registry
        .playlist_player(&request.player_type_id)
        .ok_or_else(|| format!("playlist player not found: {}", request.player_type_id))?;
    let playlist_id = request
        .playlist_id
        .as_deref()
        .map(validate_playlist_id)
        .transpose()?
        .unwrap_or_else(|| playlist_id_for(&request.repo_id, &request.name));
    let name = validate_playlist_name(&request.name)?;
    let sort_order = next_playlist_sort_order(&connection, &request.repo_id).map_err(db_error)?;
    let now = now_rfc3339();
    connection
        .execute(
            r#"
        INSERT INTO playlists (
          playlist_id, repo_id, name, player_type_id, player_plugin_id,
          player_label, file_class, sort_order, created_at, updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
        "#,
            params![
                playlist_id,
                request.repo_id,
                name,
                player.player_type_id,
                player.plugin_id,
                player.label,
                player.file_class,
                sort_order,
                now,
            ],
        )
        .map_err(db_error)?;
    let playlists = load_playlists(&connection, &request.repo_id).map_err(db_error)?;
    let playlist = playlists
        .iter()
        .find(|item| item.playlist_id == playlist_id)
        .cloned();
    Ok(PlaylistMutationResponse {
        playlists,
        playlist,
    })
}

pub(super) fn update_playlist(
    state: &RepositoryState,
    request: PlaylistUpdateRequest,
) -> Result<PlaylistMutationResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let existing = load_playlist_summary(&connection, &request.repo_id, &request.playlist_id)
        .map_err(db_error)?
        .ok_or_else(|| format!("playlist not found: {}", request.playlist_id))?;
    let registry = backend_plugin_registry(&state.root);
    let player = if let Some(player_type_id) = request.player_type_id.as_deref() {
        registry
            .playlist_player(player_type_id)
            .ok_or_else(|| format!("playlist player not found: {player_type_id}"))?
    } else {
        registry
            .playlist_player(&existing.player_type_id)
            .unwrap_or(PlaylistPlayerRegistration {
                plugin_id: existing.player_plugin_id.clone(),
                player_type_id: existing.player_type_id.clone(),
                label: existing.player_label.clone(),
                file_class: existing.file_class.clone(),
                supported_extensions: Vec::new(),
                supports_seek: false,
                supports_volume: false,
                supports_preview_navigation: false,
                description: None,
            })
    };
    let name = request
        .name
        .as_deref()
        .map(validate_playlist_name)
        .transpose()?
        .unwrap_or(existing.name.clone());
    let now = now_rfc3339();
    connection
        .execute(
            r#"
        UPDATE playlists
        SET
          name = ?3,
          player_type_id = ?4,
          player_plugin_id = ?5,
          player_label = ?6,
          file_class = ?7,
          updated_at = ?8
        WHERE repo_id = ?1 AND playlist_id = ?2
        "#,
            params![
                request.repo_id,
                request.playlist_id,
                name,
                player.player_type_id,
                player.plugin_id,
                player.label,
                player.file_class,
                now,
            ],
        )
        .map_err(db_error)?;
    let playlists = load_playlists(&connection, &request.repo_id).map_err(db_error)?;
    let playlist = playlists
        .iter()
        .find(|item| item.playlist_id == request.playlist_id)
        .cloned();
    Ok(PlaylistMutationResponse {
        playlists,
        playlist,
    })
}

pub(super) fn delete_playlist(
    state: &RepositoryState,
    repo_id: &str,
    playlist_id: &str,
) -> Result<PlaylistMutationResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    connection
        .execute(
            "DELETE FROM playlists WHERE repo_id = ?1 AND playlist_id = ?2",
            params![repo_id, validate_playlist_id(playlist_id)?],
        )
        .map_err(db_error)?;
    let playlists = load_playlists(&connection, repo_id).map_err(db_error)?;
    Ok(PlaylistMutationResponse {
        playlists,
        playlist: None,
    })
}

pub(super) fn get_playlist_detail(
    state: &RepositoryState,
    repo_id: &str,
    playlist_id: &str,
) -> Result<PlaylistDetail, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    load_playlist_detail(
        &connection,
        &repo,
        &backend_plugin_registry(&state.root),
        repo_id,
        playlist_id,
    )
    .map_err(db_error)
}

pub(super) fn add_playlist_items(
    state: &RepositoryState,
    request: PlaylistItemsAddRequest,
) -> Result<PlaylistDetail, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let playlist = load_playlist_summary(&connection, &request.repo_id, &request.playlist_id)
        .map_err(db_error)?
        .ok_or_else(|| format!("playlist not found: {}", request.playlist_id))?;
    let registry = backend_plugin_registry(&state.root);
    let player = registry.playlist_player(&playlist.player_type_id);
    let mut sort_order =
        next_playlist_item_sort_order(&connection, &request.repo_id, &request.playlist_id)
            .map_err(db_error)?;
    let tx = connection.transaction().map_err(db_error)?;
    for asset_id in normalize_id_list(&request.asset_ids) {
        let asset = load_asset_summary_from_transaction(&tx, &request.repo_id, &asset_id)
            .map_err(db_error)?
            .ok_or_else(|| format!("asset not found: {asset_id}"))?;
        if let Some(player) = &player {
            if !playlist_player_supports_extension(player, &asset.extension) {
                continue;
            }
        }
        tx.execute(
            r#"
            INSERT OR IGNORE INTO playlist_items (
              playlist_item_id, repo_id, playlist_id, asset_id, sort_order, added_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                playlist_item_id_for(&request.playlist_id, &asset.asset_id),
                request.repo_id,
                request.playlist_id,
                asset.asset_id,
                sort_order,
                now_rfc3339(),
            ],
        )
        .map_err(db_error)?;
        sort_order += 1;
    }
    tx.commit().map_err(db_error)?;
    load_playlist_detail(
        &connection,
        &repo,
        &registry,
        &request.repo_id,
        &request.playlist_id,
    )
    .map_err(db_error)
}

pub(super) fn add_playlist_items_by_paths(
    state: &RepositoryState,
    request: PlaylistItemsByPathsAddRequest,
) -> Result<PlaylistDetail, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;

    let normalized_paths = request
        .paths
        .iter()
        .map(|path| normalize_entry_path(path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut asset_ids = Vec::<String>::new();

    for path in normalized_paths {
        let direct = connection
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2 AND status != 'deleted'",
                params![request.repo_id, path],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?;
        if let Some(asset_id) = direct {
            asset_ids.push(asset_id);
            continue;
        }

        let prefix = format!("{path}/%");
        let mut stmt = connection
            .prepare(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND status != 'deleted' AND path LIKE ?2 ORDER BY path COLLATE NOCASE",
            )
            .map_err(db_error)?;
        let rows = stmt
            .query_map(params![request.repo_id, prefix], |row| {
                row.get::<_, String>(0)
            })
            .map_err(db_error)?;
        for row in rows {
            asset_ids.push(row.map_err(db_error)?);
        }
    }

    asset_ids.sort();
    asset_ids.dedup();
    add_playlist_items(
        state,
        PlaylistItemsAddRequest {
            repo_id: request.repo_id,
            playlist_id: request.playlist_id,
            asset_ids,
        },
    )
}

pub(super) fn reorder_playlist_items(
    state: &RepositoryState,
    request: PlaylistItemsOrderRequest,
) -> Result<PlaylistDetail, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let tx = connection.transaction().map_err(db_error)?;
    for (index, item_id) in request.item_ids.iter().enumerate() {
        tx.execute(
            r#"
            UPDATE playlist_items
            SET sort_order = ?4
            WHERE repo_id = ?1 AND playlist_id = ?2 AND playlist_item_id = ?3
            "#,
            params![request.repo_id, request.playlist_id, item_id, index as i64],
        )
        .map_err(db_error)?;
    }
    tx.execute(
        r#"
        UPDATE playlists
        SET updated_at = ?3
        WHERE repo_id = ?1 AND playlist_id = ?2
        "#,
        params![request.repo_id, request.playlist_id, now_rfc3339()],
    )
    .map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    load_playlist_detail(
        &connection,
        &repo,
        &backend_plugin_registry(&state.root),
        &request.repo_id,
        &request.playlist_id,
    )
    .map_err(db_error)
}

pub(super) fn remove_playlist_item(
    state: &RepositoryState,
    request: PlaylistItemRemoveRequest,
) -> Result<PlaylistDetail, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    connection.execute(
        "DELETE FROM playlist_items WHERE repo_id = ?1 AND playlist_id = ?2 AND playlist_item_id = ?3",
        params![request.repo_id, request.playlist_id, request.playlist_item_id],
    ).map_err(db_error)?;
    load_playlist_detail(
        &connection,
        &repo,
        &backend_plugin_registry(&state.root),
        &request.repo_id,
        &request.playlist_id,
    )
    .map_err(db_error)
}

pub(super) fn set_playlist_membership(
    state: &RepositoryState,
    request: PlaylistMembershipRequest,
) -> Result<PlaylistMembershipSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let asset = load_asset_summary(&connection, &request.repo_id, &request.asset_id)
        .map_err(db_error)?
        .ok_or_else(|| format!("asset not found: {}", request.asset_id))?;
    let playlists = load_playlists(&connection, &request.repo_id).map_err(db_error)?;
    let registry = backend_plugin_registry(&state.root);
    let valid_target_ids = normalize_id_list(&request.playlist_ids);
    let tx = connection.transaction().map_err(db_error)?;
    let mut kept = Vec::new();
    for playlist in playlists {
        let Some(player) = registry.playlist_player(&playlist.player_type_id) else {
            continue;
        };
        if !playlist_player_supports_extension(&player, &asset.extension) {
            continue;
        }
        if valid_target_ids
            .iter()
            .any(|item| item == &playlist.playlist_id)
        {
            let sort_order =
                next_playlist_item_sort_order(&tx, &request.repo_id, &playlist.playlist_id)
                    .map_err(db_error)?;
            tx.execute(
                r#"
                INSERT OR IGNORE INTO playlist_items (
                  playlist_item_id, repo_id, playlist_id, asset_id, sort_order, added_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    playlist_item_id_for(&playlist.playlist_id, &asset.asset_id),
                    request.repo_id,
                    playlist.playlist_id,
                    asset.asset_id,
                    sort_order,
                    now_rfc3339(),
                ],
            )
            .map_err(db_error)?;
            kept.push(playlist.playlist_id);
        } else {
            tx.execute(
                "DELETE FROM playlist_items WHERE repo_id = ?1 AND playlist_id = ?2 AND asset_id = ?3",
                params![request.repo_id, playlist.playlist_id, request.asset_id],
            )
            .map_err(db_error)?;
        }
    }
    tx.commit().map_err(db_error)?;
    kept.sort();
    Ok(PlaylistMembershipSnapshot {
        asset_id: request.asset_id,
        playlist_ids: kept,
    })
}

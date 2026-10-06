//! Hardlink candidate bookkeeping for repository synchronization.

use super::*;

pub(super) fn hardlink_group_id_for(repo_id: &str, content_hash: &str, size_bytes: i64) -> String {
    format!(
        "hardlink-{}",
        sha256_hex(&[
            repo_id.as_bytes(),
            content_hash.as_bytes(),
            size_bytes.to_string().as_bytes()
        ])
    )
}

pub(super) fn hardlink_candidate_id_for(
    repo_id: &str,
    new_asset_id: &str,
    existing_asset_id: &str,
) -> String {
    format!(
        "hardlink-candidate-{}",
        sha256_hex(&[
            repo_id.as_bytes(),
            new_asset_id.as_bytes(),
            existing_asset_id.as_bytes()
        ])
    )
}

pub(super) fn ensure_hardlink_group(
    tx: &Transaction<'_>,
    repo_id: &str,
    content_hash: &str,
    size_bytes: i64,
) -> Result<String, rusqlite::Error> {
    let group_id = hardlink_group_id_for(repo_id, content_hash, size_bytes);
    let now = now_rfc3339();
    tx.execute(
        r#"
        INSERT INTO hardlink_groups (group_id, repo_id, content_hash, size_bytes, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?5)
        ON CONFLICT(repo_id, content_hash, size_bytes)
        DO UPDATE SET updated_at = excluded.updated_at
        "#,
        params![group_id, repo_id, content_hash, size_bytes, now],
    )?;
    tx.query_row(
        r#"
        SELECT group_id
        FROM hardlink_groups
        WHERE repo_id = ?1 AND content_hash = ?2 AND size_bytes = ?3
        "#,
        params![repo_id, content_hash, size_bytes],
        |row| row.get(0),
    )
}

pub(super) fn upsert_hardlink_member(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
    path: &str,
    content_hash: &str,
    size_bytes: i64,
    link_state: &str,
) -> Result<(), rusqlite::Error> {
    let group_id = ensure_hardlink_group(tx, repo_id, content_hash, size_bytes)?;
    let now = now_rfc3339();
    tx.execute(
        r#"
        INSERT INTO hardlink_members (group_id, repo_id, asset_id, path, link_state, linked_at, verified_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
        ON CONFLICT(repo_id, asset_id)
        DO UPDATE SET
          group_id = excluded.group_id,
          path = excluded.path,
          link_state = excluded.link_state,
          verified_at = excluded.verified_at
        "#,
        params![group_id, repo_id, asset_id, path, link_state, now],
    )?;
    Ok(())
}

pub(super) fn update_hardlink_member_verification(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
    path: &str,
    content_hash: &str,
) -> Result<(), rusqlite::Error> {
    let Some((group_id, expected_hash, current_state)) = tx
        .query_row(
            r#"
            SELECT hm.group_id, hg.content_hash, hm.link_state
            FROM hardlink_members hm
            JOIN hardlink_groups hg ON hg.group_id = hm.group_id
            WHERE hm.repo_id = ?1 AND hm.asset_id = ?2
            "#,
            params![repo_id, asset_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(());
    };
    let state = if expected_hash != content_hash {
        "broken"
    } else if current_state == "copiedFallback" {
        "copiedFallback"
    } else {
        "linked"
    };
    tx.execute(
        r#"
        UPDATE hardlink_members
        SET path = ?4, link_state = ?5, verified_at = ?6
        WHERE repo_id = ?1 AND asset_id = ?2 AND group_id = ?3
        "#,
        params![repo_id, asset_id, group_id, path, state, now_rfc3339()],
    )?;
    Ok(())
}

pub(super) fn mark_hardlink_member_missing(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
) -> Result<(), rusqlite::Error> {
    tx.execute(
        r#"
        UPDATE hardlink_members
        SET link_state = 'missing', verified_at = ?3
        WHERE repo_id = ?1 AND asset_id = ?2
        "#,
        params![repo_id, asset_id, now_rfc3339()],
    )?;
    Ok(())
}

pub(super) fn record_hardlink_candidate_for_new_asset(
    tx: &Transaction<'_>,
    repo_id: &str,
    new_asset_id: &str,
    new_path: &str,
    content_hash: &str,
    size_bytes: i64,
) -> Result<(), rusqlite::Error> {
    let existing = tx
        .query_row(
            r#"
            SELECT asset_id, path
            FROM assets
            WHERE repo_id = ?1
              AND asset_id != ?2
              AND hash = ?3
              AND size_bytes = ?4
              AND status != 'deleted'
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
            params![repo_id, new_asset_id, content_hash, size_bytes],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((existing_asset_id, existing_path)) = existing else {
        return Ok(());
    };
    let candidate_id = hardlink_candidate_id_for(repo_id, new_asset_id, &existing_asset_id);
    tx.execute(
        r#"
        INSERT OR IGNORE INTO hardlink_candidates (
          candidate_id, repo_id, new_asset_id, new_path, existing_asset_id, existing_path,
          content_hash, size_bytes, created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            candidate_id,
            repo_id,
            new_asset_id,
            new_path,
            existing_asset_id,
            existing_path,
            content_hash,
            size_bytes,
            now_rfc3339()
        ],
    )?;
    Ok(())
}

pub(super) fn count_pending_hardlink_candidates(
    tx: &Transaction<'_>,
    repo_id: &str,
) -> Result<i64, rusqlite::Error> {
    tx.query_row(
        r#"
        SELECT COUNT(*)
        FROM hardlink_candidates hc
        JOIN assets new_asset
          ON new_asset.repo_id = hc.repo_id
         AND new_asset.asset_id = hc.new_asset_id
         AND new_asset.path = hc.new_path
         AND new_asset.hash = hc.content_hash
         AND new_asset.size_bytes = hc.size_bytes
         AND new_asset.status != 'deleted'
        JOIN assets existing_asset
          ON existing_asset.repo_id = hc.repo_id
         AND existing_asset.asset_id = hc.existing_asset_id
         AND existing_asset.path = hc.existing_path
         AND existing_asset.hash = hc.content_hash
         AND existing_asset.size_bytes = hc.size_bytes
         AND existing_asset.status != 'deleted'
        WHERE hc.repo_id = ?1
        "#,
        [repo_id],
        |row| row.get(0),
    )
}

pub(super) fn load_hardlink_asset_for_path(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<Option<HardlinkAssetRecord>, rusqlite::Error> {
    let record = tx
        .query_row(
            r#"
            SELECT asset_id, hash, size_bytes
            FROM assets
            WHERE repo_id = ?1 AND path = ?2 AND status != 'deleted'
            "#,
            params![repo_id, path],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(record.and_then(|(asset_id, hash, size_bytes)| {
        hash.filter(|value| is_content_hash(value))
            .map(|content_hash| HardlinkAssetRecord {
                asset_id,
                content_hash,
                size_bytes,
            })
    }))
}

pub(super) fn hardlink_outcome_target_paths(outcomes: &[HardlinkCopyOutcome]) -> HashSet<String> {
    outcomes
        .iter()
        .map(|outcome| outcome.target_path.clone())
        .collect()
}

pub(super) fn load_hardlink_candidates(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<HardlinkCandidate>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT hc.candidate_id, hc.repo_id, hc.new_asset_id, hc.new_path,
               hc.existing_asset_id, hc.existing_path, hc.content_hash,
               hc.size_bytes, hc.created_at
        FROM hardlink_candidates hc
        JOIN assets new_asset
          ON new_asset.repo_id = hc.repo_id
         AND new_asset.asset_id = hc.new_asset_id
         AND new_asset.path = hc.new_path
         AND new_asset.hash = hc.content_hash
         AND new_asset.size_bytes = hc.size_bytes
         AND new_asset.status != 'deleted'
        JOIN assets existing_asset
          ON existing_asset.repo_id = hc.repo_id
         AND existing_asset.asset_id = hc.existing_asset_id
         AND existing_asset.path = hc.existing_path
         AND existing_asset.hash = hc.content_hash
         AND existing_asset.size_bytes = hc.size_bytes
         AND existing_asset.status != 'deleted'
        WHERE hc.repo_id = ?1
        ORDER BY hc.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map([repo_id], map_hardlink_candidate_row)?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_hardlink_candidate_from_transaction(
    tx: &Transaction<'_>,
    repo_id: &str,
    candidate_id: &str,
) -> Result<Option<HardlinkCandidate>, rusqlite::Error> {
    tx.query_row(
        r#"
        SELECT hc.candidate_id, hc.repo_id, hc.new_asset_id, hc.new_path,
               hc.existing_asset_id, hc.existing_path, hc.content_hash,
               hc.size_bytes, hc.created_at
        FROM hardlink_candidates hc
        JOIN assets new_asset
          ON new_asset.repo_id = hc.repo_id
         AND new_asset.asset_id = hc.new_asset_id
         AND new_asset.path = hc.new_path
         AND new_asset.hash = hc.content_hash
         AND new_asset.size_bytes = hc.size_bytes
         AND new_asset.status != 'deleted'
        JOIN assets existing_asset
          ON existing_asset.repo_id = hc.repo_id
         AND existing_asset.asset_id = hc.existing_asset_id
         AND existing_asset.path = hc.existing_path
         AND existing_asset.hash = hc.content_hash
         AND existing_asset.size_bytes = hc.size_bytes
         AND existing_asset.status != 'deleted'
        WHERE hc.repo_id = ?1 AND hc.candidate_id = ?2
        "#,
        params![repo_id, candidate_id],
        map_hardlink_candidate_row,
    )
    .optional()
}

pub(super) fn delete_hardlink_candidate(
    tx: &Transaction<'_>,
    repo_id: &str,
    candidate_id: &str,
) -> Result<(), rusqlite::Error> {
    tx.execute(
        "DELETE FROM hardlink_candidates WHERE repo_id = ?1 AND candidate_id = ?2",
        params![repo_id, candidate_id],
    )?;
    Ok(())
}

pub(super) fn map_hardlink_candidate_row(
    row: &rusqlite::Row<'_>,
) -> Result<HardlinkCandidate, rusqlite::Error> {
    let size_bytes = row.get::<_, i64>(7)?;
    Ok(HardlinkCandidate {
        candidate_id: row.get(0)?,
        repo_id: row.get(1)?,
        new_asset_id: row.get(2)?,
        new_path: row.get(3)?,
        existing_asset_id: row.get(4)?,
        existing_path: row.get(5)?,
        content_hash: row.get(6)?,
        size_bytes,
        size_label: format_size_label(size_bytes),
        created_at: row.get(8)?,
    })
}

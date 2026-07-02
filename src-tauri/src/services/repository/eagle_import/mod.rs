//! EagleLibrary 原生导入共享模块。
//!
//! 该模块承接 EagleLibrary -> 当前仓库 的直接导入逻辑，
//! 供宿主与原生后端插件共用同一套实现。

use self::planner::{
    build_conversion_plan, flatten_folder_nodes, warning, AssetMembership, AssetPlan,
    ConversionPlan, FolderNode,
};
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) mod planner;
pub(super) mod repository_objects;
pub(super) mod smart_folder;
pub(super) mod smart_folder_fields;

const EAGLE_IMPORTER_PLUGIN_SOURCE: &str = "eagle-importer";

/// 供原生插件调用的共享 Eagle 导入入口。
pub fn import_eagle_library_with_service_root(
    service_root: &Path,
    request: EagleLibraryImportRequest,
) -> Result<EagleLibraryPluginImportResponse, String> {
    let state = RepositoryState::from_root(service_root.to_path_buf());
    import_eagle_library_with_state(&state, request)
}

/// 直接把 EagleLibrary 合并写入当前仓库，不再依赖临时转换仓库。
pub fn import_eagle_library_with_state(
    state: &RepositoryState,
    request: EagleLibraryImportRequest,
) -> Result<EagleLibraryPluginImportResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    ensure_repository_supports_local_write_access(&repo, "importing Eagle libraries")?;

    let repo_root = PathBuf::from(&repo.summary.path);
    let parent_path = normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
    let target_dir = resolve_repository_relative_path(&repo_root, &parent_path)?;
    if !target_dir.exists() || !target_dir.is_dir() {
        return Err(format!("directory not found: {parent_path}"));
    }

    let library_root = canonicalize_local_path(Path::new(request.library_path.trim()))?;
    validate_eagle_library_root(&library_root)?;
    let mode = normalize_eagle_import_mode(&request.mode)?;

    let mut plan = build_conversion_plan(&library_root, &repo.summary.repo_id, &parent_path)?;
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    validate_conversion_plan(
        &connection,
        &repo,
        &repo_root,
        state.repository_thumbnail_root(&repo)?,
        &plan,
    )?;
    execute_conversion_plan(
        &mut connection,
        &repo,
        &repo_root,
        state.repository_thumbnail_root(&repo)?,
        &mut plan,
        mode,
    )?;

    Ok(EagleLibraryPluginImportResponse {
        summary: summarize_plan(&plan),
        warnings: plan.warnings,
    })
}

/// 校验 EagleLibrary 目录结构。
fn validate_eagle_library_root(library_root: &Path) -> Result<(), String> {
    if !library_root.is_dir() {
        return Err(format!(
            "Eagle library is not a directory: {}",
            library_root.to_string_lossy()
        ));
    }
    for required in ["metadata.json", "images"] {
        if !library_root.join(required).exists() {
            return Err(format!("invalid EagleLibrary, missing: {required}"));
        }
    }
    Ok(())
}

/// 归一化 Eagle 导入模式。
fn normalize_eagle_import_mode(mode: &str) -> Result<&'static str, String> {
    match mode.trim() {
        "copy" => Ok("copy"),
        "move" => Ok("move"),
        value => Err(format!("unsupported eagle import mode: {value}")),
    }
}

/// 对转换计划做 fail-fast 冲突检查。
fn validate_conversion_plan(
    connection: &Connection,
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: PathBuf,
    plan: &ConversionPlan,
) -> Result<(), String> {
    let existing_assets =
        load_asset_path_map(connection, &repo.summary.repo_id).map_err(db_error)?;
    let mut planned_visible_files = BTreeSet::new();
    let mut planned_trash_files = BTreeSet::new();
    let mut planned_thumbnails = BTreeSet::new();
    let mut required_directories = BTreeSet::new();

    for folder in flatten_folder_nodes(&plan.folder_nodes) {
        required_directories.insert(folder.path);
    }
    for asset in &plan.assets {
        for membership in &asset.memberships {
            if !asset.is_deleted {
                required_directories.insert(membership.target_relative_dir.clone());
            }
        }
    }

    for directory_path in required_directories {
        let absolute = resolve_repository_relative_path(repo_root, &directory_path)?;
        if absolute.exists() && !absolute.is_dir() {
            return Err(format!("target already exists: {directory_path}"));
        }
    }

    let destination_trash_root = repository_trash_dir(repo_root);
    let trash_manifest = load_trash_manifest(repo_root)?;
    for asset in &plan.assets {
        for membership in &asset.memberships {
            if existing_assets.contains_key(&membership.target_relative_path) {
                return Err(format!(
                    "entry already exists: {}",
                    membership.target_relative_path
                ));
            }

            if asset.is_deleted {
                if !planned_trash_files.insert(membership.target_relative_path.clone()) {
                    return Err(format!(
                        "duplicate Eagle trash target: {}",
                        membership.target_relative_path
                    ));
                }
                let trash_abs = resolve_trash_relative_path(
                    &destination_trash_root,
                    &membership.target_relative_path,
                )?;
                if trash_abs.exists() {
                    return Err(format!(
                        "trash file already exists: {}",
                        membership.target_relative_path
                    ));
                }
            } else {
                if !planned_visible_files.insert(membership.target_relative_path.clone()) {
                    return Err(format!(
                        "duplicate Eagle target: {}",
                        membership.target_relative_path
                    ));
                }
                let target_abs =
                    resolve_repository_relative_path(repo_root, &membership.target_relative_path)?;
                if target_abs.exists() {
                    return Err(format!(
                        "entry already exists: {}",
                        membership.target_relative_path
                    ));
                }
            }
        }

        if asset.is_deleted {
            let primary = primary_membership(asset)?;
            if trash_manifest.entries.iter().any(|entry| {
                entry.original_path == primary.target_relative_path
                    || entry.trash_path == primary.target_relative_path
            }) {
                return Err(format!(
                    "trash target already exists: {}",
                    primary.target_relative_path
                ));
            }
        }

        if asset.source_thumbnail.is_some() {
            let thumbnail_path =
                build_thumbnail_target_path(repo, thumbnail_root.as_path(), asset)?;
            let marker = thumbnail_path.to_string_lossy().to_string();
            if !planned_thumbnails.insert(marker.clone()) {
                return Err(format!("thumbnail target already exists: {marker}"));
            }
            if thumbnail_path.exists() {
                return Err(format!("thumbnail target already exists: {marker}"));
            }
        }
    }

    Ok(())
}

/// 执行计划写入：文件、缩略图、回收站与 SQLite 记录。
fn execute_conversion_plan(
    connection: &mut Connection,
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: PathBuf,
    plan: &mut ConversionPlan,
    mode: &str,
) -> Result<(), String> {
    create_plan_directories(repo_root, &plan.folder_nodes)?;
    move_plan_assets(repo, repo_root, thumbnail_root.as_path(), plan, mode)?;

    let tx = connection.transaction().map_err(db_error)?;
    let now = now_rfc3339();
    write_folder_metadata_records(&tx, &repo.summary.repo_id, &plan.folder_nodes, &now)?;
    write_repository_shortcuts(&tx, &repo.summary.repo_id, &plan.quick_access, &now)?;
    write_repository_actions(&tx, &repo.summary.repo_id, &plan.repository_actions, &now)?;
    write_tag_groups(&tx, &repo.summary.repo_id, &plan.tag_groups, &now)?;
    write_smart_folders(&tx, &repo.summary.repo_id, &plan.smart_folders, &now)?;
    write_asset_records(
        &tx,
        repo,
        repo_root,
        thumbnail_root.as_path(),
        &plan.assets,
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    append_trash_manifest(repo_root, &plan.assets, &now)
}

/// 预创建可见目录，保留空目录结构。
fn create_plan_directories(repo_root: &Path, folder_nodes: &[FolderNode]) -> Result<(), String> {
    for folder in flatten_folder_nodes(folder_nodes) {
        let target_abs = resolve_repository_relative_path(repo_root, &folder.path)?;
        fs::create_dir_all(target_abs).map_err(io_error)?;
    }
    Ok(())
}

/// 搬运或复制素材，并在失败时记录硬链接回退警告。
fn move_plan_assets(
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: &Path,
    plan: &mut ConversionPlan,
    mode: &str,
) -> Result<(), String> {
    let trash_root = repository_trash_dir(repo_root);
    for asset in &mut plan.assets {
        let is_deleted = asset.is_deleted;
        let source_file = asset.source_file.clone();
        let primary = primary_membership_mut(asset)?;
        let primary_target = membership_output_path(repo_root, &trash_root, is_deleted, primary)?;
        if let Some(parent) = primary_target.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        transfer_file(&source_file, &primary_target, mode)?;
        primary.link_state = Some("primary".to_string());

        let primary_snapshot = primary.clone();
        for membership in asset.memberships.iter_mut().skip(1) {
            let alias_target =
                membership_output_path(repo_root, &trash_root, asset.is_deleted, membership)?;
            if let Some(parent) = alias_target.parent() {
                fs::create_dir_all(parent).map_err(io_error)?;
            }
            match fs::hard_link(&primary_target, &alias_target) {
                Ok(_) => {
                    membership.link_state = Some("linked".to_string());
                }
                Err(error) => {
                    fs::copy(&primary_target, &alias_target).map_err(io_error)?;
                    membership.link_state = Some("copied".to_string());
                    plan.warnings.push(
                        warning("aliasHardlinkFallback")
                            .with_asset_id(&asset.eagle_asset_id)
                            .with_reason(&error.to_string())
                            .with_details(serde_json::json!({
                                "targetRelativePath": membership.target_relative_path,
                                "primaryTargetRelativePath": primary_snapshot.target_relative_path,
                            }))
                            .into(),
                    );
                }
            }
        }

        if asset.source_thumbnail.is_some() {
            let thumbnail_target = build_thumbnail_target_path(repo, thumbnail_root, asset)?;
            if let Some(parent) = thumbnail_target.parent() {
                fs::create_dir_all(parent).map_err(io_error)?;
            }
            transfer_file(
                asset.source_thumbnail.as_ref().expect("checked above"),
                &thumbnail_target,
                mode,
            )?;
        }

        if mode == "move" && asset.source_info_dir.exists() {
            fs::remove_dir_all(&asset.source_info_dir).map_err(io_error)?;
        }
    }
    Ok(())
}

/// 复制或移动单个文件，移动模式兼容跨卷回退。
fn transfer_file(source: &Path, target: &Path, mode: &str) -> Result<(), String> {
    match mode {
        "copy" => {
            fs::copy(source, target).map_err(io_error)?;
            Ok(())
        }
        "move" => match fs::rename(source, target) {
            Ok(_) => Ok(()),
            Err(_) => {
                fs::copy(source, target).map_err(io_error)?;
                fs::remove_file(source).map_err(io_error)
            }
        },
        other => Err(format!("unsupported eagle import mode: {other}")),
    }
}

/// 把导入计划的保护目录信息写入仓库。
fn write_folder_metadata_records(
    tx: &Transaction<'_>,
    repo_id: &str,
    folder_nodes: &[FolderNode],
    now: &str,
) -> Result<(), String> {
    for folder in flatten_folder_nodes(folder_nodes) {
        if !folder.protected && folder.password_tip.is_none() {
            continue;
        }
        tx.execute(
            r#"
            INSERT OR REPLACE INTO folder_metadata (repo_id, path, protected, password_tip, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                repo_id,
                folder.path,
                if folder.protected { 1 } else { 0 },
                folder.password_tip,
                now
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

/// 写入 Eagle 快捷入口。
fn write_repository_shortcuts(
    tx: &Transaction<'_>,
    repo_id: &str,
    shortcuts: &[planner::RepositoryShortcutPlan],
    now: &str,
) -> Result<(), String> {
    for shortcut in shortcuts {
        tx.execute(
            r#"
            INSERT OR REPLACE INTO repository_shortcuts (
              shortcut_id, repo_id, label, target_kind, target_path, target_id, sort_order, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                shortcut.shortcut_id,
                repo_id,
                shortcut.label,
                shortcut.target_kind,
                shortcut.target_path,
                shortcut.target_id,
                shortcut.sort_order,
                now
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

/// 写入 Eagle 动作与步骤。
fn write_repository_actions(
    tx: &Transaction<'_>,
    repo_id: &str,
    actions: &[planner::RepositoryActionPlan],
    now: &str,
) -> Result<(), String> {
    for action in actions {
        tx.execute(
            r#"
            INSERT OR REPLACE INTO repository_actions (
              action_id, repo_id, source, source_action_id, name, status, enabled, raw_json,
              unsupported_reason, sort_order, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
            params![
                action.action_id,
                repo_id,
                EAGLE_IMPORTER_PLUGIN_SOURCE,
                action.source_action_id,
                action.name,
                action.status,
                if action.enabled { 1 } else { 0 },
                action.raw.to_string(),
                action.unsupported_reason,
                action.sort_order,
                now,
                now
            ],
        )
        .map_err(db_error)?;

        for step in &action.steps {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO repository_action_steps (
                  step_id, action_id, repo_id, step_kind, label, status, config_json, raw_json,
                  unsupported_reason, sort_order
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
                params![
                    step.step_id,
                    action.action_id,
                    repo_id,
                    step.step_kind,
                    step.label,
                    step.status,
                    step.config.to_string(),
                    step.raw.to_string(),
                    step.unsupported_reason,
                    step.sort_order
                ],
            )
            .map_err(db_error)?;
        }
    }
    Ok(())
}

/// 写入标签组。
fn write_tag_groups(
    tx: &Transaction<'_>,
    repo_id: &str,
    groups: &[planner::TagGroupPlan],
    now: &str,
) -> Result<(), String> {
    for group in groups {
        tx.execute(
            r#"
            INSERT OR REPLACE INTO tag_groups (tag_group_id, repo_id, name, sort_order, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![group.tag_group_id, repo_id, group.name, group.sort_order, now, now],
        )
        .map_err(db_error)?;
        for (index, tag) in group.tags.iter().enumerate() {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO tag_group_members (
                  tag_group_id, repo_id, tag, normalized_tag, sort_order
                )
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![
                    group.tag_group_id,
                    repo_id,
                    tag,
                    tag.to_lowercase(),
                    index as i64
                ],
            )
            .map_err(db_error)?;
        }
    }
    Ok(())
}

/// 写入智能文件夹。
fn write_smart_folders(
    tx: &Transaction<'_>,
    repo_id: &str,
    smart_folders: &[planner::SmartFolderPlan],
    now: &str,
) -> Result<(), String> {
    for smart_folder in smart_folders {
        tx.execute(
            r#"
            INSERT OR REPLACE INTO smart_folders (
              smart_folder_id, repo_id, parent_id, name, filter_json, sort_order, created_at, updated_at
            )
            VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                smart_folder.smart_folder_id,
                repo_id,
                smart_folder.name,
                serde_json::to_string(&smart_folder.filter).map_err(json_error)?,
                smart_folder.sort_order,
                now,
                now
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

/// 写入素材、metadata、alias、hardlink、缩略图与事件。
fn write_asset_records(
    tx: &Transaction<'_>,
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: &Path,
    assets: &[AssetPlan],
    now: &str,
) -> Result<(), String> {
    let trash_root = repository_trash_dir(repo_root);
    for asset in assets {
        let existing_memberships = asset
            .memberships
            .iter()
            .filter_map(|membership| {
                membership_output_path(repo_root, &trash_root, asset.is_deleted, membership)
                    .ok()
                    .filter(|path| path.is_file())
                    .map(|_| membership.clone())
            })
            .collect::<Vec<_>>();
        if existing_memberships.is_empty() {
            continue;
        }

        let primary_target = membership_output_path(
            repo_root,
            &trash_root,
            asset.is_deleted,
            &existing_memberships[0],
        )?;
        let file_hash = file_sha256_hash(&primary_target)?;
        let size_bytes = i64::try_from(fs::metadata(&primary_target).map_err(io_error)?.len())
            .map_err(|_| "file size exceeds supported range".to_string())?;
        let alias_group_id = alias_group_id_for_asset(&repo.summary.repo_id, &asset.eagle_asset_id);
        let hardlink_group_id =
            hardlink_group_id_for_asset(&repo.summary.repo_id, &file_hash, size_bytes);

        if existing_memberships.len() > 1 {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO asset_alias_groups (alias_group_id, repo_id, source, created_at, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![
                    alias_group_id,
                    repo.summary.repo_id,
                    EAGLE_IMPORTER_PLUGIN_SOURCE,
                    now,
                    now
                ],
            )
            .map_err(db_error)?;
            tx.execute(
                r#"
                INSERT OR REPLACE INTO hardlink_groups (
                  group_id, repo_id, content_hash, size_bytes, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    hardlink_group_id,
                    repo.summary.repo_id,
                    file_hash,
                    size_bytes,
                    now,
                    now
                ],
            )
            .map_err(db_error)?;
        }

        let primary_thumbnail_path = if asset.source_thumbnail.is_some() {
            Some(
                build_thumbnail_target_path(repo, thumbnail_root, asset)?
                    .to_string_lossy()
                    .to_string(),
            )
        } else {
            None
        };
        let metadata_entries = seed_asset_metadata_entries(asset);

        for membership in existing_memberships {
            let member_path =
                membership_output_path(repo_root, &trash_root, asset.is_deleted, &membership)?;
            let member_metadata = fs::metadata(&member_path).map_err(io_error)?;
            let modified_at = metadata_modified_at(&member_metadata)?;
            let member_size = i64::try_from(member_metadata.len())
                .map_err(|_| "file size exceeds supported range".to_string())?;
            let asset_id =
                asset_id_for_path(&repo.summary.repo_id, &membership.target_relative_path);
            let thumbnail_path = (membership.role == "primary")
                .then(|| primary_thumbnail_path.clone())
                .flatten();

            tx.execute(
                r#"
                INSERT OR REPLACE INTO assets (
                  asset_id, repo_id, path, filename, extension, size_bytes, created_at, modified_at, hash,
                  status, version, updated_at, last_accessed_at, thumbnail_path, is_virtual, provider_id,
                  provider_item_id, source_payload_json, local_absolute_path
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, NULL, ?12, 0, NULL, NULL, NULL, NULL)
                "#,
                params![
                    asset_id,
                    repo.summary.repo_id,
                    membership.target_relative_path,
                    membership.target_filename,
                    asset.extension,
                    member_size,
                    now,
                    modified_at,
                    file_hash,
                    if asset.is_deleted { "deleted" } else { "synced" },
                    now,
                    thumbnail_path
                ],
            )
            .map_err(db_error)?;

            replace_seeded_asset_metadata(tx, &asset_id, &metadata_entries, now)?;
            replace_seeded_asset_tags(tx, &asset_id, &asset.tags, now)?;

            if let Some(thumbnail_path) = thumbnail_path.as_deref() {
                tx.execute(
                    r#"
                    INSERT OR REPLACE INTO entry_thumbnails (repo_id, path, kind, thumbnail_path, custom, updated_at)
                    VALUES (?1, ?2, 'file', ?3, 0, ?4)
                    "#,
                    params![
                        repo.summary.repo_id,
                        membership.target_relative_path,
                        thumbnail_path,
                        now
                    ],
                )
                .map_err(db_error)?;
            }

            tx.execute(
                r#"
                INSERT OR REPLACE INTO revisions (
                  revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
                )
                VALUES (?1, ?2, ?3, ?4, 'metadata.seeded', '{}', ?5, ?6)
                "#,
                params![
                    format!("rev-{asset_id}"),
                    repo.summary.repo_id,
                    asset_id,
                    now,
                    serde_json::to_string(&metadata_entries).map_err(json_error)?,
                    EAGLE_IMPORTER_PLUGIN_SOURCE
                ],
            )
            .map_err(db_error)?;
            tx.execute(
                r#"
                INSERT OR REPLACE INTO events (
                  event_id, repo_id, asset_id, event_type, path, payload_json, created_at
                )
                VALUES (?1, ?2, ?3, 'asset.discovered', ?4, ?5, ?6)
                "#,
                params![
                    format!("evt-{asset_id}"),
                    repo.summary.repo_id,
                    asset_id,
                    membership.target_relative_path,
                    serde_json::json!({
                        "origin": EAGLE_IMPORTER_PLUGIN_SOURCE,
                        "aliasRole": membership.role,
                    })
                    .to_string(),
                    now
                ],
            )
            .map_err(db_error)?;

            if asset.memberships.len() > 1 {
                tx.execute(
                    r#"
                    INSERT OR REPLACE INTO asset_alias_members (
                      alias_group_id, repo_id, asset_id, path, role, created_at
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    "#,
                    params![
                        alias_group_id,
                        repo.summary.repo_id,
                        asset_id,
                        membership.target_relative_path,
                        membership.role,
                        now
                    ],
                )
                .map_err(db_error)?;
                tx.execute(
                    r#"
                    INSERT OR REPLACE INTO hardlink_members (
                      group_id, repo_id, asset_id, path, link_state, linked_at, verified_at
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    "#,
                    params![
                        hardlink_group_id,
                        repo.summary.repo_id,
                        asset_id,
                        membership.target_relative_path,
                        membership
                            .link_state
                            .clone()
                            .unwrap_or_else(|| "linked".to_string()),
                        now,
                        now
                    ],
                )
                .map_err(db_error)?;
            }
        }
    }
    Ok(())
}

/// 生成素材 metadata 种子集。
fn seed_asset_metadata_entries(asset: &AssetPlan) -> BTreeMap<String, serde_json::Value> {
    let mut metadata_entries = BTreeMap::new();
    metadata_entries.insert(
        "title".to_string(),
        serde_json::Value::String(asset.display_title.clone()),
    );
    metadata_entries.insert(
        "type".to_string(),
        serde_json::Value::String(asset.extension.clone()),
    );
    metadata_entries.insert("favorite".to_string(), serde_json::Value::Bool(false));
    if let Some(note) = asset.note.as_ref() {
        metadata_entries.insert(
            "comment".to_string(),
            serde_json::Value::String(note.clone()),
        );
    }
    if let Some(color) = asset.palette.first() {
        metadata_entries.insert(
            "color".to_string(),
            serde_json::Value::String(color.clone()),
        );
    }
    if !asset.palette.is_empty() {
        metadata_entries.insert(
            "palette".to_string(),
            serde_json::Value::Array(
                asset
                    .palette
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }
    for (key, value) in &asset.preserved_metadata {
        metadata_entries.insert(key.clone(), value.clone());
    }
    if !asset.tags.is_empty() {
        metadata_entries.insert(
            "tagGroups".to_string(),
            serde_json::Value::Array(
                asset
                    .tags
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }
    metadata_entries
}

/// 覆盖写入素材 metadata。
fn replace_seeded_asset_metadata(
    tx: &Transaction<'_>,
    asset_id: &str,
    metadata_entries: &BTreeMap<String, serde_json::Value>,
    now: &str,
) -> Result<(), String> {
    tx.execute("DELETE FROM metadata WHERE asset_id = ?1", [asset_id])
        .map_err(db_error)?;
    for (key, value) in metadata_entries {
        tx.execute(
            r#"
            INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
            VALUES (?1, ?2, ?3, ?4, 1, ?5)
            "#,
            params![
                asset_id,
                key,
                infer_value_type(value),
                serde_json::to_string(value).map_err(json_error)?,
                now
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

/// 覆盖写入素材 tags。
fn replace_seeded_asset_tags(
    tx: &Transaction<'_>,
    asset_id: &str,
    tags: &[String],
    _now: &str,
) -> Result<(), String> {
    tx.execute("DELETE FROM tags WHERE asset_id = ?1", [asset_id])
        .map_err(db_error)?;
    for tag in tags {
        tx.execute(
            r#"
            INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag)
            VALUES (?1, ?2, ?3)
            "#,
            params![asset_id, tag, tag.to_lowercase()],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

/// 追加回收站 manifest。
fn append_trash_manifest(repo_root: &Path, assets: &[AssetPlan], now: &str) -> Result<(), String> {
    let mut manifest = load_trash_manifest(repo_root)?;
    for asset in assets {
        if !asset.is_deleted {
            continue;
        }
        let primary = primary_membership(asset)?;
        manifest.entries.push(TrashManifestEntry {
            original_path: primary.target_relative_path.clone(),
            trash_path: primary.target_relative_path.clone(),
            deleted_at: now.to_string(),
            kind: "file".to_string(),
        });
    }
    save_trash_manifest(repo_root, &manifest)
}

/// 计算导入摘要。
fn summarize_plan(plan: &ConversionPlan) -> EagleLibraryImportSummary {
    EagleLibraryImportSummary {
        imported_files: plan
            .assets
            .iter()
            .map(|asset| asset.memberships.len())
            .sum(),
        imported_directories: flatten_folder_nodes(&plan.folder_nodes).len(),
        imported_trash_entries: plan.assets.iter().filter(|asset| asset.is_deleted).count(),
        imported_shortcuts: plan.quick_access.len(),
        imported_smart_folders: plan.smart_folders.len(),
        imported_repository_actions: plan.repository_actions.len(),
        imported_tag_groups: plan.tag_groups.len(),
        imported_alias_groups: plan
            .assets
            .iter()
            .filter(|asset| asset.memberships.len() > 1)
            .count(),
        imported_hardlink_groups: plan
            .assets
            .iter()
            .filter(|asset| asset.memberships.len() > 1)
            .count(),
    }
}

/// 根据仓库约定计算 Eagle 缩略图落盘路径。
fn build_thumbnail_target_path(
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    asset: &AssetPlan,
) -> Result<PathBuf, String> {
    let primary = primary_membership(asset)?;
    let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
        &repo.summary.repo_id,
        &repo.summary.path,
    ));
    let mut file_name = thumbnail_file_name(
        &repo.summary.repo_id,
        &repo.summary.path,
        &primary.target_relative_path,
        "file",
        "eagle",
    );
    if let Some(source_thumbnail) = asset.source_thumbnail.as_ref() {
        if let Some(extension) = source_thumbnail
            .extension()
            .and_then(|value| value.to_str())
        {
            file_name = format!(
                "{}.{}",
                Path::new(&file_name)
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("thumbnail"),
                extension.to_ascii_lowercase()
            );
        }
    }
    Ok(thumbnail_dir.join(file_name))
}

/// 返回主 membership。
fn primary_membership(asset: &AssetPlan) -> Result<&AssetMembership, String> {
    asset.memberships.first().ok_or_else(|| {
        format!(
            "Eagle asset has no target memberships: {}",
            asset.eagle_asset_id
        )
    })
}

/// 返回主 membership 的可变引用。
fn primary_membership_mut(asset: &mut AssetPlan) -> Result<&mut AssetMembership, String> {
    asset.memberships.first_mut().ok_or_else(|| {
        format!(
            "Eagle asset has no target memberships: {}",
            asset.eagle_asset_id
        )
    })
}

/// 解析 membership 的目标输出路径。
fn membership_output_path(
    repo_root: &Path,
    trash_root: &Path,
    is_deleted: bool,
    membership: &AssetMembership,
) -> Result<PathBuf, String> {
    if is_deleted {
        return resolve_trash_relative_path(trash_root, &membership.target_relative_path);
    }
    resolve_repository_relative_path(repo_root, &membership.target_relative_path)
}

/// 读取文件修改时间并转成 RFC3339。
fn metadata_modified_at(metadata: &fs::Metadata) -> Result<String, String> {
    metadata
        .modified()
        .map_err(io_error)
        .and_then(|value| system_time_to_rfc3339(value).map_err(time_error))
}

/// 生成 alias group ID。
fn alias_group_id_for_asset(repo_id: &str, eagle_asset_id: &str) -> String {
    format!(
        "alias-{}",
        sha256_hex(&[repo_id.as_bytes(), eagle_asset_id.as_bytes()])
    )
}

/// 生成 hardlink group ID。
fn hardlink_group_id_for_asset(repo_id: &str, content_hash: &str, size_bytes: i64) -> String {
    format!(
        "hardlink-{}",
        sha256_hex(&[
            repo_id.as_bytes(),
            content_hash.as_bytes(),
            size_bytes.to_string().as_bytes(),
        ])
    )
}

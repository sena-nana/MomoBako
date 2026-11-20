//! 基于插件运行时的文件系统后端适配器实现。

use super::*;

impl FileSystemBackendAdapter for RuntimeFileSystemBackendAdapter {
    fn ensure_attachable(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.ensureAttachable",
            serde_json::json!({
                "repoRoot": repo_root,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn prepare_repository_root(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.prepareRepositoryRoot",
            serde_json::json!({
                "repoRoot": repo_root,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn list_files(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<Vec<DiscoveredFile>, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.listFiles",
            serde_json::json!({
                "repoRoot": repo_root,
                "config": config,
            }),
        )?;
        serde_json::from_value::<Vec<BackendDiscoveredFile>>(response)
            .map_err(json_error)?
            .into_iter()
            .map(|file| file.into_discovered_file(repo_root))
            .collect()
    }

    fn list_tree(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<Vec<FileTreeNode>, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.listTree",
            serde_json::json!({
                "repoRoot": repo_root,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn list_directory_entries(
        &self,
        repo_root: &Path,
        directory_path: &str,
        config: &serde_json::Value,
    ) -> Result<Vec<FileSystemEntry>, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.listDirectory",
            serde_json::json!({
                "repoRoot": repo_root,
                "directoryPath": directory_path,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn list_directory_entries_page(
        &self,
        repo_root: &Path,
        directory_path: &str,
        offset: usize,
        limit: usize,
        config: &serde_json::Value,
    ) -> Result<DirectoryPageResult, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.listDirectoryPage",
            serde_json::json!({
                "repoRoot": repo_root,
                "directoryPath": directory_path,
                "offset": offset,
                "limit": limit,
                "config": config,
            }),
        );
        match response {
            Ok(value) => serde_json::from_value(value).map_err(json_error),
            Err(error) if is_unsupported_filesystem_backend_method(&error) => {
                let entries = self.list_directory_entries(repo_root, directory_path, config)?;
                let total_entries = entries.len();
                Ok(DirectoryPageResult {
                    entries: entries.into_iter().skip(offset).take(limit).collect(),
                    total_entries,
                })
            }
            Err(error) => Err(error),
        }
    }

    fn create_directory(
        &self,
        repo_root: &Path,
        parent_path: &str,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.createDirectory",
            serde_json::json!({
                "repoRoot": repo_root,
                "parentPath": parent_path,
                "name": name,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn create_file(
        &self,
        repo_root: &Path,
        parent_path: &str,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.createFile",
            serde_json::json!({
                "repoRoot": repo_root,
                "parentPath": parent_path,
                "name": name,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn stat_entry(
        &self,
        repo_root: &Path,
        entry_path: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.statEntry",
            serde_json::json!({
                "repoRoot": repo_root,
                "entryPath": entry_path,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn rename_entry(
        &self,
        repo_root: &Path,
        source_path: &str,
        new_name: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.renameEntry",
            serde_json::json!({
                "repoRoot": repo_root,
                "sourcePath": source_path,
                "newName": new_name,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn move_entry(
        &self,
        repo_root: &Path,
        source_path: &str,
        target_parent_path: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.moveEntry",
            serde_json::json!({
                "repoRoot": repo_root,
                "sourcePath": source_path,
                "targetParentPath": target_parent_path,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn delete_entry(
        &self,
        repo_root: &Path,
        entry_path: &str,
        recursive: bool,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.deleteEntry",
            serde_json::json!({
                "repoRoot": repo_root,
                "entryPath": entry_path,
                "recursive": recursive,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn describe_repository_state(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<SourceRepositoryStateSnapshot, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.describeRepositoryState",
            serde_json::json!({
                "repoRoot": repo_root,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn write_asset_metadata(
        &self,
        repo_root: &Path,
        path: &str,
        shared_asset_id: Option<&str>,
        metadata: &BTreeMap<String, serde_json::Value>,
        previous_metadata: &BTreeMap<String, serde_json::Value>,
        operation: &str,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.writeAssetMetadata",
            serde_json::json!({
                "repoRoot": repo_root,
                "path": path,
                "sharedAssetId": shared_asset_id,
                "metadata": metadata,
                "previousMetadata": previous_metadata,
                "operation": operation,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn write_repository_state(
        &self,
        repo_root: &Path,
        state: &SourceRepositoryStateSnapshot,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.writeRepositoryState",
            serde_json::json!({
                "repoRoot": repo_root,
                "directoryMetadataByPath": &state.directory_metadata_by_path,
                "quickAccess": &state.quick_access,
                "tagGroups": &state.tag_groups,
                "smartFolders": &state.smart_folders,
                "repositoryActions": &state.repository_actions,
                "config": config,
            }),
        )?;
        Ok(())
    }
}

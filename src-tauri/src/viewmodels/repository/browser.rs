//! File-browser command orchestration for repository entries and trash operations.

use crate::services::repository::{
    EagleLibraryImportRequest, EagleLibraryImportResponse, FileArchiveImportRequest,
    FileBrowserRequest, FileBrowserSnapshot, FileCopyRequest, FileCreateRequest, FileDeleteRequest,
    FileImportRequest, FileMoveRequest, FileRenameRequest, RepositoryTreeSnapshot,
    TrashMutationRequest,
};
use crate::services::runtime::RepositoryRuntime;

#[derive(Clone)]
pub struct FileBrowserViewModel {
    runtime: RepositoryRuntime,
}

impl FileBrowserViewModel {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub async fn get_file_browser(
        &self,
        request: FileBrowserRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_read(move |state| state.load_file_browser(request))
            .await
    }

    pub async fn get_repository_tree(
        &self,
        repo_id: String,
    ) -> Result<RepositoryTreeSnapshot, String> {
        self.runtime
            .run_read(move |state| state.load_repository_tree(&repo_id))
            .await
    }

    pub async fn create_directory(
        &self,
        request: FileCreateRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_write(move |state| state.create_directory(request))
            .await
    }

    pub async fn create_file(
        &self,
        request: FileCreateRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_write(move |state| state.create_file(request))
            .await
    }

    pub async fn import_entries(
        &self,
        request: FileImportRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_write(move |state| state.import_entries(request))
            .await
    }

    pub async fn import_archive_entries(
        &self,
        request: FileArchiveImportRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_write(move |state| state.import_archive_entries(request))
            .await
    }

    pub async fn import_eagle_library(
        &self,
        request: EagleLibraryImportRequest,
    ) -> Result<EagleLibraryImportResponse, String> {
        self.runtime
            .run_write(move |state| state.import_eagle_library(request))
            .await
    }

    pub async fn copy_entries(
        &self,
        request: FileCopyRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_write(move |state| state.copy_entries(request))
            .await
    }

    pub async fn move_entries(
        &self,
        request: FileMoveRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_write(move |state| state.move_entries(request))
            .await
    }

    pub async fn rename_entry(
        &self,
        request: FileRenameRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_write(move |state| state.rename_entry(request))
            .await
    }

    pub async fn delete_entry(
        &self,
        request: FileDeleteRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_write(move |state| state.delete_entry(request))
            .await
    }

    pub async fn mutate_trash(
        &self,
        request: TrashMutationRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_write(move |state| state.mutate_trash(request))
            .await
    }
}

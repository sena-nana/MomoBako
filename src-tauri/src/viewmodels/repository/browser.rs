//! File-browser command orchestration for repository entries and trash operations.

use crate::services::repository::{
    FileBrowserRequest, FileBrowserSnapshot, FileCopyRequest, FileCreateRequest, FileDeleteRequest,
    FileImportRequest, FileMoveRequest, FileRenameRequest, TrashMutationRequest,
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

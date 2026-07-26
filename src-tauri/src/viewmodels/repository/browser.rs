//! File-browser command orchestration for repository entries and trash operations.

use crate::services::repository::{
    FileBrowserRequest, FileBrowserSnapshot, FileCreateRequest, FileRenameRequest,
    RepositoryTreeSnapshot, TrashMutationRequest,
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

    pub async fn rename_entry(
        &self,
        request: FileRenameRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .run_write(move |state| state.rename_entry(request))
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

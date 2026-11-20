//! File-browser orchestration for repository entries and trash operations.

use crate::services::repository::{
    FileBrowserRequest, FileBrowserSnapshot, FileCreateRequest, FileRenameRequest,
    RepositoryTreeSnapshot, ThumbnailRequest, ThumbnailResponse, TrashMutationRequest,
};
use crate::services::runtime::RepositoryRuntime;

#[derive(Clone)]
pub struct FileBrowserService {
    runtime: RepositoryRuntime,
}

impl FileBrowserService {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub fn get_file_browser(
        &self,
        request: FileBrowserRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .read(move |state| state.load_file_browser(request))
    }

    pub fn get_repository_tree(&self, repo_id: String) -> Result<RepositoryTreeSnapshot, String> {
        self.runtime
            .read(move |state| state.load_repository_tree(&repo_id))
    }

    pub fn create_directory(
        &self,
        request: FileCreateRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime
            .write(move |state| state.create_directory(request))
    }

    pub fn create_file(&self, request: FileCreateRequest) -> Result<FileBrowserSnapshot, String> {
        self.runtime.write(move |state| state.create_file(request))
    }

    pub fn rename_entry(&self, request: FileRenameRequest) -> Result<FileBrowserSnapshot, String> {
        self.runtime.write(move |state| state.rename_entry(request))
    }

    pub fn mutate_trash(
        &self,
        request: TrashMutationRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.runtime.write(move |state| state.mutate_trash(request))
    }

    /// 保存、清除或恢复条目缩略图。写入走现有 `ensure_thumbnail`，不另建解码器。
    pub fn ensure_thumbnail(&self, request: ThumbnailRequest) -> Result<ThumbnailResponse, String> {
        self.runtime
            .write(move |state| state.ensure_thumbnail(request))
    }
}

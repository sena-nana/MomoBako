use crate::repository_runtime::RepositoryRuntime;
use crate::repository_service::{
    AssetDetail, EntryPlaybackProgressEvent, EntryPlaybackRequest, EntryPlaybackSourceResponse,
    FilePreviewSourceResponse, FileReadRequest, MetadataUpdateRequest, MetadataUpdateResponse,
    RepositorySnapshot, RepositorySummary, SearchRequest, SearchResponse,
};
use std::path::PathBuf;
use tauri::ipc::Channel;

#[derive(Clone)]
pub struct RepositoryQueryViewModel {
    runtime: RepositoryRuntime,
}

impl RepositoryQueryViewModel {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub async fn list_repositories(&self) -> Result<Vec<RepositorySummary>, String> {
        self.runtime
            .run_read(|state| state.list_repositories())
            .await
    }

    pub async fn get_repository_snapshot(
        &self,
        repo_id: String,
    ) -> Result<RepositorySnapshot, String> {
        self.runtime
            .run_read(move |state| state.load_snapshot(&repo_id))
            .await
    }

    pub async fn get_asset_detail(
        &self,
        repo_id: String,
        asset_id: String,
    ) -> Result<AssetDetail, String> {
        self.runtime
            .run_read(move |state| state.load_asset_detail(&repo_id, &asset_id))
            .await
    }

    pub async fn search_assets(&self, request: SearchRequest) -> Result<SearchResponse, String> {
        self.runtime
            .run_read(move |state| state.search_assets(request))
            .await
    }

    pub async fn update_asset_metadata(
        &self,
        request: MetadataUpdateRequest,
    ) -> Result<MetadataUpdateResponse, String> {
        self.runtime
            .run_write(move |state| state.update_asset_metadata(request))
            .await
    }

    pub async fn read_file(&self, request: FileReadRequest) -> Result<Vec<u8>, String> {
        self.runtime
            .run_read(move |state| state.read_file(request))
            .await
    }

    pub async fn prepare_preview_file_source(
        &self,
        request: FileReadRequest,
    ) -> Result<FilePreviewSourceResponse, String> {
        self.runtime
            .run_read(move |state| state.prepare_preview_file_source(request))
            .await
    }

    pub async fn prepare_entry_playback_source(
        &self,
        request: EntryPlaybackRequest,
    ) -> Result<EntryPlaybackSourceResponse, String> {
        let mut response = self
            .runtime
            .run_read(move |state| state.prepare_entry_playback_source(request))
            .await?;
        self.attach_playback_preview_urls(&mut response).await?;
        Ok(response)
    }

    pub async fn prepare_entry_playback_source_with_progress(
        &self,
        request: EntryPlaybackRequest,
        progress: Channel<EntryPlaybackProgressEvent>,
    ) -> Result<EntryPlaybackSourceResponse, String> {
        let mut emit = move |event: EntryPlaybackProgressEvent| {
            progress.send(event).map_err(|error| error.to_string())
        };
        let mut response = self
            .runtime
            .run_read(move |state| {
                state.prepare_entry_playback_source_with_progress(request, &mut emit)
            })
            .await?;
        self.attach_playback_preview_urls(&mut response).await?;
        Ok(response)
    }

    async fn attach_playback_preview_urls(
        &self,
        response: &mut EntryPlaybackSourceResponse,
    ) -> Result<(), String> {
        if response.source_url.is_none() {
            let source_path = response
                .local_path
                .as_deref()
                .or(response.temp_file_path.as_deref())
                .map(PathBuf::from);
            if let Some(source_path) = source_path {
                let media_type = response.media_type.clone();
                let token = self
                    .runtime
                    .run_read(move |state| {
                        state.register_preview_source_path(source_path, &media_type)
                    })
                    .await?;
                response.source_url = Some(self.runtime.preview_source_url(&token));
            }
        }
        if response.lyric_source_url.is_none() {
            if let Some(lyric_path) = response.lyric_path.as_deref().map(PathBuf::from) {
                let token = self
                    .runtime
                    .run_read(move |state| {
                        state.register_preview_source_path(lyric_path, "text/plain; charset=utf-8")
                    })
                    .await?;
                response.lyric_source_url = Some(self.runtime.preview_source_url(&token));
            }
        }
        if response.word_lyric_source_url.is_none() {
            if let Some(word_lyric_path) = response.word_lyric_path.as_deref().map(PathBuf::from) {
                let token = self
                    .runtime
                    .run_read(move |state| {
                        state.register_preview_source_path(
                            word_lyric_path,
                            "text/plain; charset=utf-8",
                        )
                    })
                    .await?;
                response.word_lyric_source_url = Some(self.runtime.preview_source_url(&token));
            }
        }
        Ok(())
    }
}

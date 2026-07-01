//! Playback command orchestration for entry playback and playlist downloads.

use crate::services::repository::{
    DownloaderPlaylistProgressEvent, DownloaderPlaylistRequest, EntryPlaybackProgressEvent,
    EntryPlaybackRequest, EntryPlaybackSourceResponse,
};
use crate::services::runtime::RepositoryRuntime;
use std::path::PathBuf;
use tauri::ipc::Channel;

#[derive(Clone)]
pub struct RepositoryPlaybackViewModel {
    runtime: RepositoryRuntime,
}

impl RepositoryPlaybackViewModel {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    /// Prepares repository entry playback and attaches preview URLs when the backend returns files.
    pub async fn prepare_entry_playback_source(
        &self,
        request: EntryPlaybackRequest,
    ) -> Result<EntryPlaybackSourceResponse, String> {
        let mut response = self
            .runtime
            .run_write(move |state| state.prepare_entry_playback_source(request))
            .await?;
        self.attach_playback_preview_urls(&mut response).await?;
        Ok(response)
    }

    /// Mirrors the playback preparation flow while forwarding backend progress over the Tauri channel.
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
            .run_write(move |state| {
                state.prepare_entry_playback_source_with_progress(request, &mut emit)
            })
            .await?;
        self.attach_playback_preview_urls(&mut response).await?;
        Ok(response)
    }

    /// Downloads a playlist through the downloader backend and forwards per-track progress updates.
    pub async fn download_playlist_with_progress(
        &self,
        request: DownloaderPlaylistRequest,
        progress: Channel<DownloaderPlaylistProgressEvent>,
    ) -> Result<serde_json::Value, String> {
        let service_root = self.runtime.service_root();
        let mut emit = move |event: DownloaderPlaylistProgressEvent| {
            progress.send(event).map_err(|error| error.to_string())
        };
        crate::services::repository::download_playlist_with_progress(
            &service_root,
            request,
            &mut emit,
        )
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

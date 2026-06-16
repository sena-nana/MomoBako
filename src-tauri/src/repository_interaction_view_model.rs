use crate::repository_runtime::RepositoryRuntime;
use crate::repository_service::{
    HardlinkCandidateResponse, HardlinkConfirmRequest, HardlinkConfirmResponse, PlaylistDetail,
    PlaylistItemRemoveRequest, PlaylistItemsAddRequest, PlaylistItemsByPathsAddRequest,
    PlaylistItemsOrderRequest, PlaylistMembershipIndex, PlaylistMembershipRequest,
    PlaylistMembershipSnapshot, PlaylistMutationRequest, PlaylistMutationResponse, PlaylistSummary,
    RepositoryAction, RepositoryActionEnabledRequest, RepositoryActionMutationResponse,
    RepositoryActionRunRequest, RepositoryActionRunResponse, RevisionActionRequest,
    RevisionActionResponse, SmartFolderMutationRequest, SmartFolderMutationResponse,
    SmartFolderResultSnapshot, SmartFolderTreeNode, SmartFolderUpdateRequest, ThumbnailRequest,
    ThumbnailResponse,
};

#[derive(Clone)]
pub struct RepositoryInteractionViewModel {
    runtime: RepositoryRuntime,
}

impl RepositoryInteractionViewModel {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub async fn list_smart_folders(
        &self,
        repo_id: String,
    ) -> Result<Vec<SmartFolderTreeNode>, String> {
        self.runtime
            .run_read(move |state| state.list_smart_folders(&repo_id))
            .await
    }

    pub async fn create_smart_folder(
        &self,
        request: SmartFolderMutationRequest,
    ) -> Result<SmartFolderMutationResponse, String> {
        self.runtime
            .run_write(move |state| state.create_smart_folder(request))
            .await
    }

    pub async fn update_smart_folder(
        &self,
        request: SmartFolderUpdateRequest,
    ) -> Result<SmartFolderMutationResponse, String> {
        self.runtime
            .run_write(move |state| state.update_smart_folder(request))
            .await
    }

    pub async fn delete_smart_folder(
        &self,
        repo_id: String,
        smart_folder_id: String,
    ) -> Result<SmartFolderMutationResponse, String> {
        self.runtime
            .run_write(move |state| state.delete_smart_folder(&repo_id, &smart_folder_id))
            .await
    }

    pub async fn query_smart_folder(
        &self,
        repo_id: String,
        smart_folder_id: String,
    ) -> Result<SmartFolderResultSnapshot, String> {
        self.runtime
            .run_read(move |state| state.query_smart_folder(&repo_id, &smart_folder_id))
            .await
    }

    pub async fn list_playlists(&self, repo_id: String) -> Result<Vec<PlaylistSummary>, String> {
        self.runtime
            .run_read(move |state| state.list_playlists(&repo_id))
            .await
    }

    pub async fn list_playlist_memberships(
        &self,
        repo_id: String,
    ) -> Result<PlaylistMembershipIndex, String> {
        self.runtime
            .run_read(move |state| state.list_playlist_memberships(&repo_id))
            .await
    }

    pub async fn create_playlist(
        &self,
        request: PlaylistMutationRequest,
    ) -> Result<PlaylistMutationResponse, String> {
        self.runtime
            .run_write(move |state| state.create_playlist(request))
            .await
    }

    pub async fn update_playlist(
        &self,
        request: crate::repository_service::PlaylistUpdateRequest,
    ) -> Result<PlaylistMutationResponse, String> {
        self.runtime
            .run_write(move |state| state.update_playlist(request))
            .await
    }

    pub async fn delete_playlist(
        &self,
        repo_id: String,
        playlist_id: String,
    ) -> Result<PlaylistMutationResponse, String> {
        self.runtime
            .run_write(move |state| state.delete_playlist(&repo_id, &playlist_id))
            .await
    }

    pub async fn get_playlist_detail(
        &self,
        repo_id: String,
        playlist_id: String,
    ) -> Result<PlaylistDetail, String> {
        self.runtime
            .run_read(move |state| state.get_playlist_detail(&repo_id, &playlist_id))
            .await
    }

    pub async fn add_playlist_items(
        &self,
        request: PlaylistItemsAddRequest,
    ) -> Result<PlaylistDetail, String> {
        self.runtime
            .run_write(move |state| state.add_playlist_items(request))
            .await
    }

    pub async fn add_playlist_items_by_paths(
        &self,
        request: PlaylistItemsByPathsAddRequest,
    ) -> Result<PlaylistDetail, String> {
        self.runtime
            .run_write(move |state| state.add_playlist_items_by_paths(request))
            .await
    }

    pub async fn reorder_playlist_items(
        &self,
        request: PlaylistItemsOrderRequest,
    ) -> Result<PlaylistDetail, String> {
        self.runtime
            .run_write(move |state| state.reorder_playlist_items(request))
            .await
    }

    pub async fn remove_playlist_item(
        &self,
        request: PlaylistItemRemoveRequest,
    ) -> Result<PlaylistDetail, String> {
        self.runtime
            .run_write(move |state| state.remove_playlist_item(request))
            .await
    }

    pub async fn set_playlist_membership(
        &self,
        request: PlaylistMembershipRequest,
    ) -> Result<PlaylistMembershipSnapshot, String> {
        self.runtime
            .run_write(move |state| state.set_playlist_membership(request))
            .await
    }

    pub async fn list_repository_actions(
        &self,
        repo_id: String,
    ) -> Result<Vec<RepositoryAction>, String> {
        self.runtime
            .run_read(move |state| state.list_repository_actions(&repo_id))
            .await
    }

    pub async fn get_repository_action(
        &self,
        repo_id: String,
        action_id: String,
    ) -> Result<RepositoryAction, String> {
        self.runtime
            .run_read(move |state| state.get_repository_action(&repo_id, &action_id))
            .await
    }

    pub async fn set_repository_action_enabled(
        &self,
        request: RepositoryActionEnabledRequest,
    ) -> Result<RepositoryActionMutationResponse, String> {
        self.runtime
            .run_write(move |state| state.set_repository_action_enabled(request))
            .await
    }

    pub async fn run_repository_action(
        &self,
        request: RepositoryActionRunRequest,
    ) -> Result<RepositoryActionRunResponse, String> {
        self.runtime
            .run_write(move |state| state.run_repository_action(request))
            .await
    }

    pub async fn list_hardlink_candidates(
        &self,
        repo_id: String,
    ) -> Result<HardlinkCandidateResponse, String> {
        self.runtime
            .run_read(move |state| state.list_hardlink_candidates(&repo_id))
            .await
    }

    pub async fn confirm_hardlink_candidate(
        &self,
        request: HardlinkConfirmRequest,
    ) -> Result<HardlinkConfirmResponse, String> {
        self.runtime
            .run_write(move |state| state.confirm_hardlink_candidate(request))
            .await
    }

    pub async fn ensure_thumbnail(
        &self,
        request: ThumbnailRequest,
    ) -> Result<ThumbnailResponse, String> {
        self.runtime
            .run_write(move |state| state.ensure_thumbnail(request))
            .await
    }

    pub async fn undo_last_revision(
        &self,
        request: RevisionActionRequest,
    ) -> Result<RevisionActionResponse, String> {
        self.runtime
            .run_write(move |state| state.undo_last_revision(request))
            .await
    }

    pub async fn redo_last_revision(
        &self,
        request: RevisionActionRequest,
    ) -> Result<RevisionActionResponse, String> {
        self.runtime
            .run_write(move |state| state.redo_last_revision(request))
            .await
    }
}

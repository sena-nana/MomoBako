//! Orchestration for repository interactions such as playlists, smart folders, and revisions.

use crate::services::repository::{
    EntryAccessRecordRequest, EntryAccessRecordResponse, HardlinkCandidateResponse,
    HardlinkConfirmRequest, HardlinkConfirmResponse, PlaylistDetail, PlaylistItemRemoveRequest,
    PlaylistItemsByPathsAddRequest, PlaylistItemsOrderRequest, PlaylistMembershipIndex,
    PlaylistMembershipRequest, PlaylistMembershipSnapshot, PlaylistMutationRequest,
    PlaylistMutationResponse, PlaylistSummary, RecentAccessHistoryClearRequest,
    RecentAccessHistoryClearResponse, RepositoryAction, RepositoryActionEnabledRequest,
    RepositoryActionMutationResponse, RevisionActionRequest, RevisionActionResponse,
    SmartFolderMutationRequest, SmartFolderMutationResponse, SmartFolderResultSnapshot,
    SmartFolderTreeNode, SmartFolderUpdateRequest,
};
use crate::services::runtime::RepositoryRuntime;

#[derive(Clone)]
pub struct RepositoryInteractionService {
    runtime: RepositoryRuntime,
}

impl RepositoryInteractionService {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub fn list_smart_folders(&self, repo_id: String) -> Result<Vec<SmartFolderTreeNode>, String> {
        self.runtime
            .read(move |state| state.list_smart_folders(&repo_id))
    }

    pub fn create_smart_folder(
        &self,
        request: SmartFolderMutationRequest,
    ) -> Result<SmartFolderMutationResponse, String> {
        self.runtime
            .write(move |state| state.create_smart_folder(request))
    }

    pub fn update_smart_folder(
        &self,
        request: SmartFolderUpdateRequest,
    ) -> Result<SmartFolderMutationResponse, String> {
        self.runtime
            .write(move |state| state.update_smart_folder(request))
    }

    pub fn delete_smart_folder(
        &self,
        repo_id: String,
        smart_folder_id: String,
    ) -> Result<SmartFolderMutationResponse, String> {
        self.runtime
            .write(move |state| state.delete_smart_folder(&repo_id, &smart_folder_id))
    }

    pub fn query_smart_folder(
        &self,
        repo_id: String,
        smart_folder_id: String,
    ) -> Result<SmartFolderResultSnapshot, String> {
        self.runtime
            .read(move |state| state.query_smart_folder(&repo_id, &smart_folder_id))
    }

    pub fn list_playlists(&self, repo_id: String) -> Result<Vec<PlaylistSummary>, String> {
        self.runtime
            .read(move |state| state.list_playlists(&repo_id))
    }

    pub fn list_playlist_memberships(
        &self,
        repo_id: String,
    ) -> Result<PlaylistMembershipIndex, String> {
        self.runtime
            .read(move |state| state.list_playlist_memberships(&repo_id))
    }

    pub fn create_playlist(
        &self,
        request: PlaylistMutationRequest,
    ) -> Result<PlaylistMutationResponse, String> {
        self.runtime
            .write(move |state| state.create_playlist(request))
    }

    pub fn delete_playlist(
        &self,
        repo_id: String,
        playlist_id: String,
    ) -> Result<PlaylistMutationResponse, String> {
        self.runtime
            .write(move |state| state.delete_playlist(&repo_id, &playlist_id))
    }

    pub fn get_playlist_detail(
        &self,
        repo_id: String,
        playlist_id: String,
    ) -> Result<PlaylistDetail, String> {
        self.runtime
            .read(move |state| state.get_playlist_detail(&repo_id, &playlist_id))
    }

    pub fn add_playlist_items_by_paths(
        &self,
        request: PlaylistItemsByPathsAddRequest,
    ) -> Result<PlaylistDetail, String> {
        self.runtime
            .write(move |state| state.add_playlist_items_by_paths(request))
    }

    pub fn reorder_playlist_items(
        &self,
        request: PlaylistItemsOrderRequest,
    ) -> Result<PlaylistDetail, String> {
        self.runtime
            .write(move |state| state.reorder_playlist_items(request))
    }

    pub fn remove_playlist_item(
        &self,
        request: PlaylistItemRemoveRequest,
    ) -> Result<PlaylistDetail, String> {
        self.runtime
            .write(move |state| state.remove_playlist_item(request))
    }

    pub fn set_playlist_membership(
        &self,
        request: PlaylistMembershipRequest,
    ) -> Result<PlaylistMembershipSnapshot, String> {
        self.runtime
            .write(move |state| state.set_playlist_membership(request))
    }

    pub fn list_repository_actions(
        &self,
        repo_id: String,
    ) -> Result<Vec<RepositoryAction>, String> {
        self.runtime
            .read(move |state| state.list_repository_actions(&repo_id))
    }

    pub fn get_repository_action(
        &self,
        repo_id: String,
        action_id: String,
    ) -> Result<RepositoryAction, String> {
        self.runtime
            .read(move |state| state.get_repository_action(&repo_id, &action_id))
    }

    pub fn set_repository_action_enabled(
        &self,
        request: RepositoryActionEnabledRequest,
    ) -> Result<RepositoryActionMutationResponse, String> {
        self.runtime
            .write(move |state| state.set_repository_action_enabled(request))
    }

    pub fn list_hardlink_candidates(
        &self,
        repo_id: String,
    ) -> Result<HardlinkCandidateResponse, String> {
        self.runtime
            .read(move |state| state.list_hardlink_candidates(&repo_id))
    }

    pub fn confirm_hardlink_candidate(
        &self,
        request: HardlinkConfirmRequest,
    ) -> Result<HardlinkConfirmResponse, String> {
        self.runtime
            .write(move |state| state.confirm_hardlink_candidate(request))
    }

    pub fn record_entry_access(
        &self,
        request: EntryAccessRecordRequest,
    ) -> Result<EntryAccessRecordResponse, String> {
        self.runtime
            .write(move |state| state.record_entry_access(request))
    }

    pub fn clear_recent_access_history(
        &self,
        request: RecentAccessHistoryClearRequest,
    ) -> Result<RecentAccessHistoryClearResponse, String> {
        self.runtime
            .write(move |state| state.clear_recent_access_history(request))
    }

    pub fn undo_last_revision(
        &self,
        request: RevisionActionRequest,
    ) -> Result<RevisionActionResponse, String> {
        self.runtime
            .write(move |state| state.undo_last_revision(request))
    }

    pub fn redo_last_revision(
        &self,
        request: RevisionActionRequest,
    ) -> Result<RevisionActionResponse, String> {
        self.runtime
            .write(move |state| state.redo_last_revision(request))
    }
}

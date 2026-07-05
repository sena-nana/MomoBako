<script setup lang="ts">
import { computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  useWorkspaceActions,
  useWorkspaceFiles,
  useWorkspaceNavigation,
  useWorkspacePlaylists,
  useWorkspaceProgress,
  useWorkspaceRepository,
  useWorkspaceSelection,
  useWorkspaceSmartFolders,
} from "../composables/useRepositoryWorkspace";
import { usePlaylistPlayer } from "../composables/usePlaylistPlayer";
import { useFolderSidebarUi } from "./useFolderSidebarUi";
import { useRepositorySwitcherUi } from "./useRepositorySwitcherUi";
import { useWorkspaceSidebarShellUi } from "./useWorkspaceSidebarShellUi";
import { useSidebarShortcutsUi } from "./useSidebarShortcutsUi";
import { useSmartFolderSidebarUi } from "./useSmartFolderSidebarUi";
import { usePlaylistSidebarUi } from "./usePlaylistSidebarUi";
import WorkspaceSidebarRepoHeader from "./WorkspaceSidebarRepoHeader.vue";
import WorkspaceSidebarShortcuts from "./WorkspaceSidebarShortcuts.vue";
import WorkspaceSidebarPlaylists from "./WorkspaceSidebarPlaylists.vue";
import WorkspaceSidebarFolders from "./WorkspaceSidebarFolders.vue";
import WorkspaceSidebarSmartFolders from "./WorkspaceSidebarSmartFolders.vue";
import WorkspaceSidebarStatus from "./WorkspaceSidebarStatus.vue";
import WorkspaceSidebarFooter from "./WorkspaceSidebarFooter.vue";
import RepositoryDeleteDialog from "./RepositoryDeleteDialog.vue";
import RepositorySwitcherPopover from "./RepositorySwitcherPopover.vue";
import WorkspaceSidebarFolderDialogs from "./WorkspaceSidebarFolderDialogs.vue";
import WorkspaceSidebarPlaylistDialog from "./WorkspaceSidebarPlaylistDialog.vue";
import WorkspaceSidebarSmartFolderDialogs from "./WorkspaceSidebarSmartFolderDialogs.vue";

const NETEASE_SOURCE_PLUGIN_ID = "momobako.source.netease-cloud-music";

const route = useRoute();
const router = useRouter();
const playlistPlayer = usePlaylistPlayer();

const {
  repositories,
  activeRepository,
  repositoryBackendOptions,
  activeRepoId,
  activeSnapshot,
  canDeletePendingRepositoryFolder,
  canDeletePendingRepositoryMetadata,
  closeRepositoryDeleteDialog,
  confirmRepositoryDelete,
  refreshActiveRepositoryWorkspaceSilently,
  isDeletingRepository,
  openRepositoryDeleteDialog,
  pendingDeleteRepository,
  selectRepository,
  createNewRepository,
  attachRepository,
  repositoryDeleteDialogError,
  repositoryDeleteDialogVisible,
} = useWorkspaceRepository();
const {
  activePanel,
  activeLibraryCategory,
  setActiveLibraryCategory,
  setActivePanel,
} = useWorkspaceNavigation();
const {
  currentDirectoryPath,
  fileTree,
  refreshFileBrowserTree,
  loadFileBrowserForDirectory,
  createDirectoryInWorkspace,
  importEntriesToWorkspace,
  moveWorkspaceEntries,
  renameWorkspaceEntry,
  deleteWorkspaceEntry,
} = useWorkspaceFiles();
const {
  dragHoverFolderPath,
  draggedWorkspacePaths,
  isExternalDragActive,
  isInternalDragActive,
  selectWorkspaceEntry,
  clearDraggedWorkspaceState,
  setDragHoverFolderPath,
} = useWorkspaceSelection();
const {
  activePlaylistId,
  activePlaylistDetail,
  playlists,
  refreshPlaylists,
  selectPlaylist,
  createPlaylistInWorkspace,
  deletePlaylistInWorkspace,
} = useWorkspacePlaylists();
const {
  activeSmartFolderId,
  smartFolders,
  selectSmartFolder,
  createSmartFolderInWorkspace,
  updateSmartFolderInWorkspace,
  deleteSmartFolderInWorkspace,
} = useWorkspaceSmartFolders();
const {
  repositoryActions,
} = useWorkspaceActions();
const {
  syncProgress,
  isBusy,
  isLoadingFileBrowser,
  isMutatingFiles,
  isMutatingSmartFolder,
  error,
} = useWorkspaceProgress();

const {
  isActiveRepositoryMissing,
  isShowingSyncProgress,
} = useWorkspaceSidebarShellUi({
  activeRepository,
  syncProgress,
});
const {
  addRepositoryError,
  addRepositoryPopoverMode,
  addRepositoryPopoverPosition,
  addRepositoryPopoverRef,
  backendName,
  backendOptions,
  backendPassword,
  backendRoot,
  backendSubmitDisabled,
  backendUrl,
  backendUsername,
  closeAddRepositoryPopover,
  deleteActiveRepositoryFromMenu,
  isSubmittingBackend,
  neteaseLoginMessage,
  neteaseQrSession,
  neteaseCachePath,
  chooseNeteaseCacheFolder,
  openRepositorySwitcherFromEvent,
  pollNeteaseQrSession,
  createNeteaseQrSession,
  repositorySwitcherButtonRef,
  selectedBackend,
  selectBackend,
  selectRepositoryFromList,
  showAddRepositoryMenuFromSwitcher,
  submitAddRepositoryForm,
} = useRepositorySwitcherUi({
  activeRepoId,
  attachRepository,
  createNewRepository,
  openRepositoryDeleteDialog,
  repositories,
  repositoryBackendOptions,
  refreshRepositoryWorkspaceSilently: refreshActiveRepositoryWorkspaceSilently,
  route,
  router,
  selectRepository,
});
const {
  closeSmartFolderDeleteDialog,
  closeSmartFolderDialog,
  confirmDeleteSmartFolder,
  expandedSmartFolderIdSet,
  flatSmartFolders,
  openCreateSmartFolderDialog,
  openDeleteSmartFolderDialog,
  openEditSmartFolderDialog,
  pendingDeleteSmartFolderLabel,
  showSmartFolderDeleteDialog,
  showSmartFolderDialog,
  smartFolderColors,
  smartFolderDateFilters,
  smartFolderDialogActionLabel,
  smartFolderDialogDisabled,
  smartFolderDialogTitle,
  smartFolderExcludeDateFilters,
  smartFolderExcludeFormats,
  smartFolderExcludeMetadataFilters,
  smartFolderExcludeNumberFilters,
  smartFolderExcludePathPrefixes,
  smartFolderExcludeQuery,
  smartFolderExcludeTags,
  smartFolderFormats,
  smartFolderLimit,
  smartFolderMatchMode,
  smartFolderMetadataFilters,
  smartFolderMinRating,
  smartFolderName,
  smartFolderNumberFilters,
  smartFolderParentId,
  smartFolderPathPrefix,
  smartFolderQuery,
  smartFolderShapes,
  smartFolderSortDirection,
  smartFolderSortField,
  smartFolderTags,
  smartFolderTargetId,
  submitSmartFolderDialog,
  toggleSmartFolderExpansion,
} = useSmartFolderSidebarUi({
  activeSmartFolderId,
  createSmartFolderInWorkspace,
  deleteSmartFolderInWorkspace,
  isMutatingSmartFolder,
  smartFolders,
  updateSmartFolderInWorkspace,
});
const {
  activePlaylist,
  availablePlaylistPlayers,
  availablePlaylistPlayerTypeIds,
  closePlaylistDialog,
  openPlaylist,
  openPlaylistDialog,
  playlistDialogDisabled,
  playlistItems,
  playlistsExpanded,
  playlistName,
  playlistPlayerTypeId,
  playPlaylist,
  removePlaylist,
  showPlaylistDialog,
  submitPlaylistDialog,
  togglePlaylistsExpanded,
} = usePlaylistSidebarUi({
  activePlaylistDetail,
  activePlaylistId,
  activeRepoId,
  createPlaylistInWorkspace,
  deletePlaylistInWorkspace,
  isActiveRepositoryMissing,
  playlistPlayer,
  playlists,
  refreshPlaylists,
  route,
  router,
  selectPlaylist,
});
const {
  closeDeleteFolderDialog,
  closeFolderDialog,
  confirmDeleteFolder,
  expandedFolderPathSet,
  fileTreeNodes,
  folderDialogActionLabel,
  folderDialogDisabled,
  folderDialogLabel,
  folderDialogMode,
  folderDialogParentPath,
  folderDialogPlaceholder,
  folderDialogTitle,
  folderDialogValue,
  handleFolderDragHover,
  handleFolderDragLeave,
  handleFolderDrop,
  isFolderDragActive,
  isTrashPanel,
  openCreateFolderDialog,
  openDeleteFolderDialog,
  openFolder,
  openRenameFolderDialog,
  pendingDeleteFolderLabel,
  showFolderDeleteDialog,
  showFolderDialog,
  submitFolderDialog,
  toggleFolderExpansion,
} = useFolderSidebarUi({
  activePanel,
  activeRepoId,
  clearDraggedWorkspaceState,
  createDirectoryInWorkspace,
  currentDirectoryPath,
  deleteWorkspaceEntry,
  dragHoverFolderPath,
  draggedWorkspacePaths,
  fileTree,
  importEntriesToWorkspace,
  isExternalDragActive,
  isInternalDragActive,
  isMutatingFiles,
  loadFileBrowserForDirectory,
  moveWorkspaceEntries,
  renameWorkspaceEntry,
  setActivePanel,
  setActiveLibraryCategory,
  setDragHoverFolderPath,
});
const {
  openSmartFolder,
  openQuickAccess,
  quickAccess,
  selectPanel,
  selectShortcut,
  shortcutIcon,
  shortcuts,
} = useSidebarShortcutsUi({
  activeSnapshot,
  isActiveRepositoryMissing,
  loadFileBrowserForDirectory,
  route,
  router,
  selectSmartFolder,
  selectWorkspaceEntry,
  setActiveLibraryCategory,
  setActivePanel,
});

const showFolderSidebar = computed(() => (
  activeSnapshot.value?.repository.backend.pluginId !== NETEASE_SOURCE_PLUGIN_ID
));

</script>

<template>
  <aside class="secondary-panel secondary-panel--workspace">
    <div class="workspace-sidebar">
      <WorkspaceSidebarRepoHeader
        v-model:repository-switcher-button-ref="repositorySwitcherButtonRef"
        :active-repository="activeRepository"
        :is-switcher-open="addRepositoryPopoverMode === 'switcher'"
        @open-switcher="openRepositorySwitcherFromEvent"
      />

      <section class="workspace-sidebar__files" aria-label="文件管理">
        <WorkspaceSidebarStatus
          :error="error"
          :is-busy="isBusy"
          :is-showing-sync-progress="isShowingSyncProgress"
          :sync-progress="syncProgress"
        />

        <WorkspaceSidebarShortcuts
          :active-panel="activePanel"
          :active-library-category="activeLibraryCategory"
          :is-active-repository-missing="isActiveRepositoryMissing"
          :quick-access="quickAccess"
          :repository-actions-count="repositoryActions.length"
          :shortcut-icon="shortcutIcon"
          :shortcuts="shortcuts"
          @open-quick-access="openQuickAccess"
          @select-actions="setActivePanel('actions')"
          @select-shortcut="selectShortcut"
        />

        <WorkspaceSidebarPlaylists
          :active-panel="activePanel"
          :active-playlist-id="activePlaylist?.playlistId"
          :active-repo-id="activeRepoId"
          :available-playlist-player-type-ids="availablePlaylistPlayerTypeIds"
          :available-playlist-players-count="availablePlaylistPlayers.length"
          :is-active-repository-missing="isActiveRepositoryMissing"
          :playlist-items="playlistItems"
          :playlists-expanded="playlistsExpanded"
          @create="openPlaylistDialog"
          @open="openPlaylist"
          @play="playPlaylist"
          @remove="removePlaylist"
          @toggle-expanded="togglePlaylistsExpanded"
        />

        <WorkspaceSidebarFolders
          v-if="showFolderSidebar"
          :active-repo-id="activeRepoId"
          :current-directory-path="currentDirectoryPath"
          :drag-hover-folder-path="dragHoverFolderPath"
          :expanded-folder-path-set="expandedFolderPathSet"
          :file-tree-nodes="fileTreeNodes"
          :is-active-repository-missing="isActiveRepositoryMissing"
          :is-folder-drag-active="isFolderDragActive"
          :is-loading-file-browser="isLoadingFileBrowser"
          :is-mutating-files="isMutatingFiles"
          :is-trash-panel="isTrashPanel"
          @create="openCreateFolderDialog"
          @delete="openDeleteFolderDialog"
          @drop-folder="handleFolderDrop"
          @hover-folder="handleFolderDragHover"
          @leave-folder="handleFolderDragLeave"
          @open="openFolder"
          @refresh="refreshFileBrowserTree"
          @rename="openRenameFolderDialog"
          @toggle="toggleFolderExpansion"
        />

        <WorkspaceSidebarSmartFolders
          :active-repo-id="activeRepoId"
          :active-smart-folder-id="activeSmartFolderId"
          :expanded-smart-folder-id-set="expandedSmartFolderIdSet"
          :is-active-repository-missing="isActiveRepositoryMissing"
          :is-mutating-smart-folder="isMutatingSmartFolder"
          :smart-folders="smartFolders"
          @create="openCreateSmartFolderDialog"
          @delete="openDeleteSmartFolderDialog"
          @edit="openEditSmartFolderDialog"
          @open="openSmartFolder"
          @toggle="toggleSmartFolderExpansion"
        />
      </section>

      <WorkspaceSidebarFooter
        :active-panel="activePanel"
        :is-settings-route="route.path === '/settings'"
        @select-extensions="selectPanel('extensions')"
      />
    </div>
  </aside>

  <RepositorySwitcherPopover
    v-model:backend-name="backendName"
    v-model:backend-password="backendPassword"
    v-model:backend-root="backendRoot"
    v-model:backend-url="backendUrl"
    v-model:backend-username="backendUsername"
    v-model:mode="addRepositoryPopoverMode"
    :active-repo-id="activeRepoId"
    :add-repository-error="addRepositoryError"
    :backend-options="backendOptions"
    :backend-submit-disabled="backendSubmitDisabled"
    :is-submitting-backend="isSubmittingBackend"
    :netease-login-message="neteaseLoginMessage"
    :netease-qr-session="neteaseQrSession"
    :netease-cache-path="neteaseCachePath"
    :position="addRepositoryPopoverPosition"
    :repositories="repositories"
    :selected-backend="selectedBackend"
    @close="closeAddRepositoryPopover"
    @choose-netease-cache-folder="chooseNeteaseCacheFolder"
    @create-netease-qr-session="createNeteaseQrSession"
    @delete-active="deleteActiveRepositoryFromMenu"
    @poll-netease-qr-session="pollNeteaseQrSession"
    @select-backend="selectBackend"
    @select-repository="selectRepositoryFromList"
    @set-popover-ref="(element) => { addRepositoryPopoverRef = element; }"
    @show-add-menu="showAddRepositoryMenuFromSwitcher"
    @submit="submitAddRepositoryForm"
  />

  <RepositoryDeleteDialog
    :open="repositoryDeleteDialogVisible"
    :repository="pendingDeleteRepository"
    :error="repositoryDeleteDialogError"
    :is-deleting="isDeletingRepository"
    :can-delete-metadata="canDeletePendingRepositoryMetadata"
    :can-delete-folder="canDeletePendingRepositoryFolder"
    @close="closeRepositoryDeleteDialog"
    @confirm="confirmRepositoryDelete"
  />

  <WorkspaceSidebarPlaylistDialog
    v-model:playlist-name="playlistName"
    v-model:playlist-player-type-id="playlistPlayerTypeId"
    :available-playlist-players="availablePlaylistPlayers"
    :playlist-dialog-disabled="playlistDialogDisabled"
    :show-playlist-dialog="showPlaylistDialog"
    @close-playlist-dialog="closePlaylistDialog"
    @submit-playlist-dialog="submitPlaylistDialog"
  />

  <WorkspaceSidebarFolderDialogs
    v-model:folder-dialog-value="folderDialogValue"
    :folder-dialog-action-label="folderDialogActionLabel"
    :folder-dialog-disabled="folderDialogDisabled"
    :folder-dialog-label="folderDialogLabel"
    :folder-dialog-mode="folderDialogMode"
    :folder-dialog-parent-path="folderDialogParentPath"
    :folder-dialog-placeholder="folderDialogPlaceholder"
    :folder-dialog-title="folderDialogTitle"
    :is-mutating-files="isMutatingFiles"
    :pending-delete-folder-label="pendingDeleteFolderLabel"
    :show-folder-delete-dialog="showFolderDeleteDialog"
    :show-folder-dialog="showFolderDialog"
    @close-folder-delete="closeDeleteFolderDialog"
    @close-folder-dialog="closeFolderDialog"
    @confirm-delete-folder="confirmDeleteFolder"
    @submit-folder-dialog="submitFolderDialog"
  />

  <WorkspaceSidebarSmartFolderDialogs
    v-model:smart-folder-colors="smartFolderColors"
    v-model:smart-folder-date-filters="smartFolderDateFilters"
    v-model:smart-folder-exclude-date-filters="smartFolderExcludeDateFilters"
    v-model:smart-folder-exclude-formats="smartFolderExcludeFormats"
    v-model:smart-folder-exclude-metadata-filters="smartFolderExcludeMetadataFilters"
    v-model:smart-folder-exclude-number-filters="smartFolderExcludeNumberFilters"
    v-model:smart-folder-exclude-path-prefixes="smartFolderExcludePathPrefixes"
    v-model:smart-folder-exclude-query="smartFolderExcludeQuery"
    v-model:smart-folder-exclude-tags="smartFolderExcludeTags"
    v-model:smart-folder-formats="smartFolderFormats"
    v-model:smart-folder-limit="smartFolderLimit"
    v-model:smart-folder-match-mode="smartFolderMatchMode"
    v-model:smart-folder-metadata-filters="smartFolderMetadataFilters"
    v-model:smart-folder-min-rating="smartFolderMinRating"
    v-model:smart-folder-name="smartFolderName"
    v-model:smart-folder-number-filters="smartFolderNumberFilters"
    v-model:smart-folder-parent-id="smartFolderParentId"
    v-model:smart-folder-path-prefix="smartFolderPathPrefix"
    v-model:smart-folder-query="smartFolderQuery"
    v-model:smart-folder-shapes="smartFolderShapes"
    v-model:smart-folder-sort-direction="smartFolderSortDirection"
    v-model:smart-folder-sort-field="smartFolderSortField"
    v-model:smart-folder-tags="smartFolderTags"
    :flat-smart-folders="flatSmartFolders"
    :is-mutating-smart-folder="isMutatingSmartFolder"
    :pending-delete-smart-folder-label="pendingDeleteSmartFolderLabel"
    :show-smart-folder-delete-dialog="showSmartFolderDeleteDialog"
    :show-smart-folder-dialog="showSmartFolderDialog"
    :smart-folder-dialog-action-label="smartFolderDialogActionLabel"
    :smart-folder-dialog-disabled="smartFolderDialogDisabled"
    :smart-folder-dialog-title="smartFolderDialogTitle"
    :smart-folder-target-id="smartFolderTargetId"
    @close-smart-folder-delete="closeSmartFolderDeleteDialog"
    @close-smart-folder-dialog="closeSmartFolderDialog"
    @confirm-delete-smart-folder="confirmDeleteSmartFolder"
    @submit-smart-folder-dialog="submitSmartFolderDialog"
  />
</template>

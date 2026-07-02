<script setup lang="ts">
import { ConfirmDialog } from "../ui/core";
import {
  CopyTargetDialog,
  ExtensionsPanel,
  HardlinkCandidateDialog,
  RepositoryActionsPanel,
  SearchPanel,
  WorkspaceFilterBar,
  WorkspaceFilesSurface,
  WorkspacePlaylistPage,
} from "./workspace/lazyComponents";
import MissingRepositoryState from "./workspace/MissingRepositoryState.vue";
import EmptyRepositoryState from "./workspace/EmptyRepositoryState.vue";
import { useWorkspaceHomeViewModel } from "./workspace/repository/useWorkspaceHomeViewModel";

const vm = useWorkspaceHomeViewModel();
</script>

<template>
  <div class="workspace-page">
  <WorkspaceFilterBar
    v-if="vm.hasRepository && vm.isFilterBarOpen"
    v-model:color-filter-input="vm.colorFilterInput"
    v-model:date-filters-input="vm.dateFiltersInput"
    v-model:exclude-date-filters-input="vm.excludeDateFiltersInput"
    v-model:exclude-formats-input="vm.excludeFormatsInput"
    v-model:exclude-metadata-filters-input="vm.excludeMetadataFiltersInput"
    v-model:exclude-number-filters-input="vm.excludeNumberFiltersInput"
    v-model:exclude-path-prefixes-input="vm.excludePathPrefixesInput"
    v-model:exclude-query-input="vm.excludeQueryInput"
    v-model:exclude-tags-input="vm.excludeTagsInput"
    v-model:limit-input="vm.limitInput"
    v-model:metadata-filters-input="vm.metadataFiltersInput"
    v-model:number-filters-input="vm.numberFiltersInput"
    v-model:shape-filter-input="vm.shapeFilterInput"
    v-model:sort-direction-input="vm.sortDirectionInput"
    v-model:sort-field-input="vm.sortFieldInput"
    :active-filter-count="vm.activeFilterCount"
    :active-library-search-shortcuts="vm.activeLibrarySearchShortcuts"
    :color-filter-options="vm.colorFilterOptions"
    :filters="vm.filters"
    :filter-color-style="vm.filterColorStyle"
    :format-filter-options="vm.formatFilterOptions"
    :has-active-filters="vm.hasActiveFilters"
    :rating-filter-options="vm.ratingFilterOptions"
    :repository-name="vm.activeSnapshot?.repository.name"
    :search-query="vm.searchQuery"
    :shape-filter-options="vm.shapeFilterOptions"
    :tag-filter-options="vm.tagFilterOptions"
    @apply-advanced-search-filters="vm.applyAdvancedSearchFilters"
    @apply-metadata-filter-shortcut="vm.applyMetadataFilterShortcut"
    @clear-search-filters="vm.clearSearchFilters"
    @close="vm.closeFilterBar"
    @select-minimum-rating="vm.selectMinimumRating"
    @submit-metadata-filter-input="vm.submitMetadataFilterInput"
    @toggle-search-filter="vm.toggleSearchFilter"
  />

  <div
    class="workspace-page__body"
    :class="{ 'workspace-page__body--fixed': vm.hasRepository && vm.isFileBrowserPanel }"
  >
    <div
      v-if="vm.activeNeteaseLoginExpired"
      class="asset-browser__state asset-browser__state--error workspace-page__notice"
    >
      <span>登录已失效，请重新登录后再刷新或播放。</span>
      <button type="button" class="ghost" :disabled="vm.isRefreshingNeteaseLogin" @click="vm.refreshActiveNeteaseLoginStatus">
        刷新状态
      </button>
      <button type="button" class="primary" :disabled="vm.isRefreshingNeteaseLogin" @click="vm.requestActiveNeteaseRelogin">
        重新登录
      </button>
    </div>

    <MissingRepositoryState
      v-if="vm.isMissingRepository"
      :active-repository="vm.activeRepository"
      :error="vm.missingRepositoryError"
      :is-busy="vm.isMissingRepositoryBusy"
      :is-deleting="vm.isDeletingMissingRepository"
      :is-repairing="vm.isRepairingMissingRepository"
      @choose-path="vm.chooseMissingRepositoryPath"
      @delete-repository="vm.openMissingRepositoryDeleteDialog"
      @refresh="vm.refreshMissingRepository"
    />

    <WorkspaceFilesSurface
      v-else-if="vm.hasRepository && vm.isFileBrowserPanel"
      v-model:create-file-name="vm.createFileName"
      v-model:file-display-mode="vm.fileDisplayMode"
      v-model:rename-value="vm.renameValue"
      v-bind="vm.filesSurfaceProps"
      v-on="vm.filesSurfaceHandlers"
    />

    <WorkspacePlaylistPage
      v-else-if="vm.hasRepository && vm.isPlaylistPanel"
      v-bind="vm.playlistPageProps"
      v-on="vm.playlistPageHandlers"
    />

    <SearchPanel
      v-else-if="vm.isSearchPanel"
      :is-searching="vm.isSearching"
      :repositories-count="vm.repositories.length"
      :results="vm.searchResults"
      :scope-label="vm.searchResultScopeLabel"
      :summary="vm.searchSummary"
      :result-context="vm.searchResultContext"
      @open-result="vm.openSearchHit"
    />

    <RepositoryActionsPanel
      v-else-if="vm.isActionsPanel"
      :actions="vm.repositoryActions"
      :active-action-id="vm.activeRepositoryActionId"
      :selected-count="vm.selectedFilePaths.length"
      :is-loading="vm.isLoadingRepositoryActions"
      :is-running="vm.isRunningRepositoryAction"
      @select="vm.selectRepositoryAction"
      @run="vm.runActiveRepositoryAction"
    />

    <ExtensionsPanel
      v-else-if="vm.isExtensionsPanel"
      :manifest="{
        pluginId: 'workspace.extensions',
        name: 'Workspace Extensions',
        version: '1.0.0',
        kind: 'workspace-extensions',
        description: 'Workspace extensions host.',
        capabilities: [],
        enabled: true,
      }"
      :active-repo-id="vm.activeRepoId"
      :active-repository="vm.activeRepository"
      :current-directory-path="vm.currentDirectoryPath"
      :is-repository-writable="vm.isRepositoryWritable"
      :is-trash-panel="vm.isTrashPanel"
      :is-virtual-view="vm.isVirtualView"
    />

    <EmptyRepositoryState
      v-else
      :error="vm.emptyRepositoryError"
      :is-dragging="vm.isDraggingRepositoryFolder"
      @drag-over="vm.handleEmptyRepositoryDragOver"
      @drag-leave="vm.handleEmptyRepositoryDragLeave"
      @drop="vm.handleEmptyRepositoryDrop"
    />
  </div>
  </div>

  <CopyTargetDialog
    v-if="vm.copyTargetDialogOpen"
    v-model:target-path="vm.copyTargetPath"
    :open="vm.copyTargetDialogOpen"
    :is-mutating="vm.isMutatingFiles"
    @cancel="vm.cancelCopyTarget"
    @submit="vm.submitCopyTarget"
  />

  <HardlinkCandidateDialog
    v-if="vm.currentHardlinkCandidate"
    :candidate="vm.currentHardlinkCandidate"
    :is-mutating="vm.isMutatingFiles"
    :message="vm.hardlinkCandidateMessage"
    @confirm="vm.confirmCurrentHardlinkCandidate"
    @skip="vm.skipCurrentHardlinkCandidate"
  />

  <ConfirmDialog
    :open="vm.showMissingRepositoryDeleteDialog"
    title="删除丢失资源库"
    message="会移除这条资源库注册记录并清理本机缓存，不会删除原路径中的用户文件。"
    confirm-text="删除"
    cancel-text="取消"
    busy-text="删除中..."
    :busy="vm.isDeletingMissingRepository"
    danger
    @confirm="vm.confirmMissingRepositoryDelete"
    @cancel="vm.closeMissingRepositoryDeleteDialog"
  />
  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="vm.entryActionRepositoryDialogOpen"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        :aria-label="vm.entryActionRepositoryDialogTitle"
        @click.self="vm.closeEntryActionRepositoryDialog()"
      >
        <div class="modal-card dialog-card repository-picker-dialog">
          <div class="dialog-card__header">
            <span>{{ vm.entryActionRepositoryDialogTitle }}</span>
          </div>
          <div class="dialog-card__body repository-picker-dialog__body">
            <button
              v-for="repository in vm.entryActionRepositoryDialogCandidates"
              :key="repository.repoId"
              type="button"
              class="repository-picker-dialog__item"
              @click="vm.closeEntryActionRepositoryDialog(repository)"
            >
              <strong>{{ repository.name }}</strong>
              <span>{{ repository.path }}</span>
            </button>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" @click="vm.closeEntryActionRepositoryDialog()">
              取消
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

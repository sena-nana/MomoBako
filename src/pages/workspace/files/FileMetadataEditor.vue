<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from "vue";
import { ChevronDown, ChevronRight, Copy, ExternalLink, Link2, MessageSquareText, Plus, Star } from "@lucide/vue";
import { openExternalUrl } from "../../../services/repositoryApi";
import type { RegisteredLibraryExtension } from "../../../plugins/sdk";
import type { FileBrowserEntry, RepositoryTagGroup } from "../../../types/repository";
import {
  formatBytes,
  formatMetadataDate,
  metadataComment,
  metadataNumber,
  metadataPalette,
  metadataRawNumber,
  metadataString,
  metadataTagGroups,
} from "../../../utils/fileMetadata";

const props = defineProps<{
  entry: FileBrowserEntry;
  isSaving: boolean;
  availableTags: string[];
  tagGroups?: RepositoryTagGroup[];
  libraryExtensions?: RegisteredLibraryExtension[];
  repoId?: string;
  playlistEntries?: FileBrowserEntry[];
  saveMetadata: (entry: FileBrowserEntry, metadata: Record<string, unknown>) => Promise<unknown>;
  saveCoverThumbnail?: (path: string, sourceUrl: string) => Promise<unknown>;
}>();

const draft = reactive({
  rating: 0,
  comment: "",
  link: "",
  tags: [] as string[],
});
const tagsExpanded = ref(false);
const tagMenuOpen = ref(false);
const tagDraft = ref("");
const tagMenuRef = ref<HTMLElement | null>(null);
const tagButtonRef = ref<HTMLElement | null>(null);
const saveState = ref<"idle" | "saving" | "saved">("idle");
let hydrateDraft = false;
let saveTimer: ReturnType<typeof setTimeout> | null = null;

const sourcePayload = computed(() => ({
  rating: metadataNumber(props.entry.metadata, "rating"),
  comment: metadataComment(props.entry.metadata),
  link: metadataString(props.entry.metadata, "link"),
  tagGroups: metadataTagGroups(props.entry.metadata),
}));

const editablePayload = computed(() => ({
  rating: draft.rating,
  comment: draft.comment.trim(),
  link: draft.link.trim(),
  tagGroups: draft.tags,
}));

const hasChanges = computed(() => (
  JSON.stringify(editablePayload.value) !== JSON.stringify(sourcePayload.value)
));

const addedToLibraryAt = computed(() => formatMetadataDate(metadataString(props.entry.metadata, "addedToLibraryAt")));
const fileCreatedAt = computed(() => formatMetadataDate(metadataString(props.entry.metadata, "fileCreatedAt")));
const fileModifiedAt = computed(() => formatMetadataDate(metadataString(props.entry.metadata, "fileModifiedAt") || props.entry.modifiedAt || ""));
const dimensionsLabel = computed(() => {
  const width = metadataRawNumber(props.entry.metadata, "width");
  const height = metadataRawNumber(props.entry.metadata, "height");
  return width && height ? `${width} × ${height}` : "未记录";
});
const originalSizeLabel = computed(() => formatBytes(metadataRawNumber(props.entry.metadata, "originalSizeBytes")));
const palette = computed(() => metadataPalette(props.entry.metadata));
const aliasPaths = computed(() => props.entry.aliasPaths?.filter((path) => path !== props.entry.path) ?? []);
const indexedTags = computed(() => props.entry.tags ?? []);
const canEdit = computed(() => props.entry.kind === "file" && Boolean(props.entry.assetId));
const sourceTitle = computed(() => metadataString(props.entry.metadata, "originTitle"));
const sourceUrl = computed(() => metadataString(props.entry.metadata, "sourceUrl"));
const sourceLinks = computed(() => [
  { key: "sourceUrl", label: "来源链接", value: sourceUrl.value },
].filter((item) => item.value));
const groupedTagOptions = computed(() => {
  const selected = new Set(draft.tags);
  return (props.tagGroups ?? [])
    .map((group) => ({
      ...group,
      tags: group.tags.filter((tag) => !selected.has(tag)),
    }))
    .filter((group) => group.tags.length);
});

const openableProtocolPattern = /^[a-z][a-z\d+.-]*:/i;
const blockedProtocolPattern = /^(javascript|data|vbscript):/i;

function isOpenableSourceLink(value: string) {
  return openableProtocolPattern.test(value) && !blockedProtocolPattern.test(value);
}

async function openSourceLink(value: string) {
  if (!isOpenableSourceLink(value)) return;
  await openExternalUrl(value);
}

async function copySourceLink(value: string) {
  await navigator.clipboard.writeText(value);
}

const filteredExistingTags = computed(() => {
  const keyword = tagDraft.value.trim().toLowerCase();
  return props.availableTags
    .filter((tag) => !draft.tags.includes(tag))
    .filter((tag) => !keyword || tag.toLowerCase().includes(keyword))
    .slice(0, 18);
});
const canCreateDraftTag = computed(() => {
  const value = tagDraft.value.trim();
  return Boolean(value) && !draft.tags.includes(value);
});

watch(sourcePayload, (payload) => {
  hydrateDraft = true;
  draft.rating = payload.rating;
  draft.comment = payload.comment;
  draft.link = payload.link;
  draft.tags = [...payload.tagGroups];
  tagsExpanded.value = payload.tagGroups.length > 0;
  saveState.value = "saved";
  queueMicrotask(() => {
    hydrateDraft = false;
  });
}, { immediate: true });

function setRating(nextRating: number) {
  draft.rating = draft.rating === nextRating ? 0 : nextRating;
}

function addTag(tag: string) {
  const normalized = tag.trim();
  if (!normalized || draft.tags.includes(normalized)) return;
  draft.tags = [...draft.tags, normalized];
  closeTagMenu();
}

function removeTag(tag: string) {
  draft.tags = draft.tags.filter((item) => item !== tag);
}

function toggleTagsExpanded() {
  tagsExpanded.value = !tagsExpanded.value;
}

function openTagMenu() {
  if (!canEdit.value || props.isSaving) return;
  tagMenuOpen.value = true;
  document.addEventListener("pointerdown", handleDocumentPointerDown, true);
  document.addEventListener("keydown", handleDocumentKeydown);
}

function closeTagMenu() {
  tagMenuOpen.value = false;
  tagDraft.value = "";
  document.removeEventListener("pointerdown", handleDocumentPointerDown, true);
  document.removeEventListener("keydown", handleDocumentKeydown);
}

function toggleTagMenu() {
  if (tagMenuOpen.value) {
    closeTagMenu();
    return;
  }
  openTagMenu();
}

function handleDocumentPointerDown(event: PointerEvent) {
  const target = event.target as Node | null;
  if (!target) return;
  if (tagMenuRef.value?.contains(target) || tagButtonRef.value?.contains(target)) return;
  closeTagMenu();
}

function handleDocumentKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    closeTagMenu();
  }
}

function createDraftTag() {
  if (!canCreateDraftTag.value) return;
  addTag(tagDraft.value);
}

async function handleSave() {
  if (!canEdit.value || !hasChanges.value || props.isSaving) return;
  saveState.value = "saving";
  await props.saveMetadata(props.entry, editablePayload.value);
}

function scheduleAutoSave() {
  if (saveTimer) {
    clearTimeout(saveTimer);
  }
  if (!canEdit.value || !hasChanges.value) {
    saveState.value = "saved";
    return;
  }
  saveState.value = "idle";
  saveTimer = setTimeout(() => {
    saveTimer = null;
    void handleSave();
  }, 260);
}

watch(
  () => JSON.stringify(editablePayload.value),
  () => {
    if (hydrateDraft) return;
    scheduleAutoSave();
  },
);

watch(
  () => props.isSaving,
  (saving) => {
    if (saving) {
      saveState.value = "saving";
      return;
    }
    if (hasChanges.value) {
      scheduleAutoSave();
      return;
    }
    saveState.value = "saved";
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  closeTagMenu();
  if (saveTimer) {
    clearTimeout(saveTimer);
  }
});
</script>

<template>
  <section v-if="entry.kind === 'file'" class="file-metadata-card">
    <label class="asset-meta__row file-metadata-card__inline-row">
      <span>注释</span>
      <div class="file-metadata-card__inline-input">
        <MessageSquareText :size="14" aria-hidden="true" />
        <input
          v-model="draft.comment"
          type="text"
          placeholder="记录这个文件的用途、状态或上下文。"
          :disabled="!canEdit || isSaving"
        />
      </div>
    </label>

    <label class="asset-meta__row file-metadata-card__inline-row">
      <span>链接</span>
      <div class="file-metadata-card__inline-input">
        <Link2 :size="14" aria-hidden="true" />
        <input v-model="draft.link" type="url" placeholder="https://example.com" :disabled="!canEdit || isSaving" />
      </div>
    </label>

    <label class="asset-meta__row file-metadata-card__inline-row">
      <span>评分</span>
      <div class="file-metadata-card__stars" aria-label="文件评分">
        <button
          v-for="rating in [1, 2, 3, 4, 5]"
          :key="rating"
          type="button"
          class="file-metadata-card__star"
          :class="{ 'is-active': draft.rating >= rating }"
          :disabled="!canEdit || isSaving"
          :title="`${rating} 星`"
          @click="setRating(rating)"
        >
          <Star :size="18" aria-hidden="true" />
        </button>
      </div>
    </label>

    <section class="asset-meta__row file-metadata-card__tags">
      <button type="button" class="file-metadata-card__collapse" @click="toggleTagsExpanded">
        <span>标签组</span>
        <span class="file-metadata-card__collapse-meta">
          {{ draft.tags.length ? `${draft.tags.length} 个标签` : "暂无标签" }}
          <ChevronDown v-if="tagsExpanded" :size="14" aria-hidden="true" />
          <ChevronRight v-else :size="14" aria-hidden="true" />
        </span>
      </button>

      <div v-if="tagsExpanded" class="file-metadata-card__tag-panel">
        <div v-if="draft.tags.length" class="file-metadata-card__tag-list">
          <span v-for="tag in draft.tags" :key="tag" class="file-metadata-card__tag-chip">
            {{ tag }}
            <button type="button" :disabled="!canEdit || isSaving" @click="removeTag(tag)">×</button>
          </span>
          <div class="file-metadata-card__tag-menu-anchor">
            <button
              ref="tagButtonRef"
              type="button"
              class="file-metadata-card__tag-add"
              :disabled="!canEdit || isSaving"
              @click="toggleTagMenu"
            >
              <Plus :size="18" aria-hidden="true" />
            </button>
            <div v-if="tagMenuOpen" ref="tagMenuRef" class="file-metadata-card__tag-menu">
              <label class="file-metadata-card__tag-field">
                <span>新建标签</span>
                <input
                  v-model="tagDraft"
                  type="text"
                  placeholder="输入新标签"
                  :disabled="!canEdit || isSaving"
                  @keydown.enter.prevent="createDraftTag"
                />
              </label>
              <button
                type="button"
                class="ghost file-metadata-card__tag-create"
                :disabled="!canCreateDraftTag || isSaving"
                @click="createDraftTag"
              >
                添加“{{ tagDraft.trim() || "新标签" }}”
              </button>
              <div v-if="groupedTagOptions.length" class="file-metadata-card__tag-options">
                <span>标签分组</span>
                <div v-for="group in groupedTagOptions" :key="group.tagGroupId" class="file-metadata-card__tag-group">
                  <strong>{{ group.name }}</strong>
                  <div class="file-metadata-card__tag-option-list">
                    <button
                      v-for="tag in group.tags"
                      :key="`${group.tagGroupId}-${tag}`"
                      type="button"
                      class="workspace-filter-chip"
                      :disabled="isSaving"
                      @click="addTag(tag)"
                    >
                      {{ tag }}
                    </button>
                  </div>
                </div>
              </div>
              <div v-if="filteredExistingTags.length" class="file-metadata-card__tag-options">
                <span>已有标签</span>
                <div class="file-metadata-card__tag-option-list">
                  <button
                    v-for="tag in filteredExistingTags"
                    :key="tag"
                    type="button"
                    class="workspace-filter-chip"
                    :disabled="isSaving"
                    @click="addTag(tag)"
                  >
                    {{ tag }}
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>

        <button
          v-else
          ref="tagButtonRef"
          type="button"
          class="ghost file-metadata-card__empty-tag-btn"
          :disabled="!canEdit || isSaving"
          @click="toggleTagMenu"
        >
          <Plus :size="18" aria-hidden="true" />
          添加标签
        </button>

        <div v-if="tagMenuOpen && !draft.tags.length" ref="tagMenuRef" class="file-metadata-card__tag-menu file-metadata-card__tag-menu--standalone">
          <label class="file-metadata-card__tag-field">
            <span>新建标签</span>
            <input
              v-model="tagDraft"
              type="text"
              placeholder="输入新标签"
              :disabled="!canEdit || isSaving"
              @keydown.enter.prevent="createDraftTag"
            />
          </label>
          <button
            type="button"
            class="ghost file-metadata-card__tag-create"
            :disabled="!canCreateDraftTag || isSaving"
            @click="createDraftTag"
          >
            添加“{{ tagDraft.trim() || "新标签" }}”
          </button>
          <div v-if="groupedTagOptions.length" class="file-metadata-card__tag-options">
            <span>标签分组</span>
            <div v-for="group in groupedTagOptions" :key="group.tagGroupId" class="file-metadata-card__tag-group">
              <strong>{{ group.name }}</strong>
              <div class="file-metadata-card__tag-option-list">
                <button
                  v-for="tag in group.tags"
                  :key="`${group.tagGroupId}-${tag}`"
                  type="button"
                  class="workspace-filter-chip"
                  :disabled="isSaving"
                  @click="addTag(tag)"
                >
                  {{ tag }}
                </button>
              </div>
            </div>
          </div>
          <div v-if="filteredExistingTags.length" class="file-metadata-card__tag-options">
            <span>已有标签</span>
            <div class="file-metadata-card__tag-option-list">
              <button
                v-for="tag in filteredExistingTags"
                :key="tag"
                type="button"
                class="workspace-filter-chip"
                :disabled="isSaving"
                @click="addTag(tag)"
              >
                {{ tag }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </section>

    <div class="file-metadata-card__grid">
      <div class="asset-meta__row">
        <span>添加到资源库</span>
        <span class="asset-meta__value">{{ addedToLibraryAt }}</span>
      </div>
      <div class="asset-meta__row">
        <span>创建时间</span>
        <span class="asset-meta__value">{{ fileCreatedAt }}</span>
      </div>
      <div class="asset-meta__row">
        <span>文件修改时间</span>
        <span class="asset-meta__value">{{ fileModifiedAt }}</span>
      </div>
      <div class="asset-meta__row">
        <span>尺寸</span>
        <span class="asset-meta__value">{{ dimensionsLabel }}</span>
      </div>
      <div class="asset-meta__row">
        <span>原始大小</span>
        <span class="asset-meta__value">{{ originalSizeLabel }}</span>
      </div>
      <div v-if="palette.length" class="asset-meta__row">
        <span>调色板</span>
        <span class="file-metadata-card__palette">
          <i v-for="color in palette" :key="color" :style="{ backgroundColor: color }" :title="color" />
        </span>
      </div>
      <div v-if="indexedTags.length" class="asset-meta__row">
        <span>索引标签</span>
        <span class="asset-meta__value">{{ indexedTags.join("，") }}</span>
      </div>
      <div v-if="aliasPaths.length" class="asset-meta__row file-metadata-card__alias-row">
        <span>多归属位置</span>
        <span class="asset-meta__value">{{ aliasPaths.join("，") }}</span>
      </div>
      <div v-if="sourceTitle" class="asset-meta__row file-metadata-card__source-row">
        <span>来源标题</span>
        <span class="asset-meta__value">{{ sourceTitle }}</span>
      </div>
      <div v-for="item in sourceLinks" :key="item.key" class="asset-meta__row file-metadata-card__source-row">
        <span>{{ item.label }}</span>
        <span class="file-metadata-card__source-value">
          <span class="asset-meta__value">{{ item.value }}</span>
          <span class="file-metadata-card__source-actions">
            <button
              type="button"
              class="file-metadata-card__source-action"
              :disabled="!isOpenableSourceLink(item.value)"
              title="打开链接"
              @click="openSourceLink(item.value)"
            >
              <ExternalLink :size="14" aria-hidden="true" />
            </button>
            <button
              type="button"
              class="file-metadata-card__source-action"
              title="复制链接"
              @click="copySourceLink(item.value)"
            >
              <Copy :size="14" aria-hidden="true" />
            </button>
          </span>
        </span>
      </div>
    </div>

    <component
      :is="extension.metadataPanel"
      v-for="extension in libraryExtensions?.filter((item) => item.metadataPanel) ?? []"
      :key="`${extension.pluginId}:metadata`"
      :repo-id="repoId ?? ''"
      :entry="entry"
      :entries="playlistEntries ?? []"
      :save-metadata="saveMetadata"
      :save-cover-thumbnail="saveCoverThumbnail"
    />
  </section>
</template>

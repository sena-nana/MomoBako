<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from "vue";
import { Check, ChevronDown, ChevronRight, Copy, ExternalLink, ImagePlus, Link2, MessageSquareText, Plus, Star } from "lucide-vue-next";
import { lookupAsmrMetadataCandidate, openExternalUrl } from "../../services/repositoryApi";
import type { FileBrowserEntry, RepositoryTagGroup } from "../../types/repository";
import {
  appendAsmrMetadataCandidate,
  buildAsmrMetadataCandidateSummary,
  formatCandidateFieldValue,
  parseAsmrMetadataCandidateJson,
  readAsmrMetadataCandidates,
} from "./asmrMetadataCandidates";
import {
  formatBytes,
  formatMetadataDate,
  metadataComment,
  metadataNumber,
  metadataPalette,
  metadataRawNumber,
  metadataString,
  metadataTagGroups,
} from "../../utils/fileMetadata";

const props = defineProps<{
  entry: FileBrowserEntry;
  isSaving: boolean;
  availableTags: string[];
  tagGroups?: RepositoryTagGroup[];
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
const asmrCandidateImportOpen = ref(false);
const asmrCandidateImportDraft = ref("");
const asmrCandidateImportError = ref("");
const asmrProviderLookup = reactive({
  provider: "dlsite",
  rjCode: "",
  isLoading: false,
});
const asmrCoverSaveState = ref<"idle" | "saving" | "saved">("idle");
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
const originReferrer = computed(() => metadataString(props.entry.metadata, "originReferrer"));
const sourceLinks = computed(() => [
  { key: "sourceUrl", label: "原始链接", value: sourceUrl.value },
  { key: "originReferrer", label: "来源页", value: originReferrer.value },
].filter((item) => item.value));
const hasSourceMetadata = computed(() => Boolean(sourceTitle.value || sourceLinks.value.length));
const isAsmrEntry = computed(() => {
  const metadata = props.entry.metadata ?? {};
  return metadata.libraryKind === "asmr" || Boolean(metadata.workId) || Boolean(metadata.rjCode);
});
const asmrRjCode = computed(() => metadataString(props.entry.metadata, "rjCode") || metadataString(props.entry.metadata, "workId"));
const asmrRows = computed(() => {
  const metadata = props.entry.metadata ?? {};
  const text = (key: string) => metadataString(metadata, key);
  const number = (key: string) => metadataRawNumber(metadata, key);
  const rows = [
    { key: "workId", label: "作品 ID", value: text("workId") || text("rjCode") },
    { key: "workTitle", label: "标题", value: text("workTitle") },
    { key: "workRoot", label: "作品目录", value: text("workRoot") },
    { key: "trackTitle", label: "音轨", value: text("trackTitle") },
    { key: "circle", label: "社团", value: text("circle") },
    { key: "voiceActors", label: "声优", value: formatMetadataList(metadata.voiceActors) },
    { key: "series", label: "系列", value: text("series") },
    { key: "scenarioTags", label: "标签", value: formatMetadataList(metadata.scenarioTags ?? metadata.tags) },
    { key: "releaseDate", label: "发售日", value: text("releaseDate") },
    { key: "ageRating", label: "年龄分级", value: text("ageRating") },
    { key: "lyricStatus", label: "歌词", value: text("lyricStatus") || "未检测" },
    { key: "asmrEntryKind", label: "条目类型", value: text("asmrEntryKind") },
    { key: "listeningStatus", label: "收听状态", value: formatListeningStatus(text("listeningStatus"), number("listeningProgress")) },
    { key: "trackDurationMs", label: "音轨时长", value: formatDuration(number("trackDurationMs")) },
    { key: "price", label: "价格", value: formatNumberValue(number("price"), " JPY") },
    { key: "sales", label: "销量", value: formatNumberValue(number("sales") ?? number("dlCount")) },
    { key: "rateAverage", label: "评分", value: formatNumberValue(number("rateAverage") ?? number("ratingAverage")) },
    { key: "reviewCount", label: "评论数", value: formatNumberValue(number("reviewCount")) },
  ];
  return rows.filter((row) => row.value);
});
const asmrMetadataCandidates = computed(() => (
  readAsmrMetadataCandidates(props.entry.metadata)
    .map(buildAsmrMetadataCandidateSummary)
    .filter((candidate) => Object.keys(candidate.patch).length || candidate.skipped.length)
));
const asmrCandidateFields = computed(() => (
  asmrMetadataCandidates.value.map((candidate) => ({
    ...candidate,
    coverUrl: readRemoteCoverUrl(candidate.patch),
    fields: Object.entries(candidate.patch).map(([key, value]) => ({
      key,
      value: formatCandidateFieldValue(value),
    })),
  }))
));
const asmrCoverUrl = computed(() => readRemoteCoverUrl(props.entry.metadata));
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

function readRemoteCoverUrl(metadata: Record<string, unknown> | undefined) {
  const cover = metadata?.coverUrl ?? metadata?.cover;
  if (typeof cover !== "string") return "";
  const value = cover.trim();
  return /^https?:\/\//i.test(value) ? value : "";
}

async function openSourceLink(value: string) {
  if (!isOpenableSourceLink(value)) return;
  await openExternalUrl(value);
}

async function copySourceLink(value: string) {
  await navigator.clipboard.writeText(value);
}

async function saveAsmrCoverThumbnail(sourceUrl: string) {
  if (!canEdit.value || props.isSaving || asmrCoverSaveState.value === "saving" || !props.saveCoverThumbnail) return;
  asmrCoverSaveState.value = "saving";
  try {
    await props.saveCoverThumbnail(props.entry.path, sourceUrl);
    asmrCoverSaveState.value = "saved";
  } catch {
    asmrCoverSaveState.value = "idle";
  }
}

function formatMetadataList(value: unknown) {
  if (Array.isArray(value)) {
    return value
      .filter((item): item is string | number | boolean => (
        typeof item === "string" || typeof item === "number" || typeof item === "boolean"
      ))
      .map(String)
      .filter(Boolean)
      .join("，");
  }
  if (typeof value === "string") return value;
  return "";
}

function formatNumberValue(value: number | null, suffix = "") {
  if (value == null) return "";
  return `${value.toLocaleString("zh-CN")}${suffix}`;
}

function formatDuration(value: number | null) {
  if (value == null || value <= 0) return "";
  const seconds = Math.round(value / 1000);
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return `${minutes}:${String(rest).padStart(2, "0")}`;
}

function formatListeningStatus(status: string, progress: number | null) {
  const labels: Record<string, string> = {
    unlistened: "未收听",
    listening: "收听中",
    listened: "已听完",
  };
  const label = labels[status] ?? status;
  if (progress == null || progress <= 0) return label;
  return `${label} · ${Math.round(progress)}%`;
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

watch(asmrRjCode, (value) => {
  if (!asmrProviderLookup.rjCode) {
    asmrProviderLookup.rjCode = value;
  }
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

async function applyAsmrMetadataCandidate(candidate: { patch: Record<string, unknown> }) {
  if (!canEdit.value || props.isSaving || !Object.keys(candidate.patch).length) return;
  saveState.value = "saving";
  await props.saveMetadata(props.entry, candidate.patch);
}

function toggleAsmrCandidateImport() {
  asmrCandidateImportOpen.value = !asmrCandidateImportOpen.value;
  asmrCandidateImportError.value = "";
}

async function importAsmrMetadataCandidate() {
  if (!canEdit.value || props.isSaving) return;
  const result = parseAsmrMetadataCandidateJson(asmrCandidateImportDraft.value);
  if (!result.ok) {
    asmrCandidateImportError.value = result.error;
    return;
  }
  asmrCandidateImportError.value = "";
  saveState.value = "saving";
  await props.saveMetadata(props.entry, {
    providerCandidates: appendAsmrMetadataCandidate(props.entry.metadata, result.candidate),
  });
  asmrCandidateImportDraft.value = "";
  asmrCandidateImportOpen.value = false;
}

async function lookupAsmrProviderCandidate() {
  if (!canEdit.value || props.isSaving || asmrProviderLookup.isLoading) return;
  const rjCode = (asmrProviderLookup.rjCode || asmrRjCode.value).trim();
  if (!rjCode) {
    asmrCandidateImportError.value = "缺少 RJ 作品 ID";
    return;
  }
  asmrProviderLookup.isLoading = true;
  asmrCandidateImportError.value = "";
  try {
    const response = await lookupAsmrMetadataCandidate({
      provider: asmrProviderLookup.provider,
      rjCode,
    });
    saveState.value = "saving";
    await props.saveMetadata(props.entry, {
      providerCandidates: appendAsmrMetadataCandidate(props.entry.metadata, response.candidate),
    });
  } catch (cause) {
    asmrCandidateImportError.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    asmrProviderLookup.isLoading = false;
  }
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
    <div class="file-metadata-card__head">
      <div>
        <p class="asset-browser__eyebrow">文件 Metadata</p>
        <strong>通用字段</strong>
      </div>
      <span class="file-metadata-card__status">
        {{ saveState === "saving" ? "保存中…" : hasChanges ? "自动保存" : "已同步" }}
      </span>
    </div>

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
    </div>

    <section v-if="isAsmrEntry && asmrRows.length" class="file-metadata-card__source file-metadata-card__asmr" aria-label="ASMR 信息">
      <div class="file-metadata-card__source-head">
        <div>
          <p class="asset-browser__eyebrow">ASMR Metadata</p>
          <strong>作品信息</strong>
        </div>
        <button
          v-if="asmrCoverUrl && saveCoverThumbnail"
          type="button"
          class="ghost"
          :disabled="!canEdit || isSaving || asmrCoverSaveState === 'saving'"
          @click="saveAsmrCoverThumbnail(asmrCoverUrl)"
        >
          <ImagePlus :size="14" aria-hidden="true" />
          {{ asmrCoverSaveState === "saving" ? "保存中" : "保存封面" }}
        </button>
      </div>
      <div class="file-metadata-card__source-grid">
        <div v-for="item in asmrRows" :key="item.key" class="asset-meta__row file-metadata-card__source-row">
          <span>{{ item.label }}</span>
          <span class="asset-meta__value">{{ item.value }}</span>
        </div>
      </div>
    </section>

    <section v-if="isAsmrEntry" class="file-metadata-card__source file-metadata-card__asmr-candidates" aria-label="ASMR 元数据候选">
      <div class="file-metadata-card__source-head">
        <div>
          <p class="asset-browser__eyebrow">ASMR Provider</p>
          <strong>补全候选</strong>
        </div>
        <button type="button" class="ghost" :disabled="!canEdit || isSaving" @click="toggleAsmrCandidateImport">
          <Plus :size="14" aria-hidden="true" />
          导入
        </button>
      </div>
      <div v-if="asmrCandidateImportOpen" class="file-metadata-card__candidate-import">
        <div class="file-metadata-card__provider-lookup">
          <select v-model="asmrProviderLookup.provider" :disabled="!canEdit || isSaving || asmrProviderLookup.isLoading" aria-label="ASMR Provider">
            <option value="dlsite">DLsite</option>
            <option value="asmr-one">ASMR One</option>
          </select>
          <input
            v-model="asmrProviderLookup.rjCode"
            type="text"
            aria-label="RJ 作品 ID"
            placeholder="RJ123456"
            :disabled="!canEdit || isSaving || asmrProviderLookup.isLoading"
          />
          <button
            type="button"
            class="ghost"
            :disabled="!canEdit || isSaving || asmrProviderLookup.isLoading"
            @click="lookupAsmrProviderCandidate"
          >
            {{ asmrProviderLookup.isLoading ? "抓取中" : "抓取候选" }}
          </button>
        </div>
        <textarea
          v-model="asmrCandidateImportDraft"
          aria-label="ASMR 候选 JSON"
          :disabled="!canEdit || isSaving"
        />
        <div>
          <span>{{ asmrCandidateImportError }}</span>
          <button type="button" class="ghost" :disabled="!canEdit || isSaving" @click="importAsmrMetadataCandidate">
            导入候选
          </button>
        </div>
      </div>
      <div v-if="asmrCandidateFields.length" class="file-metadata-card__candidate-list">
        <article v-for="(candidate, index) in asmrCandidateFields" :key="`${candidate.source}-${candidate.confidence}-${index}`" class="file-metadata-card__candidate">
          <header>
            <span>
              <strong>{{ candidate.source }}</strong>
              <small>{{ candidate.confidence }}</small>
            </span>
            <span class="file-metadata-card__candidate-actions">
              <button
                v-if="candidate.coverUrl && saveCoverThumbnail"
                type="button"
                class="ghost"
                :disabled="!canEdit || isSaving || asmrCoverSaveState === 'saving'"
                @click="saveAsmrCoverThumbnail(candidate.coverUrl)"
              >
                <ImagePlus :size="14" aria-hidden="true" />
                封面
              </button>
              <button
                type="button"
                class="ghost"
                :disabled="!canEdit || isSaving || !candidate.fields.length"
                @click="applyAsmrMetadataCandidate(candidate)"
              >
                <Check :size="14" aria-hidden="true" />
                应用
              </button>
            </span>
          </header>
          <div v-if="candidate.fields.length" class="file-metadata-card__candidate-fields">
            <span v-for="field in candidate.fields" :key="field.key">
              {{ field.key }}={{ field.value }}
            </span>
          </div>
          <small v-if="candidate.skipped.length" class="file-metadata-card__candidate-skipped">
            跳过 {{ candidate.skipped.join("，") }}
          </small>
        </article>
      </div>
      <small v-else-if="!asmrCandidateImportOpen" class="file-metadata-card__candidate-skipped">暂无候选</small>
    </section>

    <section v-if="hasSourceMetadata" class="file-metadata-card__source" aria-label="来源信息">
      <div class="file-metadata-card__source-head">
        <div>
          <p class="asset-browser__eyebrow">来源 Metadata</p>
          <strong>来源信息</strong>
        </div>
      </div>
      <div class="file-metadata-card__source-grid">
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
    </section>

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

    <label class="asset-meta__row file-metadata-card__inline-row">
      <span>链接</span>
      <div class="file-metadata-card__inline-input">
        <Link2 :size="14" aria-hidden="true" />
        <input v-model="draft.link" type="url" placeholder="https://example.com" :disabled="!canEdit || isSaving" />
      </div>
    </label>

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
  </section>
</template>

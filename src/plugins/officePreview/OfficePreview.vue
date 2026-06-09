<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, ref, watch, type Component } from "vue";
import { FileSpreadsheet, FileText, FileType, Presentation } from "lucide-vue-next";
import { useRepositoryWorkspace } from "../../composables/useRepositoryWorkspace";
import type { FileBrowserEntry } from "../../types/repository";
import {
  getOfficePreviewKind,
  isPptxPreviewExtension,
  isVueOfficePreviewExtension,
  officeKindLabel,
} from "./officeExtensions";
import {
  loadOfficePreviewDocument,
  prepareOfficePreviewSource,
  type OfficePreviewDocument,
} from "./officeSource";
import { generateOfficeThumbnailForEntry, generateOfficeThumbnailFromDocument } from "./officeThumbnail";

const props = defineProps<{
  entry: FileBrowserEntry;
  repoId: string;
}>();

const VueOfficeDocxPreview = defineAsyncComponent(async () => {
  await import("@vue-office/docx/lib/index.css");
  const module = await import("@vue-office/docx");
  return module.default as unknown as Component;
});
const VueOfficeExcelPreview = defineAsyncComponent(async () => {
  await import("@vue-office/excel/lib/index.css");
  const module = await import("@vue-office/excel");
  return module.default as unknown as Component;
});
const VueOfficePdfPreview = defineAsyncComponent(async () => {
  const module = await import("@vue-office/pdf");
  return module.default as unknown as Component;
});
const PptxPreview = defineAsyncComponent(async () => {
  const module = await import("./PptxPreview.vue");
  return module.default as Component;
});

const state = ref<"idle" | "loading" | "ready" | "error">("idle");
const errorMessage = ref("");
const documentPreview = ref<OfficePreviewDocument | null>(null);
const sourceUrl = ref("");
const { saveGeneratedWorkspaceEntryThumbnail } = useRepositoryWorkspace();
let loadToken = 0;

const kind = computed(() => getOfficePreviewKind(props.entry.extension));
const usesVueOfficePreview = computed(() => isVueOfficePreviewExtension(props.entry.extension));
const usesPptxPreview = computed(() => isPptxPreviewExtension(props.entry.extension));
const kindLabel = computed(() => officeKindLabel(kind.value));
const extensionLabel = computed(() => props.entry.extension?.toUpperCase() || kindLabel.value.toUpperCase());
const iconComponent = computed(() => {
  if (kind.value === "spreadsheet") return FileSpreadsheet;
  if (kind.value === "presentation") return Presentation;
  if (kind.value === "pdf") return FileType;
  return FileText;
});
const vueOfficeComponent = computed(() => {
  const extension = props.entry.extension?.toLowerCase() ?? "";
  if (extension === "docx") return VueOfficeDocxPreview;
  if (extension === "xlsx") return VueOfficeExcelPreview;
  if (extension === "pdf") return VueOfficePdfPreview;
  return null;
});
const vueOfficeOptions = computed(() => {
  if (props.entry.extension?.toLowerCase() !== "xlsx") return undefined;
  return {
    minColLength: 12,
    minRowLength: 32,
    showContextmenu: false,
  };
});
const statusDetail = computed(() => {
  if (usesVueOfficePreview.value && state.value === "ready") return "vue-office 预览";
  if (usesVueOfficePreview.value) return props.entry.sizeLabel || "准备文档";
  if (usesPptxPreview.value && state.value === "ready") return "pptx-preview 预览";
  if (usesPptxPreview.value) return props.entry.sizeLabel || "准备演示文稿";
  if (documentPreview.value?.unsupported) return "文件信息";
  return documentPreview.value?.subtitle || props.entry.sizeLabel || "准备文档";
});

watch(
  [() => props.repoId, () => props.entry.path],
  () => {
    void loadPreview();
  },
  { immediate: true },
);

async function loadPreview() {
  const token = ++loadToken;
  state.value = "loading";
  errorMessage.value = "";
  documentPreview.value = null;
  sourceUrl.value = "";

  try {
    if (usesVueOfficePreview.value || usesPptxPreview.value) {
      await loadPreviewSource(token);
    } else {
      await loadOfficeDocumentPreview(token);
      if (token !== loadToken) return;
      state.value = "ready";
    }
  } catch (cause) {
    if (token !== loadToken) return;
    state.value = "error";
    errorMessage.value = cause instanceof Error ? cause.message : String(cause);
  }
}

async function loadPreviewSource(token: number) {
  const source = await prepareOfficePreviewSource(props.repoId, props.entry.path);
  if (token !== loadToken) return;
  sourceUrl.value = source.sourceUrl;
}

async function loadOfficeDocumentPreview(token: number) {
  const document = await loadOfficePreviewDocument(props.repoId, props.entry);
  if (token !== loadToken) return;
  documentPreview.value = document;
  void persistOfficeThumbnail(token, document);
}

function handleVueOfficeRendered() {
  if (!usesVueOfficePreview.value || !sourceUrl.value) return;
  const token = loadToken;
  state.value = "ready";
  void persistVueOfficeThumbnail(token);
}

function handlePptxRendered() {
  if (!usesPptxPreview.value || !sourceUrl.value) return;
  const token = loadToken;
  state.value = "ready";
  void persistOfficeThumbnailForEntry(token);
}

function handleVueOfficeError(cause: unknown) {
  state.value = "error";
  errorMessage.value = cause instanceof Error ? cause.message : String(cause || "vue-office 渲染失败");
}

function handlePptxError(cause: unknown) {
  state.value = "error";
  errorMessage.value = cause instanceof Error ? cause.message : String(cause || "pptx-preview 渲染失败");
}

async function persistVueOfficeThumbnail(token: number) {
  await persistOfficeThumbnailForEntry(token);
}

async function persistOfficeThumbnailForEntry(token: number) {
  await nextTick();
  if (token !== loadToken) return;
  const thumbnail = await generateOfficeThumbnailForEntry(props.repoId, props.entry);
  if (token !== loadToken || !thumbnail) return;
  await saveGeneratedWorkspaceEntryThumbnail(props.entry.path, thumbnail.bytes, thumbnail.mediaType);
}

async function persistOfficeThumbnail(token: number, document: OfficePreviewDocument) {
  await nextTick();
  if (token !== loadToken) return;
  const thumbnail = await generateOfficeThumbnailFromDocument(props.entry, document);
  if (token !== loadToken || !thumbnail) return;
  await saveGeneratedWorkspaceEntryThumbnail(props.entry.path, thumbnail.bytes, thumbnail.mediaType);
}
</script>

<template>
  <div class="office-preview" :class="`office-preview--${kind}`">
    <div class="office-preview__toolbar">
      <span class="office-preview__kind">
        <component :is="iconComponent" :size="14" aria-hidden="true" />
        {{ kindLabel }}
      </span>
      <span>{{ extensionLabel }}</span>
      <span>{{ statusDetail }}</span>
    </div>

    <div v-if="state === 'loading'" class="office-preview__status">
      <span>读取文档</span>
      <span>{{ entry.sizeLabel ? `准备 ${entry.sizeLabel}` : "建立预览" }}</span>
    </div>

    <div v-else-if="state === 'error'" class="office-preview__overlay office-preview__overlay--error">
      <strong>无法预览该文档</strong>
      <span>{{ errorMessage }}</span>
    </div>

    <div
      v-if="usesVueOfficePreview && sourceUrl && vueOfficeComponent"
      class="office-preview__viewer"
      :class="`office-preview__viewer--${kind}`"
    >
      <component
        :is="vueOfficeComponent"
        :key="`${entry.path}:${sourceUrl}`"
        class="office-preview__vue-office"
        :src="sourceUrl"
        :options="vueOfficeOptions"
        @rendered="handleVueOfficeRendered"
        @error="handleVueOfficeError"
      />
    </div>

    <div
      v-if="usesPptxPreview && sourceUrl"
      class="office-preview__viewer office-preview__viewer--presentation"
    >
      <PptxPreview
        :key="`${entry.path}:${sourceUrl}`"
        :src="sourceUrl"
        @rendered="handlePptxRendered"
        @error="handlePptxError"
      />
    </div>

    <div v-if="!usesVueOfficePreview && !usesPptxPreview && documentPreview" class="office-preview__document">
      <header class="office-preview__document-head">
        <component :is="iconComponent" :size="26" aria-hidden="true" />
        <div>
          <h2>{{ documentPreview.title }}</h2>
          <p>{{ documentPreview.subtitle }}</p>
        </div>
      </header>

      <div class="office-preview__stats">
        <span v-for="item in documentPreview.stats" :key="`${item.label}:${item.value}`">
          {{ item.label }}: {{ item.value }}
        </span>
      </div>

      <section
        v-for="section in documentPreview.sections"
        :key="section.title"
        class="office-preview__section"
      >
        <h3>{{ section.title }}</h3>
        <div class="office-preview__rows">
          <div
            v-for="(row, rowIndex) in section.rows"
            :key="`${section.title}:${rowIndex}`"
            class="office-preview__row"
          >
            <span v-for="(cell, cellIndex) in row" :key="`${cellIndex}:${cell}`">{{ cell || " " }}</span>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

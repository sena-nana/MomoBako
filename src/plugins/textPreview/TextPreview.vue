<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import Markdown from "vue3-markdown-it";
import { FileText } from "lucide-vue-next";
import { useRepositoryWorkspace } from "../../composables/useRepositoryWorkspace";
import type { FileBrowserEntry } from "../../types/repository";
import { isMarkdownExtension } from "./textExtensions";
import { loadTextPreviewContent, type TextPreviewContent } from "./textSource";
import { generateTextThumbnailFromContent } from "./textThumbnail";

const props = defineProps<{
  entry: FileBrowserEntry;
  repoId: string;
}>();

const state = ref<"idle" | "loading" | "ready" | "error">("idle");
const content = ref("");
const previewInfo = ref<TextPreviewContent | null>(null);
const errorMessage = ref("");
const { saveGeneratedWorkspaceEntryThumbnail } = useRepositoryWorkspace();
let loadToken = 0;

const isMarkdown = computed(() => isMarkdownExtension(props.entry.extension));
const extensionLabel = computed(() => props.entry.extension?.toUpperCase() || "TEXT");
const lineCount = computed(() => (
  content.value ? content.value.replace(/\r\n?/g, "\n").split("\n").length : 0
));
const truncatedSizeLabel = computed(() => {
  const info = previewInfo.value;
  if (!info?.truncated) return "";
  return `仅显示前 ${formatByteCount(info.bytesRead)}`;
});

watch(
  [() => props.repoId, () => props.entry.path],
  () => {
    void loadText();
  },
  { immediate: true },
);

async function loadText() {
  const token = ++loadToken;
  state.value = "loading";
  content.value = "";
  previewInfo.value = null;
  errorMessage.value = "";

  try {
    const nextContent = await loadTextPreviewContent(props.repoId, props.entry.path);
    if (token !== loadToken) return;
    content.value = nextContent.text;
    previewInfo.value = nextContent;
    state.value = "ready";
    void persistTextThumbnail(token);
  } catch (cause) {
    if (token !== loadToken) return;
    state.value = "error";
    errorMessage.value = cause instanceof Error ? cause.message : String(cause);
  }
}

async function persistTextThumbnail(token: number) {
  await nextTick();
  if (token !== loadToken) return;
  const thumbnail = await generateTextThumbnailFromContent(props.entry, content.value);
  if (token !== loadToken || !thumbnail) return;
  await saveGeneratedWorkspaceEntryThumbnail(props.entry.path, thumbnail.bytes, thumbnail.mediaType);
}

function formatByteCount(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
</script>

<template>
  <div class="text-preview" :class="{ 'text-preview--markdown': isMarkdown }">
    <div v-if="state === 'loading'" class="text-preview__status">
      <span>读取文本</span>
      <span>{{ entry.sizeLabel ? `准备 ${entry.sizeLabel}` : "准备文本内容" }}</span>
    </div>

    <div v-else-if="state === 'error'" class="text-preview__overlay text-preview__overlay--error">
      <strong>无法预览该文本</strong>
      <span>{{ errorMessage }}</span>
    </div>

    <template v-else>
      <div class="text-preview__toolbar">
        <span class="text-preview__kind">
          <FileText :size="14" aria-hidden="true" />
          {{ isMarkdown ? "Markdown" : extensionLabel }}
        </span>
        <span>{{ lineCount }} 行</span>
        <span v-if="truncatedSizeLabel">{{ truncatedSizeLabel }}</span>
      </div>

      <div class="text-preview__content">
        <Markdown
          v-if="isMarkdown && content"
          class="text-preview__markdown"
          :source="content"
        />
        <pre v-else class="text-preview__plain"><code>{{ content || "空文件" }}</code></pre>
      </div>
    </template>
  </div>
</template>

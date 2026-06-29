<script setup lang="ts">
import { LoaderCircle } from "@lucide/vue";
import type { SmartFolder } from "../types/repository";

defineProps<{
  flatSmartFolders: SmartFolder[];
  isMutatingSmartFolder: boolean;
  pendingDeleteSmartFolderLabel: string;
  showSmartFolderDeleteDialog: boolean;
  showSmartFolderDialog: boolean;
  smartFolderDialogActionLabel: string;
  smartFolderDialogDisabled: boolean;
  smartFolderDialogTitle: string;
  smartFolderTargetId: string;
}>();

const smartFolderColors = defineModel<string>("smartFolderColors", { required: true });
const smartFolderDateFilters = defineModel<string>("smartFolderDateFilters", { required: true });
const smartFolderExcludeDateFilters = defineModel<string>("smartFolderExcludeDateFilters", { required: true });
const smartFolderExcludeFormats = defineModel<string>("smartFolderExcludeFormats", { required: true });
const smartFolderExcludeMetadataFilters = defineModel<string>("smartFolderExcludeMetadataFilters", { required: true });
const smartFolderExcludeNumberFilters = defineModel<string>("smartFolderExcludeNumberFilters", { required: true });
const smartFolderExcludePathPrefixes = defineModel<string>("smartFolderExcludePathPrefixes", { required: true });
const smartFolderExcludeQuery = defineModel<string>("smartFolderExcludeQuery", { required: true });
const smartFolderExcludeTags = defineModel<string>("smartFolderExcludeTags", { required: true });
const smartFolderFormats = defineModel<string>("smartFolderFormats", { required: true });
const smartFolderLimit = defineModel<string>("smartFolderLimit", { required: true });
const smartFolderMatchMode = defineModel<"and" | "or">("smartFolderMatchMode", { required: true });
const smartFolderMetadataFilters = defineModel<string>("smartFolderMetadataFilters", { required: true });
const smartFolderMinRating = defineModel<string>("smartFolderMinRating", { required: true });
const smartFolderName = defineModel<string>("smartFolderName", { required: true });
const smartFolderNumberFilters = defineModel<string>("smartFolderNumberFilters", { required: true });
const smartFolderParentId = defineModel<string>("smartFolderParentId", { required: true });
const smartFolderPathPrefix = defineModel<string>("smartFolderPathPrefix", { required: true });
const smartFolderQuery = defineModel<string>("smartFolderQuery", { required: true });
const smartFolderShapes = defineModel<string>("smartFolderShapes", { required: true });
const smartFolderSortDirection = defineModel<"asc" | "desc">("smartFolderSortDirection", { required: true });
const smartFolderSortField = defineModel<string>("smartFolderSortField", { required: true });
const smartFolderTags = defineModel<string>("smartFolderTags", { required: true });

const emit = defineEmits<{
  closeSmartFolderDelete: [];
  closeSmartFolderDialog: [];
  confirmDeleteSmartFolder: [];
  submitSmartFolderDialog: [];
}>();
</script>

<template>
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="showSmartFolderDialog" class="modal-overlay" role="dialog" aria-modal="true" :aria-label="smartFolderDialogTitle" @click.self="emit('closeSmartFolderDialog')">
        <div class="modal-card dialog-card smart-folder-dialog">
          <div class="dialog-card__header">
            <span>{{ smartFolderDialogTitle }}</span>
          </div>
          <div class="dialog-card__body smart-folder-dialog__body">
            <label class="dialog-field">
              <span>名称</span>
              <input v-model="smartFolderName" type="text" placeholder="例如 高评分 PSD" @keydown.enter.prevent="emit('submitSmartFolderDialog')" />
            </label>

            <label class="dialog-field">
              <span>父级</span>
              <select v-model="smartFolderParentId">
                <option value="">顶层智能文件夹</option>
                <option v-for="folder in flatSmartFolders.filter((item) => item.smartFolderId !== smartFolderTargetId)" :key="folder.smartFolderId" :value="folder.smartFolderId">
                  {{ folder.name }}
                </option>
              </select>
            </label>

            <div class="smart-folder-dialog__grid">
              <label class="dialog-field">
                <span>关键词</span>
                <input v-model="smartFolderQuery" type="text" placeholder="文件名、标签或元数据" />
              </label>
              <label class="dialog-field">
                <span>路径前缀</span>
                <input v-model="smartFolderPathPrefix" type="text" placeholder="Campaigns/Summer" />
              </label>
              <label class="dialog-field">
                <span>格式</span>
                <input v-model="smartFolderFormats" type="text" placeholder="psd，png" />
              </label>
              <label class="dialog-field">
                <span>标签</span>
                <input v-model="smartFolderTags" type="text" placeholder="封面，主视觉" />
              </label>
              <label class="dialog-field">
                <span>颜色</span>
                <input v-model="smartFolderColors" type="text" placeholder="红色，绿色" />
              </label>
              <label class="dialog-field">
                <span>形状</span>
                <input v-model="smartFolderShapes" type="text" placeholder="方形，横版" />
              </label>
              <label class="dialog-field">
                <span>最低评分</span>
                <input v-model="smartFolderMinRating" type="number" min="1" max="5" step="1" placeholder="4" />
              </label>
              <label class="dialog-field">
                <span>匹配方式</span>
                <select v-model="smartFolderMatchMode">
                  <option value="and">全部匹配</option>
                  <option value="or">任一匹配</option>
                </select>
              </label>
            </div>

            <label class="dialog-field">
              <span>元数据键值</span>
              <textarea v-model="smartFolderMetadataFilters" rows="3" placeholder="artist=demo&#10;source=reference" />
            </label>

            <div class="smart-folder-dialog__grid">
              <label class="dialog-field">
                <span>排除关键词</span>
                <input v-model="smartFolderExcludeQuery" type="text" placeholder="draft，archive" />
              </label>
              <label class="dialog-field">
                <span>排除路径</span>
                <input v-model="smartFolderExcludePathPrefixes" type="text" placeholder="Archive，Temp" />
              </label>
              <label class="dialog-field">
                <span>排除标签</span>
                <input v-model="smartFolderExcludeTags" type="text" placeholder="草稿，临时" />
              </label>
              <label class="dialog-field">
                <span>排除格式</span>
                <input v-model="smartFolderExcludeFormats" type="text" placeholder="gif，webp" />
              </label>
              <label class="dialog-field">
                <span>排序字段</span>
                <input v-model="smartFolderSortField" type="text" placeholder="modifiedAt / rating / metadata.width" />
              </label>
              <label class="dialog-field">
                <span>排序方向</span>
                <select v-model="smartFolderSortDirection">
                  <option value="asc">升序</option>
                  <option value="desc">降序</option>
                </select>
              </label>
              <label class="dialog-field">
                <span>结果数量</span>
                <input v-model="smartFolderLimit" type="number" min="1" step="1" placeholder="100" />
              </label>
            </div>

            <label class="dialog-field">
              <span>排除元数据</span>
              <textarea v-model="smartFolderExcludeMetadataFilters" rows="2" placeholder="status=archived" />
            </label>

            <div class="smart-folder-dialog__grid">
              <label class="dialog-field">
                <span>排除数值范围</span>
                <textarea v-model="smartFolderExcludeNumberFilters" rows="2" placeholder="width=0..640" />
              </label>
              <label class="dialog-field">
                <span>排除日期范围</span>
                <textarea v-model="smartFolderExcludeDateFilters" rows="2" placeholder="fileCreatedAt=2024-01-01T00:00:00Z..2024-01-31T00:00:00Z" />
              </label>
            </div>

            <label class="dialog-field">
              <span>数值范围</span>
              <textarea v-model="smartFolderNumberFilters" rows="2" placeholder="width=1024..4096&#10;originalSizeBytes=..10485760" />
            </label>

            <label class="dialog-field">
              <span>日期范围</span>
              <textarea v-model="smartFolderDateFilters" rows="2" placeholder="fileCreatedAt=2024-01-01T00:00:00Z..2024-12-31T23:59:59Z" />
            </label>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingSmartFolder" @click="emit('closeSmartFolderDialog')">
              取消
            </button>
            <button type="button" class="primary" :disabled="smartFolderDialogDisabled" @click="emit('submitSmartFolderDialog')">
              <LoaderCircle v-if="isMutatingSmartFolder" class="spin" :size="13" aria-hidden="true" />
              {{ smartFolderDialogActionLabel }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>

  <Teleport to="body">
    <Transition name="modal">
      <div v-if="showSmartFolderDeleteDialog" class="modal-overlay" role="dialog" aria-modal="true" aria-label="删除智能文件夹" @click.self="emit('closeSmartFolderDelete')">
        <div class="modal-card dialog-card folder-delete-dialog">
          <div class="dialog-card__header dialog-card__header--danger">
            <span>删除智能文件夹</span>
          </div>
          <div class="dialog-card__body folder-delete-dialog__body">
            <p>将删除“{{ pendingDeleteSmartFolderLabel }}”及其子智能文件夹。实际文件和真实目录不会被删除。</p>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingSmartFolder" @click="emit('closeSmartFolderDelete')">
              取消
            </button>
            <button type="button" class="ghost danger" :disabled="isMutatingSmartFolder" @click="emit('confirmDeleteSmartFolder')">
              删除
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

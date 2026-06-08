<script setup lang="ts">
const props = withDefaults(defineProps<{
  value: number;
  label?: string;
  detail?: string;
  indeterminate?: boolean;
}>(), {
  label: "",
  detail: "",
  indeterminate: false,
});

function clampPercent(value: number) {
  return Math.max(0, Math.min(100, Math.round(value)));
}
</script>

<template>
  <div
    class="progress-bar"
    :class="{ 'progress-bar--indeterminate': props.indeterminate }"
    role="progressbar"
    :aria-label="props.label || '进度'"
    :aria-valuemin="0"
    :aria-valuemax="100"
    :aria-valuenow="props.indeterminate ? undefined : clampPercent(props.value)"
  >
    <div class="progress-bar__track">
      <div class="progress-bar__fill" :style="{ width: `${clampPercent(props.value)}%` }"></div>
    </div>
    <div v-if="props.label || props.detail" class="progress-bar__meta">
      <span v-if="props.label">{{ props.label }}</span>
      <span v-if="props.detail">{{ props.detail }}</span>
      <span v-if="!props.indeterminate">{{ clampPercent(props.value) }}%</span>
    </div>
  </div>
</template>

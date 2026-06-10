<script setup lang="ts">
defineProps<{
  colors?: string[];
}>();
</script>

<template>
  <div v-if="colors?.length" class="thumbnail-palette" aria-label="缩略图色板">
    <span
      v-for="color in colors"
      :key="color"
      class="thumbnail-palette__swatch"
      :style="{ '--thumbnail-palette-color': color }"
      :title="color"
    ></span>
  </div>
</template>

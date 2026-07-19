<script setup lang="ts">
import { RouterView } from "vue-router";
import { ContextMenuHost, useTheme } from "./ui";
import { appUIPreset } from "./ui/preset";

const { theme } = useTheme();
</script>

<template>
  <component :is="appUIPreset.provider" :theme="theme" agent-id="momobako-ui">
    <RouterView />
    <ContextMenuHost />
  </component>
</template>

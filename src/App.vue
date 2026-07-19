<script setup lang="ts">
import type { Component } from "vue";
import { RouterView } from "vue-router";
import { ContextMenuHost, useTheme } from "./ui";
import { appUIPreset } from "./ui/preset";

const AppUIProvider = appUIPreset.provider as Component;
const { theme } = useTheme();
</script>

<template>
  <component :is="AppUIProvider" :theme="theme" agent-id="momobako-ui">
    <RouterView />
    <ContextMenuHost />
  </component>
</template>

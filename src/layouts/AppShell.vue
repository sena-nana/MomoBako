<script setup lang="ts">
import { RouterView } from "vue-router";
import TitleBar from "../components/TitleBar.vue";
import SecondaryPanel from "./SecondaryPanel.vue";
</script>

<template>
  <div class="shell">
    <TitleBar />
    <SecondaryPanel />
    <main class="shell__main">
      <RouterView />
    </main>
  </div>
</template>

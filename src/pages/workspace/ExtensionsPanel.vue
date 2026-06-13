<script setup lang="ts">
import PluginManagerPanel from "../../components/PluginManagerPanel.vue";
</script>

<template>
  <section class="extensions-workbench">
    <PluginManagerPanel
      title="文件系统与插件"
      eyebrow="拓展能力"
      subline="这里集中展示当前插件和后端能力。"
      search-placeholder="筛选导入器、脚本或元数据拓展"
      empty-title="没有匹配的插件"
      empty-description="试试其他关键词，或从 .momoplug 安装新的插件。"
    />
  </section>
</template>

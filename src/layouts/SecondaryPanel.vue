<script setup lang="ts">
import { RouterLink } from "vue-router";
import {
  FilePlus2,
  Folder,
  Home,
  MoreHorizontal,
  Plus,
  Search,
  Settings,
} from "lucide-vue-next";

const globalActions = [
  { key: "new", label: "新建", icon: FilePlus2 },
  { key: "search", label: "搜索", icon: Search },
];

const nav = [
  { to: "/", label: "概览", icon: Home },
];

function noop() {
  // 模板占位：业务项目可以在这里接入真实命令。
}
</script>

<template>
  <aside class="secondary-panel">
    <div class="sb-section sb-section--actions">
      <button
        v-for="action in globalActions"
        :key="action.key"
        type="button"
        class="sb-action"
        :title="action.label"
        :aria-label="action.label"
        @click="noop"
      >
        <component :is="action.icon" :size="16" aria-hidden="true" />
      </button>
    </div>

    <div class="sb-section">
      <div class="sb-section__header">
        <span class="sb-section__title">工作区</span>
        <div class="sb-section__tools">
          <button type="button" class="sb-icon-btn" title="添加" aria-label="添加" @click="noop">
            <Plus :size="14" aria-hidden="true" />
          </button>
        </div>
      </div>
      <nav class="sb-tree" aria-label="主导航">
        <RouterLink
          v-for="item in nav"
          :key="item.to"
          :to="item.to"
          class="sb-tree__row"
          active-class="is-active"
        >
          <component :is="item.icon" :size="14" aria-hidden="true" />
          <span class="sb-tree__name">{{ item.label }}</span>
        </RouterLink>
      </nav>
    </div>

    <div class="sb-section">
      <div class="sb-section__header">
        <span class="sb-section__title">示例分组</span>
        <div class="sb-section__tools">
          <button type="button" class="sb-icon-btn" title="更多" aria-label="更多" @click="noop">
            <MoreHorizontal :size="14" aria-hidden="true" />
          </button>
        </div>
      </div>
      <div class="sb-tree">
        <div class="sb-tree__row sb-tree__row--project">
          <Folder :size="14" aria-hidden="true" />
          <span class="sb-tree__name">Template Workspace</span>
        </div>
        <p class="sb-tree__empty">替换为你的业务导航。</p>
      </div>
    </div>

    <div class="sb-footer">
      <RouterLink
        to="/settings"
        class="sb-footer__btn"
        active-class="is-active"
        title="设置"
        aria-label="设置"
      >
        <Settings :size="16" aria-hidden="true" />
      </RouterLink>
      <span class="sb-conn sb-conn--ok">
        <span class="sb-conn__label">Ready</span>
      </span>
    </div>
  </aside>
</template>

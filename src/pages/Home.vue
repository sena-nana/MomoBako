<script setup lang="ts">
import { Copy, FolderOpen, MousePointer2, Pencil, Trash2 } from "lucide-vue-next";
import ViewTabs from "../components/ViewTabs.vue";

const cardMenu = [
  { id: "rename", label: "重命名", icon: Pencil, onSelect: () => {} },
  { id: "duplicate", label: "复制页面", icon: Copy, onSelect: () => {} },
  {
    id: "delete",
    label: "删除",
    icon: Trash2,
    danger: true,
    confirmLabel: "确认删除？再点一次",
    onSelect: () => {},
  },
];
</script>

<template>
  <section>
    <ViewTabs active="overview" />
    <div class="page-header">
      <div>
        <h1>Tauri 应用模板</h1>
        <p>从这里开始替换成你的业务页面。</p>
      </div>
      <button type="button" class="ghost">
        <FolderOpen :size="14" aria-hidden="true" />
        打开项目
      </button>
    </div>

    <div class="template-grid">
      <div class="card" v-context-menu="cardMenu">
        <h2>应用外壳</h2>
        <p class="muted">
          已包含自绘标题栏、可拖拽侧栏、暗浅主题和紧凑工作台 UI。
        </p>
      </div>
      <div class="card" v-context-menu="cardMenu">
        <h2>右键菜单接口</h2>
        <p class="muted">
          <MousePointer2 :size="13" aria-hidden="true" />
          组件可以在点击位置声明自己的菜单项。
        </p>
      </div>
      <div class="card">
        <h2>验证脚本</h2>
        <ul class="kv">
          <li><span>前端测试</span><span>yarn test</span></li>
          <li><span>前端构建</span><span>yarn build</span></li>
          <li><span>完整验证</span><span>yarn verify</span></li>
        </ul>
      </div>
    </div>
  </section>
</template>

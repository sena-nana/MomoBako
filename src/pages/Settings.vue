<script setup lang="ts">
import { Moon, Sun } from "lucide-vue-next";
import { useTheme } from "../composables/useTheme";

const { theme, setTheme } = useTheme();
</script>

<template>
  <section>
    <div class="page-header">
      <div>
        <h1>设置</h1>
        <p>这里只保留模板级偏好，业务配置由具体项目添加。</p>
      </div>
    </div>

    <div class="card">
      <h2>外观</h2>
      <div class="settings-row">
        <div class="settings-row__label">
          <div>主题</div>
          <div class="settings-row__hint">选择应用配色，立即生效并记忆到本地。</div>
        </div>
        <div class="segmented" role="radiogroup" aria-label="主题">
          <button
            type="button"
            role="radio"
            :aria-checked="theme === 'dark'"
            :class="{ 'is-active': theme === 'dark' }"
            @click="setTheme('dark')"
          >
            <Moon :size="14" aria-hidden="true" />
            暗色
          </button>
          <button
            type="button"
            role="radio"
            :aria-checked="theme === 'light'"
            :class="{ 'is-active': theme === 'light' }"
            @click="setTheme('light')"
          >
            <Sun :size="14" aria-hidden="true" />
            浅色
          </button>
        </div>
      </div>
      <div class="settings-row">
        <div class="settings-row__label">
          <div>语言</div>
          <div class="settings-row__hint">模板默认使用简体中文界面文案。</div>
        </div>
        <span class="muted">简体中文</span>
      </div>
    </div>

    <div class="card">
      <h2>关于</h2>
      <ul class="kv">
        <li><span>名称</span><span>Tauri Template</span></li>
        <li><span>版本</span><span>0.1.0</span></li>
        <li><span>框架</span><span>Tauri 2 + Vue 3</span></li>
      </ul>
    </div>
  </section>
</template>

import { ref, watch } from "vue";

export type Theme = "dark" | "light";

const STORAGE_KEY = "tauri-template.theme";
const DEFAULT_THEME: Theme = "dark";

function loadInitial(): Theme {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    // localStorage 不可用时回到默认主题。
  }
  return DEFAULT_THEME;
}

function apply(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
}

function persist(next: Theme): void {
  apply(next);
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // ignore
  }
}

const theme = ref<Theme>(loadInitial());
apply(theme.value);

watch(theme, persist);

export function useTheme() {
  return {
    theme,
    setTheme(next: Theme) {
      theme.value = next;
      persist(next);
    },
    toggleTheme() {
      const next = theme.value === "dark" ? "light" : "dark";
      theme.value = next;
      persist(next);
    },
  };
}

/**
 * MomoBako 主题兼容包装。
 * 继续兼容旧存储键，再委托给 LiliaUI 主题状态。
 */
import { useTheme as useLiliaTheme } from "@lilia/ui";
import { configureMomoBakoUiCore } from "./configure";

const STORAGE_KEY = "momobako.theme";
const LEGACY_STORAGE_KEY = "tauri-template.theme";

function migrateLegacyThemePreference() {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "light" || stored === "dark") return;

    const legacyStored = localStorage.getItem(LEGACY_STORAGE_KEY);
    if (legacyStored === "light" || legacyStored === "dark") {
      localStorage.setItem(STORAGE_KEY, legacyStored);
    }
  } catch {
    // localStorage 不可用时交给下游默认逻辑处理。
  }
}

export type { Theme } from "@lilia/ui";

export function useTheme() {
  configureMomoBakoUiCore();
  migrateLegacyThemePreference();
  return useLiliaTheme();
}

/**
 * MomoBako 主题状态包装。
 * 在读取主题状态前补齐 LiliaUI 全局配置。
 */
import { useTheme as useLiliaTheme } from "@lilia/ui/composables/useTheme";
import { configureMomoBakoUiCore } from "./configure";

export type { Theme } from "@lilia/ui/composables/useTheme";

export function useTheme() {
  configureMomoBakoUiCore();
  return useLiliaTheme();
}

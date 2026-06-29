/**
 * MomoBako 圆角偏好兼容包装。
 * 继续兼容旧存储键，并将默认半径保持为 8px。
 */
import {
  CORNER_RADIUS_MAX,
  CORNER_RADIUS_MIN,
  useCornerStyle as useLiliaCornerStyle,
} from "@lilia/ui";
import { configureMomoBakoUiCore } from "./configure";

const STORAGE_KEY = "momobako.corners";
const RADIUS_STORAGE_KEY = "momobako.cornerRadius";
const LEGACY_STORAGE_KEY = "tauri-template.corners";
const LEGACY_RADIUS_STORAGE_KEY = "tauri-template.cornerRadius";
const DEFAULT_CORNER_RADIUS = 8;

let initializedDefaultRadius = false;

function migrateLegacyCornerPreferences() {
  try {
    const storedStyle = localStorage.getItem(STORAGE_KEY);
    if (storedStyle !== "smooth" && storedStyle !== "round") {
      const legacyStyle = localStorage.getItem(LEGACY_STORAGE_KEY);
      if (legacyStyle === "smooth" || legacyStyle === "round") {
        localStorage.setItem(STORAGE_KEY, legacyStyle);
      }
    }

    const storedRadius = localStorage.getItem(RADIUS_STORAGE_KEY);
    if (storedRadius == null) {
      const legacyRadius = localStorage.getItem(LEGACY_RADIUS_STORAGE_KEY);
      if (legacyRadius != null) {
        localStorage.setItem(RADIUS_STORAGE_KEY, legacyRadius);
      }
    }
  } catch {
    // localStorage 不可用时交给下游默认逻辑处理。
  }
}

function hasStoredRadiusPreference() {
  try {
    return localStorage.getItem(RADIUS_STORAGE_KEY) != null
      || localStorage.getItem(LEGACY_RADIUS_STORAGE_KEY) != null;
  } catch {
    return false;
  }
}

export type { CornerStyle } from "@lilia/ui";
export { CORNER_RADIUS_MAX, CORNER_RADIUS_MIN };

export function useCornerStyle() {
  configureMomoBakoUiCore();
  migrateLegacyCornerPreferences();
  const hasStoredRadius = hasStoredRadiusPreference();
  const state = useLiliaCornerStyle();

  if (!initializedDefaultRadius) {
    initializedDefaultRadius = true;
    if (!hasStoredRadius) {
      state.setCornerRadius(DEFAULT_CORNER_RADIUS);
    }
  }

  return state;
}

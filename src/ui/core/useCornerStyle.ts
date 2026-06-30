/**
 * MomoBako 圆角偏好包装。
 * 保持项目默认半径为 8px。
 */
import {
  CORNER_RADIUS_MAX,
  CORNER_RADIUS_MIN,
  useCornerStyle as useLiliaCornerStyle,
} from "@lilia/ui";
import { configureMomoBakoUiCore } from "./configure";

const RADIUS_STORAGE_KEY = "momobako.cornerRadius";
const DEFAULT_CORNER_RADIUS = 8;

let initializedDefaultRadius = false;

function hasStoredRadiusPreference() {
  try {
    return localStorage.getItem(RADIUS_STORAGE_KEY) != null;
  } catch {
    return false;
  }
}

export type { CornerStyle } from "@lilia/ui";
export { CORNER_RADIUS_MAX, CORNER_RADIUS_MIN };

export function useCornerStyle() {
  configureMomoBakoUiCore();
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

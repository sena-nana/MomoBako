import { ref, watch } from "vue";

export type CornerStyle = "smooth" | "round";

const STORAGE_KEY = "momobako.corners";
const RADIUS_STORAGE_KEY = "momobako.cornerRadius";
const LEGACY_STORAGE_KEY = "tauri-template.corners";
const LEGACY_RADIUS_STORAGE_KEY = "tauri-template.cornerRadius";
const DEFAULT_CORNER_STYLE: CornerStyle = "smooth";
export const CORNER_RADIUS_MIN = 0;
export const CORNER_RADIUS_MAX = 20;
export const DEFAULT_CORNER_RADIUS = 8;

function isCornerStyle(value: string | null): value is CornerStyle {
  return value === "smooth" || value === "round";
}

function clampRadius(value: number): number {
  return Math.min(CORNER_RADIUS_MAX, Math.max(CORNER_RADIUS_MIN, value));
}

function loadInitialStyle(): CornerStyle {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (isCornerStyle(stored)) return stored;

    const legacyStored = localStorage.getItem(LEGACY_STORAGE_KEY);
    if (isCornerStyle(legacyStored)) {
      localStorage.setItem(STORAGE_KEY, legacyStored);
      return legacyStored;
    }
  } catch {
    // localStorage 不可用时回到默认圆角。
  }
  return DEFAULT_CORNER_STYLE;
}

function parseStoredRadius(value: string | null): number | null {
  const parsed = value === null ? NaN : Number.parseFloat(value);
  return Number.isFinite(parsed) ? clampRadius(parsed) : null;
}

function loadInitialRadius(): number {
  try {
    const stored = parseStoredRadius(localStorage.getItem(RADIUS_STORAGE_KEY));
    if (stored !== null) return stored;

    const legacyStored = parseStoredRadius(localStorage.getItem(LEGACY_RADIUS_STORAGE_KEY));
    if (legacyStored !== null) {
      localStorage.setItem(RADIUS_STORAGE_KEY, String(legacyStored));
      return legacyStored;
    }
  } catch {
    // localStorage 不可用时回到默认半径。
  }
  return DEFAULT_CORNER_RADIUS;
}

function applyCornerPreferences(style: CornerStyle, radius: number): void {
  const nextRadius = clampRadius(radius);
  document.documentElement.dataset.corners = style;
  document.documentElement.style.setProperty("--app-corner-radius", `${nextRadius}px`);
  try {
    localStorage.setItem(STORAGE_KEY, style);
    localStorage.setItem(RADIUS_STORAGE_KEY, String(nextRadius));
  } catch {
    // ignore
  }
}

const cornerStyle = ref<CornerStyle>(loadInitialStyle());
const cornerRadius = ref(loadInitialRadius());

watch(
  [cornerStyle, cornerRadius],
  ([style, radius]) => applyCornerPreferences(style, radius),
  { flush: "sync", immediate: true },
);

export function useCornerStyle() {
  applyCornerPreferences(cornerStyle.value, cornerRadius.value);

  return {
    cornerStyle,
    cornerRadius,
    setCornerStyle(next: CornerStyle) {
      cornerStyle.value = next;
    },
    setCornerRadius(next: number) {
      cornerRadius.value = clampRadius(next);
    },
  };
}

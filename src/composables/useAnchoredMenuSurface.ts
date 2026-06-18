import { nextTick, ref } from "vue";
import {
  clampAnchoredMenuPosition,
  createAnchoredMenuPosition,
  resolveMenuTransformOrigin,
  type AnchoredMenuPosition,
} from "./menuMotion";

/**
 * Shared anchored floating-surface state for menus and menu-like popovers.
 */
export function useAnchoredMenuSurface(
  initialPosition = createAnchoredMenuPosition(0, 0),
) {
  const surfaceEl = ref<HTMLElement | null>(null);
  const position = ref(initialPosition);
  const origin = ref(resolveMenuTransformOrigin(initialPosition));

  /**
   * Syncs the rendered geometry from an anchored position after the surface mounts.
   */
  async function syncPosition(nextPosition = position.value) {
    position.value = nextPosition;
    origin.value = resolveMenuTransformOrigin(nextPosition);
    await nextTick();
    const element = surfaceEl.value;
    if (!element) return;
    if (element.offsetWidth <= 0 || element.offsetHeight <= 0) return;
    const clampedPosition = clampAnchoredMenuPosition(
      nextPosition,
      element.offsetWidth,
      element.offsetHeight,
    );
    position.value = clampedPosition;
    origin.value = resolveMenuTransformOrigin(
      clampedPosition,
      element.offsetWidth,
      element.offsetHeight,
    );
  }

  function setPosition(nextPosition: AnchoredMenuPosition) {
    position.value = nextPosition;
    origin.value = resolveMenuTransformOrigin(nextPosition);
  }

  return {
    surfaceEl,
    position,
    origin,
    setPosition,
    syncPosition,
  };
}

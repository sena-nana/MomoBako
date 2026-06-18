/**
 * Shared anchored menu geometry helpers for dropdown and context-menu surfaces.
 */
export interface AnchoredMenuPosition {
  x: number;
  y: number;
  anchorX: number;
  anchorY: number;
}

export interface MenuTransformOrigin {
  x: number;
  y: number;
}

export type MenuPlacement = "top" | "bottom";

export const SB_MENU_EDGE_PADDING = 4;
export const SB_MENU_GAP = 6;
export const SB_MENU_POP_TRANSITION_MS = 180;
export const SB_LAYER_Z_INDEX = {
  dropdown: 1900,
  popover: 1900,
  contextMenu: 2000,
} as const;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/**
 * Stores the menu position together with the anchor point that drives the scale origin.
 */
export function createAnchoredMenuPosition(
  x: number,
  y: number,
  anchorX = x,
  anchorY = y,
): AnchoredMenuPosition {
  return { x, y, anchorX, anchorY };
}

/**
 * Creates a menu position from a trigger rect so dropdowns can share the same viewport logic.
 */
export function createPlacedAnchoredMenuPosition(
  triggerRect: DOMRect,
  placement: MenuPlacement,
  height = 0,
  anchorX = triggerRect.left + triggerRect.width / 2,
): AnchoredMenuPosition {
  const y = placement === "bottom"
    ? triggerRect.bottom + SB_MENU_GAP
    : triggerRect.top - height - SB_MENU_GAP;
  const anchorY = placement === "bottom" ? triggerRect.bottom : triggerRect.top;
  return createAnchoredMenuPosition(triggerRect.left, y, anchorX, anchorY);
}

/**
 * Clamps the menu box into the viewport while preserving its anchor point for animation.
 */
export function clampAnchoredMenuPosition(
  position: AnchoredMenuPosition,
  width: number,
  height: number,
): AnchoredMenuPosition {
  const maxX = Math.max(SB_MENU_EDGE_PADDING, window.innerWidth - width - SB_MENU_EDGE_PADDING);
  const maxY = Math.max(SB_MENU_EDGE_PADDING, window.innerHeight - height - SB_MENU_EDGE_PADDING);
  return {
    ...position,
    x: clamp(position.x, SB_MENU_EDGE_PADDING, maxX),
    y: clamp(position.y, SB_MENU_EDGE_PADDING, maxY),
  };
}

/**
 * Resolves the transform origin inside the rendered box so the pop animation follows the anchor.
 */
export function resolveMenuTransformOrigin(
  position: AnchoredMenuPosition,
  width = Number.POSITIVE_INFINITY,
  height = Number.POSITIVE_INFINITY,
): MenuTransformOrigin {
  return {
    x: clamp(position.anchorX - position.x, 0, width),
    y: clamp(position.anchorY - position.y, 0, height),
  };
}

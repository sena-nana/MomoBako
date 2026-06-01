import { reactive, type Component } from "vue";

export interface ContextMenuItem {
  id?: string;
  label: string;
  icon?: Component;
  disabled?: boolean;
  danger?: boolean;
  confirmLabel?: string;
  onSelect: () => void | Promise<void>;
}

export type ContextMenuProvider = (
  event: MouseEvent,
) => ContextMenuItem[] | null | undefined;

interface MenuState {
  open: boolean;
  x: number;
  y: number;
  items: ContextMenuItem[];
  pendingConfirmId: string | null;
}

const state = reactive<MenuState>({
  open: false,
  x: 0,
  y: 0,
  items: [],
  pendingConfirmId: null,
});

const providers = new WeakMap<Element, ContextMenuProvider>();
let installed = false;

export function registerContextMenu(
  element: Element,
  provider: ContextMenuProvider,
) {
  providers.set(element, provider);
  return () => providers.delete(element);
}

function itemKey(item: ContextMenuItem): string {
  return item.id ?? item.label;
}

function collectItemsFor(event: MouseEvent): ContextMenuItem[] {
  let node = event.target as Element | null;
  while (node) {
    const provider = providers.get(node);
    const items = provider?.(event);
    if (items?.length) return items;
    node = node.parentElement;
  }
  return [];
}

function openMenu(x: number, y: number, items: ContextMenuItem[]) {
  state.items = items;
  state.x = x;
  state.y = y;
  state.open = items.length > 0;
  state.pendingConfirmId = null;
}

export function openContextMenuAt(
  x: number,
  y: number,
  items: ContextMenuItem[],
) {
  if (!items.length) return;
  openMenu(x, y, items);
}

export function closeContextMenu() {
  if (!state.open) return;
  state.open = false;
  state.items = [];
  state.pendingConfirmId = null;
}

export function isContextMenuItemPending(item: ContextMenuItem): boolean {
  if (!item.confirmLabel) return false;
  return state.pendingConfirmId === itemKey(item);
}

export async function selectContextMenuItem(item: ContextMenuItem) {
  if (item.disabled) return;
  const key = itemKey(item);
  if (item.confirmLabel && state.pendingConfirmId !== key) {
    state.pendingConfirmId = key;
    return;
  }
  closeContextMenu();
  await item.onSelect();
}

export function installContextMenu() {
  if (installed || typeof window === "undefined") return;
  installed = true;

  window.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    const items = collectItemsFor(event);
    if (items.length) openMenu(event.clientX, event.clientY, items);
    else closeContextMenu();
  });

  window.addEventListener(
    "pointerdown",
    (event) => {
      if (!state.open) return;
      const target = event.target as Element | null;
      if (target?.closest?.(".ctx-menu")) return;
      closeContextMenu();
    },
    true,
  );

  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && state.open) {
      closeContextMenu();
      event.stopPropagation();
    }
  });

  window.addEventListener(
    "scroll",
    () => {
      if (state.open) closeContextMenu();
    },
    true,
  );
  window.addEventListener("resize", closeContextMenu);
  window.addEventListener("blur", closeContextMenu);
}

export function useContextMenu() {
  return {
    state,
    close: closeContextMenu,
    select: selectContextMenuItem,
  };
}

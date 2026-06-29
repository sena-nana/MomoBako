/**
 * 右键菜单指令兼容层。
 * 继续复用 MomoBako 现有菜单注册能力，并通过核心入口对外暴露。
 */
import type { Directive } from "vue";
import {
  registerContextMenu,
  type ContextMenuItem,
  type ContextMenuProvider,
} from "../composables/useContextMenu";

type ContextMenuValue = ContextMenuItem[] | ContextMenuProvider | null | undefined;

const cleanups = new WeakMap<Element, () => void>();

function rebind(element: Element, value: ContextMenuValue) {
  cleanups.get(element)?.();
  cleanups.delete(element);

  if (!value) return;

  const provider: ContextMenuProvider = typeof value === "function"
    ? value
    : () => value;
  cleanups.set(element, registerContextMenu(element, provider));
}

export const vContextMenu: Directive<Element, ContextMenuValue> = {
  mounted(element, binding) {
    rebind(element, binding.value);
  },
  updated(element, binding) {
    if (binding.value === binding.oldValue) return;
    rebind(element, binding.value);
  },
  beforeUnmount(element) {
    cleanups.get(element)?.();
    cleanups.delete(element);
  },
};

/**
 * MomoBako 核心 UI 统一入口。
 * 对齐 LiliaUI 基础层，并保留本项目所需的兼容配置。
 */
import {
  ConfirmDialog,
  Dropdown,
  ViewTabs,
} from "@lilia/ui";
import ContextMenuHost from "../../components/ContextMenuHost.vue";
import {
  closeContextMenu,
  finalizeClosedContextMenu,
  openContextMenuAt,
  registerContextMenu,
  selectContextMenuItem,
  useContextMenu,
} from "../../composables/useContextMenu";
import {
  installGlobalScrollbarVisibility,
  uninstallGlobalScrollbarVisibility,
} from "../../composables/useGlobalScrollbarVisibility";
import { useResizablePane } from "../../composables/useResizablePane";
import { vContextMenu } from "../../directives/contextMenu";
import { installContextMenu } from "../../composables/useContextMenu";
import type { ContextMenuItem, ContextMenuProvider } from "../../composables/useContextMenu";
import { configureMomoBakoUiCore } from "./configure";
import {
  CORNER_RADIUS_MAX,
  CORNER_RADIUS_MIN,
  useCornerStyle,
} from "./useCornerStyle";
import { useTheme } from "./useTheme";

export {
  closeContextMenu,
  ConfirmDialog,
  configureMomoBakoUiCore,
  CORNER_RADIUS_MAX,
  CORNER_RADIUS_MIN,
  ContextMenuHost,
  Dropdown,
  finalizeClosedContextMenu,
  ViewTabs,
  installContextMenu,
  installGlobalScrollbarVisibility,
  openContextMenuAt,
  registerContextMenu,
  selectContextMenuItem,
  uninstallGlobalScrollbarVisibility,
  useContextMenu,
  useResizablePane,
  vContextMenu,
  useCornerStyle,
  useTheme,
};

export type {
  ContextMenuItem,
  ContextMenuProvider,
};

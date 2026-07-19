/**
 * MomoBako UI 稳定入口。
 * 业务模块只依赖此 facade，具体视觉 Layer 由 preset 决定。
 */
export * from "@lilia/ui";
export * from "@lilia/ui/commands";
export * from "@lilia/ui/diagnostics";
export * from "@lilia/ui/layouts";
export * from "@lilia/ui/provider";
export {
  getNativeAppearanceAdapter,
  installCornerStyle,
  installLiliaContextMenu,
  installNativeAppearance,
  setNativeAppearanceAdapter,
  type NativeAppearanceAdapter,
  type NativeBackdropRequest,
} from "@lilia/ui/runtime";
export * from "@lilia/ui/runtime/tauri";
export * from "@lilia/ui/settings";
export * from "@lilia/ui/shell";
export * from "./core";

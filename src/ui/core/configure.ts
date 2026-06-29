/**
 * 共享的 LiliaUI 配置初始化。
 * 需要在主题、圆角等全局状态首次读取前完成。
 */
import { setLiliaAppConfig } from "@lilia/ui";
import type { LiliaAppConfig } from "@lilia/ui";

let configured = false;

function createMomoBakoUiConfig(): LiliaAppConfig {
  return {
    appName: "momobako",
    productTitle: "MomoBako",
    version: "0.1.0",
    storageKeyPrefix: "momobako",
  };
}

export function configureMomoBakoUiCore() {
  if (configured) return;
  setLiliaAppConfig(createMomoBakoUiConfig());
  configured = true;
}

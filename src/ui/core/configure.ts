/**
 * 共享的 LiliaUI 配置初始化。
 * 需要在主题、圆角等全局状态首次读取前完成。
 */
import { setLiliaUiConfig, type LiliaUiConfig } from "@lilia/ui/shell";

let configured = false;

function createMomoBakoUiConfig(): LiliaUiConfig {
  return {
    appName: "momobako",
    productTitle: "MomoBako",
    version: "0.1.0",
    storageKeyPrefix: "momobako",
  };
}

export function configureMomoBakoUiCore() {
  if (configured) return;
  setLiliaUiConfig(createMomoBakoUiConfig());
  configured = true;
}

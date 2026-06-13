import type { FileBrowserEntry, HardlinkCandidate } from "../../types/repository";

export function hardlinkStateLabel(entry: FileBrowserEntry) {
  switch (entry.hardlinkState) {
    case "primary":
      return "主归属";
    case "linked":
      return "硬链接关联";
    case "copied":
    case "copiedFallback":
      return "普通复制";
    case "broken":
      return "关联异常";
    case "missing":
      return "关联缺失";
    default:
      return "";
  }
}

export function hardlinkCandidateMessage(candidate: HardlinkCandidate) {
  return `${candidate.newPath} 与 ${candidate.existingPath} 内容哈希一致，大小 ${candidate.sizeLabel}。确认后会将新文件加入硬链接关联。`;
}

export function statusLabel(status: string) {
  switch (status) {
    case "synced":
      return "已同步";
    case "processing":
      return "处理中";
    case "indexed":
      return "已索引";
    case "deleted":
      return "已删除";
    case "ready":
      return "已同步";
    default:
      return status;
  }
}

export function fileTone(entry: FileBrowserEntry) {
  if (entry.kind === "directory") {
    return "linear-gradient(135deg, #c7a566 0%, #73552f 100%)";
  }
  return "var(--thumbnail-placeholder-bg)";
}

export function entryDeletedAtLabel(entry: FileBrowserEntry) {
  const value = entry.metadata?.deletedAt;
  if (typeof value !== "string" || !value) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("zh-CN");
}

export function entryModifiedAtLabel(entry: FileBrowserEntry) {
  return entry.modifiedAt ? new Date(entry.modifiedAt).toLocaleString("zh-CN") : "未记录";
}

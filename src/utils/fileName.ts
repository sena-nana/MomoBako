/**
 * 文件名展示工具。
 * 统一处理资源库内文件名的去扩展名显示，避免各个视图重复实现。
 */
export function displayNameWithoutExtension(name: string, extension?: string | null) {
  const normalizedName = name.trim();
  const normalizedExtension = extension?.trim();
  if (!normalizedName || !normalizedExtension) return normalizedName;
  const suffix = `.${normalizedExtension}`;
  return normalizedName.toLowerCase().endsWith(suffix.toLowerCase())
    ? normalizedName.slice(0, -suffix.length)
    : normalizedName;
}

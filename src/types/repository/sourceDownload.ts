// 来源插件下载（`contributes.source.entryActions[]` 的 `download-entry` / `download-directory`）的载荷去处和结果，
// 约定见 docs/api-design.md。

/**
 * 来源下载载荷里的去处：用户选的本地目录，或者目标资源库。下载到其他资源库时宿主给这次下载新建一个空的暂存目录放在
 * `stagingPath`，插件把文件都写在里面，宿主导入以后清掉。
 */
export type SourceDownloadDestination =
  | { kind: "localFolder"; path: string }
  | { kind: "repository"; repoId: string; path?: string; parentPath?: string; stagingPath?: string };

/**
 * 来源下载里一项的结果。`paths` 只列这次写下的文件；下载到本地目录时，目录里已有的同名文件保留原样、不写入，
 * 列在 `skippedEntries`（绝对路径，没有时不写这个键）。
 */
export type SourceDownloadEntryResult = {
  songId?: number;
  songName?: string | null;
  paths: string[];
  skippedEntries?: string[];
};

/** 来源下载里失败的一项。 */
export type SourceDownloadFailure = {
  songId?: number;
  songName?: string | null;
  error: string;
};

/** 来源下载的结果：单曲是一项（`paths`、`skippedEntries`），歌单是一批（`completed`、`failed`）。部分失败时调用照样成功。 */
export type SourceDownloadResult = Partial<SourceDownloadEntryResult> & {
  playlistId?: number;
  playlistName?: string;
  completed?: SourceDownloadEntryResult[];
  failed?: SourceDownloadFailure[];
  summary?: { total: number; succeeded: number; failed: number };
};

//! MomoBako 内建长任务的稳定协议标识，以及宿主侧的类型化协议。

use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use crate::services::repository::{
    DownloaderPlaylistRequest, EagleLibraryImportRequest, EagleLibraryImportResponse,
    EntryDeleteRequest, EntryDeleteResponse, EntryPlaybackRequest, EntryPlaybackSourceResponse,
    FileArchiveImportRequest, FileBrowserSnapshot, FileCopyRequest, FileImportRequest,
    FileImportResponse, FileMoveRequest, RepositoryActionRunRequest, RepositoryActionRunResponse,
    RepositoryExportRequest, RepositoryExportResponse, RepositoryFolderRequest,
    RepositoryMutationRequest, RepositoryMutationResponse, RepositoryRelocateRequest, SyncRequest,
    SyncResult, ThumbnailRequest, ThumbnailResponse,
};

pub const PROTOCOL_REPOSITORY_CREATE: &str = "momobako.repository.create";
pub const PROTOCOL_REPOSITORY_IMPORT: &str = "momobako.repository.import";
pub const PROTOCOL_REPOSITORY_ATTACH: &str = "momobako.repository.attach";
pub const PROTOCOL_REPOSITORY_RELOCATE: &str = "momobako.repository.relocate";
pub const PROTOCOL_REPOSITORY_EXPORT: &str = "momobako.repository.export";
pub const PROTOCOL_REPOSITORY_SYNC: &str = "momobako.repository.sync";
pub const PROTOCOL_ENTRY_IMPORT: &str = "momobako.entry.import";
pub const PROTOCOL_ARCHIVE_IMPORT: &str = "momobako.archive.import";
pub const PROTOCOL_EAGLE_IMPORT: &str = "momobako.eagle.import";
pub const PROTOCOL_ENTRY_COPY: &str = "momobako.entry.copy";
pub const PROTOCOL_ENTRY_MOVE: &str = "momobako.entry.move";
pub const PROTOCOL_ENTRY_DELETE: &str = "momobako.entry.delete";
pub const PROTOCOL_REPOSITORY_ACTION_RUN: &str = "momobako.repository.action.run";
pub const PROTOCOL_THUMBNAIL_REQUEST: &str = "momobako.thumbnail.request";
pub const PROTOCOL_PLAYBACK_PREPARE: &str = "momobako.playback.prepare";
pub const PROTOCOL_PLAYLIST_DOWNLOAD: &str = "momobako.playlist.download";

/// 宿主提交的内建长任务：协议标识、请求类型和结果类型绑在一起，宿主拿到的是带类型的结果
/// （见 `MomoTaskRuntime::run`）。任务在 runner 里照旧按 JSON 收发，这里只是两头的类型。
pub trait TaskProtocol {
    const ID: &'static str;
    type Request: Serialize + Send + 'static;
    type Response: DeserializeOwned;
}

/// 任务的结果和它报过的进度事件。
#[derive(Debug, Clone)]
pub struct TaskOutput<T> {
    pub result: T,
    pub events: Vec<Value>,
}

macro_rules! task_protocol {
    ($(#[$doc:meta])* $name:ident, $id:ident, $request:ty => $response:ty) => {
        $(#[$doc])*
        pub struct $name;

        impl TaskProtocol for $name {
            const ID: &'static str = $id;
            type Request = $request;
            type Response = $response;
        }
    };
}

task_protocol!(/// 新建资源库。
    RepositoryCreate, PROTOCOL_REPOSITORY_CREATE, RepositoryMutationRequest => RepositoryMutationResponse);
task_protocol!(/// 导入已有的资源库目录。
    RepositoryImport, PROTOCOL_REPOSITORY_IMPORT, RepositoryMutationRequest => RepositoryMutationResponse);
task_protocol!(/// 把本地文件夹附加成资源库。
    RepositoryAttach, PROTOCOL_REPOSITORY_ATTACH, RepositoryFolderRequest => RepositoryMutationResponse);
task_protocol!(/// 资源库换位置。
    RepositoryRelocate, PROTOCOL_REPOSITORY_RELOCATE, RepositoryRelocateRequest => RepositoryMutationResponse);
task_protocol!(/// 导出资源库。
    RepositoryExport, PROTOCOL_REPOSITORY_EXPORT, RepositoryExportRequest => RepositoryExportResponse);
task_protocol!(/// 全量同步资源库。
    RepositorySync, PROTOCOL_REPOSITORY_SYNC, SyncRequest => SyncResult);
task_protocol!(/// 从外部路径导入文件。
    EntryImport, PROTOCOL_ENTRY_IMPORT, FileImportRequest => FileImportResponse);
task_protocol!(/// 解开 ZIP 导入。
    ArchiveImport, PROTOCOL_ARCHIVE_IMPORT, FileArchiveImportRequest => FileBrowserSnapshot);
task_protocol!(/// 导入 Eagle 资源库。
    EagleImport, PROTOCOL_EAGLE_IMPORT, EagleLibraryImportRequest => EagleLibraryImportResponse);
task_protocol!(/// 复制条目。
    EntryCopy, PROTOCOL_ENTRY_COPY, FileCopyRequest => FileBrowserSnapshot);
task_protocol!(/// 移动条目。
    EntryMove, PROTOCOL_ENTRY_MOVE, FileMoveRequest => FileBrowserSnapshot);
task_protocol!(/// 删除一个或一批条目。
    EntryDelete, PROTOCOL_ENTRY_DELETE, EntryDeleteRequest => EntryDeleteResponse);
task_protocol!(/// 运行资源库动作。
    RepositoryActionRun, PROTOCOL_REPOSITORY_ACTION_RUN, RepositoryActionRunRequest => RepositoryActionRunResponse);
task_protocol!(/// 生成或保存缩略图。
    ThumbnailRequestTask, PROTOCOL_THUMBNAIL_REQUEST, ThumbnailRequest => ThumbnailResponse);
task_protocol!(/// 准备条目的播放源。
    PlaybackPrepare, PROTOCOL_PLAYBACK_PREPARE, EntryPlaybackRequest => EntryPlaybackSourceResponse);
task_protocol!(/// 下载播放列表；结果是来源插件给的载荷。
    PlaylistDownload, PROTOCOL_PLAYLIST_DOWNLOAD, DownloaderPlaylistRequest => Value);

/// 返回宿主内建 runner 接受的全部协议。
#[cfg(test)]
const fn all() -> [&'static str; 16] {
    [
        PROTOCOL_REPOSITORY_CREATE,
        PROTOCOL_REPOSITORY_IMPORT,
        PROTOCOL_REPOSITORY_ATTACH,
        PROTOCOL_REPOSITORY_RELOCATE,
        PROTOCOL_REPOSITORY_EXPORT,
        PROTOCOL_REPOSITORY_SYNC,
        PROTOCOL_ENTRY_IMPORT,
        PROTOCOL_ARCHIVE_IMPORT,
        PROTOCOL_EAGLE_IMPORT,
        PROTOCOL_ENTRY_COPY,
        PROTOCOL_ENTRY_MOVE,
        PROTOCOL_ENTRY_DELETE,
        PROTOCOL_REPOSITORY_ACTION_RUN,
        PROTOCOL_THUMBNAIL_REQUEST,
        PROTOCOL_PLAYBACK_PREPARE,
        PROTOCOL_PLAYLIST_DOWNLOAD,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn protocol_ids_are_unique_and_namespaced() {
        let protocols = all();
        assert_eq!(
            protocols.iter().copied().collect::<BTreeSet<_>>().len(),
            protocols.len()
        );
        assert!(protocols
            .iter()
            .all(|protocol| protocol.starts_with("momobako.")));
    }
}

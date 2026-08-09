//! MomoBako 内建长任务的稳定协议标识。

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

//! 来源目录还有下一页时，空闲后再读下一页，只为生成缩略图，不把那一页画进列表。
//!
//! 对应 Vue `files.ts` 的 `queueVirtualThumbnailPrefetch`：当前页里有虚拟条目（网易云这类来源的单曲、
//! 歌单目录）、目录还有下一页时，等 `PREFETCH_IDLE_MS`（420ms），用继续加载的页大小再读一页。读回来只交给
//! 浏览时的缩略图队列（`files_thumbnail_ensure.rs`），列表、选择和「继续读取目录」都不变。换了目录、
//! 仓库或回收站再回来的这一页丢掉。读失败只记日志，不写进文件列的错误。回收站不预取：缩略图队列本来就不排回收站。
//! 没有虚拟条目的普通目录也不预取，和 Vue 一样。

use std::collections::HashSet;

use crate::backend::services::repository::FileBrowserSnapshot;

use super::super::motion::PREFETCH_IDLE_MS;
use super::{FileRow, FilesEffect, FilesState, normalize_path};

/// 正在等空闲，或者已经发出、还没回来的下一页。
#[derive(Clone, Debug, Default)]
pub(super) struct VirtualPagePrefetch {
    /// 空闲到这一刻再发读取。新的目录快照会把还没发出的计时清掉重排。
    due_ms: Option<u64>,
    /// 当前列表属于哪个仓库。结果回来时用它认是不是还在这一份目录上。
    list_repo: Option<String>,
    /// 计时结束要读的那一页。
    armed: Option<PageRequest>,
    /// 已经发出、还没回来的页。同一页不重复发。
    inflight: HashSet<String>,
}

/// 一次下一页读取。
#[derive(Clone, Debug, PartialEq, Eq)]
struct PageRequest {
    repo_id: String,
    path: String,
    trash: bool,
    offset: usize,
}

impl FilesState {
    /// 目录快照已经写进列表。有虚拟条目并且还有下一页时，重新等一个空闲间隔再读下一页。
    ///
    /// 同一页已经在读时不再排。普通文件目录、回收站、没有下一页时只清掉还没发出的计时。
    pub(in crate::shell::files) fn arm_virtual_page_prefetch(&mut self, repo_id: &str) {
        self.virtual_page.due_ms = None;
        self.virtual_page.armed = None;
        self.virtual_page.list_repo = Some(repo_id.to_string());
        if self.trash || !self.has_more || repo_id.is_empty() {
            return;
        }
        if !self
            .rows
            .iter()
            .chain(&self.virtual_rows)
            .any(|row| row.is_virtual)
        {
            return;
        }
        let offset = self.rows.len();
        let request = PageRequest {
            repo_id: repo_id.to_string(),
            path: self.current_path.clone(),
            trash: self.trash,
            offset,
        };
        if self.virtual_page.inflight.contains(&page_key(&request)) {
            return;
        }
        self.virtual_page.armed = Some(request);
        self.virtual_page.due_ms = Some(self.prefetch_clock.saturating_add(PREFETCH_IDLE_MS));
    }

    /// 空闲计时还没到。准备帧据此继续要帧。
    pub(in crate::shell::files) fn virtual_page_pending(&self) -> bool {
        self.virtual_page.due_ms.is_some()
    }

    /// 空闲到点就发出下一页读取。还没到、或者没有要读的页时什么也不做。
    ///
    /// 返回有没有发出。列表和加载标记都不动。
    pub fn poll_virtual_page_prefetch(&mut self, now_ms: u64) -> bool {
        let Some(due) = self.virtual_page.due_ms else {
            return false;
        };
        if now_ms < due {
            return false;
        }
        self.virtual_page.due_ms = None;
        let Some(request) = self.virtual_page.armed.take() else {
            eprintln!("Nana 下一页缩略图的空闲计时到了，却没有要读的页");
            return false;
        };
        let key = page_key(&request);
        if !self.virtual_page.inflight.insert(key) {
            eprintln!(
                "Nana 下一页缩略图已经在读，不重复发：{} @{}",
                request.path, request.offset
            );
            return false;
        }
        self.effects.push(FilesEffect::PrefetchVirtualPage {
            repo_id: request.repo_id,
            path: request.path,
            trash: request.trash,
            offset: request.offset,
            limit: super::browse::APPEND_PAGE_SIZE,
        });
        true
    }

    /// 下一页读回来了。还在请求时的仓库、目录和回收站上，才把条目交给缩略图队列；列表本身不换。
    ///
    /// 不是这次发出的、或者目录已经换了，只记日志。读取失败也不写文件列的错误。
    pub(in crate::shell) fn note_virtual_thumbnail_page(
        &mut self,
        repo_id: &str,
        path: &str,
        trash: bool,
        offset: usize,
        result: Result<FileBrowserSnapshot, String>,
    ) {
        let key = page_key(&PageRequest {
            repo_id: repo_id.to_string(),
            path: path.to_string(),
            trash,
            offset,
        });
        if !self.virtual_page.inflight.remove(&key) {
            eprintln!("Nana 下一页缩略图不是这次预取的，不生成：{repo_id} {path} @{offset}");
            return;
        }
        let snapshot = match result {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!("Nana 预取下一页缩略图失败：{path}：{error}");
                return;
            }
        };
        let same_repo = self.virtual_page.list_repo.as_deref() == Some(repo_id);
        let same_path = normalize_path(&self.current_path) == normalize_path(path);
        if !same_repo || !same_path || self.trash != trash {
            eprintln!("Nana 预取的下一页已经不是当前目录，不生成缩略图：{repo_id} {path}");
            return;
        }
        let rows: Vec<FileRow> = snapshot.entries.iter().map(FileRow::from_entry).collect();
        self.ensure.offer_unlisted(rows);
    }
}

/// 同一页的身份：仓库、规范化路径、是不是回收站、偏移。
fn page_key(request: &PageRequest) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        request.repo_id,
        normalize_path(&request.path),
        request.trash,
        request.offset
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use crate::backend::services::repository::{
        FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState,
    };
    use crate::shell::{FilesEffect, ShellMessage, ShellViewModel, WorkspacePanel};

    const REPO: &str = "repo";
    const COVER: &str = "https://music.example/next.jpg";

    fn entry(
        path: &str,
        is_virtual: bool,
        thumbnail: Option<&str>,
        cover: Option<&str>,
    ) -> FileBrowserEntry {
        let mut metadata = BTreeMap::new();
        let source_payload = cover.map(|url| json!({ "coverUrl": url }));
        if let Some(url) = cover {
            metadata.insert("coverUrl".into(), json!(url));
        }
        FileBrowserEntry {
            path: path.into(),
            name: path.rsplit('/').next().unwrap_or(path).into(),
            kind: "file".into(),
            extension: Some("mp3".into()),
            size_bytes: Some(10),
            size_label: Some("10 B".into()),
            modified_at: None,
            asset_id: Some(format!("asset:{path}")),
            status: None,
            thumbnail_path: thumbnail.map(str::to_string),
            thumbnail_custom: false,
            hardlink_group_id: None,
            hardlink_state: None,
            tags: Vec::new(),
            alias_paths: Vec::new(),
            folder_metadata: None,
            metadata,
            is_virtual,
            provider_id: None,
            provider_item_id: None,
            source_payload,
            local_absolute_path: None,
        }
    }

    fn browser(path: &str, total: usize, entries: Vec<FileBrowserEntry>) -> FileBrowserSnapshot {
        FileBrowserSnapshot {
            repo_id: REPO.into(),
            root_path: "C:/repo".into(),
            backend_plugin_id: "netease".into(),
            backend_kind: "source".into(),
            cache_state: RepositoryStructureCacheState::Ready,
            indexed_at: None,
            current_path: path.into(),
            total_entries: total,
            loaded_count: entries.len(),
            next_offset: (entries.len() < total).then_some(entries.len()),
            has_more: entries.len() < total,
            special_location: None,
            tree: None,
            entries,
        }
    }

    fn ready() -> ShellViewModel {
        let mut model = ShellViewModel::default();
        model
            .workspace
            .repositories
            .push(crate::shell::WorkspaceRepository {
                repo_id: REPO.into(),
                name: "库".into(),
                path: "C:/repo".into(),
                status: "ready".into(),
                backend_plugin_id: "filesystem".into(),
                capabilities: vec!["write".into()],
                cache_required: false,
                cache_status: String::new(),
                cache_path: None,
                login_expired: false,
                logged_out: false,
            });
        model.workspace.active_repo_id = Some(REPO.into());
        model.workspace.panel = WorkspacePanel::Files;
        model
    }

    fn prefetch_pages(model: &mut ShellViewModel) -> Vec<(String, usize)> {
        model
            .files
            .take_effects()
            .into_iter()
            .filter_map(|effect| match effect {
                FilesEffect::PrefetchVirtualPage {
                    path,
                    offset,
                    limit,
                    repo_id,
                    trash,
                } => {
                    assert_eq!(repo_id, REPO);
                    assert!(!trash);
                    assert_eq!(limit, 160);
                    Some((path, offset))
                }
                FilesEffect::Browse { .. } => panic!("下一页预取不该走会改列表的目录读取"),
                _ => None,
            })
            .collect()
    }

    fn ensure_jobs(model: &mut ShellViewModel) -> Vec<(String, Option<String>)> {
        model
            .files
            .take_effects()
            .into_iter()
            .filter_map(|effect| match effect {
                FilesEffect::EnsureThumbnail {
                    path, source_url, ..
                } => Some((path, source_url)),
                _ => None,
            })
            .collect()
    }

    /// 来源目录还有下一页：空闲 420ms 后只读下一页来生成封面，列表不增加，也不进入「继续读取目录」。
    /// 没有封面地址的不排。换了目录再回来的那一页不生成。普通文件目录即使还有下一页也不预取。
    #[test]
    fn virtual_directory_prefetches_the_next_page_thumbnails_without_listing_it() {
        let mut plain = ready();
        plain.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
            "photos",
            4,
            vec![entry("photos/a.png", false, None, None)],
        ))));
        let _ = plain.files.take_effects();
        assert!(
            !plain.files.poll_virtual_page_prefetch(420),
            "没有虚拟条目不预取下一页"
        );
        assert_eq!(plain.files.rows.len(), 1);
        assert!(!plain.files.loading_more);

        let mut model = ready();
        model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
            "playlists/night",
            3,
            vec![entry(
                "playlists/night/song.mp3",
                true,
                Some("thumbs/song.jpg"),
                Some(COVER),
            )],
        ))));
        assert!(prefetch_pages(&mut model).is_empty(), "还没到空闲间隔");
        assert!(!model.files.poll_virtual_page_prefetch(419));
        assert!(prefetch_pages(&mut model).is_empty());
        assert!(model.files.poll_virtual_page_prefetch(420));
        assert_eq!(
            prefetch_pages(&mut model),
            vec![("playlists/night".into(), 1)]
        );
        assert_eq!(
            model
                .files
                .rows
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            ["playlists/night/song.mp3"]
        );
        assert!(
            !model.files.loading && !model.files.loading_more,
            "预取不该显示继续读取"
        );
        assert!(model.files.error.is_empty());

        model.reduce(ShellMessage::VirtualThumbnailPage {
            repo_id: REPO.into(),
            path: "playlists/night".into(),
            trash: false,
            offset: 1,
            result: Ok(browser(
                "playlists/night",
                3,
                vec![
                    entry("playlists/night/next.mp3", true, None, Some(COVER)),
                    entry("playlists/night/bare.mp3", true, None, None),
                ],
            )),
        });
        assert_eq!(model.files.rows.len(), 1, "下一页不画进列表");
        assert!(!model.files.loading_more);
        assert!(model.files.error.is_empty());
        assert_eq!(
            ensure_jobs(&mut model),
            vec![("playlists/night/next.mp3".into(), Some(COVER.into()))],
            "没有封面地址的不排"
        );

        model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
            "playlists/night",
            3,
            vec![entry(
                "playlists/night/song.mp3",
                true,
                Some("thumbs/song.jpg"),
                Some(COVER),
            )],
        ))));
        let _ = model.files.take_effects();
        assert!(model.files.poll_virtual_page_prefetch(420));
        let _ = model.files.take_effects();
        model.reduce(ShellMessage::VirtualThumbnailPage {
            repo_id: REPO.into(),
            path: "playlists/night".into(),
            trash: false,
            offset: 1,
            result: Err("歌单分页超时".into()),
        });
        assert!(model.files.error.is_empty(), "预取失败不写进文件列");
        assert!(ensure_jobs(&mut model).is_empty());

        model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
            "playlists/night",
            3,
            vec![entry(
                "playlists/night/song.mp3",
                true,
                Some("thumbs/song.jpg"),
                Some(COVER),
            )],
        ))));
        let _ = model.files.take_effects();
        assert!(model.files.poll_virtual_page_prefetch(420));
        let _ = model.files.take_effects();
        model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
            "photos",
            1,
            vec![entry("photos/a.png", false, None, None)],
        ))));
        let _ = model.files.take_effects();
        model.reduce(ShellMessage::VirtualThumbnailPage {
            repo_id: REPO.into(),
            path: "playlists/night".into(),
            trash: false,
            offset: 1,
            result: Ok(browser(
                "playlists/night",
                3,
                vec![entry("playlists/night/next.mp3", true, None, Some(COVER))],
            )),
        });
        assert_eq!(
            model
                .files
                .rows
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            ["photos/a.png"]
        );
        assert!(ensure_jobs(&mut model).is_empty(), "换了目录的下一页不生成");
    }
}

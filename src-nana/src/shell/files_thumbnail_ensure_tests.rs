//! 浏览时生成缩略图的队列：经产品的壳层归约（目录快照、收尾、生成结果）走，断言排下的 `EnsureThumbnail`
//! 和写回的行。照 Vue `loadThumbnailsForSnapshot`：还没有缩略图的本地文件走 `ensure`；来源封面有真实
//! http(s) 地址时走 `save + sourceUrl`；没有地址不预取。回收站不生成。

use std::collections::BTreeMap;

use serde_json::json;

use crate::backend::services::repository::{
    FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState,
};
use crate::shell::{
    EnsuredThumbnail, FilesEffect, FilesMessage, ShellMessage, ShellViewModel, WorkspacePanel,
};

const REPO: &str = "repo";

fn entry(path: &str, kind: &str, thumbnail: Option<&str>) -> FileBrowserEntry {
    FileBrowserEntry {
        path: path.into(),
        name: path.rsplit('/').next().unwrap_or(path).into(),
        kind: kind.into(),
        extension: path
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_string()),
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
        metadata: BTreeMap::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    }
}

fn snapshot(
    repo_id: &str,
    path: &str,
    trash: bool,
    entries: Vec<FileBrowserEntry>,
) -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: repo_id.into(),
        root_path: "C:/repo".into(),
        backend_plugin_id: "local".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: path.into(),
        total_entries: entries.len(),
        loaded_count: entries.len(),
        next_offset: None,
        has_more: false,
        special_location: trash.then(|| "trash".to_string()),
        tree: None,
        entries,
    }
}

/// 根目录：文件夹、两张没有缩略图的图、一张已经有缩略图的、一个文本，以及一首来源上的虚拟歌曲。
fn root_entries() -> Vec<FileBrowserEntry> {
    let mut song = entry("song.mp3", "file", None);
    song.is_virtual = true;
    vec![
        entry("photos", "directory", None),
        entry("a.png", "file", None),
        entry("clip.mp4", "file", None),
        entry("done.png", "file", Some("thumbs/done.jpg")),
        entry("note.txt", "file", None),
        song,
    ]
}

/// 有活动仓库、在文件面板、首屏目录已经读回来的壳层。
fn browsing(entries: Vec<FileBrowserEntry>) -> ShellViewModel {
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
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot(
        REPO, "", false, entries,
    ))));
    model
}

/// 这次归约排下的生成或封面请求：路径，以及 `save` 用的真实地址（本地 `ensure` 为 `None`）。
fn jobs(model: &mut ShellViewModel) -> Vec<(String, Option<String>)> {
    model
        .files
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            FilesEffect::EnsureThumbnail {
                repo_id,
                path,
                source_url,
            } => {
                assert_eq!(repo_id, REPO, "按活动仓库请求");
                Some((path, source_url))
            }
            _ => None,
        })
        .collect()
}

/// 这次归约排下的本地生成请求。带来源地址的请求会让断言失败。
fn ensures(model: &mut ShellViewModel) -> Vec<String> {
    jobs(model)
        .into_iter()
        .map(|(path, source_url)| {
            assert!(source_url.is_none(), "本地生成不带来源地址：{path}");
            path
        })
        .collect()
}

/// 来源上的一行，并带上来源载荷。
fn virtual_entry(path: &str, kind: &str, payload: serde_json::Value) -> FileBrowserEntry {
    let mut row = entry(path, kind, None);
    row.is_virtual = true;
    row.source_payload = Some(payload);
    row
}

fn ensured(path: &str, thumbnail: Option<&str>) -> ShellMessage {
    let metadata = BTreeMap::from([(
        "thumbnailPalette".to_string(),
        json!(["#112233", "#445566"]),
    )]);
    ShellMessage::Files(FilesMessage::ThumbnailEnsured {
        repo_id: REPO.into(),
        path: path.into(),
        result: Ok(EnsuredThumbnail {
            thumbnail_path: thumbnail.map(str::to_string),
            custom: false,
            metadata: Some(metadata),
        }),
    })
}

fn failed(path: &str) -> ShellMessage {
    ShellMessage::Files(FilesMessage::ThumbnailEnsured {
        repo_id: REPO.into(),
        path: path.into(),
        result: Err("ffmpeg 不可用".into()),
    })
}

fn row<'a>(model: &'a ShellViewModel, path: &str) -> &'a crate::shell::FileRow {
    model
        .files
        .rows
        .iter()
        .find(|row| row.path == path)
        .expect("列表里有这一行")
}

/// 读回目录以后，逐条给还没有缩略图的本地文件生成：一次只排一条，结果回来再排下一条；文件夹、已经有缩略图的、
/// 虚拟条目不排；生成失败和后端交回空（文本生成不了）都不卡住队列，也不重试。
#[test]
fn browsing_generates_missing_thumbnails_one_at_a_time() {
    let mut model = browsing(root_entries());
    assert_eq!(
        ensures(&mut model),
        ["a.png"],
        "先排列表里第一个没有缩略图的文件，一次只排一条"
    );

    model.reduce(ShellMessage::Files(FilesMessage::ListResized(800.0)));
    assert!(ensures(&mut model).is_empty(), "在途的没回来，不排第二条");

    model.reduce(ensured("a.png", Some("thumbs/a.jpg")));
    let generated = row(&model, "a.png");
    assert_eq!(
        generated.thumbnail_path.as_deref(),
        Some("thumbs/a.jpg"),
        "生成的缩略图写回这一行"
    );
    assert!(!generated.texture_ready, "纹理等挂上以后再解码");
    assert_eq!(
        generated.palette,
        ["#112233", "#445566"],
        "色板跟着回来的元数据"
    );
    assert_eq!(ensures(&mut model), ["clip.mp4"]);

    model.reduce(failed("clip.mp4"));
    assert!(
        row(&model, "clip.mp4").thumbnail_path.is_none(),
        "失败的不假装有图"
    );
    assert_eq!(ensures(&mut model), ["note.txt"], "失败不卡住队列");

    model.reduce(ensured("note.txt", None));
    assert!(
        ensures(&mut model).is_empty(),
        "排过的不再排，文件夹、已有缩略图的和虚拟条目不排"
    );

    model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot(
        REPO,
        "",
        false,
        root_entries(),
    ))));
    assert!(
        ensures(&mut model).is_empty(),
        "重读同一个目录不重试失败的和生成不了的"
    );
}

/// 主选中的文件先生成，和 Vue `thumbnailPriority` 一样。
#[test]
fn the_selected_file_is_generated_first() {
    let mut model = browsing(root_entries());
    ensures(&mut model);
    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(
        "note.txt".into(),
    )));
    model.reduce(ensured("a.png", Some("thumbs/a.jpg")));
    assert_eq!(ensures(&mut model), ["note.txt"], "选中的文件插到前面");
}

/// 回收站不生成缩略图（Vue 跳过 `specialLocation === "trash"`）。
#[test]
fn the_trash_generates_nothing() {
    let mut model = browsing(Vec::new());
    ensures(&mut model);
    model.workspace.panel = WorkspacePanel::Trash;
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot(
        REPO,
        "",
        true,
        vec![entry("old.png", "file", None)],
    ))));
    assert!(ensures(&mut model).is_empty());
}

/// 换了目录就重新排：回到原来的目录时，上次失败的文件可以再试。
#[test]
fn another_directory_starts_a_new_queue() {
    let mut model = browsing(vec![entry("a.png", "file", None)]);
    assert_eq!(ensures(&mut model), ["a.png"]);
    model.reduce(failed("a.png"));
    assert!(ensures(&mut model).is_empty());

    model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot(
        REPO,
        "photos",
        false,
        vec![entry("photos/b.png", "file", None)],
    ))));
    assert_eq!(ensures(&mut model), ["photos/b.png"]);
    model.reduce(ensured("photos/b.png", Some("thumbs/b.jpg")));
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot(
        REPO,
        "",
        false,
        vec![entry("a.png", "file", None)],
    ))));
    assert_eq!(
        ensures(&mut model),
        ["a.png"],
        "回到根目录，失败过的再试一次"
    );
}

/// 结果回来时已经换了仓库：不写回新仓库里同名的文件，新仓库照常排自己的。
#[test]
fn a_result_for_another_repository_is_dropped() {
    let mut model = browsing(vec![entry("a.png", "file", None)]);
    assert_eq!(ensures(&mut model), ["a.png"]);
    model
        .workspace
        .repositories
        .push(crate::shell::WorkspaceRepository {
            repo_id: "other".into(),
            ..model.workspace.repositories[0].clone()
        });
    model.workspace.active_repo_id = Some("other".into());
    model.files.reset_for_repository();
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot(
        "other",
        "",
        false,
        vec![entry("a.png", "file", None)],
    ))));

    model.reduce(ensured("a.png", Some("thumbs/old-repo-a.jpg")));
    assert!(
        row(&model, "a.png").thumbnail_path.is_none(),
        "别的仓库的结果不写回"
    );
    let next: Vec<String> = model
        .files
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            FilesEffect::EnsureThumbnail { repo_id, path, .. } if repo_id == "other" => Some(path),
            _ => None,
        })
        .collect();
    assert_eq!(next, ["a.png"], "新仓库的文件照常生成");
}

const SONG_COVER: &str = "https://music.example/song.jpg";
const PLAYLIST_COVER: &str = "https://music.example/playlist.jpg";
const MIRRORED_COVER: &str = "https://music.example/mirrored.jpg";

/// 封面 `save` 的结果：后端交回缓存文件，并标成自定义。这里不带像素。
fn cover_saved(path: &str, thumbnail: &str) -> ShellMessage {
    ShellMessage::Files(FilesMessage::ThumbnailEnsured {
        repo_id: REPO.into(),
        path: path.into(),
        result: Ok(EnsuredThumbnail {
            thumbnail_path: Some(thumbnail.into()),
            custom: true,
            metadata: None,
        }),
    })
}

/// 歌曲用载荷里的 `coverUrl`，歌单目录用 `playlistCoverUrl`，只有元数据镜像时用元数据里的地址。
/// 选中的歌曲插到本地文件前面。空白、null、ftp 和相对路径不预取，也不写成缩略图。
#[test]
fn source_cover_uses_the_real_url() {
    let mut song = virtual_entry(
        "song.mp3",
        "file",
        json!({ "coverUrl": format!("  {SONG_COVER}  "), "songId": 186016 }),
    );
    song.name = "晴天.mp3".into();
    let mut mirrored = virtual_entry("mirrored.mp3", "file", json!({ "songId": 2 }));
    mirrored
        .metadata
        .insert("coverUrl".into(), json!(MIRRORED_COVER));
    let mut model = browsing(vec![
        entry("a.png", "file", None),
        entry("note.txt", "file", None),
        virtual_entry("bare.mp3", "file", json!({ "coverUrl": null })),
        song,
        virtual_entry(
            "liked",
            "directory",
            json!({ "entryKind": "playlist-folder", "playlistCoverUrl": PLAYLIST_COVER }),
        ),
        virtual_entry(
            "ftp.mp3",
            "file",
            json!({ "coverUrl": "ftp://files.example/a.jpg" }),
        ),
        mirrored,
    ]);
    assert_eq!(
        jobs(&mut model),
        vec![("a.png".into(), None)],
        "本地文件仍先走 ensure"
    );

    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(
        "song.mp3".into(),
    )));
    model.reduce(ensured("a.png", Some("thumbs/a.jpg")));
    assert_eq!(
        jobs(&mut model),
        vec![("song.mp3".into(), Some(SONG_COVER.into()))],
        "选中的歌曲用去掉空白的真实地址"
    );

    model.reduce(cover_saved("song.mp3", "thumbs/song.jpg"));
    let saved = row(&model, "song.mp3");
    assert_eq!(saved.thumbnail_path.as_deref(), Some("thumbs/song.jpg"));
    assert!(saved.thumbnail_custom, "来源封面存成自定义缩略图");
    assert!(!saved.texture_ready, "没有像素，不假装已经能画");
    assert!(saved.palette.is_empty(), "结果没带色板就不编造");

    assert_eq!(jobs(&mut model), vec![("note.txt".into(), None)]);
    model.reduce(ensured("note.txt", Some("thumbs/note.jpg")));
    assert_eq!(
        jobs(&mut model),
        vec![("liked".into(), Some(PLAYLIST_COVER.into()))]
    );

    model.reduce(cover_saved("liked", "thumbs/liked.jpg"));
    let folder = row(&model, "liked");
    assert_eq!(folder.kind, "directory");
    assert_eq!(folder.thumbnail_path.as_deref(), Some("thumbs/liked.jpg"));
    assert!(folder.thumbnail_custom);
    assert!(!folder.texture_ready);

    assert_eq!(
        jobs(&mut model),
        vec![("mirrored.mp3".into(), Some(MIRRORED_COVER.into()))],
        "载荷没有地址时用元数据里镜像的封面"
    );
    model.reduce(cover_saved("mirrored.mp3", "thumbs/mirrored.jpg"));
    assert!(
        jobs(&mut model).is_empty(),
        "没有真实地址的歌曲和空歌单不再排"
    );
    assert!(row(&model, "bare.mp3").thumbnail_path.is_none());
    assert!(row(&model, "ftp.mp3").thumbnail_path.is_none());
}

/// 封面地址缺失或不是 http(s) 时，整页都不预取。
#[test]
fn source_cover_without_a_url_is_not_prefetched() {
    let mut blank = virtual_entry("blank.mp3", "file", json!({ "coverUrl": "   " }));
    blank.metadata.insert("coverUrl".into(), json!(""));
    let mut model = browsing(vec![
        virtual_entry("bare.mp3", "file", json!({ "coverUrl": null })),
        virtual_entry(
            "ftp.mp3",
            "file",
            json!({ "coverUrl": "ftp://files.example/a.jpg" }),
        ),
        virtual_entry(
            "relative.mp3",
            "file",
            json!({ "coverUrl": "/covers/a.jpg" }),
        ),
        blank,
        virtual_entry(
            "empty-list",
            "directory",
            json!({ "playlistCoverUrl": null }),
        ),
    ]);
    assert!(jobs(&mut model).is_empty());
}

/// 回收站里即使有封面地址也不预取。
#[test]
fn source_cover_is_not_prefetched_in_the_trash() {
    let mut model = browsing(Vec::new());
    assert!(jobs(&mut model).is_empty());
    model.workspace.panel = WorkspacePanel::Trash;
    let song = virtual_entry("song.mp3", "file", json!({ "coverUrl": SONG_COVER }));
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot(
        REPO,
        "",
        true,
        vec![song],
    ))));
    assert!(jobs(&mut model).is_empty());
}

/// 本地文件生成不出来、但元数据里有封面地址时，紧接着用这个地址 `save` 一次。`save` 再空就不重试，后面的文件照常排。
#[test]
fn source_cover_follows_a_local_ensure_that_wrote_nothing() {
    let mut local = entry("a.png", "file", None);
    local
        .metadata
        .insert("coverUrl".into(), json!("https://music.example/a.jpg"));
    let mut model = browsing(vec![local, entry("b.png", "file", None)]);
    assert_eq!(jobs(&mut model), vec![("a.png".into(), None)]);

    model.reduce(ensured("a.png", None));
    assert!(
        row(&model, "a.png").thumbnail_path.is_none(),
        "生成不出来时不假装有图"
    );
    assert_eq!(
        jobs(&mut model),
        vec![("a.png".into(), Some("https://music.example/a.jpg".into()))]
    );

    model.reduce(ensured("a.png", None));
    assert!(
        row(&model, "a.png").thumbnail_path.is_none(),
        "封面也没写入时仍不编造"
    );
    assert_eq!(
        jobs(&mut model),
        vec![("b.png".into(), None)],
        "空的 save 不卡住后面的文件"
    );
}

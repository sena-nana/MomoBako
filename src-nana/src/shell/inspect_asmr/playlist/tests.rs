//! 播放列表的测试：加入作品、随机、清空、按资源库分开、写文件再读回，以及音轨的自然序。
//! 已加入条目的标题重算另见 `added_playlist_titles_follow_the_list_row_not_the_draft`。
//! 加入时的快照和卡片是否出现另见 `playlist_snapshot_and_card_follow_the_entry_not_the_draft`。

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde_json::json;

use super::super::AsmrMessage;
use super::super::tests::{asset_id, audio, detail, scene};
use super::{add_random_with, natural_cmp, panel, parse_items};
use crate::backend::services::repository::{PlaylistDetail, PlaylistItem, PlaylistSummary};
use crate::shell::player::PlayerMessage;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel};

fn work() -> Vec<(
    &'static str,
    std::collections::BTreeMap<String, serde_json::Value>,
)> {
    vec![
        (
            "work/1.mp3",
            audio(
                Some("work"),
                &[
                    ("trackTitle", json!("序章")),
                    ("listeningStatus", json!("listening")),
                ],
            ),
        ),
        (
            "work/2.mp3",
            audio(Some("work"), &[("workTitle", json!("夜曲"))]),
        ),
        ("work/10.mp3", audio(Some("work"), &[])),
    ]
}

fn lines(model: &crate::shell::ShellViewModel) -> Vec<(String, String, bool)> {
    panel(model)
        .map(|panel| {
            panel
                .items
                .into_iter()
                .map(|line| (line.title, line.status, line.current))
                .collect()
        })
        .unwrap_or_default()
}

/// 「加入作品」按自然序加入全部音轨。标题和状态分开：音轨名（没有时是文件名），右侧是收听状态或作品名；
/// 正在预览的那一条标成当前项，不把「当前」写进标题。改过就标脏，由宿主写文件；再加一次不重复。
#[test]
fn adding_the_work_lists_every_track_once() {
    let files = work();
    let mut model = scene(&files, "work/1.mp3");
    model.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    assert_eq!(
        lines(&model),
        [
            ("序章".into(), "收听中".into(), true),
            ("2.mp3".into(), "夜曲".into(), false),
            ("10.mp3".into(), String::new(), false),
        ]
    );
    assert!(model.asmr.take_playlist_dirty());
    model.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    assert_eq!(lines(&model).len(), 3);
    assert!(!model.asmr.take_playlist_dirty(), "没有新条目就不写文件");
    assert_eq!(panel(&model).expect("播放列表").count, "3 项");
}

/// 「随机」从还没加入的音轨里挑；都加过时从全部音轨里挑（加了也不重复）。
#[test]
fn random_picks_among_tracks_not_yet_added() {
    let files = work();
    let mut model = scene(&files, "work/1.mp3");
    add_random_with(&mut model, |len| {
        assert_eq!(len, 3);
        2
    });
    assert_eq!(lines(&model), [("10.mp3".into(), String::new(), false)]);
    add_random_with(&mut model, |len| {
        assert_eq!(len, 2, "只在没加入的两轨里挑");
        0
    });
    assert_eq!(
        lines(&model),
        [
            ("10.mp3".into(), String::new(), false),
            ("序章".into(), "收听中".into(), true)
        ]
    );
}

/// 「清空」只清当前资源库的条目；别的资源库的留着。
#[test]
fn clearing_keeps_other_repositories() {
    let files = work();
    let mut model = scene(&files, "work/1.mp3");
    model.asmr.playlist.items =
        parse_items(r#"[{"repoId":"other","path":"x.mp3","title":"别处"}]"#);
    model.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    model.reduce(ShellMessage::Asmr(AsmrMessage::ClearPlaylist));
    assert!(lines(&model).is_empty());
    assert_eq!(model.asmr.playlist.items.len(), 1, "别的资源库的条目留着");
}

/// 写进文件再读回来，条目一样；坏的项跳过，不是数组的文件当成空列表。
#[test]
fn the_playlist_survives_a_restart() {
    let files = work();
    let mut model = scene(&files, "work/1.mp3");
    model.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    let dir = std::env::temp_dir().join(format!("momobako-asmr-playlist-{}", std::process::id()));
    let path = dir.join("asmr-playlist.json");
    model.asmr.save_playlist_file(&path);
    let mut restored = super::super::AsmrUi::default();
    restored.load_playlist_file(&path);
    assert_eq!(restored.playlist.items, model.asmr.playlist.items);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        parse_items(
            r#"[{"path":""},{"title":"缺路径"},{"repoId":"r","path":"a.mp3","title":"a"}]"#
        )
        .len(),
        1
    );
    assert!(parse_items("{}").is_empty());
    let mut missing = super::super::AsmrUi::default();
    missing.load_playlist_file(&dir.join("没有这个文件.json"));
    assert!(missing.playlist.items.is_empty());
}

/// 已加入条目的标题重算跟 Vue `resolveActivePlaylist`：还在当前列表里的，用这一行自己的
/// `trackTitle`（没有就用文件名）和收听状态，不跟未保存草稿，也不跟详情叠层。
/// 不在列表里的保留存下的标题，不拿草稿或作品名编一个。
#[test]
fn added_playlist_titles_follow_the_list_row_not_the_draft() {
    let files = vec![
        (
            "work/1.mp3",
            audio(
                Some("work"),
                &[
                    ("trackTitle", json!("列表轨")),
                    ("listeningStatus", json!("listening")),
                    ("workTitle", json!("列表作品")),
                ],
            ),
        ),
        (
            "work/2.mp3",
            audio(
                Some("work"),
                &[
                    ("trackTitle", json!("第二轨")),
                    ("workTitle", json!("夜曲")),
                ],
            ),
        ),
    ];
    let mut model = scene(&files, "work/1.mp3");
    model.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    let repo = model.asmr.playlist.items[0].repo_id.clone();
    let stored = parse_items(&format!(
        r#"[{{"repoId":"{repo}","path":"gone.mp3","title":"存下的标题","status":"未收听","workTitle":"不该顶上"}}]"#
    ));
    model
        .asmr
        .playlist
        .items
        .push(stored.into_iter().next().expect("存下的条目"));
    let listed = [
        ("列表轨".to_string(), "收听中".to_string(), true),
        ("第二轨".to_string(), "夜曲".to_string(), false),
        ("存下的标题".to_string(), "未收听".to_string(), false),
    ];
    assert_eq!(lines(&model), listed);

    let overlaid = audio(
        Some("work"),
        &[
            ("trackTitle", json!("详情轨")),
            ("listeningStatus", json!("listened")),
            ("workTitle", json!("详情作品")),
        ],
    );
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        "work/1.mp3",
        &overlaid,
        2,
    ))));
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("trackTitle")
            .and_then(|value| value.as_str()),
        Some("详情轨"),
        "详情已进草稿"
    );
    assert_eq!(lines(&model), listed, "详情叠层不改已加入条目的标题");
    assert_eq!(
        list_title(&model, "work/1.mp3"),
        Some("列表轨"),
        "列表行自己的音轨名还在"
    );

    let patched = model.inspect.apply_metadata_patch(
        &asset_id("work/1.mp3"),
        BTreeMap::from([
            ("trackTitle".into(), json!("草稿轨")),
            ("listeningStatus".into(), json!("unlistened")),
            ("workTitle".into(), json!("草稿作品")),
        ]),
    );
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("trackTitle")
            .and_then(|value| value.as_str()),
        Some("草稿轨")
    );
    assert_eq!(lines(&model), listed, "未保存草稿不改已加入条目的标题");
    assert_eq!(
        list_title(&model, "work/1.mp3"),
        Some("列表轨"),
        "草稿不写回列表行"
    );

    let bare = vec![
        (
            "work/1.mp3",
            audio(
                Some("work"),
                &[
                    ("listeningStatus", json!("listening")),
                    ("workTitle", json!("列表作品")),
                ],
            ),
        ),
        (
            "work/2.mp3",
            audio(Some("work"), &[("trackTitle", json!("第二轨"))]),
        ),
    ];
    let mut unnamed = scene(&bare, "work/1.mp3");
    unnamed.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    assert_eq!(lines(&unnamed)[0].0, "1.mp3", "没有音轨名时用文件名");
    let drafted = audio(
        Some("work"),
        &[
            ("trackTitle", json!("草稿轨")),
            ("workTitle", json!("草稿作品")),
        ],
    );
    unnamed.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        "work/1.mp3",
        &drafted,
        2,
    ))));
    let patched = unnamed.inspect.apply_metadata_patch(
        &asset_id("work/1.mp3"),
        BTreeMap::from([("trackTitle".into(), json!("草稿轨"))]),
    );
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert_eq!(
        unnamed
            .inspect
            .draft_custom()
            .get("trackTitle")
            .and_then(|value| value.as_str()),
        Some("草稿轨")
    );
    assert_eq!(
        lines(&unnamed)[0].0,
        "1.mp3",
        "没有音轨名时不拿草稿或作品名编标题"
    );
    assert_eq!(lines(&unnamed)[0].1, "收听中", "收听状态仍是列表行上的");
    assert!(
        list_title(&unnamed, "work/1.mp3").is_none(),
        "列表行本来就没有音轨名"
    );
}

/// 加入时写入的快照跟 Vue `createPlaylistItem(previewFileEntry)`：列表行用这一行自己的元数据；
/// 没有作品目录、又不在列表里时，用播放集条目自己的元数据和文件名。
/// 卡片是否出现跟 `isAsmrAudioEntry(previewFileEntry)` 或这个资源库已经有条目，先要预览条目自己是 ASMR。
/// 详情叠层和未保存草稿不改快照，也不决定卡片出不出现。已加入条目的标题重算仍读列表行。
#[test]
fn playlist_snapshot_and_card_follow_the_entry_not_the_draft() {
    let files = vec![
        (
            "work/1.mp3",
            audio(
                Some("work"),
                &[
                    ("trackTitle", json!("列表轨")),
                    ("listeningStatus", json!("listening")),
                    ("workTitle", json!("列表作品")),
                ],
            ),
        ),
        (
            "work/2.mp3",
            audio(
                Some("work"),
                &[
                    ("trackTitle", json!("第二轨")),
                    ("workTitle", json!("夜曲")),
                ],
            ),
        ),
    ];
    let mut model = scene(&files, "work/1.mp3");
    contradict(&mut model, "work/1.mp3");
    let shown = playlist_card(&model).expect("列表行是 ASMR 音频，草稿改掉种类也不收起卡片");
    assert!(shown.audio, "加入按钮跟着列表行，不跟着草稿");
    assert!(shown.items.is_empty());
    let panels = super::super::preview_panels(&model).expect("作品队列还在");
    assert_eq!(panels.queue.expect("队列不跟草稿目录").1[0].title, "第二轨");

    model.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    assert_eq!(
        stored(&model),
        [
            (
                "work/1.mp3".into(),
                "列表轨".into(),
                "列表作品".into(),
                "收听中".into(),
                Some("work".into())
            ),
            (
                "work/2.mp3".into(),
                "第二轨".into(),
                "夜曲".into(),
                String::new(),
                Some("work".into())
            ),
        ]
    );
    assert_eq!(
        lines(&model),
        [
            ("列表轨".into(), "收听中".into(), true),
            ("第二轨".into(), "夜曲".into(), false)
        ],
        "已经加入的标题重算仍读列表行"
    );

    let image = vec![(
        "cover.mp3",
        audio(
            None,
            &[
                ("asmrEntryKind", json!("image")),
                ("trackTitle", json!("封面")),
            ],
        ),
    )];
    let mut still = scene(&image, "cover.mp3");
    force_audio_draft(&mut still, "cover.mp3");
    assert!(
        playlist_card(&still).is_none(),
        "列表行不是音频时，草稿不能把卡片变出来"
    );
    still.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    assert!(
        still.asmr.playlist.items.is_empty(),
        "草稿里的音轨名不能写成快照"
    );

    let plain = BTreeMap::from([("title".into(), json!("普通"))]);
    let mut foreign = scene(&[("note.mp3", plain)], "note.mp3");
    let repo = foreign
        .workspace
        .active_repo_id
        .clone()
        .expect("活动资源库");
    foreign.asmr.playlist.items = parse_items(&format!(
        r#"[{{"repoId":"{repo}","path":"kept.mp3","title":"留着"}}]"#
    ));
    force_audio_draft(&mut foreign, "note.mp3");
    assert!(
        playlist_card(&foreign).is_none(),
        "列表行不是 ASMR 时，草稿不能把已有播放列表的卡片挂出来"
    );

    let mut playlist = playlist_preview(
        "album/01.mp3",
        &audio(
            None,
            &[
                ("trackTitle", json!("播放集轨")),
                ("workTitle", json!("播放集作品")),
                ("listeningStatus", json!("listening")),
            ],
        ),
    );
    contradict(&mut playlist, "album/01.mp3");
    let shown = playlist_card(&playlist).expect("播放集条目是 ASMR 音频");
    assert!(shown.audio);
    assert_eq!(shown.count, "0 项");
    playlist.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    assert_eq!(
        stored(&playlist),
        [(
            "album/01.mp3".into(),
            "播放集轨".into(),
            "播放集作品".into(),
            "收听中".into(),
            None
        )]
    );
    assert_eq!(
        playlist.asmr.playlist.items[0].asset_id.as_deref(),
        Some(asset_id("album/01.mp3").as_str())
    );
    assert_eq!(
        lines(&playlist),
        [("播放集轨".into(), "收听中".into(), true)],
        "不在列表里的保留刚写入的快照"
    );

    let mut unnamed = playlist_preview(
        "album/01.mp3",
        &audio(None, &[("workTitle", json!("播放集作品"))]),
    );
    contradict(&mut unnamed, "album/01.mp3");
    unnamed.reduce(ShellMessage::Asmr(AsmrMessage::AddRandom));
    assert_eq!(
        stored(&unnamed),
        [(
            "album/01.mp3".into(),
            "01.mp3".into(),
            "播放集作品".into(),
            String::new(),
            None
        )]
    );
}

/// 把详情和未保存补丁都写成另一套 ASMR 字段，用来证明快照和卡片不读它们。
fn contradict(model: &mut ShellViewModel, path: &str) {
    let away = BTreeMap::from([
        ("libraryKind".into(), json!("comic")),
        ("asmrEntryKind".into(), json!("image")),
        ("trackTitle".into(), json!("草稿轨")),
        ("workTitle".into(), json!("草稿作品")),
        ("listeningStatus".into(), json!("unlistened")),
        ("workRoot".into(), json!("草稿目录")),
    ]);
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(path, &away, 2))));
    let patched = model.inspect.apply_metadata_patch(&asset_id(path), away);
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("trackTitle")
            .and_then(|value| value.as_str()),
        Some("草稿轨")
    );
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("libraryKind")
            .and_then(|value| value.as_str()),
        Some("comic")
    );
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("asmrEntryKind")
            .and_then(|value| value.as_str()),
        Some("image")
    );
}

/// 详情和补丁把当前文件说成 ASMR 音频。列表行或播放集条目自己不是时，卡片和快照都不跟。
fn force_audio_draft(model: &mut ShellViewModel, path: &str) {
    let drafted = audio(
        Some("draft-work"),
        &[
            ("trackTitle", json!("草稿轨")),
            ("workTitle", json!("草稿作品")),
            ("listeningStatus", json!("listening")),
        ],
    );
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        path, &drafted, 2,
    ))));
    let patched = model.inspect.apply_metadata_patch(&asset_id(path), drafted);
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("asmrEntryKind")
            .and_then(|value| value.as_str()),
        Some("audio")
    );
    assert_eq!(
        model
            .inspect
            .draft_custom()
            .get("trackTitle")
            .and_then(|value| value.as_str()),
        Some("草稿轨")
    );
}

fn playlist_card(model: &ShellViewModel) -> Option<super::PlaylistPanel> {
    super::super::preview_panels(model).and_then(|panels| panels.playlist)
}

/// 存下的快照：路径、标题、作品名、收听状态、作品目录。
fn stored(model: &ShellViewModel) -> Vec<(String, String, String, String, Option<String>)> {
    model
        .asmr
        .playlist
        .items
        .iter()
        .map(|item| {
            (
                item.path.clone(),
                item.title.clone(),
                item.work_title.clone(),
                item.status.clone(),
                item.work_root.clone(),
            )
        })
        .collect()
}

/// 打开不在当前列表里的播放集条目。文件名是路径最后一段。
fn playlist_preview(path: &str, metadata: &BTreeMap<String, serde_json::Value>) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.player.listed = Some(PlaylistDetail {
        playlist: PlaylistSummary {
            playlist_id: "pl".into(),
            repo_id: model.workspace.active_repo_id.clone().unwrap_or_default(),
            name: "专辑".into(),
            player_type_id: "audio".into(),
            player_plugin_id: "audio".into(),
            player_label: "音频".into(),
            file_class: "audio".into(),
            item_count: 1,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        },
        items: vec![PlaylistItem {
            playlist_item_id: "item".into(),
            playlist_id: "pl".into(),
            asset_id: asset_id(path),
            path: path.into(),
            filename: path.rsplit('/').next().unwrap_or(path).into(),
            extension: "mp3".into(),
            thumbnail_path: None,
            status: "ready".into(),
            status_reason: None,
            sort_order: 0,
            added_at: String::new(),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            metadata: Some(serde_json::to_value(metadata).expect("播放集条目的元数据")),
            local_absolute_path: None,
        }],
    });
    model.reduce(ShellMessage::Player(PlayerMessage::OpenPreview {
        item_id: Some("item".into()),
    }));
    assert!(
        crate::shell::files_view::previewing(&model),
        "播放集打开预览页"
    );
    model
}

/// 列表行上的音轨名。没有这个字段时是 `None`，不把空串当成有标题。
fn list_title<'a>(model: &'a ShellViewModel, path: &str) -> Option<&'a str> {
    model
        .files
        .rows
        .iter()
        .find(|row| row.path == path)
        .and_then(|row| row.metadata.get("trackTitle"))
        .and_then(|value| value.as_str())
}

/// 自然序：数字段按数值比，`2` 在 `10` 前面；前导 0 只在别的都一样时才分先后。
#[test]
fn natural_order_compares_numbers_by_value() {
    assert_eq!(natural_cmp("track2.mp3", "track10.mp3"), Ordering::Less);
    assert_eq!(natural_cmp("10.mp3", "9.mp3"), Ordering::Greater);
    assert_eq!(natural_cmp("a/2/x", "a/10/x"), Ordering::Less);
    assert_eq!(natural_cmp("b1", "a2"), Ordering::Greater);
    assert_ne!(natural_cmp("01.mp3", "1.mp3"), Ordering::Equal);
    assert_eq!(natural_cmp("1.mp3", "1.mp3"), Ordering::Equal);
}

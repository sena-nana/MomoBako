//! 播放列表的测试：加入作品、随机、清空、按资源库分开、写文件再读回，以及音轨的自然序。

use std::cmp::Ordering;

use serde_json::json;

use super::super::tests::{audio, scene};
use super::super::AsmrMessage;
use super::{add_random_with, natural_cmp, panel, parse_items};
use crate::shell::ShellMessage;

fn work() -> Vec<(&'static str, std::collections::BTreeMap<String, serde_json::Value>)> {
    vec![
        ("work/1.mp3", audio(Some("work"), &[("trackTitle", json!("序章")), ("listeningStatus", json!("listening"))])),
        ("work/2.mp3", audio(Some("work"), &[("workTitle", json!("夜曲"))])),
        ("work/10.mp3", audio(Some("work"), &[])),
    ]
}

fn labels(model: &crate::shell::ShellViewModel) -> Vec<String> {
    panel(model).map(|panel| panel.items.into_iter().map(|(_, label)| label).collect()).unwrap_or_default()
}

/// 「加入作品」按自然序加入全部音轨，标题是音轨名（没有时是文件名），后面是收听状态或作品名；当前项写「当前」。
/// 改过就标脏，由宿主写文件；再加一次不重复。
#[test]
fn adding_the_work_lists_every_track_once() {
    let files = work();
    let mut model = scene(&files, "work/1.mp3");
    model.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    assert_eq!(labels(&model), ["当前 序章 收听中", "2.mp3 夜曲", "10.mp3"]);
    assert!(model.asmr.take_playlist_dirty());
    model.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    assert_eq!(labels(&model).len(), 3);
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
    assert_eq!(labels(&model), ["10.mp3"]);
    add_random_with(&mut model, |len| {
        assert_eq!(len, 2, "只在没加入的两轨里挑");
        0
    });
    assert_eq!(labels(&model), ["10.mp3", "当前 序章 收听中"]);
}

/// 「清空」只清当前资源库的条目；别的资源库的留着。
#[test]
fn clearing_keeps_other_repositories() {
    let files = work();
    let mut model = scene(&files, "work/1.mp3");
    model.asmr.playlist.items = parse_items(r#"[{"repoId":"other","path":"x.mp3","title":"别处"}]"#);
    model.reduce(ShellMessage::Asmr(AsmrMessage::AddWork));
    model.reduce(ShellMessage::Asmr(AsmrMessage::ClearPlaylist));
    assert!(labels(&model).is_empty());
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

    assert_eq!(parse_items(r#"[{"path":""},{"title":"缺路径"},{"repoId":"r","path":"a.mp3","title":"a"}]"#).len(), 1);
    assert!(parse_items("{}").is_empty());
    let mut missing = super::super::AsmrUi::default();
    missing.load_playlist_file(&dir.join("没有这个文件.json"));
    assert!(missing.playlist.items.is_empty());
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

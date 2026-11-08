//! 音频预览的歌词。对齐 `player-audio` 的 `audioPreview.js` / `lyrics.js`。
//!
//! 播放源带回本地 `lyricPath`（或不是 http(s) 的 `lyricSourceUrl`）时读那个文件；都没有时读同级
//! `.lrc`。解析成 LRC 行以后画在唱片右边，播放头走到的那一行加重。没有文件、读失败、正文空，
//! 或只有网络地址时写「暂无歌词」，不下载、不编造歌词。音频舞台出来以后、文件还在读时写「读取歌词...」。
//! 逐字歌词 `wordLyricPath` 这首预览也不画。

use serde_json::Value;

use super::SourcePrep;

/// 播放源准备结果里和歌词有关的那一项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LyricHint {
    /// 本地歌词文件。
    Local(String),
    /// 只有 http(s) 地址。Nana 不下载它。
    RemoteOnly(String),
    /// 准备结果没带歌词，改读同级 `.lrc`。
    Absent,
}

/// 一行歌词。`time_ms` 为空是没有时间标签的行，不跟着播放头亮。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LyricLine {
    pub text: String,
    pub time_ms: Option<u64>,
}

/// 从准备结果里取歌词位置。Vue 先用 `lyricSourceUrl`，没有再用 `lyricPath`。
/// 网址是 http(s) 时，旁边若还有 `lyricPath` 就读本地文件（同一份歌词，不走网络）。
pub(crate) fn hint_from_prepare(output: &Value) -> LyricHint {
    let Some(fields) = output.as_object() else {
        return LyricHint::Absent;
    };
    let filled = |key: &str| {
        fields
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    if let Some(url) = filled("lyricSourceUrl") {
        if is_remote(&url) {
            return match filled("lyricPath") {
                Some(path) => LyricHint::Local(path),
                None => LyricHint::RemoteOnly(url),
            };
        }
        return LyricHint::Local(url);
    }
    filled("lyricPath")
        .map(LyricHint::Local)
        .unwrap_or(LyricHint::Absent)
}

/// 同级歌词路径。`song.mp3` 得到 `song.lrc`，没有点时在末尾加 `.lrc`。
pub(crate) fn sibling_lrc_path(path: &str) -> String {
    match path.rfind('.') {
        Some(index) => format!("{}.lrc", &path[..index]),
        None => format!("{path}.lrc"),
    }
}

/// 音频解出来以后记下还要读的歌词。舞台先出来，读的时候写「读取歌词...」。
/// 视频、解不开、只有网址不排这次读取，也不下载。
pub(crate) fn schedule_lyrics(prep: &mut SourcePrep, hint: LyricHint, playable_audio: bool) {
    if !playable_audio {
        return;
    }
    match hint {
        LyricHint::Local(path) => {
            prep.lyrics_pending = true;
            prep.lyric_path = Some(path);
        }
        LyricHint::Absent => {
            prep.lyrics_pending = true;
            prep.lyric_path = None;
        }
        LyricHint::RemoteOnly(url) => {
            eprintln!("Nana 歌词只有网络地址，不下载：{url}");
        }
    }
}

/// 按提示读出歌词正文。失败、空文件和只有网址都得到空串，调用方画「暂无歌词」。
pub(crate) fn text_for(
    hint: LyricHint,
    audio_path: &str,
    read_sibling: impl FnOnce(&str) -> Result<Vec<u8>, String>,
) -> String {
    match hint {
        LyricHint::Local(path) => match std::fs::read(&path) {
            Ok(bytes) => decode_lyric_bytes(&bytes),
            Err(error) => {
                eprintln!("Nana 读取歌词文件失败：{path}：{error}");
                String::new()
            }
        },
        LyricHint::RemoteOnly(url) => {
            eprintln!("Nana 歌词只有网络地址，不下载：{url}");
            String::new()
        }
        LyricHint::Absent => {
            let path = sibling_lrc_path(audio_path);
            match read_sibling(&path) {
                Ok(bytes) => decode_lyric_bytes(&bytes),
                Err(error) => {
                    eprintln!("Nana 当前音频没有同级歌词：{path}：{error}");
                    String::new()
                }
            }
        }
    }
}

/// UTF-8 歌词。开头的 BOM 丢掉；不是合法 UTF-8 时按替换字符解码并记日志。
pub(crate) fn decode_lyric_bytes(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("Nana 歌词不是合法 UTF-8：{error}");
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

/// 把 LRC 展开成按时间排序的行。一行里多个时间标签各占一行；没有时间标签的行排在最后。
pub(super) fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut parsed = Vec::new();
    for raw in normalized.split('\n') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let tags = time_tags(raw);
        let plain = strip_time_tags(raw);
        if tags.is_empty() {
            if !plain.is_empty() {
                parsed.push(LyricLine {
                    text: plain,
                    time_ms: None,
                });
            }
            continue;
        }
        let text = if plain.is_empty() {
            "…".to_string()
        } else {
            plain
        };
        for time_ms in tags {
            parsed.push(LyricLine {
                text: text.clone(),
                time_ms: Some(time_ms),
            });
        }
    }
    parsed.sort_by(|left, right| match (left.time_ms, right.time_ms) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(left), Some(right)) => left.cmp(&right),
    });
    parsed
}

/// 播放头已经走到的最后一行。没有时间标签的行不算。还没到第一行时是 `None`。
pub(super) fn active_index(lines: &[LyricLine], playback_ms: u64) -> Option<usize> {
    let mut index = None;
    for (cursor, line) in lines.iter().enumerate() {
        if line.time_ms.is_some_and(|time| time <= playback_ms) {
            index = Some(cursor);
        }
    }
    index
}

fn is_remote(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn time_tags(line: &str) -> Vec<u64> {
    let mut tags = Vec::new();
    let mut index = 0;
    while index < line.len() {
        if let Some((time_ms, next)) = parse_tag(line, index) {
            tags.push(time_ms);
            index = next;
            continue;
        }
        let width = line[index..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
        index += width;
    }
    tags
}

fn strip_time_tags(line: &str) -> String {
    let mut plain = String::new();
    let mut index = 0;
    while index < line.len() {
        if let Some((_, next)) = parse_tag(line, index) {
            index = next;
            continue;
        }
        let ch = line[index..].chars().next().unwrap_or('\u{FFFD}');
        plain.push(ch);
        index += ch.len_utf8();
    }
    plain.trim().to_string()
}

/// `[分:秒]` 或 `[分:秒.毫秒]` / `[分:秒:毫秒]`。对不上就不是时间标签。
fn parse_tag(line: &str, start: usize) -> Option<(u64, usize)> {
    let bytes = line.as_bytes();
    if bytes.get(start) != Some(&b'[') {
        return None;
    }
    let mut index = start + 1;
    let minutes = read_digits(bytes, &mut index, 1, 2)?;
    if bytes.get(index) != Some(&b':') {
        return None;
    }
    index += 1;
    let seconds = read_digits(bytes, &mut index, 1, 2)?;
    let fraction = if matches!(bytes.get(index), Some(b'.' | b':')) {
        index += 1;
        Some(read_digit_str(bytes, &mut index, 1, 3)?)
    } else {
        None
    };
    if bytes.get(index) != Some(&b']') {
        return None;
    }
    Some((
        timestamp_to_ms(minutes, seconds, fraction.as_deref()),
        index + 1,
    ))
}

fn read_digits(bytes: &[u8], index: &mut usize, min: usize, max: usize) -> Option<u64> {
    read_digit_str(bytes, index, min, max)?.parse().ok()
}

fn read_digit_str(bytes: &[u8], index: &mut usize, min: usize, max: usize) -> Option<String> {
    let start = *index;
    while *index < bytes.len() && *index - start < max && bytes[*index].is_ascii_digit() {
        *index += 1;
    }
    let count = *index - start;
    if count < min {
        *index = start;
        return None;
    }
    Some(std::str::from_utf8(&bytes[start..*index]).ok()?.to_string())
}

/// 小数按末尾补零再取前三位，和 `padEnd(3, "0").slice(0, 3)` 一样。
fn timestamp_to_ms(minutes: u64, seconds: u64, fraction: Option<&str>) -> u64 {
    let fraction = fraction
        .map(|value| {
            let mut padded = value.to_string();
            while padded.len() < 3 {
                padded.push('0');
            }
            padded.truncate(3);
            padded.parse::<u64>().unwrap_or(0)
        })
        .unwrap_or(0);
    minutes
        .saturating_mul(60_000)
        .saturating_add(seconds.saturating_mul(1000))
        .saturating_add(fraction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::services::repository::PlaybackSessionState;
    use crate::shell::view_harness::ShellHarness;
    use crate::shell::{InspectEffect, InspectMessage, ShellMessage, ShellViewModel, SourcePrep};

    /// 同级 `.lrc` 和播放源 `lyricPath` 的歌词画在音频舞台上，当前行跟着播放头走。
    /// 没有歌词、过期结果和预览失败都回到「暂无歌词」，不留下上一份正文。
    #[test]
    fn sibling_lrc_shows_on_the_audio_stage_and_follows_playback() {
        assert_eq!(sibling_lrc_path("music/song.mp3"), "music/song.lrc");
        assert_eq!(sibling_lrc_path("song"), "song.lrc");
        assert_eq!(sibling_lrc_path("folder.name/song"), "folder.lrc");

        let lrc = "[00:03.00][00:01.00]重唱\r\n[00:00.50]半秒\n作词：桃\n[00:02] \n";
        let lines = parse_lrc(lrc);
        assert_eq!(
            lines
                .iter()
                .map(|line| (line.time_ms, line.text.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (Some(500), "半秒"),
                (Some(1000), "重唱"),
                (Some(2000), "…"),
                (Some(3000), "重唱"),
                (None, "作词：桃")
            ]
        );
        assert_eq!(active_index(&lines, 0), None);
        assert_eq!(active_index(&lines, 500), Some(0));
        assert_eq!(active_index(&lines, 2000), Some(2));
        assert_eq!(active_index(&lines, 9_999), Some(3), "没有时间标签的行不亮");

        let bom = {
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend("[00:01.5]五百".as_bytes());
            decode_lyric_bytes(&bytes)
        };
        assert_eq!(parse_lrc(&bom)[0].time_ms, Some(1500));

        assert!(matches!(
            hint_from_prepare(&serde_json::json!({})),
            LyricHint::Absent
        ));
        assert!(matches!(
            hint_from_prepare(&serde_json::json!({ "lyricPath": "C:/cache/a.lrc" })),
            LyricHint::Local(path) if path == "C:/cache/a.lrc"
        ));
        assert!(matches!(
            hint_from_prepare(&serde_json::json!({ "lyricSourceUrl": "https://music.example/a.lrc" })),
            LyricHint::RemoteOnly(url) if url == "https://music.example/a.lrc"
        ));
        assert!(matches!(
            hint_from_prepare(&serde_json::json!({
                "lyricSourceUrl": "https://127.0.0.1/a.lrc",
                "lyricPath": "C:/cache/a.lrc"
            })),
            LyricHint::Local(path) if path == "C:/cache/a.lrc"
        ));

        let file = std::env::temp_dir().join(format!("momobako-lyric-{}.lrc", std::process::id()));
        std::fs::write(&file, "[00:00.00]文件里的一句").expect("写入临时歌词");
        let from_file = text_for(
            LyricHint::Local(file.display().to_string()),
            "song.mp3",
            |_| panic!("本地歌词不读同级"),
        );
        let _ = std::fs::remove_file(&file);
        assert!(from_file.contains("文件里的一句"));
        assert!(
            text_for(
                LyricHint::Local("D:/no/such-momobako.lrc".into()),
                "song.mp3",
                |_| panic!("缺文件不读同级")
            )
            .is_empty()
        );
        assert!(
            text_for(
                LyricHint::RemoteOnly("https://music.example/a.lrc".into()),
                "song.mp3",
                |_| panic!("网址不读同级")
            )
            .is_empty()
        );
        let mut seen = String::new();
        let from_sibling = text_for(LyricHint::Absent, "music/song.mp3", |path| {
            seen = path.to_string();
            Ok("[00:00.00]同级".as_bytes().to_vec())
        });
        assert_eq!(seen, "music/song.lrc");
        assert!(from_sibling.contains("同级"));
        assert!(
            text_for(LyricHint::Absent, "music/song.mp3", |_| Err(
                "not found".into()
            ))
            .is_empty()
        );

        let mut model = scene("preview-audio");
        assert!(
            show(&model).find("暂无歌词").is_some(),
            "还没读到歌词时写暂无歌词"
        );
        let path = model.inspect.target_path.clone().expect("音频预览路径");
        let generation = model.inspect.generation;
        let session = model.inspect.media_session().expect("音频会话").clone();
        let mut prep = SourcePrep::default();
        prep.lyric_text = "[00:00.00]第一句\n[00:02.00]第二句".into();
        model.reduce(loaded(
            path.clone(),
            generation,
            Ok(session.clone()),
            prep.clone(),
        ));
        assert_eq!(model.inspect.lyrics.len(), 2);
        let harness = show(&model);
        assert!(harness.find("暂无歌词").is_none(), "有歌词时不写暂无歌词");
        assert!(harness.find("第一句").is_some());
        assert!(harness.find("第二句").is_some());
        assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "第一句");

        let mut later = session.clone();
        later.current_time_ms = 2000;
        model.inspect.replace_shared_media(later);
        assert_eq!(
            node_text(&show(&model), "inspect-audio-lyric-active"),
            "第二句"
        );

        let mut stale = SourcePrep::default();
        stale.lyric_text = "[00:00.00]过期的一句".into();
        model.reduce(loaded(path.clone(), generation + 9, Ok(session), stale));
        assert_eq!(
            model
                .inspect
                .lyrics
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["第一句", "第二句"]
        );
        assert!(
            show(&model).find("过期的一句").is_none(),
            "过期的歌词结果丢掉"
        );

        let mut empty = SourcePrep::default();
        empty.lyric_text.clear();
        model.reduce(loaded(
            path.clone(),
            generation,
            Err("解码失败".into()),
            empty,
        ));
        let failed = show(&model);
        assert!(model.inspect.lyrics.is_empty(), "预览失败清掉歌词");
        assert!(failed.find("无法预览该音频").is_some());
        assert!(failed.find("第一句").is_none());
    }

    /// 音频舞台出来以后、歌词文件还在读时写「读取歌词...」。
    /// 读到正文就换成歌词；读完是空的写成「暂无歌词」。过期结果和预览失败都不留着这句。
    /// 视频、解不开、只有网址不排这次读取。
    #[test]
    fn audio_stage_says_reading_lyrics_until_the_file_arrives() {
        let mut remote = SourcePrep::default();
        schedule_lyrics(
            &mut remote,
            LyricHint::RemoteOnly("https://music.example/a.lrc".into()),
            true,
        );
        assert!(!remote.lyrics_pending, "只有网址不读、也不写读取中");
        let mut skipped = SourcePrep::default();
        schedule_lyrics(&mut skipped, LyricHint::Absent, false);
        assert!(!skipped.lyrics_pending, "视频和解不开的不读歌词");
        let mut sibling = SourcePrep::default();
        schedule_lyrics(&mut sibling, LyricHint::Absent, true);
        assert!(
            sibling.lyrics_pending && sibling.lyric_path.is_none(),
            "没有路径时读同级"
        );
        let mut local = SourcePrep::default();
        schedule_lyrics(&mut local, LyricHint::Local("C:/cache/a.lrc".into()), true);
        assert_eq!(local.lyric_path.as_deref(), Some("C:/cache/a.lrc"));

        let mut model = scene("preview-audio");
        let path = model.inspect.target_path.clone().expect("音频预览路径");
        let generation = model.inspect.generation;
        let session = model.inspect.media_session().expect("音频会话").clone();
        let mut prep = SourcePrep::default();
        prep.lyrics_pending = true;
        prep.lyric_path = Some("C:/cache/a.lrc".into());
        model.reduce(loaded(path.clone(), generation, Ok(session.clone()), prep));
        let reading = show(&model);
        assert!(reading.find("读取歌词...").is_some(), "歌词还在读");
        assert!(reading.find("暂无歌词").is_none(), "还没读完不写成没有歌词");
        assert!(
            model.inspect.take_effects().iter().any(|effect| matches!(
                effect,
                InspectEffect::LoadLyrics { audio_path, lyric_path, generation: got, .. }
                    if audio_path == &path && lyric_path.as_deref() == Some("C:/cache/a.lrc") && *got == generation
            )),
            "排了这次歌词读取"
        );

        model.reduce(ShellMessage::Inspect(InspectMessage::LyricsLoaded {
            path: path.clone(),
            generation,
            text: "[00:00.00]读到了".into(),
        }));
        let arrived = show(&model);
        assert!(arrived.find("读到了").is_some());
        assert!(arrived.find("读取歌词...").is_none());

        model.reduce(ShellMessage::Inspect(InspectMessage::LyricsLoaded {
            path: path.clone(),
            generation: generation + 3,
            text: "[00:00.00]过期".into(),
        }));
        assert!(show(&model).find("过期").is_none(), "过期的歌词丢掉");
        assert!(show(&model).find("读到了").is_some());

        let mut again = SourcePrep::default();
        again.lyrics_pending = true;
        model.reduce(loaded(path.clone(), generation, Ok(session), again));
        assert!(
            show(&model).find("读取歌词...").is_some(),
            "重新读时再写读取中"
        );
        model.reduce(ShellMessage::Inspect(InspectMessage::LyricsLoaded {
            path: path.clone(),
            generation,
            text: String::new(),
        }));
        let empty = show(&model);
        assert!(empty.find("暂无歌词").is_some(), "读完没有正文");
        assert!(empty.find("读取歌词...").is_none());

        model.reduce(loaded(
            path,
            generation,
            Err("解码失败".into()),
            SourcePrep::default(),
        ));
        let failed = show(&model);
        assert!(failed.find("读取歌词...").is_none(), "预览失败不再写读取中");
        assert!(failed.find("无法预览该音频").is_some());
    }

    fn scene(name: &str) -> ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(scene, _)| *scene == name)
            .unwrap_or_else(|| panic!("没有场景 {name}"))
            .1
    }

    fn loaded(
        path: String,
        generation: u64,
        result: Result<PlaybackSessionState, String>,
        prep: SourcePrep,
    ) -> ShellMessage {
        ShellMessage::Inspect(InspectMessage::MediaLoaded {
            path,
            generation,
            result,
            pcm: None,
            frames: None,
            prep,
        })
    }

    fn show(model: &ShellViewModel) -> ShellHarness {
        ShellHarness::mount(model.clone())
    }

    fn node_text(harness: &ShellHarness, key: &str) -> String {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .text(id)
            .unwrap_or_default()
            .to_string()
    }
}

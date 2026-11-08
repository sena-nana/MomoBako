//! ASMR 音频的收听进度。对齐 `library-asmr` 对 `media.playback` 的处理
//! （`asmrLibrary.js` 的 `shouldPersistListeningProgress` / `buildListeningProgressMetadata`）。
//!
//! Vue 的音频预览组件在时长就绪、播放推进、暂停和结束时发出这条事件，插件再写元数据。
//! 文件预览对音频走播放器挂载，不挂那个组件，所以真窗口里播放条自己的运行时并不发事件。
//! Nana 没有插件前端：预览页正在放这条 ASMR 音频时，由宿主按同一套规则写入。离开预览页就停。
//! 验收场景是停住的画面，不在这里写，免得截图停在「保存中」。

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::ShellViewModel;
use super::player::ClockStep;
use super::player::PlayerState;

/// 上一笔非「时长就绪」写入的节流记号。键是「仓库 + 路径」。
#[derive(Clone, Debug, Default)]
pub(crate) struct Gate {
    marks: BTreeMap<String, Mark>,
}

#[derive(Clone, Copy, Debug)]
struct Mark {
    saved_at_ms: u64,
    saved_second: i64,
}

/// 播放器这一刻的条目。时钟往前走以后当前项可能已经换成下一首，结束事件仍用这一份。
#[derive(Clone, Debug)]
pub(crate) struct PlayingClip {
    pub playing: bool,
    pub path: String,
    pub asset_id: String,
    pub current_ms: u64,
    pub duration_ms: u64,
}

/// 抄下当前项。没有当前项时路径是空的，调用方不会把它当成一条音频。
pub(crate) fn playing_clip(player: &PlayerState) -> PlayingClip {
    let item = player.current_item();
    PlayingClip {
        playing: player.clock_running(),
        path: item.map(|item| item.path.clone()).unwrap_or_default(),
        asset_id: item.map(|item| item.asset_id.clone()).unwrap_or_default(),
        current_ms: player.session.current_time_ms,
        duration_ms: player.session.duration_ms.unwrap_or(0),
    }
}

/// 预览把音频装进播放条、时长已经知道。对应 `<audio>` 的 `loadedmetadata`。
pub(crate) fn on_duration(model: &mut ShellViewModel) {
    let clip = playing_clip(&model.player);
    note(
        model,
        Moment::Duration,
        &clip,
        clip.current_ms,
        clip.duration_ms,
    );
}

/// 用户从播放中按了暂停。自然结束走 [`on_clock`]，不在这里再记一次。
pub(crate) fn on_pause(model: &mut ShellViewModel, clip: PlayingClip) {
    note(
        model,
        Moment::Pause,
        &clip,
        clip.current_ms,
        clip.duration_ms,
    );
}

/// 播放时钟拨完以后。往前走是 `timeupdate`；这一首到头是 `ended`，用拨动前的那一条。
pub(crate) fn on_clock(model: &mut ShellViewModel, before: &PlayingClip, step: ClockStep) {
    match step {
        ClockStep::Progressed => note(
            model,
            Moment::TimeUpdate,
            before,
            model.player.session.current_time_ms,
            before.duration_ms,
        ),
        ClockStep::Changed => note(
            model,
            Moment::Ended,
            before,
            before.duration_ms,
            before.duration_ms,
        ),
        ClockStep::Idle => {}
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Moment {
    Duration,
    TimeUpdate,
    Pause,
    Ended,
}

fn note(
    model: &mut ShellViewModel,
    moment: Moment,
    clip: &PlayingClip,
    current_ms: u64,
    duration_ms: u64,
) {
    let now_ms = unix_now_ms();
    if !write_progress(model, moment, clip, current_ms, duration_ms, now_ms) {
        return;
    }
    model.mark_surface_dirty();
}

/// 按 Vue 的规则决定要不要写，并排一次元数据保存。写了返回真。
///
/// 只在预览页正在放这条、而且 ASMR 资源库插件开着时写。验收场景跳过。
/// 时长就绪只写 `trackDurationMs`；其余状态写收听进度、状态、时间和位置。
fn write_progress(
    model: &mut ShellViewModel,
    moment: Moment,
    clip: &PlayingClip,
    current_ms: u64,
    duration_ms: u64,
    now_ms: u64,
) -> bool {
    if model.acceptance_scene || clip.path.is_empty() {
        return false;
    }
    if !super::files_view::previewing(model)
        || model.inspect.open_path() != Some(clip.path.as_str())
    {
        return false;
    }
    if !super::inspect_library::asmr_enabled(&model.admin.plugins) {
        return false;
    }
    if model
        .files
        .rows
        .iter()
        .any(|row| row.path == clip.path && row.kind != "file")
    {
        eprintln!("Nana 收听进度不写目录：{}", clip.path);
        return false;
    }
    let asmr_audio = is_asmr_audio(model.inspect.draft_custom());
    let existing_duration = model.inspect.draft_custom().get("trackDurationMs").cloned();
    if !asmr_audio {
        return false;
    }
    let previous = model
        .listening
        .marks
        .get(&gate_key(model, &clip.path))
        .copied();
    if !should_persist(
        moment,
        current_ms,
        duration_ms,
        existing_duration.as_ref(),
        previous,
        now_ms,
    ) {
        return false;
    }
    let patch = match moment {
        Moment::Duration => BTreeMap::from([("trackDurationMs".to_string(), json!(duration_ms))]),
        Moment::TimeUpdate | Moment::Pause | Moment::Ended => {
            progress_patch(moment, current_ms, duration_ms, now_ms)
        }
    };
    let Some(asset_id) = model.inspect.current_asset_id().map(str::to_string) else {
        eprintln!("Nana 收听进度没有素材：{}", clip.path);
        return false;
    };
    if asset_id != clip.asset_id && !clip.asset_id.is_empty() {
        eprintln!(
            "Nana 收听进度的素材和正在看的不是同一条：{asset_id} / {}",
            clip.asset_id
        );
        return false;
    }
    if let Err(error) = model.inspect.apply_metadata_patch(&asset_id, patch) {
        eprintln!("Nana 收听进度没有写入：{error}");
        return false;
    }
    if moment != Moment::Duration {
        let key = gate_key(model, &clip.path);
        model.listening.marks.insert(
            key,
            Mark {
                saved_at_ms: now_ms,
                saved_second: (current_ms / 1000) as i64,
            },
        );
    }
    true
}

fn gate_key(model: &ShellViewModel, path: &str) -> String {
    let repo = model.inspect.current_repo_id().unwrap_or("");
    format!("{repo}\u{0}{path}")
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// 是不是 ASMR 音频（Vue `isAsmrAudioEntry` 的元数据部分）。种类由调用方确认是文件。
fn is_asmr_audio(custom: &BTreeMap<String, Value>) -> bool {
    text(custom, "asmrEntryKind") == "audio"
        && (text(custom, "libraryKind") == "asmr"
            || !text(custom, "workId").is_empty()
            || !text(custom, "rjCode").is_empty())
}

fn text(custom: &BTreeMap<String, Value>, key: &str) -> String {
    match custom.get(key) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

/// 要不要把这一拍写进元数据。`previous` 是上一笔播放推进或暂停的记号，时长就绪不看它。
fn should_persist(
    moment: Moment,
    current_ms: u64,
    duration_ms: u64,
    existing_duration: Option<&Value>,
    previous: Option<Mark>,
    now_ms: u64,
) -> bool {
    if moment == Moment::Duration {
        return duration_ms > 0
            && !existing_duration.is_some_and(|value| same_number(value, duration_ms));
    }
    if moment == Moment::Ended {
        return true;
    }
    if moment == Moment::Pause {
        return current_ms > 0;
    }
    if duration_ms == 0 || current_ms == 0 {
        return false;
    }
    let current_second = (current_ms / 1000) as i64;
    match previous {
        None => true,
        Some(mark) => {
            now_ms.saturating_sub(mark.saved_at_ms) >= 15_000
                || (current_second - mark.saved_second).abs() >= 5
        }
    }
}

/// JSON 数字和时长是同一个数。字符串不算（Vue 用 `!==`，`"120000" !== 120000`）。
fn same_number(value: &Value, duration: u64) -> bool {
    value.as_u64() == Some(duration)
        || value.as_i64() == Some(duration as i64)
        || value.as_f64() == Some(duration as f64)
}

/// 播放推进、暂停或结束要写的五个字段。进度到 95% 或状态是结束，算听完。
fn progress_patch(
    moment: Moment,
    current_ms: u64,
    duration_ms: u64,
    now_ms: u64,
) -> BTreeMap<String, Value> {
    let progress = if duration_ms > 0 {
        ((current_ms as f64 / duration_ms as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as i64
    } else {
        0
    };
    let finished = moment == Moment::Ended || (duration_ms > 0 && progress >= 95);
    BTreeMap::from([
        (
            "listeningProgress".into(),
            json!(if finished { 100 } else { progress }),
        ),
        (
            "listeningStatus".into(),
            json!(if finished { "listened" } else { "listening" }),
        ),
        (
            "lastListenedAt".into(),
            json!(super::admin::iso8601_from_unix_ms(now_ms)),
        ),
        ("trackDurationMs".into(), json!(duration_ms)),
        (
            "trackPositionMs".into(),
            json!(if finished { duration_ms } else { current_ms }),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use crate::backend::services::repository::{AssetDetail, AssetSummary, MetadataEntry};
    use crate::shell::{InspectEffect, InspectMessage, ShellMessage, ShellPage, ShellViewModel};

    use super::{Moment, progress_patch, should_persist};

    const PATH: &str = "voice/track.wav";

    /// 预览页放 ASMR 音频时，按 Vue 的节流把收听进度写进元数据。
    /// 插件停用、离开预览、间隔不够都不写。听完写成已听完。
    #[test]
    fn asmr_preview_playback_writes_listening_progress() {
        assert_eq!(
            crate::shell::admin::iso8601_from_unix_ms(0),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(
            crate::shell::admin::iso8601_from_unix_ms(1_500),
            "1970-01-01T00:00:01.500Z"
        );
        let ended = progress_patch(Moment::Ended, 118_000, 120_000, 0);
        assert_eq!(ended.get("listeningProgress"), Some(&json!(100)));
        assert_eq!(ended.get("listeningStatus"), Some(&json!("listened")));
        assert_eq!(ended.get("trackPositionMs"), Some(&json!(120_000)));
        assert_eq!(
            ended.get("lastListenedAt").and_then(Value::as_str),
            Some("1970-01-01T00:00:00.000Z")
        );
        let almost = progress_patch(Moment::TimeUpdate, 114_000, 120_000, 1_500);
        assert_eq!(
            almost.get("listeningStatus"),
            Some(&json!("listened")),
            "95% 算听完"
        );
        assert_eq!(almost.get("trackPositionMs"), Some(&json!(120_000)));
        assert!(!should_persist(
            Moment::Duration,
            0,
            120_000,
            Some(&json!(120_000)),
            None,
            0
        ));
        assert!(
            should_persist(
                Moment::Duration,
                0,
                120_000,
                Some(&json!("120000")),
                None,
                0
            ),
            "字符串时长和数字不相等"
        );
        assert!(!should_persist(
            Moment::TimeUpdate,
            0,
            120_000,
            None,
            None,
            0
        ));
        assert!(!should_persist(Moment::Pause, 0, 120_000, None, None, 0));
        assert!(should_persist(Moment::Ended, 0, 0, None, None, 0));
        let mark = super::Mark {
            saved_at_ms: 1_000,
            saved_second: 3,
        };
        assert!(
            !should_persist(Moment::TimeUpdate, 4_000, 120_000, None, Some(mark), 2_000),
            "不到 5 秒也不到 15 秒"
        );
        assert!(
            should_persist(Moment::TimeUpdate, 4_000, 120_000, None, Some(mark), 16_000),
            "过了 15 秒要写"
        );
        assert!(
            should_persist(Moment::TimeUpdate, 8_000, 120_000, None, Some(mark), 2_000),
            "进度过了 5 秒要写"
        );

        let mut model = playing_preview();
        let duration = model.player.session.duration_ms.expect("测试音频有时长");
        assert!(duration >= 8_000, "节流要能走出 5 秒，实际 {duration}");
        let saved = take_save(&mut model).expect("时长就绪要写 trackDurationMs");
        assert_eq!(
            saved.get("trackDurationMs").and_then(Value::as_u64),
            Some(duration)
        );
        assert_eq!(saved.len(), 1, "时长就绪不写收听进度：{saved:?}");
        ack(&mut model, &saved);

        disable_asmr(&mut model);
        model.reduce(ShellMessage::Player(
            crate::shell::player::PlayerMessage::SetPlaying(true),
        ));
        assert_eq!(model.player.session.status, "playing");
        crate::shell::poll_timers(&mut model, crate::frame_clock::FrameStep::millis(2_000));
        assert!(
            take_save(&mut model).is_none(),
            "插件停用时播放不写收听进度"
        );
        enable_asmr(&mut model);

        crate::shell::poll_timers(&mut model, crate::frame_clock::FrameStep::millis(1_000));
        let progress = take_save(&mut model).expect("重新启用后第一拍要写");
        assert_eq!(
            progress.get("listeningStatus").and_then(Value::as_str),
            Some("listening")
        );
        assert!(
            progress
                .get("listeningProgress")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                > 0
        );
        assert!(
            progress
                .get("lastListenedAt")
                .and_then(Value::as_str)
                .is_some_and(|text| text.ends_with('Z'))
        );
        ack(&mut model, &progress);

        crate::shell::poll_timers(&mut model, crate::frame_clock::FrameStep::millis(1_000));
        assert!(
            take_save(&mut model).is_none(),
            "不到 5 秒、也不到 15 秒，不重复写"
        );

        crate::shell::poll_timers(&mut model, crate::frame_clock::FrameStep::millis(4_000));
        let later = take_save(&mut model).expect("进度过了 5 秒要再写");
        assert!(
            later
                .get("trackPositionMs")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                >= 5_000
        );
        ack(&mut model, &later);

        // 再走 1 秒：还没到 5 秒节流，播放推进不写；暂停不看节流，位置变了就要写。
        crate::shell::poll_timers(&mut model, crate::frame_clock::FrameStep::millis(1_000));
        assert!(
            take_save(&mut model).is_none(),
            "刚写过不到 5 秒，播放推进不写"
        );
        model.reduce(ShellMessage::Player(
            crate::shell::player::PlayerMessage::SetPlaying(false),
        ));
        let paused = take_save(&mut model).expect("暂停要写");
        assert!(
            paused
                .get("trackPositionMs")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 8_000,
            "暂停写下新的位置：{paused:?}"
        );
        ack(&mut model, &paused);

        model.reduce(ShellMessage::OpenDirectory(String::new()));
        model.reduce(ShellMessage::Player(
            crate::shell::player::PlayerMessage::SetPlaying(true),
        ));
        crate::shell::poll_timers(&mut model, crate::frame_clock::FrameStep::millis(6_000));
        assert!(take_save(&mut model).is_none(), "离开预览页不再写");
    }

    fn playing_preview() -> ShellViewModel {
        let metadata = BTreeMap::from([
            ("libraryKind".to_string(), json!("asmr")),
            ("asmrEntryKind".to_string(), json!("audio")),
        ]);
        let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
        model.files.rows = vec![file_row(&metadata)];
        model.reduce(ShellMessage::SelectFile {
            path: PATH.into(),
            asset_id: Some(asset_id()),
        });
        model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(&metadata, 1))));
        let (path, generation) = model
            .inspect
            .take_effects()
            .into_iter()
            .find_map(|effect| match effect {
                InspectEffect::LoadMedia {
                    path, generation, ..
                } => Some((path, generation)),
                _ => None,
            })
            .expect("打开音频预览要去读字节");
        model.acceptance_scene = false;
        let bytes = pcm_wav(8_000, &vec![128u8; 8_000 * 30]);
        let parts = crate::shell::preview_media_parts("acceptance-repo", &bytes).expect("wav");
        model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded {
            path,
            generation,
            result: Ok(parts.session),
            pcm: parts.pcm,
            frames: parts.frames,
            prep: crate::shell::SourcePrep::default(),
        }));
        model
    }

    fn file_row(metadata: &BTreeMap<String, Value>) -> crate::shell::FileRow {
        crate::shell::FileRow {
            path: PATH.into(),
            name: "track.wav".into(),
            kind: "file".into(),
            asset_id: Some(asset_id()),
            is_virtual: false,
            local_absolute_path: None,
            thumbnail_path: None,
            hardlink_state: None,
            extension: Some("wav".into()),
            pixel_width: 0,
            pixel_height: 0,
            texture_ready: false,
            palette: Vec::new(),
            size_label: String::new(),
            modified_at: String::new(),
            tags: Vec::new(),
            alias_paths: Vec::new(),
            thumbnail_custom: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            metadata: metadata.clone(),
        }
    }

    fn detail(metadata: &BTreeMap<String, Value>, version: i64) -> AssetDetail {
        AssetDetail {
            summary: AssetSummary {
                asset_id: asset_id(),
                repo_id: "acceptance-repo".into(),
                path: PATH.into(),
                extension: "wav".into(),
                filename: "track.wav".into(),
                size_bytes: 1,
                size_label: "1 B".into(),
                status: "ready".into(),
                modified_at: String::new(),
                last_accessed_at: None,
                version,
                tags: Vec::new(),
                thumbnail_path: None,
                hardlink_group_id: None,
                hardlink_state: None,
                is_virtual: false,
                provider_id: None,
                provider_item_id: None,
                source_payload: None,
                local_absolute_path: None,
            },
            metadata: metadata
                .iter()
                .map(|(key, value)| MetadataEntry {
                    key: key.clone(),
                    value_type: "json".into(),
                    value: value.clone(),
                    version,
                    updated_at: String::new(),
                })
                .collect(),
            revisions: Vec::new(),
        }
    }

    fn asset_id() -> String {
        PATH.replace('/', "-")
    }

    fn take_save(model: &mut ShellViewModel) -> Option<BTreeMap<String, Value>> {
        model
            .inspect
            .take_effects()
            .into_iter()
            .find_map(|effect| match effect {
                InspectEffect::SaveMetadata { metadata, .. } => Some(metadata),
                _ => None,
            })
    }

    /// 用这次写出的字段交回成功，保存中的标志放下，下一拍才能再写。
    fn ack(model: &mut ShellViewModel, patch: &BTreeMap<String, Value>) {
        let mut metadata = model.inspect.draft_custom().clone();
        metadata.extend(patch.clone());
        let version = 2;
        model.reduce(ShellMessage::Inspect(InspectMessage::MetadataSaved(Ok((
            "success".into(),
            detail(&metadata, version),
        )))));
    }

    fn disable_asmr(model: &mut ShellViewModel) {
        set_asmr(model, false);
    }

    fn enable_asmr(model: &mut ShellViewModel) {
        set_asmr(model, true);
    }

    fn set_asmr(model: &mut ShellViewModel, enabled: bool) {
        let plugin = model.admin.plugins.iter_mut().find(|plugin| {
            plugin
                .contributes
                .get("libraryExtension")
                .and_then(|extension| extension.get("libraryKind"))
                .and_then(Value::as_str)
                == Some("asmr")
        });
        let Some(plugin) = plugin else {
            panic!("验收底子里没有 ASMR 资源库插件");
        };
        plugin.enabled = enabled;
    }

    fn pcm_wav(sample_rate: u32, data: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&sample_rate.to_le_bytes());
        body.extend_from_slice(&sample_rate.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&8u16.to_le_bytes());
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(&body);
        bytes
    }
}

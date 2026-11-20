//! 包内 PDF 画自绘页，包内 zip/7z 再列当前层真实名字。
//! Office 只抽文本，模型用现有软件光栅。能在内存里解开的音频走唱片舞台。
//! 未压缩 AVI 用现有纯 Rust 解码画出真实帧。夹具都在内存里，不打开声卡，也不播放。

use std::time::{Duration, Instant};

use super::super::super::super::inspect_library::{
    archive_directory_activation, directory_double_click,
};
use super::super::super::bridge::ArchiveEntryKind;
use super::super::super::{InspectEffect, InspectMessage};
use super::super::{archive, pdf};
use super::{native_text, opened_archive, sample_zip};

mod documents;
mod media;
mod navigation;

/// sevenz-rust2 写出的未加密 7z：目录 dir，文件 dir/a.txt=hi、b.txt=yo。
const SEVEN_Z: &[u8] = &[
    0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c, 0x00, 0x04, 0x2c, 0x4a, 0xb6, 0xf6, 0x0c, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x99, 0x8e, 0x1f, 0x0e,
    0x01, 0x00, 0x01, 0x68, 0x69, 0x00, 0x01, 0x00, 0x01, 0x79, 0x6f, 0x00, 0x01, 0x04, 0x06, 0x00,
    0x02, 0x09, 0x06, 0x06, 0x0a, 0x01, 0x15, 0x5f, 0xd9, 0x30, 0xd4, 0x31, 0x67, 0x7b, 0x00, 0x07,
    0x0b, 0x02, 0x00, 0x01, 0x21, 0x21, 0x01, 0x16, 0x01, 0x21, 0x21, 0x01, 0x16, 0x0c, 0x02, 0x02,
    0x00, 0x08, 0x0a, 0x01, 0xac, 0x2a, 0x93, 0xd8, 0x89, 0xac, 0x29, 0x62, 0x00, 0x00, 0x05, 0x03,
    0x0e, 0x01, 0x80, 0x11, 0x29, 0x00, 0x64, 0x00, 0x69, 0x00, 0x72, 0x00, 0x00, 0x00, 0x64, 0x00,
    0x69, 0x00, 0x72, 0x00, 0x2f, 0x00, 0x61, 0x00, 0x2e, 0x00, 0x74, 0x00, 0x78, 0x00, 0x74, 0x00,
    0x00, 0x00, 0x62, 0x00, 0x2e, 0x00, 0x74, 0x00, 0x78, 0x00, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00,
];
fn assert_audio(preview: &super::super::super::bridge::ArchiveEntryPreview, bytes: &[u8]) {
    let pcm = crate::shell::player::pcm_from_bytes(bytes).expect("对照解码");
    assert!(
        !pcm.pcm.is_empty() && pcm.frame_count > 0,
        "对照解码没有样本"
    );
    match &preview.kind {
        ArchiveEntryKind::Audio {
            duration_ms,
            sample_rate,
            channels,
            error: None,
        } => {
            assert_eq!(
                (*sample_rate, *channels, *duration_ms),
                (pcm.sample_rate, pcm.channels, pcm.duration_ms)
            );
        }
        other => panic!("不是解开的音频：{other:?}"),
    }
}

/// 8 kHz 单声道 16 位，8 个样本，时长 1 毫秒。测试用，不播放。
fn voice_wav() -> Vec<u8> {
    use crate::shell::audio_decode::pcm::{fmt_chunk, wav_file};
    let samples = [0i16, 1000, -1000, 500, 0, 1000, -1000, 500];
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    wav_file(&fmt_chunk(1, 1, 8_000, 16, None), &data)
}

/// 这一帧编成的内容图。和预览框里 `content_image` 用的是同一个编码器。
fn frame_png(frame: &crate::shell::inspect::support::VideoFrame) -> String {
    super::super::super::rgba_png_data_url(frame.width, frame.height, &frame.rgba)
        .expect("帧能编成 PNG")
}

/// 读出预览框里已经画上的内容图。对不上解码帧就说明没有按像素画。
fn painted(harness: &crate::shell::view_harness::ShellHarness, key: &str) -> String {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    let context = harness.document().context();
    let style = context
        .world()
        .node_style(id)
        .unwrap_or_else(|| panic!("{key} 没有样式"));
    match &style.layout.paint.content_image {
        Some(nana_ui_core::BackgroundImage::Url { url, .. }) => url.clone(),
        other => panic!("{key} 不是解码出的画面：{other:?}"),
    }
}

fn shows(harness: &crate::shell::view_harness::ShellHarness, needle: &str) -> bool {
    harness.nodes().into_iter().any(|node| {
        node.label
            .as_deref()
            .is_some_and(|label| label.contains(needle))
    })
}

fn mount(state: super::super::super::InspectState) -> crate::shell::view_harness::ShellHarness {
    let mut model = crate::shell::ShellViewModel::for_page(crate::shell::ShellPage::SelectedFile);
    model.inspect = state;
    crate::shell::view_harness::ShellHarness::mount(model)
}

fn unmount(
    mut harness: crate::shell::view_harness::ShellHarness,
) -> super::super::super::InspectState {
    let state = std::mem::replace(&mut harness.model.inspect, Default::default());
    drop(harness);
    state
}

fn model_rgba(preview: &super::super::super::bridge::ArchiveEntryPreview) -> &[u8] {
    match &preview.kind {
        ArchiveEntryKind::Model {
            frame: Some(frame),
            error: None,
            ..
        } => &frame.rgba,
        other => panic!("不是光栅：{other:?}"),
    }
}

fn model_has_mesh(preview: &super::super::super::bridge::ArchiveEntryPreview) -> bool {
    matches!(&preview.kind, ArchiveEntryKind::Model { mesh: Some(_), .. })
}

/// 一个三角形的 glb。VRM 就是这种文件头，走现有 glTF 光栅。
fn glb_triangle() -> Vec<u8> {
    let mut bin = Vec::new();
    for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let mut json = br#"{"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}],"bufferViews":[{"byteLength":36}],"buffers":[{"byteLength":36}]}"#.to_vec();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    bytes.extend(json);
    bytes.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0x004E_4942u32.to_le_bytes());
    bytes.extend(bin);
    bytes
}

/// 已经读出的条目名。选中和进入都不该改这份名单。
fn catalog_names(state: &super::super::super::InspectState) -> Vec<String> {
    state
        .archive
        .as_ref()
        .expect("catalog")
        .names
        .iter()
        .map(|name| format!("{}:{}", name.path, name.directory))
        .collect()
}

fn take(
    state: &mut super::super::super::InspectState,
    bytes: &[u8],
) -> super::super::super::bridge::ArchiveEntryPreview {
    let effect = state.take_effects().pop().expect("要读这一条");
    let InspectEffect::LoadArchiveEntry { entry, through, .. } = effect else {
        panic!("不是包内文件：{effect:?}")
    };
    archive::preview_chain(bytes, &through, &entry)
        .unwrap_or_else(|error| panic!("{entry}：{error}"))
}

/// 左边一块和右边一块的布局盒要左右分开，并且在竖直方向上重叠。
fn assert_beside(
    harness: &crate::shell::view_harness::ShellHarness,
    left_key: &str,
    right_key: &str,
) {
    let left = bounds(harness, left_key);
    let right = bounds(harness, right_key);
    assert!(left.0 < right.0, "名单应在舞台左边：{left:?} {right:?}");
    let overlap = (left.1 + left.3).min(right.1 + right.3) - left.1.max(right.1);
    assert!(overlap > 1.0, "名单和舞台应并排：{left:?} {right:?}");
}

fn bounds(harness: &crate::shell::view_harness::ShellHarness, key: &str) -> (f32, f32, f32, f32) {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    let placed = harness
        .document()
        .context()
        .world()
        .layout_box(id)
        .unwrap_or_else(|| panic!("{key} 没有布局"));
    (placed.x, placed.y, placed.width, placed.height)
}

/// 名单上能读到的无障碍名和值。
fn node_texts(harness: &crate::shell::view_harness::ShellHarness) -> Vec<String> {
    harness
        .nodes()
        .into_iter()
        .flat_map(|node| {
            [
                node.label.map(|label| label.to_string()),
                node.value.as_ref().map(|value| value.to_string()),
            ]
            .into_iter()
            .flatten()
        })
        .collect()
}

/// 名单行的无障碍名：文件名，后面是目录说明或大小。
fn row_text(harness: &crate::shell::view_harness::ShellHarness, index: usize) -> String {
    let key = format!("inspect-archive-row-{index}");
    let id = harness
        .keyed(&key)
        .unwrap_or_else(|| panic!("没有名单行 {key}"));
    harness
        .nodes()
        .into_iter()
        .find(|node| node.id == id)
        .and_then(|node| node.label.map(|label| label.to_string()))
        .unwrap_or_default()
}

/// 行首种类上的字。
fn kind_text(harness: &crate::shell::view_harness::ShellHarness, index: usize) -> String {
    let key = format!("inspect-archive-kind-{index}");
    let id = harness
        .keyed(&key)
        .unwrap_or_else(|| panic!("没有行首种类 {key}"));
    harness
        .nodes()
        .into_iter()
        .find(|node| node.id == id)
        .and_then(|node| node.label.map(|label| label.to_string()))
        .unwrap_or_default()
}

/// 面包屑按钮上的字。键落在按钮上，无障碍名就是这一段。
fn crumb_text(harness: &crate::shell::view_harness::ShellHarness, key: &str) -> String {
    let id = harness
        .keyed(key)
        .unwrap_or_else(|| panic!("没有面包屑 {key}"));
    harness
        .nodes()
        .into_iter()
        .find(|node| node.id == id)
        .and_then(|node| node.label.map(|label| label.to_string()))
        .unwrap_or_default()
}

fn stage_of(state: &super::super::super::InspectState) -> (&str, &str, bool) {
    crate::shell::inspect_library::archive_stage(state)
}

fn mount_archive(
    state: &super::super::super::InspectState,
) -> crate::shell::view_harness::ShellHarness {
    let mut model = crate::shell::ShellViewModel::for_page(crate::shell::ShellPage::SelectedFile);
    model.inspect = state.clone();
    crate::shell::view_harness::ShellHarness::mount(model)
}

fn finish(
    state: &mut super::super::super::InspectState,
    path: &str,
    result: Result<super::super::super::bridge::ArchiveEntryPreview, String>,
) {
    let (entry, through) = match state.archive_request.clone() {
        Some(key) => request_parts(&key),
        None => panic!("没有待回来的包内文件"),
    };
    let generation = state.generation;
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::ArchiveEntryLoaded {
            path: path.into(),
            entry,
            through,
            generation,
            result,
        },
    );
}

/// 和 `request_key` 同一套分隔：嵌套路径在前，条目名在最后一个分隔之后。
fn request_parts(key: &str) -> (String, Vec<String>) {
    let mut parts = key.split('\u{1}').map(str::to_string).collect::<Vec<_>>();
    let entry = parts.pop().unwrap_or_default();
    if parts.first().is_some_and(|part| part.is_empty()) {
        parts.remove(0);
    }
    (entry, parts)
}

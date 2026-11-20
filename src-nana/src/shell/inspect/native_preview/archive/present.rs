//! 压缩包内条目的呈现：图片、文本、Office、模型、音频、AVI、PDF 与嵌套压缩包，以及翻页与模型旋转。

use super::*;

pub(super) fn present(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    let extension = extension_of(path);
    match super::super::super::support::classify(&extension) {
        PreviewKind::Image => present_image(raw, path),
        PreviewKind::Markdown => present_text(raw, path, &extension, true),
        PreviewKind::Text => present_text(raw, path, &extension, false),
        PreviewKind::Native { view_id, .. } if view_id == super::super::PDF_VIEW => {
            present_pdf(raw, path)
        }
        PreviewKind::Native { view_id, .. } if view_id == super::super::OFFICE_VIEW => {
            present_office(raw, path)
        }
        PreviewKind::Native { view_id, .. } if view_id == super::super::MODEL_VIEW => {
            present_model(raw, path)
        }
        PreviewKind::Native { view_id, .. }
            if view_id == super::super::ARCHIVE_VIEW && nestable(&extension) =>
        {
            present_nest(raw, path)
        }
        PreviewKind::Media if audible(&extension) => present_audio(raw, path),
        PreviewKind::Media if avi_name(&extension) => present_avi(raw, path),
        _ => {
            eprintln!("Nana 压缩包里的这个类型不能预览：{path}");
            Err(format!("不能预览 {path}"))
        }
    }
}

pub(super) fn present_image(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    let pixels = crate::decode_preview_pixels(raw).map_err(|error| {
        eprintln!("Nana 包内图片解码失败：{path}：{error}");
        error
    })?;
    Ok(ArchiveEntryPreview {
        path: path.to_string(),
        kind: ArchiveEntryKind::Image {
            width: pixels.width,
            height: pixels.height,
            rgba: pixels.rgba,
        },
    })
}

pub(super) fn present_text(
    raw: &[u8],
    path: &str,
    extension: &str,
    markdown: bool,
) -> Result<ArchiveEntryPreview, String> {
    let read = super::super::super::prepare_text(raw);
    Ok(ArchiveEntryPreview {
        path: path.to_string(),
        kind: ArchiveEntryKind::Text {
            markdown,
            extension: extension.to_string(),
            text: read.text,
            truncated_at: read.truncated_at,
        },
    })
}

/// 用现有 Open XML / OLE 抽取。失败时留下原因，预览框写「无法预览该文档」，不还原版式。
pub(super) fn present_office(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    match super::super::read_office(raw) {
        Ok(text) => Ok(ArchiveEntryPreview {
            path: path.to_string(),
            kind: ArchiveEntryKind::Office { text, error: None },
        }),
        Err(error) => {
            eprintln!("Nana 包内文档抽取失败：{path}：{error}");
            Ok(ArchiveEntryPreview {
                path: path.to_string(),
                kind: ArchiveEntryKind::Office {
                    text: String::new(),
                    error: Some(error),
                },
            })
        }
    }
}

/// 用现有软件光栅。FBX 和 BLEND 没有网格，只留下文件头摘要。解析失败用现有模型失败状态。
pub(super) fn present_model(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    match super::super::mesh::load(raw) {
        Ok(loaded) => {
            let frame = loaded
                .frames
                .into_iter()
                .next()
                .filter(|frame| frame.error.is_none() && !frame.rgba.is_empty());
            let extent = loaded
                .mesh
                .as_ref()
                .map(super::super::mesh::extent_label)
                .unwrap_or_default();
            Ok(ArchiveEntryPreview {
                path: path.to_string(),
                kind: ArchiveEntryKind::Model {
                    text: loaded.text,
                    frame,
                    mesh: loaded.mesh,
                    yaw: 0.0,
                    zoom: 1.0,
                    error: None,
                    extent,
                },
            })
        }
        Err(error) => {
            eprintln!("Nana 包内模型解析失败：{path}：{error}");
            Ok(ArchiveEntryPreview {
                path: path.to_string(),
                kind: ArchiveEntryKind::Model {
                    text: String::new(),
                    frame: None,
                    mesh: None,
                    yaw: 0.0,
                    zoom: 1.0,
                    error: Some(error),
                    extent: String::new(),
                },
            })
        }
    }
}

/// 用现有音频解码在内存里解这一条。WAV 走自有解析，mp3、flac、ogg 走 symphonia，
/// m4a、aac、opus 在前面失败后走现有媒体基础拉样本。解出的 PCM 只用来确认有样本，随即丢掉，
/// 不装进播放条，也不调用播放。附带的画面不在这里画。解不开时留下现有失败原因。
pub(super) fn present_audio(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    let extension = extension_of(path);
    match crate::shell::player::decode_media("archive", &extension, raw) {
        Ok(parts) => match audio_facts(&parts) {
            Some((duration_ms, sample_rate, channels)) => Ok(ArchiveEntryPreview {
                path: path.to_string(),
                kind: ArchiveEntryKind::Audio {
                    duration_ms,
                    sample_rate,
                    channels,
                    error: None,
                },
            }),
            None => {
                eprintln!("Nana 包内音频没有解出样本：{path}");
                Ok(failed_audio(path, "音频无法播放"))
            }
        },
        Err(error) => {
            eprintln!("Nana 包内音频解码失败：{path}：{error}");
            Ok(failed_audio(path, &error))
        }
    }
}

/// 有非空 PCM、采样率和声道，才算无声解开。封面之类的画面不参与判断。
pub(super) fn audio_facts(parts: &crate::shell::MediaParts) -> Option<(u64, u32, u16)> {
    let pcm = parts.pcm.as_ref()?;
    if pcm.pcm.is_empty() || pcm.sample_rate == 0 || pcm.channels == 0 {
        return None;
    }
    Some((pcm.duration_ms, pcm.sample_rate, pcm.channels))
}

/// 用现有纯 Rust 路径解未压缩 AVI。解出画面就留下，播放头从片头开始；PCM 由解码函数丢掉，这里不装进播放条。
/// 不是这一类、或解不开，返回错误，调用方停在名单上。不调用系统媒体基础。
pub(super) fn present_avi(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    match super::super::super::support::uncompressed_avi_frames(raw) {
        Ok(Some(frames)) => Ok(ArchiveEntryPreview {
            path: path.to_string(),
            kind: ArchiveEntryKind::Video {
                frames,
                position_ms: 0,
            },
        }),
        Ok(None) => {
            eprintln!("Nana 包内视频不是能解开的未压缩 AVI，停在名单上：{path}");
            Err(format!("不能预览 {path}"))
        }
        Err(error) => {
            eprintln!("Nana 包内 AVI 解码失败，停在名单上：{path}：{error}");
            Err(error)
        }
    }
}

/// 把包内 AVI 这条预览自己的播放头拨到 `time_ms`。播放条保持原样，不改成播放会话。
///
/// 已经挂着宿主纹理记号（`video:`）时，记号和像素跟着换到不晚于这个时刻的已解帧，真窗口重新上传。
/// 没有记号时只改播放头：不把未注册的记号挂回去，离屏继续画解码像素。没有这条视频时只记日志。
pub(in super::super) fn place_video(
    model: &mut super::super::super::super::ShellViewModel,
    time_ms: u64,
) {
    let host = model
        .preview_token
        .as_deref()
        .is_some_and(|token| token.starts_with("video:"));
    let uploaded = {
        let Some(entry) = model.inspect.archive_entry.as_mut() else {
            eprintln!("Nana 包内没有正在看的文件，播放头留在原地");
            return;
        };
        let path = entry.path.clone();
        let ArchiveEntryKind::Video {
            frames,
            position_ms,
        } = &mut entry.kind
        else {
            eprintln!("Nana 包内这一条不是视频，播放头留在原地：{path}");
            return;
        };
        *position_ms = time_ms;
        if !host {
            return;
        }
        let Some(frame) = super::super::super::bridge::frame_at(frames, time_ms)
            .filter(|frame| frame.width > 0 && frame.height > 0 && !frame.rgba.is_empty())
        else {
            eprintln!("Nana 包内 AVI 在 {time_ms} 毫秒没有可画的画面：{path}");
            return;
        };
        Some((
            path,
            frame.time_ms,
            frame.width,
            frame.height,
            frame.rgba.clone(),
        ))
    };
    let Some((path, frame_time, width, height, rgba)) = uploaded else {
        return;
    };
    model.preview_pixels = Some(super::super::super::super::PreviewPixels {
        width,
        height,
        rgba,
    });
    model.preview_token = Some(format!("video:{path}:{frame_time}:{width}"));
}

pub(super) fn failed_audio(path: &str, error: &str) -> ArchiveEntryPreview {
    ArchiveEntryPreview {
        path: path.to_string(),
        kind: ArchiveEntryKind::Audio {
            duration_ms: 0,
            sample_rate: 0,
            channels: 0,
            error: Some(error.to_string()),
        },
    }
}

/// 用现有自绘 PDF 画这一条的页。解析失败留下失败原因，不另找一份 PDF。
pub(super) fn present_pdf(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    match super::super::pdf::load(raw) {
        Ok(loaded) => Ok(ArchiveEntryPreview {
            path: path.to_string(),
            kind: ArchiveEntryKind::Pdf {
                frames: loaded.frames,
                index: 0,
                error: None,
            },
        }),
        Err(error) => {
            eprintln!("Nana 包内 PDF 解析失败：{path}：{error}");
            Ok(ArchiveEntryPreview {
                path: path.to_string(),
                kind: ArchiveEntryKind::Pdf {
                    frames: Vec::new(),
                    index: 0,
                    error: Some(error),
                },
            })
        }
    }
}

/// 用现有名单列出这一层的真实名字。列完就丢掉容器字节。打不开或不能按层进入时停在上一层。
pub(super) fn present_nest(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    let loaded = match list(raw) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("Nana 嵌套压缩包列不出来：{path}：{error}");
            return Err(error);
        }
    };
    let Some(catalog) = loaded.archive else {
        eprintln!("Nana 嵌套压缩包没有名单：{path}");
        return Err(format!("{path} 没有名单"));
    };
    if !catalog.layered {
        eprintln!("Nana 嵌套压缩包不能按层打开：{path}");
        return Err(format!("{path} 不能按层打开"));
    }
    Ok(ArchiveEntryPreview {
        path: path.to_string(),
        kind: ArchiveEntryKind::Nested { catalog },
    })
}

/// 包内 PDF 的翻页。不是这种预览时返回假，让原来的页甲板处理。
pub(in super::super) fn turn_pdf(state: &mut InspectState, delta: i32) -> bool {
    let Some(entry) = state.archive_entry.as_mut() else {
        return false;
    };
    let ArchiveEntryKind::Pdf {
        frames,
        index,
        error,
    } = &mut entry.kind
    else {
        return false;
    };
    if error.is_some() || frames.is_empty() {
        eprintln!("Nana 包内 PDF 没有可翻的页");
        return true;
    }
    let len = frames.len() as i32;
    let next = *index as i32 + delta;
    if next < 0 || next >= len {
        eprintln!("Nana PDF 页码越界：{next}");
        return true;
    }
    *index = next as usize;
    true
}

/// 包内模型绕竖直轴旋转，或按比例缩放。没有网格时不假装已经转过。不是模型时返回假。
pub(in super::super) fn orbit_model(state: &mut InspectState, yaw: f32, zoom: f32) -> bool {
    let Some(entry) = state.archive_entry.as_mut() else {
        return false;
    };
    let data = {
        let ArchiveEntryKind::Model { mesh, error, .. } = &entry.kind else {
            return false;
        };
        if error.is_some() {
            eprintln!("Nana 包内模型不能旋转");
            return true;
        }
        let Some(data) = mesh.clone() else {
            eprintln!("Nana 包内模型没有可旋转的网格");
            return true;
        };
        data
    };
    let ArchiveEntryKind::Model {
        frame,
        yaw: slot_yaw,
        zoom: slot_zoom,
        ..
    } = &mut entry.kind
    else {
        return false;
    };
    *slot_yaw += yaw;
    if zoom > 0.0 {
        *slot_zoom = (*slot_zoom * zoom).clamp(0.25, 8.0);
    }
    *frame = Some(super::super::paint_mesh(&data, *slot_yaw, *slot_zoom));
    true
}

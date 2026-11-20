//! 歌词视口上下的淡出。
//!
//! Vue `.media-preview__audio-lyrics` 写
//! `mask-image: linear-gradient(180deg, transparent 0, #000 12%, #000 86%, transparent 100%)`。
//! 锁定的 NanaUI 把渐变遮罩只套在节点自己的底色和 Painter 上，子节点和节点自己的文字都不进遮罩，
//! 也没有把子树收成一层再罩的接口。这里按每一行中心在视口里的位置取样同一条遮罩，写进行的不透明度。
//! 一行内部不再做连续渐变。不另铺色块盖住歌词。

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui::runtime::{Button, Entity, FrameworkError, RuntimeDocument, StableNodeId, Text};
use nana_ui_core::LayoutStyle;

use super::LyricFollow;

/// 遮罩从顶上透明收到实的位置。
const FADE_TOP: f32 = 0.12;
/// 遮罩开始往底下透明收的位置。底下这一段是 `1 - 0.86`。
const FADE_BOTTOM: f32 = 0.86;
/// 不透明度量化步长。避免布局的亚像素抖动来回改样式、不停要帧。
const OPACITY_STEP: f32 = 1.0 / 16.0;

/// 按当前滚动，把视口边缘的歌词行淡出。没有歌词滚动区时不动。
///
/// 滚动区和偏移都没变时不再扫整棵树。返回有没有改到某一行的不透明度，调用方据此再画一帧。
/// 正中一带是整不透明，不写不透明度，三档明暗、放大和投影保持原样。
pub(super) fn apply(document: &mut RuntimeDocument, followed: &mut Option<LyricFollow>) -> bool {
    let Some(scroll) = scroll_id(document, followed) else {
        return false;
    };
    let Some((height, scroll_y)) = viewport(document, scroll) else {
        return false;
    };
    let key = fade_key(scroll, height, scroll_y);
    if followed
        .as_ref()
        .is_some_and(|state| state.scroll == scroll && state.fade_key == Some(key))
    {
        return false;
    }
    let Some(samples) = samples(document, scroll, height, scroll_y) else {
        return false;
    };
    let mut changed = false;
    let mut failed = false;
    for (id, opacity) in samples {
        match write_opacity(document, id, opacity) {
            Ok(true) => changed = true,
            Ok(false) => {}
            Err(()) => failed = true,
        }
    }
    if !failed {
        if let Some(state) = followed.as_mut().filter(|state| state.scroll == scroll) {
            state.fade_key = Some(key);
        }
    }
    changed
}

/// 记下这次淡出对应的滚动区和偏移。下次一样就跳过。
fn fade_key(scroll: StableNodeId, height: f32, scroll_y: f32) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    scroll.hash(&mut hasher);
    height.to_bits().hash(&mut hasher);
    scroll_y.to_bits().hash(&mut hasher);
    hasher.finish()
}

/// 已经跟上的滚动区直接用，避免每一帧再把整棵树扫一遍找它。
fn scroll_id(document: &RuntimeDocument, followed: &Option<LyricFollow>) -> Option<StableNodeId> {
    if let Some(state) = followed.as_ref() {
        let still = document
            .context()
            .assembly_path(state.scroll)
            .is_some_and(|path| path.rsplit('/').next() == Some("inspect-audio-lyric-scroll"));
        if still {
            return Some(state.scroll);
        }
    }
    keyed(document, "inspect-audio-lyric-scroll")
}

/// 视口高度和纵向滚动。高度还没排出来，或偏移不是有限数时不淡出。
fn viewport(document: &RuntimeDocument, scroll: StableNodeId) -> Option<(f32, f32)> {
    let world = document.context().world();
    let height = world.scroll_metrics(scroll)?.viewport_height;
    if !height.is_finite() || height <= 0.0 {
        return None;
    }
    let scroll_y = world
        .scroll_offset(scroll)
        .map(|offset| offset.y)
        .unwrap_or(0.0);
    if !scroll_y.is_finite() {
        eprintln!("Nana 歌词滚动偏移无效，上下淡出先不写：{scroll_y}");
        return None;
    }
    Some((height, scroll_y))
}

/// `t` 是视口高度上的比例，0 在顶、1 在底。
///
/// `linear-gradient(180deg, transparent 0, #000 12%, #000 86%, transparent 100%)`：
/// 180deg 向下，色标的不透明度就是遮罩强度。
fn mask_alpha(t: f32) -> f32 {
    if !t.is_finite() {
        return 1.0;
    }
    if t <= 0.0 {
        0.0
    } else if t < FADE_TOP {
        t / FADE_TOP
    } else if t <= FADE_BOTTOM {
        1.0
    } else if t < 1.0 {
        (1.0 - t) / (1.0 - FADE_BOTTOM)
    } else {
        0.0
    }
}

/// 整不透明收成 `None`，其余按步长取整，避免和「没写不透明度」叠成两套。
fn quantize(alpha: f32) -> Option<f32> {
    if !alpha.is_finite() {
        eprintln!("Nana 歌词遮罩算出的不透明度无效，这一行保持原样：{alpha}");
        return None;
    }
    let stepped = (alpha / OPACITY_STEP).round() * OPACITY_STEP;
    if stepped >= 1.0 - OPACITY_STEP * 0.5 {
        None
    } else {
        Some(stepped.clamp(0.0, 1.0))
    }
}

/// 一行中心在视口里的比例。布局盒不含滚动，要减去滚动偏移。
fn viewport_t(line_y: f32, line_h: f32, view_y: f32, scroll_y: f32, viewport_h: f32) -> f32 {
    (line_y - view_y - scroll_y + line_h * 0.5) / viewport_h
}

/// 每一行该写成的不透明度。有一行还没排出来时返回 `None`，这次不记成已经淡过。
fn samples(
    document: &RuntimeDocument,
    scroll: StableNodeId,
    height: f32,
    scroll_y: f32,
) -> Option<Vec<(StableNodeId, Option<f32>)>> {
    let context = document.context();
    let world = context.world();
    let view = world.layout_box(scroll)?;
    let mut rows = Vec::new();
    for id in world.document_order(document.document()) {
        let Some(path) = context.assembly_path(id) else {
            continue;
        };
        let Some(key) = path.rsplit('/').next() else {
            continue;
        };
        if !is_lyric_line(key) {
            continue;
        }
        let Some(line) = world.layout_box(id) else {
            return None;
        };
        if !line.y.is_finite() || !line.height.is_finite() || line.height <= 0.0 {
            eprintln!("Nana 歌词行还没有可用的布局，这一行先不淡出：{key}");
            return None;
        }
        let t = viewport_t(line.y, line.height, view.y, scroll_y, height);
        rows.push((id, quantize(mask_alpha(t))));
    }
    if rows.is_empty() { None } else { Some(rows) }
}

/// 键是 `inspect-audio-lyric-active` 或 `inspect-audio-lyric-` 加行号。滚动区和留白不是行。
fn is_lyric_line(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("inspect-audio-lyric-") else {
        return false;
    };
    rest == "active" || rest.chars().all(|ch| ch.is_ascii_digit())
}

/// 键路径最后一段是 `key` 的节点。
fn keyed(document: &RuntimeDocument, key: &str) -> Option<StableNodeId> {
    let context = document.context();
    context
        .world()
        .document_order(document.document())
        .into_iter()
        .find(|id| {
            context
                .assembly_path(*id)
                .is_some_and(|path| path.rsplit('/').next() == Some(key))
        })
}

/// 把不透明度写进行组件。有时间的行是按钮，没有时间的行是文字。写成一样就不动。
/// 写不进去时返回错误，调用方这轮不把淡出记成已经完成。
fn write_opacity(
    document: &mut RuntimeDocument,
    id: StableNodeId,
    opacity: Option<f32>,
) -> Result<bool, ()> {
    let current = document
        .context()
        .world()
        .node_style(id)
        .and_then(|style| style.layout.opacity);
    if same_opacity(current, opacity) {
        return Ok(false);
    }
    match document.context_mut().update_component(
        Entity::<Button>::from_stable_id(id),
        |button, _| {
            set_layout_opacity(&mut button.style.layout, opacity);
        },
    ) {
        Ok(()) => Ok(true),
        Err(FrameworkError::ViewType(_)) => write_text_opacity(document, id, opacity),
        Err(error) => {
            eprintln!("Nana 歌词淡出写不进这一行：{id:?}：{error}");
            Err(())
        }
    }
}

/// 没有时间的歌词行是文字节点。
fn write_text_opacity(
    document: &mut RuntimeDocument,
    id: StableNodeId,
    opacity: Option<f32>,
) -> Result<bool, ()> {
    match document
        .context_mut()
        .update_component(Entity::<Text>::from_stable_id(id), |text, _| {
            set_layout_opacity(&mut text.style.layout, opacity);
        }) {
        Ok(()) => Ok(true),
        Err(error) => {
            eprintln!("Nana 歌词淡出写不进这一行：{id:?}：{error}");
            Err(())
        }
    }
}

/// 只改不透明度，字色、放大和投影留在原来的样式里。
fn set_layout_opacity(layout: &mut Arc<LayoutStyle>, opacity: Option<f32>) {
    Arc::make_mut(layout).opacity = opacity;
}

/// 两次取样落在同一步上就算没变。`None` 只和 `None` 相同。
fn same_opacity(current: Option<f32>, next: Option<f32>) -> bool {
    match (current, next) {
        (None, None) => true,
        (Some(current), Some(next)) => (current - next).abs() < OPACITY_STEP * 0.25,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use nana_ui::runtime::{Entity, ScrollOffset, ScrollView};

    use crate::shell::ShellViewModel;
    use crate::shell::view_harness::ShellHarness;

    /// 歌词视口上下按 Vue 的遮罩淡出：顶上 12% 从透明收到实，底下从 86% 收到透明。
    /// 正中的当前行不写不透明度。滚开以后按新位置重算，不把已经滚开的当前行拉回去。
    /// 滚动区自己不挂遮罩，也不铺色块。
    #[test]
    fn lyric_viewport_fades_lines_at_the_vue_mask() {
        assert!(super::mask_alpha(0.0).abs() < 1e-4, "顶上是透明");
        assert!(
            (super::mask_alpha(0.06) - 0.5).abs() < 1e-4,
            "12% 处收到一半"
        );
        assert!((super::mask_alpha(0.12) - 1.0).abs() < 1e-4);
        assert!(
            (super::mask_alpha(0.5) - 1.0).abs() < 1e-4,
            "中间整段是实的"
        );
        assert!((super::mask_alpha(0.86) - 1.0).abs() < 1e-4);
        assert!(
            (super::mask_alpha(0.93) - 0.5).abs() < 1e-4,
            "86% 往下收到一半"
        );
        assert!(super::mask_alpha(1.0).abs() < 1e-4, "底下是透明");
        assert_eq!(super::quantize(1.0), None, "整不透明不另写一层");
        assert_eq!(super::quantize(0.0), Some(0.0));

        let mut model = scene();
        let mut lines = (0..30)
            .map(|index| format!("{}第{index}行", lrc_tag(index as u64)))
            .collect::<Vec<_>>();
        lines.push("没有时间".into());
        model.inspect.lyrics = crate::shell::inspect_lyrics::parse_lrc(&lines.join("\n"));
        place_playhead(&mut model, 15);
        let mut harness = ShellHarness::mount(model);
        let rows = line_rows(&harness);
        let active = rows
            .iter()
            .find(|row| row.key == "inspect-audio-lyric-active")
            .expect("当前行");
        assert!(
            (active.t - 0.5).abs() < 0.08,
            "当前行仍在视口正中，遮罩不改居中：{}",
            active.t
        );
        assert_eq!(active.opacity, None, "正中的当前行保持原来的不透明度");
        assert!(active.mix.is_none(), "当前行仍是完整正文");
        let solid = rows
            .iter()
            .find(|row| (0.25..0.75).contains(&row.t) && row.key != "inspect-audio-lyric-active");
        assert!(
            solid.is_some_and(|row| row.opacity.is_none()),
            "遮罩的实心段不另淡一行：{rows:?}"
        );
        let faded = rows.iter().find(|row| matches!(super::quantize(super::mask_alpha(row.t)), Some(alpha) if alpha < 0.85)).unwrap_or_else(|| {
            panic!("视口边缘要有一行被遮罩淡出：{rows:?}");
        });
        assert_eq!(
            faded.opacity,
            super::quantize(super::mask_alpha(faded.t)),
            "淡出跟遮罩在行心的取样"
        );
        assert!(faded.mix.is_some(), "淡出不改这一行原来的明暗");
        assert!(faded.background.is_none(), "不用色块盖住歌词");
        let scroll = node_style(&harness, "inspect-audio-lyric-scroll");
        assert!(scroll.painter.is_none(), "淡出不挂在滚动区的自绘上");
        assert!(
            scroll.layout.paint.mask.is_none(),
            "子节点吃不到节点遮罩，不装一个没用的遮罩充数"
        );
        assert!(scroll.background.is_none(), "滚动区不铺底色盖住歌词");

        let scroll_id = harness
            .keyed("inspect-audio-lyric-scroll")
            .expect("歌词滚动");
        harness
            .window
            .document
            .context_mut()
            .scroll_to(
                Entity::<ScrollView>::from_stable_id(scroll_id),
                ScrollOffset { x: 0.0, y: 0.0 },
            )
            .expect("滚回顶部");
        harness.flush();
        let offset = harness
            .document()
            .context()
            .world()
            .scroll_offset(scroll_id)
            .unwrap_or_default()
            .y;
        assert!(
            offset.abs() < 0.5,
            "同一行、同一视口高度时，读者滚开的位置不被拉回去：{offset}"
        );
        let again = line_rows(&harness);
        let moved = again
            .iter()
            .find(|row| row.key == "inspect-audio-lyric-active")
            .expect("当前行还在");
        assert!(moved.t > 0.85, "滚到顶以后当前行离开正中：{}", moved.t);
        assert_eq!(
            moved.opacity,
            super::quantize(super::mask_alpha(moved.t)),
            "滚开以后按新位置重算淡出"
        );
    }

    /// 一行在视口里的位置、不透明度、字色混合和底色。
    #[derive(Debug)]
    struct Row {
        key: String,
        t: f32,
        opacity: Option<f32>,
        mix: Option<nana_ui_core::SemanticColorMix>,
        background: Option<nana_ui::runtime::SemanticColorRole>,
    }

    /// 读出每一行歌词现在的遮罩取样。
    fn line_rows(harness: &ShellHarness) -> Vec<Row> {
        let scroll = harness
            .keyed("inspect-audio-lyric-scroll")
            .expect("歌词滚动");
        let context = harness.document().context();
        let world = context.world();
        let view = world.layout_box(scroll).expect("视口布局");
        let height = world
            .scroll_metrics(scroll)
            .expect("视口尺寸")
            .viewport_height;
        let scroll_y = world.scroll_offset(scroll).unwrap_or_default().y;
        let mut rows = Vec::new();
        for id in world.document_order(harness.document().document()) {
            let Some(path) = context.assembly_path(id) else {
                continue;
            };
            let Some(key) = path.rsplit('/').next() else {
                continue;
            };
            if !super::is_lyric_line(key) {
                continue;
            }
            let Some(line) = world.layout_box(id) else {
                continue;
            };
            let style = world.node_style(id);
            rows.push(Row {
                key: key.to_string(),
                t: super::viewport_t(line.y, line.height, view.y, scroll_y, height),
                opacity: style.as_ref().and_then(|item| item.layout.opacity),
                mix: style
                    .as_ref()
                    .and_then(|item| item.interaction.base.foreground_mix),
                background: style.and_then(|item| item.background),
            });
        }
        rows
    }

    fn node_style(harness: &ShellHarness, key: &str) -> nana_ui::runtime::NodeStyle {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .node_style(id)
            .unwrap_or_else(|| panic!("{key} 没有样式"))
            .clone()
    }

    fn scene() -> ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "preview-audio")
            .expect("音频预览场景")
            .1
    }

    fn place_playhead(model: &mut ShellViewModel, ms: u64) {
        let mut session = model.inspect.media_session().cloned().expect("预览会话");
        session.current_time_ms = ms;
        model.inspect.replace_shared_media(session);
    }

    fn lrc_tag(ms: u64) -> String {
        let minutes = ms / 60_000;
        let seconds = (ms % 60_000) / 1000;
        let fraction = ms % 1000;
        format!("[{minutes:02}:{seconds:02}.{fraction:03}]")
    }
}

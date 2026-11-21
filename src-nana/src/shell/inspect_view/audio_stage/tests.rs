//! 唱片舞台的测试：元数据标题、封面、歌词点击跳转、淡入与居中跟随。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{AudioMeta, audio_caption};
use crate::shell::SearchRow;
use crate::shell::ShellViewModel;
use crate::shell::view_harness::ShellHarness;

/// 音频舞台按元数据写标题、艺人和专辑。
/// 文件列表里有这一条时用列表上的元数据，不用搜索命中覆盖；列表里没有时才用搜索命中。
#[test]
fn audio_stage_shows_metadata_title_artist_and_album() {
    let empty = audio_caption("track-01.mp3", "mp3", AudioMeta::Empty);
    assert_eq!(empty.title, "track-01");
    assert_eq!(empty.subtitle, "通用音频");
    assert!(empty.album.is_empty());

    let mut fields = BTreeMap::new();
    fields.insert("title".into(), json!("  "));
    fields.insert("artist".into(), json!("某人"));
    fields.insert("artists".into(), json!([" 周杰伦 ", " ", 1, "方文山"]));
    fields.insert("album".into(), json!(" 十一月的萧邦 "));
    let full = audio_caption("track-01.mp3", "mp3", AudioMeta::Map(&fields));
    assert_eq!(full.title, "track-01", "空白标题退回文件名");
    assert_eq!(
        full.subtitle, "周杰伦 / 方文山",
        "艺人数组里的空白和非字符串丢掉，也不再看 artist"
    );
    assert_eq!(full.album, "十一月的萧邦");

    fields.insert("title".into(), json!("夜曲"));
    fields.insert("artists".into(), json!([]));
    fields.remove("album");
    let ignored = audio_caption("track-01.mp3", "mp3", AudioMeta::Map(&fields));
    assert_eq!(ignored.title, "夜曲");
    assert_eq!(
        ignored.subtitle, "通用音频",
        "空的 artists 数组不退回 artist"
    );
    assert!(ignored.album.is_empty());

    let object = json!({"artist": " 桃 ", "album": "专辑"});
    let from_object = audio_caption(
        "song.mp3",
        "mp3",
        AudioMeta::Object(object.as_object().unwrap()),
    );
    assert_eq!(from_object.title, "song");
    assert_eq!(from_object.subtitle, "桃");
    assert_eq!(from_object.album, "专辑");

    let only_album = {
        let mut fields = BTreeMap::new();
        fields.insert("album".into(), Value::String("只有专辑".into()));
        fields
    };
    let album_only = audio_caption("track-01.mp3", "mp3", AudioMeta::Map(&only_album));
    assert_eq!(album_only.subtitle, "只有专辑");
    assert_eq!(album_only.album, "只有专辑");

    let mut model = scene("preview-audio");
    let path = model.inspect.target_path.clone().expect("音频预览路径");
    set_row(
        &mut model,
        &path,
        json!({
            "title": "夜曲",
            "artists": ["周杰伦", " ", 1, "方文山"],
            "artist": "不该出现",
            "album": "十一月的萧邦",
        }),
    );
    let shown = show(&model);
    assert_eq!(node_text(&shown, "inspect-audio-title"), "夜曲");
    assert_eq!(node_text(&shown, "inspect-audio-artist"), "周杰伦 / 方文山");
    assert_eq!(node_text(&shown, "inspect-audio-album"), "十一月的萧邦");
    assert!(shown.find("通用音频").is_none(), "有艺人时不写通用音频");

    set_row(
        &mut model,
        &path,
        json!({"artists": [], "artist": "某人", "album": "只有专辑"}),
    );
    let album = show(&model);
    assert_eq!(node_text(&album, "inspect-audio-title"), "track-01");
    assert_eq!(node_text(&album, "inspect-audio-artist"), "只有专辑");
    assert_eq!(node_text(&album, "inspect-audio-album"), "只有专辑");
    assert!(album.find("通用音频").is_none());

    set_row(&mut model, &path, json!({"artists": [], "artist": "某人"}));
    let mut hit = SearchRow {
        path: path.clone(),
        filename: "track-01.mp3".into(),
        ..SearchRow::default()
    };
    hit.metadata.insert("title".into(), json!("检索标题"));
    model.inspect.results = vec![hit];
    let listed = show(&model);
    assert_eq!(node_text(&listed, "inspect-audio-artist"), "通用音频");
    assert!(
        listed.keyed("inspect-audio-album").is_none(),
        "没有专辑不画专辑胶囊"
    );
    assert_ne!(
        node_text(&listed, "inspect-audio-title"),
        "检索标题",
        "文件还在列表里时不用搜索命中的标题"
    );

    drop_rows(&mut model, &path);
    let searched = show(&model);
    assert_eq!(
        node_text(&searched, "inspect-audio-title"),
        "检索标题",
        "列表里没有这一条时用搜索命中"
    );
}

/// 有真实图片时唱片中间用这张图；没有可读的图时仍是封面圆，不编造像素。
/// `thumbnailPath` 优先于 `coverArt`。自定义缩略图落地后就是 `thumbnailPath`。只有网址、读失败或不是图片时不画。
#[test]
fn audio_stage_draws_real_cover_or_keeps_the_disc() {
    let dir = std::env::temp_dir().join(format!("momobako-audio-cover-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("临时目录");
    let red = write_png(&dir, "red.png", [200, 20, 20, 255]);
    let blue = write_png(&dir, "blue.png", [20, 20, 200, 255]);
    let note = dir.join("note.txt");
    std::fs::write(&note, b"not an image").expect("写入非图片");
    let red_url = encoded(&red, [200, 20, 20, 255]);
    let blue_url = encoded(&blue, [20, 20, 200, 255]);

    let fields = BTreeMap::from([("coverArt".into(), json!(blue))]);
    assert_eq!(
        super::cover_file(Some(&red), AudioMeta::Map(&fields)),
        Some(red.as_str()),
        "thumbnailPath 优先"
    );
    assert_eq!(
        super::cover_file(Some("  "), AudioMeta::Map(&fields)),
        Some(blue.as_str()),
        "空白缩略图改用 coverArt"
    );
    assert_eq!(
        super::cover_file(Some("https://example.com/a.png"), AudioMeta::Map(&fields)),
        None,
        "网址不落地，也不改去画 coverArt"
    );
    assert_eq!(
        super::cover_file(None, AudioMeta::Map(&fields)),
        Some(blue.as_str())
    );

    let mut model = scene("preview-audio");
    let path = model.inspect.target_path.clone().expect("音频预览路径");
    let title = node_text(&show(&model), "inspect-audio-title");
    assert!(record_image(&show(&model)).is_none(), "没有图时不画封面");
    assert!(
        show(&model).keyed("inspect-audio-chip").is_some(),
        "没有图时唱片中间仍是音频"
    );

    set_row(&mut model, &path, json!({}));
    set_thumb(&mut model, &path, Some(&red), true);
    let custom = show(&model);
    assert_eq!(
        record_image(&custom).as_deref(),
        Some(red_url.as_str()),
        "自定义缩略图落地后画这张图"
    );
    assert!(
        custom.keyed("inspect-audio-chip").is_none(),
        "有图时不再写唱片中间的音频"
    );
    assert_eq!(
        node_text(&custom, "inspect-audio-title"),
        title,
        "封面不改标题"
    );

    set_row(&mut model, &path, json!({"coverArt": blue}));
    set_thumb(&mut model, &path, Some(&red), false);
    assert_eq!(
        record_image(&show(&model)).as_deref(),
        Some(red_url.as_str()),
        "有缩略图时不用 coverArt"
    );

    set_thumb(&mut model, &path, None, false);
    assert_eq!(
        record_image(&show(&model)).as_deref(),
        Some(blue_url.as_str()),
        "没有缩略图时画 coverArt 指向的文件"
    );

    set_thumb(
        &mut model,
        &path,
        Some("https://example.com/cover.png"),
        false,
    );
    let remote = show(&model);
    assert!(record_image(&remote).is_none(), "只有网址时不画，也不编造");
    assert!(remote.keyed("inspect-audio-chip").is_some());

    set_row(&mut model, &path, json!({}));
    set_thumb(
        &mut model,
        &path,
        Some(dir.join("missing.png").to_str().unwrap()),
        false,
    );
    assert!(
        record_image(&show(&model)).is_none(),
        "文件不在时保持封面圆"
    );
    set_thumb(&mut model, &path, Some(note.to_str().unwrap()), false);
    assert!(
        record_image(&show(&model)).is_none(),
        "不是图片时保持封面圆"
    );

    set_thumb(&mut model, &path, None, false);
    let mut hit = SearchRow {
        path: path.clone(),
        filename: "track-01.mp3".into(),
        ..SearchRow::default()
    };
    hit.metadata.insert("coverArt".into(), json!(blue));
    model.inspect.results = vec![hit];
    assert!(
        record_image(&show(&model)).is_none(),
        "文件还在列表里、这一条没有图时，不用搜索命中的封面"
    );
    drop_rows(&mut model, &path);
    assert_eq!(
        record_image(&show(&model)).as_deref(),
        Some(blue_url.as_str()),
        "列表里没有这一条时用搜索命中的 coverArt"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 有时间的歌词行点一下，播放头跳到这一行；没有时间的行点不了。暂停着的会话不被点成播放。
/// 按钮把 `currentTime` 设成 `timeMs / 1000`，`timeMs == null` 时禁用。
#[test]
fn timed_lyric_click_seeks_playback_and_untimed_lines_do_not() {
    let mut model = scene("preview-audio");
    let duration = model.player.session.duration_ms.expect("音频预览已经装好");
    assert!(duration > 0, "没有时长就分不出点了哪一行");
    assert!(model.player.session.can_seek, "装好的音频要能跳转");
    assert_eq!(model.player.session.status, "paused");
    let target = (duration / 2).max(1).min(duration);
    assert_ne!(target, 0);
    let lrc = format!("[00:00.00]开头\n{}中段\n作词：桃", lrc_tag(target));
    model.inspect.lyrics = crate::shell::inspect_lyrics::parse_lrc(&lrc);
    let mut harness = show(&model);
    assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "开头");
    assert_eq!(
        role_of(&harness, "中段"),
        nana_ui::runtime::AccessibilityRole::Button,
        "有时间的行是按钮"
    );
    assert_ne!(
        role_of(&harness, "作词：桃"),
        nana_ui::runtime::AccessibilityRole::Button,
        "没有时间的行不是按钮"
    );

    harness.click_label("中段");
    assert_eq!(
        harness.model.player.session.current_time_ms, target,
        "播放头跳到点中的那一行"
    );
    assert_eq!(
        harness.model.player.session.status, "paused",
        "点歌词只跳转，不开始播放"
    );
    assert!(
        harness.model.player.session.error.is_none(),
        "跳转不应把会话标成失败"
    );
    assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "中段");

    harness.click_label("作词：桃");
    assert_eq!(
        harness.model.player.session.current_time_ms, target,
        "没有时间的行点不了"
    );
    assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "中段");
}

/// 歌词三档明暗：当前行不淡，已经唱过的略实，还没唱到和没有时间的行更淡。有时间的行悬停再亮。
#[test]
fn lyric_lines_stay_faint_until_the_playhead_reaches_them() {
    let mut model = scene("preview-audio");
    model.inspect.lyrics =
        crate::shell::inspect_lyrics::parse_lrc("[00:00.00]开头\n[00:00.50]后来\n作词：桃");
    place_playhead(&mut model, 500);
    let harness = show(&model);
    let mix = |label: &str, hover: bool| {
        let style = harness
            .document()
            .context()
            .world()
            .node_style(harness.node(label))?;
        if hover {
            style.interaction.hovered.foreground_mix
        } else {
            style.interaction.base.foreground_mix
        }
    };
    let ink = |ratio| {
        nana_ui_core::SemanticColorMix::new(
            nana_ui::runtime::SemanticColorRole::Text,
            nana_ui::runtime::SemanticColorRole::Background,
            ratio,
        )
    };
    assert_eq!(mix("后来", false), None, "当前行是完整正文");
    assert_eq!(
        mix("开头", false),
        Some(ink(super::LYRIC_PASSED_MIX)),
        "已经唱过的略实"
    );
    assert_eq!(
        mix("作词：桃", false),
        Some(ink(super::LYRIC_AHEAD_MIX)),
        "没有时间的行和还没唱到的一样淡"
    );
    assert_eq!(
        mix("开头", true),
        Some(ink(super::LYRIC_HOVER_MIX)),
        "有时间的行悬停再亮"
    );
    let align = harness
        .document()
        .context()
        .world()
        .node_style(harness.node("后来"))
        .map(|style| style.text_horizontal_alignment);
    assert_eq!(
        align,
        Some(nana_ui::runtime::TextHorizontalAlignment::Center)
    );
}

/// 当前行滚到歌词视口正中。没有歌词或还没到第一行时停在顶上，短到滚不动时也不跳走。
#[test]
fn active_lyric_centers_in_the_viewport() {
    assert_eq!(
        super::lyric_inset(100.0),
        96.0,
        "视口不到 192 时留白至少 96"
    );
    assert_eq!(super::lyric_inset(400.0), 200.0);
    assert_eq!(super::lyric_inset(401.0), 200.0, "一半向下取整");
    assert!(
        (super::lyric_scroll_top(0, 400.0, 200.0) - (super::LYRIC_LINE / 2.0 - 1.0)).abs() < 0.01
    );

    let bare = show(&scene("preview-audio"));
    assert!(
        bare.keyed("inspect-audio-lyric-scroll").is_none(),
        "没有歌词不建滚动区"
    );
    assert!(bare.keyed("inspect-audio-lyrics-empty").is_some());

    let mut waiting = scene("preview-audio");
    waiting.inspect.lyrics = crate::shell::inspect_lyrics::parse_lrc("[00:05.00]以后\n没有时间");
    let waiting = show(&waiting);
    assert!(
        waiting.keyed("inspect-audio-lyric-active").is_none(),
        "还没到第一行时没有当前行"
    );
    assert_eq!(lyric_scroll(&waiting), 0.0, "没有当前行不滚");

    let mut model = scene("preview-audio");
    let duration = model.player.session.duration_ms.expect("音频预览已经装好");
    let mut lines = (0..30)
        .map(|index| format!("{}第{index}行", lrc_tag(index as u64)))
        .collect::<Vec<_>>();
    lines.push("没有时间".into());
    model.inspect.lyrics = crate::shell::inspect_lyrics::parse_lrc(&lines.join("\n"));
    place_playhead(&mut model, 15);
    let mut harness = show(&model);
    assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "第15行");
    assert_eq!(
        node_text(&harness, "inspect-audio-lyric-30"),
        "没有时间",
        "没有时间的行不成当前行"
    );
    assert_centered(&harness);
    assert_stage_pad(&harness);
    let middle = lyric_scroll(&harness);
    assert!(
        middle > super::LYRIC_LINE,
        "中间那一行要真正滚开顶部，现在 {middle}"
    );

    click_lyric(&mut harness, "第17行");
    assert_eq!(
        harness.model.player.session.current_time_ms, 17,
        "点歌词仍按这一行的毫秒跳"
    );
    assert_eq!(harness.model.player.session.status, "paused");
    assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "第17行");
    assert_centered(&harness);
    assert!(
        lyric_scroll(&harness) > middle,
        "新的当前行更靠下，滚动也跟着下去"
    );

    let mut one = scene("preview-audio");
    one.inspect.lyrics = crate::shell::inspect_lyrics::parse_lrc("[00:00.00]只有一行");
    let one = show(&one);
    assert_centered(&one);
    let alone = lyric_scroll(&one);
    assert!(
        alone < super::LYRIC_LINE,
        "一行歌词只为留白挪一点点，不应跳走：{alone}"
    );
    assert!(duration > 17, "夹具要长过被点的那一行");
}

/// 歌词视口上下渐隐交给滚动区：顶上 12%、底下 14% 视口高的渐隐带写在滚动区样式上，
/// 滚动区连同歌词合成一层淡出。行自己不写不透明度，也不铺色块盖住歌词。
#[test]
fn lyric_viewport_fades_at_its_edges() {
    let band = super::lyric_edge_fade(400.0);
    assert_eq!((band.top, band.bottom), (48.0, 56.0));
    assert_eq!(
        super::lyric_edge_fade(401.3),
        band,
        "带长按整像素取，亚像素抖动不改样式"
    );

    let mut model = scene("preview-audio");
    let mut lines = (0..30)
        .map(|index| format!("{}第{index}行", lrc_tag(index as u64)))
        .collect::<Vec<_>>();
    lines.push("没有时间".into());
    model.inspect.lyrics = crate::shell::inspect_lyrics::parse_lrc(&lines.join("\n"));
    place_playhead(&mut model, 15);
    let harness = show(&model);
    let scroll = harness
        .keyed("inspect-audio-lyric-scroll")
        .expect("歌词滚动");
    let world = harness.document().context().world();
    let height = world
        .scroll_metrics(scroll)
        .expect("视口尺寸")
        .viewport_height;
    let style = world.node_style(scroll).expect("滚动区样式");
    assert_eq!(
        style.layout.paint.scroll_edge_fade,
        Some(super::lyric_edge_fade(height)),
        "渐隐带跟着量出来的视口高度"
    );
    assert!(style.background.is_none(), "滚动区不铺底色盖住歌词");
    for key in [
        "inspect-audio-lyric-active",
        "inspect-audio-lyric-0",
        "inspect-audio-lyric-30",
    ] {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        let line = world.node_style(id).expect("行样式");
        assert_eq!(line.layout.opacity, None, "{key} 不另写不透明度");
    }
}

/// 把预览会话的播放头拨到 `ms`。歌词当前行读的是这份会话。
fn place_playhead(model: &mut ShellViewModel, ms: u64) {
    let mut session = model.inspect.media_session().cloned().expect("预览会话");
    session.current_time_ms = ms;
    model.inspect.replace_shared_media(session);
}

fn lyric_scroll(harness: &ShellHarness) -> f32 {
    let id = harness
        .keyed("inspect-audio-lyric-scroll")
        .expect("歌词滚动");
    harness
        .document()
        .context()
        .world()
        .scroll_offset(id)
        .unwrap_or_default()
        .y
}

/// 当前行的布局盒中心落在视口中心；超出可滚范围时停在滚动区的两端。
fn assert_centered(harness: &ShellHarness) {
    let scroll = harness
        .keyed("inspect-audio-lyric-scroll")
        .expect("歌词滚动");
    let active = harness.keyed("inspect-audio-lyric-active").expect("当前行");
    let world = harness.document().context().world();
    let item = world.layout_box(active).expect("当前行布局");
    let view = world.layout_box(scroll).expect("视口布局");
    let metrics = world.scroll_metrics(scroll).expect("滚动尺寸");
    let offset = world.scroll_offset(scroll).unwrap_or_default().y;
    let raw = item.y - view.y - view.height / 2.0 + item.height / 2.0;
    let expected = raw.max(metrics.origin_y).min(metrics.max_offset().y);
    assert!(
        (offset - expected).abs() < 1.0,
        "当前行应在视口正中：偏移 {offset}，期望 {expected}，行 {:?}，视口 {:?}",
        item,
        view
    );
}

/// 第一行的顶到滚动内容顶的距离是上留白 `inset - 1`。
fn assert_stage_pad(harness: &ShellHarness) {
    let scroll = harness
        .keyed("inspect-audio-lyric-scroll")
        .expect("歌词滚动");
    let first = harness.keyed("inspect-audio-lyric-0").expect("第一行");
    let world = harness.document().context().world();
    let view = world.layout_box(scroll).expect("视口布局");
    let line = world.layout_box(first).expect("第一行布局");
    let inset = super::lyric_inset(view.height);
    let pad = (line.y - view.y) - (inset - 1.0);
    assert!(
        pad.abs() < 1.0,
        "上留白应是 {inset} - 1，差了 {pad}（视口高 {}）",
        view.height
    );
}

/// 点歌词行的视觉位置。布局盒不含滚动，得点在滚过以后的那一处。
fn click_lyric(harness: &mut ShellHarness, label: &str) {
    let id = harness.node(label);
    let (x, y) = harness.center(id);
    let offset = lyric_scroll(harness);
    let messages = harness.click_messages(x, y - offset);
    harness.settle(messages);
}

/// `[分:秒.毫秒]`，毫秒取三位数，和 LRC 解析那一头对得上。
fn lrc_tag(ms: u64) -> String {
    let minutes = ms / 60_000;
    let seconds = (ms % 60_000) / 1000;
    let fraction = ms % 1000;
    format!("[{minutes:02}:{seconds:02}.{fraction:03}]")
}

fn role_of(harness: &ShellHarness, label: &str) -> nana_ui::runtime::AccessibilityRole {
    let id = harness
        .find(label)
        .unwrap_or_else(|| panic!("没有 {label}"));
    harness
        .nodes()
        .into_iter()
        .find(|node| node.id == id)
        .unwrap_or_else(|| panic!("{label} 不在无障碍树里"))
        .role
}

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

fn set_row(model: &mut ShellViewModel, path: &str, metadata: Value) {
    let row = model
        .files
        .rows
        .iter_mut()
        .find(|row| row.path == path)
        .unwrap_or_else(|| panic!("列表里没有 {path}"));
    row.metadata = metadata
        .as_object()
        .expect("元数据对象")
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
}

fn drop_rows(model: &mut ShellViewModel, path: &str) {
    model.files.rows.retain(|row| row.path != path);
    model.files.virtual_rows.retain(|row| row.path != path);
    model.files.category_rows.retain(|row| row.path != path);
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

/// 写成一张纯色 PNG。颜色要能在解码结果里对上，避免舞台拿别的像素充封面。
fn write_png(dir: &std::path::Path, name: &str, color: [u8; 4]) -> String {
    let path = dir.join(name);
    image::RgbaImage::from_pixel(8, 8, image::Rgba(color))
        .save(&path)
        .unwrap_or_else(|error| panic!("写入 {name}：{error}"));
    path.to_string_lossy().into_owned()
}

/// 用和舞台同一套解码、编码，把文件收成 data URL，并核对像素就是写进去的那张。
fn encoded(path: &str, color: [u8; 4]) -> String {
    let frame = crate::shell::decode_thumbnail_file(path)
        .unwrap_or_else(|error| panic!("测试图解不开：{error}"));
    assert_eq!(
        frame.rgba.len(),
        frame.width as usize * frame.height as usize * 4,
        "像素长度对不上"
    );
    assert_eq!(&frame.rgba[..4], &color, "封面左上角不是写进文件的颜色");
    crate::shell::inspect::rgba_png_data_url(frame.width, frame.height, &frame.rgba)
        .expect("测试图能编码")
}

fn set_thumb(model: &mut ShellViewModel, path: &str, thumb: Option<&str>, custom: bool) {
    let mut found = false;
    for row in model
        .files
        .rows
        .iter_mut()
        .chain(model.files.virtual_rows.iter_mut())
        .chain(model.files.category_rows.iter_mut())
    {
        if row.path == path {
            row.thumbnail_path = thumb.map(str::to_string);
            row.thumbnail_custom = custom;
            found = true;
        }
    }
    assert!(found, "列表里没有 {path}");
}

/// 唱片自绘里的封面图。没有 `Image` 命令就是没画图。
fn record_image(harness: &ShellHarness) -> Option<String> {
    let id = harness.keyed("inspect-audio-record").expect("唱片");
    let style = harness
        .document()
        .context()
        .world()
        .node_style(id)
        .expect("唱片样式");
    let painter = style.painter.as_ref().expect("唱片自绘").painter();
    let theme = nana_ui::theme::builtin_theme_arc(nana_ui::theme::ThemeAppearance::Dark);
    let recording = nana_ui::runtime::PaintRecording::record(
        painter,
        theme.as_ref(),
        [200.0, 200.0],
        nana_ui::runtime::PaintState::default(),
    );
    let mut found = None;
    for op in recording
        .behind_children
        .iter()
        .chain(&recording.over_children)
    {
        if let nana_ui::runtime::PaintOp::Image {
            source, fit, rect, ..
        } = op
        {
            assert_eq!(*fit, nana_ui::runtime::ImageFit::Cover);
            let side = 200.0 * 0.58;
            assert!(
                (rect.width - side).abs() < 0.01 && (rect.height - side).abs() < 0.01,
                "封面应落在中间 58% 的圆里"
            );
            assert!(found.replace(source.to_string()).is_none(), "只画一张封面");
        }
    }
    found
}

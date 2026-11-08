//! 播放条的零件：停留时长与画面适配设置行、当前队列浮层，以及几处共用的小部件。
//!
//! 浮层贴在卡片右上方，和 Vue `.workspace-player__queue` 一样右侧内缩 14px、
//! 底边离卡片 10px。绝对定位相对父级内容盒，所以偏移要把卡片内边距算进去。
//! 队列封面和播放条自己的封面都只认条目上的 `thumbnailPath`：读得到本地图片就按 cover 画进方块，否则仍写类型字。

use nana_ui::Icon;
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, IconButton, IconGlyph, JustifySpec, LengthSpec, RadiusTier,
    RangeField, RangeInput, ScrollAxes, ScrollView, SemanticColorRole, Stack, Text,
    TextHorizontalAlignment,
};

use super::super::super::ShellMessage;
use super::super::super::player::{PlayerMessage, QueueItem};
use super::super::paint::{HoverSurface, SliderTrack};
use super::{BarProps, display_title};

/// 卡片内边距：上 12、左右 16。浮层的绝对定位从内容盒算起。
const CARD_PADDING_TOP: f32 = 12.0;
const CARD_PADDING_X: f32 = 16.0;
/// 浮层上限高度，和 Vue `max-height: 320px` 一致。
const QUEUE_MAX_HEIGHT: f32 = 320.0;
/// 浮层内边距和行距。
const QUEUE_PADDING: f32 = 10.0;
const QUEUE_HEAD: f32 = 20.15;

/// 单行文字：给定字号、字重、颜色和行高，超长省略。
pub(super) fn line(
    label: impl Into<String>,
    size: f32,
    weight: u16,
    role: SemanticColorRole,
    line_height: f32,
) -> Text {
    let mut text = Text::new(label.into())
        .color(role)
        .font_size(size)
        .font_weight(weight)
        .line_height(line_height)
        .truncating();
    let layout = std::sync::Arc::make_mut(&mut text.style.layout);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Fill);
    text
}

/// 整块不透明度盖在已知底色上时的字色：在 sRGB 里和底色直接混，和浏览器的合成一致。
/// `dim` 是（底色，不透明度）。
pub(crate) fn dim_text(mut text: Text, dim: Option<(SemanticColorRole, f32)>) -> Text {
    if let Some((base, ratio)) = dim {
        let role = text.style.foreground.unwrap_or(SemanticColorRole::Text);
        text.style.interaction.base.foreground_mix =
            Some(nana_ui_core::SemanticColorMix::new(role, base, ratio));
    }
    text
}

/// 同 [`dim_text`]，给方块底色用。
pub(crate) fn dim_surface(
    stack: Stack,
    role: SemanticColorRole,
    dim: Option<(SemanticColorRole, f32)>,
) -> Stack {
    match dim {
        Some((base, ratio)) => {
            let style = stack
                .node_style()
                .surface_mix(nana_ui_core::SemanticColorMix::new(role, base, ratio));
            stack.style(style)
        }
        None => stack.surface(role),
    }
}

/// 固定像素的图标字形。
pub(super) fn icon(icon: Icon, size: f32, role: SemanticColorRole) -> IconGlyph {
    IconGlyph::new(icon).size(size).role(role)
}

/// 没有缩略图时的类型方块：bg 底、10px 圆角、11px 粗体弱色字居中。
pub(super) fn type_cover(
    label: &str,
    size: f32,
    key: impl Into<String>,
    dim: Option<(SemanticColorRole, f32)>,
) -> AnyView {
    let mut text = Text::new(label)
        .color(SemanticColorRole::Muted)
        .font_size(11.0)
        .font_weight(700)
        .line_height(17.05);
    text.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let frame = Stack::column(0.0)
        .align(AlignSpec::Center)
        .justify(JustifySpec::Center)
        .width(LengthSpec::Px(size))
        .height(LengthSpec::Px(size))
        .min_width(LengthSpec::Px(size))
        .min_height(LengthSpec::Px(size))
        .grow(0.0)
        .shrink(0.0)
        .radius(RadiusTier::Lg);
    widget(dim_surface(frame, SemanticColorRole::Background, dim))
        .children((widget(dim_text(text, dim)),))
        .key(key.into())
        .into_any()
}

/// 队列里的 40px 封面，对应 Vue `.workspace-player__queue-thumb`。
/// 有可读的本地 `thumbnailPath` 时把那张图画满这一格；否则写当前播放器的类型字。
fn queue_cover(
    item: &QueueItem,
    media: &'static str,
    dim: Option<(SemanticColorRole, f32)>,
) -> AnyView {
    let key = format!("player-queue-cover-{}", super::super::key_part(&item.id));
    match queue_cover_url(item.thumbnail_path.as_deref()) {
        Some(url) => cover_image(url, 40.0, key, dim, false),
        None => type_cover(media, 40.0, key, dim),
    }
}

/// 播放条自己的封面。有可读本地图时按 cover 铺进方块，这一格不接收点击，点下去仍是打开预览。
pub(super) fn player_still(path: Option<&str>, size: f32, key: String) -> Option<AnyView> {
    player_cover_url(path).map(|url| cover_image(url, size, key, None, true))
}

/// 队列封面用的本地图。日志沿用「播放条队列缩略图」。
fn queue_cover_url(path: Option<&str>) -> Option<String> {
    local_cover_url(path, "播放条队列缩略图")
}

/// 播放条自己的封面用的本地图。
fn player_cover_url(path: Option<&str>) -> Option<String> {
    local_cover_url(path, "播放条封面")
}

/// 把条目的 `thumbnailPath` 收成封面 data URL。
///
/// 只认这一个字段，不改用 `coverArt`。去掉空白后为空、或是还没落地的 http(s)，都不算有图。
/// 读失败、不是图片、编不成图时记日志并交回空，调用方继续写类型字，不填假像素。
fn local_cover_url(path: Option<&str>, place: &str) -> Option<String> {
    let path = path.map(str::trim).filter(|text| !text.is_empty())?;
    if is_remote(path) {
        return None;
    }
    let frame = match crate::shell::decode_thumbnail_file(path) {
        Ok(frame) => frame,
        Err(error) => {
            eprintln!("Nana {place}读不出来，仍写类型字：{path}：{error}");
            return None;
        }
    };
    match crate::shell::inspect::rgba_png_data_url(frame.width, frame.height, &frame.rgba) {
        Some(url) => Some(url),
        None => {
            eprintln!("Nana {place}编码失败，仍写类型字：{path}");
            None
        }
    }
}

/// `http://` 或 `https://`。这种地址还不是本地文件，播放条不下载。
fn is_remote(path: &str) -> bool {
    (path.len() >= 7 && path[..7].eq_ignore_ascii_case("http://"))
        || (path.len() >= 8 && path[..8].eq_ignore_ascii_case("https://"))
}

/// 把已经解码的本地图按 `object-fit: cover` 铺进方块。圆角和类型方块一样，多出来的裁掉。
/// 不可播放的队列条目按整行的 0.52 把这张图淡一次。`ignore_pointer` 时点击穿透到下面的按钮。
fn cover_image(
    url: String,
    size: f32,
    key: String,
    dim: Option<(SemanticColorRole, f32)>,
    ignore_pointer: bool,
) -> AnyView {
    let fade = dim.map(|(_, ratio)| ratio);
    let image = nana_ui_core::BackgroundImage::Url {
        url,
        fit: nana_ui_core::BackgroundImageFit::Cover,
        size_width: None,
        size_height: None,
        position: nana_ui_core::BackgroundPosition::center(),
        repeat: nana_ui_core::BackgroundRepeat::NoRepeat,
        sampling: nana_ui_core::ImageSampling::Resample,
    };
    let frame = Stack::column(0.0)
        .width(LengthSpec::Px(size))
        .height(LengthSpec::Px(size))
        .min_width(LengthSpec::Px(size))
        .min_height(LengthSpec::Px(size))
        .grow(0.0)
        .shrink(0.0)
        .radius(RadiusTier::Lg)
        .surface(SemanticColorRole::Background)
        .with_layout(move |layout| {
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            layout.paint.content_image = Some(image);
            layout.opacity = fade;
            if ignore_pointer {
                layout.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
            }
        });
    widget(frame).key(key).into_any()
}

/// 盖满父级的透明可点区域。名字只给无障碍；悬停、按下时铺底色，内容由兄弟节点画在上面。
/// 要放在兄弟节点前面，绘制顺序才在下层。绝对定位从父级内容盒算起，`inset` 是父级内边距，
/// 往外扩回去才盖住整块。
pub(crate) fn hit_area(label: &str, disabled: bool, inset: f32, radius: f32) -> IconButton {
    let mut button = IconButton::new(super::icons::PLAY, label)
        .colors_from_style()
        .disabled(disabled);
    button.style.background = None;
    button.style.control_padding_x = None;
    button.style.painter = Some(HoverSurface { radius }.into());
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.position = nana_ui_core::PositionSpec::Absolute;
    layout.offset_left = Some(LengthSpec::Px(-inset));
    layout.offset_top = Some(LengthSpec::Px(-inset));
    layout.width = Some(LengthSpec::CalcPercentOffset {
        percent: 100.0,
        offset_px: inset * 2.0,
    });
    layout.height = Some(LengthSpec::CalcPercentOffset {
        percent: 100.0,
        offset_px: inset * 2.0,
    });
    button
}

/// 细滑杆：宽 `width`，压缩时最窄 `min_width`，高 20px。禁用时整条 0.45 不透明。
#[allow(clippy::too_many_arguments)]
pub(super) fn slider(
    value: f64,
    minimum: f64,
    maximum: f64,
    step: f64,
    label: &str,
    disabled: bool,
    width: f32,
    min_width: f32,
) -> RangeField {
    let mut range = RangeField::new(value, minimum, maximum, step)
        .label(label)
        .show_label(false)
        .show_value(false)
        .disabled(disabled);
    let ratio = range.ratio();
    range.style.painter = Some(
        SliderTrack {
            ratio,
            dim: disabled,
        }
        .into(),
    );
    range.style.background = None;
    range.style.border = None;
    let layout = std::sync::Arc::make_mut(&mut range.style.layout);
    layout.width = Some(LengthSpec::Px(width));
    layout.min_width = Some(LengthSpec::Px(min_width));
    layout.height = Some(LengthSpec::Px(20.0));
    layout.min_height = Some(LengthSpec::Px(20.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(1.0);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    range
}

/// 图片、视频的设置行：图片多一条停留时长，两类都有适应 / 填充。宽窗靠右，窄窗两端对齐并可折行。
pub(super) fn settings_row(props: &BarProps, narrow: bool) -> AnyView {
    let mut children: Vec<AnyView> = Vec::new();
    if props.file_class == "image" {
        let seconds = props.image_seconds.clamp(2, 30);
        let duration = slider(
            f64::from(seconds),
            2.0,
            30.0,
            1.0,
            "图片停留时长",
            false,
            120.0,
            120.0,
        );
        children.push(
            widget(Stack::row(8.0).align(AlignSpec::Center))
                .children((
                    widget(
                        Text::new(format!("停留 {seconds} 秒"))
                            .color(SemanticColorRole::Muted)
                            .font_size(12.0)
                            .line_height(18.6),
                    )
                    .key("player-duration-label"),
                    // Vue 停留时长 range 听 `@input`：拖动中每一格都写入。只听 `RangeInput`。
                    // 松手的 `RangeChanged` 是同一格的提交，再听一次会把这次拖动写两遍。
                    widget(duration)
                        .key("player-duration")
                        .on_cx(|_, event: &RangeInput, cx| {
                            let seconds = event.value.round().clamp(2.0, 30.0) as u32;
                            cx.dispatch_program_all(player_message(
                                PlayerMessage::SetImageDuration(seconds * 1000),
                            ));
                        }),
                ))
                .into_any(),
        );
    }
    children.push(fit_switch(props.cover));
    let justify = if narrow {
        JustifySpec::SpaceBetween
    } else {
        JustifySpec::End
    };
    widget(
        Stack::bar(12.0)
            .justify(justify)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(34.0))
            .wrap(narrow),
    )
    .children(children)
    .key("player-settings")
    .into_any()
}

/// 适应 / 填充两档。外框是 bg 底、border-soft 细边、8px 圆角；当前档铺 accent-soft、字用 accent。
fn fit_switch(cover: bool) -> AnyView {
    widget(
        Stack::row(2.0)
            .align(AlignSpec::Center)
            .padding(2.0)
            .surface(SemanticColorRole::Background)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((
        widget(fit_option("适应", !cover))
            .key("player-fit-contain")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(player_message(PlayerMessage::SetObjectFit {
                    cover: false,
                }));
            }),
        widget(fit_option("填充", cover))
            .key("player-fit-cover")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(player_message(PlayerMessage::SetObjectFit {
                    cover: true,
                }));
            }),
    ))
    .key("player-fit")
    .into_any()
}

/// 一档适配按钮。Vue 组件声明的是 26px 高（全局按钮规则把它顶成 32px），这里按组件设计值画。
fn fit_option(label: &str, active: bool) -> Button {
    let mut button = Button::new(label);
    button.style.background = active.then_some(SemanticColorRole::AccentSoft);
    button.style.foreground = Some(if active {
        SemanticColorRole::Accent
    } else {
        SemanticColorRole::Muted
    });
    button.style.border = None;
    button.style.radius = Some(RadiusTier::Sm);
    button.style.control_height = None;
    button.style.control_padding_x = None;
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.height = Some(LengthSpec::Px(26.0));
    layout.min_height = Some(LengthSpec::Px(26.0));
    layout.padding_left = Some(LengthSpec::Px(8.0));
    layout.padding_right = Some(LengthSpec::Px(8.0));
    layout.padding_top = Some(LengthSpec::Px(4.0));
    layout.padding_bottom = Some(LengthSpec::Px(4.0));
    layout.font_size = Some(12.0);
    layout.font_weight = Some(500);
    let style = button.style.clone();
    Button::new(label).style(style)
}

/// 当前队列浮层。每一项是封面加两行字，当前项铺 accent-soft；不可播放的项变淡、不可点。
/// 封面看条目自己的 `thumbnailPath`，类型字仍是当前播放器的类别。
pub(super) fn queue_popover(props: &BarProps) -> AnyView {
    let media = props.media_label();
    let rows = props
        .queue
        .iter()
        .map(|item| {
            queue_row(
                item,
                media,
                props.current_id.as_deref() == Some(item.id.as_str()),
            )
        })
        .collect::<Vec<_>>();
    let head = widget(
        Stack::bar(10.0)
            .justify(JustifySpec::SpaceBetween)
            .align(AlignSpec::Center),
    )
    .children((
        widget(
            Text::new("当前队列")
                .color(SemanticColorRole::Text)
                .font_size(13.0)
                .font_weight(700)
                .line_height(QUEUE_HEAD),
        )
        .key("player-queue-title"),
        widget(
            Text::new(format!("{} 项", props.queue.len()))
                .color(SemanticColorRole::Muted)
                .font_size(12.0)
                .line_height(18.6),
        )
        .key("player-queue-count"),
    ))
    .into_any();
    let list_limit = QUEUE_MAX_HEIGHT - QUEUE_PADDING * 2.0 - 2.0 - QUEUE_HEAD - 8.0;
    let list = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.width = Some(LengthSpec::Fill);
        layout.max_height = Some(LengthSpec::Px(list_limit));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(1.0);
    }))
    .children((widget(Stack::column(6.0)).children(rows),))
    .key(props.queue_key.clone())
    .into_any();
    widget(
        Stack::column(8.0)
            .padding(QUEUE_PADDING)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderStrong, 1.0)
            .radius(RadiusTier::Xl)
            .with_layout(|layout| {
                layout.position = nana_ui_core::PositionSpec::Absolute;
                // Vue 相对卡片内边距盒：右 14、底边在卡片上沿再往上 10。这里换算到内容盒。
                layout.offset_right = Some(LengthSpec::Px(14.0 - CARD_PADDING_X));
                layout.offset_bottom = Some(LengthSpec::CalcPercentOffset {
                    percent: 100.0,
                    offset_px: 10.0 + CARD_PADDING_TOP,
                });
                layout.width = Some(LengthSpec::Px(360.0));
                layout.max_width = Some(LengthSpec::CalcViewportOffset {
                    axis: nana_ui_core::ViewportAxis::Width,
                    value: 100.0,
                    offset_px: -48.0,
                });
                layout.max_height = Some(LengthSpec::Px(QUEUE_MAX_HEIGHT));
                layout.paint.box_shadows = vec![nana_ui_core::BoxShadowSpec {
                    paint_color: None,
                    offset_x: 0.0,
                    offset_y: 14.0,
                    blur_radius: 34.0,
                    spread_radius: -18.0,
                    color: [0.0, 0.0, 0.0, 0.6],
                    inset: false,
                }];
            }),
    )
    .children((head, list))
    .key("player-queue-popover")
    .into_any()
}

fn queue_row(item: &QueueItem, media: &'static str, active: bool) -> AnyView {
    let ready = item.status == "ready";
    let title = display_title(&item.filename, &item.extension);
    let detail = if ready {
        item.path.clone()
    } else {
        item.status_reason
            .clone()
            .unwrap_or_else(|| item.status.clone())
    };
    let id = item.id.clone();
    let hit = widget(hit_area(&title, !ready, 8.0, 10.0))
        .key(format!("player-queue-{}", super::super::key_part(&item.id)))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::PlayItem {
                item_id: id.clone(),
            }));
        });
    let title_role = if active {
        SemanticColorRole::Accent
    } else {
        SemanticColorRole::Text
    };
    // 不可播放的条目整行按 Vue 的 0.52 淡化，在浮层底色上混出来。
    let dim = (!ready).then_some((SemanticColorRole::Surface, 0.52));
    let meta = widget(
        Stack::column(2.0)
            .width(LengthSpec::Shrink)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0),
    )
    .children((
        widget(dim_text(line(title, 14.0, 700, title_role, 21.7), dim)),
        widget(dim_text(
            line(detail, 11.667, 500, SemanticColorRole::Muted, 18.08),
            dim,
        )),
    ));
    let surface = if active {
        SemanticColorRole::AccentSoft
    } else {
        SemanticColorRole::Background
    };
    let row = Stack::row(10.0)
        .align(AlignSpec::Start)
        .padding(8.0)
        .min_height(LengthSpec::Px(56.0))
        .width(LengthSpec::Fill)
        .radius(RadiusTier::Lg);
    widget(dim_surface(row, surface, dim))
        .children((hit, queue_cover(item, media, dim), meta))
        .into_any()
}

fn player_message(message: PlayerMessage) -> ShellMessage {
    ShellMessage::Player(message)
}

#[cfg(test)]
mod tests {
    use crate::shell::player::QueueItem;
    use crate::shell::view_harness::ShellHarness;

    /// 队列封面照 Vue `queueItem.thumbnailPath`：有可读的本地图时画这张图，
    /// 路径空、只有网址、读失败或不是图片时仍写当前播放器的类型字，不编造像素。
    #[test]
    fn queue_draws_the_real_thumbnail_or_keeps_the_type_word() {
        let dir = std::env::temp_dir().join(format!("momobako-queue-thumb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("临时目录");
        let red = write_png(&dir, "red.png", [200, 20, 20, 255]);
        let blue = write_png(&dir, "blue.png", [20, 20, 200, 255]);
        let note = dir.join("note.txt");
        std::fs::write(&note, b"not an image").expect("写入非图片");
        let red_url = encoded(&red, [200, 20, 20, 255]);
        let blue_url = encoded(&blue, [20, 20, 200, 255]);
        let missing = dir.join("missing.png");

        let mut harness = ShellHarness::mount(scene());
        settle(&mut harness);
        harness.model.player.contributions.push(
            crate::backend::services::repository::PlaylistPlayerContribution {
                player_type_id: "momobako.playlist.audio".into(),
                label: "音频".into(),
                file_class: "audio".into(),
                supported_extensions: vec!["mp3".into()],
                supports_seek: true,
                supports_volume: true,
                supports_preview_navigation: false,
                description: None,
            },
        );
        harness.model.player.queue = vec![
            item("red", Some(&red), "ready", "audio"),
            item("blue", Some(&blue), "ready", "audio"),
            item("plain", None, "ready", "video"),
            item("blank", Some("  "), "ready", "audio"),
            item("empty", Some(""), "ready", "audio"),
            item(
                "https",
                Some("https://example.com/cover.png"),
                "ready",
                "audio",
            ),
            item(
                "http",
                Some("HTTP://example.com/cover.png"),
                "ready",
                "audio",
            ),
            item(
                "missing",
                Some(&missing.to_string_lossy()),
                "ready",
                "audio",
            ),
            item("note", Some(&note.to_string_lossy()), "ready", "audio"),
            item("held", Some(&red), "blocked", "audio"),
        ];
        harness.model.player.current_id = Some("red".into());
        harness.model.player.queue_open = true;
        harness.model.player.session.status = "paused".into();
        harness.sync();
        harness.flush();

        assert!(
            harness.keyed("player-queue-popover").is_some(),
            "队列应打开"
        );
        assert_image(&harness, "red", &red_url, false);
        assert_image(&harness, "blue", &blue_url, false);
        assert_ne!(red_url, blue_url, "两张图必须能分开");
        assert_image(&harness, "held", &red_url, true);
        for id in [
            "plain", "blank", "empty", "https", "http", "missing", "note",
        ] {
            assert_type_word(&harness, id);
        }
        let images = popover_images(&harness);
        assert_eq!(
            images.iter().filter(|url| url.as_str() == red_url).count(),
            2,
            "红色图应出现两次，实际 {images:?}"
        );
        assert_eq!(
            images.iter().filter(|url| url.as_str() == blue_url).count(),
            1,
            "蓝色图应出现一次，实际 {images:?}"
        );
        assert_eq!(images.len(), 3, "队列里不应出现编造的第三张图：{images:?}");
        assert_eq!(
            painted(&harness, "player-cover").as_deref(),
            Some(red_url.as_str()),
            "当前项有可读本地图时，播放条自己的封面画这张图"
        );
        assert!(
            texts_under(&harness, "player-cover").is_empty(),
            "播放条自己的封面有图时不写类型字"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 播放条自己的 58px 封面照 Vue `item.thumbnailPath`：可读本地图按 cover 画上，
    /// 路径空、只有网址、读失败或不是图片时仍写当前播放器的类型字。没有条目时写「媒体」。
    /// 点这张图仍打开预览。队列关着，不把队列封面算进来。
    #[test]
    fn player_cover_draws_the_real_thumbnail_or_keeps_the_type_word() {
        let dir =
            std::env::temp_dir().join(format!("momobako-player-cover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("临时目录");
        let red = write_png(&dir, "red.png", [200, 20, 20, 255]);
        let note = dir.join("note.txt");
        std::fs::write(&note, b"not an image").expect("写入非图片");
        let red_url = encoded(&red, [200, 20, 20, 255]);
        let missing = dir.join("missing.png").to_string_lossy().into_owned();
        let note_path = note.to_string_lossy().into_owned();

        let mut harness = ShellHarness::mount(scene());
        settle(&mut harness);
        harness.model.player.contributions.push(
            crate::backend::services::repository::PlaylistPlayerContribution {
                player_type_id: "momobako.playlist.audio".into(),
                label: "音频".into(),
                file_class: "audio".into(),
                supported_extensions: vec!["mp3".into()],
                supports_seek: true,
                supports_volume: true,
                supports_preview_navigation: false,
                description: None,
            },
        );
        harness.model.player.queue_open = false;
        show_cover(&mut harness, Some(red.as_str()));

        assert!(
            harness.keyed("player-queue-popover").is_none(),
            "队列应关着"
        );
        assert_player_image(&harness, &red_url);
        let id = harness.keyed("player-cover").expect("封面");
        let (x, y) = harness.center(id);
        let messages = harness.click_messages(x, y);
        assert!(
            messages.iter().any(|message| matches!(
                message,
                crate::shell::ShellMessage::Player(
                    crate::shell::player::PlayerMessage::OpenPreview { item_id: None }
                )
            )),
            "点封面应仍打开预览，实际收到 {} 条别的消息",
            messages.len()
        );

        for path in [
            None,
            Some("  "),
            Some(""),
            Some("https://example.com/cover.png"),
            Some("HTTP://example.com/cover.png"),
        ] {
            show_cover(&mut harness, path);
            assert_player_word(&harness, "音频");
        }
        show_cover(&mut harness, Some(missing.as_str()));
        assert_player_word(&harness, "音频");
        show_cover(&mut harness, Some(note_path.as_str()));
        assert_player_word(&harness, "音频");

        harness.model.player.queue.clear();
        harness.model.player.current_id = None;
        harness.sync();
        harness.flush();
        assert_player_word(&harness, "媒体");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 换上当前这一条并重画播放条。队列保持关闭。
    fn show_cover(harness: &mut ShellHarness, thumb: Option<&str>) {
        harness.model.player.queue = vec![item("now", thumb, "ready", "audio")];
        harness.model.player.current_id = Some("now".into());
        harness.model.player.queue_open = false;
        harness.model.player.session.status = "paused".into();
        harness.sync();
        harness.flush();
    }

    fn assert_player_image(harness: &ShellHarness, url: &str) {
        assert_player_box(harness);
        assert_eq!(
            painted(harness, "player-cover").as_deref(),
            Some(url),
            "封面应画这张本地图"
        );
        assert!(
            texts_under(harness, "player-cover").is_empty(),
            "有图时不写类型字"
        );
        assert_eq!(
            opacity(harness, "player-cover"),
            None,
            "可播放条目的封面不另淡"
        );
        let path = assembly(harness, "player-cover");
        assert!(
            !path.contains("player-queue-popover"),
            "封面应在播放条上，实际 {path}"
        );
    }

    fn assert_player_word(harness: &ShellHarness, word: &str) {
        assert_player_box(harness);
        assert!(
            painted(harness, "player-cover").is_none(),
            "没有可读图片时不应出现像素"
        );
        assert_eq!(
            texts_under(harness, "player-cover"),
            vec![word.to_string()],
            "应写类型字 {word}"
        );
    }

    fn assert_player_box(harness: &ShellHarness) {
        let id = harness.keyed("player-cover").expect("没有 player-cover");
        let box_ = harness
            .document()
            .context()
            .world()
            .layout_box(id)
            .expect("player-cover 没有布局");
        assert!(
            (box_.width - 58.0).abs() < 0.5 && (box_.height - 58.0).abs() < 0.5,
            "播放条封面应是 58px，实际 {}x{}",
            box_.width,
            box_.height
        );
    }

    fn assembly(harness: &ShellHarness, key: &str) -> String {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .assembly_path(id)
            .unwrap_or_default()
    }

    fn cover_key(id: &str) -> String {
        format!(
            "player-queue-cover-{}",
            crate::shell::player_view::key_part(id)
        )
    }

    fn assert_image(harness: &ShellHarness, id: &str, url: &str, faded: bool) {
        let key = cover_key(id);
        in_queue(harness, &key);
        assert_eq!(
            painted(harness, &key).as_deref(),
            Some(url),
            "{id} 应画这张图"
        );
        assert!(
            texts_under(harness, &key).is_empty(),
            "{id} 有图时不再写类型字"
        );
        assert_side(harness, &key);
        let fade = opacity(harness, &key);
        if faded {
            assert_eq!(fade, Some(0.52), "{id} 不可播放时应把这张图淡一次");
        } else {
            assert_eq!(fade, None, "{id} 可播放的封面不应再淡");
        }
    }

    fn assert_type_word(harness: &ShellHarness, id: &str) {
        let key = cover_key(id);
        in_queue(harness, &key);
        assert!(
            painted(harness, &key).is_none(),
            "{id} 没有可读图片时不应出现像素"
        );
        assert_eq!(
            texts_under(harness, &key),
            vec!["音频".to_string()],
            "{id} 应仍写类型字"
        );
        assert_side(harness, &key);
    }

    fn assert_side(harness: &ShellHarness, key: &str) {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        let box_ = harness
            .document()
            .context()
            .world()
            .layout_box(id)
            .unwrap_or_else(|| panic!("{key} 没有布局"));
        assert!(
            (box_.width - 40.0).abs() < 0.5 && (box_.height - 40.0).abs() < 0.5,
            "{key} 应是 40px，实际 {}x{}",
            box_.width,
            box_.height
        );
    }

    fn in_queue(harness: &ShellHarness, key: &str) {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        let path = harness
            .document()
            .context()
            .assembly_path(id)
            .unwrap_or_default();
        assert!(
            path.contains("player-queue-popover"),
            "{key} 应画在队列浮层里，实际 {path}"
        );
    }

    /// 这一格已经画上的内容图。没有内容图就是没画像素。
    fn painted(harness: &ShellHarness, key: &str) -> Option<String> {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        let style = harness
            .document()
            .context()
            .world()
            .node_style(id)
            .unwrap_or_else(|| panic!("{key} 没有样式"));
        assert!(style.painter.is_none(), "{key} 不应另画一层像素");
        match &style.layout.paint.content_image {
            Some(nana_ui_core::BackgroundImage::Url { url, fit, .. }) => {
                assert_eq!(
                    *fit,
                    nana_ui_core::BackgroundImageFit::Cover,
                    "{key} 应按 cover 铺满"
                );
                Some(url.clone())
            }
            None => None,
            _ => panic!("{key} 的封面不是解码出的本地图"),
        }
    }

    fn opacity(harness: &ShellHarness, key: &str) -> Option<f32> {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .node_style(id)
            .and_then(|style| style.layout.opacity)
    }

    fn texts_under(harness: &ShellHarness, key: &str) -> Vec<String> {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        let world = harness.document().context().world();
        let mut pending = vec![id];
        let mut texts = Vec::new();
        while let Some(current) = pending.pop() {
            if let Some(text) = world.text(current) {
                let text = text.to_string();
                if !text.is_empty() {
                    texts.push(text);
                }
            }
            let children = world
                .node(current)
                .map(|node| node.children.to_vec())
                .unwrap_or_default();
            pending.extend(children);
        }
        texts
    }

    /// 队列浮层里所有内容图，用来确认没有多出来的像素。
    fn popover_images(harness: &ShellHarness) -> Vec<String> {
        let root = harness.keyed("player-queue-popover").expect("队列浮层");
        let world = harness.document().context().world();
        let mut pending = vec![root];
        let mut images = Vec::new();
        while let Some(current) = pending.pop() {
            if let Some(style) = world.node_style(current) {
                if let Some(nana_ui_core::BackgroundImage::Url { url, .. }) =
                    &style.layout.paint.content_image
                {
                    images.push(url.clone());
                }
            }
            let children = world
                .node(current)
                .map(|node| node.children.to_vec())
                .unwrap_or_default();
            pending.extend(children);
        }
        images
    }

    fn item(id: &str, thumb: Option<&str>, status: &str, class: &str) -> QueueItem {
        QueueItem {
            id: id.into(),
            playlist_id: "list".into(),
            asset_id: "asset".into(),
            path: format!("music/{id}.mp3"),
            filename: format!("{id}.mp3"),
            extension: "mp3".into(),
            status: status.into(),
            status_reason: (status != "ready").then(|| "未就绪".into()),
            transient: true,
            player_type_id: "momobako.playlist.audio".into(),
            player_label: "音频".into(),
            file_class: class.into(),
            thumbnail_path: thumb.map(str::to_string),
            is_virtual: false,
            local_absolute_path: None,
        }
    }

    fn scene() -> crate::shell::ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "files-multi-select")
            .expect("没有 files-multi-select")
            .1
    }

    fn settle(harness: &mut ShellHarness) {
        for _ in 0..6 {
            harness.flush();
            for message in harness.take_messages() {
                harness.apply(message);
            }
        }
        harness.flush();
    }

    /// 写成一张纯色 PNG。颜色要能在解码结果里对上。
    fn write_png(dir: &std::path::Path, name: &str, color: [u8; 4]) -> String {
        let path = dir.join(name);
        image::RgbaImage::from_pixel(8, 8, image::Rgba(color))
            .save(&path)
            .unwrap_or_else(|error| panic!("写入 {name}：{error}"));
        path.to_string_lossy().into_owned()
    }

    /// 用和封面同一套解码、编码，把文件收成 data URL，并核对像素就是写进去的那张。
    fn encoded(path: &str, color: [u8; 4]) -> String {
        let frame = crate::shell::decode_thumbnail_file(path)
            .unwrap_or_else(|error| panic!("测试图解不开：{error}"));
        assert_eq!(&frame.rgba[..4], &color, "封面左上角不是写进文件的颜色");
        crate::shell::inspect::rgba_png_data_url(frame.width, frame.height, &frame.rgba)
            .expect("测试图能编码")
    }
}

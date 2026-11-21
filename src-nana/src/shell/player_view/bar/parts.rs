//! 播放条的零件：图片、视频的设置行，当前队列（`Popover`），以及媒体行、队列行共用的封面和文字。
//!
//! 队列封面和播放条自己的封面都只认条目上的 `thumbnailPath`：读得到本地图片就按 cover 画进方块，否则仍写类型字。

use nana_ui::runtime::view::{AnyView, IntoView, segmented_option, widget};
use nana_ui::runtime::{
    Activate, AlignSpec, JustifySpec, LengthSpec, List, ListItem, Popover, PopoverAlignment,
    PopoverToggled, RadiusTier, RangeField, RangeInput, RovingFocusGroup, ScrollAxes, ScrollView,
    SegmentedControl, SegmentedOptionChosen, SemanticColorRole, Stack, Text,
    TextHorizontalAlignment,
};
use nana_ui::{ControlSize, PopoverPlacement};
use nana_ui_core::{SpacingStep, TextWeight, TypeRole};

use super::super::super::ShellMessage;
use super::super::super::admin::style::{LINE, lined, typeset};
use super::super::super::player::{PlayerMessage, QueueItem};
use super::{BarProps, display_title};
use crate::theme_map::tiers;
use crate::theme_map::tiers::DISABLED_OPACITY;

/// 队列面板宽。
const QUEUE_WIDTH: f32 = 360.0;
/// 队列条目滚动区的上限高度。
const QUEUE_LIST_MAX_HEIGHT: f32 = 260.0;

/// 单行文字：给定字号档、字重档、颜色和行高倍数（按字号档），超长省略。
pub(super) fn line(
    label: impl Into<String>,
    size: TypeRole,
    weight: TextWeight,
    role: SemanticColorRole,
    ratio: f32,
) -> Text {
    let mut text = lined(
        typeset(Text::new(label.into()).color(role), size, weight),
        size,
        ratio,
    )
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

/// 没有缩略图时的类型方块：bg 底、lg 圆角、`Hint` 粗体弱色字居中，行高 1.55 倍。
pub(super) fn type_cover(
    label: &str,
    size: f32,
    key: impl Into<String>,
    dim: Option<(SemanticColorRole, f32)>,
) -> AnyView {
    let mut text = lined(
        Text::new(label)
            .color(SemanticColorRole::Muted)
            .role(TypeRole::Hint)
            .weight(TextWeight::Bold),
        TypeRole::Hint,
        LINE,
    );
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

/// 队列里的 40px 封面。
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
/// 不可播放的队列条目按整行的停用档把这张图淡一次。`ignore_pointer` 时点击穿透到下面的按钮。
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

/// 图片、视频的设置行，放在传输条的第二行：图片多一条停留时长（NanaUI 的 `RangeField`，读数写「N 秒」），
/// 两类都有适应 / 填充（`SegmentedControl`）。宽窗靠右，窄窗两端对齐并可折行。
pub(super) fn settings_row(props: &BarProps, narrow: bool) -> AnyView {
    let mut children: Vec<AnyView> = Vec::new();
    if props.file_class == "image" {
        let seconds = props.image_seconds.clamp(2, 30);
        let mut duration = RangeField::new(f64::from(seconds), 2.0, 30.0, 1.0)
            .label("停留")
            .unit("秒")
            .size(ControlSize::Small);
        let layout = std::sync::Arc::make_mut(&mut duration.style.layout);
        layout.width = Some(LengthSpec::Px(240.0));
        layout.min_width = Some(LengthSpec::Px(160.0));
        layout.flex_shrink = Some(1.0);
        // 拖动中每一格都写入。只听 `RangeInput`。
        // 松手的 `RangeChanged` 是同一格的提交，再听一次会把这次拖动写两遍。
        children.push(
            widget(duration)
                .key("player-duration")
                .on_cx(|_, event: &RangeInput, cx| {
                    let seconds = event.value.round().clamp(2.0, 30.0) as u32;
                    cx.dispatch_program_all(player_message(PlayerMessage::SetImageDuration(
                        seconds * 1000,
                    )));
                })
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
        Stack::bar(0.0)
            .gap_step(SpacingStep::Xl)
            .justify(justify)
            .align(AlignSpec::Center)
            .wrap(narrow),
    )
    .children(children)
    .key("player-settings")
    .into_any()
}

/// 适应 / 填充两档：NanaUI 的 `SegmentedControl`，选中哪档由 ViewModel 的设置决定。
fn fit_switch(cover: bool) -> AnyView {
    let option = |label: &'static str, active: bool, key: &'static str| {
        segmented_option(label).selected(active).key(key).on_cx(
            move |_, _: &SegmentedOptionChosen, cx| {
                cx.dispatch_program_all(player_message(PlayerMessage::SetObjectFit {
                    cover: key == "player-fit-cover",
                }));
            },
        )
    };
    widget(
        SegmentedControl::new()
            .label("画面适配")
            .size(ControlSize::Small),
    )
    .children((
        option("适应", !cover, "player-fit-contain"),
        option("填充", cover, "player-fit-cover"),
    ))
    .key("player-fit")
    .into_any()
}

/// 当前队列：NanaUI 的 `Popover`，触发器是传输按钮里的列表图标，面板在它上方靠右展开。
///
/// 开合写回 ViewModel（`QueueToggled`），播放条按 `queue_open` 重建，所以面板里的行只在开着时建。
/// 每一项是封面加两行字，当前项是选中的行；不可播放的项停用、变淡。
/// 封面看条目自己的 `thumbnailPath`，类型字仍是当前播放器的类别。
pub(super) fn queue_popover(props: &BarProps) -> AnyView {
    let mut popover = Popover::new()
        .trigger_icon(super::icons::LIST_MUSIC, "当前队列")
        .bare_trigger(true)
        .placement(PopoverPlacement::Top)
        .alignment(PopoverAlignment::End)
        .width(QUEUE_WIDTH);
    popover.open = props.queue_open;
    let body: Vec<AnyView> = if props.queue_open {
        vec![queue_body(props)]
    } else {
        Vec::new()
    };
    widget(popover)
        .children(body)
        .key("player-queue")
        .on_cx(|_, toggled: &PopoverToggled, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::QueueToggled(toggled.open)))
        })
        .into_any()
}

/// 队列面板里的内容：「当前队列」和项数，下面是纵向滚动的条目。
fn queue_body(props: &BarProps) -> AnyView {
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
        Stack::bar(0.0)
            .gap_step(SpacingStep::Lg)
            .justify(JustifySpec::SpaceBetween)
            .align(AlignSpec::Center),
    )
    .children((
        widget(lined(
            Text::new("当前队列")
                .color(SemanticColorRole::Text)
                .role(TypeRole::Body)
                .weight(TextWeight::Bold),
            TypeRole::Body,
            LINE,
        ))
        .key("player-queue-title"),
        widget(lined(
            Text::new(format!("{} 项", props.queue.len()))
                .color(SemanticColorRole::Muted)
                .role(TypeRole::Meta),
            TypeRole::Meta,
            LINE,
        ))
        .key("player-queue-count"),
    ))
    .into_any();
    let list = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.width = Some(LengthSpec::Fill);
        layout.max_height = Some(LengthSpec::Px(QUEUE_LIST_MAX_HEIGHT));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(1.0);
    }))
    .children((widget(
        List::new()
            .label("队列条目")
            .style(Stack::column(0.0).gap_step(SpacingStep::Xs).node_style()),
    )
    .roving_focus(RovingFocusGroup::vertical())
    .children(rows),))
    .key(props.queue_key.clone())
    .into_any();
    widget(
        Stack::column(0.0)
            .gap_step(SpacingStep::Md)
            .width(LengthSpec::Fill),
    )
    .children((head, list))
    .key("player-queue-popover")
    .into_any()
}

/// 队列里的一项：NanaUI 的 `ListItem`，行首 40px 封面，内容是文件名和路径（不可播放时写原因）。
/// 当前项是选中的行、标题用 accent；不可播放的项停用，字和封面按主题停用档在面板底色上淡化。
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
    let title_role = if active {
        SemanticColorRole::Accent
    } else {
        SemanticColorRole::Text
    };
    let dim = (!ready).then_some((SemanticColorRole::Surface, DISABLED_OPACITY));
    let meta = widget(
        Stack::column(0.0)
            .gap_step(SpacingStep::Xxs)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0)
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((
        widget(dim_text(
            line(
                title.clone(),
                TypeRole::Section,
                TextWeight::Bold,
                title_role,
                LINE,
            ),
            dim,
        )),
        widget(dim_text(
            line(
                detail,
                TypeRole::Meta,
                TextWeight::Medium,
                SemanticColorRole::Muted,
                LINE,
            ),
            dim,
        )),
    ));
    let mut row = ListItem::new(title)
        .auto_height(true)
        .gap(tiers::space_px(SpacingStep::Lg))
        .selected(active)
        .disabled(!ready);
    let layout = std::sync::Arc::make_mut(&mut row.style.layout);
    tiers::gap(layout, SpacingStep::Lg);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    let id = item.id.clone();
    widget(row)
        .leading(queue_cover(item, media, dim))
        .content(meta)
        .key(format!("player-queue-{}", super::super::key_part(&item.id)))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::PlayItem {
                item_id: id.clone(),
            }));
        })
        .into_any()
}

fn player_message(message: PlayerMessage) -> ShellMessage {
    ShellMessage::Player(message)
}

#[cfg(test)]
mod tests {
    use crate::shell::player::QueueItem;
    use crate::shell::view_harness::ShellHarness;

    /// 队列封面看条目的 `thumbnailPath`：有可读的本地图时画这张图，
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

    /// 播放条自己的 58px 封面看条目的 `thumbnailPath`：可读本地图按 cover 画上，
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

    /// 当前队列是 `Popover`：点触发器发 `QueueToggled(true)`，播放条按开着重建后面板留着、
    /// 不因重建自己关上；点外面由框架收起，发 `QueueToggled(false)`。
    #[test]
    fn the_queue_popover_writes_its_open_state_back() {
        let mut harness = ShellHarness::mount(scene());
        settle(&mut harness);
        show_cover(&mut harness, None);
        let trigger = harness.keyed("player-queue").expect("当前队列");
        let (x, y) = harness.center(trigger);
        let messages = harness.click_messages(x, y);
        let toggled = |messages: &[crate::shell::ShellMessage]| {
            messages
                .iter()
                .filter_map(|message| match message {
                    crate::shell::ShellMessage::Player(
                        crate::shell::player::PlayerMessage::QueueToggled(open),
                    ) => Some(*open),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(toggled(&messages), vec![true], "点触发器打开队列");
        for message in messages {
            harness.apply(message);
        }
        settle(&mut harness);
        assert!(harness.model.player.queue_open, "重建后队列仍开着");
        let row = harness.keyed("player-queue-now").expect("队列里的当前项");
        let bounds = harness
            .document()
            .context()
            .world()
            .layout_box(row)
            .expect("队列行的布局盒");
        assert!(bounds.width > 0.0 && bounds.height > 0.0, "队列行排出来了");

        let messages = harness.click_messages(4.0, 4.0);
        assert_eq!(toggled(&messages), vec![false], "点外面收起队列");
        for message in messages {
            harness.apply(message);
        }
        settle(&mut harness);
        assert!(!harness.model.player.queue_open);
        assert!(harness.keyed("player-queue-popover").is_none(), "队列关上");
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
            assert_eq!(
                fade,
                Some(crate::theme_map::tiers::DISABLED_OPACITY),
                "{id} 不可播放时应按停用档把这张图淡一次"
            );
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

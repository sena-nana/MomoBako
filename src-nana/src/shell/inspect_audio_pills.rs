//! 音频舞台上的胶囊底色。
//!
//! Vue 的唱片中间「音频」和标题下的「音频 / 专辑」不是同一块不透明底：
//! `.media-preview__audio-chip` 是 `color-mix(in srgb, var(--bg-elev) 88%, transparent)`，
//! `.media-preview__audio-meta span` 是 `color-mix(in srgb, var(--bg) 24%, transparent)`。
//! 标题下那一层很透，卡片上的浅光能从胶囊后面透出来。

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{LengthSpec, SemanticColorRole, Stack};
use nana_ui_core::SemanticColorMix;

use super::super::frame::text;

/// 唱片中间那枚「音频」的底：bg-elev 占 88%，其余透明。
const CHIP_FILL: f32 = 0.88;
/// 标题下「音频」和专辑的底：bg 只占 24%，其余透明。
const META_FILL: f32 = 0.24;

/// 一枚胶囊。`chip` 为真时是唱片中间的「音频」，否则是标题下的种类或专辑。
///
/// 底只写混合色，不再铺一层不透明的 bg-elev，否则浅光和唱片纹会被胶囊盖住。
/// 字仍是 12px 半粗弱色。标题下的胶囊不省略：长专辑整枚换行，不收成一行省略号。
/// 唱片中间那枚只有「音频」，超长才省略。键名留给原来的文字节点，外框另用 `{key}-face`。
pub(super) fn pill(label: &str, chip: bool, key: &'static str) -> AnyView {
    let border = if chip {
        SemanticColorRole::Border
    } else {
        SemanticColorRole::BorderSoft
    };
    let fill = if chip {
        SemanticColorMix::alpha(SemanticColorRole::Surface, CHIP_FILL)
    } else {
        SemanticColorMix::alpha(SemanticColorRole::Background, META_FILL)
    };
    let frame = Stack::row(0.0)
        .align(nana_ui::runtime::AlignSpec::Center)
        .min_height(LengthSpec::Px(26.0))
        .padding_xy(8.0, 4.0)
        .outline(border, 1.0)
        .radius_px(999.0)
        .grow(0.0)
        .shrink(0.0);
    // 这一版 NanaUI 的 `Stack` 没有 `surface_mix`，混合底写在节点样式上。
    let style = frame.node_style().surface_mix(fill);
    let mut label_text = text(label, 12.0, 600, SemanticColorRole::Muted, 18.6);
    if chip {
        label_text = label_text.truncating();
    }
    widget(frame.style(style))
        .children((widget(label_text).key(key),))
        .key(format!("{key}-face"))
        .into_any()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CHIP_FILL, META_FILL};
    use crate::shell::ShellViewModel;
    use crate::shell::view_harness::ShellHarness;
    use nana_ui::runtime::SemanticColorRole;
    use nana_ui_core::SemanticColorMix;

    /// 唱片中间的「音频」和标题下的胶囊各用 Vue 的那一层透明底。
    /// 没有专辑时不编第三枚；有封面时中间那枚收掉，标题下的底色不变。
    #[test]
    fn audio_stage_pills_use_the_vue_fills() {
        let mut model = scene("preview-audio");
        let path = model.inspect.target_path.clone().expect("音频预览路径");
        let bare = show(&model);
        assert_fill(
            &bare,
            "inspect-audio-chip-face",
            SemanticColorRole::Surface,
            CHIP_FILL,
            SemanticColorRole::Border,
        );
        assert_fill(
            &bare,
            "inspect-audio-kind-face",
            SemanticColorRole::Background,
            META_FILL,
            SemanticColorRole::BorderSoft,
        );
        assert!(
            bare.keyed("inspect-audio-album-face").is_none(),
            "没有专辑不画专辑胶囊"
        );
        assert!(
            bare.keyed("inspect-audio-chip").is_some(),
            "文字键还在，给原来的舞台测试用"
        );

        set_row(&mut model, &path, json!({"album": " 十一月的萧邦 "}));
        let album = show(&model);
        assert_eq!(node_text(&album, "inspect-audio-album"), "十一月的萧邦");
        assert_fill(
            &album,
            "inspect-audio-album-face",
            SemanticColorRole::Background,
            META_FILL,
            SemanticColorRole::BorderSoft,
        );
        assert_fill(
            &album,
            "inspect-audio-kind-face",
            SemanticColorRole::Background,
            META_FILL,
            SemanticColorRole::BorderSoft,
        );
        assert_fill(
            &album,
            "inspect-audio-chip-face",
            SemanticColorRole::Surface,
            CHIP_FILL,
            SemanticColorRole::Border,
        );

        set_row(
            &mut model,
            &path,
            json!({"album": "   ", "coverArt": "https://example.com/cover.png"}),
        );
        let remote = show(&model);
        assert!(
            remote.keyed("inspect-audio-album-face").is_none(),
            "空白专辑不画胶囊"
        );
        assert!(
            remote.keyed("inspect-audio-chip-face").is_some(),
            "只有网址的封面不落地，中间的音频还在"
        );
        assert_fill(
            &remote,
            "inspect-audio-kind-face",
            SemanticColorRole::Background,
            META_FILL,
            SemanticColorRole::BorderSoft,
        );
    }

    /// 标题下的胶囊在标题宽度里横排。短专辑和「音频」同一行；
    /// 放不下时专辑整枚换到下一行，两行都在这一宽里居中，专辑全文还在，不收成省略号。
    #[test]
    fn audio_meta_pills_wrap_inside_the_caption() {
        let mut model = scene("preview-audio");
        let path = model.inspect.target_path.clone().expect("音频预览路径");
        let short_name = "十一月的萧邦";
        set_row(&mut model, &path, json!({"album": short_name}));
        let short = ShellHarness::mount_at(model.clone(), 1000.0, 400.0);
        let short_kind = box_of(&short, "inspect-audio-kind-face");
        let short_album = box_of(&short, "inspect-audio-album-face");
        let caption = box_of(&short, "inspect-audio-caption");
        assert!(
            (short_album.y - short_kind.y).abs() < 1.5,
            "短专辑应和种类同一行：种类 {}，专辑 {}",
            short_kind.y,
            short_album.y
        );
        assert!(
            short_kind.width + 8.0 + short_album.width <= caption.width + 1.0,
            "短的两枚应放得进标题宽：种类 {}，专辑 {}，标题 {}",
            short_kind.width,
            short_album.width,
            caption.width
        );

        let long_name = "十一月的萧邦与很长很长的专辑名字要换到下一行";
        set_row(&mut model, &path, json!({"album": long_name}));
        let long = ShellHarness::mount_at(model, 1000.0, 400.0);
        let kind = box_of(&long, "inspect-audio-kind-face");
        let album = box_of(&long, "inspect-audio-album-face");
        let row = box_of(&long, "inspect-audio-meta");
        let title = box_of(&long, "inspect-audio-caption");
        assert_eq!(node_text(&long, "inspect-audio-album"), long_name);
        assert!(
            !node_text(&long, "inspect-audio-album").contains('…'),
            "专辑不应收成省略号"
        );
        assert!(
            album.y >= kind.y + kind.height - 1.0,
            "长专辑应折到下一行：种类底 {}，专辑顶 {}",
            kind.y + kind.height,
            album.y
        );
        let row_mid = row.x + row.width / 2.0;
        let album_mid = album.x + album.width / 2.0;
        let kind_mid = kind.x + kind.width / 2.0;
        assert!(
            (kind_mid - row_mid).abs() < 2.0,
            "第一行的种类应在标题宽里居中：{kind_mid}，行 {row_mid}"
        );
        assert!(
            (album_mid - row_mid).abs() < 2.0,
            "折下去的专辑应在标题宽里居中：{album_mid}，行 {row_mid}"
        );
        assert!(
            (row.width - title.width).abs() < 1.0,
            "胶囊行应跟标题一样宽：行 {}，标题 {}",
            row.width,
            title.width
        );
        assert!(
            kind.width + 8.0 + album.width > row.width + 1.0,
            "这枚专辑应比种类旁边的空还宽，否则测不到折行：种类 {}，专辑 {}，行 {}",
            kind.width,
            album.width,
            row.width
        );
    }

    /// 外框底色是这一层混合，不再另铺不透明表面。边框角色跟着 Vue 的 chip / meta。
    fn assert_fill(
        harness: &ShellHarness,
        key: &str,
        role: SemanticColorRole,
        ratio: f32,
        border: SemanticColorRole,
    ) {
        let style = node_style(harness, key);
        assert_eq!(style.background, None, "{key} 不能再铺不透明底");
        assert_eq!(
            style.interaction.base.background_mix,
            Some(SemanticColorMix::alpha(role, ratio)),
            "{key}"
        );
        assert_eq!(style.border, Some(border), "{key}");
        assert_eq!(style.layout.border_width, Some(1.0), "{key}");
    }

    fn scene(name: &str) -> ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(scene, _)| *scene == name)
            .unwrap_or_else(|| panic!("没有场景 {name}"))
            .1
    }

    fn set_row(model: &mut ShellViewModel, path: &str, metadata: serde_json::Value) {
        let row = model
            .files
            .rows
            .iter_mut()
            .find(|row| row.path == path)
            .unwrap_or_else(|| panic!("列表里没有 {path}"));
        let Some(object) = metadata.as_object() else {
            eprintln!("Nana 音频胶囊测试的元数据不是对象：{path}");
            return;
        };
        row.metadata = object
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
    }

    fn show(model: &ShellViewModel) -> ShellHarness {
        ShellHarness::mount(model.clone())
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

    fn box_of(harness: &ShellHarness, key: &str) -> nana_ui::runtime::LayoutBox {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .layout_box(id)
            .unwrap_or_else(|| panic!("{key} 没有布局"))
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

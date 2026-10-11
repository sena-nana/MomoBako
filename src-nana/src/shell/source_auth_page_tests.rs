//! 来源账号区的扫码二维码：会话里有扫码地址时重新编码画出来；只有二维码图片（`qrImage`、`qrimg` 的
//! data URL）、没有扫码地址时照 Vue `<img :src="qrImage">` 直接画这张图。

use nana_ui_core::BackgroundImage;
use serde_json::json;

use crate::shell::admin::{AdminMessage, SourceStep};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellViewModel};

/// 1×1 的 PNG。
const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 节点画出来了：在树里，自己和祖先都没有藏起来，布局盒宽高都大于 0。
fn shown(harness: &ShellHarness, key: &str) -> bool {
    let Some(id) = harness.keyed(key) else {
        return false;
    };
    let world = harness.document().context().world();
    let mut current = Some(id);
    while let Some(node) = current {
        if world.node_style(node).is_some_and(|style| style.layout.hidden) {
            return false;
        }
        current = world.parent_id(node);
    }
    world.layout_box(id).is_some_and(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
}

/// 新的会话只有二维码图片：画这张图（180 见方），重新编码的二维码藏起来；换回有扫码地址的会话又画二维码。
#[test]
fn a_session_with_only_an_image_shows_the_image() {
    let mut harness = ShellHarness::mount(scene("source-auth-methods"));
    assert!(shown(&harness, "admin-source-qr"), "有扫码地址时画重新编码的二维码");

    let image_only = json!({ "unikey": "mock-unikey-0002", "qrimg": PNG });
    harness.apply(ShellMessage::Admin(AdminMessage::SourceStepFinished { step: SourceStep::CreateSession, result: Ok(image_only) }));
    harness.flush();
    let image = harness.keyed("admin-source-qr-image").expect("二维码图片节点");
    let world = harness.document().context().world();
    let style = world.node_style(image).expect("二维码图片的样式").clone();
    assert!(
        matches!(&style.layout.paint.content_image, Some(BackgroundImage::Url { url, .. }) if url == PNG),
        "二维码图片没有画会话里的图：{:?}",
        style.layout.paint.content_image
    );
    let bounds = world.layout_box(image).expect("二维码图片的布局");
    assert_eq!((bounds.width, bounds.height), (180.0, 180.0));
    assert!(!shown(&harness, "admin-source-qr"), "没有扫码地址时不画重新编码的二维码");
    harness.assert_same_as_fresh_mount();

    let with_url = json!({ "unikey": "mock-unikey-0003", "qrurl": "https://music.163.com/login?codekey=mock-unikey-0003", "qrimg": PNG });
    harness.apply(ShellMessage::Admin(AdminMessage::SourceStepFinished { step: SourceStep::CreateSession, result: Ok(with_url) }));
    harness.flush();
    assert!(shown(&harness, "admin-source-qr"), "有扫码地址时画重新编码的二维码");
    assert!(!shown(&harness, "admin-source-qr-image"), "有扫码地址时不另画图片");
    harness.assert_same_as_fresh_mount();
}

//! NanaUI 原生页面的真实 Runtime/WGPU 离屏验收。

use momobako_nana::acceptance_document;
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use nana_ui_devtools::offscreen;

#[test]
fn renders_light_and_dark_shell_at_product_viewports() {
    if !offscreen::pixels_available() {
        return;
    }
    let output = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    for (name, width, height) in [("light-main", 1200, 800), ("dark-min", 960, 600)] {
        let document = acceptance_document().expect("acceptance document");
        let mut session = RuntimeAgentSession::new(document, width, height).expect("agent session");
        if name.starts_with("dark") {
            session
                .set_theme(nana_ui_devtools::agent::protocol::ThemeName::Dark)
                .expect("dark theme");
        }
        let path = output.join(format!("{name}.png"));
        let stats = session.screenshot_png(&path).expect("PNG snapshot");
        assert_eq!(stats.width, width);
        assert_eq!(stats.height, height);
        assert!(
            stats.nonclear_ratio > 0.0,
            "snapshot must contain painted UI"
        );
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|node| node.label.as_deref() == Some("MomoBako"))
        );
    }
}

//! 按场景名出 Nana 截图，供和 Vue 对照图逐张比较。
//!
//! 只渲染 `NANA_SCENES` 点名的场景，比整套离屏验收快得多：
//!
//! ```text
//! NANA_SCENES=live-files,settings NANA_SIZES=1200x800,960x600 NANA_THEMES=light,dark \
//!   cargo test -p momobako-nana --test scene_shots -- --nocapture
//! ```
//!
//! 输出到 `src-nana/target/nana-scene-shots/<场景>-<主题>-<宽>x<高>.png`，旁边的
//! `.nodes.txt` 列出每个语义节点的角色、名称和布局盒，方便量尺寸。没给场景名时不出图。

use std::fs;
use std::path::PathBuf;

use momobako_nana::acceptance_document_at_width;
use momobako_nana::shell::{acceptance_gap_models, ShellPage, ShellViewModel};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession, protocol::ThemeName};
use nana_ui_devtools::offscreen;

const PAGES: [(&str, ShellPage); 15] = [
    ("loading", ShellPage::Loading),
    ("empty-repository", ShellPage::EmptyRepository),
    ("error", ShellPage::Error),
    ("file-list", ShellPage::FileList),
    ("selected-file", ShellPage::SelectedFile),
    ("playlists", ShellPage::Playlists),
    ("plugin-settings", ShellPage::PluginSettings),
    ("task-running", ShellPage::TaskRunning),
    ("playback-running", ShellPage::PlaybackRunning),
    ("task-cancelling", ShellPage::TaskCancelling),
    ("conflict", ShellPage::Conflict),
    ("unsaved-edit", ShellPage::UnsavedEdit),
    ("settings", ShellPage::Settings),
    ("settings-error", ShellPage::SettingsError),
    ("logs", ShellPage::Logs),
];

#[test]
fn renders_requested_scenes() {
    let Ok(requested) = std::env::var("NANA_SCENES") else {
        eprintln!("没有 NANA_SCENES，不出图");
        return;
    };
    assert!(offscreen::pixels_available(), "读不到离屏像素，不能出图");
    let sizes = list("NANA_SIZES", "1200x800");
    let themes = list("NANA_THEMES", "light");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/nana-scene-shots");
    fs::create_dir_all(&dir).expect("截图目录");
    let catalog = catalog();
    for name in requested.split(',').map(str::trim).filter(|name| !name.is_empty()) {
        let Some((_, model)) = catalog.iter().find(|(scene, _)| *scene == name) else {
            let known = catalog.iter().map(|(scene, _)| *scene).collect::<Vec<_>>().join(", ");
            panic!("没有场景 {name}。可选：{known}");
        };
        for size in &sizes {
            let (width, height) = parse_size(size);
            for theme in &themes {
                let stem = format!("{name}-{theme}-{width}x{height}");
                shoot(model.clone(), width, height, theme_of(theme), &dir, &stem);
                println!("{}", dir.join(format!("{stem}.png")).display());
            }
        }
    }
}

fn catalog() -> Vec<(&'static str, ShellViewModel)> {
    let mut scenes = PAGES.iter().map(|(name, page)| (*name, ShellViewModel::for_page(page.clone()))).collect::<Vec<_>>();
    scenes.extend(acceptance_gap_models());
    scenes
}

fn shoot(model: ShellViewModel, width: u32, height: u32, theme: ThemeName, dir: &std::path::Path, stem: &str) {
    let document = acceptance_document_at_width(model, width as f32).expect("文档");
    let mut session = RuntimeAgentSession::new(document, width, height).expect("会话");
    session.set_theme(theme).expect("主题");
    session.flush().expect("布局");
    session.screenshot_png(dir.join(format!("{stem}.png"))).expect("截图");
    let nodes = session
        .accessibility_dump()
        .into_iter()
        .map(|node| {
            let bounds = node.bounds;
            format!(
                "{:>4} {:<12} {:<28} x={:.0} y={:.0} w={:.0} h={:.0}",
                node.id,
                node.role,
                node.label.as_deref().unwrap_or("-"),
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.join(format!("{stem}.nodes.txt")), nodes).expect("节点清单");
    let _ = AgentSession::describe(&session);
}

fn list(key: &str, default: &str) -> Vec<String> {
    std::env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn parse_size(size: &str) -> (u32, u32) {
    let (width, height) = size.split_once('x').unwrap_or_else(|| panic!("尺寸写成 宽x高：{size}"));
    (width.parse().expect("宽"), height.parse().expect("高"))
}

fn theme_of(name: &str) -> ThemeName {
    match name {
        "dark" => ThemeName::Dark,
        "light" => ThemeName::Light,
        other => panic!("主题只能是 light 或 dark：{other}"),
    }
}

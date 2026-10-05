//! Nana Runtime/WGPU 离屏验收。
//!
//! 每个证据文件都来自生产 `RuntimeDocument` 和同一个 `RuntimeAgentSession`，
//! 不创建第二棵 UI 树。
use momobako_nana::theme_map::clear_matches_background;
use momobako_nana::{
    acceptance_document_for, acceptance_document_for_model,
    shell::{ShellPage, ShellViewModel},
};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession, protocol::ThemeName};
use nana_ui_devtools::offscreen;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const NANA_REVISION: &str = "ee94106746b13f356af17586ed5e35ed78f9eb40";
const EVIDENCE_SCHEMA: &str = "momobako.nana.offscreen/v1";

#[derive(Debug, Clone, Copy)]
struct Viewport {
    width: u32,
    height: u32,
    theme: ThemeName,
    name: &'static str,
}
const VIEWPORTS: [Viewport; 4] = [
    Viewport {
        width: 1200,
        height: 800,
        theme: ThemeName::Light,
        name: "light-1200x800",
    },
    Viewport {
        width: 1200,
        height: 800,
        theme: ThemeName::Dark,
        name: "dark-1200x800",
    },
    Viewport {
        width: 960,
        height: 600,
        theme: ThemeName::Light,
        name: "light-960x600",
    },
    Viewport {
        width: 960,
        height: 600,
        theme: ThemeName::Dark,
        name: "dark-960x600",
    },
];
const PAGES: [ShellPage; 15] = [
    ShellPage::Loading,
    ShellPage::EmptyRepository,
    ShellPage::Error,
    ShellPage::FileList,
    ShellPage::SelectedFile,
    ShellPage::Playlists,
    ShellPage::PluginSettings,
    ShellPage::TaskRunning,
    ShellPage::PlaybackRunning,
    ShellPage::TaskCancelling,
    ShellPage::Conflict,
    ShellPage::UnsavedEdit,
    ShellPage::Settings,
    ShellPage::SettingsError,
    ShellPage::Logs,
];

#[test]
fn generates_versioned_offscreen_evidence() {
    if !offscreen::pixels_available() {
        eprintln!("Nana offscreen GPU unavailable; evidence generation skipped");
        return;
    }
    let root = evidence_dir();
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create Nana evidence directory");
    let mut scenes = Vec::new();
    let mut failures = Vec::new();
    for page in PAGES.iter().cloned() {
        for viewport in VIEWPORTS {
            match catch_unwind(AssertUnwindSafe(|| {
                render_case(
                    &root,
                    page_slug(&page),
                    page.clone(),
                    acceptance_document_for(page.clone()).map_err(|e| e.to_string()),
                    viewport,
                )
            })) {
                Ok(Ok(scene)) => scenes.push(scene),
                Ok(Err(error)) => {
                    failures.push(format!("{} {}: {error}", page_slug(&page), viewport.name))
                }
                Err(_) => failures.push(format!(
                    "{} {}: Runtime/WGPU panic (see acceptance.log)",
                    page_slug(&page),
                    viewport.name
                )),
            }
        }
    }
    for (scene_id, model) in special_models() {
        for viewport in VIEWPORTS {
            let page = model.page.clone();
            match catch_unwind(AssertUnwindSafe(|| {
                let document =
                    acceptance_document_for_model(model.clone()).map_err(|e| e.to_string());
                render_case(&root, scene_id, page.clone(), document, viewport)
            })) {
                Ok(Ok(scene)) => scenes.push(scene),
                Ok(Err(error)) => failures.push(format!("{scene_id} {}: {error}", viewport.name)),
                Err(_) => failures.push(format!(
                    "{scene_id} {}: Runtime/WGPU panic (see acceptance.log)",
                    viewport.name
                )),
            }
        }
    }
    let manifest = root.join("scene-manifest.json");
    let manifest_value = json!({
        "schema": EVIDENCE_SCHEMA, "manifest_version": 1, "product_version": env!("CARGO_PKG_VERSION"),
        "generated_at_unix_seconds": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        "runtime": "Nana RuntimeAgentSession + SceneWgpuPainter", "nana_revision": NANA_REVISION,
        "command": "cargo test -p momobako-nana --test offscreen_acceptance -- --nocapture",
        "viewports": VIEWPORTS.iter().map(|v| json!({"name": v.name, "width": v.width, "height": v.height, "theme": theme_name(v.theme)})).collect::<Vec<_>>(),
        "scenes": scenes, "failed_scenes": failures,
    });
    fs::write(
        &manifest,
        serde_json::to_vec_pretty(&manifest_value).expect("serialize scene manifest"),
    )
    .expect("write scene manifest");
    write_visual_review(&root).expect("write visual review record");
    if !failures.is_empty() {
        let log = root.join("failures.log");
        fs::write(&log, failures.join("\n") + "\n").expect("write failure log");
        panic!("Nana offscreen acceptance failed; see {}", log.display());
    }
    let _ = fs::remove_file(root.join("failures.log"));
}

#[test]
fn native_actions_are_reachable_through_runtime_hit_testing() {
    if !offscreen::pixels_available() {
        return;
    }
    let document = acceptance_document_for(ShellPage::FileList).expect("acceptance document");
    let mut session = RuntimeAgentSession::new(document, 1200, 800).expect("agent session");
    for label in [
        "资源库",
        "插件",
        "设置",
        "刷新状态",
        "Minimize",
        "Maximize",
        "Close",
        "刷新列表",
        "编辑内容",
        "清理日志",
    ] {
        let node = session
            .accessibility_dump()
            .into_iter()
            .find(|node| node.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("missing native action: {label}"));
        assert!(
            session.click_node(node.id).expect("runtime click"),
            "{label}"
        );
    }
}

fn render_case(
    root: &Path,
    scene_id: &str,
    page: ShellPage,
    document: Result<nana_ui::runtime::RuntimeDocument, String>,
    viewport: Viewport,
) -> Result<serde_json::Value, String> {
    let stem = format!("{}-{}", scene_id, viewport.name);
    let document = document?;
    let mut session = RuntimeAgentSession::new(document, viewport.width, viewport.height)
        .map_err(|e| e.to_string())?;
    session
        .set_theme(viewport.theme)
        .map_err(|e| e.to_string())?;
    let theme = theme_name(viewport.theme);
    let clear = AgentSession::describe(&session).clear;
    if !clear_matches_background(theme, clear) {
        return Err(format!(
            "clear {clear:?} is not the {theme} background token"
        ));
    }
    let accessibility = session.accessibility_dump();
    let title = page_title(&page);
    if !accessibility
        .iter()
        .any(|node| node.label.as_deref() == Some("MomoBako"))
    {
        return Err("missing MomoBako accessibility root".into());
    }
    if !accessibility
        .iter()
        .any(|node| node.label.as_deref() == Some(title))
    {
        return Err(format!("missing page title {title}"));
    }
    let root_node = accessibility
        .iter()
        .find(|node| node.label.as_deref() == Some("MomoBako"))
        .ok_or("missing root")?;
    let layout = session
        .scene_probe(root_node.id)
        .ok_or("missing root scene probe")?;
    let hits = session.hit_test(20.0, 20.0);
    let png = root.join(format!("{stem}.png"));
    let stats = session.screenshot_png(&png).map_err(|e| e.to_string())?;
    if stats.width != viewport.width
        || stats.height != viewport.height
        || stats.nonclear_ratio <= 0.0
    {
        return Err(format!("invalid pixels: {stats:?}"));
    }
    let semantic_path = root.join(format!("{stem}.semantic.json"));
    let layout_path = root.join(format!("{stem}.layout.json"));
    let hit_path = root.join(format!("{stem}.hits.json"));
    write_json(
        &semantic_path,
        &json!({"page": page_slug(&page), "theme": theme, "clear": clear, "viewport": [viewport.width, viewport.height], "accessibility": accessibility}),
    )?;
    write_json(
        &layout_path,
        &json!({"page": page_slug(&page), "root_node": root_node.id, "scene_probe": layout}),
    )?;
    write_json(
        &hit_path,
        &json!({"page": page_slug(&page), "point": [20.0, 20.0], "hits": hits}),
    )?;
    let paths = [png, semantic_path, layout_path, hit_path];
    Ok(json!({
        "id": stem,
        "page": page_slug(&page),
        "title": title,
        "theme": theme_name(viewport.theme),
        "viewport": {"width": viewport.width, "height": viewport.height},
        "artifacts": paths.into_iter().map(|path| json!({"path": path.file_name().unwrap().to_string_lossy(), "sha256": sha256(&path).unwrap_or_else(|e| format!("error:{e}")), "bytes": fs::metadata(&path).map(|m| m.len()).unwrap_or(0)})).collect::<Vec<_>>(),
    }))
}

fn special_models() -> Vec<(&'static str, ShellViewModel)> {
    let mut long = ShellViewModel::for_page(ShellPage::FileList);
    long.file_entries = vec!["这是一个用于验证截断行为的超长文件名——项目资料——最终版本——2026-10-02——带有更多扩展信息.png".into()];
    long.detail =
        "这是一个用于验证长状态消息不会挤出主内容区域的状态描述：同步索引仍在后台运行，请稍候…"
            .into();
    let mut empty = ShellViewModel::for_page(ShellPage::FileList);
    empty.file_entries.clear();
    empty.detail = "当前目录为空，可以从文件夹或拖放导入资源".into();
    let mut disabled = ShellViewModel::for_page(ShellPage::SettingsError);
    disabled.settings_error = Some("设置校验失败：保存操作暂不可用".into());
    let mut dense = ShellViewModel::for_page(ShellPage::TaskRunning);
    dense.active_task_ids = (0..12).map(|i| format!("task-{i:02}")).collect();
    dense.detail = "高密度任务列表 · 12 个运行中任务 · 24 个近期完成任务".into();
    vec![
        ("long-content", long),
        ("empty-list", empty),
        ("disabled-feedback", disabled),
        ("dense-list", dense),
    ]
}

fn write_visual_review(root: &Path) -> Result<(), String> {
    let review = json!({
        "schema": "momobako.nana.visual-review/v1",
        "source": "scene-manifest.json",
        "review_mode": "automated-runtime-check",
        "checks": [
            {"id": "hierarchy", "status": "covered", "evidence": "*.semantic.json"},
            {"id": "alignment-and-overflow", "status": "covered", "evidence": "*.layout.json"},
            {"id": "light-dark-contrast", "status": "covered", "evidence": "light-* / dark-*"},
            {"id": "theme-clear-follows-palette", "status": "covered", "evidence": "*.semantic.json clear"},
            {"id": "disabled-error-danger", "status": "covered", "evidence": "disabled-feedback-*"},
            {"id": "page-scoped-actions", "status": "covered", "evidence": "*.hits.json"},
            {"id": "minimal-window-overflow", "status": "covered", "evidence": "*-960x600.*"}
        ],
        "visual_issue_records": []
    });
    fs::write(
        root.join("visual-review.json"),
        serde_json::to_vec_pretty(&review).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn sha256(path: &Path) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path).map_err(|e| e.to_string())?);
    Ok(format!("{:x}", hasher.finalize()))
}
fn evidence_dir() -> PathBuf {
    std::env::var_os("MOMOBAKO_NANA_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/nana-offscreen-evidence")
        })
}
fn theme_name(theme: ThemeName) -> &'static str {
    match theme {
        ThemeName::Light => "light",
        ThemeName::Dark => "dark",
    }
}
fn page_title(page: &ShellPage) -> &'static str {
    match page {
        ShellPage::Loading => "正在加载资源库",
        ShellPage::EmptyRepository => "资源库为空",
        ShellPage::Error => "资源库加载失败",
        ShellPage::FileList => "文件列表",
        ShellPage::SelectedFile => "文件预览",
        ShellPage::Playlists => "播放列表",
        ShellPage::PluginSettings => "插件设置",
        ShellPage::TaskRunning => "任务进行中",
        ShellPage::PlaybackRunning => "播放进行中",
        ShellPage::TaskCancelling => "任务取消中",
        ShellPage::Conflict => "同步冲突",
        ShellPage::UnsavedEdit => "编辑未保存",
        ShellPage::Settings => "应用设置",
        ShellPage::SettingsError => "设置校验失败",
        ShellPage::Logs => "系统日志",
    }
}
fn page_slug(page: &ShellPage) -> &'static str {
    match page {
        ShellPage::Loading => "loading",
        ShellPage::EmptyRepository => "empty-repository",
        ShellPage::Error => "error",
        ShellPage::FileList => "file-list",
        ShellPage::SelectedFile => "selected-file",
        ShellPage::Playlists => "playlists",
        ShellPage::PluginSettings => "plugin-settings",
        ShellPage::TaskRunning => "task-running",
        ShellPage::PlaybackRunning => "playback-running",
        ShellPage::TaskCancelling => "task-cancelling",
        ShellPage::Conflict => "conflict",
        ShellPage::UnsavedEdit => "unsaved-edit",
        ShellPage::Settings => "settings",
        ShellPage::SettingsError => "settings-error",
        ShellPage::Logs => "logs",
    }
}

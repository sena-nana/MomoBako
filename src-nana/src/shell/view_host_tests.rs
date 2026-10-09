//! 常驻壳层视图的回归。
//!
//! 一是输入法：标题栏搜索框和对话框输入框在组合输入中时，日志、缩略图、任务、播放消息和动效帧
//! 都不能换掉输入框节点、打断预编辑；组合结束后延后的浮层按最新状态补挂，焦点回到输入框。
//! 二是热路径：动效帧和播放推进只改绑定的字段，内容一次都不重挂。
//! 三是一致性：一串消息增量同步之后，文档和同一 ViewModel 新挂的文档无障碍树相同。

use crate::backend::services::repository::{SystemLogLocation, SystemLogRecord, SystemLogSource, TaskProgressSnapshot};
use crate::shell::host_events::HostMessage;
use crate::shell::player::{PlayerMessage, QueueItem};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{GapMessage, InspectMessage, ShellMessage, ShellViewModel, SidebarMessage, ThumbnailFrame};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 不改输入框所在内容的后台消息：日志广播、缩略图、任务进度、播放音量。
fn background_messages() -> Vec<ShellMessage> {
    vec![
        ShellMessage::Host(HostMessage::LogRecorded(SystemLogRecord {
            id: "log-1".into(),
            timestamp: "2026-10-09T08:00:00Z".into(),
            level: "info".into(),
            category: "repository".into(),
            action: "sync".into(),
            message: "后台同步完成".into(),
            source: SystemLogSource { kind: "host".into(), label: Some("MomoBako".into()), plugin_id: None, repo_id: None },
            location: SystemLogLocation::default(),
            context: serde_json::json!({}),
        })),
        ShellMessage::ThumbnailPixels(vec![ThumbnailFrame {
            path: "cover.png".into(),
            natural_width: 2,
            natural_height: 2,
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        }]),
        ShellMessage::TaskProgressLoaded(vec![TaskProgressSnapshot {
            task_id: "task-1".into(),
            protocol_id: "momobako.sync".into(),
            status: "running".into(),
            phase: Some("scanning".into()),
            label: Some("扫描文件".into()),
            current: Some(4),
            total: Some(10),
            percent: Some(40.0),
            error: None,
            updated_at: "now".into(),
        }]),
        ShellMessage::Player(PlayerMessage::SetVolume(0.5)),
    ]
}

/// 预编辑和节点都还在，焦点也没动。
fn assert_composing(harness: &ShellHarness, label: &str, id: nana_ui::runtime::StableNodeId, preedit: &str, after: &str) {
    assert_eq!(harness.input(label), id, "{after}后输入框 {label} 被换成了新节点");
    assert_eq!(harness.preedit(id).as_deref(), Some(preedit), "{after}后输入框 {label} 的预编辑断了");
    assert_eq!(harness.focused(), Some(id), "{after}后焦点离开了输入框 {label}");
}

#[test]
fn title_bar_search_keeps_its_preedit_through_messages_and_frames() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let search = harness.input("全局搜索");
    harness.focus(search);
    harness.compose("zhong");
    for message in background_messages() {
        harness.apply(message);
        harness.flush();
        assert_composing(&harness, "全局搜索", search, "zhong", "后台消息");
    }
    harness.apply(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderCreate(String::new()))));
    for _ in 0..12 {
        harness.frame();
        assert_composing(&harness, "全局搜索", search, "zhong", "动效帧");
    }
    harness.commit("中");
    for message in harness.take_messages() {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(harness.input("全局搜索"), search);
    assert_eq!(harness.value(search), "中");
    assert_eq!(harness.model.inspect.query, "中");
}

/// 受控输入的回滚竞态：按键的消息还排在后台消息后面没归约时，同步不能把 ViewModel 里较旧的
/// 草稿写回搜索框，否则下一次刷新就把刚打的字冲掉。
#[test]
fn search_draft_survives_a_background_message_ahead_of_its_keystroke() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let search = harness.input("全局搜索");
    harness.focus(search);
    harness.type_text("a");
    let queued = harness.take_messages();
    assert!(!queued.is_empty(), "打字没有发出消息");
    harness.apply(background_messages().remove(0));
    harness.flush();
    assert_eq!(harness.value(search), "a", "后台消息的同步把刚打的字回滚了");
    harness.type_text("b");
    harness.flush();
    assert_eq!(harness.value(search), "ab");
    let mut later = harness.take_messages();
    for message in queued.into_iter().chain(later.drain(..)) {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(harness.value(search), "ab");
    assert_eq!(harness.model.inspect.query, "ab");
}

#[test]
fn dialog_input_defers_its_remount_until_the_composition_ends() {
    let mut harness = ShellHarness::mount(scene("folder-create-dialog"));
    let field = harness.input("文件夹名称");
    harness.focus(field);
    harness.compose("wenjian");
    let deferred = harness.view_stats().deferred;
    for message in background_messages() {
        harness.apply(message);
        harness.flush();
        assert_composing(&harness, "文件夹名称", field, "wenjian", "后台消息");
    }
    for _ in 0..12 {
        harness.frame();
        assert_composing(&harness, "文件夹名称", field, "wenjian", "动效帧");
    }
    assert!(harness.view_stats().deferred > deferred, "组合中的对话框应该延后重挂");

    // 输入法取消：没有新文字，下一帧按最新状态补挂，焦点和空草稿回到新的输入框上。
    harness.cancel_composition();
    harness.frame();
    let remounted = harness.input("文件夹名称");
    assert_ne!(remounted, field, "组合结束后对话框应该补挂");
    assert_eq!(harness.focused(), Some(remounted), "补挂后焦点没有回到输入框");
    harness.assert_same_as_fresh_mount();

    // 再组合一次并提交：文字进草稿，对话框重挂后输入框里是提交的文字。
    harness.compose("wenjian");
    harness.commit("文件");
    for message in harness.take_messages() {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(harness.model.sidebar.folder_dialog.value, "文件");
    let committed = harness.input("文件夹名称");
    assert_eq!(harness.value(committed), "文件");
    assert_eq!(harness.focused(), Some(committed));
    harness.assert_same_as_fresh_mount();
}

/// 对话框打开动效：遮罩透明度逐帧变大，浮层和主区一次都不重挂，浮层根节点不换。
#[test]
fn modal_frames_only_touch_the_bound_layer() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderCreate(String::new()))));
    harness.flush();
    let (overlay, primary) = harness.content_roots();
    let overlay = overlay.expect("对话框在浮层里");
    let remounts = harness.view_stats().remounts;
    // 浮层根下面铺满它的那一层带透明度和变换。
    let layer = harness.document().context().world().node(overlay).and_then(|node| node.children.first().copied()).expect("浮层内层");
    let opacity = |harness: &ShellHarness| {
        harness.document().context().world().node_style(layer).and_then(|style| style.layout.opacity).expect("浮层内层有透明度")
    };
    let mut seen = vec![opacity(&harness)];
    for _ in 0..14 {
        harness.frame();
        seen.push(opacity(&harness));
    }
    assert_eq!(harness.view_stats().remounts, remounts, "动效帧重挂了内容");
    assert_eq!(harness.content_roots(), (Some(overlay), primary), "动效帧换掉了内容根节点");
    assert_eq!(seen.first().copied(), Some(0.0), "打开时遮罩从 0 开始");
    assert_eq!(seen.last().copied(), Some(1.0), "动效走完遮罩不透明");
    assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "遮罩透明度应逐帧变大：{seen:?}");
    assert!(seen.windows(2).filter(|pair| pair[0] < pair[1]).count() >= 5, "透明度没有逐帧变化：{seen:?}");
    harness.assert_same_as_fresh_mount();
}

/// 播放推进：时间和进度逐帧走，播放条的节点不换，内容一次都不重挂；绑定只改变了的字段。
#[test]
fn playback_frames_only_touch_the_bound_bar_fields() {
    let dir = std::env::temp_dir().join(format!("momobako-nana-view-host-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("临时目录");
    let path = dir.join("tone.wav");
    std::fs::write(&path, pcm_wav(8_000, &[128u8; 24_000])).expect("写测试音频");
    let mut model = scene("live-files-plain");
    model.player.repo_id = model.workspace.active_repo_id.clone();
    model.player.queue = vec![wav_item(&path)];
    model.reduce(ShellMessage::Player(PlayerMessage::PlayItem { item_id: "tone".into() }));
    crate::shell::player::fulfill_loads(&mut model);
    assert_eq!(model.player.session.status, "playing", "测试音频没有开始播放");
    let mut harness = ShellHarness::mount(model);
    let time = harness.node("0:00 / 0:03");
    let seek = nana_ui::runtime::Entity::<nana_ui::runtime::RangeField>::from_stable_id(harness.node("播放进度"));
    let position = |harness: &ShellHarness| harness.document().context().read(seek, |range| range.value).expect("拖动条");
    let remounts = harness.view_stats().remounts;
    let patched = ShellHarness::stats().nodes_patched;
    let mut positions = vec![position(&harness)];
    for _ in 0..70 {
        harness.frame();
        positions.push(position(&harness));
    }
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "拖动条位置应逐帧前进：{positions:?}");
    let world = harness.document().context().world();
    assert_eq!(world.text(time), Some("0:01 / 0:03"), "播放一秒后时间没有跟着走");
    assert_eq!(harness.view_stats().remounts, remounts, "播放推进重挂了内容");
    assert!(harness.model.player.session.current_time_ms >= 1_000);
    assert!(ShellHarness::stats().nodes_patched > patched, "播放推进没有经绑定改字段");
    harness.assert_same_as_fresh_mount();
    let _ = std::fs::remove_dir_all(dir);
}

/// 一串消息：搜索、筛选、对话框开合、侧栏收起展开、切到设置再回来。每步之后增量同步的文档
/// 和同一 ViewModel 新挂的一致，动效中间也一致。
#[test]
fn incremental_sync_matches_a_fresh_mount_step_by_step() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let steps = [
        ShellMessage::Inspect(InspectMessage::SetQuery("封面".into())),
        ShellMessage::Inspect(InspectMessage::ToggleFilterBar),
        ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderCreate(String::new()))),
        ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseFolderDialog)),
        ShellMessage::ToggleSidebar,
        ShellMessage::ToggleSidebar,
        ShellMessage::Navigate(crate::shell::ShellPage::Settings),
        ShellMessage::Navigate(crate::shell::ShellPage::FileList),
    ];
    for message in steps {
        harness.apply(message);
        harness.assert_same_as_fresh_mount();
        for _ in 0..3 {
            harness.frame();
        }
        harness.assert_same_as_fresh_mount();
        for _ in 0..20 {
            harness.frame();
        }
        harness.assert_same_as_fresh_mount();
    }
}

/// `mount_shell` 在同一线程上同时开着两份文档时，按标记认出各自的骨架：同步谁只改谁，
/// 文档里始终只有一棵壳层。
#[test]
fn mount_shell_tells_live_documents_apart() {
    use nana_ui::runtime::component_descriptors::APP_SHELL;
    let model = scene("live-files-plain");
    let mut first = crate::acceptance_document_for_model(model.clone()).expect("第一份文档");
    let second = crate::acceptance_document_for_model(model.clone()).expect("第二份文档");
    let mut changed = model.clone();
    changed.reduce(ShellMessage::Inspect(InspectMessage::SetQuery("封面".into())));
    crate::shell::mount_shell(&mut first, &changed).expect("同步第一份");
    // 同步只写信号，搜索框的值在下一次刷新时落到节点上。
    first
        .flush(nana_ui::runtime::LayoutViewport::new(1200.0, 800.0), &mut nana_ui::NanaTextShaper::default())
        .expect("刷新第一份");
    let query = |document: &nana_ui::runtime::RuntimeDocument| {
        let world = document.context().world();
        world
            .project_accessibility(document.document())
            .into_iter()
            .find(|node| node.label.as_deref() == Some("全局搜索"))
            .and_then(|node| world.text_input(node.id).map(|input| input.value.to_string()))
            .expect("搜索框")
    };
    let shells = |document: &nana_ui::runtime::RuntimeDocument| {
        document.context().world().nodes_of_component(document.document(), APP_SHELL.type_id).count()
    };
    assert_eq!(query(&first), "封面");
    assert_eq!(query(&second), "", "同步第一份文档改到了第二份");
    assert_eq!((shells(&first), shells(&second)), (1, 1), "同步时又挂了一棵壳层");
}

/// 8 kHz、8-bit 单声道 PCM。
fn pcm_wav(sample_rate: u32, data: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&sample_rate.to_le_bytes());
    body.extend_from_slice(&sample_rate.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&8u16.to_le_bytes());
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.extend_from_slice(data);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(&body);
    bytes
}

fn wav_item(path: &std::path::Path) -> QueueItem {
    QueueItem {
        id: "tone".into(),
        playlist_id: "pl".into(),
        asset_id: "asset-tone".into(),
        path: path.display().to_string(),
        filename: "tone.wav".into(),
        extension: "wav".into(),
        status: "ready".into(),
        status_reason: None,
        transient: false,
        player_type_id: "momobako.playlist.wav".into(),
        player_label: "WAV".into(),
        file_class: "audio".into(),
        thumbnail_path: None,
    }
}

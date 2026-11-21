//! 挂上完整壳层的文件页测试，和离屏场景是同一棵树。
//!
//! 一是输入框的键路径在打字时不变（壳层重挂后按键路径找回焦点），用户文本拼进键也不会让挂载失败，
//! 滚动区按显示的内容取键（换目录、换选中项回顶，同一内容重挂保持位置）；
//! 二是浮层的指针行为：点在导入菜单、右键菜单外面就收起，点右键菜单的分组只在下面展开子项，回收站的「彻底删除」要点两次，
//! 点对话框遮罩等于取消，点卡片里面不取消；
//! 三是元数据保存冲突：冲突说明紧挨注释、草稿保留，点「采用服务器版本」换成服务器内容；
//! 四是预览页只有预览框架贴在页底的那一条播放条，页底不再多出间距；
//! 五是音视频预览失败时预览框里写出标题和原因，不再只画唱片舞台；
//! 六是目录还有下一页时写「滚动继续加载」，续读中改成「继续读取目录...」；分类视图和智能文件夹不跟目录的下一页。

use nana_ui::runtime::LayoutViewport;
use nana_ui::{ApplicationWindow, HeadlessInput, NanaTextShaper, PointerPhase};

use crate::backend::services::repository::{AssetDetail, AssetSummary};
use crate::shell::inspect::{InspectMessage, PreviewBody};
use crate::shell::status::StatusLine;
use crate::shell::{InspectEffect, ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};

use super::{FileDialog, FilesMessage};

mod menus;
mod previews;

/// 挂一次完整壳层，找出所有文本输入并返回它们的键路径（排好序）。
fn input_paths(model: &ShellViewModel) -> Vec<String> {
    let document = crate::acceptance_document_for_model(model.clone()).expect("挂载壳层");
    let document_id = document.document();
    let context = document.context();
    let mut paths = context
        .world()
        .nodes_of_component(
            document_id,
            nana_ui::runtime::component_descriptors::TEXT_INPUT.type_id,
        )
        .map(|node| {
            context
                .assembly_path(node)
                .unwrap_or_else(|| panic!("输入框 {node:?} 没有键路径"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

fn has_input(paths: &[String], key: &str) -> bool {
    paths
        .iter()
        .any(|path| path.rsplit('/').next() == Some(key))
}

/// 按 1200×800 挂载并排版。首帧布局回报的消息（列表宽度等）在实况里早一帧就送走了，这里先取掉。
fn laid_out(model: &ShellViewModel) -> ApplicationWindow {
    let mut window = ApplicationWindow::new();
    window.document = crate::acceptance_document_for_model(model.clone()).expect("挂载壳层");
    window
        .document
        .flush(
            LayoutViewport::new(1200.0, 800.0),
            &mut NanaTextShaper::default(),
        )
        .expect("排版");
    let _ = window.document.context_mut().take_program_messages();
    window
}

/// 在 (x, y) 按下再抬起，返回这次点击发出的壳层消息。
fn click(window: &mut ApplicationWindow, x: f32, y: f32) -> Vec<ShellMessage> {
    let document_id = window.document.document();
    let mut input = HeadlessInput::bind(window.document.context_mut(), document_id);
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        input
            .pointer(window.document.context_mut(), phase, x, y)
            .expect("指针");
    }
    window
        .document
        .context_mut()
        .take_program_messages()
        .into_iter()
        .map(|message| *message.downcast::<ShellMessage>().expect("壳层消息"))
        .collect()
}

/// 无障碍名为 `label` 的节点中心。
fn labeled_center(window: &ApplicationWindow, label: &str) -> (f32, f32) {
    let document = window.document.document();
    let node = window
        .document
        .context()
        .world()
        .project_accessibility_in_viewport(document)
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("没有 {label}"));
    (
        node.bounds.x + node.bounds.width / 2.0,
        node.bounds.y + node.bounds.height / 2.0,
    )
}

/// 无障碍名为 `label` 的节点的布局盒。
fn labeled_bounds(window: &ApplicationWindow, label: &str) -> nana_ui::runtime::LayoutBox {
    let document = window.document.document();
    window
        .document
        .context()
        .world()
        .project_accessibility_in_viewport(document)
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("没有 {label}"))
        .bounds
}

fn has_label(window: &ApplicationWindow, label: &str) -> bool {
    let document = window.document.document();
    window
        .document
        .context()
        .world()
        .project_accessibility_in_viewport(document)
        .iter()
        .any(|node| node.label.as_deref() == Some(label))
}

/// 右键菜单面板的布局盒。
fn menu_bounds(window: &ApplicationWindow) -> nana_ui::runtime::LayoutBox {
    let document = window.document.document();
    let context = window.document.context();
    let id = context
        .world()
        .document_order(document)
        .into_iter()
        .find(|id| {
            context
                .assembly_path(*id)
                .is_some_and(|path| path.rsplit('/').next() == Some("file-context-menu"))
        })
        .expect("右键菜单");
    context.world().layout_box(id).expect("菜单没有布局")
}

/// 挂一次壳层，返回文件页列表和详情两个滚动区键路径的末段。
fn scroll_keys(model: &ShellViewModel) -> (String, String) {
    let document = crate::acceptance_document_for_model(model.clone()).expect("挂载壳层");
    let document_id = document.document();
    let context = document.context();
    let keys = context
        .world()
        .nodes_of_component(
            document_id,
            nana_ui::runtime::component_descriptors::SCROLL_VIEW.type_id,
        )
        .filter_map(|node| context.assembly_path(node))
        .filter_map(|path| path.rsplit('/').next().map(str::to_string))
        .collect::<Vec<_>>();
    let pick = |prefix: &str| {
        keys.iter()
            .find(|key| key.starts_with(prefix))
            .cloned()
            .unwrap_or_else(|| panic!("没有 {prefix}：{keys:?}"))
    };
    (pick("files-scroll-"), pick("file-detail-scroll-"))
}

/// 输入框里的值等于 `value` 的节点在不在。
fn has_value(window: &ApplicationWindow, value: &str) -> bool {
    let document = window.document.document();
    window
        .document
        .context()
        .world()
        .project_accessibility_in_viewport(document)
        .iter()
        .any(|node| {
            node.value
                .as_ref()
                .is_some_and(|text| text.as_str() == value)
        })
}

/// 键路径最后一段是 `key` 的节点的布局盒。
fn keyed_bounds(window: &ApplicationWindow, key: &str) -> nana_ui::runtime::LayoutBox {
    let document = window.document.document();
    let context = window.document.context();
    context
        .world()
        .document_order(document)
        .into_iter()
        .find(|id| {
            context
                .assembly_path(*id)
                .is_some_and(|path| path.rsplit('/').next() == Some(key))
        })
        .and_then(|id| context.world().layout_box(id))
        .unwrap_or_else(|| panic!("没有 {key} 的布局盒"))
}

/// 文档里有没有键路径最后一段是 `key` 的节点。
fn has_key(window: &ApplicationWindow, key: &str) -> bool {
    let document = window.document.document();
    let context = window.document.context();
    context
        .world()
        .document_order(document)
        .into_iter()
        .any(|id| {
            context
                .assembly_path(id)
                .is_some_and(|path| path.rsplit('/').next() == Some(key))
        })
}

/// 音视频文件的素材详情：只有基本字段。
fn media_detail(path: &str) -> AssetDetail {
    let filename = path.rsplit('/').next().unwrap_or(path).to_string();
    let extension = filename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_string())
        .unwrap_or_default();
    AssetDetail {
        summary: AssetSummary {
            asset_id: path.replace('/', "-"),
            repo_id: "acceptance-repo".into(),
            path: path.into(),
            filename,
            extension,
            size_bytes: 1_820,
            size_label: "1820 B".into(),
            status: "ready".into(),
            modified_at: String::new(),
            last_accessed_at: None,
            version: 1,
            tags: Vec::new(),
            thumbnail_path: None,
            hardlink_group_id: None,
            hardlink_state: None,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        },
        metadata: Vec::new(),
        revisions: Vec::new(),
    }
}

/// 打开 `path` 的预览，宿主读完文件后解码以 `error` 失败，和 `dispatch::inspect` 送回的消息一样。
fn failed_media_preview(path: &str, error: &str) -> ShellViewModel {
    let mut model = scene("live-files-plain");
    model.reduce(ShellMessage::SelectFile {
        path: path.into(),
        asset_id: Some(path.replace('/', "-")),
    });
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(media_detail(path))));
    let (path, generation) = model
        .inspect
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            InspectEffect::LoadMedia {
                path, generation, ..
            } => Some((path, generation)),
            _ => None,
        })
        .expect("预览排下了音视频读取");
    let prep = crate::shell::SourcePrep::default();
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded {
        path,
        generation,
        result: Err(error.into()),
        pcm: None,
        frames: None,
        prep,
    }));
    model
}

/// 把预览会话的播放头拨到 `time_ms`，并换成已经解出的对应画面。没有会话时只记日志，不编造画面。
fn place_playhead(model: &mut ShellViewModel, time_ms: u64) {
    let Some(mut session) = model.inspect.media_session().cloned() else {
        eprintln!("Nana 预览没有播放会话，播放头留在原地");
        return;
    };
    session.current_time_ms = time_ms;
    model.inspect.replace_shared_media(session);
    crate::shell::inspect::sync_preview_frame(model);
}

/// 打开 `path` 的预览，素材详情到了、音视频还没读回来：预览框是准备卡。
fn loading_media_preview(path: &str) -> ShellViewModel {
    let mut model = scene("live-files-plain");
    model.reduce(ShellMessage::SelectFile {
        path: path.into(),
        asset_id: Some(path.replace('/', "-")),
    });
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(media_detail(path))));
    model
}

//! 挂上完整壳层的文件页测试，和离屏场景是同一棵树。
//!
//! 一是输入框的键路径在打字时不变（壳层重挂后按键路径找回焦点），用户文本拼进键也不会让挂载失败，
//! 滚动区按显示的内容取键（换目录、换选中项回顶，同一内容重挂保持位置）；
//! 二是浮层的指针行为：点在导入菜单、右键菜单外面就收起，回收站的「彻底删除」要点两次，
//! 点对话框遮罩等于取消，点卡片里面不取消；
//! 三是元数据保存冲突：冲突说明紧挨注释、草稿保留，点「采用服务器版本」换成服务器内容；
//! 四是预览页只有预览框架贴在页底的那一条播放条，页底不再多出间距；
//! 五是音视频预览失败时预览框里写出标题和原因，不再只画唱片舞台。

use nana_ui::runtime::LayoutViewport;
use nana_ui::{ApplicationWindow, HeadlessInput, NanaTextShaper, PointerPhase};

use crate::backend::services::repository::{AssetDetail, AssetSummary};
use crate::shell::inspect::{InspectMessage, PreviewBody};
use crate::shell::status::StatusLine;
use crate::shell::{InspectEffect, ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};

use super::{FileDialog, FilesMessage};

/// 挂一次完整壳层，找出所有文本输入并返回它们的键路径（排好序）。
fn input_paths(model: &ShellViewModel) -> Vec<String> {
    let document = crate::acceptance_document_for_model(model.clone()).expect("挂载壳层");
    let document_id = document.document();
    let context = document.context();
    let mut paths = context
        .world()
        .nodes_of_component(document_id, nana_ui::runtime::component_descriptors::TEXT_INPUT.type_id)
        .map(|node| context.assembly_path(node).unwrap_or_else(|| panic!("输入框 {node:?} 没有键路径")))
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
    paths.iter().any(|path| path.rsplit('/').next() == Some(key))
}

#[test]
fn metadata_inputs_keep_their_key_paths_while_typing() {
    let mut model = scene("files-selected-metadata");
    model.reduce(ShellMessage::Files(FilesMessage::ToggleTagMenu));
    let before = input_paths(&model);
    for key in ["file-create-name", "inspect-comment", "inspect-link", "inspect-tag-draft"] {
        assert!(has_input(&before, key), "缺少输入框 {key}：{before:?}");
    }
    model.reduce(ShellMessage::Files(FilesMessage::SetCreateName("n".into())));
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment("新的注释".into())));
    model.reduce(ShellMessage::Inspect(InspectMessage::SetLink("https://example.com/a".into())));
    model.reduce(ShellMessage::Files(FilesMessage::SetTagDraft("新".into())));
    assert_eq!(input_paths(&model), before, "打字不能改变输入框的键路径，否则重挂后找不回焦点");
}

#[test]
fn dialog_inputs_keep_their_key_paths_while_typing() {
    let mut model = scene("copy-dialog");
    let before = input_paths(&model);
    assert!(has_input(&before, "file-dialog-input"), "{before:?}");
    model.reduce(ShellMessage::Files(FilesMessage::DraftChanged("assets".into())));
    assert_eq!(input_paths(&model), before);

    let mut model = scene("export-dialog");
    model.reduce(ShellMessage::Files(FilesMessage::SetExportField { field: "encrypt".into(), value: "1".into() }));
    let before = input_paths(&model);
    assert!(has_input(&before, "export-password"), "{before:?}");
    model.reduce(ShellMessage::Files(FilesMessage::SetExportField { field: "password".into(), value: "p".into() }));
    assert_eq!(input_paths(&model), before);
}

#[test]
fn slashes_in_tags_and_repeated_palette_colors_still_mount() {
    let mut model = scene("files-selected-metadata");
    model.reduce(ShellMessage::Inspect(InspectMessage::AddTag("角色/主角".into())));
    model.reduce(ShellMessage::Inspect(InspectMessage::AddTag("a\\b".into())));
    let repeated = vec!["#ffffff".to_string(), "#ffffff".to_string()];
    model.inspect.palette = repeated.clone();
    if let Some(row) = model.files.rows.iter_mut().find(|row| row.path == "cover.png") {
        row.palette = repeated;
        row.tags = vec!["角色/主角".into()];
    }
    model.reduce(ShellMessage::Files(FilesMessage::ToggleTagMenu));
    let paths = input_paths(&model);
    assert!(has_input(&paths, "inspect-tag-draft"), "标签菜单应在：{paths:?}");
}

/// 按 1200×800 挂载并排版。首帧布局回报的消息（列表宽度等）在实况里早一帧就送走了，这里先取掉。
fn laid_out(model: &ShellViewModel) -> ApplicationWindow {
    let mut window = ApplicationWindow::new();
    window.document = crate::acceptance_document_for_model(model.clone()).expect("挂载壳层");
    window.document.flush(LayoutViewport::new(1200.0, 800.0), &mut NanaTextShaper::default()).expect("排版");
    let _ = window.document.context_mut().take_program_messages();
    window
}

/// 在 (x, y) 按下再抬起，返回这次点击发出的壳层消息。
fn click(window: &mut ApplicationWindow, x: f32, y: f32) -> Vec<ShellMessage> {
    let document_id = window.document.document();
    let mut input = HeadlessInput::bind(window.document.context_mut(), document_id);
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        input.pointer(window.document.context_mut(), phase, x, y).expect("指针");
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
        .project_accessibility(document)
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("没有 {label}"));
    (node.bounds.x + node.bounds.width / 2.0, node.bounds.y + node.bounds.height / 2.0)
}

fn has_label(window: &ApplicationWindow, label: &str) -> bool {
    let document = window.document.document();
    window.document.context().world().project_accessibility(document).iter().any(|node| node.label.as_deref() == Some(label))
}

#[test]
fn clicking_outside_the_import_menu_closes_it_anywhere_in_the_window() {
    // 列表空白处、右侧详情、左侧侧栏。
    for (x, y) in [(640.0, 640.0), (1050.0, 420.0), (120.0, 420.0)] {
        let mut model = scene("live-files");
        assert!(model.files.import_open, "场景里导入菜单是开着的");
        let mut window = laid_out(&model);
        let messages = click(&mut window, x, y);
        assert!(
            matches!(messages.as_slice(), [ShellMessage::Files(FilesMessage::ToggleImportMenu)]),
            "点 ({x}, {y}) 应只收起导入菜单：{} 条消息",
            messages.len()
        );
        for message in messages {
            model.reduce(message);
        }
        assert!(!model.files.import_open);
    }
}

#[test]
fn import_menu_items_still_take_their_own_clicks() {
    let mut window = laid_out(&scene("live-files"));
    let (x, y) = labeled_center(&window, "从文件夹导入");
    let messages = click(&mut window, x, y);
    assert!(
        matches!(messages.as_slice(), [ShellMessage::Files(FilesMessage::OpenDialog(FileDialog::Import))]),
        "菜单项在收起层上面：{} 条消息",
        messages.len()
    );
}

#[test]
fn context_menu_closes_outside_and_opens_submenus_on_click() {
    let mut model = scene("live-menu");
    let mut window = laid_out(&model);
    let (x, y) = labeled_center(&window, "缩略图");
    for message in click(&mut window, x, y) {
        model.reduce(message);
    }
    assert_eq!(model.files.menu_branch.as_deref(), Some("thumbnail"));
    let mut window = laid_out(&model);
    assert!(has_label(&window, "刷新缩略图"), "子菜单展开");

    let messages = click(&mut window, 1150.0, 120.0);
    assert!(
        matches!(messages.as_slice(), [ShellMessage::Files(FilesMessage::CloseEntryMenu)]),
        "点在菜单外关闭：{} 条消息",
        messages.len()
    );
    for message in messages {
        model.reduce(message);
    }
    assert!(model.files.entry_menu.is_none() && model.files.menu_branch.is_none());
}

#[test]
fn dialog_scrim_cancels_but_the_card_does_not() {
    let mut model = scene("copy-dialog");
    let mut window = laid_out(&model);
    let (x, y) = labeled_center(&window, "目标目录");
    let inside = click(&mut window, x, y);
    assert!(
        inside.iter().all(|message| !matches!(message, ShellMessage::Files(FilesMessage::CloseDialog))),
        "点卡片里的输入框不取消"
    );
    let messages = click(&mut window, 20.0, 780.0);
    assert!(
        matches!(messages.as_slice(), [ShellMessage::Files(FilesMessage::CloseDialog)]),
        "点遮罩取消：{} 条消息",
        messages.len()
    );
    for message in messages {
        model.reduce(message);
    }
    assert_eq!(model.files.dialog, FileDialog::Closed);
}

#[test]
fn permanent_delete_in_the_trash_menu_needs_a_second_click() {
    let mut model = scene("live-menu");
    model.workspace.panel = WorkspacePanel::Trash;
    let mut window = laid_out(&model);
    let (x, y) = labeled_center(&window, "彻底删除");
    let first = click(&mut window, x, y);
    assert!(
        matches!(first.as_slice(), [ShellMessage::Files(FilesMessage::ArmMenuConfirm(id))] if id == "delete"),
        "第一次只进入待确认：{} 条消息",
        first.len()
    );
    for message in first {
        model.reduce(message);
    }
    assert!(model.files.entry_menu.is_some(), "待确认时菜单不关");

    let mut window = laid_out(&model);
    assert!(has_label(&window, "彻底删除"));
    let (x, y) = labeled_center(&window, "彻底删除");
    let second = click(&mut window, x, y);
    assert!(
        matches!(
            second.as_slice(),
            [ShellMessage::Files(FilesMessage::CloseEntryMenu), ShellMessage::Files(FilesMessage::DeleteSelected)]
        ),
        "第二次关菜单并删除：{} 条消息",
        second.len()
    );
}

/// 挂一次壳层，返回文件页列表和详情两个滚动区键路径的末段。
fn scroll_keys(model: &ShellViewModel) -> (String, String) {
    let document = crate::acceptance_document_for_model(model.clone()).expect("挂载壳层");
    let document_id = document.document();
    let context = document.context();
    let keys = context
        .world()
        .nodes_of_component(document_id, nana_ui::runtime::component_descriptors::SCROLL_VIEW.type_id)
        .filter_map(|node| context.assembly_path(node))
        .filter_map(|path| path.rsplit('/').next().map(str::to_string))
        .collect::<Vec<_>>();
    let pick = |prefix: &str| keys.iter().find(|key| key.starts_with(prefix)).cloned().unwrap_or_else(|| panic!("没有 {prefix}：{keys:?}"));
    (pick("files-scroll-"), pick("file-detail-scroll-"))
}

#[test]
fn scroll_areas_are_keyed_by_what_they_show() {
    let mut model = scene("live-files-selected");
    let (list, detail) = scroll_keys(&model);
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment("草稿".into())));
    assert_eq!(scroll_keys(&model), (list.clone(), detail.clone()), "同一目录、同一选中项重挂保持位置");

    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow("cover.png".into())));
    let (same_list, other_detail) = scroll_keys(&model);
    assert_eq!(same_list, list, "换选中项时列表不动");
    assert_ne!(other_detail, detail, "换选中项时详情回顶");

    model.files.current_path = "assets".into();
    assert_ne!(scroll_keys(&model).0, list, "进子目录时列表回顶");
    model.files.current_path = String::new();
    model.workspace.panel = WorkspacePanel::Trash;
    assert_ne!(scroll_keys(&model).0, list, "进回收站时列表回顶");
}

/// 预览页只有框架贴在页底的那一条播放条，预览面板一直铺到主区内容的底边，下面不再空出一格间距。
#[test]
fn preview_page_keeps_its_own_player_bar_without_a_trailing_slot() {
    let window = laid_out(&scene("preview-audio"));
    let document = window.document.document();
    let context = window.document.context();
    let world = context.world();
    let bars = world
        .project_accessibility(document)
        .into_iter()
        .filter(|node| node.label.as_deref() == Some("播放进度"))
        .collect::<Vec<_>>();
    assert_eq!(bars.len(), 1, "预览页只有一条播放条");
    let mut ancestors = Vec::new();
    let mut cursor = world.parent_id(bars[0].id);
    while let Some(id) = cursor {
        ancestors.push((context.assembly_path(id).unwrap_or_default(), world.layout_box(id)));
        cursor = world.parent_id(id);
    }
    let boxed = |key: &str| {
        ancestors
            .iter()
            .find(|(path, _)| path.rsplit('/').next() == Some(key))
            .and_then(|(_, bounds)| *bounds)
            .unwrap_or_else(|| panic!("播放条不在 {key} 里"))
    };
    let page = boxed("inspect-preview-page");
    let body = boxed("workspace-page-body");
    assert!(
        (page.y + page.height - (body.y + body.height)).abs() < 0.5,
        "预览面板底边 {} 应贴着主区内容底边 {}",
        page.y + page.height,
        body.y + body.height
    );
}

/// 输入框里的值等于 `value` 的节点在不在。
fn has_value(window: &ApplicationWindow, value: &str) -> bool {
    let document = window.document.document();
    window
        .document
        .context()
        .world()
        .project_accessibility(document)
        .iter()
        .any(|node| node.value.as_ref().is_some_and(|text| text.as_str() == value))
}

/// 15 页的「冲突」：冲突说明排在注释前面、落在 800 高的窗口里，本地草稿还在注释框；
/// 点「采用服务器版本」换成服务器上的注释，冲突说明收起。
#[test]
fn conflict_notice_sits_above_the_fields_and_adopts_the_server_version() {
    let mut model = ShellViewModel::for_page(ShellPage::Conflict);
    let mut window = laid_out(&model);
    let (_, notice_y) = labeled_center(&window, "版本冲突，未写入");
    let (_, comment_y) = labeled_center(&window, "注释");
    assert!(notice_y < comment_y && notice_y < 800.0, "冲突说明应在注释前面、在窗口里：{notice_y} / {comment_y}");
    assert!(has_value(&window, "封面改用暖色版本。"), "冲突时本地草稿留在注释框");
    let (x, y) = labeled_center(&window, "采用服务器版本");
    for message in click(&mut window, x, y) {
        model.reduce(message);
    }
    let window = laid_out(&model);
    assert!(!has_label(&window, "版本冲突，未写入"), "采用后冲突说明收起");
    assert!(has_value(&window, "封面定稿，沿用冷色版本。"), "注释框换成服务器上的注释");
}

/// 文档里有没有键路径最后一段是 `key` 的节点。
fn has_key(window: &ApplicationWindow, key: &str) -> bool {
    let document = window.document.document();
    let context = window.document.context();
    context.world().document_order(document).into_iter().any(|id| context.assembly_path(id).is_some_and(|path| path.rsplit('/').next() == Some(key)))
}

/// 音视频文件的素材详情：和 Vue 夹具 `file()` 一样只有基本字段。
fn media_detail(path: &str) -> AssetDetail {
    let filename = path.rsplit('/').next().unwrap_or(path).to_string();
    let extension = filename.rsplit_once('.').map(|(_, extension)| extension.to_string()).unwrap_or_default();
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

/// 打开 `path` 的预览，宿主读完文件后解码以 `error` 失败，和 `inspect_dispatch` 送回的消息一样。
fn failed_media_preview(path: &str, error: &str) -> ShellViewModel {
    let mut model = scene("live-files-plain");
    model.reduce(ShellMessage::SelectFile { path: path.into(), asset_id: Some(path.replace('/', "-")) });
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(media_detail(path))));
    let (path, generation) = model
        .inspect
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            InspectEffect::LoadMedia { path, generation, .. } => Some((path, generation)),
            _ => None,
        })
        .expect("预览排下了音视频读取");
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded { path, generation, result: Err(error.into()), pcm: None, frames: None }));
    model
}

/// 音视频预览失败就近写在预览框里：照 Vue 预览插件的失败浮层替换唱片舞台，红色标题下一行原因。
/// Nana 自己的「没有原生解码器」和某格式「解码失败」都这样；视频的标题是「无法预览该媒体」。
/// 这类失败不进侧栏的全局状态区。
#[test]
fn media_preview_failures_show_their_reason_in_place() {
    for (path, error, title) in [
        ("music/track-01.mp3", "没有原生解码器", "无法预览该音频"),
        ("music/track-02.flac", "无法识别压缩音频：end of stream", "无法预览该音频"),
        ("movies/clip.mp4", "MP4 解码失败：没有解出画面", "无法预览该媒体"),
        ("movies/clip.webm", "没有原生解码器", "无法预览该媒体"),
    ] {
        let model = failed_media_preview(path, error);
        let window = laid_out(&model);
        assert!(has_label(&window, title), "{path} 失败时没有标题 {title}");
        assert!(has_label(&window, error), "{path} 失败时没有写出原因 {error}");
        assert!(has_key(&window, "inspect-failed"), "{path} 失败时没有失败浮层");
        assert!(!has_key(&window, "inspect-audio-stage"), "{path} 失败时不该还画唱片舞台");
        assert_eq!(StatusLine::project(&model), StatusLine::Hidden, "{path} 的预览失败不进全局状态区");
    }
}

/// 播放条装不上预览中的同一个文件、把失败会话写回预览页：唱片舞台换成失败浮层；会话没带原因时写 Vue
/// 运行时的「音频无法播放」。
#[test]
fn player_failures_written_back_replace_the_audio_stage() {
    let mut model = scene("preview-audio");
    assert!(has_key(&laid_out(&model), "inspect-audio-stage"), "能播的音频预览是唱片舞台");
    let PreviewBody::Media(session) = &model.inspect.body else { panic!("预览不是音视频") };
    let mut failed = session.clone();
    failed.status = "failed".into();
    failed.error = None;
    model.inspect.replace_shared_media(failed);
    let window = laid_out(&model);
    assert!(has_label(&window, "无法预览该音频"), "写回失败后没有失败标题");
    assert!(has_label(&window, "音频无法播放"), "没有原因时没有写 Vue 的默认原因");
    assert!(!has_key(&window, "inspect-audio-stage"), "写回失败后不该还画唱片舞台");
}

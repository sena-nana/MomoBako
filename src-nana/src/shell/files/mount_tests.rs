//! 挂上完整壳层的文件页测试，和离屏场景是同一棵树。
//!
//! 一是输入框的键路径在打字时不变（壳层重挂后按键路径找回焦点），用户文本拼进键也不会让挂载失败，
//! 滚动区按显示的内容取键（换目录、换选中项回顶，同一内容重挂保持位置）；
//! 二是浮层的指针行为：点在导入菜单、右键菜单外面就收起，点右键菜单的分组只展开子菜单，回收站的「彻底删除」要点两次，
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

#[test]
fn metadata_inputs_keep_their_key_paths_while_typing() {
    let mut model = scene("files-selected-metadata");
    model.reduce(ShellMessage::Files(FilesMessage::ToggleTagMenu));
    let before = input_paths(&model);
    for key in [
        "file-create-name",
        "inspect-comment",
        "inspect-link",
        "inspect-tag-draft",
    ] {
        assert!(has_input(&before, key), "缺少输入框 {key}：{before:?}");
    }
    model.reduce(ShellMessage::Files(FilesMessage::SetCreateName("n".into())));
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "新的注释".into(),
    )));
    model.reduce(ShellMessage::Inspect(InspectMessage::SetLink(
        "https://example.com/a".into(),
    )));
    model.reduce(ShellMessage::Files(FilesMessage::SetTagDraft("新".into())));
    assert_eq!(
        input_paths(&model),
        before,
        "打字不能改变输入框的键路径，否则重挂后找不回焦点"
    );
}

#[test]
fn dialog_inputs_keep_their_key_paths_while_typing() {
    let mut model = scene("copy-dialog");
    let before = input_paths(&model);
    assert!(has_input(&before, "file-dialog-input"), "{before:?}");
    model.reduce(ShellMessage::Files(FilesMessage::DraftChanged(
        "assets".into(),
    )));
    assert_eq!(input_paths(&model), before);

    let mut model = scene("export-dialog");
    model.reduce(ShellMessage::Files(FilesMessage::SetExportField {
        field: "encrypt".into(),
        value: "1".into(),
    }));
    let before = input_paths(&model);
    assert!(has_input(&before, "export-password"), "{before:?}");
    model.reduce(ShellMessage::Files(FilesMessage::SetExportField {
        field: "password".into(),
        value: "p".into(),
    }));
    assert_eq!(input_paths(&model), before);
}

#[test]
fn slashes_in_tags_and_repeated_palette_colors_still_mount() {
    let mut model = scene("files-selected-metadata");
    model.reduce(ShellMessage::Inspect(InspectMessage::AddTag(
        "角色/主角".into(),
    )));
    model.reduce(ShellMessage::Inspect(InspectMessage::AddTag("a\\b".into())));
    let repeated = vec!["#ffffff".to_string(), "#ffffff".to_string()];
    model.inspect.palette = repeated.clone();
    if let Some(row) = model
        .files
        .rows
        .iter_mut()
        .find(|row| row.path == "cover.png")
    {
        row.palette = repeated;
        row.tags = vec!["角色/主角".into()];
    }
    model.reduce(ShellMessage::Files(FilesMessage::ToggleTagMenu));
    let paths = input_paths(&model);
    assert!(
        has_input(&paths, "inspect-tag-draft"),
        "标签菜单应在：{paths:?}"
    );
}

/// 标签菜单铺在元数据的标签区里，不按坐标夹进写死的 1280×800：960×600 的窗口里打开，菜单整块落在
/// 右侧详情卡片里，也在窗口里面；窄到详情卡片叠到列表下面（800 宽）时同样落在卡片里。
#[test]
fn tag_menu_stays_inside_the_detail_card_in_other_window_sizes() {
    for (width, height) in [(960.0, 600.0), (800.0, 600.0)] {
        let mut model = scene("files-selected-metadata");
        model.reduce(ShellMessage::Files(FilesMessage::ToggleTagMenu));
        assert!(
            model.inspect.tag_menu_open(),
            "{width}×{height} 打开标签菜单"
        );
        let harness = crate::shell::view_harness::ShellHarness::mount_at(model, width, height);
        let layout = |key: &str| {
            let id = harness
                .keyed(key)
                .unwrap_or_else(|| panic!("{width}×{height} 缺少 {key}"));
            harness
                .document()
                .context()
                .world()
                .layout_box(id)
                .unwrap_or_else(|| panic!("{key} 没有布局盒"))
        };
        let panel = layout("inspect-tag-menu-panel");
        let card = layout("file-detail");
        assert!(
            panel.width > 0.0 && panel.height > 0.0,
            "{width}×{height} 菜单要排出来：{panel:?}"
        );
        assert!(
            panel.x >= card.x - 0.5 && panel.x + panel.width <= card.x + card.width + 0.5,
            "{width}×{height} 菜单横向落在详情卡片里：{panel:?} / {card:?}"
        );
        assert!(
            panel.x >= 0.0 && panel.x + panel.width <= width + 0.5,
            "{width}×{height} 菜单不出窗口：{panel:?}"
        );
    }
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
        .project_accessibility(document)
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("没有 {label}"));
    (
        node.bounds.x + node.bounds.width / 2.0,
        node.bounds.y + node.bounds.height / 2.0,
    )
}

fn has_label(window: &ApplicationWindow, label: &str) -> bool {
    let document = window.document.document();
    window
        .document
        .context()
        .world()
        .project_accessibility(document)
        .iter()
        .any(|node| node.label.as_deref() == Some(label))
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
            matches!(
                messages.as_slice(),
                [ShellMessage::Files(FilesMessage::ToggleImportMenu)]
            ),
            "点 ({x}, {y}) 应只收起导入菜单：{} 条消息",
            messages.len()
        );
        for message in messages {
            model.reduce(message);
        }
        assert!(!model.files.import_open);
    }
}

/// 标签候选铺在元数据卡里。点卡片外面关掉，不新增标签。
#[test]
fn tag_menu_closes_when_clicking_outside() {
    let mut model = scene("files-selected-metadata");
    model.reduce(ShellMessage::Files(FilesMessage::ToggleTagMenu));
    assert!(model.inspect.tag_menu_open());
    let tags_before = model.inspect.draft_tags().to_vec();
    let mut window = laid_out(&model);
    let messages = click(&mut window, 120.0, 420.0);
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, ShellMessage::Inspect(InspectMessage::CloseTagMenu))),
        "点外面应关掉标签菜单，实际 {} 条消息",
        messages.len()
    );
    for message in messages {
        model.reduce(message);
    }
    assert!(!model.inspect.tag_menu_open());
    assert_eq!(
        model.inspect.draft_tags(),
        tags_before.as_slice(),
        "点外面不新增标签"
    );
}

#[test]
fn import_menu_items_still_take_their_own_clicks() {
    let mut window = laid_out(&scene("live-files"));
    let (x, y) = labeled_center(&window, "从文件夹导入");
    let messages = click(&mut window, x, y);
    assert!(
        matches!(
            messages.as_slice(),
            [ShellMessage::Files(FilesMessage::OpenDialog(
                FileDialog::Import
            ))]
        ),
        "菜单项在收起层上面：{} 条消息",
        messages.len()
    );
}

#[test]
fn context_menu_closes_outside_and_a_clicked_group_stays_open() {
    let mut model = scene("live-menu");
    let mut window = laid_out(&model);
    let (x, y) = labeled_center(&window, "缩略图");
    let messages = click(&mut window, x, y);
    assert!(
        messages.iter().all(|message| matches!(
            message,
            ShellMessage::Files(FilesMessage::MenuBranchChanged { inside: true, .. })
        )),
        "点分组只展开它的子菜单：{} 条消息",
        messages.len()
    );
    for message in messages {
        model.reduce(message);
    }
    assert_eq!(model.files.menu_branch.open(), Some("thumbnail"));
    let mut window = laid_out(&model);
    assert!(has_label(&window, "刷新缩略图"), "子菜单展开");

    let messages = click(&mut window, 1150.0, 120.0);
    assert!(
        matches!(
            messages.as_slice(),
            [ShellMessage::Files(FilesMessage::CloseEntryMenu)]
        ),
        "点在菜单外关闭：{} 条消息",
        messages.len()
    );
    for message in messages {
        model.reduce(message);
    }
    assert!(model.files.entry_menu.is_none() && model.files.menu_branch.open().is_none());
}

#[test]
fn dialog_scrim_cancels_but_the_card_does_not() {
    let mut model = scene("copy-dialog");
    let mut window = laid_out(&model);
    let (x, y) = labeled_center(&window, "目标目录");
    let inside = click(&mut window, x, y);
    assert!(
        inside
            .iter()
            .all(|message| !matches!(message, ShellMessage::Files(FilesMessage::CloseDialog))),
        "点卡片里的输入框不取消"
    );
    let messages = click(&mut window, 20.0, 780.0);
    assert!(
        matches!(
            messages.as_slice(),
            [ShellMessage::Files(FilesMessage::CloseDialog)]
        ),
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
    assert!(
        has_label(&window, "确认彻底删除？再点一次"),
        "第一次以后这一行换成确认文案"
    );
    let (x, y) = labeled_center(&window, "确认彻底删除？再点一次");
    let second = click(&mut window, x, y);
    assert!(
        matches!(
            second.as_slice(),
            [
                ShellMessage::Files(FilesMessage::CloseEntryMenu),
                ShellMessage::Files(FilesMessage::DeleteSelected)
            ]
        ),
        "第二次关菜单并删除：{} 条消息",
        second.len()
    );
}

/// 回收站「彻底删除」第一次换成确认句：菜单按这句撑开，开在窗口右边时往左让，右边仍留 4px。
/// 再点确认句才彻底删除。对应 Vue `displayLabel` 和 `clampAnchoredMenuPosition`。
#[test]
fn trash_delete_confirm_widens_the_menu_to_the_vue_label() {
    let mut model = scene("live-menu");
    model.workspace.panel = WorkspacePanel::Trash;
    model.files.entry_menu.as_mut().expect("场景里有右键菜单").x = 1100.0;
    let mut window = laid_out(&model);
    let before = menu_bounds(&window);
    assert!(has_label(&window, "彻底删除"));
    assert!(
        before.x + before.width <= 1200.0 - 4.0 + 1.0,
        "菜单一开始就夹在窗口内：{before:?}"
    );

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

    let mut window = laid_out(&model);
    assert!(
        has_label(&window, "确认彻底删除？再点一次"),
        "这一行换成确认句"
    );
    assert!(!has_label(&window, "彻底删除"), "确认句换掉短标签");
    let after = menu_bounds(&window);
    assert!(
        after.width > before.width + 8.0,
        "确认句比「彻底删除」长，菜单要撑开：{before:?} -> {after:?}"
    );
    assert!(
        after.x + 1.0 < before.x,
        "变宽以后靠右的菜单要往左让：{before:?} -> {after:?}"
    );
    assert!(
        after.x >= 4.0 - 1.0 && after.x + after.width <= 1200.0 - 4.0 + 1.0,
        "右边仍留 4px：{after:?}"
    );

    let (x, y) = labeled_center(&window, "确认彻底删除？再点一次");
    let second = click(&mut window, x, y);
    assert!(
        matches!(
            second.as_slice(),
            [
                ShellMessage::Files(FilesMessage::CloseEntryMenu),
                ShellMessage::Files(FilesMessage::DeleteSelected)
            ]
        ),
        "第二次点确认句才删除：{} 条消息",
        second.len()
    );
    for message in second {
        model.reduce(message);
    }
    assert!(model.files.entry_menu.is_none(), "删的时候菜单关掉");
    assert!(
        model.files.take_effects().iter().any(|effect| matches!(effect, super::FilesEffect::Delete { mode: Some(mode), .. } if mode == "permanentDelete")),
        "回收站删除是彻底删除"
    );
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

#[test]
fn scroll_areas_are_keyed_by_what_they_show() {
    let mut model = scene("live-files-selected");
    let (list, detail) = scroll_keys(&model);
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "草稿".into(),
    )));
    assert_eq!(
        scroll_keys(&model),
        (list.clone(), detail.clone()),
        "同一目录、同一选中项重挂保持位置"
    );

    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(
        "cover.png".into(),
    )));
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
        ancestors.push((
            context.assembly_path(id).unwrap_or_default(),
            world.layout_box(id),
        ));
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
        .any(|node| {
            node.value
                .as_ref()
                .is_some_and(|text| text.as_str() == value)
        })
}

/// 15 页的「冲突」：冲突说明排在注释前面、落在 800 高的窗口里，本地草稿还在注释框；
/// 点「采用服务器版本」换成服务器上的注释，冲突说明收起。详情卡片顶上的「注释」导语写的是服务器上现在的注释
/// （冲突应答带回的素材已经换进文件行），这里比的是元数据编辑里的注释框。
#[test]
fn conflict_notice_sits_above_the_fields_and_adopts_the_server_version() {
    let mut model = ShellViewModel::for_page(ShellPage::Conflict);
    let mut window = laid_out(&model);
    let (_, notice_y) = labeled_center(&window, "版本冲突，未写入");
    let comment_y = keyed_bounds(&window, "inspect-comment-field").y;
    assert!(
        notice_y < comment_y && notice_y < 800.0,
        "冲突说明应在注释前面、在窗口里：{notice_y} / {comment_y}"
    );
    assert!(
        has_value(&window, "封面改用暖色版本。"),
        "冲突时本地草稿留在注释框"
    );
    let (x, y) = labeled_center(&window, "采用服务器版本");
    for message in click(&mut window, x, y) {
        model.reduce(message);
    }
    let window = laid_out(&model);
    assert!(
        !has_label(&window, "版本冲突，未写入"),
        "采用后冲突说明收起"
    );
    assert!(
        has_value(&window, "封面定稿，沿用冷色版本。"),
        "注释框换成服务器上的注释"
    );
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

/// 音视频文件的素材详情：和 Vue 夹具 `file()` 一样只有基本字段。
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

/// 打开 `path` 的预览，宿主读完文件后解码以 `error` 失败，和 `inspect_dispatch` 送回的消息一样。
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

/// 视频按文件类型画，不按有没有画面分出唱片：离屏没有宿主纹理时画解码出的那一帧，不画图标、不画唱片舞台；
/// 清掉像素后才画视频图标。有宿主纹理时仍画纹理槽。准备卡视频写「准备媒体」（Vue `media-preview`），
/// 音频写「准备音频」（`audioPreview.js`）。
#[test]
fn video_previews_never_fall_into_the_audio_stage() {
    let mut model = scene("preview-no-player");
    let window = laid_out(&model);
    assert!(
        !has_key(&window, "inspect-audio-stage"),
        "视频不该画唱片舞台"
    );
    assert!(!has_label(&window, "通用音频") && !has_label(&window, "暂无歌词"));
    assert!(
        has_key(&window, "inspect-video-pixels"),
        "离屏没有宿主纹理时画解码出的这一帧"
    );
    assert!(
        !has_key(&window, "inspect-fallback-icon"),
        "有解码画面时不画视频图标"
    );
    assert!(
        !has_key(&window, "inspect-video"),
        "没有宿主纹理时不挂 file-preview"
    );
    assert!(
        !crate::shell::wants_preview_texture(&model),
        "离屏不上传宿主纹理"
    );
    let frame = model.preview_pixels.as_ref().expect("解码画面");
    assert_eq!((frame.width, frame.height), (32, 18));
    assert!(
        frame
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0x2A, 0x2D, 0x33, 0xFF]),
        "播放头在后半段时应是夹具第二帧"
    );
    assert!(model.preview_token.is_none(), "离屏不挂纹理记号");
    assert!(
        has_label(&window, "没有可用于播放此媒体的插件"),
        "播放条照 Vue 写没有播放插件"
    );
    // 拨回片头仍是解码出的第一帧，再拨到后半段也不把纹理记号挂回去。
    place_playhead(&mut model, 0);
    let rewound = model.preview_pixels.as_ref().expect("片头画面");
    assert!(
        rewound
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0x20, 0x22, 0x26, 0xFF]),
        "片头应是夹具第一帧"
    );
    assert!(model.preview_token.is_none(), "拨回片头也不挂纹理记号");
    let later = model
        .inspect
        .media_session()
        .and_then(|session| session.duration_ms)
        .unwrap_or(0)
        .saturating_sub(1);
    place_playhead(&mut model, later);
    let window = laid_out(&model);
    assert!(
        has_key(&window, "inspect-video-pixels"),
        "后半段仍画解码像素"
    );
    assert!(
        !has_key(&window, "inspect-video"),
        "播放头往前走也不挂 file-preview"
    );
    assert!(
        !has_key(&window, "inspect-fallback-icon"),
        "有解码画面时不画视频图标"
    );
    let frame = model.preview_pixels.as_ref().expect("后半段画面");
    assert!(
        frame
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0x2A, 0x2D, 0x33, 0xFF]),
        "后半段仍是夹具第二帧"
    );

    model.preview_pixels = None;
    let bare = laid_out(&model);
    assert!(
        has_key(&bare, "inspect-fallback-icon"),
        "没有画面时画视频图标"
    );
    assert!(
        !has_key(&bare, "inspect-audio-stage"),
        "没有画面时也不画唱片舞台"
    );

    model.preview_token = Some("video:clip.avi:0:32".into());
    model.preview_pixels = Some(crate::shell::PreviewPixels {
        width: 2,
        height: 2,
        rgba: vec![255; 16],
    });
    assert!(
        has_key(&laid_out(&model), "inspect-video"),
        "有宿主纹理时画纹理槽"
    );
    assert!(
        crate::shell::wants_preview_texture(&model),
        "有宿主纹理时仍要上传"
    );

    assert!(has_label(
        &laid_out(&loading_media_preview("movies/clip.mp4")),
        "准备媒体"
    ));
    assert!(has_label(
        &laid_out(&loading_media_preview("music/track-01.mp3")),
        "准备音频"
    ));
}

/// 音视频预览失败就近写在预览框里：照 Vue 预览插件的失败浮层替换唱片舞台，红色标题下一行原因。
/// Nana 自己的「没有原生解码器」和某格式「解码失败」都这样；视频的标题是「无法预览该媒体」。
/// 这类失败不进侧栏的全局状态区。
#[test]
fn media_preview_failures_show_their_reason_in_place() {
    for (path, error, title) in [
        ("music/track-01.mp3", "没有原生解码器", "无法预览该音频"),
        (
            "music/track-02.flac",
            "无法识别压缩音频：end of stream",
            "无法预览该音频",
        ),
        (
            "movies/clip.mp4",
            "MP4 解码失败：没有解出画面",
            "无法预览该媒体",
        ),
        ("movies/clip.webm", "没有原生解码器", "无法预览该媒体"),
    ] {
        let model = failed_media_preview(path, error);
        let window = laid_out(&model);
        assert!(has_label(&window, title), "{path} 失败时没有标题 {title}");
        assert!(
            has_label(&window, error),
            "{path} 失败时没有写出原因 {error}"
        );
        assert!(
            has_key(&window, "inspect-failed"),
            "{path} 失败时没有失败浮层"
        );
        assert!(
            !has_key(&window, "inspect-audio-stage"),
            "{path} 失败时不该还画唱片舞台"
        );
        assert_eq!(
            StatusLine::project(&model),
            StatusLine::Hidden,
            "{path} 的预览失败不进全局状态区"
        );
    }
}

/// 播放条装不上预览中的同一个文件、把失败会话写回预览页：唱片舞台换成失败浮层；会话没带原因时写 Vue
/// 运行时的「音频无法播放」。
#[test]
fn player_failures_written_back_replace_the_audio_stage() {
    let mut model = scene("preview-audio");
    assert!(
        has_key(&laid_out(&model), "inspect-audio-stage"),
        "能播的音频预览是唱片舞台"
    );
    let PreviewBody::Media(session) = &model.inspect.body else {
        panic!("预览不是音视频")
    };
    let mut failed = session.clone();
    failed.status = "failed".into();
    failed.error = None;
    model.inspect.replace_shared_media(failed);
    let window = laid_out(&model);
    assert!(
        has_label(&window, "无法预览该音频"),
        "写回失败后没有失败标题"
    );
    assert!(
        has_label(&window, "音频无法播放"),
        "没有原因时没有写 Vue 的默认原因"
    );
    assert!(
        !has_key(&window, "inspect-audio-stage"),
        "写回失败后不该还画唱片舞台"
    );
}

/// 还有下一页时列表末尾写「滚动继续加载」；正在续读时改成「继续读取目录...」。
/// 没有更多、分类视图和智能文件夹都不再分页，这两句都不出现。
#[test]
fn load_more_caption_follows_paging_and_stays_off_virtual_views() {
    let absent = |model: &ShellViewModel, why: &str| {
        let window = laid_out(model);
        assert!(
            !has_label(&window, "滚动继续加载"),
            "{why} 不该出现「滚动继续加载」"
        );
        assert!(
            !has_label(&window, "继续读取目录..."),
            "{why} 不该出现「继续读取目录...」"
        );
    };

    let mut model = scene("live-files-plain");
    assert!(!model.files.has_more, "夹具目录一页就读完");
    absent(&model, "没有更多");

    model.files.has_more = true;
    let paging = laid_out(&model);
    assert!(
        has_label(&paging, "滚动继续加载"),
        "还有下一页时应写出继续加载"
    );
    assert!(
        !has_label(&paging, "继续读取目录..."),
        "还没续读时不写续读中"
    );

    model.files.loading_more = true;
    let loading = laid_out(&model);
    assert!(has_label(&loading, "继续读取目录..."), "续读中应改写文案");
    assert!(
        !has_label(&loading, "滚动继续加载"),
        "续读中不再留原来的文案"
    );

    let category = scene("uncategorized");
    assert!(!category.files.has_more, "分类视图不再分页");
    absent(&category, "分类视图");

    let smart = scene("smart-folder");
    assert!(!smart.files.has_more, "智能文件夹不再分页");
    absent(&smart, "智能文件夹");
}

/// 侧栏「未分类 / 全部」照 Vue `selectShortcut`：不重读目录，文件列换成分类或回到当前目录，选择不改。
///
/// 分类视图不跟目录的「滚动继续加载」。不在这一类里的选择，详情回到「选择一个文件或文件夹」，路径仍留着，
/// 回到「全部」再亮起来。从失败的回收站点「全部」是非静默读取：清掉文件列错误，换成「正在读取目录」，选择仍不在这一下清掉。
#[test]
fn opening_a_library_shortcut_keeps_the_list_and_the_selection() {
    use crate::shell::view_harness::ShellHarness;
    use crate::shell::{ShortcutId, SidebarMessage};

    let shortcut = |id| ShellMessage::Sidebar(SidebarMessage::SelectShortcut(id));
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Files(FilesMessage::ActivateRow(
        "cover.png".into(),
    )));
    harness.model.files.has_more = true;
    harness.sync();
    harness.flush();
    assert!(harness.find("滚动继续加载").is_some(), "目录还有下一页");
    assert_eq!(
        harness.model.files.selected_paths(),
        &["cover.png".to_string()]
    );

    harness.apply(shortcut(ShortcutId::Uncategorized));
    harness.flush();
    assert!(harness.find("正在读取目录").is_none(), "点未分类不重读目录");
    assert!(
        harness.find("滚动继续加载").is_none(),
        "分类视图不跟目录分页"
    );
    assert!(
        harness.find("cover.png").is_some(),
        "未分类里有根目录的 cover.png"
    );
    assert!(
        harness.find("选择一个文件或文件夹").is_none(),
        "还在这一类里的选择留着"
    );
    assert_eq!(
        harness.model.files.selected_paths(),
        &["cover.png".to_string()]
    );
    assert!(!harness.model.files.loading);

    harness.apply(shortcut(ShortcutId::All));
    harness.flush();
    assert!(
        harness.find("正在读取目录").is_none(),
        "点全部也不重读当前目录"
    );
    assert!(
        harness.find("滚动继续加载").is_some(),
        "回到全部仍按目录分页"
    );
    assert_eq!(
        harness.model.files.selected_paths(),
        &["cover.png".to_string()]
    );

    harness.apply(ShellMessage::Files(FilesMessage::ActivateRow(
        "notes/page.pdf".into(),
    )));
    harness.flush();
    assert!(harness.find("page.pdf").is_some());
    assert_eq!(
        harness.model.files.selected_paths(),
        &["notes/page.pdf".to_string()]
    );

    harness.apply(shortcut(ShortcutId::Uncategorized));
    harness.flush();
    assert!(
        harness.find("page.pdf").is_none(),
        "子目录里的文件不进未分类"
    );
    assert!(
        harness.find("选择一个文件或文件夹").is_some(),
        "不在这一类里，详情是空的"
    );
    assert_eq!(
        harness.model.files.selected_paths(),
        &["notes/page.pdf".to_string()],
        "选择留着，不在点快捷方式时清掉"
    );
    assert!(harness.find("正在读取目录").is_none());

    harness.apply(shortcut(ShortcutId::All));
    harness.flush();
    assert!(
        harness.find("page.pdf").is_some(),
        "回到全部，原来的选择还在列表里"
    );
    assert!(harness.find("选择一个文件或文件夹").is_none());
    assert_eq!(
        harness.model.files.selected_paths(),
        &["notes/page.pdf".to_string()]
    );

    harness.apply(shortcut(ShortcutId::Trash));
    harness.model.stage_browses();
    harness.model.files.loading = false;
    harness.apply(ShellMessage::Files(FilesMessage::NoteError(
        "回收站读失败".into(),
    )));
    harness.flush();
    assert!(
        harness.keyed("file-error").is_some(),
        "回收站失败写在文件列"
    );
    assert!(harness.find("正在读取目录").is_none());

    harness.apply(shortcut(ShortcutId::All));
    harness.model.stage_browses();
    harness.sync();
    harness.flush();
    assert!(
        harness.find("正在读取目录").is_some(),
        "从回收站回到全部要显示正在读取目录"
    );
    assert!(
        harness.keyed("file-error").is_none(),
        "非静默读取清掉文件列上的失败"
    );
    assert!(harness.find("回收站读失败").is_none());
    assert_eq!(
        harness.model.files.selected_paths(),
        &["notes/page.pdf".to_string()],
        "读取过程中选择先留着"
    );
    assert!(harness.model.files.loading);
}

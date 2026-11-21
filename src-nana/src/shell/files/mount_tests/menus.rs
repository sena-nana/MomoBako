//! 输入框键路径、标签菜单、导入菜单、右键菜单、对话框遮罩与回收站删除确认的挂载测试。

use super::*;

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

/// 标签菜单是 NanaUI 的 `Popover`，挂在「+」上，放不下就翻面再收进窗口：960×600 和窄到详情卡片叠到
/// 列表下面（800 宽）时，面板都整块在窗口里。
#[test]
fn tag_menu_stays_inside_the_window_in_other_window_sizes() {
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
        assert!(
            panel.width > 0.0 && panel.height > 0.0,
            "{width}×{height} 菜单要排出来：{panel:?}"
        );
        assert!(
            panel.x >= 0.0
                && panel.x + panel.width <= width + 0.5
                && panel.y >= 0.0
                && panel.y + panel.height <= height + 0.5,
            "{width}×{height} 菜单不出窗口：{panel:?}"
        );
    }
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
                [ShellMessage::Files(FilesMessage::ImportMenuToggled(false))]
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

/// 点标签菜单外面，框架收起菜单、发 `PopoverToggled`，不新增标签。
#[test]
fn tag_menu_closes_when_clicking_outside() {
    let mut model = scene("files-selected-metadata");
    model.reduce(ShellMessage::Files(FilesMessage::ToggleTagMenu));
    assert!(model.inspect.tag_menu_open());
    let tags_before = model.inspect.draft_tags().to_vec();
    let mut window = laid_out(&model);
    let messages = click(&mut window, 120.0, 420.0);
    assert!(
        messages.iter().any(|message| matches!(
            message,
            ShellMessage::Files(FilesMessage::TagMenuToggled(false))
        )),
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
            [
                ShellMessage::Files(FilesMessage::OpenDialog(FileDialog::Import)),
                ShellMessage::Files(FilesMessage::ImportMenuToggled(false)),
            ]
        ),
        "点菜单项打开对话框，菜单随之收起：{} 条消息",
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
/// 再点确认句才彻底删除。
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
        model.files.take_effects().iter().any(|effect| matches!(effect, super::super::FilesEffect::Delete { mode: Some(mode), .. } if mode == "permanentDelete")),
        "回收站删除是彻底删除"
    );
}

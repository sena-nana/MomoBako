//! 文件列表视图。
//!
//! 实况目录用 `each_virtual` 只构建窗口内的行。列表高 28、预取 56；
//! 自适应、瀑布流和网格共用同一套网格，缩略图走 Nana `Thumbnail`。
//! 对话框放在文件列里，避免和壳层唯一的浮层槽抢位置。

use nana_ui::icons_tabler::{
    ARCHIVE, ARROW_FORWARD, COPY, FILE, FOLDER_OPEN, FOLDER_PLUS, LAYOUT_BOARD, LAYOUT_DASHBOARD, LAYOUT_GRID,
    LAYOUT_LIST, LIST_CHECK, PENCIL, REPLACE, ROTATE, SCISSORS, SWITCH, TRASH,
};
use nana_ui::runtime::view::{button, each_virtual, signal, text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, Icon, Stack, TextChanged, TextInput, Thumbnail};

use super::files::{
    hardlink_label, DisplayMode, FileContext, FileDialog, FileRow, FilesMessage, SelectionMode,
};
use super::{ShellMessage, ShellViewModel};

/// 目录说明、展示方式和工具条。列表单独占剩余高度，播放条才能留在窗口里。
fn files_tools(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let files = &model.files;
    let mut rows = Vec::new();
    rows.push(text(location_eyebrow(&ctx)).key("file-location").into_any());
    rows.push(text(location_title(&ctx, &files.current_path)).key("file-path").into_any());
    rows.push(display_modes().into_any());
    rows.push(selection_modes().into_any());
    if !ctx.is_virtual() {
        rows.push(breadcrumbs(&files.current_path).into_any());
    }
    rows.push(toolbar(&ctx, files).into_any());
    widget(Stack::column(6.0)).children(rows).into_any()
}

/// 在 `mount_view_root` 里调用，这样虚拟列表的 signal 落在当前视图作用域。
pub(super) fn files_surface(model: &ShellViewModel) -> AnyView {
    widget(Stack::column(8.0))
        .children((files_tools(model), files_browser(model)))
        .into_any()
}

fn files_browser(model: &ShellViewModel) -> AnyView {
    widget(Stack::column(8.0))
        .children((
            files_status(model),
            file_list(model.files.visible_rows(&FileContext::from_model(model)), model.files.display_mode),
            load_more(model),
            file_dialog(&model.files),
        ))
        .into_any()
}

fn files_status(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let files = &model.files;
    let mut rows = Vec::new();
    if !files.error.is_empty() {
        rows.push(text(files.error.clone()).key("file-error").into_any());
    }
    if !files.activity.is_empty() {
        rows.push(text(files.activity.clone()).key("file-activity").into_any());
    }
    if files.visible_rows(&ctx).is_empty() && !files.loading {
        rows.push(text(empty_copy(&ctx)).key("file-empty").into_any());
    }
    widget(Stack::column(6.0)).children(rows).into_any()
}

fn load_more(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    button("加载更多")
        .key("file-load-more")
        .disabled(!model.files.can_load_more(&ctx))
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(file_message(FilesMessage::LoadMore)))
        .into_any()
}

/// 实况文件页的纵带：任务读数、目录工具、文件名和播放条各占自己的盒子。
pub(super) fn live_file_column(model: &ShellViewModel) -> AnyView {
    let filter = model.inspect.filter_bar_open.then(|| {
        super::inspect_view::filter_bar(&model.inspect.filters, model.inspect.searching)
    });
    let player = model.player_surface_visible().then(|| super::player_view::player_surface(model));
    let close_prompt = super::input::close_prompt(model);
    widget(
        Stack::fill_column(8.0)
            .padding_xy(16.0, 8.0)
            .min_width(nana_ui::runtime::LengthSpec::Px(0.0))
            .min_height(nana_ui::runtime::LengthSpec::Px(0.0)),
    )
    .children((
        close_prompt,
        text(format!("任务 {}", model.active_tasks)).key("task-count"),
        files_tools(model),
        files_status(model),
        file_list(model.files.visible_rows(&FileContext::from_model(model)), model.files.display_mode),
        load_more(model),
        filter,
        player,
    ))
    .into_any()
}

fn location_eyebrow(ctx: &FileContext) -> &'static str {
    if ctx.is_virtual() {
        "分类视图"
    } else if ctx.trash {
        "回收站"
    } else {
        "当前目录"
    }
}

fn location_title(ctx: &FileContext, path: &str) -> String {
    if ctx.is_virtual() {
        "分类视图".into()
    } else if ctx.trash {
        "回收站".into()
    } else if path.is_empty() {
        "根目录".into()
    } else {
        path.to_string()
    }
}

fn empty_copy(ctx: &FileContext) -> &'static str {
    if ctx.smart_folder {
        "只读智能文件夹，不能在这里新建、重命名或删除。"
    } else if ctx.trash {
        "回收站是空的"
    } else {
        "当前目录没有条目"
    }
}

fn display_modes() -> impl IntoView {
    widget(Stack::row(4.0)).children((
        mode_button(DisplayMode::Adaptive, LAYOUT_DASHBOARD),
        mode_button(DisplayMode::Masonry, LAYOUT_BOARD),
        mode_button(DisplayMode::Grid, LAYOUT_GRID),
        mode_button(DisplayMode::List, LAYOUT_LIST),
    ))
}

fn mode_button(mode: DisplayMode, icon: Icon) -> impl IntoView {
    widget(super::title_bar::shell_icon(icon, mode.label(), false))
        .key(format!("file-display-{}", mode.storage_value()))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(file_message(FilesMessage::SetDisplayMode(mode)));
        })
}

fn selection_modes() -> impl IntoView {
    widget(Stack::row(4.0)).children((
        selection_button(SelectionMode::Replace, REPLACE),
        selection_button(SelectionMode::Toggle, SWITCH),
        selection_button(SelectionMode::Range, LIST_CHECK),
    ))
}

fn selection_button(mode: SelectionMode, icon: Icon) -> impl IntoView {
    widget(super::title_bar::shell_icon(icon, mode.label(), false))
        .key(format!("file-selection-{}", mode.label()))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(file_message(FilesMessage::SetSelectionMode(mode)));
        })
}

fn breadcrumbs(path: &str) -> impl IntoView {
    let mut crumbs = Vec::new();
    crumbs.push(
        button("根目录")
            .key("file-crumb-root")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program(file_message(FilesMessage::OpenPath(String::new()))))
            .into_any(),
    );
    let mut cursor = String::new();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        if cursor.is_empty() {
            cursor = segment.to_string();
        } else {
            cursor = format!("{cursor}/{segment}");
        }
        let target = cursor.clone();
        crumbs.push(
            button(segment.to_string())
                .key(format!("file-crumb-{cursor}"))
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(file_message(FilesMessage::OpenPath(target.clone()))))
                .into_any(),
        );
    }
    widget(Stack::row(8.0)).children(crumbs)
}

fn toolbar(ctx: &FileContext, files: &super::files::FilesState) -> impl IntoView {
    let mut actions = Vec::new();
    if !ctx.trash && !ctx.is_virtual() {
        actions.push(action(FOLDER_PLUS, "新建文件夹", "file-create-directory", files.can_create(ctx), FilesMessage::OpenDialog(FileDialog::CreateDirectory)));
        actions.push(action(FILE, "建文件", "file-create-file", files.can_create(ctx), FilesMessage::OpenDialog(FileDialog::CreateFile)));
        actions.push(action(FOLDER_OPEN, "导入", "file-import", files.can_import(ctx), FilesMessage::OpenDialog(FileDialog::Import)));
        actions.push(action(ARCHIVE, "从 ZIP 导入", "file-import-archive", files.can_import(ctx), FilesMessage::OpenDialog(FileDialog::ImportArchive)));
        actions.push(action(COPY, "复制导入", "file-import-eagle-copy", files.can_import(ctx), FilesMessage::OpenEagle("copy".into())));
        actions.push(action(SCISSORS, "剪切导入", "file-import-eagle-move", files.can_import(ctx), FilesMessage::OpenEagle("move".into())));
    }
    actions.push(action(COPY, "复制", "file-copy", files.can_transfer(ctx), FilesMessage::OpenDialog(FileDialog::Copy)));
    actions.push(action(ARROW_FORWARD, "移动", "file-move", files.can_transfer(ctx), FilesMessage::OpenDialog(FileDialog::Move)));
    actions.push(action(PENCIL, "重命名", "file-rename", files.can_rename(ctx), FilesMessage::OpenDialog(FileDialog::Rename)));
    let delete_label = if ctx.trash { "永久删除" } else { "删除" };
    actions.push(action(TRASH, delete_label, "file-delete", files.can_delete(ctx), FilesMessage::DeleteSelected));
    if ctx.trash {
        actions.push(action(ROTATE, "还原", "file-restore", files.can_restore(ctx), FilesMessage::RestoreSelected));
        actions.push(action(ROTATE, "还原所有项目", "file-restore-all", files.can_empty_trash(ctx), FilesMessage::RestoreAll));
        actions.push(action(TRASH, "清空回收站", "file-empty-trash", files.can_empty_trash(ctx), FilesMessage::EmptyTrash));
    }
    widget(Stack::row(4.0)).children(actions).key("file-toolbar")
}

fn action(icon: Icon, label: &'static str, key: &'static str, enabled: bool, message: FilesMessage) -> AnyView {
    widget(super::title_bar::shell_icon(icon, label, !enabled))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(file_message(message.clone()));
        })
        .into_any()
}

/// 列表行高 28。网格行更高，以便放下缩略图和标题。
fn file_list(rows: Vec<FileRow>, mode: DisplayMode) -> AnyView {
    let items = signal(rows);
    let list_mode = mode.is_list();
    let height = if list_mode { 28.0 } else { 96.0 };
    let list = each_virtual(items, |row| row.key(), height, move |row| file_row(row, list_mode))
        .overscan(56.0)
        .grow()
        .key("file-virtual-list");
    if list_mode { list.into_any() } else { list.grid(160.0, 8.0).into_any() }
}

fn file_row(row: FileRow, list_mode: bool) -> AnyView {
    let path = row.path.clone();
    let label = row_label(&row, list_mode);
    let activate = button(label).key(format!("file-row-{}", row.key())).on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(file_message(FilesMessage::ActivateRow(path.clone())));
    });
    if list_mode {
        return activate.into_any();
    }
    let thumb = match row.thumbnail_path.as_deref().map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => widget(Thumbnail::new(path)),
        None => widget(Thumbnail::empty()),
    };
    widget(Stack::column(4.0)).children((thumb, activate)).into_any()
}

fn row_label(row: &FileRow, list_mode: bool) -> String {
    let hardlink = hardlink_label(row.hardlink_state.as_deref());
    if list_mode {
        if hardlink.is_empty() {
            format!("{} · {}", row.name, row.path)
        } else {
            format!("{} · {} · {hardlink}", row.name, row.path)
        }
    } else if hardlink.is_empty() {
        row.name.clone()
    } else {
        format!("{} · {hardlink}", row.name)
    }
}

fn file_dialog(files: &super::files::FilesState) -> Option<AnyView> {
    match files.dialog {
        FileDialog::Closed => None,
        FileDialog::CreateDirectory => Some(name_dialog("新建文件夹", &files.name_draft, "文件夹名称", files.mutating)),
        FileDialog::CreateFile => Some(name_dialog("建文件", &files.name_draft, "新建空文件，例如 note.txt", files.mutating)),
        FileDialog::Rename => Some(name_dialog("重命名", &files.name_draft, "新名称", files.mutating)),
        FileDialog::Copy => Some(target_dialog("复制到文件夹", &files.target_draft, "留空表示根目录", "复制", files.mutating)),
        FileDialog::Move => Some(target_dialog("移动到文件夹", &files.target_draft, "留空表示根目录", "移动", files.mutating)),
        FileDialog::Import => Some(target_dialog("导入", &files.import_draft, "多个路径用分号分隔", "导入", files.mutating)),
        FileDialog::ImportArchive => Some(target_dialog("从 ZIP 导入", &files.import_draft, "压缩包路径", "导入压缩包", files.mutating)),
        FileDialog::ImportEagle => {
            let title = if files.eagle_mode == "move" { "剪切导入" } else { "复制导入" };
            Some(target_dialog(title, &files.import_draft, "Eagle 库路径", "导入", files.mutating))
        }
        FileDialog::Hardlink => files.current_hardlink().map(|prompt| hardlink_dialog(prompt.message(), files.mutating)),
    }
}

fn name_dialog(title: &'static str, draft: &str, placeholder: &'static str, mutating: bool) -> AnyView {
    target_dialog(title, draft, placeholder, "确认", mutating)
}

fn target_dialog(title: &'static str, draft: &str, placeholder: &'static str, submit: &'static str, mutating: bool) -> AnyView {
    let submit_label = if mutating { "处理中..." } else { submit };
    widget(Stack::column(8.0)).children((
        text(title).key("file-dialog-title"),
        widget(TextInput::new(draft.to_string()).label(placeholder)).on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program(file_message(FilesMessage::DraftChanged(event.value.to_string())));
        }),
        widget(Stack::row(8.0)).children((
            button("取消").key("file-dialog-cancel").disabled(mutating).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::CloseDialog));
            }),
            button(submit_label).key("file-dialog-submit").disabled(mutating).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::SubmitDialog));
            }),
        )),
    )).into_any()
}

fn hardlink_dialog(message: String, mutating: bool) -> AnyView {
    let confirm = if mutating { "处理中..." } else { "加入关联" };
    widget(Stack::column(8.0)).children((
        text("加入硬链接关联").key("file-hardlink-title"),
        text(message).key("file-hardlink-message"),
        widget(Stack::row(8.0)).children((
            button("跳过").key("file-hardlink-skip").disabled(mutating).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::SkipHardlink));
            }),
            button(confirm).key("file-hardlink-confirm").disabled(mutating).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::ConfirmHardlink));
            }),
        )),
    )).into_any()
}

fn file_message(message: FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

//! 文件列表视图。
//!
//! 实况目录用 `each_virtual` 只构建窗口内的行。列表高 28、预取 56；
//! 自适应和瀑布流按宽高比换行，网格和列表用固定盒子。缩略图走 Nana `Thumbnail`。
//! 对话框用 Nana `Dialog`，固定定位盖住窗口，不占用壳层唯一浮层槽。

use std::sync::Arc;

use nana_ui::icons_tabler::{
    ARCHIVE, ARROW_FORWARD, COPY, FILE, FOLDER_OPEN, FOLDER_PLUS, LAYOUT_BOARD, LAYOUT_DASHBOARD, LAYOUT_GRID,
    LAYOUT_LIST, LIST_CHECK, PENCIL, REPLACE, ROTATE, SCISSORS, SWITCH, TRASH,
};
use nana_ui::runtime::view::{button, each_virtual, signal, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, ConfirmDialog, Dialog, Icon, LengthSpec, RadiusTier, ScrollAxes, ScrollView, Stack, TextChanged,
    TextInput, Thumbnail,
};
use nana_ui::ContentFit;

use super::files::{
    hardlink_label, DisplayMode, FileContext, FileDialog, FileRow, FilesMessage, SelectionMode,
};
use super::{ShellMessage, ShellViewModel};

/// 文件头和 Tauri 一样左右对齐：左边目录，右边工具。
fn files_tools(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let files = &model.files;
    let path = if ctx.is_virtual() {
        text(location_title(&ctx, &files.current_path)).key("file-path").into_any()
    } else {
        breadcrumbs(&files.current_path).into_any()
    };
    widget(Stack::column(8.0))
        .children((
            widget(Stack::bar(12.0)).children((
                widget(Stack::column(2.0)).children((
                    text(location_eyebrow(&ctx)).key("file-location"),
                )),
                widget(Stack::spacer()),
                display_modes(),
                selection_modes(),
            )),
            widget(Stack::bar(12.0)).children((
                path,
                widget(Stack::spacer()),
                toolbar(&ctx, files),
            )),
        ))
        .into_any()
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
    if let Some(label) = files.operation_label() {
        rows.push(text(label).key("file-operation").into_any());
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
        files_tools(model),
        files_status(model),
        file_list(model.files.visible_rows(&FileContext::from_model(model)), model.files.display_mode),
        load_more(model),
        file_dialog(&model.files),
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

/// 列表行高 72，网格行高 190。自适应和瀑布流按条目宽度换行。
fn file_list(rows: Vec<FileRow>, mode: DisplayMode) -> AnyView {
    match mode {
        DisplayMode::List => each_virtual(signal(rows), |row| row.key(), 72.0, move |row| file_row(row, mode))
            .overscan(56.0)
            .grow()
            .key("file-virtual-list")
            .into_any(),
        DisplayMode::Grid => each_virtual(signal(rows), |row| row.key(), 190.0, move |row| file_row(row, mode))
            .overscan(56.0)
            .grid(148.0, 14.0)
            .grow()
            .key("file-virtual-list")
            .into_any(),
        DisplayMode::Adaptive | DisplayMode::Masonry => {
            let cards: Vec<_> = rows.into_iter().map(|row| file_row(row, mode)).collect();
            widget(
                ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
                    layout.flex_grow = Some(1.0);
                    layout.flex_shrink = Some(1.0);
                    layout.flex_basis = Some(LengthSpec::Px(0.0));
                    layout.min_height = Some(LengthSpec::Px(0.0));
                }),
            )
            .children((widget(Stack::row(14.0).wrap(true).align(AlignSpec::Start)).children(cards),))
            .key("file-wrap-list")
            .into_any()
        }
    }
}

fn file_row(row: FileRow, mode: DisplayMode) -> AnyView {
    let metrics = super::thumbs::thumb_box(mode, row.pixel_width, row.pixel_height);
    let path = row.path.clone();
    let label = row_label(&row, mode.is_list());
    let activate = button(label).key(format!("file-row-{}", row.key())).on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(file_message(FilesMessage::ActivateRow(path.clone())));
    });
    let marker = path_marker(&row.path);
    let thumb = thumbnail(&row, metrics.preview_width, metrics.preview_height, mode.is_list());
    if mode.is_list() {
        widget(Stack::row(12.0).align(AlignSpec::Center).min_height(LengthSpec::Px(72.0)))
            .children((thumb, activate, marker))
            .into_any()
    } else {
        let mut card = Stack::column(8.0).padding_xy(8.0, 8.0);
        if metrics.item_width > 0.0 {
            card = card.width(LengthSpec::Px(metrics.item_width));
        }
        widget(card).children((thumb, activate, marker)).into_any()
    }
}

/// 行上的隐藏路径。命中按钮后沿父节点找到它，实况指针才能对上条目。
fn path_marker(path: &str) -> AnyView {
    let mut marker = nana_ui::runtime::Text::new(format!("momobako-path:{path}"));
    let layout = std::sync::Arc::make_mut(&mut marker.style.layout);
    layout.hidden = true;
    layout.height = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Px(0.0));
    widget(marker).into_any()
}

/// 预览盒按计算出的宽高固定。图片用 cover，和 Vue 的 `object-fit: cover` 一样。
fn thumbnail(row: &FileRow, width: f32, height: f32, list_mode: bool) -> AnyView {
    let mut thumb = if row.texture_ready {
        row.thumbnail_path.as_deref().map(str::trim).filter(|path| !path.is_empty()).map(|path| {
            Thumbnail::new(super::thumbs::thumbnail_slot(path))
                .fit(ContentFit::Cover)
                .aspect(super::thumbs::content_aspect(row.pixel_width, row.pixel_height))
        })
    } else {
        None
    }
    .unwrap_or_else(Thumbnail::empty);
    let layout = Arc::make_mut(&mut thumb.style.layout);
    layout.width = Some(LengthSpec::Px(width));
    layout.height = Some(LengthSpec::Px(height));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    thumb.style.radius = Some(if list_mode { RadiusTier::Sm } else { RadiusTier::Md });
    widget(thumb).into_any()
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
    widget(Dialog::new(title))
        .body(widget(TextInput::new(draft.to_string()).label(placeholder)).on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program(file_message(FilesMessage::DraftChanged(event.value.to_string())));
        }))
        .footer(widget(Stack::row(8.0)).children((
            button("取消").key("file-dialog-cancel").disabled(mutating).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::CloseDialog));
            }),
            button(submit_label).key("file-dialog-submit").disabled(mutating).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::SubmitDialog));
            }),
        )))
        .into_any()
}

fn hardlink_dialog(message: String, mutating: bool) -> AnyView {
    let confirm = if mutating { "处理中..." } else { "加入关联" };
    widget(ConfirmDialog::new("加入硬链接关联", message))
        .cancel(button("跳过").key("file-hardlink-skip").disabled(mutating).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(file_message(FilesMessage::SkipHardlink));
        }))
        .confirm(button(confirm).key("file-hardlink-confirm").disabled(mutating).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(file_message(FilesMessage::ConfirmHardlink));
        }))
        .into_any()
}

fn file_message(message: FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

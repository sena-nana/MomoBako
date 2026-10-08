//! 文件列表视图。
//!
//! 列表和网格用 `each_virtual` 只构建窗口内的行。列表高 72。网格卡片只写标题。
//! 自适应和瀑布流按卡片宽高比换行，滚动容器撑满剩余高度，框选才能扫到多张卡片。
//! 缩略图走 Nana `Thumbnail`。
//! 对话框用 Nana `Dialog`，固定定位盖住窗口，不占用壳层唯一浮层槽。

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use nana_ui::icons_tabler::{ERASER, FILE, FOLDER_OPEN, ROTATE, TRASH};
use nana_ui::runtime::view::{button, each_virtual, node_ref, signal, text, widget, AnyView, IntoView, NodeRef};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, Dialog, EmptyState, FileDropEvent, Icon, IconGlyph, JustifySpec, LengthSpec, ListItem,
    Progress, RadiusTier, ScrollAxes, ScrollView, SecondaryPress, Select, SelectChanged, SelectOption, SemanticColorRole,
    Stack, TextChanged, TextInput, Thumbnail, ValidationIntent, ValidationMessage,
};
use nana_ui::ButtonKind;
use nana_ui::ContentFit;
use nana_ui::ControlSize;

use super::files::{hardlink_label, DisplayMode, FileContext, FileDialog, FileRow, FilesMessage};
use super::sidebar::SidebarMessage;
use super::workspace::{LibraryCategory, WorkspacePanel};
use super::{ShellMessage, ShellViewModel};

#[path = "files_menu.rs"]
mod menu;
#[path = "files_detail.rs"]
mod detail;

/// 文件右键菜单。没有目标时不占浮层。
pub(super) fn entry_menu(model: &ShellViewModel) -> Option<AnyView> {
    menu::entry_menu(model)
}

thread_local! {
    static LAST_ROW_CLICK: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

/// 400 毫秒内连点同一条算双击，第二次才进入目录或打开预览。
fn row_activation(path: &str) -> FilesMessage {
    let open = LAST_ROW_CLICK.with(|slot| {
        let mut slot = slot.borrow_mut();
        let now = Instant::now();
        let open = slot.as_ref().is_some_and(|(previous, at)| previous == path && now.duration_since(*at).as_millis() < 400);
        *slot = Some((path.to_string(), now));
        open
    });
    if open {
        FilesMessage::OpenRow(path.to_string())
    } else {
        FilesMessage::ActivateRow(path.to_string())
    }
}

/// 文件头：眉题和当前位置在上，主操作和密集工具在下。过程信息另走状态条。
fn files_tools(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let files = &model.files;
    let path = if ctx.is_virtual() {
        widget(super::workbench::page_title(location_title(&ctx, &files.current_path))).key("file-path").into_any()
    } else {
        breadcrumbs(&files.current_path).into_any()
    };
    widget(Stack::column(8.0))
        .children((
            widget(Stack::column(2.0)).children((
                widget(super::workbench::eyebrow(location_eyebrow(&ctx))).key("file-location"),
                path,
            )),
            widget(Stack::bar(12.0).align(AlignSpec::Center).wrap(true)).children((
                display_mode_field(files.display_mode),
                toolbar(model),
            )),
        ))
        .into_any()
}

fn files_status(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let files = &model.files;
    let mut rows = Vec::new();
    if !files.error.is_empty() {
        rows.push(widget(ValidationMessage::new(files.error.clone(), ValidationIntent::Danger)).key("file-error").into_any());
    }
    if !files.activity.is_empty() {
        rows.push(widget(super::workbench::meta(files.activity.clone())).key("file-activity").into_any());
    }
    if let Some(percent) = model.motion.operation_percent() {
        let mut progress = Progress::new(f64::from(percent), 100.0).label(files.operation_label().unwrap_or_default());
        let layout = std::sync::Arc::make_mut(&mut progress.style.layout);
        layout.opacity = Some(model.motion.pulse_opacity());
        layout.height = Some(LengthSpec::Px(6.0));
        rows.push(widget(progress).key("file-operation-progress").into_any());
    }
    if model.motion.spinner_on() {
        let mut mark = nana_ui::runtime::Text::new("忙");
        let layout = std::sync::Arc::make_mut(&mut mark.style.layout);
        layout.transform = Some(super::motion::spin_transform(model.motion.spinner_degrees()));
        rows.push(widget(mark).key("motion-spinner").into_any());
    }
    if files.visible_rows(&ctx).is_empty() && !files.loading {
        rows.push(empty_state(&ctx));
    }
    widget(Stack::column(6.0)).children(rows).into_any()
}

/// 还有下一页，或正在续读时，才画出 Vue 的分页提示。
/// 空目录和已经读完的列表不放禁用按钮。点下去仍走原来的加载更多。
fn load_more(model: &ShellViewModel) -> Option<AnyView> {
    let ctx = FileContext::from_model(model);
    let loading = model.files.loading_more;
    if !model.files.has_more && !loading {
        return None;
    }
    let label = if loading { "继续读取目录..." } else { "滚动继续加载" };
    Some(
        button(label)
            .key("file-load-more")
            .disabled(!model.files.can_load_more(&ctx))
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program(file_message(FilesMessage::LoadMore)))
            .into_any(),
    )
}

/// 实况文件页的纵带：任务读数、目录工具、文件名和播放条各占自己的盒子。
/// 目录列表右侧是详情。进入预览后主区换成预览和元数据，和 Vue 的 `files-preview-page` 一样。
pub(super) fn live_file_column(model: &ShellViewModel) -> AnyView {
    let previewing = model.page == super::ShellPage::SelectedFile && model.inspect.has_target();
    let on_file_list = model.page == super::ShellPage::FileList;
    let filter = (!previewing && model.inspect.filter_bar_open).then(|| super::inspect_search_view::filter_bar(model));
    // 文件列表的详情在右侧。其它带目标的页面仍挂完整预览，冲突和未保存标题才在。
    let show_inspect = previewing || model.inspect.filter_bar_open || (model.inspect.has_target() && !on_file_list);
    let inspect = show_inspect.then(|| {
        widget(
            Stack::fill_column(0.0)
                .grow(1.0)
                .shrink(1.0)
                .min_width(LengthSpec::Px(0.0))
                .min_height(LengthSpec::Px(0.0))
                .with_layout(|layout| {
                    layout.flex_basis = Some(LengthSpec::Px(0.0));
                }),
        )
        .children((super::inspect_view::inspect_surface(model),))
        .into_any()
    });
    let browser = (!previewing).then(|| file_browser(model));
    // 列表页的播放条在左列底部。预览页仍把播放条放在画面下面。
    // 预览页播放条固定在详情下面，不跟着内容把尺寸挤出窗口。
    let player = (previewing && model.player_surface_visible()).then(|| {
        widget(
            Stack::column(0.0)
                .grow(0.0)
                .shrink(0.0)
                .width(LengthSpec::Fill)
                .min_width(LengthSpec::Px(0.0)),
        )
        .children((super::player_view::player_surface(model),))
        .into_any()
    });
    let close_prompt = super::input::close_prompt(model);
    widget(
        Stack::fill_column(8.0)
            .padding_xy(16.0, 8.0)
            .min_width(nana_ui::runtime::LengthSpec::Px(0.0))
            .min_height(nana_ui::runtime::LengthSpec::Px(0.0)),
    )
    .on_cx({
        let flags = super::input::file_drop_flags(model);
        move |_, event: &FileDropEvent, cx| {
            cx.dispatch_program(super::input::file_drop_message(&flags, event));
        }
    })
    .children((
        super::input::drop_marker("files"),
        close_prompt,
        filter,
        browser,
        file_dialog(&model.files),
        super::workspace_dialogs::export_dialog(model),
        inspect,
        player,
    ))
    .into_any()
}

/// 左列是工具、列表和播放条。详情单独一列，伸到这一行的底，播放条不盖住它。
fn file_browser(model: &ShellViewModel) -> AnyView {
    let list = widget(
        Stack::fill_column(8.0)
            .grow(1.0)
            .shrink(1.0)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0)),
    )
    .children((files_status(model), file_list(model)))
    .into_any();
    let player = model.player_surface_visible().then(|| {
        widget(
            Stack::column(0.0)
                .grow(0.0)
                .shrink(0.0)
                .width(LengthSpec::Fill)
                .min_width(LengthSpec::Px(0.0)),
        )
        .children((super::player_view::player_surface(model),))
        .into_any()
    });
    let main = widget(
        Stack::fill_column(8.0)
            .grow(1.0)
            .shrink(1.0)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0)),
    )
    .children((files_tools(model), list, player))
    .into_any();
    widget(
        Stack::fill_row(12.0)
            .align(AlignSpec::Stretch)
            .grow(1.0)
            .shrink(1.0)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                layout.flex_basis = Some(LengthSpec::Px(0.0));
                layout.height = Some(LengthSpec::Fill);
            }),
    )
    .children((main, detail::detail_aside(model)))
    .key("file-browser")
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

/// 空目录说明下一步，而不是只写「没有」。
fn empty_state(ctx: &FileContext) -> AnyView {
    let (title, message) = if ctx.smart_folder {
        ("没有匹配的文件", "只读智能文件夹，不能在这里新建、重命名或删除。调整筛选后再看。")
    } else if ctx.trash {
        ("回收站是空的", "删除的文件会出现在这里，可以还原，或确认后永久删除。")
    } else if ctx.is_virtual() {
        ("这个分类里还没有文件", "换一个分类，或回到目录里导入文件。")
    } else {
        ("当前目录没有条目", "新建文件夹、建文件，或从外部导入。")
    };
    widget(EmptyState::new(title).message(message).compact(true)).key("file-empty").into_any()
}

/// 展示方式用下拉写出当前值。四个图标挤在工具后面时，当前是自适应还是网格看不出来。
fn display_mode_field(current: DisplayMode) -> impl IntoView {
    let options = [DisplayMode::Adaptive, DisplayMode::Masonry, DisplayMode::Grid, DisplayMode::List]
        .into_iter()
        .map(|mode| SelectOption::new(mode.storage_value(), mode.label()))
        .collect::<Vec<_>>();
    let mut select = Select::new(Some(current.storage_value().to_string()))
        .options(options)
        .placeholder("展示方式")
        .size(ControlSize::Small);
    let layout = Arc::make_mut(&mut select.style.layout);
    layout.min_width = Some(LengthSpec::Px(96.0));
    layout.width = Some(LengthSpec::Px(112.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    widget(Stack::row(6.0).align(AlignSpec::Center).grow(0.0).shrink(0.0))
        .children((
            widget(super::workbench::section_title("展示方式")).key("file-display-label"),
            widget(select).key("file-display-mode").on_cx(|_, event: &SelectChanged, cx| {
                cx.dispatch_program(file_message(FilesMessage::SetDisplayMode(DisplayMode::parse(&event.value))));
            }),
        ))
}

/// 面包屑：走过的路径是文字按钮，当前一段是标题，中间用弱分隔。
fn breadcrumbs(path: &str) -> impl IntoView {
    let segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
    let mut crumbs = Vec::new();
    crumbs.push(crumb_button("根目录", "file-crumb-root".into(), String::new()));
    let mut cursor = String::new();
    for (index, segment) in segments.iter().enumerate() {
        if cursor.is_empty() {
            cursor = (*segment).to_string();
        } else {
            cursor = format!("{cursor}/{segment}");
        }
        crumbs.push(widget(super::workbench::meta("/")).into_any());
        let current = index + 1 == segments.len();
        if current {
            crumbs.push(widget(super::workbench::section_title(*segment)).key(format!("file-crumb-{cursor}")).into_any());
        } else {
            crumbs.push(crumb_button(segment, format!("file-crumb-{cursor}"), cursor.clone()));
        }
    }
    widget(Stack::row(6.0).align(AlignSpec::Center).wrap(true)).children(crumbs)
}

fn crumb_button(label: &str, key: String, target: String) -> AnyView {
    widget(Button::new(label).kind(ButtonKind::Text))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(file_message(FilesMessage::OpenPath(target.clone()))))
        .into_any()
}

/// 和 Vue `FileBrowserPanel` 同一排：新建空文件输入、建文件、导入菜单。
/// 打开、移动、重命名在右键；从文件夹、ZIP、Eagle 在导入菜单里。
fn toolbar(model: &ShellViewModel) -> impl IntoView {
    let ctx = FileContext::from_model(model);
    let files = &model.files;
    let mut row = Vec::new();
    if model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == LibraryCategory::Recent {
        let enabled = model.sidebar.counts.recent > 0 && model.workspace.active_repo_id.is_some() && !model.navigation_locked();
        row.push(shell_text(Some(ERASER), "清空记录", "file-clear-recent", enabled, ButtonKind::Ghost, || {
            ShellMessage::Sidebar(SidebarMessage::ClearRecent)
        }));
    }
    if !ctx.trash && !ctx.is_virtual() {
        row.push(create_name_field(&files.create_name, files.mutating));
        row.push(file_text(Some(FILE), "建文件", "file-create-file", files.can_create(&ctx), ButtonKind::Ghost, FilesMessage::SubmitCreateFile));
        row.push(file_text(Some(FOLDER_OPEN), "导入", "file-import", files.can_import(&ctx), ButtonKind::Ghost, FilesMessage::ToggleImportMenu));
        if files.import_open {
            row.push(import_menu(files, &ctx));
        }
    } else if ctx.trash {
        row.push(file_text(Some(ROTATE), "还原所有项目", "file-restore-all", files.can_empty_trash(&ctx), ButtonKind::Ghost, FilesMessage::RestoreAll));
        row.push(file_text(Some(TRASH), "清空回收站", "file-empty-trash", files.can_empty_trash(&ctx), ButtonKind::Danger, FilesMessage::EmptyTrash));
    }
    widget(Stack::row(8.0).align(AlignSpec::Center).wrap(true).grow(1.0).shrink(1.0).min_width(LengthSpec::Px(0.0)))
        .children(row)
        .key("file-toolbar")
}

/// 新建空文件的输入。占位和 Vue 相同，不另开名称对话框。
fn create_name_field(draft: &str, mutating: bool) -> AnyView {
    let mut input = TextInput::new(draft.to_string())
        .placeholder("新建空文件，例如 note.txt")
        .size(ControlSize::Small)
        .disabled(mutating);
    let layout = Arc::make_mut(&mut input.style.layout);
    layout.min_width = Some(LengthSpec::Px(180.0));
    layout.width = Some(LengthSpec::Px(220.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(1.0);
    widget(input).key("file-create-name").on_cx(|_, event: &TextChanged, cx| {
        cx.dispatch_program(file_message(FilesMessage::SetCreateName(event.value.to_string())));
    }).into_any()
}

/// 导入展开后的三项。Eagle 再展开才出现复制和剪切。
fn import_menu(files: &super::files::FilesState, ctx: &FileContext) -> AnyView {
    let enabled = files.can_import(ctx);
    let mut rows = vec![
        file_text(None, "从文件夹导入", "file-import-folder", enabled, ButtonKind::Ghost, FilesMessage::OpenDialog(FileDialog::Import)),
        file_text(None, "从 ZIP 导入", "file-import-zip", enabled, ButtonKind::Ghost, FilesMessage::OpenDialog(FileDialog::ImportArchive)),
        file_text(None, "从 Eagle 导入", "file-import-eagle", enabled, ButtonKind::Ghost, FilesMessage::ToggleEagleImport),
    ];
    if files.eagle_open {
        rows.push(file_text(None, "复制导入", "file-import-eagle-copy", enabled, ButtonKind::Ghost, FilesMessage::OpenEagle("copy".into())));
        rows.push(file_text(None, "剪切导入", "file-import-eagle-move", enabled, ButtonKind::Ghost, FilesMessage::OpenEagle("move".into())));
    }
    widget(Stack::column(4.0).align(AlignSpec::Start).grow(0.0).shrink(0.0))
        .children(rows)
        .key("file-import-menu")
        .into_any()
}

fn shell_text(
    icon: Option<Icon>,
    label: &'static str,
    key: &'static str,
    enabled: bool,
    kind: ButtonKind,
    message: impl Fn() -> ShellMessage + Send + 'static,
) -> AnyView {
    let mut button = Button::new(label).kind(kind).disabled(!enabled);
    if let Some(icon) = icon {
        button = button.icon(icon);
    }
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    widget(button).key(key).on_cx(move |_, _: &Activate, cx| cx.dispatch_program(message())).into_any()
}

fn file_text(icon: Option<Icon>, label: &'static str, key: &'static str, enabled: bool, kind: ButtonKind, message: FilesMessage) -> AnyView {
    shell_text(icon, label, key, enabled, kind, move || file_message(message.clone()))
}

/// 列表和网格只构建视口内的行。还有下一页时，提示跟在卡片后面，不盖住最后一行。
fn file_list(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let rows = model.files.visible_rows(&ctx);
    let mode = model.files.display_mode;
    let picked = picked_paths(&model.files);
    let scroll = node_ref();
    // 网格不用虚拟列均分。均分会把短卡片那一行撑到页图的高度，标题就被顶出窗口。
    // 卡片按自己的宽度换行，页图不再独占一整行。
    let body = match mode {
        DisplayMode::List => virtual_rows(rows, mode, 72.0, picked, scroll),
        DisplayMode::Grid | DisplayMode::Adaptive | DisplayMode::Masonry => wrapping_row(rows, mode, picked),
    };
    let mut scroller = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
    }))
    .node_ref(scroll);
    if mode != DisplayMode::List {
        scroller = scroller.key("file-wrap-list");
    }
    scroller
        .children((
            // 列表空白要铺满滚动区。分页提示不在时，框选仍从卡片下面的空白开始。
            widget(
                Stack::column(8.0)
                    .width(LengthSpec::Fill)
                    .height(LengthSpec::Fill)
                    .min_width(LengthSpec::Px(0.0))
                    .min_height(LengthSpec::Px(0.0))
                    .grow(1.0)
                    .align(AlignSpec::Start),
            )
            .children((body, load_more(model))),
        ))
        .into_any()
}

/// 自适应和瀑布流：卡片按自己的宽度换行，不拉满整行。
fn wrapping_row(rows: Vec<FileRow>, mode: DisplayMode, picked: Vec<String>) -> AnyView {
    let cards: Vec<_> = rows
        .into_iter()
        .map(|row| {
            let selected = picked.iter().any(|item| item == &row.path);
            file_row(row, mode, selected)
        })
        .collect();
    widget(
        Stack::row(14.0)
            .wrap(true)
            .align(AlignSpec::Start)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children(cards)
    .into_any()
}

fn picked_paths(files: &super::files::FilesState) -> Vec<String> {
    let mut paths = files.selected.clone();
    if let Some(primary) = &files.primary {
        if !paths.iter().any(|item| item == primary) {
            paths.push(primary.clone());
        }
    }
    paths
}

/// 列表只构建视口内的行。滚动跟外层容器走，加载更多排在行后面。
fn virtual_rows(rows: Vec<FileRow>, mode: DisplayMode, estimate: f32, picked: Vec<String>, scroll: NodeRef) -> AnyView {
    each_virtual(signal(rows), |row| row.key(), estimate, move |row| {
        let selected = picked.iter().any(|item| item == &row.path);
        file_row(row, mode, selected)
    })
    .overscan(240.0)
    .within(scroll)
    .key("file-virtual-list")
    .into_any()
}

/// 一行文件：缩略图和标题。网格不把路径再画一遍。选中行用激活底。
fn file_row(row: FileRow, mode: DisplayMode, selected: bool) -> AnyView {
    let metrics = super::thumbs::thumb_box(mode, row.pixel_width, row.pixel_height);
    let path = row.path.clone();
    let menu_path = path.clone();
    let activate = widget(ListItem::new(row.name.clone()).detail(row_detail(&row, mode.is_list())).selected(selected))
        .key(view_key("file-row", &row))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(file_message(row_activation(&path)));
        })
        .on_cx(move |_, event: &SecondaryPress, cx| {
            cx.dispatch_program(file_message(FilesMessage::OpenEntryMenu {
                path: menu_path.clone(),
                x: event.x,
                y: event.y,
            }));
        });
    let marker = path_marker(&row.kind, &row.path);
    let palette = super::palette::swatches(&row.palette, &view_key("file-palette", &row));
    let thumb = thumbnail(&row, metrics.preview_width, metrics.preview_height, mode.is_list());
    let mut children = vec![thumb, activate.into_any(), marker];
    if let Some(palette) = palette {
        children.push(palette);
    }
    let frame = if mode.is_list() {
        let mut row = Stack::row(12.0)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(72.0))
            .radius(RadiusTier::Sm);
        if selected {
            row = row.surface(SemanticColorRole::Active);
        }
        row
    } else {
        let mut card = Stack::column(8.0)
            .padding_xy(8.0, 8.0)
            .radius(RadiusTier::Md)
            .grow(0.0)
            .shrink(0.0)
            .with_layout(|layout| {
                layout.align_self = Some(AlignSpec::Start);
            });
        if metrics.item_width > 0.0 {
            card = card.width(LengthSpec::Px(metrics.item_width)).max_width(metrics.item_width);
        }
        if metrics.item_height > 0.0 {
            card = card.min_height(LengthSpec::Px(metrics.item_height));
        }
        if selected {
            card = card.surface(SemanticColorRole::Active);
        }
        card
    };
    widget(frame).children(children).into_any()
}

/// 组装键不能带路径分隔符。`notes/page.pdf` 里的 `/` 会让整行挂载失败，格子里什么都没有。
fn view_key(prefix: &str, row: &FileRow) -> String {
    let body = row.key().replace(['/', '\\'], "\u{2215}");
    format!("{prefix}-{body}")
}

/// 行上的隐藏路径。命中按钮后沿父节点找到它，实况指针才能对上条目。
fn path_marker(kind: &str, path: &str) -> AnyView {
    let mut marker = nana_ui::runtime::Text::new(format!("momobako-entry:{kind}:{path}"));
    let layout = std::sync::Arc::make_mut(&mut marker.style.layout);
    layout.hidden = true;
    layout.height = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Px(0.0));
    widget(marker).into_any()
}

/// 预览盒按计算出的宽高固定。已有像素画进盒子；没有像素时只画类型图标，不画假照片。
fn thumbnail(row: &FileRow, width: f32, height: f32, list_mode: bool) -> AnyView {
    if let Some(view) = pixel_thumbnail(row, width, height, list_mode) {
        return view;
    }
    let ready = row.texture_ready.then(|| {
        row.thumbnail_path.as_deref().map(str::trim).filter(|path| !path.is_empty()).map(|path| {
            Thumbnail::new(super::thumbs::thumbnail_slot(path))
                .fit(ContentFit::Cover)
                .aspect(super::thumbs::content_aspect(row.pixel_width, row.pixel_height))
        })
    });
    if let Some(Some(mut thumb)) = ready {
        let layout = Arc::make_mut(&mut thumb.style.layout);
        layout.width = Some(LengthSpec::Px(width));
        layout.height = Some(LengthSpec::Px(height));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        thumb.style.radius = Some(if list_mode { RadiusTier::Sm } else { RadiusTier::Md });
        return widget(thumb).into_any();
    }
    let icon = if row.kind == "directory" { FOLDER_OPEN } else { FILE };
    let radius = if list_mode { RadiusTier::Sm } else { RadiusTier::Md };
    widget(
        Stack::column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Px(width))
            .height(LengthSpec::Px(height))
            .min_width(LengthSpec::Px(width))
            .min_height(LengthSpec::Px(height))
            .surface(SemanticColorRole::Subtle)
            .radius(radius),
    )
    .children((widget(IconGlyph::new(icon).size(28.0)),))
    .into_any()
}

/// 把已经缩小的像素画进预览盒。离屏不走宿主纹理槽，避免空槽被当成图标。
fn pixel_thumbnail(row: &FileRow, width: f32, height: f32, list_mode: bool) -> Option<AnyView> {
    let (pixel_w, pixel_h, rgba) = row.thumbnail_rgba.as_ref()?;
    rgba_preview(&view_key("file-thumb", row), width, height, *pixel_w, *pixel_h, rgba, list_mode)
}

/// 整页或缩略图像素按 contain 画进固定预览盒。盒子比页面宽时，左右留出页边。
pub(super) fn rgba_preview(
    key: impl Into<String>,
    width: f32,
    height: f32,
    pixel_w: u32,
    pixel_h: u32,
    rgba: &[u8],
    list_mode: bool,
) -> Option<AnyView> {
    let url = super::inspect::rgba_png_data_url(pixel_w, pixel_h, rgba)?;
    let image = nana_ui_core::BackgroundImage::Url {
        url,
        fit: nana_ui_core::BackgroundImageFit::Contain,
        size_width: None,
        size_height: None,
        position: nana_ui_core::BackgroundPosition::center(),
        repeat: nana_ui_core::BackgroundRepeat::NoRepeat,
        sampling: nana_ui_core::ImageSampling::Resample,
    };
    let radius = if list_mode { RadiusTier::Sm } else { RadiusTier::Md };
    let frame = Stack::column(0.0)
        .align(AlignSpec::Center)
        .justify(JustifySpec::Center)
        .width(LengthSpec::Px(width))
        .height(LengthSpec::Px(height))
        .min_width(LengthSpec::Px(width))
        .min_height(LengthSpec::Px(height))
        .surface(SemanticColorRole::Subtle)
        .radius(radius)
        .with_layout(|layout| {
            layout.paint.content_image = Some(image);
        });
    Some(widget(frame).key(key.into()).into_any())
}

/// 列表才写路径。网格、自适应和瀑布流只留标题；硬链接仍是次级。
fn row_detail(row: &FileRow, list_mode: bool) -> String {
    let hardlink = hardlink_label(row.hardlink_state.as_deref());
    if !list_mode || row.path.is_empty() {
        hardlink.to_string()
    } else if hardlink.is_empty() {
        row.path.clone()
    } else {
        format!("{} · {hardlink}", row.path)
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
        FileDialog::Hardlink => files.current_hardlink().is_some().then(|| hardlink_dialog(files, files.mutating)),
    }
}

fn name_dialog(title: &'static str, draft: &str, placeholder: &'static str, mutating: bool) -> AnyView {
    target_dialog(title, draft, placeholder, "确认", mutating)
}

fn target_dialog(title: &'static str, draft: &str, placeholder: &'static str, submit: &'static str, mutating: bool) -> AnyView {
    let submit_label = if mutating { "处理中..." } else { submit };
    let folder = title == "复制到文件夹" || title == "移动到文件夹";
    let label = if folder { "目标目录" } else { placeholder };
    let dialog = Dialog::new(title);
    let dialog = if folder { dialog } else { dialog.description(placeholder) };
    widget(dialog)
        .body(widget(TextInput::new(draft.to_string()).label(label).placeholder(placeholder)).on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program(file_message(FilesMessage::DraftChanged(event.value.to_string())));
        }))
        .footer(widget(Stack::row(8.0)).children((
            widget(super::workbench::ghost_button("取消")).key("file-dialog-cancel").disabled(mutating).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::CloseDialog));
            }),
            widget(super::workbench::primary_button(submit_label)).key("file-dialog-submit").disabled(mutating).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::SubmitDialog));
            }),
        )))
        .into_any()
}

fn hardlink_dialog(files: &super::files::FilesState, mutating: bool) -> AnyView {
    let confirm = if mutating { "处理中..." } else { "加入关联" };
    let prompt = files.current_hardlink();
    let message = prompt.map(|item| item.message()).unwrap_or_else(|| "内容哈希一致。确认后会将新文件加入硬链接关联。".into());
    let existing = prompt.map(|item| item.existing_path.clone()).unwrap_or_default();
    let new_path = prompt.map(|item| item.new_path.clone()).unwrap_or_default();
    widget(Dialog::new("加入硬链接关联"))
        .body(widget(Stack::column(6.0)).children((
            text(message).key("file-hardlink-message"),
            text(existing).key("file-hardlink-existing"),
            text(new_path).key("file-hardlink-new"),
        )))
        .footer(widget(Stack::row(8.0)).children((
            widget(super::workbench::ghost_button("跳过")).key("file-hardlink-skip").disabled(mutating).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::SkipHardlink));
            }),
            widget(super::workbench::primary_button(confirm)).key("file-hardlink-confirm").disabled(mutating).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(file_message(FilesMessage::ConfirmHardlink));
            }),
        )))
        .into_any()
}

fn file_message(message: FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

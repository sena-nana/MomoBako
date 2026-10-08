//! 文件卡片头部，对应 Vue `FileBrowserPanel.vue` 的 `files-browser__header`。
//!
//! 左边是眉题和面包屑药丸，右边是工具条：展示方式选择框、新建空文件输入、建文件和导入。
//! 导入菜单是锚在「导入」按钮下方、右对齐的浮层，Eagle 的复制 / 剪切导入在菜单里展开。
//! 左列按内容宽度排、不被工具条挤窄；工具条在剩余宽度里换行并右对齐。

use std::sync::Arc;

use nana_ui::icons_tabler::{FILE, FOLDER_OPEN, PLUS, ROTATE, TRASH};
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, JustifySpec, LengthSpec, PositionSpec, RadiusTier, Select, SelectChanged, SelectOption,
    SemanticColorRole, Stack, TextChanged, TextInput,
};

use super::super::files::{DisplayMode, FileContext, FileDialog, FilesMessage};
use super::super::sidebar::SidebarMessage;
use super::super::workspace::{LibraryCategory, WorkspacePanel};
use super::super::{ShellMessage, ShellViewModel};
use super::file_message;
use super::style::{self, ButtonLook};

/// 头部：左列位置、右侧工具条，下边一条发丝线。
pub(super) fn header(model: &ShellViewModel) -> AnyView {
    let row = Stack::bar(12.0)
        .align(AlignSpec::Start)
        .justify(JustifySpec::SpaceBetween)
        .with_layout(|layout| {
            layout.padding_top = Some(LengthSpec::Px(18.0));
            layout.padding_left = Some(LengthSpec::Px(20.0));
            layout.padding_right = Some(LengthSpec::Px(20.0));
            layout.padding_bottom = Some(LengthSpec::Px(14.0));
        });
    widget(style::bottom_rule(row))
        .children((location(model), toolbar(model)))
        .key("file-browser-header")
        .into_any()
}

/// 眉题、面包屑和分类视图的说明。按内容宽度排，不随工具条收窄。
fn location(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let eyebrow = if ctx.is_virtual() {
        "分类视图"
    } else if ctx.trash {
        "回收站"
    } else {
        "当前目录"
    };
    let subline = ctx.is_virtual().then(|| virtual_subline(model)).filter(|text| !text.is_empty());
    widget(Stack::column(8.0).width(LengthSpec::Shrink).grow(0.0).shrink(0.0))
        .children((
            widget(style::eyebrow(eyebrow)).key("file-location"),
            breadcrumbs(model, &ctx),
            subline.map(|text| widget(style::text(text, 14.0, 400, SemanticColorRole::Muted, 21.7)).key("file-location-subline")),
        ))
        .key("file-location-column")
        .into_any()
}

/// 根目录加上每一级路径，都是同一种白底药丸。分类视图只有一枚不可点的标题。
fn breadcrumbs(model: &ShellViewModel, ctx: &FileContext) -> AnyView {
    let mut crumbs = Vec::new();
    if ctx.is_virtual() {
        crumbs.push(crumb(virtual_title(model), "file-crumb-root", None));
    } else {
        let root = if ctx.trash { "回收站" } else { "根目录" };
        crumbs.push(crumb(root.into(), "file-crumb-root", Some(String::new())));
        let mut cursor = String::new();
        for segment in model.files.current_path.split('/').filter(|segment| !segment.is_empty()) {
            cursor = if cursor.is_empty() { segment.to_string() } else { format!("{cursor}/{segment}") };
            let key = format!("file-crumb-{}", cursor.replace('/', "\u{2215}"));
            crumbs.push(crumb(segment.to_string(), &key, Some(cursor.clone())));
        }
    }
    widget(Stack::row(8.0).wrap(true).align(AlignSpec::Center))
        .children(crumbs)
        .key("file-breadcrumbs")
        .into_any()
}

/// `files-breadcrumbs__item`：高 28、左右 10、白底、弱化色 14px/500，单行不折。
fn crumb(label: String, key: &str, target: Option<String>) -> AnyView {
    let look = ButtonLook {
        height: 28.0,
        background: Some(SemanticColorRole::Background),
        hover: Some(SemanticColorRole::Hover),
        foreground: SemanticColorRole::Muted,
        ..ButtonLook::TOOLBAR
    };
    let button = style::styled_button(label, None, look).disabled(target.is_none());
    let mut node = widget(button).key(key.to_string());
    if let Some(target) = target {
        node = node.on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(file_message(FilesMessage::OpenPath(target.clone()))));
    }
    node.into_any()
}

/// 智能文件夹名或分类名。
fn virtual_title(model: &ShellViewModel) -> String {
    if model.workspace.panel == WorkspacePanel::SmartFolder {
        let active = model.sidebar.active_smart_folder_id.as_deref();
        return model
            .sidebar
            .smart_folders
            .iter()
            .find(|folder| Some(folder.id.as_str()) == active)
            .map(|folder| folder.name.clone())
            .unwrap_or_else(|| "智能文件夹".into());
    }
    category_label(model.workspace.library_category).unwrap_or("分类视图").to_string()
}

/// 分类视图的条目说明，和 Vue `libraryCategorySummary` 一致。智能文件夹没有可读的筛选摘要时不写。
fn virtual_subline(model: &ShellViewModel) -> String {
    if model.workspace.panel == WorkspacePanel::SmartFolder {
        return String::new();
    }
    let ctx = FileContext::from_model(model);
    let count = model.files.visible_rows(&ctx).len();
    match model.workspace.library_category {
        LibraryCategory::All => String::new(),
        LibraryCategory::Recent => format!("按最近访问时间排序，共 {count} 项。"),
        category => format!("{}共 {count} 项。", category_label(category).unwrap_or("分类视图")),
    }
}

fn category_label(category: LibraryCategory) -> Option<&'static str> {
    match category {
        LibraryCategory::All => None,
        LibraryCategory::Uncategorized => Some("未分类"),
        LibraryCategory::Untagged => Some("未标签"),
        LibraryCategory::Recent => Some("最近使用"),
    }
}

/// 工具条：在剩余宽度里换行，每一行右对齐，行列间距都是 10。
fn toolbar(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let files = &model.files;
    let mut items = vec![display_mode_field(files.display_mode)];
    if model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == LibraryCategory::Recent {
        let enabled = model.sidebar.counts.recent > 0 && model.workspace.active_repo_id.is_some() && !model.navigation_locked();
        items.push(
            widget(style::styled_button("清空记录", Some(TRASH), ButtonLook::TOOLBAR).disabled(!enabled))
                .key("file-clear-recent")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Sidebar(SidebarMessage::ClearRecent)))
                .into_any(),
        );
    }
    if !ctx.trash && !ctx.is_virtual() {
        items.push(create_name_field(&files.create_name, files.mutating));
        items.push(tool_button("建文件", FILE, "file-create-file", files.can_create(&ctx), FilesMessage::SubmitCreateFile));
        items.push(import_anchor(files, &ctx));
    } else if ctx.trash {
        items.push(tool_button("还原所有项目", ROTATE, "file-restore-all", files.can_empty_trash(&ctx), FilesMessage::RestoreAll));
        let look = ButtonLook { foreground: SemanticColorRole::Danger, ..ButtonLook::TOOLBAR };
        items.push(
            widget(style::styled_button("清空回收站", Some(TRASH), look).disabled(!files.can_empty_trash(&ctx)))
                .key("file-empty-trash")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(file_message(FilesMessage::EmptyTrash)))
                .into_any(),
        );
    }
    widget(
        Stack::row(10.0)
            .wrap(true)
            .justify(JustifySpec::End)
            .align(AlignSpec::Center)
            .grow(1.0)
            .shrink(1.0)
            .min_width(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                layout.flex_basis = Some(LengthSpec::Px(0.0));
                layout.row_gap = Some(LengthSpec::Px(10.0));
            }),
    )
    .children(items)
    .key("file-toolbar")
    .into_any()
}

fn tool_button(label: &'static str, icon: nana_ui_core::Icon, key: &'static str, enabled: bool, message: FilesMessage) -> AnyView {
    widget(style::styled_button(label, Some(icon), ButtonLook::TOOLBAR).disabled(!enabled))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(file_message(message.clone())))
        .into_any()
}

/// `files-toolbar__select`：描边框里左边写「展示方式」，右边是当前值和下拉箭头，框宽 156、高 34。
///
/// Nana `Select` 总会画自己的底和边。这里让它四边各伸出 1px，放进一个裁剪层：边线被裁掉，
/// 底色和外框同为 `--bg`，看上去就是 Vue 里框中直接放下拉的样子；角色和键盘行为仍是下拉框。
fn display_mode_field(current: DisplayMode) -> AnyView {
    let options = [DisplayMode::Adaptive, DisplayMode::Masonry, DisplayMode::Grid, DisplayMode::List]
        .into_iter()
        .map(|mode| SelectOption::new(mode.storage_value(), mode.label()))
        .collect::<Vec<_>>();
    let mut select = Select::new(Some(current.storage_value().to_string())).options(options).placeholder("展示方式");
    select.style.foreground = Some(SemanticColorRole::Text);
    {
        let layout = Arc::make_mut(&mut select.style.layout);
        layout.height = Some(LengthSpec::Px(34.0));
        layout.min_height = Some(LengthSpec::Px(34.0));
        layout.width = Some(LengthSpec::CalcPercentOffset { percent: 100.0, offset_px: 2.0 });
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.margin_left = Some(LengthSpec::Px(-1.0));
        layout.margin_top = Some(LengthSpec::Px(-1.0));
        layout.font_size = Some(14.0);
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
    }
    let clip = Stack::row(0.0)
        .height(LengthSpec::Px(32.0))
        .min_width(LengthSpec::Px(0.0))
        .grow(1.0)
        .shrink(1.0)
        .with_layout(|layout| {
            layout.flex_basis = Some(LengthSpec::Px(0.0));
            // 选择框文字有 10px 内边距；往左让 1px 后，值正好落在标签后 8px 处。
            layout.margin_left = Some(LengthSpec::Px(-1.0));
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        });
    let frame = field_box(156.0, 34.0)
        .width(LengthSpec::Px(156.0))
        .gap(0.0)
        .with_layout(|layout| layout.padding_right = Some(LengthSpec::Px(0.0)));
    widget(frame)
        .children((
            widget(style::text("展示方式", 12.0, 400, SemanticColorRole::Muted, 18.6).nowrap(true)).key("file-display-label"),
            widget(clip).key("file-display-clip").children((widget(select).key("file-display-mode").on_cx(|_, event: &SelectChanged, cx| {
                cx.dispatch_program_all(file_message(FilesMessage::SetDisplayMode(DisplayMode::parse(&event.value))));
            }),)),
        ))
        .key("file-display-field")
        .into_any()
}

/// `files-toolbar__field`：加号图标和不带边的输入，外面一圈 `--border`，至少 220 宽。
/// 窄到放不下 220 时跟着工具条收窄，不压到左边的面包屑上。
fn create_name_field(draft: &str, mutating: bool) -> AnyView {
    let mut input = TextInput::new(draft.to_string()).placeholder("新建空文件，例如 note.txt").disabled(mutating);
    input.style.border = None;
    input.style.background = None;
    input.style.radius = None;
    input.style.control_height = None;
    input.style.control_padding_x = None;
    input.style.interaction.hovered.border = None;
    input.style.interaction.focused.border = None;
    input.style.interaction.hovered.background = None;
    input.style.interaction.focused.background = None;
    input.style.interaction.disabled.background = None;
    input.style.interaction.disabled.border = None;
    let layout = Arc::make_mut(&mut input.style.layout);
    layout.height = Some(LengthSpec::Px(32.0));
    layout.min_height = Some(LengthSpec::Px(32.0));
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    layout.font_size = Some(14.0);
    layout.border_width = Some(0.0);
    let frame = field_box(0.0, 34.0).width(LengthSpec::Px(222.0)).shrink(1.0);
    widget(frame)
        .children((
            widget(nana_ui::runtime::IconGlyph::new(PLUS).size(14.0).role(SemanticColorRole::Muted)).key("file-create-icon"),
            widget(input).key("file-create-name").on_cx(|_, event: &TextChanged, cx| {
                cx.dispatch_program_all(file_message(FilesMessage::SetCreateName(event.value.to_string())));
            }),
        ))
        .key("file-create-field")
        .into_any()
}

/// 工具条里的描边框：白底、`--border` 描边、`Lg` 圆角、左右 10、子项间距 8。
fn field_box(min_width: f32, height: f32) -> Stack {
    Stack::row(8.0)
        .align(AlignSpec::Center)
        .padding_xy(10.0, 0.0)
        .height(LengthSpec::Px(height))
        .min_height(LengthSpec::Px(height))
        .min_width(LengthSpec::Px(min_width))
        .grow(0.0)
        .shrink(0.0)
        .surface(SemanticColorRole::Background)
        .outline(SemanticColorRole::Border, 1.0)
        .radius(RadiusTier::Lg)
}

/// 「导入」和它的菜单。菜单相对这一格绝对定位：按钮下方 8px、右边对齐。
fn import_anchor(files: &super::super::files::FilesState, ctx: &FileContext) -> AnyView {
    let enabled = files.can_import(ctx);
    let button = widget(style::styled_button("导入", Some(FOLDER_OPEN), ButtonLook::TOOLBAR).disabled(!enabled))
        .key("file-import")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(file_message(FilesMessage::ToggleImportMenu)))
        .into_any();
    let open = files.import_open && enabled;
    let menu = open.then(|| import_menu(files));
    widget(Stack::row(0.0).grow(0.0).shrink(0.0).with_layout(|layout| layout.position = PositionSpec::Relative))
        .children((button, open.then(outside_closer), menu))
        .key("file-import-anchor")
        .into_any()
}

/// 菜单打开时铺满窗口的透明层：点在菜单外就收起，和 Vue 的全局 pointerdown 一样。
fn outside_closer() -> AnyView {
    let mut style = nana_ui::runtime::NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.position = PositionSpec::Fixed;
    layout.offset_top = Some(LengthSpec::Px(0.0));
    layout.offset_left = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Percent(100.0));
    layout.height = Some(LengthSpec::Percent(100.0));
    layout.z_index = Some(19);
    widget(nana_ui::runtime::ListItem::new("收起导入菜单").style(style))
        .content(widget(Stack::column(0.0)))
        .key("file-import-closer")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(file_message(FilesMessage::ToggleImportMenu)))
        .into_any()
}

/// `files-toolbar__menu`：至少 184 宽，内边距 8，项间距 6，圆角 12，主题浮层底和投影。
///
/// Vue 这里用了未定义的 `--panel-bg`，回退成 #15171a 深底、浅色主题下字看不清；这里按设计意图用浮层色。
fn import_menu(files: &super::super::files::FilesState) -> AnyView {
    let enabled = !files.mutating;
    let mut rows = vec![
        menu_item("从文件夹导入", "file-import-folder", enabled, FilesMessage::OpenDialog(FileDialog::Import)),
        menu_item("从 ZIP 导入", "file-import-zip", enabled, FilesMessage::OpenDialog(FileDialog::ImportArchive)),
        menu_item("从 Eagle 导入", "file-import-eagle", enabled, FilesMessage::ToggleEagleImport),
    ];
    if files.eagle_open {
        let sub = widget(Stack::column(6.0).with_layout(|layout| layout.padding_top = Some(LengthSpec::Px(2.0))))
            .children((
                menu_item("复制导入", "file-import-eagle-copy", enabled, FilesMessage::OpenEagle("copy".into())),
                menu_item("剪切导入", "file-import-eagle-move", enabled, FilesMessage::OpenEagle("move".into())),
            ))
            .key("file-import-eagle-actions")
            .into_any();
        rows.push(sub);
    }
    let surface = Stack::column(6.0)
        .width(LengthSpec::Shrink)
        .min_width(LengthSpec::Px(184.0))
        .padding(8.0)
        .surface(SemanticColorRole::Surface)
        .outline(SemanticColorRole::BorderSoft, 1.0)
        .radius(RadiusTier::Xl)
        .with_layout(|layout| {
            layout.position = PositionSpec::Absolute;
            layout.offset_top = Some(LengthSpec::Px(34.0 + 8.0));
            layout.offset_right = Some(LengthSpec::Px(0.0));
            layout.z_index = Some(20);
        });
    widget(style::with_shadows(surface, vec![style::shadow(16.0, 32.0, 0.0, 0.24)]))
        .children(rows)
        .key("file-import-menu")
        .into_any()
}

/// `files-toolbar__menu-item`：撑满菜单宽，内边距上下 10、左右 12，文字靠左，圆角 10，悬停 `--bg-hover`。
///
/// Vue 里 Lilia 的全局 `button { height: 32px }` 把它压成 32 高，文字溢出内容盒；
/// 这里按组件自己声明的内边距画：10 + 21.7 行高 + 10。
fn menu_item(label: &'static str, key: &'static str, enabled: bool, message: FilesMessage) -> AnyView {
    const PADDING_Y: f32 = 10.0;
    const LINE: f32 = 21.7;
    let mut style = nana_ui::runtime::NodeStyle::default();
    style.radius = Some(RadiusTier::Lg);
    style.foreground = Some(SemanticColorRole::Text);
    style.interaction.hovered = nana_ui::runtime::SemanticPaint { background: Some(SemanticColorRole::Hover), ..Default::default() };
    style.interaction.pressed = style.interaction.hovered;
    style.interaction.disabled = nana_ui::runtime::SemanticPaint::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.direction = Some(nana_ui_core::FlexDirection::Row);
    layout.align_items = AlignSpec::Center;
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.height = Some(LengthSpec::Px(PADDING_Y * 2.0 + LINE));
    layout.min_height = Some(LengthSpec::Px(PADDING_Y * 2.0 + LINE));
    layout.padding_top = Some(LengthSpec::Px(PADDING_Y));
    layout.padding_bottom = Some(LengthSpec::Px(PADDING_Y));
    layout.padding_left = Some(LengthSpec::Px(12.0));
    layout.padding_right = Some(LengthSpec::Px(12.0));
    layout.opacity = (!enabled).then_some(0.55);
    let mut node = widget(nana_ui::runtime::ListItem::new(label).disabled(!enabled).style(style))
        .content(widget(style::text(label, 14.0, 500, SemanticColorRole::Text, LINE).nowrap(true)))
        .key(key);
    if enabled {
        node = node.on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(file_message(message.clone())));
    }
    node.into_any()
}

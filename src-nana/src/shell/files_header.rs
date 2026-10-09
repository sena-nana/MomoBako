//! 文件卡片头部，对应 Vue `FileBrowserPanel.vue` 的 `files-browser__header`。
//!
//! 左边是眉题和面包屑药丸，右边是工具条：展示方式选择框、新建空文件输入、建文件和导入。
//! 导入菜单是锚在「导入」按钮下方、右对齐的浮层，Eagle 的复制 / 剪切导入在菜单里展开。
//! 左列按内容宽度排、不被工具条挤窄；工具条在剩余宽度里换行并右对齐。
//!
//! 常驻：头部只建一次。眉题、说明、展示方式和各按钮能不能点按字段绑定；面包屑是带键的 `each`，
//! 每一级按「路径 + 文字」认；只在某种面板出现的工具用 `.visible`（不占布局，不多一个间距）。
//! 新建文件名的输入框受控（`.model` 加 [`DraftField`]），处理器只发消息。

use std::sync::Arc;

use nana_ui::icons_tabler::{FILE, FOLDER_OPEN, PLUS, ROTATE, TRASH};
use nana_ui::runtime::view::fields::{self, HiddenWhenEmpty};
use nana_ui::runtime::view::{css, each, signal, widget, AnyView, IntoView, Signal};
use nana_ui::runtime::{
    Activate, AlignSpec, JustifySpec, LengthSpec, NodeStyle, PositionSpec, RadiusTier, Select, SelectChanged,
    SelectOption, SemanticColorRole, Stack, TextChanged, TextInput,
};

use super::super::files::{DisplayMode, FileContext, FileDialog, FilesMessage};
use super::super::sidebar::SidebarMessage;
use super::super::workspace::{LibraryCategory, WorkspacePanel};
use super::super::{ShellMessage, ShellViewModel};
use super::bind::{DraftField, StyleField};
use super::file_message;
use super::style::{self, ButtonLook};

/// 左列的文字：眉题和分类视图的说明（空时不占位）。
#[derive(Clone, Debug, PartialEq)]
struct HeaderText {
    eyebrow: &'static str,
    subline: String,
}

/// 一枚面包屑：文字、组装键，以及点了去哪个目录（分类视图的标题点不了）。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Crumb {
    label: String,
    key: String,
    target: Option<String>,
}

/// 工具条上按面板出现的工具和它们能不能点。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Tools {
    /// 最近使用分类里的「清空记录」，值是能不能点。
    clear_recent: Option<bool>,
    /// 目录视图的新建文件和导入。
    create: Option<CreateTools>,
    /// 回收站的还原所有项目和清空回收站，值是能不能点。
    trash: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CreateTools {
    mutating: bool,
    can_create: bool,
    can_import: bool,
    import_open: bool,
    eagle_open: bool,
}

impl Tools {
    fn create(&self) -> CreateTools {
        self.create.unwrap_or_default()
    }

    /// 导入菜单开着：展开了而且能导入。
    fn menu_open(&self) -> bool {
        self.create.is_some_and(|create| create.import_open && create.can_import)
    }
}

/// 头部的常驻信号。
#[derive(Clone, Copy)]
pub(super) struct HeaderSignals {
    text: Signal<HeaderText>,
    crumbs: Signal<Vec<Crumb>>,
    tools: Signal<Tools>,
    mode: Signal<DisplayMode>,
    create_name: DraftField,
}

impl HeaderSignals {
    /// 在常驻作用域里建信号，初值是这一刻的投影。
    pub(super) fn new(model: &ShellViewModel) -> Self {
        let ctx = FileContext::from_model(model);
        Self {
            text: signal(header_text(model, &ctx)),
            crumbs: signal(crumbs(model, &ctx)),
            tools: signal(tools(model, &ctx)),
            mode: signal(model.files.display_mode),
            create_name: DraftField::new(&model.files.create_name),
        }
    }

    /// 写入投影，只写变了的；新建文件名只在 ViewModel 的值变了时写回输入框。
    pub(super) fn write(&self, model: &ShellViewModel) {
        let ctx = FileContext::from_model(model);
        self.text.try_set_if_changed(header_text(model, &ctx));
        self.crumbs.try_set_if_changed(crumbs(model, &ctx));
        self.tools.try_set_if_changed(tools(model, &ctx));
        self.mode.try_set_if_changed(model.files.display_mode);
        self.create_name.sync(&model.files.create_name);
    }
}

fn header_text(model: &ShellViewModel, ctx: &FileContext) -> HeaderText {
    let eyebrow = if ctx.is_virtual() {
        "分类视图"
    } else if ctx.trash {
        "回收站"
    } else {
        "当前目录"
    };
    let subline = if ctx.is_virtual() { virtual_subline(model, ctx) } else { String::new() };
    HeaderText { eyebrow, subline }
}

/// 根目录加上每一级路径，都是同一种白底药丸。分类视图只有一枚不可点的标题。
fn crumbs(model: &ShellViewModel, ctx: &FileContext) -> Vec<Crumb> {
    if ctx.is_virtual() {
        return vec![Crumb { label: virtual_title(model), key: "file-crumb-root".into(), target: None }];
    }
    let root = if ctx.trash { "回收站" } else { "根目录" };
    let mut crumbs = vec![Crumb { label: root.into(), key: "file-crumb-root".into(), target: Some(String::new()) }];
    let mut cursor = String::new();
    for segment in model.files.current_path.split('/').filter(|segment| !segment.is_empty()) {
        cursor = if cursor.is_empty() { segment.to_string() } else { format!("{cursor}/{segment}") };
        let key = format!("file-crumb-{}", cursor.replace('/', "\u{2215}"));
        crumbs.push(Crumb { label: segment.to_string(), key, target: Some(cursor.clone()) });
    }
    crumbs
}

fn tools(model: &ShellViewModel, ctx: &FileContext) -> Tools {
    let files = &model.files;
    let recent = model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == LibraryCategory::Recent;
    let clear_recent = recent
        .then(|| model.sidebar.counts.recent > 0 && model.workspace.active_repo_id.is_some() && !model.navigation_locked());
    let create = (!ctx.trash && !ctx.is_virtual()).then(|| CreateTools {
        mutating: files.mutating,
        can_create: files.can_create(ctx),
        can_import: files.can_import(ctx),
        import_open: files.import_open,
        eagle_open: files.eagle_open,
    });
    let trash = ctx.trash.then(|| files.can_empty_trash(ctx));
    Tools { clear_recent, create, trash }
}

/// 头部：左列位置、右侧工具条，下边一条发丝线。
pub(super) fn header(signals: HeaderSignals) -> AnyView {
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
        .children((location(signals), toolbar(signals)))
        .key("file-browser-header")
        .into_any()
}

/// 眉题、面包屑和分类视图的说明。按内容宽度排，不随工具条收窄。
fn location(signals: HeaderSignals) -> AnyView {
    let text = signals.text;
    let initial = text.get_untracked();
    let breadcrumbs = each(signals.crumbs, Crumb::clone, crumb).horizontal(8.0).css(css! { flex-wrap: wrap; }).key("file-breadcrumbs");
    widget(Stack::column(8.0).width(LengthSpec::Shrink).grow(0.0).shrink(0.0))
        .children((
            widget(style::eyebrow(initial.eyebrow))
                .prop::<String, fields::text::value>(move || text.with(|text| text.eyebrow.to_string()))
                .key("file-location"),
            breadcrumbs,
            widget(style::text(initial.subline.clone(), 14.0, 400, SemanticColorRole::Muted, 21.7))
                .prop::<String, HiddenWhenEmpty<fields::text::value>>(move || text.with(|text| text.subline.clone()))
                .key("file-location-subline"),
        ))
        .key("file-location-column")
        .into_any()
}

/// `files-breadcrumbs__item`：高 28、左右 10、白底、弱化色 14px/500，单行不折。
fn crumb(crumb: Crumb) -> AnyView {
    let look = ButtonLook {
        height: 28.0,
        background: Some(SemanticColorRole::Background),
        hover: Some(SemanticColorRole::Hover),
        foreground: SemanticColorRole::Muted,
        ..ButtonLook::TOOLBAR
    };
    let button = style::styled_button(crumb.label, None, look).disabled(crumb.target.is_none());
    let mut node = widget(button).key(crumb.key);
    if let Some(target) = crumb.target {
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
fn virtual_subline(model: &ShellViewModel, ctx: &FileContext) -> String {
    if model.workspace.panel == WorkspacePanel::SmartFolder {
        return String::new();
    }
    let files = &model.files;
    let count = if ctx.smart_folder {
        files.virtual_rows.len()
    } else if ctx.category_virtual {
        files.rows.iter().filter(|row| row.kind != "directory").count()
    } else {
        files.rows.len()
    };
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

/// 工具条：在剩余宽度里换行，每一行右对齐，行列间距都是 10。只在某种面板出现的工具藏着时不占位。
fn toolbar(signals: HeaderSignals) -> AnyView {
    let tools = signals.tools;
    let shown = move |pick: fn(&Tools) -> bool| move || tools.with(pick);
    let clear = widget(style::styled_button("清空记录", Some(TRASH), ButtonLook::TOOLBAR))
        .visible(shown(|tools| tools.clear_recent.is_some()))
        .prop::<bool, fields::button::disabled>(move || tools.with(|tools| !tools.clear_recent.unwrap_or(false)))
        .key("file-clear-recent")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Sidebar(SidebarMessage::ClearRecent)));
    let trash_look = ButtonLook { foreground: SemanticColorRole::Danger, ..ButtonLook::TOOLBAR };
    let items = (
        display_mode_field(signals.mode),
        clear,
        create_name_field(signals),
        tool_button("建文件", FILE, "file-create-file", ButtonLook::TOOLBAR, tools, |tools| tools.create.map(|create| create.can_create), FilesMessage::SubmitCreateFile),
        import_anchor(tools),
        tool_button("还原所有项目", ROTATE, "file-restore-all", ButtonLook::TOOLBAR, tools, |tools| tools.trash, FilesMessage::RestoreAll),
        tool_button("清空回收站", TRASH, "file-empty-trash", trash_look, tools, |tools| tools.trash, FilesMessage::EmptyTrash),
    );
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

/// 只在某种面板出现的工具按钮：`state` 为 `None` 时藏着，否则是能不能点。
fn tool_button(
    label: &'static str,
    icon: nana_ui_core::Icon,
    key: &'static str,
    look: ButtonLook,
    tools: Signal<Tools>,
    state: fn(&Tools) -> Option<bool>,
    message: FilesMessage,
) -> AnyView {
    widget(style::styled_button(label, Some(icon), look))
        .visible(move || tools.with(|tools| state(tools).is_some()))
        .prop::<bool, fields::button::disabled>(move || tools.with(|tools| !state(tools).unwrap_or(false)))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(file_message(message.clone())))
        .into_any()
}

/// `files-toolbar__select`：描边框里左边写「展示方式」，右边是当前值和下拉箭头，框宽 156、高 34。
///
/// Nana `Select` 总会画自己的底和边。这里让它四边各伸出 1px，放进一个裁剪层：边线被裁掉，
/// 底色和外框同为 `--bg`，看上去就是 Vue 里框中直接放下拉的样子；角色和键盘行为仍是下拉框。
fn display_mode_field(mode: Signal<DisplayMode>) -> AnyView {
    let options = [DisplayMode::Adaptive, DisplayMode::Masonry, DisplayMode::Grid, DisplayMode::List]
        .into_iter()
        .map(|mode| SelectOption::new(mode.storage_value(), mode.label()))
        .collect::<Vec<_>>();
    let current = mode.get_untracked();
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
    let select = widget(select)
        .prop::<Option<Arc<str>>, fields::select::value>(move || Some(Arc::from(mode.with(|mode| mode.storage_value()))))
        .key("file-display-mode")
        .on_cx(|_, event: &SelectChanged, cx| {
            cx.dispatch_program_all(file_message(FilesMessage::SetDisplayMode(DisplayMode::parse(&event.value))));
        });
    widget(frame)
        .children((
            widget(style::text("展示方式", 12.0, 400, SemanticColorRole::Muted, 18.6).nowrap(true)).key("file-display-label"),
            widget(clip).key("file-display-clip").children((select,)),
        ))
        .key("file-display-field")
        .into_any()
}

/// `files-toolbar__field`：加号图标和不带边的输入，外面一圈 `--border`，至少 220 宽。
/// 窄到放不下 220 时跟着工具条收窄，不压到左边的面包屑上。
fn create_name_field(signals: HeaderSignals) -> AnyView {
    let tools = signals.tools;
    let draft = signals.create_name;
    let mutating = tools.with_untracked(|tools| tools.create().mutating);
    let mut input = TextInput::new(draft.current()).placeholder("新建空文件，例如 note.txt").disabled(mutating);
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
        .visible(move || tools.with(|tools| tools.create.is_some()))
        .children((
            widget(nana_ui::runtime::IconGlyph::new(PLUS).size(14.0).role(SemanticColorRole::Muted)).key("file-create-icon"),
            widget(input)
                .model(draft.signal())
                .prop::<bool, fields::text_input::disabled>(move || tools.with(|tools| tools.create().mutating))
                .key("file-create-name")
                .on_cx(|_, event: &TextChanged, cx| {
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

/// 「导入」和它的菜单。菜单相对这一格绝对定位：按钮下方 8px、右边对齐；开着时才露出菜单和收起层。
fn import_anchor(tools: Signal<Tools>) -> AnyView {
    let button = widget(style::styled_button("导入", Some(FOLDER_OPEN), ButtonLook::TOOLBAR))
        .prop::<bool, fields::button::disabled>(move || tools.with(|tools| !tools.create().can_import))
        .key("file-import")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(file_message(FilesMessage::ToggleImportMenu)))
        .into_any();
    widget(Stack::row(0.0).grow(0.0).shrink(0.0).with_layout(|layout| layout.position = PositionSpec::Relative))
        .visible(move || tools.with(|tools| tools.create.is_some()))
        .children((button, outside_closer(tools), import_menu(tools)))
        .key("file-import-anchor")
        .into_any()
}

/// 菜单打开时铺满窗口的透明层：点在菜单外就收起，和 Vue 的全局 pointerdown 一样。
fn outside_closer(tools: Signal<Tools>) -> AnyView {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.position = PositionSpec::Fixed;
    layout.offset_top = Some(LengthSpec::Px(0.0));
    layout.offset_left = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Percent(100.0));
    layout.height = Some(LengthSpec::Percent(100.0));
    layout.z_index = Some(19);
    widget(nana_ui::runtime::ListItem::new("收起导入菜单").style(style))
        .visible(move || tools.with(Tools::menu_open))
        .content(widget(Stack::column(0.0)))
        .key("file-import-closer")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(file_message(FilesMessage::ToggleImportMenu)))
        .into_any()
}

/// `files-toolbar__menu`：至少 184 宽，内边距 8，项间距 6，圆角 12，主题浮层底和投影。
///
/// Vue 这里用了未定义的 `--panel-bg`，回退成 #15171a 深底、浅色主题下字看不清；这里按设计意图用浮层色。
fn import_menu(tools: Signal<Tools>) -> AnyView {
    let rows = (
        menu_item("从文件夹导入", "file-import-folder", tools, FilesMessage::OpenDialog(FileDialog::Import)),
        menu_item("从 ZIP 导入", "file-import-zip", tools, FilesMessage::OpenDialog(FileDialog::ImportArchive)),
        menu_item("从 Eagle 导入", "file-import-eagle", tools, FilesMessage::ToggleEagleImport),
        widget(Stack::column(6.0).with_layout(|layout| layout.padding_top = Some(LengthSpec::Px(2.0))))
            .visible(move || tools.with(|tools| tools.create().eagle_open))
            .children((
                menu_item("复制导入", "file-import-eagle-copy", tools, FilesMessage::OpenEagle("copy".into())),
                menu_item("剪切导入", "file-import-eagle-move", tools, FilesMessage::OpenEagle("move".into())),
            ))
            .key("file-import-eagle-actions"),
    );
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
        .visible(move || tools.with(Tools::menu_open))
        .children(rows)
        .key("file-import-menu")
        .into_any()
}

/// `files-toolbar__menu-item`：撑满菜单宽，内边距上下 10、左右 12，文字靠左，圆角 10，悬停 `--bg-hover`。
/// 变更进行中时禁用并淡到 55%；点下时按当时的状态再判断一次，禁用时不发消息。
///
/// Vue 里 Lilia 的全局 `button { height: 32px }` 把它压成 32 高，文字溢出内容盒；
/// 这里按组件自己声明的内边距画：10 + 21.7 行高 + 10。
fn menu_item(label: &'static str, key: &'static str, tools: Signal<Tools>, message: FilesMessage) -> AnyView {
    const LINE: f32 = 21.7;
    let enabled = move || tools.with(|tools| !tools.create().mutating);
    let initial = tools.with_untracked(|tools| !tools.create().mutating);
    widget(nana_ui::runtime::ListItem::new(label).disabled(!initial).style(menu_item_style(initial)))
        .prop::<bool, fields::list_item::disabled>(move || !enabled())
        .prop::<NodeStyle, StyleField>(move || menu_item_style(enabled()))
        .content(widget(style::text(label, 14.0, 500, SemanticColorRole::Text, LINE).nowrap(true)))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            if tools.with_untracked(|tools| !tools.create().mutating) {
                cx.dispatch_program_all(file_message(message.clone()));
            }
        })
        .into_any()
}

fn menu_item_style(enabled: bool) -> NodeStyle {
    const PADDING_Y: f32 = 10.0;
    const LINE: f32 = 21.7;
    let mut style = NodeStyle::default();
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
    style
}

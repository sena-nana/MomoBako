//! 文件卡片头部。
//!
//! 左边是眉题和面包屑药丸，右边是工具条：展示方式选择框、新建空文件输入、建文件和导入。
//! 导入菜单是 NanaUI 的 `ActionMenu`，挂在「导入」下方、右对齐，Eagle 的复制 / 剪切导入在菜单里展开；
//! 开合、键盘和焦点都由框架负责。
//! 左列按内容宽度排、不被工具条挤窄；工具条在剩余宽度里换行并右对齐。
//!
//! 常驻：头部只建一次。眉题、说明、展示方式和各按钮能不能点按字段绑定；面包屑是带键的 `each`，
//! 每一级按「路径 + 文字」认；只在某种面板出现的工具用 `.visible`（不占布局，不多一个间距）。
//! 新建文件名的输入框受控（`.model` 加 [`DraftField`]），处理器只发消息。

use std::sync::Arc;

use nana_ui::icons_tabler::{CORNER_DOWN_RIGHT, FILE, FOLDER_OPEN, PLUS, ROTATE, TRASH};
use nana_ui::runtime::view::fields::{self, HiddenWhenEmpty};
use nana_ui::runtime::view::{
    AnyView, El, FieldWrite, IntoView, Signal, css, each, signal, widget,
};
use nana_ui::runtime::{
    ActionMenu, ActionMenuItem, Activate, AlignSpec, JustifySpec, LengthSpec, NodeStyle,
    PopoverAlignment, PopoverToggled, RadiusTier, Select, SelectChanged, SelectOption,
    SemanticColorRole, Stack, TextChanged, TextInput, Toolbar,
};

use super::super::files::{DisplayMode, FileContext, FileDialog, FilesMessage};
use super::super::sidebar::SidebarMessage;
use super::super::workspace::{LibraryCategory, WorkspacePanel};
use super::super::{ShellMessage, ShellViewModel};
use super::bind::DraftField;
use super::file_message;
use super::style::{self, ButtonLook, Height};
use crate::theme_map::tiers::{self, Edge};
use nana_ui_core::{ControlSize, SpacingStep, TextWeight};

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
    /// 最近使用分类里的「清空记录」。`None` 表示这一页不显示。
    clear_recent: Option<ClearRecent>,
    /// 目录视图的新建文件和导入。
    create: Option<CreateTools>,
    /// 回收站的还原所有项目和清空回收站，值是能不能点。
    trash: Option<bool>,
}

/// 最近使用的清空按钮。处理中写成「清空中...」并且不能再点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClearRecent {
    /// 有记录、有仓库、没锁、也没在清空时才能点。
    enabled: bool,
    /// 请求已经发出、结果还没回来。
    clearing: bool,
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
        self.create
            .is_some_and(|create| create.import_open && create.can_import)
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
    let subline = if ctx.is_virtual() {
        virtual_subline(model, ctx)
    } else {
        String::new()
    };
    HeaderText { eyebrow, subline }
}

/// 根目录加上每一级路径，都是同一种白底药丸。分类视图只有一枚不可点的标题。
fn crumbs(model: &ShellViewModel, ctx: &FileContext) -> Vec<Crumb> {
    if ctx.is_virtual() {
        return vec![Crumb {
            label: virtual_title(model),
            key: "file-crumb-root".into(),
            target: None,
        }];
    }
    let root = if ctx.trash { "回收站" } else { "根目录" };
    let mut crumbs = vec![Crumb {
        label: root.into(),
        key: "file-crumb-root".into(),
        target: Some(String::new()),
    }];
    let mut cursor = String::new();
    for segment in model
        .files
        .current_path
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        cursor = if cursor.is_empty() {
            segment.to_string()
        } else {
            format!("{cursor}/{segment}")
        };
        let key = format!("file-crumb-{}", cursor.replace('/', "\u{2215}"));
        crumbs.push(Crumb {
            label: segment.to_string(),
            key,
            target: Some(cursor.clone()),
        });
    }
    crumbs
}

fn tools(model: &ShellViewModel, ctx: &FileContext) -> Tools {
    let files = &model.files;
    let recent = model.workspace.panel == WorkspacePanel::Files
        && model.workspace.library_category == LibraryCategory::Recent;
    let clear_recent = recent.then(|| {
        let clearing = model.sidebar.clearing_recent;
        ClearRecent {
            enabled: model.sidebar.counts.recent > 0
                && model.workspace.active_repo_id.is_some()
                && !model.navigation_locked()
                && !clearing,
            clearing,
        }
    });
    let create = (!ctx.trash && !ctx.is_virtual()).then(|| CreateTools {
        mutating: files.mutating,
        can_create: files.can_create(ctx),
        can_import: files.can_import(ctx),
        import_open: files.import_open,
        eagle_open: files.eagle_open,
    });
    let trash = ctx.trash.then(|| files.can_empty_trash(ctx));
    Tools {
        clear_recent,
        create,
        trash,
    }
}

/// 头部：左列位置、右侧工具条，下边一条发丝线。上、左、右留 `PageTight`，下留 `Xxl`。
pub(super) fn header(signals: HeaderSignals) -> AnyView {
    let row = Stack::bar(0.0)
        .gap_theme(SpacingStep::Xl)
        .align(AlignSpec::Start)
        .justify(JustifySpec::SpaceBetween)
        .with_layout(|layout| {
            tiers::padding(
                layout,
                &[Edge::Top, Edge::Left, Edge::Right],
                SpacingStep::PageTight,
            );
            tiers::padding(layout, &[Edge::Bottom], SpacingStep::Xxl);
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
    let breadcrumbs = each(signals.crumbs, Crumb::clone, crumb)
        .horizontal(tiers::space_px(SpacingStep::Md))
        .css(css! { flex-wrap: wrap; gap: theme(space-md); })
        .key("file-breadcrumbs");
    widget(
        style::column(SpacingStep::Md)
            .width(LengthSpec::Shrink)
            .grow(0.0)
            .shrink(0.0),
    )
    .children((
        widget(style::eyebrow(initial.eyebrow))
            .prop::<String, fields::text::value>(move || text.with(|text| text.eyebrow.to_string()))
            .key("file-location"),
        breadcrumbs,
        widget(style::text(
            initial.subline.clone(),
            14.0,
            TextWeight::Regular,
            SemanticColorRole::Muted,
            21.7,
        ))
        .prop::<String, HiddenWhenEmpty<fields::text::value>>(move || {
            text.with(|text| text.subline.clone())
        })
        .key("file-location-subline"),
    ))
    .key("file-location-column")
    .into_any()
}

/// 面包屑项：小号控件高、左右取控件内边距档、白底、弱化色 14px/500，单行不折。
fn crumb(crumb: Crumb) -> AnyView {
    let look = ButtonLook {
        height: Height::Control(ControlSize::Small),
        background: Some(SemanticColorRole::Background),
        hover: Some(SemanticColorRole::Hover),
        foreground: SemanticColorRole::Muted,
        ..ButtonLook::TOOLBAR
    };
    let button = style::styled_button(crumb.label, None, look).disabled(crumb.target.is_none());
    let mut node = widget(button).key(crumb.key);
    if let Some(target) = crumb.target {
        node = node.on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(file_message(FilesMessage::OpenPath(target.clone())))
        });
    }
    node.into_any()
}

/// 智能文件夹名（子级也认得，见 `SidebarState::smart_title`）或分类名。
fn virtual_title(model: &ShellViewModel) -> String {
    if model.workspace.panel == WorkspacePanel::SmartFolder {
        return model.sidebar.smart_title();
    }
    category_label(model.workspace.library_category)
        .unwrap_or("分类视图")
        .to_string()
}

/// 虚拟视图的说明行：智能文件夹是查询结果的摘要（查回来以前为空），分类视图是条目说明。
fn virtual_subline(model: &ShellViewModel, ctx: &FileContext) -> String {
    if model.workspace.panel == WorkspacePanel::SmartFolder {
        return model.sidebar.smart_summary();
    }
    let files = &model.files;
    let count = if ctx.category_virtual {
        files.category_rows.len()
    } else {
        files.rows.len()
    };
    match model.workspace.library_category {
        LibraryCategory::All => String::new(),
        LibraryCategory::Recent => format!("按最近访问时间排序，共 {count} 项。"),
        category => format!(
            "{}共 {count} 项。",
            category_label(category).unwrap_or("分类视图")
        ),
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

/// 工具条：在剩余宽度里换行，每一行右对齐，行列间距都是 `Lg`。只在某种面板出现的工具藏着时不占位。
fn toolbar(signals: HeaderSignals) -> AnyView {
    let tools = signals.tools;
    let shown = move |pick: fn(&Tools) -> bool| move || tools.with(pick);
    // 有访问记录才能点；请求在途时改成「清空中...」并禁用，图标仍是垃圾桶。
    let clear_label = move || {
        if tools.with(|tools| tools.clear_recent.is_some_and(|item| item.clearing)) {
            "清空中...".to_string()
        } else {
            "清空记录".to_string()
        }
    };
    let clear = widget(style::styled_button(
        clear_label(),
        Some(TRASH),
        ButtonLook::TOOLBAR,
    ))
    .visible(shown(|tools| tools.clear_recent.is_some()))
    .prop::<bool, fields::button::disabled>(move || {
        tools.with(|tools| !tools.clear_recent.is_some_and(|item| item.enabled))
    })
    .prop::<String, fields::button::label>(clear_label)
    .key("file-clear-recent")
    .on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program_all(ShellMessage::Sidebar(SidebarMessage::ClearRecent))
    });
    let trash_look = ButtonLook {
        foreground: SemanticColorRole::Danger,
        ..ButtonLook::TOOLBAR
    };
    let items = (
        display_mode_field(signals.mode),
        clear,
        create_name_field(signals),
        tool_button(
            "建文件",
            FILE,
            "file-create-file",
            ButtonLook::TOOLBAR,
            tools,
            |tools| tools.create.map(|create| create.can_create),
            FilesMessage::SubmitCreateFile,
        ),
        import_anchor(tools),
        tool_button(
            "还原所有项目",
            ROTATE,
            "file-restore-all",
            ButtonLook::TOOLBAR,
            tools,
            |tools| tools.trash,
            FilesMessage::RestoreAll,
        ),
        tool_button(
            "清空回收站",
            TRASH,
            "file-empty-trash",
            trash_look,
            tools,
            |tools| tools.trash,
            FilesMessage::EmptyTrash,
        ),
    );
    // NanaUI 的工具栏：读屏把这一排念成一组。父级卡片已经有底，不画工具栏自己的表面，
    // 排法照原来的折行行。行里有下拉框和输入框，方向键留给它们，不做方向键焦点组。
    let row = style::row(SpacingStep::Lg)
        .wrap(true)
        .justify(JustifySpec::End)
        .align(AlignSpec::Center)
        .grow(1.0)
        .shrink(1.0)
        .min_width(LengthSpec::Px(0.0))
        .with_layout(|layout| {
            layout.flex_basis = Some(LengthSpec::Px(0.0));
            tiers::row_gap(layout, SpacingStep::Lg);
        });
    widget(
        Toolbar::new()
            .chrome(false)
            .label("文件工具")
            .style(row.node_style()),
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
        .prop::<bool, fields::button::disabled>(move || {
            tools.with(|tools| !state(tools).unwrap_or(false))
        })
        .key(key)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(file_message(message.clone())))
        .into_any()
}

/// 展示方式选择框：描边框里左边写「展示方式」，右边是当前值和下拉箭头，框宽 156、高 34。
///
/// Nana `Select` 总会画自己的底和边。这里让它四边各伸出一根发丝边，放进一个裁剪层：边线被裁掉，
/// 底色和外框同为 `--bg`，看上去就是框中直接放下拉的样子；角色和键盘行为仍是下拉框。
fn display_mode_field(mode: Signal<DisplayMode>) -> AnyView {
    let options = [
        DisplayMode::Adaptive,
        DisplayMode::Masonry,
        DisplayMode::Grid,
        DisplayMode::List,
    ]
    .into_iter()
    .map(|mode| SelectOption::new(mode.storage_value(), mode.label()))
    .collect::<Vec<_>>();
    let current = mode.get_untracked();
    let mut select = Select::new(Some(current.storage_value().to_string()))
        .options(options)
        .placeholder("展示方式");
    select.style.foreground = Some(SemanticColorRole::Text);
    let hairline = tiers::hairline_px();
    {
        let layout = Arc::make_mut(&mut select.style.layout);
        layout.height = Some(LengthSpec::Px(style::TOOLBAR_HEIGHT));
        layout.min_height = Some(LengthSpec::Px(style::TOOLBAR_HEIGHT));
        layout.width = Some(LengthSpec::percent_offset(100.0, hairline * 2.0));
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.margin_left = Some(LengthSpec::Px(-hairline));
        layout.margin_top = Some(LengthSpec::Px(-hairline));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
    }
    let clip = Stack::row(0.0)
        .height(LengthSpec::Px(style::TOOLBAR_HEIGHT - hairline * 2.0))
        .min_width(LengthSpec::Px(0.0))
        .grow(1.0)
        .shrink(1.0)
        .with_layout(|layout| {
            layout.flex_basis = Some(LengthSpec::Px(0.0));
            // 选择框文字有 `Lg` 内边距；往左让一根发丝边后，值正好落在标签后 `Md` 处。
            layout.margin_left = Some(LengthSpec::Px(-hairline));
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        });
    let frame = field_box(DISPLAY_FIELD_WIDTH)
        .width(LengthSpec::Px(DISPLAY_FIELD_WIDTH))
        .gap(0.0)
        .with_layout(|layout| {
            layout.padding_right = Some(LengthSpec::Px(0.0));
            layout.theme.padding_right = None;
        });
    let select = widget(select)
        .prop::<Option<Arc<str>>, fields::select::value>(move || {
            Some(Arc::from(mode.with(|mode| mode.storage_value())))
        })
        .key("file-display-mode")
        .on_cx(|_, event: &SelectChanged, cx| {
            cx.dispatch_program_all(file_message(FilesMessage::SetDisplayMode(
                DisplayMode::parse(&event.value),
            )));
        });
    widget(frame)
        .children((
            widget(
                style::text(
                    "展示方式",
                    12.0,
                    TextWeight::Regular,
                    SemanticColorRole::Muted,
                    18.6,
                )
                .nowrap(true),
            )
            .key("file-display-label"),
            widget(clip).key("file-display-clip").children((select,)),
        ))
        .key("file-display-field")
        .into_any()
}

/// 新建文件名输入框：加号图标和不带边的输入，外面一圈 `--border`，至少 220 宽。
/// 窄到放不下 220 时跟着工具条收窄，不压到左边的面包屑上。
///
/// 变更进行中只禁用「建文件」，这个输入框仍可改：下一份名字可以先写着。
/// 建成成功才清空，移动、导入这些变更不清。
fn create_name_field(signals: HeaderSignals) -> AnyView {
    let tools = signals.tools;
    let draft = signals.create_name;
    let mut input = TextInput::new(draft.current()).placeholder("新建空文件，例如 note.txt");
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
    let height = style::TOOLBAR_HEIGHT - tiers::hairline_px() * 2.0;
    layout.height = Some(LengthSpec::Px(height));
    layout.min_height = Some(LengthSpec::Px(height));
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    layout.border_width = Some(0.0);
    layout.theme.border_width = None;
    let frame = field_box(0.0)
        .width(LengthSpec::Px(CREATE_FIELD_WIDTH))
        .shrink(1.0);
    widget(frame)
        .visible(move || tools.with(|tools| tools.create.is_some()))
        .children((
            widget(
                nana_ui::runtime::IconGlyph::new(PLUS)
                    .size(style::TOOLBAR_ICON)
                    .role(SemanticColorRole::Muted),
            )
            .key("file-create-icon"),
            widget(input)
                .model(draft.signal())
                .key("file-create-name")
                .on_cx(|_, event: &TextChanged, cx| {
                    cx.dispatch_program_all(file_message(FilesMessage::SetCreateName(
                        event.value.to_string(),
                    )));
                }),
        ))
        .key("file-create-field")
        .into_any()
}

/// 展示方式框的宽度。
const DISPLAY_FIELD_WIDTH: f32 = 156.0;
/// 新建文件名框的宽度：输入区 220 加两根发丝边。
const CREATE_FIELD_WIDTH: f32 = 222.0;

/// 工具条里的描边框：白底、`--border` 发丝边、`Lg` 圆角、左右 `Lg`、子项间距 `Md`，和工具栏按钮一样高。
fn field_box(min_width: f32) -> Stack {
    let height = style::TOOLBAR_HEIGHT;
    style::hairline(
        style::row(SpacingStep::Md)
            .align(AlignSpec::Center)
            .with_layout(|layout| {
                tiers::padding(layout, &Edge::X, SpacingStep::Lg);
                layout.padding_top = Some(LengthSpec::Px(0.0));
                layout.padding_bottom = Some(LengthSpec::Px(0.0));
            })
            .height(LengthSpec::Px(height))
            .min_height(LengthSpec::Px(height))
            .min_width(LengthSpec::Px(min_width))
            .grow(0.0)
            .shrink(0.0)
            .surface(SemanticColorRole::Background),
        SemanticColorRole::Border,
    )
    .radius(RadiusTier::Lg)
}

/// 「导入」和它的菜单：NanaUI `ActionMenu`。菜单挂在触发器下方、右边对齐，开合、点外面和 Escape 收起、
/// 方向键在条目间走、关上后焦点回到触发器都由框架负责；框架开合时发 [`PopoverToggled`]，这里换成
/// `ImportMenuToggled` 写回 ViewModel，ViewModel 关掉菜单（提交了导入、变更开始）时再经 `open` 写回菜单。
/// 「从 Eagle 导入」点了不收起菜单，只展开下面的复制导入和剪切导入。
fn import_anchor(tools: Signal<Tools>) -> AnyView {
    let mut menu = ActionMenu::new()
        .trigger("导入")
        .bare_trigger(true)
        .width(184.0)
        .trigger_style(import_trigger_style());
    menu.popover.alignment = PopoverAlignment::End;
    let menu = widget(menu)
        .prop::<bool, fields::action_menu::open>(move || tools.with(Tools::menu_open))
        .prop::<bool, ImportDisabled>(move || tools.with(|tools| !tools.create().can_import))
        .trigger(import_trigger())
        .children((
            menu_item(
                ActionMenuItem::new("从文件夹导入"),
                "file-import-folder",
                tools,
                FilesMessage::OpenDialog(FileDialog::Import),
            ),
            menu_item(
                ActionMenuItem::new("从 ZIP 导入"),
                "file-import-zip",
                tools,
                FilesMessage::OpenDialog(FileDialog::ImportArchive),
            ),
            menu_item(
                ActionMenuItem::new("从 Eagle 导入").keep_open(true),
                "file-import-eagle",
                tools,
                FilesMessage::ToggleEagleImport,
            ),
            eagle_item(tools, "复制导入", "file-import-eagle-copy", "copy"),
            eagle_item(tools, "剪切导入", "file-import-eagle-move", "move"),
        ))
        .key("file-import")
        .on_cx(|_, toggled: &PopoverToggled, cx| {
            cx.dispatch_program_all(file_message(FilesMessage::ImportMenuToggled(toggled.open)))
        });
    widget(Stack::row(0.0).grow(0.0).shrink(0.0))
        .visible(move || tools.with(|tools| tools.create.is_some()))
        .children((menu,))
        .key("file-import-anchor")
        .into_any()
}

/// 「导入」触发器上画的内容：文件夹图标加文字，和工具条上别的按钮同一个样子。
fn import_trigger() -> AnyView {
    widget(style::row(ButtonLook::TOOLBAR.gap).align(AlignSpec::Center))
        .children((
            widget(
                nana_ui::runtime::IconGlyph::new(FOLDER_OPEN)
                    .size(ButtonLook::TOOLBAR.icon_size)
                    .role(SemanticColorRole::Text),
            ),
            widget(
                style::text(
                    "导入",
                    14.0,
                    TextWeight::Medium,
                    SemanticColorRole::Text,
                    21.7,
                )
                .nowrap(true),
            ),
        ))
        .key("file-import-trigger")
        .into_any()
}

/// 触发器的尺寸和圆角照工具条按钮：高 34、左右取控件内边距档、`Sm` 圆角，悬停 `--bg-hover`。
fn import_trigger_style() -> NodeStyle {
    style::styled_button("导入", Some(FOLDER_OPEN), ButtonLook::TOOLBAR).style
}

/// 菜单能不能打开：当前视图不能导入时禁用触发器。
struct ImportDisabled;

impl FieldWrite<ActionMenu, bool> for ImportDisabled {
    const FIELD: &'static str = "ActionMenu.disabled";

    fn write(target: &mut ActionMenu, disabled: bool) {
        *target = std::mem::take(target).disabled(disabled);
    }

    fn differs(target: &ActionMenu, disabled: &bool) -> bool {
        target.disabled != *disabled
    }
}

/// 导入菜单的一项。变更进行中时禁用；点下时按当时的状态再判断一次，禁用时不发消息。
/// `item` 带着文字、图标和点了收不收起菜单。
fn menu_item(
    item: ActionMenuItem,
    key: &'static str,
    tools: Signal<Tools>,
    message: FilesMessage,
) -> El<ActionMenuItem> {
    widget(item)
        .prop::<bool, fields::action_menu_item::disabled>(move || {
            tools.with(|tools| tools.create().mutating)
        })
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            if tools.with_untracked(|tools| !tools.create().mutating) {
                cx.dispatch_program_all(file_message(message.clone()));
            }
        })
}

/// Eagle 的复制导入或剪切导入：「从 Eagle 导入」展开时才出现，前面的拐角图标表示它们属于上一项。
fn eagle_item(
    tools: Signal<Tools>,
    label: &'static str,
    key: &'static str,
    mode: &'static str,
) -> AnyView {
    menu_item(
        ActionMenuItem::new(label).leading(CORNER_DOWN_RIGHT),
        key,
        tools,
        FilesMessage::OpenEagle(mode.into()),
    )
    .visible(move || tools.with(|tools| tools.create().eagle_open))
    .into_any()
}

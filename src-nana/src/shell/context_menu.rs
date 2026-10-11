//! 右键菜单的共用视图：文件条目和侧栏文件夹行的右键菜单都从这里建，对应 Vue `ContextMenuHost.vue`
//! （两处在 Vue 里也是同一个 `v-context-menu` 宿主）。
//!
//! 外观照 `.ctx-menu`：抬升底、`--border-strong` 描边、`Md` 圆角、内边距 4、行间距 1，
//! 行高 28、13px/500、13px 图标；分组带 ›，子菜单贴在这一行右边 4px。
//! 子菜单照 `.ctx-menu__slot:hover` / `:focus-within`：指针或键盘焦点进到分组那一行（含子菜单）
//! 就展开，都离开就收起，没有延迟；点分组本身不收起（按下时焦点落在分组上）。禁用的分组不展开。
//! 菜单按指针落点放置，靠右或靠下放不下时夹回窗口内 4px。
//! 菜单和每个子菜单各是一个方向键焦点组：上下键在本层的项之间走、首尾相接，Tab 从分组走进子菜单；
//! 带子菜单的分组照 Vue 报成菜单按钮（`aria-haspopup="menu"`）。菜单键和 Shift+F10 打开的菜单，
//! 焦点落在首项（`menu_focus.rs`），Enter 执行，Escape 由全局 Escape 关掉。
//! 要点两次的项第一次只把这一行变成待确认。点菜单外关闭。
//! 不能用的项禁用，后面可以跟一行弱色小字写原因（不是菜单项，不能点、不能聚焦）。
//!
//! 菜单常驻：同一个目标、同一个落点的菜单打开期间，条目按内容做键，变了的那一行才重建；
//! 展开哪个子菜单只切换子菜单的显隐。点条目发的消息是条目数据的一部分（[`MenuAction`]），
//! 条目变了行就重建，处理器不会拿着过期的值。

use std::sync::Arc;

use nana_ui::icons_tabler::CHECK;
use nana_ui::runtime::view::{computed, each, widget, AnyView, FieldWrite, IntoView, Signal, StyledComponent};
use nana_ui::runtime::{
    Activate, AlignSpec, FocusWithinChanged, IconGlyph, LengthSpec, ListItem, NodeStyle, PointerHoverChanged, PositionSpec,
    RadiusTier, SecondaryPress, SemanticColorRole, SemanticPaint, Stack,
};
use nana_ui_core::{Icon, LengthAtom, SemanticColorMix};

use super::admin::AdminMessage;
use super::files::{BranchSource, FilesMessage};
use super::files_view::style;
use super::input::InputMessage;
use super::menu_focus::MENU_KEYS;
use super::player::PlayerMessage;
use super::sidebar::GapMessage;
use super::{ShellMessage, SidebarMessage};

/// `.ctx-menu` 的最小宽度、内边距、行高和行间距。
const MENU_MIN_WIDTH: f32 = 180.0;
const MENU_PADDING: f32 = 4.0;
const ITEM_HEIGHT: f32 = 28.0;
const ITEM_GAP: f32 = 1.0;
/// 菜单离窗口边至少 4px（`SB_MENU_EDGE_PADDING`）。
const EDGE: f32 = 4.0;
/// 子菜单离这一行右缘的距离（`.ctx-menu__submenu { left: calc(100% + 4px) }`）。
const SUBMENU_GAP: f32 = 4.0;

/// 点菜单项时发的消息，存成数据：能比较（按调试文本），条目变了就知道。`ShellMessage` 不能克隆，
/// 每次点击现做一条。
#[derive(Clone, Debug)]
pub(crate) enum MenuAction {
    Files(FilesMessage),
    Input(InputMessage),
    Admin(AdminMessage),
    /// 切换播放列表成员。
    Membership { playlist_id: String, kind: String, extension: String, asset_id: String, is_virtual: bool, path: String },
    /// 打开侧栏里的文件夹。
    OpenFolder(String),
    /// 侧栏文件夹的新建、重命名和删除。
    Gap(GapMessage),
}

impl MenuAction {
    /// 点这一项时发的消息。
    pub(crate) fn message(&self) -> ShellMessage {
        match self.clone() {
            Self::Files(message) => ShellMessage::Files(message),
            Self::Input(message) => ShellMessage::Input(message),
            Self::Admin(message) => ShellMessage::Admin(message),
            Self::Membership { playlist_id, kind, extension, asset_id, is_virtual, path } => {
                ShellMessage::Player(PlayerMessage::ToggleMembership { playlist_id, kind, extension, asset_id, is_virtual, path })
            }
            Self::OpenFolder(path) => ShellMessage::Sidebar(SidebarMessage::OpenFolder(path)),
            Self::Gap(message) => ShellMessage::Sidebar(SidebarMessage::Gap(message)),
        }
    }
}

impl PartialEq for MenuAction {
    fn eq(&self, other: &Self) -> bool {
        format!("{self:?}") == format!("{other:?}")
    }
}

/// 菜单里的一行。`children` 非空时是带 › 的分组，悬停或聚焦时展开子菜单，点它不做别的。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MenuEntry {
    pub id: String,
    pub label: String,
    pub icon: Option<Icon>,
    pub checked: bool,
    pub disabled: bool,
    pub danger: bool,
    /// 二次确认时第一次点击后显示的文案。
    pub confirm_label: Option<&'static str>,
    /// 已经点过一次、等第二次确认。
    pub pending: bool,
    /// 不是菜单项，是写在禁用项后面的一行原因：弱色 12px，不能点、不能聚焦。
    pub note: bool,
    pub action: Option<MenuAction>,
    pub children: Vec<MenuEntry>,
}

impl MenuEntry {
    pub(crate) fn new(id: &str, label: impl Into<String>, icon: Option<Icon>) -> Self {
        Self {
            id: id.to_string(),
            label: label.into(),
            icon,
            checked: false,
            disabled: false,
            danger: false,
            confirm_label: None,
            pending: false,
            note: false,
            action: None,
            children: Vec::new(),
        }
    }

    /// 写在禁用项后面的一行原因。
    pub(crate) fn note(id: &str, text: &str) -> Self {
        Self { note: true, ..Self::new(id, text, None) }
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 危险项：错误色，悬停浅红底。
    pub(crate) fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub(crate) fn message(mut self, action: MenuAction) -> Self {
        self.action = Some(action);
        self
    }
}

/// 右键菜单要显示的东西：条目、展开的子菜单和菜单尺寸。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MenuView {
    pub entries: Vec<MenuEntry>,
    /// 展开着的分组。
    pub branch: Option<String>,
    pub width: f32,
    pub height: f32,
}

impl MenuView {
    /// 按条目算出菜单尺寸。
    pub(crate) fn new(entries: Vec<MenuEntry>, branch: Option<String>) -> Self {
        Self { width: menu_width(&entries), height: menu_height(entries.len()), branch, entries }
    }
}

/// 一种右键菜单的外壳：键的前缀和它发的几条消息。
#[derive(Clone, Copy)]
pub(crate) struct MenuChrome {
    /// 键的前缀：菜单层 `{prefix}-context-layer`、透明底 `{prefix}-context-backdrop`、菜单
    /// `{prefix}-context-menu`（方向键焦点组，`menu_focus.rs` 按它找）、每一项 `{prefix}-menu-{条目}`。
    pub prefix: &'static str,
    /// 点菜单外面、选了一项以后关菜单。
    pub close: fn() -> ShellMessage,
    /// 指针或焦点进出带子菜单的分组。没有分组的菜单为 `None`。
    pub branch: Option<fn(String, BranchSource, bool) -> ShellMessage>,
    /// 要点两次的项第一次被点。没有这种项的菜单为 `None`，这样的项点一次就执行。
    pub arm: Option<fn(String) -> ShellMessage>,
}

/// 落点 `(x, y)` 的右键菜单。`view` 由调用方在浮层的挂载作用域里建好并登记投影。
pub(crate) fn context_menu(view: Signal<MenuView>, x: f32, y: f32, chrome: MenuChrome) -> AnyView {
    let entries = computed(move || view.with(|view| view.entries.clone()));
    let rows = each(entries, |entry: &MenuEntry| format!("{entry:?}"), move |entry: MenuEntry| slot(view, entry, chrome)).gap(ITEM_GAP);
    // 菜单这一层铺满浮层层（浮层层铺满窗口），透明底和菜单都相对它绝对定位，画在浮层层之上、
    // 按先后叠放，不用给层级。叠层默认不接指针（给弹幕、HUD 用），这里清掉，让透明底和菜单接点击。
    let surface = Stack::column(0.0)
        .padding(MENU_PADDING)
        .surface(SemanticColorRole::Surface)
        .outline(SemanticColorRole::BorderStrong, 1.0)
        .radius(RadiusTier::Md)
        .with_layout(move |layout| layout.position = PositionSpec::Absolute);
    let surface = style::with_shadows(surface, vec![style::shadow(10.0, 28.0, -10.0, 0.55)]);
    widget(Stack::overlay_layer().with_layout(|layout| layout.pointer_events = None))
        .children((
            backdrop(chrome),
            widget(surface)
                .prop::<(f32, f32, f32, f32), MenuPlacement>(move || view.with(|view| (x, y, view.width, view.height)))
                .children((rows,))
                .key(format!("{}-context-menu", chrome.prefix))
                .roving_focus(MENU_KEYS),
        ))
        .key(format!("{}-context-layer", chrome.prefix))
        .into_any()
}

/// 菜单左上角和宽：落点和「窗口边减去菜单尺寸再留 4」取小，放不下时贴着右边或下边。
/// 包含块是铺满窗口的菜单层，百分比就是窗口宽高。值是（落点 x、落点 y、宽、高）。
struct MenuPlacement;

impl MenuPlacement {
    fn clamped(anchor: f32, extent: f32) -> LengthSpec {
        LengthSpec::Min2(LengthAtom::Px(anchor.max(EDGE)), LengthAtom::CalcPercent { percent: 100.0, offset_px: -(extent + EDGE) })
    }
}

impl FieldWrite<Stack, (f32, f32, f32, f32)> for MenuPlacement {
    const FIELD: &'static str = "Stack.style.layout.offset+width(menu)";

    fn write(target: &mut Stack, (x, y, width, height): (f32, f32, f32, f32)) {
        let layout = Arc::make_mut(&mut target.node_style_mut().layout);
        layout.offset_left = Some(Self::clamped(x, width));
        layout.offset_top = Some(Self::clamped(y, height));
        layout.width = Some(LengthSpec::Px(width));
    }

    fn differs(target: &Stack, (x, y, width, height): &(f32, f32, f32, f32)) -> bool {
        let layout = &target.node_style().layout;
        layout.offset_left != Some(Self::clamped(*x, *width))
            || layout.offset_top != Some(Self::clamped(*y, *height))
            || layout.width != Some(LengthSpec::Px(*width))
    }
}

/// 铺满窗口的透明底，点在菜单外就关闭，和 Vue 的全局 pointerdown 一样。
fn backdrop(chrome: MenuChrome) -> AnyView {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.position = PositionSpec::Absolute;
    layout.offset_top = Some(LengthSpec::Px(0.0));
    layout.offset_left = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Percent(100.0));
    layout.height = Some(LengthSpec::Percent(100.0));
    let close = chrome.close;
    widget(ListItem::new("关闭菜单").style(style))
        .content(widget(Stack::column(0.0)))
        .key(format!("{}-context-backdrop", chrome.prefix))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(close()))
        .on_cx(move |_, _: &SecondaryPress, cx| cx.dispatch_program_all(close()))
        .into_any()
}

/// 一行和它的子菜单（`.ctx-menu__slot`）。可用的分组挂上指针和焦点的进出：子树里包括子菜单，
/// 在分组和子菜单之间移动不算离开。展开哪个分组只切换子菜单的显隐。
fn slot(view: Signal<MenuView>, entry: MenuEntry, chrome: MenuChrome) -> AnyView {
    let frame = Stack::column(0.0).width(LengthSpec::Fill).with_layout(|layout| layout.position = PositionSpec::Relative);
    if entry.note {
        return widget(frame).children((note(&entry, chrome),)).into_any();
    }
    if entry.children.is_empty() || entry.disabled {
        return widget(frame).children((item(&entry, chrome),)).into_any();
    }
    let Some(branch) = chrome.branch else {
        eprintln!("Nana {} 右键菜单没有分组的进出消息，子菜单展不开：{}", chrome.prefix, entry.id);
        return widget(frame).children((item(&entry, chrome),)).into_any();
    };
    let (pointer, focus) = (entry.id.clone(), entry.id.clone());
    widget(frame)
        .children((item(&entry, chrome), submenu(view, &entry, chrome)))
        .on_cx(move |_, event: &PointerHoverChanged, cx| {
            cx.dispatch_program_all(branch(pointer.clone(), BranchSource::Pointer, event.hovered));
        })
        .on_cx(move |_, event: &FocusWithinChanged, cx| {
            cx.dispatch_program_all(branch(focus.clone(), BranchSource::Focus, event.focused));
        })
        .into_any()
}

/// 子菜单（`.ctx-menu__submenu`）：从这一行右缘起，先是 4px 透明接缝，再是菜单面板。
///
/// 整块接住指针：接缝盖住父菜单右侧的内边距和描边，面板的内边距和行距落在它上面。指针从分组移进
/// 子菜单、在子菜单里移动，都还在这一行的子树里，子菜单不闪关。Vue 的子菜单离这一行也是 4px，
/// 但中间是父菜单的内边距，不属于 `.ctx-menu__slot`，指针慢慢移过去时 `:hover` 先断一下。
fn submenu(view: Signal<MenuView>, entry: &MenuEntry, chrome: MenuChrome) -> AnyView {
    let id = entry.id.clone();
    let rows = entry.children.iter().map(|child| item(child, chrome)).collect::<Vec<_>>();
    let panel = Stack::column(ITEM_GAP)
        .width(LengthSpec::Px(menu_width(&entry.children)))
        .padding(MENU_PADDING)
        .surface(SemanticColorRole::Surface)
        .outline(SemanticColorRole::BorderStrong, 1.0)
        .radius(RadiusTier::Md);
    let bridge = Stack::column(0.0).hittable().with_layout(|layout| {
        layout.position = PositionSpec::Absolute;
        layout.offset_top = Some(LengthSpec::Px(0.0));
        layout.offset_left = Some(LengthSpec::Percent(100.0));
        layout.width = Some(LengthSpec::Shrink);
        layout.padding_left = Some(LengthSpec::Px(SUBMENU_GAP));
        layout.z_index = Some(1);
    });
    widget(bridge)
        .visible(move || view.with(|view| view.branch.as_deref() == Some(id.as_str())))
        .children((widget(style::with_shadows(panel, vec![style::shadow(10.0, 28.0, -10.0, 0.55)])).children(rows).roving_focus(MENU_KEYS),))
        .key(format!("{}-menu-sub-{}", chrome.prefix, entry.id))
        .into_any()
}

/// 点一项做什么：要点两次的项第一次只进入待确认，别的发这一项的消息。
enum Click {
    Arm(fn(String) -> ShellMessage, String),
    Act(Option<MenuAction>),
}

/// `.ctx-menu__item`：图标、文字、分组的 ›。危险项是错误色，待确认时浅红底、600 字重。
fn item(entry: &MenuEntry, chrome: MenuChrome) -> AnyView {
    let pending = entry.pending;
    let foreground = if entry.danger || pending { SemanticColorRole::Danger } else { SemanticColorRole::Text };
    let mut style = NodeStyle::default();
    style.radius = Some(RadiusTier::Sm);
    style.foreground = Some(foreground);
    let danger_soft = SemanticColorMix::alpha(SemanticColorRole::Danger, 0.1);
    style.interaction.hovered = if entry.danger || pending {
        SemanticPaint { background_mix: Some(danger_soft), ..SemanticPaint::default() }
    } else {
        SemanticPaint { background: Some(SemanticColorRole::Hover), ..SemanticPaint::default() }
    };
    style.interaction.pressed = style.interaction.hovered;
    style.interaction.disabled = SemanticPaint::default();
    if pending {
        style.interaction.base = SemanticPaint { background_mix: Some(danger_soft), ..SemanticPaint::default() };
    }
    let layout = Arc::make_mut(&mut style.layout);
    layout.direction = Some(nana_ui_core::FlexDirection::Row);
    layout.height = Some(LengthSpec::Px(ITEM_HEIGHT));
    layout.min_height = Some(LengthSpec::Px(ITEM_HEIGHT));
    layout.width = Some(LengthSpec::Fill);
    layout.padding_left = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(10.0));
    layout.align_items = AlignSpec::Center;
    layout.opacity = entry.disabled.then_some(crate::theme_map::DISABLED_OPACITY);
    let weight = if pending { 600 } else { 500 };
    let label = if pending { entry.confirm_label.unwrap_or_default().to_string() } else { entry.label.clone() };
    let mut parts = Vec::new();
    if entry.checked {
        parts.push(widget(IconGlyph::new(CHECK).size(13.0).role(foreground)).into_any());
    }
    if let Some(icon) = entry.icon {
        parts.push(widget(IconGlyph::new(icon).size(13.0).role(foreground)).into_any());
    }
    parts.push(widget(style::text(label, 13.0, weight, foreground, 20.0).nowrap(true)).into_any());
    if !entry.children.is_empty() {
        parts.push(widget(style::text("›", 13.0, 500, SemanticColorRole::Faint, 20.0)).into_any());
    }
    let content = widget(Stack::row(10.0).align(AlignSpec::Center)).children(parts);
    let mut node = widget(ListItem::new(entry.label.clone()).disabled(entry.disabled).has_popup(!entry.children.is_empty()).style(style))
        .content(content)
        .key(format!("{}-menu-{}", chrome.prefix, entry.id));
    // 分组没有自己的动作（Vue `selectContextMenuItem` 遇到 `children` 直接返回），子菜单由所在的那一行管。
    if !entry.disabled && entry.children.is_empty() {
        let click = match chrome.arm {
            Some(arm) if entry.confirm_label.is_some() && !pending => Click::Arm(arm, entry.id.clone()),
            _ => Click::Act(entry.action.clone()),
        };
        let close_after = matches!(click, Click::Act(_));
        let close = chrome.close;
        node = node.on_cx(move |_, _: &Activate, cx| {
            if close_after {
                cx.dispatch_program_all(close());
            }
            match &click {
                Click::Arm(arm, id) => cx.dispatch_program_all(arm(id.clone())),
                Click::Act(Some(action)) => cx.dispatch_program_all(action.message()),
                Click::Act(None) => {}
            }
        });
    }
    node.into_any()
}

/// 禁用项后面写原因的一行：和菜单项一样高（菜单高按行数算），左边和菜单项的文字对齐，弱色 12px。
fn note(entry: &MenuEntry, chrome: MenuChrome) -> AnyView {
    let row = Stack::row(0.0).align(AlignSpec::Center).with_layout(|layout| {
        layout.height = Some(LengthSpec::Px(ITEM_HEIGHT));
        layout.min_height = Some(LengthSpec::Px(ITEM_HEIGHT));
        layout.width = Some(LengthSpec::Fill);
        layout.padding_left = Some(LengthSpec::Px(10.0));
        layout.padding_right = Some(LengthSpec::Px(10.0));
    });
    widget(row)
        .children((widget(style::text(entry.label.clone(), 12.0, 400, SemanticColorRole::Faint, 18.0).nowrap(true)),))
        .key(format!("{}-menu-{}", chrome.prefix, entry.id))
        .into_any()
}

/// 菜单宽：至少 180，长文案按字宽撑开（中文按字号算，其余按半个字号；写原因的小字按 12px 算）。
fn menu_width(entries: &[MenuEntry]) -> f32 {
    let widest = entries
        .iter()
        .map(|entry| {
            let (wide, narrow) = if entry.note { (12.0, 6.0) } else { (13.0, 7.0) };
            let text: f32 = entry.label.chars().map(|ch| if ch.is_ascii() { narrow } else { wide }).sum();
            let icons = 23.0 * (u8::from(entry.icon.is_some()) + u8::from(entry.checked)) as f32;
            let chevron = if entry.children.is_empty() { 0.0 } else { 14.0 };
            20.0 + icons + text + chevron
        })
        .fold(0.0_f32, f32::max);
    (widest + MENU_PADDING * 2.0 + 2.0).max(MENU_MIN_WIDTH)
}

/// 菜单高：行高、行间距、内边距和描边。
fn menu_height(count: usize) -> f32 {
    count as f32 * ITEM_HEIGHT + count.saturating_sub(1) as f32 * ITEM_GAP + MENU_PADDING * 2.0 + 2.0
}

//! 右键菜单的共用视图：文件条目和侧栏文件夹行的右键菜单都从这里建。
//!
//! 菜单是 NanaUI 的 `AnchoredActionMenu`，钉在指针落点（菜单键和 Shift+F10 时是焦点控件的中心），
//! 是它自己那个 `OverlayHost` 的直接子节点，放在浮层块里。浮层块放好这一块以后经会话（[`MenuPresence`]）
//! 用宿主激活菜单；之后定位（放不下时翻面、收进窗口）、点外面和 Escape 收起、Tab 留在菜单里、
//! 方向键和 Home / End 在项之间走、打开时焦点落在首项、收起时焦点还回打开前的位置，都交给框架。
//! 框架收起菜单时发 `AnchoredMenuDismissed`，这里换成调用方给的关闭消息写回 ViewModel；
//! ViewModel 自己关掉菜单（滚动、窗口缩放、失焦）时浮层块换下这一块，会话经宿主收起它。
//!
//! 条目：图标、文字；危险项走危险色；勾选项（已经在里面的播放列表）报成勾选框、前面画勾。
//! 分组（「缩略图」「加入播放列表」）带 ›，子菜单贴在这一行右边 4px：指针或键盘焦点进到分组那一行
//! （含子菜单）就展开，都离开就收起，没有延迟；点分组本身不收起菜单（按下时焦点落在分组上），
//! 禁用的分组不展开。指针和焦点各在一个分组时只展开指针那一个。方向键按先后走：焦点在分组上时
//! 子菜单展开，往下一格进到子菜单的首项，走出子菜单子菜单就收起。要点两次的项（回收站的「彻底删除」）第一次点不收起，只把这一行变成待确认：
//! 文案换成确认句、标成当前项；再点一次才执行并收起。不能用的项禁用，后面可以跟一行弱色小字写原因
//! （不是菜单项，不能点、不能聚焦）。在菜单上按右键或菜单键也收起：菜单自己不提供右键菜单。
//! 任意滚动区的偏移变了、窗口失焦、窗口逻辑尺寸变了也收起，文件菜单和文件夹菜单一起收；
//! 尺寸没变不关，菜单项仍能点。菜单关掉或换了一份锚点，再滚才算下一次。
//!
//! 菜单常驻：同一个目标、同一个落点的菜单打开期间，条目按内容做键，变了的那一行才重建；
//! 禁用和待确认只改绑定的字段，焦点留在原来那一行。点条目发的消息是条目数据的一部分
//! （[`MenuAction`]），条目变了行就重建，处理器不会拿着过期的值。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nana_ui::InputPayload;
use nana_ui::icons_tabler::CHECK;
use nana_ui::runtime::view::css;
use nana_ui::runtime::view::{
    AnyView, El, EntityRef, FieldWrite, IntoView, Signal, computed, each, entity_ref, fields,
    widget,
};
use nana_ui::runtime::{
    ActionMenuItem, Activate, AlignSpec, AnchoredActionMenu, AnchoredMenuDismissed, AppContext,
    FocusWithinChanged, LengthSpec, OverlayHost, PointerHoverChanged, PositionSpec, RadiusTier,
    SecondaryPress, SemanticColorRole, Stack, Text,
};
use nana_ui::runtime::{BoxPaint, PaintContext, Painter, Radius};
use nana_ui_core::{
    BorderWidth, ElevationRole, Icon, SpacingStep, TextWeight, ThemeExpr, ThemeToken,
};

use crate::theme_map::tiers::{self, Edge};

use super::admin::AdminMessage;
use super::files::{BranchSource, FilesMessage};
use super::input::InputMessage;
use super::player::PlayerMessage;
use super::sidebar::GapMessage;
use super::view_part_overlay::session::{OverlaySession, register};
use super::{ShellMessage, ShellViewModel, SidebarMessage};

/// 一份打开着的右键菜单，以及当时各滚动区的像素偏移。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MenuScrollSnapshot {
    /// 文件菜单和文件夹菜单的锚点。换菜单就整份重记。
    key: String,
    offsets: BTreeMap<String, (i32, i32)>,
}

/// 菜单的最小宽度。
const MENU_MIN_WIDTH: f32 = 180.0;
/// 子菜单离分组那一行右缘的距离：`Xs` 档（默认 4）。
const SUBMENU_GAP: SpacingStep = SpacingStep::Xs;
/// 分组后面的箭头。
const BRANCH_MARK: &str = "›";

/// 菜单里行与行的间距和面板内边距都取主题菜单配方的档（`MenuItemGap`、`MenuSurfacePadding`）。
fn menu_spacing(layout: &mut nana_ui_core::LayoutStyle) {
    layout.gap = Some(LengthSpec::Px(tiers::space_px(SpacingStep::Xxs)));
    layout.theme.gap = Some(ThemeToken::MenuItemGap.into());
    let padding = Some(LengthSpec::Px(tiers::space_px(SpacingStep::Xs)));
    layout.padding_top = padding;
    layout.padding_right = padding;
    layout.padding_bottom = padding;
    layout.padding_left = padding;
    let inset = Some(ThemeExpr::from(ThemeToken::MenuSurfacePadding));
    layout.theme.padding_top = inset.clone();
    layout.theme.padding_right = inset.clone();
    layout.theme.padding_bottom = inset.clone();
    layout.theme.padding_left = inset;
}

/// 子菜单面板的外观：`--bg-elev` 底、一根发丝宽的 `--border-strong` 描边、`Md` 圆角，投影取主题的菜单
/// 高度档（`ElevationRole::Menu`），和主菜单一样。都在画的时候按装上的主题解析。
#[derive(Clone, Copy, Debug, PartialEq)]
struct SubmenuSurface;

impl Painter for SubmenuSurface {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let hairline = cx.theme().border().resolve(BorderWidth::Hairline);
        let paint = BoxPaint::fill(SemanticColorRole::Surface)
            .border(SemanticColorRole::BorderStrong, hairline)
            .shadow(ElevationRole::Menu);
        let bounds = cx.bounds();
        cx.rounded_rect(bounds, Radius::Tier(RadiusTier::Md), paint);
    }

    fn paint_key(&self) -> u64 {
        0x5355_424d_454e_55
    }
}

/// 点菜单项时发的消息，存成数据：能比较（按调试文本），条目变了就知道。`ShellMessage` 不能克隆，
/// 每次点击现做一条。
#[derive(Clone, Debug)]
pub(crate) enum MenuAction {
    Files(FilesMessage),
    Input(InputMessage),
    Admin(AdminMessage),
    /// 切换播放列表成员。
    Membership {
        playlist_id: String,
        kind: String,
        extension: String,
        asset_id: String,
        is_virtual: bool,
        path: String,
    },
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
            Self::Membership {
                playlist_id,
                kind,
                extension,
                asset_id,
                is_virtual,
                path,
            } => ShellMessage::Player(PlayerMessage::ToggleMembership {
                playlist_id,
                kind,
                extension,
                asset_id,
                is_virtual,
                path,
            }),
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

/// 菜单里的一行。`children` 非空时是分组，点它在下面展开或收起子项，不做别的。
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
        Self {
            note: true,
            ..Self::new(id, text, None)
        }
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 危险项：危险色，悬停浅红底。
    pub(crate) fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub(crate) fn message(mut self, action: MenuAction) -> Self {
        self.action = Some(action);
        self
    }

    /// 这一行现在写出来的文字。待确认时换成确认句。
    pub(crate) fn shown_label(&self) -> String {
        if !self.pending {
            return self.label.clone();
        }
        match self
            .confirm_label
            .map(str::trim)
            .filter(|label| !label.is_empty())
        {
            Some(label) => label.to_string(),
            None => {
                eprintln!("Nana 右键菜单「{}」在待确认时没有确认文案", self.id);
                self.label.clone()
            }
        }
    }
}

/// 右键菜单要显示的东西：条目和展开着的分组。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MenuView {
    pub entries: Vec<MenuEntry>,
    /// 展开着的分组。
    pub expanded: Option<String>,
}

impl MenuView {
    pub(crate) fn new(entries: Vec<MenuEntry>, expanded: Option<String>) -> Self {
        Self { entries, expanded }
    }

    /// 按编号找一行，分组里的子项也找。
    fn find(&self, id: &str) -> Option<&MenuEntry> {
        self.entries.iter().find_map(|entry| {
            if entry.id == id {
                Some(entry)
            } else {
                entry.children.iter().find(|child| child.id == id)
            }
        })
    }

    /// 菜单里的一行一行。分组带着它的子项，子菜单挂在那一行上。
    fn rows(&self) -> Vec<MenuRow> {
        self.entries.iter().map(MenuRow::of).collect()
    }
}

/// 菜单里画出来的一行：条目和它的子项（分组时）。
#[derive(Clone, Debug, PartialEq)]
struct MenuRow {
    entry: MenuEntry,
}

impl MenuRow {
    fn of(entry: &MenuEntry) -> Self {
        Self {
            entry: entry.clone(),
        }
    }

    fn group(&self) -> bool {
        !self.entry.children.is_empty()
    }

    /// 行的键：条目内容，不含禁用和待确认，这两样只改绑定的字段，这一行不重建。
    fn key(&self) -> String {
        let mut entry = self.entry.clone();
        entry.disabled = false;
        entry.pending = false;
        for child in &mut entry.children {
            child.disabled = false;
            child.pending = false;
        }
        format!("{entry:?}")
    }
}

/// 一种右键菜单的外壳：键的前缀和它发的几条消息。
#[derive(Clone, Copy)]
pub(crate) struct MenuChrome {
    /// 键的前缀：宿主 `{prefix}-context-layer`、菜单 `{prefix}-context-menu`、每一项 `{prefix}-menu-{条目}`。
    pub prefix: &'static str,
    /// 框架收起了菜单（点外面、Escape、选了一项）或者在菜单上按了右键时发的关闭消息。
    pub close: fn() -> ShellMessage,
    /// 指针或焦点进出带子菜单的分组。没有分组的菜单为 `None`。
    pub branch: Option<fn(String, BranchSource, bool) -> ShellMessage>,
    /// 要点两次的项第一次被点。没有这种项的菜单为 `None`，这样的项点一次就执行。
    pub arm: Option<fn(String) -> ShellMessage>,
}

/// 右键菜单开着时，任一滚动区的偏移相对这份菜单记下的位置变了，就关掉文件菜单和文件夹菜单。
///
/// 听的是滚动，不是点在菜单上。第一次见到这份菜单只记下偏移；
/// 菜单关掉或锚点换了就重新记，避免菜单关着时滚过的距离在重新打开时被当成这次的滚动。
/// 滚动区还没排出来、或者偏移不是有限数时跳过，不当成已经滚动。
pub(crate) fn dismiss_context_menus_on_scroll(
    model: &mut ShellViewModel,
    document: &nana_ui::runtime::RuntimeDocument,
) -> Vec<ShellMessage> {
    let Some(key) = open_menu_key(model) else {
        model.context_menu_scroll = None;
        return Vec::new();
    };
    let offsets = scroll_offsets(document);
    if model
        .context_menu_scroll
        .as_ref()
        .is_some_and(|seen| seen.key == key && !offsets_moved(&seen.offsets, &offsets))
    {
        model.context_menu_scroll = Some(MenuScrollSnapshot { key, offsets });
        return Vec::new();
    }
    let moved = model
        .context_menu_scroll
        .as_ref()
        .is_some_and(|seen| seen.key == key);
    model.context_menu_scroll = Some(MenuScrollSnapshot { key, offsets });
    if !moved {
        return Vec::new();
    }
    close_context_menus(model)
}

/// 窗口失焦时要发的消息。
///
/// 失焦时关掉右键菜单，
/// 在内部拖动已经离开够远时交给系统拖放（`WindowBlur`）。焦点还在窗口里时什么都不发。
/// 点菜单项走的是点击，不经过这里。没有拖动会话时不发 `WindowBlur`，那条归约什么都不做。
pub(crate) fn focus_loss_messages(
    model: &ShellViewModel,
    payload: &InputPayload,
) -> Vec<ShellMessage> {
    let InputPayload::Focus { focused: false } = payload else {
        return Vec::new();
    };
    let mut messages = close_context_menus(model);
    if model.input.internal_active {
        messages.push(ShellMessage::Input(InputMessage::WindowBlur));
    }
    messages
}

/// 窗口逻辑尺寸变了时要发的消息。
///
/// 窗口缩放时关掉右键菜单。第一次见到有限尺寸只记下像素，
/// 不当成缩放；之后宽或高差了至少 1px 才收起文件菜单和文件夹菜单。同一像素（含不到 1px 的抖动）
/// 和无效尺寸不关，菜单留着，点菜单项仍走点击。不发 `WindowBlur`：内部拖动交给系统只在失焦时发生。
pub(crate) fn window_resize_messages(
    model: &mut ShellViewModel,
    width: f32,
    height: f32,
) -> Vec<ShellMessage> {
    let Some(next) = logical_window_px(width, height) else {
        eprintln!("Nana 窗口尺寸无效，右键菜单不据此关闭：{width}×{height}");
        return Vec::new();
    };
    // 按四舍五入后的逻辑像素比。浏览器的 resize 看的是整数视口，差不到 1px 不算尺寸变了。
    let changed = model.context_menu_window.is_some_and(|seen| seen != next);
    model.context_menu_window = Some(next);
    if !changed {
        return Vec::new();
    }
    close_context_menus(model)
}

/// 收起还开着的文件菜单和文件夹菜单。哪份没开就不发那一条。
fn close_context_menus(model: &ShellViewModel) -> Vec<ShellMessage> {
    let mut messages = Vec::new();
    if model.files.entry_menu.is_some() {
        messages.push(ShellMessage::Files(FilesMessage::CloseEntryMenu));
    }
    if model.sidebar.folder_menu.is_some() {
        messages.push(ShellMessage::Sidebar(SidebarMessage::Gap(
            GapMessage::CloseFolderMenu,
        )));
    }
    messages
}

/// 窗口逻辑宽高换成像素。非有限数或小于 1px 的尺寸不能拿来比较。
fn logical_window_px(width: f32, height: f32) -> Option<(i32, i32)> {
    if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
        return None;
    }
    Some((width.round() as i32, height.round() as i32))
}

/// 当前打开的右键菜单锚点。两份都关着时没有。
fn open_menu_key(model: &ShellViewModel) -> Option<String> {
    let file = model
        .files
        .entry_menu
        .as_ref()
        .map(|menu| format!("file:{}@{:.0},{:.0}", menu.path, menu.x, menu.y));
    let folder = model
        .sidebar
        .folder_menu
        .as_ref()
        .map(|menu| format!("folder:{}@{:.0},{:.0}", menu.path, menu.x, menu.y));
    match (file, folder) {
        (None, None) => None,
        (file, folder) => Some(format!(
            "{}|{}",
            file.unwrap_or_default(),
            folder.unwrap_or_default()
        )),
    }
}

/// 已经记下的滚动区，像素偏移变了才算滚过。新出现的滚动区只补进基线，不当成滚动。
fn offsets_moved(
    previous: &BTreeMap<String, (i32, i32)>,
    current: &BTreeMap<String, (i32, i32)>,
) -> bool {
    current
        .iter()
        .any(|(path, offset)| previous.get(path).is_some_and(|before| before != offset))
}

/// 文档里每个滚动区的路径和像素偏移。无效偏移记一笔并跳过，不拿它去关菜单。
fn scroll_offsets(document: &nana_ui::runtime::RuntimeDocument) -> BTreeMap<String, (i32, i32)> {
    use nana_ui::runtime::component_descriptors;
    let document_id = document.document();
    let ids = document
        .context()
        .world()
        .nodes_of_component(document_id, component_descriptors::SCROLL_VIEW.type_id)
        .collect::<Vec<_>>();
    let context = document.context();
    let mut offsets = BTreeMap::new();
    for id in ids {
        let Some(path) = context.assembly_path(id) else {
            continue;
        };
        let Some(offset) = context.world().scroll_offset(id) else {
            continue;
        };
        if !offset.x.is_finite() || !offset.y.is_finite() {
            eprintln!("Nana 滚动区偏移无效，右键菜单不据此关闭：{path}");
            continue;
        }
        offsets.insert(path, (offset.x.round() as i32, offset.y.round() as i32));
    }
    offsets
}

/// 落点 `(x, y)` 的右键菜单。`view` 由调用方在浮层的挂载作用域里建好并登记投影；这里登记菜单的
/// 开合会话（[`MenuPresence`]）。
pub(crate) fn context_menu(view: Signal<MenuView>, x: f32, y: f32, chrome: MenuChrome) -> AnyView {
    let host = entity_ref::<OverlayHost>();
    let menu = entity_ref::<AnchoredActionMenu>();
    let retired = Arc::new(AtomicBool::new(false));
    register(MenuPresence {
        host,
        menu,
        activated: false,
        retired: retired.clone(),
    });
    let rows = each(
        computed(move || view.with(MenuView::rows)),
        MenuRow::key,
        move |row: MenuRow| slot(view, row, chrome),
    )
    .css(css! { gap: theme(space-xxs); });
    let close = chrome.close;
    let surface = widget(AnchoredActionMenu::new(x, y))
        .prop::<f32, MenuWidth>(move || view.with(|view| menu_width(&view.entries)))
        .entity_ref(menu)
        .children((rows,))
        .key(format!("{}-context-menu", chrome.prefix))
        .on_cx(move |_, _: &AnchoredMenuDismissed, cx| {
            // 浮层块换下这一块时自己收起菜单，那时 ViewModel 已经关了，不再发关闭消息。
            if !retired.load(Ordering::Relaxed) {
                cx.dispatch_program_all(close());
            }
        })
        // 右键和菜单键从命中的那一行往上找处理函数，落在菜单上就收起。
        .on_cx(move |_, _: &SecondaryPress, cx| cx.dispatch_program_all(close()));
    widget(OverlayHost::new())
        .entity_ref(host)
        .children((surface,))
        .key(format!("{}-context-layer", chrome.prefix))
        .into_any()
}

/// 右键菜单的开合会话：浮层块放好这一块以后经宿主激活菜单；换下这一块时经宿主收起，焦点还回
/// 打开前的位置，这一块随即卸掉，浮层层不再挡住页面。
struct MenuPresence {
    host: EntityRef<OverlayHost>,
    menu: EntityRef<AnchoredActionMenu>,
    /// 已经激活过。框架收起菜单以后不再激活：关闭消息正在路上，ViewModel 随后关掉它。
    activated: bool,
    /// 换下了：之后框架发的收起不再变成关闭消息。
    retired: Arc<AtomicBool>,
}

impl OverlaySession for MenuPresence {
    fn show(&mut self, context: &mut AppContext) {
        if self.activated {
            return;
        }
        let (Some(host), Some(menu)) = (self.host.get(), self.menu.get()) else {
            eprintln!("Nana 右键菜单还没有挂好，等下一帧再打开");
            return;
        };
        self.activated = true;
        if let Err(error) = context.activate_overlay(host, menu) {
            eprintln!("Nana 打开右键菜单失败：{error}");
        }
    }

    fn hide(&mut self, context: &mut AppContext) -> bool {
        self.retired.store(true, Ordering::Relaxed);
        if let Some(menu) = self.menu.get()
            && let Err(error) = context.dismiss_anchored_menu(menu)
        {
            eprintln!("Nana 收起右键菜单失败：{error}");
        }
        false
    }
}

/// 菜单宽：至少 180，按这一层所有可能露出来的文案撑开（确认句、写原因的小字、分组后面的 ›），待确认时
/// 宽度不跳。中文按字号算，其余按半个字号多一点。子菜单按自己的条目另算。
fn menu_width(entries: &[MenuEntry]) -> f32 {
    fn text(label: &str, size: f32) -> f32 {
        label
            .chars()
            .map(|ch| if ch.is_ascii() { size * 0.55 } else { size })
            .sum()
    }
    let widest = entries
        .iter()
        .map(|entry| {
            if entry.note {
                return 20.0 + text(&entry.label, 12.0);
            }
            let label = text(&entry.label, 13.0).max(
                entry
                    .confirm_label
                    .map(|label| text(label, 13.0))
                    .unwrap_or(0.0),
            );
            let icon = if entry.icon.is_some() || entry.checked {
                22.0
            } else {
                0.0
            };
            let mark = if entry.children.is_empty() { 0.0 } else { 20.0 };
            24.0 + icon + label + mark
        })
        .fold(0.0_f32, f32::max);
    (widest + 12.0).max(MENU_MIN_WIDTH)
}

/// 菜单宽度。值是像素宽，高度按内容排。
struct MenuWidth;

impl FieldWrite<AnchoredActionMenu, f32> for MenuWidth {
    const FIELD: &'static str = "AnchoredActionMenu.width";

    fn write(target: &mut AnchoredActionMenu, width: f32) {
        target.width = width;
        Arc::make_mut(&mut target.style.layout).width = Some(LengthSpec::Px(width));
    }

    fn differs(target: &AnchoredActionMenu, width: &f32) -> bool {
        target.width != *width
    }
}

/// 点了不收起菜单：分组（点它只把焦点放上去，子菜单跟着展开），和还没进入待确认的要点两次的项。
struct KeepOpen;

impl FieldWrite<ActionMenuItem, bool> for KeepOpen {
    const FIELD: &'static str = "ActionMenuItem.keep_open";

    fn write(target: &mut ActionMenuItem, keep_open: bool) {
        target.keep_open = keep_open;
    }

    fn differs(target: &ActionMenuItem, keep_open: &bool) -> bool {
        target.keep_open != *keep_open
    }
}

/// 一行：写原因的小字、菜单项，或者带子菜单的分组。分组那一格报告指针和焦点的进出，子菜单挂在它右边。
fn slot(view: Signal<MenuView>, row: MenuRow, chrome: MenuChrome) -> AnyView {
    let key = format!("{}-menu-{}", chrome.prefix, row.entry.id);
    if row.entry.note {
        return note(&row.entry, key);
    }
    if !row.group() {
        return item(view, row.entry, false, chrome).key(key).into_any();
    }
    let Some(branch) = chrome.branch else {
        eprintln!(
            "Nana {} 右键菜单没有分组的进出消息，子菜单展不开：{}",
            chrome.prefix, row.entry.id
        );
        return item(view, row.entry, true, chrome).key(key).into_any();
    };
    let frame = Stack::column(0.0)
        .width(LengthSpec::Fill)
        .with_layout(|layout| layout.position = PositionSpec::Relative);
    let (pointer, focus) = (row.entry.id.clone(), row.entry.id.clone());
    let sub = submenu(view, &row.entry, chrome);
    widget(frame)
        .children((item(view, row.entry, true, chrome).key(key), sub))
        .on_cx(move |_, event: &PointerHoverChanged, cx| {
            cx.dispatch_program_all(branch(
                pointer.clone(),
                BranchSource::Pointer,
                event.hovered,
            ));
        })
        .on_cx(move |_, event: &FocusWithinChanged, cx| {
            cx.dispatch_program_all(branch(focus.clone(), BranchSource::Focus, event.focused));
        })
        .into_any()
}

/// 分组的子菜单：贴在这一行右边 `Xs` 档（默认 4px），顶边和这一行齐。中间这一段也算在子菜单里，
/// 指针从分组移过去不会中途离开。分组禁用时不展开。
fn submenu(view: Signal<MenuView>, entry: &MenuEntry, chrome: MenuChrome) -> AnyView {
    let id = entry.id.clone();
    let rows = entry
        .children
        .iter()
        .map(|child| {
            item(view, child.clone(), false, chrome)
                .key(format!("{}-menu-{}", chrome.prefix, child.id))
                .into_any()
        })
        .collect::<Vec<_>>();
    let panel = Stack::column(0.0)
        .width(LengthSpec::Px(menu_width(&entry.children)))
        .radius(RadiusTier::Md)
        .painter(SubmenuSurface)
        .with_layout(|layout| {
            menu_spacing(layout);
            tiers::border(layout, 1);
        });
    let bridge = Stack::column(0.0).hittable().with_layout(|layout| {
        layout.position = PositionSpec::Absolute;
        // 往上提一个菜单面板内边距，子菜单第一项和分组那一行齐。
        layout.offset_top = Some(LengthSpec::Px(-tiers::space_px(SpacingStep::Xs)));
        layout.theme.offset_top =
            Some(ThemeExpr::from(ThemeToken::MenuSurfacePadding).scaled(-1.0));
        layout.offset_left = Some(LengthSpec::Percent(100.0));
        layout.width = Some(LengthSpec::Shrink);
        tiers::padding(layout, &[Edge::Left], SUBMENU_GAP);
        layout.z_index = Some(1);
    });
    widget(bridge)
        .visible(move || {
            view.with(|view| {
                view.expanded.as_deref() == Some(id.as_str())
                    && view.find(&id).is_some_and(|entry| !entry.disabled)
            })
        })
        .children((widget(panel).children(rows),))
        .key(format!("{}-menu-sub-{}", chrome.prefix, entry.id))
        .into_any()
}

/// 点一项做什么：分组什么都不发，要点两次的项第一次只进入待确认，别的发这一项的消息。
fn click(entry: &MenuEntry, group: bool, chrome: MenuChrome) -> Option<ShellMessage> {
    if group {
        // 点分组本身只把焦点放到它上面，子菜单跟着焦点展开，菜单不收起。
        return None;
    }
    match chrome.arm {
        Some(arm) if entry.confirm_label.is_some() && !entry.pending => Some(arm(entry.id.clone())),
        _ => entry.action.as_ref().map(MenuAction::message),
    }
}

/// 菜单项。禁用、待确认的文案、标成当前项和点了收不收起都按这一行现在的样子绑定。
fn item(
    view: Signal<MenuView>,
    entry: MenuEntry,
    group: bool,
    chrome: MenuChrome,
) -> El<ActionMenuItem> {
    let mut item = ActionMenuItem::new(entry.shown_label()).danger(entry.danger);
    if group {
        item = item.hint(BRANCH_MARK);
    }
    if entry.checked {
        item = item.check(true).leading(CHECK);
    } else if let Some(icon) = entry.icon {
        item = item.leading(icon);
    }
    let armable = chrome.arm.is_some() && entry.confirm_label.is_some();
    let id = entry.id.clone();
    let current = move |id: &str| view.with(|view| view.find(id).cloned());
    let (label, disabled, active, keep) = (id.clone(), id.clone(), id.clone(), id.clone());
    widget(item.disabled(entry.disabled).keep_open(group || armable))
        .prop::<Arc<str>, fields::action_menu_item::label>(move || {
            Arc::from(
                current(&label)
                    .map(|entry| entry.shown_label())
                    .unwrap_or_default(),
            )
        })
        .prop::<bool, fields::action_menu_item::disabled>(move || {
            current(&disabled).is_none_or(|entry| entry.disabled)
        })
        .prop::<bool, fields::action_menu_item::active>(move || {
            current(&active).is_some_and(|entry| entry.pending)
        })
        .prop::<bool, KeepOpen>(move || {
            group || (armable && current(&keep).is_some_and(|entry| !entry.pending))
        })
        .on_cx(move |_, _: &Activate, cx| {
            let Some(entry) = view.with_untracked(|view| view.find(&id).cloned()) else {
                return;
            };
            if entry.disabled {
                return;
            }
            if let Some(message) = click(&entry, group, chrome) {
                cx.dispatch_program_all(message);
            }
        })
}

/// 禁用项后面写原因的一行：左边和菜单项的文字对齐（左右 `Lg` 档，默认 10px），弱色 12px。
fn note(entry: &MenuEntry, key: String) -> AnyView {
    let row = Stack::row(0.0)
        .align(AlignSpec::Center)
        .with_layout(|layout| {
            layout.min_height = Some(LengthSpec::Px(24.0));
            layout.width = Some(LengthSpec::Fill);
            tiers::padding(layout, &Edge::X, SpacingStep::Lg);
        });
    widget(row)
        .children((widget(
            crate::theme_map::text_line_height(
                crate::theme_map::typed_text(
                    Text::new(entry.label.clone()),
                    12.0,
                    TextWeight::Regular,
                ),
                18.0,
            )
            .color(SemanticColorRole::Faint)
            .nowrap(true),
        ),))
        .key(key)
        .into_any()
}

//! 文件右键菜单，对应 Vue `useWorkspaceContextMenu.ts` 和 `ContextMenuHost.vue`。
//!
//! 外观照 `.ctx-menu`：抬升底、`--border-strong` 描边、`Md` 圆角、内边距 4、行间距 1，
//! 行高 28、13px/500、13px 图标；「加入播放列表」「缩略图」带 › 并展开子菜单，子菜单贴在
//! 这一行右边 4px。菜单按指针落点放置，靠右或靠下放不下时夹回窗口内 4px。
//! 回收站里的「彻底删除」要点两次：第一次只把这一行变成待确认。点菜单外关闭。
//!
//! 菜单常驻：同一个目标、同一个落点的菜单打开期间，条目按内容做键，变了的那一行才重建；
//! 展开哪个子菜单只切换子菜单的显隐。点条目发的消息是条目数据的一部分（[`MenuAction`]），
//! 条目变了行就重建，处理器不会拿着过期的值。

use std::sync::Arc;

use nana_ui::icons_tabler::{CHECK, CLIPBOARD, EYE, FILES, FOLDER_OPEN, PENCIL, PHOTO, PHOTO_OFF, PHOTO_PLUS, REFRESH, ROTATE, TRASH};
use nana_ui::runtime::view::{computed, each, signal, widget, AnyView, FieldWrite, IntoView, Signal, StyledComponent};
use nana_ui::runtime::{
    Activate, AlignSpec, IconGlyph, LengthSpec, ListItem, NodeStyle, PositionSpec, RadiusTier, SecondaryPress,
    SemanticColorRole, SemanticPaint, Stack,
};
use nana_ui_core::{Icon, LengthAtom, SemanticColorMix};

use super::super::admin::AdminMessage;
use super::super::entry_actions::{self, FilePluginAction, FilePluginDispatch};
use super::super::files::{FileContext, FileDialog, FileRow, FilesMessage};
use super::super::input::{repository_absolute, InputMessage};
use super::super::player::PlayerMessage;
use super::super::view_part_overlay::session::Projected;
use super::super::workspace::WorkspacePanel;
use super::super::{ShellMessage, ShellViewModel};
use super::style;

/// `.ctx-menu` 的最小宽度、内边距、行高和行间距。
const MENU_MIN_WIDTH: f32 = 180.0;
const MENU_PADDING: f32 = 4.0;
const ITEM_HEIGHT: f32 = 28.0;
const ITEM_GAP: f32 = 1.0;
/// 菜单离窗口边至少 4px（`SB_MENU_EDGE_PADDING`）。
const EDGE: f32 = 4.0;
/// Vue 右键菜单的层级（`SB_LAYER_Z_INDEX.contextMenu`）。浮层根比它低 1，点菜单外的透明底和它同层。
const MENU_Z: i32 = 2000;

/// 点菜单项时发的消息，存成数据：能比较（按调试文本），条目变了就知道。`ShellMessage` 不能克隆，
/// 每次点击现做一条。
#[derive(Clone, Debug)]
pub(crate) enum MenuAction {
    Files(FilesMessage),
    Input(InputMessage),
    Admin(AdminMessage),
    /// 切换播放列表成员。
    Membership { playlist_id: String, kind: String, extension: String, asset_id: String, is_virtual: bool, path: String },
}

impl MenuAction {
    fn message(&self) -> ShellMessage {
        match self.clone() {
            Self::Files(message) => ShellMessage::Files(message),
            Self::Input(message) => ShellMessage::Input(message),
            Self::Admin(message) => ShellMessage::Admin(message),
            Self::Membership { playlist_id, kind, extension, asset_id, is_virtual, path } => {
                ShellMessage::Player(PlayerMessage::ToggleMembership { playlist_id, kind, extension, asset_id, is_virtual, path })
            }
        }
    }
}

impl PartialEq for MenuAction {
    fn eq(&self, other: &Self) -> bool {
        format!("{self:?}") == format!("{other:?}")
    }
}

fn files(message: FilesMessage) -> MenuAction {
    MenuAction::Files(message)
}

fn input(message: InputMessage) -> MenuAction {
    MenuAction::Input(message)
}

fn admin(message: AdminMessage) -> MenuAction {
    MenuAction::Admin(message)
}

/// 菜单里的一行。`children` 非空时是带 › 的分组，点它只展开子菜单。
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
    pub action: Option<MenuAction>,
    pub children: Vec<MenuEntry>,
}

impl MenuEntry {
    fn new(id: &str, label: impl Into<String>, icon: Option<Icon>) -> Self {
        Self {
            id: id.to_string(),
            label: label.into(),
            icon,
            checked: false,
            disabled: false,
            danger: false,
            confirm_label: None,
            pending: false,
            action: None,
            children: Vec::new(),
        }
    }

    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn message(mut self, action: MenuAction) -> Self {
        self.action = Some(action);
        self
    }
}

/// 右键菜单要显示的东西：条目、展开的子菜单和菜单尺寸。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EntryMenuView {
    pub entries: Vec<MenuEntry>,
    /// 展开着的分组。
    pub branch: Option<String>,
    pub width: f32,
    pub height: f32,
}

impl EntryMenuView {
    /// 目标条目已经不在列表里时为 `None`。
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let menu = model.files.entry_menu.as_ref()?;
        let ctx = FileContext::from_model(model);
        let row = model.files.visible_rows(&ctx).into_iter().find(|row| row.path == menu.path)?;
        let mut entries = menu_entries(model, &row);
        let pending = model.files.menu_pending.as_deref();
        for entry in &mut entries {
            entry.pending = entry.confirm_label.is_some() && pending == Some(entry.id.as_str());
        }
        Some(Self {
            width: menu_width(&entries),
            height: menu_height(entries.len()),
            branch: model.files.menu_branch.clone(),
            entries,
        })
    }
}

/// 右键打开的条目菜单。没有目标或目标已不在列表里时不占浮层。
pub(super) fn entry_menu(model: &ShellViewModel) -> Option<AnyView> {
    let menu = model.files.entry_menu.clone()?;
    let Some(initial) = EntryMenuView::project(model) else {
        eprintln!("Nana 右键菜单的条目已经不在列表里：{}", menu.path);
        return None;
    };
    let view = signal(initial);
    Projected::register(view, EntryMenuView::project);
    let entries = computed(move || view.with(|view| view.entries.clone()));
    let rows = each(entries, |entry: &MenuEntry| format!("{entry:?}"), move |entry: MenuEntry| slot(view, entry)).gap(ITEM_GAP);
    let (x, y) = (menu.x, menu.y);
    let surface = Stack::column(0.0)
        .padding(MENU_PADDING)
        .surface(SemanticColorRole::Surface)
        .outline(SemanticColorRole::BorderStrong, 1.0)
        .radius(RadiusTier::Md)
        .with_layout(move |layout| {
            layout.position = PositionSpec::Fixed;
            layout.z_index = Some(MENU_Z + 1);
        });
    let surface = style::with_shadows(surface, vec![style::shadow(10.0, 28.0, -10.0, 0.55)]);
    Some(
        widget(Stack::overlay_layer().with_layout(|layout| {
            layout.position = PositionSpec::Fixed;
            layout.pointer_events = None;
            layout.z_index = Some(MENU_Z - 1);
        }))
        .children((
            backdrop(),
            widget(surface)
                .prop::<(f32, f32, f32, f32), MenuPlacement>(move || view.with(|view| (x, y, view.width, view.height)))
                .children((rows,))
                .key("file-context-menu"),
        ))
        .key("file-context-layer")
        .into_any(),
    )
}

/// 菜单左上角和宽：落点和「窗口边减去菜单尺寸再留 4」取小，放不下时贴着右边或下边。
/// 固定定位的包含块是窗口，百分比就是窗口宽高。值是（落点 x、落点 y、宽、高）。
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
fn backdrop() -> AnyView {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.position = PositionSpec::Fixed;
    layout.offset_top = Some(LengthSpec::Px(0.0));
    layout.offset_left = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Percent(100.0));
    layout.height = Some(LengthSpec::Percent(100.0));
    // 固定定位的节点按自己的层级参与整窗排序；不给层级就排在壳层浮层根之下，点不到。
    layout.z_index = Some(MENU_Z);
    widget(ListItem::new("关闭菜单").style(style))
        .content(widget(Stack::column(0.0)))
        .key("file-context-backdrop")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(close()))
        .on_cx(|_, _: &SecondaryPress, cx| cx.dispatch_program_all(close()))
        .into_any()
}

fn close() -> ShellMessage {
    ShellMessage::Files(FilesMessage::CloseEntryMenu)
}

/// 一行和它的子菜单。子菜单相对这一行绝对定位在右边 4px，展开哪个分组只切换显隐。
fn slot(view: Signal<EntryMenuView>, entry: MenuEntry) -> AnyView {
    let submenu = (!entry.children.is_empty()).then(|| {
        let id = entry.id.clone();
        let child_width = menu_width(&entry.children);
        let rows = entry.children.iter().map(item).collect::<Vec<_>>();
        let panel = Stack::column(ITEM_GAP)
            .width(LengthSpec::Px(child_width))
            .padding(MENU_PADDING)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderStrong, 1.0)
            .radius(RadiusTier::Md)
            .with_layout(|layout| {
                layout.position = PositionSpec::Absolute;
                layout.offset_top = Some(LengthSpec::Px(0.0));
                layout.z_index = Some(1);
            });
        widget(style::with_shadows(panel, vec![style::shadow(10.0, 28.0, -10.0, 0.55)]))
            .visible(move || view.with(|view| view.branch.as_deref() == Some(id.as_str())))
            .prop::<f32, OffsetLeft>(move || view.with(|view| view.width - MENU_PADDING * 2.0 - 2.0 + 4.0))
            .children(rows)
            .key(format!("file-menu-sub-{}", entry.id))
            .into_any()
    });
    widget(Stack::column(0.0).width(LengthSpec::Fill).with_layout(|layout| layout.position = PositionSpec::Relative))
        .children((item(&entry), submenu))
        .into_any()
}

/// 子菜单离这一行左边的距离：父菜单宽减去内边距和描边，再右移 4。
struct OffsetLeft;

impl FieldWrite<Stack, f32> for OffsetLeft {
    const FIELD: &'static str = "Stack.style.layout.offset_left";

    fn write(target: &mut Stack, left: f32) {
        Arc::make_mut(&mut target.node_style_mut().layout).offset_left = Some(LengthSpec::Px(left));
    }

    fn differs(target: &Stack, left: &f32) -> bool {
        target.node_style().layout.offset_left != Some(LengthSpec::Px(*left))
    }
}

/// `.ctx-menu__item`：图标、文字、分组的 ›。危险项是错误色，待确认时浅红底、600 字重。
fn item(entry: &MenuEntry) -> AnyView {
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
    layout.opacity = entry.disabled.then_some(0.45);
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
    let mut node = widget(ListItem::new(entry.label.clone()).disabled(entry.disabled).style(style))
        .content(content)
        .key(format!("file-menu-{}", entry.id));
    if !entry.disabled {
        let action = if !entry.children.is_empty() {
            Some(files(FilesMessage::ToggleMenuBranch(entry.id.clone())))
        } else if entry.confirm_label.is_some() && !pending {
            Some(files(FilesMessage::ArmMenuConfirm(entry.id.clone())))
        } else {
            entry.action.clone()
        };
        let close_after = entry.children.is_empty() && (entry.confirm_label.is_none() || pending);
        node = node.on_cx(move |_, _: &Activate, cx| {
            if close_after {
                cx.dispatch_program_all(close());
            }
            if let Some(action) = &action {
                cx.dispatch_program_all(action.message());
            }
        });
    }
    node.into_any()
}

/// 菜单宽：至少 180，长文案按字宽撑开（中文按字号算，其余按半个字号）。
fn menu_width(entries: &[MenuEntry]) -> f32 {
    let widest = entries
        .iter()
        .map(|entry| {
            let text: f32 = entry.label.chars().map(|ch| if ch.is_ascii() { 7.0 } else { 13.0 }).sum();
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

/// 和 Vue `fileEntryContextMenu` 同样的条目、顺序和禁用规则。
fn menu_entries(model: &ShellViewModel, row: &FileRow) -> Vec<MenuEntry> {
    let repository = model.workspace.active_repository();
    let repo_id = repository.map(|item| item.repo_id.clone());
    let absolute = repository_absolute(&repository.map(|item| item.path.clone()).unwrap_or_default(), &row.path);
    let has_repo = repository.is_some();
    let trash = model.workspace.panel == WorkspacePanel::Trash;
    let smart = model.workspace.panel == WorkspacePanel::SmartFolder;
    let multiple = model.files.selected.len() > 1;
    let mutating = model.files.mutating;
    let file = row.kind == "file";
    let preview = || files(FilesMessage::OpenRow(row.path.clone()));
    let open = input(InputMessage::OpenEntry { has_repo, absolute_path: absolute.clone() });
    let reveal = input(InputMessage::RevealEntry { absolute_path: absolute.clone() });
    if smart {
        return vec![
            MenuEntry::new("preview", "预览", Some(EYE)).disabled(!file).message(preview()),
            MenuEntry::new("open", "打开", Some(EYE)).disabled(!file).message(open),
            MenuEntry::new("reveal", "定位", Some(FOLDER_OPEN)).message(reveal),
        ];
    }
    let mut entries = Vec::new();
    if trash {
        entries.push(MenuEntry::new("restore", "还原", Some(ROTATE)).disabled(mutating).message(files(FilesMessage::RestoreSelected)));
    }
    entries.push(MenuEntry::new("preview", "预览", Some(EYE)).disabled(!file || trash || multiple).message(preview()));
    let open_entry = if row.kind == "directory" {
        MenuEntry::new("open", "进入", Some(EYE)).message(files(FilesMessage::OpenPath(row.path.clone())))
    } else {
        MenuEntry::new("open", "打开", Some(EYE)).message(open)
    };
    entries.push(open_entry.disabled(trash || multiple));
    entries.push(MenuEntry::new("reveal", "定位", Some(FOLDER_OPEN)).disabled(trash).message(reveal));
    entries.push(
        MenuEntry::new("copy-target", "复制到…", Some(FILES))
            .disabled(trash || mutating)
            .message(files(FilesMessage::OpenDialog(FileDialog::Copy))),
    );
    if !trash && !multiple {
        let playlists = playlist_entries(model, row);
        if !playlists.is_empty() {
            let label = if row.kind == "directory" { "整个加入播放列表" } else { "加入播放列表" };
            let mut group = MenuEntry::new("playlist-membership", label, None);
            group.children = playlists;
            entries.push(group);
        }
    }
    if !trash {
        for action in entry_actions::actions_for(&model.admin.plugins, row) {
            entries.push(plugin_entry(&action, repo_id.clone()));
        }
    }
    let mut thumbnail = MenuEntry::new("thumbnail", "缩略图", Some(PHOTO)).disabled(trash);
    thumbnail.children = thumbnail_entries(row, repo_id.clone());
    entries.push(thumbnail);
    entries.push(
        MenuEntry::new("rename", "重命名", Some(PENCIL))
            .disabled(trash || multiple)
            .message(files(FilesMessage::OpenDialog(FileDialog::Rename))),
    );
    let mut delete = MenuEntry::new("delete", if trash { "彻底删除" } else { "删除" }, Some(TRASH))
        .disabled(mutating)
        .message(files(FilesMessage::DeleteSelected));
    delete.danger = true;
    delete.confirm_label = trash.then_some("确认彻底删除？再点一次");
    entries.push(delete);
    entries
}

/// 兼容的播放列表：名字前面打勾表示已经在里面。文件切换成员，目录和虚拟条目按路径加入。
fn playlist_entries(model: &ShellViewModel, row: &FileRow) -> Vec<MenuEntry> {
    let extension = row.extension.clone().unwrap_or_default();
    let asset_id = row.asset_id.clone().unwrap_or_default();
    model
        .player
        .membership_actions(&row.kind, &extension, &asset_id, row.is_virtual)
        .into_iter()
        .map(|action| {
            let name = model
                .player
                .playlists
                .iter()
                .find(|playlist| playlist.playlist_id == action.playlist_id)
                .map(|playlist| playlist.name.clone())
                .unwrap_or_else(|| action.label.clone());
            let toggle = MenuAction::Membership {
                playlist_id: action.playlist_id.clone(),
                kind: row.kind.clone(),
                extension: extension.clone(),
                asset_id: asset_id.clone(),
                is_virtual: row.is_virtual,
                path: row.path.clone(),
            };
            let mut entry = MenuEntry::new(&format!("playlist-{}", style::key_part(&action.playlist_id)), name, None).message(toggle);
            entry.checked = action.checked;
            entry
        })
        .collect()
}

/// 来源插件的条目动作。需要目录或名称的先弹对应对话框。
fn plugin_entry(action: &FilePluginAction, repository_id: Option<String>) -> MenuEntry {
    let message = match &action.dispatch {
        FilePluginDispatch::Call { plugin_id, method, payload } => admin(AdminMessage::CallFilePlugin {
            plugin_id: plugin_id.clone(),
            method: method.clone(),
            payload: payload.clone(),
            repository_id,
        }),
        FilePluginDispatch::AskFolder { plugin_id, method, payload } => input(InputMessage::BeginDownloadFolder {
            plugin_id: plugin_id.clone(),
            method: method.clone(),
            payload: payload.clone(),
            repository_id,
        }),
        FilePluginDispatch::AskName { plugin_id, method, payload } => input(InputMessage::OpenSourcePlaylist {
            plugin_id: plugin_id.clone(),
            method: method.clone(),
            payload: payload.clone(),
            repository_id,
        }),
    };
    MenuEntry::new(&action.value.replace('/', "-"), action.label.clone(), None).message(message)
}

/// 「缩略图」子菜单：选文件、从剪贴板、取消自定义（只有自定义过才可点）、刷新。
fn thumbnail_entries(row: &FileRow, repo_id: Option<String>) -> Vec<MenuEntry> {
    let path = row.path.clone();
    let kind = row.kind.clone();
    let custom = |build: fn(String, String, String) -> InputMessage| -> MenuAction {
        match repo_id.clone() {
            Some(repo_id) => input(build(repo_id, path.clone(), kind.clone())),
            None => {
                eprintln!("Nana 自定义缩略图缺少仓库");
                files(FilesMessage::NoteError("自定义缩略图缺少仓库。".into()))
            }
        }
    };
    vec![
        MenuEntry::new("thumbnail-custom-file", "自定义缩略图（选择文件）", Some(PHOTO_PLUS))
            .message(custom(|repo_id, path, kind| InputMessage::BeginThumbnailFile { repo_id, path, kind })),
        MenuEntry::new("thumbnail-custom-clipboard", "新增自定义缩略图（从剪贴板）", Some(CLIPBOARD))
            .message(custom(|repo_id, path, kind| InputMessage::PasteThumbnail { repo_id, path, kind })),
        MenuEntry::new("thumbnail-clear-custom", "取消自定义缩略图", Some(PHOTO_OFF))
            .disabled(!row.thumbnail_custom)
            .message(custom(|repo_id, path, kind| InputMessage::ClearThumbnail { repo_id, path, kind })),
        MenuEntry::new("thumbnail-refresh", "刷新缩略图", Some(REFRESH))
            .message(files(FilesMessage::RefreshThumbnail(row.path.clone()))),
    ]
}

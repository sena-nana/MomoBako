//! 浮层关掉以后把焦点还回去，照 WAI-ARIA 对话框的做法：回到打开它的控件；那个控件已经不在了，
//! 就回到它所在区域里合理的控件。对话框、弹层、右键菜单和文件页的「导入」菜单都按这一条走。
//!
//! 规则：
//! - 一层打开时记下「打开它的控件」：右键菜单是它所属的文件条目或文件夹行（右键不挪焦点，菜单键时
//!   焦点本来就在上面）；别的层是打开那一刻焦点所在的页面控件。这一层是从刚关掉的上一层里打开的
//!   （菜单项打开对话框、弹层换页、排在后面的对话框接着显示）时，接着用上一层记下的控件：菜单项和
//!   上一页随上一层收起，不能当归还目标。焦点那一刻不在页面上、也没有上一层时，用这一层所属区域的
//!   落脚点（见 [`Home`]）。
//! - 一层关掉、没有别的层接手，而焦点没了（随浮层卸掉、留在藏起来的菜单里，或者框架没有能还的目标）
//!   时，焦点回到打开它的控件；它不在了或者不能聚焦，就从它原来的上层往外找第一层还在的容器，取里面
//!   选中的项（当前文件夹、选中的文件条目），没有就取第一个能聚焦的控件，最远找到它所在区域的根（侧栏、
//!   主区）。整个区域都不在（换仓库时侧栏收起）时等它回来再还，期间焦点被放到别处就作罢。对话框关上
//!   以前（放退场的那一帧里）焦点还在对话框里，等框架关上它再看。
//! - 还回去的控件随后被删掉、换掉（删除确认关掉以后目录树重读，重命名以后条目换了键），焦点还在它
//!   上面时，照上一条在它那一片里重新找。焦点被放到别处以后不再管。
//! - 弹层换页（切换列表、添加菜单、后端表单）时焦点跟进新的一页：页里有输入框时落在第一个输入框，
//!   否则落在菜单的首项。对话框打开前先把焦点放回打开它的控件，框架据此记下关上以后还到哪。
//!
//! 每次整体同步收尾时调 [`FocusReturn::sync`]（这时换下的浮层块已经卸掉、新的已经挂好），每帧画完
//! 以后调 [`FocusReturn::settle`]：对话框在刷新时才真正关上，列表里的行也在刷新时才卸掉，刷新以后再看
//! 焦点还了没有。

use nana_ui::runtime::{
    AccessibilityRole, AppContext, ConfirmDialog, Dialog, DocumentId, Entity, RovingEdge, StableNodeId, UiWorld,
};

use super::menu_focus::MenuSurface;
use super::view_part_overlay::{OverlayIdentity, OverlayKey};
use super::ShellViewModel;

/// 会临时拿走焦点、关掉以后要把焦点还回去的一层。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    /// 浮层块：对话框、弹层和右键菜单。身份里的结构变了（换页、换目标）算换了一层。
    Overlay(OverlayIdentity),
    /// 文件页工具条「导入」的菜单，在页面里开合，不在浮层层里。
    ImportMenu,
}

impl Surface {
    /// 模态的对话框：框架打开时把焦点拿进去，关上时还到打开前的位置。
    fn modal(&self) -> bool {
        matches!(self, Self::Overlay(identity) if identity.key.modal())
    }

    /// 同一种：浮层的种类相同（弹层换页、文件对话框换了一种）。
    fn same_kind(&self, other: &Surface) -> bool {
        match (self, other) {
            (Self::Overlay(a), Self::Overlay(b)) => a.key == b.key,
            (Self::ImportMenu, Self::ImportMenu) => true,
            _ => false,
        }
    }

    /// 没有打开它的控件时，焦点落在哪个区域。
    fn home(&self) -> Home {
        let Self::Overlay(identity) = self else {
            return Home::Files;
        };
        match identity.key {
            OverlayKey::CloseConfirm => Home::None,
            OverlayKey::EntryMenu | OverlayKey::DownloadTarget | OverlayKey::ExportDialog | OverlayKey::FileDialog => Home::Files,
            OverlayKey::PluginDelete => Home::Primary,
            OverlayKey::FolderDelete
            | OverlayKey::SmartDelete
            | OverlayKey::RepositoryDelete
            | OverlayKey::PlaylistCreate
            | OverlayKey::FolderDialog
            | OverlayKey::SmartFolderDialog
            | OverlayKey::RepositoryPopover
            | OverlayKey::FolderMenu
            | OverlayKey::TaskPopover => Home::Sidebar,
        }
    }
}

/// 一层所属的区域：没有打开它的控件时焦点的落脚点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Home {
    /// 侧栏：选中的项，没有时是仓库头（侧栏里第一个能聚焦的控件）。
    Sidebar,
    /// 文件列表：选中的文件条目，没有时是第一个条目；列表不在时退到主区。
    Files,
    /// 主区：这一页选中的项，没有时是第一个能聚焦的控件。
    Primary,
    /// 不归哪个区域（关闭确认）：打开前没有焦点，关上以后也不放。
    None,
}

/// 页面的几块：区域的根和浮层层。由壳层视图在每次同步时交过来。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Regions {
    pub sidebar: Option<StableNodeId>,
    pub primary: Option<StableNodeId>,
    /// 浮层层：浮层块都是它的子节点。
    pub layer: Option<StableNodeId>,
    /// 浮层层里现在显示的这一块。
    pub overlay: Option<StableNodeId>,
}

/// 这次同步开始前（换下浮层块以前）焦点在哪：换下的块随即卸掉，事后就查不到焦点原来在不在里面。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Before {
    focused: Option<StableNodeId>,
    /// 焦点在浮层层里：菜单、弹层或对话框里的控件。
    in_overlay: bool,
    /// 焦点在「导入」菜单的项上。
    in_import_menu: bool,
}

impl Before {
    pub(crate) fn capture(context: &AppContext, document: DocumentId, layer: Option<StableNodeId>) -> Self {
        let world = context.world();
        let Some(focused) = world.focused(document) else {
            return Self::default();
        };
        Self { focused: Some(focused), in_overlay: within(world, focused, layer), in_import_menu: in_import_menu(context, focused) }
    }

    /// 焦点在这一层里：关掉以后焦点随它没了。
    fn inside(&self, surface: &Surface) -> bool {
        match surface {
            Surface::Overlay(_) => self.in_overlay,
            Surface::ImportMenu => self.in_import_menu,
        }
    }
}

/// 归还目标：打开这一层的控件，和它不在了以后去哪找。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Origin {
    /// 打开这一层的控件。
    opener: Option<StableNodeId>,
    /// 打开它的控件从近到远的上层，最远到所在区域的根（含）。控件不在了在这里找落脚点。
    around: Vec<StableNodeId>,
    /// 这些上层都不在时退到哪个区域。
    home: Option<Home>,
}

/// 开着的一层和它的归还目标。
#[derive(Clone, Debug)]
struct Layer {
    surface: Surface,
    origin: Origin,
}

/// 壳层视图里管焦点归还的那一份状态。
#[derive(Debug, Default)]
pub(crate) struct FocusReturn {
    /// 现在开着的各层。
    open: Vec<Layer>,
    /// 关掉以后还没还上的目标。
    pending: Option<Origin>,
    /// 还欠着的目标这会儿哪里都放不下（整个区域藏着），已经记过日志，等它回来再还时不重复记。
    stalled: bool,
    /// 刚还到的控件：它随后被删掉或换掉、焦点还在它上面时，在它那一片里重新找。
    watching: Option<Origin>,
}

impl FocusReturn {
    /// 新挂的视图：挂载时已经开着的层没有记下的控件，关掉时按所属区域找落脚点。
    pub(crate) fn new(model: &ShellViewModel, shown: Option<&OverlayIdentity>) -> Self {
        let open = surfaces(model, shown).into_iter().map(|surface| Layer { origin: Origin::home(&surface), surface }).collect();
        Self { open, ..Self::default() }
    }

    /// 整体同步收尾时调：比对开着的层，给新开的记下归还目标，给关掉的安排归还，再试着还上。
    /// `before` 是这次同步开始前焦点在哪；`shown` 是浮层块现在显示的那一层。
    pub(crate) fn sync(
        &mut self,
        context: &mut AppContext,
        document: DocumentId,
        model: &ShellViewModel,
        shown: Option<&OverlayIdentity>,
        before: Before,
        regions: Regions,
    ) {
        let now = surfaces(model, shown);
        let (mut closed, kept): (Vec<Layer>, Vec<Layer>) = std::mem::take(&mut self.open).into_iter().partition(|layer| !now.contains(&layer.surface));
        self.open = kept;
        for surface in now {
            if self.open.iter().any(|layer| layer.surface == surface) {
                continue;
            }
            // 右键菜单总是还给它所属的条目或文件夹行，不接上一层（换到另一个条目上重新打开时也一样）。
            let anchored = context_anchor(&surface, model)
                .and_then(|key| keyed(context, document, &key))
                .map(|anchor| Origin::of(context.world(), anchor, regions));
            let handed = match anchored {
                Some(_) => None,
                None => closed.iter().position(|layer| surface.modal() || surface.same_kind(&layer.surface)),
            };
            let origin = match (anchored, handed) {
                (Some(origin), _) => origin,
                (None, Some(index)) => closed.remove(index).origin,
                (None, None) => self.origin_for(context, model, &surface, before.focused, &closed, regions),
            };
            self.watching = None;
            if handed.is_some() || self.pending.as_ref() == Some(&origin) {
                self.pending = None;
                self.stalled = false;
            }
            let origin = self.enter(context, document, model, &surface, origin, handed.is_some(), regions);
            self.open.push(Layer { surface, origin });
        }
        // 对话框打开时总把焦点拿进去；菜单和弹层只在焦点本来就在里面时才算把焦点带走了。
        for layer in closed {
            if layer.surface.modal() || before.inside(&layer.surface) {
                self.pending = Some(layer.origin);
                self.stalled = false;
            }
        }
        self.settle(context, document, model, regions);
    }

    /// 每帧收尾和整体同步收尾时调：关掉的层还没还上焦点、焦点又没了，就还到打开它的控件或它那一片里
    /// 合理的控件；刚还到的控件随后被删掉、换掉时重新找。
    pub(crate) fn settle(&mut self, context: &mut AppContext, document: DocumentId, model: &ShellViewModel, regions: Regions) {
        if let Some(origin) = self.pending.take() {
            match focus_state(context, document, model, regions) {
                FocusState::Held => self.pending = Some(origin),
                // 框架关上对话框时已经还到了打开它的控件，或者焦点被放到了别处。
                FocusState::Placed(focused) if origin.opener == Some(focused) => self.watch(context, document, focused, regions),
                FocusState::Placed(_) => {}
                FocusState::Lost => match restore(context, document, &origin, regions) {
                    Some(target) => self.watch(context, document, target, regions),
                    None => self.stall(origin),
                },
            }
            if self.pending.is_none() {
                self.stalled = false;
            }
            return;
        }
        let Some(watched) = self.watching.take() else {
            return;
        };
        let Some(target) = watched.opener else {
            return;
        };
        let world = context.world();
        let focused = world.focused(document);
        let gone = !live(world, target);
        if focused.is_some_and(|focused| focused != target) || (focused.is_none() && !gone) {
            // 焦点被放到别处，或者用户自己让它失焦：不再管。
            return;
        }
        if !gone {
            self.watching = Some(watched);
            return;
        }
        eprintln!("Nana 还到的控件随后不在了，在它那一片里重新找焦点：{}", target.get());
        match restore(context, document, &watched, regions) {
            Some(next) => self.watch(context, document, next, regions),
            None => self.stall(watched),
        }
    }

    /// 哪里都放不下焦点：留着这个目标，等它所在的区域回来再还。只在头一次记日志。
    fn stall(&mut self, origin: Origin) {
        if !self.stalled {
            eprintln!("Nana 浮层关掉以后暂时找不到能还焦点的控件，等它所在的区域回来再还：{:?}", origin.opener.map(StableNodeId::get));
            self.stalled = true;
        }
        self.pending = Some(origin);
    }

    /// 从此盯着 `target`：它随后被删掉或换掉时在它那一片里重新找。
    fn watch(&mut self, context: &AppContext, document: DocumentId, target: StableNodeId, regions: Regions) {
        let world = context.world();
        if world.focused(document) != Some(target) || within(world, target, regions.layer) {
            self.watching = None;
            return;
        }
        self.watching = Some(Origin::of(world, target, regions));
    }

    /// 新开的一层不是右键菜单、也没有接上一层的目标时，记下它自己的：打开前焦点所在的页面控件；
    /// 没有时接着还欠着的目标、刚关掉的上一层，最后是所属区域。
    fn origin_for(
        &self,
        context: &AppContext,
        model: &ShellViewModel,
        surface: &Surface,
        before: Option<StableNodeId>,
        closed: &[Layer],
        regions: Regions,
    ) -> Origin {
        if let Some(focused) = before.filter(|focused| page_control(context, model, *focused, regions)) {
            return Origin::of(context.world(), focused, regions);
        }
        if let Some(pending) = &self.pending {
            return pending.clone();
        }
        if let Some(layer) = closed.last() {
            return layer.origin.clone();
        }
        Origin::home(surface)
    }

    /// 新开的一层接手焦点：焦点随上一层没了时，弹层换页跟进新的一页，对话框打开前先放回打开它的
    /// 控件（框架打开对话框时记下它，关上以后还回来）。别的层不动焦点。返回这一层的归还目标：打开它的
    /// 控件已经不在、焦点放到了它那一片里别的控件上时，换成那个控件。
    #[allow(clippy::too_many_arguments)]
    fn enter(
        &mut self,
        context: &mut AppContext,
        document: DocumentId,
        model: &ShellViewModel,
        surface: &Surface,
        origin: Origin,
        handed: bool,
        regions: Regions,
    ) -> Origin {
        if focus_state(context, document, model, regions) != FocusState::Lost {
            return origin;
        }
        if handed && !surface.modal() && follow(context, document, regions.overlay) {
            return origin;
        }
        if !surface.modal() && !handed {
            return origin;
        }
        match restore(context, document, &origin, regions) {
            Some(target) if origin.opener != Some(target) => Origin { home: origin.home, ..Origin::of(context.world(), target, regions) },
            Some(_) => origin,
            None => {
                eprintln!("Nana 打开 {surface:?} 时找不到打开它的控件，关上以后按所属区域找落脚点");
                origin
            }
        }
    }
}

/// 现在开着的层：浮层块显示的那一层和开着的「导入」菜单。
fn surfaces(model: &ShellViewModel, shown: Option<&OverlayIdentity>) -> Vec<Surface> {
    let mut surfaces = Vec::new();
    if let Some(identity) = shown {
        surfaces.push(Surface::Overlay(identity.clone()));
    }
    if MenuSurface::Import.is_open(model) {
        surfaces.push(Surface::ImportMenu);
    }
    surfaces
}

/// 右键菜单所属的控件（文件条目、文件夹行）的键。
fn context_anchor(surface: &Surface, model: &ShellViewModel) -> Option<String> {
    let Surface::Overlay(identity) = surface else {
        return None;
    };
    match identity.key {
        OverlayKey::EntryMenu => super::files_view::entry_menu_card_key(model),
        OverlayKey::FolderMenu => model.sidebar.folder_menu.as_ref().map(|menu| super::sidebar_view::folder_row_key(&menu.path)),
        _ => None,
    }
}

/// 焦点现在的处境。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusState {
    /// 在一个还挂着、看得见的页面控件或者开着的这一层里。
    Placed(StableNodeId),
    /// 在换下来、还没关上的对话框里：等框架关上它。
    Held,
    /// 没有焦点，或者焦点所在的控件已经卸掉、藏起来，或者留在关掉的「导入」菜单里。
    Lost,
}

fn focus_state(context: &AppContext, document: DocumentId, model: &ShellViewModel, regions: Regions) -> FocusState {
    let world = context.world();
    match world.focused(document) {
        Some(focused) if live(world, focused) && !in_closed_import_menu(context, model, focused) => {
            if within(world, focused, regions.layer) && !within(world, focused, regions.overlay) {
                FocusState::Held
            } else {
                FocusState::Placed(focused)
            }
        }
        // 对话框处理中禁用了按下的选项时焦点会被清掉；它关上以前焦点放不出去。
        _ if modal_open(context, regions.layer) => FocusState::Held,
        _ => FocusState::Lost,
    }
}

/// 浮层层里还有开着的模态对话框：换下来的对话框要到刷新时才真正关上，在那之前框架不让焦点出去。
fn modal_open(context: &AppContext, layer: Option<StableNodeId>) -> bool {
    let world = context.world();
    let mut stack: Vec<StableNodeId> = layer.into_iter().collect();
    while let Some(id) = stack.pop() {
        if let Some(root) = world.overlay_host(id).and_then(|state| state.active) {
            let open = context
                .read(Entity::<Dialog>::from_stable_id(root), |dialog| dialog.open)
                .or_else(|_| context.read(Entity::<ConfirmDialog>::from_stable_id(root), |dialog| dialog.open));
            if open.unwrap_or(false) {
                return true;
            }
        }
        if let Some(node) = world.node(id) {
            stack.extend(node.children);
        }
    }
    false
}

/// 焦点在「导入」菜单的项上，而菜单已经关了（藏起来的绑定要到刷新才生效）。
fn in_closed_import_menu(context: &AppContext, model: &ShellViewModel, focused: StableNodeId) -> bool {
    !MenuSurface::Import.is_open(model) && in_import_menu(context, focused)
}

/// `focused` 是「导入」菜单里的控件。
fn in_import_menu(context: &AppContext, focused: StableNodeId) -> bool {
    let menu = MenuSurface::Import.group_key();
    context.assembly_path(focused).is_some_and(|path| path.split('/').any(|segment| segment == menu))
}

/// 页面上的控件：挂着、看得见，不在浮层层里，也不在关掉的「导入」菜单里。
fn page_control(context: &AppContext, model: &ShellViewModel, id: StableNodeId, regions: Regions) -> bool {
    let world = context.world();
    live(world, id) && !within(world, id, regions.layer) && !in_closed_import_menu(context, model, id)
}

/// 把焦点还到 `origin`：打开它的控件还在、能聚焦就回到它，否则在它的上层里、最后在所属区域里找。
/// 返回焦点落到的控件；哪里都放不下时返回 `None`，由调用方记日志。
fn restore(context: &mut AppContext, document: DocumentId, origin: &Origin, regions: Regions) -> Option<StableNodeId> {
    if let Some(opener) = origin.opener {
        if focus(context, document, opener) {
            return Some(opener);
        }
    }
    let mut places = origin.around.clone();
    if let Some(home) = origin.home {
        places.extend(home_root(context, document, home, regions));
    }
    for place in places {
        if !live(context.world(), place) {
            continue;
        }
        if let Some(target) = selected_within(context.world(), document, place) {
            if focus(context, document, target) {
                return Some(target);
            }
        }
        if let Some(target) = first_within(context.world(), document, place) {
            if focus(context, document, target) {
                return Some(target);
            }
        }
    }
    None
}

/// 区域的根：侧栏、文件列表（不在时退到主区）、主区。
fn home_root(context: &AppContext, document: DocumentId, home: Home, regions: Regions) -> Vec<StableNodeId> {
    match home {
        Home::Sidebar => regions.sidebar.into_iter().collect(),
        Home::Files => keyed(context, document, "file-list-content").into_iter().chain(regions.primary).collect(),
        Home::Primary => regions.primary.into_iter().collect(),
        Home::None => Vec::new(),
    }
}

/// 弹层换到新的一页：焦点落在页里第一个输入框，没有输入框时落在菜单的首项。
fn follow(context: &mut AppContext, document: DocumentId, page: Option<StableNodeId>) -> bool {
    let Some(page) = page else {
        return false;
    };
    let world = context.world();
    let order = world.document_order(document);
    let field = order.iter().copied().find(|id| {
        world.is_descendant_or_self(*id, page)
            && world.accessibility(*id).is_some_and(|node| node.role == AccessibilityRole::TextInput && !node.disabled)
            && focusable(world, *id)
    });
    if let Some(field) = field {
        return focus(context, document, field);
    }
    let group = order.iter().copied().find(|id| context.roving_focus_group(*id).is_some() && world.is_descendant_or_self(*id, page));
    match group.map(|group| context.focus_roving_edge(group, RovingEdge::Start)) {
        Some(Ok(Some(_))) => true,
        Some(Err(error)) => {
            eprintln!("Nana 弹层换页后聚焦首项失败：{error}");
            false
        }
        _ => false,
    }
}

/// 原地聚焦 `target`（不滚动，和框架关上对话框时还焦点一样），成功（或者它本来就有焦点）时为真。
/// 不能聚焦时不报错，由调用方往外找。
fn focus(context: &mut AppContext, document: DocumentId, target: StableNodeId) -> bool {
    if !focusable(context.world(), target) {
        return false;
    }
    match context.focus_node_in_place(document, target) {
        Ok(_) => context.world().focused(document) == Some(target),
        Err(error) => {
            eprintln!("Nana 把焦点还到 {} 失败：{error}", target.get());
            false
        }
    }
}

/// `place` 里第一个能聚焦的控件（按文档顺序）。
fn first_within(world: &UiWorld, document: DocumentId, place: StableNodeId) -> Option<StableNodeId> {
    world.document_order(document).into_iter().find(|id| world.is_descendant_or_self(*id, place) && focusable(world, *id))
}

/// `place` 里选中的、能聚焦的项（当前文件夹、选中的文件条目），按文档顺序取第一个。
fn selected_within(world: &UiWorld, document: DocumentId, place: StableNodeId) -> Option<StableNodeId> {
    world.document_order(document).into_iter().find(|id| {
        world.is_descendant_or_self(*id, place)
            && world.accessibility(*id).is_some_and(|node| node.selected == Some(true))
            && focusable(world, *id)
    })
}

/// 挂着、看得见、能聚焦，没有禁用。
fn focusable(world: &UiWorld, id: StableNodeId) -> bool {
    live(world, id)
        && world.interaction(id).is_some_and(|interaction| interaction.focusable)
        && !world.accessibility(id).is_some_and(|node| node.disabled)
}

/// 挂在文档里（脱离文档挂着的块不算），而且沿途没有藏起来的上层。
fn live(world: &UiWorld, id: StableNodeId) -> bool {
    world.is_mounted(id) && world.is_overlay_reachable(id)
}

/// `node` 在 `ancestor` 下面（或者就是它）。
fn within(world: &UiWorld, node: StableNodeId, ancestor: Option<StableNodeId>) -> bool {
    ancestor.is_some_and(|ancestor| world.is_descendant_or_self(node, ancestor))
}

/// 键路径最后一段是 `key` 的第一个节点（按文档顺序）。
fn keyed(context: &AppContext, document: DocumentId, key: &str) -> Option<StableNodeId> {
    context
        .world()
        .document_order(document)
        .into_iter()
        .find(|id| context.assembly_path(*id).is_some_and(|path| path.rsplit('/').next() == Some(key)))
}

impl Origin {
    /// 没有打开它的控件：按这一层所属的区域找落脚点。
    fn home(surface: &Surface) -> Self {
        Self { opener: None, around: Vec::new(), home: Some(surface.home()) }
    }

    /// 以 `opener` 为归还目标：记下它从近到远的上层，到所在区域的根为止。不在哪个区域里时一直记到最外层。
    fn of(world: &UiWorld, opener: StableNodeId, regions: Regions) -> Self {
        let roots = [regions.sidebar, regions.primary];
        let mut around = Vec::new();
        let mut cursor = world.parent_id(opener);
        while let Some(id) = cursor {
            around.push(id);
            if roots.contains(&Some(id)) {
                break;
            }
            cursor = world.parent_id(id);
        }
        let home = if within(world, opener, regions.sidebar) {
            Some(Home::Sidebar)
        } else if within(world, opener, regions.primary) {
            Some(Home::Primary)
        } else {
            None
        };
        Self { opener: Some(opener), around, home }
    }
}

#[cfg(test)]
#[path = "focus_return_tests.rs"]
mod tests;

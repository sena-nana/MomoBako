//! 窗口准备阶段的动效时钟、预取和分隔条回写。

use std::cell::Cell;

use nana_ui::runtime::{component_descriptors, Entity, Workspace};
use nana_ui_core::{DropAccepts, DropEffect};
use nana_ui::{ApplicationWindow, RegionId, RuntimeProgramContext, RuntimeProgramUpdate};

use crate::shell::{ShellMessage, ShellViewModel, WindowAction};
use crate::{host_api, MomoBakoApplication};

thread_local! {
    static ESCAPE_KEY: Cell<bool> = const { Cell::new(false) };
}

/// 动效还在走时按 30 帧继续画，并把工作区拖动后的宽度写回壳层。
///
/// 指针手势先读当前文档。手势进行中不拆树，否则按下目标会随节点一起消失。
pub(crate) fn prepare_motion(shell: &mut ShellViewModel, window: &mut ApplicationWindow) {
    take_escape(shell);
    note_dismissed_dialog(shell, &window.document);
    drive_prefetch(shell);
    if shell.input.live_gesture.is_some() || shell.sidebar.hover_since_ms.is_some() {
        shell.sidebar.tick_hover(16);
    }
    let tracking = crate::shell::observe_live_pointer(shell, &window.document);
    shell.stage_browses();
    let placed = place_live_popover(shell, &window.document);
    if shell.motion.active() {
        shell.motion.advance(shell.motion.now_ms().saturating_add(16));
    }
    let refresh = shell.motion.active() || placed || shell.surface_dirty;
    if refresh && !tracking {
        shell.surface_dirty = false;
        if let Err(error) = crate::shell::mount_shell(&mut window.document, shell) {
            eprintln!("Nana 动效帧重建失败：{error}");
        }
    }
    window.demand = if shell.motion.active() || tracking || shell.files.prefetch_pending() {
        nana_ui::FrameDemand::Continuous(std::num::NonZeroU32::new(30).expect("30"))
    } else {
        nana_ui::FrameDemand::OnDemand
    };
    sync_sidebar_resize(shell, &window.document);
}

fn drive_prefetch(shell: &mut ShellViewModel) {
    if !shell.files.prefetch_pending() {
        return;
    }
    let now = shell.files.tick_prefetch(16);
    let paths = shell.files.undecoded_thumbnail_paths();
    shell.files.poll_thumbnail_prefetch(now, &paths);
}

fn take_escape(shell: &mut ShellViewModel) {
    let pressed = ESCAPE_KEY.with(|flag| flag.replace(false));
    if pressed {
        shell.reduce(ShellMessage::Sidebar(crate::shell::SidebarMessage::Gap(crate::shell::GapMessage::Escape)));
    }
}

fn note_dismissed_dialog(shell: &mut ShellViewModel, document: &nana_ui::runtime::RuntimeDocument) {
    if !dialog_layer_open(shell) {
        return;
    }
    let document_id = document.document();
    let present = document.context().world().project_accessibility(document_id).into_iter().any(|node| {
        matches!(node.role, nana_ui::runtime::AccessibilityRole::Dialog | nana_ui::runtime::AccessibilityRole::AlertDialog)
    });
    if present {
        return;
    }
    eprintln!("Nana 对话框已从树上消失，按 Escape 关掉壳层状态");
    shell.reduce(ShellMessage::Sidebar(crate::shell::SidebarMessage::Gap(crate::shell::GapMessage::Escape)));
}

fn dialog_layer_open(shell: &ShellViewModel) -> bool {
    shell.sidebar.folder_dialog.open
        || shell.sidebar.folder_delete_open()
        || shell.sidebar.smart_draft.open
        || shell.sidebar.smart_delete_open()
        || shell.playlist_dialog_open
        || shell.workspace.delete_dialog_open()
}

/// 文件区或空库标记的父节点接受系统文件拖放。事件由视图收成 `HostDrag`。
pub(crate) fn bind_file_drop(document: &mut nana_ui::runtime::RuntimeDocument) {
    let document_id = document.document();
    let hosts: Vec<_> = {
        let world = document.context().world();
        world
            .document_order(document_id)
            .into_iter()
            .filter_map(|id| {
                let text = world.text(id)?;
                text.starts_with("momobako-drop:").then(|| drop_host(world, id))
            })
            .collect()
    };
    for id in hosts {
        if let Err(error) = document.context_mut().set_drop_target_node(id, DropAccepts::files().effect(DropEffect::Copy)) {
            eprintln!("Nana 文件拖放目标没有挂上：{error}");
        }
    }
}

fn drop_host(world: &nana_ui::runtime::UiWorld, marker: nana_ui::runtime::StableNodeId) -> nana_ui::runtime::StableNodeId {
    world.parent_id(marker).unwrap_or(marker)
}

/// 焦点在按钮或输入框上时，Escape 关掉最上面一层。对话框遮罩会先吞掉按键，由 `note_dismissed_dialog` 补上。
pub(crate) fn bind_escape(document: &mut nana_ui::runtime::RuntimeDocument) {
    let document_id = document.document();
    let inputs: Vec<_> = document
        .context()
        .world()
        .nodes_of_component(document_id, component_descriptors::TEXT_INPUT.type_id)
        .collect();
    let buttons: Vec<_> = document
        .context()
        .world()
        .nodes_of_component(document_id, component_descriptors::BUTTON.type_id)
        .collect();
    for id in inputs {
        let entity = Entity::<nana_ui::runtime::TextInput>::from_stable_id(id);
        if let Err(error) = document.context_mut().on_key(entity, note_escape_key) {
            eprintln!("Nana Escape 没有接到输入框：{error}");
        }
    }
    for id in buttons {
        let entity = Entity::<nana_ui::runtime::Button>::from_stable_id(id);
        if let Err(error) = document.context_mut().on_key(entity, note_escape_key) {
            eprintln!("Nana Escape 没有接到按钮：{error}");
        }
    }
}

fn note_escape_key(key: &nana_ui::runtime::KeyInput) -> bool {
    if key.pressed && !key.repeat && key.key.as_ref() == "Escape" {
        ESCAPE_KEY.with(|flag| flag.set(true));
        return true;
    }
    false
}

/// 打开的仓库弹层按锚点和自身尺寸夹进视口。坐标来自当前树上的布局盒。
fn place_live_popover(shell: &mut ShellViewModel, document: &nana_ui::runtime::RuntimeDocument) -> bool {
    if !shell.sidebar.popover_is_open() {
        return false;
    }
    let document_id = document.document();
    let world = document.context().world();
    let nodes = world.project_accessibility(document_id);
    let Some(anchor) = nodes.iter().find(|node| node.label.as_deref().is_some_and(|label| label.starts_with("资源库 ·"))) else {
        return false;
    };
    let Some(title) = nodes.iter().find(|node| node.label.as_deref() == Some("资源库")) else {
        return false;
    };
    let panel = popover_box(world, title.id);
    if panel.width < 40.0 || panel.height < 40.0 {
        eprintln!("Nana 仓库弹层还没有尺寸");
        return false;
    }
    let (viewport_w, viewport_h) = viewport_size(world, document_id);
    let before = (shell.sidebar.popover_x, shell.sidebar.popover_y);
    shell.reduce(ShellMessage::Sidebar(crate::shell::SidebarMessage::Gap(crate::shell::GapMessage::PlacePopover {
        x: anchor.bounds.x,
        y: anchor.bounds.y,
        width: panel.width,
        height: panel.height,
        viewport_w,
        viewport_h,
    })));
    (shell.sidebar.popover_x, shell.sidebar.popover_y) != before
}

fn popover_box(world: &nana_ui::runtime::UiWorld, id: nana_ui::runtime::StableNodeId) -> nana_ui::runtime::LayoutBox {
    let mut current = id;
    let mut best = world.layout_box(id).unwrap_or_default();
    while let Some(parent) = world.parent_id(current) {
        let Some(bounds) = world.layout_box(parent) else {
            break;
        };
        if bounds.width > 1100.0 || bounds.height > 700.0 || bounds.width + 8.0 < best.width {
            break;
        }
        if bounds.width >= best.width && bounds.height >= best.height {
            best = bounds;
            current = parent;
            continue;
        }
        break;
    }
    best
}

fn viewport_size(world: &nana_ui::runtime::UiWorld, document_id: nana_ui::runtime::DocumentId) -> (f32, f32) {
    let mut width = 0.0_f32;
    let mut height = 0.0_f32;
    for id in world.document_order(document_id) {
        if let Some(bounds) = world.layout_box(id) {
            width = width.max(bounds.x + bounds.width);
            height = height.max(bounds.y + bounds.height);
        }
    }
    (width, height)
}

/// 分隔条的松手被 Nana 工作区吃掉。指针捕获结束时才写入宽度。
fn sync_sidebar_resize(shell: &mut ShellViewModel, document: &nana_ui::runtime::RuntimeDocument) {
    let context = document.context();
    let document_id = document.document();
    let Some(node) = context
        .world()
        .nodes_of_component(document_id, component_descriptors::WORKSPACE.type_id)
        .next()
    else {
        return;
    };
    let Ok(extent) = context.read(Entity::<Workspace>::from_stable_id(node), |workspace| {
        workspace.model.region_extent(&RegionId::Resources)
    }) else {
        return;
    };
    let dragging = context.world().pointer_captures(document_id).iter().any(|(_, node)| {
        context
            .read(Entity::<nana_ui::runtime::WorkspaceResizeHandle>::from_stable_id(*node), |_| ())
            .is_ok()
    });
    let frame = crate::shell::note_sidebar_resize(
        shell.input.sidebar_dragging,
        dragging,
        extent,
        shell.workspace.sidebar_width,
        shell.input.sidebar_resize_dirty,
    );
    shell.input.sidebar_dragging = frame.dragging;
    shell.input.sidebar_resize_dirty = frame.dirty;
    if (frame.width - shell.workspace.sidebar_width).abs() > 0.5 {
        shell.workspace.set_sidebar_width(frame.width);
    }
    if frame.persist {
        shell.workspace.commit_sidebar_width();
    }
}

/// 系统关闭请求按关闭设置回答。确认和托盘只重绘，不立刻关闭。
pub(crate) fn answer_close(
    app: &mut MomoBakoApplication,
    id: nana_ui_platform::WindowId,
    context: &RuntimeProgramContext<ShellMessage>,
) -> RuntimeProgramUpdate {
    let decision = crate::shell::input::decide_close(app.shell.settings.close_behavior.as_str(), app.shell.close_is_dirty());
    match decision {
        crate::shell::input::CloseDecision::CloseNow => RuntimeProgramUpdate {
            window_commands: host_api::WindowCommand::Close
                .to_platform_command(id, context.geometry().maximized)
                .into_iter()
                .collect(),
            ..RuntimeProgramUpdate::redraw(id)
        },
        crate::shell::input::CloseDecision::Ask { .. } | crate::shell::input::CloseDecision::HoldForTray => {
            context.dispatch(ShellMessage::WindowAction(WindowAction::Close));
            RuntimeProgramUpdate::redraw(id)
        }
    }
}

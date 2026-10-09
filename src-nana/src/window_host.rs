//! 窗口准备阶段的动效时钟、预取、分隔条回写，全局 Escape，以及关闭、托盘和几何恢复。

use nana_ui::runtime::{component_descriptors, Entity, Workspace};
use nana_ui::{ApplicationWindow, InputPayload, KeyState, RegionId, RuntimeProgramContext, RuntimeProgramUpdate};

use crate::shell::{GapMessage, ShellMessage, ShellView, ShellViewModel, SidebarMessage, WindowAction};
use crate::{host_api, MomoBakoApplication};

/// 托盘和图标失败原因。窗口几何会话跟这一扇主窗口走。
pub(crate) struct HostSession {
    pub tray: Option<crate::tray::TrayHost>,
    pub tray_error: Option<String>,
    pub windows: crate::window_state::Session,
}

impl Default for HostSession {
    fn default() -> Self {
        Self { tray: None, tray_error: None, windows: crate::window_state::Session::default() }
    }
}

/// 进程启动后建一次托盘。建失败就记下原因，之后不能把隐藏窗口说成已经进了托盘。
pub(crate) fn install_tray(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    if app.host.tray.is_some() || app.host.tray_error.is_some() {
        return;
    }
    match crate::tray::install(context.window()) {
        Ok(tray) => app.host.tray = Some(tray),
        Err(error) => {
            eprintln!("Nana 托盘没有建起来：{error}");
            app.host.tray_error = Some(error);
        }
    }
}

/// 打开、拖出和托盘请求在这一帧执行。拖出结果如果已经回来，也在这里写回壳层。
pub(crate) fn after_update(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    let window = context.window();
    let tray_ready = app.host.tray.is_some();
    let tray_error = app.host.tray_error.clone();
    crate::host_bridge::perform_live(&mut app.shell, &window, tray_ready, tray_error.as_deref());
    crate::shell::sync_media_session(&window, &app.shell);
}

/// 动效还在走时按 30 帧继续画，并把工作区拖动后的宽度写回壳层。
///
/// 指针手势先读当前文档。动效和播放推进只同步热信号，不重挂；这一帧里有过归约、标了脏
/// 或者工作台排法要换时才整体同步，手势进行中不重挂，否则按下目标会随节点一起消失。
pub(crate) fn prepare_motion(shell: &mut ShellViewModel, view: Option<&mut ShellView>, window: &mut ApplicationWindow) {
    crate::shell::poll_media_keys(shell);
    crate::drag_out::apply_result(shell);
    drive_prefetch(shell);
    if shell.input.live_gesture.is_some() || shell.sidebar.hover_since_ms.is_some() {
        shell.sidebar.tick_hover(16);
    }
    let tracking = crate::shell::observe_live_pointer(shell, &window.document);
    shell.stage_browses();
    // 拖出结果这类归约以外写下的失败收进状态区，状态区变了要整体同步。
    if shell.observe_failures() {
        shell.mark_surface_dirty();
    }
    if shell.motion.active() {
        shell.motion.advance(shell.motion.now_ms().saturating_add(16));
    }
    let mut overlay_leaving = false;
    if let Some(view) = view {
        let structural = shell.surface_dirty || view.stale(shell);
        let synced = if tracking {
            view.sync_hot(&mut window.document, shell)
        } else if structural {
            shell.surface_dirty = false;
            view.sync(&mut window.document, shell)
        } else {
            view.sync_hot(&mut window.document, shell).and_then(|()| view.retry_deferred(&mut window.document, shell))
        };
        if let Err(error) = synced {
            eprintln!("Nana 准备帧同步壳层失败：{error}");
        }
        // 换下的对话框放完退场才卸，卸掉以前浮层层还挡着点击，所以放退场期间一直要帧。
        view.settle_overlays(&mut window.document);
        overlay_leaving = view.overlay_leaving();
    }
    let busy = shell.motion.active() || tracking || overlay_leaving;
    window.demand = if busy || shell.files.prefetch_pending() || shell.inspect.timers_pending() {
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

/// 记下这次输入按着的修饰键。运行时路由以后文档不保留修饰键，点选条目（切换、范围）和
/// 框选（追加）要用；输入在消息归约之前到这里，单击消息归约时读到的就是点下去那一刻的。
pub(crate) fn note_modifiers(shell: &mut ShellViewModel, payload: &InputPayload) {
    let modifiers = match payload {
        InputPayload::Pointer(pointer) => pointer.modifiers,
        InputPayload::Key(key) => key.modifiers,
        _ => return,
    };
    shell.input.modifiers = crate::shell::input::HeldModifiers { shift: modifiers.shift, command: modifiers.control || modifiers.meta };
}

/// 运行时路由完一个输入事件以后的全局 Escape：按下、不是连发、控件没有自己处理掉（`consumed`
/// 为假），而且 ViewModel 里还有能关的一层时，发一条关掉最上面一层的消息。
///
/// 焦点在哪都一样：对话框、弹层和右键菜单的开合都记在 ViewModel 里，关哪一层由
/// [`crate::shell::escape_layer`] 按先后决定，归约时 `dismiss_top` 照同一个顺序关。下拉框的选项、
/// 数字框这类控件自己处理 Escape 时运行时会标 `prevent_default`，这里不再重复处理。
pub(crate) fn escape_message(shell: &ShellViewModel, payload: &InputPayload, consumed: bool) -> Option<ShellMessage> {
    let InputPayload::Key(key) = payload else {
        return None;
    };
    if consumed || key.state != KeyState::Pressed || key.repeat || key.logical.0 != "Escape" {
        return None;
    }
    crate::shell::escape_layer(shell)?;
    Some(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::Escape)))
}

/// 分隔条的松手被 Nana 工作区吃掉。指针捕获结束时才写入宽度。
fn sync_sidebar_resize(shell: &mut ShellViewModel, document: &nana_ui::runtime::RuntimeDocument) {
    let context = document.context();
    let document_id = document.document();
    // 主区独占时工作区停放在树外，它的区域尺寸不是侧栏宽度。
    let Some(node) = context
        .world()
        .nodes_of_component(document_id, component_descriptors::WORKSPACE.type_id)
        .find(|node| context.world().is_mounted(*node))
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

/// 几何变化、外观和文件对话框从这里进壳层。关窗请求由 `close_requested` 回答。
pub(crate) fn on_window_event(
    app: &mut MomoBakoApplication,
    event: &nana_ui_platform::WindowEvent,
    context: &RuntimeProgramContext<ShellMessage>,
) -> RuntimeProgramUpdate {
    match event {
        nana_ui_platform::WindowEvent::Ready { id, geometry } => {
            app.shell.set_viewport_width(geometry.logical_size.0);
            let window_commands = app.host.windows.on_ready(*id);
            RuntimeProgramUpdate { window_commands, ..RuntimeProgramUpdate::redraw(*id) }
        }
        nana_ui_platform::WindowEvent::Resized { id, geometry } => {
            app.host.windows.on_geometry(geometry);
            if app.shell.set_viewport_width(geometry.logical_size.0) {
                return RuntimeProgramUpdate::redraw(*id);
            }
            RuntimeProgramUpdate::default()
        }
        nana_ui_platform::WindowEvent::Moved { geometry, .. } => {
            app.host.windows.on_geometry(geometry);
            RuntimeProgramUpdate::default()
        }
        nana_ui_platform::WindowEvent::Closed { .. } => {
            app.host.windows.persist();
            RuntimeProgramUpdate::default()
        }
        nana_ui_platform::WindowEvent::AppearanceChanged { id, appearance } => {
            if crate::appearance::note_system_appearance(app, *appearance) {
                return RuntimeProgramUpdate::redraw(*id);
            }
            RuntimeProgramUpdate::default()
        }
        nana_ui_platform::WindowEvent::ReducedMotionChanged { reduced, .. } => {
            app.shell.motion.set_reduced(*reduced);
            RuntimeProgramUpdate::redraw(context.window_id())
        }
        nana_ui_platform::WindowEvent::FileDialogCompleted { id, result } => {
            let failed = result.error.as_ref().map(|error| format!("{error:?}"));
            let paths = result.paths.iter().map(|path| path.display().to_string()).collect();
            context.dispatch(ShellMessage::Input(crate::shell::input::InputMessage::FileDialogCompleted {
                request_id: result.id,
                paths,
                failed,
            }));
            RuntimeProgramUpdate::redraw(*id)
        }
        nana_ui_platform::WindowEvent::FileDialogRejected { id, request_id, error } => {
            eprintln!("Nana 文件对话框被拒绝：{error:?}");
            context.dispatch(ShellMessage::Input(crate::shell::input::InputMessage::FileDialogCompleted {
                request_id: *request_id,
                paths: Vec::new(),
                failed: Some(format!("{error:?}")),
            }));
            RuntimeProgramUpdate::redraw(*id)
        }
        _ => RuntimeProgramUpdate::default(),
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

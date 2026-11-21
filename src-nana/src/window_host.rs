//! 窗口准备阶段的进度条填充、计时器、分隔条回写，全局 Escape，以及关闭、托盘和几何恢复。
//!
//! 计时器（预取、悬停展开、仓库操作行）不要帧：每个回调开头拨钟，到点由 NanaUI 的 `wake` 叫醒触发；
//! 进度条填充、播放这些动效按帧推进。两条时间线见 `frame_clock.rs`。

use nana_ui::runtime::{Entity, Workspace, component_descriptors};
use nana_ui::{
    ApplicationWindow, InputPayload, KeyState, RegionId, RuntimeProgramContext,
    RuntimeProgramUpdate,
};

use crate::frame_clock::{FrameClock, FrameStep};
use crate::shell::{
    GapMessage, ShellMessage, ShellView, ShellViewModel, SidebarMessage, WindowAction,
};
use crate::{MomoBakoApplication, host_api};

/// 托盘和图标失败原因，以及跟这一扇主窗口走的宿主时钟。窗口几何由 NanaUI 持久化（`window_state.rs`）。
pub(crate) struct HostSession {
    /// 托盘图标装上了没有。装上的托盘在会话结束时拿掉。
    pub tray_installed: bool,
    pub tray_error: Option<String>,
    /// 宿主时钟：计时器的时间线和动效的帧步长。
    pub frames: FrameClock,
}

impl Drop for HostSession {
    fn drop(&mut self) {
        if self.tray_installed {
            crate::tray::remove();
        }
    }
}

impl Default for HostSession {
    fn default() -> Self {
        Self {
            tray_installed: false,
            tray_error: None,
            frames: FrameClock::default(),
        }
    }
}

/// 进程启动后建一次托盘。建失败就记下原因，之后不能把隐藏窗口说成已经进了托盘。
pub(crate) fn install_tray(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    if app.host.tray_installed || app.host.tray_error.is_some() {
        return;
    }
    match crate::tray::install(context) {
        Ok(()) => app.host.tray_installed = true,
        Err(error) => {
            eprintln!("Nana 托盘没有建起来：{error}");
            app.host.tray_error = Some(error);
        }
    }
}

/// 托盘菜单选中的动作：显示窗口或退出。
pub(crate) fn on_tray(
    context: &RuntimeProgramContext<ShellMessage>,
    action: crate::tray::TrayAction,
) {
    let window = context.window();
    match action {
        crate::tray::TrayAction::Show => crate::tray::show(&window),
        crate::tray::TrayAction::Quit => crate::tray::quit(&window),
    }
}

/// 打开、拖出和托盘请求在这一帧执行。拖出结果如果已经回来，也在这里写回壳层。
pub(crate) fn after_update(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    let window = context.window();
    let tray_ready = app.host.tray_installed;
    let tray_error = app.host.tray_error.clone();
    crate::host_bridge::perform_live(&mut app.shell, &window, tray_ready, tray_error.as_deref());
    crate::shell::sync_media_session(&window, &app.shell);
}

/// 进度条填充、侧栏收起展开、指针手势或播放还在走时按 30 帧继续画，并把工作区拖动后的宽度写回壳层。
/// 只有计时器在等时不要帧，到点由 `wake` 叫醒（[`next_timer_in_ms`]、[`run_timers`]）。
///
/// 指针手势先读当前文档。填充过渡、悬停和播放推进只同步热信号，不重挂；这一帧里有过归约、标了脏
/// 或者工作台排法要换时才整体同步，手势进行中不重挂，否则按下目标会随节点一起消失。
/// 预取、悬停和仓库操作行按 `step.timer_ms` 拨钟并触发到点的，填充过渡按 `step.motion_ms` 走。填充过渡每帧照
/// 文档上下文跟上减少动态效果的设置；别的动效（侧栏收起展开、底部入口淡入淡出、转圈和不确定进度）由 NanaUI
/// 推进，也看同一个设置（侧栏收起展开由 `ShellView` 按它决定播不播）。文档变了以后先把挂着的缩略图报给文件表面，
/// 预取只解码挂着的，纹理回收也按它（`thumbnail_host.rs`）。
pub(crate) fn prepare_motion(
    shell: &mut ShellViewModel,
    mut view: Option<&mut ShellView>,
    window: &mut ApplicationWindow,
    step: FrameStep,
) {
    if let Some(mounted) = view
        .as_deref_mut()
        .and_then(|view| view.mounted_thumbnails(&window.document))
    {
        shell.files.note_mounted_thumbnails(mounted);
    }
    shell
        .fills
        .set_reduced(window.document.context().reduced_motion());
    crate::shell::poll_media_keys(shell);
    crate::drag_out::apply_result(shell);
    run_timers(shell, step.timer_ms);
    let tracking = crate::shell::observe_live_pointer(shell, &window.document);
    shell.stage_browses();
    // 拖出结果这类归约以外写下的失败收进状态区，状态区变了要整体同步。
    if shell.observe_failures() {
        shell.mark_surface_dirty();
    }
    shell.fills.tick(step.motion_ms);
    let mut overlay_leaving = false;
    let mut resources_moving = false;
    if let Some(view) = view {
        let structural = shell.surface_dirty || view.stale(&window.document, shell);
        let synced = if tracking {
            view.sync_hot(&mut window.document, shell)
        } else if structural {
            shell.surface_dirty = false;
            view.sync(&mut window.document, shell)
        } else {
            view.sync_hot(&mut window.document, shell)
                .and_then(|()| view.retry_deferred(&mut window.document, shell))
        };
        if let Err(error) = synced {
            eprintln!("Nana 准备帧同步壳层失败：{error}");
        }
        // 换下的对话框放完退场才卸，卸掉以前浮层层还挡着点击，所以放退场期间一直要帧。
        view.settle_overlays(&mut window.document);
        overlay_leaving = view.overlay_leaving();
        // 侧栏收起展开时工作区每帧重投影，AppShell 要补回 body 的布局补丁；走完以前一直要帧。
        view.settle_workspace(&mut window.document);
        resources_moving = view.resources_moving();
    }
    let busy = shell.fills.active()
        || tracking
        || overlay_leaving
        || resources_moving
        || shell.player.clock_running();
    window.demand = if busy {
        nana_ui::FrameDemand::Continuous(std::num::NonZeroU32::new(30).expect("30"))
    } else {
        nana_ui::FrameDemand::OnDemand
    };
    sync_sidebar_resize(shell, &window.document);
}

/// `update` 和 `input_event` 开头：把上次同步以来流逝的时间交给所有计时器，不触发。归约里武装的计时器
/// 从这一刻起算，到点的留给 `wake` 或准备帧触发。
pub(crate) fn sync_timers(app: &mut MomoBakoApplication) {
    let step_ms = app.host.frames.sync_timers(std::time::Instant::now());
    pass_timer_time(&mut app.shell, step_ms);
}

/// 把流逝的 `step_ms` 交给所有计时器，不触发。
pub(crate) fn pass_timer_time(shell: &mut ShellViewModel, step_ms: u64) {
    shell.inspect.pass_time(step_ms);
    shell.files.tick_prefetch(step_ms);
    shell.sidebar.tick_hover(step_ms);
    shell.operation.pass_time(step_ms);
}

/// 最早到期的计时器离现在还有多少毫秒，没有在等的计时器时为 `None`：搜索和元数据保存的延时、缩略图和
/// 下一页的空闲预取、仓库操作行的爬升和收起、拖放停在文件夹上的展开。宿主据此要 NanaUI 到点叫醒。
pub(crate) fn next_timer_in_ms(shell: &ShellViewModel) -> Option<u64> {
    [
        shell.inspect.timer_due_in(),
        shell.files.prefetch_due_in(),
        shell.operation.due_in(),
        shell.sidebar.hover_due_in(),
    ]
    .into_iter()
    .flatten()
    .min()
}

/// 计时器（检视的延时除外，见 `poll_timers`）拨 `step_ms` 并触发到点的：空闲预取排下解码和下一页读取，
/// 外部拖放停满时展开文件夹，仓库操作行爬升或收起时任务弹层和「任务」计数跟着整体同步，进度条的宽度过渡到
/// 新值。
pub(crate) fn run_timers(shell: &mut ShellViewModel, step_ms: u64) {
    drive_prefetch(shell, step_ms);
    shell.sidebar.tick_hover(step_ms);
    crate::shell::tick_external_folder_hover(shell);
    if shell.operation.tick(step_ms) {
        shell.mark_surface_dirty();
        shell.follow_task_fills();
    }
}

fn drive_prefetch(shell: &mut ShellViewModel, step_ms: u64) {
    let now = shell.files.tick_prefetch(step_ms);
    if !shell.files.prefetch_pending() {
        return;
    }
    let paths = shell.files.undecoded_thumbnail_paths();
    shell.files.poll_thumbnail_prefetch(now, &paths);
    // 来源目录的下一页：到点就读，只生成缩略图，不画进列表。
    shell.files.poll_virtual_page_prefetch(now);
}

/// 记下这次输入按着的修饰键。运行时路由以后文档不保留修饰键，点选条目（切换、范围）和
/// 框选（追加）要用；输入在消息归约之前到这里，单击消息归约时读到的就是点下去那一刻的。
pub(crate) fn note_modifiers(shell: &mut ShellViewModel, payload: &InputPayload) {
    let modifiers = match payload {
        InputPayload::Pointer(pointer) => pointer.modifiers,
        InputPayload::Key(key) => key.modifiers,
        _ => return,
    };
    shell.input.modifiers = crate::shell::input::HeldModifiers {
        shift: modifiers.shift,
        command: modifiers.control || modifiers.meta,
    };
}

/// 运行时路由完一个输入事件以后的全局 Escape：按下、不是连发、控件没有自己处理掉（`consumed`
/// 为假），而且 ViewModel 里还有能关的一层时，发一条关掉最上面一层的消息。
///
/// 焦点在哪都一样：对话框、弹层和右键菜单的开合都记在 ViewModel 里，关哪一层由
/// [`crate::shell::escape_layer`] 按先后决定，归约时 `dismiss_top` 照同一个顺序关。下拉框的选项、
/// 数字框这类控件自己处理 Escape 时运行时会标 `prevent_default`，这里不再重复处理。
pub(crate) fn escape_message(
    shell: &ShellViewModel,
    payload: &InputPayload,
    consumed: bool,
) -> Option<ShellMessage> {
    let InputPayload::Key(key) = payload else {
        return None;
    };
    if consumed || key.state != KeyState::Pressed || key.repeat || key.logical.0 != "Escape" {
        return None;
    }
    crate::shell::escape_layer(shell)?;
    Some(ShellMessage::Sidebar(SidebarMessage::Gap(
        GapMessage::Escape,
    )))
}

/// 分隔条的松手被 Nana 工作区吃掉。指针捕获结束时才写入宽度。
fn sync_sidebar_resize(shell: &mut ShellViewModel, document: &nana_ui::runtime::RuntimeDocument) {
    let context = document.context();
    let document_id = document.document();
    // 主区独占时工作区停放在树外，它的区域尺寸不是侧栏宽度；资源区收着或正在收起展开时也不是。
    let Some(node) = context
        .world()
        .nodes_of_component(document_id, component_descriptors::WORKSPACE.type_id)
        .find(|node| context.world().is_mounted(*node))
    else {
        return;
    };
    let Ok(Some(extent)) = context.read(Entity::<Workspace>::from_stable_id(node), |workspace| {
        let settled = !workspace.model.region_transitioning(&RegionId::Resources)
            && workspace
                .layout
                .region(&RegionId::Resources)
                .is_some_and(|region| !region.collapsed_value());
        settled.then(|| workspace.model.region_extent(&RegionId::Resources))
    }) else {
        return;
    };
    let dragging = context
        .world()
        .pointer_captures(document_id)
        .iter()
        .any(|(_, node)| {
            context
                .read(
                    Entity::<nana_ui::runtime::WorkspaceResizeHandle>::from_stable_id(*node),
                    |_| (),
                )
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

/// 记下窗口逻辑尺寸。相对上次变了至少 1px 时，收起文件菜单和文件夹菜单。
/// 发出的消息和失焦一样经 `update` 归约。尺寸没变或无效时不发。
fn note_window_resize(
    app: &mut MomoBakoApplication,
    geometry: &nana_ui_platform::WindowGeometry,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    let (width, height) = geometry.logical_size;
    for message in crate::shell::window_resize_messages(&mut app.shell, width, height) {
        context.dispatch(message);
    }
}

/// 几何变化、外观和文件对话框从这里进壳层。关窗请求由 `close_requested` 回答。
/// 窗口逻辑尺寸相对上次记下的像素变了时，收起文件菜单和文件夹菜单。
pub(crate) fn on_window_event(
    app: &mut MomoBakoApplication,
    event: &nana_ui_platform::WindowEvent,
    context: &RuntimeProgramContext<ShellMessage>,
) -> RuntimeProgramUpdate {
    match event {
        nana_ui_platform::WindowEvent::Ready { id, geometry } => {
            // 打开时记下尺寸。随后的缩放才和它对，尺寸没变不关菜单。
            note_window_resize(app, geometry, context);
            app.shell.set_viewport_width(geometry.logical_size.0);
            RuntimeProgramUpdate::redraw(*id)
        }
        nana_ui_platform::WindowEvent::Resized { id, geometry } => {
            note_window_resize(app, geometry, context);
            if app.shell.set_viewport_width(geometry.logical_size.0) {
                return RuntimeProgramUpdate::redraw(*id);
            }
            RuntimeProgramUpdate::default()
        }
        nana_ui_platform::WindowEvent::AppearanceChanged { id, appearance } => {
            if crate::appearance::note_system_appearance(app, *appearance) {
                return RuntimeProgramUpdate::redraw(*id);
            }
            RuntimeProgramUpdate::default()
        }
        // NanaUI 在转发这个事件以前已经把系统设置交给了文档上下文，视图层的过渡由它自己跟上；
        // 进度条填充在下一次准备帧里照文档上下文跟上（`prepare_motion`）。
        nana_ui_platform::WindowEvent::ReducedMotionChanged { .. } => {
            RuntimeProgramUpdate::redraw(context.window_id())
        }
        nana_ui_platform::WindowEvent::FileDialogCompleted { id, result } => {
            let failed = result.error.as_ref().map(|error| format!("{error:?}"));
            let paths = result
                .paths
                .iter()
                .map(|path| path.display().to_string())
                .collect();
            context.dispatch(ShellMessage::Input(
                crate::shell::input::InputMessage::FileDialogCompleted {
                    request_id: result.id,
                    paths,
                    failed,
                },
            ));
            RuntimeProgramUpdate::redraw(*id)
        }
        nana_ui_platform::WindowEvent::FileDialogRejected {
            id,
            request_id,
            error,
        } => {
            eprintln!("Nana 文件对话框被拒绝：{error:?}");
            context.dispatch(ShellMessage::Input(
                crate::shell::input::InputMessage::FileDialogCompleted {
                    request_id: *request_id,
                    paths: Vec::new(),
                    failed: Some(format!("{error:?}")),
                },
            ));
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
    let decision = crate::shell::input::decide_close(
        app.shell.settings.close_behavior.as_str(),
        app.shell.close_is_dirty(),
    );
    match decision {
        crate::shell::input::CloseDecision::CloseNow => RuntimeProgramUpdate {
            window_commands: vec![
                host_api::WindowCommand::Close
                    .to_platform_command(id, context.geometry().maximized),
            ],
            ..RuntimeProgramUpdate::redraw(id)
        },
        crate::shell::input::CloseDecision::Ask { .. }
        | crate::shell::input::CloseDecision::HoldForTray => {
            context.dispatch(ShellMessage::WindowAction(WindowAction::Close));
            RuntimeProgramUpdate::redraw(id)
        }
    }
}

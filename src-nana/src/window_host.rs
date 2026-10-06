//! 窗口准备阶段的动效时钟和分隔条回写。

use nana_ui::runtime::{component_descriptors, Entity, Workspace};
use nana_ui::{ApplicationWindow, RegionId, RuntimeProgramContext, RuntimeProgramUpdate};

use crate::shell::{ShellMessage, ShellViewModel, WindowAction};
use crate::{host_api, MomoBakoApplication};

/// 动效还在走时按 30 帧继续画，并把工作区拖动后的宽度写回壳层。
///
/// 指针手势先读当前文档。手势进行中不拆树，否则按下目标会随节点一起消失。
pub(crate) fn prepare_motion(shell: &mut ShellViewModel, window: &mut ApplicationWindow) {
    let tracking = crate::shell::observe_live_pointer(shell, &window.document);
    if shell.motion.active() {
        shell.motion.advance(shell.motion.now_ms().saturating_add(16));
        if !tracking {
            if let Err(error) = crate::shell::mount_shell(&mut window.document, shell) {
                eprintln!("Nana 动效帧重建失败：{error}");
            }
        }
    }
    window.demand = if shell.motion.active() || tracking {
        nana_ui::FrameDemand::Continuous(std::num::NonZeroU32::new(30).expect("30"))
    } else {
        nana_ui::FrameDemand::OnDemand
    };
    sync_sidebar_resize(shell, &window.document);
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

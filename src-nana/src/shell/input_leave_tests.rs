//! 指针离开窗口时，把内部拖动交给已有的 `WindowPointerLeave`。
//!
//! 对应 Vue `useWorkspaceDragDrop` 的 `handleWindowPointerLeave`：拖够 72px 再离开窗口就交给系统拖放，
//! 没拖够则留下会话。测试只走消息，不开窗口、不开声卡。

use nana_ui::InputPayload;
use nana_ui_platform::PointerId;

use super::super::{ShellMessage, ShellViewModel};
use super::InputMessage;
use crate::host_api::{HostInputRequest, HostRequest};

/// 主键指针离开窗口。
fn leave(id: u64) -> InputPayload {
    InputPayload::PointerLeave {
        pointer_id: PointerId(id),
    }
}

fn ready() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    model.repository_id = Some("repo".into());
    model.files.current_path = "photos".into();
    model
}

fn begin() -> InputMessage {
    InputMessage::BeginEntryDrag {
        path: "a.png".into(),
        selected: vec!["a.png".into()],
        x: 0.0,
        y: 0.0,
        writable: true,
        trash: false,
        smart_folder: false,
        backend_kind: "filesystem".into(),
        repo_root: "C:\\repo".into(),
        hover_folder: None,
        over_browser: true,
    }
}

fn drag_to(model: &mut ShellViewModel, x: f32, y: f32) {
    model.reduce(ShellMessage::Input(InputMessage::EntryDragMove {
        x,
        y,
        bounds_width: 400.0,
        bounds_height: 400.0,
        hover_folder: None,
        over_browser: true,
    }));
}

/// 指针还在窗口里拖够 72px 时，移动本身不交给系统；离开窗口才交。
/// 没拖够、没有拖动、或不是主键指针时不交。
#[test]
fn pointer_leave_hands_a_long_drag_to_the_system() {
    let mut model = ready();
    assert!(
        super::pointer_leave_message(&model, &leave(1)).is_none(),
        "没有拖动时不发"
    );

    model.reduce(ShellMessage::Input(begin()));
    drag_to(&mut model, 10.0, 0.0);
    let short =
        super::pointer_leave_message(&model, &leave(1)).expect("短拖动离开也要交给已有归约");
    assert!(matches!(
        short,
        ShellMessage::Input(InputMessage::WindowPointerLeave { x, y }) if x == 10.0 && y == 0.0
    ));
    model.reduce(short);
    assert!(
        model.input.session.is_some(),
        "没拖够 72px，离开窗口不交给系统"
    );
    assert!(model.input.host_requests.is_empty());
    assert!(
        super::pointer_leave_message(&model, &leave(2)).is_none(),
        "别的指针离开不打断这次拖动"
    );

    drag_to(&mut model, 80.0, 4.0);
    assert!(
        model.input.session.is_some(),
        "指针还在窗口里，移动本身不交给系统"
    );
    let long = super::pointer_leave_message(&model, &leave(1)).expect("拖够了离开窗口");
    assert!(matches!(
        long,
        ShellMessage::Input(InputMessage::WindowPointerLeave { x, y }) if x == 80.0 && y == 4.0
    ));
    model.reduce(long);
    assert!(model.input.session.is_none(), "拖够了再离开窗口，会话结束");
    assert!(
        matches!(
            model.input.host_requests.last(),
            Some(HostRequest::Input(HostInputRequest::DragOut { paths })) if paths == &["C:\\repo\\a.png".to_string()]
        ),
        "交给系统拖放"
    );
}

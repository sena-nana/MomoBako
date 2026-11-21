//! 框选、打开与定位、外部拖出、外部链接协议限制与失焦时交给系统拖动的测试。

use super::*;

#[test]
fn box_selection_open_reveal_and_external_drag_follow_their_guards() {
    let mut model = ready("repo");
    model.files.set_drag_selection(
        vec!["a.png".into()],
        Some("a.png".into()),
        Some("old".into()),
    );
    send(
        &mut model,
        InputMessage::BoxSelect {
            paths: Vec::new(),
            append: true,
        },
    );
    assert_eq!(model.files.selected_paths(), ["a.png"]);
    send(
        &mut model,
        InputMessage::BoxSelect {
            paths: vec!["a.png".into(), "b.png".into()],
            append: true,
        },
    );
    assert_eq!(model.files.selected_paths(), ["a.png", "b.png"]);
    assert_eq!(model.files.primary.as_deref(), Some("a.png"));
    assert_eq!(model.files.anchor.as_deref(), Some("a.png"));
    send(
        &mut model,
        InputMessage::BoxSelect {
            paths: Vec::new(),
            append: false,
        },
    );
    assert!(model.files.selected_paths().is_empty());
    assert!(model.files.primary.is_none());
    send(
        &mut model,
        InputMessage::BoxSelect {
            paths: vec!["c.png".into(), "d.png".into()],
            append: false,
        },
    );
    assert_eq!(model.files.primary.as_deref(), Some("c.png"));
    assert_eq!(model.files.anchor.as_deref(), Some("c.png"));

    // 守卫拦下的请求不清状态区里的失败，真正开始的打开才清。
    model.status.fail(FailureSource::Host, "定位失败：上一次");
    send(
        &mut model,
        InputMessage::OpenEntry {
            has_repo: false,
            absolute_path: "C:\\a.png".into(),
            path: "a.png".into(),
        },
    );
    send(
        &mut model,
        InputMessage::OpenEntry {
            has_repo: true,
            absolute_path: " ".into(),
            path: "a.png".into(),
        },
    );
    send(
        &mut model,
        InputMessage::RevealEntry {
            absolute_path: " ".into(),
        },
    );
    send(
        &mut model,
        InputMessage::OpenExternalUrl { url: " ".into() },
    );
    assert!(
        model.status.failure().is_some(),
        "守卫拦下的请求不该清掉上一次失败"
    );
    assert!(model.input.host_requests.is_empty());
    send(
        &mut model,
        InputMessage::OpenEntry {
            has_repo: true,
            absolute_path: "C:\\a.png".into(),
            path: "a.png".into(),
        },
    );
    assert!(model.status.failure().is_none());
    send(
        &mut model,
        InputMessage::RevealEntry {
            absolute_path: "C:\\a.png".into(),
        },
    );
    assert!(
        matches!(model.input.host_requests.last(), Some(crate::host_api::HostRequest::OpenExternal(request)) if request.reveal && request.target == "C:\\a.png")
    );
    send(
        &mut model,
        InputMessage::OpenExternalUrl {
            url: "https://momobako.local".into(),
        },
    );
    assert!(model.status.failure().is_none());
    assert_eq!(model.input.host_requests.len(), 3);

    send(
        &mut model,
        InputMessage::StartExternalDrag {
            paths: vec!["a.png".into()],
            trash: true,
            backend_kind: "filesystem".into(),
            repo_root: "C:\\repo".into(),
        },
    );
    assert_eq!(model.input.external_drag_result, Some(false));
    assert_eq!(model.input.host_requests.len(), 3);
    send(
        &mut model,
        InputMessage::StartExternalDrag {
            paths: vec!["a.png".into()],
            trash: false,
            backend_kind: "webdav".into(),
            repo_root: "C:\\repo".into(),
        },
    );
    send(
        &mut model,
        InputMessage::StartExternalDrag {
            paths: vec![" ".into()],
            trash: false,
            backend_kind: "filesystem".into(),
            repo_root: "C:\\repo".into(),
        },
    );
    assert_eq!(model.input.host_requests.len(), 3);
    model.status.fail(FailureSource::Host, "拖出失败：上一次");
    send(
        &mut model,
        InputMessage::StartExternalDrag {
            paths: vec!["a.png".into()],
            trash: false,
            backend_kind: "filesystem".into(),
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(model.status.failure().is_none(), "拖出开始时清掉上一次失败");
    assert_eq!(model.input.external_drag_result, None);

    model.input.external_active = true;
    model.input.dragging_files = true;
    model.input.dragging_repository_folder = true;
    model.status.fail(FailureSource::Host, "保留");
    send(&mut model, InputMessage::ClearDrag);
    assert!(!model.input.external_active);
    assert!(!model.input.dragging_files);
    assert!(!model.input.dragging_repository_folder);
    assert_eq!(
        model
            .status
            .failure()
            .map(|failure| failure.message.as_str()),
        Some("保留")
    );
}

/// 外部网址只排 http、https、mailto、tel：盘符路径和 `file:` 不交给系统；能开的整串原样交出（去掉首尾空白），
/// 链接里的 `&` 由宿主当成网址的一部分，不会被拆开。
#[test]
fn external_urls_outside_the_allowed_protocols_are_not_opened() {
    let mut model = ready("repo");
    for url in [
        "C:\\Windows\\System32\\calc.exe",
        "file:///C:/Windows/System32/calc.exe",
        "eagle://item/1",
        "javascript:alert(1)",
    ] {
        send(
            &mut model,
            InputMessage::OpenExternalUrl { url: url.into() },
        );
    }
    assert!(
        model.input.host_requests.is_empty(),
        "协议不在范围内的网址不该排给宿主"
    );

    send(
        &mut model,
        InputMessage::OpenExternalUrl {
            url: " https://example.com/?a=1&calc ".into(),
        },
    );
    assert!(matches!(
        model.input.host_requests.as_slice(),
        [crate::host_api::HostRequest::OpenExternal(request)] if !request.reveal && request.target == "https://example.com/?a=1&calc"
    ));
}

/// 窗口失焦把已经拖远的内部拖动交给系统。
/// 拖得还不够远时会话留着。窗口还聚焦时不发这条消息。不开窗口。
#[test]
fn focus_loss_hands_a_long_drag_to_the_system() {
    let mut model = ready("repo");
    send(&mut model, begin("a.png", &["a.png"], 0.0));
    send(
        &mut model,
        InputMessage::EntryDragMove {
            x: 10.0,
            y: 10.0,
            bounds_width: 400.0,
            bounds_height: 400.0,
            hover_folder: None,
            over_browser: true,
        },
    );
    assert!(model.input.session.is_some(), "短拖动还在窗口里");
    let staying =
        crate::shell::focus_loss_messages(&model, &nana_ui::InputPayload::Focus { focused: true });
    assert!(staying.is_empty(), "窗口还聚焦时不交拖动");
    let short =
        crate::shell::focus_loss_messages(&model, &nana_ui::InputPayload::Focus { focused: false });
    assert!(
        matches!(
            short.as_slice(),
            [ShellMessage::Input(InputMessage::WindowBlur)]
        ),
        "失焦把拖动会话交回已有的归约"
    );
    for message in short {
        model.reduce(message);
    }
    assert!(model.input.session.is_some(), "没拖够 72px，失焦不交给系统");
    assert!(
        model.input.host_requests.is_empty(),
        "短拖动失焦不产生系统拖放"
    );

    send(
        &mut model,
        InputMessage::EntryDragMove {
            x: 80.0,
            y: 10.0,
            bounds_width: 400.0,
            bounds_height: 400.0,
            hover_folder: None,
            over_browser: true,
        },
    );
    assert!(
        model.input.session.is_some(),
        "指针还在窗口里，移动本身不交给系统"
    );
    let long =
        crate::shell::focus_loss_messages(&model, &nana_ui::InputPayload::Focus { focused: false });
    assert!(matches!(
        long.as_slice(),
        [ShellMessage::Input(InputMessage::WindowBlur)]
    ));
    for message in long {
        model.reduce(message);
    }
    assert!(model.input.session.is_none(), "拖够了再失焦，会话结束");
    assert!(
        matches!(model.input.host_requests.last(), Some(HostRequest::Input(HostInputRequest::DragOut { paths })) if paths == &["C:\\repo\\a.png".to_string()]),
        "交给系统拖放"
    );
}

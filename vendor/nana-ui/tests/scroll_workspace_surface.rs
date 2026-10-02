use nana_ui::{HeadlessInput, runtime::*};
use nana_ui_core::{
    RadiusTier, RegionId, RegionRole, RegionState, WorkspaceLayout, WorkspaceModel,
};
use nana_ui_platform::{
    InputModifiers, InputPayload, PointerId, PointerInput, PointerPhase, PointerType,
};
fn pointer(phase: PointerPhase, x: f32, y: f32) -> InputPayload {
    InputPayload::Pointer(PointerInput {
        phase,
        pointer_id: PointerId(1),
        pointer_type: PointerType::Mouse,
        x,
        y,
        screen_x: x,
        screen_y: y,
        button: 0,
        buttons: u16::from(phase != PointerPhase::Up),
        pressure: 1.0,
        tangential_pressure: 0.0,
        tilt_x: 0,
        tilt_y: 0,
        twist: 0,
        is_primary: true,
        activation_click: false,
        modifiers: InputModifiers::default(),
    })
}

#[test]
fn borrowed_workspace_scrollport_preserves_region_surface_through_hover_and_drag() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let scroll = cx
        .create_detached_component(
            doc,
            ScrollView::new(ScrollAxes::Vertical).label("project content"),
        )
        .unwrap();
    let content = cx
        .create_detached_component(
            doc,
            Stack::column(0.0).height(nana_ui_core::LengthSpec::Px(900.0)),
        )
        .unwrap();
    cx.append_child(scroll, content).unwrap();
    let resources = cx
        .create_detached_component(doc, Stack::column(0.0))
        .unwrap();
    let model = WorkspaceModel::with_layout(
        WorkspaceLayout::new([
            RegionState::new(RegionId::Resources, RegionRole::Resources).size(120.0),
            RegionState::new(RegionId::Primary, RegionRole::Primary).fill_priority(1),
        ])
        .unwrap(),
    );
    let workspace = cx
        .create_component(
            doc,
            Workspace::from_model(
                &model,
                [
                    WorkspaceRegionSlot::new(RegionId::Resources, resources.stable_id()),
                    WorkspaceRegionSlot::borrowed(RegionId::Primary, scroll.stable_id()),
                ],
            ),
        )
        .unwrap();
    cx.assemble_workspace(workspace).unwrap();
    cx.layout_document(doc, LayoutViewport::new(500.0, 400.0))
        .unwrap();
    cx.rebuild_hit_test(doc);
    assert!(
        cx.world()
            .interaction(scroll.stable_id())
            .unwrap()
            .pointer_events
    );
    assert_eq!(
        cx.world()
            .accessibility(scroll.stable_id())
            .unwrap()
            .label
            .as_deref(),
        Some("project content")
    );
    cx.update_component(scroll, |view, _| {
        view.label = Some("renamed project content".into())
    })
    .unwrap();
    cx.reproject_component(workspace).unwrap();
    let style = cx.world().node_style(scroll.stable_id()).unwrap().clone();
    let accessibility = cx
        .world()
        .accessibility(scroll.stable_id())
        .unwrap()
        .clone();
    let interaction = cx.world().interaction(scroll.stable_id()).unwrap();
    let bounds = cx.world().layout_box(scroll.stable_id()).unwrap();
    assert_eq!(style.background, Some(SemanticColorRole::Background));
    // Workspace corners name the tier and let the installed theme pick the
    // number; the radius no longer lands in `layout.border_radius`.
    assert_eq!(style.radius, Some(RadiusTier::Lg));
    assert_eq!(
        accessibility.label.as_deref(),
        Some("renamed project content")
    );
    assert!(
        !cx.world()
            .interaction(resources.stable_id())
            .unwrap()
            .pointer_events
    );
    assert_eq!(
        cx.world()
            .accessibility(resources.stable_id())
            .unwrap()
            .label
            .as_deref(),
        Some("resources")
    );
    assert!(!cx.read(scroll, |view| view.scrollbars_revealed()).unwrap());
    let (hover_x, hover_y) = cx
        .world()
        .layout_pointer_position(
            scroll.stable_id(),
            bounds.x + bounds.width * 0.5,
            bounds.y + bounds.height * 0.5,
        )
        .unwrap();
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    adapter
        .route(&mut cx, pointer(PointerPhase::Move, hover_x, hover_y))
        .unwrap();
    assert!(
        cx.read(scroll, |view| view.scrollbars_revealed()).unwrap(),
        "scroll={:?}, bounds={:?}, hovered={:?}, candidates={:?}, content_bounds={:?}",
        scroll.stable_id(),
        bounds,
        cx.world().pointer_hover(doc, 1),
        cx.world().hit_test_candidates(doc, hover_x, hover_y),
        cx.world().layout_box(content.stable_id())
    );
    assert_eq!(cx.world().node_style(scroll.stable_id()), Some(&style));
    let ComponentGeometry::Scrollbar {
        vertical: Some(bar),
        ..
    } = cx.world().component_geometry(scroll.stable_id()).unwrap()
    else {
        panic!("visible scrollbar")
    };
    let x = bar.thumb.x + bar.thumb.width / 2.0;
    let y = bar.thumb.y + bar.thumb.height / 2.0;
    adapter
        .route(&mut cx, pointer(PointerPhase::Down, x, y))
        .unwrap();
    assert_eq!(cx.world().pointer_capture(doc, 1), Some(scroll.stable_id()));
    adapter
        .route(&mut cx, pointer(PointerPhase::Move, x, y + 70.0))
        .unwrap();
    assert!(cx.world().scroll_offset(scroll.stable_id()).unwrap().y > 0.0);
    assert_eq!(cx.world().node_style(scroll.stable_id()), Some(&style));
    adapter
        .route(&mut cx, pointer(PointerPhase::Up, x, y + 70.0))
        .unwrap();
    assert!(cx.read(scroll, |view| view.dragging.is_none()).unwrap());
    adapter
        .route(&mut cx, pointer(PointerPhase::Move, -20.0, -20.0))
        .unwrap();
    assert!(!cx.read(scroll, |view| view.scrollbars_revealed()).unwrap());
    assert_eq!(cx.world().node_style(scroll.stable_id()), Some(&style));
    assert_eq!(
        cx.world().accessibility(scroll.stable_id()),
        Some(&accessibility)
    );
    assert_eq!(
        cx.world().interaction(scroll.stable_id()),
        Some(interaction)
    );
    assert_eq!(cx.world().layout_box(scroll.stable_id()), Some(bounds));
}

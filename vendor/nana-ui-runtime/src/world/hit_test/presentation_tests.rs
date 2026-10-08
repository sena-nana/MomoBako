use super::*;
use crate::{
    AccessibilityRole, AccessibilityState, AnimatableProperty, AnimationId, AnimationSpec, Easing,
    MotionTo, MotionValue,
};
use nana_ui_core::{PaintMat4, PaintTransform};
use std::{sync::Arc, time::Duration};

fn node(value: u64) -> StableNodeId {
    StableNodeId::new(value).unwrap()
}

fn document(value: u64) -> DocumentId {
    DocumentId::new(value).unwrap()
}

fn box_at(x: f32, y: f32, width: f32, height: f32) -> LayoutBox {
    LayoutBox {
        x,
        y,
        width,
        height,
    }
}

fn transform_overlay(
    id: u64,
    target: StableNodeId,
    from: PaintTransform,
    to: PaintTransform,
) -> AnimationSpec {
    AnimationSpec::new(
        AnimationId::new(id).unwrap(),
        target,
        Duration::ZERO,
        Duration::from_millis(100),
        Duration::from_millis(16),
        Easing::Linear,
    )
    .with_property(AnimatableProperty::Transform)
    .with_range(
        MotionValue::Transform(from),
        MotionTo::Value(MotionValue::Transform(to)),
    )
}

fn width_overlay(id: u64, target: StableNodeId, from: f32, to: f32) -> AnimationSpec {
    AnimationSpec::new(
        AnimationId::new(id).unwrap(),
        target,
        Duration::ZERO,
        Duration::from_millis(100),
        Duration::from_millis(16),
        Easing::Linear,
    )
    .with_property(AnimatableProperty::Width)
    .with_range(
        MotionValue::Scalar(from),
        MotionTo::Value(MotionValue::Scalar(to)),
    )
}

fn style_with_transform(transform: PaintTransform) -> NodeStyle {
    NodeStyle {
        layout: Arc::new(LayoutStyle {
            transform: Some(transform),
            ..LayoutStyle::default()
        }),
        ..NodeStyle::default()
    }
}

fn commit_button(
    world: &mut UiWorld,
    layout: LayoutBox,
    style: NodeStyle,
    animation: AnimationSpec,
) {
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.write_layout(node(1), layout);
    queue.set_style(node(1), style);
    queue.set_interaction(
        node(1),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.start_animation(animation);
    world.commit(queue).unwrap();
    world.advance_animations(Duration::ZERO);
    world.rebuild_hit_test(document(1));
}

#[test]
fn hit_test_follows_presentation_translate_not_logical_target() {
    let mut world = UiWorld::new();
    let target = PaintTransform {
        e: 40.0,
        ..PaintTransform::default()
    };
    commit_button(
        &mut world,
        box_at(0.0, 0.0, 40.0, 40.0),
        style_with_transform(target),
        transform_overlay(1, node(1), PaintTransform::default(), target),
    );

    let before = world.presentation_values_cpu_sampled();
    let idle = world.advance_animations(Duration::from_millis(50));
    assert!(idle.samples.is_empty());
    assert_eq!(idle.animation_deadlines_scanned, 0);
    assert_eq!(idle.animations_considered, 0);
    assert_eq!(world.presentation_values_cpu_sampled(), before);
    assert_eq!(
        world.motion_work_counters().presentation_values_cpu_sampled,
        before
    );
    assert_eq!(
        world.node_style(node(1)).unwrap().layout.transform,
        Some(target),
        "logical transform stays at the target"
    );

    assert_eq!(world.hit_test(document(1), 30.0, 10.0), Some(node(1)));
    assert_eq!(world.hit_test(document(1), 70.0, 10.0), None);
    assert!(world.presentation_values_cpu_sampled() > before);

    let local = world
        .pointer_layout_position(node(1), 30.0, 10.0)
        .expect("maps through presentation");
    assert!((local.0 - 10.0).abs() < 1e-4, "got {}", local.0);
    assert!((local.1 - 10.0).abs() < 1e-4, "got {}", local.1);
}

#[test]
fn hit_test_follows_presentation_scale_not_logical_target() {
    let mut world = UiWorld::new();
    let target = PaintTransform {
        a: 2.0,
        d: 2.0,
        ..PaintTransform::default()
    };
    commit_button(
        &mut world,
        box_at(0.0, 0.0, 40.0, 40.0),
        style_with_transform(target),
        transform_overlay(1, node(1), PaintTransform::default(), target),
    );
    world.advance_animations(Duration::from_millis(50));

    // Mid scale is 1.5 around the 20,20 origin → visual box -10..50.
    // Logical scale 2 occupies -20..60, so x=55 is the logical rest, not presentation.
    assert_eq!(world.hit_test(document(1), 20.0, 20.0), Some(node(1)));
    assert_eq!(world.hit_test(document(1), 55.0, 20.0), None);
    assert_eq!(
        world.hit_test_candidates(document(1), 20.0, 20.0).first(),
        Some(&node(1))
    );
}

#[test]
fn hit_test_respects_ellipse_clip_path() {
    let mut world = UiWorld::new();
    let style = NodeStyle {
        layout: Arc::new(LayoutStyle {
            paint: nana_ui_core::PaintStyle {
                clip_path: Some(nana_ui_core::ClipPath::Circle(nana_ui_core::ClipCircle {
                    radius: nana_ui_core::ClipShapeRadius::ClosestSide,
                    cx: nana_ui_core::LengthSpec::Percent(50.0),
                    cy: nana_ui_core::LengthSpec::Percent(50.0),
                })),
                ..nana_ui_core::PaintStyle::default()
            }
            .into(),
            ..LayoutStyle::default()
        }),
        ..NodeStyle::default()
    };
    commit_button(
        &mut world,
        box_at(0.0, 0.0, 100.0, 100.0),
        style,
        transform_overlay(
            1,
            node(1),
            PaintTransform::default(),
            PaintTransform::default(),
        ),
    );
    assert_eq!(world.hit_test(document(1), 50.0, 50.0), Some(node(1)));
    assert_eq!(world.hit_test(document(1), 2.0, 2.0), None);
}

#[test]
fn hit_test_respects_polygon_clip_path() {
    let mut world = UiWorld::new();
    let point = |x, y| nana_ui_core::ClipPoint {
        x: nana_ui_core::LengthSpec::Percent(x),
        y: nana_ui_core::LengthSpec::Percent(y),
    };
    let style = NodeStyle {
        layout: Arc::new(LayoutStyle {
            paint: nana_ui_core::PaintStyle {
                clip_path: Some(nana_ui_core::ClipPath::Polygon(vec![
                    point(50.0, 0.0),
                    point(100.0, 100.0),
                    point(0.0, 100.0),
                ])),
                ..nana_ui_core::PaintStyle::default()
            }
            .into(),
            ..LayoutStyle::default()
        }),
        ..NodeStyle::default()
    };
    commit_button(
        &mut world,
        box_at(0.0, 0.0, 100.0, 100.0),
        style,
        transform_overlay(
            1,
            node(1),
            PaintTransform::default(),
            PaintTransform::default(),
        ),
    );
    assert_eq!(world.hit_test(document(1), 50.0, 20.0), Some(node(1)));
    assert_eq!(world.hit_test(document(1), 5.0, 5.0), None);
}

#[test]
fn hit_test_respects_rounded_inset_clip_path() {
    let mut world = UiWorld::new();
    let style = NodeStyle {
        layout: Arc::new(LayoutStyle {
            paint: nana_ui_core::PaintStyle {
                clip_path: Some(nana_ui_core::ClipPath::Inset(nana_ui_core::ClipInset {
                    top: nana_ui_core::LengthSpec::Px(0.0),
                    right: nana_ui_core::LengthSpec::Px(0.0),
                    bottom: nana_ui_core::LengthSpec::Px(0.0),
                    left: nana_ui_core::LengthSpec::Px(0.0),
                    round: Some(nana_ui_core::LengthSpec::Px(20.0)),
                })),
                ..nana_ui_core::PaintStyle::default()
            }
            .into(),
            ..LayoutStyle::default()
        }),
        ..NodeStyle::default()
    };
    commit_button(
        &mut world,
        box_at(0.0, 0.0, 100.0, 100.0),
        style,
        transform_overlay(
            1,
            node(1),
            PaintTransform::default(),
            PaintTransform::default(),
        ),
    );
    assert_eq!(world.hit_test(document(1), 50.0, 50.0), Some(node(1)));
    assert_eq!(world.hit_test(document(1), 2.0, 2.0), None);
}

#[test]
fn tiny_finite_presentation_scale_remains_hittable() {
    let transform = [0.0001, 0.0, 0.0, 0.0001, 0.0, 0.0];
    assert!(transformed_contains(
        box_at(0.0, 0.0, 1_000_000.0, 1_000_000.0),
        transform,
        [0.0, 0.0],
        50.0,
        50.0,
    ));
}

#[test]
fn layout_width_animation_keeps_logical_hit_box() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.write_layout(node(1), box_at(0.0, 0.0, 80.0, 40.0));
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(nana_ui_core::LengthSpec::Px(80.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.start_animation(width_overlay(1, node(1), 40.0, 80.0));
    world.commit(queue).unwrap();
    world.advance_animations(Duration::from_millis(50));
    world.rebuild_hit_test(document(1));

    assert!(
        world
            .presentation_applied_value(
                node(1),
                AnimatableProperty::Width,
                Duration::from_millis(50)
            )
            .is_none()
    );
    assert_eq!(world.hit_test(document(1), 70.0, 10.0), Some(node(1)));
    assert_eq!(world.hit_test(document(1), 90.0, 10.0), None);
    let bounds = world.presentation_input_bounds(node(1)).unwrap();
    assert!((bounds.width - 80.0).abs() < 1e-4, "got {}", bounds.width);
}

#[test]
fn descendant_hit_test_follows_ancestor_presentation_transform() {
    let mut world = UiWorld::new();
    let target = PaintTransform {
        e: 40.0,
        ..PaintTransform::default()
    };
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "row".into() },
    );
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.insert(node(1), node(2), None);
    queue.write_layout(node(1), box_at(0.0, 0.0, 40.0, 40.0));
    queue.write_layout(node(2), box_at(0.0, 0.0, 40.0, 40.0));
    queue.set_style(node(1), style_with_transform(target));
    queue.set_interaction(
        node(1),
        InteractionState {
            pointer_events: false,
            focusable: false,
        },
    );
    queue.start_animation(transform_overlay(
        1,
        node(1),
        PaintTransform::default(),
        target,
    ));
    world.commit(queue).unwrap();
    world.advance_animations(Duration::from_millis(50));
    world.rebuild_hit_test(document(1));

    assert_eq!(world.hit_test(document(1), 30.0, 10.0), Some(node(2)));
    assert_eq!(world.hit_test(document(1), 70.0, 10.0), None);
}

#[test]
fn hit_test_follows_projective_parent_and_motion_overlay() {
    let parent = node(1);
    let child = node(2);
    let mat = PaintMat4::perspective(800.0)
        .expect("perspective")
        .then(PaintMat4::rotate_y(30_f32.to_radians()));
    let target = PaintTransform {
        e: 60.0,
        ..PaintTransform::default()
    };
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        parent,
        document(1),
        NodeKind::Element {
            tag: "panel".into(),
        },
    );
    queue.create(
        child,
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.insert(parent, child, None);
    queue.write_layout(parent, box_at(0.0, 0.0, 128.0, 96.0));
    queue.write_layout(child, box_at(20.0, 20.0, 24.0, 24.0));
    queue.set_style(
        parent,
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                transform_3d: Some(mat),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        child,
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                transform_origin: Some(nana_ui_core::TransformOrigin {
                    x: nana_ui_core::LengthSpec::Percent(25.0),
                    y: nana_ui_core::LengthSpec::Percent(75.0),
                }),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_interaction(
        parent,
        InteractionState {
            pointer_events: false,
            focusable: false,
        },
    );
    queue.set_interaction(
        child,
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.start_animation(transform_overlay(
        1,
        child,
        PaintTransform::default(),
        target,
    ));
    world.commit(queue).unwrap();
    world.advance_animations(Duration::from_millis(50));
    world.rebuild_hit_test(document(1));

    let forward = |([a, b, c, d, e, f], [g, h]): ([f32; 6], [f32; 2]), x: f32, y: f32| {
        let w = g * x + h * y + 1.0;
        [(a * x + c * y + e) / w, (b * x + d * y + f) / w]
    };
    let current = world
        .layout_projection_transform(child)
        .expect("presented projective transform");
    let current_center = forward(current, 32.0, 32.0);
    assert_eq!(
        world.hit_test(document(1), current_center[0], current_center[1]),
        Some(child),
        "hit-test must follow the normalized projective presentation"
    );

    let parent_transform = LayoutStyle {
        transform_3d: Some(mat),
        ..LayoutStyle::default()
    }
    .world_scene_transform(0.0, 0.0, 128.0, 96.0)
    .expect("parent projective transform");
    let old_center = forward(
        super::then_hit(
            parent_transform,
            ([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], [0.0, 0.0]),
        ),
        32.0,
        32.0,
    );
    assert_ne!(
        world.hit_test(document(1), old_center[0], old_center[1]),
        Some(child),
        "old logical geometry must not remain hittable during motion"
    );
}

#[test]
fn focus_and_accessibility_bounds_follow_presentation_transform() {
    let mut world = UiWorld::new();
    let target = PaintTransform {
        e: 40.0,
        ..PaintTransform::default()
    };
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.write_layout(node(1), box_at(0.0, 0.0, 40.0, 40.0));
    queue.set_style(node(1), style_with_transform(target));
    queue.set_interaction(
        node(1),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            role: AccessibilityRole::Button,
            ..AccessibilityState::default()
        },
    );
    queue.request_focus(document(1), Some(node(1)));
    queue.start_animation(transform_overlay(
        1,
        node(1),
        PaintTransform::default(),
        target,
    ));
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    world.advance_animations(Duration::from_millis(50));

    let bounds = world.focused_geometry(document(1)).expect("focus geometry");
    assert!((bounds.x - 20.0).abs() < 1e-4, "got {}", bounds.x);
    assert!((bounds.width - 40.0).abs() < 1e-4, "got {}", bounds.width);
    assert_eq!(world.presentation_input_bounds(node(1)), Some(bounds));

    let projected = world.project_accessibility_nodes(&[node(1)]);
    assert_eq!(projected.len(), 1);
    assert!((projected[0].bounds.x - 20.0).abs() < 1e-4);
    assert!((projected[0].bounds.width - 40.0).abs() < 1e-4);
}

#[test]
fn idle_compositor_frame_does_not_sample_presentation_for_hit_test() {
    let mut world = UiWorld::new();
    commit_button(
        &mut world,
        box_at(0.0, 0.0, 40.0, 40.0),
        style_with_transform(PaintTransform {
            e: 40.0,
            ..PaintTransform::default()
        }),
        transform_overlay(
            1,
            node(1),
            PaintTransform::default(),
            PaintTransform {
                e: 40.0,
                ..PaintTransform::default()
            },
        ),
    );
    let before = world.motion_work_counters().presentation_values_cpu_sampled;
    assert_eq!(before, 0);
    for ms in [10, 40, 70] {
        let frame = world.advance_animations(Duration::from_millis(ms));
        assert!(frame.samples.is_empty());
        assert_eq!(frame.animation_deadlines_scanned, 0);
        assert_eq!(
            world.motion_work_counters().presentation_values_cpu_sampled,
            before
        );
    }
}

#[test]
fn layout_width_animation_writes_px_not_scale() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.write_layout(node(1), box_at(0.0, 0.0, 80.0, 40.0));
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(nana_ui_core::LengthSpec::Px(80.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.start_animation(width_overlay(1, node(1), 40.0, 80.0));
    world.commit(queue).unwrap();
    world.advance_animations(Duration::from_millis(50));

    let width = match world.node_style(node(1)).unwrap().layout.width {
        Some(nana_ui_core::LengthSpec::Px(px)) => px,
        other => panic!("expected px width, got {other:?}"),
    };
    assert!((width - 60.0).abs() < 1e-3, "got {width}");
    assert_eq!(
        world.node_style(node(1)).unwrap().layout.transform,
        None,
        "width animation must not steal a scale transform"
    );
    assert!(
        world
            .presentation_applied_value(
                node(1),
                AnimatableProperty::Width,
                Duration::from_millis(50)
            )
            .is_none()
    );
}

#[test]
fn flip_play_keeps_last_layout_and_hit_test_follows_presentation() {
    let mut world = UiWorld::new();
    let first = nana_ui_core::FlipRect::new(10.0, 0.0, 40.0, 40.0);
    let last = nana_ui_core::FlipRect::new(40.0, 0.0, 40.0, 40.0);
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.write_layout(node(1), box_at(40.0, 0.0, 40.0, 40.0));
    queue.set_style(node(1), NodeStyle::default());
    queue.set_interaction(
        node(1),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.start_layout_flip(
        node(1),
        first,
        last,
        Duration::ZERO,
        Duration::from_millis(100),
        crate::Easing::Linear,
    );
    world.commit(queue).unwrap();
    world.rebuild_hit_test(document(1));

    assert_eq!(world.layout_box(node(1)).unwrap().x, 40.0);
    assert_eq!(world.node_style(node(1)).unwrap().layout.transform, None);
    match world.presentation_applied_value(node(1), AnimatableProperty::Transform, Duration::ZERO) {
        Some(MotionValue::Transform(transform)) => {
            assert!((transform.e + 30.0).abs() < 1e-3, "got {}", transform.e);
            assert_eq!(transform.a, 1.0);
        }
        other => panic!("expected invert overlay, got {other:?}"),
    }

    assert_eq!(
        world.hit_test(document(1), 20.0, 10.0),
        Some(node(1)),
        "hit-test follows presentation invert"
    );
    assert_eq!(
        world.hit_test(document(1), 70.0, 10.0),
        None,
        "Last box is not hittable until Play reaches identity"
    );

    world.advance_animations(Duration::from_millis(100));
    world.rebuild_hit_test(document(1));
    match world.presentation_applied_value(
        node(1),
        AnimatableProperty::Transform,
        Duration::from_millis(100),
    ) {
        Some(MotionValue::Transform(transform)) => {
            assert!(transform.e.abs() < 1e-3, "got {}", transform.e);
        }
        None => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(world.hit_test(document(1), 70.0, 10.0), Some(node(1)));
    assert_eq!(world.layout_box(node(1)).unwrap().x, 40.0);
}

#[test]
fn flip_animate_size_uses_layout_class_not_scale() {
    let mut world = UiWorld::new();
    let first = nana_ui_core::FlipRect::new(0.0, 0.0, 40.0, 20.0);
    let last = nana_ui_core::FlipRect::new(80.0, 0.0, 80.0, 20.0);
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.write_layout(node(1), box_at(80.0, 0.0, 80.0, 20.0));
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(nana_ui_core::LengthSpec::Px(80.0)),
                height: Some(nana_ui_core::LengthSpec::Px(20.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue
        .node(node(1), Duration::ZERO)
        .flip(first, last)
        .animate_size()
        .duration(Duration::from_millis(100))
        .ease(crate::Easing::Linear);
    world.commit(queue).unwrap();
    world.advance_animations(Duration::from_millis(50));

    assert_eq!(world.layout_box(node(1)).unwrap().x, 80.0);
    let width = match world.node_style(node(1)).unwrap().layout.width {
        Some(nana_ui_core::LengthSpec::Px(px)) => px,
        other => panic!("expected px width, got {other:?}"),
    };
    assert!((width - 60.0).abs() < 1e-3, "got {width}");
    match world.presentation_applied_value(
        node(1),
        AnimatableProperty::Transform,
        Duration::from_millis(50),
    ) {
        Some(MotionValue::Transform(transform)) => {
            assert_eq!(transform.a, 1.0);
            assert_eq!(transform.d, 1.0);
            assert!((transform.e + 40.0).abs() < 1e-3, "got {}", transform.e);
        }
        other => panic!("expected FLIP translate overlay, got {other:?}"),
    }
}

/// `perspective` on the parent fails the child's `matrix3d` closed, and paint
/// refuses it. The pointer has to refuse it too: an overlay is sampled on the
/// way to a node's local transform, and taking it before the refusal would
/// leave a click landing where the node is not drawn.
#[test]
fn a_closed_3d_context_refuses_a_presented_transform_for_the_pointer_too() {
    let sampled = |overlay: bool| {
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        queue.create(
            node(1),
            document(1),
            NodeKind::Element {
                tag: "stage".into(),
            },
        );
        queue.create(
            node(2),
            document(1),
            NodeKind::Element { tag: "card".into() },
        );
        queue.insert(node(1), node(2), None);
        queue.write_layout(node(1), box_at(0.0, 0.0, 80.0, 40.0));
        queue.write_layout(node(2), box_at(0.0, 0.0, 40.0, 40.0));
        queue.set_style(
            node(1),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    css_perspective: Some(800.0),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
        queue.set_style(
            node(2),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    transform_3d: Some(
                        nana_ui_core::PaintMat4::perspective(800.0)
                            .unwrap()
                            .then(nana_ui_core::PaintMat4::rotate_y(30_f32.to_radians())),
                    ),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
        if overlay {
            queue.start_animation(transform_overlay(
                1,
                node(2),
                PaintTransform::default(),
                PaintTransform {
                    e: 40.0,
                    ..PaintTransform::default()
                },
            ));
        }
        world.commit(queue).unwrap();
        world.advance_animations(Duration::from_millis(50));
        world.rebuild_hit_test(document(1));
        world
            .presentation_input_bounds(node(2))
            .expect("input bounds")
    };

    // No overlay: the closed context already refuses the node's own 3D
    // transform, so its input box is its plain layout box.
    let refused = sampled(false);
    assert_eq!(refused, box_at(0.0, 0.0, 40.0, 40.0));

    // With one: still refused. A node the rule has flattened must not be
    // moved by an overlay the pointer samples and paint does not.
    assert_eq!(
        sampled(true),
        refused,
        "a presented transform reopened a closed 3D context for the pointer"
    );
}

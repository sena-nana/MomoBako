use nana_ui::{HeadlessInput, runtime::*};
use nana_ui_core::{
    SemanticColor, SemanticColorMix, SemanticColorRole as R, StyleModelRef, ThemeMode,
};
use nana_ui_platform::{
    InputModifiers, InputPayload, PointerId, PointerInput, PointerPhase, PointerType,
};
use std::time::Duration;

fn close(a: [f32; 4], b: [f32; 4]) {
    for i in 0..4 {
        assert!((a[i] - b[i]).abs() < 0.00001, "{a:?} != {b:?}");
    }
}
fn hover(x: f32, y: f32) -> InputPayload {
    InputPayload::Pointer(PointerInput {
        phase: PointerPhase::Move,
        pointer_id: PointerId(1),
        pointer_type: PointerType::Mouse,
        x,
        y,
        screen_x: x,
        screen_y: y,
        button: 0,
        buttons: 0,
        pressure: 0.0,
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
fn semantic_mix_premultiplies_alpha_and_preserves_exact_historical_percentages() {
    let mut model = StyleModelRef::new(ThemeMode::Light);
    model.palette.accent = SemanticColor::rgba(1.0, 0.0, 0.0, 0.25);
    model.palette.border = SemanticColor::rgba(0.0, 0.0, 1.0, 0.75);
    close(
        SemanticColorMix::new(R::Accent, R::Border, 0.5)
            .resolve(model)
            .as_rgba_array(),
        [0.25, 0.0, 0.75, 0.5],
    );
    close(
        SemanticColorMix::alpha(R::Accent, 0.4)
            .resolve(model)
            .as_rgba_array(),
        [1.0, 0.0, 0.0, 0.1],
    );
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let model = StyleModelRef::new(mode);
        for (first, second, ratio) in [
            (R::Accent, R::Border, 0.50),
            (R::Accent, R::Border, 0.58),
            (R::Accent, R::Surface, 0.22),
            (R::Danger, R::Border, 0.35),
        ] {
            let a = model.color(first).as_rgba_array();
            let b = model.color(second).as_rgba_array();
            close(
                SemanticColorMix::new(first, second, ratio)
                    .resolve(model)
                    .as_rgba_array(),
                std::array::from_fn(|i| a[i] * ratio + b[i] * (1.0 - ratio)),
            );
        }
    }
}

#[test]
fn semantic_mix_theme_hover_disabled_and_raw_priority_share_the_normal_resolver() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let mix = SemanticColorMix::new(R::Accent, R::Surface, 0.22);
    let mut style = NodeStyle::default()
        .surface_mix(mix)
        .outline_mix(SemanticColorMix::new(R::Accent, R::Border, 0.58), 1.0);
    // A local mix overrides a role in the same paint; a later state role clears it.
    style.interaction.base.background = Some(R::Danger);
    style.interaction.hovered.background = Some(R::Hover);
    style.interaction.disabled.background = Some(R::Subtle);
    let layout = std::sync::Arc::make_mut(&mut style.layout);
    layout.width = Some(LengthSpec::Px(160.0));
    layout.height = Some(LengthSpec::Px(40.0));
    let button = cx
        .create_component(doc, Button::new("Save").style(style))
        .unwrap();
    let id = button.stable_id();
    let mut input = HeadlessInput::bind(&mut cx, doc);
    for (n, mode) in [ThemeMode::Light, ThemeMode::Dark].into_iter().enumerate() {
        cx.set_theme(mode).unwrap();
        cx.resolve_styles(&[id]).unwrap();
        cx.layout_document(doc, LayoutViewport::new(240.0, 120.0))
            .unwrap();
        cx.rebuild_hit_test(doc);
        close(
            cx.world().computed_style(id).unwrap().background.unwrap(),
            mix.resolve(cx.world().style_model()).as_rgba_array(),
        );
        assert_eq!(cx.pointer_target(doc, 10.0, 10.0), Some(id));
        input.route(&mut cx, hover(10.0, 10.0)).unwrap();
        cx.advance_animations(Duration::from_secs((n * 4 + 1) as u64));
        cx.resolve_styles(&[id]).unwrap();
        close(
            cx.world().computed_style(id).unwrap().background.unwrap(),
            cx.world().style_model().color(R::Hover).as_rgba_array(),
        );
        input.route(&mut cx, hover(400.0, 400.0)).unwrap();
        cx.advance_animations(Duration::from_secs((n * 4 + 2) as u64));
        cx.resolve_styles(&[id]).unwrap();
        close(
            cx.world().computed_style(id).unwrap().background.unwrap(),
            mix.resolve(cx.world().style_model()).as_rgba_array(),
        );
    }
    cx.update_component(button, |v, _| v.disabled = true)
        .unwrap();
    cx.resolve_styles(&[id]).unwrap();
    close(
        cx.world().computed_style(id).unwrap().background.unwrap(),
        cx.world().style_model().color(R::Subtle).as_rgba_array(),
    );
    cx.update_component(button, |v, _| {
        std::sync::Arc::make_mut(&mut v.style.layout).background = Some([0.2, 0.3, 0.4, 0.5])
    })
    .unwrap();
    cx.resolve_styles(&[id]).unwrap();
    close(
        cx.world().computed_style(id).unwrap().background.unwrap(),
        [0.2, 0.3, 0.4, 0.5],
    );
}

#[test]
fn semantic_mix_and_role_builders_obey_last_assignment() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let mix = SemanticColorMix::new(R::Accent, R::Border, 0.5);
    let style = NodeStyle::default()
        .surface_mix(mix)
        .surface(R::Surface)
        .outline_mix(mix, 1.0)
        .outline(R::Border, 2.0);
    let node = cx
        .create_component(doc, Text::new("x").style(style))
        .unwrap();
    let id = node.stable_id();
    cx.resolve_styles(&[id]).unwrap();
    let actual = cx.world().computed_style(id).unwrap();
    let model = cx.world().style_model();
    close(
        actual.background.unwrap(),
        model.color(R::Surface).as_rgba_array(),
    );
    close(
        actual.border_color.unwrap(),
        model.color(R::Border).as_rgba_array(),
    );
    cx.update_component(node, |v, _| {
        v.style = v.style.clone().surface_mix(mix).outline_mix(mix, 1.0)
    })
    .unwrap();
    cx.resolve_styles(&[id]).unwrap();
    let actual = cx.world().computed_style(id).unwrap();
    close(
        actual.background.unwrap(),
        mix.resolve(model).as_rgba_array(),
    );
    close(
        actual.border_color.unwrap(),
        mix.resolve(model).as_rgba_array(),
    );
}

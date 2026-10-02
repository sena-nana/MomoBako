//! Scene validation shared by [`super::SceneWgpuPainter`].
//!
//! Unknown custom GPU nodes and missing host textures are rejected instead
//! of being silently skipped. Affine transforms, letter-spacing, and named
//! fonts are painted, not fail-closed.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use nana_gpu::DeviceGeneration;
use nana_ui_runtime::StableNodeId;
use nana_ui_scene::{PrimitiveId, RenderOperation, ScenePrimitiveKind, UiScene};

use crate::scene_gpu::SceneGpuRendererRegistry;
use crate::{HostTextureBinding, HostTextureRegistry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenePaintError {
    InvalidRenderGraph,
    CustomPrimitive(PrimitiveId),
    UnsupportedCustomRenderer(PrimitiveId),
    MissingCustomResource(PrimitiveId),
    MissingNode(StableNodeId),
    /// The frame or target belongs to another device than the painter.
    DeviceMismatch {
        expected: DeviceGeneration,
        found: DeviceGeneration,
    },
    /// A host texture still samples a texture from a replaced device.
    StaleHostTexture(PrimitiveId),
    /// An earlier frame painted this target and is neither submitted nor
    /// dropped yet. `None` is the state behind `SceneWgpuPainter::paint`.
    TargetInFlight(Option<super::RenderTargetId>),
}

impl fmt::Display for ScenePaintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRenderGraph => formatter.write_str("scene render graph is invalid"),
            Self::CustomPrimitive(id) => write!(
                formatter,
                "scene primitive {}:{} requires a registered custom renderer",
                id.node.get(),
                id.slot
            ),
            Self::UnsupportedCustomRenderer(id) => write!(
                formatter,
                "scene primitive {}:{} names an unsupported custom renderer; \
                 register its renderer explicitly with scene_gpu_renderers",
                id.node.get(),
                id.slot
            ),
            Self::MissingCustomResource(id) => write!(
                formatter,
                "scene primitive {}:{} references an unavailable host resource",
                id.node.get(),
                id.slot
            ),
            Self::MissingNode(id) => write!(formatter, "scene node {} is unavailable", id.get()),
            Self::DeviceMismatch { expected, found } => write!(
                formatter,
                "frame from GPU device {found} painted with a painter on device {expected}"
            ),
            Self::StaleHostTexture(id) => write!(
                formatter,
                "scene primitive {}:{} samples a host texture from a replaced GPU device",
                id.node.get(),
                id.slot
            ),
            Self::TargetInFlight(Some(id)) => write!(
                formatter,
                "render target {} is still held by an unsubmitted frame",
                id.0
            ),
            Self::TargetInFlight(None) => {
                formatter.write_str("the painter's target is still held by an unsubmitted frame")
            }
        }
    }
}

impl std::error::Error for ScenePaintError {}

/// Resolves `"nana.host-texture"` custom scene nodes from the host registry.
/// Scene GPU painters such as `"gpu-view"` are resolved by
/// [`super::SceneWgpuPainter`], not this lookup.
#[derive(Debug, Clone)]
pub struct HostTextureSceneResolver {
    bindings: HashMap<u64, HostTextureBinding>,
}

impl HostTextureSceneResolver {
    pub fn new(
        scene: &UiScene,
        host_textures: &HostTextureRegistry,
    ) -> Result<Self, ScenePaintError> {
        let graph = scene
            .frame_plan()
            .map_err(|_| ScenePaintError::InvalidRenderGraph)?;
        let mut bindings = HashMap::new();
        for operation in graph.operations.iter() {
            let RenderOperation::InvokeCustom(id) = operation else {
                continue;
            };
            let Some(primitive) = scene.primitive(*id) else {
                continue;
            };
            let ScenePrimitiveKind::Custom { node: custom, .. } = &primitive.kind else {
                continue;
            };
            // Scene GPU painters such as `"gpu-view"` are resolved by
            // `SceneWgpuPainter`, not this host-texture lookup.
            if custom.renderer.as_ref() != "nana.host-texture" {
                continue;
            }
            let binding = host_textures
                .get(custom.resource.as_ref())
                .ok_or(ScenePaintError::MissingCustomResource(*id))?;
            bindings.insert(primitive.node.get(), binding);
        }
        Ok(Self { bindings })
    }

    pub fn binding(&self, node: u64) -> Option<HostTextureBinding> {
        self.bindings.get(&node).cloned()
    }
}

/// What the painter resolved about the frame's custom nodes.
///
/// [`validate_scene`] produces this from the lookups it already has to do, so
/// the painter never walks `FramePlan::custom_nodes` a second time.
#[derive(Debug, Default)]
pub(super) struct ResolvedCustomNodes {
    /// Host-texture binding identities, in `custom_nodes` order.
    pub(super) resources: Vec<super::TextureBindingKey>,
    /// `(renderer identity, preparation version)`, in `custom_nodes` order.
    pub(super) renderers: Vec<(usize, u64)>,
    /// `false` once a renderer declines to version its preparation: it can
    /// then change what it prepared without saying so, and nothing built this
    /// frame may be reused.
    pub(super) cacheable: bool,
}

/// Reject the frame if a custom node's host-texture slot or GPU renderer is
/// missing, and collect the [`super::PreparedBatch`] key while doing it.
///
/// Rejection has to stay ahead of every draw, the dest-reuse fast path
/// included, so the painter calls this first. The key falls out of the same
/// walk: resolving a node means looking its resource or renderer up, which is
/// exactly what keying it needs.
pub(super) fn validate_scene(
    scene: &UiScene,
    host_textures: Option<&HostTextureRegistry>,
    gpu_renderers: Option<&SceneGpuRendererRegistry>,
    device: DeviceGeneration,
) -> Result<ResolvedCustomNodes, ScenePaintError> {
    let plan = scene
        .frame_plan()
        .map_err(|_| ScenePaintError::InvalidRenderGraph)?;
    let mut resolved = ResolvedCustomNodes {
        resources: Vec::with_capacity(plan.custom_nodes.len()),
        renderers: Vec::new(),
        cacheable: true,
    };
    for id in plan.custom_nodes.iter() {
        let primitive = scene
            .primitive(*id)
            .ok_or(ScenePaintError::MissingNode(id.node))?;
        let ScenePrimitiveKind::Custom { node: custom, .. } = &primitive.kind else {
            continue;
        };
        if custom.renderer.as_ref() != "nana.host-texture" {
            let renderer = gpu_renderers
                .and_then(|renderers| renderers.get(custom.renderer.as_ref()))
                .ok_or(ScenePaintError::UnsupportedCustomRenderer(primitive.id))?;
            match renderer.preparation_version(custom) {
                Some(version) => resolved
                    .renderers
                    .push((Arc::as_ptr(&renderer) as *const () as usize, version)),
                None => resolved.cacheable = false,
            }
            continue;
        }
        let Some(host_textures) = host_textures else {
            return Err(ScenePaintError::CustomPrimitive(primitive.id));
        };
        let binding = host_textures
            .get(custom.resource.as_ref())
            .ok_or(ScenePaintError::MissingCustomResource(primitive.id))?;
        // A registry the host forgot to rebuild after a device replacement
        // fails the frame here instead of reaching the backend.
        if binding.texture.device_generation() != device {
            return Err(ScenePaintError::StaleHostTexture(primitive.id));
        }
        // Contents can change without replacing a sampled view, so only
        // binding identity and geometry invalidate prepared UI data;
        // re-encoding still samples the latest host pixels every frame.
        resolved.resources.push(super::TextureBindingKey {
            identity: binding.texture.instance_identity(),
            generation: binding.texture.generation(),
            width: binding.width,
            height: binding.height,
            alpha: binding.alpha_mode,
        });
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use nana_ui_runtime::{
        AppContext, Button as RuntimeButton, CustomRenderNode, DocumentId, GPU_VIEW_RENDERER,
        GpuTextureView, GpuView, LayoutBox, MutationQueue,
    };
    use nana_ui_scene::UiScene;

    use super::*;
    use crate::scene_gpu::{
        SceneGpuNode, SceneGpuPrepareContext, SceneGpuRenderContext, SceneGpuRenderer,
    };
    use crate::{HostTextureRegistry, SceneGpuRendererRegistry, default_scene_gpu_renderers};

    fn device() -> DeviceGeneration {
        crate::test_gpu::context().generation()
    }

    #[derive(Debug)]
    struct NoopSceneRenderer;

    impl SceneGpuRenderer for NoopSceneRenderer {
        fn prepare(&self, _node: &SceneGpuNode, _context: SceneGpuPrepareContext<'_>) {}

        fn render(&self, _node: &SceneGpuNode, _context: SceneGpuRenderContext<'_>) {}
    }

    fn button_scene() -> (UiScene, nana_ui_runtime::StableNodeId) {
        let mut context = AppContext::new();
        let button = context
            .create_component(DocumentId::new(1).unwrap(), RuntimeButton::new("构建"))
            .unwrap();
        let mut layout = MutationQueue::new();
        layout.write_layout(
            button.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );
        context.commit_mutations(layout).unwrap();
        let work = context.take_system_work();
        context.resolve_styles(&work.style).unwrap();
        let mut scene = UiScene::new();
        scene.apply_delta(
            context.world().extract_nodes(&work.render_extraction),
            work.render_removals,
        );
        (scene, button.stable_id())
    }

    fn gpu_view_scene() -> (UiScene, nana_ui_runtime::StableNodeId) {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let view = context.create_component(document, GpuView::new(1)).unwrap();
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            view.stable_id(),
            LayoutBox {
                x: 4.0,
                y: 8.0,
                width: 120.0,
                height: 60.0,
            },
        );
        context.commit_mutations(mutations).unwrap();
        let work = context.take_system_work();
        context.resolve_styles(&work.style).unwrap();
        let mut scene = UiScene::new();
        scene.apply_delta(
            context.world().extract_nodes(&work.render_extraction),
            work.render_removals,
        );
        (scene, view.stable_id())
    }

    /// `validate_scene` no longer returns the plan's operations — it returns
    /// what the painter resolved. Tests that assert operation order read the
    /// plan, which is where that order lives.
    fn plan_operations(scene: &UiScene) -> Arc<[RenderOperation]> {
        Arc::clone(&scene.frame_plan().expect("frame plan").operations)
    }

    fn assert_gpu_view_operation(
        operations: &[RenderOperation],
        scene: &UiScene,
        id: nana_ui_runtime::StableNodeId,
    ) {
        assert!(operations.iter().any(|operation| matches!(
            operation,
            RenderOperation::InvokeCustom(primitive) if primitive.node == id
                && scene.primitive(*primitive).is_some_and(|primitive| {
                    matches!(
                        &primitive.kind,
                        ScenePrimitiveKind::Custom { node: custom, .. }
                            if custom.renderer.as_ref() == GPU_VIEW_RENDERER
                    )
                })
        )));
    }

    #[test]
    fn runtime_button_validates_and_custom_content_is_explicit() {
        let mut context = AppContext::new();
        let button = context
            .create_component(DocumentId::new(1).unwrap(), RuntimeButton::new("构建"))
            .unwrap();
        let mut layout = MutationQueue::new();
        layout.write_layout(
            button.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );
        context.commit_mutations(layout).unwrap();
        let work = context.take_system_work();
        context.resolve_styles(&work.style).unwrap();
        let mut scene = UiScene::new();
        scene.apply_delta(
            context.world().extract_nodes(&work.render_extraction),
            work.render_removals,
        );
        validate_scene(&scene, None, None, device()).expect("a plain button validates");
        assert!(!plan_operations(&scene).is_empty());
        assert_eq!(scene.primitives().count(), 2);

        let mut custom = MutationQueue::new();
        custom.set_custom_render(
            button.stable_id(),
            Some(CustomRenderNode::new("nana.host-texture", "preview", 1)),
        );
        context.commit_mutations(custom).unwrap();
        let work = context.take_system_work();
        scene.apply_delta(
            context.world().extract_nodes(&work.render_extraction),
            work.render_removals,
        );
        assert!(matches!(
            validate_scene(&scene, None, None, device()),
            Err(ScenePaintError::CustomPrimitive(_))
        ));
        assert!(matches!(
            validate_scene(&scene, Some(&HostTextureRegistry::new()), None, device()),
            Err(ScenePaintError::MissingCustomResource(_))
        ));
    }

    #[test]
    fn registered_custom_renderer_is_an_executable_scene_operation() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let button = context
            .create_component(document, RuntimeButton::new("Preview"))
            .unwrap();
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            button.stable_id(),
            LayoutBox {
                x: 4.0,
                y: 8.0,
                width: 120.0,
                height: 60.0,
            },
        );
        mutations.set_custom_render(
            button.stable_id(),
            Some(CustomRenderNode::new("live2d.direct", "model", 7)),
        );
        context.commit_mutations(mutations).unwrap();
        let work = context.take_system_work();
        context.resolve_styles(&work.style).unwrap();
        let mut scene = UiScene::new();
        scene.apply_delta(
            context.world().extract_nodes(&work.render_extraction),
            work.render_removals,
        );
        let mut renderers = SceneGpuRendererRegistry::new();
        renderers.insert("live2d.direct", Arc::new(NoopSceneRenderer));
        validate_scene(&scene, None, Some(&renderers), device())
            .expect("registered renderer validates");
        assert!(plan_operations(&scene).iter().any(|operation| matches!(
            operation,
            RenderOperation::InvokeCustom(id) if *id == PrimitiveId {
                node: button.stable_id(),
                slot: 1,
            }
        )));
    }

    #[test]
    fn host_texture_layers_keep_standard_draws_between_them() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let background = context
            .create_component(document, GpuTextureView::new("live2d.bg"))
            .unwrap();
        let chrome = context
            .create_component(document, RuntimeButton::new("Start"))
            .unwrap();
        let foreground = context
            .create_component(document, GpuTextureView::new("live2d.fg"))
            .unwrap();
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            background.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
            },
        );
        mutations.write_layout(
            chrome.stable_id(),
            LayoutBox {
                x: 8.0,
                y: 24.0,
                width: 64.0,
                height: 28.0,
            },
        );
        mutations.write_layout(
            foreground.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
            },
        );
        context.commit_mutations(mutations).unwrap();
        let work = context.take_system_work();
        context.resolve_styles(&work.style).unwrap();
        let mut scene = UiScene::new();
        scene.apply_delta(
            context.world().extract_nodes(&work.render_extraction),
            work.render_removals,
        );
        let graph = scene
            .frame_graph(nana_ui_scene::ResourceId(1))
            .expect("layered host textures compile");
        let operations = graph
            .passes
            .iter()
            .flat_map(|pass| pass.operations.iter())
            .filter(|operation| !matches!(operation, RenderOperation::PrepareExternal(_)))
            .collect::<Vec<_>>();
        let background_at = operations.iter().position(|operation| {
            matches!(
                operation,
                RenderOperation::InvokeCustom(id) if id.node == background.stable_id()
            )
        });
        let chrome_at = operations.iter().position(|operation| {
            matches!(
                operation,
                RenderOperation::Draw(id) if id.node == chrome.stable_id()
            )
        });
        let foreground_at = operations.iter().position(|operation| {
            matches!(
                operation,
                RenderOperation::InvokeCustom(id) if id.node == foreground.stable_id()
            )
        });
        assert!(
            background_at.expect("background layer") < chrome_at.expect("chrome draw")
                && chrome_at.expect("chrome draw") < foreground_at.expect("foreground layer"),
            "GUI draws must sit between Live2D layer slots, got {operations:?}"
        );
    }

    #[test]
    fn validate_scene_accepts_gpu_view_with_default_renderers() {
        let (scene, id) = gpu_view_scene();
        let renderers = default_scene_gpu_renderers();
        assert!(renderers.get(GPU_VIEW_RENDERER).is_some());
        assert!(renderers.get("gpu-view").is_some());
        validate_scene(&scene, None, Some(&renderers), device())
            .expect("default renderers validate");
        assert_gpu_view_operation(&plan_operations(&scene), &scene, id);
    }

    #[test]
    fn validate_scene_rejects_gpu_view_without_renderers() {
        let (scene, _) = gpu_view_scene();
        assert!(matches!(
            validate_scene(&scene, None, None, device()),
            Err(ScenePaintError::UnsupportedCustomRenderer(_))
        ));
    }

    #[test]
    fn validate_scene_rejects_gpu_view_with_empty_registry() {
        let (scene, _) = gpu_view_scene();
        let err = validate_scene(
            &scene,
            None,
            Some(&SceneGpuRendererRegistry::new()),
            device(),
        )
        .expect_err("empty registry must not paint gpu-view");
        assert!(matches!(err, ScenePaintError::UnsupportedCustomRenderer(_)));
    }

    #[test]
    fn host_texture_resolver_skips_gpu_view_custom_nodes() {
        let (scene, _) = gpu_view_scene();
        HostTextureSceneResolver::new(&scene, &HostTextureRegistry::new()).unwrap();
    }

    #[test]
    fn empty_button_scene_validates_without_gpu_registry() {
        let (scene, _) = button_scene();
        validate_scene(&scene, None, None, device()).expect("a button scene validates");
        assert!(!plan_operations(&scene).is_empty());
    }

    #[test]
    fn validate_scene_accepts_rotation_and_letter_spacing() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let button = context
            .create_component(document, RuntimeButton::new("标题"))
            .unwrap();
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            button.stable_id(),
            LayoutBox {
                x: 8.0,
                y: 8.0,
                width: 120.0,
                height: 32.0,
            },
        );
        let mut style = nana_ui_runtime::NodeStyle::default();
        Arc::make_mut(&mut style.layout).transform = Some(nana_ui_core::PaintTransform {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
        });
        Arc::make_mut(&mut style.layout).letter_spacing = Some(0.5);
        Arc::make_mut(&mut style.layout).font_family = Some("Noto Sans SC".into());
        mutations.set_style(button.stable_id(), style);
        context.commit_mutations(mutations).unwrap();
        let work = context.take_system_work();
        context.resolve_styles(&work.style).unwrap();
        let mut scene = UiScene::new();
        scene.apply_delta(
            context.world().extract_nodes(&work.render_extraction),
            work.render_removals,
        );

        validate_scene(&scene, None, None, device()).expect("rotation and tracking validate");
        assert!(!plan_operations(&scene).is_empty());
        let mut saw_rotation = false;
        let mut saw_tracking = false;
        let mut saw_named_font = false;
        for primitive in scene.primitives() {
            let [a, b, c, d, _, _] = primitive.transform.0;
            if a != 1.0 || b != 0.0 || c != 0.0 || d != 1.0 {
                saw_rotation = true;
            }
            if let ScenePrimitiveKind::Text {
                letter_spacing,
                family,
                ..
            } = &primitive.kind
            {
                if *letter_spacing != 0.0 {
                    saw_tracking = true;
                }
                if family.as_deref() == Some("Noto Sans SC") {
                    saw_named_font = true;
                }
            }
        }
        assert!(saw_rotation, "scene must keep the 90° paint affine");
        assert!(saw_tracking, "scene must keep 0.5px letter-spacing");
        assert!(saw_named_font, "scene must keep the named Noto family");
    }
}

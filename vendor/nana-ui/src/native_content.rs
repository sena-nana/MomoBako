use std::sync::Arc;
#[cfg(feature = "hosted")]
use std::sync::Mutex;

#[cfg(feature = "hosted")]
use crate::scene_gpu::{
    SceneGpuNode, SceneGpuPrepareContext, SceneGpuRenderContext, SceneGpuRenderer,
};
use nana_ui_runtime::{NATIVE_CONTENT_RENDERER, StableNodeId};
use nana_ui_scene::{AffineTransform, ScenePrimitiveKind, SceneRect, UiScene};

#[derive(Clone, Debug, PartialEq)]
pub struct NativeContentRegion {
    pub node: StableNodeId,
    pub resource: Arc<str>,
    pub generation: u64,
    pub bounds: SceneRect,
    pub clip: SceneRect,
}

/// Whether this scene has any node a native compositor would have to mirror.
///
/// A scene with none — which is every ordinary window, and a window whose only
/// GPU content is host textures — must not pay for [`native_content_regions`]
/// at all. The plan's custom nodes are already compiled, so this is a walk over
/// that list rather than over the scene.
#[cfg_attr(
    not(any(test, all(feature = "hosted", target_os = "windows"))),
    allow(dead_code)
)]
pub(crate) fn scene_has_native_content(scene: &UiScene) -> bool {
    let Ok(plan) = scene.frame_plan() else {
        return false;
    };
    plan.custom_nodes.iter().any(|id| {
        scene.draw_primitive(*id).is_some_and(|primitive| {
            matches!(
                &primitive.kind,
                ScenePrimitiveKind::Custom { node, .. }
                    if node.renderer.as_ref() == NATIVE_CONTENT_RENDERER
            )
        })
    })
}

/// Resolves the same retained transforms and clip chain used by the painter.
/// Nonrectangular and offscreen composition require a platform capability that
/// this backend does not provide, so they fail before a native visual is shown.
pub fn native_content_regions(
    scene: &UiScene,
    viewport: SceneRect,
) -> Result<Vec<NativeContentRegion>, String> {
    let plan = scene.frame_plan().map_err(|error| error.to_string())?;
    let mut regions = Vec::new();
    let mut resources = std::collections::HashSet::new();
    for id in plan.custom_nodes.iter() {
        let Some(primitive) = scene.draw_primitive(*id) else {
            continue;
        };
        let ScenePrimitiveKind::Custom { node, mask, .. } = &primitive.kind else {
            continue;
        };
        if node.renderer.as_ref() != NATIVE_CONTENT_RENDERER {
            continue;
        }
        if primitive.opacity != 1.0
            || mask.is_some()
            || !scene.opacity_groups(primitive.node).is_empty()
            || !scene.filter_groups(primitive.node).is_empty()
        {
            return Err("native content requires an opaque, unfiltered composition parent".into());
        }
        let bounds = translated_rect(primitive.bounds, primitive.transform)?;
        let mut clip = intersection(bounds, viewport);
        for parent in primitive.clips.iter() {
            if parent.corner_radius != 0.0 || parent.polygon_clip.is_some() {
                return Err("native content currently requires rectangular clipping".into());
            }
            clip = intersection(clip, translated_rect(parent.bounds, parent.transform)?);
        }
        if clip.width <= 0.0 || clip.height <= 0.0 {
            continue;
        }
        if !resources.insert(Arc::clone(&node.resource)) {
            return Err("a native resource cannot occupy two regions in one window".into());
        }
        regions.push(NativeContentRegion {
            node: primitive.node,
            resource: Arc::clone(&node.resource),
            generation: node.revision,
            bounds,
            clip,
        });
    }
    Ok(regions)
}

/// Native-content mirroring a host has done.
///
/// The steady-state contract is that all three stop moving once a window
/// settles, however many GPU frames it goes on presenting: a platform
/// compositor is retained, so re-deriving and re-staging the same geometry is
/// work with no effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeContentWork {
    /// Region extractions that walked the scene.
    pub region_rebuilds: usize,
    /// Regions those extractions produced, changed or not.
    pub regions_considered: usize,
    /// Regions handed to a backend because they differed from the ones it was
    /// last given.
    pub regions_changed: usize,
}

/// What the scene's native-content regions were derived from.
///
/// The scene half is the projection revision the scene already maintains for
/// its own caches; the viewport half is there because the same scene laid out
/// at a different client size produces different regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeContentRevision {
    scene: (u64, u64),
    /// The whole viewport rectangle, by bits: this comparison is identity, not
    /// distance. The origin is in it because this type is public and a caller
    /// may clip against a rectangle that is not at (0, 0); a cache keyed on the
    /// size alone would hand such a caller a stale answer.
    viewport: [u32; 4],
}

/// What this frame has to do to keep the platform compositor in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, all(feature = "hosted", target_os = "windows"))),
    allow(dead_code)
)]
pub(crate) enum NativeContentSync {
    /// Nothing to stage.
    Settled { present: bool },
    /// The regions moved. Hand [`NativeContentMirror::regions`] to the backend.
    ///
    /// An emptied list is staged once, because a scene that has *lost* its
    /// native content has to say so — the backend's visuals are still in the
    /// retained tree until it is told to drop them, and nothing else will tell
    /// it.
    Stage { present: bool },
}

#[cfg_attr(
    not(any(test, all(feature = "hosted", target_os = "windows"))),
    allow(dead_code)
)]
impl NativeContentSync {
    /// Whether the scene has a native-content node at all.
    ///
    /// This is what decides whether the host registers the renderer that
    /// punches the opening: a scene holding such a node needs it registered
    /// even on a frame where every region happened to be clipped away, because
    /// an unregistered custom renderer fails the whole frame. It is therefore
    /// *not* "the region list is non-empty".
    pub const fn scene_has_native_content(self) -> bool {
        match self {
            Self::Settled { present } | Self::Stage { present } => present,
        }
    }
}

/// Keeps a platform compositor's visuals in step with a scene's
/// native-content nodes, and does nothing on a frame where nothing moved.
///
/// Three gates, cheapest first, because every one of them sits on the path of
/// a window presenting continuous GPU frames over a static visual tree:
///
/// 1. a scene whose projections and viewport did not move reuses the answer it
///    already has, so nothing rescans the node list;
/// 2. a scene with no native-content node never runs extraction at all;
/// 3. regions equal to the ones the backend was last given are not handed to
///    it again, so it stages no mutation and the frame's commit finds the tree
///    clean.
#[derive(Debug, Default)]
pub(crate) struct NativeContentMirror {
    revision: Option<NativeContentRevision>,
    present: bool,
    /// What the backend was last handed, so an unchanged answer is not handed
    /// over again.
    regions: Vec<NativeContentRegion>,
    /// Whether the backend may be holding visuals for regions it was given.
    ///
    /// Not the same as `!regions.is_empty()`: a refused stage forgets what was
    /// handed over, but the backend may already have built visuals before it
    /// failed, and a scene that then loses its native content still has to be
    /// told so those visuals come out of the retained tree.
    backend_may_hold_visuals: bool,
    work: NativeContentWork,
}

#[cfg_attr(
    not(any(test, all(feature = "hosted", target_os = "windows"))),
    allow(dead_code)
)]
impl NativeContentMirror {
    /// Resolves what this frame owes the compositor.
    ///
    /// `viewport` is the window's logical client rectangle. An extraction that
    /// fails leaves the mirror untouched, so the next frame retries from the
    /// same state rather than from a half-applied one.
    pub fn sync(
        &mut self,
        scene: &UiScene,
        viewport: SceneRect,
    ) -> Result<NativeContentSync, String> {
        let revision = NativeContentRevision {
            scene: scene.projection_revision(),
            viewport: [
                viewport.x.to_bits(),
                viewport.y.to_bits(),
                viewport.width.to_bits(),
                viewport.height.to_bits(),
            ],
        };
        if self.revision == Some(revision) {
            return Ok(NativeContentSync::Settled {
                present: self.present,
            });
        }
        if !scene_has_native_content(scene) {
            self.revision = Some(revision);
            self.present = false;
            self.regions.clear();
            // A scene that never had native content stages nothing. One that
            // just lost it stages the empty list once, so the backend drops the
            // visuals the retained tree is still holding.
            return Ok(if self.backend_may_hold_visuals {
                self.backend_may_hold_visuals = false;
                NativeContentSync::Stage { present: false }
            } else {
                NativeContentSync::Settled { present: false }
            });
        }
        self.work.region_rebuilds = self.work.region_rebuilds.saturating_add(1);
        let regions = native_content_regions(scene, viewport)?;
        self.work.regions_considered = self.work.regions_considered.saturating_add(regions.len());
        self.revision = Some(revision);
        self.present = true;
        if self.regions == regions {
            // The scan ran because something else in the scene changed, but
            // this window's native geometry did not. Staging it again would
            // dirty the tree and cost a commit for no visible difference.
            return Ok(NativeContentSync::Settled { present: true });
        }
        self.work.regions_changed = self.work.regions_changed.saturating_add(regions.len());
        self.regions = regions;
        self.backend_may_hold_visuals = !self.regions.is_empty();
        // `present` is about the scene, not the region list: a node whose
        // regions were all clipped away is still a node the painter will invoke
        // a renderer for.
        Ok(NativeContentSync::Stage { present: true })
    }

    /// The regions a [`NativeContentSync::Stage`] wants staged.
    pub fn regions(&self) -> &[NativeContentRegion] {
        &self.regions
    }

    /// Forgets what the compositor is believed to hold, so the next frame
    /// stages again.
    ///
    /// A backend that failed to take the last staged regions did not apply
    /// them, and a mirror that still claimed it had would report the retry as
    /// settled and stage nothing — the visuals would stay wrong until something
    /// else in the scene moved. `backend_may_hold_visuals` deliberately
    /// survives: the backend may have built visuals before it failed, and those
    /// still have to be cleaned up if the scene loses its native content.
    pub fn invalidate(&mut self) {
        self.revision = None;
        self.regions.clear();
    }

    /// What mirroring this window has cost so far. Totals since the mirror was
    /// created; the steady-state contract is about the *delta* over a settled
    /// stretch, so a caller gates on the difference between two readings.
    pub const fn work(&self) -> NativeContentWork {
        self.work
    }
}

fn translated_rect(rect: SceneRect, transform: AffineTransform) -> Result<SceneRect, String> {
    let [a, b, c, d, x, y] = transform.0;
    if transform.is_projective()
        || [a, b, c, d] != [1.0, 0.0, 0.0, 1.0]
        || ![rect.x, rect.y, rect.width, rect.height, x, y]
            .iter()
            .all(|value| value.is_finite())
    {
        return Err("native content requires a finite translation-only transform".into());
    }
    Ok(SceneRect {
        x: rect.x + x,
        y: rect.y + y,
        ..rect
    })
}

fn intersection(a: SceneRect, b: SceneRect) -> SceneRect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    SceneRect {
        x,
        y,
        width: (a.x + a.width).min(b.x + b.width).max(x) - x,
        height: (a.y + a.height).min(b.y + b.height).max(y) - y,
    }
}

#[cfg(feature = "hosted")]
#[derive(Debug, Default)]
pub(crate) struct NativeContentRenderer {
    /// Built for one device and target format; either changing rebuilds it.
    pipeline: Mutex<Option<(PipelineKey, wgpu::RenderPipeline)>>,
}

#[cfg(feature = "hosted")]
type PipelineKey = (nana_gpu::DeviceGeneration, nana_gpu::GpuTextureFormat);

#[cfg(feature = "hosted")]
impl SceneGpuRenderer for NativeContentRenderer {
    fn prepare(&self, _: &SceneGpuNode, context: SceneGpuPrepareContext<'_>) {
        use crate::gpu_raw::GpuRaw;
        let key = (context.gpu.generation(), context.target_format);
        let mut cached = self.pipeline.lock().expect("native content pipeline");
        if cached.as_ref().is_some_and(|(built, _)| *built == key) {
            return;
        }
        let device = context.gpu.raw_device();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native content opening"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@vertex fn vertex(@builtin(vertex_index) i:u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>,3>(vec2(-1.0,-1.0),vec2(3.0,-1.0),vec2(-1.0,3.0));
    return vec4(p[i],0.0,1.0);
}
@fragment fn fragment() -> @location(0) vec4<f32> { return vec4(0.0); }
"#
                .into(),
            ),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("native content opening"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: nana_gpu::__framework::format_to_wgpu(context.target_format),
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        *cached = Some((key, pipeline));
    }

    fn render(&self, node: &SceneGpuNode, mut context: SceneGpuRenderContext<'_>) {
        if node.custom.param(0) == Some(0.0) {
            return;
        }
        let cached = self.pipeline.lock().expect("native content pipeline");
        let Some((built, pipeline)) = cached.as_ref() else {
            return;
        };
        if *built != (context.gpu.generation(), context.target_format) {
            return;
        }
        let bounds = crate::gpu_view::intersect_physical(context.bounds, context.clip);
        if bounds.width == 0 || bounds.height == 0 {
            return;
        }
        context.with_pass("native content opening", |pass| {
            pass.set_viewport(bounds);
            pass.set_scissor(bounds);
            let raw = pass.raw();
            raw.set_pipeline(pipeline);
            raw.draw(0..3, 0..1);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene(opacity: f32, duplicate: bool) -> UiScene {
        let mut context = nana_ui_runtime::AppContext::new();
        let document = nana_ui_runtime::DocumentId::new(1).unwrap();
        let mut view = nana_ui_runtime::NativeContent::new("browser").generation(3);
        Arc::make_mut(&mut view.style.layout).opacity = Some(opacity);
        let first = context.create_component(document, view.clone()).unwrap();
        let second = duplicate.then(|| context.create_component(document, view).unwrap());
        let mut mutations = nana_ui_runtime::MutationQueue::new();
        for entity in [Some(first), second].into_iter().flatten() {
            mutations.write_layout(
                entity.stable_id(),
                nana_ui_runtime::LayoutBox {
                    x: 10.0,
                    y: 20.0,
                    width: 80.0,
                    height: 40.0,
                },
            );
        }
        context.commit_mutations(mutations).unwrap();
        let work = context.take_system_work();
        context.resolve_styles(&work.style).unwrap();
        let mut scene = UiScene::new();
        scene.apply_delta(
            context.world().extract_nodes(&work.render_extraction),
            work.render_removals,
        );
        scene
    }
    #[test]
    fn native_regions_clip_to_window_and_reject_unsupported_composition() {
        let viewport = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let regions = native_content_regions(&scene(1.0, false), viewport).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].generation, 3);
        assert_eq!(
            regions[0].clip,
            SceneRect {
                x: 10.0,
                y: 20.0,
                width: 40.0,
                height: 30.0
            }
        );
        assert!(native_content_regions(&scene(0.5, false), viewport).is_err());
        assert!(native_content_regions(&scene(1.0, true), viewport).is_err());
    }

    /// A scene with no native content must never pay for extraction, and must
    /// say so cheaply: this is every ordinary window, and every window whose
    /// only GPU content is host textures.
    #[test]
    fn a_scene_without_native_content_never_runs_extraction() {
        let empty = UiScene::new();
        assert!(!scene_has_native_content(&empty));

        let mut mirror = NativeContentMirror::default();
        let viewport = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        assert_eq!(
            mirror.sync(&empty, viewport).unwrap(),
            NativeContentSync::Settled { present: false }
        );
        assert_eq!(mirror.work(), NativeContentWork::default());
        assert!(mirror.regions().is_empty());

        assert!(scene_has_native_content(&scene(1.0, false)));
    }

    /// The Live2D / video / particle case: the UI presents GPU frame after GPU
    /// frame into its visual while the composition tree stands still. A
    /// retained compositor must not be re-derived or re-staged for any of
    /// them — only the first frame does work.
    #[test]
    fn continuous_gpu_frames_over_a_static_scene_stage_nothing_after_the_first() {
        let scene = scene(1.0, false);
        let viewport = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let mut mirror = NativeContentMirror::default();

        assert_eq!(
            mirror.sync(&scene, viewport).unwrap(),
            NativeContentSync::Stage { present: true }
        );
        let first = mirror.work();
        assert_eq!(first.region_rebuilds, 1);
        assert_eq!(first.regions_changed, 1);
        assert_eq!(mirror.regions().len(), 1);

        for frame in 0..120 {
            assert_eq!(
                mirror.sync(&scene, viewport).unwrap(),
                NativeContentSync::Settled { present: true },
                "frame {frame} restaged a tree nothing moved"
            );
        }
        assert_eq!(
            mirror.work(),
            first,
            "120 GPU frames over an unchanged scene must cost no extraction"
        );
    }

    /// A rebuild the scene forced — because something else in it changed —
    /// still must not stage geometry the backend already has. The scan is the
    /// price of the scene moving; the tree mutation and its commit are not.
    #[test]
    fn a_rescan_that_finds_the_same_geometry_stages_nothing() {
        let viewport = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let mut mirror = NativeContentMirror::default();
        assert_eq!(
            mirror.sync(&scene(1.0, false), viewport).unwrap(),
            NativeContentSync::Stage { present: true }
        );
        let staged = mirror.work().regions_changed;

        // An independently built scene has a different revision but the same
        // native geometry, which is what an unrelated UI change looks like.
        assert_eq!(
            mirror.sync(&scene(1.0, false), viewport).unwrap(),
            NativeContentSync::Settled { present: true }
        );
        assert_eq!(mirror.work().region_rebuilds, 2, "the scan had to run");
        assert_eq!(
            mirror.work().regions_changed,
            staged,
            "but nothing was handed to the backend a second time"
        );
    }

    /// The same scene in a different client size lays its regions out
    /// differently, so the viewport is part of what the answer is cached
    /// against.
    #[test]
    fn a_resized_window_restages_the_regions_the_new_viewport_clips() {
        let scene = scene(1.0, false);
        let mut mirror = NativeContentMirror::default();
        let small = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let wide = SceneRect {
            width: 200.0,
            ..small
        };
        assert_eq!(
            mirror.sync(&scene, small).unwrap(),
            NativeContentSync::Stage { present: true }
        );
        assert_eq!(
            mirror.sync(&scene, wide).unwrap(),
            NativeContentSync::Stage { present: true }
        );
        assert_eq!(mirror.regions()[0].clip.width, 80.0);
        assert_eq!(
            mirror.sync(&scene, wide).unwrap(),
            NativeContentSync::Settled { present: true }
        );
    }

    /// A backend that refused the staged regions did not apply them. The retry
    /// after that failure has to stage them again rather than find itself
    /// settled — otherwise the visuals stay wrong until something else in the
    /// scene happens to move.
    #[test]
    fn a_refused_stage_is_offered_again_on_the_next_frame() {
        let scene = scene(1.0, false);
        let viewport = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let mut mirror = NativeContentMirror::default();
        assert_eq!(
            mirror.sync(&scene, viewport).unwrap(),
            NativeContentSync::Stage { present: true }
        );
        // Without this the same revision reads as settled forever.
        assert_eq!(
            mirror.sync(&scene, viewport).unwrap(),
            NativeContentSync::Settled { present: true }
        );
        mirror.invalidate();
        assert_eq!(
            mirror.sync(&scene, viewport).unwrap(),
            NativeContentSync::Stage { present: true },
            "a refused stage has to be offered again"
        );
    }

    /// The cache is keyed on the whole viewport rectangle, not just its size:
    /// a clip rectangle that moved produces different regions from the same
    /// scene, and answering from the cache there would leave the compositor
    /// showing the old ones.
    #[test]
    fn a_viewport_that_moved_is_not_the_same_viewport() {
        let scene = scene(1.0, false);
        let mut mirror = NativeContentMirror::default();
        let origin = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let shifted = SceneRect { x: 12.0, ..origin };
        assert_eq!(
            mirror.sync(&scene, origin).unwrap(),
            NativeContentSync::Stage { present: true }
        );
        let rebuilds = mirror.work().region_rebuilds;
        let _ = mirror.sync(&scene, shifted).unwrap();
        assert_eq!(
            mirror.work().region_rebuilds,
            rebuilds + 1,
            "a moved clip rectangle has to be re-derived"
        );
    }

    /// A native-content node whose regions are all clipped away is still a
    /// node the painter will look up a renderer for, and an unregistered
    /// custom renderer fails the whole frame. So "the scene has native
    /// content" must not be inferred from the region list being non-empty.
    #[test]
    fn a_fully_clipped_node_still_reports_that_the_scene_has_native_content() {
        let scene = scene(1.0, false);
        // The laid-out node sits at (10, 20); a viewport above and left of it
        // clips every region away.
        let offscreen = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 5.0,
            height: 5.0,
        };
        let mut mirror = NativeContentMirror::default();
        let outcome = mirror.sync(&scene, offscreen).unwrap();
        assert!(mirror.regions().is_empty(), "everything was clipped away");
        assert!(
            outcome.scene_has_native_content(),
            "the node is still in the scene, so its renderer is still needed"
        );
        // And the settled frame after it keeps saying so.
        let settled = mirror.sync(&scene, offscreen).unwrap();
        assert_eq!(settled, NativeContentSync::Settled { present: true });
        assert!(settled.scene_has_native_content());
    }

    /// A backend can fail *after* building some visuals. If the scene then
    /// loses its native content, the empty list still has to reach it, or
    /// those visuals stay in the retained tree with nothing left to remove
    /// them.
    #[test]
    fn a_backend_that_failed_mid_stage_is_still_told_when_the_content_goes() {
        let viewport = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let mut mirror = NativeContentMirror::default();
        assert_eq!(
            mirror.sync(&scene(1.0, false), viewport).unwrap(),
            NativeContentSync::Stage { present: true }
        );
        // The backend refused, so the mirror forgets what it handed over.
        mirror.invalidate();
        assert!(mirror.regions().is_empty());

        assert_eq!(
            mirror.sync(&UiScene::new(), viewport).unwrap(),
            NativeContentSync::Stage { present: false },
            "visuals built before the failure still have to be cleaned up"
        );
        assert_eq!(
            mirror.sync(&UiScene::new(), viewport).unwrap(),
            NativeContentSync::Settled { present: false }
        );
    }

    /// A failed extraction must not leave the mirror claiming the compositor
    /// holds regions it never received, or the next frame would skip staging
    /// them.
    #[test]
    fn a_failed_extraction_leaves_nothing_staged() {
        let viewport = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let mut mirror = NativeContentMirror::default();
        assert!(mirror.sync(&scene(0.5, false), viewport).is_err());
        assert!(mirror.regions().is_empty());
        assert_eq!(
            mirror.sync(&scene(1.0, false), viewport).unwrap(),
            NativeContentSync::Stage { present: true }
        );
    }

    /// A scene that loses its native content has to say so once. The backend's
    /// visuals are still in the retained tree, and only this call tells it to
    /// drop them — a compositor that is never told keeps showing them.
    #[test]
    fn losing_native_content_stages_the_empty_list_exactly_once() {
        let viewport = SceneRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let mut mirror = NativeContentMirror::default();
        assert_eq!(
            mirror.sync(&scene(1.0, false), viewport).unwrap(),
            NativeContentSync::Stage { present: true }
        );
        let empty = UiScene::new();
        assert_eq!(
            mirror.sync(&empty, viewport).unwrap(),
            NativeContentSync::Stage { present: false },
            "the backend has visuals it has not been told to remove"
        );
        assert!(mirror.regions().is_empty());
        // Said once, not every frame after.
        assert_eq!(
            mirror.sync(&empty, viewport).unwrap(),
            NativeContentSync::Settled { present: false }
        );
    }

    /// A scene that never had native content stages nothing at all.
    #[test]
    fn a_scene_that_never_had_native_content_stages_nothing() {
        let mut mirror = NativeContentMirror::default();
        assert_eq!(
            mirror
                .sync(
                    &UiScene::new(),
                    SceneRect {
                        x: 0.0,
                        y: 0.0,
                        width: 50.0,
                        height: 50.0,
                    }
                )
                .unwrap(),
            NativeContentSync::Settled { present: false }
        );
    }
}

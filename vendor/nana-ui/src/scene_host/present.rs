//! Scene host present coordination.

use super::*;
#[cfg(target_os = "windows")]
use crate::SceneGpuRendererRegistry;
use crate::hosted_context::SurfaceFrame;

impl<Program: RuntimeProgram> WindowManager<Program> {
    /// `true` when prepare ran (encode may have been skipped). `false` on abort.
    pub(super) fn tick_hidden_gpu(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
    ) -> bool {
        if self.render_suspended {
            return false;
        }
        if self.graphics.take_device_lost() {
            self.recover_device(event_loop);
            return false;
        }
        if !self.window_contexts.contains_key(&id) {
            return false;
        }
        self.program.prepare_window_frame(id, &self.context_for(id));
        if !super::schedule::drawable_surface(self.geometry_of(id).physical_size) {
            return true;
        }
        let Some(producers) = self.program.scene_resource_producers(id) else {
            return true;
        };
        let Some(scene) = self
            .program
            .read_document(id, |document| document.shared_scene())
        else {
            return false;
        };
        let mut frame = self.graphics.gpu().begin_frame("NanaUI hidden gpu tick");
        match producers.encode_scene(scene.as_ref(), &mut frame) {
            Ok(prepared) => {
                prepared.submitted(&frame.submit());
                true
            }
            Err(error) => {
                drop(frame);
                self.program
                    .report_host_failure(HostFailure::ResourceProduction {
                        window: id,
                        error: error.to_string(),
                    });
                false
            }
        }
    }

    fn rearm_frame_demand(&mut self, id: WindowId) {
        self.update_frame_schedule(id, crate::runtime_host::FrameSchedule::defer);
    }

    fn serve_frame_demand(&mut self, id: WindowId) {
        self.update_frame_schedule(id, crate::runtime_host::FrameSchedule::advance_served);
    }

    /// A frame may close its own window; a closed window keeps no schedule.
    fn update_frame_schedule(
        &mut self,
        id: WindowId,
        update: impl FnOnce(
            &mut crate::runtime_host::FrameSchedule,
            crate::FrameDemand,
            Instant,
        ) -> Option<Instant>,
    ) {
        if !self.window_contexts.contains_key(&id) {
            return;
        }
        let demand = self.window_frame_demand(id);
        update(
            self.frame_schedules.entry(id).or_default(),
            demand,
            Instant::now(),
        );
    }

    pub(super) fn redraw(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId) {
        if self.render_suspended || !self.can_present(id) {
            self.rearm_frame_demand(id);
            return;
        }
        let frame_started = nana_diagnostics::metrics_enabled().then(Instant::now);
        if frame_started.is_some() {
            // Deliver completion callbacks of earlier submissions for
            // `gpu.completion`. Before the device-lost check, so a loss this
            // poll reports is handled in this frame either way.
            crate::host_diagnostics::poll_completions(self.graphics.gpu().raw_device());
        }
        if self.graphics.take_device_lost() {
            self.recover_device(event_loop);
            self.rearm_frame_demand(id);
            return;
        }
        if !self.window_contexts.contains_key(&id) {
            self.rearm_frame_demand(id);
            return;
        }
        let queued = self.drain_program_messages(id);
        self.apply_update(event_loop, queued, Some(id));
        if event_loop.exiting() || self.render_suspended || !self.can_present(id) {
            self.rearm_frame_demand(id);
            return;
        }
        self.resize_window(id);
        self.program.prepare_window_frame(id, &self.context_for(id));
        self.sync_compositor_clock(id);
        self.reconcile_browser_lifetimes();
        let geometry = self.geometry_of(id);
        let material = self.material_of(id);
        let viewport = LayoutViewport::new(geometry.logical_size.0, geometry.logical_size.1);
        let Some(flush) = self
            .program
            .write_document(id, |document| document.flush(viewport, &mut self.text))
        else {
            self.program
                .report_host_failure(HostFailure::MissingDocument { window: id });
            self.rearm_frame_demand(id);
            return;
        };
        let update = match flush {
            Ok(update) => update,
            Err(error) => {
                // The frame did not settle; Runtime restored its dirty work,
                // so the next redraw retries. Skipping keeps the process alive.
                self.program
                    .report_host_failure(HostFailure::FrameDidNotSettle {
                        window: id,
                        error: error.to_string(),
                    });
                self.rearm_frame_demand(id);
                return;
            }
        };
        // A cursor declaration can change under a resting pointer; the
        // Runtime re-derives it once the document's computed styles settle.
        if update.cursor_changed {
            self.refresh_window_input(id);
        }
        self.sync_native_window_controls(id, &update);
        let Some(pending) = self.accessibility_pending_mut(id) else {
            self.rearm_frame_demand(id);
            return;
        };
        pending.stage(update.accessibility);
        let Some(scene) = self
            .program
            .write_document(id, |document| document.shared_scene())
        else {
            self.program
                .report_host_failure(HostFailure::MissingDocument { window: id });
            self.rearm_frame_demand(id);
            return;
        };
        self.sync_native_browsers(id, scene.as_ref());
        self.sync_shadow_body(id, scene.as_ref());
        self.update_image_targets(id, scene.as_ref());
        let Some(host) = self.window_contexts.get(&id) else {
            return;
        };
        let format = host.surface.format();
        // Decided before the drawable is acquired: a macOS handoff changes how
        // this frame is presented.
        let takes_over = self.prepare_startup_frame(id);
        // A primary window under its splash lays out and settles like any
        // other, so a takeover finds its first frame ready, but presents
        // nothing until something asked to take over.
        if self.startup_holds(id) {
            self.rearm_frame_demand(id);
            return;
        }
        let frame = match self.acquire_frame(id) {
            Ok(SurfaceFrame::Ready(frame)) => frame,
            Ok(SurfaceFrame::Retry) => {
                self.request_redraw(id);
                return;
            }
            Ok(SurfaceFrame::Skipped) => {
                nana_diagnostics::metric!(nana_diagnostics::framework::gpu::FRAMES_SKIPPED);
                self.rearm_frame_demand(id);
                return;
            }
            Err(error) => {
                self.rearm_frame_demand(id);
                self.suspend_surface(id, error);
                return;
            }
        };
        let target = nana_gpu::__framework::render_target(
            self.graphics.gpu(),
            frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
            frame.texture.format(),
            [frame.texture.width(), frame.texture.height()],
        );
        // Everything recorded for this window goes into one frame. On every
        // failure path it is dropped before the surface texture it targets:
        // nothing may still reference the texture the surface is asked to
        // abandon (DX12), and dropping it rolls the painter's retained writes
        // back.
        let mut recording = self.graphics.gpu().begin_frame("NanaUI scene host frame");
        let prepared = if let Some(producers) = self.program.scene_resource_producers(id) {
            match producers.encode_scene(scene.as_ref(), &mut recording) {
                Ok(prepared) => Some(prepared),
                Err(error) => {
                    drop(recording);
                    drop(target);
                    self.discard_frame(id, frame);
                    self.program
                        .report_host_failure(HostFailure::ResourceProduction {
                            window: id,
                            error: error.to_string(),
                        });
                    self.rearm_frame_demand(id);
                    return;
                }
            }
        } else {
            None
        };
        let host_textures = self.program.host_textures(id);
        if let Some(registry) = host_textures.as_ref() {
            if let Ok(plan) = scene.frame_plan() {
                let slots = plan
                    .custom_nodes
                    .iter()
                    .filter_map(|node| {
                        let primitive = scene.primitive(*node)?;
                        match &primitive.kind {
                            nana_ui_scene::ScenePrimitiveKind::Custom { node, .. }
                                if node.renderer.as_ref() == "nana.host-texture" =>
                            {
                                Some(Arc::clone(&node.resource))
                            }
                            _ => None,
                        }
                    })
                    .collect::<HashSet<_>>();
                let current = self.texture_subscriptions.get(&id).is_some_and(
                    |(subscribed, subscription)| {
                        *subscribed == slots && subscription.observes(registry)
                    },
                );
                if !current {
                    let pending = Arc::clone(&self.texture_redraws);
                    let proxy = self.proxy.clone();
                    let observed = slots.clone();
                    let subscription = registry.subscribe(move |slot| {
                        if slot.is_empty() || observed.contains(slot) {
                            pending.lock().expect("texture redraws").insert(id);
                            proxy.wake_up();
                        }
                    });
                    self.texture_subscriptions.insert(id, (slots, subscription));
                }
            }
        } else {
            self.texture_subscriptions.remove(&id);
        }
        // Only the Windows native-content path inserts into the registry.
        #[allow(unused_mut)]
        let mut gpu_renderers = self.program.scene_gpu_renderers(id);
        #[cfg(target_os = "windows")]
        let composition = self
            .window_contexts
            .get(&id)
            .and_then(|host| host.surface.windows_composition())
            .cloned();
        #[cfg(target_os = "windows")]
        if let Some(composition) = composition.as_ref() {
            match self.sync_native_content(id, composition, &scene, geometry.logical_size) {
                Ok(true) => {
                    let renderer = self.native_renderers.entry(format).or_default().clone();
                    gpu_renderers
                        .get_or_insert_with(SceneGpuRendererRegistry::new)
                        .insert(nana_ui_runtime::NATIVE_CONTENT_RENDERER, renderer);
                }
                Ok(false) => {}
                Err(error) => {
                    drop(recording);
                    drop(target);
                    self.discard_frame(id, frame);
                    self.program
                        .report_host_failure(HostFailure::ResourceProduction { window: id, error });
                    self.rearm_frame_demand(id);
                    return;
                }
            }
        }
        let theme = self.program.theme_mode();
        let window_background = self.program.window_background();
        let fetch_host = self.program.resource_fetch_host(id);
        // Subpixel text only onto a surface the compositor shows opaque: over
        // a transparent material the per-channel coverage would be composited
        // as colored alpha.
        let opaque = self
            .window_contexts
            .get(&id)
            .is_some_and(|host| host.surface.alpha_mode() == crate::SurfaceAlphaMode::Opaque);
        let painter = self.painter_mut(format);
        // Painters are shared per format, so every window supplies its own
        // egress, including none, and its own surface's text mode.
        painter.set_resource_fetch_host(fetch_host);
        painter.set_subpixel_text(
            opaque
                .then(crate::scene_paint::SubpixelOrder::system)
                .flatten(),
        );
        // Compositors blend a surface as premultiplied in its encoded space,
        // Metal's `PostMultiplied` included.
        painter.set_alpha_encoding(if opaque {
            crate::scene_paint::AlphaEncoding::Linear
        } else {
            crate::scene_paint::AlphaEncoding::Gamma
        });
        let paint = painter.paint_target(
            crate::RenderTargetId(id.0),
            scene.as_ref(),
            &mut recording,
            &target,
            scene_paint_viewport(&geometry, material, theme, window_background),
            host_textures.as_ref(),
            gpu_renderers.as_ref(),
        );
        if let Err(error) = paint {
            drop(recording);
            drop(target);
            self.discard_frame(id, frame);
            self.program
                .report_host_failure(HostFailure::UnpaintableScene {
                    window: id,
                    error: error.to_string(),
                });
            self.rearm_frame_demand(id);
            return;
        }
        let submission = recording.submit();
        if frame_started.is_some() {
            crate::host_diagnostics::watch_submission(self.graphics.gpu().raw_queue());
        }
        if let Some(prepared) = prepared {
            prepared.submitted(&submission);
        }
        let submit = submission.cpu_duration();
        let painter = self.painter_mut(format);
        painter.record_submit(&submission);
        let gpu_work = painter.last_gpu_work();
        self.graphics.present_frame(frame);
        crate::host_diagnostics::frame_presented(frame_started, submit, gpu_work);
        if let Some(host) = self.window_contexts.get_mut(&id) {
            self.graphics.apply_pending_reconfigure(&mut host.surface);
        }
        self.serve_frame_demand(id);
        // The one transaction boundary for this window's composition tree.
        // Backends stage; the host publishes, and only when something was
        // staged — a retained compositor does not follow the GPU's frame rate.
        #[cfg(target_os = "windows")]
        if let Some(composition) = composition
            && let Err(error) = composition.commit()
        {
            self.program
                .report_host_failure(HostFailure::ResourceProduction {
                    window: id,
                    error: error.to_string(),
                });
            self.request_redraw(id);
            return;
        }
        // The frame is presented (and, on Windows, its composition published):
        // it may now end the startup.
        if takes_over {
            self.startup_frame_presented(event_loop);
        }
        // Publish semantics for the frame just presented. Application callbacks
        // below may commit new work intended for the next frame.
        #[cfg(not(target_os = "android"))]
        if !self.is_live_resize(id) {
            self.synchronize_accessibility(id);
        }
        // Layout moved the caret, or the field under it: the IME follows.
        self.refresh_window_input(id);
        let mut update = self
            .program
            .window_frame_presented(id, &self.context_for(id));
        if self.bind_after_present.remove(&id) {
            update = update.merge(self.program.bind_window(id, &self.context_for(id)));
        }
        self.sync_appearance();
        self.apply_update(event_loop, update, None);
    }
    /// Mirrors this frame's native-content regions into the window's
    /// composition tree, doing nothing when nothing they depend on moved.
    ///
    /// Returns whether the scene has native content at all, which is what
    /// decides if the opening renderer has to be registered for this frame.
    /// The decision itself lives on [`crate::native_content::NativeContentMirror`]; this only
    /// hands the result to the backend.
    #[cfg(target_os = "windows")]
    fn sync_native_content(
        &mut self,
        id: WindowId,
        composition: &crate::WindowsComposition,
        scene: &nana_ui_scene::UiScene,
        logical_size: (f32, f32),
    ) -> Result<bool, String> {
        // The mirror stays in the map across the backend call: the context that
        // call receives reports this window's mirroring counters, and taking the
        // mirror out would hand the backend zeros.
        let outcome = self.native_content.entry(id).or_default().sync(
            scene,
            nana_ui_scene::SceneRect {
                x: 0.0,
                y: 0.0,
                width: logical_size.0,
                height: logical_size.1,
            },
        )?;
        // Whether the opening renderer is needed this frame is a property of
        // the scene, not of how many regions survived clipping: an
        // unregistered custom renderer fails the whole frame.
        let present = outcome.scene_has_native_content();
        let crate::native_content::NativeContentSync::Stage { .. } = outcome else {
            return Ok(present);
        };
        // Copied out so the backend call can borrow the host. This is the
        // changed-geometry path, which already walked the scene; a settled frame
        // never gets here.
        let regions = self
            .native_content
            .get(&id)
            .map(|mirror| mirror.regions().to_vec())
            .unwrap_or_default();
        let staged = self.program.native_content_frame(
            id,
            composition.tree(),
            &regions,
            &self.context_for(id),
        );
        if staged.is_err() {
            // The backend did not take them, so the mirror must not go on
            // claiming the compositor has them — the retry after this frame's
            // failure would otherwise find itself settled and stage nothing.
            if let Some(mirror) = self.native_content.get_mut(&id) {
                mirror.invalidate();
            }
        }
        staged.map(|()| present)
    }

    fn discard_frame(&mut self, id: WindowId, frame: wgpu::SurfaceTexture) {
        if let Some(host) = self.window_contexts.get_mut(&id) {
            self.graphics.abandon_frame(&mut host.surface, frame);
        }
    }

    pub(super) fn acquire_frame(&mut self, id: WindowId) -> Result<SurfaceFrame, HostedGpuError> {
        let host = self
            .window_contexts
            .get_mut(&id)
            .ok_or(HostedGpuError::SurfaceValidation)?;
        self.graphics.acquire(&mut host.surface)
    }
    pub(super) fn recover_device(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // Present only on the first attempt after a loss; retries find none.
        if let Some(lost) = self.graphics.take_device_lost_report() {
            nana_diagnostics::fault!(
                nana_diagnostics::framework::gpu::DEVICE_LOST,
                reason = u64::from(lost.reason == nana_gpu::GpuLossReason::Destroyed);
                "{:?}: {}",
                lost.reason,
                lost.message
            );
            nana_diagnostics::snapshot("device-lost");
        }
        if self.embedded {
            self.render_suspended = true;
            self.next_gpu_retry = None;
            return;
        }
        let mut recovery_windows = self.known_window_ids();
        if recovery_windows.is_empty() {
            // Nothing can seed a replacement device yet. The next window's
            // first frame finds the loss again and recovers with its surface.
            self.graphics.rearm_device_loss();
            return;
        }
        recovery_windows.sort_unstable();
        // Any live window may seed the replacement device, since adapter choice
        // depends on its surface. A device failure does not, so stop there.
        let mut rebuilt = None;
        for &id in &recovery_windows {
            let surface = &mut self.window_contexts.get_mut(&id).unwrap().surface;
            match pollster::block_on(crate::HostedGpuShared::rebuild_for_surface(surface)) {
                Ok(graphics) => {
                    rebuilt = Some((id, graphics));
                    break;
                }
                Err(HostedGpuError::Device(_)) => break,
                Err(_) => {}
            }
        }
        let Some((base, graphics)) = rebuilt else {
            // Retried every GPU_RETRY_INTERVAL; report the first failure only.
            if self.next_gpu_retry.is_none() {
                nana_diagnostics::fault!(nana_diagnostics::framework::gpu::DEVICE_RECOVERY_FAILED);
            }
            self.render_suspended = true;
            self.next_gpu_retry = Some(Instant::now() + GPU_RETRY_INTERVAL);
            return;
        };
        let mut outcomes = vec![(base, Ok(()))];
        for &id in recovery_windows.iter().filter(|&&id| id != base) {
            let surface = &mut self.window_contexts.get_mut(&id).unwrap().surface;
            outcomes.push((id, graphics.recreate_surface(surface)));
        }
        self.switch_gpu(graphics, outcomes);
    }

    /// Adopt a replacement device. Windows whose surface failed on it recover
    /// individually; every other window presents on the new device immediately.
    /// Adopt `graphics`, onto which every window's surface was already rebound.
    pub(super) fn switch_gpu(
        &mut self,
        graphics: crate::HostedGpuShared,
        outcomes: Vec<(WindowId, Result<(), HostedGpuError>)>,
    ) {
        let mut failed = Vec::new();
        for (id, outcome) in outcomes {
            self.window_contexts.get_mut(&id).unwrap().surface_retry = None;
            if let Err(error) = outcome {
                failed.push((id, error));
            }
        }
        self.graphics = graphics;
        self.reset_startup_latch();
        // A replacement device can be on a different backend than the one this
        // process started on. Whether a window opened from now on can reach a
        // platform compositor is that device's answer, not the old one's.
        self.composition = crate::presentation::CompositionAvailability::for_backend(
            self.gpu_backend_policy,
            self.graphics.adapter_info().backend,
        );
        self.painters.clear();
        self.native_renderers.clear();
        self.next_gpu_retry = None;
        self.render_suspended = false;
        self.bump_surface_generation();
        for (id, error) in failed {
            self.suspend_surface(id, error);
        }
        self.refresh_material();
        invalidate_program_host_textures(self.known_window_ids(), |id| {
            self.program.host_textures(id)
        });
        self.program.rebuild_gpu(&self.context());
        crate::host_diagnostics::record_adapter(self.graphics.adapter_info());
        nana_diagnostics::event!(nana_diagnostics::framework::gpu::DEVICE_RECOVERED);
        self.request_redraw_all();
    }
    pub(super) fn suspend_surface(&mut self, id: WindowId, error: HostedGpuError) {
        // WGPU delivers a destroyed device's callback during polling, after its
        // submissions finish. Surface failure alone must not decide its scope.
        let _ = self.graphics.gpu().raw_device().poll(wgpu::PollType::Poll);
        // A lost device fails every surface; leave it to process-wide recovery
        // instead of reporting per-window surface failures.
        if !self.embedded && self.graphics.is_device_lost() {
            return;
        }
        let Some(host) = self.window_contexts.get_mut(&id) else {
            return;
        };
        // Surface errors do not imply device loss. Keep the other windows alive,
        // and never let repeated appearance updates postpone or bypass this retry.
        if host.surface_retry.is_none() {
            host.surface_retry = Some(Instant::now() + GPU_RETRY_INTERVAL);
            nana_diagnostics::event!(
                nana_diagnostics::framework::gpu::SURFACE_SUSPENDED,
                window = id.0
            );
            self.program
                .report_host_failure(HostFailure::SurfaceRecovery {
                    window: id,
                    error: error.to_string(),
                });
        }
    }

    pub(super) fn retry_surfaces(&mut self, now: Instant) {
        if self.render_suspended {
            return;
        }
        let mut recovered = Vec::new();
        for id in self.known_window_ids() {
            let host = self.window_contexts.get_mut(&id).unwrap();
            if retry_surface(&mut host.surface, &mut host.surface_retry, now, |surface| {
                self.graphics.recreate_surface(surface)
            }) {
                host.applied_appearance = None;
                recovered.push(id);
            }
        }
        if recovered.is_empty() {
            return;
        }
        self.bump_surface_generation();
        for id in recovered {
            if let Err(error) = self.sync_window_material(id) {
                self.suspend_surface(id, error);
            } else {
                self.request_redraw(id);
            }
        }
    }
    /// Painters are created lazily per surface format and own their image waker
    /// from creation, so the per-frame lookup does no allocation.
    pub(super) fn painter_mut(
        &mut self,
        format: nana_gpu::GpuTextureFormat,
    ) -> &mut SceneWgpuPainter {
        // A device switch clears the map; a painter that still names another
        // device would only refuse every frame.
        let current = self.graphics.gpu().generation();
        // 圆角指数编进了着色器模块，设置变了只能整份重建。
        let exponent = crate::scene_paint::corner_exponent();
        if self.painters.get(&format).is_some_and(|painter| {
            painter.gpu().generation() != current || painter.corner_exponent() != exponent
        }) {
            self.painters.remove(&format);
        }
        if !self.painters.contains_key(&format) {
            let painter = SceneWgpuPainter::new(self.graphics.gpu(), format);
            self.adopt_painter(format, painter);
        }
        self.painters
            .get_mut(&format)
            .expect("painter was just inserted")
    }

    /// Installs a painter built elsewhere (the startup thread builds the first
    /// one alongside the device) with this host's image waker.
    pub(super) fn adopt_painter(
        &mut self,
        format: nana_gpu::GpuTextureFormat,
        mut painter: SceneWgpuPainter,
    ) {
        let (targets, redraws, proxy) = (
            Arc::clone(&self.image_targets),
            Arc::clone(&self.texture_redraws),
            self.proxy.clone(),
        );
        painter.set_image_update_waker(Arc::new(move |key| {
            let ids = targets
                .lock()
                .ok()
                .and_then(|targets| targets.get(key).cloned())
                .unwrap_or_default();
            if !ids.is_empty()
                && let Ok(mut pending) = redraws.lock()
            {
                pending.extend(ids);
            }
            proxy.wake_up();
        }));
        self.note_startup_painter();
        self.painters.insert(format, painter);
    }
}

/// Preserve the retained native target until a complete replacement is ready.
/// A failed attempt consumes its deadline, so repeated loop wakes cannot spin.
fn retry_surface<T, E>(
    surface: &mut T,
    deadline: &mut Option<Instant>,
    now: Instant,
    rebind: impl FnOnce(&mut T) -> Result<(), E>,
) -> bool {
    if !deadline.is_some_and(|deadline| now >= deadline) {
        return false;
    }
    match rebind(surface) {
        Ok(()) => {
            *deadline = None;
            true
        }
        Err(_) => {
            *deadline = Some(now + GPU_RETRY_INTERVAL);
            false
        }
    }
}

#[cfg(test)]
mod surface_recovery_tests {
    use super::*;

    #[test]
    fn failed_window_retains_surface_and_backs_off_without_touching_healthy_window() {
        let now = Instant::now();
        let mut failed_surface = 10;
        let mut healthy_surface = 20;
        let mut failed_deadline = Some(now);
        let mut healthy_deadline = None;
        assert!(!retry_surface(
            &mut failed_surface,
            &mut failed_deadline,
            now,
            |_| Err::<(), _>("unavailable")
        ));
        assert_eq!(failed_surface, 10);
        assert_eq!(failed_deadline, Some(now + GPU_RETRY_INTERVAL));
        for time in [now, now + GPU_RETRY_INTERVAL / 2] {
            assert!(!retry_surface(
                &mut failed_surface,
                &mut failed_deadline,
                time,
                |_| -> Result<(), ()> { panic!("retried before deadline") }
            ));
            assert!(!retry_surface(
                &mut healthy_surface,
                &mut healthy_deadline,
                time,
                |_| -> Result<(), ()> { panic!("healthy window recreated") }
            ));
        }
        assert!(retry_surface(
            &mut failed_surface,
            &mut failed_deadline,
            now + GPU_RETRY_INTERVAL,
            |surface| {
                assert_eq!(*surface, 10);
                *surface = 11;
                Ok::<_, ()>(())
            }
        ));
        assert_eq!(failed_surface, 11);
        assert_eq!(failed_deadline, None);
        assert_eq!(healthy_surface, 20);
        assert_eq!(healthy_deadline, None);
    }
}

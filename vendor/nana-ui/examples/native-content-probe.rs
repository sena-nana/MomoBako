//! Real Windows DirectComposition acceptance probe.
//!
//! Default run: two composed windows and one plain window on one GPU device,
//! then a steady-state check (a static composition tree is not re-synchronised
//! by continuous GPU frames) and a move-only check (moving a window costs no
//! chrome rewrite and no compositor work).
//!
//! `--hold` keeps one window up for the non-client rendering strategy
//! comparison. `NANA_FORCE_COMPOSITION_FAILURE=1` makes the composition target
//! fail after its window was created, to exercise the fallback.
//! `--surface-lifecycle` additionally verifies surface and device recreation on
//! one tree.
#[cfg(target_os = "windows")]
mod windows_probe {
    use nana_ui::runtime::view::widget;
    use nana_ui::runtime::{DocumentId, NativeContent, RuntimeDocument, Stack, Text};
    use nana_ui::{
        DocumentAccessError, GpuBackendPolicy, HostedSurfaceMode, NativeContentRegion,
        RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate, SurfaceTargetFallback,
        WindowDescriptor, WindowSurfacePreference, WindowSurfaceTarget, WindowsCompositionRect,
        WindowsCompositionTree, WindowsNativeVisual,
    };
    use nana_ui_platform::host::WindowCommand;
    use nana_ui_platform::{WindowEvent, WindowId};
    use std::{collections::BTreeMap, convert::Infallible, sync::Arc, time::Duration};
    const AUX: WindowId = WindowId(1);
    /// A plain opaque window in the same process, on the platform's own window
    /// surface. Acceptance E: one GPU device, two different surface targets.
    const SETTINGS: WindowId = WindowId(2);
    /// A window that asks for a transparent client on the plain DX12 path,
    /// where the surface can only negotiate `Opaque`. Acceptance A: the request
    /// falls back to `Solid` and the window's native chrome must be the opaque
    /// policy — and must stay that way across every later style change.
    const FALLBACK: WindowId = WindowId(3);
    /// Move steps the probe walks the auxiliary window through.
    const MOVE_STEPS: u32 = 60;
    /// Frames each window presents over a static scene before the steady-state
    /// check is considered done.
    const STEADY_FRAMES: usize = 120;

    /// Which acceptance the probe is currently running.
    ///
    /// An explicit phase rather than inferring one from empty maps: "not
    /// settled yet" and "settled, no frame presented yet" look identical from
    /// the counters, and a gate that cannot tell them apart stops asking for
    /// frames and the probe hangs.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Phase {
        /// Resize and visibility are still changing the scene.
        Warmup,
        /// The scene is static; every frame must find the compositor in step.
        Steady,
        /// The window is being moved and must cost no compositor work.
        Moving,
        Done,
    }

    /// `--hold` keeps the window open for the non-client strategy comparison
    /// instead of running the automated sequence.
    fn hold() -> bool {
        std::env::args().any(|arg| arg == "--hold")
    }

    /// Whether this run asked the host to fail the composition target on
    /// purpose, to exercise the fallback the acceptance needs covered.
    fn forcing_composition_failure() -> bool {
        std::env::var_os("NANA_FORCE_COMPOSITION_FAILURE")
            .is_some_and(|value| !value.is_empty() && value != "0")
    }

    /// What changed between two readings of the same counters.
    fn composition_delta(
        now: nana_ui::CompositionWork,
        base: nana_ui::CompositionWork,
    ) -> nana_ui::CompositionWork {
        nana_ui::CompositionWork {
            commits: now.commits.saturating_sub(base.commits),
            tree_mutations: now.tree_mutations.saturating_sub(base.tree_mutations),
            native_chrome_writes: now
                .native_chrome_writes
                .saturating_sub(base.native_chrome_writes),
            native_content: nana_ui::NativeContentWork {
                region_rebuilds: now
                    .native_content
                    .region_rebuilds
                    .saturating_sub(base.native_content.region_rebuilds),
                regions_considered: now
                    .native_content
                    .regions_considered
                    .saturating_sub(base.native_content.regions_considered),
                regions_changed: now
                    .native_content
                    .regions_changed
                    .saturating_sub(base.native_content.regions_changed),
            },
        }
    }

    /// Writes the performance-contract envelope for this run.
    ///
    /// The counters are deltas over the settled stretch, which is what
    /// `perf/scenarios/windows-composition-steady.json` gates. Only a real run
    /// on real hardware writes this file; there is no synthetic substitute, so
    /// a machine that cannot reach a composition target must produce no report
    /// rather than a report full of zeros.
    fn write_contract_report(delta: &nana_ui::CompositionWork) {
        let work = [
            ("commits", delta.commits),
            ("tree_mutations", delta.tree_mutations),
            (
                "native_content_region_rebuilds",
                delta.native_content.region_rebuilds,
            ),
            (
                "native_content_regions_considered",
                delta.native_content.regions_considered,
            ),
            (
                "native_content_regions_changed",
                delta.native_content.regions_changed,
            ),
            ("native_chrome_writes", delta.native_chrome_writes),
        ]
        .map(|(key, value)| format!("    \"{key}\": {value}"))
        .join(",\n");
        let report = [
            "{".to_owned(),
            "  \"schema_version\": 1,".to_owned(),
            "  \"runner\": \"nana\",".to_owned(),
            "  \"status\": \"ok\",".to_owned(),
            "  \"scenario_id\": \"windows-composition-steady\",".to_owned(),
            "  \"composition_work\": {".to_owned(),
            work,
            "  }".to_owned(),
            "}".to_owned(),
        ]
        .join("\n")
            + "\n";
        let directory = std::path::Path::new("target/performance");
        let path = directory.join("windows-composition-steady.json");
        match std::fs::create_dir_all(directory).and_then(|()| std::fs::write(&path, report)) {
            Ok(()) => println!("CONTRACT_REPORT {}", path.display()),
            Err(error) => println!("CONTRACT_REPORT_FAILED {error}"),
        }
    }

    /// Prints what this run actually resolved, so the two non-client
    /// strategies can be compared from the same evidence on one machine.
    fn report_presentation(context: &RuntimeProgramContext<Message>) {
        let presentation = context.presentation();
        let chrome = presentation.chrome();
        println!(
            "PRESENTATION requested={:?} effective={:?} fallback={:?} alpha={:?}",
            presentation.requested(),
            presentation.effective().effect,
            presentation.effective().fallback,
            presentation.alpha_mode()
        );
        println!(
            "PRESENTATION_TARGET requested={:?} resolved={:?} fallback={:?}",
            presentation.requested_target(),
            presentation.surface_target(),
            presentation.target_fallback()
        );
        println!(
            "NC_STRATEGY strategy={:?} suppress={} frameless={} rounded={}",
            chrome.map(|chrome| chrome.non_client_strategy),
            chrome.is_some_and(|chrome| chrome.suppress_non_client),
            chrome.is_some_and(|chrome| chrome.frameless()),
            chrome.is_some_and(|chrome| chrome.rounded_corners)
        );
    }
    #[derive(Clone)]
    pub enum Message {
        Resize,
        Visibility(bool),
        /// Start the steady-state check: from here the scene stands still and
        /// the compositor must stop being told about it.
        Settle,
        /// Move a window without changing anything else. A move is not a
        /// window flag, so it must cost no chrome rewrite and no compositor
        /// work at all.
        MoveOnly(u32),
        Finish,
    }
    pub struct Probe {
        documents: BTreeMap<WindowId, RuntimeDocument>,
        visuals: BTreeMap<WindowId, WindowsNativeVisual>,
        visible: bool,
        seen_primary: bool,
        seen_auxiliary: bool,
        /// Compositor work as of the settle point, per window.
        settled: BTreeMap<WindowId, nana_ui::CompositionWork>,
        /// Frames presented since then, per window.
        steady_frames: BTreeMap<WindowId, usize>,
        /// Compositor and chrome work as of the first move step.
        move_baseline: Option<nana_ui::CompositionWork>,
        phase: Phase,
        /// The one GPU device every window in this process must share.
        device_generation: nana_ui::DeviceGeneration,
        /// Counter deltas across the whole settled stretch, including the
        /// move-only leg. The performance contract judges these, so they are
        /// deltas from the settle point rather than totals: the window did real
        /// work to get there, and what is gated is that it stops.
        steady_delta: nana_ui::CompositionWork,
    }
    impl Probe {
        /// `documents` is the only thing the two entry paths disagree on:
        /// `--hold` opens the primary window alone.
        fn new(
            documents: BTreeMap<WindowId, RuntimeDocument>,
            device_generation: nana_ui::DeviceGeneration,
        ) -> Self {
            Self {
                documents,
                visuals: BTreeMap::new(),
                visible: true,
                seen_primary: false,
                seen_auxiliary: false,
                settled: BTreeMap::new(),
                steady_frames: BTreeMap::new(),
                move_baseline: None,
                phase: Phase::Warmup,
                device_generation,
                steady_delta: nana_ui::CompositionWork::default(),
            }
        }
    }

    /// A transparent client pinned to the platform's own window surface.
    ///
    /// On DX12 that surface only ever negotiates `Opaque`, so this is the
    /// window whose request must come back as a reported fallback with opaque
    /// chrome — the P0 inconsistency, on real hardware.
    fn transparent_on_the_plain_path() -> WindowDescriptor {
        let mut settings = WindowDescriptor::new("Transparent on the plain path")
            .initial_size(360.0, 260.0)
            .surface(WindowSurfacePreference::NativeWindow);
        settings.transparent = true;
        settings
    }

    /// Asserts the fallback window's presentation, which must hold on every
    /// frame it ever presents, not just the first.
    fn assert_plain_path_fallback(context: &RuntimeProgramContext<Message>) {
        use nana_ui::{MaterialEffect, MaterialFallback, NativeChromePolicy};
        let presentation = context.presentation();
        assert_eq!(presentation.requested(), MaterialEffect::Transparent);
        assert_eq!(
            presentation.surface_target(),
            WindowSurfaceTarget::NativeWindow
        );
        assert_eq!(
            presentation.effective().effect,
            MaterialEffect::Solid,
            "a DX12 HWND surface cannot present this request"
        );
        assert_eq!(
            presentation.effective().fallback,
            Some(MaterialFallback::NativeMaterialUnavailable)
        );
        assert_eq!(
            presentation.chrome(),
            Some(NativeChromePolicy::OPAQUE),
            "the renderer says Solid, so the HWND must not wear transparent chrome"
        );
    }

    /// A window with no native content at all. The plain settings window in
    /// acceptance E is an ordinary opaque window: it has no composition target,
    /// so a native-content node in it would have no backend to reach.
    fn plain_document(id: WindowId) -> RuntimeDocument {
        let document_id = DocumentId::new(id.0 + 1).unwrap();
        let mut document = RuntimeDocument::new(document_id);
        document
            .context_mut()
            .mount_view_root(document_id, || {
                widget(Stack::fill_column(12.0).padding(16.0))
                    .children(widget(Text::new("Plain window, native surface")))
            })
            .unwrap();
        document
    }

    fn document(id: WindowId) -> RuntimeDocument {
        let document_id = DocumentId::new(id.0 + 1).unwrap();
        let mut document = RuntimeDocument::new(document_id);
        document
            .context_mut()
            .mount_view_root(document_id, || {
                widget(Stack::fill_column(12.0).padding(16.0)).children((
                    widget(Text::new("DirectComposition native-content probe")),
                    widget(NativeContent::new("probe-native")),
                    widget(Text::new("NanaUI controls remain on the UI surface")),
                ))
            })
            .unwrap();
        document
    }
    impl RuntimeProgram for Probe {
        type Message = Message;
        type Error = Infallible;
        fn theme_mode(&self) -> nana_ui::ThemeMode {
            nana_ui::ThemeMode::Dark
        }
        fn gpu_backend_policy() -> GpuBackendPolicy {
            GpuBackendPolicy::CompositionCapable
        }
        fn initialize(
            context: &RuntimeProgramContext<Message>,
        ) -> Result<(Self, Vec<Message>), Infallible> {
            report_presentation(context);
            let presentation = context.presentation();
            if forcing_composition_failure() {
                // Acceptance B: the composition target was made to fail after
                // the window had already been created for it. The application
                // must still be running, on the plain path, and must be able to
                // say so.
                assert_eq!(
                    presentation.requested_target(),
                    WindowSurfaceTarget::Composition,
                    "this run asked for composition"
                );
                assert_eq!(
                    presentation.surface_target(),
                    WindowSurfaceTarget::NativeWindow,
                    "a failed composition target has to leave a plain window behind"
                );
                assert_eq!(
                    presentation.target_fallback(),
                    Some(SurfaceTargetFallback::TargetUnavailable)
                );
            } else {
                assert_eq!(
                    presentation.surface_target(),
                    WindowSurfaceTarget::Composition
                );
                assert_eq!(
                    context.surface_alpha_mode(),
                    nana_ui::SurfaceAlphaMode::PreMultiplied
                );
                assert_eq!(
                    context.gpu().capabilities().backend(),
                    nana_ui::GpuBackend::Dx12
                );
            }
            if hold() {
                // Non-client strategy acceptance: the window stays up so an
                // operator can try Aero Snap, the Snap Layouts hover menu,
                // Alt+Space, minimize/maximize animations, resize and a DPI
                // change against the strategy this run is using. Nothing here
                // can measure those; the run only pins down what it applied.
                println!("NC_STRATEGY_HOLD interact and close the window when done");
                return Ok((
                    Self::new(
                        BTreeMap::from([(WindowId::PRIMARY, document(WindowId::PRIMARY))]),
                        context.gpu().generation(),
                    ),
                    vec![],
                ));
            }
            let sender = context.clone();
            std::thread::spawn(move || {
                for message in [
                    Message::Resize,
                    Message::Visibility(false),
                    Message::Visibility(true),
                    Message::Settle,
                ] {
                    std::thread::sleep(Duration::from_secs(2));
                    sender.dispatch(message);
                }
            });
            Ok((
                Self::new(
                    BTreeMap::from([
                        (WindowId::PRIMARY, document(WindowId::PRIMARY)),
                        (AUX, document(AUX)),
                        (SETTINGS, plain_document(SETTINGS)),
                        (FALLBACK, plain_document(FALLBACK)),
                    ]),
                    context.gpu().generation(),
                ),
                vec![],
            ))
        }
        fn with_document<R>(
            &self,
            id: WindowId,
            f: impl FnOnce(&RuntimeDocument) -> R,
        ) -> Result<Option<R>, DocumentAccessError> {
            Ok(self.documents.get(&id).map(f))
        }
        fn with_document_mut<R>(
            &mut self,
            id: WindowId,
            f: impl FnOnce(&mut RuntimeDocument) -> R,
        ) -> Result<Option<R>, DocumentAccessError> {
            Ok(self.documents.get_mut(&id).map(f))
        }
        fn update(
            &mut self,
            message: Message,
            context: &RuntimeProgramContext<Message>,
        ) -> RuntimeProgramUpdate {
            match message {
                Message::Resize => RuntimeProgramUpdate {
                    window_commands: vec![
                        WindowCommand::SetBounds {
                            id: AUX,
                            position: (480.0, 180.0),
                            size: (500.0, 380.0),
                        },
                        // A maximize is the style rewrite most likely to bring
                        // transparent chrome back, so the fallback window takes
                        // one and its per-frame assertion has to survive it.
                        WindowCommand::SetMaximized {
                            id: FALLBACK,
                            maximized: true,
                        },
                    ],
                    ..RuntimeProgramUpdate::redraw_all()
                },
                Message::Visibility(visible) => {
                    self.visible = visible;
                    // Staged here, not inside `native_content_frame`: the host
                    // only calls that back when the *scene's* regions moved,
                    // and this is the backend's own reason to change a visual.
                    // The next frame's commit publishes it, because staging is
                    // what makes the tree dirty.
                    for visual in self.visuals.values() {
                        visual.set_visible(visible).expect("retained visual");
                    }
                    println!("NATIVE_VISIBLE {visible}");
                    RuntimeProgramUpdate::redraw_all()
                }
                Message::Settle => {
                    // Nothing below this point changes the scene, so every
                    // frame from here is a GPU frame over a static visual tree.
                    self.settled.clear();
                    self.steady_frames.clear();
                    self.phase = Phase::Steady;
                    RuntimeProgramUpdate::redraw_all()
                }
                Message::MoveOnly(step) => {
                    let baseline = self
                        .move_baseline
                        .get_or_insert_with(|| context.composition_work());
                    let work = context.composition_work();
                    assert_eq!(
                        work, *baseline,
                        "moving a window rewrote chrome or touched the compositor"
                    );
                    self.steady_delta = composition_delta(work, *baseline);
                    if step == MOVE_STEPS {
                        println!("NATIVE_MOVE_ONLY_PASS steps={step}");
                        self.phase = Phase::Done;
                        context.dispatch(Message::Finish);
                        return RuntimeProgramUpdate::default();
                    }
                    context.dispatch(Message::MoveOnly(step + 1));
                    RuntimeProgramUpdate {
                        window_commands: vec![WindowCommand::Move {
                            id: AUX,
                            position: (480.0 + step as f32, 180.0),
                        }],
                        ..RuntimeProgramUpdate::default()
                    }
                }
                Message::Finish => {
                    if forcing_composition_failure() {
                        // No window reached a composition tree, so none of them
                        // submitted native regions. What this run proves is
                        // that the application ran anyway.
                        assert!(!self.seen_primary && !self.seen_auxiliary);
                        println!("COMPOSITION_FALLBACK_PASS application ran on the plain path");
                        return RuntimeProgramUpdate::exit();
                    }
                    assert!(
                        self.seen_primary && self.seen_auxiliary,
                        "both composed windows must submit native regions"
                    );
                    for (id, frames) in &self.steady_frames {
                        println!(
                            "NATIVE_STEADY_STATE window={} frames={frames} work={:?}",
                            id.0,
                            self.settled.get(id).copied().unwrap_or_default()
                        );
                    }
                    write_contract_report(&self.steady_delta);
                    for visual in self.visuals.values() {
                        visual.remove().unwrap();
                    }
                    self.visuals.clear();
                    println!("RUNTIME_NATIVE_COMPOSITION_PASS");
                    RuntimeProgramUpdate::exit()
                }
            }
        }
        fn native_content_frame(
            &mut self,
            id: WindowId,
            composition: &WindowsCompositionTree,
            regions: &[NativeContentRegion],
            context: &RuntimeProgramContext<Message>,
        ) -> Result<(), String> {
            if regions.is_empty() {
                return Ok(());
            }
            if id == WindowId::PRIMARY {
                self.seen_primary = true;
            } else {
                self.seen_auxiliary = true;
            }
            let visual = match self.visuals.entry(id) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    println!("NATIVE_REGION_READY {} {:?}", id.0, regions[0].bounds);
                    entry.insert(
                        composition
                            .create_native_visual()
                            .map_err(|e| e.to_string())?,
                    )
                }
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            };
            let region = &regions[0];
            let scale = context.geometry().scale_factor;
            let rect = |r: nana_ui_scene::SceneRect| WindowsCompositionRect {
                x: r.x * scale,
                y: r.y * scale,
                width: r.width * scale,
                height: r.height * scale,
            };
            visual
                .set_geometry(rect(region.bounds), Some(rect(region.clip)))
                .map_err(|e| e.to_string())?;
            visual
                .set_visible(self.visible)
                .map_err(|e| e.to_string())?;
            // No commit here: the Scene host owns the transaction and
            // publishes this frame's staged changes once, for every window.
            Ok(())
        }
        /// The steady-state gate: once the scene has settled, every further
        /// presented frame must find the compositor already in step. A
        /// retained tree is not re-derived, not re-staged and not committed
        /// again just because the GPU produced another frame.
        fn window_frame_presented(
            &mut self,
            id: WindowId,
            context: &RuntimeProgramContext<Message>,
        ) -> RuntimeProgramUpdate {
            if id == FALLBACK {
                // Checked on every frame, so a maximize, a restore, a DPI
                // change or any other style rewrite anywhere in this run cannot
                // walk the chrome back to the transparent policy.
                assert_plain_path_fallback(context);
            }
            if self.phase != Phase::Steady {
                return RuntimeProgramUpdate::default();
            }
            let work = context.composition_work();
            let Some(baseline) = self.settled.get(&id).copied() else {
                // First frame after settling: this one may still publish the
                // change the settle message itself asked to redraw.
                self.settled.insert(id, work);
                self.steady_frames.insert(id, 0);
                return RuntimeProgramUpdate::redraw_all();
            };
            assert_eq!(
                work, baseline,
                "window {} re-synchronised a static composition tree",
                id.0
            );
            // Equal to the baseline, so the delta this window contributes is
            // zero — recorded explicitly rather than assumed, because the
            // contract judges the number, not the assertion.
            self.steady_delta = composition_delta(work, baseline);
            let frames = self.steady_frames.entry(id).or_default();
            *frames += 1;
            if self
                .steady_frames
                .values()
                .all(|frames| *frames >= STEADY_FRAMES)
            {
                self.settled.insert(id, work);
                self.phase = Phase::Moving;
                context.dispatch(Message::MoveOnly(0));
                RuntimeProgramUpdate::default()
            } else {
                RuntimeProgramUpdate::redraw_all()
            }
        }
        fn window_event(
            &mut self,
            event: WindowEvent,
            context: &RuntimeProgramContext<Message>,
        ) -> RuntimeProgramUpdate {
            match event {
                WindowEvent::Ready {
                    id: WindowId::PRIMARY,
                    ..
                } => RuntimeProgramUpdate {
                    window_commands: vec![
                        WindowCommand::Open {
                            id: AUX,
                            settings: WindowDescriptor::new("Native auxiliary")
                                .initial_size(420.0, 300.0)
                                .surface(WindowSurfacePreference::Composition),
                        },
                        WindowCommand::Open {
                            id: SETTINGS,
                            // Acceptance E: an ordinary opaque window in a
                            // process whose other windows are composed. It
                            // stays on the platform's own window surface and
                            // shares the one GPU device.
                            settings: WindowDescriptor::new("Plain settings")
                                .initial_size(320.0, 240.0)
                                .surface(WindowSurfacePreference::NativeWindow),
                        },
                        WindowCommand::Open {
                            id: FALLBACK,
                            settings: transparent_on_the_plain_path(),
                        },
                    ],
                    ..RuntimeProgramUpdate::redraw_all()
                },
                WindowEvent::Ready { id, .. } => {
                    let presentation = context.presentation();
                    // A run that forced the composition target to fail has
                    // narrowed the whole process: no window gets it, and every
                    // one of them still opens.
                    let expected = if id == SETTINGS || forcing_composition_failure() {
                        WindowSurfaceTarget::NativeWindow
                    } else {
                        WindowSurfaceTarget::Composition
                    };
                    assert_eq!(
                        presentation.surface_target(),
                        expected,
                        "window {} took the wrong surface target",
                        id.0
                    );
                    assert_eq!(
                        context.gpu().generation(),
                        self.device_generation,
                        "every window in this process shares one GPU device"
                    );
                    println!(
                        "WINDOW_TARGET window={} target={:?} device={}",
                        id.0,
                        presentation.surface_target(),
                        context.gpu().generation()
                    );
                    RuntimeProgramUpdate::redraw_all()
                }
                WindowEvent::OpenFailed { error, .. } => panic!("{error}"),
                WindowEvent::Closed { id } => {
                    self.visuals.remove(&id);
                    RuntimeProgramUpdate::default()
                }
                WindowEvent::CloseRequested { .. } => RuntimeProgramUpdate::exit(),
                _ => RuntimeProgramUpdate::default(),
            }
        }
    }

    pub fn surface_lifecycle() {
        use nana_ui::{HostedGpuContext, HostedSurfaceFrame};
        use winit::{
            application::ApplicationHandler,
            event::WindowEvent,
            event_loop::{ActiveEventLoop, EventLoop},
            window::{WindowAttributes, WindowId},
        };
        #[derive(Default)]
        struct Lifecycle {
            done: bool,
        }
        impl ApplicationHandler for Lifecycle {
            fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
                if self.done {
                    return;
                }
                self.done = true;
                let window: Arc<dyn winit::window::Window> = Arc::from(
                    event_loop
                        .create_window(
                            WindowAttributes::default().with_title("DComp lifecycle probe"),
                        )
                        .unwrap(),
                );
                let auxiliary: Arc<dyn winit::window::Window> = Arc::from(
                    event_loop
                        .create_window(
                            WindowAttributes::default().with_title("DComp lifecycle auxiliary"),
                        )
                        .unwrap(),
                );
                let mut graphics = pollster::block_on(HostedGpuContext::new_with_surface_mode(
                    window,
                    wgpu::Features::empty(),
                    false,
                    HostedSurfaceMode::WindowsComposition,
                ))
                .unwrap();
                let mut aux = graphics
                    .create_surface_with_mode(
                        auxiliary,
                        false,
                        HostedSurfaceMode::WindowsComposition,
                    )
                    .unwrap();
                let original = graphics.windows_composition().unwrap().clone();
                let visual = original.create_native_visual().unwrap();
                let identity = visual.as_raw();
                visual
                    .set_geometry(
                        WindowsCompositionRect {
                            x: 20.0,
                            y: 20.0,
                            width: 200.0,
                            height: 100.0,
                        },
                        None,
                    )
                    .unwrap();
                visual.set_visible(true).unwrap();
                original.commit().unwrap();
                graphics.recover_surface().unwrap();
                pollster::block_on(graphics.recreate(wgpu::Features::empty())).unwrap();
                graphics.recreate_surface(&mut aux).unwrap();
                assert_eq!(visual.as_raw(), identity);
                assert_eq!(
                    graphics.windows_composition().unwrap().window_handle(),
                    original.window_handle()
                );
                assert_eq!(
                    graphics.alpha_mode(),
                    nana_ui::SurfaceAlphaMode::PreMultiplied
                );
                let mut presented = 0;
                for auxiliary in [false, true] {
                    let frame = if auxiliary {
                        graphics.acquire_surface_frame(&mut aux)
                    } else {
                        graphics.acquire_frame()
                    }
                    .unwrap();
                    let HostedSurfaceFrame::Ready(frame) = frame else {
                        panic!("surface must produce a drawable frame");
                    };
                    let view = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    let mut encoder = graphics
                        .gpu()
                        .wgpu()
                        .device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                    {
                        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 0.1,
                                        g: 0.2,
                                        b: 0.3,
                                        a: 1.0,
                                    }),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            ..Default::default()
                        });
                    }
                    graphics.gpu().wgpu().queue().submit([encoder.finish()]);
                    graphics.present(frame);
                    presented += 1;
                }
                visual.remove().unwrap();
                visual.remove().unwrap();
                assert!(visual.set_visible(true).is_err());
                original.commit().unwrap();
                println!(
                    "SURFACE_LIFECYCLE_PASS presented={presented} retained_native_visual=true"
                );
                event_loop.exit();
            }
            fn window_event(&mut self, _: &dyn ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        }
        EventLoop::new()
            .unwrap()
            .run_app(Lifecycle::default())
            .unwrap();
    }
}
#[cfg(target_os = "windows")]
fn main() {
    if std::env::args().any(|arg| arg == "--surface-lifecycle") {
        windows_probe::surface_lifecycle();
    } else {
        nana_ui::run_runtime::<windows_probe::Probe>(
            nana_ui::WindowDescriptor::new("NanaUI native composition probe")
                .initial_size(800.0, 600.0)
                .surface(nana_ui::WindowSurfacePreference::Composition),
        )
        .unwrap();
    }
}
#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("DirectComposition is available on Windows only");
}

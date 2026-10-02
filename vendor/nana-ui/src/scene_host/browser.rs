use super::*;
use crate::native_browser::{
    BrowserCommand, BrowserEvent, BrowserRect, BrowserState, NativeBrowserEvent,
    NativeBrowserRequest,
};

pub(super) struct HostedBrowser {
    browser: Option<nana_window::NativeBrowser>,
    node: StableNodeId,
    policy: nana_window::BrowserPolicy,
    revision: Option<u64>,
    events: Vec<nana_window::BrowserCompletion>,
}

fn request_node_state(
    document: &nana_ui_scene::RuntimeDocument,
    request: &NativeBrowserRequest,
) -> Option<bool> {
    let context = document.context();
    let node = context.world().node(request.node)?;
    if node.document != document.document()
        || !context.world().is_mounted(request.node)
        || request.id.is_empty()
        || !context
            .read(
                Entity::<nana_ui_runtime::BrowserView>::from_stable_id(request.node),
                |view| view.browser_id == request.id,
            )
            .unwrap_or(false)
    {
        return None;
    }
    Some(context.world().is_overlay_reachable(request.node))
}

fn initial_browser_command(request: &NativeBrowserRequest) -> BrowserCommand {
    match &request.command {
        Some(BrowserCommand::Navigate(url)) => BrowserCommand::Navigate(url.clone()),
        _ => BrowserCommand::Navigate(request.restore_url.clone()),
    }
}

impl<Program: RuntimeProgram> WindowManager<Program> {
    /// Reconcile against the live Runtime tree before delivering callbacks, even
    /// when its next Scene flush has not happened yet.
    pub(super) fn reconcile_browser_lifetimes(&mut self) {
        if self.browsers.is_empty() {
            return;
        }
        for window in self.known_window_ids() {
            let requests = self.program.native_browser_requests(window);
            self.browsers.retain(|(owner, id), hosted| {
                if *owner != window {
                    return true;
                }
                let mut matching = requests.iter().filter(|request| request.id == *id);
                let Some(request) = matching.next() else {
                    return false;
                };
                if matching.next().is_some()
                    || request.node != hosted.node
                    || request.policy != hosted.policy
                {
                    return false;
                }
                let state = self
                    .program
                    .read_document(window, |document| request_node_state(document, request))
                    .flatten();
                let Some(reachable) = state else {
                    return false;
                };
                if (!request.visible || !reachable)
                    && let Some(browser) = &mut hosted.browser
                {
                    browser.set_geometry(BrowserRect::default(), BrowserRect::default(), false);
                }
                true
            });
        }
    }

    pub(super) fn sync_native_browsers(&mut self, id: WindowId, scene: &nana_ui_scene::UiScene) {
        self.reconcile_browser_lifetimes();
        let requests = self.program.native_browser_requests(id);
        let Some(window) = self.window(id).cloned() else {
            return;
        };
        for request in &requests {
            if requests
                .iter()
                .filter(|other| other.id == request.id)
                .count()
                != 1
            {
                continue;
            }
            let reachable = self
                .program
                .read_document(id, |document| request_node_state(document, request))
                .flatten();
            let Some(reachable) = reachable else {
                continue;
            };
            let key = (id, request.id.clone());
            let geometry =
                native_browser_geometry(scene, request.node, self.geometry_of(id).logical_size);
            let (bounds, clip, unobscured) = geometry.unwrap_or_default();
            let visible = request.visible && reachable && unobscured;
            if !visible && !self.browsers.contains_key(&key) {
                continue;
            }
            let replace = self.browsers.get(&key).is_none_or(|hosted| {
                hosted.node != request.node
                    || hosted.policy != request.policy
                    || (hosted.browser.is_none()
                        && hosted
                            .revision
                            .is_none_or(|revision| request.revision > revision))
            });
            if replace {
                // Drop the old native child before installing a new owner of the same id.
                self.browsers.remove(&key);
                let proxy = self.proxy.clone();
                let result = nana_window::NativeBrowser::new(
                    window.as_ref(),
                    request.policy.clone(),
                    Box::new(move || proxy.wake_up()),
                );
                let (browser, events) = match result {
                    Ok(mut browser) => {
                        browser.set_geometry(bounds, clip, visible);
                        let mut events = Vec::new();
                        if let Err(error) = browser
                            .command(request.revision, Some(&initial_browser_command(request)))
                        {
                            events.push(nana_window::BrowserCompletion {
                                revision: request.revision,
                                event: BrowserEvent::State(BrowserState {
                                    attached: true,
                                    error: Some(error),
                                    ..Default::default()
                                }),
                            });
                        }
                        if matches!(request.command, Some(BrowserCommand::Capture)) {
                            events.push(nana_window::BrowserCompletion {
                                revision: request.revision,
                                event: BrowserEvent::CaptureFailed(
                                    "浏览页面已重新打开，请重新截图".into(),
                                ),
                            });
                        }
                        (Some(browser), events)
                    }
                    Err(error) => (
                        None,
                        vec![nana_window::BrowserCompletion {
                            revision: request.revision,
                            event: BrowserEvent::State(BrowserState {
                                error: Some(error),
                                ..Default::default()
                            }),
                        }],
                    ),
                };
                self.browsers.insert(
                    key.clone(),
                    HostedBrowser {
                        revision: Some(request.revision),
                        browser,
                        node: request.node,
                        policy: request.policy.clone(),
                        events,
                    },
                );
                self.proxy.wake_up();
            }
            let hosted = self.browsers.get_mut(&key).expect("browser inserted");
            if let Some(browser) = &mut hosted.browser {
                browser.set_geometry(bounds, clip, visible);
                if visible
                    && hosted
                        .revision
                        .is_none_or(|revision| request.revision > revision)
                {
                    hosted.revision = Some(request.revision);
                    if let Err(error) = browser.command(request.revision, request.command.as_ref())
                    {
                        hosted.events.push(nana_window::BrowserCompletion {
                            revision: request.revision,
                            event: if matches!(request.command, Some(BrowserCommand::Capture)) {
                                BrowserEvent::CaptureFailed(error)
                            } else {
                                BrowserEvent::State(BrowserState {
                                    error: Some(error),
                                    attached: true,
                                    ..Default::default()
                                })
                            },
                        });
                        self.proxy.wake_up();
                    }
                }
            }
        }
    }

    pub(super) fn drain_browser_events(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.reconcile_browser_lifetimes();
        let mut events = Vec::new();
        for ((window, id), hosted) in &mut self.browsers {
            let mut completed = hosted
                .browser
                .as_mut()
                .map(|browser| browser.take_events())
                .unwrap_or_default();
            completed.append(&mut hosted.events);
            events.extend(completed.into_iter().map(|completion| {
                (
                    *window,
                    NativeBrowserEvent {
                        id: id.clone(),
                        node: hosted.node,
                        revision: completion.revision,
                        event: completion.event,
                    },
                )
            }));
        }
        for (window, event) in events {
            let requests = self.program.native_browser_requests(window);
            let current = requests
                .iter()
                .filter(|request| request.id == event.id)
                .collect::<Vec<_>>();
            if current.len() != 1 {
                continue;
            }
            let request = current[0];
            if !request.visible
                || request.node != event.node
                || request.revision != event.revision
                || self
                    .program
                    .read_document(window, |document| request_node_state(document, request))
                    .flatten()
                    != Some(true)
                || !self.browsers.contains_key(&(window, event.id.clone()))
            {
                continue;
            }
            self.bind_after_present.insert(window);
            let update =
                self.program
                    .native_browser_event(window, event, &self.context_for(window));
            self.apply_update(event_loop, update, None);
            if event_loop.exiting() {
                break;
            }
        }
    }
}

/// Native child views cannot participate in nonrectangular WGPU composition.
/// Hide only while unsupported clipping or a later painted sibling covers this pane.
fn native_browser_geometry(
    scene: &nana_ui_scene::UiScene,
    node: StableNodeId,
    size: (f32, f32),
) -> Option<(BrowserRect, BrowserRect, bool)> {
    let primitives: Vec<_> = scene.primitives().collect();
    let anchor = primitives.iter().find(|primitive| primitive.node == node)?;
    let bounds = translated_rect(scene.node_bounds(node)?, anchor.transform)?;
    if anchor.opacity != 1.0
        || !scene.opacity_groups(node).is_empty()
        || !scene.filter_groups(node).is_empty()
        || matches!(&anchor.kind, nana_ui_scene::ScenePrimitiveKind::Quad { corner_radius, .. } if corner_radius.iter().any(|radius| *radius != 0.0))
    {
        return Some((bounds, BrowserRect::default(), false));
    }
    let mut clip = intersect(
        bounds,
        BrowserRect {
            x: 0.0,
            y: 0.0,
            width: size.0 as f64,
            height: size.1 as f64,
        },
    );
    for region in anchor.clips.iter() {
        if region.polygon_clip.is_some() || region.corner_radius > 0.0 {
            return Some((bounds, clip, false));
        }
        clip = intersect(clip, translated_rect(region.bounds, region.transform)?);
    }
    let obscured = primitives.iter().any(|other| {
        other.node != node
            && (other.z_index, other.document_order) > (anchor.z_index, anchor.document_order)
            && !scene.is_node_in_subtree(node, other.node)
            && other.opacity > 0.0
            && translated_rect(other.bounds, other.transform).is_none_or(|bounds| {
                let mut bounds = bounds;
                for region in other.clips.iter() {
                    if let Some(region) = translated_rect(region.bounds, region.transform) {
                        bounds = intersect(bounds, region);
                    }
                }
                let overlap = intersect(clip, bounds);
                overlap.width > 0.5 && overlap.height > 0.5
            })
    });
    Some((bounds, clip, !obscured))
}

fn translated_rect(
    rect: nana_ui_scene::SceneRect,
    transform: nana_ui_scene::AffineTransform,
) -> Option<BrowserRect> {
    let [a, b, c, d, x, y] = transform.0;
    if a != 1.0
        || b != 0.0
        || c != 0.0
        || d != 1.0
        || transform.1 != [0.0, 0.0]
        || ![rect.x, rect.y, rect.width, rect.height, x, y]
            .iter()
            .all(|value| value.is_finite())
    {
        return None;
    }
    Some(BrowserRect {
        x: (rect.x + x) as f64,
        y: (rect.y + y) as f64,
        width: rect.width as f64,
        height: rect.height as f64,
    })
}

fn intersect(a: BrowserRect, b: BrowserRect) -> BrowserRect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    BrowserRect {
        x,
        y,
        width: (a.x + a.width).min(b.x + b.width).max(x) - x,
        height: (a.y + a.height).min(b.y + b.height).max(y) - y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_ownership_tracks_the_real_retained_node_and_document() {
        use nana_ui_runtime::{BrowserView, DocumentId, Stack};
        let id = DocumentId::new(1).unwrap();
        let mut document = nana_ui_scene::RuntimeDocument::new(id);
        let root = document
            .context_mut()
            .create_component(id, Stack::fill_column(0.0))
            .unwrap();
        let browser = document
            .context_mut()
            .create_detached_component(id, BrowserView::new("page"))
            .unwrap();
        document.context_mut().append_child(root, browser).unwrap();
        let mut request = NativeBrowserRequest {
            id: "page".into(),
            node: browser.stable_id(),
            policy: Default::default(),
            restore_url: "https://example.com/current".into(),
            visible: true,
            revision: 1,
            command: None,
        };
        assert_eq!(request_node_state(&document, &request), Some(true));
        document
            .context_mut()
            .reconcile_children(root.stable_id(), &[])
            .unwrap();
        assert_eq!(request_node_state(&document, &request), None);
        document
            .context_mut()
            .reconcile_children(root.stable_id(), &[browser.stable_id()])
            .unwrap();
        assert_eq!(request_node_state(&document, &request), Some(true));
        request.id = "other".into();
        assert_eq!(request_node_state(&document, &request), None);
        request.id = "page".into();
        document.context_mut().remove_view(browser).unwrap();
        assert_eq!(request_node_state(&document, &request), None);
        let replacement = document
            .context_mut()
            .create_component(id, BrowserView::new("page"))
            .unwrap();
        assert_ne!(replacement.stable_id(), request.node);
        assert_eq!(request_node_state(&document, &request), None);
        let other_document = DocumentId::new(2).unwrap();
        let other = document
            .context_mut()
            .create_component(other_document, BrowserView::new("page"))
            .unwrap();
        request.node = other.stable_id();
        assert_eq!(request_node_state(&document, &request), None);
    }

    #[test]
    fn recreating_a_native_instance_restores_the_page_without_replaying_transient_commands() {
        let mut request = NativeBrowserRequest {
            id: "page".into(),
            node: StableNodeId::new(1).unwrap(),
            policy: Default::default(),
            restore_url: "https://example.com/current".into(),
            visible: true,
            revision: 7,
            command: None,
        };
        for command in [
            BrowserCommand::Capture,
            BrowserCommand::Back,
            BrowserCommand::Forward,
            BrowserCommand::Reload,
            BrowserCommand::Stop,
            BrowserCommand::Focus,
        ] {
            request.command = Some(command);
            assert_eq!(
                initial_browser_command(&request),
                BrowserCommand::Navigate(request.restore_url.clone())
            );
        }
        request.command = Some(BrowserCommand::Navigate("https://example.com/next".into()));
        assert_eq!(initial_browser_command(&request), request.command.unwrap());
    }

    #[test]
    fn later_overlay_hides_browser_only_until_removed() {
        use nana_ui_runtime::{
            BrowserView, DocumentId, LengthSpec, MeasureTextShaper, SemanticColorRole, Stack,
        };
        let id = DocumentId::new(1).unwrap();
        let mut document = nana_ui_scene::RuntimeDocument::new(id);
        use nana_ui_runtime::view::{entity_ref, widget, with_refs};
        let (_, (browser, overlay)) = document
            .context_mut()
            .mount_view_root(id, || {
                let browser = entity_ref::<BrowserView>();
                let overlay = entity_ref::<Stack>();
                let root = widget(Stack::fill_column(0.0)).children((
                    widget(BrowserView::new("page")).entity_ref(browser),
                    widget(
                        Stack::row(0.0)
                            .height(LengthSpec::Px(20.0))
                            .surface(SemanticColorRole::Surface),
                    )
                    .entity_ref(overlay),
                ));
                with_refs(root, (browser, overlay))
            })
            .unwrap();
        document
            .flush(LayoutViewport::new(600.0, 400.0), &mut MeasureTextShaper)
            .unwrap();
        let node = browser.stable_id();
        let mut scene = document.scene().clone();
        assert!(
            native_browser_geometry(&scene, node, (600.0, 400.0))
                .unwrap()
                .2
        );
        let mut projected = document
            .context()
            .world()
            .extract_nodes(&[overlay.stable_id()]);
        projected[0].layout.x = 40.0;
        projected[0].layout.y = 40.0;
        projected[0].layout.width = 120.0;
        projected[0].layout.height = 80.0;
        scene.apply_delta(projected, []);
        assert!(
            !native_browser_geometry(&scene, node, (600.0, 400.0))
                .unwrap()
                .2
        );
        scene.apply_delta([], [overlay.stable_id()]);
        assert!(
            native_browser_geometry(&scene, node, (600.0, 400.0))
                .unwrap()
                .2
        );
    }

    #[test]
    fn scroll_translation_and_clip_keep_original_page_size() {
        let page = translated_rect(
            nana_ui_scene::SceneRect {
                x: 30.0,
                y: 80.0,
                width: 400.0,
                height: 300.0,
            },
            nana_ui_scene::AffineTransform::from_matrix([1.0, 0.0, 0.0, 1.0, 0.0, -60.0]),
        )
        .unwrap();
        let clip = intersect(
            page,
            BrowserRect {
                x: 10.0,
                y: 50.0,
                width: 600.0,
                height: 500.0,
            },
        );
        assert_eq!(page.height, 300.0);
        assert_eq!(clip.y, 50.0);
        assert_eq!(clip.height, 270.0);
        assert_eq!(page.y - clip.y, -30.0);
    }

    #[test]
    fn offscreen_or_non_axis_aligned_content_cannot_intercept_input() {
        let clip = intersect(
            BrowserRect {
                x: -100.0,
                y: -100.0,
                width: 20.0,
                height: 20.0,
            },
            BrowserRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        assert_eq!((clip.width, clip.height), (0.0, 0.0));
        assert!(
            translated_rect(
                nana_ui_scene::SceneRect::default(),
                nana_ui_scene::AffineTransform::from_matrix([0.0, 1.0, -1.0, 0.0, 0.0, 0.0])
            )
            .is_none()
        );
    }
}

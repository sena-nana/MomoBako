//! Advanced host integration: this executable owns the sole event loop and GPU.
mod window_lifecycle;
use nana_ui::{GpuContext, HostedGpuShared, RuntimeApplication, platform_host::EmbeddedRuntime};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::WindowId,
};
struct Host {
    runtime: Option<EmbeddedRuntime<RuntimeApplication<window_lifecycle::App>>>,
    proxy: EventLoopProxy,
}
impl ApplicationHandler for Host {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.runtime.is_some() {
            return;
        }
        let graphics = create_host_gpu();
        let mut runtime = EmbeddedRuntime::new(
            event_loop,
            self.proxy.clone(),
            graphics.clone(),
            window_lifecycle::descriptor("Embedded lifecycle"),
        )
        .unwrap();
        if std::env::args().any(|arg| arg == "--probe-device-loss") {
            // The host owns loss detection and reports it before forwarding events.
            // Exercise replacement with a live Surface before its first present.
            let old_generation = graphics.gpu().generation();
            graphics.gpu().wgpu().device().destroy();
            runtime.notify_device_lost();
            assert!(runtime.needs_gpu_replacement());
            let replacement = create_host_gpu();
            assert_ne!(replacement.gpu().generation(), old_generation);
            runtime.replace_gpu(replacement).unwrap();
            assert!(!runtime.needs_gpu_replacement());
        }
        if std::env::args().any(|arg| arg == "--probe-host-stop") {
            drop(runtime);
            event_loop.exit();
            return;
        }
        // The host thread creates windows through the runtime it already drives.
        let host_window = runtime
            .create_window(event_loop, window_lifecycle::descriptor("Host created"))
            .unwrap();
        assert_ne!(host_window.id(), nana_ui_platform::WindowId::PRIMARY);
        // Submitted immediately; completion is observed through the lifecycle events.
        drop(host_window.close());
        self.runtime = Some(runtime);
    }
    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.window_event(event_loop, id, event);
        }
    }
    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.wake(event_loop);
        }
    }
    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        if let Some(runtime) = self.runtime.as_mut() {
            let deadline = runtime.about_to_wait(event_loop);
            event_loop.set_control_flow(deadline.map_or(
                winit::event_loop::ControlFlow::Wait,
                winit::event_loop::ControlFlow::WaitUntil,
            ));
            if runtime.is_empty() {
                event_loop.exit();
            }
        }
    }
}
fn main() {
    let event_loop = EventLoop::new().unwrap();
    let proxy = event_loop.create_proxy();
    event_loop
        .run_app(Host {
            runtime: None,
            proxy,
        })
        .unwrap();
    window_lifecycle::verify();
}

fn create_host_gpu() -> HostedGpuShared {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();
        HostedGpuShared::from_device(instance, GpuContext::from_wgpu(adapter, device, queue))
    })
}

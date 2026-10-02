#[path = "hosted_gpu/panel.rs"]
mod panel;
#[path = "hosted_gpu/performance.rs"]
mod performance;
#[path = "hosted_gpu/runner.rs"]
mod runner;
#[path = "hosted_gpu/scene.rs"]
mod scene;

fn main() -> Result<(), nana_ui::HostedRunError> {
    runner::run(std::time::Instant::now())
}

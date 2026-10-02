mod window_lifecycle;
fn main() {
    nana_ui::run_runtime::<nana_ui::RuntimeApplication<window_lifecycle::App>>(
        window_lifecycle::descriptor("Window lifecycle"),
    )
    .unwrap();
    window_lifecycle::verify();
}

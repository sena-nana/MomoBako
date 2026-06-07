fn main() {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("--service-mode"), Some(addr)) => {
            if let Err(error) = tauri_template_lib::service_process::run_service_process(&addr) {
                eprintln!("[service-process] {error}");
                std::process::exit(1);
            }
        }
        _ => {
            tauri_template_lib::run();
        }
    }
}

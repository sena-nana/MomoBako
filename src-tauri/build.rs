fn main() {
    if let Ok(target) = std::env::var("TARGET") {
        println!("cargo:rustc-env=MOMO_TARGET_TRIPLE={target}");
    }
    tauri_build::build()
}

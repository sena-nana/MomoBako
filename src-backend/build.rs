fn main() {
    // 与 Tauri 迁移期构建保持同一插件目标约束，避免服务库在独立宿主中
    // 依赖未定义的编译期环境变量。
    println!("cargo:rustc-env=MOMO_TARGET_TRIPLE={}", std::env::var("TARGET").unwrap_or_default());
}

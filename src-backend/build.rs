fn main() {
    // 插件包的目标三元组要和宿主一致（`plugin.rs` 校验包信封），这里把编译目标交给代码，
    // 不依赖宿主另设编译期环境变量。
    println!("cargo:rustc-env=MOMO_TARGET_TRIPLE={}", std::env::var("TARGET").unwrap_or_default());
}

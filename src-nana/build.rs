//! momobako-nana 的构建脚本：应用身份和 Windows 可执行文件资源。
//!
//! 应用身份只在这里写一份：经 `cargo:rustc-env` 交给代码里的 `application_identity!`（`src/app_paths.rs`），
//! 同时写进 Windows 可执行文件的版本资源。`nana-package.toml` 的 `[application]` 和 `[executable]` 要和这里一致，
//! `nana-packager` 打包和校验时拿二进制里的身份标记比对。版本号取 `Cargo.toml` 的 `version`。
//! 图标是 `icons/icon.ico`，资源编号 1，资源管理器、快捷方式和「属性」里都用它。

use std::env;
use std::fs;
use std::path::PathBuf;

/// 应用 id，也是用户数据目录的名字，发布以后不要改。
const APP_ID: &str = "com.momobako.desktop";
/// 产品名，也是发布包根目录和窗口标题。
const APP_NAME: &str = "MomoBako";
/// 发行者，和 LICENSE 的版权人一致。
const APP_VENDOR: &str = "Sena";
/// 发布包里的可执行文件名。
const EXECUTABLE: &str = "MomoBako.exe";
/// 版权声明，和 LICENSE 一致。
const COPYRIGHT: &str = "Copyright (c) 2026 Sena";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=MOMOBAKO_APP_ID={APP_ID}");
    println!("cargo:rustc-env=MOMOBAKO_APP_NAME={APP_NAME}");
    println!("cargo:rustc-env=MOMOBAKO_APP_VENDOR={APP_VENDOR}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_resources();
    }
}

/// 生成资源脚本（图标和版本信息）并编进可执行文件。编不进去时构建失败，发布包不能没有图标和版本信息。
fn embed_windows_resources() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let icon = manifest_dir.join("icons").join("icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let numeric = ["MAJOR", "MINOR", "PATCH"]
        .map(|part| {
            let value = env::var(format!("CARGO_PKG_VERSION_{part}")).expect("版本号");
            value.parse::<u16>().unwrap_or_else(|_| panic!("版本号 {version} 的 {part} 段放不进版本资源"))
        });
    let file_version = format!("{},{},{},0", numeric[0], numeric[1], numeric[2]);
    let icon_literal = icon.display().to_string().replace('\\', "/");
    let script = format!(
        r#"1 ICON "{icon_literal}"

1 VERSIONINFO
FILEVERSION {file_version}
PRODUCTVERSION {file_version}
FILEFLAGSMASK 0x3F
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "080404B0"
        BEGIN
            VALUE "CompanyName", "{APP_VENDOR}"
            VALUE "FileDescription", "{APP_NAME}"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "{APP_NAME}"
            VALUE "LegalCopyright", "{COPYRIGHT}"
            VALUE "OriginalFilename", "{EXECUTABLE}"
            VALUE "ProductName", "{APP_NAME}"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x804, 1200
    END
END
"#
    );
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let script_path = out_dir.join("momobako.rc");
    fs::write(&script_path, script).expect("写不了资源脚本");
    embed_resource::compile(&script_path, embed_resource::NONE)
        .manifest_required()
        .unwrap_or_else(|error| panic!("图标和版本信息编不进可执行文件：{error}"));
}

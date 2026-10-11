//! 应用身份和三种布局下的读写位置：安装版和便携版不往安装目录写，便携版全落在 `<根>/data`，开发时照旧。

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use nana_ui_platform::{ApplicationPaths, PathEnvironment, PathPlatform, RuntimeLayout};

use super::{identity, HostLocations, LegacyInputs, APP_ID};

const ROAMING: &str = "C:/Users/u/AppData/Roaming";
const LOCAL: &str = "C:/Users/u/AppData/Local";

/// Windows 上某个可执行文件位置解析出的应用路径，环境变量是假的。
fn windows_paths(executable: &str, portable: bool) -> ApplicationPaths {
    let vars: BTreeMap<String, OsString> = [("APPDATA", ROAMING), ("LOCALAPPDATA", LOCAL)]
        .into_iter()
        .map(|(name, value)| (name.to_string(), OsString::from(value)))
        .collect();
    let environment = PathEnvironment {
        platform: PathPlatform::Windows,
        executable: PathBuf::from(executable),
        vars,
        android_files_dir: None,
        android_cache_dir: None,
        portable_marker: portable,
    };
    ApplicationPaths::resolve_with(&identity(), &environment).expect("应用路径")
}

fn legacy() -> LegacyInputs {
    LegacyInputs {
        current_dir: PathBuf::from("D:/work"),
        local_app_data: Some(PathBuf::from(LOCAL)),
        app_data: Some(PathBuf::from(ROAMING)),
        dev_plugin_packages: PathBuf::from("D:/repo/External/Plugins/.packages"),
    }
}

/// 所有可写位置。
fn writable(locations: &HostLocations) -> Vec<PathBuf> {
    [
        Some(locations.service_root.clone()),
        Some(locations.settings_dir.clone()),
        locations.window_state_dir.clone(),
        locations.tool_downloads.clone(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[test]
fn identity_comes_from_the_build_script_and_the_crate_version() {
    let identity = identity();
    assert_eq!(identity.id, APP_ID);
    assert_eq!(identity.id, "com.momobako.desktop");
    assert_eq!(identity.name, "MomoBako");
    assert_eq!(identity.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(identity.vendor.as_deref(), Some("Sena"));
}

#[test]
fn the_binary_carries_the_identity_marker_the_packager_checks() {
    let _ = identity();
    let binary = std::fs::read(std::env::current_exe().expect("测试程序路径")).expect("读测试程序");
    let marker = format!(
        "NANA-IDENTITY-V1\0com.momobako.desktop\0MomoBako\0{}\0Sena\0END\0",
        env!("CARGO_PKG_VERSION")
    );
    assert!(binary.windows(marker.len()).any(|window| window == marker.as_bytes()));
}

#[test]
fn installed_layout_writes_only_to_user_directories() {
    let install_root = Path::new("C:/Program Files/MomoBako");
    let paths = windows_paths("C:/Program Files/MomoBako/MomoBako.exe", false);
    let locations = HostLocations::resolve(Some(&paths), &legacy());

    assert_eq!(locations.layout, Some(RuntimeLayout::Installed));
    assert!(locations.is_packaged());
    let data = Path::new(ROAMING).join(APP_ID);
    assert_eq!(locations.service_root, data.join("service-data"));
    assert_eq!(locations.settings_dir, data.join("config"));
    assert_eq!(locations.window_state_dir, Some(data));
    assert_eq!(locations.bundled_plugins, Some(install_root.join("runtime").join("plugins")));
    assert_eq!(locations.bundled_tools, Some(install_root.join("runtime").join("tools")));
    assert_eq!(locations.tool_downloads, Some(Path::new(LOCAL).join(APP_ID).join("Cache").join("tools")));
    for path in writable(&locations) {
        assert!(!path.starts_with(install_root), "{} 在安装目录里", path.display());
    }
}

#[test]
fn portable_layout_keeps_everything_under_root_data() {
    let root = Path::new("D:/Apps/MomoBako");
    let paths = windows_paths("D:/Apps/MomoBako/MomoBako.exe", true);
    let locations = HostLocations::resolve(Some(&paths), &legacy());

    assert_eq!(locations.layout, Some(RuntimeLayout::Portable));
    assert!(locations.is_packaged());
    let data = root.join("data");
    assert_eq!(locations.service_root, data.join("service-data"));
    assert_eq!(locations.settings_dir, data.join("config"));
    assert_eq!(locations.window_state_dir, Some(data.clone()));
    assert_eq!(locations.tool_downloads, Some(data.join("cache").join("tools")));
    assert_eq!(locations.bundled_plugins, Some(root.join("runtime").join("plugins")));
    for path in writable(&locations) {
        assert!(path.starts_with(&data), "{} 不在便携版的 data 里", path.display());
    }
}

#[test]
fn development_keeps_the_old_locations_and_reads_the_plugin_packages() {
    let paths = windows_paths("D:/repo/target/debug/momobako-nana.exe", false);
    let locations = HostLocations::resolve(Some(&paths), &legacy());

    assert_eq!(locations.layout, Some(RuntimeLayout::Development));
    assert!(!locations.is_packaged());
    assert_eq!(locations.service_root, Path::new("D:/work").join(".service-data"));
    assert_eq!(locations.settings_dir, Path::new(LOCAL).join("MomoBako"));
    assert_eq!(locations.window_state_dir, Some(Path::new(ROAMING).join(APP_ID)));
    assert_eq!(locations.bundled_plugins, Some(PathBuf::from("D:/repo/External/Plugins/.packages")));
    assert_eq!(locations.bundled_tools, None);
    assert_eq!(locations.tool_downloads, Some(Path::new(LOCAL).join(APP_ID).join("Cache").join("tools")));
}

#[test]
fn unresolved_paths_fall_back_without_bundled_plugins() {
    let mut inputs = legacy();
    let locations = HostLocations::resolve(None, &inputs);
    assert_eq!(locations.layout, None);
    assert_eq!(locations.service_root, Path::new("D:/work").join(".service-data"));
    assert_eq!(locations.settings_dir, Path::new(LOCAL).join("MomoBako"));
    assert_eq!(locations.window_state_dir, Some(Path::new(ROAMING).join(APP_ID)));
    assert_eq!((locations.bundled_plugins, locations.bundled_tools, locations.tool_downloads), (None, None, None));

    inputs.local_app_data = None;
    assert_eq!(HostLocations::resolve(None, &inputs).settings_dir, Path::new(ROAMING).join("MomoBako"));
    inputs.app_data = None;
    let bare = HostLocations::resolve(None, &inputs);
    assert_eq!(bare.settings_dir, Path::new("D:/work").join("MomoBako"));
    assert_eq!(bare.window_state_dir, None);
}

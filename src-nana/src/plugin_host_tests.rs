//! 这次同步哪些随附插件：发布包照 manifest，开发时照插件打包输出目录。

use std::fs;
use std::path::PathBuf;

use nana_ui::nana_package::manifest::PackageManifest;

use super::bundled_packages;

/// 测试独占的随附插件目录，测试结束时删掉。
struct PluginDir {
    path: PathBuf,
}

impl PluginDir {
    fn new(name: &str, files: &[&str]) -> Self {
        let path = std::env::temp_dir().join(format!("momobako-plugin-host-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("随附插件目录");
        for file in files {
            fs::write(path.join(file), b"package").expect("写随附插件");
        }
        Self { path }
    }
}

impl Drop for PluginDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// 只列了两个插件的发布包 manifest。
fn manifest() -> PackageManifest {
    PackageManifest::from_json(
        br#"{"schema":"nana.package-manifest","schema_version":1,
        "application":{"id":"com.momobako.desktop","name":"MomoBako","version":"0.1.0","vendor":"Sena","build_id":null},
        "target":{"triple":"x86_64-pc-windows-msvc","platform":"windows"},"nana":{"version":"0"},
        "build":{"profile":"dist","debug_assertions_suspected":false},
        "static_app_plan":{"status":"unavailable","reason":"r"},
        "distribution":{"backend":"portable","self_update":false},
        "layout":{"executable":"MomoBako.exe","runtime_bin":"runtime/bin","runtime_resources":"runtime/resources",
          "runtime_plugins":"runtime/plugins","runtime_tools":"runtime/tools",
          "runtime_manifest":"runtime/manifest","root_exceptions":[]},
        "resource_packs":[],
        "plugins":[
          {"name":"momobako.local-filesystem","file":"local-filesystem-1.0.0-x86_64-pc-windows-msvc.momoplug","abi":"momoplug-v2","version":"1.0.0"},
          {"name":"momobako.library.audio","file":"library-audio-0.1.0-any.momoplug","abi":"momoplug-v2","version":"0.1.0"}
        ],
        "runtime_files":[],
        "signing":{"publisher":{"state":"not-configured"},"publisher_key":null,"platform":[]}}"#,
    )
    .expect("manifest")
}

#[test]
fn packages_follow_the_manifest_and_ignore_leftovers() {
    let dir = PluginDir::new(
        "manifest",
        &[
            "local-filesystem-1.0.0-x86_64-pc-windows-msvc.momoplug",
            "library-audio-0.1.0-any.momoplug",
            "local-filesystem-0.9.0-x86_64-pc-windows-msvc.momoplug",
        ],
    );

    let packages = bundled_packages(&dir.path, true, Some(&manifest())).expect("清单").expect("要同步");

    assert_eq!(
        packages,
        vec![
            dir.path.join("local-filesystem-1.0.0-x86_64-pc-windows-msvc.momoplug"),
            dir.path.join("library-audio-0.1.0-any.momoplug"),
        ],
        "旧版本留下的包不算随附插件"
    );
}

#[test]
fn a_package_without_manifest_lists_its_plugin_dir() {
    let dir = PluginDir::new("no-manifest", &["b-0.1.0-any.momoplug", "a-0.1.0-any.momoplug"]);

    let packages = bundled_packages(&dir.path, true, None).expect("清单").expect("要同步");

    assert_eq!(packages, vec![dir.path.join("a-0.1.0-any.momoplug"), dir.path.join("b-0.1.0-any.momoplug")]);
}

#[test]
fn development_reads_the_packages_dir_and_skips_when_it_is_missing() {
    let dir = PluginDir::new("dev", &["a-0.1.0-any.momoplug"]);
    assert_eq!(
        bundled_packages(&dir.path, false, Some(&manifest())).expect("清单"),
        Some(vec![dir.path.join("a-0.1.0-any.momoplug")]),
        "开发时不看 manifest"
    );

    let missing = dir.path.join("not-packaged-yet");
    assert_eq!(bundled_packages(&missing, false, None).expect("清单"), None, "还没打包插件时不同步");
}

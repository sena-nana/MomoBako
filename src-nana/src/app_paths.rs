//! 应用身份和本机读写位置。
//!
//! 身份用 NanaUI 的 `application_identity!` 写进二进制（`NANA-IDENTITY-V1` 标记），字段值来自 `build.rs`；
//! `nana-packager` 打包和校验时拿它和 `src-nana/nana-package.toml` 的 `[application]` 比对。
//!
//! 位置照 NanaUI 的 `ApplicationPaths`，`<data>`、`<config>`、`<cache>` 是它解析出的目录：
//!
//! | 内容 | 安装版 | 便携版 | 开发（从 `target/` 运行） |
//! | --- | --- | --- | --- |
//! | 领域服务目录 | `<data>/service-data` | `<根>/data/service-data` | 当前目录下的 `.service-data` |
//! | 应用设置和界面偏好 | `<config>` | `<根>/data/config` | `%LOCALAPPDATA%\MomoBako` |
//! | 主窗口几何 | `<data>` | `<根>/data` | `<data>`（就是 `%APPDATA%\com.momobako.desktop`） |
//! | 随附插件 | `runtime/plugins` | `runtime/plugins` | `External/Plugins/.packages`（插件打包输出） |
//! | 随包外部程序 | `runtime/tools` | `runtime/tools` | 无 |
//! | 下载的外部程序 | `<cache>/tools` | `<根>/data/cache/tools` | `<cache>/tools` |
//!
//! 安装版的 `<data>` 是 `%APPDATA%\com.momobako.desktop`，`<config>` 在它下面的 `config`，`<cache>` 是
//! `%LOCALAPPDATA%\com.momobako.desktop\Cache`；便携版都在应用根目录的 `data` 下。两种都不往安装目录写。开发时照旧，
//! 不打扰已有的开发数据。应用路径解析不出来时（缺环境变量）也按开发时的位置走，并且不同步随附插件。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use nana_ui_platform::{ApplicationIdentity, ApplicationPaths, RuntimeLayout};

/// 应用 id，也是用户数据目录的名字。
pub const APP_ID: &str = env!("MOMOBAKO_APP_ID");

/// 开发时设置文件所在目录的名字，在 `%LOCALAPPDATA%` 下。
const LEGACY_SETTINGS_DIR: &str = "MomoBako";

/// 二进制里的应用身份：id、名称、版本和发行者。
pub fn identity() -> ApplicationIdentity {
    nana_ui_platform::application_identity!(
        id: env!("MOMOBAKO_APP_ID"),
        name: env!("MOMOBAKO_APP_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        vendor: env!("MOMOBAKO_APP_VENDOR"),
    )
}

/// MomoBako 在这台机器上读写的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLocations {
    /// NanaUI 解析出的布局；解析不出来时为空。
    pub layout: Option<RuntimeLayout>,
    /// 领域服务目录：资源库注册表、插件、插件数据、外部 API 连接文件和系统日志。
    pub service_root: PathBuf,
    /// 应用设置和界面偏好（`settings.json`、`sidebar.json` 等）所在目录。
    pub settings_dir: PathBuf,
    /// 主窗口几何文件所在目录；没有应用数据目录时为空。
    pub window_state_dir: Option<PathBuf>,
    /// 随附插件（`.momoplug`）所在目录；为空时不同步随附插件。
    pub bundled_plugins: Option<PathBuf>,
    /// 随包带的外部程序目录。
    pub bundled_tools: Option<PathBuf>,
    /// 下载外部程序（ffmpeg）用的可写目录。
    pub tool_downloads: Option<PathBuf>,
}

/// 开发时的位置用到的外部输入。测试里换成假的，不读真环境。
#[derive(Debug, Clone)]
pub struct LegacyInputs {
    pub current_dir: PathBuf,
    pub local_app_data: Option<PathBuf>,
    pub app_data: Option<PathBuf>,
    /// 插件打包输出 `External/Plugins/.packages`。
    pub dev_plugin_packages: PathBuf,
}

impl LegacyInputs {
    /// 从当前进程读：工作目录、`%LOCALAPPDATA%`、`%APPDATA%` 和编译时的仓库位置。
    pub fn from_env() -> Self {
        Self {
            current_dir: std::env::current_dir().unwrap_or_else(|error| {
                eprintln!("Nana 读不到当前目录，服务目录落在相对路径：{error}");
                PathBuf::from(".")
            }),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            app_data: std::env::var_os("APPDATA").map(PathBuf::from),
            dev_plugin_packages: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("External")
                .join("Plugins")
                .join(".packages"),
        }
    }
}

impl HostLocations {
    /// 按布局给出位置，见模块文档的表。
    pub fn resolve(paths: Option<&ApplicationPaths>, legacy: &LegacyInputs) -> Self {
        match paths {
            Some(paths) if paths.layout() != RuntimeLayout::Development => Self::packaged(paths),
            Some(paths) => Self::development(paths, legacy),
            None => Self::unresolved(legacy),
        }
    }

    /// 安装版和便携版：都从应用路径取，可写的全在用户目录或便携版的 `data` 下。
    fn packaged(paths: &ApplicationPaths) -> Self {
        Self {
            layout: Some(paths.layout()),
            service_root: paths.data().join("service-data"),
            settings_dir: paths.config().to_path_buf(),
            window_state_dir: Some(paths.data().to_path_buf()),
            bundled_plugins: Some(paths.runtime_plugins().to_path_buf()),
            bundled_tools: Some(paths.runtime_tools().to_path_buf()),
            tool_downloads: Some(paths.cache().join("tools")),
        }
    }

    /// 开发：服务目录和设置照旧，插件读打包输出，下载的外部程序放进应用缓存目录。
    fn development(paths: &ApplicationPaths, legacy: &LegacyInputs) -> Self {
        Self {
            layout: Some(RuntimeLayout::Development),
            window_state_dir: Some(paths.data().to_path_buf()),
            bundled_plugins: Some(legacy.dev_plugin_packages.clone()),
            tool_downloads: Some(paths.cache().join("tools")),
            ..Self::unresolved(legacy)
        }
    }

    /// 应用路径解析不出来：服务目录和设置照开发时的位置，不同步随附插件，外部程序照 ffmpeg-sidecar 原样处理。
    fn unresolved(legacy: &LegacyInputs) -> Self {
        let settings_root = legacy
            .local_app_data
            .clone()
            .or_else(|| legacy.app_data.clone())
            .unwrap_or_else(|| legacy.current_dir.clone());
        Self {
            layout: None,
            service_root: legacy.current_dir.join(".service-data"),
            settings_dir: settings_root.join(LEGACY_SETTINGS_DIR),
            window_state_dir: legacy.app_data.as_ref().map(|dir| dir.join(APP_ID)),
            bundled_plugins: None,
            bundled_tools: None,
            tool_downloads: None,
        }
    }

    /// 是安装版或便携版的发布包。
    pub fn is_packaged(&self) -> bool {
        matches!(self.layout, Some(RuntimeLayout::Installed | RuntimeLayout::Portable))
    }
}

static LOCATIONS: OnceLock<HostLocations> = OnceLock::new();

/// 启动时设置一次；之后再设不生效。
pub fn install(locations: HostLocations) {
    if LOCATIONS.set(locations).is_err() {
        eprintln!("Nana 应用位置已经设置过，这次的不生效");
    }
}

/// 当前进程的位置。没有设置时（单元测试）按应用路径解析不出来的情况算，和以前的开发位置一样。
pub fn current() -> &'static HostLocations {
    LOCATIONS.get_or_init(|| HostLocations::resolve(None, &LegacyInputs::from_env()))
}

#[cfg(test)]
#[path = "app_paths_tests.rs"]
mod tests;

//! Process-level application setup: identity, [`ApplicationPaths`], and
//! diagnostics (Issues #226 / #227).
//!
//! ```ignore
//! NanaApplication::builder(ApplicationIdentity::new("dev.nana.live", "NanaLive", "1.0"))
//!     .diagnostics(DiagnosticsConfig::default())
//!     .run::<RuntimeApplication<App>>(WindowDescriptor::new("NanaLive"))
//! ```
//!
//! Diagnostics are opt-in: without `.diagnostics(..)` nothing is recorded and
//! every framework call site costs one relaxed atomic load.

use std::sync::OnceLock;

use nana_diagnostics::{
    Diagnostics, DiagnosticsConfig, DiagnosticsGuard, DiagnosticsPaths, SessionMetadata,
};
use nana_ui_platform::{ApplicationIdentity, ApplicationPaths};

/// The paths the first started builder resolved; first one wins.
static PATHS: OnceLock<ApplicationPaths> = OnceLock::new();

/// Entry point for process-level setup.
pub struct NanaApplication;

impl NanaApplication {
    pub fn builder(identity: ApplicationIdentity) -> NanaApplicationBuilder {
        NanaApplicationBuilder {
            identity,
            paths: None,
            diagnostics: DiagnosticsConfig::disabled(),
            #[cfg(feature = "packaged-resources")]
            resource_packs: None,
            #[cfg(feature = "hosted")]
            splash: None,
        }
    }

    /// The paths published by the first builder started in this process.
    pub fn paths() -> Option<&'static ApplicationPaths> {
        PATHS.get()
    }

    /// The process's diagnostics runtime, if one is installed.
    pub fn diagnostics() -> Option<Diagnostics> {
        nana_diagnostics::global()
    }

    /// The package manifest this application was started with (packaged
    /// builds that enabled [`NanaApplicationBuilder::resource_packs`]).
    #[cfg(feature = "packaged-resources")]
    pub fn package_manifest() -> Option<&'static nana_package::manifest::PackageManifest> {
        crate::packaged_resources::package_manifest()
    }
}

/// Base for relative resource URLs when no host set one: the runtime
/// resources location of an installed or portable package. `None` in
/// development and for embedded hosts without application paths, which
/// keep resolving against the working directory.
#[allow(dead_code)] // Read by the GPU image loader only.
pub(crate) fn packaged_resources_base() -> Option<std::path::PathBuf> {
    PATHS
        .get()
        .filter(|paths| paths.layout() != nana_ui_platform::RuntimeLayout::Development)
        .map(|paths| paths.runtime_resources().to_path_buf())
}

#[must_use]
pub struct NanaApplicationBuilder {
    identity: ApplicationIdentity,
    paths: Option<ApplicationPaths>,
    diagnostics: DiagnosticsConfig,
    #[cfg(feature = "packaged-resources")]
    resource_packs: Option<crate::packaged_resources::ResourcePackOptions>,
    #[cfg(feature = "hosted")]
    splash: Option<crate::SplashSpec>,
}

/// Keeps process-level services alive. Dropping it shuts diagnostics down
/// with a final durable flush, so bind it for the life of the app.
#[must_use = "dropping the session shuts diagnostics down immediately"]
pub struct ApplicationSession {
    paths: Option<&'static ApplicationPaths>,
    diagnostics: Option<DiagnosticsGuard>,
}

impl ApplicationSession {
    /// `None` when the platform directories could not be resolved (e.g. no
    /// `HOME` in a container); the application still runs.
    pub fn paths(&self) -> Option<&'static ApplicationPaths> {
        self.paths
    }

    /// `None` when diagnostics are disabled or another instance was already
    /// installed in this process.
    pub fn diagnostics(&self) -> Option<&Diagnostics> {
        self.diagnostics.as_ref().map(DiagnosticsGuard::diagnostics)
    }
}

impl NanaApplicationBuilder {
    /// Use these paths instead of resolving them for the running process.
    /// Paths are published once per process: if an earlier builder already
    /// published some, those stay in effect.
    pub fn paths(mut self, paths: ApplicationPaths) -> Self {
        self.paths = Some(paths);
        self
    }

    pub fn diagnostics(mut self, config: DiagnosticsConfig) -> Self {
        self.diagnostics = config;
        self
    }

    /// Show `splash` on the primary window before the GPU device and the
    /// program exist; see [`crate::startup`].
    #[cfg(feature = "hosted")]
    pub fn early_splash(mut self, splash: crate::SplashSpec) -> Self {
        self.splash = Some(splash);
        self
    }

    /// Mount the package's resource packs behind `nana://res/` at
    /// [`Self::start`]: the package manifest is read (and its signature
    /// checked as `options` demands) and each pack opens on its first
    /// lookup. Starting with `NANA_PACKAGE_VALIDATE=1` turns startup into
    /// the packager's self-check (`docs/reference/packaging.md`).
    #[cfg(feature = "packaged-resources")]
    pub fn resource_packs(
        mut self,
        options: crate::packaged_resources::ResourcePackOptions,
    ) -> Self {
        self.resource_packs = Some(options);
        self
    }

    /// Resolve and publish the paths and start diagnostics. For hosts that
    /// run their own loop (Vue, embedded); [`Self::run`] calls this.
    ///
    /// Never fails the application: when the platform directories cannot be
    /// resolved it logs to stderr, the session has no paths, and diagnostics
    /// stay in memory.
    pub fn start(self) -> ApplicationSession {
        #[cfg(feature = "packaged-resources")]
        let splash_logo = self.packaged_splash_logo();
        let resolved = match self.paths {
            Some(paths) => Ok(paths),
            None => ApplicationPaths::resolve(&self.identity),
        };
        let paths = match resolved {
            Ok(paths) => Some(PATHS.get_or_init(|| paths)),
            Err(error) => {
                eprintln!(
                    "NanaUI: cannot resolve application paths: {error}; running without them"
                );
                None
            }
        };
        let diagnostics = if self.diagnostics.enabled {
            install_diagnostics(&self.identity, self.diagnostics, paths)
        } else {
            None
        };
        #[cfg(feature = "packaged-resources")]
        if let Some(code) = crate::packaged_resources::start(
            &self.identity,
            paths,
            self.resource_packs.as_ref(),
            splash_logo,
        ) {
            // Self-check: flush diagnostics (faults recorded while mounting)
            // and exit before any window, device or program exists.
            drop(diagnostics);
            std::process::exit(code);
        }
        ApplicationSession { paths, diagnostics }
    }

    /// The `nana://res/` URL of a packaged Early Splash logo, for the
    /// package self-check.
    #[cfg(feature = "packaged-resources")]
    fn packaged_splash_logo(&self) -> Option<&'static str> {
        #[cfg(feature = "hosted")]
        if let Some(crate::SplashLogoSource::Packaged(url)) =
            self.splash.map(|splash| splash.logo.source())
        {
            return Some(url);
        }
        None
    }

    /// Start process services, run the Scene host, then shut down cleanly.
    #[cfg(feature = "hosted")]
    pub fn run<Program: crate::RuntimeProgram>(
        self,
        settings: crate::WindowDescriptor,
    ) -> Result<(), crate::HostedRunError> {
        let startup = crate::StartupOptions {
            splash: self.splash,
        };
        let session = self.start();
        let result = crate::with_startup(startup, || crate::run_runtime::<Program>(settings));
        record_run_result(&result);
        drop(session);
        result
    }

    /// [`Self::run`] with a host-injected persistent store.
    #[cfg(feature = "hosted")]
    pub fn run_with_store<Program: crate::RuntimeProgram>(
        self,
        settings: crate::WindowDescriptor,
        store: crate::SharedStore,
    ) -> Result<(), crate::HostedRunError> {
        let startup = crate::StartupOptions {
            splash: self.splash,
        };
        let session = self.start();
        let result = crate::with_startup(startup, || {
            crate::run_runtime_with_store::<Program>(settings, store)
        });
        record_run_result(&result);
        drop(session);
        result
    }
}

fn install_diagnostics(
    identity: &ApplicationIdentity,
    config: DiagnosticsConfig,
    paths: Option<&ApplicationPaths>,
) -> Option<DiagnosticsGuard> {
    let layout = paths.map_or("unresolved", |p| p.layout().as_str());
    let mut meta = SessionMetadata::new(&identity.id, &identity.name, &identity.version)
        .framework_version(env!("CARGO_PKG_VERSION"))
        .extra("layout", layout);
    if let Some(build_id) = &identity.build_id {
        meta = meta.build_id(build_id);
    }
    if let Some(vendor) = &identity.vendor {
        meta = meta.extra("vendor", vendor);
    }
    let files = paths.map_or_else(DiagnosticsPaths::in_memory, |p| {
        DiagnosticsPaths::new(p.logs(), p.crash())
    });
    // A second builder in one process keeps the first runtime.
    let guard = nana_diagnostics::install(config, meta, files).ok();
    // `start` runs on the thread that will drive the event loop: register
    // it now so its first frame does not pay for ring allocation.
    nana_diagnostics::register_thread();
    guard
}

#[cfg(feature = "hosted")]
fn record_run_result(result: &Result<(), crate::HostedRunError>) {
    if let Err(error) = result {
        nana_diagnostics::fault!(
            nana_diagnostics::framework::host::RUN_FAILED;
            "{error}"
        );
    }
}

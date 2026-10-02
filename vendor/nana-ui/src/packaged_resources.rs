//! Packaged resources at startup (Issue #226): read the package manifest,
//! mount its `.nrpack` resource packs behind `nana://res/`, and answer the
//! packager's `NANA_PACKAGE_VALIDATE` self-check.
//!
//! Packs open lazily on their first lookup, so startup reads the manifest
//! and nothing else; a lookup routes to one pack by its path prefixes and
//! touches only that entry's blocks. Everything a pack returns has passed
//! its hash and authentication checks. Nothing here runs per frame: reads
//! happen on cache misses of the image, font and stylesheet loaders.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use nana_diagnostics::framework::{package as package_diag, resource as resource_diag};
use nana_diagnostics::{event, fault, metric};
use nana_package::manifest::PackageManifest;
use nana_package::{
    ExpectedPack, KeyProvider, ManifestError, NoKeys, PackError, PackReader, ReadStats,
    SignatureState, TrustPolicy,
};
use nana_ui_core::{PackagedReadError, PackagedResourceSource};
use nana_ui_platform::{ApplicationIdentity, ApplicationPaths, RuntimeLayout};

pub use nana_package::SELF_CHECK_ENV;

/// How to open the package's resource packs.
#[derive(Clone)]
pub struct ResourcePackOptions {
    keys: Arc<dyn KeyProvider>,
    trust: TrustPolicy,
    loose_root: Option<PathBuf>,
}

impl ResourcePackOptions {
    /// Unencrypted packs only, no publisher signature required.
    pub fn new() -> Self {
        Self {
            keys: Arc::new(NoKeys),
            trust: TrustPolicy::AllowUnsigned,
            loose_root: None,
        }
    }

    /// Where content keys come from. How the application obtains them
    /// (embedded, a license file, a platform API) is its own policy.
    pub fn keys(mut self, keys: impl KeyProvider + 'static) -> Self {
        self.keys = Arc::new(keys);
        self
    }

    /// Require every pack and the manifest to be signed by this publisher.
    pub fn trust(mut self, trust: TrustPolicy) -> Self {
        self.trust = trust;
        self
    }

    /// In the development layout (running from `target/`), serve logical
    /// paths from this directory, the packager's `resources.root`.
    pub fn loose_root(mut self, dir: impl Into<PathBuf>) -> Self {
        self.loose_root = Some(dir.into());
        self
    }
}

impl Default for ResourcePackOptions {
    fn default() -> Self {
        Self::new()
    }
}

static MANIFEST: OnceLock<PackageManifest> = OnceLock::new();
/// The mount the manifest describes, kept for the self-check and the Early
/// Splash read — also when another source already holds the `nana-ui-core`
/// hook (which otherwise holds a type-erased handle to this same mount).
static MOUNT: OnceLock<Arc<PackMount>> = OnceLock::new();
/// The installed development source, kept for the Early Splash logo read.
#[cfg_attr(not(feature = "hosted"), allow(dead_code))]
static LOOSE: OnceLock<Arc<LooseSource>> = OnceLock::new();

/// The package manifest the application started with, if it has one.
pub(crate) fn package_manifest() -> Option<&'static PackageManifest> {
    MANIFEST.get()
}

struct LazyPack {
    name: String,
    path: PathBuf,
    prefixes: Vec<String>,
    expected: Option<ExpectedPack>,
    reader: Mutex<Slot>,
    integrity_reported: AtomicBool,
    /// Retryable open failures (missing key, I/O) are reported once per pack.
    open_failure_reported: AtomicBool,
}

/// A pack's open state. Integrity failures are final; a missing key or an
/// I/O error is retried on the next lookup (a license or network key may
/// arrive later).
enum Slot {
    Closed,
    Open(Arc<PackReader>),
    Failed(PackError),
}

struct PackMount {
    packs: Vec<LazyPack>,
    keys: Arc<dyn KeyProvider>,
    trust: TrustPolicy,
}

impl PackMount {
    fn route(&self, path: &str) -> Option<&LazyPack> {
        self.packs
            .iter()
            .flat_map(|pack| pack.prefixes.iter().map(move |prefix| (prefix, pack)))
            .filter(|(prefix, _)| nana_package::manifest::prefix_claims(prefix, path))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, pack)| pack)
    }

    /// The pack's reader, opening it on first use. The key provider runs
    /// outside the lock, so it may take its time (it must not read the pack
    /// it is keying through `nana://res/`, which would recurse). Two threads
    /// racing on the first lookup may both open; one result is kept.
    fn reader(&self, pack: &LazyPack) -> Result<Arc<PackReader>, PackError> {
        match &*pack.reader.lock().unwrap_or_else(|e| e.into_inner()) {
            Slot::Open(reader) => return Ok(reader.clone()),
            Slot::Failed(error) => return Err(error.clone()),
            Slot::Closed => {}
        }
        let started = Instant::now();
        let opened = PackReader::open(
            &pack.path,
            &pack.name,
            self.keys.as_ref(),
            &self.trust,
            pack.expected.as_ref(),
        );
        let mut slot = pack.reader.lock().unwrap_or_else(|e| e.into_inner());
        if let Slot::Open(reader) = &*slot {
            return Ok(reader.clone());
        }
        match opened {
            Ok(reader) => {
                metric!(resource_diag::PACK_MOUNT_NS, started.elapsed());
                metric!(resource_diag::PACK_TOC_BYTES, reader.toc_len());
                event!(
                    resource_diag::PACK_MOUNTED,
                    entries = reader.entry_count() as u64,
                    toc_bytes = reader.toc_len(),
                    encrypted = reader.header().encrypted(),
                    signed = reader.header().signed(),
                );
                let reader = Arc::new(reader);
                *slot = Slot::Open(reader.clone());
                Ok(reader)
            }
            Err(error) => {
                if error.is_integrity_failure() {
                    metric!(resource_diag::INTEGRITY_FAILURES);
                }
                let retry = matches!(error, PackError::KeyUnavailable(_) | PackError::Io(_));
                // A final failure always reports (it happens once: the slot
                // keeps it); retryable ones report the first time only.
                if !retry || !pack.open_failure_reported.swap(true, Ordering::Relaxed) {
                    fault!(
                        resource_diag::PACK_OPEN_FAILED,
                        code = error.code();
                        "resource pack `{}` cannot be opened: {error}", pack.name
                    );
                }
                if !retry {
                    *slot = Slot::Failed(error.clone());
                }
                Err(error)
            }
        }
    }
}

impl PackMount {
    /// Read `path` from `pack`, opening it on first use, with the metrics and
    /// faults every packaged read records.
    fn read_entry(
        &self,
        pack: &LazyPack,
        path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, PackError> {
        let reader = self.reader(pack)?;
        let started = Instant::now();
        let mut stats = ReadStats::default();
        let result = reader.read(path, max_bytes, &mut stats);
        metric!(resource_diag::READS);
        metric!(resource_diag::BYTES_READ, stats.bytes_read);
        match &result {
            Ok(_) => {
                metric!(resource_diag::ENTRY_READ_NS, started.elapsed());
                if stats.auth_ns > 0 {
                    metric!(resource_diag::ENTRY_AUTH_NS, stats.auth_ns);
                }
                if stats.decompress_ns > 0 {
                    metric!(resource_diag::ENTRY_DECOMPRESS_NS, stats.decompress_ns);
                }
            }
            Err(PackError::NotFound) => metric!(resource_diag::MISSES),
            Err(error) if error.is_integrity_failure() => {
                metric!(resource_diag::INTEGRITY_FAILURES);
                if !pack.integrity_reported.swap(true, Ordering::Relaxed) {
                    fault!(
                        resource_diag::INTEGRITY_FAILED,
                        code = error.code();
                        "resource pack `{}`: `{path}` failed verification: {error}", pack.name
                    );
                }
            }
            Err(_) => {}
        }
        result
    }
}

impl PackagedResourceSource for PackMount {
    fn read(&self, path: &str, max_bytes: u64) -> Result<Vec<u8>, PackagedReadError> {
        let Some(pack) = self.route(path) else {
            metric!(resource_diag::MISSES);
            return Err(PackagedReadError::NotFound);
        };
        self.read_entry(pack, path, max_bytes)
            .map_err(|error| map_error(&error))
    }
}

fn map_error(error: &PackError) -> PackagedReadError {
    match error {
        PackError::NotFound | PackError::Missing => PackagedReadError::NotFound,
        PackError::TooLarge { .. } => PackagedReadError::TooLarge,
        PackError::KeyUnavailable(_) => PackagedReadError::KeyUnavailable,
        PackError::Io(_) => PackagedReadError::Io,
        _ => PackagedReadError::Integrity,
    }
}

/// Development: logical paths straight from the resource source directory.
struct LooseSource {
    root: PathBuf,
}

impl LooseSource {
    /// `path` is already a validated logical path: join its segments (no
    /// second URL decoding), then require it to stay inside the root.
    fn locate(&self, path: &str) -> Option<PathBuf> {
        let file = path
            .split('/')
            .fold(self.root.clone(), |dir, part| dir.join(part));
        nana_ui_core::canonicalize_within_jail(&file, &self.root)
    }
}

/// Why a loose read failed; `TooLarge` carries the length seen.
#[cfg_attr(not(feature = "hosted"), allow(dead_code))]
enum LooseReadError {
    NotFound,
    TooLarge(u64),
    Io(String),
}

impl LooseSource {
    /// At most `max_bytes`, however the file changes while it is read.
    fn read_bounded(&self, path: &str, max_bytes: u64) -> Result<Vec<u8>, LooseReadError> {
        use std::io::Read;
        let file = self.locate(path).ok_or(LooseReadError::NotFound)?;
        let file = std::fs::File::open(&file).map_err(|_| LooseReadError::NotFound)?;
        let len = file.metadata().map_err(|_| LooseReadError::NotFound)?.len();
        if len > max_bytes {
            return Err(LooseReadError::TooLarge(len));
        }
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| LooseReadError::Io(error.to_string()))?;
        if bytes.len() as u64 > max_bytes {
            return Err(LooseReadError::TooLarge(bytes.len() as u64));
        }
        Ok(bytes)
    }
}

impl PackagedResourceSource for LooseSource {
    fn read(&self, path: &str, max_bytes: u64) -> Result<Vec<u8>, PackagedReadError> {
        self.read_bounded(path, max_bytes)
            .map_err(|error| match error {
                LooseReadError::NotFound => PackagedReadError::NotFound,
                LooseReadError::TooLarge(_) => PackagedReadError::TooLarge,
                LooseReadError::Io(_) => PackagedReadError::Io,
            })
    }
}

/// Read the Early Splash logo `url` names (Issue #225): one entry of the
/// `early-splash` pack the manifest routes it to, through the same mount,
/// cached reader and diagnostics as every `nana://res/` read. A path in a pack
/// of another class is refused before anything is opened; the reader caps the
/// pack file after its header and the entry before its data. Early-splash
/// packs are never encrypted, so no key provider runs. In the development
/// layout without a manifest, the one file under the loose root, where there
/// is no class to check.
#[cfg(feature = "hosted")]
pub(crate) fn read_splash_logo(url: &str) -> Result<Vec<u8>, crate::startup::SplashFailure> {
    use crate::startup::{
        MAX_LOGO_ENCODED_BYTES, SplashFailure, SplashLogoError, SplashPackageError as E,
    };
    use nana_package::EarlySplashError;

    let package = |error| SplashFailure::Package(error);
    let too_large = |bytes: u64| {
        SplashFailure::Logo(SplashLogoError::TooLarge {
            bytes: usize::try_from(bytes).unwrap_or(usize::MAX),
        })
    };
    let path = nana_ui_core::packaged_logical_path(url, None).ok_or(package(E::InvalidUrl))?;
    let max = MAX_LOGO_ENCODED_BYTES as u64;
    let (Some(manifest), Some(mount)) = (MANIFEST.get(), MOUNT.get()) else {
        let loose = LOOSE.get().ok_or(package(E::NotMounted))?;
        return loose.read_bounded(&path, max).map_err(|error| match error {
            LooseReadError::NotFound => package(E::NotFound),
            LooseReadError::TooLarge(len) => too_large(len),
            LooseReadError::Io(reason) => package(E::Io(reason)),
        });
    };
    let pack = manifest.early_splash_pack(&path).map_err(|error| {
        package(match error {
            EarlySplashError::InvalidPath => E::InvalidUrl,
            EarlySplashError::NotFound => E::NotFound,
            EarlySplashError::WrongClass { pack, class } => E::NotEarlySplash {
                pack,
                class: class.as_str(),
            },
        })
    })?;
    let lazy = mount
        .packs
        .iter()
        .find(|lazy| lazy.name == pack.name)
        .ok_or(package(E::NotFound))?;
    mount
        .read_entry(lazy, &path, max)
        .map_err(|error| match error {
            PackError::NotFound => package(E::NotFound),
            PackError::TooLarge { len } => too_large(len),
            error => package(E::Pack {
                pack: lazy.name.clone(),
                code: error.code(),
                reason: error.to_string(),
            }),
        })
}

/// Which `nana://res/` source startup installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Packs,
    Loose,
}

/// What startup found, for the self-check.
struct Mounted {
    manifest: Result<(&'static PackageManifest, SignatureState), ManifestError>,
    identity_matches: Option<bool>,
    source: Option<Source>,
}

fn identity_matches(identity: &ApplicationIdentity, manifest: &PackageManifest) -> bool {
    manifest.application.matches(
        &identity.id,
        &identity.name,
        &identity.version,
        identity.vendor.as_deref(),
    )
}

fn mount(
    identity: &ApplicationIdentity,
    paths: Option<&ApplicationPaths>,
    options: &ResourcePackOptions,
) -> Mounted {
    let Some(paths) = paths else {
        return Mounted {
            manifest: Err(ManifestError::Io("application paths are unresolved".into())),
            identity_matches: None,
            source: None,
        };
    };
    let started = Instant::now();
    let manifest = PackageManifest::read(paths.runtime_manifest(), &options.trust)
        // Kept for the life of the process; the first builder's wins.
        .map(|(manifest, state)| (MANIFEST.get_or_init(|| manifest), state));
    metric!(package_diag::MANIFEST_READ_NS, started.elapsed());
    let mut mounted = Mounted {
        manifest,
        identity_matches: None,
        source: None,
    };
    match &mounted.manifest {
        Ok((manifest, state)) => {
            let bytes =
                std::fs::metadata(paths.runtime_manifest().join(nana_package::MANIFEST_FILE))
                    .map_or(0, |meta| meta.len());
            event!(
                package_diag::MANIFEST_LOADED,
                bytes = bytes,
                packs = manifest.resource_packs.len() as u64,
                signed = *state == SignatureState::Verified,
            );
            let matches = identity_matches(identity, manifest);
            mounted.identity_matches = Some(matches);
            if !matches {
                fault!(
                    package_diag::IDENTITY_MISMATCH;
                    "the package manifest describes {} {}, the binary is {} {}",
                    manifest.application.id, manifest.application.version,
                    identity.id, identity.version
                );
            }
            let resources = paths.runtime_resources();
            let packs = manifest
                .resource_packs
                .iter()
                .map(|pack| LazyPack {
                    name: pack.name.clone(),
                    path: resources.join(&pack.file),
                    prefixes: pack.prefixes.clone(),
                    expected: pack.expected(),
                    reader: Mutex::new(Slot::Closed),
                    integrity_reported: AtomicBool::new(false),
                    open_failure_reported: AtomicBool::new(false),
                })
                .collect();
            let mount = Arc::new(PackMount {
                packs,
                keys: options.keys.clone(),
                trust: options.trust.clone(),
            });
            let _ = MOUNT.set(mount.clone());
            let installed = nana_ui_core::install_packaged_source(mount);
            mounted.source = installed.then_some(Source::Packs);
        }
        Err(ManifestError::Missing) if paths.layout() == RuntimeLayout::Development => {
            if let Some(root) = &options.loose_root {
                let loose = Arc::new(LooseSource { root: root.clone() });
                let installed = nana_ui_core::install_packaged_source(loose.clone());
                if installed {
                    let _ = LOOSE.set(loose);
                }
                mounted.source = installed.then_some(Source::Loose);
            }
        }
        Err(ManifestError::Missing) => event!(package_diag::MANIFEST_MISSING),
        Err(error) => {
            fault!(
                package_diag::MANIFEST_INVALID,
                code = error.code();
                "{error}"
            );
        }
    }
    mounted
}

/// Called from `NanaApplicationBuilder::start`. Returns the process exit
/// code when startup was a self-check (the caller shuts diagnostics down,
/// so faults recorded here reach the log, and exits).
///
/// `splash_logo` is the `nana://res/` URL of the application's packaged
/// Early Splash logo, if it declared one; the self-check reads it exactly as
/// the host would.
pub(crate) fn start(
    identity: &ApplicationIdentity,
    paths: Option<&ApplicationPaths>,
    options: Option<&ResourcePackOptions>,
    splash_logo: Option<&'static str>,
) -> Option<i32> {
    let self_check = std::env::var_os(SELF_CHECK_ENV).is_some_and(|v| v == "1");
    let default = ResourcePackOptions::new();
    let mounted = options.map(|options| mount(identity, paths, options));
    self_check.then(|| {
        let mounted = mounted.unwrap_or_else(|| mount(identity, paths, &default));
        run_self_check(paths, &mounted, splash_logo)
    })
}

#[derive(serde::Serialize)]
struct CheckLine {
    name: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Print `{"nana_package_validate":1,...}` and return the exit code: 0 when
/// every check passed, 3 otherwise. Never prints key material.
fn run_self_check(
    paths: Option<&ApplicationPaths>,
    mounted: &Mounted,
    splash_logo: Option<&'static str>,
) -> i32 {
    let mut checks = Vec::new();
    let mut push = |name, ok, detail: Option<String>| checks.push(CheckLine { name, ok, detail });

    match paths {
        Some(paths) => {
            let layout = paths.layout();
            push(
                "paths.layout",
                layout != RuntimeLayout::Development,
                Some(layout.as_str().into()),
            );
        }
        None => push("paths.layout", false, Some("unresolved".into())),
    }
    match &mounted.manifest {
        Ok((manifest, state)) => {
            push("manifest.read", true, Some(format!("{state:?}")));
            // Delivery channels drop empty directories; what must exist is
            // every file the manifest lists.
            if let Some(paths) = paths {
                let missing: Vec<&str> = manifest
                    .runtime_files
                    .iter()
                    .filter(|file| {
                        !file
                            .path
                            .split('/')
                            .fold(paths.app_root().to_path_buf(), |dir, part| dir.join(part))
                            .is_file()
                    })
                    .map(|file| file.path.as_str())
                    .collect();
                push(
                    "files.present",
                    missing.is_empty(),
                    (!missing.is_empty()).then(|| missing.join(", ")),
                );
            }
            push(
                "identity.matches-manifest",
                mounted.identity_matches == Some(true),
                None,
            );
            push(
                "resources.source",
                mounted.source == Some(Source::Packs),
                mounted.source.map(|source| format!("{source:?}")),
            );
            for pack in &manifest.resource_packs {
                let ok = pack_readable(pack);
                push(
                    "pack.entries",
                    ok.is_ok(),
                    Some(match ok {
                        Ok(()) => pack.name.clone(),
                        Err(error) => format!("{}: {error}", pack.name),
                    }),
                );
            }
        }
        Err(error) => push(
            "manifest.read",
            false,
            Some(format!("{error} (code {})", error.code())),
        ),
    }
    #[cfg(feature = "hosted")]
    if let Some(url) = splash_logo {
        let (ok, detail) = splash_logo_check(url);
        push("startup.early-splash-logo", ok, Some(detail));
    }
    #[cfg(not(feature = "hosted"))]
    let _ = splash_logo;
    let ok = checks.iter().all(|c| c.ok);
    // Fixed key order: the validator finds the line by its prefix.
    let checks = serde_json::to_string(&checks).expect("checks serialize");
    println!(
        "{}1,\"ok\":{ok},\"checks\":{checks}}}",
        nana_package::SELF_CHECK_PREFIX
    );
    if ok { 0 } else { 3 }
}

/// The packaged Early Splash logo, read and header-checked as the host
/// would before showing it. The native splash itself needs a window.
#[cfg(feature = "hosted")]
fn splash_logo_check(url: &str) -> (bool, String) {
    use crate::startup::{SplashFailure, validate_logo};
    match read_splash_logo(url).and_then(|png| validate_logo(&png).map_err(SplashFailure::Logo)) {
        Ok(info) => (true, format!("{url}: {}x{}", info.width, info.height)),
        Err(failure) => (false, format!("{url}: {failure}")),
    }
}

/// Open `pack` with the application's keys and read every entry: the first
/// through the installed `nana://res/` hook, exactly as a stylesheet or
/// image would (routing included), the rest directly. Self-check only;
/// normal startup never walks a pack.
fn pack_readable(pack: &nana_package::manifest::ManifestPack) -> Result<(), String> {
    let mount = MOUNT.get().ok_or("no pack mount")?;
    let lazy = mount
        .packs
        .iter()
        .find(|lazy| lazy.name == pack.name)
        .ok_or("pack is not mounted")?;
    let reader = mount
        .reader(lazy)
        .map_err(|error| format!("{error} (code {})", error.code()))?;
    let keys = reader.keys().map_err(|error| error.to_string())?;
    let first = keys.first().ok_or("pack is empty")?;
    if nana_ui_core::read_packaged(&nana_ui_core::packaged_url(first), None, u64::MAX).is_none() {
        return Err(
            match reader.read(first, u64::MAX, &mut ReadStats::default()) {
                Err(error) => format!("{first}: {error} (code {})", error.code()),
                Ok(_) => format!("{first}: the nana://res/ hook routes elsewhere"),
            },
        );
    }
    for key in &keys[1..] {
        reader
            .read(key, u64::MAX, &mut ReadStats::default())
            .map_err(|error| format!("{key}: {error} (code {})", error.code()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lazy(name: &str, prefixes: &[&str]) -> LazyPack {
        LazyPack {
            name: name.into(),
            path: PathBuf::from(format!("/nonexistent/{name}.nrpack")),
            prefixes: prefixes.iter().map(|p| (*p).to_owned()).collect(),
            expected: None,
            reader: Mutex::new(Slot::Closed),
            integrity_reported: AtomicBool::new(false),
            open_failure_reported: AtomicBool::new(false),
        }
    }

    fn mount(packs: Vec<LazyPack>) -> PackMount {
        PackMount {
            packs,
            keys: Arc::new(NoKeys),
            trust: TrustPolicy::AllowUnsigned,
        }
    }

    #[test]
    fn lookups_route_by_longest_prefix() {
        let mount = mount(vec![
            lazy("ui", &["ui/"]),
            lazy("fonts", &["ui/fonts/"]),
            lazy("readme", &["readme.txt"]),
        ]);
        let route = |path| mount.route(path).map(|pack| pack.name.as_str());
        assert_eq!(route("ui/app.css"), Some("ui"));
        assert_eq!(route("ui/fonts/a.ttf"), Some("fonts"));
        assert_eq!(route("readme.txt"), Some("readme"));
        assert_eq!(route("readme.txt.bak"), None);
        assert_eq!(route("uix/a.css"), None);
    }

    #[test]
    fn a_missing_pack_is_not_found_and_stays_closed_for_good() {
        let mount = mount(vec![lazy("ui", &["ui/"])]);
        assert_eq!(
            mount.read("ui/a.css", 1024),
            Err(PackagedReadError::NotFound)
        );
        assert!(matches!(
            &*mount.packs[0].reader.lock().unwrap(),
            Slot::Failed(PackError::Missing)
        ));
        assert_eq!(
            mount.read("other/a.css", 1024),
            Err(PackagedReadError::NotFound)
        );
    }

    #[test]
    fn loose_source_reads_logical_paths_inside_its_root_only() {
        let root = std::env::temp_dir().join(format!("nana-loose-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ui")).unwrap();
        std::fs::write(root.join("ui/a%41.css"), b"x").unwrap();
        std::fs::write(root.join("ui/big.bin"), [0u8; 16]).unwrap();
        let source = LooseSource { root: root.clone() };
        // No second percent-decoding: the logical path names the file as is.
        assert_eq!(source.read("ui/a%41.css", 16), Ok(b"x".to_vec()));
        assert_eq!(
            source.read("ui/big.bin", 8),
            Err(PackagedReadError::TooLarge)
        );
        assert_eq!(
            source.read("ui/missing", 8),
            Err(PackagedReadError::NotFound)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_and_missing_vendor_are_the_same_identity() {
        let mut manifest = PackageManifest::from_json(
            br#"{"schema":"nana.package-manifest","schema_version":1,
            "application":{"id":"a.b","name":"A","version":"1","vendor":"","build_id":null},
            "target":{"triple":"t","platform":"linux"},"nana":{"version":"0"},
            "build":{"profile":"dist","debug_assertions_suspected":false},
            "static_app_plan":{"status":"unavailable","reason":"r"},
            "distribution":{"backend":"portable","self_update":false},
            "layout":{"executable":"A","runtime_bin":"runtime/bin","runtime_resources":"runtime/resources",
              "runtime_plugins":"runtime/plugins","runtime_tools":"runtime/tools",
              "runtime_manifest":"runtime/manifest","root_exceptions":[]},
            "resource_packs":[],"plugins":[],"runtime_files":[],
            "signing":{"publisher":{"state":"not-configured"},"publisher_key":null,"platform":[]}}"#,
        )
        .unwrap();
        let identity = ApplicationIdentity::new("a.b", "A", "1");
        assert!(identity_matches(&identity, &manifest));
        manifest.application.vendor = Some("V".into());
        assert!(!identity_matches(&identity, &manifest));
        assert!(identity_matches(&identity.vendor("V"), &manifest));
    }
}

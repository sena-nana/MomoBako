//! Test-only hooks for repository backend and downloader seams.

#[cfg(test)]
use super::{FileSystemEntry, RepositoryRecord};
#[cfg(test)]
use std::{
    path::Path,
    sync::{Mutex, OnceLock},
};

#[cfg(test)]
type TestDownloaderPlaybackHook = fn(serde_json::Value) -> Result<serde_json::Value, String>;
#[cfg(test)]
type TestDownloaderTrackPackageHook = fn(serde_json::Value) -> Result<serde_json::Value, String>;
#[cfg(test)]
type TestBackendStatEntryHook =
    fn(&RepositoryRecord, &Path, &str) -> Option<Result<FileSystemEntry, String>>;

#[cfg(test)]
static TEST_DOWNLOADER_PLAYBACK_HOOK: OnceLock<Mutex<Option<TestDownloaderPlaybackHook>>> =
    OnceLock::new();
#[cfg(test)]
static TEST_DOWNLOADER_TRACK_PACKAGE_HOOK: OnceLock<Mutex<Option<TestDownloaderTrackPackageHook>>> =
    OnceLock::new();
#[cfg(test)]
static TEST_BACKEND_STAT_ENTRY_HOOK: OnceLock<Mutex<Option<TestBackendStatEntryHook>>> =
    OnceLock::new();

#[cfg(test)]
pub(crate) fn downloader_playback_hook(
) -> Result<Option<TestDownloaderPlaybackHook>, String> {
    TEST_DOWNLOADER_PLAYBACK_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map(|guard| guard.as_ref().copied())
        .map_err(|_| "test downloader playback hook lock poisoned".to_string())
}

#[cfg(test)]
pub(crate) fn downloader_track_package_hook(
) -> Result<Option<TestDownloaderTrackPackageHook>, String> {
    TEST_DOWNLOADER_TRACK_PACKAGE_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map(|guard| guard.as_ref().copied())
        .map_err(|_| "test downloader track package hook lock poisoned".to_string())
}

#[cfg(test)]
pub(crate) fn backend_stat_entry_hook(
    repo: &RepositoryRecord,
    repo_root: &Path,
    entry_path: &str,
) -> Result<Option<Result<FileSystemEntry, String>>, String> {
    TEST_BACKEND_STAT_ENTRY_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map(|guard| guard.as_ref().and_then(|hook| hook(repo, repo_root, entry_path)))
        .map_err(|_| "test backend stat entry hook lock poisoned".to_string())
}

#[cfg(test)]
pub(crate) fn set_test_downloader_playback_hook(hook: Option<TestDownloaderPlaybackHook>) {
    *TEST_DOWNLOADER_PLAYBACK_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("test downloader playback hook lock should succeed") = hook;
}

#[cfg(test)]
pub(crate) fn set_test_downloader_track_package_hook(hook: Option<TestDownloaderTrackPackageHook>) {
    *TEST_DOWNLOADER_TRACK_PACKAGE_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("test downloader track package hook lock should succeed") = hook;
}

#[cfg(test)]
pub(crate) fn set_test_backend_stat_entry_hook(hook: Option<TestBackendStatEntryHook>) {
    *TEST_BACKEND_STAT_ENTRY_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("test backend stat entry hook lock should succeed") = hook;
}

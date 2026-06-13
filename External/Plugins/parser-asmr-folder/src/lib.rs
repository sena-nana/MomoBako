use std::{collections::BTreeMap, ffi::CString, os::raw::c_char};

use momobako_backend_plugin_sdk::{free_c_string, read_request, response_error, response_ok};
use serde::Deserialize;

const MANIFEST: &str = include_str!("../manifest.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefaultsPayload {
    entries: Vec<MetadataDefaultEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetadataDefaultEntry {
    path: String,
    name: String,
    extension: String,
    kind: String,
}

#[derive(Debug, Clone)]
struct WorkContext {
    work_id: String,
    work_root: String,
    work_title: String,
}

#[no_mangle]
pub extern "C" fn momobako_plugin_manifest() -> *mut c_char {
    CString::new(MANIFEST)
        .expect("manifest should not contain null bytes")
        .into_raw()
}

#[no_mangle]
pub extern "C" fn momobako_plugin_call(input: *const c_char) -> *mut c_char {
    match handle_call(input) {
        Ok(value) => response_ok(value),
        Err(error) => response_error(error),
    }
}

#[no_mangle]
pub unsafe extern "C" fn momobako_plugin_free(value: *mut c_char) {
    unsafe { free_c_string(value) };
}

fn handle_call(input: *const c_char) -> Result<serde_json::Value, String> {
    let request = read_request(input)?;
    match request.method.as_str() {
        "metadata.defaults.batch" => {
            let payload: DefaultsPayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            let defaults = metadata_defaults(payload.entries);
            Ok(serde_json::json!({ "defaultsByPath": defaults }))
        }
        method => Err(format!("unsupported method: {method}")),
    }
}

fn metadata_defaults(
    entries: Vec<MetadataDefaultEntry>,
) -> BTreeMap<String, BTreeMap<String, serde_json::Value>> {
    entries
        .into_iter()
        .filter_map(|entry| {
            if entry.kind != "file" {
                return None;
            }
            let defaults = default_metadata(&entry.path, &entry.name, &entry.extension);
            if defaults.is_empty() {
                None
            } else {
                Some((entry.path, defaults))
            }
        })
        .collect()
}

fn default_metadata(
    relative_path: &str,
    filename: &str,
    extension: &str,
) -> BTreeMap<String, serde_json::Value> {
    let Some(context) = WorkContext::from_relative_path(relative_path) else {
        return BTreeMap::new();
    };
    let extension = extension.to_ascii_lowercase();
    let mut defaults = BTreeMap::from([
        ("libraryKind".to_string(), serde_json::json!("asmr")),
        ("workId".to_string(), serde_json::json!(context.work_id.clone())),
        ("rjCode".to_string(), serde_json::json!(context.work_id.clone())),
        ("workRoot".to_string(), serde_json::json!(context.work_root)),
        ("workTitle".to_string(), serde_json::json!(context.work_title)),
        ("trackPath".to_string(), serde_json::json!(relative_path)),
        ("trackTitle".to_string(), serde_json::json!(filename)),
        (
            "sourceUrl".to_string(),
            serde_json::json!(format!(
                "https://www.dlsite.com/maniax/work/=/product_id/{}.html",
                context.work_id
            )),
        ),
    ]);

    if is_audio_extension(&extension) {
        defaults.extend([
            ("asmrEntryKind".to_string(), serde_json::json!("audio")),
            ("listeningStatus".to_string(), serde_json::json!("unlistened")),
            ("listeningProgress".to_string(), serde_json::json!(0)),
            ("trackDurationMs".to_string(), serde_json::json!(0)),
        ]);
    } else if is_lyric_extension(&extension) {
        defaults.extend([
            ("asmrEntryKind".to_string(), serde_json::json!("lyric")),
            ("lyricStatus".to_string(), serde_json::json!("local")),
        ]);
    } else if is_companion_extension(&extension) {
        defaults.insert("asmrEntryKind".to_string(), serde_json::json!("companion"));
    }

    defaults
}

impl WorkContext {
    fn from_relative_path(relative_path: &str) -> Option<Self> {
        let parts = relative_path
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let (index, work_id) = parts
            .iter()
            .enumerate()
            .find_map(|(index, part)| extract_work_id(part).map(|work_id| (index, work_id)))?;
        Some(Self {
            work_id,
            work_root: parts[..=index].join("/"),
            work_title: parts[index].to_string(),
        })
    }
}

fn extract_work_id(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    for index in 0..bytes.len().saturating_sub(1) {
        if !bytes[index].eq_ignore_ascii_case(&b'R')
            || !bytes[index + 1].eq_ignore_ascii_case(&b'J')
        {
            continue;
        }
        let digits_start = index + 2;
        let mut digits_end = digits_start;
        while digits_end < bytes.len() && bytes[digits_end].is_ascii_digit() {
            digits_end += 1;
        }
        let digit_count = digits_end.saturating_sub(digits_start);
        if (6..=8).contains(&digit_count) {
            return Some(format!("RJ{}", &value[digits_start..digits_end]));
        }
    }
    None
}

fn is_audio_extension(extension: &str) -> bool {
    matches!(
        extension,
        "mp3" | "ogg" | "opus" | "wav" | "aac" | "flac" | "webm" | "mp4" | "m4a" | "mka"
    )
}

fn is_lyric_extension(extension: &str) -> bool {
    matches!(extension, "lrc" | "srt" | "ass" | "vtt")
}

fn is_companion_extension(extension: &str) -> bool {
    matches!(extension, "txt" | "pdf" | "jpg" | "jpeg" | "png" | "webp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_defaults_for_work_tracks() {
        let defaults = default_metadata("Voice/RJ123456 Work/01.mp3", "01.mp3", "mp3");
        assert_eq!(defaults.get("libraryKind"), Some(&serde_json::json!("asmr")));
        assert_eq!(defaults.get("rjCode"), Some(&serde_json::json!("RJ123456")));
        assert_eq!(defaults.get("asmrEntryKind"), Some(&serde_json::json!("audio")));
    }
}

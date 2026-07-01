use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
    os::raw::c_char,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCallEnvelope {
    pub method: String,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub runtime: PluginRuntimeContext,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRuntimeContext {
    pub plugin_id: String,
    pub plugin_data_dir: String,
    #[serde(default)]
    pub service_root_dir: String,
    #[serde(default)]
    pub plugin_config: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCallResponse {
    pub ok: bool,
    pub payload: Option<serde_json::Value>,
    pub error: Option<String>,
}

pub fn read_request(input: *const c_char) -> Result<PluginCallEnvelope, String> {
    if input.is_null() {
        return Err("plugin request pointer is null".to_string());
    }
    let raw = unsafe { CStr::from_ptr(input) }
        .to_str()
        .map_err(|error| error.to_string())?;
    serde_json::from_str(raw).map_err(|error| error.to_string())
}

pub fn response_ok(payload: serde_json::Value) -> *mut c_char {
    write_response(PluginCallResponse {
        ok: true,
        payload: Some(payload),
        error: None,
    })
}

pub fn response_error(error: impl Into<String>) -> *mut c_char {
    write_response(PluginCallResponse {
        ok: false,
        payload: None,
        error: Some(error.into()),
    })
}

pub fn write_response(response: PluginCallResponse) -> *mut c_char {
    let json = serde_json::to_string(&response).unwrap_or_else(|error| {
        format!(
            "{{\"ok\":false,\"payload\":null,\"error\":\"failed to encode plugin response: {error}\"}}"
        )
    });
    CString::new(json)
        .unwrap_or_else(|_| CString::new("{\"ok\":false,\"payload\":null,\"error\":\"plugin response contained a null byte\"}").expect("static CString should be valid"))
        .into_raw()
}

/// # Safety
///
/// `value` must be returned from this SDK's response helpers.
pub unsafe fn free_c_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    let _ = CString::from_raw(value);
}

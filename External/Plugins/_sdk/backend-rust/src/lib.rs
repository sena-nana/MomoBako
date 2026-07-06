use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
    os::raw::c_char,
    sync::{Mutex, OnceLock},
};

use serde::{Deserialize, Serialize};

pub type HostPluginCallFn = unsafe extern "C" fn(*const c_char) -> *mut c_char;
pub type HostPluginFreeFn = unsafe extern "C" fn(*mut c_char);

static HOST_PLUGIN_API: OnceLock<Mutex<Option<HostPluginApi>>> = OnceLock::new();

#[derive(Clone, Copy)]
struct HostPluginApi {
    call: HostPluginCallFn,
    free: HostPluginFreeFn,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCallEnvelope {
    pub method: String,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub runtime: PluginRuntimeContext,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRuntimeContext {
    pub plugin_id: String,
    pub plugin_data_dir: String,
    #[serde(default)]
    pub service_root_dir: String,
    #[serde(default)]
    pub plugin_runtime_dir: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostPluginCallEnvelope {
    pub service_root_dir: String,
    pub plugin_id: String,
    pub method: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostLogLocation {
    pub module_path: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostLogWriteRequest {
    pub level: String,
    pub category: String,
    pub action: String,
    pub message: String,
    #[serde(default)]
    pub context: serde_json::Value,
    #[serde(default)]
    pub repo_id: Option<String>,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub source_label: Option<String>,
    #[serde(default)]
    pub location: Option<HostLogLocation>,
}

pub fn register_host_plugin_api(
    call: Option<HostPluginCallFn>,
    free: Option<HostPluginFreeFn>,
) {
    let api = match (call, free) {
        (Some(call), Some(free)) => Some(HostPluginApi { call, free }),
        _ => None,
    };
    let slot = HOST_PLUGIN_API.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = api;
    }
}

pub fn call_host_plugin(
    runtime: &PluginRuntimeContext,
    plugin_id: &str,
    method: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let api = {
        let slot = HOST_PLUGIN_API
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "host plugin API lock poisoned".to_string())?;
        slot.as_ref()
            .copied()
            .ok_or_else(|| "host plugin API is unavailable".to_string())?
    };
    let request = HostPluginCallEnvelope {
        service_root_dir: runtime.service_root_dir.trim().to_string(),
        plugin_id: plugin_id.trim().to_string(),
        method: method.trim().to_string(),
        payload,
    };
    if request.service_root_dir.is_empty() {
        return Err("host plugin call requires serviceRootDir".to_string());
    }
    if request.plugin_id.is_empty() {
        return Err("host plugin call requires pluginId".to_string());
    }
    if request.method.is_empty() {
        return Err("host plugin call requires method".to_string());
    }
    let request_json = serde_json::to_string(&request).map_err(|error| error.to_string())?;
    let request_cstring = CString::new(request_json)
        .map_err(|_| "host plugin request contained a null byte".to_string())?;
    let response_ptr = unsafe { (api.call)(request_cstring.as_ptr()) };
    if response_ptr.is_null() {
        return Err("host plugin call returned a null response".to_string());
    }
    let response_json = unsafe { CStr::from_ptr(response_ptr) }
        .to_str()
        .map_err(|error| error.to_string())?
        .to_string();
    unsafe { (api.free)(response_ptr) };
    let response: PluginCallResponse =
        serde_json::from_str(&response_json).map_err(|error| error.to_string())?;
    if response.ok {
        Ok(response.payload.unwrap_or_else(|| serde_json::json!({})))
    } else {
        Err(response
            .error
            .unwrap_or_else(|| "host plugin call failed without an error message".to_string()))
    }
}

/// 通过宿主内建日志总线写入一条标准化日志。
pub fn write_host_log<T: Serialize>(
    runtime: &PluginRuntimeContext,
    level: &str,
    action: &str,
    message: &str,
    context: T,
) -> Result<serde_json::Value, String> {
    let request = HostLogWriteRequest {
        level: level.trim().to_string(),
        category: "plugin.backend".to_string(),
        action: action.trim().to_string(),
        message: message.trim().to_string(),
        context: serde_json::to_value(context).map_err(|error| error.to_string())?,
        repo_id: None,
        plugin_id: Some(runtime.plugin_id.clone()),
        source_kind: Some("backend-plugin".to_string()),
        source_label: Some(runtime.plugin_id.clone()),
        location: Some(HostLogLocation {
            module_path: Some(runtime.plugin_id.clone()),
            file: None,
            line: None,
        }),
    };
    call_host_plugin(
        runtime,
        "momobako.system",
        "system.log.write",
        serde_json::to_value(request).map_err(|error| error.to_string())?,
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static LAST_REQUEST: OnceLock<Mutex<Option<HostPluginCallEnvelope>>> = OnceLock::new();

    unsafe extern "C" fn test_host_call(input: *const c_char) -> *mut c_char {
        let raw = unsafe { CStr::from_ptr(input) }
            .to_str()
            .expect("host bridge request should be utf-8")
            .to_string();
        let envelope: HostPluginCallEnvelope =
            serde_json::from_str(&raw).expect("host bridge request should decode");
        let slot = LAST_REQUEST.get_or_init(|| Mutex::new(None));
        *slot.lock().expect("request slot should lock") = Some(envelope);
        response_ok(serde_json::json!({
            "taskId": "task-1",
            "status": "queued"
        }))
    }

    unsafe extern "C" fn test_host_free(value: *mut c_char) {
        unsafe { free_c_string(value) };
    }

    #[test]
    fn call_host_plugin_uses_registered_host_bridge() {
        register_host_plugin_api(Some(test_host_call), Some(test_host_free));
        let runtime = PluginRuntimeContext {
            plugin_id: "momobako.service.office-convert".to_string(),
            plugin_data_dir: "C:/Service/plugin-data/momobako-service-office-convert".to_string(),
            service_root_dir: "C:/Service".to_string(),
            plugin_runtime_dir: "C:/Service/runtime/plugins/office-convert".to_string(),
            plugin_config: BTreeMap::new(),
        };
        let response = call_host_plugin(
            &runtime,
            "momobako.service.downloader",
            "downloader.enqueueDownload",
            serde_json::json!({
                "url": "https://example.com/runtime.msi"
            }),
        )
        .expect("host bridge call should succeed");
        assert_eq!(response.get("taskId").and_then(serde_json::Value::as_str), Some("task-1"));
        let request = LAST_REQUEST
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("request slot should lock")
            .clone()
            .expect("request should be recorded");
        assert_eq!(request.service_root_dir, "C:/Service");
        assert_eq!(request.plugin_id, "momobako.service.downloader");
        assert_eq!(request.method, "downloader.enqueueDownload");
        register_host_plugin_api(None, None);
    }

    #[test]
    fn write_host_log_calls_internal_system_logger_route() {
        register_host_plugin_api(Some(test_host_call), Some(test_host_free));
        let runtime = PluginRuntimeContext {
            plugin_id: "momobako.service.office-convert".to_string(),
            plugin_data_dir: "C:/Service/plugin-data/momobako-service-office-convert".to_string(),
            service_root_dir: "C:/Service".to_string(),
            plugin_runtime_dir: "C:/Service/runtime/plugins/office-convert".to_string(),
            plugin_config: BTreeMap::new(),
        };

        let _ = write_host_log(
            &runtime,
            "warn",
            "runtimeHealthChanged",
            "运行时状态发生变化。",
            serde_json::json!({
                "healthy": false,
                "reason": "helper exited",
            }),
        )
        .expect("host log write should succeed");

        let request = LAST_REQUEST
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("request slot should lock")
            .clone()
            .expect("request should be recorded");
        assert_eq!(request.plugin_id, "momobako.system");
        assert_eq!(request.method, "system.log.write");
        assert_eq!(
            request.payload.get("pluginId").and_then(serde_json::Value::as_str),
            Some("momobako.service.office-convert")
        );
        assert_eq!(
            request.payload.get("sourceKind").and_then(serde_json::Value::as_str),
            Some("backend-plugin")
        );
        assert_eq!(
            request.payload.get("action").and_then(serde_json::Value::as_str),
            Some("runtimeHealthChanged")
        );
        register_host_plugin_api(None, None);
    }
}

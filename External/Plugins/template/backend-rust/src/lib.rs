use std::ffi::{c_char, CString};

use momobako_backend_plugin_sdk::{
    free_c_string, read_request, register_host_plugin_api, response_error, response_with_error_log,
    HostPluginCallFn, HostPluginFreeFn,
};

#[no_mangle]
pub extern "C" fn momobako_plugin_manifest() -> *mut c_char {
    CString::new(
        r#"{"pluginId":"momobako.template.backend","name":"Template Backend Plugin","version":"0.1.0"}"#,
    )
    .expect("static manifest should be valid")
    .into_raw()
}

#[no_mangle]
pub extern "C" fn momobako_plugin_call(input: *const c_char) -> *mut c_char {
    let request = match read_request(input) {
        Ok(request) => request,
        Err(error) => return response_error(error),
    };

    let method = request.method.clone();
    let runtime = request.runtime.clone();
    let result = match request.method.as_str() {
        "ping" => Ok(serde_json::json!({
            "ok": true,
            "plugin": "momobako.template.backend"
        })),
        other => Err(format!("unsupported method: {other}")),
    };
    response_with_error_log(&runtime, &method, result)
}

#[no_mangle]
pub extern "C" fn momobako_plugin_register_host_api(
    call: Option<HostPluginCallFn>,
    free: Option<HostPluginFreeFn>,
) {
    register_host_plugin_api(call, free);
}

#[no_mangle]
pub unsafe extern "C" fn momobako_plugin_free(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    unsafe { free_c_string(value) };
}

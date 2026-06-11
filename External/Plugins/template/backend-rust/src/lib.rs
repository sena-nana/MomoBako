use std::{
    ffi::{c_char, CString},
    ptr,
};

use momobako_backend_plugin_sdk::{
    free_c_string, read_request, response_error, response_ok,
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

    match request.method.as_str() {
        "ping" => response_ok(serde_json::json!({
            "ok": true,
            "plugin": "momobako.template.backend"
        })),
        other => response_error(format!("unsupported method: {other}")),
    }
}

#[no_mangle]
pub unsafe extern "C" fn momobako_plugin_free(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    unsafe { free_c_string(value) };
}

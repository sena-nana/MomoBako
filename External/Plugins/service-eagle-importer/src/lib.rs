use std::{ffi::CString, os::raw::c_char, path::Path};

use momobako_backend_plugin_sdk::{
    free_c_string, read_request, response_error, response_with_error_log, PluginCallEnvelope,
};
use momobako_lib::{import_eagle_library_with_service_root, EagleLibraryImportRequest};

const MANIFEST: &str = include_str!("../manifest.json");

#[no_mangle]
pub extern "C" fn momobako_plugin_manifest() -> *mut c_char {
    CString::new(MANIFEST)
        .expect("manifest should not contain null bytes")
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
    response_with_error_log(&runtime, &method, handle_call(request))
}

#[no_mangle]
pub unsafe extern "C" fn momobako_plugin_free(value: *mut c_char) {
    unsafe { free_c_string(value) };
}

fn handle_call(request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    match request.method.as_str() {
        "eagleImporter.importLibrary" => {
            let payload: EagleLibraryImportRequest =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            let service_root_dir = request.runtime.service_root_dir.trim();
            if service_root_dir.is_empty() {
                return Err("eagle importer requires serviceRootDir".to_string());
            }
            let response =
                import_eagle_library_with_service_root(Path::new(service_root_dir), payload)?;
            serde_json::to_value(response).map_err(|error| error.to_string())
        }
        method => Err(format!("unsupported method: {method}")),
    }
}

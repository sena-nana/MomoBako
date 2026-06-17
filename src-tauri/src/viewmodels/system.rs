//! Desktop-system command orchestration for lightweight shell-facing helpers.

use crate::services::repository::{
    BinaryFileWriteRequest, BinaryFileWriteResponse,
};
use crate::services::runtime::{ExternalApiConnectionStatus, RepositoryRuntime};
use std::{fs, path::PathBuf};

#[derive(Clone)]
pub struct SystemViewModel {
    runtime: RepositoryRuntime,
}

impl SystemViewModel {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    /// Writes arbitrary bytes to a desktop-selected path while preserving the existing command contract.
    pub async fn write_binary_file(
        &self,
        request: BinaryFileWriteRequest,
    ) -> Result<BinaryFileWriteResponse, String> {
        tauri::async_runtime::spawn_blocking(move || {
            let output_path = PathBuf::from(&request.path);
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(&output_path, &request.bytes).map_err(|error| error.to_string())?;
            Ok(BinaryFileWriteResponse {
                path: request.path,
                size_bytes: i64::try_from(request.bytes.len())
                    .map_err(|_| "written file is too large".to_string())?,
            })
        })
        .await
        .map_err(|error| error.to_string())?
    }

    /// Returns the latest external API connection payload without exposing runtime details to the command layer.
    pub async fn get_external_api_connection_status(
        &self,
    ) -> Result<ExternalApiConnectionStatus, String> {
        Ok(self.runtime.external_api_connection_status())
    }
}

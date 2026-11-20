//! 宿主调用的系统辅助接口：写入用户选定路径的文件、系统日志和外部 API 连接状态。

use crate::services::logging::{clear_logs, list_logs, write_log};
use crate::services::repository::{BinaryFileWriteRequest, BinaryFileWriteResponse};
use crate::services::repository::{
    SystemLogPage, SystemLogQuery, SystemLogRecord, SystemLogWriteRequest,
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

    /// 把字节写到用户在宿主里选定的路径，失败时记日志并交回原因。
    pub async fn write_binary_file(
        &self,
        request: BinaryFileWriteRequest,
    ) -> Result<BinaryFileWriteResponse, String> {
        match tokio::task::spawn_blocking(move || {
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
        {
            Ok(result) => {
                match &result {
                    Ok(response) => {
                        crate::app_log!(
                            "info",
                            "system.file",
                            "writeBinaryFile",
                            "二进制文件写入完成。",
                            serde_json::json!({
                                "path": response.path.as_str(),
                                "sizeBytes": response.size_bytes,
                            })
                        );
                    }
                    Err(error) => {
                        crate::app_log!(
                            "error",
                            "system.file",
                            "writeBinaryFileFailed",
                            "二进制文件写入失败。",
                            serde_json::json!({ "error": error })
                        );
                    }
                }
                result
            }
            Err(error) => {
                let error = error.to_string();
                crate::app_log!(
                    "error",
                    "system.file",
                    "writeBinaryFileTaskFailed",
                    "二进制文件写入任务执行失败。",
                    serde_json::json!({ "error": error.as_str() })
                );
                Err(error)
            }
        }
    }

    /// 外部 API 的连接信息（地址、令牌、是否就绪），不暴露 Runtime 内部细节。
    pub async fn get_external_api_connection_status(
        &self,
    ) -> Result<ExternalApiConnectionStatus, String> {
        Ok(self.runtime.external_api_connection_status())
    }

    /// 返回系统日志分页结果，供日志中心面板读取。
    pub async fn list_system_logs(
        &self,
        query: Option<SystemLogQuery>,
    ) -> Result<SystemLogPage, String> {
        list_logs(query)
    }

    /// 写入一条来自宿主界面的系统日志。
    pub async fn write_system_log(
        &self,
        request: SystemLogWriteRequest,
    ) -> Result<SystemLogRecord, String> {
        write_log(request)
    }

    /// 清空系统日志文件与内存缓存。
    pub async fn clear_system_logs(&self) -> Result<(), String> {
        clear_logs()
    }
}

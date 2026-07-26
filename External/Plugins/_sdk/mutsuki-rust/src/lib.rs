//! MomoBako 原生插件到 Mutsuki ABI v2 的产品适配层。

use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};

use mutsuki_runtime_contracts::{
    ArtifactType, CompletionBatch, DomainEvent, EntryCompletion, ExecutionClass, PermissionGrant,
    PluginArtifact, ProtocolClass, RunnerDescriptor, RunnerPurity, RunnerResult, RunnerStatus,
    RuntimeError, WorkBatch,
};
use mutsuki_runtime_core::{Runner, RunnerContext};
use mutsuki_runtime_sdk::{
    HandlerBindingBuilder, PluginBuilder, ProtocolDescriptorBuilder, RunnerDescriptorBuilder,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type PluginHandler = fn(PluginCallEnvelope) -> Result<Value, String>;
pub use mutsuki_runtime_core::RuntimeResult;
pub use mutsuki_runtime_sdk::{export_mutsuki_plugin_abi_v2, AbiHostClientV2, LoadedPlugin};

static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static PENDING_EVENTS: RefCell<Vec<DomainEvent>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCallEnvelope {
    pub method: String,
    pub payload: Value,
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
    pub plugin_config: BTreeMap<String, Value>,
}

/// 从统一 ABI 初始化配置与协议表构造一个 Mutsuki 原生插件。
pub fn build_abi_plugin(
    _host: AbiHostClientV2,
    config: Value,
    plugin_id: &'static str,
    version: &'static str,
    protocols: &'static [&'static str],
    requires: &'static [&'static str],
    permissions: &'static [&'static str],
    handler: PluginHandler,
) -> RuntimeResult<LoadedPlugin> {
    let mut runtime = serde_json::from_value::<PluginRuntimeContext>(config.clone())
        .unwrap_or_else(|_| PluginRuntimeContext {
            plugin_id: plugin_id.to_string(),
            ..PluginRuntimeContext::default()
        });
    if runtime.plugin_runtime_dir.is_empty() {
        runtime.plugin_runtime_dir = config
            .pointer("/_mutsuki/runtime_dir")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
    }
    let runner_id = format!("{plugin_id}.runner");
    let descriptor = RunnerDescriptorBuilder::new(&runner_id, plugin_id)
        .purity(RunnerPurity::Effectful)
        .execution_class(ExecutionClass::Io);
    let descriptor = protocols
        .iter()
        .fold(descriptor, |builder, protocol| {
            builder.accepted_protocol(format!("momobako.{protocol}"))
        })
        .build();
    let mut plugin = PluginBuilder::new(plugin_id)
        .version(version)
        .artifact(PluginArtifact {
            artifact_type: ArtifactType::Abi,
            path: "plugin".into(),
            sha256: "sha256:guest".into(),
            companion_artifacts: Vec::new(),
        })
        .permissions(PermissionGrant {
            effects: permissions
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            resources: Vec::new(),
        })
        .runner(Box::new(MomoPluginRunner {
            descriptor,
            runtime,
            handler,
        }));
    for requirement in requires {
        plugin = plugin.requires(*requirement);
    }
    for protocol in protocols {
        let protocol_id = format!("momobako.{protocol}");
        plugin = plugin
            .protocol_descriptor(ProtocolDescriptorBuilder::new(&protocol_id).build())
            .protocol_class(&protocol_id, ProtocolClass::Effect)
            .handler_binding(
                HandlerBindingBuilder::new(
                    format!("binding:{plugin_id}:{protocol_id}"),
                    plugin_id,
                    &protocol_id,
                    &protocol_id,
                )
                .target_runner_hint(&runner_id)
                .pool_id("default")
                .build(),
            );
    }
    Ok(plugin.build())
}

/// 为一个现有 Momo handler 导出唯一的 Mutsuki ABI v2 入口。
#[macro_export]
macro_rules! export_mutsuki_momobako_plugin {
    (
        $plugin_id:literal,
        $version:literal,
        protocols = [$($protocol:literal),+ $(,)?],
        requires = [$($requirement:literal),* $(,)?],
        permissions = [$($permission:literal),* $(,)?],
        $handler:path
    ) => {
        fn create_mutsuki_plugin(
            host: $crate::AbiHostClientV2,
            config: serde_json::Value,
        ) -> $crate::RuntimeResult<$crate::LoadedPlugin> {
            $crate::build_abi_plugin(
                host,
                config,
                $plugin_id,
                $version,
                &[$($protocol),+],
                &[$($requirement),*],
                &[$($permission),*],
                $handler,
            )
        }

        $crate::export_mutsuki_plugin_abi_v2!(create_mutsuki_plugin);
    };
}

struct MomoPluginRunner {
    descriptor: RunnerDescriptor,
    runtime: PluginRuntimeContext,
    handler: PluginHandler,
}

impl Runner for MomoPluginRunner {
    fn descriptor(&self) -> &RunnerDescriptor {
        &self.descriptor
    }

    fn run_batch(
        &mut self,
        ctx: RunnerContext,
        batch: WorkBatch,
    ) -> RuntimeResult<CompletionBatch> {
        let results = batch
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                if ctx.cancel_requested {
                    let mut result = RunnerResult::completed(entry.task_id.clone());
                    result.status = RunnerStatus::Cancelled;
                    return EntryCompletion {
                        entry_id: entry.entry_id.clone(),
                        task_id: entry.task_id.clone(),
                        result: Some(result),
                        error: None,
                    };
                }
                let task = match batch.payload_task(index) {
                    Ok(task) => task,
                    Err(error) => {
                        return EntryCompletion {
                            entry_id: entry.entry_id.clone(),
                            task_id: entry.task_id.clone(),
                            result: None,
                            error: Some(error),
                        };
                    }
                };
                let method = task
                    .protocol_id
                    .strip_prefix("momobako.")
                    .unwrap_or(task.protocol_id.as_str())
                    .to_string();
                PENDING_EVENTS.with(|events| events.borrow_mut().clear());
                let response = (self.handler)(PluginCallEnvelope {
                    method,
                    payload: task.payload.as_value().clone(),
                    runtime: self.runtime.clone(),
                });
                let events =
                    PENDING_EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()));
                match response {
                    Ok(output) => {
                        let mut result = RunnerResult::completed(entry.task_id.clone());
                        result.output = Some(output);
                        result.events = events;
                        EntryCompletion {
                            entry_id: entry.entry_id.clone(),
                            task_id: entry.task_id.clone(),
                            result: Some(result),
                            error: None,
                        }
                    }
                    Err(message) => EntryCompletion {
                        entry_id: entry.entry_id.clone(),
                        task_id: entry.task_id.clone(),
                        result: None,
                        error: Some(RuntimeError::new(
                            "momobako.plugin_call_failed",
                            self.runtime.plugin_id.clone(),
                            message,
                        )),
                    },
                }
            })
            .collect();
        Ok(CompletionBatch::from_results(&batch, results))
    }
}

/// 将插件日志转换为可观察的 Mutsuki DomainEvent。
pub fn write_host_log_silently<T: Serialize>(
    runtime: &PluginRuntimeContext,
    level: &str,
    action: &str,
    message: &str,
    context: T,
) {
    let sequence = EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let payload = serde_json::json!({
        "level": level,
        "action": action,
        "message": message,
        "pluginId": runtime.plugin_id,
        "context": serde_json::to_value(context).unwrap_or(Value::Null),
    });
    PENDING_EVENTS.with(|events| {
        events.borrow_mut().push(DomainEvent {
            event_id: format!("{}:{sequence}", runtime.plugin_id),
            kind: "momobako.plugin.log".into(),
            payload,
        });
    });
}

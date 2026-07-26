//! Mutsuki ABI v2 后端插件最小示例。

use momobako_mutsuki_plugin_sdk::{export_mutsuki_momobako_plugin, PluginCallEnvelope};

fn handle(request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    match request.method.as_str() {
        "ping" => Ok(serde_json::json!({
            "message": "pong",
            "pluginId": "momobako.example.backend-ping"
        })),
        other => Err(format!("unsupported method: {other}")),
    }
}

export_mutsuki_momobako_plugin!(
    "momobako.example.backend-ping",
    "0.1.0",
    protocols = ["ping"],
    requires = [],
    permissions = [],
    handle
);

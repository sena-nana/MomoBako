//! Mutsuki ABI v2 后端插件模板。

use momobako_mutsuki_plugin_sdk::{export_mutsuki_momobako_plugin, PluginCallEnvelope};

fn handle(request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    match request.method.as_str() {
        "ping" => Ok(serde_json::json!({
            "ok": true,
            "plugin": "momobako.template.backend"
        })),
        other => Err(format!("unsupported method: {other}")),
    }
}

export_mutsuki_momobako_plugin!(
    "momobako.template.backend",
    "0.1.0",
    protocols = ["ping"],
    requires = [],
    permissions = [],
    handle
);

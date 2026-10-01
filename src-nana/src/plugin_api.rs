//! NanaUI 插件贡献契约。
//!
//! 旧版前端插件通过 Vue `Component` 注册页面，原生宿主不能把该类型带入
//! Runtime。这里将贡献能力收敛为稳定的声明式描述，真正的绘制和交互由
//! Nana ViewModel 负责；旧插件被明确标记为需要升级，避免静默丢失功能。

use std::fmt;

/// 原生插件可以贡献的页面或媒体能力。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeContributionKind {
    Preview,
    PlaylistPlayer,
    ToolPage,
    SettingsPage,
}

impl NativeContributionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::PlaylistPlayer => "playlist-player",
            Self::ToolPage => "tool-page",
            Self::SettingsPage => "settings-page",
        }
    }
}

/// 插件在 Nana 宿主中声明的原生贡献。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativePluginContribution {
    pub plugin_id: String,
    pub kind: NativeContributionKind,
    pub label: String,
    pub order: i32,
    /// 由 Nana ViewModel 解析的稳定贡献标识，不允许携带 Vue 组件对象。
    pub view_id: String,
}

impl NativePluginContribution {
    pub fn new(
        plugin_id: impl Into<String>,
        kind: NativeContributionKind,
        label: impl Into<String>,
        view_id: impl Into<String>,
    ) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            kind,
            label: label.into(),
            order: 0,
            view_id: view_id.into(),
        }
    }

    pub fn with_order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }
}

/// 前端插件迁移状态，供设置页和日志使用。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PluginUiCompatibility {
    Native(Vec<NativePluginContribution>),
    UpgradeRequired { plugin_id: String, reason: String },
}

impl PluginUiCompatibility {
    pub fn upgrade_required(plugin_id: impl Into<String>) -> Self {
        let plugin_id = plugin_id.into();
        Self::UpgradeRequired {
            reason: "该插件仍注册 Vue Component，需要迁移为 Nana 原生贡献接口".to_string(),
            plugin_id,
        }
    }

    pub const fn is_usable(&self) -> bool {
        matches!(self, Self::Native(_))
    }
}

impl fmt::Display for PluginUiCompatibility {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(contributions) => {
                write!(formatter, "native contributions: {}", contributions.len())
            }
            Self::UpgradeRequired { plugin_id, reason } => {
                write!(formatter, "plugin {plugin_id} requires upgrade: {reason}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_contribution_preserves_kind_and_order() {
        let contribution = NativePluginContribution::new(
            "com.example.preview",
            NativeContributionKind::Preview,
            "预览",
            "preview.com.example",
        )
        .with_order(20);

        assert_eq!(contribution.kind.as_str(), "preview");
        assert_eq!(contribution.order, 20);
        assert_eq!(contribution.view_id, "preview.com.example");
    }

    #[test]
    fn legacy_vue_plugin_is_explicitly_blocked() {
        let compatibility = PluginUiCompatibility::upgrade_required("legacy.plugin");

        assert!(!compatibility.is_usable());
        assert!(compatibility.to_string().contains("legacy.plugin"));
        assert!(compatibility.to_string().contains("Vue Component"));
    }
}

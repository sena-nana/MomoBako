//! Settings model re-exports.
//!
//! Leaf chrome (`SettingsRow`, `SettingsCard`) is implemented in Runtime.

pub use nana_ui_core::AppearanceEvent;
pub use nana_ui_core::settings::{
    AppearanceSettings, BackdropTarget, SettingsError, SettingsModel, SettingsState, SettingsTab,
    SettingsTabId, WindowMaterialMode,
};
#[cfg(feature = "hosted")]
use nana_window::{MaterialEffect, PlatformMaterialSupport, hosted_platform_material_support};

#[cfg(feature = "hosted")]
pub fn window_material_effect(mode: WindowMaterialMode) -> MaterialEffect {
    match mode {
        WindowMaterialMode::Solid => MaterialEffect::Solid,
        WindowMaterialMode::Translucent => MaterialEffect::Transparent,
        WindowMaterialMode::Vibrancy => MaterialEffect::Vibrancy,
        WindowMaterialMode::Mica => MaterialEffect::Mica,
        WindowMaterialMode::Acrylic => MaterialEffect::Acrylic,
    }
}

/// Window materials the hosted Scene path can actually apply on this target.
#[cfg(feature = "hosted")]
pub fn hosted_window_material_modes() -> Vec<WindowMaterialMode> {
    let mut modes = vec![WindowMaterialMode::Solid, WindowMaterialMode::Translucent];
    match hosted_platform_material_support() {
        PlatformMaterialSupport::MicaAcrylic => {
            modes.push(WindowMaterialMode::Mica);
            modes.push(WindowMaterialMode::Acrylic);
        }
        PlatformMaterialSupport::Vibrancy => modes.push(WindowMaterialMode::Vibrancy),
        PlatformMaterialSupport::None => {}
    }
    modes
}

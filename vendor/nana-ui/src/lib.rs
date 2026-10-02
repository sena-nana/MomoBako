//! NanaUI adapter and the Scene/WGPU paint of Runtime/UiScene.
//!
//! Product retained/render contracts live in `nana-ui-runtime` and `nana-ui-scene`.
//! New applications should use [`runtime`] (`AppContext`, `build`, `mount`,
//! `ComponentView`, `register_component`). See
//! [`docs/reference/how-it-works.md`](../../../docs/reference/how-it-works.md),
//! [`docs/reference/start.md`](../../../docs/reference/start.md),
//! [`docs/reference/application-api.md`](../../../docs/reference/application-api.md).
//! Runtime owns the widget surface; the crate root exposes host adapters only.
//! Vue + JS (`nana-ui-vue`, `nanavue-*`) map into the same model.
//!
//! [`WorkspaceController`] is a host adapter (Instant→Duration, pointer →
//! [`WorkspaceMutation`]). Product region state is [`WorkspaceModel`].

#![recursion_limit = "256"]

#[cfg(any(feature = "hosted", feature = "accesskit-tree"))]
mod accessibility;
#[cfg(feature = "accesskit-tree")]
mod accessibility_tree;
pub mod command;
pub mod component_support;
pub mod components;
#[cfg(feature = "gpu")]
mod default_gpu_view;
pub mod dialog;
/// Host adapter (`nana_ui::dock::*`): pointer/dwell/frame → [`dock::DockMutation`].
/// Product dock is Runtime `nana_ui::runtime::DockWorkspace`.
pub mod dock;
#[cfg(feature = "gpu")]
mod font_face_ingest;
#[cfg(feature = "gpu")]
mod frame_binding;
pub mod geometry;
#[cfg(feature = "gpu")]
mod gpu_raw;
#[cfg(feature = "gpu")]
pub mod gpu_texture;
#[cfg(feature = "gpu")]
pub mod gpu_view;
#[cfg(feature = "gpu")]
mod gpu_work;
#[cfg(feature = "graph-canvas")]
pub mod graph;
#[cfg(feature = "hosted")]
mod host_diagnostics;
#[cfg(feature = "hosted")]
mod hosted_context;
#[cfg(feature = "gpu")]
mod painted_demand;
#[cfg(all(feature = "hosted", target_os = "windows"))]
mod windows_composition;
#[cfg(all(feature = "hosted", target_os = "windows"))]
pub use windows_composition::{
    WindowsComposition, WindowsCompositionError, WindowsCompositionRect, WindowsCompositionTree,
    WindowsCompositionWork, WindowsNativeVisual,
};
#[cfg(feature = "hosted")]
mod application;
mod application_builder;
#[cfg(feature = "packaged-resources")]
mod packaged_resources;
#[cfg(feature = "hosted")]
pub use application::{ApplicationState, ApplicationWindow, RuntimeApplication};
pub use application_builder::{ApplicationSession, NanaApplication, NanaApplicationBuilder};
/// Structured diagnostics (Issue #227): define application events and
/// metrics with these types and record them with its macros.
pub use nana_diagnostics as diagnostics;
pub use nana_diagnostics::{DiagnosticsConfig, PersistMode};
#[cfg(feature = "packaged-resources")]
pub use nana_package;
#[cfg(feature = "packaged-resources")]
pub use packaged_resources::{ResourcePackOptions, SELF_CHECK_ENV};
pub mod icons;
pub mod layout;
pub mod menu;
mod nana_text;
#[cfg(feature = "hosted")]
mod native_browser;
#[cfg(feature = "gpu")]
mod native_content;
mod text_engine;
#[cfg(feature = "hosted")]
pub use native_browser::{
    BrowserCommand, BrowserEvent, BrowserPolicy, BrowserRect, BrowserState, NativeBrowserEvent,
    NativeBrowserRequest,
};
pub mod overlay;
pub mod pane;
#[cfg(feature = "hosted")]
mod presentation;
#[cfg(feature = "hosted")]
pub use presentation::{
    GpuBackendPolicy, NativeChromePolicy, ResolvedWindowPresentation, SurfaceAlphaMode,
    SurfaceTargetFallback, WindowSurfaceTarget,
};
mod runtime_animation;
#[cfg(feature = "hosted")]
mod runtime_dock;
#[cfg(feature = "hosted")]
mod runtime_host;
#[cfg(feature = "gpu")]
mod scene_gpu;
#[cfg(feature = "gpu")]
pub use native_content::{NativeContentRegion, NativeContentWork, native_content_regions};
#[cfg(feature = "hosted")]
mod scene_host;
/// Two-phase startup (Issue #225): Early Splash, `UiReady`, takeover.
#[cfg(feature = "hosted")]
pub mod startup;
#[cfg(feature = "hosted")]
pub use scene_host::CompositionWork;
#[cfg(feature = "hosted")]
pub use startup::{
    SplashAnimation, SplashAnimationOutcome, SplashBackground, SplashFailure, SplashLogo,
    SplashLogoSource, SplashOutcome, SplashPackageError, SplashSkip, SplashSpec, StartupError,
    StartupHandle, StartupOptions, StartupPhase, StartupStatus, StartupTakeover, StartupTicket,
    StartupTimeline, StartupWork,
};
#[cfg(feature = "gpu")]
mod scene_paint;
pub mod selection;
pub mod settings;
pub mod split_pane;
pub mod theme;
pub mod tooltip;
pub mod virtual_list;
pub mod widgets;
pub mod window_chrome;
pub mod workspace;

#[cfg(all(test, feature = "gpu"))]
mod test_gpu;

pub mod runtime;

#[cfg(feature = "accesskit-tree")]
pub use accessibility_tree::AccessTreeProjector;
pub use command::{
    ActionDescriptor, ActionId, ActionMatch, ActionPickerNavigation, ActionPickerSelection,
    ActionPickerState, ActionRegistry, ActionRegistryError, ContextPredicate, KeyBinding,
    KeyContext, KeyModifiers, KeyStroke, Keymap, KeymapMatch, KeymapState,
    action_picker_from_key_name,
};
pub use component_support::{
    ComponentCapability, ComponentFamily, ComponentId, ComponentSupport, component_catalog,
    component_ids, component_support, component_uses_runtime,
};
#[cfg(feature = "gpu")]
pub use default_gpu_view::{
    DefaultGpuViewRenderer, default_scene_gpu_renderers, resolve_scene_gpu_renderers,
};
pub use dialog::{DialogClosePolicy, DialogCloseTrigger, DialogSize};
#[cfg(feature = "gpu")]
pub use font_face_ingest::{HostFontFaceSpec, ingest_host_font_faces};
#[cfg(feature = "gpu")]
pub use frame_binding::FrameBinding;
pub use geometry::{LogicalPoint, LogicalRect, PhysicalRect, RegionRect, WorkspaceGeometry};
#[cfg(feature = "gpu")]
pub use gpu_texture::{
    HostTexture, HostTextureAlphaMode, HostTextureBinding, HostTextureRegistry, TextureSlot,
    TextureSubscription,
};
#[cfg(feature = "gpu")]
pub use gpu_view::RenderSlot;
#[cfg(feature = "gpu")]
pub use gpu_work::{GpuStageTimings, GpuWorkSink};
#[cfg(feature = "graph-canvas")]
pub use graph::{
    GRAPH_EDGE_HIT_TOLERANCE, GRAPH_MAX_ZOOM, GRAPH_MIN_ZOOM, GRAPH_NODE_TITLE_HEIGHT,
    GRAPH_PORT_HIT_RADIUS, GRAPH_PORT_INSET, GRAPH_PORT_PITCH, GraphCanvasId, GraphEdge,
    GraphEdgeId, GraphEndpoint, GraphModel, GraphModelError, GraphNode, GraphNodeId, GraphPoint,
    GraphPort, GraphPortId, GraphPortKind, GraphPortSide, GraphRect, GraphSelection, GraphSize,
    GraphTarget, GraphTargetDescriptor, GraphTargetId, GraphTargetKind, GraphViewport,
    graph_node_fitted_height,
};
#[cfg(all(feature = "hosted", feature = "wgpu-interop"))]
pub use hosted_context::HostedSurfaceFrame;
#[cfg(feature = "hosted")]
pub use hosted_context::{
    HostedGpuContext, HostedGpuError, HostedGpuShared, HostedGpuSurface, HostedRunError,
    HostedSurfaceMode,
};
pub use icons::Icon;
pub use layout::{
    NarrowBehavior, RegionId, RegionPlacement, RegionRole, RegionScope, RegionState,
    WorkspaceLayout, WorkspaceLayoutError,
};
pub use menu::{MenuConfirmation, MenuSelection};
#[cfg(feature = "hosted")]
pub use nana_app_icon::{default_window_icon, window_icon_from_png};
/// Producer half of the latest-frame exchange; [`FrameBinding`] is the consumer.
#[cfg(feature = "gpu")]
pub use nana_frame_exchange::{
    self as frame_exchange, CopyOutcome, DEFAULT_CAPACITY, FrameExchange, FrameExchangeStats,
    FrameInbox, FrameLease, FrameToken,
};
/// The GPU backend contract: the device, frames and textures every renderer,
/// producer and host works with. See the `nana-gpu` crate.
#[cfg(feature = "gpu")]
pub use nana_gpu::{
    DeviceGeneration, FrameContext, FrameId, GpuBackend, GpuCapabilities, GpuCapability,
    GpuCapabilityOutcome, GpuContext, GpuDeviceLost, GpuDeviceType, GpuError, GpuFeatureSet,
    GpuLimits, GpuLossReason, GpuRenderTarget, GpuResourceGroup, GpuResourceLayout, GpuSubmission,
    GpuTexture, GpuTextureDescriptor, GpuTextureFormat, GpuTextureRegion, GpuTextureUsages,
    LogicalBinding, LogicalBindingType, LogicalResource, ResourceBinding, ResourceClass,
    ResourceSet, ResourceTable, RetainedWrites, ShaderInterface, ShaderStage, VertexAttribute,
};
/// The explicit WGPU escape hatch (feature `wgpu-interop`): the exact `wgpu`
/// the framework links, and the raw objects behind [`GpuContext`]. Hosts that
/// bring their own device and renderers that record their own pipelines
/// import through this re-export; a direct `wgpu` dependency silently resolves
/// to a second copy when the framework moves to a new major version.
#[cfg(feature = "wgpu-interop")]
pub use nana_gpu::{WgpuInterop, wgpu};
/// Full generated Tabler catalog (`icons_tabler::USER`, `::KEYBOARD`, …) as
/// typed [`Icon`] constants, behind the `icons-tabler` feature. The built-in
/// catalog only covers shell chrome; reach here before hand-authoring
/// [`IconData`]. The linker drops every constant the product never names.
#[cfg(feature = "icons-tabler")]
pub use nana_icons_tabler as icons_tabler;
#[cfg(feature = "bundled-fonts")]
pub use nana_text::use_hermetic_fonts;
pub use nana_text::{
    HostFontError, HostFontStyle, NanaTextShaper, alias_host_font_face_local,
    register_host_font_bytes, register_host_font_face, register_host_font_face_styled,
    register_host_font_file, set_sans_serif_family, shaped_face_families,
};
pub use nana_ui_core::ContentFit;
pub use nana_ui_core::ControlSize;
#[cfg(feature = "gpu")]
pub use nana_ui_core::GpuWorkObservation;
pub use nana_ui_core::ImageSampling;
pub use nana_ui_core::{AnchoredMenuPlacement, StatusTone, ToastTone, ValidationIntent};
pub use nana_ui_core::{AppearanceEvent, CommandPaletteEvent, CommandPaletteItem};
pub use nana_ui_core::{DrawerSide, PopoverAlignment, PopoverPlacement};
pub use nana_ui_core::{
    ExpansionState, SplitPaneModel, SplitPaneMutation, WORKSPACE_REGION_TRANSITION_DURATION,
    WorkspaceModel, WorkspaceMutation,
};
pub use nana_ui_core::{XYPadEvent, XYPadValue};
/// Canonical input: what hosts deliver and programs observe in
/// [`RoutedInput`], routed through the context each source is bound to.
pub use nana_ui_platform::{
    CanonicalInputEvent, CommittedText, CompositionInput, CursorIcon, FileDragInput, FileDragKind,
    HeadlessHostServices, HostServiceError, HostServices, InputDisposition, InputModifiers,
    InputPayload, KeyInput, KeyState, PointerInput, PointerPhase, PointerType, TextInputContext,
    TextInputPurpose, WheelInput, WheelUnit,
};
#[cfg(feature = "hosted")]
pub use nana_ui_platform::{
    DisplayId, DisplayInfo, FullscreenMode, FullscreenRequest, MousePassthroughMode, WindowIcon,
    WindowIconError, WindowModeState, clear_registered_application_icon, register_application_icon,
};
/// Fetch host boundary, re-exported so hosts can supply
/// [`SceneWgpuPainter::set_resource_fetch_host`] without depending on
/// `nana-ui-platform` directly.
#[cfg(feature = "gpu")]
pub use nana_ui_platform::{
    FetchCancellation, FetchError, FetchErrorKind, FetchHost, FetchPolicy, FetchRequest,
    FetchResponse, NativeFetchHost, SharedFetchHost, shared_fetch_host,
};
pub use nana_ui_runtime::{
    AccessibilityActionRequest, AccessibilityNode, AccessibilityRole, AccessibilityUpdate,
};
pub use nana_ui_runtime::{
    HeadlessInput, InputBindError, InputCounters, InputRouteError, InputRouteOutcome, RoutedEvent,
};
#[cfg(feature = "hosted")]
pub use nana_window::apply_hosted_system_material;
#[cfg(feature = "hosted")]
pub use nana_window::{
    Appearance as WindowAppearance, FallbackColor, MaterialEffect, MaterialFallback,
    MaterialOutcome, Menu, MenuBar, MenuBarSupport, MenuEntry, MenuShortcut,
    PlatformMaterialSupport, apply_system_material, clear_system_material,
    hosted_platform_material_support, install_menu_bar, menu_bar_support,
    platform_material_support, take_menu_activations,
};
pub use overlay::ExclusiveOverlay;
pub use pane::ratio_pane_split;
pub use runtime_animation::RuntimeAnimationClock;
#[cfg(feature = "hosted")]
pub use runtime_dock::{dock_workspace_window_id, runtime_dock_window_update};
#[cfg(feature = "hosted")]
pub use runtime_host::{
    FrameDemand, HostFailure, ReportHostFailure, RoutedInput, RuntimeProgram,
    RuntimeProgramContext, RuntimeProgramUpdate, RuntimeRedraw, RuntimeTaskError, run_runtime,
    run_runtime_with_store, with_startup,
};
#[cfg(feature = "gpu")]
pub use scene_gpu::{
    PreparedSceneResources, SceneGpuBatchNode, SceneGpuBatchPassContext, SceneGpuNode,
    SceneGpuPassContext, SceneGpuPrepareContext, SceneGpuRenderContext, SceneGpuRenderer,
    SceneGpuRendererRegistry, ScenePass, SceneResourceEncodeContext, SceneResourceProduceError,
    SceneResourceProducer, SceneResourceProducerRegistry,
};
#[cfg(feature = "hosted")]
pub use scene_host::run_runtime_scene;
#[cfg(feature = "gpu")]
pub use scene_paint::{
    AlphaEncoding, HostTextureSceneResolver, RenderTargetId, ScenePaintError, ScenePaintViewport,
    SceneWgpuPainter, SubpixelOrder, TextGlyphCounters, resolve_background_image_url,
    set_background_image_url_base,
};
pub use selection::{SelectionMove, SingleSelection};
pub use settings::{
    AppearanceSettings, BackdropTarget, SettingsError, SettingsModel, SettingsState, SettingsTab,
    SettingsTabId, WindowMaterialMode,
};
#[cfg(feature = "hosted")]
pub use settings::{hosted_window_material_modes, window_material_effect};
pub use split_pane::{SplitAxis, SplitPaneAction, SplitPaneController};
pub use theme::{
    Color, HAIRLINE, SemanticColor, SemanticPalette, ThemeMetrics, ThemeMode, ThemeModeExt,
    ThemeTokens, UI_BASE_TEXT_SIZE, UI_METRICS, install_theme_tokens, space, type_scale,
};
#[cfg(feature = "bundled-fonts")]
pub use theme::{UI_FONT_BOLD, UI_FONT_MEDIUM, UI_FONT_REGULAR, UI_FONT_SEMIBOLD, ui_font_sources};
pub use tooltip::{TooltipConfig, TooltipPlacement};
pub use virtual_list::{
    TableColumn, TableCursor, TableNavigation, VirtualAlignment, VirtualFrozenWindow,
    VirtualListLayout, VirtualListMaterialization, VirtualListMaterializationError,
    VirtualListMaterializer, VirtualListMount, VirtualListWindow, VirtualScrollAnchor,
    VirtualTableFrozenWindow, VirtualTableLayout, VirtualTableMaterialization,
    VirtualTableMaterializer, VirtualTableWindow, VirtualTreeLayout, VirtualTreeRow,
    VirtualTreeWindow, VirtualViewport,
};
pub use widgets::{ButtonKind, ButtonPaintOverride, CardKind};
pub use window_chrome::{
    TitleBarDragTracker, WindowChrome, WindowChromeAction, WindowChromeEvent, WindowChromeState,
    WindowControlMode, apply_title_bar_pointer, title_bar_hits_window_control,
    window_commands_for_chrome_action,
};
pub use workspace::{WorkspaceAction, WorkspaceController};

pub use nana_ui_scene::DocumentAccessError;

pub use nana_ui_core::{
    AppSettings, FileDialogError, FileDialogKind, FileDialogRequest, FileDialogResult, FileFilter,
    KEY_APPEARANCE_PREFIX, KEY_DOCK_PREFIX, KEY_WINDOW_PREFIX, KvBackend, LocalStorageAdapter,
    MemoryStore, RestorationKey, RestorationPath, RestorationScopeId, SharedStore, StoreError,
    ViewStateSchemaVersion, ViewStateStore, appearance_storage_key, dock_storage_key,
    is_framework_storage_key, memory_store, shared_store, window_storage_key,
};
pub use nana_ui_platform::{
    ApplicationIdentity, ApplicationLocation, ApplicationPaths, FileStore, PathsError,
    PersistedWindowGeometry, PersistenceCoordinator, PersistenceWork, RuntimeLayout, app_data_dir,
    persist_live_window_geometry, restore_window_geometry,
};

#[cfg(feature = "hosted")]
mod window_service;
#[cfg(feature = "hosted")]
mod window_shadow;
#[cfg(feature = "hosted")]
pub use window_service::{
    WindowCapture, WindowCursor, WindowDescriptor, WindowEffects, WindowError, WindowHandle,
    WindowLevel, WindowRequest, WindowService, WindowSurfacePreference,
};

/// Native event-loop integration for advanced hosts.
#[cfg(feature = "hosted")]
pub mod platform_host {
    pub use crate::scene_host::EmbeddedRuntime;
}

pub use nana_ui_platform::{
    WindowShadow, WindowShadowBackend, WindowShadowFallback, WindowShadowOutcome,
    WindowShadowSource, WindowShadowStyle, WindowShadowWork, WindowVisualShape,
};

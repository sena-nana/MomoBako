//! Backend-neutral retained runtime for NanaUI.
//!
//! Applications and compatibility adapters use stable Nana IDs. The internal
//! node table is deliberately not part of the public contract, so changing
//! storage cannot invalidate JS handles, diagnostics, snapshots, or persisted
//! data.

#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

mod animation;
mod avatar;
mod bars;
mod breadcrumb;
mod builtin_components;
#[cfg(feature = "calendar")]
mod calendar;
#[cfg(feature = "charts")]
mod charts;
mod chip;
mod color_field;
mod command_palette;
pub mod component_descriptors;
mod component_registry;
mod components;
mod custom_paint;
mod paint_script;
pub use paint_script::PaintScript;
mod date_picker;
mod diff_view;
mod dock;
mod dropdown;
mod feedback;
mod file_tab;
mod form_surfaces;
mod framework;
mod glyph_cache;
mod gpu_slots;
#[cfg(feature = "graph-canvas")]
mod graph_canvas;
#[cfg(feature = "graph-canvas")]
mod graph_minimap;
mod hover_card;
mod json_num;
pub(crate) use json_num::json_u64;
#[cfg(feature = "image-viewer")]
mod image_viewer;
mod key_layers;
mod layout_engine;
/// Benchmark-only view of whether scoped layout actually reused its cached
/// container placements. Hosts use it to tell "the dirty set is small" apart
/// from "the frame was cheap".
#[cfg(feature = "benchmark")]
pub use layout_engine::plan_stats;
mod browser_view;
mod menus;
mod motion_api;
mod mutation;
mod native_content;
pub use browser_view::BrowserView;
#[cfg(feature = "rich-text")]
mod markdown_drawing;
mod media_transport;
mod overlay_surfaces;
mod overlay_visibility;
mod pane;
mod pane_section;
mod panel;
mod path_field;
mod placeholders;
mod popover;
mod presentation;
mod profiler;
mod qr_code;
mod query;
#[cfg(feature = "controls")]
mod reorder_list;
#[cfg(feature = "rich-text")]
pub use markdown_drawing::{MarkdownDrawing, MarkdownDrawingCommand};
#[cfg(feature = "rich-text")]
mod rich_text;
mod schedule;
mod search_dropdown;
mod select;
mod selection;
mod settings;
mod shell;
mod sidebar;
mod snippet;
mod split_pane;
pub use snippet::{
    SnippetExpansion, SnippetPlaceholder, SnippetTransform, expand_text_snippet,
    expand_text_snippet_with_variables,
};
mod store;
mod tabs;
mod terminal;
mod text_editing;
mod text_engine_shaper;
mod text_input_limit;
mod text_layout_cache;
mod text_node;
#[cfg(any(test, feature = "benchmark"))]
pub mod text_shape_stats;
mod text_width;
mod thumbnail;
mod toast;
mod tree_view;
mod video;
pub mod view;
mod view_components;
mod workspace;
mod world;
mod xy_pad;

pub use animation::{
    AnimatableProperty, AnimationClass, AnimationDirection, AnimationEvent, AnimationEventKind,
    AnimationFillMode, AnimationFrame, AnimationId, AnimationIteration, AnimationPlayState,
    AnimationPlayback, AnimationSample, AnimationSpec, CompiledMotion, DecayParams, Easing,
    FlipRect, Keyframe, MOTION_DESCRIPTOR_VERSION, MotionCodecError, MotionCodecId,
    MotionCodecInfo, MotionCodecRegistry, MotionCurve, MotionDescriptor, MotionDescriptorError,
    MotionDescriptorStore, MotionEvaluatorBackend, MotionGraph, MotionHandle, MotionInspectorEntry,
    MotionInterrupt, MotionLayer, MotionSample, MotionTargetId, MotionTiming, MotionTo,
    MotionTrack, MotionTrackId, MotionValue, MotionValueKind, MotionWorkCounters,
    PresentationOverlay, PresentationPair, PresentationSlot, PresentationStore, Spring,
    SpringParams, StepJump, Timeline, classify_animatable_property, compile_motion_descriptor,
    component_animation_id, component_animation_kinds, cpu_fallback_reason, decode_motion_track,
    evaluate_descriptor, evaluate_progress, evaluate_track, evaluate_track_at,
    invert_flip_translate, is_font_variation_settings, layout_flip_hold_spec,
    layout_flip_play_spec, layout_flip_spec, loading_animation, retarget_track,
    track_completion_deadline, workspace_animation,
};
pub use avatar::{Avatar, DEFAULT_SIZE as AVATAR_DEFAULT_SIZE};
pub use bars::{StatusBar, Toolbar};
pub use breadcrumb::{
    Breadcrumb, BreadcrumbEvent, BreadcrumbItem, BreadcrumbSegment, BreadcrumbTone,
};
pub use builtin_components::NanaBuiltinComponents;
#[cfg(feature = "calendar")]
pub use calendar::{
    CalendarHeatmap, CalendarHeatmapActiveCell, CalendarHeatmapCell, CalendarHeatmapCellPaint,
    CalendarHeatmapDatum, CalendarHeatmapDayLabel, CalendarHeatmapEvent, CalendarHeatmapLabelPaint,
    CalendarHeatmapModel, CalendarHeatmapMonthLabel, CalendarHeatmapOptions, CalendarLevelResolver,
    CalendarLevelStrategy, CalendarMonthFormatter, CalendarTitleFormatter,
    build_calendar_heatmap_model, calendar_cell_fill,
};
#[cfg(feature = "charts")]
pub use charts::{
    DonutChart, DonutSlice, TimeSeriesChart, TimeSeriesLayer, TimeSeriesPaint, time_series_paint,
};
pub use chip::{Chip, ChipDismissed};
pub use color_field::{
    ColorChanged, ColorField, ColorInput, format_hex, hsv_to_rgb, parse_hex, rgb_to_hsv,
    sanitize_rgba,
};
pub use command_palette::CommandPalette;
pub use component_registry::{
    ComponentBindKind, ComponentRegistry, ComponentTypeId, PreparedSemanticBinding,
    RegisterableComponent, SemanticOption, SemanticSpec, normalize_tag,
};
pub use components::{
    AccessibilityAction, AccessibilityActionRequest, AccessibilityDelta, AccessibilityNode,
    AccessibilityRole, AccessibilityState, AccessibilityUpdate, CalendarHoverGeometry,
    ComponentElevation, ComponentGeometry, ComponentTextRegion, ComponentTriggerSurface,
    ComputedStyle, CustomRenderNode, DocumentTextSelection, DropHoverOverlay, EventListeners,
    EventRoute, ExtractedCompositor, ExtractedNode, ExtractedTextSpan, ImeComposition, ImeView,
    InteractionState, InteractionStyle, LayoutBox, LayoutInput, LineLabel, MeasureTextShaper,
    MenuSurfaceKind, ModalLayoutInput, MountState, NodeStyle, NumberSteppers, OverlayHostState,
    PointerCaptureChange, RadioIndicator, ScrollMetrics, ScrollOffset, ScrollbarBar,
    SelectMenuGeometry, SelectOptionData, SelectOptionGeometry, SemanticPaint, StandardVisual,
    TextAffinity, TextAtomChip, TextAtomClosed, TextAtomSpan, TextCodeFold, TextColorSwatchSpan,
    TextCompletion, TextCompletionEdit, TextCompletionPopup, TextCompletionRow,
    TextCompletionSnapshot, TextContent, TextDiagnosticSeverity, TextDiagnosticSpan,
    TextEditorRenderOptions, TextFoldGeometry, TextFoldGutter, TextGitGutterGeometry, TextGitMark,
    TextGitMarkKind, TextHit, TextHorizontalAlignment, TextHover, TextHoverPopup, TextInlay,
    TextInputScroll, TextInputState, TextInputView, TextMatchMarker, TextMatchSpan, TextMetrics,
    TextMinimapGeometry, TextSelection, TextShapeConstraints, TextShaper, TextShaping,
    TextSignatureHelp, TextSignaturePopup, TextSnippet, TextStickyLineGeometry, TextValue,
    TextVerticalAlignment, TextWhitespaceKind, TooltipVisual, TriggeredMenuOverlay,
};
pub use custom_paint::{
    AFFINE_IDENTITY, Affine, BlendMode, BoxPaint, ColorStop, CornerRadii, FillRule, Gradient,
    GradientExtend, GradientShape, ImageFit, LineCap, LineJoin, NodePainter, Paint, PaintColor,
    PaintContext, PaintOp, PaintPath, PaintPhase, PaintRecording, PaintShadow, PaintState,
    PaintText, Painter, PathVerb, Radius, ResolvedGradient, ResolvedPaint, StrokeStyle, TextSize,
    affine_multiply,
};
pub use date_picker::{DateChanged, DateCursorMoved, DatePicker, DatePickerSlots};
pub use diff_view::{DiffEvent, DiffHunk, DiffLayout, DiffLine, DiffLineKind, DiffView};
pub use dock::{
    DOCK_DIVIDER_HIT_SIZE, DOCK_SPLIT_KEYBOARD_STEP, Dock, DockAxis, DockBoundsPersist,
    DockCommand, DockCommandOutcome, DockDropZone, DockFloatingPersist, DockFloatingSurface,
    DockItemLimits, DockNode, DockNodePersist, DockPanel, DockSurfaceSpec, DockWorkspace,
    DockWorkspaceEvent, DockWorkspacePersist, MAIN_SURFACE_ID, MAX_SPLIT_RATIO, MIN_SPLIT_RATIO,
    clamp_ratio, dock_nudge_split_ratio, dock_split_child_lengths, dock_split_ratio_from_pointer,
    dock_surface_window_key,
};
pub use dropdown::{Dropdown, DropdownOption};
pub use feedback::{
    EmptyState, LabeledValue, Progress, ProgressCancelled, Spinner, StatusBadge, ValidationMessage,
    ValueEmphasis,
};
pub use file_tab::{FileTab, FileTabEvent};
pub use form_surfaces::{FormField, InteractiveCard};
pub use framework::MountedView;
pub(crate) use framework::UiBuilder;
pub use framework::{
    ASSEMBLY_PATH_SEPARATOR, ActiveRuntimeOverlay, AppContext, AssemblyScope, BuiltinComponents,
    Entity, ExtensionRegistrar, FormValidity, FrameworkError, HeadlessInput, InputBindError,
    InputCounters, InputRouteError, InputRouteOutcome, OverlayKey, OverlayPointerDecision,
    OverlayPointerPhase, RoutedEvent, RuntimeOverlayKind, Subscription, Task, TextDeleteKind,
    TextEditOrigin, TextFindScope, UiExtension, View, ViewContext, VirtualListItems,
    VirtualTableItems, VirtualTreeItems,
};
#[doc(hidden)]
pub use framework::{EditableHooks, TypeBehavior, TypeHooks};
pub use glyph_cache::GlyphCache;
pub use gpu_slots::{
    GPU_TEXTURE_VIEW_RENDERER, GPU_VIEW_RENDERER, GpuTextureView, GpuView, GpuViewMode,
    GpuViewPalette, HOST_TEXTURE_RENDERER, gpu_view_params, pack_gpu_revision, unpack_gpu_revision,
};
#[cfg(feature = "graph-canvas")]
pub use graph_canvas::{
    GRAPH_CANVAS_RENDERER, GraphCanvas, GraphCanvasAdjustment, GraphCanvasEvent, GraphCanvasHit,
    GraphEdgePaint, GraphInteraction, GraphNodeContent, GraphNodePaint, GraphPointerButton,
    GraphPortPaint, GraphScrollDelta, wheel_zoom_factor,
};
#[cfg(feature = "graph-canvas")]
pub use graph_minimap::{GraphMinimap, GraphMinimapDrag, GraphMinimapEvent};
pub use hover_card::HoverCard;
#[cfg(feature = "image-viewer")]
pub use image_viewer::{
    ImageViewer, ImageViewerContent, ImageViewerDrag, ImageViewerEvent, ImageViewerGeometry,
    ImageViewerHit, ImageViewerOffset, ImageViewerPosition, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP,
};
pub use key_layers::{
    ActionDescriptor, ActionMatch, ActionRegistry, ActionRegistryError, CapturedStroke, KeyBinding,
    KeyCaptureEvent, KeyCaptureLayer, KeyInput, KeyModifiers, Keymap, KeymapLayer, KeymapMatch,
    KeymapState,
};
pub use layout_engine::{
    LayoutViewport, RetainedLayoutCache, RuntimeLayoutEngine, StyleLayoutNode,
};
pub use media_transport::{
    MediaTransportBar, MediaTransportDensity, MediaTransportEvent, MediaTransportIcons,
    MediaTransportPlacement, MediaTransportSlots, media_clock,
};
pub use menus::{
    ActionMenuItem, AnchoredActionMenu, ContextMenu, ContextMenuEvent, ContextMenuItem,
    resolve_anchored_origin,
};
pub use motion_api::{FlipBuilder, NodeMotion, SpringBuilder, TimelineBuilder, TransitionBuilder};
pub use mutation::{MutationQueue, UiMutation};
pub use nana_ui_core::{
    ActionId, ActionPickerNavigation, AlignSpec, ClipCircle, ClipEllipse, ClipInset, ClipPath,
    ClipPoint, ClipShapeRadius, CommandPaletteEvent, CommandPaletteItem, CompiledTheme, ContentFit,
    ContextPredicate, CursorSpec, DirSpec, DropAccepts, DropEffect, DropKind, DropdownEvent,
    DropdownSelection, ElevationRole, FileDragKind, FlexDirection, FlexWrap, FontFeatureSetting,
    FontKerningSpec, FontVariationSetting, FrameStage, GRAPH_EDGE_HIT_TOLERANCE, GRAPH_MAX_ZOOM,
    GRAPH_MIN_ZOOM, GRAPH_NODE_TITLE_HEIGHT, GRAPH_PORT_HIT_RADIUS, GRAPH_PORT_INSET,
    GRAPH_PORT_PITCH, GpuWorkObservation, GraphCanvasId, GraphEdge, GraphEdgeId, GraphEndpoint,
    GraphModel, GraphModelError, GraphNode, GraphNodeId, GraphPoint, GraphPort, GraphPortId,
    GraphPortKind, GraphPortSide, GraphRect, GraphSelection, GraphSize, GraphTarget,
    GraphTargetDescriptor, GraphTargetId, GraphTargetKind, GraphViewport, Icon, IconData,
    IconPathCommand, IconShape, ImageSampling, JustifySpec, KeyContext, LayoutStyle, LengthSpec,
    LineBreakSpec, LineHeightSpec, OverflowSpec, PaintTransform, PhysicalEdge, PointerEventsSpec,
    PopoverAlignment, PopoverPlacement, PositionSpec, RadiusTier, SemanticColorMix,
    SemanticColorRole, StatusTone, TITLE_BAR_HEIGHT, TabDragGroup, TabDragLease, TabDragRect,
    TabDragSurface, TabDropIndicator, TabStripPaint, TableCursor, TableNavigation, TextAlignSpec,
    TextShadowSpec, ThemeMode, ThemeWorkCounters, TransformOrigin, TreeNavigation, TreeNode,
    TreeViewEvent, UserSelectSpec, ValidationIntent, VirtualAlignment, VirtualFrozenWindow,
    VirtualListLayout, VirtualListMaterialization, VirtualListMaterializationError,
    VirtualListMaterializer, VirtualListMount, VirtualListWindow, VirtualScrollAnchor,
    VirtualTableFrozenWindow, VirtualTableLayout, VirtualTableMaterialization,
    VirtualTableMaterializer, VirtualTableWindow, VirtualTreeLayout, VirtualTreeRow,
    VirtualTreeWindow, VirtualViewport, VisibilitySpec, WINDOW_CONTROL_GAP, WINDOW_CONTROL_WIDTH,
    WhiteSpaceSpec, WordBreakSpec, WorkCounters, WritingContext, WritingModeSpec,
    custom_window_controls_width, graph_node_fitted_height, port_tangent, space,
    tree_navigation_event, type_scale,
};
pub use overlay_surfaces::{
    ConfirmDialog, ConfirmIntent, ConfirmSlots, Drawer, ModalBehavior, ModalInitialFocus,
    ModalSlots, ModalSurface, ModalSurfaceKind,
};
pub use overlay_visibility::{
    OVERLAY_IDLE, OverlayLocks, OverlayVisibility, OverlayVisibilityConfig,
};
pub use pane::{
    PaneChrome, PaneChromeAction, PaneChromeActionKind, PaneSlot, PaneTree, PaneTreeNode,
};
pub use pane_section::PaneSection;
pub use path_field::{BrowseRequested, PathField};
pub use placeholders::{LevelMeter, Skeleton};
pub use popover::{
    ActionMenu, MENU_OVERLAY_Z_INDEX, Popover, PopoverClosed, PopoverToggled,
    resolve_popover_origin,
};
pub use presentation::{
    HIGHLIGHT_PRESENTER, HighlightRequest, TextPresentation, TextPresenter, TextSpan,
};
#[cfg(feature = "syntax-highlighting")]
pub use presentation::{HighlightPresentation, SyntectHighlighter};
pub use profiler::{FrameProfile, FrameProfiler, StageStatus, StageTiming};
pub use qr_code::{QrCode, QrCodeError};
#[cfg(feature = "controls")]
pub use reorder_list::{
    ReorderItem, ReorderList, ReorderListEvent, ReorderListPointer, ReorderRowPaint,
    TreeDropIntent, TreeDropPosition,
};
#[cfg(feature = "rich-text")]
pub use rich_text::{
    MarkdownBlock, MarkdownBlockKind, MarkdownImage, MarkdownImageResource, MarkdownSpan,
    MarkdownTable, MarkdownTableAlignment, NativeMarkdown, RichSpan, RichTextEvent,
    SelectableRichText, TextSelectionGroup, TextSelectionGroupId, TextSelectionSnapshot,
};
pub use schedule::SystemWork;
pub use search_dropdown::{SearchDropdown, SearchDropdownEvent, SearchDropdownOption};
pub use select::{Select, SelectChanged, SelectOption};
pub use selection::{
    RovingFocusIntent, RovingFocusPolicy, SegmentedControl, SegmentedOption, SegmentedOptionChosen,
    SegmentedSelectionRequested, SelectionChrome, SelectionOrientation,
};
pub use settings::{
    AboutMetadata, AboutSection, AboutSectionAssembly, AppearanceSection,
    AppearanceSectionAssembly, SettingsBack, SettingsCard, SettingsCollapsibleCard, SettingsPage,
    SettingsPageAssembly, SettingsRow, SettingsSidebar, SettingsSidebarAssembly,
    SettingsTabSelected, apply_appearance_event,
};
pub use shell::{AppShell, AppTitleBar, AppTitleBarControls, DesktopShell, WindowChromeAction};
pub use sidebar::{
    SidebarFooter, SidebarFooterButton, SidebarFrame, SidebarRow, SidebarRowIcon, SidebarRowState,
    SidebarRowTone, SidebarSection, SidebarSectionSlots, SidebarSectionState,
    sidebar_row_depth_inset, sidebar_row_tool_button, sidebar_section_tool_button,
    sidebar_top_bar_tool_button,
};
pub use split_pane::SplitPane;
pub use tabs::{TabOption, Tabs, TabsEvent};
pub use terminal::{
    MAX_TERMINAL_CELLS, TerminalCell, TerminalCursor, TerminalCursorShape, TerminalEvent,
    TerminalPosition, TerminalScreen, TerminalSelection, TerminalView,
};
pub use text_editing::{
    TextCaretIntent, TextLineDirection, TextSearchOptions, expanded_selection, find_matches,
    find_matches_capped, find_matches_in_range, find_next_match, find_previous_match,
    matching_bracket_pair, preserve_case_replacement, replace_all_matches,
    replace_all_matches_in_range, sort_lines,
};
pub use text_engine_shaper::NanaTextEngineShaper;
pub use text_node::{RetainedTextLayout, TextDirty, TextRevisions, TextWork, nana_font_family};
pub use thumbnail::{DEFAULT_ASPECT as THUMBNAIL_DEFAULT_ASPECT, Thumbnail, ThumbnailState};
pub use toast::{Toast, ToastDismissed, ToastTone};
pub use tree_view::TreeView;
pub use video::Video;
pub use view_components::{
    Activate, Button, Card, Checkbox, CodeEditing, ComponentView, Dialog, Divider, FileDropEvent,
    HostedTextarea, IconButton, IconButtonTooltip, IconGlyph, List, ListItem, ListItemSlots,
    NumberChanged, NumberInput, OverlayChanged, OverlayClosing, OverlayHost, RangeAdjustment,
    RangeChanged, RangeDragState, RangeDragging, RangeField, RangeInput, ScrollAnchor, ScrollAxes,
    ScrollChanged, ScrollLaidOut, ScrollView, ScrollViewportChanged, ScrollbarDragState,
    SecondaryPress, SizeChanged, Stack, Switch, Table, TableCell, TableCellFocused, TableRow, Text,
    TextArea, TextChanged, TextClamped, TextInput, TextSubmitted, ToggleChanged, Tooltip,
    UserScroll,
};
pub use workspace::{Workspace, WorkspaceRegionSlot, WorkspaceResizeHandle};
pub use world::{
    BuildIdHasher, CommitReport, DocumentId, IdHasher, InputModality, NodeKind, NodeMap, NodeSet,
    NodeSnapshot, StableNodeId, UiWorld, UiWorldError,
};
pub use xy_pad::{
    XYPad, XYPadAdjustment, XYPadAxisLock, XYPadDragState, XYPadEvent, XYPadValue, xy_pad_height,
};

pub use native_content::{NATIVE_CONTENT_RENDERER, NativeContent};
pub use panel::{Panel, PanelEdge, PanelInsets};

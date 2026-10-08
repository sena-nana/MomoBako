//! Retained terminal grid. Hosts supply parsed cells and own the PTY.

use std::sync::Arc;

use nana_ui_core::{
    LengthSpec, LineHeightSpec, OverflowSpec, PositionSpec, SemanticColorRole, TextDecorationLine,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::component_registry::{RegisterableComponent, SemanticSpec};
use crate::json_u64;
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, AppContext, ComponentView, DocumentId, Entity,
    FrameworkError, InteractionState, LayoutBox, MutationQueue, NodeKind, NodeStyle, StableNodeId,
    Text, UiWorld,
};

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalCell {
    pub text: Arc<str>,
    /// Zero is the continuation of a wide cell; otherwise one or two columns.
    pub width: u8,
    pub foreground: Option<[f32; 4]>,
    pub background: Option<[f32; 4]>,
    pub bold: bool,
    pub underline: bool,
    pub dim: bool,
    pub italic: bool,
    pub inverse: bool,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            text: Arc::from(" "),
            width: 1,
            foreground: None,
            background: None,
            bold: false,
            underline: false,
            dim: false,
            italic: false,
            inverse: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TerminalPosition {
    pub row: u16,
    pub column: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalCursorShape {
    Block,
    Bar,
    Underline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalCursor {
    pub position: TerminalPosition,
    pub shape: TerminalCursorShape,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalScreen {
    pub columns: u16,
    pub rows: u16,
    pub cells: Arc<[TerminalCell]>,
    pub cursor: Option<TerminalCursor>,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
}

pub const MAX_TERMINAL_CELLS: usize = 2_000_000;

impl TerminalScreen {
    pub fn blank(columns: u16, rows: u16) -> Self {
        Self {
            columns,
            rows,
            cells: if usize::from(columns) * usize::from(rows) <= MAX_TERMINAL_CELLS {
                vec![TerminalCell::default(); usize::from(columns) * usize::from(rows)].into()
            } else {
                Arc::from([])
            },
            cursor: None,
            application_cursor: false,
            bracketed_paste: false,
        }
    }

    fn valid(&self) -> bool {
        self.columns > 0
            && self.rows > 0
            && self.cells.len() <= MAX_TERMINAL_CELLS
            && self.cells.len() == usize::from(self.columns) * usize::from(self.rows)
            && self
                .cells
                .iter()
                .all(|cell| cell.width <= 2 && cell.text.len() <= 256)
    }

    fn index(&self, position: TerminalPosition) -> usize {
        usize::from(position.row.min(self.rows.saturating_sub(1))) * usize::from(self.columns)
            + usize::from(position.column.min(self.columns.saturating_sub(1)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSelection {
    pub anchor: TerminalPosition,
    pub focus: TerminalPosition,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TerminalEvent {
    Input(Vec<u8>),
    Resize { columns: u16, rows: u16 },
    SelectionChanged(Option<TerminalSelection>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalView {
    pub screen: TerminalScreen,
    pub selection: Option<TerminalSelection>,
    pub cell_width: f32,
    pub cell_height: f32,
    pub font_size: f32,
    pub font_family: String,
    pub disabled: bool,
    pub read_only: bool,
    pub style: NodeStyle,
    preedit: String,
    dragging: Option<u64>,
    viewport: Option<(u16, u16)>,
}

impl TerminalView {
    pub const CELL_WIDTH: f32 = nana_ui_core::space::MD;
    pub const CELL_HEIGHT: f32 = nana_ui_core::type_scale::LINE_TALL;
    pub const FONT_SIZE: f32 = nana_ui_core::type_scale::SECTION;

    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn new(screen: TerminalScreen) -> Self {
        let mut style = NodeStyle::default().surface(SemanticColorRole::Surface);
        let layout = Arc::make_mut(&mut style.layout);
        layout.position = PositionSpec::Relative;
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Fill);
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.overflow_x = OverflowSpec::Hidden;
        layout.overflow_y = OverflowSpec::Hidden;
        Self {
            screen,
            selection: None,
            cell_width: Self::CELL_WIDTH,
            cell_height: Self::CELL_HEIGHT,
            font_size: Self::FONT_SIZE,
            font_family: "Cascadia Mono".to_owned(),
            disabled: false,
            read_only: false,
            style,
            preedit: String::new(),
            dragging: None,
            viewport: None,
        }
    }

    pub fn selected_text(&self) -> String {
        if !self.screen.valid() {
            return String::new();
        }
        let Some(selection) = self.selection else {
            return String::new();
        };
        let (start, end) = self.selection_indices(selection);
        let mut output = String::new();
        for index in start..=end {
            if index > start && index % usize::from(self.screen.columns) == 0 {
                while output.ends_with(' ') {
                    output.pop();
                }
                output.push('\n');
            }
            if let Some(cell) = self.screen.cells.get(index).filter(|cell| cell.width > 0) {
                output.push_str(&cell.text);
            }
        }
        output
    }

    fn selection_indices(&self, selection: TerminalSelection) -> (usize, usize) {
        let mut a = self.screen.index(selection.anchor);
        let mut b = self.screen.index(selection.focus);
        if self.screen.cells.get(a).is_some_and(|cell| cell.width == 0) {
            a = a.saturating_sub(1);
        }
        if self.screen.cells.get(b).is_some_and(|cell| cell.width == 0) {
            b = b.saturating_sub(1);
        }
        (a.min(b), a.max(b))
    }

    fn position_at(&self, bounds: LayoutBox, x: f32, y: f32) -> TerminalPosition {
        TerminalPosition {
            row: (((y - bounds.y).max(0.0) / self.cell_height.max(1.0)) as u16)
                .min(self.screen.rows.saturating_sub(1)),
            column: (((x - bounds.x).max(0.0) / self.cell_width.max(1.0)) as u16)
                .min(self.screen.columns.saturating_sub(1)),
        }
    }
}

impl ComponentView for TerminalView {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        lifecycle: Some(crate::framework::lifecycle_hooks::terminal),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "terminal".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &self.style,
            InteractionState {
                pointer_events: !self.disabled,
                focusable: !self.disabled,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::from("Terminal")),
                disabled: self.disabled,
                ..Default::default()
            },
        );
    }
}

#[derive(Clone, PartialEq)]
struct TerminalRow(NodeStyle);
impl ComponentView for TerminalRow {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "terminal-row".into(),
        }
    }
    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &self.0,
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState::default(),
        );
    }
}

impl RegisterableComponent for TerminalView {
    const TYPE_ID: &'static str = crate::component_descriptors::TERMINAL.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TERMINAL.tags;
    const RETAIN_SEMANTIC_STATE: bool = true;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        view_from_spec(spec, authored_screen(spec).flatten())
    }
    fn reconcile_semantic(spec: &SemanticSpec<'_>, previous: Option<&Self>) -> Self {
        let authored = authored_screen(spec);
        let keep_host = authored.is_none();
        let mut view = view_from_spec(spec, authored.flatten());
        let Some(previous) = previous else {
            return view;
        };
        if keep_host {
            view.screen = previous.screen.clone();
        }
        view.selection = previous.selection;
        view.viewport = previous.viewport;
        view.cell_width = previous.cell_width;
        view.cell_height = previous.cell_height;
        view.font_size = previous.font_size;
        view.font_family = previous.font_family.clone();
        view.preedit = previous.preedit.clone();
        view.dragging = previous.dragging;
        view
    }
    fn finish_semantic(
        context: &mut AppContext,
        entity: Entity<Self>,
    ) -> Result<(), FrameworkError> {
        context.refresh_terminal_view(entity)
    }
}

impl AppContext {
    pub fn sync_terminal_screen(
        &mut self,
        entity: Entity<TerminalView>,
        screen: TerminalScreen,
    ) -> Result<(), FrameworkError> {
        if !screen.valid() {
            return Err(FrameworkError::InvalidInput);
        }
        self.update_component(entity, |view, _| {
            view.screen = screen;
        })?;
        Ok(())
    }

    pub fn refresh_terminal_view(
        &mut self,
        entity: Entity<TerminalView>,
    ) -> Result<(), FrameworkError> {
        let view = self.read(entity, Clone::clone)?;
        if !view.screen.valid()
            || !view.cell_width.is_finite()
            || !view.cell_height.is_finite()
            || view.cell_width <= 0.0
            || view.cell_height <= 0.0
        {
            return Err(FrameworkError::InvalidInput);
        }
        let selection = view
            .selection
            .map(|selection| view.selection_indices(selection));
        self.mount(entity, |ui| {
            for (row, cells) in view
                .screen
                .cells
                .chunks(usize::from(view.screen.columns))
                .enumerate()
            {
                let mut style = NodeStyle::default();
                let layout = Arc::make_mut(&mut style.layout);
                layout.position = PositionSpec::Absolute;
                layout.offset_left = Some(LengthSpec::Px(0.0));
                layout.offset_top = Some(LengthSpec::Px(row as f32 * view.cell_height));
                layout.width = Some(LengthSpec::Px(
                    f32::from(view.screen.columns) * view.cell_width,
                ));
                layout.height = Some(LengthSpec::Px(view.cell_height));
                ui.with_child(format!("row-{row}"), TerminalRow(style), |ui| {
                    for (column, cell) in cells.iter().enumerate() {
                        let index = row * usize::from(view.screen.columns) + column;
                        if cell.width == 0 {
                            continue;
                        }
                        let mut style = NodeStyle::default();
                        let selected = selection.is_some_and(|(a, b)| {
                            index <= b && index + usize::from(cell.width) > a
                        });
                        style.foreground = Some(if cell.inverse {
                            SemanticColorRole::Surface
                        } else if cell.dim {
                            SemanticColorRole::Muted
                        } else {
                            SemanticColorRole::Text
                        });
                        if cell.inverse {
                            style.background = Some(SemanticColorRole::Text);
                        }
                        if selected {
                            style.background = Some(SemanticColorRole::Selected);
                        }
                        let layout = Arc::make_mut(&mut style.layout);
                        layout.position = PositionSpec::Absolute;
                        layout.offset_left = Some(LengthSpec::Px(
                            (index % usize::from(view.screen.columns)) as f32 * view.cell_width,
                        ));
                        layout.offset_top = Some(LengthSpec::Px(0.0));
                        layout.width =
                            Some(LengthSpec::Px(view.cell_width * f32::from(cell.width)));
                        layout.height = Some(LengthSpec::Px(view.cell_height));
                        layout.font_size = Some(view.font_size);
                        layout.font_family = Some(view.font_family.clone());
                        layout.line_height = Some(LineHeightSpec::Absolute(view.cell_height));
                        layout.font_weight = Some(if cell.bold { 700 } else { 400 });
                        layout.font_italic = Some(cell.italic);
                        let (foreground, background) = if cell.inverse {
                            (cell.background, cell.foreground)
                        } else {
                            (cell.foreground, cell.background)
                        };
                        layout.color = foreground.map(|mut color| {
                            if cell.dim {
                                color[3] *= 0.5;
                            }
                            color
                        });
                        if !selected {
                            layout.background = background;
                        }
                        layout.text_decoration = Some(TextDecorationLine {
                            underline: cell.underline,
                            line_through: false,
                        });
                        layout.white_space_nowrap = true;
                        layout.overflow_x = OverflowSpec::Hidden;
                        layout.overflow_y = OverflowSpec::Hidden;
                        ui.child(
                            format!("cell-{index}"),
                            Text::new(cell.text.to_string()).style(style),
                        )?;
                    }
                    Ok(())
                })?;
            }
            let cursor = view.screen.cursor.unwrap_or(TerminalCursor {
                position: TerminalPosition::default(),
                shape: TerminalCursorShape::Block,
                visible: false,
            });
            if (cursor.visible || !view.preedit.is_empty())
                && cursor.position.row < view.screen.rows
                && cursor.position.column < view.screen.columns
            {
                let mut style =
                    NodeStyle::default().outline(SemanticColorRole::Text, nana_ui_core::HAIRLINE);
                let layout = Arc::make_mut(&mut style.layout);
                layout.position = PositionSpec::Absolute;
                layout.offset_left = Some(LengthSpec::Px(
                    f32::from(cursor.position.column) * view.cell_width,
                ));
                layout.offset_top = Some(LengthSpec::Px(
                    f32::from(cursor.position.row) * view.cell_height
                        + if cursor.shape == TerminalCursorShape::Underline {
                            view.cell_height - nana_ui_core::space::XXS
                        } else {
                            0.0
                        },
                ));
                layout.width = Some(LengthSpec::Px(
                    if cursor.shape == TerminalCursorShape::Bar {
                        nana_ui_core::space::XXS
                    } else {
                        view.cell_width
                    },
                ));
                layout.height = Some(LengthSpec::Px(
                    if cursor.shape == TerminalCursorShape::Underline {
                        nana_ui_core::space::XXS
                    } else {
                        view.cell_height
                    },
                ));
                if cursor.visible {
                    ui.child("cursor", Text::new("").style(style))?;
                }
                if !view.preedit.is_empty() {
                    let mut style = NodeStyle::default().surface(SemanticColorRole::Surface);
                    let layout = Arc::make_mut(&mut style.layout);
                    layout.position = PositionSpec::Absolute;
                    layout.offset_left = Some(LengthSpec::Px(
                        f32::from(cursor.position.column) * view.cell_width,
                    ));
                    layout.offset_top = Some(LengthSpec::Px(
                        f32::from(cursor.position.row) * view.cell_height,
                    ));
                    layout.font_size = Some(view.font_size);
                    layout.font_family = Some(view.font_family.clone());
                    layout.text_decoration = Some(TextDecorationLine {
                        underline: true,
                        line_through: false,
                    });
                    ui.child("preedit", Text::new(view.preedit.clone()).style(style))?;
                }
            }
            Ok(())
        })
    }

    pub fn resize_terminal_view(
        &mut self,
        entity: Entity<TerminalView>,
        width: f32,
        height: f32,
    ) -> Result<bool, FrameworkError> {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Ok(false);
        }
        let (columns, rows, previous) = self.read(entity, |view| {
            (
                (width / view.cell_width.max(1.0)).floor().max(1.0) as u16,
                (height / view.cell_height.max(1.0)).floor().max(1.0) as u16,
                view.viewport,
            )
        })?;
        if previous == Some((columns, rows)) {
            return Ok(false);
        }
        let mut changed = false;
        self.update_component(entity, |view, cx| {
            let columns = (width / view.cell_width.max(1.0)).floor().max(1.0) as u16;
            let rows = (height / view.cell_height.max(1.0)).floor().max(1.0) as u16;
            if view.viewport != Some((columns, rows)) {
                view.viewport = Some((columns, rows));
                cx.emit(TerminalEvent::Resize { columns, rows });
                changed = true;
            }
        })?;
        Ok(changed)
    }

    pub fn focused_terminal(&self, document: DocumentId) -> Option<Entity<TerminalView>> {
        let id = self.world().focused(document)?;
        let entity = Entity::from_stable_id(id);
        self.read(entity, |view: &TerminalView| !view.disabled)
            .ok()
            .filter(|value| *value)
            .map(|_| entity)
    }

    pub fn terminal_accepts_input(&self, document: DocumentId) -> bool {
        self.focused_terminal(document)
            .is_some_and(|entity| self.read(entity, |view| !view.read_only).unwrap_or(false))
    }

    pub fn terminal_caret_bounds(&self, document: DocumentId) -> Option<LayoutBox> {
        let entity = self.focused_terminal(document)?;
        let bounds = self.world().layout_box(entity.stable_id())?;
        self.read(entity, |view| {
            let position = view
                .screen
                .cursor
                .map(|cursor| cursor.position)
                .unwrap_or_default();
            LayoutBox {
                x: bounds.x + f32::from(position.column) * view.cell_width,
                y: bounds.y + f32::from(position.row) * view.cell_height,
                width: view.cell_width,
                height: view.cell_height,
            }
        })
        .ok()
    }

    pub fn set_terminal_preedit(
        &mut self,
        document: DocumentId,
        text: &str,
    ) -> Result<bool, FrameworkError> {
        let Some(entity) = self.focused_terminal(document) else {
            return Ok(false);
        };
        self.update_component(entity, |view, _| {
            view.preedit = if view.read_only {
                String::new()
            } else {
                text.to_owned()
            }
        })?;
        Ok(true)
    }

    pub fn terminal_selected_text(&self, document: DocumentId) -> Option<String> {
        self.read(
            self.focused_terminal(document)?,
            TerminalView::selected_text,
        )
        .ok()
    }

    pub fn terminal_input(
        &mut self,
        document: DocumentId,
        bytes: Vec<u8>,
    ) -> Result<bool, FrameworkError> {
        let Some(entity) = self.focused_terminal(document) else {
            return Ok(false);
        };
        if self.read(entity, |view| view.read_only)? {
            return Ok(true);
        }
        if !bytes.is_empty() {
            self.update_component(entity, |view, cx| {
                view.selection = None;
                view.preedit.clear();
                cx.emit(TerminalEvent::Input(bytes));
            })?;
        }
        Ok(true)
    }

    pub fn paste_terminal(
        &mut self,
        document: DocumentId,
        text: &str,
    ) -> Result<bool, FrameworkError> {
        let Some(entity) = self.focused_terminal(document) else {
            return Ok(false);
        };
        let bracketed = self.read(entity, |view| view.screen.bracketed_paste)?;
        self.terminal_input(
            document,
            if bracketed {
                format!("\x1b[200~{text}\x1b[201~").into_bytes()
            } else {
                text.as_bytes().to_vec()
            },
        )
    }

    pub fn terminal_key(
        &mut self,
        document: DocumentId,
        key: &str,
        text: Option<&str>,
        control: bool,
        alt: bool,
        shift: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(entity) = self.focused_terminal(document) else {
            return Ok(false);
        };
        if self.read(entity, |view| !view.preedit.is_empty())? {
            return Ok(true);
        }
        let application_cursor = self.read(entity, |view| view.screen.application_cursor)?;
        let bytes = terminal_key_bytes(key, text, control, alt, shift, application_cursor);
        // A printable key without its text sends nothing: the text arrives as
        // its own event, and the key must not count as handled or that text
        // would be dropped with it.
        if bytes.is_empty() {
            return Ok(false);
        }
        self.terminal_input(document, bytes)
    }

    /// `phase`: 0 press, 1 drag, 2 release, 3 cancel; host owns pointer capture.
    pub fn terminal_pointer(
        &mut self,
        document: DocumentId,
        target: Option<StableNodeId>,
        pointer: u64,
        phase: u8,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let entity = if phase == 0 {
            target.and_then(|id| {
                let entity = Entity::from_stable_id(id);
                self.read(entity, |view: &TerminalView| !view.disabled)
                    .ok()
                    .filter(|enabled| *enabled)
                    .map(|_| entity)
            })
        } else {
            self.focused_terminal(document).filter(|entity| {
                self.read(*entity, |view| view.dragging == Some(pointer))
                    .unwrap_or(false)
            })
        };
        let Some(entity) = entity else {
            return Ok(false);
        };
        let Some(bounds) = self.world().layout_box(entity.stable_id()) else {
            return Ok(false);
        };
        if phase == 0 {
            self.focus_node(document, entity.stable_id())?;
            self.press_pointer(document, pointer, entity.stable_id())?;
        }
        if phase >= 2 {
            self.release_pointer(document, pointer);
        }
        self.update_component(entity, |view, cx| {
            let position = view.position_at(bounds, x, y);
            match phase {
                0 => {
                    view.dragging = Some(pointer);
                    view.selection = Some(TerminalSelection {
                        anchor: position,
                        focus: position,
                    });
                }
                1 | 2 => {
                    if let Some(selection) = &mut view.selection {
                        selection.focus = position;
                    }
                    if phase == 2 {
                        view.dragging = None;
                    }
                }
                _ => {
                    view.dragging = None;
                    view.selection = None;
                }
            }
            cx.emit(TerminalEvent::SelectionChanged(view.selection));
        })?;
        Ok(true)
    }
}

fn view_from_spec(spec: &SemanticSpec<'_>, screen: Option<TerminalScreen>) -> TerminalView {
    let (columns, rows) = clamped_grid(
        spec_u16(spec, &["columns", "cols"], 1),
        spec_u16(spec, &["rows"], 1),
    );
    let mut view =
        TerminalView::new(screen.unwrap_or_else(|| TerminalScreen::blank(columns, rows)));
    view.disabled = spec.disabled;
    view.read_only = spec.read_only || attr_enabled(spec, &["read-only", "readOnly"]);
    Arc::make_mut(&mut view.style.layout).overlay_css_size_overrides(spec.layout.as_ref());
    view
}

fn authored_screen(spec: &SemanticSpec<'_>) -> Option<Option<TerminalScreen>> {
    spec.attr("screen").map(parse_terminal_screen)
}

fn parse_terminal_screen(raw: &str) -> Option<TerminalScreen> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let object = value.as_object()?;
    let columns = json_u16(object.get("columns"))?;
    let rows = json_u16(object.get("rows"))?;
    let count = usize::from(columns).checked_mul(usize::from(rows))?;
    if count == 0 || count > MAX_TERMINAL_CELLS {
        return None;
    }
    let mut cells = vec![TerminalCell::default(); count];
    match object.get("cells")? {
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().take(count).enumerate() {
                cells[index] = match item {
                    serde_json::Value::String(text) => cell_from_text(text),
                    serde_json::Value::Object(_) => parse_terminal_cell(item)?,
                    _ => return None,
                };
            }
        }
        serde_json::Value::String(text) => {
            place_packed_cells(&mut cells, usize::from(columns), text);
        }
        _ => return None,
    }
    let screen = TerminalScreen {
        columns,
        rows,
        cells: cells.into(),
        cursor: object.get("cursor").and_then(parse_terminal_cursor),
        application_cursor: json_flag(object, "applicationCursor")
            || json_flag(object, "application_cursor"),
        bracketed_paste: json_flag(object, "bracketedPaste")
            || json_flag(object, "bracketed_paste"),
    };
    screen.valid().then_some(screen)
}

fn json_u16(value: Option<&serde_json::Value>) -> Option<u16> {
    u16::try_from(json_u64(value?)?)
        .ok()
        .filter(|value| *value > 0)
}

fn parse_terminal_cell(value: &serde_json::Value) -> Option<TerminalCell> {
    let object = value.as_object()?;
    let text = object
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(" ");
    Some(TerminalCell {
        text: Arc::from(text),
        width: object
            .get("width")
            .and_then(json_u64)
            .and_then(|width| u8::try_from(width).ok())
            .filter(|width| *width <= 2)
            .unwrap_or_else(|| cell_display_width(text)),
        foreground: object.get("foreground").and_then(parse_cell_color),
        background: object.get("background").and_then(parse_cell_color),
        bold: json_flag(object, "bold"),
        underline: json_flag(object, "underline"),
        dim: json_flag(object, "dim"),
        italic: json_flag(object, "italic"),
        inverse: json_flag(object, "inverse"),
    })
}

fn place_packed_cells(cells: &mut [TerminalCell], columns: usize, text: &str) {
    let count = cells.len();
    if columns == 0 || count == 0 {
        return;
    }
    let mut index = 0usize;
    for grapheme in text.graphemes(true) {
        if index >= count {
            break;
        }
        if cell_display_width(grapheme) == 2 && columns >= 2 && index % columns + 1 >= columns {
            index += 1;
            if index >= count {
                break;
            }
        }
        let mut cell = cell_from_text(grapheme);
        let fits = cell.width == 2 && index % columns + 1 < columns && index + 1 < count;
        if cell.width == 2 && !fits {
            cell.width = 1;
        }
        cells[index] = cell;
        if fits {
            cells[index + 1] = TerminalCell {
                text: Arc::from(""),
                width: 0,
                ..TerminalCell::default()
            };
            index += 2;
        } else {
            index += 1;
        }
    }
}

fn cell_from_text(text: &str) -> TerminalCell {
    TerminalCell {
        text: Arc::from(text),
        width: cell_display_width(text),
        ..TerminalCell::default()
    }
}

fn cell_display_width(text: &str) -> u8 {
    match text.graphemes(true).next().map(UnicodeWidthStr::width) {
        None | Some(0) => 0,
        Some(1) => 1,
        Some(_) => 2,
    }
}

fn json_flag(object: &serde_json::Map<String, serde_json::Value>, key: &str) -> bool {
    object
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn parse_cell_color(value: &serde_json::Value) -> Option<[f32; 4]> {
    let values = value.as_array()?;
    if values.len() < 3 {
        return None;
    }
    let r = values[0].as_f64()?;
    let g = values[1].as_f64()?;
    let b = values[2].as_f64()?;
    let a = values
        .get(3)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(1.0);
    if ![r, g, b, a].iter().all(|channel| channel.is_finite()) {
        return None;
    }
    // Host dumps mix 0–1 and 0–255; any RGB channel > 1 selects the 255 scale.
    let scale = if r > 1.0 || g > 1.0 || b > 1.0 {
        255.0
    } else {
        1.0
    };
    let rgb = |channel: f64| (channel / scale).clamp(0.0, 1.0) as f32;
    let alpha = if scale > 1.0 && a > 1.0 {
        (a / 255.0).clamp(0.0, 1.0) as f32
    } else {
        a.clamp(0.0, 1.0) as f32
    };
    Some([rgb(r), rgb(g), rgb(b), alpha])
}

fn parse_terminal_cursor(value: &serde_json::Value) -> Option<TerminalCursor> {
    let object = value.as_object()?;
    let (row, column) = if let Some(position) = object.get("position") {
        let position = position.as_object()?;
        (position.get("row"), position.get("column"))
    } else {
        (object.get("row"), object.get("column"))
    };
    let row = row
        .and_then(json_u64)
        .and_then(|row| u16::try_from(row).ok())?;
    let column = column
        .and_then(json_u64)
        .and_then(|column| u16::try_from(column).ok())?;
    Some(TerminalCursor {
        position: TerminalPosition { row, column },
        shape: match object.get("shape").and_then(serde_json::Value::as_str) {
            Some("bar") => TerminalCursorShape::Bar,
            Some("underline") => TerminalCursorShape::Underline,
            _ => TerminalCursorShape::Block,
        },
        visible: object
            .get("visible")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
    })
}

/// Author-declared dimensions capped at [`MAX_TERMINAL_CELLS`].
///
/// `TerminalScreen::blank` answers an oversized grid with *no* cells, which
/// fails `valid()` — and since the binding's `finish_semantic` propagates that,
/// a `<terminal columns="1500" rows="1500">` would fail the whole binding
/// rather than render. Rows give way first: a terminal is read top to bottom,
/// so the visible window keeps its width.
fn clamped_grid(columns: u16, rows: u16) -> (u16, u16) {
    let columns = columns.max(1);
    let rows = rows.max(1);
    if usize::from(columns) * usize::from(rows) <= MAX_TERMINAL_CELLS {
        return (columns, rows);
    }
    let rows = u16::try_from(MAX_TERMINAL_CELLS / usize::from(columns))
        .unwrap_or(u16::MAX)
        .max(1);
    (columns, rows)
}

fn spec_u16(spec: &SemanticSpec<'_>, keys: &[&str], fallback: u16) -> u16 {
    keys.iter()
        .find_map(|key| spec.attr(key))
        .and_then(|value| value.trim().parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn attr_enabled(spec: &SemanticSpec<'_>, keys: &[&str]) -> bool {
    keys.iter()
        .find_map(|key| spec.attr(key))
        .is_some_and(|value| {
            let value = value.trim();
            !(value.eq_ignore_ascii_case("false") || value == "0")
        })
}

fn terminal_key_bytes(
    key: &str,
    text: Option<&str>,
    control: bool,
    alt: bool,
    shift: bool,
    application_cursor: bool,
) -> Vec<u8> {
    let arrow = match key {
        "ArrowUp" => Some("A"),
        "ArrowDown" => Some("B"),
        "ArrowRight" => Some("C"),
        "ArrowLeft" => Some("D"),
        "Home" => Some("H"),
        "End" => Some("F"),
        _ => None,
    };
    let function = match key {
        "F1" => Some(11),
        "F2" => Some(12),
        "F3" => Some(13),
        "F4" => Some(14),
        "F5" => Some(15),
        "F6" => Some(17),
        "F7" => Some(18),
        "F8" => Some(19),
        "F9" => Some(20),
        "F10" => Some(21),
        "F11" => Some(23),
        "F12" => Some(24),
        _ => None,
    };
    let mut bytes = if let Some(code) = function {
        let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(control);
        if modifier > 1 {
            format!("\x1b[{code};{modifier}~").into_bytes()
        } else if code <= 14 {
            vec![0x1b, b'O', b'P' + (code - 11)]
        } else {
            format!("\x1b[{code}~").into_bytes()
        }
    } else if control && key == "Space" {
        vec![0]
    } else if let Some(arrow) = arrow {
        let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(control);
        if modifier > 1 {
            format!("\x1b[1;{modifier}{arrow}").into_bytes()
        } else {
            format!("\x1b{}{arrow}", if application_cursor { "O" } else { "[" }).into_bytes()
        }
    } else if control
        && key.len() == 1
        && (key.as_bytes()[0].is_ascii_alphabetic() || b"@[\\]^_ ".contains(&key.as_bytes()[0]))
    {
        vec![key.as_bytes()[0].to_ascii_uppercase() & 0x1f]
    } else {
        match key {
            "Enter" => b"\r".to_vec(),
            "Backspace" => vec![0x7f],
            "Tab" if shift => b"\x1b[Z".to_vec(),
            "Tab" => b"\t".to_vec(),
            "Escape" => vec![0x1b],
            "Delete" => b"\x1b[3~".to_vec(),
            "Insert" => b"\x1b[2~".to_vec(),
            "PageUp" => b"\x1b[5~".to_vec(),
            "PageDown" => b"\x1b[6~".to_vec(),
            _ => text.unwrap_or_default().as_bytes().to_vec(),
        }
    };
    if alt && arrow.is_none() && function.is_none() && !bytes.is_empty() {
        bytes.insert(0, 0x1b);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An author can write any number; `TerminalScreen::blank` answers an
    /// oversized grid with no cells at all, and the binding then fails instead
    /// of rendering.
    #[test]
    fn an_oversized_grid_is_clamped_rather_than_rejected() {
        let (columns, rows) = clamped_grid(1500, 1500);
        assert_eq!(columns, 1500, "the visible width is kept");
        assert!(
            usize::from(columns) * usize::from(rows) <= MAX_TERMINAL_CELLS,
            "{columns}x{rows} still exceeds the cap"
        );
        let screen = TerminalScreen::blank(columns, rows);
        assert!(!screen.cells.is_empty(), "a clamped screen has cells");

        // A grid inside the cap is untouched.
        assert_eq!(clamped_grid(80, 24), (80, 24));
        // And zero is not a terminal.
        assert_eq!(clamped_grid(0, 0), (1, 1));
    }

    use std::sync::Mutex;

    #[test]
    fn layout_emits_size_and_rebinding_republishes_it() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let terminal = context
            .create_component(document, TerminalView::new(TerminalScreen::blank(3, 2)))
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        context
            .on(terminal, move |_, event: &TerminalEvent, _| {
                observed.lock().unwrap().push(event.clone())
            })
            .unwrap();
        context
            .layout_document(document, crate::LayoutViewport::new(80.0, 36.0))
            .unwrap();
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[TerminalEvent::Resize {
                columns: 10,
                rows: 2
            }]
        );
        let rows = context.world().node(terminal.stable_id()).unwrap().children;
        let second_cell = context.world().node(rows[1]).unwrap().children[1];
        assert_eq!(
            context.world().layout_box(second_cell),
            Some(LayoutBox {
                x: 8.0,
                y: 18.0,
                width: 8.0,
                height: 18.0
            })
        );
        context
            .layout_document(document, crate::LayoutViewport::new(81.0, 36.0))
            .unwrap();
        assert_eq!(events.lock().unwrap().len(), 1);
        context
            .update_component(terminal, |view, _| {
                *view = TerminalView::new(TerminalScreen::blank(3, 2))
            })
            .unwrap();
        assert_eq!(events.lock().unwrap().len(), 2);
        let oversized = TerminalScreen::blank(u16::MAX, u16::MAX);
        assert!(oversized.cells.is_empty());
        assert!(context.sync_terminal_screen(terminal, oversized).is_err());
        assert_eq!(
            context.read(terminal, |view| view.screen.columns).unwrap(),
            3
        );
    }

    #[test]
    fn grid_reuses_cells_and_removes_stale_nodes_on_resize() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let terminal = context
            .create_component(document, TerminalView::new(TerminalScreen::blank(3, 2)))
            .unwrap();
        let initial = context
            .world()
            .node(terminal.stable_id())
            .unwrap()
            .children
            .clone();
        assert_eq!(initial.len(), 2);
        let mut screen = TerminalScreen::blank(3, 2);
        Arc::make_mut(&mut screen.cells)[0].text = Arc::from("a");
        context.sync_terminal_screen(terminal, screen).unwrap();
        assert_eq!(
            context.world().node(terminal.stable_id()).unwrap().children,
            initial
        );
        context
            .sync_terminal_screen(terminal, TerminalScreen::blank(2, 1))
            .unwrap();
        assert_eq!(
            context
                .world()
                .node(terminal.stable_id())
                .unwrap()
                .children
                .len(),
            1
        );
        assert!(
            initial[1..]
                .iter()
                .all(|id| context.world().node(*id).is_none())
        );
    }

    #[test]
    fn wide_continuation_selection_copies_the_whole_grapheme() {
        let mut screen = TerminalScreen::blank(3, 2);
        let cells = Arc::make_mut(&mut screen.cells);
        cells[0].text = Arc::from("你");
        cells[0].width = 2;
        cells[1].text = Arc::from("");
        cells[1].width = 0;
        cells[3].text = Arc::from("e\u{301}");
        let mut view = TerminalView::new(screen);
        view.selection = Some(TerminalSelection {
            anchor: TerminalPosition { row: 0, column: 1 },
            focus: TerminalPosition { row: 1, column: 0 },
        });
        assert_eq!(view.selected_text(), "你\ne\u{301}");
    }

    #[test]
    fn terminal_input_is_scoped_and_resize_is_coalesced() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let first = context
            .create_component(document, TerminalView::new(TerminalScreen::blank(3, 2)))
            .unwrap();
        let mut screen = TerminalScreen::blank(3, 2);
        screen.bracketed_paste = true;
        let second = context
            .create_component(document, TerminalView::new(screen))
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        context
            .on(second, move |_, event: &TerminalEvent, _| {
                observed.lock().unwrap().push(event.clone())
            })
            .unwrap();
        context.focus_node(document, second.stable_id()).unwrap();
        context
            .terminal_key(document, "c", None, true, false, false)
            .unwrap();
        context.paste_terminal(document, "中文").unwrap();
        assert!(context.resize_terminal_view(second, 80.0, 36.0).unwrap());
        assert!(!context.resize_terminal_view(second, 81.0, 36.0).unwrap());
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[
                TerminalEvent::Input(vec![3]),
                TerminalEvent::Input("\x1b[200~中文\x1b[201~".as_bytes().to_vec()),
                TerminalEvent::Resize {
                    columns: 10,
                    rows: 2
                }
            ]
        );
        assert!(
            context
                .read(first, |view| view.selection.is_none())
                .unwrap()
        );
        context
            .update_component(second, |view, _| {
                view.read_only = true;
                view.selection = Some(TerminalSelection {
                    anchor: TerminalPosition::default(),
                    focus: TerminalPosition::default(),
                });
            })
            .unwrap();
        context.terminal_input(document, vec![b'a']).unwrap();
        assert_eq!(events.lock().unwrap().len(), 3);
        assert_eq!(
            context.terminal_selected_text(document).as_deref(),
            Some(" ")
        );
        assert!(!context.terminal_accepts_input(document));
        assert_eq!(
            terminal_key_bytes("ArrowUp", None, false, false, false, true),
            b"\x1bOA"
        );
        assert_eq!(
            terminal_key_bytes("ArrowLeft", None, true, false, false, false),
            b"\x1b[1;5D"
        );
    }

    fn terminal_spec<'a>(
        type_id: &'a crate::component_registry::ComponentTypeId,
        layout: &'a Arc<nana_ui_core::LayoutStyle>,
        attrs: &'a [(&'a str, &'a str)],
    ) -> SemanticSpec<'a> {
        SemanticSpec {
            attrs,
            ..SemanticSpec::from_parts(type_id, layout)
        }
    }

    fn screen_view(
        type_id: &crate::component_registry::ComponentTypeId,
        layout: &Arc<nana_ui_core::LayoutStyle>,
        screen: &str,
    ) -> TerminalView {
        TerminalView::from_semantic(&terminal_spec(
            type_id,
            layout,
            &[("columns", "80"), ("rows", "24"), ("screen", screen)],
        ))
    }

    #[test]
    fn semantic_rebind_keeps_pty_cells_when_grid_size_is_unchanged() {
        use crate::component_registry::ComponentTypeId;
        use nana_ui_core::LayoutStyle;

        let type_id = ComponentTypeId::new("nana.terminal").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [("columns", "3"), ("rows", "2")];
        let spec = SemanticSpec {
            attrs: &attrs,
            ..SemanticSpec::from_parts(&type_id, &layout)
        };
        let mut previous = TerminalView::from_semantic(&spec);
        Arc::make_mut(&mut previous.screen.cells)[0].text = Arc::from("a");
        previous.selection = Some(TerminalSelection {
            anchor: TerminalPosition { row: 0, column: 0 },
            focus: TerminalPosition { row: 0, column: 1 },
        });
        let rebound = TerminalView::reconcile_semantic(&spec, Some(&previous));
        assert_eq!(rebound.screen.cells[0].text.as_ref(), "a");
        assert_eq!(rebound.selection, previous.selection);
    }

    #[test]
    fn vue_reconcile_keeps_an_application_fed_screen_when_size_attrs_are_unchanged() {
        let type_id = crate::component_registry::ComponentTypeId::new("nana.terminal").unwrap();
        let layout_style = Arc::new(nana_ui_core::LayoutStyle::default());
        let attrs = [("columns", "80"), ("rows", "24")];
        let spec = terminal_spec(&type_id, &layout_style, &attrs);
        let mut view = TerminalView::from_semantic(&spec);
        assert_eq!(view.screen.columns, 80);
        assert_eq!(view.screen.rows, 24);

        let mut fed = TerminalScreen::blank(10, 2);
        Arc::make_mut(&mut fed.cells)[0].text = Arc::from("a");
        view.screen = fed.clone();
        view.viewport = Some((10, 2));

        let reconciled = TerminalView::reconcile_semantic(&spec, Some(&view));
        assert_eq!(reconciled.screen.columns, 10);
        assert_eq!(reconciled.screen.rows, 2);
        assert_eq!(reconciled.screen.cells[0].text.as_ref(), "a");
        assert_eq!(reconciled.viewport, Some((10, 2)));

        let changed_attrs = [("columns", "40"), ("rows", "12")];
        let changed = TerminalView::reconcile_semantic(
            &terminal_spec(&type_id, &layout_style, &changed_attrs),
            Some(&reconciled),
        );
        assert_eq!(
            changed.screen.cells[0].text.as_ref(),
            "a",
            "a later Vue columns/rows change still must not blank the application buffer"
        );
        assert_eq!(changed.screen.columns, 10);
        assert_eq!(changed.screen.rows, 2);
    }

    #[test]
    fn from_semantic_reads_screen_json_and_reconcile_keeps_host_feed_without_it() {
        let type_id = crate::component_registry::ComponentTypeId::new("nana.terminal").unwrap();
        let layout_style = Arc::new(nana_ui_core::LayoutStyle::default());
        let view = screen_view(
            &type_id,
            &layout_style,
            r#"{"columns":2,"rows":1,"cells":[{"text":"h"},{"text":"i"}]}"#,
        );
        assert_eq!(view.screen.columns, 2);
        assert_eq!(view.screen.rows, 1);
        assert_eq!(view.screen.cells[0].text.as_ref(), "h");
        assert_eq!(view.screen.cells[1].text.as_ref(), "i");

        let without_screen = terminal_spec(
            &type_id,
            &layout_style,
            &[("columns", "80"), ("rows", "24")],
        );
        let mut hosted = TerminalView::from_semantic(&without_screen);
        hosted.screen = view.screen.clone();
        let reconciled = TerminalView::reconcile_semantic(&without_screen, Some(&hosted));
        assert_eq!(reconciled.screen.cells[0].text.as_ref(), "h");

        let replaced = TerminalView::reconcile_semantic(
            &terminal_spec(
                &type_id,
                &layout_style,
                &[
                    ("columns", "80"),
                    ("rows", "24"),
                    ("screen", r#"{"columns":1,"rows":1,"cells":[{"text":"x"}]}"#),
                ],
            ),
            Some(&reconciled),
        );
        assert_eq!(replaced.screen.columns, 1);
        assert_eq!(replaced.screen.cells[0].text.as_ref(), "x");

        let ignored = TerminalView::from_semantic(&terminal_spec(
            &type_id,
            &layout_style,
            &[
                ("columns", "80"),
                ("rows", "24"),
                ("cells", r#"[{"text":"z"}]"#),
            ],
        ));
        assert_eq!(ignored.screen.columns, 80);
        assert_eq!(ignored.screen.cells[0].text.as_ref(), " ");

        let nested = screen_view(
            &type_id,
            &layout_style,
            r#"{"columns":2,"rows":1,"cells":["h","i"],"cursor":{"position":{"row":0,"column":1},"shape":"bar"}}"#,
        );
        assert_eq!(nested.screen.cells[0].text.as_ref(), "h");
        let cursor = nested.screen.cursor.expect("nested cursor.position");
        assert_eq!(cursor.position, TerminalPosition { row: 0, column: 1 });
        assert_eq!(cursor.shape, TerminalCursorShape::Bar);

        let flat = screen_view(
            &type_id,
            &layout_style,
            r#"{"columns":1,"rows":1,"cells":[{"text":"x"}],"cursor":{"row":3,"column":5}}"#,
        );
        assert_eq!(
            flat.screen.cursor.map(|cursor| cursor.position),
            Some(TerminalPosition { row: 3, column: 5 })
        );
        assert!(
            screen_view(
                &type_id,
                &layout_style,
                r#"{"columns":1,"rows":1,"cells":[{"text":"x"}],"cursor":{"shape":"bar"}}"#,
            )
            .screen
            .cursor
            .is_none()
        );

        let rejected = screen_view(
            &type_id,
            &layout_style,
            r#"{"columns":2,"rows":1,"cells":[1,2]}"#,
        );
        assert_eq!(rejected.screen.columns, 80);
        assert_eq!(rejected.screen.cells[0].text.as_ref(), " ");

        let packed = screen_view(
            &type_id,
            &layout_style,
            r#"{"columns":4,"rows":1,"cells":"你好"}"#,
        );
        assert_eq!(packed.screen.cells[0].text.as_ref(), "你");
        assert_eq!(packed.screen.cells[0].width, 2);
        assert_eq!(packed.screen.cells[1].width, 0);
        assert_eq!(packed.screen.cells[2].text.as_ref(), "好");
        assert_eq!(packed.screen.cells[2].width, 2);

        let wrap = screen_view(
            &type_id,
            &layout_style,
            r#"{"columns":3,"rows":2,"cells":"你好"}"#,
        );
        assert_eq!(wrap.screen.cells[0].width, 2);
        assert_eq!(wrap.screen.cells[2].text.as_ref(), " ");
        assert_eq!(wrap.screen.cells[3].text.as_ref(), "好");
        assert_eq!(wrap.screen.cells[3].width, 2);

        let wiped = TerminalView::reconcile_semantic(
            &terminal_spec(
                &type_id,
                &layout_style,
                &[("columns", "80"), ("rows", "24"), ("screen", "{")],
            ),
            Some(&hosted),
        );
        assert_eq!(wiped.screen.columns, 80);
        assert_eq!(wiped.screen.cells[0].text.as_ref(), " ");

        let array = screen_view(
            &type_id,
            &layout_style,
            r#"{"columns":2,"rows":1,"cells":["你",""]}"#,
        );
        assert_eq!(array.screen.cells[0].width, 2);
        assert_eq!(array.screen.cells[1].width, 0);
        let objects = screen_view(
            &type_id,
            &layout_style,
            r#"{"columns":2,"rows":1,"cells":[{"text":"你"},{"text":""}]}"#,
        );
        assert_eq!(objects.screen.cells[0].width, 2);
        assert_eq!(objects.screen.cells[1].width, 0);
        assert_eq!(
            screen_view(
                &type_id,
                &layout_style,
                r#"{"columns":1,"rows":1,"cells":[{"text":"你","width":1}]}"#,
            )
            .screen
            .cells[0]
                .width,
            1
        );

        let colors = screen_view(
            &type_id,
            &layout_style,
            r#"{"columns":1,"rows":1,"cells":[{"text":"x","foreground":[255,0,0],"background":[0,1,0,0.5]}]}"#,
        );
        assert_eq!(
            colors.screen.cells[0].foreground,
            Some([1.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(
            colors.screen.cells[0].background,
            Some([0.0, 1.0, 0.0, 0.5])
        );
        assert_eq!(
            screen_view(
                &type_id,
                &layout_style,
                r#"{"columns":1,"rows":1,"cells":[{"text":"x","foreground":[255,128,0,128]}]}"#,
            )
            .screen
            .cells[0]
                .foreground,
            Some([1.0, 128.0 / 255.0, 0.0, 128.0 / 255.0])
        );
        assert_eq!(
            screen_view(
                &type_id,
                &layout_style,
                r#"{"columns":1,"rows":1,"cells":[{"text":"x","foreground":[1e400,0,0]}]}"#,
            )
            .screen
            .cells[0]
                .foreground,
            None
        );
    }

    #[test]
    fn from_semantic_overlays_vue_css_size_onto_structural_defaults() {
        let type_id = crate::component_registry::ComponentTypeId::new("nana.terminal").unwrap();
        let layout_style = Arc::new(nana_ui_core::LayoutStyle {
            height: Some(LengthSpec::Px(216.0)),
            width: Some(LengthSpec::Px(640.0)),
            ..nana_ui_core::LayoutStyle::default()
        });
        let view = TerminalView::from_semantic(&terminal_spec(&type_id, &layout_style, &[]));
        assert_eq!(view.style.layout.height, Some(LengthSpec::Px(216.0)));
        assert_eq!(view.style.layout.width, Some(LengthSpec::Px(640.0)));
        assert_eq!(view.style.layout.overflow_y, OverflowSpec::Hidden);
        assert_eq!(view.style.layout.flex_grow, Some(1.0));

        let fill = TerminalView::from_semantic(&terminal_spec(
            &type_id,
            &Arc::new(nana_ui_core::LayoutStyle::default()),
            &[],
        ));
        assert_eq!(fill.style.layout.width, Some(LengthSpec::Fill));
        assert_eq!(fill.style.layout.height, Some(LengthSpec::Fill));
    }
}

//! Modal command list. Search uses committed TextInput state.

use std::sync::Arc;

use nana_ui_core::{
    ActionId, ActionPickerNavigation, CommandPaletteEvent, CommandPaletteItem, ControlSize,
    DialogSize,
};

use crate::overlay_surfaces::{MODAL_PAD_X, modal_root_style, modal_surface_bounds};
use crate::query::query_matches;
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentElevation, ComponentTextRegion, ComponentView,
    InteractionState, LayoutBox, MutationQueue, NodeKind, NodeStyle, StableNodeId, StandardVisual,
    TextInputState, UiWorld,
};

const MAX_VISIBLE_ROWS: usize = 12;
const INPUT_GAP: f32 = nana_ui_core::space::MD;
const ROW_PAD_X: f32 = nana_ui_core::space::LG;
const SHORTCUT_TEXT_SIZE: f32 = nana_ui_core::type_scale::HINT;
const SHORTCUT_LABEL_GAP: f32 = nana_ui_core::space::MD;

fn palette_row_height(metrics: nana_ui_core::ThemeMetrics) -> f32 {
    ControlSize::Large.height_in(metrics) + nana_ui_core::space::XS
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteRowData {
    pub action: ActionId,
    pub label: Arc<str>,
    pub category: Option<Arc<str>>,
    pub shortcut: Option<Arc<str>>,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaletteRowGeometry {
    pub action: ActionId,
    pub bounds: LayoutBox,
    pub label: ComponentTextRegion,
    pub category: Option<ComponentTextRegion>,
    pub shortcut: Option<ComponentTextRegion>,
    pub selected: bool,
    pub background: Option<[f32; 4]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandPalette {
    pub title: Arc<str>,
    pub placeholder: Arc<str>,
    pub empty_label: Arc<str>,
    pub items: Vec<CommandPaletteItem>,
    /// Items have already been filtered and ranked by the host.
    pub filtered_items: bool,
    pub query: String,
    pub selected: usize,
    pub state: TextInputState,
    pub style: NodeStyle,
}

impl CommandPalette {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn new(
        title: impl Into<Arc<str>>,
        items: impl IntoIterator<Item = CommandPaletteItem>,
    ) -> Self {
        let items = items.into_iter().collect::<Vec<_>>();
        Self {
            title: title.into(),
            placeholder: Arc::from("搜索操作"),
            empty_label: Arc::from("没有可用操作"),
            items,
            filtered_items: false,
            query: String::new(),
            selected: 0,
            state: TextInputState::new(""),
            style: modal_root_style(),
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<Arc<str>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn empty_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.empty_label = label.into();
        self
    }

    pub fn query(mut self, query: impl Into<String>) -> Self {
        let query = query.into();
        self.query = query.clone();
        self.state = TextInputState::new(query);
        self.selected = 0;
        self
    }

    /// Preserve host-filtered results and their ranking without applying the
    /// palette's substring filter again. Query editing still emits Search.
    pub fn filtered_items(mut self, filtered: bool) -> Self {
        self.filtered_items = filtered;
        self
    }

    pub fn visible_items(&self) -> Vec<&CommandPaletteItem> {
        self.items
            .iter()
            .filter(|item| self.filtered_items || item_matches(item, &self.query))
            .collect()
    }

    pub fn set_query(&mut self, query: impl Into<String>) -> CommandPaletteEvent {
        let query = query.into();
        self.query = query.clone();
        self.state.replace_value(query.clone());
        self.selected = 0;
        CommandPaletteEvent::Search(query)
    }

    pub fn navigate(&mut self, navigation: ActionPickerNavigation) -> Option<CommandPaletteEvent> {
        let visible = self.visible_items().len();
        match navigation {
            ActionPickerNavigation::Dismiss => Some(CommandPaletteEvent::Dismiss),
            ActionPickerNavigation::Confirm => self.confirm(),
            ActionPickerNavigation::Previous if visible > 0 => {
                self.selected = self.selected.saturating_sub(1);
                Some(CommandPaletteEvent::Navigate(navigation))
            }
            ActionPickerNavigation::Next if visible > 0 => {
                self.selected = (self.selected + 1).min(visible.saturating_sub(1));
                Some(CommandPaletteEvent::Navigate(navigation))
            }
            ActionPickerNavigation::First if visible > 0 => {
                self.selected = 0;
                Some(CommandPaletteEvent::Navigate(navigation))
            }
            ActionPickerNavigation::Last if visible > 0 => {
                self.selected = visible.saturating_sub(1);
                Some(CommandPaletteEvent::Navigate(navigation))
            }
            _ => None,
        }
    }

    pub fn confirm(&self) -> Option<CommandPaletteEvent> {
        self.visible_items()
            .get(self.selected)
            .map(|item| CommandPaletteEvent::Select(item.action.clone()))
    }

    pub fn replace_selection(&mut self, text: &str) -> bool {
        if !self.state.replace_selection(text) {
            return false;
        }
        self.query = self.state.value.to_string();
        self.selected = 0;
        true
    }

    /// Delete IME surrounding text; `composing` keeps the selection a preedit
    /// stands in for (see [`crate::TextInputState::delete_ime_surrounding`]).
    pub fn delete_surrounding(
        &mut self,
        before_bytes: usize,
        after_bytes: usize,
        composing: bool,
    ) -> bool {
        if !self
            .state
            .delete_ime_surrounding(before_bytes, after_bytes, composing)
        {
            return false;
        }
        self.query = self.state.value.to_string();
        self.selected = 0;
        true
    }

    fn windowed_rows(&self) -> Vec<PaletteRowData> {
        let visible = self.visible_items();
        if visible.is_empty() {
            return Vec::new();
        }
        let len = visible.len();
        let start = if len <= MAX_VISIBLE_ROWS {
            0
        } else {
            self.selected
                .saturating_sub(MAX_VISIBLE_ROWS / 2)
                .min(len - MAX_VISIBLE_ROWS)
        };
        visible
            .into_iter()
            .enumerate()
            .skip(start)
            .take(MAX_VISIBLE_ROWS)
            .map(|(index, item)| PaletteRowData {
                action: item.action.clone(),
                label: Arc::from(item.label.as_str()),
                category: item.category.as_deref().map(Arc::from),
                shortcut: item.shortcut.as_deref().map(Arc::from),
                selected: index == self.selected,
            })
            .collect()
    }
}

impl ComponentView for CommandPalette {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activate_at: Some(crate::AppContext::activate_command_palette_at),
        hooks: Some(|hooks| hooks.command_palette = Some(&COMMAND_PALETTE_HOOKS)),
        editable: Some(&crate::framework::Editable::<Self>::HOOKS),
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
            tag: "command-palette".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let rows = self.windowed_rows();
        let empty = rows.is_empty().then(|| Arc::clone(&self.empty_label));
        let visual = StandardVisual::CommandPalette {
            title: Arc::clone(&self.title),
            query: Arc::from(self.query.as_str()),
            placeholder: Arc::clone(&self.placeholder),
            empty,
            rows: rows.into(),
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        if world
            .text_input(id)
            .is_none_or(|current| current != self.state)
        {
            mutations.set_text_input(id, Some(self.state.clone()));
        }
        project_common(
            id,
            world,
            mutations,
            &self.style,
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
            AccessibilityState {
                role: AccessibilityRole::Dialog,
                label: Some(Arc::clone(&self.title)),
                value: (!self.query.is_empty()).then(|| Arc::from(self.query.as_str())),
                modal: true,
                editable: true,
                ..AccessibilityState::default()
            },
        );
    }
}

pub(crate) fn command_palette_geometry(
    bounds: LayoutBox,
    title: &Arc<str>,
    query: &Arc<str>,
    placeholder: &Arc<str>,
    empty: Option<&Arc<str>>,
    rows: &[PaletteRowData],
    palette: &nana_ui_core::SemanticPalette,
    metrics: nana_ui_core::ThemeMetrics,
    measure: crate::text_width::ChromeTextMeasure<'_>,
) -> crate::ComponentGeometry {
    let input_height = ControlSize::Medium.height_in(metrics);
    let row_height = palette_row_height(metrics);
    let list_height = if rows.is_empty() {
        row_height
    } else {
        rows.len() as f32 * row_height
    };
    let intrinsic = nana_ui_core::space::PAGE * 2.0
        + input_height
        + INPUT_GAP
        + list_height
        + nana_ui_core::space::XXXL;
    let surface = modal_surface_bounds(
        bounds,
        crate::ModalSurfaceKind::Dialog(DialogSize::Wide),
        Some(intrinsic),
    );
    let mut y = surface.y + nana_ui_core::space::XXXL;
    let title_region = ComponentTextRegion {
        bounds: LayoutBox {
            x: surface.x + MODAL_PAD_X,
            y,
            width: (surface.width - MODAL_PAD_X * 2.0).max(0.0),
            height: nana_ui_core::space::PAGE_TIGHT + nana_ui_core::space::XXS,
        },
        content: Arc::clone(title).into(),
        color: Some(palette.text.as_rgba_array()),
        font_size: nana_ui_core::type_scale::HEADING,
        font_weight: Some(nana_ui_core::type_scale::SEMIBOLD),
    };
    y += ControlSize::Small.height_in(metrics);
    let input = ComponentTextRegion {
        bounds: LayoutBox {
            x: surface.x + MODAL_PAD_X,
            y,
            width: (surface.width - MODAL_PAD_X * 2.0).max(0.0),
            height: input_height,
        },
        content: if query.is_empty() {
            Arc::clone(placeholder).into()
        } else {
            Arc::clone(query).into()
        },
        color: Some(if query.is_empty() {
            palette.faint.as_rgba_array()
        } else {
            palette.text.as_rgba_array()
        }),
        font_size: ControlSize::Medium.text_size(),
        font_weight: None,
    };
    y += input_height + INPUT_GAP;
    let empty_label = empty.map(|label| ComponentTextRegion {
        bounds: LayoutBox {
            x: surface.x + MODAL_PAD_X,
            y,
            width: (surface.width - MODAL_PAD_X * 2.0).max(0.0),
            height: row_height,
        },
        content: Arc::clone(label).into(),
        color: Some(palette.muted.as_rgba_array()),
        font_size: nana_ui_core::type_scale::META,
        font_weight: None,
    });
    let rows = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let bounds = LayoutBox {
                x: surface.x + nana_ui_core::space::MD,
                y: y + index as f32 * row_height,
                width: (surface.width - nana_ui_core::space::XXXL).max(0.0),
                height: row_height,
            };
            let shortcut = row.shortcut.as_ref().map(|shortcut| {
                let shortcut_width = measure.width(shortcut, SHORTCUT_TEXT_SIZE, None);
                ComponentTextRegion {
                    bounds: LayoutBox {
                        x: bounds.x + bounds.width - ROW_PAD_X - shortcut_width,
                        y: bounds.y + nana_ui_core::space::XL,
                        width: shortcut_width,
                        height: nana_ui_core::type_scale::LINE,
                    },
                    content: Arc::clone(shortcut).into(),
                    color: Some(palette.muted.as_rgba_array()),
                    font_size: SHORTCUT_TEXT_SIZE,
                    font_weight: None,
                }
            });
            // The label yields to the (End-aligned) shortcut instead of
            // running underneath it.
            let label_right = match &shortcut {
                Some(shortcut) => shortcut.bounds.x - SHORTCUT_LABEL_GAP,
                None => bounds.x + bounds.width - ROW_PAD_X,
            };
            let label = ComponentTextRegion {
                bounds: LayoutBox {
                    x: bounds.x + ROW_PAD_X,
                    y: bounds.y + nana_ui_core::space::SM,
                    width: (label_right - bounds.x - ROW_PAD_X).max(0.0),
                    height: nana_ui_core::type_scale::LINE,
                },
                content: Arc::clone(&row.label).into(),
                color: Some(palette.text.as_rgba_array()),
                font_size: nana_ui_core::type_scale::META,
                font_weight: Some(nana_ui_core::type_scale::MEDIUM),
            };
            let category = row.category.as_ref().map(|category| ComponentTextRegion {
                bounds: LayoutBox {
                    x: bounds.x + ROW_PAD_X,
                    y: bounds.y + nana_ui_core::space::PAGE_TIGHT + nana_ui_core::space::XXS,
                    width: (bounds.width - ROW_PAD_X * 2.0).max(0.0),
                    height: nana_ui_core::type_scale::META,
                },
                content: Arc::clone(category).into(),
                color: Some(palette.muted.as_rgba_array()),
                font_size: SHORTCUT_TEXT_SIZE,
                font_weight: None,
            });
            PaletteRowGeometry {
                action: row.action.clone(),
                bounds,
                label,
                category,
                shortcut,
                selected: row.selected,
                background: row.selected.then_some(palette.selected.as_rgba_array()),
            }
        })
        .collect();
    crate::ComponentGeometry::CommandPalette {
        scrim: bounds,
        surface,
        title: title_region,
        input,
        empty: empty_label,
        rows,
        background: palette.surface.as_rgba_array(),
        input_background: palette.background.as_rgba_array(),
        input_border: palette.border.as_rgba_array(),
        elevation: ComponentElevation {
            color: [0.0, 0.0, 0.0, 0.4],
            offset_x: 0.0,
            offset_y: nana_ui_core::space::XL,
            blur_radius: nana_ui_core::space::PAGE,
            spread_radius: 0.0,
            inset: false,
        },
    }
}

pub(crate) fn palette_row_at(rows: &[PaletteRowGeometry], x: f32, y: f32) -> Option<usize> {
    rows.iter().position(|row| row.bounds.contains(x, y))
}

pub(crate) fn activate_command_palette_at(
    palette: &mut CommandPalette,
    surface: LayoutBox,
    input: LayoutBox,
    rows: &[PaletteRowGeometry],
    x: f32,
    y: f32,
) -> Option<CommandPaletteEvent> {
    if let Some(index) = palette_row_at(rows, x, y) {
        palette.selected = palette
            .visible_items()
            .iter()
            .position(|item| item.action == rows[index].action)
            .unwrap_or(palette.selected);
        return Some(CommandPaletteEvent::Select(rows[index].action.clone()));
    }
    if input.contains(x, y) || surface.contains(x, y) {
        return None;
    }
    Some(CommandPaletteEvent::Dismiss)
}

fn item_matches(item: &CommandPaletteItem, query: &str) -> bool {
    query_matches(&item.label, query)
        || query_matches(item.action.as_str(), query)
        || item
            .category
            .as_ref()
            .is_some_and(|category| query_matches(category, query))
}

/// The router reaches this picker's keyboard only once one exists.
pub(crate) static COMMAND_PALETTE_HOOKS: crate::framework::NavigateHooks<
    nana_ui_core::ActionPickerNavigation,
> = crate::framework::NavigateHooks {
    navigate: crate::AppContext::navigate_focused_command_palette,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn sample() -> CommandPalette {
        CommandPalette::new(
            "命令面板",
            [
                CommandPaletteItem::new("workspace.files", "打开文件").category("工作区"),
                CommandPaletteItem::new("workspace.settings", "设置").shortcut("Ctrl+,"),
                CommandPaletteItem::new("edit.rename", "重命名"),
            ],
        )
    }

    #[test]
    fn query_filters_and_confirm_selects_the_highlighted_action() {
        let mut palette = sample();
        palette.set_query("设");
        assert_eq!(palette.visible_items().len(), 1);
        assert_eq!(
            palette.confirm(),
            Some(CommandPaletteEvent::Select(ActionId::new(
                "workspace.settings"
            )))
        );
    }

    #[test]
    fn host_filtered_results_keep_ranking_for_navigation_and_selection() {
        let mut palette = sample().filtered_items(true);
        assert_eq!(
            palette.set_query("wfs"),
            CommandPaletteEvent::Search("wfs".into())
        );
        assert_eq!(palette.visible_items().len(), 3);
        assert_eq!(
            palette.confirm(),
            Some(CommandPaletteEvent::Select(ActionId::new(
                "workspace.files"
            )))
        );
        palette.navigate(ActionPickerNavigation::Next);
        assert_eq!(
            palette.confirm(),
            Some(CommandPaletteEvent::Select(ActionId::new(
                "workspace.settings"
            )))
        );
        palette.items.clear();
        assert_eq!(palette.confirm(), None);
    }

    #[test]
    fn palette_projects_modal_search_and_rows() {
        let mut context = AppContext::new();
        let palette = context.create_component(document(), sample()).unwrap();
        let id = palette.stable_id();
        assert!(matches!(
            context.world().standard_visual(id),
            Some(StandardVisual::CommandPalette { ref rows, .. }) if rows.len() == 3
        ));
        assert!(context.world().text_input(id).is_some());
    }

    #[test]
    fn palette_commits_ime_into_the_query() {
        let mut context = AppContext::new();
        let palette = context.create_component(document(), sample()).unwrap();
        context.focus_node(document(), palette.stable_id()).unwrap();
        assert!(context.commit_ime(document(), "设").unwrap());
        context
            .read(palette, |palette| {
                assert_eq!(palette.query, "设");
                assert_eq!(palette.visible_items().len(), 1);
            })
            .unwrap();
    }

    #[test]
    fn long_shortcut_reserves_its_estimate_and_stays_clear_of_the_label() {
        let rows = [PaletteRowData {
            action: ActionId::new("workspace.settings"),
            label: Arc::from("打开设置"),
            category: None,
            shortcut: Some(Arc::from("Ctrl+Alt+Delete")),
            selected: false,
        }];
        let geometry = command_palette_geometry(
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 720.0,
                height: 480.0,
            },
            &Arc::from("命令面板"),
            &Arc::from(""),
            &Arc::from("搜索命令"),
            None,
            &rows,
            &nana_ui_core::SemanticPalette::dark(),
            nana_ui_core::UI_METRICS,
            crate::text_width::ChromeTextMeasure::ESTIMATE,
        );
        let crate::ComponentGeometry::CommandPalette { rows, .. } = geometry else {
            panic!("command palette geometry");
        };
        let row = &rows[0];
        let shortcut = row.shortcut.as_ref().expect("shortcut region");
        // "Ctrl+Alt+Delete" at 10px ≈ 93px, far past the old fixed 70px box.
        let estimated =
            crate::text_width::estimated_text_width(shortcut.content.as_ref(), SHORTCUT_TEXT_SIZE);
        assert!(
            estimated > 80.0,
            "estimate {estimated} should exceed the old box"
        );
        assert!(shortcut.bounds.width + 0.01 >= estimated);
        assert!((shortcut.bounds.width - estimated).abs() < 0.5);
        // End edge stays pinned to the row's trailing padding edge.
        let shortcut_end = shortcut.bounds.x + shortcut.bounds.width;
        assert!((shortcut_end - (row.bounds.x + row.bounds.width - ROW_PAD_X)).abs() < 0.01);
        // The label region ends before the shortcut region begins.
        assert!(row.label.bounds.width > 0.0);
        assert!(row.label.bounds.x + row.label.bounds.width <= shortcut.bounds.x);
    }
}

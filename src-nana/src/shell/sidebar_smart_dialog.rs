//! 新建或编辑智能文件夹的对话框，照 Vue `WorkspaceSidebarSmartFolderDialogs.vue`。
//!
//! 名称、父级和全部筛选字段。输入框和多行输入各有一份草稿信号，按 `ModelField` 的规矩回写；
//! 父级候选、两个下拉框的当前值、错误、主按钮文案和禁用放在一个投影信号里。正文是纵向滚动区，
//! 高度由 NanaUI 对话框按窗口限定，底栏始终露在外面（Vue 在 800 高的窗口里会把底栏裁掉）。

use crate::shell::sidebar::{SidebarSmartFolder, SmartFolderField};
use crate::shell::view_part_overlay::dialog::{
    action, action_button, error_line, footer, select_field, text_area_field, text_field, two_columns, Choices, DialogFrame,
    DialogWidth,
};
use crate::shell::view_part_overlay::session::{Draft, Projected};
use crate::shell::{ShellMessage, ShellViewModel, SidebarMessage};
use nana_ui::runtime::view::{fields, signal, widget, AnyView, IntoView, Signal};
use nana_ui::runtime::{LengthSpec, ScrollAxes, ScrollView, Stack};
use nana_ui::ButtonKind;

/// 智能文件夹对话框要显示的东西。各输入框的草稿不在这里，见 [`Draft`]。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SmartDialogView {
    pub title: &'static str,
    pub action: &'static str,
    pub busy: bool,
    /// 保存中或名称为空时主按钮不可用。
    pub blocked: bool,
    /// 父级候选：顶层，加上除了正在编辑的自己以外、按树序列出的智能文件夹。
    pub parents: Choices,
    pub parent: String,
    pub match_mode: String,
    pub sort_direction: String,
    pub error: String,
}

impl SmartDialogView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let draft = &model.sidebar.smart_draft;
        if !draft.open {
            return None;
        }
        let mut parents = vec![(String::new(), "顶层智能文件夹".to_string())];
        flatten(&model.sidebar.smart_folders, &draft.target_id, &mut parents);
        Some(Self {
            title: model.sidebar.smart_dialog_title(),
            action: draft.action_label(),
            busy: draft.busy,
            blocked: draft.busy || draft.name.trim().is_empty(),
            parents,
            parent: draft.value(SmartFolderField::Parent).to_string(),
            match_mode: draft.value(SmartFolderField::Match).to_string(),
            sort_direction: draft.value(SmartFolderField::SortDirection).to_string(),
            error: draft.error.clone(),
        })
    }
}

/// 父级候选：除了正在编辑的自己，按树序列出所有智能文件夹。
fn flatten(folders: &[SidebarSmartFolder], exclude: &str, out: &mut Vec<(String, String)>) {
    for folder in folders {
        if folder.id != exclude {
            out.push((folder.id.clone(), folder.name.clone()));
        }
        flatten(&folder.children, exclude, out);
    }
}

fn set_field(field: SmartFolderField) -> impl Fn(String) -> ShellMessage + Send + Sync + 'static {
    move |value| ShellMessage::Sidebar(SidebarMessage::SetSmartFolderField { field, value })
}

/// 一个字段的草稿：对话框开着时读草稿里这一项。
fn draft(model: &ShellViewModel, field: SmartFolderField) -> Signal<String> {
    Draft::register(model, move |model| {
        let draft = &model.sidebar.smart_draft;
        draft.open.then(|| draft.value(field).to_string())
    })
}

/// 新建或编辑智能文件夹：名称、父级和全部筛选字段。名称为空或保存中时主按钮不可用。
pub fn smart_folder_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(SmartDialogView::project(model)?);
    Projected::register(view, SmartDialogView::project);
    let busy = move || view.with(|view| view.busy);
    let input = |field: SmartFolderField, label: &'static str, key: &'static str, placeholder: &'static str| -> AnyView {
        text_field(label, key, draft(model, field), placeholder, false, busy, set_field(field), None)
    };
    let area = |field: SmartFolderField, label: &'static str, key: &'static str, placeholder: &'static str, rows: f32| -> AnyView {
        text_area_field(label, key, draft(model, field), placeholder, rows, busy, set_field(field))
    };
    let choice = |field: SmartFolderField, label: &'static str, key: &'static str, read: fn(&SmartDialogView) -> String, choices: Choices| -> AnyView {
        let fixed = choices.clone();
        let parents = field == SmartFolderField::Parent;
        select_field(
            label,
            key,
            move || Some(view.with(read)),
            move || if parents { view.with(|view| view.parents.clone()) } else { fixed.clone() },
            busy,
            set_field(field),
        )
    };
    let pairs = |items: &[(&str, &str)]| -> Choices { items.iter().map(|(value, text)| ((*value).to_string(), (*text).to_string())).collect() };
    // `.smart-folder-dialog__grid`：两列等宽，行列间距 12px。
    let body = vec![
        input(SmartFolderField::Name, "名称", "smart-field-name", "例如 高评分 PSD"),
        choice(SmartFolderField::Parent, "父级", "smart-field-parent", |view| view.parent.clone(), Vec::new()),
        two_columns(
            "smart-filter-grid",
            12.0,
            vec![
                input(SmartFolderField::Query, "关键词", "smart-field-query", "文件名、标签或元数据"),
                input(SmartFolderField::Path, "路径前缀", "smart-field-path", "Campaigns/Summer"),
                input(SmartFolderField::Formats, "格式", "smart-field-formats", "psd，png"),
                input(SmartFolderField::Tags, "标签", "smart-field-tags", "封面，主视觉"),
                input(SmartFolderField::Colors, "颜色", "smart-field-colors", "红色，绿色"),
                input(SmartFolderField::Shapes, "形状", "smart-field-shapes", "方形，横版"),
                input(SmartFolderField::MinRating, "最低评分", "smart-field-rating", "4"),
                choice(SmartFolderField::Match, "匹配方式", "smart-field-match", |view| view.match_mode.clone(), pairs(&[("and", "全部匹配"), ("or", "任一匹配")])),
            ],
        ),
        area(SmartFolderField::Metadata, "元数据键值", "smart-field-metadata", "artist=demo\nsource=reference", 3.0),
        two_columns(
            "smart-exclude-grid",
            12.0,
            vec![
                input(SmartFolderField::ExcludeQuery, "排除关键词", "smart-field-exclude-query", "draft，archive"),
                input(SmartFolderField::ExcludePaths, "排除路径", "smart-field-exclude-paths", "Archive，Temp"),
                input(SmartFolderField::ExcludeTags, "排除标签", "smart-field-exclude-tags", "草稿，临时"),
                input(SmartFolderField::ExcludeFormats, "排除格式", "smart-field-exclude-formats", "gif，webp"),
                input(SmartFolderField::SortField, "排序字段", "smart-field-sort-field", "modifiedAt / rating / metadata.width"),
                choice(
                    SmartFolderField::SortDirection,
                    "排序方向",
                    "smart-field-sort-direction",
                    |view| view.sort_direction.clone(),
                    pairs(&[("asc", "升序"), ("desc", "降序")]),
                ),
                input(SmartFolderField::Limit, "结果数量", "smart-field-limit", "100"),
            ],
        ),
        area(SmartFolderField::ExcludeMetadata, "排除元数据", "smart-field-exclude-metadata", "status=archived", 2.0),
        two_columns(
            "smart-range-grid",
            12.0,
            vec![
                area(SmartFolderField::ExcludeNumbers, "排除数值范围", "smart-field-exclude-numbers", "width=0..640", 2.0),
                area(
                    SmartFolderField::ExcludeDates,
                    "排除日期范围",
                    "smart-field-exclude-dates",
                    "fileCreatedAt=2024-01-01T00:00:00Z..2024-01-31T00:00:00Z",
                    2.0,
                ),
            ],
        ),
        area(SmartFolderField::Numbers, "数值范围", "smart-field-numbers", "width=1024..4096\noriginalSizeBytes=..10485760", 2.0),
        area(SmartFolderField::Dates, "日期范围", "smart-field-dates", "fileCreatedAt=2024-01-01T00:00:00Z..2024-12-31T23:59:59Z", 2.0),
        error_line(move || view.with(|view| view.error.clone()), "smart-dialog-error"),
    ];
    // `.smart-folder-dialog__body`：竖排、间距 12，右边留 4 给滚动条。
    let column = Stack::column(12.0).width(LengthSpec::Fill).with_layout(|layout| layout.padding_right = Some(LengthSpec::Px(4.0)));
    let scroll = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.width = Some(LengthSpec::Fill);
        layout.min_height = Some(LengthSpec::Px(0.0));
    }))
    .key("smart-dialog-scroll")
    .children((widget(column).children(body),));
    let close = || ShellMessage::Sidebar(SidebarMessage::CloseSmartFolderDialog);
    let buttons = vec![
        action("取消", ButtonKind::Ghost, busy, "smart-dialog-cancel", close),
        smart_submit(view),
    ];
    Some(
        DialogFrame::new("smart-folder-dialog", move || view.with(|view| view.title.to_string()), close)
            .width(DialogWidth::Wide)
            .busy(busy)
            .dialog(scroll, footer(None, buttons)),
    )
}

/// 「创建 / 保存」：保存中转圈并标忙（Vue `primary` 按钮的 loading）。
fn smart_submit(view: Signal<SmartDialogView>) -> AnyView {
    action_button(
        move || view.with(|view| view.action.to_string()),
        ButtonKind::Primary,
        move || view.with(|view| view.blocked),
        "smart-create-submit",
        || ShellMessage::Sidebar(SidebarMessage::SubmitSmartFolder),
    )
    .prop::<bool, fields::button::loading>(move || view.with(|view| view.busy))
    .into_any()
}

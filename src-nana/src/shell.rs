//! MomoBako 原生应用壳层和页面 ViewModel。
//!
//! 壳层只描述稳定的导航、状态和主内容层级；仓库、插件和任务服务通过
//! `ShellViewModel` 注入文本状态，避免把领域服务直接耦合到 Nana 控件树。

use nana_ui::runtime::view::{button, text, widget};
use nana_ui::runtime::{FrameworkError, LengthSpec, List, RuntimeDocument, Stack};

/// 主内容页面的可观察状态。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ShellPage {
    /// 服务尚未返回仓库结构。
    #[default]
    Loading,
    /// 当前仓库没有可展示文件。
    EmptyRepository,
    /// 仓库服务返回可向用户解释的错误。
    Error,
    /// 展示文件列表。
    FileList,
    /// 展示当前选中的文件及预览入口。
    SelectedFile,
    /// 插件原生设置贡献页。
    PluginSettings,
    /// 任务中心有正在运行的任务。
    TaskRunning,
    /// 当前资源存在同步冲突。
    Conflict,
    /// 编辑器存在尚未保存的内容。
    UnsavedEdit,
    /// 应用设置页。
    Settings,
    /// 系统日志页。
    Logs,
}

impl ShellPage {
    fn title(&self) -> &'static str {
        match self {
            Self::Loading => "正在加载资源库",
            Self::EmptyRepository => "资源库为空",
            Self::Error => "资源库加载失败",
            Self::FileList => "文件列表",
            Self::SelectedFile => "文件预览",
            Self::PluginSettings => "插件设置",
            Self::TaskRunning => "任务进行中",
            Self::Conflict => "同步冲突",
            Self::UnsavedEdit => "编辑未保存",
            Self::Settings => "应用设置",
            Self::Logs => "系统日志",
        }
    }

    fn status(&self) -> &'static str {
        match self {
            Self::Loading => "正在读取仓库结构…",
            Self::EmptyRepository => "还没有可显示的文件",
            Self::Error => "需要处理仓库错误",
            Self::FileList => "已加载仓库文件",
            Self::SelectedFile => "已选中一个文件",
            Self::PluginSettings => "正在编辑官方插件的原生设置",
            Self::TaskRunning => "扫描任务正在运行 · 42%",
            Self::Conflict => "本地与远端 Revision 不一致",
            Self::UnsavedEdit => "编辑内容尚未写入仓库",
            Self::Settings => "应用偏好和服务配置",
            Self::Logs => "最近的服务和插件事件",
        }
    }

    fn primary_action(&self) -> &'static str {
        match self {
            Self::Loading => "取消加载",
            Self::EmptyRepository => "打开文件夹",
            Self::Error => "重试加载",
            Self::FileList => "刷新列表",
            Self::SelectedFile => "打开预览",
            Self::PluginSettings => "保存插件设置",
            Self::TaskRunning => "查看任务",
            Self::Conflict => "查看冲突",
            Self::UnsavedEdit => "保存更改",
            Self::Settings => "应用设置",
            Self::Logs => "刷新日志",
        }
    }
}

/// 壳层所需的宿主无关页面状态。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellViewModel {
    pub page: ShellPage,
    pub repository_name: String,
    pub selected_path: Option<String>,
    pub detail: String,
    pub dirty: bool,
}

impl Default for ShellViewModel {
    fn default() -> Self {
        Self {
            page: ShellPage::default(),
            repository_name: "默认资源库".into(),
            selected_path: None,
            detail: "等待资源库服务响应".into(),
            dirty: false,
        }
    }
}

impl ShellViewModel {
    /// 创建用于验收某一状态的页面模型。
    pub fn for_page(page: ShellPage) -> Self {
        let mut model = Self {
            page,
            ..Self::default()
        };
        match model.page {
            ShellPage::Error => model.detail = "无法读取仓库目录，请检查路径和权限".into(),
            ShellPage::EmptyRepository => model.detail = "可从文件夹或拖放导入资源".into(),
            ShellPage::FileList => model.detail = "12 个文件 · 按名称排序".into(),
            ShellPage::SelectedFile => {
                model.selected_path = Some("assets/cover.png".into());
                model.detail = "PNG 图片 · 1920 × 1080 · 2.4 MB".into();
            }
            ShellPage::PluginSettings => {
                model.detail = "官方插件 · Nana 原生贡献接口 · 已加载 3 项配置".into();
            }
            ShellPage::TaskRunning => {
                model.detail = "扫描默认资源库 · 1,284 / 3,040 个文件".into();
            }
            ShellPage::Conflict => {
                model.selected_path = Some("assets/cover.png".into());
                model.detail = "远端修改时间较新，需要选择保留本地或远端版本".into();
            }
            ShellPage::UnsavedEdit => {
                model.selected_path = Some("notes/readme.md".into());
                model.dirty = true;
                model.detail = "Markdown · 3 行未保存 · 最后保存于 2 分钟前".into();
            }
            ShellPage::Settings => model.detail = "主题、缩略图缓存和默认播放器".into(),
            ShellPage::Logs => model.detail = "最近 24 小时 · 18 条记录 · 0 个错误".into(),
            ShellPage::Loading => {}
        }
        model
    }

    fn selection_label(&self) -> String {
        self.selected_path
            .as_deref()
            .map(|path| format!("当前文件：{path}"))
            .unwrap_or_else(|| "未选择文件".into())
    }

    fn edit_label(&self) -> &'static str {
        if self.dirty {
            "编辑内容 · 未保存"
        } else {
            "编辑内容"
        }
    }
}

/// 在给定 Runtime 文档中挂载完整的 MomoBako 壳层。
pub fn mount_shell(
    document: &mut RuntimeDocument,
    model: &ShellViewModel,
) -> Result<(), FrameworkError> {
    let document_id = document.document();
    let view_model = model.clone();
    document
        .context_mut()
        .mount_view_root(document_id, move || {
            // 标题栏、导航栏和主工作区分别承担窗口级操作、上下文导航和资源主线。
            let navigation = widget(
                Stack::fill_column(8.0)
                    .width(LengthSpec::Px(220.0))
                    .grow(0.0)
                    .shrink(0.0)
                    .padding_xy(16.0, 18.0),
            )
            .children((
                text("资源库").key("nav-library"),
                text("播放列表").key("nav-playlists"),
                text("插件").key("nav-plugins"),
                text("设置").key("nav-settings"),
            ));
            let content = widget(
                Stack::fill_column(12.0)
                    .padding_xy(24.0, 20.0)
                    .min_width(LengthSpec::Px(0.0)),
            )
            .children((widget(
                List::new()
                    .label(view_model.page.title())
                    .style(Stack::column(12.0).node_style()),
            )
            .children((
                text(view_model.page.status()).key("page-status"),
                text(view_model.selection_label()).key("selection"),
                text(view_model.detail.clone()).key("page-detail"),
                button(view_model.page.primary_action()).key("primary-action"),
                button(view_model.edit_label()).key("edit-action"),
            )),));
            let process = widget(
                Stack::fill_column(8.0)
                    .width(LengthSpec::Px(240.0))
                    .grow(0.0)
                    .shrink(0.0)
                    .padding_xy(16.0, 20.0),
            )
            .children((
                text("当前状态").key("process-heading"),
                text(view_model.page.status()).key("process-status"),
            ));
            let title_bar = widget(
                Stack::bar(12.0)
                    .height(LengthSpec::Px(48.0))
                    .padding_xy(20.0, 12.0),
            )
            .children((
                text("MomoBako").key("title"),
                text("资源库工作区").key("subtitle"),
                button("刷新状态").key("refresh"),
            ));
            let body = widget(Stack::fill_row(0.0).min_height(LengthSpec::Px(0.0)))
                .children((navigation, content, process))
                .key("workspace-body");
            widget(Stack::fill_column(0.0).min_width(LengthSpec::Px(0.0)))
                .children((title_bar, body))
        })?;
    Ok(())
}

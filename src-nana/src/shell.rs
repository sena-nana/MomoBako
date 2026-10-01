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
}

impl ShellPage {
    fn title(&self) -> &'static str {
        match self {
            Self::Loading => "正在加载资源库",
            Self::EmptyRepository => "资源库为空",
            Self::Error => "资源库加载失败",
            Self::FileList => "文件列表",
            Self::SelectedFile => "文件预览",
        }
    }

    fn status(&self) -> &'static str {
        match self {
            Self::Loading => "正在读取仓库结构…",
            Self::EmptyRepository => "还没有可显示的文件",
            Self::Error => "需要处理仓库错误",
            Self::FileList => "已加载仓库文件",
            Self::SelectedFile => "已选中一个文件",
        }
    }

    fn primary_action(&self) -> &'static str {
        match self {
            Self::Loading => "取消加载",
            Self::EmptyRepository => "打开文件夹",
            Self::Error => "重试加载",
            Self::FileList => "刷新列表",
            Self::SelectedFile => "打开预览",
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

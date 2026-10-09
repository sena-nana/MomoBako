//! 仓库切换弹层的状态：切换资源库、添加菜单、后端表单和系统文件夹附加。
//!
//! 对应 Vue `useRepositorySwitcherUi.ts`。添加菜单列出所有来源插件；本地文件夹和 Eagle
//! 先关掉弹层再打开系统文件夹对话框，需要登录的来源跳到插件设置，其余来源打开表单。
//! 附加或创建失败时回到添加菜单并显示错误。

use crate::backend::services::repository::PluginManifest;

use super::{PopoverMode, SidebarEffect, SidebarState};

/// Eagle Library 来源插件。
pub const EAGLE_PLUGIN_ID: &str = "momobako.source.eagle-library";
const LOCAL_FILESYSTEM_PLUGIN_ID: &str = "momobako.local-filesystem";
const CLOUD_DRIVE_PLUGIN_ID: &str = "momobako.cloud-drive";
const LOCAL_ROOT_CAPABILITY: &str = "localRootPath";
const AUTHENTICATION_CAPABILITY: &str = "authentication";
/// 旧版没有 `category` 的来源插件按 kind 归类，和 Vue `pluginCategoryForKind` 一致。
const LEGACY_SOURCE_KINDS: [&str; 3] = ["filesystem", "webdav", "cloud"];

/// 添加菜单里的一个来源后端。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendOption {
    pub plugin_id: String,
    /// 菜单里显示的名字。本地文件夹、Eagle 和云盘用固定文案，其余用插件名。
    pub label: String,
    /// 插件名，表单标题用它。
    pub name: String,
    pub description: String,
    /// 插件已启用、是原生后端且状态可用。
    pub enabled: bool,
    pub local_root: bool,
    pub authentication: bool,
}

/// 选了一个后端之后要做的事。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendRoute {
    /// 打开系统文件夹对话框，选完附加本地文件夹。
    PickLocalFolder,
    /// 打开系统文件夹对话框，选完按 Eagle Library 建仓。
    PickEagleLibrary,
    /// 来源需要登录，跳到这个插件的设置页。
    OpenSettings(String),
    /// 打开后端表单。
    Form,
}

/// 从插件清单收出来源后端，对应 Vue `repositoryBackendOptionsFromPlugins`。
pub fn backend_options(plugins: &[PluginManifest]) -> Vec<BackendOption> {
    plugins
        .iter()
        .filter(|plugin| is_source_plugin(plugin))
        .map(|plugin| BackendOption {
            plugin_id: plugin.plugin_id.clone(),
            label: backend_label(&plugin.plugin_id, &plugin.name),
            name: plugin.name.clone(),
            description: plugin.description.clone(),
            enabled: runtime_available(plugin),
            local_root: plugin.capabilities.iter().any(|item| item == LOCAL_ROOT_CAPABILITY),
            authentication: plugin.capabilities.iter().any(|item| item == AUTHENTICATION_CAPABILITY),
        })
        .collect()
}

fn is_source_plugin(plugin: &PluginManifest) -> bool {
    if plugin.category.is_empty() {
        LEGACY_SOURCE_KINDS.contains(&plugin.kind.as_str())
    } else {
        plugin.category == "source"
    }
}

/// Vue `isRepositoryBackendRuntimeAvailable`：启用、后端 SDK、原生动态库，且状态不是不可用或出错。
fn runtime_available(plugin: &PluginManifest) -> bool {
    plugin.enabled
        && plugin.sdk == "backend"
        && plugin.runtime == "native-dylib"
        && plugin.status != "unavailable"
        && plugin.status != "error"
}

fn backend_label(plugin_id: &str, fallback: &str) -> String {
    match plugin_id {
        LOCAL_FILESYSTEM_PLUGIN_ID => "本地文件夹".into(),
        EAGLE_PLUGIN_ID => "Eagle Library".into(),
        CLOUD_DRIVE_PLUGIN_ID => "云盘".into(),
        _ => fallback.to_string(),
    }
}

/// 从 Eagle Library 目录推资源库名：取最后一段并去掉 `.library`，空了用 `Eagle Library`。
pub fn eagle_repository_name(path: &str) -> String {
    let last = path.split(['/', '\\']).filter(|segment| !segment.is_empty()).next_back().unwrap_or("");
    let trimmed = if last.to_ascii_lowercase().ends_with(".library") { &last[..last.len() - ".library".len()] } else { last };
    if trimmed.is_empty() { "Eagle Library".into() } else { trimmed.to_string() }
}

impl SidebarState {
    pub fn popover_is_open(&self) -> bool {
        self.popover != PopoverMode::Closed
    }

    /// 打开切换列表。提交中或已经打开时不动。
    pub fn open_switcher(&mut self) {
        if self.submitting || self.popover == PopoverMode::Switcher {
            return;
        }
        self.popover_error.clear();
        self.popover = PopoverMode::Switcher;
    }

    /// 从切换列表进入添加菜单。提交中不切换，也不清掉错误。
    pub fn show_add_menu(&mut self) -> bool {
        if self.submitting {
            return false;
        }
        self.reset_backend_form(String::new());
        self.popover = PopoverMode::AddMenu;
        true
    }

    pub fn close_popover(&mut self) -> bool {
        if self.submitting {
            return false;
        }
        self.popover = PopoverMode::Closed;
        true
    }

    /// 选择后关闭弹层。提交中保持打开，避免附加过程中换仓库。
    pub fn select_from_switcher(&mut self, _repo_id: &str) -> bool {
        if self.submitting {
            return false;
        }
        self.popover = PopoverMode::Closed;
        true
    }

    pub fn delete_from_switcher(&mut self, active_repo_id: Option<&str>) -> bool {
        if active_repo_id.is_none() || self.submitting {
            return false;
        }
        self.popover_error.clear();
        self.popover = PopoverMode::Closed;
        true
    }

    /// 选了添加菜单里的一个后端。不可用或提交中时返回 `None`。
    ///
    /// 本地文件夹和 Eagle 先关掉弹层，等系统文件夹对话框的结果；登录型来源关掉弹层去设置页；
    /// 其余来源切到表单。
    pub fn choose_backend(&mut self, option: &BackendOption) -> Option<BackendRoute> {
        if self.submitting || !option.enabled {
            return None;
        }
        self.reset_backend_form(option.plugin_id.clone());
        let route = if option.plugin_id == EAGLE_PLUGIN_ID {
            BackendRoute::PickEagleLibrary
        } else if option.local_root {
            BackendRoute::PickLocalFolder
        } else if option.authentication {
            BackendRoute::OpenSettings(option.plugin_id.clone())
        } else {
            BackendRoute::Form
        };
        match &route {
            BackendRoute::PickEagleLibrary => {
                self.attach_eagle = true;
                self.popover = PopoverMode::Closed;
            }
            BackendRoute::PickLocalFolder => {
                self.attach_eagle = false;
                self.popover = PopoverMode::Closed;
            }
            BackendRoute::OpenSettings(_) => self.popover = PopoverMode::Closed,
            BackendRoute::Form => self.popover = PopoverMode::BackendForm,
        }
        Some(route)
    }

    /// 表单的「返回」：回到添加菜单。
    pub fn back_to_add_menu(&mut self) {
        if self.submitting {
            return;
        }
        self.popover = PopoverMode::AddMenu;
    }

    pub fn set_attach_path(&mut self, path: String) {
        self.attach_path = path;
    }

    /// 系统文件夹对话框选好了目录。空白路径不提交，和对话框取消一样。
    /// Eagle 按目录名建仓，本地文件夹直接附加。
    pub fn submit_attach(&mut self) -> bool {
        if self.submitting {
            return false;
        }
        let path = self.attach_path.trim().to_string();
        if path.is_empty() {
            return false;
        }
        self.submitting = true;
        self.popover_error.clear();
        if std::mem::take(&mut self.attach_eagle) {
            self.effects.push(SidebarEffect::CreateBackendRepository {
                name: eagle_repository_name(&path),
                path,
                plugin_id: EAGLE_PLUGIN_ID.into(),
                config: None,
            });
        } else {
            self.effects.push(SidebarEffect::AttachRepository { path });
        }
        true
    }

    /// 空库拖放附加。空白路径不提交，也不打开添加菜单。
    pub(crate) fn queue_attach(&mut self, path: String) {
        let path = path.trim().to_string();
        if path.is_empty() {
            return;
        }
        self.effects.push(SidebarEffect::AttachRepository { path });
    }

    /// 附加或创建结束。成功关弹层；失败回到添加菜单并显示错误（表单失败留在表单）。
    pub fn note_attach_finished(&mut self, result: Result<(), String>) {
        self.submitting = false;
        match result {
            Ok(()) => {
                self.popover = PopoverMode::Closed;
                self.attach_path.clear();
                self.popover_error.clear();
            }
            Err(error) => {
                eprintln!("Nana 附加资源库失败：{error}");
                self.popover_error = error;
                if self.popover != PopoverMode::BackendForm {
                    self.popover = PopoverMode::AddMenu;
                }
            }
        }
    }

    /// 表单「创建」是否可用：服务地址必填。
    pub fn backend_submit_disabled(&self, option: Option<&BackendOption>) -> bool {
        !option.is_some_and(|option| option.enabled) || self.backend_url.trim().is_empty()
    }

    /// 提交后端表单。名称留空时用后端名称，再不行用「新资源库」。
    pub fn submit_backend(&mut self, option: Option<&BackendOption>) {
        if self.submitting || self.backend_submit_disabled(option) {
            return;
        }
        let path = self.backend_url.trim().to_string();
        let name = if self.backend_name.trim().is_empty() {
            option.map(|option| option.name.clone()).filter(|name| !name.is_empty()).unwrap_or_else(|| "新资源库".into())
        } else {
            self.backend_name.trim().to_string()
        };
        let mut config = serde_json::Map::new();
        config.insert("baseUrl".into(), serde_json::Value::String(path.clone()));
        if !self.backend_user.trim().is_empty() {
            config.insert("username".into(), serde_json::Value::String(self.backend_user.trim().to_string()));
        }
        if !self.backend_password.trim().is_empty() {
            config.insert("password".into(), serde_json::Value::String(self.backend_password.trim().to_string()));
        }
        config.insert("rootPath".into(), serde_json::Value::String(self.backend_root.trim().to_string()));
        self.submitting = true;
        self.popover_error.clear();
        self.effects.push(SidebarEffect::CreateBackendRepository {
            name,
            path,
            plugin_id: self.backend_plugin_id.clone(),
            config: Some(serde_json::Value::Object(config)),
        });
    }

    fn reset_backend_form(&mut self, plugin_id: String) {
        self.backend_plugin_id = plugin_id;
        self.backend_name.clear();
        self.backend_url.clear();
        self.backend_user.clear();
        self.backend_password.clear();
        self.backend_root.clear();
        self.popover_error.clear();
        self.attach_eagle = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(id: &str, category: &str, capabilities: &[&str], enabled: bool) -> PluginManifest {
        serde_json::from_value(serde_json::json!({
            "pluginId": id,
            "name": format!("插件 {id}"),
            "version": "1.0.0",
            "kind": "source",
            "category": category,
            "description": "说明",
            "capabilities": capabilities,
            "enabled": enabled,
            "sdk": "backend",
            "entry": null,
            "source": "builtin",
            "runtime": "native-dylib",
            "permissions": [],
            "compat": {"sdkVersion": "1"},
            "status": "ready"
        }))
        .expect("插件清单")
    }

    #[test]
    fn backend_options_follow_vue_labels_and_routes() {
        let plugins = vec![
            plugin(LOCAL_FILESYSTEM_PLUGIN_ID, "source", &["localRootPath"], true),
            plugin(EAGLE_PLUGIN_ID, "source", &[], true),
            plugin("momobako.source.login", "source", &["authentication"], true),
            plugin("momobako.source.dav", "source", &[], true),
            plugin("momobako.preview.pdf", "preview", &[], true),
            plugin("momobako.source.off", "source", &["localRootPath"], false),
        ];
        let options = backend_options(&plugins);
        assert_eq!(
            options.iter().map(|option| option.label.as_str()).collect::<Vec<_>>(),
            ["本地文件夹", "Eagle Library", "插件 momobako.source.login", "插件 momobako.source.dav", "插件 momobako.source.off"]
        );
        let mut sidebar = SidebarState::default();
        sidebar.open_switcher();
        assert!(sidebar.show_add_menu());
        assert_eq!(sidebar.choose_backend(&options[4]), None, "未启用的后端不能选");
        assert_eq!(sidebar.choose_backend(&options[0]), Some(BackendRoute::PickLocalFolder));
        assert_eq!(sidebar.popover, PopoverMode::Closed);
        sidebar.popover = PopoverMode::AddMenu;
        assert_eq!(sidebar.choose_backend(&options[1]), Some(BackendRoute::PickEagleLibrary));
        sidebar.attach_path = "D:/素材/Anime.library".into();
        assert!(sidebar.submit_attach());
        assert!(matches!(
            sidebar.take_effects().as_slice(),
            [SidebarEffect::CreateBackendRepository { name, plugin_id, .. }] if name == "Anime" && plugin_id == EAGLE_PLUGIN_ID
        ));
        sidebar.note_attach_finished(Ok(()));
        sidebar.popover = PopoverMode::AddMenu;
        assert_eq!(
            sidebar.choose_backend(&options[2]),
            Some(BackendRoute::OpenSettings("momobako.source.login".into()))
        );
        sidebar.popover = PopoverMode::AddMenu;
        assert_eq!(sidebar.choose_backend(&options[3]), Some(BackendRoute::Form));
        assert_eq!(sidebar.popover, PopoverMode::BackendForm);
        assert!(sidebar.backend_submit_disabled(Some(&options[3])), "服务地址必填");
        sidebar.backend_url = " https://example.com/dav/ ".into();
        sidebar.backend_user = "alice".into();
        sidebar.submit_backend(Some(&options[3]));
        assert!(sidebar.submitting);
        let effects = sidebar.take_effects();
        let Some(SidebarEffect::CreateBackendRepository { name, path, config: Some(config), .. }) = effects.first() else {
            panic!("表单没有提交：{effects:?}");
        };
        assert_eq!(name, "插件 momobako.source.dav");
        assert_eq!(path, "https://example.com/dav/");
        assert_eq!(config["username"], "alice");
        assert_eq!(config["rootPath"], "");
        sidebar.note_attach_finished(Err("连接失败".into()));
        assert_eq!(sidebar.popover, PopoverMode::BackendForm, "表单失败留在表单");
        assert_eq!(sidebar.popover_error, "连接失败");
    }

    #[test]
    fn eagle_names_drop_the_library_suffix() {
        assert_eq!(eagle_repository_name("D:\\素材\\动画.library\\"), "动画");
        assert_eq!(eagle_repository_name("/"), "Eagle Library");
        assert_eq!(eagle_repository_name("E:/Refs"), "Refs");
    }
}

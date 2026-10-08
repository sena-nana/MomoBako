//! 来源插件还缺的目录和名称。
//!
//! 下载先排队系统文件夹对话框，选中后再 `call_plugin`。取消不调用。
//! 创建来源播放列表弹出名称输入，空白不提交，确认后带上名称和当前仓库。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, Dialog, Stack, TextChanged, TextInput};
use serde_json::Value;

use super::super::admin::AdminMessage;
use super::super::{ShellMessage, ShellViewModel};
use super::InputMessage;

/// 已经点过下载、还在等目录的插件动作。
#[derive(Clone, Debug)]
pub struct PendingDownload {
    pub plugin_id: String,
    pub method: String,
    pub payload: Value,
    pub repository_id: Option<String>,
}

/// 正在问名称的来源播放列表。
#[derive(Clone, Debug)]
pub struct SourcePlaylistPrompt {
    pub plugin_id: String,
    pub method: String,
    pub payload: Value,
    pub repository_id: Option<String>,
    pub draft: String,
}

/// 把选中的目录写进 `destination` 再调用。没有待下载动作时只记日志。
pub(super) fn submit_download(model: &mut ShellViewModel, path: String) {
    let Some(mut pending) = model.input.pending_download.take() else {
        eprintln!("Nana 下载目录没有对应的插件动作");
        return;
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        eprintln!("Nana 取消下载目录选择");
        return;
    }
    if let Some(object) = pending.payload.as_object_mut() {
        object.insert(
            "destination".into(),
            serde_json::json!({ "kind": "localFolder", "path": trimmed }),
        );
    }
    model.reduce(ShellMessage::Admin(AdminMessage::CallFilePlugin {
        plugin_id: pending.plugin_id,
        method: pending.method,
        payload: pending.payload,
        repository_id: pending.repository_id,
    }));
}

/// 名称去掉空白后仍为空就不调用。确认后载荷里同时有名称和当前仓库。
pub(super) fn submit_source_playlist(model: &mut ShellViewModel) {
    let Some(mut prompt) = model.input.source_playlist.clone() else {
        eprintln!("Nana 没有待确认的来源播放列表");
        return;
    };
    let name = prompt.draft.trim();
    if name.is_empty() {
        eprintln!("Nana 来源播放列表名称是空的");
        return;
    }
    let name = name.to_string();
    if let Some(object) = prompt.payload.as_object_mut() {
        object.insert("name".into(), Value::String(name.clone()));
        object.insert("playlistName".into(), Value::String(name));
        if let Some(repo) = &prompt.repository_id {
            object.insert("repositoryId".into(), Value::String(repo.clone()));
        }
    }
    model.input.source_playlist = None;
    model.reduce(ShellMessage::Admin(AdminMessage::CallFilePlugin {
        plugin_id: prompt.plugin_id,
        method: prompt.method,
        payload: prompt.payload,
        repository_id: prompt.repository_id,
    }));
}

/// 名称对话框。没有提示时不占浮层。空白时确认按钮不可用。
pub(crate) fn playlist_name_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let prompt = model.input.source_playlist.as_ref()?;
    let blank = prompt.draft.trim().is_empty();
    Some(
        widget(Dialog::new("创建来源播放列表"))
            .key("source-playlist-dialog")
            .body(
                widget(TextInput::new(prompt.draft.clone()).label("播放列表名称"))
                    .key("source-playlist-name")
                    .on_cx(|_, event: &TextChanged, cx| {
                        cx.dispatch_program(ShellMessage::Input(InputMessage::SourcePlaylistDraft(event.value.to_string())));
                    }),
            )
            .footer(widget(Stack::row(8.0)).children((
                widget(super::super::workbench::ghost_button("取消")).key("source-playlist-cancel").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Input(InputMessage::CloseSourcePlaylist));
                }),
                widget(super::super::workbench::primary_button("确认")).key("source-playlist-submit").disabled(blank).on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Input(InputMessage::SubmitSourcePlaylist));
                }),
            )))
            .into_any(),
    )
}

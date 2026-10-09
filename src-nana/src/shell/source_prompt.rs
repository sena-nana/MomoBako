//! 来源插件还缺的目录和名称。
//!
//! 下载先排队系统文件夹对话框，选中后再 `call_plugin`。取消不调用。
//! 创建来源播放列表弹出名称输入（统一对话框框架，常驻、输入框受控），空白不提交，确认后带上名称和当前仓库。

use std::sync::Arc;

use nana_ui::runtime::view::{signal, AnyView};
use nana_ui::ButtonKind;
use serde_json::Value;

use super::super::admin::AdminMessage;
use super::super::view_part_overlay::dialog::{action, footer, text_field, DialogFrame, MODAL_CARD};
use super::super::view_part_overlay::session::{Draft, Projected};
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

/// 名称对话框要显示的东西。名称草稿不在这里，见 [`Draft`]。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourcePlaylistView {
    /// 名称去掉空白后为空：「确认」不可用。
    pub blank: bool,
}

impl SourcePlaylistView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let prompt = model.input.source_playlist.as_ref()?;
        Some(Self { blank: prompt.draft.trim().is_empty() })
    }
}

fn input_message(message: InputMessage) -> ShellMessage {
    ShellMessage::Input(message)
}

/// 名称对话框。没有提示时不占浮层。空白时确认按钮不可用，回车和确认都提交，归约里再挡一次空白。
pub(crate) fn playlist_name_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(SourcePlaylistView::project(model)?);
    Projected::register(view, SourcePlaylistView::project);
    let draft = Draft::register(model, |model| model.input.source_playlist.as_ref().map(|prompt| prompt.draft.clone()));
    let submit = || input_message(InputMessage::SubmitSourcePlaylist);
    let close = || input_message(InputMessage::CloseSourcePlaylist);
    let body = text_field(
        "播放列表名称",
        "source-playlist-name",
        draft,
        "",
        false,
        false,
        |value| input_message(InputMessage::SourcePlaylistDraft(value)),
        Some(Arc::new(submit)),
    );
    let buttons = vec![
        action("取消", ButtonKind::Ghost, false, "source-playlist-cancel", close),
        action("确认", ButtonKind::Primary, move || view.with(|view| view.blank), "source-playlist-submit", submit),
    ];
    Some(DialogFrame::new("source-playlist-dialog", || "创建来源播放列表".to_string(), close).size(MODAL_CARD).dialog(body, footer(None, buttons)))
}

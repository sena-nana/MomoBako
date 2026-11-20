//! 详情与像素回写、开始预览、预览内容回写、共享媒体会话与播放集条目打开。

use super::*;

impl InspectState {
    /// 测试用：按扩展名打开预览，并用水合后的元数据草稿替换上一份（等于双击打开）。
    #[cfg(test)]
    pub(in super::super) fn note_detail(&mut self, detail: &AssetDetail) {
        self.note_selected_detail(detail, true);
    }

    /// 记下素材详情。预览页的四个输入随后按列表条目改写，单击详情卡按导语叠层改写。正在输入时都不覆盖。
    /// `preview` 时再按扩展名读预览内容。单击不挂预览：不读内容、不接管播放条、不准备播放源，也不记访问。
    pub(in super::super) fn note_selected_detail(&mut self, detail: &AssetDetail, preview: bool) {
        let extension = support::extension_of(&detail.summary.extension, &detail.summary.filename);
        let kind = support::classify(&extension);
        self.generation += 1;
        self.target_path = Some(detail.summary.path.clone());
        self.repo_id = Some(detail.summary.repo_id.clone());
        self.asset_id = Some(detail.summary.asset_id.clone());
        self.expected_version = detail.summary.version;
        self.virtual_asset = detail.summary.is_virtual;
        self.local_absolute_path = detail.summary.local_absolute_path.clone();
        self.kind = Some(kind.clone());
        self.error.clear();
        self.conflict.clear();
        self.conflict_detail = None;
        // 上一个素材的保存还在路上时 `saving` 留着：同一时刻只发一次保存，结果按 `in_flight` 认素材。
        // 预览页四个输入随后按列表改写，单击详情卡按导语叠层改写。正在输入时不让这份详情冲掉草稿。
        if self.dirty() {
            eprintln!("Nana 还没保存的元数据草稿不随素材详情换掉");
        } else {
            let draft = MetadataDraft::from_detail(detail);
            self.baseline = draft.clone();
            self.draft = draft;
        }
        self.palette = super::super::palette::from_metadata_entries(&detail.metadata);
        self.facts = FileFacts {
            extension: extension.clone(),
            size_label: detail.summary.size_label.clone(),
            modified_at: detail.summary.modified_at.clone(),
            hardlink_state: detail.summary.hardlink_state.clone(),
            thumbnail_path: detail.summary.thumbnail_path.clone(),
            ..FileFacts::default()
        };
        support::fill_recorded_facts(&mut self.facts, &detail.metadata);
        self.timers.clear_metadata();
        if preview {
            self.start_preview(&detail.summary.repo_id, &detail.summary.path, kind);
        } else {
            self.loading = false;
            self.body = PreviewBody::Empty;
        }
    }

    pub(in super::super) fn note_detail_error(&mut self, error: &str) {
        eprintln!("Nana 读取文件元数据失败：{error}");
        self.loading = false;
        self.body = PreviewBody::Failed(error.to_string());
        self.error = error.to_string();
    }

    /// 图片解码失败是错误态。路径与当前目标不一致时保留原预览。
    pub(in super::super) fn note_pixels(
        &mut self,
        path: &str,
        pixels_ok: bool,
        error: Option<&str>,
    ) -> bool {
        if self
            .target_path
            .as_ref()
            .is_some_and(|target| target != path)
        {
            eprintln!("Nana 忽略过期的图片预览：{path}");
            return false;
        }
        self.loading = false;
        if pixels_ok {
            self.body = PreviewBody::Image;
            self.error.clear();
        } else {
            let message = error.unwrap_or("图片解码失败").to_string();
            eprintln!("Nana 图片预览失败：{message}");
            self.body = PreviewBody::Failed(message.clone());
            self.error = message;
        }
        true
    }

    pub(super) fn start_preview(&mut self, repo_id: &str, path: &str, kind: PreviewKind) {
        self.loading = false;
        match kind {
            PreviewKind::Image => {
                self.loading = true;
                self.body = PreviewBody::Empty;
                self.effects.push(InspectEffect::LoadImage {
                    repo_id: repo_id.to_string(),
                    path: path.to_string(),
                });
            }
            PreviewKind::Markdown | PreviewKind::Text => {
                let markdown = matches!(kind, PreviewKind::Markdown);
                self.loading = true;
                self.body = PreviewBody::Empty;
                self.effects.push(InspectEffect::LoadText {
                    repo_id: repo_id.to_string(),
                    path: path.to_string(),
                    markdown,
                    generation: self.generation,
                });
            }
            PreviewKind::Media => support::begin_media(self, repo_id, path),
            PreviewKind::Native { view_id, label } => {
                native_preview::begin(self, repo_id, path, view_id, label)
            }
            PreviewKind::Unsupported => {
                let message = "无法预览此类型".to_string();
                eprintln!("Nana 无法预览此类型：{path}");
                self.body = PreviewBody::Failed(message.clone());
                self.error = message;
            }
        }
    }

    pub(super) fn note_body(
        &mut self,
        path: String,
        markdown: bool,
        generation: u64,
        result: Result<support::PreviewText, String>,
    ) {
        if generation != self.generation || self.target_path.as_deref() != Some(path.as_str()) {
            eprintln!("Nana 忽略过期的文本预览：{path}");
            return;
        }
        self.loading = false;
        match result {
            Ok(read) => {
                self.body = PreviewBody::Document {
                    markdown,
                    text: read.text,
                    truncated_at: read.truncated_at,
                };
                self.error.clear();
            }
            Err(error) => {
                eprintln!("Nana 读取文本预览失败：{error}");
                self.body = PreviewBody::Failed(error.clone());
                self.error = error;
            }
        }
    }

    pub(crate) fn media_session(&self) -> Option<&PlaybackSessionState> {
        match &self.body {
            PreviewBody::Media(session) => Some(session),
            _ => None,
        }
    }

    /// 播放列表当前项的画面。空则清掉上一份，避免旧帧留在新文件上。
    pub(crate) fn install_clip_frames(&mut self, frames: Option<Vec<support::VideoFrame>>) {
        bridge::install_video(self, frames);
    }

    /// 只有预览体已经是音视频时，才把共用会话写回去。
    pub(crate) fn replace_shared_media(&mut self, session: PlaybackSessionState) {
        if !matches!(self.body, PreviewBody::Media(_)) {
            return;
        }
        self.error = session.error.clone().unwrap_or_default();
        self.body = PreviewBody::Media(session);
    }

    /// 从播放列表打开文件。有素材 id 时再读取详情。
    pub(crate) fn open_playlist_item(&mut self, path: &str, repo_id: &str, asset_id: &str) {
        self.begin_selection(path);
        self.repo_id = Some(repo_id.to_string());
        if !asset_id.is_empty() {
            self.effects.push(InspectEffect::LoadAsset {
                repo_id: repo_id.to_string(),
                asset_id: asset_id.to_string(),
            });
        }
    }
}

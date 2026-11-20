//! 元数据草稿的保存、补丁应用、冲突采纳与保存结果处理。

use super::*;

impl InspectState {
    pub(super) fn save_metadata(&mut self) {
        if self.saving {
            eprintln!("Nana 元数据保存进行中");
            return;
        }
        if self.virtual_asset || self.asset_id.is_none() {
            eprintln!("Nana 当前素材不能编辑元数据");
            return;
        }
        if !self.dirty() {
            return;
        }
        let Some(repo_id) = self.repo_id.clone() else {
            eprintln!("Nana 保存元数据没有活动仓库");
            return;
        };
        let asset_id = self.asset_id.clone().unwrap_or_default();
        let metadata = self.draft.to_metadata_map(&self.baseline);
        self.saving = true;
        self.error.clear();
        self.in_flight = Some(SaveInFlight {
            asset_id: asset_id.clone(),
            sent: self.draft.clone(),
            tags_written: metadata.contains_key("tagGroups"),
        });
        self.effects.push(InspectEffect::SaveMetadata {
            repo_id,
            asset_id,
            expected_version: self.expected_version,
            metadata,
        });
    }

    /// 按原始 JSON 类型给当前素材写一组元数据（ASMR 候选的应用、导入和抓取结果），并立刻保存，
    /// 编辑器里还没存的改动一起写出去。
    ///
    /// `asset_id` 必须还是正在看的素材，补丁不会落到别的文件上。编辑器四个输入的键不收（记日志跳过）。
    /// 上一次保存还在路上时补丁先进草稿，结果回来以后接着保存（见 [`MetadataDraft::rebase`]）。
    /// 拒绝写入时返回给用户看的原因。
    pub(crate) fn apply_metadata_patch(
        &mut self,
        asset_id: &str,
        patch: BTreeMap<String, Value>,
    ) -> Result<(), String> {
        if self.asset_id.as_deref() != Some(asset_id) {
            eprintln!("Nana 元数据补丁的素材已经不是当前文件：{asset_id}");
            return Err("当前文件已经换了，没有写入".into());
        }
        if self.virtual_asset {
            eprintln!("Nana 虚拟素材不能写元数据补丁：{asset_id}");
            return Err("虚拟条目不能写元数据".into());
        }
        if !self.conflict.is_empty() {
            eprintln!("Nana 元数据冲突未处理，补丁不写入：{asset_id}");
            return Err("版本冲突，先采用服务器版本再写入".into());
        }
        for (key, value) in patch {
            let key = key.trim().to_string();
            if key.is_empty() || EDITABLE_METADATA.contains(&key.as_str()) {
                eprintln!("Nana 元数据补丁跳过编辑器自己的字段：{key}");
                continue;
            }
            self.draft.custom.insert(key, value);
        }
        self.timers.clear_metadata();
        if !self.saving {
            self.save_metadata();
        }
        Ok(())
    }

    pub(super) fn adopt_conflict(&mut self) {
        let Some(detail) = self.conflict_detail.clone() else {
            return;
        };
        let draft = MetadataDraft::from_detail(&detail);
        self.expected_version = detail.summary.version;
        self.baseline = draft.clone();
        self.draft = draft;
        self.conflict.clear();
        self.conflict_detail = None;
        self.error.clear();
    }

    /// 保存结果。结果按 `in_flight` 认素材：
    /// - 是正在看的素材：成功时换上服务器的版本和元数据，发出以后才写进草稿的改动留着并接着保存；冲突时留住草稿、
    ///   给「采用服务器版本」。
    /// - 换选前写出去的上一个素材：不动当前素材的草稿、冲突和预览，当前素材等着写的补丁接着保存；
    ///   上一个素材的冲突写进 `error`，由状态区报出来。
    pub(super) fn note_metadata_result(&mut self, outcome: &str, detail: AssetDetail) {
        self.saving = false;
        let asset_id = detail.summary.asset_id.clone();
        let flight = self
            .in_flight
            .take()
            .filter(|flight| flight.asset_id == asset_id);
        if outcome != "success" && outcome != "conflict" {
            self.note_metadata_error(format!("元数据结果无法识别：{outcome}"));
            return;
        }
        if self.asset_id.as_deref() != Some(asset_id.as_str()) {
            eprintln!("Nana 收到别的素材的元数据保存结果：{asset_id}（{outcome}）");
            self.save_pending();
            if outcome == "conflict" {
                self.error = format!("「{}」版本冲突，修改没有写入", detail.summary.filename);
            }
            return;
        }
        if outcome == "conflict" {
            eprintln!("Nana 元数据版本冲突：{asset_id}");
            self.conflict = "版本冲突，未写入".into();
            self.conflict_detail = Some(detail);
            return;
        }
        let server = MetadataDraft::from_detail(&detail);
        self.draft = match flight {
            Some(flight) => self.draft.rebase(&flight.sent, &server),
            None => server.clone(),
        };
        self.baseline = server;
        self.expected_version = detail.summary.version;
        self.virtual_asset = detail.summary.is_virtual;
        self.local_absolute_path = detail.summary.local_absolute_path.clone();
        self.conflict.clear();
        self.conflict_detail = None;
        self.error.clear();
        self.save_pending();
    }

    /// 保存失败。失败的是换选前的上一个素材时，当前素材等着写的补丁照常写出去；失败的是当前素材时
    /// 草稿留着，不立刻重试（同样的原因多半还会失败），下一次编辑再自动保存。
    pub(super) fn note_metadata_error(&mut self, error: String) {
        eprintln!("Nana 保存元数据失败：{error}");
        self.saving = false;
        let flight = self.in_flight.take();
        if flight.is_some_and(|flight| self.asset_id.as_deref() != Some(flight.asset_id.as_str())) {
            self.save_pending();
        }
        self.error = error;
    }

    /// 上一次保存回来以后，当前素材的草稿里还有没写出去的改动（保存期间补进来的补丁或编辑）时接着保存。
    /// 冲突没处理时不发，免得带着旧版本号再撞一次。
    pub(super) fn save_pending(&mut self) {
        if self.dirty() && self.conflict.is_empty() {
            self.save_metadata();
        }
    }

    /// 这条保存结果回来以后要重读哪个仓库的摘要：成功写进了标签组的保存，后端重排了这个素材的索引标签，
    /// 侧栏「未标签」计数、筛选栏和标签菜单的候选都要跟着变。要在归约这条结果之前问，归约会收掉 `in_flight`。
    pub(in super::super) fn retagged_repository(&self, message: &InspectMessage) -> Option<String> {
        let InspectMessage::MetadataSaved(Ok((outcome, detail))) = message else {
            return None;
        };
        let retagged = outcome == "success"
            && self.in_flight.as_ref().is_some_and(|flight| {
                flight.asset_id == detail.summary.asset_id && flight.tags_written
            });
        retagged.then(|| detail.summary.repo_id.clone())
    }
}

impl MetadataDraft {
    pub(super) fn from_detail(detail: &AssetDetail) -> Self {
        let metadata = &detail.metadata;
        Self {
            rating: support::metadata_rating(metadata),
            comment: support::metadata_string(metadata, "comment")
                .or_else(|| support::metadata_string(metadata, "note"))
                .unwrap_or_default(),
            link: support::metadata_string(metadata, "link").unwrap_or_default(),
            tags: support::metadata_tags(metadata).unwrap_or_else(|| detail.summary.tags.clone()),
            custom: metadata
                .iter()
                .filter(|entry| !EDITABLE_METADATA.contains(&entry.key.as_str()))
                .map(|entry| (entry.key.clone(), entry.value.clone()))
                .collect(),
        }
    }

    /// 保存请求的元数据：只写和 `baseline` 不同的，也就是这次改过的。后端按键覆盖写入，没动过的不回写：
    /// 旧格式的标签组（`{ label, tags }`）不会因为只改了评分就被改写成展平的字符串，小数评分、只有旧键 note 的注释
    /// 也照原样留着；补丁写进来的字段按原始 JSON 类型写，数组和数字不会变成字符串。
    pub(super) fn to_metadata_map(&self, baseline: &MetadataDraft) -> BTreeMap<String, Value> {
        let mut metadata = BTreeMap::new();
        if self.rating != baseline.rating {
            metadata.insert("rating".into(), Value::from(self.rating));
        }
        if self.comment != baseline.comment {
            metadata.insert(
                "comment".into(),
                Value::String(self.comment.trim().to_string()),
            );
        }
        if self.link != baseline.link {
            metadata.insert("link".into(), Value::String(self.link.trim().to_string()));
        }
        if self.tags != baseline.tags {
            metadata.insert(
                "tagGroups".into(),
                Value::Array(self.tags.iter().cloned().map(Value::String).collect()),
            );
        }
        for (key, value) in &self.custom {
            if baseline.custom.get(key) != Some(value) {
                metadata.insert(key.clone(), value.clone());
            }
        }
        metadata
    }

    /// 保存成功以后的新草稿：以服务器读回的 `server` 为底，只留下发出（`sent`）以后草稿里又改过的字段。
    ///
    /// 算法：评分、注释、链接、标签组逐个比，现在的值和发出时不同就用现在的，否则用服务器的；补丁字段按键同样比，
    /// 发出以后新写的键或改过的值留下。这样保存期间补进来的候选不会被这次结果冲掉，也不会把服务器整理过的值
    /// （例如去掉首尾空白的注释）换回旧的。
    pub(super) fn rebase(&self, sent: &MetadataDraft, server: &MetadataDraft) -> MetadataDraft {
        fn pick<T: Clone + PartialEq>(now: &T, sent: &T, server: &T) -> T {
            if now != sent {
                now.clone()
            } else {
                server.clone()
            }
        }
        let mut custom = server.custom.clone();
        for (key, value) in &self.custom {
            if sent.custom.get(key) != Some(value) {
                custom.insert(key.clone(), value.clone());
            }
        }
        MetadataDraft {
            rating: pick(&self.rating, &sent.rating, &server.rating),
            comment: pick(&self.comment, &sent.comment, &server.comment),
            link: pick(&self.link, &sent.link, &server.link),
            tags: pick(&self.tags, &sent.tags, &server.tags),
            custom,
        }
    }
}

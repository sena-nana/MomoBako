//! 同一个仓库刷新以后重读素材详情。
//!
//! 对照 Vue `selectRepository` 原地分支：摘要一到，当前素材还在这份摘要里就重读它，否则改记第一个。
//! 先清掉详情卡上叠着的元数据（`activeAssetDetail = null`），再请求 `getAssetDetail`。
//! 选中的路径、预览页和播放都不换。编辑器有未保存的改动时不覆盖草稿。

use crate::backend::services::repository::{AssetDetail, RepositorySnapshot};

use super::super::ShellViewModel;
use super::{InspectEffect, InspectState};

impl ShellViewModel {
    /// 摘要到了就排一次重读。没有可记的素材时只清掉叠层。
    pub(in crate::shell) fn reload_asset_after_refresh(&mut self, snapshot: &RepositorySnapshot) {
        let previous = self.inspect.current_asset_id().map(str::to_string);
        let chosen = previous
            .as_deref()
            .filter(|id| snapshot.assets.iter().any(|asset| asset.asset_id == *id))
            .map(str::to_string)
            .or_else(|| snapshot.assets.first().map(|asset| asset.asset_id.clone()))
            .filter(|id| !id.trim().is_empty());
        self.files.clear_detail_metadata();
        let Some(asset_id) = chosen else {
            eprintln!("Nana 刷新后的摘要里没有素材，不重读详情");
            return;
        };
        if previous.as_deref() != Some(asset_id.as_str()) {
            eprintln!("Nana 刷新后当前素材不在摘要里，改记第一个：{asset_id}");
        }
        let repo_id = snapshot.repository.repo_id.clone();
        self.inspect.queue_refresh_asset(repo_id, asset_id);
    }

    /// 重读的结果。别的仓库丢掉。失败只记日志：Vue 写进全局错误、文件列整页变红，这里不照抄。
    pub(in crate::shell) fn note_refresh_asset(&mut self, result: Result<AssetDetail, String>) {
        let detail = match result {
            Ok(detail) => detail,
            Err(error) => {
                eprintln!("Nana 刷新后重读素材详情失败：{error}");
                return;
            }
        };
        if self.workspace.active_repo_id.as_deref() != Some(detail.summary.repo_id.as_str()) {
            eprintln!(
                "Nana 忽略别的仓库的刷新素材详情：{}",
                detail.summary.repo_id
            );
            return;
        }
        self.files.note_detail_metadata(&detail);
        self.inspect
            .note_refreshed_detail(&detail, selected_asset_id(&self.files));
    }
}

impl InspectState {
    /// 排一次刷新重读。和单击的 `LoadAsset` 分开，回来不改选中路径。
    pub(in crate::shell) fn queue_refresh_asset(&mut self, repo_id: String, asset_id: String) {
        self.effects
            .push(InspectEffect::ReloadAsset { repo_id, asset_id });
    }

    /// 读回的就是当前选中文件、而且没有未保存改动时，编辑器换成服务器上的值。
    /// 不是这一条，或草稿还脏着，编辑器不动。路径和预览都不改。
    pub(in crate::shell) fn note_refreshed_detail(
        &mut self,
        detail: &AssetDetail,
        selected_asset_id: Option<&str>,
    ) {
        let asset_id = detail.summary.asset_id.as_str();
        if selected_asset_id != Some(asset_id) {
            eprintln!("Nana 刷新读回的素材不是当前选中的文件，编辑器不换：{asset_id}");
            return;
        }
        if self.dirty() {
            eprintln!("Nana 刷新读回的素材详情不覆盖还没保存的编辑：{asset_id}");
            return;
        }
        let draft = super::MetadataDraft::from_detail(detail);
        self.asset_id = Some(asset_id.to_string());
        self.repo_id = Some(detail.summary.repo_id.clone());
        self.expected_version = detail.summary.version;
        self.virtual_asset = detail.summary.is_virtual;
        self.local_absolute_path = detail.summary.local_absolute_path.clone();
        self.baseline = draft.clone();
        self.draft = draft;
    }
}

/// 当前选中那一行的素材 id。目录、分类和智能文件夹结果都认。
fn selected_asset_id(files: &super::super::files::FilesState) -> Option<&str> {
    let path = files.primary.as_deref()?;
    files
        .rows
        .iter()
        .chain(files.virtual_rows.iter())
        .chain(files.category_rows.iter())
        .find(|row| row.path == path)
        .and_then(|row| row.asset_id.as_deref())
        .filter(|id| !id.is_empty())
}

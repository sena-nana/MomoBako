//! 在线条目的播放源准备：预览页和播放条装载结果里共用的附带说明。
//!
//! 网易云这类来源资源库里的单曲是虚拟条目（`is_virtual`），本地没有文件。播放和预览它们之前要先经后端的
//! Mutsuki 协议 `momobako.playback.prepare`，由来源插件把音频准备到资源库的本地缓存，再读这个本地文件解码
//! （宿主派发在 `src-nana/src/playback_source.rs`）。本地条目照旧直接读字节，不走这一步。

/// 播放源准备好时后端写下的一次访问：哪个资源库的哪一条、什么时候。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedAccess {
    pub repo_id: String,
    pub path: String,
    pub at: String,
}

/// 一次装载附带的播放源准备结果。本地文件没有这一步，保持默认。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourcePrep {
    /// 播放源准备好了：后端在这一刻写下了访问时间，归约照 Vue `emitRepositoryEntryAccess` 当场改「最近使用」；
    /// 结果过期了也改，因为后端已经写了。
    pub accessed: Option<PreparedAccess>,
    /// 失败出在准备播放源（来源登录失效、网络不通、缓存目录不可用、准备好的文件读不出）：可以重试。
    /// 读到本地文件以后的解码失败重试也是一样的结果，为假。
    pub retryable: bool,
}

impl SourcePrep {
    /// 播放源准备好了，后端在 `at` 写下了这一条的访问时间。
    pub fn prepared(repo_id: &str, path: &str, at: &str) -> Self {
        Self {
            accessed: Some(PreparedAccess { repo_id: repo_id.to_string(), path: path.to_string(), at: at.to_string() }),
            retryable: false,
        }
    }

    /// 准备播放源失败，可以重试。
    pub fn failed() -> Self {
        Self { accessed: None, retryable: true }
    }
}

#[cfg(test)]
#[path = "playback_prep_tests.rs"]
mod tests;

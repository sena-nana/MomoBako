//! 网易云接口失败的原因写成用户能照着处理的话：超时、登录失效、限流、接口拒绝、歌单不存在各不相同。

use ncm_api_rs::NcmError;

use super::{describe, playlist_detail_error, CallError};

#[test]
fn each_kind_of_failure_says_what_to_do() {
    assert_eq!(describe(&CallError::Timeout), "网易云接口请求超时（>30 秒），请检查网络后重试");
    assert_eq!(
        describe(&CallError::Ncm(NcmError::AuthRequired("需要登录".into()))),
        "网易云登录已失效，请重新登录（需要登录）"
    );
    assert_eq!(
        describe(&CallError::Ncm(NcmError::RateLimited("操作频繁".into()))),
        "网易云接口请求太频繁，请稍后再试（操作频繁）"
    );
    assert_eq!(
        describe(&CallError::Ncm(NcmError::Api { code: 250, msg: "风险提示".into() })),
        "网易云接口拒绝了请求（code 250）：风险提示"
    );
    assert!(describe(&CallError::Ncm(NcmError::Timeout("deadline".into()))).starts_with("网易云接口请求超时"));
    let unreadable = describe(&CallError::Ncm(NcmError::Crypto("bad block".into())));
    assert!(unreadable.starts_with("网易云接口返回了无法识别的内容"), "{unreadable}");
}

/// 歌单详情：接口说没有这个歌单时写歌单不存在，别的原因照通用说法（登录失效不会被说成歌单不存在）。
#[test]
fn a_missing_playlist_is_named_as_such() {
    let missing = playlist_detail_error(9001, &CallError::Ncm(NcmError::Api { code: 404, msg: String::new() }));
    assert_eq!(missing, "网易云上没有歌单 9001，可能已被删除或设为私密");
    let expired = playlist_detail_error(9001, &CallError::Ncm(NcmError::AuthRequired("需要登录".into())));
    assert_eq!(expired, "网易云登录已失效，请重新登录（需要登录）");
    assert!(playlist_detail_error(9001, &CallError::Timeout).contains("超时"));
}

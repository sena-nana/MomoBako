//! 仓库操作槽位的爬升、收起和编号规则，对照 Vue `tasks.ts`。

use super::{
    OperationPatch, OperationRequests, OperationStart, RepositoryOperation, CLIMB_CEILING, CLIMB_INTERVAL_MS,
    FINISH_LINGER_MS,
};

fn value(slot: &RepositoryOperation) -> Option<f64> {
    slot.shown().map(|progress| progress.value)
}

/// 每 220 ms 爬一级：35 以下加 5、70 以下加 3、其余加 1，最多到 88。
#[test]
fn waiting_operations_climb_every_220ms_up_to_the_ceiling() {
    let mut slot = RepositoryOperation::default();
    slot.start(OperationStart::new("加载资源库", "读取已注册资源库", 12.0).indeterminate());
    assert_eq!(value(&slot), Some(12.0));
    assert!(!slot.tick(CLIMB_INTERVAL_MS - 1), "不到 220 ms 不爬");
    assert!(slot.tick(1));
    assert_eq!(value(&slot), Some(17.0));
    for _ in 0..4 {
        slot.tick(CLIMB_INTERVAL_MS);
    }
    assert_eq!(value(&slot), Some(37.0), "17 → 22 → 27 → 32 → 37，过了 35 以后改加 3");
    slot.tick(CLIMB_INTERVAL_MS);
    assert_eq!(value(&slot), Some(40.0));
    for _ in 0..200 {
        slot.tick(CLIMB_INTERVAL_MS);
    }
    assert_eq!(value(&slot), Some(CLIMB_CEILING), "最多爬到 88");
    assert!(!slot.pending(), "停在上限以后不再要帧");
    assert_eq!(slot.shown().map(|progress| progress.indeterminate), Some(true), "不确定进度一样爬");
}

/// 完成停在 100%，180 ms 以后收起；取消立刻收起。
#[test]
fn finished_operations_linger_at_100_then_hide() {
    let mut slot = RepositoryOperation::default();
    let id = slot.start(OperationStart::new("刷新文件树", "同步并读取目录结构", 12.0));
    assert!(slot.update(id, OperationPatch::step("已扫描 3 个文件", 58.0)));
    assert!(slot.finish(id));
    assert_eq!(value(&slot), Some(100.0));
    assert!(slot.pending());
    slot.tick(FINISH_LINGER_MS - 16);
    assert_eq!(value(&slot), Some(100.0), "完成以后不再爬，也还没收起");
    assert!(slot.tick(16));
    assert!(slot.shown().is_none(), "180 ms 以后收起");

    let id = slot.start(OperationStart::new("复制文件", "创建硬链接或复制文件", 32.0));
    assert!(slot.cancel(id));
    assert!(slot.shown().is_none(), "取消立刻收起");
}

/// 后开始的操作顶掉前一个：旧编号的更新、完成和取消都不生效。
#[test]
fn a_newer_operation_owns_the_slot() {
    let mut slot = RepositoryOperation::default();
    let refresh = slot.start(OperationStart::new("加载资源库", "读取已注册资源库", 12.0).indeterminate());
    let read = slot.start(OperationStart::new("读取目录", "正在读取根目录", 14.0).indeterminate());
    assert!(!slot.update(refresh, OperationPatch::step("加载资源库摘要", 38.0)));
    assert!(!slot.finish(refresh));
    assert!(!slot.cancel(refresh));
    assert_eq!(slot.shown().map(|progress| progress.label.as_str()), Some("读取目录"));
    assert!(slot.is_current(read));
    assert!(!slot.is_current(refresh));
}

/// 更新到上限以上的值不被爬升压回去。
#[test]
fn values_above_the_ceiling_are_not_pulled_back() {
    let mut slot = RepositoryOperation::default();
    let id = slot.start(OperationStart::new("读取目录", "正在读取根目录", 14.0).indeterminate());
    slot.update(id, OperationPatch::step("整理目录条目", 92.0));
    slot.tick(CLIMB_INTERVAL_MS * 3);
    assert_eq!(value(&slot), Some(92.0));
}

/// 子状态机排下的请求按顺序落到槽位：开始记下编号，完成和取消落到这次操作上。
#[test]
fn queued_requests_apply_in_order() {
    let mut slot = RepositoryOperation::default();
    let mut requests = OperationRequests::default();
    requests.start(OperationStart::new("移动文件", "移动到目标文件夹", 36.0));
    assert!(requests.active(), "排下还没交给槽位的开始也算进行中");
    slot.apply(&mut requests);
    assert_eq!(value(&slot), Some(36.0));
    requests.update(OperationPatch::step("刷新文件索引", 82.0));
    requests.finish();
    assert!(!requests.active());
    slot.apply(&mut requests);
    assert_eq!(slot.shown().map(|progress| (progress.detail.as_str(), progress.value)), Some(("刷新文件索引", 100.0)));

    // 别处开始的操作顶掉以后，这里的取消不碰它。
    requests.start(OperationStart::new("读取目录", "正在读取 assets", 14.0));
    slot.apply(&mut requests);
    slot.start(OperationStart::new("加载资源库", "读取资源库快照", 10.0).indeterminate());
    requests.cancel();
    slot.apply(&mut requests);
    assert_eq!(slot.shown().map(|progress| progress.label.as_str()), Some("加载资源库"));
}

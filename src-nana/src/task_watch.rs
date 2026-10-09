//! 运行中任务的宿主观察：Mutsuki 任务运行时只存快照、不发变化通知，宿主线程定时读一次，
//! 有变化才交给界面。
//!
//! Vue 的任务弹层直接读响应式的任务列表（`TaskPopover.vue`），新任务、阶段变化和结束都即时反映在
//! 弹层和侧栏「任务」的计数上。Nana 用同样的口径：只交出排队、运行和取消中的任务，结束的任务从
//! 列表里拿掉；没有活动任务、上次也没有时连快照都不读。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::backend::services::repository::TaskProgressSnapshot;
use crate::backend::viewmodels::MutsukiTaskViewModel;

/// 两次读快照之间隔多久。
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// 还在进行的任务状态。完成、失败、取消完成的任务不进任务弹层。
const ACTIVE_STATUSES: [&str; 3] = ["queued", "running", "cancelling"];

/// 上一次交给界面的活动任务，用来判断这一次有没有变化。
#[derive(Default)]
pub(crate) struct TaskWatch {
    last: Vec<TaskProgressSnapshot>,
}

impl TaskWatch {
    /// 读一次：`active_ids` 是运行时登记中的任务，`snapshots` 按需读全部快照。
    /// 活动任务和上一次不同才返回新的列表；都为空时不读快照。
    pub(crate) fn poll(
        &mut self,
        active_ids: &[String],
        snapshots: impl FnOnce() -> Vec<TaskProgressSnapshot>,
    ) -> Option<Vec<TaskProgressSnapshot>> {
        if active_ids.is_empty() && self.last.is_empty() {
            return None;
        }
        let active = active_snapshots(active_ids, snapshots());
        if same_tasks(&active, &self.last) {
            return None;
        }
        self.last = active.clone();
        Some(active)
    }
}

/// 只留登记中、状态还在进行的任务，按任务编号排好，两次读的顺序一致。
pub(crate) fn active_snapshots(active_ids: &[String], snapshots: Vec<TaskProgressSnapshot>) -> Vec<TaskProgressSnapshot> {
    let mut active = snapshots
        .into_iter()
        .filter(|snapshot| active_ids.iter().any(|id| id == &snapshot.task_id))
        .filter(|snapshot| ACTIVE_STATUSES.contains(&snapshot.status.as_str()))
        .collect::<Vec<_>>();
    active.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    active
}

/// 两份快照列表一样：编号、状态、阶段、文案、进度、错误和更新时间都相同。
fn same_tasks(left: &[TaskProgressSnapshot], right: &[TaskProgressSnapshot]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.task_id == right.task_id
                && left.status == right.status
                && left.phase == right.phase
                && left.label == right.label
                && left.current == right.current
                && left.total == right.total
                && left.percent == right.percent
                && left.error == right.error
                && left.updated_at == right.updated_at
        })
}

/// 起一个观察线程，任务有变化就调 `dispatch`。`stop` 为真时线程退出。
pub(crate) fn spawn<F>(tasks: MutsukiTaskViewModel, stop: Arc<AtomicBool>, dispatch: F)
where
    F: Fn(Vec<TaskProgressSnapshot>) + Send + 'static,
{
    let started = std::thread::Builder::new().name("nana-task-watch".into()).spawn(move || {
        let mut watch = TaskWatch::default();
        while !stop.load(Ordering::Acquire) {
            if let Some(active) = watch.poll(&tasks.active_task_ids(), || tasks.progress_snapshots()) {
                dispatch(active);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });
    if let Err(error) = started {
        eprintln!("Nana 任务观察线程启动失败，任务弹层不会更新：{error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(task_id: &str, status: &str, percent: Option<f32>) -> TaskProgressSnapshot {
        TaskProgressSnapshot {
            task_id: task_id.into(),
            protocol_id: "momobako.repository.sync".into(),
            status: status.into(),
            phase: None,
            label: None,
            current: None,
            total: None,
            percent,
            error: None,
            updated_at: "1".into(),
        }
    }

    /// 没有活动任务、上次也没有时不读快照；任务出现、变了、结束都各交一次，没变不交。
    #[test]
    fn poll_reports_only_changes_of_active_tasks() {
        let mut watch = TaskWatch::default();
        assert!(watch.poll(&[], || panic!("空闲时不该读快照")).is_none());

        let ids = vec!["task-1".to_string()];
        let first = watch.poll(&ids, || vec![snapshot("task-1", "queued", None), snapshot("old", "completed", Some(100.0))]);
        assert_eq!(first.as_ref().map(|tasks| tasks.iter().map(|task| task.task_id.as_str()).collect::<Vec<_>>()), Some(vec!["task-1"]));
        assert!(watch.poll(&ids, || vec![snapshot("task-1", "queued", None)]).is_none(), "没变不交");
        let running = watch.poll(&ids, || vec![snapshot("task-1", "running", Some(40.0))]).expect("状态变了要交");
        assert_eq!(running[0].status, "running");

        let finished = watch.poll(&ids, || vec![snapshot("task-1", "completed", Some(100.0))]).expect("结束了要交一次空列表");
        assert!(finished.is_empty(), "结束的任务不进弹层");
        assert!(watch.poll(&[], || panic!("都空了不再读")).is_none());
    }

    /// 快照里不在运行时登记表里的（已经移走的）任务不交，交出的按编号排好。
    #[test]
    fn active_snapshots_keep_registered_running_tasks_in_order() {
        let ids = vec!["b".to_string(), "a".to_string()];
        let active = active_snapshots(&ids, vec![snapshot("b", "running", None), snapshot("gone", "running", None), snapshot("a", "cancelling", None)]);
        assert_eq!(active.iter().map(|task| task.task_id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }
}

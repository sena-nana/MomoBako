//! 领域服务派发层：把壳层的意图交给 `momobako-backend` 的应用服务，结果作为新消息回到事件循环。
//!
//! 壳层（`shell`）只归约状态、排出副作用，不碰服务；这里是唯一调用服务的地方。每次 `update` 分两步：
//! - 归约之前，[`before_reduce`] 处理少数按消息本身就要提交任务的情况（`app.rs`）；
//! - 归约之后，[`after_reduce`] 依次取走各表面排下的副作用（`take_effects`），交给对应的子模块。
//!
//! 每个子模块对应壳层的一个表面，入口都叫 `effects`。任务一律经 [`task_guard::spawn_service`] 提交到 NanaUI 的
//! 任务线程：服务没起来、任务排不进去或任务 panic 时，按失败消息送回，界面不会停在进行中。服务调用是同步的，
//! 只在任务线程里做，不占 UI 线程。

use nana_ui::RuntimeProgramContext;

use crate::MomoBakoApplication;
use crate::shell::ShellMessage;

pub(crate) mod admin;
pub(crate) mod app;
pub(crate) mod download;
pub(crate) mod files;
pub(crate) mod inspect;
pub(crate) mod library;
pub(crate) mod playback_source;
pub(crate) mod player;
pub(crate) mod sidebar;
pub(crate) mod sync;
pub(crate) mod task_guard;
pub(crate) mod workspace;

/// 归约之前：按消息本身提交服务任务（打开目录、读元数据、移除播放集条目）。
pub(crate) fn before_reduce(
    app: &MomoBakoApplication,
    message: &ShellMessage,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    app::on_message(app, message, context);
}

/// 归约之后：按固定顺序取走各表面的副作用交给领域服务。工作台在前（换仓库会先停播放），文件和检查器在后。
pub(crate) fn after_reduce(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    workspace::effects(app, context);
    player::effects(app, context);
    admin::effects(app, context);
    sidebar::effects(app, context);
    sync::effects(app, context);
    files::effects(app, context);
    inspect::effects(app, context);
    library::effects(app, context);
}

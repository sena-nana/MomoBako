//! Request ownership is local to a host, and callbacks carry a unique attempt
//! token so a late result cannot complete a new request or a reopened window.
use super::*;
use nana_ui_core::{FileDialogError, FileDialogRequest, FileDialogResult};

type Completion = (WindowId, u64, FileDialogResult);

#[derive(Default)]
pub(super) struct FileDialogs {
    next: u64,
    native: HashMap<WindowId, nana_window::FileDialogHandle>,
    active: HashMap<WindowId, (u64, u64)>,
    shutting_down: bool,
    completed: Arc<Mutex<Vec<Completion>>>,
}

impl FileDialogs {
    fn begin(&mut self, id: WindowId, request: u64) -> Result<u64, FileDialogError> {
        if self.shutting_down {
            return Err(FileDialogError::WindowClosed);
        }
        if let Some((_, active)) = self.active.get(&id) {
            return Err(if *active == request {
                FileDialogError::DuplicateRequest
            } else {
                FileDialogError::Busy
            });
        }
        self.next = self
            .next
            .checked_add(1)
            .expect("file dialog attempt counter exhausted");
        self.active.insert(id, (self.next, request));
        Ok(self.next)
    }

    fn finish(
        &mut self,
        id: WindowId,
        token: u64,
        result: FileDialogResult,
    ) -> Option<FileDialogResult> {
        if self.active.get(&id) != Some(&(token, result.id)) {
            return None;
        }
        self.active.remove(&id);
        self.native.remove(&id);
        Some(result)
    }

    fn close(&mut self, id: WindowId) -> Option<FileDialogResult> {
        self.native.remove(&id);
        self.active
            .remove(&id)
            .map(|(_, request)| FileDialogResult::failed(request, FileDialogError::WindowClosed))
    }
}

impl<Program: RuntimeProgram> WindowManager<Program> {
    pub(super) fn open_file_dialog(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        request: FileDialogRequest,
    ) {
        let Some(window) = self
            .window(id)
            .filter(|_| !self.closing_windows.contains(&id))
            .cloned()
        else {
            self.reject_file_dialog(event_loop, id, request.id, FileDialogError::WindowClosed);
            return;
        };
        let token = match self.file_dialogs.begin(id, request.id) {
            Ok(token) => token,
            Err(error) => {
                self.reject_file_dialog(event_loop, id, request.id, error);
                return;
            }
        };
        let queue = Arc::clone(&self.file_dialogs.completed);
        let proxy = self.proxy.clone();
        let request_id = request.id;
        match nana_window::open_file_dialog(window, request, move |result| {
            queue
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push((id, token, result));
            proxy.wake_up();
        }) {
            Ok(handle) => {
                self.file_dialogs.native.insert(id, handle);
            }
            Err(error) => {
                if let Some(result) =
                    self.file_dialogs
                        .finish(id, token, FileDialogResult::failed(request_id, error))
                {
                    self.deliver_file_dialog(event_loop, id, result);
                }
            }
        }
    }

    fn reject_file_dialog(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        request_id: u64,
        error: FileDialogError,
    ) {
        let update = self.program.window_event(
            WindowEvent::FileDialogRejected {
                id,
                request_id,
                error,
            },
            &self.context_for(id),
        );
        self.apply_update(event_loop, update, None);
    }

    fn deliver_file_dialog(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        id: WindowId,
        result: FileDialogResult,
    ) {
        let update = self.program.window_event(
            WindowEvent::FileDialogCompleted { id, result },
            &self.context_for(id),
        );
        self.apply_update(event_loop, update, None);
    }

    pub(super) fn complete_file_dialogs(&mut self, event_loop: &dyn ActiveEventLoop) {
        let completions = std::mem::take(
            &mut *self
                .file_dialogs
                .completed
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
        );
        for (id, token, result) in completions {
            if let Some(result) = self.file_dialogs.finish(id, token, result) {
                self.deliver_file_dialog(event_loop, id, result);
            }
        }
    }

    pub(super) fn close_file_dialog(&mut self, event_loop: &dyn ActiveEventLoop, id: WindowId) {
        if let Some(result) = self.file_dialogs.close(id) {
            self.deliver_file_dialog(event_loop, id, result);
        }
    }

    pub(super) fn close_all_file_dialogs(&mut self) {
        self.file_dialogs.shutting_down = true;
        let ids: Vec<_> = self.file_dialogs.active.keys().copied().collect();
        for id in ids {
            if let Some(window) = self.window(id) {
                window.set_visible(false);
            }
            if let Some(result) = self.file_dialogs.close(id) {
                // The host is already exiting: report terminal outcomes, but
                // do not execute commands that try to reopen windows/dialogs.
                let _ = self.program.window_event(
                    WindowEvent::FileDialogCompleted { id, result },
                    &self.context_for(id),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_active_request_per_window_and_completion_is_once() {
        let mut dialogs = FileDialogs::default();
        let a = WindowId(1);
        let b = WindowId(2);
        let token = dialogs.begin(a, 7).unwrap();
        assert_eq!(dialogs.begin(a, 8), Err(FileDialogError::Busy));
        assert_eq!(dialogs.begin(a, 7), Err(FileDialogError::DuplicateRequest));
        assert_eq!(dialogs.active.get(&a), Some(&(token, 7)));
        let other = dialogs.begin(b, 7).unwrap();
        assert!(
            dialogs
                .finish(a, token, FileDialogResult::cancelled(8))
                .is_none()
        );
        assert!(
            dialogs
                .finish(a, token, FileDialogResult::cancelled(7))
                .unwrap()
                .is_cancelled()
        );
        assert!(
            dialogs
                .finish(a, token, FileDialogResult::cancelled(7))
                .is_none()
        );
        assert!(
            dialogs
                .finish(
                    b,
                    other,
                    FileDialogResult::selected(7, vec!["chosen".into()])
                )
                .is_some()
        );
    }

    #[test]
    fn closed_and_reopened_window_rejects_late_callback_even_when_request_id_is_reused() {
        let mut dialogs = FileDialogs::default();
        let id = WindowId(1);
        let old = dialogs.begin(id, 42).unwrap();
        assert_eq!(
            dialogs.close(id).unwrap().error,
            Some(FileDialogError::WindowClosed)
        );
        assert!(dialogs.close(id).is_none());
        let new = dialogs.begin(id, 42).unwrap();
        assert!(
            dialogs
                .finish(id, old, FileDialogResult::cancelled(42))
                .is_none()
        );
        let result = dialogs
            .finish(
                id,
                new,
                FileDialogResult::failed(42, FileDialogError::Platform("backend failed".into())),
            )
            .unwrap();
        assert!(!result.is_cancelled());
        assert!(dialogs.active.is_empty());
    }

    #[test]
    fn callback_queue_belongs_to_one_host() {
        let mut first = FileDialogs::default();
        let second = FileDialogs::default();
        let token = first.begin(WindowId(1), 1).unwrap();
        first
            .completed
            .lock()
            .unwrap()
            .push((WindowId(1), token, FileDialogResult::cancelled(1)));
        assert!(second.completed.lock().unwrap().is_empty());
    }
}

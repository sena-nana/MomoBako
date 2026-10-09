//! 文件页对话框投影的单测：取值、同一状态两次投影相等，以及哪些消息改投影、哪些不改。

use super::export::ExportView;
use super::{HardlinkView, TextDialogView};
use crate::shell::files::{FileDialog, FilesMessage, HardlinkPrompt};
use crate::shell::{ShellMessage, ShellViewModel};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 不改文件页对话框的消息：播放音量和标题栏搜索。
fn unrelated(model: &mut ShellViewModel) {
    model.reduce(ShellMessage::Player(crate::shell::player::PlayerMessage::SetVolume(0.2)));
    model.reduce(ShellMessage::Inspect(crate::shell::InspectMessage::SetQuery("封面".into())));
}

#[test]
fn text_dialog_projection_follows_the_dialog_kind_and_the_file_service() {
    let mut model = scene("copy-dialog");
    let view = TextDialogView::project(&model).expect("复制对话框开着");
    assert_eq!((view.title.as_str(), view.submit.as_str(), view.busy), ("复制到文件夹", "复制", false));
    assert_eq!(TextDialogView::project(&model), Some(view.clone()), "同一状态的投影相等");
    unrelated(&mut model);
    assert_eq!(TextDialogView::project(&model), Some(view), "无关消息不该改投影");

    model.files.mutating = true;
    let busy = TextDialogView::project(&model).expect("复制对话框开着");
    assert_eq!((busy.submit.as_str(), busy.busy), ("处理中...", true));

    model.files.mutating = false;
    model.files.eagle_mode = "move".into();
    model.files.dialog = FileDialog::ImportEagle;
    assert_eq!(TextDialogView::project(&model).map(|view| view.title), Some("从 Eagle 剪切导入".to_string()));
    model.files.dialog = FileDialog::Hardlink;
    assert_eq!(TextDialogView::project(&model), None, "硬链接确认不是文本对话框");
}

#[test]
fn hardlink_projection_moves_to_the_next_prompt() {
    let mut model = scene("hardlink-dialog");
    let view = HardlinkView::project(&model).expect("硬链接确认开着");
    assert_eq!((view.existing_path.as_str(), view.new_path.as_str()), ("cover.png", "inbox/cover.png"));
    assert_eq!(view.confirm, "加入关联");
    model.files.present_hardlink(HardlinkPrompt {
        id: "link-2".into(),
        new_path: "inbox/page.pdf".into(),
        existing_path: "notes/page.pdf".into(),
        size_label: "1.8 KB".into(),
    });
    let next = HardlinkView::project(&model).expect("硬链接确认开着");
    assert_eq!(next.new_path, "inbox/page.pdf", "队列换了一条，投影跟着换");
    model.reduce(ShellMessage::Files(FilesMessage::SkipHardlink));
    assert_eq!(HardlinkView::project(&model), None, "跳过最后一条后对话框关掉");
}

#[test]
fn export_projection_follows_the_target_and_busy_state() {
    let mut model = scene("export-dialog");
    let view = ExportView::project(&model).expect("导出对话框开着");
    assert!(view.archive && !view.busy && !view.encrypt);
    assert_eq!((view.format.as_str(), view.compression.as_str()), ("zip", "balanced"));
    assert_eq!(view.repo_name, "默认资源库");
    assert_eq!(ExportView::project(&model), Some(view.clone()));
    unrelated(&mut model);
    assert_eq!(ExportView::project(&model), Some(view), "无关消息不该改投影");

    model.reduce(ShellMessage::Files(FilesMessage::SetExportField { field: "target".into(), value: "git".into() }));
    assert!(!ExportView::project(&model).expect("导出对话框开着").archive, "切到 Git");
    model.reduce(ShellMessage::Files(FilesMessage::CloseExport));
    assert_eq!(ExportView::project(&model), None);
}

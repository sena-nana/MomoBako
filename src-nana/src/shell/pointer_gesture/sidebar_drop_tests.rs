// 侧栏文件夹是放下目标：内部拖动松手移入，外部文件放下导入，停满 450ms 打开。

use std::path::PathBuf;

use nana_ui::{FileDragInput, FileDragKind, InputModifiers, InputPayload};

use crate::shell::FilesEffect;

/// 拖到侧栏「照片」上：450ms 后打开这个文件夹，松手把 cover.png 移进去。
/// 外部文件在同一行放下则导入到 photos，不进当前目录。
#[test]
fn sidebar_folder_drop_moves_imports_and_opens() {
    let mut model = hover_model();
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let cover = row_center(&window, "cover.png");
    let folder = labeled_center(&window, "照片");
    pointer(&mut window, &mut input, nana_ui::PointerPhase::Down, cover.0, cover.1);
    prepare_motion(&mut model, &mut window);
    pointer(&mut window, &mut input, nana_ui::PointerPhase::Move, folder.0, folder.1);
    prepare_motion(&mut model, &mut window);
    assert_ne!(model.current_directory, "photos", "刚拖到侧栏文件夹还不到 450ms");
    assert!(model.input.internal_active, "越过 7px 以后内部拖动应该起步");
    for _ in 0..40 {
        prepare_motion(&mut model, &mut window);
    }
    assert_eq!(model.current_directory, "photos", "在侧栏文件夹上停满 450ms 应该打开它");
    assert_eq!(model.input.hover_folder.as_deref(), Some("photos"), "拖动中的放下目标是侧栏这个文件夹");
    pointer(&mut window, &mut input, nana_ui::PointerPhase::Up, folder.0, folder.1);
    prepare_motion(&mut model, &mut window);
    let effects = model.files.take_effects();
    assert!(
        effects.iter().any(|effect| matches!(effect, FilesEffect::Move { sources, parent, .. } if sources == &["cover.png".to_string()] && parent == "photos")),
        "松手应该把 cover.png 移进侧栏文件夹：{effects:?}"
    );

    let mut model = hover_model();
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let folder = labeled_center(&window, "照片");
    drag_file(&mut window, &mut input, FileDragKind::Hover, "D:\\other\\c.png", folder);
    crate::shell::note_external_folder_drag(&mut model, &window.document, &file_drag(FileDragKind::Hover, "D:\\other\\c.png", folder));
    reduce_queued(&mut model, &mut window);
    assert_eq!(model.input.hover_folder.as_deref(), Some("photos"));
    assert!(model.input.hover_on_sidebar, "外部拖放应记在侧栏行上");
    for _ in 0..40 {
        prepare_motion(&mut model, &mut window);
    }
    assert_eq!(model.current_directory, "photos", "外部拖放停在侧栏文件夹上满 450ms 也应该打开");
    drag_file(&mut window, &mut input, FileDragKind::Drop, "D:\\other\\c.png", folder);
    crate::shell::note_external_folder_drag(&mut model, &window.document, &file_drag(FileDragKind::Drop, "D:\\other\\c.png", folder));
    reduce_queued(&mut model, &mut window);
    let effects = model.files.take_effects();
    assert!(
        effects.iter().any(|effect| matches!(effect, FilesEffect::Import { parent, sources, .. } if parent.as_deref() == Some("photos") && sources == &["D:\\other\\c.png".to_string()])),
        "外部文件放到侧栏文件夹上应该导入到那里：{effects:?}"
    );

    fn drag_file(window: &mut nana_ui::ApplicationWindow, input: &mut nana_ui::HeadlessInput, kind: FileDragKind, path: &str, point: (f32, f32)) {
        input.route(window.document.context_mut(), file_drag(kind, path, point)).expect("文件拖放");
    }

    fn file_drag(kind: FileDragKind, path: &str, point: (f32, f32)) -> InputPayload {
        InputPayload::FileDrag(FileDragInput {
            kind,
            paths: vec![PathBuf::from(path)],
            position: Some(point),
            modifiers: InputModifiers::default(),
        })
    }
}

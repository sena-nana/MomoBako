//! 滚动区键、预览页播放条、冲突提示、媒体预览失败、加载更多与资源库快捷方式的挂载测试。

use super::*;

#[test]
fn scroll_areas_are_keyed_by_what_they_show() {
    let mut model = scene("live-files-selected");
    let (list, detail) = scroll_keys(&model);
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "草稿".into(),
    )));
    assert_eq!(
        scroll_keys(&model),
        (list.clone(), detail.clone()),
        "同一目录、同一选中项重挂保持位置"
    );

    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(
        "cover.png".into(),
    )));
    let (same_list, other_detail) = scroll_keys(&model);
    assert_eq!(same_list, list, "换选中项时列表不动");
    assert_ne!(other_detail, detail, "换选中项时详情回顶");

    model.files.current_path = "assets".into();
    assert_ne!(scroll_keys(&model).0, list, "进子目录时列表回顶");
    model.files.current_path = String::new();
    model.workspace.panel = WorkspacePanel::Trash;
    assert_ne!(scroll_keys(&model).0, list, "进回收站时列表回顶");
}

/// 预览页只有框架贴在页底的那一条播放条，预览面板一直铺到主区内容的底边，下面不再空出一格间距。
#[test]
fn preview_page_keeps_its_own_player_bar_without_a_trailing_slot() {
    let window = laid_out(&scene("preview-audio"));
    let document = window.document.document();
    let context = window.document.context();
    let world = context.world();
    let bars = world
        .project_accessibility_in_viewport(document)
        .into_iter()
        .filter(|node| node.label.as_deref() == Some("进度"))
        .collect::<Vec<_>>();
    assert_eq!(bars.len(), 1, "预览页只有一条播放条");
    let mut ancestors = Vec::new();
    let mut cursor = world.parent_id(bars[0].id);
    while let Some(id) = cursor {
        ancestors.push((
            context.assembly_path(id).unwrap_or_default(),
            world.layout_box(id),
        ));
        cursor = world.parent_id(id);
    }
    let boxed = |key: &str| {
        ancestors
            .iter()
            .find(|(path, _)| path.rsplit('/').next() == Some(key))
            .and_then(|(_, bounds)| *bounds)
            .unwrap_or_else(|| panic!("播放条不在 {key} 里"))
    };
    let page = boxed("inspect-preview-page");
    let body = boxed("workspace-page-body");
    assert!(
        (page.y + page.height - (body.y + body.height)).abs() < 0.5,
        "预览面板底边 {} 应贴着主区内容底边 {}",
        page.y + page.height,
        body.y + body.height
    );
}

/// 15 页的「冲突」：冲突说明排在注释前面、落在 800 高的窗口里，本地草稿还在注释框；
/// 点「采用服务器版本」换成服务器上的注释，冲突说明收起。详情卡片顶上的「注释」导语写的是服务器上现在的注释
/// （冲突应答带回的素材已经换进文件行），这里比的是元数据编辑里的注释框。
#[test]
fn conflict_notice_sits_above_the_fields_and_adopts_the_server_version() {
    let mut model = ShellViewModel::for_page(ShellPage::Conflict);
    let mut window = laid_out(&model);
    let (_, notice_y) = labeled_center(&window, "版本冲突，未写入");
    let comment_y = keyed_bounds(&window, "inspect-comment-field").y;
    assert!(
        notice_y < comment_y && notice_y < 800.0,
        "冲突说明应在注释前面、在窗口里：{notice_y} / {comment_y}"
    );
    assert!(
        has_value(&window, "封面改用暖色版本。"),
        "冲突时本地草稿留在注释框"
    );
    let (x, y) = labeled_center(&window, "采用服务器版本");
    for message in click(&mut window, x, y) {
        model.reduce(message);
    }
    let window = laid_out(&model);
    assert!(
        !has_label(&window, "版本冲突，未写入"),
        "采用后冲突说明收起"
    );
    assert!(
        has_value(&window, "封面定稿，沿用冷色版本。"),
        "注释框换成服务器上的注释"
    );
}

/// 视频按文件类型画，不按有没有画面分出唱片：离屏没有宿主纹理时画解码出的那一帧，不画图标、不画唱片舞台；
/// 清掉像素后才画视频图标。有宿主纹理时仍画纹理槽。准备卡视频写「准备媒体」，
/// 音频写「准备音频」。
#[test]
fn video_previews_never_fall_into_the_audio_stage() {
    let mut model = scene("preview-no-player");
    let window = laid_out(&model);
    assert!(
        !has_key(&window, "inspect-audio-stage"),
        "视频不该画唱片舞台"
    );
    assert!(!has_label(&window, "通用音频") && !has_label(&window, "暂无歌词"));
    assert!(
        has_key(&window, "inspect-video-pixels"),
        "离屏没有宿主纹理时画解码出的这一帧"
    );
    assert!(
        !has_key(&window, "inspect-fallback-icon"),
        "有解码画面时不画视频图标"
    );
    assert!(
        !has_key(&window, "inspect-video"),
        "没有宿主纹理时不挂 file-preview"
    );
    assert!(
        !crate::shell::wants_preview_texture(&model),
        "离屏不上传宿主纹理"
    );
    let frame = model.preview_pixels.as_ref().expect("解码画面");
    assert_eq!((frame.width, frame.height), (32, 18));
    assert!(
        frame
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0x2A, 0x2D, 0x33, 0xFF]),
        "播放头在后半段时应是夹具第二帧"
    );
    assert!(model.preview_token.is_none(), "离屏不挂纹理记号");
    assert!(
        has_label(&window, "没有可用于播放此媒体的插件"),
        "播放条照 Vue 写没有播放插件"
    );
    // 拨回片头仍是解码出的第一帧，再拨到后半段也不把纹理记号挂回去。
    place_playhead(&mut model, 0);
    let rewound = model.preview_pixels.as_ref().expect("片头画面");
    assert!(
        rewound
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0x20, 0x22, 0x26, 0xFF]),
        "片头应是夹具第一帧"
    );
    assert!(model.preview_token.is_none(), "拨回片头也不挂纹理记号");
    let later = model
        .inspect
        .media_session()
        .and_then(|session| session.duration_ms)
        .unwrap_or(0)
        .saturating_sub(1);
    place_playhead(&mut model, later);
    let window = laid_out(&model);
    assert!(
        has_key(&window, "inspect-video-pixels"),
        "后半段仍画解码像素"
    );
    assert!(
        !has_key(&window, "inspect-video"),
        "播放头往前走也不挂 file-preview"
    );
    assert!(
        !has_key(&window, "inspect-fallback-icon"),
        "有解码画面时不画视频图标"
    );
    let frame = model.preview_pixels.as_ref().expect("后半段画面");
    assert!(
        frame
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [0x2A, 0x2D, 0x33, 0xFF]),
        "后半段仍是夹具第二帧"
    );

    model.preview_pixels = None;
    let bare = laid_out(&model);
    assert!(
        has_key(&bare, "inspect-fallback-icon"),
        "没有画面时画视频图标"
    );
    assert!(
        !has_key(&bare, "inspect-audio-stage"),
        "没有画面时也不画唱片舞台"
    );

    model.preview_token = Some("video:clip.avi:0:32".into());
    model.preview_pixels = Some(crate::shell::PreviewPixels {
        width: 2,
        height: 2,
        rgba: vec![255; 16],
    });
    assert!(
        has_key(&laid_out(&model), "inspect-video"),
        "有宿主纹理时画纹理槽"
    );
    assert!(
        crate::shell::wants_preview_texture(&model),
        "有宿主纹理时仍要上传"
    );

    assert!(has_label(
        &laid_out(&loading_media_preview("movies/clip.mp4")),
        "准备媒体"
    ));
    assert!(has_label(
        &laid_out(&loading_media_preview("music/track-01.mp3")),
        "准备音频"
    ));
}

/// 音视频预览失败就近写在预览框里：失败浮层替换唱片舞台，红色标题下一行原因。
/// Nana 自己的「没有原生解码器」和某格式「解码失败」都这样；视频的标题是「无法预览该媒体」。
/// 这类失败不进侧栏的全局状态区。
#[test]
fn media_preview_failures_show_their_reason_in_place() {
    for (path, error, title) in [
        ("music/track-01.mp3", "没有原生解码器", "无法预览该音频"),
        (
            "music/track-02.flac",
            "无法识别压缩音频：end of stream",
            "无法预览该音频",
        ),
        (
            "movies/clip.mp4",
            "MP4 解码失败：没有解出画面",
            "无法预览该媒体",
        ),
        ("movies/clip.webm", "没有原生解码器", "无法预览该媒体"),
    ] {
        let model = failed_media_preview(path, error);
        let window = laid_out(&model);
        assert!(has_label(&window, title), "{path} 失败时没有标题 {title}");
        assert!(
            has_label(&window, error),
            "{path} 失败时没有写出原因 {error}"
        );
        assert!(
            has_key(&window, "inspect-failed"),
            "{path} 失败时没有失败浮层"
        );
        assert!(
            !has_key(&window, "inspect-audio-stage"),
            "{path} 失败时不该还画唱片舞台"
        );
        assert_eq!(
            StatusLine::project(&model),
            StatusLine::Hidden,
            "{path} 的预览失败不进全局状态区"
        );
    }
}

/// 播放条装不上预览中的同一个文件、把失败会话写回预览页：唱片舞台换成失败浮层；会话没带原因时写默认的
/// 「音频无法播放」。
#[test]
fn player_failures_written_back_replace_the_audio_stage() {
    let mut model = scene("preview-audio");
    assert!(
        has_key(&laid_out(&model), "inspect-audio-stage"),
        "能播的音频预览是唱片舞台"
    );
    let PreviewBody::Media(session) = &model.inspect.body else {
        panic!("预览不是音视频")
    };
    let mut failed = session.clone();
    failed.status = "failed".into();
    failed.error = None;
    model.inspect.replace_shared_media(failed);
    let window = laid_out(&model);
    assert!(
        has_label(&window, "无法预览该音频"),
        "写回失败后没有失败标题"
    );
    assert!(
        has_label(&window, "音频无法播放"),
        "没有原因时没有写 Vue 的默认原因"
    );
    assert!(
        !has_key(&window, "inspect-audio-stage"),
        "写回失败后不该还画唱片舞台"
    );
}

/// 还有下一页时列表末尾写「滚动继续加载」；正在续读时改成「继续读取目录...」。
/// 没有更多、分类视图和智能文件夹都不再分页，这两句都不出现。
#[test]
fn load_more_caption_follows_paging_and_stays_off_virtual_views() {
    let absent = |model: &ShellViewModel, why: &str| {
        let window = laid_out(model);
        assert!(
            !has_label(&window, "滚动继续加载"),
            "{why} 不该出现「滚动继续加载」"
        );
        assert!(
            !has_label(&window, "继续读取目录..."),
            "{why} 不该出现「继续读取目录...」"
        );
    };

    let mut model = scene("live-files-plain");
    assert!(!model.files.has_more, "夹具目录一页就读完");
    absent(&model, "没有更多");

    model.files.has_more = true;
    let paging = laid_out(&model);
    assert!(
        has_label(&paging, "滚动继续加载"),
        "还有下一页时应写出继续加载"
    );
    assert!(
        !has_label(&paging, "继续读取目录..."),
        "还没续读时不写续读中"
    );

    model.files.loading_more = true;
    let loading = laid_out(&model);
    assert!(has_label(&loading, "继续读取目录..."), "续读中应改写文案");
    assert!(
        !has_label(&loading, "滚动继续加载"),
        "续读中不再留原来的文案"
    );

    let category = scene("uncategorized");
    assert!(!category.files.has_more, "分类视图不再分页");
    absent(&category, "分类视图");

    let smart = scene("smart-folder");
    assert!(!smart.files.has_more, "智能文件夹不再分页");
    absent(&smart, "智能文件夹");
}

/// 侧栏「未分类 / 全部」：不重读目录，文件列换成分类或回到当前目录，选择不改。
///
/// 分类视图不跟目录的「滚动继续加载」。不在这一类里的选择，详情回到「选择一个文件或文件夹」，路径仍留着，
/// 回到「全部」再亮起来。从失败的回收站点「全部」是非静默读取：清掉文件列错误，换成「正在读取目录」，选择仍不在这一下清掉。
#[test]
fn opening_a_library_shortcut_keeps_the_list_and_the_selection() {
    use crate::shell::view_harness::ShellHarness;
    use crate::shell::{ShortcutId, SidebarMessage};

    let shortcut = |id| ShellMessage::Sidebar(SidebarMessage::SelectShortcut(id));
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Files(FilesMessage::ActivateRow(
        "cover.png".into(),
    )));
    harness.model.files.has_more = true;
    harness.sync();
    harness.flush();
    assert!(harness.find("滚动继续加载").is_some(), "目录还有下一页");
    assert_eq!(
        harness.model.files.selected_paths(),
        &["cover.png".to_string()]
    );

    harness.apply(shortcut(ShortcutId::Uncategorized));
    harness.flush();
    assert!(harness.find("正在读取目录").is_none(), "点未分类不重读目录");
    assert!(
        harness.find("滚动继续加载").is_none(),
        "分类视图不跟目录分页"
    );
    assert!(
        harness.find("cover.png").is_some(),
        "未分类里有根目录的 cover.png"
    );
    assert!(
        harness.find("选择一个文件或文件夹").is_none(),
        "还在这一类里的选择留着"
    );
    assert_eq!(
        harness.model.files.selected_paths(),
        &["cover.png".to_string()]
    );
    assert!(!harness.model.files.loading);

    harness.apply(shortcut(ShortcutId::All));
    harness.flush();
    assert!(
        harness.find("正在读取目录").is_none(),
        "点全部也不重读当前目录"
    );
    assert!(
        harness.find("滚动继续加载").is_some(),
        "回到全部仍按目录分页"
    );
    assert_eq!(
        harness.model.files.selected_paths(),
        &["cover.png".to_string()]
    );

    harness.apply(ShellMessage::Files(FilesMessage::ActivateRow(
        "notes/page.pdf".into(),
    )));
    harness.flush();
    assert!(harness.find("page.pdf").is_some());
    assert_eq!(
        harness.model.files.selected_paths(),
        &["notes/page.pdf".to_string()]
    );

    harness.apply(shortcut(ShortcutId::Uncategorized));
    harness.flush();
    assert!(
        harness.find("page.pdf").is_none(),
        "子目录里的文件不进未分类"
    );
    assert!(
        harness.find("选择一个文件或文件夹").is_some(),
        "不在这一类里，详情是空的"
    );
    assert_eq!(
        harness.model.files.selected_paths(),
        &["notes/page.pdf".to_string()],
        "选择留着，不在点快捷方式时清掉"
    );
    assert!(harness.find("正在读取目录").is_none());

    harness.apply(shortcut(ShortcutId::All));
    harness.flush();
    assert!(
        harness.find("page.pdf").is_some(),
        "回到全部，原来的选择还在列表里"
    );
    assert!(harness.find("选择一个文件或文件夹").is_none());
    assert_eq!(
        harness.model.files.selected_paths(),
        &["notes/page.pdf".to_string()]
    );

    harness.apply(shortcut(ShortcutId::Trash));
    harness.model.stage_browses();
    harness.model.files.loading = false;
    harness.apply(ShellMessage::Files(FilesMessage::NoteError(
        "回收站读失败".into(),
    )));
    harness.flush();
    assert!(
        harness.keyed("file-error").is_some(),
        "回收站失败写在文件列"
    );
    assert!(harness.find("正在读取目录").is_none());

    harness.apply(shortcut(ShortcutId::All));
    harness.model.stage_browses();
    harness.sync();
    harness.flush();
    assert!(
        harness.find("正在读取目录").is_some(),
        "从回收站回到全部要显示正在读取目录"
    );
    assert!(
        harness.keyed("file-error").is_none(),
        "非静默读取清掉文件列上的失败"
    );
    assert!(harness.find("回收站读失败").is_none());
    assert_eq!(
        harness.model.files.selected_paths(),
        &["notes/page.pdf".to_string()],
        "读取过程中选择先留着"
    );
    assert!(harness.model.files.loading);
}

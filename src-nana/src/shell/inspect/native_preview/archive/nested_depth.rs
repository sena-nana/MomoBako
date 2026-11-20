//! 压缩包嵌套层数。
//!
//! 对齐 Vue `preview-archive` 的 `nestedDepthExceeded` 和
//! `service-archive-preview` 的默认 `maxNestedDepth`（提交 `7bc847c`，默认 5）。
//! 外层压缩包不算一层。点进第 5 个 zip、cbz 或 7z 以后，这一层里的 zip、cbz 和 7z 不再展开：
//! 行尾写「嵌套深度已达上限」，点开停在名单上，舞台写「无法预览内部文件」。
//! rar 和 cbr 仍不进入下一层，大小照旧，也不加深 RAR。

use super::super::super::{ArchiveFileNotice, InspectState};

/// Vue 默认嵌套上限。扫到这一层里的压缩包文件就不再解开。
const MAX_NESTED_DEPTH: usize = 5;

/// 行尾替换大小的那句。和 Vue 名单上的说明同一句。
const DEPTH_NOTE: &str = "嵌套深度已达上限";

/// 这一层已经到上限时，把 zip、cbz 和 7z 行尾的大小换成「嵌套深度已达上限」。
///
/// 目录仍写「目录」。其它文件的大小不动。头部「压缩包 / … · N 项」即使路径里带着 `.zip` 也不改。
/// 还没到上限时整段原样返回。
pub(super) fn limit_layer_text(text: String, depth: usize) -> String {
    if depth < MAX_NESTED_DEPTH {
        return text;
    }
    text.lines()
        .map(rewrite_archive_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 点开的是已经到上限的 zip、cbz 或 7z 时停住，并写上 Vue 取包内预览时的那句原因。
///
/// 返回真表示这次点击已经处理完，调用方不要再去取字节。不到上限、或不是这三种时返回假。
pub(super) fn refuse_past_depth(state: &mut InspectState, path: &str) -> bool {
    if state.archive_nests.len() < MAX_NESTED_DEPTH || !nestable_file(path) {
        return false;
    }
    // 和 Vue `prepareEntryPreview` 在嵌套目录没解开时交回的原因同一句，舞台原样显示。
    // `selectEntry` 仍把这一行标成当前项，不因为展不开就把选中清掉。
    let message = format!("nested archive depth limit reached: {path}");
    eprintln!("Nana 压缩包嵌套已到第 {MAX_NESTED_DEPTH} 层，不再展开：{path}");
    state.archive_entry = None;
    state.archive_selected = Some(path.to_string());
    state.archive_request = None;
    state.archive_notice = Some(ArchiveFileNotice::Failed(message));
    true
}

/// 只改 zip、cbz 和 7z 文件行。头部以「项」收尾，目录行是「目录」，这两类留下。
fn rewrite_archive_line(line: &str) -> String {
    let Some((name, note)) = line.rsplit_once(" · ") else {
        return line.to_string();
    };
    if note.ends_with('项') || note == "目录" || !nestable_file(name) {
        return line.to_string();
    }
    format!("{name} · {DEPTH_NOTE}")
}

/// 最后一段有扩展名，并且是 zip、cbz 或 7z。只有前导点（`.7z`）不算。cbr 和 rar 不算。
fn nestable_file(name: &str) -> bool {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let Some((stem, extension)) = file.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && matches!(
            extension.to_ascii_lowercase().as_str(),
            "zip" | "cbz" | "7z"
        )
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::super::super::super::{InspectEffect, InspectMessage, InspectState, PreviewBody};
    use super::DEPTH_NOTE;

    /// 第 4 层的 zip 仍按大小列出并能再进一层；第 5 层里的 zip/7z 行尾换成上限说明，点开不取字节。
    /// rar 仍写原来的大小，点开是不能内联，不套这句上限。
    #[test]
    fn archive_nested_zip_stops_at_vue_depth_limit() {
        let shallow = "压缩包 / a.zip · 1 项\ne.zip · 12 B\nclip.rar · 4 B";
        assert_eq!(
            super::limit_layer_text(shallow.into(), 4),
            shallow,
            "第 4 层还按大小列 zip"
        );
        let limited = super::limit_layer_text(shallow.into(), 5);
        assert!(
            limited.lines().next().unwrap().ends_with("1 项"),
            "头部路径里的 .zip 不是文件行：{limited}"
        );
        assert!(
            limited.contains(&format!("e.zip · {DEPTH_NOTE}")),
            "{limited}"
        );
        assert!(limited.contains("clip.rar · 4 B"), "{limited}");

        let leaf = sample_zip(&[
            ("deep.zip", &sample_zip(&[("x.txt", b"x")])),
            ("more.7z", b"not-a-7z"),
            ("note.txt", b"n"),
            ("clip.rar", b"Rar!"),
        ]);
        let zip = wrap(
            "a.zip",
            &wrap(
                "b.zip",
                &wrap("c.zip", &wrap("d.zip", &wrap("e.zip", &leaf))),
            ),
        );
        let mut state = opened_archive(&zip, "pack.zip");
        match &state.body {
            PreviewBody::Native { content, .. } => {
                assert!(content.contains("a.zip · "), "{content}");
                assert!(!content.contains(DEPTH_NOTE), "最外层还没到上限");
            }
            other => panic!("不是压缩包名单：{other:?}"),
        }

        let mut chain = String::new();
        for name in ["a.zip", "b.zip", "c.zip", "d.zip"] {
            push_chain(&mut chain, name);
            open_nested(&mut state, &zip, &chain);
        }
        assert_eq!(state.archive_nests.len(), 4, "前四层都还进得去");
        let fourth = state.archive_nests.last().expect("第 4 层").text.clone();
        assert!(fourth.contains("e.zip · "), "{fourth}");
        assert!(
            !fourth.contains(DEPTH_NOTE),
            "第 4 层里的 zip 仍写大小：{fourth}"
        );

        push_chain(&mut chain, "e.zip");
        open_nested(&mut state, &zip, &chain);
        assert_eq!(state.archive_nests.len(), 5);
        let listed = state.archive_nests.last().expect("第 5 层").text.clone();
        let head = listed.lines().next().unwrap_or("");
        assert!(head.ends_with("4 项"), "这一层四个文件：{head}");
        assert!(head.contains("a.zip/b.zip/c.zip/d.zip/e.zip"), "{head}");
        assert!(
            listed.contains(&format!("deep.zip · {DEPTH_NOTE}")),
            "{listed}"
        );
        assert!(
            listed.contains(&format!("more.7z · {DEPTH_NOTE}")),
            "{listed}"
        );
        assert!(listed.contains("note.txt · 1 B"), "{listed}");
        assert!(listed.contains("clip.rar · 4 B"), "{listed}");

        let view = mount_archive(&state);
        assert_eq!(
            kind_text(&view, 1),
            "ZIP",
            "到上限的 zip 仍是文件，不是目录"
        );
        assert!(row_text(&view, 1).contains(DEPTH_NOTE), "行尾写出上限");
        assert!(row_text(&view, 2).contains(DEPTH_NOTE), "7z 行尾也是上限");
        assert!(!row_text(&view, 0).contains(DEPTH_NOTE), "rar 行尾仍是大小");
        assert!(row_text(&view, 3).contains("1 B"), "普通文件仍写大小");

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(format!("{chain}/clip.rar")),
        );
        assert!(state.take_effects().is_empty(), "rar 不按嵌套上限去取");
        assert_eq!(
            stage_of(&state),
            ("clip.rar", "无法内联预览该文件类型", false)
        );

        let deep = format!("{chain}/deep.zip");
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(deep.clone()),
        );
        assert!(state.take_effects().is_empty(), "到上限的 zip 不再展开");
        assert!(state.archive_entry.is_none(), "失败后仍是名单");
        assert_eq!(state.archive_nests.len(), 5, "层数停在第 5 层");
        let reason = format!("nested archive depth limit reached: {deep}");
        assert_eq!(
            stage_of(&state),
            ("无法预览内部文件", reason.as_str(), true)
        );
        let failed = mount_archive(&state);
        assert!(failed.find("无法预览内部文件").is_some());
        assert!(
            failed.find(&reason).is_some(),
            "原因用 Vue 取包内预览时的那句"
        );
        assert!(
            failed.keyed("inspect-archive-list").is_some(),
            "失败后名单还在"
        );
        assert!(
            row_text(&failed, 1).contains(DEPTH_NOTE),
            "失败后名单行尾的上限说明还在"
        );

        let seven = format!("{chain}/more.7z");
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(seven.clone()),
        );
        assert!(state.take_effects().is_empty(), "到上限的 7z 不尝试解开");
        assert_eq!(
            stage_of(&state).1,
            format!("nested archive depth limit reached: {seven}").as_str()
        );

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(format!("{chain}/note.txt")),
        );
        let effect = state.take_effects().pop().expect("普通文件仍要取");
        let InspectEffect::LoadArchiveEntry {
            entry,
            through,
            generation,
            ..
        } = effect
        else {
            panic!("不是包内文件")
        };
        assert_eq!(entry, "note.txt");
        let preview = super::super::preview_chain(&zip, &through, &entry).expect("文本取得出");
        let text = match &preview.kind {
            super::super::super::super::bridge::ArchiveEntryKind::Text { text, .. } => text.clone(),
            other => panic!("不是文本：{other:?}"),
        };
        assert_eq!(text, "n");
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::ArchiveEntryLoaded {
                path: "pack.zip".into(),
                entry,
                through,
                generation,
                result: Ok(preview),
            },
        );
        assert_eq!(state.archive_nests.len(), 5, "打开普通文件不丢掉这一层");
    }

    /// Vue `archivePreviewExtensions` 把 cbz 和 zip 放在一起，`archive_format_for_path` 把 cbz 当成 Zip。
    /// 目录单击只选中，再次进入只过滤已经列出的名字。cbr 在同一函数里是 Rar，仍不进入。
    #[test]
    fn archive_cbz_enters_current_layer_like_zip() {
        let page = sample_zip(&[
            ("photos/", b""),
            ("photos/b.txt", b"yo"),
            ("a.txt", b"hi"),
            ("clip.cbr", b"Rar!"),
        ]);
        let zip = sample_zip(&[("book.cbz", &page), ("clip.cbr", b"Rar!")]);
        let mut state = opened_archive(&zip, "pack.zip");
        let outer_names = name_list(state.archive.as_ref().expect("外层"));
        open_nested(&mut state, &zip, "book.cbz");
        assert_eq!(state.archive_nests.len(), 1);
        assert!(state.archive_entry.is_none(), "列完仍是名单");
        let listed = state.archive_nests.last().expect("cbz").text.clone();
        assert!(listed.contains("压缩包 / book.cbz · 3 项"), "{listed}");
        assert!(
            listed.contains("photos · 目录") && listed.contains("a.txt · 2 B"),
            "{listed}"
        );
        assert!(listed.contains("clip.cbr · 4 B"), "{listed}");
        assert!(
            !listed.contains(DEPTH_NOTE) && !listed.contains("yo") && !listed.contains("hi"),
            "{listed}"
        );
        let nest_names = name_list(&state.archive_nests.last().expect("cbz").catalog);
        assert_eq!(
            nest_names.join(","),
            "a.txt,clip.cbr,photos,photos/b.txt",
            "只留下包里的真实名字"
        );

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::SelectArchive("book.cbz/photos".into()),
        );
        assert_eq!(state.archive_selected.as_deref(), Some("book.cbz/photos"));
        assert!(
            state
                .archive_nests
                .last()
                .expect("cbz")
                .directory
                .is_empty(),
            "单击目录不进入"
        );
        assert_eq!(state.archive_nests.last().expect("cbz").text, listed);
        assert!(state.take_effects().is_empty(), "选中不重新打开压缩包");
        for path in ["book.cbz/missing", "book.cbz/a.txt", "book.cbz/clip.cbr"] {
            state.reduce(
                true,
                Some("repo"),
                InspectMessage::SelectArchive(path.into()),
            );
            assert_eq!(
                state.archive_selected.as_deref(),
                Some("book.cbz/photos"),
                "{path} 不能换成选中"
            );
        }
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::EnterArchive("book.cbz/missing".into()),
        );
        assert_eq!(
            state.archive_nests.last().expect("cbz").text,
            listed,
            "没有的名字不进入"
        );
        assert_eq!(
            name_list(state.archive.as_ref().expect("外层")),
            outer_names
        );
        assert_eq!(
            name_list(&state.archive_nests.last().expect("cbz").catalog),
            nest_names
        );
        assert!(state.take_effects().is_empty());

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::EnterArchive("book.cbz/photos".into()),
        );
        assert!(state.archive_selected.is_none(), "进入后清掉选中");
        assert_eq!(state.archive_nests.last().expect("cbz").directory, "photos");
        let inside = state.archive_nests.last().expect("cbz").text.clone();
        assert!(
            inside.contains("压缩包 / book.cbz/photos · 1 项"),
            "{inside}"
        );
        assert!(inside.contains("b.txt · 2 B"), "{inside}");
        assert!(
            !inside.contains("a.txt") && !inside.contains("clip.cbr") && !inside.contains("yo"),
            "{inside}"
        );
        assert_eq!(
            name_list(&state.archive_nests.last().expect("cbz").catalog),
            nest_names,
            "进入只过滤已经列出的名字"
        );
        assert_eq!(
            name_list(state.archive.as_ref().expect("外层")),
            outer_names
        );
        assert!(state.take_effects().is_empty(), "进入目录不重新打开压缩包");
        assert_eq!(stage_of(&state), ("photos", "选择内部文件进行预览", false));

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::EnterArchive("book.cbz".into()),
        );
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile("book.cbz/clip.cbr".into()),
        );
        assert!(
            state.take_effects().is_empty(),
            "cbr 不进入下一层，也不取字节"
        );
        assert_eq!(state.archive_nests.len(), 1, "点开 cbr 不增加一层");
        assert_eq!(
            stage_of(&state),
            ("clip.cbr", "无法内联预览该文件类型", false)
        );
        assert!(
            state
                .archive_nests
                .last()
                .expect("cbz")
                .text
                .contains("clip.cbr · 4 B")
        );

        let leaf = sample_zip(&[
            ("photos/", b""),
            ("deep.cbz", &sample_zip(&[("x.txt", b"x")])),
            ("clip.cbr", b"Rar!"),
            ("note.txt", b"n"),
        ]);
        let deep_zip = wrap(
            "a.cbz",
            &wrap(
                "b.cbz",
                &wrap("c.cbz", &wrap("d.cbz", &wrap("e.cbz", &leaf))),
            ),
        );
        let mut state = opened_archive(&deep_zip, "pack.zip");
        let mut chain = String::new();
        for name in ["a.cbz", "b.cbz", "c.cbz", "d.cbz"] {
            push_chain(&mut chain, name);
            open_nested(&mut state, &deep_zip, &chain);
        }
        assert_eq!(state.archive_nests.len(), 4, "前四个 cbz 都还进得去");
        let fourth = state.archive_nests.last().expect("第 4 层").text.clone();
        assert!(
            fourth.contains("e.cbz · ") && !fourth.contains(DEPTH_NOTE),
            "第 4 层里的 cbz 仍写大小：{fourth}"
        );

        push_chain(&mut chain, "e.cbz");
        open_nested(&mut state, &deep_zip, &chain);
        assert_eq!(state.archive_nests.len(), 5);
        let limited = state.archive_nests.last().expect("第 5 层").text.clone();
        assert!(limited.contains("photos · 目录"), "{limited}");
        assert!(
            limited.contains(&format!("deep.cbz · {DEPTH_NOTE}")),
            "{limited}"
        );
        assert!(limited.contains("clip.cbr · 4 B"), "{limited}");
        assert!(limited.contains("note.txt · 1 B"), "{limited}");
        assert!(
            !limited.contains("x.txt"),
            "没打开的 cbz 里不编造条目：{limited}"
        );
        let view = mount_archive(&state);
        assert_eq!(kind_text(&view, 0), "DIR", "目录行首仍是 DIR");
        assert_eq!(kind_text(&view, 1), "CBR");
        assert_eq!(
            kind_text(&view, 2),
            "CBZ",
            "到上限的 cbz 行首仍是扩展名前四位"
        );
        assert!(row_text(&view, 0).contains("目录"));
        assert!(!row_text(&view, 1).contains(DEPTH_NOTE), "cbr 行尾仍是大小");
        assert!(row_text(&view, 2).contains(DEPTH_NOTE));
        assert!(row_text(&view, 3).contains("1 B"));

        let deep = format!("{chain}/deep.cbz");
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(deep.clone()),
        );
        assert!(state.take_effects().is_empty(), "到上限的 cbz 不再展开");
        assert_eq!(state.archive_nests.len(), 5);
        let reason = format!("nested archive depth limit reached: {deep}");
        assert_eq!(
            stage_of(&state),
            ("无法预览内部文件", reason.as_str(), true)
        );

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(format!("{chain}/clip.cbr")),
        );
        assert!(
            state.take_effects().is_empty(),
            "到上限这一层的 cbr 仍不取字节"
        );
        assert_eq!(
            stage_of(&state),
            ("clip.cbr", "无法内联预览该文件类型", false)
        );

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(format!("{chain}/note.txt")),
        );
        let effect = state.take_effects().pop().expect("普通文件仍要取");
        let InspectEffect::LoadArchiveEntry { entry, through, .. } = effect else {
            panic!("不是包内文件")
        };
        assert_eq!(entry, "note.txt");
        let preview = super::super::preview_chain(&deep_zip, &through, &entry).expect("文本取得出");
        let text = match &preview.kind {
            super::super::super::super::bridge::ArchiveEntryKind::Text { text, .. } => text.clone(),
            other => panic!("不是文本：{other:?}"),
        };
        assert_eq!(text, "n");
    }

    /// 点开的文件行是当前项，和 Vue `is-selected` 一样。目录不再亮。再点另一条，当前项跟着换。
    /// 不能内联的类型也标这一行。名单里没有的名字不改当前项。点目录后当前项回到目录。
    #[test]
    fn archive_opened_file_row_is_selected() {
        let zip = sample_zip(&[
            ("photos/", b""),
            ("a.txt", b"alpha"),
            ("b.txt", b"beta"),
            ("clip.bin", b"nope"),
        ]);
        let mut state = opened_archive(&zip, "pack.zip");
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::SelectArchive("photos".into()),
        );
        assert_eq!(state.archive_selected.as_deref(), Some("photos"));

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile("a.txt".into()),
        );
        assert_eq!(
            state.archive_selected.as_deref(),
            Some("a.txt"),
            "点文件后当前项换成这一条"
        );
        let harness = mount_archive(&state);
        assert!(
            row_selected(&harness, row_index(&harness, "a.txt")),
            "正在打开的文件行是当前项"
        );
        assert!(
            !row_selected(&harness, row_index(&harness, "photos")),
            "目录行不再是当前项"
        );

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile("b.txt".into()),
        );
        assert_eq!(
            state.archive_selected.as_deref(),
            Some("b.txt"),
            "再点另一条，当前项跟着换"
        );
        let preparing = mount_archive(&state);
        assert!(row_selected(&preparing, row_index(&preparing, "b.txt")));
        assert!(
            !row_selected(&preparing, row_index(&preparing, "a.txt")),
            "上一条不再是当前项"
        );
        let _ = state.take_effects();

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile("clip.bin".into()),
        );
        assert!(state.take_effects().is_empty(), "不能内联的类型不取字节");
        assert_eq!(
            state.archive_selected.as_deref(),
            Some("clip.bin"),
            "不能内联时这一行仍是当前项"
        );
        let inline = mount_archive(&state);
        assert!(row_selected(&inline, row_index(&inline, "clip.bin")));
        assert!(!row_selected(&inline, row_index(&inline, "b.txt")));

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile("missing.txt".into()),
        );
        assert_eq!(
            state.archive_selected.as_deref(),
            Some("clip.bin"),
            "名单里没有的名字不改当前项"
        );

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::SelectArchive("photos".into()),
        );
        assert_eq!(
            state.archive_selected.as_deref(),
            Some("photos"),
            "点目录后当前项回到目录"
        );
        let picked = mount_archive(&state);
        assert!(row_selected(&picked, row_index(&picked, "photos")));
        assert!(!row_selected(&picked, row_index(&picked, "clip.bin")));
    }

    /// 点目录照 Vue `selectEntry` 再经 `renderInternalPreview`：目录没有扩展名，舞台写目录名和
    /// 「无法内联预览该文件类型」。已经打开的内部文件收起，名单留在左边。进入这一层后才回到空闲提示。
    #[test]
    fn archive_directory_click_puts_the_directory_on_the_stage() {
        let zip = sample_zip(&[("photos/", b""), ("a.txt", b"alpha")]);
        let mut state = opened_archive(&zip, "pack.zip");
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile("a.txt".into()),
        );
        let effect = state.take_effects().pop().expect("应去取 a.txt");
        let InspectEffect::LoadArchiveEntry {
            entry,
            through,
            generation,
            ..
        } = effect
        else {
            panic!("不是包内文件")
        };
        let preview = super::super::preview_chain(&zip, &through, &entry).expect("文本");
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::ArchiveEntryLoaded {
                path: "pack.zip".into(),
                entry,
                through,
                generation,
                result: Ok(preview),
            },
        );
        let opened = mount_archive(&state);
        assert!(opened.find("alpha").is_some(), "打开后舞台是这一条");
        assert!(opened.keyed("inspect-archive-list").is_some(), "名单还在");

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::SelectArchive("photos".into()),
        );
        assert_eq!(state.archive_selected.as_deref(), Some("photos"));
        assert!(state.archive_entry.is_none(), "点目录收起已经打开的文件");
        assert!(state.archive_request.is_none(), "点目录不再取字节");
        assert_eq!(
            stage_of(&state),
            ("photos", "无法内联预览该文件类型", false)
        );
        let picked = mount_archive(&state);
        assert!(picked.find("alpha").is_none(), "舞台不再是刚才那条文本");
        assert!(picked.find("无法内联预览该文件类型").is_some());
        assert!(picked.find("photos").is_some(), "舞台写目录名");
        assert!(
            picked.keyed("inspect-archive-list").is_some(),
            "名单留在左边"
        );
        assert!(row_selected(&picked, row_index(&picked, "photos")));
        assert!(!row_selected(&picked, row_index(&picked, "a.txt")));

        state.reduce(
            true,
            Some("repo"),
            InspectMessage::EnterArchive("photos".into()),
        );
        assert!(state.archive_selected.is_none(), "进入后清掉选中");
        assert_eq!(
            stage_of(&state),
            ("photos", "选择内部文件进行预览", false),
            "进入后回到这一层的空闲提示"
        );
    }

    /// 已经读出的条目路径。选中和进入都不该改这份名单。
    fn name_list(catalog: &super::super::super::super::bridge::ArchiveCatalog) -> Vec<String> {
        let mut paths = catalog
            .names
            .iter()
            .map(|name| name.path.clone())
            .collect::<Vec<_>>();
        paths.sort_unstable();
        paths
    }

    fn push_chain(chain: &mut String, name: &str) {
        if !chain.is_empty() {
            chain.push('/');
        }
        chain.push_str(name);
    }

    /// 点开这一层的 zip，把列出来的名单收进嵌套层。
    fn open_nested(state: &mut InspectState, zip: &[u8], path: &str) {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(path.into()),
        );
        let effect = state
            .take_effects()
            .pop()
            .unwrap_or_else(|| panic!("应能进入 {path}"));
        let InspectEffect::LoadArchiveEntry {
            entry,
            through,
            generation,
            ..
        } = effect
        else {
            panic!("不是包内文件")
        };
        let preview = super::super::preview_chain(zip, &through, &entry)
            .unwrap_or_else(|error| panic!("{path}：{error}"));
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::ArchiveEntryLoaded {
                path: "pack.zip".into(),
                entry,
                through,
                generation,
                result: Ok(preview),
            },
        );
    }

    fn opened_archive(bytes: &[u8], path: &str) -> InspectState {
        let loaded = super::super::super::load("momobako.preview.archive", bytes).expect("archive");
        let mut state = InspectState::default();
        state.generation = 1;
        state.repo_id = Some("repo".into());
        state.target_path = Some(path.into());
        state.body = PreviewBody::Native {
            view_id: "momobako.preview.archive".into(),
            label: "压缩包".into(),
            content: String::new(),
        };
        super::super::super::note_loaded(&mut state, path.into(), 1, Ok(loaded));
        state
    }

    fn stage_of(state: &InspectState) -> (&str, &str, bool) {
        crate::shell::inspect_library::archive_stage(state)
    }

    fn mount_archive(state: &InspectState) -> crate::shell::view_harness::ShellHarness {
        let mut model =
            crate::shell::ShellViewModel::for_page(crate::shell::ShellPage::SelectedFile);
        model.inspect = state.clone();
        crate::shell::view_harness::ShellHarness::mount(model)
    }

    fn row_text(harness: &crate::shell::view_harness::ShellHarness, index: usize) -> String {
        node_label(harness, &format!("inspect-archive-row-{index}"))
    }

    /// 名单里名字落在哪一行。
    fn row_index(harness: &crate::shell::view_harness::ShellHarness, name: &str) -> usize {
        (0..8)
            .find(|index| {
                harness
                    .keyed(&format!("inspect-archive-row-{index}"))
                    .is_some()
                    && row_text(harness, *index).contains(name)
            })
            .unwrap_or_else(|| panic!("名单里没有 {name}"))
    }

    /// 这一行是不是当前项。和目录行用的是同一个列表选中。
    fn row_selected(harness: &crate::shell::view_harness::ShellHarness, index: usize) -> bool {
        let id = harness
            .keyed(&format!("inspect-archive-row-{index}"))
            .unwrap_or_else(|| panic!("没有名单行 {index}"));
        harness
            .document()
            .context()
            .world()
            .accessibility(id)
            .and_then(|state| state.selected)
            .unwrap_or(false)
    }

    fn kind_text(harness: &crate::shell::view_harness::ShellHarness, index: usize) -> String {
        node_label(harness, &format!("inspect-archive-kind-{index}"))
    }

    fn node_label(harness: &crate::shell::view_harness::ShellHarness, key: &str) -> String {
        let id = harness
            .keyed(key)
            .unwrap_or_else(|| panic!("没有节点 {key}"));
        harness
            .nodes()
            .into_iter()
            .find(|node| node.id == id)
            .and_then(|node| node.label.map(|label| label.to_string()))
            .unwrap_or_default()
    }

    fn wrap(name: &str, inner: &[u8]) -> Vec<u8> {
        sample_zip(&[(name, inner)])
    }

    fn sample_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, bytes) in entries {
                archive.start_file(*name, options).expect("entry");
                archive.write_all(bytes).expect("bytes");
            }
            archive.finish().expect("finish");
        }
        cursor.into_inner()
    }
}

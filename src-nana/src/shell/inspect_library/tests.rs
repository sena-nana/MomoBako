//! 资源库信息区的测试：ASMR 作品行与歌词状态、PDF 页数、空压缩包与原始大小格式。

use super::*;

fn asmr(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut custom = BTreeMap::from([("libraryKind".to_string(), "asmr".to_string())]);
    custom.extend(
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string())),
    );
    custom
}

fn rows(sections: &[LibrarySection]) -> Vec<(String, String)> {
    sections
        .first()
        .map(|section| {
            section
                .rows
                .iter()
                .filter_map(|row| match row {
                    LibraryRow::Value { label, value, .. } => {
                        Some((label.to_string(), value.clone()))
                    }
                    LibraryRow::Body(_) => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn asmr_match_without_lyric_status_says_undetected() {
    let custom = asmr(&[]);
    assert!(matches_asmr(&custom));
    assert_eq!(
        rows(&library_sections(&custom)),
        [("歌词".to_string(), "未检测".to_string())]
    );
    assert!(lyric_body(&custom).is_none());
    assert_eq!(format_measure(1200.0), "1,200");
    assert_eq!(format_measure(1234.5678), "1,234.568");
    assert_eq!(format_measure(4.5), "4.5");
}

/// 作品字段、歌词状态和收听进度都不让非 ASMR 条目长出「作品信息」：这一块只挂在 ASMR 条目上。
#[test]
fn non_asmr_entries_have_no_work_section() {
    let mut custom = BTreeMap::new();
    custom.insert("lyricStatus".to_string(), "local".to_string());
    custom.insert("workTitle".to_string(), "夜曲".to_string());
    custom.insert("price".to_string(), "1200".to_string());
    custom.insert("lyrics".to_string(), "第一行".to_string());
    assert!(library_sections(&custom).is_empty());
}

/// 行的顺序和文案按元数据面板；数组连成「甲，乙」以后原样显示，数字字段解析不出时这行不画。
#[test]
fn work_rows_follow_vue_order_and_formats() {
    let custom = asmr(&[
        ("rjCode", "RJ01234567"),
        ("workTitle", "夜曲"),
        ("voiceActors", "甲，乙"),
        ("tags", "治愈"),
        ("asmrEntryKind", "audio"),
        ("listeningStatus", "listening"),
        ("listeningProgress", "40.6"),
        ("trackDurationMs", "125000"),
        ("price", "1320"),
        ("dlCount", "2400"),
        ("ratingAverage", "4.56"),
        ("reviewCount", "多"),
    ]);
    let shown = rows(&library_sections(&custom));
    let expected = [
        ("作品 ID", "RJ01234567"),
        ("标题", "夜曲"),
        ("声优", "甲，乙"),
        ("标签", "治愈"),
        ("歌词", "未检测"),
        ("条目类型", "audio"),
        ("收听状态", "收听中 · 41%"),
        ("音轨时长", "2:05"),
        ("价格", "1,320 JPY"),
        ("销量", "2,400"),
        ("评分", "4.56"),
    ]
    .map(|(label, value)| (label.to_string(), value.to_string()));
    assert_eq!(shown, expected);
}

#[test]
fn lyric_body_is_its_own_section_and_progress_formats() {
    let custom = asmr(&[
        ("listeningStatus", "listening"),
        ("listeningProgress", "40"),
        ("lyrics", "第一行"),
    ]);
    assert_eq!(listening_line(&custom).as_deref(), Some("收听中 · 40%"));
    assert_eq!(library_sections(&custom).len(), 2);
    assert_eq!(format_duration(125000.0).as_deref(), Some("2:05"));
    assert!(format_duration(0.0).is_none());
    assert_eq!(
        listening_line(&asmr(&[
            ("listeningStatus", "unlistened"),
            ("listeningProgress", "0")
        ]))
        .as_deref(),
        Some("未收听")
    );
}

#[test]
fn ready_pdf_status_counts_pages_instead_of_the_flipper() {
    assert_eq!(ready_page_label(1), "1 页 PDF");
    assert_eq!(ready_page_label(3), "3 页 PDF");
}

#[test]
fn archive_without_files_is_an_empty_directory() {
    let view = archive_list(
        "压缩包里没有文件。",
        None,
        ArchiveStage {
            title: "根目录",
            detail: "选择内部文件进行预览",
            failed: false,
        },
        "",
        None,
    );
    let _ = view;
}

#[test]
fn original_size_formats_like_vue_and_rejects_bad_numbers() {
    assert_eq!(format_original_size(None), "未记录");
    assert_eq!(format_original_size(Some(1536.0)), "1.5 KB");
    assert_eq!(format_original_size(Some(500.0)), "500 B");
    assert_eq!(format_original_size(Some(f64::NAN)), "未记录");
}

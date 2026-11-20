//! 压缩包名单上的文件大小。
//!
//! 对齐 Vue `preview-archive` 的 `formatByteCount`（`7bc847c:External/Plugins/preview-archive/src/register.js`）。
//! 只格式化已经从压缩包读到的字节数。目录不经过这里，仍写「目录」。
//! 没有大小字段时不要调用本函数编一个数。

/// 把已经读到的字节数写成 Vue 压缩包名单的大小。
///
/// 0 字节是空文件，写「0 B」，不是大小未知。其余按 1024 进到 KB、MB、GB，到 GB 不再进位。
/// 单位仍是 B，或换算后不小于 10 时不留小数；换算后小于 10 留一位小数。
/// 数字和单位之间一个空格。小数按 `Number#toFixed` 四舍五入，半数远离 0。
pub(super) fn format_archive_entry_size(size: u64) -> String {
    if size == 0 {
        return "0 B".to_string();
    }
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut current = size as f64;
    let mut unit = 0usize;
    // 和 Vue 一样：当前值不小于 1024 就进一档，停在 GB。
    while current >= 1024.0 && unit + 1 < UNITS.len() {
        current /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{size} B");
    }
    let digits = if current >= 10.0 { 0 } else { 1 };
    format!("{} {}", fixed_digits(current, digits), UNITS[unit])
}

/// 对齐 `Number#toFixed`：放大 10^digits 后四舍五入，再补齐小数位。
fn fixed_digits(value: f64, digits: u32) -> String {
    let scale = 10f64.powi(digits as i32);
    let rounded = (value * scale).round();
    if digits == 0 {
        return format!("{}", rounded as u64);
    }
    let scale_int = scale as u64;
    let rounded_int = rounded as u64;
    let whole = rounded_int / scale_int;
    let frac = rounded_int % scale_int;
    format!("{whole}.{frac:0width$}", width = digits as usize)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::super::super::super::bridge::{ArchiveCatalog, ArchiveName};
    use super::format_archive_entry_size;

    #[test]
    fn archive_entry_size_uses_vue_byte_units() {
        assert_eq!(format_archive_entry_size(0), "0 B");
        assert_eq!(format_archive_entry_size(1), "1 B");
        assert_eq!(format_archive_entry_size(6), "6 B");
        assert_eq!(format_archive_entry_size(1023), "1023 B");
        assert_eq!(format_archive_entry_size(1024), "1.0 KB");
        assert_eq!(format_archive_entry_size(1075), "1.0 KB");
        assert_eq!(format_archive_entry_size(1076), "1.1 KB");
        assert_eq!(format_archive_entry_size(1536), "1.5 KB");
        assert_eq!(format_archive_entry_size(2048), "2.0 KB");
        assert_eq!(format_archive_entry_size(10239), "10.0 KB");
        assert_eq!(format_archive_entry_size(10240), "10 KB");
        assert_eq!(format_archive_entry_size(1_048_575), "1024 KB");
        assert_eq!(format_archive_entry_size(1_048_576), "1.0 MB");
        assert_eq!(format_archive_entry_size(10_485_760), "10 MB");
        assert_eq!(format_archive_entry_size(1_073_741_824), "1.0 GB");
        assert_eq!(format_archive_entry_size(10_737_418_240), "10 GB");
        assert_eq!(format_archive_entry_size(1_099_511_627_776), "1024 GB");

        let zip = sample_zip(&[
            ("dir/", b""),
            ("empty.txt", b""),
            ("kilo.bin", &vec![0_u8; 1024]),
            ("mid.bin", &vec![0_u8; 1536]),
            ("ten.bin", &vec![0_u8; 10240]),
        ]);
        let loaded = super::super::list(&zip).expect("zip");
        let text = loaded.text;
        assert!(text.contains("dir · 目录"), "{text}");
        assert!(text.contains("empty.txt · 0 B"), "{text}");
        assert!(text.contains("kilo.bin · 1.0 KB"), "{text}");
        assert!(text.contains("mid.bin · 1.5 KB"), "{text}");
        assert!(text.contains("ten.bin · 10 KB"), "{text}");
        assert!(!text.contains("字节"), "{text}");
        assert!(!text.contains("dir · 0"), "{text}");
        let kilo = loaded
            .archive
            .expect("名单")
            .names
            .into_iter()
            .find(|name| name.path == "kilo.bin")
            .expect("kilo");
        assert_eq!(kilo.size, 1024, "显示换成单位，读到的字节数还在");

        let flat = super::super::flat_text(&ArchiveCatalog {
            names: vec![
                ArchiveName {
                    path: "dir".into(),
                    size: 0,
                    directory: true,
                },
                ArchiveName {
                    path: "empty.txt".into(),
                    size: 0,
                    directory: false,
                },
                ArchiveName {
                    path: "kilo.bin".into(),
                    size: 1024,
                    directory: false,
                },
            ],
            layered: false,
            truncated: false,
        });
        assert!(flat.contains("empty.txt · 0 B"), "{flat}");
        assert!(flat.contains("kilo.bin · 1.0 KB"), "{flat}");
        assert!(!flat.contains("dir"), "{flat}");
        assert!(!flat.contains("字节"), "{flat}");
    }

    fn sample_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, bytes) in entries {
                archive.start_file(*name, options).expect("entry");
                if !name.ends_with('/') {
                    archive.write_all(bytes).expect("bytes");
                }
            }
            archive.finish().expect("finish");
        }
        cursor.into_inner()
    }
}

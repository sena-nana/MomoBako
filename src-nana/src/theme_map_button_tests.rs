//! 壳层按钮不写死颜色的源码检查。
//!
//! 扫 `src/shell` 里所有画按钮的文件：直接建 `Button` / `IconButton`，或者调用别处建按钮的函数。
//! 按钮的底、字和边都取主题角色（`theme_map.rs` 的对应），这些文件里不用 `ButtonPaintOverride`，
//! 也不出现颜色字面量；确实不是按钮配色、Vue 也写死的颜色列在 [`ALLOWED_LITERALS`] 里，写清出处。

use std::path::Path;

/// 画按钮的壳层文件里允许出现的颜色字面量：文件名、字面量原文和出处。都不是按钮的配色，而是 Vue
/// 写死、两套主题共用的颜色。没用上的条目也算失败，免得清单过时。
const ALLOWED_LITERALS: &[(&str, &str, &str)] = &[
    ("admin_logs_view.rs", "[0x5c, 0x70, 0x88]", "日志级别「调试」徽章，workspace.css `.logs-workbench__level.is-debug`"),
    ("admin_logs_view.rs", "[0xdc, 0xa3, 0x3a]", "日志级别「警告」徽章底，`.logs-workbench__level.is-warn`"),
    ("admin_logs_view.rs", "[0xb6, 0x7a, 0x09]", "日志级别「警告」徽章字，`.logs-workbench__level.is-warn`"),
    ("files_style.rs", "[58.0 / 255.0, 58.0 / 255.0, 58.0 / 255.0, 1.0]", "缩略图占位底（深色），index.css `--thumbnail-placeholder-bg`"),
    ("files_style.rs", "[216.0 / 255.0, 220.0 / 255.0, 226.0 / 255.0, 1.0]", "缩略图占位底（浅色），index.css `--thumbnail-placeholder-bg`"),
    ("files_style.rs", "[199.0 / 255.0, 165.0 / 255.0, 102.0 / 255.0, 1.0]", "文件夹缩略图渐变起点，filePresentation.ts"),
    ("files_style.rs", "[115.0 / 255.0, 85.0 / 255.0, 47.0 / 255.0, 1.0]", "文件夹缩略图渐变终点，filePresentation.ts"),
    ("files_style.rs", "[1.0, 1.0, 1.0, 0.92]", "缩略图上的类型图标，workspace.css `rgba(255, 255, 255, 0.92)`"),
    ("inspect_metadata_view.rs", "[212.0 / 255.0, 166.0 / 255.0, 61.0 / 255.0, 1.0]", "评分星的金色，`.file-metadata-card__star.is-active`"),
    ("inspect_preview_body.rs", "[0.02, 0.02, 0.02, 1.0]", "视频画面的黑底，`.media-preview__video`"),
    ("inspect_preview_page.rs", "[1.0, 1.0, 1.0, 1.0]", "PDF 页纸，`.office-preview__viewer--pdf canvas`"),
    ("inspect_preview_page.rs", "[0.0, 0.0, 0.0, 0.24]", "PDF 页纸投影，`.office-preview__viewer--pdf canvas`"),
    ("player_bar.rs", "[0.0, 0.0, 0.0, 0.45]", "播放条卡片投影，shell.css `.workspace-player`"),
    ("player_bar_parts.rs", "[0.0, 0.0, 0.0, 0.6]", "播放队列弹层投影，shell.css `.workspace-player__queue`"),
    ("sidebar_popover_view.rs", "[0.0, 0.0, 0.0, 0.62]", "侧栏弹层投影，shell.css `.task-popover`、`.repository-add-popover`"),
    ("source_auth_page.rs", "[1.0, 1.0, 1.0, 1.0]", "扫码登录二维码图片的白底，`.source-auth-settings__qr` 的 `background: white`"),
];

/// 别的文件拿来建按钮的壳层函数。调用它们的文件也算画按钮。
const BUTTON_HELPERS: &[&str] = &[
    "action",
    "action_button",
    "intent_button",
    "styled_button",
    "glyph_button",
    "hit_area",
    "primary_button",
    "bar_button",
    "pill_button",
    "close_button",
    "chip",
];

/// 去掉文件末尾的内联测试模块，只留产品代码的行。壳层文件的内联测试模块都在文件末尾。
fn product_lines(source: &str) -> Vec<&str> {
    let lines: Vec<&str> = source.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() != "#[cfg(test)]" {
            continue;
        }
        let next = lines[index + 1..].iter().map(|line| line.trim()).find(|line| !line.starts_with("#["));
        if next.is_some_and(|line| line.contains("mod ") && line.ends_with('{')) {
            return lines[..index].to_vec();
        }
    }
    lines
}

/// 一行里 `//` 之前的代码。
fn code_of(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

/// 这段代码是不是在建按钮：直接 `Button::new` / `IconButton::new`，或者调用别处建按钮的函数
/// （方法调用、更长的名字和函数定义不算）。
fn builds_buttons(code: &str) -> bool {
    if code.contains("Button::new(") {
        return true;
    }
    BUTTON_HELPERS.iter().any(|helper| {
        let call = format!("{helper}(");
        code.match_indices(&call).any(|(at, _)| {
            let before = &code[..at];
            let bare = !before.ends_with(|ch: char| ch.is_alphanumeric() || ch == '_' || ch == '.');
            bare && !before.ends_with("fn ")
        })
    })
}

/// 一段数组元素是不是写死的数：`0.45`、`212.0 / 255.0` 这类。
fn is_number(part: &str) -> bool {
    let part = part.trim().trim_end_matches("f32");
    !part.is_empty()
        && part.split('/').all(|piece| {
            let piece = piece.trim();
            !piece.is_empty() && piece.chars().all(|ch| ch.is_ascii_digit() || ch == '.' || ch == '_')
        })
}

/// 一段数组元素是不是 `0x5c` 这类十六进制数。
fn is_hex_byte(part: &str) -> bool {
    part.trim().strip_prefix("0x").is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_hexdigit()))
}

/// 一行代码里的颜色字面量：`"#rgb"` 这类十六进制串、四个数的 RGBA 数组和三个 `0x` 数的 RGB 数组。
/// 属性（`#[derive]`）和名字里带 rgba 的函数不算。
fn color_literals(code: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in code.match_indices("\"#") {
        let digits: String = code[at + 2..].chars().take_while(char::is_ascii_hexdigit).collect();
        if matches!(digits.len(), 3 | 4 | 6 | 8) && code[at + 2 + digits.len()..].starts_with('"') {
            found.push(format!("\"#{digits}\""));
        }
    }
    let mut rest = code;
    while let Some(open) = rest.find('[') {
        let tail = &rest[open + 1..];
        let Some(close) = tail.find(['[', ']']) else {
            break;
        };
        if tail[close..].starts_with(']') {
            let parts: Vec<&str> = tail[..close].split(',').collect();
            let rgba = parts.len() == 4 && parts.iter().all(|part| is_number(part));
            let rgb = parts.len() == 3 && parts.iter().all(|part| is_hex_byte(part));
            if rgba || rgb {
                found.push(format!("[{}]", &tail[..close]));
            }
        }
        rest = &tail[close..];
    }
    found
}

#[test]
fn shell_buttons_do_not_hardcode_paint_colors() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shell");
    let mut paths: Vec<_> = std::fs::read_dir(&dir).expect("读壳层目录").map(|entry| entry.expect("目录项").path()).collect();
    paths.sort();
    let mut scanned = 0;
    let mut used = vec![false; ALLOWED_LITERALS.len()];
    for path in paths {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("读壳层源码");
        let lines = product_lines(&source);
        let code: Vec<&str> = lines.iter().map(|line| code_of(line)).collect();
        if !builds_buttons(&code.join("\n")) {
            continue;
        }
        scanned += 1;
        for (index, line) in code.iter().enumerate() {
            assert!(!line.contains("ButtonPaintOverride"), "shell/{name}:{} 用 ButtonPaintOverride 写死按钮颜色", index + 1);
            for literal in color_literals(line) {
                let Some(slot) = ALLOWED_LITERALS.iter().position(|(file, text, _)| *file == name && *text == literal) else {
                    panic!("shell/{name}:{} 画按钮的文件里写死了颜色 {literal}：{}", index + 1, lines[index].trim());
                };
                used[slot] = true;
            }
        }
    }
    assert!(scanned >= 30, "只找到 {scanned} 个画按钮的壳层文件，扫描条件可能失效了");
    for ((file, text, why), used) in ALLOWED_LITERALS.iter().zip(used) {
        assert!(used, "允许清单里的 {file} {text}（{why}）已经不在了，删掉这一条");
    }
}

#[test]
fn the_scan_tells_color_literals_from_attributes_and_names() {
    assert!(color_literals("#[derive(Clone)] let pixels = raster.rgba(); fn rgba(rgb: [u8; 3]) -> [f32; 4]").is_empty());
    assert_eq!(color_literals("let a = \"#c7a566\"; let b = [0.0, 0.0, 0.0, 0.45];"), ["\"#c7a566\"", "[0.0, 0.0, 0.0, 0.45]"]);
    assert_eq!(color_literals("rgb: [0x5c, 0x70, 0x88], size: [0.0, 1.0, 2.0], at: [4.0, 6.0, 8.0, w]"), ["[0x5c, 0x70, 0x88]"]);
    assert!(builds_buttons("let save = action(\"保存\", None, Tone::Plain, false);"));
    assert!(!builds_buttons("fn action(text: &str) {} self.action(1); source_action(icon);"));
}

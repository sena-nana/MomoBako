//! 压缩包条目的解压读取，以及 zip 与其他格式的列举、层级文本与当前目录行。

use super::*;

/// zip 打得开就只读这一条。打不开再试 7z。不试 rar，也不读取其它条目的内容。
pub(super) fn extract_listed(bytes: &[u8], path: &str) -> Result<Vec<u8>, String> {
    match ZipArchive::new(Cursor::new(bytes)) {
        Ok(archive) => extract_zip(archive, path),
        Err(error) => {
            eprintln!("Nana 压缩包按 zip 取出失败：{error}");
            extract_sevenz(bytes, path)
        }
    }
}

pub(super) fn extract_zip(
    mut archive: ZipArchive<Cursor<&[u8]>>,
    path: &str,
) -> Result<Vec<u8>, String> {
    let mut found = None;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|error| {
            eprintln!("Nana 压缩包条目读取失败：{error}");
            format!("无法读取压缩包条目：{error}")
        })?;
        let Some((name, directory)) = normalize_entry_name(file.name(), file.is_dir()) else {
            continue;
        };
        if directory || name != path {
            continue;
        }
        if file.size() > ENTRY_CAP {
            eprintln!(
                "Nana 压缩包这一条有 {} 字节，超过 {ENTRY_CAP}，不读进内存：{path}",
                file.size()
            );
            return Err(format!("{path} 太大，不能预览"));
        }
        found = Some(index);
        break;
    }
    let Some(index) = found else {
        eprintln!("Nana 压缩包里没有这一条：{path}");
        return Err(format!("压缩包里没有 {path}"));
    };
    let mut file = archive.by_index(index).map_err(|error| {
        eprintln!("Nana 压缩包条目读取失败：{error}");
        format!("无法读取压缩包条目：{error}")
    })?;
    read_capped(&mut file, path)
}

pub(super) fn extract_sevenz(bytes: &[u8], path: &str) -> Result<Vec<u8>, String> {
    let mut archive = UnifiedArchive::open_with_format(Cursor::new(bytes), ArchiveFormat::SevenZ)
        .map_err(|error| {
        eprintln!("Nana 压缩包按 7z 打开失败：{error}");
        format!("无法读取压缩包：{error}")
    })?;
    let mut matched = None;
    for item in archive.entries_iter() {
        let entry = item.map_err(|error| {
            eprintln!("Nana 压缩包条目读取失败：{error}");
            format!("无法读取压缩包条目：{error}")
        })?;
        let Some((name, directory)) = normalize_entry_name(entry.name(), entry.is_directory())
        else {
            continue;
        };
        if directory || name != path {
            continue;
        }
        if entry.original_size() > ENTRY_CAP {
            eprintln!(
                "Nana 压缩包这一条有 {} 字节，超过 {ENTRY_CAP}，不读进内存：{path}",
                entry.original_size()
            );
            return Err(format!("{path} 太大，不能预览"));
        }
        // 固实块里只有块内第一个文件带压缩大小。后面的文件要先把前面的都解开，这里不取。
        if entry.original_size() > 0 && entry.compressed_size() == 0 {
            eprintln!("Nana 7z 这一条不在压缩块开头，不能单独取出：{path}");
            return Err(format!("{path} 不能单独取出"));
        }
        matched = Some(entry);
        break;
    }
    let Some(entry) = matched else {
        eprintln!("Nana 压缩包里没有这一条：{path}");
        return Err(format!("压缩包里没有 {path}"));
    };
    let data = archive.read(&entry).map_err(|error| {
        eprintln!("Nana 压缩包取出失败：{path}：{error}");
        format!("无法取出 {path}：{error}")
    })?;
    if data.len() as u64 > ENTRY_CAP {
        eprintln!(
            "Nana 压缩包这一条解开后有 {} 字节，超过 {ENTRY_CAP}，丢掉：{path}",
            data.len()
        );
        return Err(format!("{path} 太大，不能预览"));
    }
    Ok(data)
}

pub(super) fn read_capped(file: &mut impl Read, path: &str) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    file.take(ENTRY_CAP)
        .read_to_end(&mut buf)
        .map_err(|error| {
            eprintln!("Nana 压缩包取出失败：{path}：{error}");
            format!("无法取出 {path}：{error}")
        })?;
    Ok(buf)
}

/// 和目录一样去掉首尾斜杠，拒绝 `.`、`..` 和盘符。文件路径不能是空的。
pub(super) fn normalize_file(path: &str) -> Option<String> {
    let normalized = normalize_directory(path)?;
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(super) fn list_zip(mut archive: ZipArchive<Cursor<&[u8]>>) -> Result<NativeLoad, String> {
    let mut names = Vec::new();
    let mut index = 0;
    let len = archive.len();
    while index < len && names.len() < LIST_LIMIT {
        let file = archive.by_index(index).map_err(|error| {
            eprintln!("Nana 压缩包条目读取失败：{error}");
            format!("无法读取压缩包条目：{error}")
        })?;
        index += 1;
        let Some((path, directory)) = normalize_entry_name(file.name(), file.is_dir()) else {
            continue;
        };
        names.push(ArchiveName {
            path,
            size: file.size(),
            directory,
        });
    }
    // 到上限就停。多出来的条目不读进列表，避免为了对齐整包目录把剩余名字都留下。
    let truncated = names.len() == LIST_LIMIT && index < len;
    if truncated {
        eprintln!("Nana 压缩包列表在 {LIST_LIMIT} 条处截断");
    }
    finish(names, truncated, true)
}

pub(super) fn list_unarc(bytes: &[u8]) -> Result<NativeLoad, String> {
    match list_unarc_format(bytes, ArchiveFormat::SevenZ, true) {
        Ok(load) => Ok(load),
        Err(error) => {
            eprintln!("Nana 压缩包按 7z 打开失败：{error}");
            match list_unarc_format(bytes, ArchiveFormat::Rar, false) {
                Ok(load) => Ok(load),
                Err(rar) => {
                    eprintln!("Nana 压缩包按 rar 打开失败：{rar}");
                    Err(rar)
                }
            }
        }
    }
}

pub(super) fn list_unarc_format(
    bytes: &[u8],
    format: ArchiveFormat,
    layered: bool,
) -> Result<NativeLoad, String> {
    let mut archive = UnifiedArchive::open_with_format(Cursor::new(bytes), format)
        .map_err(|error| format!("无法读取压缩包：{error}"))?;
    let mut names = Vec::new();
    let mut truncated = false;
    for item in archive.entries_iter() {
        if names.len() == LIST_LIMIT {
            truncated = true;
            eprintln!("Nana 压缩包列表在 {LIST_LIMIT} 条处截断");
            break;
        }
        let entry = item.map_err(|error| {
            eprintln!("Nana 压缩包条目读取失败：{error}");
            format!("无法读取压缩包条目：{error}")
        })?;
        let Some((path, directory)) = normalize_entry_name(entry.name(), entry.is_directory())
        else {
            continue;
        };
        names.push(ArchiveName {
            path,
            size: entry.original_size(),
            directory,
        });
    }
    finish(names, truncated, layered)
}

pub(super) fn finish(
    mut names: Vec<ArchiveName>,
    truncated: bool,
    layered: bool,
) -> Result<NativeLoad, String> {
    // 只在已经留下的名字里认目录。7z 的目录常常不带斜杠，靠后面的 `name/` 前缀认出来。
    mark_prefixes(&mut names);
    let catalog = ArchiveCatalog {
        names,
        layered,
        truncated,
    };
    let text = layer_text(&catalog, "");
    Ok(NativeLoad {
        text,
        frames: Vec::new(),
        mesh: None,
        paged: false,
        archive: Some(catalog),
    })
}

pub(super) fn normalize_entry_name(raw: &str, directory: bool) -> Option<(String, bool)> {
    let name = raw.replace('\\', "/");
    let directory = directory || name.ends_with('/');
    let path = name.trim_matches('/').to_string();
    if path.is_empty() {
        None
    } else {
        Some((path, directory))
    }
}

pub(super) fn mark_prefixes(names: &mut [ArchiveName]) {
    let paths: Vec<String> = names.iter().map(|name| name.path.clone()).collect();
    for name in names {
        let prefix = format!("{}/", name.path);
        if paths.iter().any(|other| other.starts_with(&prefix)) {
            name.directory = true;
        }
    }
}

/// 当前目录的直接子项。名字只取路径的下一段，目录排在文件前面。
pub(super) fn layer_text(catalog: &ArchiveCatalog, directory: &str) -> String {
    if !catalog.layered {
        return flat_text(catalog);
    }
    let rows = current_rows(catalog, directory);
    if rows.is_empty() && directory.is_empty() && !catalog.truncated {
        return "压缩包里没有文件。".into();
    }
    let mut lines = Vec::new();
    if directory.is_empty() {
        lines.push(format!("压缩包 · {} 项", rows.len()));
    } else {
        lines.push(format!("压缩包 / {directory} · {} 项", rows.len()));
    }
    for row in rows {
        match row {
            LayerRow::Dir(name) => lines.push(format!("{name} · 目录")),
            LayerRow::File(name, size) => lines.push(format!(
                "{name} · {}",
                entry_size::format_archive_entry_size(size)
            )),
        }
    }
    if catalog.truncated {
        lines.push("…".into());
    }
    lines.join("\n")
}

pub(super) fn flat_text(catalog: &ArchiveCatalog) -> String {
    let mut lines = Vec::new();
    for name in &catalog.names {
        if name.directory {
            continue;
        }
        lines.push(format!(
            "{} · {}",
            name.path,
            entry_size::format_archive_entry_size(name.size)
        ));
    }
    if lines.is_empty() && !catalog.truncated {
        return "压缩包里没有文件。".into();
    }
    let count = lines.len();
    if catalog.truncated {
        lines.push("…".into());
    }
    format!("压缩包 · {count} 个文件\n{}", lines.join("\n"))
}

pub(super) enum LayerRow {
    Dir(String),
    File(String, u64),
}

pub(super) fn current_rows(catalog: &ArchiveCatalog, directory: &str) -> Vec<LayerRow> {
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for name in &catalog.names {
        let rest = if directory.is_empty() {
            name.path.as_str()
        } else if let Some(rest) = name.path.strip_prefix(&prefix) {
            rest
        } else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        if let Some((head, _)) = rest.split_once('/') {
            push_unique(&mut dirs, head);
        } else if name.directory {
            push_unique(&mut dirs, rest);
        } else {
            files.push((rest.to_string(), name.size));
        }
    }
    files.retain(|(file, _)| !dirs.iter().any(|dir| dir == file));
    dirs.sort_by(|left, right| left.to_lowercase().cmp(&right.to_lowercase()));
    files.sort_by(|left, right| left.0.to_lowercase().cmp(&right.0.to_lowercase()));
    let mut rows = dirs.into_iter().map(LayerRow::Dir).collect::<Vec<_>>();
    rows.extend(
        files
            .into_iter()
            .map(|(name, size)| LayerRow::File(name, size)),
    );
    rows
}

pub(super) fn push_unique(dirs: &mut Vec<String>, name: &str) {
    if !dirs.iter().any(|dir| dir == name) {
        dirs.push(name.to_string());
    }
}

/// 路径是当前层名单上的直接子目录。只认已经读出的名字，文件、`..` 和更深的未列出路径都不算。
pub(super) fn listed_child_directory(state: &InspectState, path: &str) -> bool {
    let Some(catalog) = current_catalog(state) else {
        return false;
    };
    let Some(name) = direct_child(&current_full_directory(state), path) else {
        return false;
    };
    let relative = if state.archive_nests.is_empty() {
        if state.archive_directory.is_empty() {
            name.to_string()
        } else {
            format!("{}/{name}", state.archive_directory)
        }
    } else {
        let dir = state
            .archive_nests
            .last()
            .map(|nest| nest.directory.as_str())
            .unwrap_or("");
        if dir.is_empty() {
            name.to_string()
        } else {
            format!("{dir}/{name}")
        }
    };
    directory_known(catalog, &relative)
}

/// 当前名单所在的目录，路径从最外层压缩包根算起。
pub(super) fn current_full_directory(state: &InspectState) -> String {
    if let Some(nest) = state.archive_nests.last() {
        nest_location(&state.archive_nests, &nest.directory)
    } else {
        state.archive_directory.clone()
    }
}

/// `path` 是 `current` 的下一级名字时返回这个名字。
pub(super) fn direct_child<'a>(current: &str, path: &'a str) -> Option<&'a str> {
    let name = if current.is_empty() {
        path
    } else {
        path.strip_prefix(current)?.strip_prefix('/')?
    };
    (!name.is_empty() && !name.contains('/')).then_some(name)
}

pub(super) fn directory_known(catalog: &ArchiveCatalog, path: &str) -> bool {
    if path.is_empty() {
        return true;
    }
    let prefix = format!("{path}/");
    catalog
        .names
        .iter()
        .any(|name| (name.directory && name.path == path) || name.path.starts_with(&prefix))
}

/// 去掉首尾斜杠。`.`、`..` 和盘符不接受。
pub(super) fn normalize_directory(path: &str) -> Option<String> {
    let raw = path.trim().replace('\\', "/");
    if raw.starts_with('/') || raw.split('/').next().is_some_and(|part| part.contains(':')) {
        return None;
    }
    let trimmed = raw.trim_matches('/');
    if trimmed.is_empty() {
        return Some(String::new());
    }
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        parts.push(part);
    }
    Some(parts.join("/"))
}

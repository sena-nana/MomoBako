from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_SCHEMA_VERSION = 1
REPO_META_DIR = ".momo"
WINDOWS_RESERVED_NAMES = {
    "CON",
    "PRN",
    "AUX",
    "NUL",
    *(f"COM{index}" for index in range(1, 10)),
    *(f"LPT{index}" for index in range(1, 10)),
}
UNSUPPORTED_CAPABILITIES = [
    "单素材多文件夹归属",
    "quickAccess",
    "tagsGroups",
    "文件夹 password / passwordTips",
    "url",
    "原始时间字段与尺寸字段",
]


class ConversionError(RuntimeError):
    pass


@dataclass(slots=True)
class FolderNode:
    folder_id: str
    name: str
    path: str
    children: list["FolderNode"] = field(default_factory=list)


@dataclass(slots=True)
class AssetPlan:
    asset_id: str
    source_info_dir: Path
    source_file: Path
    source_thumbnail: Path | None
    target_relative_path: str
    target_relative_dir: str
    target_filename: str
    display_title: str
    extension: str
    tags: list[str]
    note: str | None
    palette: list[str]
    is_deleted: bool
    discarded_folder_names: list[str] = field(default_factory=list)
    missing_folder_ids: list[str] = field(default_factory=list)


@dataclass(slots=True)
class SmartFolderPlan:
    smart_folder_id: str
    source: str
    source_id: str
    name: str
    filter: dict[str, Any]
    sort_order: int


@dataclass(slots=True)
class SkippedSmartFolder:
    source: str
    source_id: str
    name: str
    reason: str
    conditions: Any


@dataclass(slots=True)
class ConversionPlan:
    input_root: Path
    output_root: Path
    repo_name: str
    repo_id: str
    assets: list[AssetPlan]
    folder_paths: dict[str, str]
    smart_folders: list[SmartFolderPlan]
    skipped_smart_folders: list[SkippedSmartFolder]
    warnings: list[dict[str, Any]]
    unsupported_hits: list[dict[str, Any]]
    report: dict[str, Any]


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    try:
        input_root = args.input.expanduser().resolve(strict=True)
        output_root = args.output.expanduser().resolve()
        repo_name = (args.name or output_root.name).strip()
        if not repo_name:
            raise ConversionError("资源库名称不能为空。")

        validate_paths(input_root, output_root, args.force)
        plan = build_conversion_plan(input_root, output_root, repo_name)
        print_summary(plan)

        if args.dry_run:
            print("\nDry run 完成，未写入任何文件。")
            return 0

        if not args.yes and not confirm_execution():
            print("已取消转换。")
            return 0

        execute_plan(plan, args.force)
        print(f"转换完成：{plan.output_root}")
        print(f"报告已写入：{report_output_path(plan.output_root)}")
        return 0
    except ConversionError as error:
        print(f"转换失败：{error}", file=sys.stderr)
        return 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="将 EagleLibrary 转换为 MomoBako 资源库。",
    )
    parser.add_argument("--input", type=Path, required=True, help="EagleLibrary 目录")
    parser.add_argument("--output", type=Path, required=True, help="输出的 MomoBako 目录")
    parser.add_argument("--name", type=str, help="可选的资源库名称，默认使用输出目录名")
    parser.add_argument("--dry-run", action="store_true", help="仅预览，不写文件")
    parser.add_argument("--yes", action="store_true", help="跳过确认，直接执行")
    parser.add_argument("--force", action="store_true", help="允许复用已存在的空输出目录")
    return parser


def validate_paths(input_root: Path, output_root: Path, force: bool) -> None:
    if not input_root.is_dir():
        raise ConversionError(f"输入路径不是目录：{input_root}")

    required = ["metadata.json", "images"]
    missing = [name for name in required if not (input_root / name).exists()]
    if missing:
        raise ConversionError(f"输入目录不是有效的 EagleLibrary，缺少：{', '.join(missing)}")

    try:
        input_root.relative_to(output_root)
        raise ConversionError("输出目录不能是输入目录的父级。")
    except ValueError:
        pass

    try:
        output_root.relative_to(input_root)
        raise ConversionError("输出目录不能位于输入目录内部。")
    except ValueError:
        pass

    if output_root.exists():
        if not output_root.is_dir():
            raise ConversionError(f"输出路径已存在且不是目录：{output_root}")
        if not force:
            raise ConversionError("输出目录已存在；如需复用空目录，请传入 --force。")
        if any(output_root.iterdir()):
            raise ConversionError("输出目录已存在且非空，拒绝覆盖。")


def build_conversion_plan(input_root: Path, output_root: Path, repo_name: str) -> ConversionPlan:
    library_metadata = load_json(input_root / "metadata.json")
    tags_json = load_json(input_root / "tags.json", default={"historyTags": [], "starredTags": []})
    actions_json = load_json(input_root / "actions.json", default=[])
    saved_filters_json = load_json(input_root / "saved-filters.json", default=[])
    mtime_json = load_json(input_root / "mtime.json", default={})

    repo_id = slugify_repo_id(repo_name, str(output_root))
    folder_index: dict[str, str] = {}
    folder_name_index: dict[str, str] = {}
    folder_nodes = build_folder_index(library_metadata.get("folders", []), "", folder_index, folder_name_index)

    assets: list[AssetPlan] = []
    warnings: list[dict[str, Any]] = []
    output_name_usage: dict[str, set[str]] = {}
    images_root = input_root / "images"

    for info_dir in sorted(images_root.iterdir(), key=lambda item: item.name.lower()):
        if not info_dir.is_dir() or not info_dir.name.endswith(".info"):
            continue
        metadata_path = info_dir / "metadata.json"
        if not metadata_path.is_file():
            continue
        asset_metadata = load_json(metadata_path)
        asset_plan = build_asset_plan(
            info_dir=info_dir,
            asset_metadata=asset_metadata,
            folder_index=folder_index,
            folder_name_index=folder_name_index,
            output_name_usage=output_name_usage,
            warnings=warnings,
        )
        assets.append(asset_plan)

    smart_folders, skipped_smart_folders = build_smart_folder_plans(
        repo_id=repo_id,
        library_smart_folders=library_metadata.get("smartFolders") or [],
        saved_filters=saved_filters_json,
        folder_index=folder_index,
        warnings=warnings,
    )

    unsupported_hits = collect_unsupported_hits(
        library_metadata=library_metadata,
        tags_json=tags_json,
        actions_json=actions_json,
        saved_filters_json=saved_filters_json,
        mtime_json=mtime_json,
        assets=assets,
        skipped_smart_folders=skipped_smart_folders,
    )

    report = build_report(
        input_root=input_root,
        output_root=output_root,
        repo_name=repo_name,
        repo_id=repo_id,
        folder_nodes=folder_nodes,
        assets=assets,
        smart_folders=smart_folders,
        skipped_smart_folders=skipped_smart_folders,
        warnings=warnings,
        unsupported_hits=unsupported_hits,
    )
    return ConversionPlan(
        input_root=input_root,
        output_root=output_root,
        repo_name=repo_name,
        repo_id=repo_id,
        assets=assets,
        folder_paths=folder_index,
        smart_folders=smart_folders,
        skipped_smart_folders=skipped_smart_folders,
        warnings=warnings,
        unsupported_hits=unsupported_hits,
        report=report,
    )


def load_json(path: Path, default: Any | None = None) -> Any:
    if not path.exists():
        if default is not None:
            return default
        raise ConversionError(f"缺少 JSON 文件：{path}")
    try:
        with path.open("r", encoding="utf-8") as handle:
            return json.load(handle)
    except json.JSONDecodeError as error:
        raise ConversionError(f"JSON 解析失败：{path} ({error})") from error


def build_folder_index(
    folders: list[dict[str, Any]],
    parent_path: str,
    folder_index: dict[str, str],
    folder_name_index: dict[str, str],
) -> list[FolderNode]:
    nodes: list[FolderNode] = []
    sibling_paths: set[str] = set()
    for folder in folders:
        folder_id = str(folder.get("id") or "").strip()
        if not folder_id:
            raise ConversionError("发现缺少 id 的 Eagle 文件夹。")
        folder_name = str(folder.get("name") or "").strip()
        safe_name = sanitize_segment(folder_name, fallback=f"folder-{folder_id[:8]}")
        unique_name = ensure_unique_segment(
            safe_name,
            sibling_paths,
            folder_id,
        )
        path = join_relative_path(parent_path, unique_name)
        folder_index[folder_id] = path
        folder_name_index[folder_id] = folder_name or unique_name
        child_nodes = build_folder_index(
            folder.get("children") or [],
            path,
            folder_index,
            folder_name_index,
        )
        nodes.append(FolderNode(folder_id=folder_id, name=unique_name, path=path, children=child_nodes))
    return nodes


def build_asset_plan(
    info_dir: Path,
    asset_metadata: dict[str, Any],
    folder_index: dict[str, str],
    folder_name_index: dict[str, str],
    output_name_usage: dict[str, set[str]],
    warnings: list[dict[str, Any]],
) -> AssetPlan:
    asset_id = str(asset_metadata.get("id") or "").strip()
    if not asset_id:
        raise ConversionError(f"{info_dir} 缺少素材 id。")

    source_file = select_source_file(info_dir, asset_metadata)
    source_thumbnail = select_thumbnail_file(info_dir)
    extension = source_file.suffix.lstrip(".").lower()
    folder_ids = [str(item).strip() for item in asset_metadata.get("folders") or [] if str(item).strip()]
    primary_dir = ""
    missing_folder_ids: list[str] = []
    discarded_folder_names: list[str] = []
    if folder_ids:
        first_folder_id = folder_ids[0]
        primary_dir = folder_index.get(first_folder_id, "")
        if not primary_dir:
            missing_folder_ids.append(first_folder_id)
            warnings.append(
                {
                    "type": "missingPrimaryFolder",
                    "assetId": asset_id,
                    "folderId": first_folder_id,
                    "fallbackTarget": "repo-root",
                }
            )
        for folder_id in folder_ids[1:]:
            name = folder_name_index.get(folder_id, folder_id)
            discarded_folder_names.append(name)
            if folder_id not in folder_index:
                missing_folder_ids.append(folder_id)

    filename = sanitize_filename(source_file.name, fallback=f"asset-{asset_id[:8]}.{extension or 'bin'}")
    directory_usage = output_name_usage.setdefault(primary_dir, set())
    filename = ensure_unique_filename(filename, directory_usage, asset_id)
    target_relative_path = join_relative_path(primary_dir, filename)

    if discarded_folder_names:
        warnings.append(
            {
                "type": "multiFolderAsset",
                "assetId": asset_id,
                "keptFolder": primary_dir or "repo-root",
                "discardedFolders": discarded_folder_names,
            }
        )
    note = str(asset_metadata.get("annotation") or "").strip() or None
    tags = [str(tag).strip() for tag in asset_metadata.get("tags") or [] if str(tag).strip()]
    palette = normalize_eagle_palette(asset_metadata.get("palettes"))
    is_deleted = asset_metadata.get("isDeleted") is True
    if source_thumbnail is None:
        warnings.append(
            {
                "type": "missingThumbnail",
                "assetId": asset_id,
                "sourceInfoDir": str(info_dir),
            }
        )
    return AssetPlan(
        asset_id=asset_id,
        source_info_dir=info_dir,
        source_file=source_file,
        source_thumbnail=source_thumbnail,
        target_relative_path=target_relative_path,
        target_relative_dir=primary_dir,
        target_filename=filename,
        display_title=str(asset_metadata.get("name") or source_file.stem).strip() or source_file.stem,
        extension=extension,
        tags=dedupe_preserve_order(tags),
        note=note,
        palette=palette,
        is_deleted=is_deleted,
        discarded_folder_names=discarded_folder_names,
        missing_folder_ids=missing_folder_ids,
    )


def normalize_eagle_palette(value: Any) -> list[str]:
    if not isinstance(value, list):
        return []

    colors: list[str] = []
    for item in value:
        color_value: Any
        if isinstance(item, dict):
            color_value = item.get("color")
        else:
            color_value = item
        color = normalize_hex_color(color_value)
        if color and color not in colors:
            colors.append(color)
        if len(colors) == 5:
            break
    return colors


def normalize_hex_color(value: Any) -> str | None:
    if not isinstance(value, str):
        return None

    color = value.strip()
    if not color:
        return None
    if color.startswith("#"):
        color = color[1:]
    if len(color) == 3 and re.fullmatch(r"[0-9a-fA-F]{3}", color):
        color = "".join(character * 2 for character in color)
    if not re.fullmatch(r"[0-9a-fA-F]{6}", color):
        return None
    return f"#{color.upper()}"


def select_source_file(info_dir: Path, asset_metadata: dict[str, Any]) -> Path:
    logical_name = str(asset_metadata.get("name") or "").strip()
    logical_ext = str(asset_metadata.get("ext") or "").strip()
    candidates = [
        item
        for item in info_dir.iterdir()
        if item.is_file()
        and item.name != "metadata.json"
        and not is_thumbnail_candidate(item)
    ]
    if not candidates:
        raise ConversionError(f"{info_dir} 未找到可转换的原始文件。")

    expected_name = f"{logical_name}.{logical_ext}" if logical_name and logical_ext else ""
    exact_matches = [item for item in candidates if item.name == expected_name]
    if len(exact_matches) == 1:
        return exact_matches[0]
    if len(candidates) == 1:
        return candidates[0]

    by_size = sorted(candidates, key=lambda item: item.stat().st_size, reverse=True)
    if len(by_size) >= 2 and by_size[0].stat().st_size > by_size[1].stat().st_size:
        return by_size[0]
    names = ", ".join(item.name for item in candidates)
    raise ConversionError(f"{info_dir} 原始文件存在歧义，候选：{names}")


def is_thumbnail_candidate(path: Path) -> bool:
    return path.stem.endswith("_thumbnail")


def select_thumbnail_file(info_dir: Path) -> Path | None:
    thumbnails = [item for item in info_dir.iterdir() if item.is_file() and is_thumbnail_candidate(item)]
    if not thumbnails:
        return None
    thumbnails.sort(key=lambda item: item.name.lower())
    return thumbnails[0]


def build_smart_folder_plans(
    repo_id: str,
    library_smart_folders: list[Any],
    saved_filters: list[Any],
    folder_index: dict[str, str],
    warnings: list[dict[str, Any]],
) -> tuple[list[SmartFolderPlan], list[SkippedSmartFolder]]:
    plans: list[SmartFolderPlan] = []
    skipped: list[SkippedSmartFolder] = []
    used_ids: set[str] = set()
    sources = [
        ("smartFolders", library_smart_folders),
        ("saved-filters", saved_filters),
    ]
    for source, entries in sources:
        if not isinstance(entries, list):
            record_skipped_smart_folder(
                skipped,
                warnings,
                source=source,
                source_id=source,
                name=source,
                reason="source is not a list",
                conditions=entries,
            )
            continue
        for entry in entries:
            if not isinstance(entry, dict):
                record_skipped_smart_folder(
                    skipped,
                    warnings,
                    source=source,
                    source_id=source,
                    name=source,
                    reason="entry is not an object",
                    conditions=entry,
                )
                continue

            source_id = smart_folder_source_id(entry, source, len(plans) + len(skipped))
            name = smart_folder_name(entry, source_id)
            filter_result = convert_eagle_smart_folder_filter(entry, folder_index)
            if filter_result["reason"]:
                record_skipped_smart_folder(
                    skipped,
                    warnings,
                    source=source,
                    source_id=source_id,
                    name=name,
                    reason=filter_result["reason"],
                    conditions=filter_result["conditions"],
                )
                continue

            smart_folder_id = unique_smart_folder_id(repo_id, source, source_id, name, used_ids)
            plans.append(
                SmartFolderPlan(
                    smart_folder_id=smart_folder_id,
                    source=source,
                    source_id=source_id,
                    name=name,
                    filter=filter_result["filter"],
                    sort_order=len(plans),
                )
            )
    return plans, skipped


def record_skipped_smart_folder(
    skipped: list[SkippedSmartFolder],
    warnings: list[dict[str, Any]],
    *,
    source: str,
    source_id: str,
    name: str,
    reason: str,
    conditions: Any,
) -> None:
    skipped_item = SkippedSmartFolder(
        source=source,
        source_id=source_id,
        name=name,
        reason=reason,
        conditions=summarize_conditions(conditions),
    )
    skipped.append(skipped_item)
    warnings.append(
        {
            "type": "smartFolderSkipped",
            "source": skipped_item.source,
            "sourceId": skipped_item.source_id,
            "name": skipped_item.name,
            "reason": skipped_item.reason,
        }
    )


def smart_folder_source_id(entry: dict[str, Any], source: str, index: int) -> str:
    for key in ("id", "uuid", "smartFolderId", "filterId"):
        value = str(entry.get(key) or "").strip()
        if value:
            return value
    return f"{source}-{index + 1}"


def smart_folder_name(entry: dict[str, Any], source_id: str) -> str:
    for key in ("name", "title", "label"):
        value = str(entry.get(key) or "").strip()
        if value:
            return sanitize_segment(value, fallback=f"Smart Folder {source_id[:8]}")
    return f"Smart Folder {source_id[:8]}"


def convert_eagle_smart_folder_filter(entry: dict[str, Any], folder_index: dict[str, str]) -> dict[str, Any]:
    filter_root = entry.get("filter") if isinstance(entry.get("filter"), dict) else entry
    if contains_or_semantics(filter_root):
        return skipped_filter_result("OR semantics are not equivalent in MomoBako", filter_root)
    if contains_negative_semantics(filter_root):
        return skipped_filter_result("negative conditions are not supported", filter_root)
    if contains_unsupported_smart_folder_keys(filter_root):
        return skipped_filter_result("unsupported filter fields are present", filter_root)

    conditions = extract_smart_folder_conditions(filter_root)
    if conditions is None:
        return skipped_filter_result("filter conditions are not a supported shape", filter_root)

    query_values: list[str] = []
    tag_values: list[str] = []
    format_values: list[str] = []
    path_values: list[str] = []
    metadata_filters: list[dict[str, str]] = []
    min_rating: float | None = None

    direct_result = apply_direct_smart_folder_fields(
        filter_root,
        folder_index,
        query_values,
        tag_values,
        format_values,
        path_values,
        metadata_filters,
    )
    if isinstance(direct_result, str):
        return skipped_filter_result(direct_result, filter_root)

    for condition in conditions:
        if not isinstance(condition, dict):
            return skipped_filter_result("filter condition is not an object", conditions)
        condition_result = apply_smart_folder_condition(
            condition,
            folder_index,
            query_values,
            tag_values,
            format_values,
            path_values,
            metadata_filters,
        )
        if isinstance(condition_result, str):
            return skipped_filter_result(condition_result, conditions)
        rating_value = rating_from_condition(condition)
        if isinstance(rating_value, str):
            return skipped_filter_result(rating_value, conditions)
        if rating_value is not None:
            min_rating = max(min_rating or 0, rating_value)

    direct_rating = direct_min_rating(filter_root)
    if isinstance(direct_rating, str):
        return skipped_filter_result(direct_rating, filter_root)
    if direct_rating is not None:
        min_rating = max(min_rating or 0, direct_rating)

    filter_value: dict[str, Any] = {}
    if query_values:
        filter_value["query"] = "\n".join(dedupe_preserve_order(query_values))
    if path_values:
        filter_value["pathPrefix"] = "\n".join(dedupe_preserve_order(path_values))
    if tag_values:
        filter_value["tags"] = dedupe_preserve_order(tag_values)
    if format_values:
        filter_value["formats"] = dedupe_preserve_order(format_values)
    if metadata_filters:
        filter_value["metadataFilters"] = dedupe_metadata_filters(metadata_filters)
    if min_rating is not None:
        filter_value["minRating"] = min_rating

    if not filter_value:
        return skipped_filter_result("no equivalent MomoBako smart folder filter fields", filter_root)
    return {"filter": filter_value, "reason": "", "conditions": conditions or filter_root}


def skipped_filter_result(reason: str, conditions: Any) -> dict[str, Any]:
    return {"filter": {}, "reason": reason, "conditions": conditions}


def extract_smart_folder_conditions(filter_root: Any) -> list[Any] | None:
    if not isinstance(filter_root, dict):
        return None
    for key in ("conditions", "rules", "filters"):
        value = filter_root.get(key)
        if value is None:
            continue
        if isinstance(value, list):
            return value
        return None
    return []


def apply_direct_smart_folder_fields(
    filter_root: dict[str, Any],
    folder_index: dict[str, str],
    query_values: list[str],
    tag_values: list[str],
    format_values: list[str],
    path_values: list[str],
    metadata_filters: list[dict[str, str]],
) -> str | None:
    direct_mappings = [
        ("query", query_values, normalize_text_values),
        ("keyword", query_values, normalize_text_values),
        ("keywords", query_values, normalize_text_values),
        ("text", query_values, normalize_text_values),
        ("tags", tag_values, normalize_text_values),
        ("tag", tag_values, normalize_text_values),
        ("formats", format_values, normalize_format_values),
        ("format", format_values, normalize_format_values),
        ("ext", format_values, normalize_format_values),
        ("extensions", format_values, normalize_format_values),
    ]
    for key, target, normalizer in direct_mappings:
        target.extend(normalizer(filter_root.get(key)))

    for key in ("folderId", "folderIds", "folders"):
        folder_values = normalize_text_values(filter_root.get(key))
        for folder_id in folder_values:
            path = folder_index.get(folder_id)
            if not path:
                return f"unknown folder id: {folder_id}"
            path_values.append(path)

    for key in ("pathPrefix", "path", "folderPath"):
        path_values.extend(normalize_path_values(filter_root.get(key)))

    color_values = normalize_text_values(filter_root.get("colors")) + normalize_text_values(filter_root.get("color"))
    shape_values = normalize_text_values(filter_root.get("shapes")) + normalize_text_values(filter_root.get("shape"))
    metadata_filters.extend({"key": "color", "value": value} for value in color_values)
    metadata_filters.extend({"key": "shape", "value": value} for value in shape_values)
    return None


def apply_smart_folder_condition(
    condition: dict[str, Any],
    folder_index: dict[str, str],
    query_values: list[str],
    tag_values: list[str],
    format_values: list[str],
    path_values: list[str],
    metadata_filters: list[dict[str, str]],
) -> str | None:
    if contains_or_semantics(condition):
        return "OR semantics are not equivalent in MomoBako"
    if contains_negative_semantics(condition):
        return "negative conditions are not supported"
    operator = condition_operator(condition)
    if operator and operator not in {"is", "eq", "equals", "contains", "include", "includes", "in", "gte", "min", "atleast"}:
        return f"unsupported operator: {operator}"

    field_name = condition_field(condition)
    values = condition_values(condition)
    if not field_name or not values:
        return None

    if field_name in {"keyword", "keywords", "query", "text", "name", "filename"}:
        query_values.extend(normalize_text_values(values))
        return None
    if field_name in {"tag", "tags"}:
        tag_values.extend(normalize_text_values(values))
        return None
    if field_name in {"format", "formats", "ext", "extension", "extensions", "filetype", "filetypes"}:
        format_values.extend(normalize_format_values(values))
        return None
    if field_name in {"folder", "folders", "folderid", "folderids"}:
        for folder_id in normalize_text_values(values):
            path = folder_index.get(folder_id)
            if not path:
                return f"unknown folder id: {folder_id}"
            path_values.append(path)
        return None
    if field_name in {"path", "pathprefix", "folderpath"}:
        path_values.extend(normalize_path_values(values))
        return None
    if field_name in {"color", "colors"}:
        metadata_filters.extend({"key": "color", "value": value} for value in normalize_text_values(values))
        return None
    if field_name in {"shape", "shapes"}:
        metadata_filters.extend({"key": "shape", "value": value} for value in normalize_text_values(values))
        return None
    if field_name in {"rating", "score"}:
        return None
    return f"unsupported field: {field_name}"


def condition_field(condition: dict[str, Any]) -> str:
    for key in ("field", "key", "type", "property", "name"):
        value = str(condition.get(key) or "").strip().lower()
        if value:
            return re.sub(r"[^a-z0-9]", "", value)
    return ""


def condition_values(condition: dict[str, Any]) -> Any:
    for key in ("value", "values", "keyword", "keywords", "text", "folderId", "folderIds"):
        if key in condition:
            return condition[key]
    return None


def condition_operator(condition: dict[str, Any]) -> str:
    for key in ("operator", "op", "match", "matcher"):
        value = str(condition.get(key) or "").strip().lower()
        if value:
            return re.sub(r"[^a-z0-9]", "", value)
    return ""


def rating_from_condition(condition: dict[str, Any]) -> float | str | None:
    if condition_field(condition) not in {"rating", "score"}:
        return None
    operator = condition_operator(condition)
    if operator and operator not in {"gte", "min", "atleast"}:
        return f"unsupported rating operator: {operator}"
    value = first_scalar_value(condition_values(condition))
    try:
        rating = float(value)
    except (TypeError, ValueError):
        return "rating value is not numeric"
    return rating if rating > 0 else None


def direct_min_rating(filter_root: dict[str, Any]) -> float | str | None:
    for key in ("minRating", "rating"):
        if key not in filter_root:
            continue
        value = first_scalar_value(filter_root.get(key))
        try:
            rating = float(value)
        except (TypeError, ValueError):
            return "rating value is not numeric"
        return rating if rating > 0 else None
    return None


def contains_or_semantics(value: Any) -> bool:
    if isinstance(value, dict):
        for key, item in value.items():
            normalized_key = str(key).strip().lower()
            if normalized_key in {"match", "logic", "operator", "join", "combinator"}:
                normalized_value = str(item).strip().lower()
                if normalized_value in {"or", "any", "some"}:
                    return True
            if contains_or_semantics(item):
                return True
    elif isinstance(value, list):
        return any(contains_or_semantics(item) for item in value)
    return False


def contains_negative_semantics(value: Any) -> bool:
    if isinstance(value, dict):
        for key, item in value.items():
            normalized_key = str(key).strip().lower()
            normalized_value = str(item).strip().lower()
            if normalized_key in {"exclude", "excluded", "negative", "not", "isnot"} and item:
                return True
            if normalized_key in {"operator", "op", "match", "matcher"} and normalized_value in {
                "not",
                "notcontains",
                "doesnotcontain",
                "ne",
                "neq",
                "notequals",
                "exclude",
            }:
                return True
            if contains_negative_semantics(item):
                return True
    elif isinstance(value, list):
        return any(contains_negative_semantics(item) for item in value)
    return False


def contains_unsupported_smart_folder_keys(value: Any) -> bool:
    unsupported_keys = {
        "sort",
        "sortby",
        "order",
        "orderby",
        "limit",
        "createdat",
        "created",
        "updatedat",
        "updated",
        "modifiedat",
        "mtime",
        "width",
        "height",
        "size",
        "filesize",
        "ratio",
        "aspectratio",
    }
    if isinstance(value, dict):
        for key, item in value.items():
            normalized_key = re.sub(r"[^a-z0-9]", "", str(key).lower())
            if normalized_key in unsupported_keys:
                return True
            if normalized_key in {"field", "key", "type", "property", "name"}:
                normalized_value = re.sub(r"[^a-z0-9]", "", str(item).lower())
                if normalized_value in unsupported_keys:
                    return True
            if contains_unsupported_smart_folder_keys(item):
                return True
    elif isinstance(value, list):
        return any(contains_unsupported_smart_folder_keys(item) for item in value)
    return False


def normalize_text_values(value: Any) -> list[str]:
    values = flatten_scalar_values(value)
    return [item for item in (str(value).strip() for value in values) if item]


def normalize_format_values(value: Any) -> list[str]:
    formats = []
    for item in normalize_text_values(value):
        normalized = item.lower().lstrip(".")
        if normalized:
            formats.append(normalized)
    return formats


def normalize_path_values(value: Any) -> list[str]:
    paths = []
    for item in normalize_text_values(value):
        normalized = item.replace("\\", "/").strip("/")
        if normalized:
            paths.append(normalized)
    return paths


def flatten_scalar_values(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        result: list[Any] = []
        for item in value:
            result.extend(flatten_scalar_values(item))
        return result
    if isinstance(value, dict):
        for key in ("id", "name", "value", "text"):
            if key in value:
                return flatten_scalar_values(value[key])
        return []
    return [value]


def first_scalar_value(value: Any) -> Any:
    values = flatten_scalar_values(value)
    return values[0] if values else None


def dedupe_metadata_filters(filters: list[dict[str, str]]) -> list[dict[str, str]]:
    seen: set[tuple[str, str]] = set()
    result: list[dict[str, str]] = []
    for item in filters:
        key = item["key"].strip()
        value = item["value"].strip()
        marker = (key, value)
        if not key or not value or marker in seen:
            continue
        seen.add(marker)
        result.append({"key": key, "value": value})
    return result


def unique_smart_folder_id(repo_id: str, source: str, source_id: str, name: str, used_ids: set[str]) -> str:
    base = slugify_ascii_component(f"smart-{repo_id}-{source}-{source_id}-{name}")
    candidate = base
    index = 2
    while candidate in used_ids:
        candidate = f"{base}-{index}"
        index += 1
    used_ids.add(candidate)
    return candidate


def summarize_conditions(value: Any) -> Any:
    text = json.dumps(value, ensure_ascii=False, sort_keys=True, default=str)
    if len(text) > 1000:
        return text[:997] + "..."
    return value


def collect_unsupported_hits(
    library_metadata: dict[str, Any],
    tags_json: dict[str, Any],
    actions_json: list[Any],
    saved_filters_json: list[Any],
    mtime_json: dict[str, Any],
    assets: list[AssetPlan],
    skipped_smart_folders: list[SkippedSmartFolder],
) -> list[dict[str, Any]]:
    hits: list[dict[str, Any]] = []
    if library_metadata.get("quickAccess"):
        hits.append({"capability": "quickAccess", "count": len(library_metadata["quickAccess"])})
    if library_metadata.get("tagsGroups"):
        hits.append({"capability": "tagsGroups", "count": len(library_metadata["tagsGroups"])})
    password_folders = [
        str(folder.get("name") or folder.get("id") or "")
        for folder in walk_folders(library_metadata.get("folders") or [])
        if str(folder.get("password") or "").strip() or str(folder.get("passwordTips") or "").strip()
    ]
    if password_folders:
        hits.append({"capability": "文件夹 password / passwordTips", "folders": password_folders})
    if actions_json:
        hits.append({"capability": "actions", "count": len(actions_json)})
    if skipped_smart_folders:
        hits.append(
            {
                "capability": "smartFolders/saved-filters",
                "count": len(skipped_smart_folders),
                "items": [serialize_skipped_smart_folder(item) for item in skipped_smart_folders],
            }
        )
    if mtime_json:
        hits.append({"capability": "mtime", "count": len(mtime_json)})
    multi_folder_assets = [
        {"assetId": asset.asset_id, "discardedFolders": asset.discarded_folder_names}
        for asset in assets
        if asset.discarded_folder_names
    ]
    if multi_folder_assets:
        hits.append({"capability": "单素材多文件夹归属", "assets": multi_folder_assets})
    return hits


def walk_folders(folders: list[dict[str, Any]]) -> list[dict[str, Any]]:
    flattened: list[dict[str, Any]] = []
    for folder in folders:
        flattened.append(folder)
        flattened.extend(walk_folders(folder.get("children") or []))
    return flattened


def build_report(
    input_root: Path,
    output_root: Path,
    repo_name: str,
    repo_id: str,
    folder_nodes: list[FolderNode],
    assets: list[AssetPlan],
    smart_folders: list[SmartFolderPlan],
    skipped_smart_folders: list[SkippedSmartFolder],
    warnings: list[dict[str, Any]],
    unsupported_hits: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "inputRoot": str(input_root),
        "outputRoot": str(output_root),
        "repoName": repo_name,
        "repoId": repo_id,
        "summary": {
            "assetCount": len(assets),
            "deletedAssetCount": sum(1 for asset in assets if asset.is_deleted),
            "folderCount": count_folder_nodes(folder_nodes),
            "smartFolderCount": len(smart_folders),
            "skippedSmartFolderCount": len(skipped_smart_folders),
            "thumbnailCount": sum(1 for asset in assets if asset.source_thumbnail is not None),
            "warningCount": len(warnings),
            "unsupportedCapabilityCount": len(unsupported_hits),
        },
        "folders": [serialize_folder_node(node) for node in folder_nodes],
        "assets": [
            {
                "assetId": asset.asset_id,
                "sourceInfoDir": str(asset.source_info_dir),
                "sourceFile": str(asset.source_file),
                "sourceThumbnail": str(asset.source_thumbnail) if asset.source_thumbnail else None,
                "targetRelativePath": asset.target_relative_path,
                "isDeleted": asset.is_deleted,
                "status": asset_status(asset),
                "trashRelativePath": asset_trash_relative_path(asset),
                "title": asset.display_title,
                "tags": asset.tags,
                "hasNote": asset.note is not None,
                "discardedFolders": asset.discarded_folder_names,
                "missingFolderIds": asset.missing_folder_ids,
            }
            for asset in assets
        ],
        "smartFolders": [serialize_smart_folder_plan(item) for item in smart_folders],
        "skippedSmartFolders": [serialize_skipped_smart_folder(item) for item in skipped_smart_folders],
        "warnings": warnings,
        "unsupportedCapabilities": unsupported_hits,
    }


def count_folder_nodes(nodes: list[FolderNode]) -> int:
    return sum(1 + count_folder_nodes(node.children) for node in nodes)


def serialize_folder_node(node: FolderNode) -> dict[str, Any]:
    return {
        "folderId": node.folder_id,
        "name": node.name,
        "path": node.path,
        "children": [serialize_folder_node(child) for child in node.children],
    }


def serialize_smart_folder_plan(plan: SmartFolderPlan) -> dict[str, Any]:
    return {
        "smartFolderId": plan.smart_folder_id,
        "source": plan.source,
        "sourceId": plan.source_id,
        "name": plan.name,
        "filter": plan.filter,
        "sortOrder": plan.sort_order,
    }


def serialize_skipped_smart_folder(skipped: SkippedSmartFolder) -> dict[str, Any]:
    return {
        "source": skipped.source,
        "sourceId": skipped.source_id,
        "name": skipped.name,
        "reason": skipped.reason,
        "conditions": skipped.conditions,
    }


def print_summary(plan: ConversionPlan) -> None:
    print(f"输入资源库: {plan.input_root}")
    print(f"输出目录: {plan.output_root}")
    print(f"资源库名称: {plan.repo_name}")
    print(f"repoId: {plan.repo_id}")
    print(f"素材数量: {len(plan.assets)}")
    print(f"文件夹数量: {len(plan.folder_paths)}")
    print(f"智能文件夹数量: {len(plan.smart_folders)}")
    print(f"跳过智能文件夹数量: {len(plan.skipped_smart_folders)}")
    print(f"缩略图数量: {sum(1 for asset in plan.assets if asset.source_thumbnail is not None)}")
    print(f"警告数量: {len(plan.warnings)}")
    print(f"能力缺口命中: {len(plan.unsupported_hits)}")
    if plan.warnings:
        print("\n警告预览:")
        for warning in plan.warnings[:10]:
            print(f"- {json.dumps(warning, ensure_ascii=False)}")
        if len(plan.warnings) > 10:
            print(f"- 其余 {len(plan.warnings) - 10} 条请查看 import-report.json")
    if plan.unsupported_hits:
        print("\n无法完整转换的能力:")
        for hit in plan.unsupported_hits:
            print(f"- {json.dumps(hit, ensure_ascii=False)}")


def confirm_execution() -> bool:
    answer = input("\n确认执行移动并写入新仓库？[y/N]: ").strip().lower()
    return answer in {"y", "yes"}


def execute_plan(plan: ConversionPlan, force: bool) -> None:
    create_output_root(plan.output_root, force)
    ensure_repo_layout(plan.output_root)
    write_todo_file(todo_file_path())
    write_repository_metadata(plan)
    move_assets(plan)
    write_trash_manifest(plan)
    write_database(plan)
    report_path = report_output_path(plan.output_root)
    report_path.write_text(json.dumps(plan.report, ensure_ascii=False, indent=2), encoding="utf-8")


def create_output_root(output_root: Path, force: bool) -> None:
    if output_root.exists():
        if not output_root.is_dir():
            raise ConversionError(f"输出路径已存在且不是目录：{output_root}")
        if any(output_root.iterdir()):
            raise ConversionError("输出目录已存在且非空，拒绝写入。")
        if not force:
            raise ConversionError("输出目录已存在，请传入 --force 复用空目录。")
        return
    output_root.mkdir(parents=True, exist_ok=False)


def ensure_repo_layout(output_root: Path) -> None:
    meta_root = output_root / REPO_META_DIR
    for relative in [
        "",
        "cache",
        "thumbnails",
        "logs",
        "indexes",
        "trash",
    ]:
        (meta_root / relative).mkdir(parents=True, exist_ok=True)
    if os.name == "nt":
        subprocess.run(["attrib", "+H", str(meta_root)], check=False, capture_output=True)


def write_repository_metadata(plan: ConversionPlan) -> None:
    metadata = {
        "repoId": plan.repo_id,
        "name": plan.repo_name,
        "rootPath": str(plan.output_root),
        "backendPluginId": "momobako.local-filesystem",
        "backendConfig": {},
        "createdAt": now_rfc3339(),
        "schemaVersion": REPO_SCHEMA_VERSION,
    }
    path = plan.output_root / REPO_META_DIR / "repository.json"
    path.write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding="utf-8")


def write_database(plan: ConversionPlan) -> None:
    database_path = plan.output_root / REPO_META_DIR / "metadata.db"
    connection = sqlite3.connect(database_path)
    try:
        connection.execute("PRAGMA journal_mode=WAL;")
        connection.execute("PRAGMA foreign_keys=ON;")
        connection.executescript(repository_schema_sql())
        write_repository_record(connection, plan)
        for asset in plan.assets:
            write_asset_record(connection, plan, asset)
        for smart_folder in plan.smart_folders:
            write_smart_folder_record(connection, plan, smart_folder)
        connection.commit()
    finally:
        connection.close()


def repository_schema_sql() -> str:
    return """
CREATE TABLE IF NOT EXISTS repositories (
  repo_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL UNIQUE,
  schema_version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS assets (
  asset_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  path TEXT NOT NULL,
  filename TEXT NOT NULL,
  extension TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  modified_at TEXT NOT NULL,
  hash TEXT,
  status TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL,
  thumbnail_path TEXT,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_assets_repo_path ON assets(repo_id, path);
CREATE INDEX IF NOT EXISTS idx_assets_repo_filename ON assets(repo_id, filename);
CREATE INDEX IF NOT EXISTS idx_assets_repo_status ON assets(repo_id, status);
CREATE INDEX IF NOT EXISTS idx_assets_repo_hash ON assets(repo_id, hash);

CREATE TABLE IF NOT EXISTS hardlink_groups (
  group_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_hardlink_groups_repo_hash_size
ON hardlink_groups(repo_id, content_hash, size_bytes);

CREATE TABLE IF NOT EXISTS hardlink_members (
  group_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  path TEXT NOT NULL,
  link_state TEXT NOT NULL,
  linked_at TEXT NOT NULL,
  verified_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, asset_id),
  FOREIGN KEY(group_id) REFERENCES hardlink_groups(group_id),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_hardlink_members_repo_path
ON hardlink_members(repo_id, path);

CREATE TABLE IF NOT EXISTS hardlink_candidates (
  candidate_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  new_asset_id TEXT NOT NULL,
  new_path TEXT NOT NULL,
  existing_asset_id TEXT NOT NULL,
  existing_path TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_hardlink_candidates_unique
ON hardlink_candidates(repo_id, new_asset_id, existing_asset_id);

CREATE TABLE IF NOT EXISTS entry_thumbnails (
  repo_id TEXT NOT NULL,
  path TEXT NOT NULL,
  kind TEXT NOT NULL,
  thumbnail_path TEXT NOT NULL,
  custom INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, path, kind)
);

CREATE TABLE IF NOT EXISTS smart_folders (
  smart_folder_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  parent_id TEXT,
  name TEXT NOT NULL,
  filter_json TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id),
  FOREIGN KEY(parent_id) REFERENCES smart_folders(smart_folder_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_smart_folders_repo_parent
ON smart_folders(repo_id, parent_id, sort_order, name);

CREATE TABLE IF NOT EXISTS metadata (
  asset_id TEXT NOT NULL,
  key TEXT NOT NULL,
  value_type TEXT NOT NULL,
  value_json TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(asset_id, key),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_metadata_key ON metadata(key);

CREATE TABLE IF NOT EXISTS tags (
  asset_id TEXT NOT NULL,
  tag TEXT NOT NULL,
  normalized_tag TEXT NOT NULL,
  PRIMARY KEY(asset_id, normalized_tag),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(normalized_tag);

CREATE TABLE IF NOT EXISTS revisions (
  revision_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  operation TEXT NOT NULL,
  before_json TEXT,
  after_json TEXT,
  source TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_revisions_asset_time ON revisions(asset_id, timestamp DESC);

CREATE TABLE IF NOT EXISTS events (
  event_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  asset_id TEXT,
  event_type TEXT NOT NULL,
  path TEXT NOT NULL,
  payload_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_events_repo_time ON events(repo_id, created_at DESC);

CREATE TABLE IF NOT EXISTS schema_version (
  component TEXT PRIMARY KEY,
  version INTEGER NOT NULL
);
"""


def write_repository_record(connection: sqlite3.Connection, plan: ConversionPlan) -> None:
    timestamp = now_rfc3339()
    connection.execute(
        """
        INSERT OR REPLACE INTO repositories (repo_id, name, root_path, schema_version, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        (plan.repo_id, plan.repo_name, str(plan.output_root), REPO_SCHEMA_VERSION, timestamp, timestamp),
    )
    connection.execute(
        """
        INSERT INTO schema_version(component, version)
        VALUES ('repository', ?)
        ON CONFLICT(component) DO UPDATE SET version = excluded.version
        """,
        (REPO_SCHEMA_VERSION,),
    )


def write_asset_record(connection: sqlite3.Connection, plan: ConversionPlan, asset: AssetPlan) -> None:
    target_path = asset_file_output_path(plan, asset)
    if not target_path.is_file():
        return
    stat = target_path.stat()
    modified_at = rfc3339_from_timestamp(stat.st_mtime)
    created_at = now_rfc3339()
    asset_id = asset_id_for_path(plan.repo_id, asset.target_relative_path)
    file_hash = file_sha256_hash(target_path)
    thumbnail_path = None
    if asset.source_thumbnail is not None:
        thumbnail_path = build_thumbnail_target_path(plan, asset)
    connection.execute(
        """
        INSERT OR REPLACE INTO assets (
          asset_id, repo_id, path, filename, extension, size_bytes,
          created_at, modified_at, hash, status, version, updated_at, thumbnail_path
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
        """,
        (
            asset_id,
            plan.repo_id,
            asset.target_relative_path,
            asset.target_filename,
            asset.extension,
            stat.st_size,
            created_at,
            modified_at,
            file_hash,
            asset_status(asset),
            created_at,
            str(thumbnail_path) if thumbnail_path else None,
        ),
    )

    metadata_entries: dict[str, Any] = {
        "title": asset.display_title,
        "type": asset.extension,
        "favorite": False,
    }
    if asset.note is not None:
        metadata_entries["note"] = asset.note
    if asset.palette:
        metadata_entries["color"] = asset.palette[0]
        metadata_entries["palette"] = asset.palette
    for key, value in metadata_entries.items():
        connection.execute(
            """
            INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
            VALUES (?, ?, ?, ?, 1, ?)
            """,
            (asset_id, key, infer_value_type(value), json.dumps(value, ensure_ascii=False), created_at),
        )

    for tag in dedupe_preserve_order(asset.tags):
        connection.execute(
            """
            INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag)
            VALUES (?, ?, ?)
            """,
            (asset_id, tag, tag.lower()),
        )

    connection.execute(
        """
        INSERT OR REPLACE INTO revisions (
          revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
        )
        VALUES (?, ?, ?, ?, 'metadata.seeded', ?, ?, 'eagle-importer')
        """,
        (
            f"rev-{asset_id}",
            plan.repo_id,
            asset_id,
            modified_at,
            "{}",
            json.dumps(metadata_entries, ensure_ascii=False),
        ),
    )
    connection.execute(
        """
        INSERT OR REPLACE INTO events (
          event_id, repo_id, asset_id, event_type, path, payload_json, created_at
        )
        VALUES (?, ?, ?, 'asset.discovered', ?, ?, ?)
        """,
        (
            f"evt-{asset_id}",
            plan.repo_id,
            asset_id,
            asset.target_relative_path,
            json.dumps({"origin": "eagle-importer"}, ensure_ascii=False),
            modified_at,
        ),
    )


def write_smart_folder_record(
    connection: sqlite3.Connection,
    plan: ConversionPlan,
    smart_folder: SmartFolderPlan,
) -> None:
    timestamp = now_rfc3339()
    connection.execute(
        """
        INSERT OR REPLACE INTO smart_folders (
          smart_folder_id, repo_id, parent_id, name, filter_json,
          sort_order, created_at, updated_at
        )
        VALUES (?, ?, NULL, ?, ?, ?, ?, ?)
        """,
        (
            smart_folder.smart_folder_id,
            plan.repo_id,
            smart_folder.name,
            json.dumps(smart_folder.filter, ensure_ascii=False, sort_keys=True),
            smart_folder.sort_order,
            timestamp,
            timestamp,
        ),
    )


def move_assets(plan: ConversionPlan) -> None:
    for asset in plan.assets:
        target_file = asset_file_output_path(plan, asset)
        target_file.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(asset.source_file), str(target_file))
        if asset.source_thumbnail is not None:
            thumbnail_target = build_thumbnail_target_path(plan, asset)
            thumbnail_target.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(asset.source_thumbnail), str(thumbnail_target))
        if asset.source_info_dir.exists():
            shutil.rmtree(asset.source_info_dir)


def asset_file_output_path(plan: ConversionPlan, asset: AssetPlan) -> Path:
    trash_path = asset_trash_relative_path(asset)
    if trash_path is not None:
        return plan.output_root / REPO_META_DIR / "trash" / trash_path
    return plan.output_root / asset.target_relative_path


def asset_status(asset: AssetPlan) -> str:
    return "deleted" if asset.is_deleted else "synced"


def asset_trash_relative_path(asset: AssetPlan) -> str | None:
    return asset.target_relative_path if asset.is_deleted else None


def write_trash_manifest(plan: ConversionPlan) -> None:
    deleted_assets = [asset for asset in plan.assets if asset.is_deleted]
    if not deleted_assets:
        return

    timestamp = now_rfc3339()
    manifest = {
        "entries": [
            {
                "originalPath": asset.target_relative_path,
                "trashPath": asset_trash_relative_path(asset),
                "deletedAt": timestamp,
                "kind": "file",
            }
            for asset in deleted_assets
        ]
    }
    manifest_path = plan.output_root / REPO_META_DIR / "trash.json"
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")


def build_thumbnail_target_path(plan: ConversionPlan, asset: AssetPlan) -> Path:
    thumbnail_root = plan.output_root / REPO_META_DIR / "thumbnails"
    repo_dir = thumbnail_repository_dir_name(plan.repo_id, str(plan.output_root))
    target_name = thumbnail_file_name(plan.repo_id, str(plan.output_root), asset.target_relative_path, "file", "eagle")
    source_extension = asset.source_thumbnail.suffix.lower() if asset.source_thumbnail else ".jpg"
    if source_extension:
        target_name = f"{Path(target_name).stem}{source_extension}"
    return thumbnail_root / repo_dir / target_name


def write_todo_file(path: Path) -> None:
    lines = [
        "# Eagle -> MomoBako Todo",
        "",
        "以下能力当前无法完整转换为 MomoBako 原生形式：",
        "",
    ]
    for capability in UNSUPPORTED_CAPABILITIES:
        lines.append(f"- {capability}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def todo_file_path() -> Path:
    return Path(__file__).resolve().parent.parent / "todo.md"


def report_output_path(output_root: Path) -> Path:
    return output_root.parent / f"{output_root.name}.import-report.json"


def now_rfc3339() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def rfc3339_from_timestamp(timestamp: float) -> str:
    return datetime.fromtimestamp(timestamp, tz=timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def infer_value_type(value: Any) -> str:
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, (int, float)):
        return "number"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        return "object"
    return "string"


def file_sha256_hash(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while True:
            chunk = handle.read(64 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def slugify_repo_id(name: str, path: str) -> str:
    return slugify_ascii_component(f"{name}-{path}")


def slugify_ascii_component(value: str) -> str:
    slug = "".join(character.lower() if character.isascii() and character.isalnum() else "-" for character in value)
    slug = re.sub(r"-{2,}", "-", slug).strip("-")
    return slug or "repo"


def asset_id_for_path(repo_id: str, relative_path: str) -> str:
    return f"asset-{sha256_hex([repo_id.encode('utf-8'), relative_path.encode('utf-8')])}"


def thumbnail_repository_dir_name(repo_id: str, repo_path: str) -> str:
    return sha256_hex([repo_id.encode("utf-8"), repo_path.encode("utf-8")])


def thumbnail_file_name(repo_id: str, repo_path: str, entry_path: str, kind: str, source: str) -> str:
    value = sha256_hex(
        [
            repo_id.encode("utf-8"),
            repo_path.encode("utf-8"),
            entry_path.encode("utf-8"),
            kind.encode("utf-8"),
            source.encode("utf-8"),
        ]
    )
    return f"{value}.jpg"


def sha256_hex(parts: list[bytes]) -> str:
    digest = hashlib.sha256()
    for part in parts:
        digest.update(part)
    return digest.hexdigest()


def sanitize_segment(value: str, fallback: str) -> str:
    sanitized = re.sub(r'[<>:"/\\\\|?*\x00-\x1f]', "_", value).strip().rstrip(".")
    sanitized = re.sub(r"\s+", " ", sanitized)
    if not sanitized:
        sanitized = fallback
    if sanitized in {REPO_META_DIR, ".meta"} or sanitized.upper() in WINDOWS_RESERVED_NAMES:
        sanitized = f"_{sanitized}"
    return sanitized


def sanitize_filename(filename: str, fallback: str) -> str:
    source = Path(filename)
    stem = sanitize_segment(source.stem, Path(fallback).stem)
    suffix = source.suffix
    if suffix:
        suffix = re.sub(r'[<>:"/\\\\|?*\x00-\x1f]', "_", suffix)
    if not suffix:
        fallback_suffix = Path(fallback).suffix
        suffix = fallback_suffix or ""
    return f"{stem}{suffix}"


def ensure_unique_segment(candidate: str, siblings: set[str], stable_key: str) -> str:
    if candidate not in siblings:
        siblings.add(candidate)
        return candidate
    unique = f"{candidate} ({stable_key[:8]})"
    siblings.add(unique)
    return unique


def ensure_unique_filename(candidate: str, siblings: set[str], stable_key: str) -> str:
    if candidate not in siblings:
        siblings.add(candidate)
        return candidate
    path = Path(candidate)
    unique = f"{path.stem} [{stable_key[:8]}]{path.suffix}"
    siblings.add(unique)
    return unique


def join_relative_path(parent: str, name: str) -> str:
    if not parent:
        return name
    return f"{parent}/{name}"


def dedupe_preserve_order(values: list[str]) -> list[str]:
    seen: set[str] = set()
    result: list[str] = []
    for value in values:
        if value in seen:
            continue
        seen.add(value)
        result.append(value)
    return result


if __name__ == "__main__":
    raise SystemExit(main())

from __future__ import annotations

import json
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent
sys.path.insert(0, str(SCRIPT_DIR))

import convert  # noqa: E402


class EagleLibraryChangerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = Path(tempfile.mkdtemp(prefix="eaglelibrarychanger-"))
        self.addCleanup(lambda: shutil.rmtree(self.temp_dir, ignore_errors=True))

    def test_build_plan_for_example_library(self) -> None:
        source = self.copy_example_library()
        output = self.temp_dir / "Converted.library"

        plan = convert.build_conversion_plan(source, output, "Converted")

        self.assertEqual(len(plan.assets), 11)
        self.assertEqual(plan.report["summary"]["deletedAssetCount"], 1)
        self.assertTrue(any(asset.target_relative_dir == "未命名文件夹" for asset in plan.assets))
        self.assertEqual(plan.assets[0].palette, ["#AA1122", "#336699", "#FFFFFF", "#000000", "#123456"])
        self.assertEqual(
            plan.assets[0].preserved_metadata,
            {
                "addedToLibraryAt": "2024-01-02T00:00:00Z",
                "fileCreatedAt": "2024-01-03T02:05:06Z",
                "fileModifiedAt": "2024-01-04T00:00:00Z",
                "height": 480,
                "link": "https://example.test/source/asset-0",
                "originalSizeBytes": 123456,
                "width": 640,
            },
        )
        asset_report = next(asset for asset in plan.report["assets"] if asset["assetId"] == "ASSET000")
        self.assertEqual(
            asset_report["preservedMetadataKeys"],
            [
                "addedToLibraryAt",
                "fileCreatedAt",
                "fileModifiedAt",
                "height",
                "link",
                "originalSizeBytes",
                "width",
            ],
        )
        self.assertEqual(plan.report["summary"]["preservedMetadataAssetCount"], 1)
        deleted_asset = next(asset for asset in plan.assets if asset.asset_id == "ASSET010")
        self.assertTrue(deleted_asset.is_deleted)
        self.assertEqual(deleted_asset.target_relative_path, "asset-10.png")
        self.assertEqual(convert.asset_trash_relative_path(deleted_asset), "asset-10.png")
        deleted_report = next(asset for asset in plan.report["assets"] if asset["assetId"] == "ASSET010")
        self.assertTrue(deleted_report["isDeleted"])
        self.assertEqual(deleted_report["status"], "deleted")
        self.assertEqual(deleted_report["trashRelativePath"], "asset-10.png")
        self.assertTrue(all(not key.startswith("eagle") for key in collect_metadata_keys(plan.report)))
        self.assertFalse(any(warning["type"] == "deletedAssetSemanticIgnored" for warning in plan.warnings))
        self.assertFalse(any(hit["capability"] == "isDeleted 语义" for hit in plan.unsupported_hits))
        self.assertFalse(any(hit["capability"] in {"url", "原始时间字段与尺寸字段", "mtime"} for hit in plan.unsupported_hits))
        self.assertTrue(
            any(
                warning["type"] == "invalidEagleMetadataField"
                and warning["assetId"] == "ASSET001"
                and warning["field"] == "importedAt"
                for warning in plan.warnings
            )
        )

    def test_dry_run_does_not_write_output(self) -> None:
        source = self.copy_example_library()
        output = self.temp_dir / "DryRun.library"

        result = subprocess.run(
            [
                sys.executable,
                str(SCRIPT_DIR / "convert.py"),
                "--input",
                str(source),
                "--output",
                str(output),
                "--dry-run",
            ],
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(output.exists())

    def test_execute_conversion_creates_importable_repository(self) -> None:
        source = self.copy_example_library()
        output = self.temp_dir / "Imported.library"

        exit_code = convert.main(
            [
                "--input",
                str(source),
                "--output",
                str(output),
                "--yes",
            ]
        )

        self.assertEqual(exit_code, 0)
        repo_meta = json.loads((output / ".momo" / "repository.json").read_text(encoding="utf-8"))
        self.assertEqual(repo_meta["backendPluginId"], "momobako.local-filesystem")
        self.assertTrue(convert.report_output_path(output).is_file())
        self.assertFalse((output / "import-report.json").exists())

        connection = sqlite3.connect(output / ".momo" / "metadata.db")
        try:
            asset_count = connection.execute("SELECT COUNT(*) FROM assets").fetchone()[0]
            deleted_asset_row = connection.execute(
                "SELECT path, status FROM assets WHERE path = ?",
                ("asset-10.png",),
            ).fetchone()
            metadata_keys = {
                row[0]
                for row in connection.execute("SELECT key FROM metadata")
            }
            asset_rows = connection.execute(
                """
                SELECT key, value_type, value_json
                FROM metadata
                WHERE asset_id = ?
                """,
                (convert.asset_id_for_path(repo_meta["repoId"], "未命名文件夹/asset-0.png"),),
            ).fetchall()
            thumbnail_paths = [
                row[0]
                for row in connection.execute("SELECT thumbnail_path FROM assets WHERE thumbnail_path IS NOT NULL")
            ]
        finally:
            connection.close()

        self.assertEqual(asset_count, 11)
        self.assertEqual(deleted_asset_row, ("asset-10.png", "deleted"))
        self.assertTrue({"color", "favorite", "palette", "title", "type"}.issubset(metadata_keys))
        asset_metadata = {key: (value_type, json.loads(value_json)) for key, value_type, value_json in asset_rows}
        self.assertEqual(asset_metadata["color"], ("string", "#AA1122"))
        self.assertEqual(asset_metadata["palette"], ("array", ["#AA1122", "#336699", "#FFFFFF", "#000000", "#123456"]))
        self.assertEqual(asset_metadata["link"], ("string", "https://example.test/source/asset-0"))
        self.assertEqual(asset_metadata["addedToLibraryAt"], ("string", "2024-01-02T00:00:00Z"))
        self.assertEqual(asset_metadata["fileCreatedAt"], ("string", "2024-01-03T02:05:06Z"))
        self.assertEqual(asset_metadata["fileModifiedAt"], ("string", "2024-01-04T00:00:00Z"))
        self.assertEqual(asset_metadata["width"], ("number", 640))
        self.assertEqual(asset_metadata["height"], ("number", 480))
        self.assertEqual(asset_metadata["originalSizeBytes"], ("number", 123456))
        self.assertTrue(thumbnail_paths)
        self.assertTrue(all(Path(path).is_file() for path in thumbnail_paths))
        self.assertFalse(any(key.startswith("eagle") for key in metadata_keys))

        self.assertFalse((output / "asset-10.png").exists())
        self.assertTrue((output / ".momo" / "trash" / "asset-10.png").is_file())
        trash_manifest = json.loads((output / ".momo" / "trash.json").read_text(encoding="utf-8"))
        self.assertEqual(
            trash_manifest["entries"],
            [
                {
                    "originalPath": "asset-10.png",
                    "trashPath": "asset-10.png",
                    "deletedAt": trash_manifest["entries"][0]["deletedAt"],
                    "kind": "file",
                }
            ],
        )
        self.assertRegex(trash_manifest["entries"][0]["deletedAt"], r"^\d{4}-\d{2}-\d{2}T")

    def test_execute_conversion_writes_eagle_repository_fields(self) -> None:
        source = self.create_eagle_fields_library()
        output = self.temp_dir / "EagleFields.library"

        exit_code = convert.main(["--input", str(source), "--output", str(output), "--yes"])

        self.assertEqual(exit_code, 0)
        repo_meta = json.loads((output / ".momo" / "repository.json").read_text(encoding="utf-8"))
        asset_id = convert.asset_id_for_path(repo_meta["repoId"], "Protected/asset.png")
        connection = sqlite3.connect(output / ".momo" / "metadata.db")
        try:
            shortcuts = connection.execute(
                """
                SELECT label, target_kind, target_path, target_id
                FROM repository_shortcuts
                ORDER BY sort_order
                """
            ).fetchall()
            tag_groups = connection.execute("SELECT name FROM tag_groups ORDER BY sort_order").fetchall()
            tag_members = connection.execute(
                """
                SELECT tag FROM tag_group_members
                WHERE tag_group_id = (
                  SELECT tag_group_id FROM tag_groups WHERE name = ?
                )
                ORDER BY sort_order
                """,
                ("用途",),
            ).fetchall()
            folder_metadata = connection.execute(
                "SELECT path, protected, password_tip FROM folder_metadata"
            ).fetchall()
            metadata_rows = {
                key: json.loads(value_json)
                for key, value_json in connection.execute(
                    "SELECT key, value_json FROM metadata WHERE asset_id = ?",
                    (asset_id,),
                )
            }
            note_count = connection.execute(
                "SELECT COUNT(*) FROM metadata WHERE asset_id = ? AND key = 'note'",
                (asset_id,),
            ).fetchone()[0]
        finally:
            connection.close()

        self.assertEqual(
            shortcuts,
            [
                ("Protected shortcut", "folder", "Protected", None),
                ("Asset shortcut", "file", "Protected/asset.png", None),
            ],
        )
        self.assertEqual(tag_groups, [("用途",), ("Starred Tags",)])
        self.assertEqual(tag_members, [("封面",), ("主视觉",)])
        self.assertEqual(folder_metadata, [("Protected", 1, "项目归档密码提示")])
        self.assertEqual(metadata_rows["comment"], "Eagle 注释")
        self.assertEqual(metadata_rows["tagGroups"], ["封面"])
        self.assertEqual(note_count, 0)

    def test_smart_folders_convert_to_momobako_filters(self) -> None:
        source = self.create_smart_folder_library()
        output = self.temp_dir / "SmartFolders.library"

        plan = convert.build_conversion_plan(source, output, "SmartFolders")

        self.assertEqual(len(plan.smart_folders), 3)
        self.assertEqual(len(plan.skipped_smart_folders), 0)
        self.assertEqual(plan.report["summary"]["smartFolderCount"], 3)
        self.assertEqual(plan.report["summary"]["skippedSmartFolderCount"], 0)

        metadata_filter = next(item for item in plan.smart_folders if item.source == "smartFolders")
        self.assertEqual(metadata_filter.name, "Campaign PNG")
        self.assertEqual(
            metadata_filter.filter,
            {
                "query": "hero",
                "pathPrefix": "Campaigns",
                "tags": ["Poster"],
                "formats": ["png"],
                "metadataFilters": [{"key": "color", "value": "red"}],
                "minRating": 4.0,
            },
        )

        saved_filter = next(item for item in plan.smart_folders if item.source == "saved-filters")
        self.assertEqual(saved_filter.name, "Saved PSD")
        self.assertEqual(saved_filter.filter, {"tags": ["Draft"], "formats": ["psd"]})
        or_filter = next(item for item in plan.smart_folders if item.source_id == "smart-or")
        self.assertEqual(
            or_filter.filter,
            {
                "tags": ["A", "B"],
                "matchMode": "or",
                "excludeTags": ["Draft"],
                "excludeFormats": ["gif"],
                "numberFilters": [
                    {"key": "width", "min": 1024, "max": 4096},
                    {"key": "originalSizeBytes", "max": 10485760},
                ],
                "dateFilters": [
                    {"key": "fileCreatedAt", "from": "2024-01-01T00:00:00Z"},
                ],
                "sort": {"field": "metadata.width", "direction": "desc"},
                "limit": 20,
            },
        )
        self.assertFalse(any(hit["capability"] == "smartFolders/saved-filters" for hit in plan.unsupported_hits))

    def test_execute_conversion_writes_smart_folders_to_database(self) -> None:
        source = self.create_smart_folder_library()
        output = self.temp_dir / "SmartFolderDb.library"

        exit_code = convert.main(
            [
                "--input",
                str(source),
                "--output",
                str(output),
                "--yes",
            ]
        )

        self.assertEqual(exit_code, 0)
        connection = sqlite3.connect(output / ".momo" / "metadata.db")
        try:
            rows = connection.execute(
                """
                SELECT smart_folder_id, parent_id, name, filter_json, sort_order
                FROM smart_folders
                ORDER BY sort_order
                """
            ).fetchall()
        finally:
            connection.close()

        self.assertEqual(len(rows), 3)
        self.assertTrue(all(row[1] is None for row in rows))
        self.assertEqual([row[2] for row in rows], ["Campaign PNG", "Too Wide", "Saved PSD"])
        self.assertEqual([row[4] for row in rows], [0, 1, 2])
        filters = [json.loads(row[3]) for row in rows]
        self.assertEqual(filters[0]["pathPrefix"], "Campaigns")
        self.assertEqual(filters[0]["formats"], ["png"])
        self.assertEqual(
            filters[1],
            {
                "matchMode": "or",
                "tags": ["A", "B"],
                "excludeTags": ["Draft"],
                "excludeFormats": ["gif"],
                "numberFilters": [
                    {"key": "width", "min": 1024, "max": 4096},
                    {"key": "originalSizeBytes", "max": 10485760},
                ],
                "dateFilters": [
                    {"key": "fileCreatedAt", "from": "2024-01-01T00:00:00Z"},
                ],
                "sort": {"field": "metadata.width", "direction": "desc"},
                "limit": 20,
            },
        )
        self.assertEqual(filters[2], {"formats": ["psd"], "tags": ["Draft"]})

    def test_multi_folder_asset_creates_alias_members(self) -> None:
        source = self.create_multi_folder_library()
        output = self.temp_dir / "MultiFolder.library"

        plan = convert.build_conversion_plan(source, output, "MultiFolder")

        self.assertEqual(len(plan.assets), 1)
        asset = plan.assets[0]
        self.assertEqual(asset.target_relative_dir, "Folder A")
        self.assertEqual([item.target_relative_path for item in asset.memberships], ["Folder A/asset.png", "Folder B/asset.png"])
        self.assertEqual(plan.report["summary"]["aliasAssetCount"], 1)

        exit_code = convert.main(["--input", str(source), "--output", str(output), "--yes"])
        self.assertEqual(exit_code, 0)
        self.assertTrue((output / "Folder A" / "asset.png").is_file())
        self.assertTrue((output / "Folder B" / "asset.png").is_file())
        connection = sqlite3.connect(output / ".momo" / "metadata.db")
        try:
            asset_rows = connection.execute("SELECT path FROM assets ORDER BY path").fetchall()
            alias_rows = connection.execute("SELECT path, role FROM asset_alias_members ORDER BY path").fetchall()
            hardlink_rows = connection.execute("SELECT path, link_state FROM hardlink_members ORDER BY path").fetchall()
        finally:
            connection.close()
        self.assertEqual(asset_rows, [("Folder A/asset.png",), ("Folder B/asset.png",)])
        self.assertEqual(alias_rows, [("Folder A/asset.png", "primary"), ("Folder B/asset.png", "alias")])
        self.assertEqual(hardlink_rows[0], ("Folder A/asset.png", "primary"))
        self.assertIn(hardlink_rows[1][1], {"linked", "copied"})

    def test_deleted_multi_folder_asset_preserves_alias_memberships_in_trash(self) -> None:
        source = self.create_multi_folder_library(is_deleted=True)
        output = self.temp_dir / "DeletedMultiFolder.library"

        exit_code = convert.main(["--input", str(source), "--output", str(output), "--yes"])
        self.assertEqual(exit_code, 0)
        self.assertTrue((output / ".momo" / "trash" / "Folder A" / "asset.png").is_file())
        self.assertTrue((output / ".momo" / "trash" / "Folder B" / "asset.png").is_file())

        connection = sqlite3.connect(output / ".momo" / "metadata.db")
        try:
            asset_rows = connection.execute("SELECT path, status FROM assets ORDER BY path").fetchall()
            alias_rows = connection.execute("SELECT path, role FROM asset_alias_members ORDER BY path").fetchall()
            hardlink_rows = connection.execute("SELECT path, link_state FROM hardlink_members ORDER BY path").fetchall()
        finally:
            connection.close()

        self.assertEqual(
            asset_rows,
            [("Folder A/asset.png", "deleted"), ("Folder B/asset.png", "deleted")],
        )
        self.assertEqual(alias_rows, [("Folder A/asset.png", "primary"), ("Folder B/asset.png", "alias")])
        self.assertEqual(hardlink_rows[0], ("Folder A/asset.png", "primary"))
        self.assertIn(hardlink_rows[1][1], {"linked", "copied"})

    def test_todo_file_path_targets_repo_external_todo(self) -> None:
        expected = REPO_ROOT / "External" / "todo.md"
        self.assertEqual(convert.todo_file_path(), expected.resolve())

    def test_report_output_path_is_outside_repository_root(self) -> None:
        output = self.temp_dir / "Repo.library"
        self.assertEqual(convert.report_output_path(output), self.temp_dir / "Repo.library.import-report.json")

    def test_name_collisions_receive_stable_suffix(self) -> None:
        source = self.create_collision_library()
        output = self.temp_dir / "Collision.library"

        plan = convert.build_conversion_plan(source, output, "Collision")
        target_paths = sorted(asset.target_relative_path for asset in plan.assets)

        self.assertEqual(len(target_paths), 2)
        self.assertNotEqual(target_paths[0], target_paths[1])
        self.assertTrue(any("[" in path for path in target_paths))

    def copy_example_library(self) -> Path:
        root = self.temp_dir / "TestBench.library"
        (root / "images").mkdir(parents=True)
        (root / "metadata.json").write_text(
            json.dumps(
                {
                    "folders": [
                        {"id": "folder-main", "name": "未命名文件夹", "children": []},
                    ],
                    "smartFolders": [],
                    "quickAccess": [],
                    "tagsGroups": [],
                },
                ensure_ascii=False,
            ),
            encoding="utf-8",
        )
        self.write_supporting_json(root)
        (root / "mtime.json").write_text(
            json.dumps({"ASSET000": 1704326400000}, ensure_ascii=False),
            encoding="utf-8",
        )
        for index in range(11):
            asset_id = f"ASSET{index:03d}"
            info_dir = root / "images" / f"{asset_id}.info"
            info_dir.mkdir()
            filename = f"asset-{index}.png"
            (info_dir / filename).write_bytes(f"png-data-{index}".encode("utf-8"))
            (info_dir / f"asset-{index}_thumbnail.png").write_bytes(f"thumb-{index}".encode("utf-8"))
            metadata = {
                "id": asset_id,
                "name": f"asset-{index}",
                "ext": "png",
                "folders": ["folder-main"] if index == 0 else [],
                "tags": ["TagA"] if index % 2 == 0 else [],
                "annotation": "note" if index == 1 else "",
                "palettes": [
                    {"color": "aa1122", "ratio": 0.5},
                    {"color": "#336699", "ratio": 0.25},
                    {"color": "fff", "ratio": 0.1},
                    {"color": "#000000", "ratio": 0.05},
                    {"color": "not-a-color", "ratio": 0.04},
                    {"color": "#123456", "ratio": 0.03},
                    {"color": "#654321", "ratio": 0.02},
                ]
                if index == 0
                else [],
                "isDeleted": index == 10,
            }
            if index == 0:
                metadata.update(
                    {
                        "url": "https://example.test/source/asset-0",
                        "importedAt": 1704153600,
                        "btime": "2024-01-03T04:05:06+02:00",
                        "width": 640,
                        "height": 480,
                        "size": 123456,
                    }
                )
            elif index == 1:
                metadata["importedAt"] = "not-a-time"
            (info_dir / "metadata.json").write_text(
                json.dumps(metadata, ensure_ascii=False),
                encoding="utf-8",
            )
        return root

    def create_eagle_fields_library(self) -> Path:
        root = self.temp_dir / "EagleFieldsSource.library"
        images_dir = root / "images" / "ASSET001.info"
        images_dir.mkdir(parents=True)
        (root / "metadata.json").write_text(
            json.dumps(
                {
                    "folders": [
                        {
                            "id": "folder-protected",
                            "name": "Protected",
                            "password": "plaintext-should-not-be-stored",
                            "passwordTips": "项目归档密码提示",
                            "children": [],
                        },
                    ],
                    "smartFolders": [],
                    "quickAccess": [
                        {"name": "Protected shortcut", "folderId": "folder-protected"},
                        {"name": "Asset shortcut", "assetId": "ASSET001"},
                    ],
                    "tagsGroups": [
                        {"id": "usage", "name": "用途", "tags": ["封面", "主视觉"]},
                    ],
                },
                ensure_ascii=False,
            ),
            encoding="utf-8",
        )
        self.write_supporting_json(root, starred_tags=["收藏"])
        (images_dir / "asset.png").write_bytes(b"png-data")
        (images_dir / "asset_thumbnail.png").write_bytes(b"thumb-data")
        (images_dir / "metadata.json").write_text(
            json.dumps(
                {
                    "id": "ASSET001",
                    "name": "asset",
                    "ext": "png",
                    "folders": ["folder-protected"],
                    "tags": ["封面"],
                    "annotation": "Eagle 注释",
                    "isDeleted": False,
                },
                ensure_ascii=False,
            ),
            encoding="utf-8",
        )
        return root

    def create_smart_folder_library(self) -> Path:
        root = self.temp_dir / "SmartFolderSource.library"
        images_dir = root / "images" / "ASSET001.info"
        images_dir.mkdir(parents=True)
        (root / "metadata.json").write_text(
            json.dumps(
                {
                    "folders": [
                        {"id": "folder-campaigns", "name": "Campaigns", "children": []},
                    ],
                    "smartFolders": [
                        {
                            "id": "smart-1",
                            "name": "Campaign PNG",
                            "query": "hero",
                            "tags": ["Poster"],
                            "formats": [".PNG"],
                            "folderIds": ["folder-campaigns"],
                            "colors": ["red"],
                            "minRating": 4,
                        },
                        {
                            "id": "smart-or",
                            "name": "Too Wide",
                            "match": "or",
                            "excludeTags": ["Draft"],
                            "excludeFormats": ["gif"],
                            "minWidth": 1024,
                            "maxWidth": 4096,
                            "maxSize": 10485760,
                            "minCreatedAt": "2024-01-01T00:00:00Z",
                            "sort": {"field": "width", "direction": "desc"},
                            "limit": 20,
                            "conditions": [
                                {"field": "tag", "operator": "contains", "value": "A"},
                                {"field": "tag", "operator": "contains", "value": "B"},
                            ],
                        },
                    ],
                    "quickAccess": [],
                    "tagsGroups": [],
                },
                ensure_ascii=False,
            ),
            encoding="utf-8",
        )
        self.write_supporting_json(
            root,
            saved_filters=[
                {
                    "id": "saved-1",
                    "title": "Saved PSD",
                    "conditions": [
                        {"field": "tag", "operator": "contains", "value": "Draft"},
                        {"field": "extension", "operator": "is", "value": "psd"},
                    ],
                }
            ],
        )
        (images_dir / "asset.png").write_bytes(b"png-data")
        (images_dir / "asset_thumbnail.png").write_bytes(b"thumb-data")
        (images_dir / "metadata.json").write_text(
            json.dumps(
                {
                    "id": "ASSET001",
                    "name": "asset",
                    "ext": "png",
                    "folders": ["folder-campaigns"],
                    "tags": ["Poster"],
                    "annotation": "",
                    "isDeleted": False,
                },
                ensure_ascii=False,
            ),
            encoding="utf-8",
        )
        return root

    def create_multi_folder_library(self, is_deleted: bool = False) -> Path:
        root = self.temp_dir / "MultiFolderSource.library"
        images_dir = root / "images" / "ASSET001.info"
        images_dir.mkdir(parents=True)
        (root / "metadata.json").write_text(
            json.dumps(
                {
                    "folders": [
                        {"id": "folder-a", "name": "Folder A", "children": []},
                        {"id": "folder-b", "name": "Folder B", "children": []},
                    ],
                    "smartFolders": [],
                    "quickAccess": [],
                    "tagsGroups": [],
                },
                ensure_ascii=False,
            ),
            encoding="utf-8",
        )
        self.write_supporting_json(root)
        (images_dir / "asset.png").write_bytes(b"png-data")
        (images_dir / "asset_thumbnail.png").write_bytes(b"thumb-data")
        (images_dir / "metadata.json").write_text(
            json.dumps(
                {
                    "id": "ASSET001",
                    "name": "asset",
                    "ext": "png",
                    "folders": ["folder-a", "folder-b"],
                    "tags": ["TagA"],
                    "annotation": "",
                    "isDeleted": is_deleted,
                },
                ensure_ascii=False,
            ),
            encoding="utf-8",
        )
        return root

    def create_collision_library(self) -> Path:
        root = self.temp_dir / "CollisionSource.library"
        (root / "images").mkdir(parents=True)
        (root / "metadata.json").write_text(
            json.dumps(
                {
                    "folders": [],
                    "smartFolders": [],
                    "quickAccess": [],
                    "tagsGroups": [],
                },
                ensure_ascii=False,
            ),
            encoding="utf-8",
        )
        self.write_supporting_json(root)
        for asset_id in ("ASSET001", "ASSET002"):
            info_dir = root / "images" / f"{asset_id}.info"
            info_dir.mkdir()
            (info_dir / "same.png").write_bytes(asset_id.encode("utf-8"))
            (info_dir / "same_thumbnail.png").write_bytes(b"thumb")
            (info_dir / "metadata.json").write_text(
                json.dumps(
                    {
                        "id": asset_id,
                        "name": "same",
                        "ext": "png",
                        "folders": [],
                        "tags": [],
                        "annotation": "",
                        "isDeleted": False,
                    },
                    ensure_ascii=False,
                ),
                encoding="utf-8",
            )
        return root

    def write_supporting_json(
        self,
        root: Path,
        saved_filters: list[dict[str, object]] | None = None,
        starred_tags: list[str] | None = None,
    ) -> None:
        (root / "tags.json").write_text(
            json.dumps({"historyTags": [], "starredTags": starred_tags or []}, ensure_ascii=False),
            encoding="utf-8",
        )
        (root / "actions.json").write_text("[]", encoding="utf-8")
        (root / "saved-filters.json").write_text(json.dumps(saved_filters or [], ensure_ascii=False), encoding="utf-8")
        (root / "mtime.json").write_text("{}", encoding="utf-8")


def collect_metadata_keys(report: dict[str, object]) -> set[str]:
    keys: set[str] = set()
    for asset in report.get("assets", []):
        if isinstance(asset, dict):
            keys.update(key for key in asset.keys() if isinstance(key, str))
    return keys


if __name__ == "__main__":
    unittest.main()

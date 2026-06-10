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
        self.assertTrue(any(asset.target_relative_dir == "未命名文件夹" for asset in plan.assets))
        self.assertTrue(all(not key.startswith("eagle") for key in collect_metadata_keys(plan.report)))

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
            metadata_keys = {
                row[0]
                for row in connection.execute("SELECT key FROM metadata")
            }
            thumbnail_paths = [
                row[0]
                for row in connection.execute("SELECT thumbnail_path FROM assets WHERE thumbnail_path IS NOT NULL")
            ]
        finally:
            connection.close()

        self.assertEqual(asset_count, 11)
        self.assertTrue({"favorite", "title", "type"}.issubset(metadata_keys))
        self.assertTrue(thumbnail_paths)
        self.assertTrue(all(Path(path).is_file() for path in thumbnail_paths))
        self.assertFalse(any(key.startswith("eagle") for key in metadata_keys))

    def test_multi_folder_asset_keeps_first_folder_only(self) -> None:
        source = self.create_multi_folder_library()
        output = self.temp_dir / "MultiFolder.library"

        plan = convert.build_conversion_plan(source, output, "MultiFolder")

        self.assertEqual(len(plan.assets), 1)
        asset = plan.assets[0]
        self.assertEqual(asset.target_relative_dir, "Folder A")
        self.assertEqual(asset.discarded_folder_names, ["Folder B"])

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
        source = REPO_ROOT / "External" / "Examples" / "TestBench.library"
        target = self.temp_dir / "TestBench.library"
        shutil.copytree(source, target)
        return target

    def create_multi_folder_library(self) -> Path:
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
                    "isDeleted": False,
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

    def write_supporting_json(self, root: Path) -> None:
        (root / "tags.json").write_text(json.dumps({"historyTags": [], "starredTags": []}), encoding="utf-8")
        (root / "actions.json").write_text("[]", encoding="utf-8")
        (root / "saved-filters.json").write_text("[]", encoding="utf-8")
        (root / "mtime.json").write_text("{}", encoding="utf-8")


def collect_metadata_keys(report: dict[str, object]) -> set[str]:
    keys: set[str] = set()
    for asset in report.get("assets", []):
        if isinstance(asset, dict):
            keys.update(key for key in asset.keys() if isinstance(key, str))
    return keys


if __name__ == "__main__":
    unittest.main()

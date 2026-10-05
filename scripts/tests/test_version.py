import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import version


class VersionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "xcode").mkdir()
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.8.1"\nedition = "2021"\n\n[workspace.dependencies]\nserde = "1"\n', encoding="utf-8")
        self.values = {"base_version": "0.8.2", "pro_revision": 9, "flash_revision": 1, "build_number": 44}
        self.write_values()

    def write_values(self):
        (self.root / "version.json").write_text(json.dumps(self.values), encoding="utf-8")

    def test_sync_updates_only_workspace_version_and_generates_ios_fields(self):
        version.sync_version(self.root)
        self.assertEqual(version.check_version(self.root)["pro_version"], "0.8.2-pro.9")
        self.assertEqual((self.root / "Cargo.toml").read_text(), '[workspace.package]\nversion = "0.8.2"\nedition = "2021"\n\n[workspace.dependencies]\nserde = "1"\n')
        self.assertIn("CURRENT_PROJECT_VERSION = 44", (self.root / "xcode/Version.xcconfig").read_text())

    def test_revision_change_is_detected_without_changing_compatibility_version(self):
        version.sync_version(self.root)
        self.values.update(pro_revision=10, build_number=45)
        self.write_values()
        with self.assertRaisesRegex(ValueError, "stale"):
            version.check_version(self.root)
        version.sync_version(self.root)
        values = version.check_version(self.root)
        self.assertEqual(values["base_version"], "0.8.2")
        self.assertEqual(values["pro_version"], "0.8.2-pro.10")
        self.assertEqual(values["flash_version"], "flash.1")

    def test_invalid_versions_fail_instead_of_producing_misnamed_packages(self):
        for key, value in [("base_version", "0.8.2-pro.9"), ("base_version", "00.8.2"), ("pro_revision", True),
                           ("flash_revision", 0), ("build_number", -1), ("build_number", 2100000001)]:
            with self.subTest(key=key, value=value):
                old = self.values[key]
                self.values[key] = value
                self.write_values()
                with self.assertRaises(ValueError):
                    version.load_version(self.root)
                self.values[key] = old


if __name__ == "__main__":
    unittest.main()

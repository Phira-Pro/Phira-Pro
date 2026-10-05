import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build


class PackageTests(unittest.TestCase):
    def test_native_names_and_zip_contents_for_all_desktop_platforms(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "assets").mkdir()
            (root / "assets/font.ttf").write_bytes(b"font")
            (root / "LICENSE").write_text("license")
            (root / "docs").mkdir()
            (root / "docs/pro-9.md").write_text("release notes excluded")
            (root / "binary").mkdir()
            for filename in ["phira-main.exe", "phira-main", "runtime.dll"]:
                (root / "binary" / filename).write_bytes(b"binary")
            (root / "binary/phira-main").chmod(0o755)
            variants = [("x86_64-pc-windows-msvc", "win64", "phira-main.exe"),
                        ("x86_64-unknown-linux-gnu", "linux-x86_64", "phira-main"),
                        ("aarch64-apple-darwin", "macos-aarch64", "phira-main")]
            with patch.object(build, "ROOT", root):
                for target, suffix, executable in variants:
                    with self.subTest(target=target):
                        destination = root / suffix
                        name = f"PhiraPro-v0.8.2-pro.9-{suffix}"
                        # Repeated packaging must preserve an unrelated user copy.
                        (destination / name / "data").mkdir(parents=True)
                        keep = destination / name / "data/keep"
                        keep.write_text("user data")
                        for _ in range(2):
                            archive = build.package_desktop(root / "binary", destination, target, {"pro_version": "0.8.2-pro.9"})
                            self.assertEqual(archive.name, name + ".zip")
                            with zipfile.ZipFile(archive) as output:
                                self.assertIsNone(output.testzip())
                                expected = {f"{name}/{executable}", f"{name}/LICENSE", f"{name}/assets/font.ttf"}
                                if "windows" in target:
                                    expected.add(f"{name}/runtime.dll")
                                self.assertEqual(set(output.namelist()), expected)
                                if sys.platform != "win32" and "windows" not in target:
                                    self.assertTrue(output.getinfo(f"{name}/{executable}").external_attr >> 16 & 0o111)
                            self.assertEqual(keep.read_text(), "user data")


if __name__ == "__main__":
    unittest.main()

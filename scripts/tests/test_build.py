import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
from package_licenses import EDITOR, EDITOR_THIRD_PARTY_DOCUMENTS, RPEN, RPEN_NOTICES, SUMMARY, verify_archive


def license_files(with_editor=True):
    gpl = b"test GPL text\n"
    polyform = b"test PolyForm text\n"
    rpen_notice = b"\n".join(RPEN_NOTICES) + b"\n"
    files = {
        "LICENSE": gpl,
        SUMMARY: b"test license map\n",
        RPEN + "LICENSE-RPEN.txt": polyform,
        RPEN + "NOTICE.txt": rpen_notice,
    }
    if with_editor:
        files.update({
            EDITOR + "LICENSE": polyform,
            EDITOR + "NOTICE": rpen_notice,
            EDITOR + "pro/LICENSE": gpl,
            EDITOR + "pro/NOTICE": b"test Pro notice\n",
            EDITOR + "pro/editor.mjs": b"// GPL-3.0-only. Test.\n",
            EDITOR + "src/ui/LICENSE-Phira-Pro": gpl,
            EDITOR + "src/ui/NOTICE-Phira-Pro": b"test overlay notice\n",
        })
        files.update({EDITOR + path: b"test third-party document\n" for path in EDITOR_THIRD_PARTY_DOCUMENTS})
    return files


def write_files(root, files):
    for name, contents in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(contents)


class PackageTests(unittest.TestCase):
    def test_native_names_and_zip_contents_for_all_desktop_platforms(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "assets").mkdir()
            (root / "assets/font.ttf").write_bytes(b"font")
            documents = license_files()
            write_files(root, documents)
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
                                expected = {f"{name}/{executable}", f"{name}/assets/font.ttf"}
                                expected.update(f"{name}/{path}" for path in documents)
                                if "windows" in target:
                                    expected.add(f"{name}/runtime.dll")
                                self.assertEqual(set(output.namelist()), expected)
                                if sys.platform != "win32" and "windows" not in target:
                                    self.assertTrue(output.getinfo(f"{name}/{executable}").external_attr >> 16 & 0o111)
                            self.assertEqual(keep.read_text(), "user data")

    def test_no_editor_zip_excludes_stale_frontend_but_keeps_player_assets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            write_files(root, license_files())
            (root / "phira-main.exe").write_bytes(b"binary")
            with patch.object(build, "ROOT", root):
                archive = build.package_desktop(root, root / "packages", "x86_64-pc-windows-msvc",
                                                {"pro_version": "0.8.3-pro.19"}, with_editor=False)
            name = "PhiraPro-v0.8.3-pro.19-win64-no-editor"
            self.assertEqual(archive.name, name + ".zip")
            with zipfile.ZipFile(archive) as output:
                self.assertEqual(set(output.namelist()), {f"{name}/{path}" for path in license_files(False)}
                                 | {f"{name}/phira-main.exe"})

    def test_mobile_archives_require_gpl_text_and_detect_new_gpl_directories(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            documents = license_files()
            write_files(root, documents)
            for suffix, prefix in [(".apk", ""), (".ipa", "Payload/Phira.app/")]:
                for omit in (None, EDITOR + "pro/LICENSE", "assets/new/LICENSE"):
                    with self.subTest(suffix=suffix, omit=omit):
                        archive = root / ("release" + suffix)
                        files = {**documents, "assets/new/plugin.mjs": b"// SPDX-License-Identifier: GPL-3.0-only\n",
                                 "assets/new/LICENSE": documents["LICENSE"], "assets/new/NOTICE": b"test notice"}
                        with zipfile.ZipFile(archive, "w") as output:
                            if suffix == ".ipa":
                                output.writestr(prefix + "phira-main", b"binary")
                            for name, contents in files.items():
                                if name == omit:
                                    continue
                                base = "assets/" if suffix == ".apk" and name in {"LICENSE", SUMMARY} else prefix
                                output.writestr(base + name, contents)
                        if omit:
                            with self.assertRaisesRegex(ValueError, omit):
                                verify_archive(archive, source=root)
                        else:
                            verify_archive(archive, source=root)


if __name__ == "__main__":
    unittest.main()

import json
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.values = {"pro_version": "0.8.2-pro.9"}
        self.tag = "v0.8.2-pro.9"
        self.sha = "a" * 40
        self.info = {"id": 17, "tag_name": self.tag, "draft": False,
                     "published_at": "2026-10-05T00:00:00Z", "immutable": False, "assets": []}

    def packages(self):
        paths = []
        for suffix in ("win64.zip", "linux-x86_64.zip", "macos-aarch64.zip",
                       "android-arm64-v8a.apk", "ios-arm64-unsigned.ipa"):
            path = self.directory / ("PhiraPro-v0.8.2-pro.9-" + suffix)
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("payload", "package: " + suffix)
            paths.append(path)
        return paths

    def remote_asset(self, path):
        return {"name": path.name, "state": "uploaded", "size": path.stat().st_size,
                "digest": "sha256:" + release.sha256(path)}

    def test_tag_matches_the_single_version_source(self):
        for tag in (self.tag, self.tag[1:]):
            release.check_tag(tag, self.values)
        for tag in ("v0.8.2-pro.10", "main", "refs/tags/" + self.tag, self.tag + "\nsha=injected"):
            with self.subTest(tag=tag), self.assertRaisesRegex(ValueError, "version.json"):
                release.check_tag(tag, self.values)

    def test_lightweight_and_annotated_tags_resolve_to_the_commit(self):
        ref = "refs/tags/" + self.tag
        for output in (f"{self.sha}\t{ref}", f"{'b' * 40}\t{ref}\n{self.sha}\t{ref}^{{}}"):
            with patch.object(release, "command_output", return_value=output):
                self.assertEqual(release.remote_commit(self.tag), self.sha)
        with patch.object(release, "command_output", return_value=""):
            with self.assertRaisesRegex(ValueError, "missing"):
                release.remote_commit(self.tag)

    def test_changed_tag_or_checkout_stops_the_release(self):
        with patch.object(release, "command_output", return_value=self.sha):
            with self.assertRaisesRegex(ValueError, "Checked-out"):
                release.check_source(self.tag, "b" * 40)
            with patch.object(release, "remote_commit", return_value="b" * 40):
                with self.assertRaisesRegex(ValueError, "tag moved"):
                    release.check_source(self.tag, self.sha)
            with patch.object(release, "remote_commit", return_value=self.sha):
                self.assertEqual(release.check_source(self.tag, self.sha), self.sha)

    def test_release_must_be_published_mutable_and_the_original_id(self):
        with patch.object(release, "command_output", return_value=json.dumps(self.info)):
            self.assertEqual(release.release_info("Phira-Pro/Phira-Pro", self.tag, 17), self.info)
        for change, message in (({"id": 18}, "replaced"), ({"draft": True}, "Publish"),
                                ({"published_at": None}, "Publish"), ({"immutable": True}, "immutable")):
            with self.subTest(change=change):
                info = dict(self.info, **change)
                with patch.object(release, "command_output", return_value=json.dumps(info)):
                    with self.assertRaisesRegex(ValueError, message):
                        release.release_info("Phira-Pro/Phira-Pro", self.tag, 17)

    def test_complete_packages_produce_checksums_for_every_platform(self):
        packages = self.packages()
        assets = release.collect_assets(self.directory, self.values)
        self.assertEqual(len(assets), 6)
        self.assertEqual(assets[-1].name, "SHA256SUMS")
        self.assertEqual(assets[-1].read_text(), "".join(
            f"{release.sha256(path)}  {path.name}\n" for path in sorted(packages)))
        self.assertEqual(release.collect_assets(self.directory, self.values), assets)

    def test_missing_wrong_version_and_duplicate_packages_are_rejected(self):
        packages = self.packages()
        removed = packages.pop()
        original = removed.read_bytes()
        removed.unlink()
        with self.assertRaisesRegex(ValueError, "five"):
            release.collect_assets(self.directory, self.values)
        wrong = self.directory / removed.name.replace("pro.9", "pro.10")
        wrong.write_bytes(original)
        with self.assertRaisesRegex(ValueError, "five"):
            release.collect_assets(self.directory, self.values)
        wrong.rename(removed)
        nested = self.directory / "duplicate"
        nested.mkdir()
        (nested / removed.name).write_bytes(original)
        with self.assertRaisesRegex(ValueError, "five"):
            release.collect_assets(self.directory, self.values)
        self.assertFalse((self.directory / "SHA256SUMS").exists())

    def test_corrupt_archive_is_rejected_before_manifest_creation(self):
        packages = self.packages()
        packages[0].write_bytes(b"truncated download")
        with self.assertRaises(zipfile.BadZipFile):
            release.collect_assets(self.directory, self.values)
        self.assertFalse((self.directory / "SHA256SUMS").exists())

    def test_retry_skips_matching_assets_and_keeps_unrelated_attachments(self):
        packages = self.packages()
        existing = [self.remote_asset(packages[0]), {"name": "maintainer-notes.txt"}]
        self.assertEqual(release.pending_uploads(packages, existing), packages[1:])
        self.assertEqual(release.pending_uploads(packages, list(map(self.remote_asset, packages))), [])

    def test_existing_asset_conflicts_fail_before_upload(self):
        packages = self.packages()
        original = self.remote_asset(packages[-1])
        for change in ({"digest": "sha256:" + "0" * 64}, {"digest": None},
                       {"state": "starter"}, {"size": original["size"] + 1}):
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "Conflicting"):
                release.pending_uploads(packages, [dict(original, **change)])

    def test_upload_cli_resumes_partial_upload_and_verifies_completion(self):
        self.packages()
        assets = release.collect_assets(self.directory, self.values)
        before = dict(self.info, assets=[self.remote_asset(assets[0])])
        after = dict(self.info, assets=list(map(self.remote_asset, assets)))
        argv = ["release.py", "upload", "--tag", self.tag, "--repository", "Phira-Pro/Phira-Pro",
                "--expected-sha", self.sha, "--release-id", "17", "--directory", str(self.directory)]
        with patch.object(sys, "argv", argv), patch.object(release, "check_version", return_value=self.values), \
                patch.object(release, "check_source", return_value=self.sha), \
                patch.object(release, "release_info", side_effect=[before, after]), \
                patch.object(release, "summary"), patch.object(release.subprocess, "run") as upload:
            release.main()
            upload.assert_called_once_with(
                ["gh", "release", "upload", self.tag, *map(str, assets[1:]),
                 "--repo", "Phira-Pro/Phira-Pro"], cwd=release.ROOT, check=True)

    def test_upload_cli_checks_all_conflicts_before_any_remote_write(self):
        self.packages()
        assets = release.collect_assets(self.directory, self.values)
        conflict = dict(self.remote_asset(assets[-1]), digest="sha256:" + "0" * 64)
        argv = ["release.py", "upload", "--tag", self.tag, "--repository", "Phira-Pro/Phira-Pro",
                "--expected-sha", self.sha, "--release-id", "17", "--directory", str(self.directory)]
        with patch.object(sys, "argv", argv), patch.object(release, "check_version", return_value=self.values), \
                patch.object(release, "check_source", return_value=self.sha), \
                patch.object(release, "release_info", return_value=dict(self.info, assets=[conflict])), \
                patch.object(release.subprocess, "run") as upload, \
                patch("sys.stderr"), self.assertRaises(SystemExit) as failure:
            release.main()
        self.assertEqual(failure.exception.code, 1)
        upload.assert_not_called()


if __name__ == "__main__":
    unittest.main()

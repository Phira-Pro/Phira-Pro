import json
import plistlib
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import ios_prerelease


class IosPrereleaseTests(unittest.TestCase):
    def setUp(self):
        self.values = dict(pro_version='0.8.2-pro.15', build_number=75, ios_version='0.8.15')
        self.tag = 'v0.8.2-pro.15-ios-diagnostic.75'
        self.info = dict(id=19, tag_name=self.tag, prerelease=True, draft=False,
                         published_at='2026-10-17T00:00:00Z', immutable=False, assets=[])
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def ipa(self, build='75'):
        path = self.directory / 'PhiraPro-v0.8.2-pro.15-ios-arm64-unsigned.ipa'
        info = dict(CFBundleIdentifier='org.flos.phirapro', CFBundleExecutable='phira-main', CFBundleShortVersionString='0.8.15',
                    CFBundleVersion=build, PhiraProVersion='0.8.2-pro.15')
        with zipfile.ZipFile(path, 'w') as archive:
            archive.writestr('Payload/Phira.app/Info.plist', plistlib.dumps(info))
            archive.writestr('Payload/Phira.app/phira-main', b'\xcf\xfa\xed\xfe\x0c\x00\x00\x01' + b'\0' * 24)
        return path

    def test_tag_cannot_target_stable_release_or_another_build(self):
        ios_prerelease.check_tag(self.tag, self.values)
        for tag in ['v0.8.2-pro.15', self.tag.replace('.75', '.74'), 'main']:
            with self.assertRaises(ValueError):
                ios_prerelease.check_tag(tag, self.values)

    def test_release_must_remain_same_published_prerelease(self):
        with patch.object(ios_prerelease.release, 'command_output', return_value=json.dumps(self.info)):
            self.assertEqual(ios_prerelease.prerelease_info('owner/repo', self.tag, 19), self.info)
        for change in [dict(prerelease=False), dict(draft=True), dict(immutable=True), dict(id=20), dict(published_at=None)]:
            with patch.object(ios_prerelease.release, 'command_output', return_value=json.dumps(dict(self.info, **change))):
                with self.assertRaises(ValueError):
                    ios_prerelease.prerelease_info('owner/repo', self.tag, 19)

    def test_only_matching_ipa_is_accepted(self):
        path = self.ipa()
        self.assertEqual(ios_prerelease.collect_ipa(self.directory, self.values), path)
        self.ipa('74')
        with self.assertRaisesRegex(ValueError, 'CFBundleVersion'):
            ios_prerelease.collect_ipa(self.directory, self.values)
        self.ipa()
        (self.directory / 'windows.zip').write_bytes(b'not an IPA')
        with self.assertRaisesRegex(ValueError, 'exactly one'):
            ios_prerelease.collect_ipa(self.directory, self.values)


if __name__ == '__main__':
    unittest.main()

"""Validate a diagnostic pre-release and upload exactly one matching iOS IPA."""
import argparse
import json
import os
import plistlib
import re
import subprocess
import zipfile
from pathlib import Path, PurePosixPath

import release
import version


def check_tag(tag, values):
    expected = f"v{values['pro_version']}-ios-diagnostic.{values['build_number']}"
    if tag != expected:
        raise ValueError(f"Diagnostic tag must be {expected}")


def prerelease_info(repository, tag, release_id=None):
    info = json.loads(release.command_output("gh", "api", f"repos/{repository}/releases/tags/{tag}"))
    if info['tag_name'] != tag or (release_id is not None and info['id'] != release_id):
        raise ValueError("Diagnostic pre-release was replaced while building")
    if not info.get('prerelease') or info.get('draft') or not info.get('published_at') or info.get('immutable'):
        raise ValueError("Expected a published mutable pre-release; refusing to change a stable Release")
    return info


def collect_ipa(directory, values):
    name = f"PhiraPro-v{values['pro_version']}-ios-arm64-unsigned.ipa"
    files = [p for p in directory.rglob('*') if p.is_file()]
    if len(files) != 1 or files[0].name != name or files[0].is_symlink():
        raise ValueError("Expected exactly one diagnostic IPA")
    path = files[0]
    with zipfile.ZipFile(path) as archive:
        if archive.testzip() is not None:
            raise ValueError("IPA integrity check failed")
        plists = [n for n in archive.namelist()
                  if len(PurePosixPath(n).parts) == 3 and n.startswith('Payload/')
                  and PurePosixPath(n).parts[1].endswith('.app') and n.endswith('/Info.plist')]
        if len(plists) != 1:
            raise ValueError("Expected one application Info.plist")
        info = plistlib.loads(archive.read(plists[0]))
        for key, expected in [('CFBundleIdentifier', 'org.flos.phirapro'),
                              ('CFBundleExecutable', 'phira-main'),
                              ('CFBundleShortVersionString', values['ios_version']),
                              ('CFBundleVersion', str(values['build_number'])),
                              ('PhiraProVersion', values['pro_version'])]:
            if info.get(key) != expected:
                raise ValueError(f"Diagnostic IPA {key} does not match its source")
        executable = str(PurePosixPath(plists[0]).parent / 'phira-main')
        with archive.open(executable) as binary:
            if binary.read(8) != b'\xcf\xfa\xed\xfe\x0c\x00\x00\x01':
                raise ValueError('Diagnostic IPA executable is not arm64 Mach-O')
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['prepare', 'upload'])
    parser.add_argument('--tag', required=True)
    parser.add_argument('--repository', default=os.environ.get('GITHUB_REPOSITORY'))
    parser.add_argument('--expected-sha', default='')
    parser.add_argument('--release-id', type=int)
    parser.add_argument('--directory', type=Path)
    args = parser.parse_args()
    try:
        if not args.repository or not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', args.repository):
            raise ValueError('Set GITHUB_REPOSITORY or supply --repository OWNER/REPO')
        values = version.check_version()
        check_tag(args.tag, values)
        sha = release.check_source(args.tag, args.expected_sha)
        info = prerelease_info(args.repository, args.tag, args.release_id)
        if args.command == 'prepare':
            if os.environ.get('GITHUB_OUTPUT'):
                with Path(os.environ['GITHUB_OUTPUT']).open('a', encoding='utf-8') as output:
                    output.write(f"sha={sha}\nrelease_id={info['id']}\n")
        else:
            if not args.expected_sha or args.release_id is None or args.directory is None:
                raise ValueError('Upload requires expected SHA, release ID and directory')
            ipa = collect_ipa(args.directory, values)
            if any(asset['name'] != ipa.name for asset in info['assets']):
                raise ValueError('Diagnostic pre-release contains unexpected attachments')
            if release.pending_uploads([ipa], info['assets']):
                subprocess.run(['gh', 'release', 'upload', args.tag, str(ipa), '--repo', args.repository], cwd=version.ROOT, check=True)
            info = prerelease_info(args.repository, args.tag, args.release_id)
            if len(info['assets']) != 1 or release.pending_uploads([ipa], info['assets']):
                raise ValueError('Diagnostic IPA upload incomplete')
        release.summary(f"Diagnostic pre-release: {args.tag}\n\nCommit: `{sha}`\n\nBuild: {values['build_number']}")
    except (ValueError, OSError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
        parser.exit(1, f'{error}\n')


if __name__ == '__main__':
    main()

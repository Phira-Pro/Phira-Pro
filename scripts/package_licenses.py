"""Verify license documents in staged assets or in a ZIP, APK or IPA."""
import argparse
import re
import zipfile
from pathlib import Path, PurePosixPath

from version import ROOT

SUMMARY = "THIRD-PARTY-NOTICES.md"
EDITOR = "assets/rpen-editor/"
RPEN = "assets/rpen/"
GPL_HEADER = re.compile(rb"(?m)^\s*(?://|/\*|\*)\s*(?:SPDX-License-Identifier: )?GPL-3\.0-only\b")
SOURCE_SUFFIXES = {".js", ".mjs", ".cjs", ".css", ".glsl"}
RPEN_NOTICES = (
    b"Required Notice: Copyright 2026 cmdysj.",
    b"Required Notice: Re:PhiEdit Next is reconstructed from the original Re:PhiEdit (RPE) project using AI (GPT).",
)
EDITOR_THIRD_PARTY_DOCUMENTS = (
    "assets/editor-skins/rpe-classic/LICENSE", "assets/editor-skins/rpe-classic/LICENSE-OFL.txt",
    "assets/editor-skins/rpe-classic/NOTICE", "assets/mods/rpe-classic-layout/LICENSE",
    "assets/mods/rpe-classic-layout/LICENSE-OFL.txt", "assets/mods/rpe-classic-layout/NOTICE",
    "assets/vendor/basic-pitch/LICENSE-Apache-2.0.txt", "assets/vendor/basic-pitch/LICENSE-basic-pitch.txt",
    "assets/vendor/mediabunny/LICENSE.txt", "assets/vendor/mediabunny/README.md",
    "src/core/vendor/LICENSE-acorn.txt", "src/core/vendor/LICENSE-html-to-image.txt",
    "src/core/vendor/LICENSE-js-yaml.txt",
)


def _lf(data):
    return data.replace(b"\r\n", b"\n")


def _verify(names, read, asset_root, license_root, with_editor, source):
    gpl = _lf((source / "LICENSE").read_bytes())
    polyform = _lf((source / RPEN / "LICENSE-RPEN.txt").read_bytes())

    def require(name, *, same_as=None, lines=()):
        if name not in names:
            raise ValueError(f"Missing license or notice in package: {name}")
        data = _lf(read(name))
        if not data.strip() or (same_as is not None and data != same_as):
            raise ValueError(f"Incorrect license or notice in package: {name}")
        if any(line not in data.splitlines() for line in lines):
            raise ValueError(f"Missing required attribution in {name}")

    require(license_root + "LICENSE", same_as=gpl)
    require(license_root + SUMMARY, same_as=_lf((source / SUMMARY).read_bytes()))
    require(asset_root + RPEN + "LICENSE-RPEN.txt", same_as=polyform)
    require(asset_root + RPEN + "NOTICE.txt", lines=RPEN_NOTICES)

    editor_root = asset_root + EDITOR
    if with_editor:
        require(editor_root + "LICENSE", same_as=polyform)
        require(editor_root + "NOTICE", lines=RPEN_NOTICES)
        for name in EDITOR_THIRD_PARTY_DOCUMENTS:
            require(editor_root + name)
    elif any(name.startswith(editor_root) for name in names):
        raise ValueError("The no-editor package contains RPEN frontend assets")

    gpl_directories = {}
    if with_editor:
        gpl_directories[editor_root + "pro/"] = ""
        gpl_directories[editor_root + "src/ui/"] = "-Phira-Pro"
    for name in sorted(names):
        if not name.startswith(asset_root + "assets/") or PurePosixPath(name).suffix not in SOURCE_SUFFIXES:
            continue
        if GPL_HEADER.search(read(name, 1024)):
            gpl_directories.setdefault(name.rsplit("/", 1)[0] + "/", "")
    for directory, suffix in gpl_directories.items():
        require(directory + "LICENSE" + suffix, same_as=gpl)
        require(directory + "NOTICE" + suffix)


def verify_directory(directory, *, with_editor=True, source=ROOT):
    def read(name, size=-1):
        with (directory / name).open("rb") as file:
            return file.read(size)

    names = {path.relative_to(directory).as_posix() for path in directory.rglob("*") if path.is_file()}
    _verify(names, read, "", "", with_editor, source)


def verify_archive(path, *, with_editor=True, source=ROOT):
    try:
        with zipfile.ZipFile(path) as archive:
            entries = [entry.filename for entry in archive.infolist() if not entry.is_dir()]
            names = set(entries)
            if len(names) != len(entries):
                raise ValueError(f"Duplicate archive entries in {path.name}")
            suffix = path.suffix.lower()
            if suffix == ".apk":
                assets, licenses = "", "assets/"
            elif suffix == ".ipa":
                bundles = {name.rsplit("/", 1)[0] + "/" for name in names
                           if re.fullmatch(r"Payload/[^/]+\.app/phira-main", name)}
                if len(bundles) != 1:
                    raise ValueError("Expected one Phira Pro app in the IPA")
                assets = licenses = bundles.pop()
            elif suffix == ".zip":
                folders = {name.split("/", 1)[0] for name in names}
                if len(folders) != 1:
                    raise ValueError("Expected one Phira Pro directory in the desktop ZIP")
                assets = licenses = folders.pop() + "/"
            else:
                raise ValueError("Expected a ZIP, APK or IPA package")

            def read(name, size=-1):
                with archive.open(name) as file:
                    return file.read(size)

            _verify(names, read, assets, licenses, with_editor, source)
    except zipfile.BadZipFile as error:
        raise ValueError(f"Invalid package ZIP {path.name}: {error}") from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--without-editor", action="store_true", help="Require that editor assets are absent")
    args = parser.parse_args()
    try:
        verify_archive(args.package, with_editor=not args.without_editor)
    except (OSError, ValueError) as error:
        parser.exit(1, f"License check failed: {error}\n")
    print(f"Verified package licenses: {args.package.name}")


if __name__ == "__main__":
    main()

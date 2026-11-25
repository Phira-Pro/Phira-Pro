"""Native build entry for Windows, Linux, macOS, Android and iOS (Python 3.8+)."""
import argparse
import json
import os
import platform
import shutil
import subprocess
import tempfile
import zipfile
from pathlib import Path

from package_licenses import SUMMARY, verify_archive, verify_directory
from version import ROOT, check_version

DESKTOP = {"windows", "linux", "macos"}


def host_platform():
    return {"Windows": "windows", "Linux": "linux", "Darwin": "macos"}.get(platform.system())


def run(command, cwd=ROOT, env=None):
    subprocess.run([str(arg) for arg in command], cwd=cwd, env=env, check=True)


def desktop_suffix(target):
    arch, _, system, *_ = target.split("-")
    if system == "windows":
        return "win64" if arch == "x86_64" else "windows-" + arch
    if system == "linux":
        return "linux-" + arch
    if system == "darwin":
        return "macos-" + arch
    raise ValueError("Unsupported desktop Rust target: " + target)


def package_desktop(binary_dir, delivery, target, versions, *, with_editor=True):
    suffix = desktop_suffix(target)
    if not with_editor:
        suffix += "-no-editor"
    name = f"PhiraPro-v{versions['pro_version']}-{suffix}"
    exe = "phira-main.exe" if "windows" in target else "phira-main"
    if not (binary_dir / exe).is_file():
        raise ValueError(f"Missing {binary_dir / exe}; build before packaging")
    delivery.mkdir(parents=True, exist_ok=True)
    archive = delivery / (name + ".zip")
    # Remove only this invocation's temporary directory, never an unpacked user copy.
    with tempfile.TemporaryDirectory(prefix=".phirapro-package-", dir=delivery) as temporary:
        Path(temporary).resolve().relative_to(delivery.resolve())
        stage = Path(temporary) / name
        stage.mkdir()
        shutil.copy2(binary_dir / exe, stage / exe)
        assets = ROOT / "assets"
        ignore = None if with_editor else lambda directory, names: {"rpen-editor"} if Path(directory) == assets else set()
        shutil.copytree(assets, stage / "assets", ignore=ignore)
        shutil.copy2(ROOT / "LICENSE", stage / "LICENSE")
        shutil.copy2(ROOT / SUMMARY, stage / SUMMARY)
        verify_directory(stage, with_editor=with_editor, source=ROOT)
        for library in binary_dir.glob("*.dll") if "windows" in target else []:
            shutil.copy2(library, stage / library.name)
        staged_zip = Path(temporary) / archive.name
        with zipfile.ZipFile(staged_zip, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as output:
            for path in sorted(stage.rglob("*")):
                if path.is_file():
                    output.write(path, path.relative_to(Path(temporary)).as_posix())
        with zipfile.ZipFile(staged_zip) as output:
            if output.testzip() is not None:
                raise ValueError("ZIP integrity check failed")
        verify_archive(staged_zip, with_editor=with_editor, source=ROOT)
        os.replace(staged_zip, archive)
    print(archive)
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    try:
        parser.add_argument("--platform", choices=sorted(DESKTOP | {"android", "ios"}), default=host_platform())
        parser.add_argument("--package", action="store_true", help="Package a desktop ZIP with assets and license")
        parser.add_argument("--skip-build", action="store_true", help="Package an existing desktop release build")
        parser.add_argument("--without-editor", action="store_true", help="Exclude the RPEN frontend from a desktop ZIP")
        parser.add_argument("--output", type=Path, help="Desktop delivery directory; default ../dist/<platform>")
        args = parser.parse_args()
        versions = check_version()
        if args.platform not in DESKTOP | {"android", "ios"}:
            raise ValueError("Unsupported host; select --platform")
        if (args.package or args.skip_build or args.output) and args.platform not in DESKTOP:
            raise ValueError("Desktop ZIP options apply to windows/linux/macos; mobile packaging uses its platform workflow")
        if args.skip_build and not args.package:
            raise ValueError("--skip-build requires --package")
        if args.without_editor and not args.package:
            raise ValueError("--without-editor requires --package")
        if args.platform in DESKTOP:
            if args.platform != host_platform():
                raise ValueError("Use the corresponding native host for " + args.platform)
            if not args.skip_build:
                build_env = dict(os.environ, PHIRA_SKIP_EDITOR_ASSETS="1") if args.without_editor else None
                run(["cargo", "build", "--locked", "--release", "-p", "phira-main"], env=build_env)
            if args.package:
                if not args.without_editor and not (ROOT / "assets" / "rpen-editor" / "files.json").is_file():
                    raise ValueError("Editor assets missing; build once (or drop --skip-build) so phira's build script stages them")
                rust = subprocess.check_output(["rustc", "-vV"], text=True)
                target = next(line[6:] for line in rust.splitlines() if line.startswith("host: "))
                configured = os.environ.get("CARGO_BUILD_TARGET")
                metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--offline", "--no-deps", "--format-version", "1"], cwd=ROOT, text=True))
                directory = Path(metadata["target_directory"])
                if configured:
                    directory /= configured
                    target = configured
                directory /= "release"
                package_desktop(directory, args.output or ROOT.parent / "dist" / args.platform, target, versions,
                                with_editor=not args.without_editor)
        elif args.platform == "android":
            if host_platform() == "windows":
                run(["cmd", "/d", "/c", "gradlew.bat", "assembleRelease", "--no-daemon", "--console=plain"], ROOT / "phira-android")
            else:
                run(["bash", "./gradlew", "assembleRelease", "--no-daemon", "--console=plain"], ROOT / "phira-android")
            apks = sorted((ROOT / "phira-android/app/build/outputs/apk/release").glob("app-release*.apk"))
            if not apks:
                raise ValueError("Android build did not produce a release APK")
            for apk in apks:
                verify_archive(apk)
        else:
            if host_platform() != "macos":
                raise ValueError("iOS requires macOS and Xcode")
            run(["xcodebuild", "-project", "phira.xcodeproj", "-scheme", "phira", "-configuration", "Release",
                 "-sdk", "iphoneos", "-destination", "generic/platform=iOS", "-derivedDataPath", "build/dd",
                 "CODE_SIGNING_ALLOWED=NO", "CODE_SIGNING_REQUIRED=NO", "CODE_SIGN_IDENTITY=", "PRODUCT_BUNDLE_IDENTIFIER=org.flos.phirapro", "build"])
            apps = list((ROOT / "build/dd/Build/Products/Release-iphoneos").glob("*.app"))
            if len(apps) != 1:
                raise ValueError("Expected one Phira Pro iOS app")
            verify_directory(apps[0])
    except KeyboardInterrupt:
        parser.exit(130, "\nBuild cancelled (Ctrl+C).\n")
    except (ValueError, OSError, subprocess.CalledProcessError, StopIteration) as error:
        parser.exit(1, f"Build failed: {error}\n")


if __name__ == "__main__":
    main()

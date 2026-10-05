"""Native build entry for Windows, Linux, macOS, Android and iOS (Python 3.8+)."""
import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import tempfile
import zipfile
from pathlib import Path

from version import ROOT, check_version

DESKTOP = {"windows", "linux", "macos"}


def host_platform():
    return {"Windows": "windows", "Linux": "linux", "Darwin": "macos"}.get(platform.system())


def run(command, cwd=ROOT):
    subprocess.run([str(arg) for arg in command], cwd=cwd, check=True)


def desktop_suffix(target):
    arch, _, system, *_ = target.split("-")
    if system == "windows":
        return "win64" if arch == "x86_64" else "windows-" + arch
    if system == "linux":
        return "linux-" + arch
    if system == "darwin":
        return "macos-" + arch
    raise ValueError("Unsupported desktop Rust target: " + target)


def package_desktop(binary_dir, delivery, target, versions):
    suffix = desktop_suffix(target)
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
        shutil.copytree(ROOT / "assets", stage / "assets")
        shutil.copy2(ROOT / "LICENSE", stage / "LICENSE")
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
        os.replace(staged_zip, archive)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    Path(str(archive) + ".sha256").write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    print(archive)
    print("SHA256: " + digest)
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=sorted(DESKTOP | {"android", "ios"}), default=host_platform())
    parser.add_argument("--package", action="store_true", help="Package a desktop ZIP with assets and license")
    parser.add_argument("--skip-build", action="store_true", help="Package an existing desktop release build")
    parser.add_argument("--output", type=Path, help="Desktop delivery directory; default ../dist/<platform>")
    args = parser.parse_args()
    try:
        versions = check_version()
        if args.platform not in DESKTOP | {"android", "ios"}:
            raise ValueError("Unsupported host; select --platform")
        if (args.package or args.skip_build or args.output) and args.platform not in DESKTOP:
            raise ValueError("Desktop ZIP options apply to windows/linux/macos; mobile packaging uses its platform workflow")
        if args.skip_build and not args.package:
            raise ValueError("--skip-build requires --package")
        if args.platform in DESKTOP:
            if args.platform != host_platform():
                raise ValueError("Use the corresponding native host for " + args.platform)
            if not args.skip_build:
                run(["cargo", "build", "--locked", "--release", "-p", "phira-main"])
            if args.package:
                rust = subprocess.check_output(["rustc", "-vV"], text=True)
                target = next(line[6:] for line in rust.splitlines() if line.startswith("host: "))
                configured = os.environ.get("CARGO_BUILD_TARGET")
                metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--offline", "--no-deps", "--format-version", "1"], cwd=ROOT, text=True))
                directory = Path(metadata["target_directory"])
                if configured:
                    directory /= configured
                    target = configured
                directory /= "release"
                package_desktop(directory, args.output or ROOT.parent / "dist" / args.platform, target, versions)
        elif args.platform == "android":
            if host_platform() == "windows":
                run(["cmd", "/d", "/c", "gradlew.bat", "assembleRelease", "--no-daemon", "--console=plain"], ROOT / "phira-android")
            else:
                run(["bash", "./gradlew", "assembleRelease", "--no-daemon", "--console=plain"], ROOT / "phira-android")
        else:
            if host_platform() != "macos":
                raise ValueError("iOS requires macOS and Xcode")
            run(["xcodebuild", "-project", "phira.xcodeproj", "-scheme", "phira", "-configuration", "Release",
                 "-sdk", "iphoneos", "-destination", "generic/platform=iOS", "-derivedDataPath", "build/dd",
                 "CODE_SIGNING_ALLOWED=NO", "CODE_SIGNING_REQUIRED=NO", "CODE_SIGN_IDENTITY=", "PRODUCT_BUNDLE_IDENTIFIER=org.flos.phirapro", "build"])
    except (ValueError, OSError, subprocess.CalledProcessError, StopIteration) as error:
        parser.exit(1, f"Build failed: {error}\n")


if __name__ == "__main__":
    main()

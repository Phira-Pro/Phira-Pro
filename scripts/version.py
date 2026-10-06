"""Shared version reader and synchronizer. Requires only Python's standard library."""
import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
KEYS = {"base_version", "pro_revision", "flash_revision", "build_number"}


def load_version(root=ROOT):
    values = json.loads((root / "version.json").read_text(encoding="utf-8"))
    if not isinstance(values, dict) or set(values) != KEYS:
        raise ValueError("version.json must contain exactly: " + ", ".join(sorted(KEYS)))
    base = values["base_version"]
    if not isinstance(base, str) or not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", base):
        raise ValueError("base_version must be a numeric major.minor.patch version")
    for key in KEYS - {"base_version"}:
        number = values[key]
        if type(number) is not int or not 1 <= number <= 2_100_000_000:
            raise ValueError(f"{key} must be a positive integer <= 2100000000")
    # iOS requires exactly three numeric components. Include the Pro revision
    # instead of keeping the system-visible version fixed at the upstream base.
    major, minor, _ = base.split(".")
    return dict(values, pro_version=f"{base}-pro.{values['pro_revision']}",
                ios_version=f"{major}.{minor}.{values['pro_revision']}", flash_version=f"flash.{values['flash_revision']}")


def xcconfig(values):
    return (
        "// Generated from version.json by scripts/version.py sync. Do not edit.\n"
        f"MARKETING_VERSION = {values['ios_version']}\n"
        f"CURRENT_PROJECT_VERSION = {values['build_number']}\n"
        f"PHIRA_PRO_VERSION = {values['pro_version']}\n"
    )


def outputs(root=ROOT):
    values = load_version(root)
    cargo = root / "Cargo.toml"
    text = cargo.read_text(encoding="utf-8")
    pattern = r'(\[workspace\.package\][^\[]*?^version\s*=\s*)"[^"]*"'
    updated, count = re.subn(pattern, lambda match: match[1] + json.dumps(values["base_version"]), text, flags=re.M)
    if count != 1:
        raise ValueError("Cannot find the single workspace.package.version in Cargo.toml")
    return {cargo: updated, root / "xcode/Version.xcconfig": xcconfig(values)}


def check_version(root=ROOT):
    stale = [str(path.relative_to(root)) for path, content in outputs(root).items()
             if not path.is_file() or path.read_text(encoding="utf-8") != content]
    if stale:
        raise ValueError("Version outputs are stale: " + ", ".join(stale) + "; run python scripts/version.py sync")
    return load_version(root)


def sync_version(root=ROOT):
    for path, content in outputs(root).items():
        if not path.is_file() or path.read_text(encoding="utf-8") != content:
            with path.open("w", encoding="utf-8", newline="\n") as output:
                output.write(content)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["show", "check", "sync"])
    parser.add_argument("--field", choices=sorted(KEYS | {"pro_version", "ios_version", "flash_version"}))
    args = parser.parse_args()
    try:
        if args.command == "sync":
            sync_version()
        values = check_version() if args.command != "show" else load_version()
    except (ValueError, OSError) as error:
        parser.exit(1, f"{error}\n")
    if args.field:
        print(values[args.field])
    elif args.command == "show":
        print(json.dumps(values, indent=2))
    else:
        print("Version definitions are consistent: " + values["pro_version"])


if __name__ == "__main__":
    main()

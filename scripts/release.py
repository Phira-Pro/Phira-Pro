"""Validate and upload the five-platform Release asset set using the GitHub CLI."""
import argparse
import hashlib
import json
import os
import re
import subprocess
import zipfile
from pathlib import Path

from version import ROOT, check_version


def check_tag(tag, values):
    if tag not in (values["pro_version"], "v" + values["pro_version"]):
        raise ValueError(f"Release tag {tag!r} must match version.json: v{values['pro_version']}")


def command_output(*command):
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def remote_commit(tag):
    # Annotated tags have both a tag object and a peeled commit; prefer the latter.
    ref = "refs/tags/" + tag
    lines = command_output("git", "ls-remote", "--tags", "origin", ref, ref + "^{}")
    refs = dict(line.split()[::-1] for line in lines.splitlines())
    sha = refs.get(ref + "^{}", refs.get(ref, ""))
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError(f"Release tag {tag!r} is missing from origin")
    return sha


def resolve_tag(tag, expected_sha=""):
    # Keep the established tag spelling. A manually entered pro10 resolves to
    # pro.10; never create a second tag with a different naming convention.
    match = re.fullmatch(r"(v?(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))-pro\.?([1-9][0-9]*)", tag)
    if not match:
        raise ValueError("Invalid Release tag; expected v0.8.2-pro.10")
    canonical = f"{match[1]}-pro.{match[2]}"
    sha = remote_commit(canonical)
    if expected_sha and sha != expected_sha:
        raise ValueError("Release event commit differs from the remote tag")
    return canonical, sha


def release_info(repository, tag, release_id=None):
    try:
        info = json.loads(command_output("gh", "api", f"repos/{repository}/releases/tags/{tag}"))
    except subprocess.CalledProcessError as error:
        try:
            missing = json.loads(error.output).get("message") == "Not Found"
        except (TypeError, ValueError, AttributeError):
            missing = False
        if missing:
            raise ValueError(f"Release {tag!r} does not exist; publish its Release first, or select validate-only to check the source") from error
        raise
    if info["tag_name"] != tag or (release_id is not None and info["id"] != release_id):
        raise ValueError("Release was replaced while building; refusing to upload")
    if info["draft"] or not info.get("published_at"):
        raise ValueError("Publish the Release before starting its asset build")
    if info.get("immutable"):
        raise ValueError("Release is immutable; published immutable releases cannot accept assets")
    revision = tag.rsplit("-pro.", 1)[-1]
    if info.get("name") != f"pro.{revision}":
        raise ValueError(f"Release name must be pro.{revision}")
    return info


def check_source(tag, expected_sha):
    sha = command_output("git", "rev-parse", "HEAD")
    if expected_sha and sha != expected_sha:
        raise ValueError("Checked-out commit differs from the requested Release commit")
    if remote_commit(tag) != sha:
        raise ValueError("Release tag moved; refusing to mix builds from different commits")
    return sha


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def collect_assets(directory, values):
    prefix = "PhiraPro-v" + values["pro_version"]
    expected = {prefix + suffix for suffix in (
        "-win64.zip", "-linux-x86_64.zip", "-macos-aarch64.zip",
        "-android-arm64-v8a.apk", "-ios-arm64-unsigned.ipa",
    )}
    found = [path for path in directory.rglob("*") if path.suffix in {".zip", ".apk", ".ipa"}]
    if len(found) != len(expected) or {path.name for path in found} != expected:
        raise ValueError("Expected exactly the five versioned platform packages; found: "
                         + ", ".join(sorted(path.name for path in found)))
    assets = sorted(found, key=lambda path: path.name)
    for path in assets:
        if path.is_symlink() or not path.is_file():
            raise ValueError("Invalid package: " + str(path))
        with zipfile.ZipFile(path) as archive:
            if not archive.namelist() or archive.testzip() is not None:
                raise ValueError("Package integrity check failed: " + path.name)
    # Digests remain an internal retry/integrity check. Release attachments
    # contain only the five packages, as requested by the project owner.
    return assets


def pending_uploads(assets, existing):
    by_name = {asset["name"]: asset for asset in existing}
    pending = []
    for path in assets:
        remote = by_name.get(path.name)
        if remote is None:
            pending.append(path)
        elif (remote.get("state") != "uploaded" or remote.get("size") != path.stat().st_size
              or remote.get("digest") != "sha256:" + sha256(path)):
            raise ValueError(f"Conflicting existing asset: {path.name}; no assets were overwritten. "
                             "Check the existing attachment before removing it and retrying.")
    return pending


def summary(content):
    print(content)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a", encoding="utf-8") as output:
            output.write(content + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("resolve", "prepare", "upload"):
        command = commands.add_parser(name)
        command.add_argument("--tag", required=True)
        command.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY"))
        command.add_argument("--expected-sha", default="", required=name == "upload")
        if name == "upload":
            command.add_argument("--release-id", type=int, required=True)
            command.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    try:
        if not args.repository or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repository):
            raise ValueError("Set GITHUB_REPOSITORY or supply --repository OWNER/REPO")
        if args.command == "resolve":
            tag, sha = resolve_tag(args.tag, args.expected_sha)
            if os.environ.get("GITHUB_OUTPUT"):
                with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
                    output.write(f"tag={tag}\nsha={sha}\n")
            summary(f"Resolved tag: {tag}\n\nCommit: `{sha}`")
            return
        values = check_version()
        check_tag(args.tag, values)
        sha = check_source(args.tag, args.expected_sha)
        info = release_info(args.repository, args.tag, getattr(args, "release_id", None))
        if args.command == "prepare":
            if os.environ.get("GITHUB_OUTPUT"):
                with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
                    output.write(f"tag={args.tag}\nsha={sha}\nrelease_id={info['id']}\n")
            summary(f"Release: {args.tag}\n\nCommit: `{sha}`\n\nBuild number: {values['build_number']}")
        else:
            assets = collect_assets(args.directory, values)
            pending = pending_uploads(assets, info["assets"])
            if pending:
                # Do not use --clobber: a failed upload must preserve existing attachments.
                subprocess.run(["gh", "release", "upload", args.tag, *map(str, pending),
                                "--repo", args.repository], cwd=ROOT, check=True)
            info = release_info(args.repository, args.tag, args.release_id)
            if pending_uploads(assets, info["assets"]):
                raise ValueError("Release upload is incomplete; rerun the failed upload job")
            summary("Release assets verified:\n\n" + "\n".join("- " + path.name for path in assets))
    except (ValueError, OSError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()

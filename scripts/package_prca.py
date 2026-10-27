"""Package the independently installed PRCA plugin (standard library only)."""
import argparse
import json
import os
import tempfile
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED

ROOT = Path(__file__).resolve().parent.parent


def package(destination):
    source = ROOT / "docs/plugins/examples/prca"
    manifest = json.loads((source / "manifest.json").read_text(encoding="utf-8"))
    if manifest["api"] != 3 or manifest["id"] != "phira-pro.prca":
        raise ValueError("Unexpected PRCA manifest")
    names = ["manifest.json", "main.rhai", *manifest.get("assetFiles", {}).values()]
    destination.mkdir(parents=True, exist_ok=True)
    result = destination / f"Phira-Pro-PRCA-v{manifest['version']}.zip"
    with tempfile.NamedTemporaryFile(prefix=".prca-", suffix=".zip", dir=destination, delete=False) as temporary:
        staging = Path(temporary.name)
    try:
        with ZipFile(staging, "w", ZIP_DEFLATED, compresslevel=9) as archive:
            for name in names:
                if Path(name).name != name:
                    raise ValueError("PRCA assets must be root-level files")
                archive.write(source / name, name)
        with ZipFile(staging) as archive:
            if archive.testzip() is not None or sorted(archive.namelist()) != sorted(names):
                raise ValueError("PRCA ZIP verification failed")
        os.replace(staging, result)
    finally:
        staging.unlink(missing_ok=True)
    print(result)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT.parent / "dist/plugins")
    package(parser.parse_args().output)

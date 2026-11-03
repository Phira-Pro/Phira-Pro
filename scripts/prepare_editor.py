"""Fetch the pinned offline RPEN resources and index them for native extraction.

`scripts/editor/upstream.json` pins the Pro fork and commit. That commit also
carries `phira-pro-bundle.json`, which decides what ships; anything else in the
checkout is dropped. The staged tree lives in the ignored `assets/rpen-editor/`.
"""
import hashlib
import json
import shutil
import subprocess
import zlib
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'scripts' / 'editor'
TARGET = ROOT / 'assets' / 'rpen-editor'
CHECKOUT = ROOT / 'target' / 'rpe-next-upstream'
STAGING = ROOT / 'target' / 'rpe-next-staging'
BUNDLE = 'phira-pro-bundle.json'
STAMP = '.staged.json'

REQUIRED = ['index.html', 'LICENSE', 'NOTICE', 'UPSTREAM.json', 'pro/bridge.js', 'pro/editor.mjs', 'src/ui/mod-runtime.mjs']


def run_git(directory, *args):
    subprocess.run(['git', *args], cwd=directory, check=True, stdout=subprocess.DEVNULL)


def git_head(directory):
    return subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=directory, text=True).strip()


def pinned():
    upstream = json.loads((SOURCE / 'upstream.json').read_text(encoding='utf-8'))
    for key in ('repository', 'revision', 'version', 'license'):
        if not isinstance(upstream.get(key), str) or not upstream[key]:
            raise ValueError(f'scripts/editor/upstream.json lacks {key}')
    return upstream


def load_bundle(root):
    path = root / BUNDLE
    try:
        bundle = json.loads(path.read_text(encoding='utf-8'))
    except FileNotFoundError:
        raise ValueError(f'Pinned RPEN revision has no {BUNDLE}') from None
    except ValueError as error:
        raise ValueError(f'Invalid {BUNDLE}: {error}') from None
    if not isinstance(bundle, dict) or bundle.get('schema') != 1:
        raise ValueError(f'Unsupported {BUNDLE} schema: {bundle.get("schema") if isinstance(bundle, dict) else bundle!r}')
    files = bundle.get('files')
    if not isinstance(files, list) or not files:
        raise ValueError(f'{BUNDLE} lists no files')
    names = []
    for name in files:
        path = PurePosixPath(name) if isinstance(name, str) else None
        if path is None or not name or path.is_absolute() or '\\' in name or any(part in ('..', '') for part in name.split('/')):
            raise ValueError(f'{BUNDLE} has an invalid path: {name!r}')
        names.append(name)
    if len(names) != len(set(names)):
        raise ValueError(f'{BUNDLE} lists a file twice')
    return names


def fingerprint(upstream):
    digest = hashlib.sha256()
    digest.update(json.dumps(upstream, sort_keys=True, ensure_ascii=False).encode('utf-8'))
    return digest.hexdigest()


def checkout(upstream):
    revision = upstream['revision']
    if (CHECKOUT / '.git').is_dir() and git_head(CHECKOUT) == revision:
        return
    shutil.rmtree(CHECKOUT, ignore_errors=True)
    CHECKOUT.mkdir(parents=True)
    run_git(CHECKOUT, 'init', '-q')
    run_git(CHECKOUT, 'remote', 'add', 'origin', upstream['repository'])
    run_git(CHECKOUT, 'fetch', '-q', '--depth', '1', 'origin', revision)
    run_git(CHECKOUT, 'checkout', '-q', '--detach', 'FETCH_HEAD')
    actual = git_head(CHECKOUT)
    if actual != revision:
        raise ValueError(f'Checked out {actual}, expected {revision}')


def stage(upstream, names):
    missing = [name for name in names if not (CHECKOUT / name).is_file()]
    if missing:
        raise ValueError(f'{BUNDLE} lists missing file: {missing[0]}')
    shutil.rmtree(STAGING, ignore_errors=True)
    for name in names:
        destination = STAGING / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(CHECKOUT / name, destination)
    (STAGING / 'UPSTREAM.json').write_text(json.dumps(upstream, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    shutil.rmtree(TARGET, ignore_errors=True)
    shutil.move(STAGING, TARGET)


def fetch():
    upstream = pinned()
    stamp = fingerprint(upstream)
    recorded = TARGET / STAMP
    if recorded.is_file():
        try:
            if json.loads(recorded.read_text(encoding='utf-8'))['fingerprint'] == stamp:
                return
        except (ValueError, KeyError):
            pass
    checkout(upstream)
    print(f'Staging RPEN {upstream["revision"][:12]} from {upstream["repository"]}')
    stage(upstream, load_bundle(CHECKOUT))
    (TARGET / STAMP).write_text(json.dumps({'fingerprint': stamp, 'revision': upstream['revision']}, indent=2) + '\n', encoding='utf-8')


def prepare():
    root = TARGET
    fetch()
    names = sorted(p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file() and p.name not in ('files.json', STAMP))
    for required in REQUIRED:
        if required not in names:
            raise ValueError('Missing offline editor resource: ' + required)
    revision = 0
    for name in names:
        revision = zlib.crc32(name.encode('utf-8') + b'\0', revision)
        revision = zlib.crc32((root / name).read_bytes(), revision)
    content = json.dumps({'schema': 1, 'contentId': format(revision, '08x'), 'files': names}, ensure_ascii=False, indent=2) + '\n'
    target = root / 'files.json'
    if not target.exists() or target.read_text(encoding='utf-8') != content:
        target.write_text(content, encoding='utf-8')


if __name__ == '__main__':
    prepare()

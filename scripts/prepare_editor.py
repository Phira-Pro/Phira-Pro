"""Index the pinned offline RPEN resources for native asset extraction."""
import json
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def prepare():
    root = ROOT / 'assets' / 'rpen-editor'
    names = sorted(p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file() and p.name != 'files.json')
    for required in ['index.html', 'LICENSE', 'NOTICE', 'UPSTREAM.json', 'pro/bridge.js', 'pro/editor.mjs', 'src/ui/mod-runtime.mjs']:
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

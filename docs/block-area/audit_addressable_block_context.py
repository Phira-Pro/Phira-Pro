"""Read the original Addressables chart/level-effect bundles, without editing them.

The catalog's key/bucket/28-byte entry layout is checked against its stored
counts. Decoding selected dependencies reveals per-chart postprocessing outside
the common ActiveBlock material. UnityPy is an optional review dependency.
"""
import base64
import io
import json
from pathlib import Path
import struct
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'target/block-area-tools'))
import UnityPy

output = ROOT / 'target/block-area-source-charts'
output.mkdir(exist_ok=True)
with zipfile.ZipFile(ROOT.parent / 'Phigros_4.0.1_APKPure.xapk') as xapk:
    with zipfile.ZipFile(io.BytesIO(xapk.read('UnityStreamingAssetsPack.apk'))) as apk:
        catalog = json.loads(apk.read('assets/aa/catalog.json'))
        keys = base64.b64decode(catalog['m_KeyDataString'])
        buckets = base64.b64decode(catalog['m_BucketDataString'])
        entries = base64.b64decode(catalog['m_EntryDataString'])
        count = struct.unpack_from('<i', entries)[0]
        assert len(entries) == 4 + count * 28
        entries = [struct.unpack_from('<7i', entries, 4 + i * 28) for i in range(count)]
        bucket_count = struct.unpack_from('<i', buckets)[0]
        assert bucket_count == struct.unpack_from('<i', keys)[0]
        bucket_rows = []
        position = 4
        for _ in range(bucket_count):
            offset, length = struct.unpack_from('<2i', buckets, position)
            indices = list(struct.unpack_from(f'<{length}i', buckets, position + 8))
            position += 8 + length * 4
            tag = keys[offset]
            if tag in (0, 1):
                size = struct.unpack_from('<i', keys, offset + 1)[0]
                key = keys[offset + 5:offset + 5 + size].decode('utf8' if tag == 0 else 'utf-16-le')
            else:
                key = f'<key type {tag} at {offset}>'
            bucket_rows.append((key, indices))
        assert position == len(buckets)
        selected = {}
        required = set()

        def dependencies(index):
            if index in required:
                return
            required.add(index)
            dependency = entries[index][2]
            if dependency >= 0:
                for child in bucket_rows[dependency][1]:
                    dependencies(child)

        for key, indices in bucket_rows:
            if any(word in key for word in ('DesultorySignals', 'Hate', 'ハテ')):
                selected[key] = [catalog['m_InternalIds'][entries[i][0]] for i in indices]
                for i in indices:
                    dependencies(i)
        (output / 'selected-catalog.json').write_text(json.dumps(selected, ensure_ascii=False, indent=2), encoding='utf8')
        bundles = {catalog['m_InternalIds'][entries[index][0]].split('/')[-1] for index in required
                   if catalog['m_InternalIds'][entries[index][0]].endswith('.bundle')}
        print('Selected dependencies:', len(bundles), 'bundles')
        environment = UnityPy.Environment()
        for name in sorted(bundles):
            data = apk.read('assets/aa/Android/' + name)
            if not data.startswith(b'Unity'):
                print('Encrypted/non-Unity bundle:', name, len(data))
                continue
            environment.load_file(data, name=name)

        findings = []
        for obj in environment.objects:
            if obj.type.name in ('TextAsset', 'MonoBehaviour', 'Material', 'Shader'):
                try:
                    value = obj.read_typetree()
                except Exception as error:
                    print('No type tree:', obj.type.name, obj.path_id, str(error)[:100])
                    continue
                name = value.get('m_Name', '')
                if obj.type.name == 'TextAsset':
                    text = value['m_Script']
                    if 'Chart' in name:
                        filename = obj.assets_file.name + '-' + str(obj.path_id) + '-' + name + '.json'
                        (output / filename).write_text(text, encoding='utf8')
                        print('Chart:', filename, len(text))
                else:
                    filename = obj.assets_file.name + '-' + str(obj.path_id) + '.json'
                    (output / filename).write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding='utf8')
                    findings.append({'file': filename, 'type': obj.type.name, 'name': name})
        (output / 'bundle-objects.json').write_text(json.dumps(findings, ensure_ascii=False, indent=2), encoding='utf8')
        print('Saved', len(findings), 'level-effect objects')

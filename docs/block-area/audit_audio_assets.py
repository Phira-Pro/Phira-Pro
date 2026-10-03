"""Read serialized LevelControl audio parameters from the original APK."""
import io
import json
import sys
import zipfile
from pathlib import Path
root = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(root / 'target/block-area-tools'))
import UnityPy
with zipfile.ZipFile(root.parent / 'Phigros_4.0.1_APKPure.xapk') as xapk:
    bundles = []
    for apk_name, bundle in [('com.PigeonGames.Phigros.apk', 'data.unity3d'),
                             ('UnityDataAssetPack.apk', 'datapack.unity3d')]:
        with zipfile.ZipFile(io.BytesIO(xapk.read(apk_name))) as apk:
            bundles.append(apk.read('assets/bin/Data/' + bundle))
env = UnityPy.load(*bundles)
result = []
for obj in env.objects:
    if obj.type.name != 'MonoBehaviour':
        continue
    try:
        mono = obj.read(check_read=False)
        script = mono.m_Script.read()
        if script.m_ClassName != 'LevelControl':
            continue
        try:
            tree = obj.read_typetree()
            result.append({'path_id': obj.path_id, 'file': obj.assets_file.name, 'tree': tree})
        except Exception as e:
            import struct
            raw = obj.get_raw_data()
            # LevelControl's two final scalar fields precede the serialized
            # specialSongNameReplacements list. Check both the value pair and
            # its following list header, rather than assuming ctor defaults.
            pair = struct.pack('<ff', 1500., 0.1)
            start = raw.index(pair)
            assert raw.count(pair) == 1
            assert struct.unpack_from('<II', raw, start + 8) == (1, 26)
            result.append({'path_id': obj.path_id, 'file': obj.assets_file.name,
                           'typetree_error': str(e), 'parameter_byte_offset': start,
                           'lowPassFilterCutoffFrequency': struct.unpack_from('<f', raw, start)[0],
                           'lowPassFilterLerpDuration': struct.unpack_from('<f', raw, start + 4)[0],
                           'raw_hex': raw.hex()})
    except Exception:
        continue
dest = root / 'target/block-area-audio-assets.json'
dest.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
print(dest, 'LevelControl count', len(result))

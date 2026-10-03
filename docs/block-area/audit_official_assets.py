"""Read-only audit of original serialized shader states and player settings.

Uses optional UnityPy in target/block-area-tools; no application dependency.
Never treats the AssetRipper DummyShaderTextExporter shell as executable code.
"""
import hashlib
import io
import json
from pathlib import Path
import sys
import zipfile

root = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(root / 'target/block-area-tools'))
import UnityPy

archive = root.parent / 'Phigros_4.0.1_APKPure.xapk'
with zipfile.ZipFile(archive) as xapk:
    with zipfile.ZipFile(io.BytesIO(xapk.read('com.PigeonGames.Phigros.apk'))) as apk:
        data = apk.read('assets/bin/Data/data.unity3d')
    with zipfile.ZipFile(io.BytesIO(xapk.read('UnityDataAssetPack.apk'))) as apk:
        datapack = apk.read('assets/bin/Data/datapack.unity3d')
environment = UnityPy.load(data, datapack)
result = {'source_archive': str(archive), 'unity_data_sha256': hashlib.sha256(data).hexdigest(), 'unity_datapack_sha256': hashlib.sha256(datapack).hexdigest(), 'shaders': {}, 'shader_properties': {}, 'materials': {}, 'textures': {}}
selected = {'Unlit/ActiveBlock','Unlit/DisabledBlock','Unlit/ReadyBlock','Unlit/BlockSprite','Unlit/BlockCompose','Unlit/SubtractBlockBlender'}
for obj in environment.objects:
    if obj.type.name == 'PlayerSettings':
        properties = obj.read_typetree()
        result['player'] = {key: properties[key] for key in ['bundleVersion','m_ActiveColorSpace','playerMinOpenGLESVersion']}
    elif obj.type.name == 'Shader':
        properties = obj.read_typetree()['m_ParsedForm']
        if properties['m_Name'] in selected:
            result['shaders'][properties['m_Name']] = [p['m_State']['rtBlend0'] for sub in properties['m_SubShaders'] for p in sub['m_Passes']]
            result['shader_properties'][properties['m_Name']] = properties['m_PropInfo']['m_Props']
    elif obj.type.name == 'Material':
        material = obj.read_typetree()
        if material['m_Name'] in {'ActiveBlock', 'DisabledBlock', 'ReadyBlock', 'BlockCompose', 'TouchEffect'}:
            result['materials'][material['m_Name']] = material['m_SavedProperties']
    elif obj.type.name == 'Texture2D':
        texture = obj.read()
        if texture.m_Name in {'BlockNoise1', 'PointNoise', 'FD_Noise_00000'}:
            properties = obj.read_typetree()
            result['textures'][texture.m_Name] = {key: properties.get(key) for key in ('m_Width', 'm_Height', 'm_TextureFormat', 'm_ColorSpace', 'm_MipCount', 'm_TextureSettings')}
            result['textures'][texture.m_Name]['exported_rgba_sha256'] = hashlib.sha256(texture.image.convert('RGBA').tobytes()).hexdigest()
            channels = texture.image.convert('RGBA').split()
            result['textures'][texture.m_Name]['channel_sha256'] = [hashlib.sha256(channel.tobytes()).hexdigest() for channel in channels]
            export = root / 'target/block-area-native-textures'
            export.mkdir(parents=True, exist_ok=True)
            texture.image.convert('RGBA').save(export / f'{texture.m_Name}.png')
            if texture.m_Name == 'FD_Noise_00000':
                import numpy as np
                from PIL import Image
                raw = texture.get_image_data()
                assert len(raw) == 256 * 256 * 2
                words = np.frombuffer(raw, dtype='<u2').reshape(256, 256)
                bits = np.stack([words >> 11, (words >> 5) & 63, words & 31], axis=-1)
                png = np.flipud(np.asarray(Image.open(root.parent / '_official_src/textures/FD_Noise_00000.png').convert('RGB')))
                # The shader samples RGBA8 through a mediump sampler before
                # recovering source channel bits. Check all texels, not a few.
                sample = (png.astype(np.float32) / 255.).astype(np.float16).astype(np.float32)
                recovered = np.floor(sample * [255 / 8, 255 / 4, 255 / 8] + 0.5).astype(np.uint16)
                if not np.array_equal(recovered, bits):
                    mismatches = np.argwhere(recovered != bits)
                    print('RGB565 recovery mismatches', len(mismatches), [(p.tolist(), int(recovered[tuple(p)]), int(bits[tuple(p)])) for p in mismatches[:8]])
                    np.save(export / 'FD_Noise_00000.words.npy', words)
                    raise AssertionError('PNG cannot recover original RGB565 texels')
                result['textures'][texture.m_Name]['rgb565_bit_recovery_matches_all_texels'] = True
                result['textures'][texture.m_Name]['source_png_expansion'] = 'channel_bits << (3,2,3), without UNorm bit replication'
                result['textures'][texture.m_Name]['native_rgb565_sha256'] = hashlib.sha256(raw).hexdigest()
                (export / 'FD_Noise_00000.rgb565').write_bytes(raw)
                # Match native RGB565 -> eight-bit UNorm fetch expansion. The
                # old exported PNG only shifted bits and darkened the texture.
                expanded = ((bits << [3, 2, 3]) | (bits >> [2, 4, 2])).astype(np.uint8)
                corrected = Image.fromarray(np.flipud(expanded))
                corrected.save(root / 'assets/blockarea/FD_Noise_00000.png')
                result['textures'][texture.m_Name]['corrected_asset_rgba_sha256'] = hashlib.sha256(corrected.convert('RGBA').tobytes()).hexdigest()
                result['textures'][texture.m_Name]['export_correction_max_rgb'] = np.max(expanded.astype(int) - png.astype(int), axis=(0, 1)).tolist()
destination = root / 'docs/block-area/official-asset-state.json'
assert set(result['shaders']) == selected, 'Missing original shaders'
destination.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
print(destination)

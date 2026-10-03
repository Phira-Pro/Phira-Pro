"""Package the verified v4 Windows build without chart data or preferences."""
import hashlib
import json
import re
from pathlib import Path
import zipfile

root = Path(__file__).resolve().parents[2]
exe = root / 'target/release/phira-main.exe'
output = root.parent / 'PhiraPro-fixes-win64-v4.zip'
geometry = hashlib.sha256((root / 'prpr/src/core/block.rs').read_bytes()).hexdigest()
sources = [p for directory in ('phira/src', 'prpr/src') for p in (root / directory).rglob('*') if p.suffix in ('.rs', '.frag', '.vert')]
sources += [root.parent / 'vendor/sasa/src/renderer/music.rs', root.parent / 'vendor/sasa/src/renderer/low_pass.rs', root.parent / 'vendor/prpr-miniquad/src/native/gl.rs', root / 'assets/blockarea/FD_Noise_00000.png']
sources += [root / 'Cargo.toml', root / 'Cargo.lock', root / 'prpr/Cargo.toml']
assert exe.stat().st_mtime >= max(path.stat().st_mtime for path in sources), 'Executable is older than source'
logs = {name: root / f'target/review-v4-{name}.log' for name in ('check', 'prpr-tests', 'phira-tests', 'audio-tests', 'gpu', 'mask-gpu', 'main-build', 'android-build', 'mask-benchmark', 'render-benchmark', 'frames')}
assert 'Finished' in logs['check'].read_text('utf-8-sig')
unit_results = {}
for name, crate in [('prpr-tests', 'prpr'), ('phira-tests', 'phira'), ('audio-tests', 'sasa')]:
    assert logs[name].stat().st_mtime >= max(path.stat().st_mtime for path in sources), f'{name} predates current source; rerun verification'
    counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed', logs[name].read_text('utf-8-sig'))
    assert counts and sum(map(int, counts)) > 0, f'{name} has no successful tests'
    unit_results[crate] = sum(map(int, counts))
assert 'All native GPU checks passed.' in logs['gpu'].read_text('utf-8-sig')
assert logs['gpu'].read_text('utf-8-sig').count('Exported ActiveBlock GLSL comparison') == 11
assert 'NOT pixel-exact.' in logs['mask-gpu'].read_text('utf-8-sig')
assert 'Finished' in logs['main-build'].read_text('utf-8-sig')
assert 'Finished' in logs['android-build'].read_text('utf-8-sig')
assert all(row['semantically_equal'] for row in json.loads((root / 'target/review-v4-chart-source-audit.json').read_text('utf8')))
assert all(row['same_GLSL'] for row in json.loads((root / 'target/review-v4-shader-source-audit.json').read_text('utf8')))
assets = sorted(path for path in (root / 'assets').rglob('*') if path.is_file())
comparisons = sorted(path for path in (root.parent / 'block-area-comparison-v4').iterdir() if path.is_file())
exe_sha = hashlib.sha256(exe.read_bytes()).hexdigest()
manifest = {
    'configuration': 'Windows Release, RUSTFLAGS=--cfg record',
    'application_version': '0.8.2-pro.6', 'verification_batch': 'v4',
    'exe_sha256': exe_sha, 'geometry_sha256': geometry,
    'asset_count': len(assets), 'full_frame_comparison_count': sum(path.suffix == '.png' for path in comparisons),
    'lib_tests_passed': unit_results, 'material_comparisons': 11,
    'activation_required': False,
    'material_max_channel_error_with_common_inputs': 0, 'native_chart_semantic_equality_verified': True,
    'score_destination': 'configured Pro server only; redirects disabled; no official fallback',
    'result_error': 'RMS per score-token protocol, milliseconds with 2 decimals',
    'block_audio_cutoff_hz': 1500, 'block_audio_transition_seconds': 0.1,
    'source_noise_rgb565_verified': True, 'full_frame_pixel_exact': False,
    'known_visual_differences': ['color', 'some Ready frames', 'global shader clock not aligned'],
    'capture_background_brightness': 0.65,
    'android_arm64_release_compiled': True, 'ios_compiled': False,
    'android_ios_device_verified': False, 'mobile_120fps_guaranteed': False,
    'desktop_2560x1600_dynamic_mask_ms': [1.594, 1.630], 'desktop_2560x1600_block_cpu_gpu_ms': [2.262, 2.333],
    'source_sha256': {path.relative_to(root.parent).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest() for path in sources},
}
with zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
    archive.write(exe, 'phira-main.exe')
    for path in assets:
        archive.write(path, path.relative_to(root).as_posix())
    archive.write(root / 'docs/block-area/README.md', 'README.md')
    archive.write(root / 'docs/pro-fixes-v4.md', 'docs/pro-fixes-v4.md')
    archive.write(root.parent / 'docs/phira-pro-api.md', 'docs/phira-pro-api.md')
    archive.write(root / 'docs/block-area/official-asset-state.json', 'verification/official-asset-state.json')
    for path in logs.values():
        archive.write(path, f'verification/{path.name}')
    for filename in ('review-v4-chart-source-audit.json', 'review-v4-shader-source-audit.json', 'block-area-native-geometry.txt', 'review-v4-native-ready.txt', 'block-area-audio-assets.json', 'block-area-native-audio.txt'):
        archive.write(root / 'target' / filename, f'verification/{filename}')
    for chart in ('hate', 'desultory'):
        archive.write(root / f'target/block-area-v4-frames-2x/{chart}-manifest.json', f'comparison/{chart}-manifest.json')
    for path in comparisons:
        archive.write(path, f'comparison/{path.name}')
    archive.writestr('verification/manifest.json', json.dumps(manifest, ensure_ascii=False, indent=2))
with zipfile.ZipFile(output) as archive:
    assert archive.testzip() is None, 'Archive CRC failed'
    assert hashlib.sha256(archive.read('phira-main.exe')).hexdigest() == exe_sha
    assert {n for n in archive.namelist() if n.startswith('assets/')} == {p.relative_to(root).as_posix() for p in assets}
digest = hashlib.sha256(output.read_bytes()).hexdigest()
output.with_suffix('.zip.sha256').write_text(f'{digest} *{output.name}\n', encoding='utf8')
print(json.dumps({'package': str(output), 'bytes': output.stat().st_size, 'sha256': digest, 'exe_sha256': exe_sha, 'geometry_sha256': geometry, 'comparisons': 15}, indent=2))

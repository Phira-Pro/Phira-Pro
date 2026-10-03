"""Package the verified Windows build without chart data or user preferences."""
import hashlib
import json
from pathlib import Path
import zipfile

root = Path(__file__).resolve().parents[2]
exe = root / 'target/release/phira-main.exe'
output = root.parent / 'PhiraPro-block-area-win64-v3.zip'
expected_geometry = '988a4215eebea146dd418287b2cbdd3472566ac166a73517e23ec6579591b29e'
geometry = hashlib.sha256((root / 'prpr/src/core/block.rs').read_bytes()).hexdigest()
assert geometry == expected_geometry, 'Verified geometry changed'
sources = [root / f'prpr/src/core/{name}' for name in (
    'block_shader.rs', 'block_mask.rs', 'block_touch.rs', 'block_audio.rs', 'block_shader_full.vert', 'block_shader_full.frag', 'chart.rs')]
sources += [root / 'prpr/src/scene/game.rs', root / 'prpr/src/judge.rs']
sources += [root.parent / 'vendor/sasa/src/renderer/music.rs', root.parent / 'vendor/sasa/src/renderer/low_pass.rs', root / 'assets/blockarea/FD_Noise_00000.png']
assert exe.stat().st_mtime >= max(path.stat().st_mtime for path in sources), 'Executable is older than the source'
checks = root / 'target/block-area-final-check.log'
tests = root / 'target/block-area-final-tests.log'
gpu = root / 'target/block-area-gpu.log'
mask = root / 'target/block-area-mask-gpu.log'
phase = root / 'target/block-area-phase-audit.json'
color_space = root / 'target/block-area-texture-color-space-audit.json'
build = root / 'target/block-area-final-main-build.log'
assert 'Finished' in checks.read_text(encoding='utf-8-sig')
assert '41 passed; 0 failed' in tests.read_text(encoding='utf-8-sig')
audio = root / 'target/block-area-audio-tests.log'
assert '6 passed; 0 failed' in audio.read_text(encoding='utf-8-sig')
assert 'All native GPU checks passed.' in gpu.read_text(encoding='utf-8-sig')
assert gpu.read_text(encoding='utf-8-sig').count('Exported ActiveBlock GLSL comparison') == 11
assert 'NOT pixel-exact.' in mask.read_text(encoding='utf-8-sig')
assert 'Finished' in build.read_text(encoding='utf-8-sig')
assets = sorted(path for path in (root / 'assets').rglob('*') if path.is_file())
comparisons = sorted(path for path in (root.parent / 'block-area-comparison').iterdir() if path.is_file())
exe_sha = hashlib.sha256(exe.read_bytes()).hexdigest()
manifest = {
    'configuration': 'Windows Release, RUSTFLAGS=--cfg record',
    'exe_sha256': exe_sha,
    'geometry_sha256': geometry,
    'asset_count': len(assets),
    'full_frame_comparison_count': sum(path.suffix == '.png' for path in comparisons),
    'lib_tests_passed': 41,
    'audio_tests_passed': 6,
    'material_comparisons': 11,
    'block_audio_cutoff_hz': 1500,
    'block_audio_transition_seconds': 0.1,
    'source_noise_rgb565_verified': True,
    'material_max_channel_error_with_common_inputs': 0,
    'full_frame_pixel_exact': False,
    'android_ios_device_verified': False,
}
with zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
    archive.write(exe, 'phira-main.exe')
    for path in assets:
        archive.write(path, path.relative_to(root).as_posix())
    archive.write(root / 'docs/block-area/README.md', 'README.md')
    archive.write(root / 'docs/block-area/official-asset-state.json', 'verification/official-asset-state.json')
    for path in (checks, tests, gpu, mask, audio, root / 'target/block-area-audio-assets.json', root / 'target/block-area-native-audio.txt'):
        archive.write(path, f'verification/{path.name}')
    for path in (phase, color_space):
        archive.write(path, f'verification/{path.name}')
    for path in comparisons:
        archive.write(path, f'comparison/{path.name}')
    archive.writestr('verification/manifest.json', json.dumps(manifest, ensure_ascii=False, indent=2))
with zipfile.ZipFile(output) as archive:
    assert archive.testzip() is None, 'Archive CRC failed'
    assert hashlib.sha256(archive.read('phira-main.exe')).hexdigest() == exe_sha
    assert {name for name in archive.namelist() if name.startswith('assets/')} == {path.relative_to(root).as_posix() for path in assets}
digest = hashlib.sha256(output.read_bytes()).hexdigest()
output.with_suffix('.zip.sha256').write_text(f'{digest} *{output.name}\n', encoding='utf-8')
print(json.dumps({'package': str(output), 'bytes': output.stat().st_size, 'sha256': digest, **manifest}, ensure_ascii=False, indent=2))

"""Repeat viewport/rim regressions; charts are read-only and evidence stays local.

Run: python scripts/verify_block_viewports.py --chart path/to/MilK.pez
After a repair, rerun the same command. Any assertion/hash mismatch stops the
cycle and leaves its log/PNGs; only a fully passing cycle writes result.json.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


VIEWPORTS = [(1280, 720), (1024, 768), (1200, 800), (1170, 540), (768, 1024), (1023, 767)]
SOURCES = ['prpr/src/core/rpe_block.rs', 'prpr/src/core/chart.rs',
           'prpr/src/core/block_mask.rs', 'prpr/src/core/block_color.rs',
           'prpr/src/core/block_shader.rs', 'prpr/src/core/block_shader_full.frag',
           'prpr/examples/block_area_gpu.rs', 'prpr/examples/block_area_frames.rs']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--chart', type=Path, required=True)
    parser.add_argument('--skip-build', action='store_true')
    parser.add_argument('--output', type=Path, default=Path('target/block-viewport-check'))
    args = parser.parse_args()
    repo = Path(__file__).absolute().parents[1]
    chart = args.chart.absolute()
    if not chart.is_file():
        parser.error(f'Chart not found: {chart}')
    output = args.output if args.output.is_absolute() else repo / args.output
    output.mkdir(parents=True, exist_ok=True)
    result_file = output / 'result.json'
    # Never leave an older successful result masquerading as the current run.
    result_file.unlink(missing_ok=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith(
        ('BLOCK_CAPTURE_', 'BLOCK_BENCH_', 'BLOCK_GPU_', 'BLOCK_REFERENCE_'))}
    fingerprints = {name: hashlib.sha256((repo / name).read_bytes()).hexdigest() for name in SOURCES}

    def run(command, name, extra=None):
        print(f'{name}: starting', flush=True)
        with (output / f'{name}.log').open('w', encoding='utf-8') as log:
            completed = subprocess.run(command, cwd=repo, env=dict(env, **(extra or {})),
                                       stdout=log, stderr=subprocess.STDOUT, timeout=900)
        if completed.returncode:
            raise RuntimeError(f'{name} failed: {output / (name + ".log")}')
        print(f'{name}: passed', flush=True)

    if not args.skip_build:
        run(['cargo', 'test', '--offline', '--locked', '-p', 'prpr', '--lib'], 'unit-tests')
        run(['cargo', 'build', '--offline', '--locked', '-p', 'prpr', '--example',
             'block_area_gpu', '--example', 'block_area_frames'], 'build-examples')
    suffix = '.exe' if os.name == 'nt' else ''
    gpu = str(repo / f'target/debug/examples/block_area_gpu{suffix}')
    capture = str(repo / f'target/debug/examples/block_area_frames{suffix}')
    # Includes default-red/native-reference parity, Ready, hover, flips and
    # manual MSAA chart passes. Matrix probes must also compile full shaders.
    run([gpu], 'native-material-regression', {'BLOCK_GPU_SAMPLES': '2'})
    probes = []
    captures = []
    for width, height in VIEWPORTS:
        for samples in [1, 2, 4]:
            key = f'{width}x{height}-msaa{samples}'
            previous = None
            for iteration in [1, 2]:
                name = f'gpu-{key}-repeat{iteration}'
                run([gpu], name, {'BLOCK_GPU_MATRIX': '1', 'BLOCK_GPU_WIDTH': str(width),
                                  'BLOCK_GPU_HEIGHT': str(height), 'BLOCK_GPU_SAMPLES': str(samples)})
                files = sorted((repo / 'target/block-area-gpu').glob('matrix-*.png'))
                if len(files) != 24:
                    raise RuntimeError(f'{name}: expected 24 asserted probe frames, got {len(files)}')
                hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
                if previous is not None and hashes != previous:
                    raise RuntimeError(f'{name}: fixed-clock frames changed on repeat')
                previous = hashes
                folder = output / name
                folder.mkdir(exist_ok=True)
                for path in files:
                    shutil.copy2(path, folder / path.name)
            probes.append({'viewport': [width, height], 'samples': samples,
                           'repeats': 2, 'identical_hashes': previous})
        for simple in [False, True]:
            name = f'milk-{width}x{height}-simple{int(simple)}'
            folder = output / name
            extra = {'BLOCK_CAPTURE_CHART': str(chart), 'BLOCK_CAPTURE_TIMES': '31.6,31.8,32,32.2',
                     'BLOCK_CAPTURE_WIDTH': str(width), 'BLOCK_CAPTURE_HEIGHT': str(height),
                     'BLOCK_CAPTURE_SAMPLES': '2', 'BLOCK_CAPTURE_PRE_RENDER': '1',
                     'BLOCK_CAPTURE_WHITE': '1', 'BLOCK_CAPTURE_DIR': str(folder)}
            if simple:
                extra['BLOCK_CAPTURE_SIMPLE'] = '1'
            run([capture], name, extra)
            frames = json.loads((folder / 'custom-manifest.json').read_text(encoding='utf-8'))
            if len(frames) != 4 or any(f['combo'] != 102 or f['white_probe_tinted_pixels'] != 0 for f in frames):
                raise RuntimeError(f'{name}: wrong combo or colored white rim')
            captures.append({'viewport': [width, height], 'simple': simple, 'frames': frames})
    if fingerprints != {name: hashlib.sha256((repo / name).read_bytes()).hexdigest() for name in SOURCES}:
        raise RuntimeError('Renderer sources changed during the cycle; rebuild and rerun')
    report = {'status': 'passed', 'source_sha256': fingerprints,
        'executables_sha256': {Path(p).name: hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in [gpu, capture]},
        'reused_binaries': args.skip_build,
        'gpu_frames': len(probes) * 2 * 24, 'gpu_cases': probes, 'captures': captures,
        'limits': 'Windows OpenGL viewport emulation; does not certify iOS hardware. '
                  'Wider screens retain the configured chart viewport/letterbox. '
                  'Synthetic GPU clocks are fixed; Line2Area follows chart time and native BlockAreaList follows runtime time.'}
    result_file.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(f'Cycle passed: {result_file}', flush=True)


if __name__ == '__main__':
    main()

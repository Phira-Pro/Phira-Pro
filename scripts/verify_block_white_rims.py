"""Repeat the native GPU rim regression and capture MilK's combo-102 interval.

Run from any directory with a local MilK .pez supplied via --chart. Charts are
read-only; results stay in target and no preferences or scores are saved.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--chart', type=Path, required=True)
    parser.add_argument('--skip-build', action='store_true', help='Reuse already-built diagnostic executables')
    args = parser.parse_args()
    chart = args.chart.absolute()
    if not chart.is_file():
        parser.error(f'Chart not found: {chart}')
    repo = Path(__file__).absolute().parents[1]
    output = repo / 'target/block-white-rim-check'
    output.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    # The capture/probe must not inherit benchmark or simple-mode switches.
    env = {k: v for k, v in env.items() if not k.startswith(('BLOCK_CAPTURE_', 'BLOCK_BENCH_', 'BLOCK_GPU_', 'BLOCK_REFERENCE_'))}

    def run(command, name, environment=env):
        print(f'{name}: starting', flush=True)
        with (output / f'{name}.log').open('w', encoding='utf-8') as log:
            result = subprocess.run(command, cwd=repo, env=environment, stdout=log, stderr=subprocess.STDOUT)
        if result.returncode:
            raise RuntimeError(f'{name} failed ({result.returncode}); see {output / (name + ".log")}')
        print(f'{name}: passed', flush=True)

    if not args.skip_build:
        run(['cargo', 'test', '--offline', '--locked', '-p', 'prpr', '--lib'], 'unit-tests')
        run(['cargo', 'build', '--offline', '--locked', '-p', 'prpr', '--example', 'block_area_gpu', '--example', 'block_area_frames'], 'build-examples')
    suffix = '.exe' if os.name == 'nt' else ''
    probe = str(repo / f'target/debug/examples/block_area_gpu{suffix}')
    first = None
    gpu_env = dict(env, BLOCK_GPU_SAMPLES='2')
    for iteration in range(2):
        run([probe], f'gpu-{iteration + 1}', gpu_env)
        files = sorted((repo / 'target/block-area-gpu').glob('white-inverted-holes-*.png'))
        if len(files) != 6:
            raise RuntimeError('Expected six white rim probe frames')
        hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
        if first is not None and hashes != first:
            raise RuntimeError('Fixed shader times produced different white rim frames on repeat')
        first = hashes
    capture_env = dict(env, BLOCK_CAPTURE_CHART=str(chart), BLOCK_CAPTURE_TIMES='31.6,31.8,32,32.2',
                       BLOCK_CAPTURE_WIDTH='1280', BLOCK_CAPTURE_HEIGHT='720', BLOCK_CAPTURE_SAMPLES='2',
                       BLOCK_CAPTURE_PRE_RENDER='1', BLOCK_CAPTURE_DIR=str(output / 'frames'))
    run([str(repo / f'target/debug/examples/block_area_frames{suffix}')], 'capture-combo102', capture_env)
    frames = json.loads((output / 'frames/custom-manifest.json').read_text(encoding='utf-8'))
    if len(frames) != 4 or any(row['combo'] != 102 for row in frames):
        raise RuntimeError('Capture chart did not reproduce the expected combo-102 interval')
    report = {'gpu_iterations': 2, 'white_probe_frames': first, 'repeated_probe_hashes': 'identical',
              'chart': str(chart), 'capture_frames': frames,
              'note': 'GPU probes assert neutral white rims at fixed clocks; GameScene PNGs are visual evidence and use the live shader clock.'}
    (output / 'result.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(f'Passed; report: {output / "result.json"}', flush=True)


if __name__ == '__main__':
    main()

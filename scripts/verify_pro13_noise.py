"""Read-only real-chart regression: canvas geometry, lifecycle and repeatable GPU frames.

Use --brain <chart directory> --milk <pez>. Recorder observations are archived
separately; this is not a claim of pixel parity with its compressed video.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]
VIEWS = [(1280, 720), (1024, 768), (1200, 800), (1170, 540), (768, 1024), (1023, 767)]


def beat(value):
    return value[0] + value[1] / value[2]


def easing(kind, x):
    # Analytic RPE easing definitions; deliberately independent of game code.
    bases = {
        'sine': lambda t: 1 - math.cos(t * math.pi / 2),
        'quad': lambda t: t ** 2, 'cubic': lambda t: t ** 3,
        'quart': lambda t: t ** 4, 'quint': lambda t: t ** 5,
        'expo': lambda t: 2 ** (10 * (t - 1)),
        'circ': lambda t: 1 - math.sqrt(max(0, 1 - t * t)),
        'back': lambda t: 2.70158 * t ** 3 - 1.70158 * t ** 2,
    }
    modes = [None, None, ('sine', 'out'), ('sine', 'in'),
             ('quad', 'out'), ('quad', 'in'), ('sine', 'both'), ('quad', 'both'),
             ('cubic', 'out'), ('cubic', 'in'), ('quart', 'out'), ('quart', 'in'),
             ('cubic', 'both'), ('quart', 'both'), ('quint', 'out'), ('quint', 'in'),
             ('expo', 'out'), ('expo', 'in'), ('circ', 'out'), ('circ', 'in'),
             ('back', 'out'), ('back', 'in'), ('circ', 'both'), ('back', 'both')]
    if kind in (0, 1):
        return x
    if kind >= len(modes):
        raise ValueError(f'Unsupported oracle easing {kind}; add an explicit definition')
    name, mode = modes[kind]
    f = bases[name]
    if mode == 'in':
        return f(x)
    if mode == 'out':
        return 1 - f(1 - x)
    return f(2 * x) / 2 if x < .5 else 1 - f(2 - 2 * x) / 2


def value(events, time, default=0):
    events = sorted(events or [], key=lambda e: beat(e['startTime']))
    candidates = [e for e in events if beat(e['startTime']) <= time + 1e-8]
    if not candidates:
        return default
    e = candidates[-1]
    a, b = beat(e['startTime']), beat(e['endTime'])
    if time >= b - 1e-8 or a == b:
        return e['end']
    if abs(time - a) < 1e-8 or e['start'] == e['end']:
        return e['start']
    if e.get('bezier', 0):
        raise ValueError('Bezier segments require a separate oracle')
    left, right = e.get('easingLeft', 0), e.get('easingRight', 1)
    kind = e.get('easingType', 1)
    low, high = easing(kind, left), easing(kind, right)
    progress = (easing(kind, left + (right - left) * (time - a) / (b - a)) - low) / (high - low)
    return e['start'] + (e['end'] - e['start']) * progress


def chart_data(path):
    if path.is_dir():
        # The supplied Brainrot directory contains one chart JSON.
        files = [p for p in path.glob('*.json') if 'judgeLineList' in p.read_text('utf-8')[:2000]]
        if len(files) != 1:
            raise ValueError('Expected one RPE chart in fixture directory')
        return json.loads(files[0].read_text('utf-8')), lambda name: (path / name).read_bytes()
    archive = zipfile.ZipFile(path)
    name = next(n for n in archive.namelist() if n.endswith('.json') and 'judgeLineList' in archive.read(n).decode('utf-8')[:2000])
    return json.loads(archive.read(name)), archive.read


def oracle(path, seconds):
    data, read = chart_data(path)
    assert len(data['BPMList']) == 1, 'These reference fixtures use a constant BPM'
    t = seconds * data['BPMList'][0]['bpm'] / 60
    lines = data['judgeLineList']

    def track(line, key):
        return sum(value(layer.get(key), t) for layer in line['eventLayers'] if layer)

    def rotate(x, y, degrees):
        a = math.radians(degrees)
        return math.cos(a) * x - math.sin(a) * y, math.sin(a) * x + math.cos(a) * y

    def pose(index):
        line = lines[index]
        x, y, angle = track(line, 'moveXEvents'), track(line, 'moveYEvents'), -track(line, 'rotateEvents')
        parent = line.get('father', -1)
        if parent >= 0:
            px, py, pa = pose(parent)
            x, y = rotate(x, y, pa)
            x, y = x + px, y + py
            if line.get('rotateWithFather', False):
                angle += pa
        return x, y, angle

    result = []
    for i, line in sorted(enumerate(lines), key=lambda pair: (pair[1].get('zOrder', 0), pair[0])):
        if Path(line.get('Texture', '')).name.lower() not in ['issubtract0.png', 'issubtract1.png']:
            continue
        codes = sorted((beat(e['startTime']), e['start']) for layer in line['eventLayers'] if layer
                       for e in layer.get('speedEvents', []) if e['start'] in [1, 2, 3, 4])
        past = [code for at, code in codes if at <= t + 1e-8]
        if not past or past[-1] == 4:
            continue
        width, height = struct.unpack('>II', read(line['Texture'])[16:24])
        ext = line.get('extended', {})
        sx, sy = value(ext.get('scaleXEvents'), t, 1), value(ext.get('scaleYEvents'), t, 1)
        if abs(sx) < 1e-6 or abs(sy) < 1e-6:
            continue
        ax, ay = line.get('anchor', [.5, .5])
        x, y, angle = pose(i)
        cx, cy = rotate((.5 - ax) * width * sx, (.5 - ay) * height * sy, angle)
        corners = []
        for dx, dy in [(-1, -1), (1, -1), (1, 1), (-1, 1)]:
            ox, oy = rotate(dx * abs(width * sx) / 2, dy * abs(height * sy) / 2, angle)
            corners.append([x + cx + ox, y + cy + oy])
        result.append((i, corners))
    return result


def inspect(path, rows):
    for row in rows:
        expected = oracle(path, row['chart_seconds'])
        actual = row['zones']
        assert len(expected) == len(actual), (row['file'], len(expected), len(actual))
        aspect = row['width'] / row['height']
        # GameScene may letterbox a wide window to its configured chart aspect.
        # The evaluated y-scale contains the actual chart viewport aspect.
        for (line, corners), zone in zip(expected, actual):
            if zone['line2area']:
                aspect = 1.5 / zone['y_scale']
            c, s = math.cos(zone['angle']), math.sin(zone['angle'])
            cx, cy = zone['center']
            hx, hy = zone['half']
            for (dx, dy), wanted in zip([(-1, -1), (1, -1), (1, 1), (-1, 1)], corners):
                px = (cx + c * dx * hx - s * dy * hy) * 675
                py = (cy + (s * dx * hx + c * dy * hy) * zone['y_scale']) * 450 * aspect
                error = max(abs(px - wanted[0]), abs(py - wanted[1]))
                assert error < .01, (row['file'], line, error, [px, py], wanted)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--brain', type=Path, required=True)
    parser.add_argument('--milk', type=Path, required=True)
    parser.add_argument('--skip-build', action='store_true')
    args = parser.parse_args()
    out = ROOT / 'target/pro13-noise-check'
    out.mkdir(exist_ok=True)
    report = out / 'result.json'
    report.unlink(missing_ok=True)
    sources = sorted((ROOT / 'prpr/src/core').glob('block*')) + [ROOT / 'prpr/src/core/rpe_block.rs', ROOT / 'prpr/src/core/chart.rs', ROOT / 'prpr/examples/block_area_frames.rs']
    fingerprints = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources if p.is_file()}
    env = {k: v for k, v in os.environ.items() if not k.startswith(('BLOCK_CAPTURE_', 'BLOCK_BENCH_', 'BLOCK_GPU_', 'BLOCK_REFERENCE_'))}

    def run(command, name, extra=None):
        print(name, flush=True)
        with (out / (name + '.log')).open('w', encoding='utf-8') as log:
            subprocess.run(command, cwd=ROOT, env=dict(env, **(extra or {})), stdout=log, stderr=subprocess.STDOUT, timeout=900, check=True)

    if not args.skip_build:
        run(['cargo', 'build', '--offline', '-p', 'prpr', '--no-default-features', '--example', 'block_area_frames', '--example', 'block_area_gpu', '--example', 'block_area_mask_gpu'], 'build')
    run([str(ROOT / 'target/debug/examples/block_area_gpu.exe')], 'native-gpu')
    run([str(ROOT / 'target/debug/examples/block_area_mask_gpu.exe')], 'native-mask-gpu')
    cases = []
    for width, height in VIEWS:
        for name, path, times in [('brain', args.brain.absolute(), '0.5,1,1.75,2,2.25,4,4.25,6.75,7,21.5,22,23,23.5'),
                                  ('milk-opening', args.milk.absolute(), '3.2,4,4.8,5.6')]:
            previous = None
            for iteration in [1, 2]:
                label = f'{name}-{width}x{height}-repeat{iteration}'
                folder = out / label
                run([str(ROOT / 'target/debug/examples/block_area_frames.exe')], label, {
                    'BLOCK_CAPTURE_CHART': str(path), 'BLOCK_CAPTURE_TIMES': times,
                    'BLOCK_CAPTURE_WIDTH': str(width), 'BLOCK_CAPTURE_HEIGHT': str(height),
                    'BLOCK_CAPTURE_PRE_RENDER': '1', 'BLOCK_CAPTURE_SAMPLES': '2', 'BLOCK_CAPTURE_DIR': str(folder)})
                rows = json.loads((folder / 'custom-manifest.json').read_text('utf-8'))
                inspect(path, rows)
                hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.glob('*.png')}
                if previous is not None:
                    assert hashes == previous, (label, 'identical chart time produced different GPU pixels')
                previous = hashes
            cases.append({'chart': name, 'viewport': [width, height], 'frames_per_repeat': len(rows), 'repeats': 2, 'hashes': previous})
    assert fingerprints == {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in fingerprints}, 'Sources changed during verification'
    report.write_text(json.dumps({'status': 'passed', 'source_sha256': fingerprints, 'cases': cases,
        'limits': 'Windows OpenGL and simulated viewports, not iOS/Android hardware certification; geometry oracle samples actual chart events in authoring pixels.'}, indent=2), encoding='utf-8')
    print(report, flush=True)


if __name__ == '__main__':
    main()

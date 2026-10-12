"""Check real GameScene imports, geometry and deterministic official rendering.

Run verify_rpe_next.mjs first. Requires Pillow and desktop OpenGL/audio.
No user chart/data is modified; all logs and screenshots stay under target.
"""
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/rpe-next-verification'
VIEWS = [(1280,720),(1024,768),(1200,800),(1170,540),(768,1024),(1023,767)]


def main():
    result = OUT / 'result.json'
    result.unlink(missing_ok=True)
    env = {k:v for k,v in os.environ.items() if not k.startswith(('BLOCK_CAPTURE_','BLOCK_BENCH_','BLOCK_REFERENCE_'))}
    env['RUSTFLAGS'] = '--cfg record'
    env['CARGO_NET_OFFLINE'] = 'true'
    env['TEMP'] = env['TMP'] = str(ROOT / 'target/test-temp')
    reference = json.loads((OUT/'visual-samples.json').read_text())
    times = ','.join(str(s['time']) for s in reference)
    subprocess.run(['cargo','build','-p','prpr','--example','block_area_frames','--no-default-features'],cwd=ROOT,env=env,check=True)
    binary = ROOT/'target/debug/examples'/('block_area_frames.exe' if os.name=='nt' else 'block_area_frames')
    checks = []
    for w,h in VIEWS:
        hashes = {}
        for source in ['directory','pez']:
            for repeat in [0,1]:
                folder = OUT/f'frames-{w}x{h}-{source}-{repeat}'
                current = dict(env, BLOCK_CAPTURE_CHART=str(OUT if source=='directory' else OUT/'noise.pez'),
                    BLOCK_CAPTURE_TIMES=times,BLOCK_CAPTURE_WIDTH=str(w),BLOCK_CAPTURE_HEIGHT=str(h),BLOCK_CAPTURE_DIR=str(folder))
                with (OUT/f'frames-{w}x{h}-{source}-{repeat}.log').open('w',encoding='utf-8') as log:
                    subprocess.run([str(binary)],cwd=ROOT,env=current,stdout=log,stderr=subprocess.STDOUT,check=True)
                manifest = json.loads((folder/'custom-manifest.json').read_text())
                for row, expected in zip(manifest,reference,strict=True):
                    image = Image.open(row['file']).convert('RGB')
                    digest = hashlib.sha256(image.tobytes()).hexdigest()
                    if row['chart_seconds'] in hashes:
                        assert hashes[row['chart_seconds']] == digest, 'JSON/PEZ or repeat render differs'
                    else:
                        hashes[row['chart_seconds']] = digest
                    zones = row['next_zones']
                    assert len(zones)==len(expected['areas'])
                    vx,vy,vw,vh = row['viewport']
                    for z,a in zip(zones,expected['areas'],strict=True):
                        assert z['state']=={'disabled':0,'ready':1,'active':2}[a['state']]
                        assert abs(z['mix']-a['mix'])<1e-6
                        c,s = math.cos(z['angle']),math.sin(z['angle'])
                        points=[]
                        for dx,dy in [(-1,-1),(1,-1),(1,1),(-1,1)]:
                            x,y = dx*z['half'][0],dy*z['half'][1]
                            points.append((z['center'][0]+c*x-s*y,z['center'][1]+(s*x+c*y)*z['y_scale']))
                        target=[(p['x']*2/1350,p['y']*2/(900*row['aspect'])) for p in a['points']]
                        for p in target:
                            assert min(math.dist(p,q) for q in points)<2e-5
                    # Official textures deliberately displace boundaries and
                    # add refraction/glow. Do not compare them to flat polygons
                    # or the editor's placeholder colors. The separate
                    # BLOCK_GPU_NEXT probe compares all phases to the native
                    # material and the extracted official shader reference.
                    if any(z['state']==2 for z in zones):
                        crop=image.crop((vx,int(h*.2),vx+vw,int(h*.84)))
                        red=[r for r,g,b in crop.getdata() if r>180 and r>g*1.5 and r>b*1.5]
                        assert len(red)>20, 'official bright noise edge missing'
                checks.append({'viewport':[w,h],'source':source,'repeat':repeat,'frames':len(manifest)})
                print(f'{w}x{h} {source} repeat {repeat}: geometry, official edges and hashes passed',flush=True)
    files=['prpr/src/core/rpe_next_block.rs','prpr/src/core/block_next_mask.rs','prpr/src/core/block_mask.rs','prpr/src/core/block_shader.rs','prpr/src/core/chart.rs','prpr/src/parse/rpe.rs']
    result.write_text(json.dumps({'checks':checks,'frames':sum(c['frames'] for c in checks),
        'source_sha256':{p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in files}},indent=2),encoding='utf-8')
    print(f'PASS: {sum(c["frames"] for c in checks)} frames; {result}')


if __name__=='__main__':
    main()

"""Test raw versus sRGB-decoded displacement as hypotheses, not production fixes.

Requires block_area_phase_audit's camera sources and difference images. Geometry
is always the production transform. Video RGB classification is heuristic and
includes glow; these costs cannot prove the video's Unity color-space setting.
"""
import json
from pathlib import Path
import numpy as np
from PIL import Image

root = Path(__file__).resolve().parents[2]
reports = json.loads((root / 'target/block-area-phase-audit.json').read_text())
raw = np.asarray(Image.open(root / 'assets/blockarea/BlockNoise1.png').convert('RGB'))[::-1,:,0].astype(np.float32)/255
decoded = np.where(raw <= .04045, raw / 12.92, ((raw+.055)/1.055)**2.4).astype(np.float32)
x,y = np.meshgrid((np.arange(240,dtype=np.float32)+.5)/240,(np.arange(180,dtype=np.float32)+.5)/180)
d = np.float32(.5/np.sqrt(np.float32(.5)))

def sample(texture, u, v):
    u,v = np.remainder(u,2), np.remainder(v,2)
    u,v = np.where(u>1,2-u,u), np.where(v>1,2-v,v)
    return texture[np.minimum((v*256).astype(int),255), np.minimum((u*256).astype(int),255)]

def compose(texture, source, clock):
    t = np.float32(clock)/np.float32(20)*np.float32(2.59)
    a = sample(texture, d*t+x*np.float32(2.13), d*t+y*np.float32(1.02))-.5
    b = sample(texture,-d*t+x*np.float32(2.13), d*t+y*np.float32(1.02))-.5
    u = np.clip(np.floor(((d*a+b*-d)*np.float32(.1)+x)*240).astype(int),0,239)
    v = np.clip(np.floor(((d*a+b*d)*np.float32(.1)+y)*180).astype(int),0,179)
    n = source[v,u,0].astype(np.float32)/255
    s = source[v,u,1].astype(np.float32)/255
    return np.abs(n-((s>=.09)&(s<.12)))>.5

result = []
for report in reports:
    name = report['chart']
    frames=[]
    for row in report['per_frame']:
        at = row['chart_seconds']
        source = np.fromfile(root/f'target/block-area-phase-{name}-{at:.5f}.sources',dtype=np.uint8).reshape(180,240,4)
        comparison = np.asarray(Image.open(root/f'target/block-area-phase-{name}-{at:.5f}.png'))[::-1]
        valid = np.any(comparison != [96,96,96],axis=2)
        observed = (comparison[:,:,1]==180)|(comparison[:,:,2]==255)
        frames.append((at,source,valid,observed))
    for mode, texture in [('raw',raw),('srgb_decode_hypothesis',decoded)]:
        scores=[]
        for offset in np.arange(0,report['compose_period_seconds'],.05):
            costs=[int(np.count_nonzero((compose(texture,source,at+offset)!=obs)&valid)) for at,source,valid,obs in frames]
            scores.append((sum(costs),float(offset),costs))
        cost,offset,frame_costs = min(scores,key=lambda v:v[0])
        row={'chart':name,'sample_hypothesis':mode,'coarse_offset_seconds':offset,'joint_mismatch_cells':cost,'per_frame_costs':frame_costs,'not_ground_truth':True}
        result.append(row)
        print(row)
        for (at,source,valid,observed) in frames:
            predicted=compose(texture,source,at+offset)
            image=np.zeros((180,240,3),dtype=np.uint8)
            image[predicted&observed]=[0,180,0]
            image[predicted&~observed]=[255,0,0]
            image[~predicted&observed]=[0,80,255]
            image[~valid]=[96,96,96]
            Image.fromarray(image[::-1]).save(root/f'target/block-area-{mode}-{name}-{at:.5f}.png')
(root/'target/block-area-texture-color-space-audit.json').write_text(json.dumps(result,indent=2),encoding='utf-8')

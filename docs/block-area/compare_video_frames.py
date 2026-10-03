"""Create labeled full-frame comparison sheets without cropping either image.

Both sources remain untouched. Captures use the actual production GameScene.
1920x1440 captures are BOX-downsampled for this 960x720 video comparison only;
the original video rendering resolution and Unity startup clock are unknown.
"""
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

root = Path(__file__).resolve().parents[2]
out = root.parent / 'block-area-comparison-v4'
out.mkdir(exist_ok=True)
font = ImageFont.truetype(str(root / 'assets/harmonyos.ttf'), 22)
for chart, times in [('desultory', [65, 67, 67.72277, 70, 71.8]), ('hate', [79.2, 79.46667, 80.2, 81.9, 82.2, 134.96667, 137, 138.8, 139.8, 141.8])]:
    meta = json.loads((root / f'target/block-area-reference/{chart}/metadata.json').read_text(encoding='utf-8'))
    manifest = json.loads((root / f'target/block-area-v4-frames-2x/{chart}-manifest.json').read_text(encoding='utf-8'))
    for at in times:
        ref = min(meta['extraction'], key=lambda r: abs(r['requested_video_seconds']-at))
        cap = min(manifest, key=lambda r: abs(r['requested_video_seconds']-at))
        a = Image.open(root / ref['file']).convert('RGB')
        b = Image.open(root / cap['file']).convert('RGB')
        original_size = b.size
        b = b.resize(a.size, Image.Resampling.BOX)
        sheet = Image.new('RGB', (a.width*2, a.height+120), '#172027')
        draw = ImageDraw.Draw(sheet)
        pts = ref.get('decoded_frame_pts_seconds')
        draw.text((12, 8), f'官方录像 / PTS {pts:.6f}s', font=font, fill='white')
        draw.text((a.width+12, 8), f'Phira Pro / 谱面 {cap["chart_seconds"]:.6f}s', font=font, fill='white')
        draw.text((12, 41), f'{chart} · v4 · Phigros 4.0.1；捕获 {original_size[0]}×{original_size[1]}；背景亮度 65%', font=font, fill='#c6d3db')
        draw.text((12, 74), f'噪声时钟：官方未知；Phira 约 {cap["shader_clock_seconds_approx"]:.2f}s；当前未校准', font=font, fill='#c6d3db')
        sheet.paste(a,(0,120)); sheet.paste(b,(a.width,120))
        file = out / f'{chart}-{at:.5f}.png'
        sheet.save(file)
        print(file)
(out / 'README.txt').write_text('左右分别为用户提供的 Phigros 4.0.1 录像和实际 GameScene 渲染；不裁剪。用户确认录像背景亮度约 60%–70%，本轮捕获取 65%（Phira backgroundDim=0.35）。谱面时刻按解码 PTS 对齐，但 Unity 启动时钟和源设备渲染分辨率未知，双方 UI、粒子和独立谱面后处理也不同。Phira 噪声时钟为渲染后的近似读数。当前色调以及部分 Ready 帧仍有明显差异，不能排除剩余实现错误；这些图不能作为逐像素一致的证明。GPU 数学对照与误差记录另见工程 docs/block-area/README.md。', encoding='utf-8')

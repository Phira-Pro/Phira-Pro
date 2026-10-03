"""Create labeled full-frame comparison sheets without cropping either image.

Both sources remain untouched. Captures use the actual production GameScene.
1920x1440 captures are BOX-downsampled for this 960x720 video comparison only;
the original video rendering resolution and Unity startup clock are unknown.
"""
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

root = Path(__file__).resolve().parents[2]
out = root.parent / 'block-area-comparison'
out.mkdir(exist_ok=True)
font = ImageFont.truetype(str(root / 'assets/harmonyos.ttf'), 22)
for chart, times in [('desultory', [65, 67, 67.72277, 70, 71.8]), ('hate', [134.96667, 137, 138.8, 139.8, 141.8])]:
    meta = json.loads((root / f'target/block-area-reference/{chart}/metadata.json').read_text(encoding='utf-8'))
    manifest = json.loads((root / f'target/block-area-frames-2x/{chart}-manifest.json').read_text(encoding='utf-8'))
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
        draw.text((12, 41), f'{chart} · v3 · 录像 960×720；捕获 {original_size[0]}×{original_size[1]} 后缩小', font=font, fill='#c6d3db')
        draw.text((12, 74), f'噪声时钟：官方未知；Phira 约 {cap["shader_clock_seconds_approx"]:.2f}s；当前未校准', font=font, fill='#c6d3db')
        sheet.paste(a,(0,120)); sheet.paste(b,(a.width,120))
        file = out / f'{chart}-{at:.5f}.png'
        sheet.save(file)
        print(file)
(out / 'README.txt').write_text('左右分别为用户提供的录像和实际 GameScene 渲染；不裁剪。谱面时刻按解码 PTS 对齐，但 Unity 启动时钟、游戏版本、源设备渲染分辨率与背景亮度未确认，双方 UI 和粒子也不同。Phira 噪声时钟为渲染后的近似读数。当前色调差异明显，不能排除剩余实现错误；这些图只用于检查结构与遮挡，不能作为逐像素一致的证明。GPU 数学对照与误差记录另见工程 docs/block-area/README.md。', encoding='utf-8')

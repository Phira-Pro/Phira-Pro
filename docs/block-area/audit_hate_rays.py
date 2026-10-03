"""Inspect native block event data around the reported 480-combo section."""
import json
import re
import html
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
doc = (ROOT / 'docs/block-area/phira-official-block-area.html').read_text(encoding='utf8')
doc = doc.split('<main>')[1].split('</main>')[0]
doc = html.unescape(re.sub('<[^>]+>', ' ', doc))
(ROOT / 'docs/block-area/phira-official-block-area.txt').write_text(doc, encoding='utf8')
chart_path = ROOT / 'data/charts/custom/19d0e386-5929-4031-85fb-28ac8e6a472f/ハテ.rNFrums.0.json'
chart = json.loads(chart_path.read_text(encoding='utf8'))
notes = sorted((n['time'] * 60 / line['bpm'] / 32, n['type'], n['positionX'])
               for line in chart['judgeLineList'] for key in ('notesAbove', 'notesBelow') for n in line[key])
print('format', chart['formatVersion'], 'offset', chart.get('offset'), 'notes', len(notes), 'blocks', len(chart['blockAreaList']))
for combo in (470, 480, 490, 500, 510, 1044):
    print('combo', combo, 'seconds', notes[combo-1][0])
t = notes[479][0]
visible = [(i, b) for i, b in enumerate(chart['blockAreaList']) if b['appearTime'] <= t < b['disappearTime']]
print('visible at 480:', len(visible))
for i, b in visible:
    print(i, json.dumps(b, ensure_ascii=False))

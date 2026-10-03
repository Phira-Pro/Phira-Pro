"""Compare downloaded JSON with charts extracted from the original 4.0.1 bundles."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[2]
pairs = [
    ('hate', 'data/charts/custom/19d0e386-5929-4031-85fb-28ac8e6a472f/ハテ.rNFrums.0.json',
     'CAB-f4dfcd0bf1cdd43a1cfa5f8e1ee3816a--8314755070122294300-Chart_AT.json'),
    ('desultory', 'data/charts/custom/f681f94e-57d3-4d7c-bfd6-fb8cc3f1dd13/DesultorySignals.technoplanet.0.json',
     'CAB-8426a7bb8f9c83f8a08c5d2dab48a49c-8575435333041100506-Chart_AT.json'),
]
results = []
for name, local, native in pairs:
    a = json.loads((root / local).read_text('utf8'))
    b = json.loads((root / 'target/block-area-source-charts' / native).read_text('utf8'))
    assert a == b, f'{name}: chart content differs from native source'
    canonical = json.dumps(a, sort_keys=True, separators=(',', ':'), ensure_ascii=False)
    result = dict(chart=name, source=native, semantically_equal=True,
                  block_count=len(a['blockAreaList']),
                  semantic_sha256=hashlib.sha256(canonical.encode()).hexdigest())
    results.append(result)
    print(json.dumps(result))
(root / 'target/review-v4-chart-source-audit.json').write_text(json.dumps(results, indent=2), 'utf8')

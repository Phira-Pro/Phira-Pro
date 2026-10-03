"""Read original IL2CPP animation methods, including easing/keyframe selection."""
import json
import struct
import sys
from pathlib import Path
from audit_native_audio import binary, offset, methods, names, disassembler

prefixes = ('PreviewBlockControl$$Interpolate', 'PreviewBlockControl$$UpdateBlockAnimations', 'PreviewBlockControl$$<UpdateBlockAnimations>',
            'PreviewBlockControl$$UpdateMovement', 'PreviewBlockControl$$CalculateEasedProgress',
            'PreviewBlockControl$$SafeDiv', 'PreviewBlockControl$$GetBlockGeometry',
            'PreviewBlockControl$$FindCurrentEventIndex', 'GetEase$$GetEaseWithProgress')
lines = []
for i, method in enumerate(methods):
    if not any(prefix in method['Name'] for prefix in prefixes):
        continue
    start, end = method['Address'], methods[i+1]['Address']
    lines.append(f"\n{method['Name']} {start:#x}")
    for ins in disassembler.disasm(binary[offset(start):offset(end)], start):
        extra = ''
        if ins.mnemonic in ('b','bl') and ins.op_str.startswith('#'):
            extra = ' ; ' + names.get(int(ins.op_str[1:],0), '')
        lines.append(f'{ins.address:#x} {ins.mnemonic:8} {ins.op_str}{extra}')
destination = Path(__file__).resolve().parents[2] / 'target/block-area-native-geometry.txt'
destination.write_text('\n'.join(lines), encoding='utf8')
print(destination)

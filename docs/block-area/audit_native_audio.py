"""Read-only native low-pass control audit; writes evidence under target."""
import json
import struct
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(root / 'phira/target/block-area-tools'))
from capstone import Cs, CS_ARCH_ARM64, CS_MODE_ARM

binary = (root / '_official_src/libil2cpp.so').read_bytes()
ph_offset = struct.unpack_from('<Q', binary, 32)[0]
ph_size, ph_count = struct.unpack_from('<HH', binary, 54)
segments = [struct.unpack_from('<IIQQQQQQ', binary, ph_offset + i * ph_size) for i in range(ph_count)]
def offset(address):
    for kind, _, start, vaddr, _, size, _, _ in segments:
        if kind == 1 and vaddr <= address < vaddr + size:
            return start + address - vaddr
    raise ValueError(hex(address))

methods = json.loads((root / '_official_src/ida/script.json').read_text())['ScriptMethod']
methods.sort(key=lambda m: m['Address'])
names = {m['Address']: m['Name'] for m in methods}
selected = {'JudgeControl$$UpdateLowPassFilterState', 'ProgressControl$$SetLowPassFilter',
            'ProgressControl.<LerpLowPassFilter>d__54$$MoveNext', 'ProgressControl$$.ctor',
            'ProgressControl$$Start', 'LevelControl$$.ctor', 'LevelControl$$Awake',
            'LevelControl.<Start>d__46$$MoveNext'}
disassembler = Cs(CS_ARCH_ARM64, CS_MODE_ARM)
disassembler.detail = True
lines = []
for i, method in enumerate(methods):
    if method['Name'] not in selected:
        continue
    start = method['Address']
    end = methods[i + 1]['Address']
    lines.append(f"\n{method['Name']} {start:#x}")
    pages = {}
    for ins in disassembler.disasm(binary[offset(start):offset(end)], start):
        annotation = ''
        if ins.mnemonic == 'adrp':
            reg, addr = ins.op_str.split(', ')
            pages[reg] = int(addr.lstrip('#'), 0)
        elif ins.mnemonic == 'ldr' and ins.op_str.startswith('s'):
            import re
            match = re.search(r'\[(x\d+), #(0x[0-9a-f]+)\]', ins.op_str)
            if match and match[1] in pages:
                addr = pages[match[1]] + int(match[2], 0)
                try:
                    annotation = f" ; literal {struct.unpack_from('<f', binary, offset(addr))[0]}"
                except ValueError:
                    pass
        elif ins.mnemonic in ('bl', 'b') and ins.op_str.startswith('#'):
            annotation = ' ; ' + names.get(int(ins.op_str[1:], 0), '')
            if ins.mnemonic == 'bl':
                pages = {k: v for k, v in pages.items() if int(k[1:]) >= 19}
        lines.append(f'{ins.address:#x} {ins.mnemonic:8} {ins.op_str}{annotation}')
        if ins.mnemonic != 'adrp':
            for register in ins.regs_access()[1]:
                name = ins.reg_name(register)
                if name.startswith('w'):
                    name = 'x' + name[1:]
                pages.pop(name, None)
destination = root / 'phira/target/block-area-native-audio.txt'
destination.write_text('\n'.join(lines), encoding='utf-8')
print(destination)

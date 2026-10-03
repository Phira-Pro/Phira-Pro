"""Read-only annotations for the small AArch64 mask-control routines.

Uses the existing dump's RVA/file-offset mapping; no IDA service or packages.
Unknown instructions stay as hex, so this is evidence rather than a decompiler.
"""
import struct
from pathlib import Path

root = Path(__file__).resolve().parents[3]
binary = (root / "_official_src/libil2cpp.so").read_bytes()
ph_offset = struct.unpack_from("<Q", binary, 32)[0]
ph_size, ph_count = struct.unpack_from("<HH", binary, 54)
segments = [struct.unpack_from("<IIQQQQQQ", binary, ph_offset + i * ph_size) for i in range(ph_count)]

def file_offset(address):
    for kind, _, offset, vaddr, _, size, _, _ in segments:
        if kind == 1 and vaddr <= address < vaddr + size:
            return offset + address - vaddr
    raise ValueError(f"Unmapped virtual address {address:#x}")

def signed(value, bits):
    return value - (1 << bits) if value & (1 << (bits - 1)) else value

def annotate(start, end):
    pages = {}
    for address in range(start, end, 4):
        word = struct.unpack_from("<I", binary, file_offset(address))[0]
        d, n, m = word & 31, (word >> 5) & 31, (word >> 16) & 31
        text = f"{word:08x}"
        if word & 0x9f000000 == 0x90000000:
            imm = signed(((word >> 5) & 0x7ffff) << 2 | ((word >> 29) & 3), 21) << 12
            pages[d] = (address & ~0xfff) + imm
            text = f"ADRP x{d}, {pages[d]:#x}"
        elif word & 0xffc00000 in (0xb9400000, 0xbd400000, 0xf9400000):
            kind = {0xb9400000: "w", 0xbd400000: "s", 0xf9400000: "x"}[word & 0xffc00000]
            offset = ((word >> 10) & 0xfff) * (8 if kind == "x" else 4)
            text = f"LDR {kind}{d}, [x{n}, #{offset:#x}]"
            if kind == "s" and n in pages:
                text += f"; constant {struct.unpack_from('<f', binary, file_offset(pages[n] + offset))[0]}"
            if kind != "s":
                pages.pop(d, None)
        elif word & 0xfc000000 == 0x94000000:
            text = f"BL {address + 4 * signed(word & 0x3ffffff, 26):#x}"
            for register in range(18):
                pages.pop(register, None)
        elif word & 0x7f800000 == 0x52800000:
            text = f"MOVZ {'x' if word >> 31 else 'w'}{d}, #{((word >> 5) & 0xffff) << (16 * ((word >> 21) & 3)):#x}"
            pages.pop(d, None)
        elif word & 0xfffffc00 == 0x1e220000:
            text = f"SCVTF s{d}, w{n}"
        elif word & 0xffe0fc00 in (0x1e201800, 0x1e200800, 0x1e202800, 0x1e203800):
            op = {0x1e201800: "FDIV", 0x1e200800: "FMUL", 0x1e202800: "FADD", 0x1e203800: "FSUB"}[word & 0xffe0fc00]
            text = f"{op} s{d}, s{n}, s{m}"
        else:
            # Conservatively drop unknown integer destinations, avoiding a
            # false constant annotation after an intervening register write.
            pages.pop(d, None)
        print(f"{address:#x}: {text}")

for name, start, end in [
    ("Start", 0x1d6dc1c, 0x1d6e580),
    ("UpdateDilateTexelSize", 0x1d6ebd4, 0x1d6ee8c),
    ("RenderEffects", 0x1d6f0e4, 0x1d6f49c),
    ("GetGlowRingWeight", 0x1d6f720, 0x1d6f7bc),
]:
    print(name)
    annotate(start, end)

"""Tiny SM83 assembler for link test ROMs: a list of bytes, ('label', name),
('jr', cond_opcode, name) and ('jp', name). Origin 0x150."""
LOGO = bytes.fromhex('CEED6666CC0D000B03730083000C000D0008111F8889000EDCCC6EE6DDDDD999BBBB67636E0EECCCDDDC999FBBB9333E')

def assemble(items, origin=0x150):
    labels, pc = {}, origin
    for it in items:
        if isinstance(it, tuple):
            if it[0] == 'label': labels[it[1]] = pc
            elif it[0] == 'jr': pc += 2
            elif it[0] == 'jp': pc += 3
        else:
            pc += 1
    out, pc = bytearray(), origin
    for it in items:
        if isinstance(it, tuple):
            if it[0] == 'label': continue
            if it[0] == 'jr':
                off = labels[it[2]] - (pc + 2)
                assert -128 <= off <= 127, it
                out += bytes([it[1], off & 0xFF]); pc += 2
            elif it[0] == 'jp':
                a = labels[it[1]]; out += bytes([0xC3, a & 0xFF, a >> 8]); pc += 3
        else:
            out.append(it); pc += 1
    return bytes(out)

def rom(main, title, vectors=None, cgb=False):
    r = bytearray(0x8000)
    for addr, code in (vectors or {}).items():
        r[addr:addr + len(code)] = code
    r[0x100:0x104] = bytes([0x00, 0xC3, 0x50, 0x01])
    r[0x104:0x134] = LOGO
    t = title.encode()[:11]; r[0x134:0x134 + len(t)] = t
    if cgb: r[0x143] = 0xC0
    r[0x150:0x150 + len(main)] = main
    c = 0
    for b in r[0x134:0x14D]: c = (c - b - 1) & 0xFF
    r[0x14D] = c
    return r

JRNZ, JRZ, JR = 0x20, 0x28, 0x18

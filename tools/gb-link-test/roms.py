#!/usr/bin/env python3
"""Writes the Game Boy link test ROMs into DIR (no commercial ROM involved).

Each pair is a master, which clocks a byte out, and a slave, which waits on
the other's clock and answers with the byte it got plus one. The master
counts the answers it expected at C000 and the others at C001; the slave
counts its bytes at C000.

  poll-*  the transfer flag is polled (DMG, normal clock)
  irq-*   both wait halted for the serial interrupt (DMG, normal clock)
  cgb-*   as irq, in double speed with the GBC's fast serial clock

usage: roms.py DIR
"""
import sys
from asm import assemble, rom, JRNZ, JRZ, JR

SERIAL_VECTOR = {0x58: bytes([0xF5, 0x3E, 0x01, 0xEA, 0x10, 0xC0, 0xF1, 0xD9])}  # flag C010, RETI


def double_speed(cgb):
    return [0x3E, 0x01, 0xE0, 0x4D, 0x10, 0x00] if cgb else []  # KEY1 = 1, STOP


def wait(irq):
    if irq:
        return [0xFB, ('label', 'h'), 0x76, 0x00, 0xFA, 0x10, 0xC0, 0xB7, ('jr', JRZ, 'h'), 0xF3]
    return [('label', 'w'), 0xF0, 0x02, 0xCB, 0x7F, ('jr', JRNZ, 'w')]


def master(sc, cgb, irq, delay):
    return assemble(
        [0xF3, 0x31, 0xFE, 0xDF, 0xAF, 0xEA, 0x00, 0xC0, 0xEA, 0x01, 0xC0, 0x16, 0x00]
        + double_speed(cgb) + [0x3E, 0x08, 0xE0, 0xFF,
        ('label', 'loop'), 0xAF, 0xEA, 0x10, 0xC0, 0x7A, 0xE0, 0x01, 0x3E, sc, 0xE0, 0x02]
        + wait(irq) + [
        0xF0, 0x01, 0xBA, ('jr', JRNZ, 'err'), 0x21, 0x00, 0xC0, 0x34, ('jr', JR, 'next'),
        ('label', 'err'), 0x21, 0x01, 0xC0, 0x34,
        ('label', 'next'), 0x14, 0x06, delay, ('label', 'd'), 0x05, ('jr', JRNZ, 'd'),
        ('jr', JR, 'loop')])


def slave(cgb, irq):
    return assemble(
        [0xF3, 0x31, 0xFE, 0xDF, 0xAF, 0xEA, 0x00, 0xC0, 0x16, 0x00]
        + double_speed(cgb) + [0x3E, 0x08, 0xE0, 0xFF,
        ('label', 'loop'), 0xAF, 0xEA, 0x10, 0xC0, 0x7A, 0xE0, 0x01, 0x3E, 0x80, 0xE0, 0x02]
        + wait(irq) + [
        0xF0, 0x01, 0x3C, 0x57, 0x21, 0x00, 0xC0, 0x34, ('jr', JR, 'loop')])


out = sys.argv[1]
for name, m, s, cgb in [
    ("poll", master(0x81, False, False, 0x30), slave(False, False), False),
    ("irq", master(0x81, False, True, 0x30), slave(False, True), False),
    ("cgb", master(0x83, True, True, 0x10), slave(True, True), True),
]:
    open(f"{out}/{name}-master.gb", "wb").write(rom(m, f"{name.upper()}MASTER", SERIAL_VECTOR, cgb))
    open(f"{out}/{name}-slave.gb", "wb").write(rom(s, f"{name.upper()}SLAVE", SERIAL_VECTOR, cgb))

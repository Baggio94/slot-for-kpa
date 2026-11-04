#!/usr/bin/env python3
"""mGBA Game Boy Link scheduler hooks and stall probes, gated to Link play.

The GB/GBC Link scheduler (zz-gb-link-mode.patch) parks a Game Boy that waits
for its peer and must hand control back to slot. from inside SM83Run and
GBProcessEvents. The same places carry read-only probes that the Android
monitor reports when a linked frame stalls.

None of it runs in solo play: every hook sits behind slot_mgba_gb_link_hooks,
raised only while two Game Boys are plugged together (_linkPlugGB) and
lowered when they are unplugged. With the flag down SM83Run and
GBProcessEvents run mGBA's own code, instruction for instruction.

Idempotent and fail-closed: anchored source must match the pinned mGBA commit.
"""
from pathlib import Path
import sys

src = Path(sys.argv[1])
MARK = "SLOT_GB_INNER_TRACE_V2"

def replace(path, changes):
    p = src / path
    text = p.read_text()
    if MARK in text:
        print(f"Already instrumented: {path}")
        return
    for label, before, after in changes:
        n = text.count(before)
        if n != 1:
            raise SystemExit(f"mGBA inner trace anchor {label} mismatch in {path}: {n}")
        text = text.replace(before, after, 1)
    p.write_text(text)
    print(f"Instrumented: {path}")

DECLS = """/* SLOT_GB_INNER_TRACE_V2 */
extern int slot_mgba_gb_link_hooks;
extern void slot_mgba_gb_link_inner_trace(unsigned, unsigned, unsigned);
extern void slot_mgba_gb_link_mark_fault(void);
extern uint32_t slot_mgba_gb_link_fault(void);
extern bool slot_mgba_gb_link_should_yield(const struct SM83Core*);
"""

replace("src/sm83/sm83.c", [
    ("SM83Run",
     """void SM83Run(struct SM83Core* cpu) {
\tbool running = true;
\twhile (running || cpu->executionState != SM83_CORE_FETCH) {
\t\tif (cpu->cycles < cpu->nextEvent) {
\t\t\trunning = _SM83TickInternal(cpu) && running;
\t\t} else {
\t\t\tcpu->irqh.processEvents(cpu);
\t\t\trunning = false;
\t\t}
\t}
}
""",
     DECLS.replace("/* SLOT_GB_INNER_TRACE_V2 */\n",
                   "/* SLOT_GB_INNER_TRACE_V2 */\n"
                   "/* Raised only while two Game Boys are linked (libretro.c). */\n"
                   "__attribute__((visibility(\"hidden\"))) int slot_mgba_gb_link_hooks = 0;\n"
                   "\n").replace("extern int slot_mgba_gb_link_hooks;\n", "") +
     """
static void _SM83RunLinked(struct SM83Core* cpu) {
\tbool running = true;
\tunsigned diagnosticLoops = 0;
\twhile (running || cpu->executionState != SM83_CORE_FETCH) {
\t\t/* A parked GB may run its serial event forever without returning
\t\t * to Slot's outer frame watchdog. Exit only if the linked GB
\t\t * fault marker confirms a failed paired frame. */
\t\tif (slot_mgba_gb_link_fault()) return;
\t\tif (diagnosticLoops >= 262144) {
\t\t\tslot_mgba_gb_link_mark_fault();
\t\t\tif (slot_mgba_gb_link_fault()) return;
\t\t\tdiagnosticLoops = 0;
\t\t}
\t\t/* Log only every 1024 ticks to keep diagnostic overhead negligible. */
\t\tif ((++diagnosticLoops & 1023) == 0) {
\t\t\tslot_mgba_gb_link_inner_trace(11, cpu->pc, cpu->cycles);
\t\t}
\t\tif (cpu->cycles < cpu->nextEvent) {
\t\t\trunning = _SM83TickInternal(cpu) && running;
\t\t\t/* This tick can dispatch a serial event. Once the current GB
\t\t\t * is parked, no further instructions may execute before wake. */
\t\t\tif (slot_mgba_gb_link_should_yield(cpu)) return;
\t\t} else {
\t\t\tslot_mgba_gb_link_inner_trace(12, cpu->pc, cpu->cycles);
\t\t\tcpu->irqh.processEvents(cpu);
\t\t\tslot_mgba_gb_link_inner_trace(13, cpu->pc, cpu->cycles);
\t\t\t/* The lockstep peer has suspended this GB. Return control
\t\t\t * to Slot's scheduler instead of processing 1-cycle events
\t\t\t * indefinitely while the CPU is supposed to sleep. */
\t\t\tif (slot_mgba_gb_link_should_yield(cpu)) return;
\t\t\trunning = false;
\t\t}
\t}
}

void SM83Run(struct SM83Core* cpu) {
\tif (slot_mgba_gb_link_hooks) {
\t\t_SM83RunLinked(cpu);
\t\treturn;
\t}
\tbool running = true;
\twhile (running || cpu->executionState != SM83_CORE_FETCH) {
\t\tif (cpu->cycles < cpu->nextEvent) {
\t\t\trunning = _SM83TickInternal(cpu) && running;
\t\t} else {
\t\t\tcpu->irqh.processEvents(cpu);
\t\t\trunning = false;
\t\t}
\t}
}
"""),
])

replace("src/gb/gb.c", [
    ("GBProcessEvents enter",
     """void GBProcessEvents(struct SM83Core* cpu) {
\tstruct GB* gb = (struct GB*) cpu->master;
\tbool wasHalted = cpu->halted;
\twhile (true) {
""",
     DECLS + """
void GBProcessEvents(struct SM83Core* cpu) {
\tstruct GB* gb = (struct GB*) cpu->master;
\tbool wasHalted = cpu->halted;
\tconst bool linked = slot_mgba_gb_link_hooks != 0;
\tunsigned diagnosticEventTicks = 0;
\tif (linked) slot_mgba_gb_link_inner_trace(21, cpu->pc, cpu->cycles);
\twhile (true) {
"""),
    ("GB tick call",
     """\t\t\tdo {
\t\t\t\tnextEvent = mTimingTick(&gb->timing, nextEvent);
\t\t\t} while (gb->cpuBlocked);
""",
     """\t\t\tdo {
\t\t\t\tif (!linked) {
\t\t\t\t\tnextEvent = mTimingTick(&gb->timing, nextEvent);
\t\t\t\t\tcontinue;
\t\t\t\t}
\t\t\t\tslot_mgba_gb_link_inner_trace(22, cpu->pc, nextEvent);
\t\t\t\tnextEvent = mTimingTick(&gb->timing, nextEvent);
\t\t\t\tslot_mgba_gb_link_inner_trace(23, cpu->pc, nextEvent);
\t\t\t\tif (slot_mgba_gb_link_should_yield(cpu)) {
\t\t\t\t\t/* mLockstep.wait/useCycles suspended this GB during
\t\t\t\t\t * the timing callback. In Qt the thread blocks HERE.
\t\t\t\t\t * Preserve the timer queue and yield to the peer. */
\t\t\t\t\tcpu->nextEvent = nextEvent;
\t\t\t\t\tgb->earlyExit = false;
\t\t\t\t\treturn;
\t\t\t\t}
\t\t\t\tif (++diagnosticEventTicks >= 32768) {
\t\t\t\t\tslot_mgba_gb_link_mark_fault();
\t\t\t\t\tif (slot_mgba_gb_link_fault()) {
\t\t\t\t\t\t/* Leave CPU event time intact so the incomplete
\t\t\t\t\t\t * state can be discarded, never committed. */
\t\t\t\t\t\tcpu->nextEvent = nextEvent;
\t\t\t\t\t\treturn;
\t\t\t\t\t}
\t\t\t\t\tdiagnosticEventTicks = 0;
\t\t\t\t}
\t\t\t} while (gb->cpuBlocked);
"""),
    ("GBProcessEvents exit",
     """\tgb->earlyExit = false;
}

void GBSetInterrupts(struct SM83Core* cpu, bool enable) {""",
     """\tif (linked) slot_mgba_gb_link_inner_trace(24, cpu->pc, cpu->cycles);
\tgb->earlyExit = false;
}

void GBSetInterrupts(struct SM83Core* cpu, bool enable) {"""),
])

replace("src/gb/sio/lockstep.c", [
    ("SIO event enter",
     """static void _GBSIOLockstepNodeProcessEvents(struct mTiming* timing, void* user, uint32_t cyclesLate) {
\tstruct GBSIOLockstepNode* node = user;
\tmLockstepLock(&node->p->d);
""",
     """/* SLOT_GB_INNER_TRACE_V2: lockstep nodes exist only while linked. */
extern void slot_mgba_gb_link_inner_trace(unsigned, unsigned, unsigned);

static void _GBSIOLockstepNodeProcessEvents(struct mTiming* timing, void* user, uint32_t cyclesLate) {
\tstruct GBSIOLockstepNode* node = user;
\tslot_mgba_gb_link_inner_trace(31, node->id, cyclesLate);
\tmLockstepLock(&node->p->d);
"""),
    ("SIO event exit",
     """\t} else {
\t\tGBInterrupt(node->d.p->p);
\t\tmTimingSchedule(timing, &node->event, cyclesLate + 1);
\t}
}

static void GBSIOLockstepNodeWriteSB(""",
     """\t} else {
\t\tGBInterrupt(node->d.p->p);
\t\tmTimingSchedule(timing, &node->event, cyclesLate + 1);
\t}
\tslot_mgba_gb_link_inner_trace(32, node->id, node->p->d.transferActive);
}

static void GBSIOLockstepNodeWriteSB("""),
])

# The flag follows the two Game Boys' cable: up once both nodes are attached,
# down before they are detached.
replace("src/platform/libretro/libretro.c", [
    ("GB hooks flag",
     """static void _linkPlugInGB(void) {
""",
     """/* SLOT_GB_INNER_TRACE_V2: defined in sm83.c, read by SM83Run and
 * GBProcessEvents. */
extern int slot_mgba_gb_link_hooks;

static void _linkPlugInGB(void) {
"""),
    ("GB plug",
     """\tlinkPlugged = true;
\tlinkStuck = false;
\t_gbDiagStart();
}
""",
     """\tlinkPlugged = true;
\tlinkStuck = false;
\t_gbDiagStart();
\t/* SLOT_GB_INNER_TRACE_V2 */
\tslot_mgba_gb_link_hooks = 1;
}
"""),
    ("GB unplug",
     """static void _linkUnplugGB(void) {
\t_gbDiagStop();
""",
     """static void _linkUnplugGB(void) {
\tslot_mgba_gb_link_hooks = 0;
\t_gbDiagStop();
"""),
])

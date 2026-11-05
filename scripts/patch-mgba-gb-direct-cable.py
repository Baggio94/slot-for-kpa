#!/usr/bin/env python3
"""mGBA Game Boy Link: a direct cable between the two Game Boys of one core.

zz-gb-link-mode.patch linked the two Game Boys with mGBA's GBSIOLockstep, which
was written for two threads that block on each other (mGBA's Qt frontend).
Run on one thread, a Game Boy waiting for its peer had to be "parked" by a
credit scheduler, and a Game Boy halted until its serial interrupt could spin
inside GBProcessEvents forever: the freezes and stalls seen on the KPAs. That
lockstep also syncs in 512-unit steps, longer than a whole byte at the GBC's
fast serial clock (128 units), so fast transfers lost their bytes.

Both Game Boys live in this one core, so no lockstep is needed:

- They take turns by emulated time: the one behind runs next, and a slice
  event every SLOT_GB_SLICE units makes each return (halted or not), so they
  are never more than one slice apart.
- When a Game Boy starts a transfer on its internal clock, the other's SB is
  what it shifts in if the other is waiting on the external clock (0xFF if
  not), and the master's SB reaches the other at the moment the transfer
  ends, with its serial interrupt: the byte exchange real hardware makes.
- Each call advances both Game Boys by exactly one frame of time, so both
  consoles run the same schedule whichever Game Boy is theirs. The screen
  shown is the local Game Boy's last complete frame.

Nothing in it depends on which console runs it, on wall-clock time or on
threads: the two consoles compute the same pair from the same inputs.

Idempotent and fail-closed: anchored source must match the pinned mGBA commit
with link-mode.patch and zz-gb-link-mode.patch applied.
"""
from pathlib import Path
import sys

if len(sys.argv) != 2:
    raise SystemExit("usage: patch-mgba-gb-direct-cable.py SOURCE_DIR")
source = Path(sys.argv[1]) / "src/platform/libretro/libretro.c"
data = source.read_text()
MARK = "SLOT_GB_DIRECT_CABLE_V1"
if MARK in data:
    print("Already applied: patch-mgba-gb-direct-cable.py")
    raise SystemExit(0)


def function_span(text, signature):
    """Start and end (after the closing brace and newline) of a C function."""
    if text.count(signature) != 1:
        raise SystemExit(f"mGBA direct cable patch cannot find the audited {signature!r}")
    start = text.index(signature)
    depth, i = 0, text.index("{", start)
    while True:
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return start, i + 2
        i += 1


CABLE = r'''/* SLOT_GB_DIRECT_CABLE_V1: see scripts/patch-mgba-gb-direct-cable.py. */
#define SLOT_GB_SLICE 64
#define SLOT_GB_FRAME (GB_VIDEO_TOTAL_LENGTH * 2)
#define SLOT_GB_MAX_RUNS 1000000

struct SlotGBCable {
	struct GBSIODriver d; /* first: the GBSIO hands this pointer back */
	int id;
	struct mTimingEvent slice;
	struct mTimingEvent deliver;
	uint8_t incoming;
	bool receiving;
};

static struct SlotGBCable gbCable[2];
static uint32_t gbCableLast[2];
static uint64_t gbCableElapsed[2];
static uint64_t gbCableClock;
static mColor* gbCablePresent;
static struct mCore* gbCableWatched;

static struct GB* _gbCableBoard(int id) {
	return (struct GB*) linkGb[id].core->board;
}

/* Emulated time since the cable went in; one unit is the same real time for
 * both Game Boys, in normal and double speed alike. The core's own clock is
 * 32-bit (mTimingGlobalTime counts only with debuggers built in), so the
 * cable sums its steps; a clock that went back (a reset) adds nothing. */
static uint64_t _gbCableTime(int id) {
	uint32_t now = (uint32_t) mTimingCurrentTime(&_gbCableBoard(id)->timing);
	uint32_t step = now - gbCableLast[id];
	gbCableLast[id] = now;
	if (step < 0x80000000u) {
		gbCableElapsed[id] += step;
	}
	return gbCableElapsed[id];
}

static void _gbCableSlice(struct mTiming* timing, void* context, uint32_t cyclesLate) {
	struct SlotGBCable* cable = context;
	/* Leave GBProcessEvents even while halted, so the other Game Boy runs. */
	_gbCableBoard(cable->id)->earlyExit = true;
	int32_t next = SLOT_GB_SLICE - (int32_t) cyclesLate;
	mTimingSchedule(timing, &cable->slice, next > 0 ? next : 1);
}

static void _gbCableDeliver(struct mTiming* timing, void* context, uint32_t cyclesLate) {
	UNUSED(timing);
	UNUSED(cyclesLate);
	struct SlotGBCable* cable = context;
	struct GB* gb = _gbCableBoard(cable->id);
	cable->receiving = false;
	if (!GBRegisterSCIsEnable(gb->memory.io[GB_REG_SC]) ||
	    GBRegisterSCIsShiftClock(gb->memory.io[GB_REG_SC])) {
		return; /* it stopped waiting in the meantime */
	}
	gb->memory.io[GB_REG_SB] = cable->incoming;
	gb->memory.io[GB_REG_SC] = GBRegisterSCClearEnable(gb->memory.io[GB_REG_SC]);
	gb->memory.io[GB_REG_IF] |= (1 << GB_IRQ_SIO);
	GBUpdateIRQs(gb);
}

static bool _gbCableInit(struct GBSIODriver* driver) {
	struct SlotGBCable* cable = (struct SlotGBCable*) driver;
	cable->receiving = false;
	cable->slice.context = cable;
	cable->slice.callback = _gbCableSlice;
	cable->slice.name = "slot. GB cable slice";
	cable->slice.priority = 0xF0;
	cable->deliver.context = cable;
	cable->deliver.callback = _gbCableDeliver;
	cable->deliver.name = "slot. GB cable byte";
	cable->deliver.priority = 0x31;
	struct mTiming* timing = &_gbCableBoard(cable->id)->timing;
	mTimingDeschedule(timing, &cable->slice);
	mTimingSchedule(timing, &cable->slice, SLOT_GB_SLICE);
	return true;
}

static void _gbCableDeinit(struct GBSIODriver* driver) {
	struct SlotGBCable* cable = (struct SlotGBCable*) driver;
	struct mTiming* timing = &_gbCableBoard(cable->id)->timing;
	mTimingDeschedule(timing, &cable->slice);
	mTimingDeschedule(timing, &cable->deliver);
	cable->receiving = false;
}

static void _gbCableWriteSB(struct GBSIODriver* driver, uint8_t value) {
	UNUSED(driver);
	UNUSED(value); /* the core keeps SB in io[]; it is read when a transfer starts */
}

static uint8_t _gbCableWriteSC(struct GBSIODriver* driver, uint8_t value) {
	struct SlotGBCable* cable = (struct SlotGBCable*) driver;
	struct GB* self = _gbCableBoard(cable->id);
	struct SlotGBCable* other = &gbCable[1 - cable->id];
	struct GB* peer = _gbCableBoard(other->id);
	if (!GBRegisterSCIsEnable(value)) {
		/* Stopped waiting: no byte arrives any more. */
		mTimingDeschedule(&self->timing, &cable->deliver);
		cable->receiving = false;
		return value;
	}
	if (!GBRegisterSCIsShiftClock(value)) {
		return value; /* waits on the other's clock: the master drives it */
	}
	/* This Game Boy clocks a byte out. GBSIOWriteSC has scheduled its eight
	 * bits from sio->pendingSB; the transfer ends after eight bit periods. */
	uint8_t peerSC = peer->memory.io[GB_REG_SC];
	if (GBRegisterSCIsEnable(peerSC) && !GBRegisterSCIsShiftClock(peerSC) && !other->receiving) {
		self->sio.pendingSB = peer->memory.io[GB_REG_SB];
		uint64_t ends = _gbCableTime(cable->id) +
			(uint64_t) self->sio.period * (2 - self->doubleSpeed) * 8;
		uint64_t now = _gbCableTime(other->id);
		other->incoming = self->memory.io[GB_REG_SB];
		other->receiving = true;
		mTimingDeschedule(&peer->timing, &other->deliver);
		mTimingSchedule(&peer->timing, &other->deliver, ends > now ? (int32_t) (ends - now) : 0);
	} else {
		self->sio.pendingSB = 0xFF; /* nobody on the other end */
	}
	return value;
}

static void _gbCableFrameEnded(void* context) {
	UNUSED(context);
	/* The local Game Boy just finished a frame: keep it to show. */
	if (linkPlugged && gbCablePresent && outputBuffer) {
		memcpy(gbCablePresent, outputBuffer, VIDEO_BUFF_SIZE);
	}
}

'''

PLUG = r'''static void _linkPlugInGB(void) {
	int i;
	/* Each device runs both Game Boys, so a cartridge clock (MBC3, HuC3, TAMA5) must read the
	 * same time on both. The GBA's fixed LINK_RTC_EPOCH_MS would wind such a clock back by
	 * decades, and Pokemon Gold/Silver/Crystal then ask for the time to be reset. Start each
	 * console's emulated clock from its own last latch instead: that comes from the savestate both
	 * devices loaded, so the two agree, and it is close to the real time the player saved at. */
	for (i = 0; i < 2; ++i) {
		struct mCore* gbCore = linkGb[i].core;
		struct GB* gb = gbCore->board;
		int64_t ranMs = (int64_t) gbCore->frameCounter(gbCore) * gbCore->frameCycles(gbCore) * 1000LL
			/ gbCore->frequency(gbCore);
		gbCore->rtc.override = RTC_FAKE_EPOCH;
		gbCore->rtc.value = gb->memory.rtcLastLatch
			? (int64_t) gb->memory.rtcLastLatch * 1000LL - ranMs
			: LINK_RTC_EPOCH_MS;
	}
	if (!gbCablePresent) {
		gbCablePresent = malloc(VIDEO_BUFF_SIZE);
		if (gbCablePresent) {
			memset(gbCablePresent, 0xFF, VIDEO_BUFF_SIZE);
		}
	}
	if (gbCableWatched != core) {
		struct mCoreCallbacks callbacks = { 0 };
		callbacks.videoFrameEnded = _gbCableFrameEnded;
		core->addCoreCallbacks(core, &callbacks);
		gbCableWatched = core;
	}
	for (i = 0; i < 2; ++i) {
		struct SlotGBCable* cable = &gbCable[i];
		memset(cable, 0, sizeof(*cable));
		cable->id = i;
		cable->d.init = _gbCableInit;
		cable->d.deinit = _gbCableDeinit;
		cable->d.writeSB = _gbCableWriteSB;
		cable->d.writeSC = _gbCableWriteSC;
	}
	/* Both Game Boys count time from here, on both consoles alike. */
	for (i = 0; i < 2; ++i) {
		gbCableLast[i] = (uint32_t) mTimingCurrentTime(&_gbCableBoard(i)->timing);
		gbCableElapsed[i] = 0;
	}
	gbCableClock = 0;
	for (i = 0; i < 2; ++i) {
		GBSIOSetDriver(&_gbCableBoard(i)->sio, &gbCable[i].d);
	}
	/* mGBA's Game Boy savestates keep SC but not a transfer in flight: a
	 * state taken mid-byte comes back with the transfer flag set and no
	 * bits left to shift, and a game halted for its serial interrupt never
	 * wakes. Start such a byte again; the cable delivers it as usual. */
	for (i = 0; i < 2; ++i) {
		struct GB* gb = _gbCableBoard(i);
		uint8_t sc = gb->memory.io[GB_REG_SC];
		if (GBRegisterSCIsEnable(sc) && GBRegisterSCIsShiftClock(sc) &&
		    !mTimingIsScheduled(&gb->timing, &gb->sio.event)) {
			GBSIOWriteSC(&gb->sio, sc);
		}
	}
	linkPlugged = true;
	linkStuck = false;
}
'''

UNPLUG = r'''static void _linkUnplugGB(void) {
	int i;
	for (i = 0; i < 2; ++i) {
		/* GBSIOSetDriver deinits the old driver: its events are descheduled. */
		GBSIOSetDriver(&_gbCableBoard(i)->sio, NULL);
	}
	linkPlugged = false;
}
'''

STEP = r'''static void _linkStepGB(void) {
	/* One frame of emulated time for both Game Boys, the one behind first. */
	uint64_t end = gbCableClock + SLOT_GB_FRAME;
	unsigned runs = 0;
	while (true) {
		uint64_t at[2] = { _gbCableTime(0), _gbCableTime(1) };
		if (at[0] >= end && at[1] >= end) {
			break;
		}
		int i = at[0] <= at[1] ? 0 : 1;
		if (at[i] >= end) {
			i = 1 - i;
		}
		if (++runs > SLOT_GB_MAX_RUNS) {
			/* A Game Boy that never returns a slice: fail closed. */
			linkStuck = true;
			fprintf(stderr, "mgba: GB cable made no progress (t=%llu/%llu)\n",
				(unsigned long long) at[0], (unsigned long long) at[1]);
			return;
		}
		linkGb[i].core->runLoop(linkGb[i].core);
	}
	gbCableClock = end;
}
'''

# 1. The cable, before the Game Boy plug.
plug_start, plug_end = function_span(data, "static void _linkPlugInGB(void) {")
data = data[:plug_start] + CABLE + PLUG + data[plug_end:]
# 2. Unplug.
start, end = function_span(data, "static void _linkUnplugGB(void) {")
data = data[:start] + UNPLUG + data[end:]
# 3. The frame step.
start, end = function_span(data, "static void _linkStepGB(void) {")
data = data[:start] + STEP + data[end:]
# 4. Show the local Game Boy's last complete frame, not the one being drawn.
anchor = """\t_linkStep();
\t_postFrame(false);
"""
if data.count(anchor) != 1:
    raise SystemExit("mGBA direct cable patch cannot find the audited _linkRunFrame")
data = data.replace(anchor, """\t_linkStep();
#ifdef M_CORE_GB
\tif (core->platform(core) == mPLATFORM_GB && gbCablePresent) {
\t\tmColor* drawing = outputBuffer;
\t\toutputBuffer = gbCablePresent;
\t\t_postFrame(false);
\t\toutputBuffer = drawing;
\t} else
#endif
\t_postFrame(false);
""", 1)
source.write_text(data)
assert source.read_text().count(MARK) == 1
print("Applied: patch-mgba-gb-direct-cable.py")

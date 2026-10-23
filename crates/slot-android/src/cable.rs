use std::collections::BTreeMap;

use slot_retro::ButtonMask;

/// Starting input delay, as upstream. Adaptive Link moves it between
/// MIN_DELAY and MAX_DELAY; the wire format never carries it, so each side
/// only schedules its own inputs and peers on other delays stay compatible.
pub const DELAY: u64 = 3;
pub const MIN_DELAY: u64 = 1;
pub const MAX_DELAY: u64 = 5;

// Two input stalls inside two seconds mean inputs travel slower than the
// current delay. Ten calm seconds earn one frame back; after a raise, wait
// thirty seconds before trying lower again so a weak Wi-Fi doesn't oscillate.
const RAISE_STALLS: u32 = 2;
const RAISE_WINDOW: u32 = 120;
const LOWER_AFTER_CALM: u32 = 600;
const HOLD_AFTER_RAISE: u32 = 1800;

const PACKET: usize = 11;

const KIND_MASK: u8 = 0;
const KIND_STATE: u8 = 1;
const KIND_STATE_END: u8 = 2;
const KIND_READY: u8 = 3;

const CHUNK: usize = 60_000;

pub fn encode(frame: u64, mask: ButtonMask) -> [u8; PACKET] {
    let mut out = [0u8; PACKET];
    out[0] = KIND_MASK;
    out[1..9].copy_from_slice(&frame.to_le_bytes());
    out[9..].copy_from_slice(&mask.0.to_le_bytes());
    out
}

pub fn decode(buf: &[u8]) -> Option<(u64, ButtonMask)> {
    if buf.len() != PACKET || buf[0] != KIND_MASK {
        return None;
    }
    let frame = u64::from_le_bytes(buf[1..9].try_into().ok()?);
    let mask = u16::from_le_bytes(buf[9..].try_into().ok()?);
    Some((frame, ButtonMask(mask)))
}

pub fn ready_packet() -> [u8; 1] {
    [KIND_READY]
}

pub fn state_packets(state: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = state
        .chunks(CHUNK)
        .map(|c| {
            let mut p = Vec::with_capacity(c.len() + 1);
            p.push(KIND_STATE);
            p.extend_from_slice(c);
            p
        })
        .collect();
    match out.last_mut() {
        Some(last) => last[0] = KIND_STATE_END,
        None => out.push(vec![KIND_STATE_END]),
    }
    out
}

/// Chooses the local input delay from missing-input stalls, one decision
/// per emulated frame.
#[derive(Clone, Debug)]
pub struct AdaptiveDelay {
    delay: u64,
    stalls: u32,
    window: u32,
    calm: u32,
    hold: u32,
}

impl AdaptiveDelay {
    pub fn new(delay: u64) -> Self {
        AdaptiveDelay { delay: delay.clamp(MIN_DELAY, MAX_DELAY), stalls: 0, window: 0, calm: 0, hold: 0 }
    }

    pub fn delay(&self) -> u64 {
        self.delay
    }

    /// `stalled` is true when this frame had to wait for the peer's input.
    pub fn observe(&mut self, stalled: bool) -> u64 {
        self.hold = self.hold.saturating_sub(1);
        if self.stalls > 0 {
            self.window += 1;
            if self.window >= RAISE_WINDOW {
                self.stalls = 0;
                self.window = 0;
            }
        }
        if stalled {
            self.calm = 0;
            self.stalls += 1;
            if self.stalls >= RAISE_STALLS {
                self.stalls = 0;
                self.window = 0;
                if self.delay < MAX_DELAY {
                    self.delay += 1;
                    self.hold = HOLD_AFTER_RAISE;
                }
            }
            return self.delay;
        }
        self.calm = self.calm.saturating_add(1);
        if self.calm >= LOWER_AFTER_CALM && self.hold == 0 && self.delay > MIN_DELAY {
            self.delay -= 1;
            self.calm = 0;
        }
        self.delay
    }
}

pub struct Cable {
    player: u8,
    frame: u64,
    // First local frame not yet sampled. Frames below it are sent and fixed.
    next_local: u64,
    adaptive: AdaptiveDelay,
    local: BTreeMap<u64, ButtonMask>,
    remote: BTreeMap<u64, ButtonMask>,
    stalled: u32,
    primed: bool,
    incoming: Vec<u8>,
    complete: bool,
    peer_ready: bool,
}

impl Cable {
    pub fn new(player: u8) -> Self {
        let idle = ButtonMask::default();
        let seed: BTreeMap<u64, ButtonMask> = (0..DELAY).map(|f| (f, idle)).collect();
        Cable {
            player,
            frame: 0,
            next_local: DELAY,
            adaptive: AdaptiveDelay::new(DELAY),
            local: seed.clone(),
            remote: seed,
            stalled: 0,
            primed: player == 0,
            incoming: Vec::new(),
            complete: false,
            peer_ready: player != 0,
        }
    }

    pub fn primed(&self) -> bool {
        self.primed
    }

    pub fn armed(&self) -> bool {
        self.primed && self.peer_ready
    }

    pub fn take_state(&mut self) -> Option<Vec<u8>> {
        if !self.complete {
            return None;
        }
        self.complete = false;
        Some(std::mem::take(&mut self.incoming))
    }

    pub fn prime(&mut self) {
        self.primed = true;
    }

    pub fn player(&self) -> u8 {
        self.player
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn stalled(&self) -> u32 {
        self.stalled
    }

    pub fn delay(&self) -> u64 {
        self.adaptive.delay()
    }

    pub fn sample(&mut self, mask: ButtonMask) -> [u8; PACKET] {
        let frame = self.frame + DELAY;
        let mask = *self.local.entry(frame).or_insert(mask);
        self.next_local = self.next_local.max(frame + 1);
        encode(frame, mask)
    }

    /// Samples up to `frame + delay` and returns the packets not sent yet:
    /// none after the delay dropped or while stalled, two or more after
    /// it rose (the gap repeats the current mask so no frame is skipped).
    pub fn sample_adaptive(&mut self, mask: ButtonMask) -> Vec<[u8; PACKET]> {
        let target = self.frame + self.adaptive.delay();
        let mut out = Vec::new();
        while self.next_local <= target {
            let frame = self.next_local;
            let mask = *self.local.entry(frame).or_insert(mask);
            out.push(encode(frame, mask));
            self.next_local += 1;
        }
        out
    }

    pub fn accept(&mut self, buf: &[u8]) -> bool {
        match buf.first() {
            Some(&KIND_READY) => {
                self.peer_ready = true;
                return true;
            }
            Some(&KIND_STATE) => {
                self.incoming.extend_from_slice(&buf[1..]);
                return true;
            }
            Some(&KIND_STATE_END) => {
                self.incoming.extend_from_slice(&buf[1..]);
                self.complete = true;
                return true;
            }
            _ => {}
        }
        let Some((frame, mask)) = decode(buf) else {
            return false;
        };
        if frame < self.frame {
            return false;
        }
        self.remote.insert(frame, mask);
        true
    }

    pub fn ready(&self) -> Option<(ButtonMask, ButtonMask)> {
        if !self.primed {
            return None;
        }
        let mine = *self.local.get(&self.frame)?;
        let theirs = *self.remote.get(&self.frame)?;
        match self.player {
            0 => Some((mine, theirs)),
            _ => Some((theirs, mine)),
        }
    }

    pub fn advance(&mut self) {
        self.local.remove(&self.frame);
        self.remote.remove(&self.frame);
        self.frame += 1;
        self.stalled = 0;
    }

    /// Advances and lets the adaptive delay react to this frame's wait.
    pub fn advance_adaptive(&mut self, stalled: bool) {
        self.advance();
        self.adaptive.observe(stalled);
    }

    pub fn stall(&mut self) {
        self.stalled = self.stalled.saturating_add(1);
    }
}

pub const QUIET_FRAMES: u32 = 60;

#[cfg(test)]
mod adaptive_tests {
    use super::*;

    fn frames(adaptive: &mut AdaptiveDelay, count: u32, stalled: bool) {
        for _ in 0..count {
            adaptive.observe(stalled);
        }
    }

    #[test]
    fn calm_link_lowers_one_frame_per_ten_seconds_down_to_one() {
        let mut a = AdaptiveDelay::new(DELAY);
        frames(&mut a, LOWER_AFTER_CALM - 1, false);
        assert_eq!(a.delay(), 3);
        frames(&mut a, 1, false);
        assert_eq!(a.delay(), 2);
        frames(&mut a, LOWER_AFTER_CALM * 4, false);
        assert_eq!(a.delay(), MIN_DELAY);
    }

    #[test]
    fn isolated_stall_does_not_raise_but_repeated_stalls_do() {
        let mut a = AdaptiveDelay::new(2);
        a.observe(true);
        frames(&mut a, RAISE_WINDOW, false);
        a.observe(true);
        assert_eq!(a.delay(), 2, "stalls two seconds apart are clock drift, not latency");
        a.observe(true);
        assert_eq!(a.delay(), 3);
        for _ in 0..10 {
            a.observe(true);
        }
        assert_eq!(a.delay(), MAX_DELAY);
    }

    #[test]
    fn raise_holds_thirty_seconds_before_lowering_again() {
        let mut a = AdaptiveDelay::new(2);
        a.observe(true);
        a.observe(true);
        assert_eq!(a.delay(), 3);
        frames(&mut a, HOLD_AFTER_RAISE - 1, false);
        assert_eq!(a.delay(), 3);
        frames(&mut a, 1, false);
        assert_eq!(a.delay(), 2);
    }

    fn sent_frames(packets: &[[u8; PACKET]]) -> Vec<u64> {
        packets.iter().map(|p| decode(p).unwrap().0).collect()
    }

    #[test]
    fn delay_changes_never_skip_or_resend_a_local_frame() {
        let mut c = Cable::new(0);
        assert_eq!(sent_frames(&c.sample_adaptive(ButtonMask(1))), vec![3]);
        assert!(c.sample_adaptive(ButtonMask(2)).is_empty(), "stalled draws send nothing");
        c.advance_adaptive(true);
        c.advance_adaptive(true);
        assert_eq!(c.delay(), 4);
        assert_eq!(sent_frames(&c.sample_adaptive(ButtonMask(5))), vec![4, 5, 6]);
        for _ in 0..HOLD_AFTER_RAISE {
            c.sample_adaptive(ButtonMask(0));
            c.advance_adaptive(false);
        }
        assert_eq!(c.delay(), 3);
        let frame = c.frame();
        let mut sent = Vec::new();
        for _ in 0..8 {
            sent.extend(sent_frames(&c.sample_adaptive(ButtonMask(0))));
            c.advance_adaptive(false);
        }
        assert_eq!(sent, (frame + 4..frame + 3 + 8).collect::<Vec<_>>(),
            "a lower delay waits for already-sent frames instead of rewriting them");
    }

    #[test]
    fn peers_on_different_delays_stay_in_lockstep() {
        let mut host = Cable::new(0);
        let mut join = Cable::new(1);
        join.prime();
        join.accept(&ready_packet());
        host.accept(&ready_packet());
        for _ in 0..LOWER_AFTER_CALM * 3 {
            host.adaptive.observe(false);
        }
        assert_eq!(host.delay(), 1);
        let mut frames = Vec::new();
        for i in 0..20u16 {
            for p in host.sample_adaptive(ButtonMask(i)) { join.accept(&p); }
            for p in join.sample_adaptive(ButtonMask(100 + i)) { host.accept(&p); }
            let (Some(a), Some(b)) = (host.ready(), join.ready()) else { break };
            assert_eq!(a, b, "both GBAs see identical inputs every frame");
            frames.push(a);
            host.advance();
            join.advance();
        }
        assert_eq!(frames.len(), 20);
    }
}

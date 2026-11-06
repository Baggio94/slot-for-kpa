use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use slot_retro::ButtonMask;

/// Starting input delay, as upstream. Adaptive Link moves it between
/// MIN_DELAY and MAX_DELAY; the wire format never carries it, so each side
/// only schedules its own inputs and peers on other delays stay compatible.
pub const DELAY: u64 = 3;
pub const MIN_DELAY: u64 = 1;
pub const MAX_DELAY: u64 = 5;
/// Over the Internet a round trip can be 100 ms and more: let the delay
/// climb further rather than stall every frame. Still below
/// link_sync::RESYNC_LEAD, which must stay ahead of any input delay.
pub const INTERNET_MAX_DELAY: u64 = 10;

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
// 4..=7 are link_sync's. Internet links measure their round trip with a
// PING carrying the sender's clock, answered by a PONG echoing it. Never
// sent on Wi-Fi, so Local peers of any version never see them.
pub const KIND_PING: u8 = 8;
pub const KIND_PONG: u8 = 9;
const PING_LEN: usize = 9;
/// One frame at 60 Hz, in milliseconds.
const FRAME_MS: f64 = 1000.0 / 60.0;
/// Round trips remembered; the worst of them sets the delay, which covers
/// the jitter of a few seconds rather than the average.
const RTT_SAMPLES: usize = 8;
/// With a measured round trip, lower one frame after 2 s without a stall
/// (instead of 10 s), and wait 5 s after the round trip asked for a raise.
const RTT_LOWER_AFTER_CALM: u32 = 120;
const RTT_HOLD_AFTER_RAISE: u32 = 300;

pub fn ping_packet(stamp_us: u64) -> [u8; PING_LEN] {
    let mut out = [0u8; PING_LEN];
    out[0] = KIND_PING;
    out[1..].copy_from_slice(&stamp_us.to_le_bytes());
    out
}

pub fn is_ping_packet(buf: &[u8]) -> bool {
    buf.len() == PING_LEN && matches!(buf[0], KIND_PING | KIND_PONG)
}

/// The answer to a PING: the same stamp, sent straight back.
pub fn pong_for(buf: &[u8]) -> Option<[u8; PING_LEN]> {
    if buf.len() != PING_LEN || buf[0] != KIND_PING { return None; }
    let mut out = [0u8; PING_LEN];
    out.copy_from_slice(buf);
    out[0] = KIND_PONG;
    Some(out)
}

/// The stamp a PONG carries back: now minus it is the round trip.
pub fn pong_stamp(buf: &[u8]) -> Option<u64> {
    if buf.len() != PING_LEN || buf[0] != KIND_PONG { return None; }
    Some(u64::from_le_bytes(buf[1..].try_into().ok()?))
}

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
    ceiling: u64,
    /// Internet: the delay the measured round trip calls for.
    rtt_target: Option<u64>,
    stalls: u32,
    window: u32,
    calm: u32,
    hold: u32,
}

impl AdaptiveDelay {
    pub fn new(delay: u64) -> Self {
        AdaptiveDelay::with_ceiling(delay, MAX_DELAY)
    }

    pub fn with_ceiling(delay: u64, ceiling: u64) -> Self {
        let ceiling = ceiling.max(MIN_DELAY);
        AdaptiveDelay { delay: delay.clamp(MIN_DELAY, ceiling), ceiling, rtt_target: None,
            stalls: 0, window: 0, calm: 0, hold: 0 }
    }

    /// Both consoles delay their own inputs, so together they must cover one
    /// round trip: each takes half of it, plus one frame for jitter and for
    /// where in its frame each console happens to be.
    pub fn target_for_rtt(rtt_ms: f64, ceiling: u64) -> u64 {
        let half = (rtt_ms.max(0.0) / 2.0 / FRAME_MS).ceil() as u64;
        (half + 1).clamp(MIN_DELAY, ceiling.max(MIN_DELAY))
    }

    /// A new round-trip target. Below it, the delay rises at once (stalls
    /// are certain otherwise); above it, it comes down faster than on Wi-Fi
    /// but never below it.
    pub fn set_rtt_target(&mut self, target: u64) {
        let target = target.clamp(MIN_DELAY, self.ceiling);
        self.rtt_target = Some(target);
        if self.delay < target {
            self.delay = target;
            self.hold = RTT_HOLD_AFTER_RAISE;
            self.calm = 0;
        }
    }

    pub fn rtt_target(&self) -> Option<u64> {
        self.rtt_target
    }

    /// Let this link climb to `ceiling` (Internet play). Never lowers the
    /// delay in use.
    pub fn raise_ceiling(&mut self, ceiling: u64) {
        self.ceiling = self.ceiling.max(ceiling);
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
                if self.delay < self.ceiling {
                    self.delay += 1;
                    self.hold = HOLD_AFTER_RAISE;
                }
            }
            return self.delay;
        }
        self.calm = self.calm.saturating_add(1);
        let (after, floor) = match self.rtt_target {
            Some(target) => (RTT_LOWER_AFTER_CALM, target.max(MIN_DELAY)),
            None => (LOWER_AFTER_CALM, MIN_DELAY),
        };
        if self.calm >= after && self.hold == 0 && self.delay > floor {
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
    /// Internet: the last round trips, in milliseconds.
    rtts: VecDeque<f64>,
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
            rtts: VecDeque::new(),
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

    /// Internet: a measured round trip. The worst of the last few sets the
    /// delay target. Returns the target when it changed (for the log).
    pub fn note_rtt(&mut self, rtt: Duration) -> Option<u64> {
        if self.rtts.len() == RTT_SAMPLES { self.rtts.pop_front(); }
        self.rtts.push_back(rtt.as_secs_f64() * 1000.0);
        let worst = self.rtts.iter().copied().fold(0.0, f64::max);
        let target = AdaptiveDelay::target_for_rtt(worst, self.adaptive.ceiling);
        let changed = self.adaptive.rtt_target() != Some(target);
        self.adaptive.set_rtt_target(target);
        changed.then_some(target)
    }

    /// Internet play: the input delay may climb to INTERNET_MAX_DELAY.
    pub fn allow_internet_delay(&mut self) {
        self.adaptive.raise_ceiling(INTERNET_MAX_DELAY);
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
    #[test]
    fn round_trip_sets_each_side_half_plus_a_frame() {
        // 30 ms: half is 15 ms, under one frame, plus one frame of margin.
        assert_eq!(AdaptiveDelay::target_for_rtt(30.0, INTERNET_MAX_DELAY), 2);
        assert_eq!(AdaptiveDelay::target_for_rtt(0.0, INTERNET_MAX_DELAY), 1);
        // 100 ms: 50 ms is three frames, plus one.
        assert_eq!(AdaptiveDelay::target_for_rtt(100.0, INTERNET_MAX_DELAY), 4);
        assert_eq!(AdaptiveDelay::target_for_rtt(900.0, INTERNET_MAX_DELAY), INTERNET_MAX_DELAY);
    }

    #[test]
    fn a_measured_round_trip_brings_the_delay_down_fast_but_not_under_it() {
        let mut a = AdaptiveDelay::with_ceiling(DELAY, INTERNET_MAX_DELAY);
        for _ in 0..200 { a.observe(true); }
        assert_eq!(a.delay(), INTERNET_MAX_DELAY);
        a.set_rtt_target(2);
        // The stall raises held the delay for 30 s; after that it falls a
        // frame every 2 s instead of every 10 s, and stops at the target.
        for _ in 0..HOLD_AFTER_RAISE { a.observe(false); }
        for _ in 0..(RTT_LOWER_AFTER_CALM * 20) { a.observe(false); }
        assert_eq!(a.delay(), 2);
    }

    #[test]
    fn a_longer_round_trip_raises_the_delay_at_once() {
        let mut a = AdaptiveDelay::with_ceiling(DELAY, INTERNET_MAX_DELAY);
        a.set_rtt_target(6);
        assert_eq!(a.delay(), 6);
        for _ in 0..RTT_HOLD_AFTER_RAISE { a.observe(false); }
        for _ in 0..(RTT_LOWER_AFTER_CALM * 10) { a.observe(false); }
        assert_eq!(a.delay(), 6, "never below what the round trip needs");
    }

    #[test]
    fn the_worst_recent_round_trip_counts() {
        let mut c = Cable::new(0);
        c.allow_internet_delay();
        assert_eq!(c.note_rtt(Duration::from_millis(30)), Some(2));
        assert_eq!(c.note_rtt(Duration::from_millis(100)), Some(4));
        assert_eq!(c.note_rtt(Duration::from_millis(30)), None, "the 100 ms spike still counts");
        for _ in 0..RTT_SAMPLES { c.note_rtt(Duration::from_millis(30)); }
        assert_eq!(c.adaptive.rtt_target(), Some(2), "until it leaves the window");
    }

    #[test]
    fn ping_and_pong_carry_the_stamp_back() {
        let ping = ping_packet(123_456);
        assert!(is_ping_packet(&ping));
        assert!(decode(&ping).is_none(), "never mistaken for an input");
        let pong = pong_for(&ping).unwrap();
        assert!(is_ping_packet(&pong));
        assert_eq!(pong_stamp(&pong), Some(123_456));
        assert_eq!(pong_for(&pong), None);
        assert!(!crate::link_sync::is_sync_packet(&ping));
    }

    #[test]
    fn internet_delay_climbs_further_but_stays_behind_the_resync_lead() {
        assert!(INTERNET_MAX_DELAY > MAX_DELAY);
        assert!(INTERNET_MAX_DELAY < crate::link_sync::RESYNC_LEAD);
        let mut a = AdaptiveDelay::new(DELAY);
        a.raise_ceiling(INTERNET_MAX_DELAY);
        for _ in 0..200 { a.observe(true); }
        assert_eq!(a.delay(), INTERNET_MAX_DELAY);
        let mut lan = AdaptiveDelay::new(DELAY);
        for _ in 0..200 { lan.observe(true); }
        assert_eq!(lan.delay(), MAX_DELAY, "Wi-Fi links keep their ceiling");
    }

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

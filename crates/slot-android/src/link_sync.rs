//! Desync check and resync for Cable lockstep on an mGBA pair.
//!
//! Both consoles run the same two Game Boys (or GBAs) from the same inputs,
//! so at every frame they hold the same pair. Every [CHECK_EVERY] frames each
//! sends a hash of its pair for that frame. When the two differ, Host picks a
//! frame [RESYNC_LEAD] frames ahead, which Join cannot have reached yet, and
//! sends its pair there; Join stops at that frame until the pair has arrived,
//! loads it, and both carry on from the same pair. A silent desync (a trade
//! that fails, a battle where each sees something else) becomes a short wait.
//!
//! Packets, all reliable and in order on the Link's TCP stream:
//!   4 HASH       frame u64 LE, hash u64 LE
//!   5 RESYNC_AT  frame u64 LE (Host to Join only)
//!   6 RESYNC     a chunk of Host's pair
//!   7 RESYNC_END the last chunk
//!
//! Pure logic, no core or socket here: link_play.rs drives it.
use std::collections::BTreeMap;

/// Two seconds at 60 frames per second.
pub const CHECK_EVERY: u64 = 120;
/// Farther than any input delay (cable::MAX_DELAY), so Join is still before it.
pub const RESYNC_LEAD: u64 = 12;

pub const KIND_HASH: u8 = 4;
pub const KIND_RESYNC_AT: u8 = 5;
pub const KIND_RESYNC: u8 = 6;
pub const KIND_RESYNC_END: u8 = 7;

const CHUNK: usize = 60_000;
const MAX_STATE: usize = 16 * 1024 * 1024;
// Hashes kept while the other side's hash for the same frame is on its way.
const KEEP: usize = 16;

pub fn is_sync_packet(packet: &[u8]) -> bool {
    matches!(packet.first(), Some(&(KIND_HASH..=KIND_RESYNC_END)))
}

/// FNV-1a over the whole state: the same bytes give the same hash on any
/// device; it only has to tell two pairs apart, not resist an attacker.
pub fn hash(state: &[u8]) -> u64 {
    state.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

pub fn hash_packet(frame: u64, hash: u64) -> [u8; 17] {
    let mut out = [0u8; 17];
    out[0] = KIND_HASH;
    out[1..9].copy_from_slice(&frame.to_le_bytes());
    out[9..].copy_from_slice(&hash.to_le_bytes());
    out
}

pub fn resync_at_packet(frame: u64) -> [u8; 9] {
    let mut out = [0u8; 9];
    out[0] = KIND_RESYNC_AT;
    out[1..].copy_from_slice(&frame.to_le_bytes());
    out
}

pub fn resync_packets(state: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = state.chunks(CHUNK).map(|chunk| {
        let mut packet = Vec::with_capacity(chunk.len() + 1);
        packet.push(KIND_RESYNC);
        packet.extend_from_slice(chunk);
        packet
    }).collect();
    match out.last_mut() {
        Some(last) => last[0] = KIND_RESYNC_END,
        None => out.push(vec![KIND_RESYNC_END]),
    }
    out
}

fn frame_of(packet: &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(packet.get(1..9)?.try_into().ok()?))
}

pub struct Sync {
    host: bool,
    every: u64,
    local: BTreeMap<u64, u64>,
    remote: BTreeMap<u64, u64>,
    /// The frame both consoles meet at to take Host's pair.
    resync_at: Option<u64>,
    incoming: Vec<u8>,
    complete: bool,
    /// Hashes at or before this frame predate the last resync.
    floor: u64,
    resyncs: u32,
}

impl Sync {
    pub fn new(host: bool) -> Self {
        Self::every(host, CHECK_EVERY)
    }

    pub fn every(host: bool, every: u64) -> Self {
        Sync {
            host, every: every.max(1), local: BTreeMap::new(), remote: BTreeMap::new(),
            resync_at: None, incoming: Vec::new(), complete: false, floor: 0, resyncs: 0,
        }
    }

    pub fn host(&self) -> bool { self.host }
    pub fn resyncs(&self) -> u32 { self.resyncs }

    /// A hash is due once the pair has reached `frame`.
    pub fn due(&self, frame: u64) -> bool {
        frame > self.floor && frame % self.every == 0
    }

    /// Keeps this console's hash for `frame`; the packet goes to the other.
    pub fn record(&mut self, frame: u64, hash: u64) -> [u8; 17] {
        self.local.insert(frame, hash);
        while self.local.len() > KEEP { self.local.pop_first(); }
        hash_packet(frame, hash)
    }

    /// Takes one sync packet; false when it breaks the protocol.
    pub fn accept(&mut self, packet: &[u8], current: u64) -> bool {
        match packet.first() {
            Some(&KIND_HASH) => {
                if packet.len() != 17 { return false; }
                let Some(frame) = frame_of(packet) else { return false };
                let hash = u64::from_le_bytes(packet[9..17].try_into().unwrap_or([0; 8]));
                if frame > self.floor {
                    self.remote.insert(frame, hash);
                    while self.remote.len() > KEEP { self.remote.pop_first(); }
                }
                true
            }
            Some(&KIND_RESYNC_AT) => {
                let Some(frame) = frame_of(packet) else { return false };
                // Only Host plans; Join must still be before the frame.
                if self.host || packet.len() != 9 || frame <= current || self.resync_at.is_some() {
                    return false;
                }
                self.resync_at = Some(frame);
                self.incoming.clear();
                self.complete = false;
                true
            }
            Some(&(KIND_RESYNC | KIND_RESYNC_END)) => {
                if self.host || self.resync_at.is_none() || self.complete ||
                    self.incoming.len() + packet.len() > MAX_STATE {
                    return false;
                }
                self.incoming.extend_from_slice(&packet[1..]);
                self.complete = packet[0] == KIND_RESYNC_END;
                true
            }
            _ => false,
        }
    }

    /// The first frame whose two hashes are both known and differ.
    pub fn mismatch(&mut self) -> Option<u64> {
        let frames: Vec<u64> = self.local.keys().copied()
            .filter(|frame| self.remote.contains_key(frame)).collect();
        let mut found = None;
        for frame in frames {
            let (mine, theirs) = (self.local.remove(&frame), self.remote.remove(&frame));
            if found.is_none() && mine != theirs { found = Some(frame); }
        }
        found
    }

    /// Host: the RESYNC_AT packet for a resync at `current + RESYNC_LEAD`,
    /// unless one is already planned.
    pub fn plan(&mut self, current: u64) -> Option<[u8; 9]> {
        if !self.host || self.resync_at.is_some() { return None; }
        let at = current + RESYNC_LEAD;
        self.resync_at = Some(at);
        Some(resync_at_packet(at))
    }

    /// Whether the pair must be resynced before running `frame`.
    pub fn resync_now(&self, frame: u64) -> bool {
        self.resync_at == Some(frame)
    }

    /// Join: Host's whole pair, once it has arrived.
    pub fn take_state(&mut self) -> Option<Vec<u8>> {
        if !self.complete { return None; }
        self.complete = false;
        Some(std::mem::take(&mut self.incoming))
    }

    /// Both consoles hold Host's pair again from `frame` on.
    pub fn resynced(&mut self, frame: u64) {
        self.resync_at = None;
        self.floor = frame;
        self.local.retain(|f, _| *f > frame);
        self.remote.retain(|f, _| *f > frame);
        self.resyncs = self.resyncs.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_hashes_never_ask_for_a_resync() {
        let (mut host, mut join) = (Sync::every(true, 4), Sync::every(false, 4));
        for frame in [4, 8, 12] {
            let a = host.record(frame, 7);
            let b = join.record(frame, 7);
            assert!(join.accept(&a, frame) && host.accept(&b, frame));
            assert_eq!((host.mismatch(), join.mismatch()), (None, None));
        }
    }

    #[test]
    fn a_difference_is_found_once_and_host_plans_ahead() {
        let (mut host, mut join) = (Sync::every(true, 4), Sync::every(false, 4));
        let a = host.record(8, 1);
        assert!(host.accept(&join.record(8, 2), 9));
        assert!(join.accept(&a, 9));
        assert_eq!(host.mismatch(), Some(8));
        assert_eq!(join.mismatch(), Some(8));
        assert_eq!(host.mismatch(), None, "a frame is compared once");
        let plan = host.plan(10).unwrap();
        assert!(host.plan(11).is_none(), "one resync at a time");
        assert!(join.accept(&plan, 10 + cable_lead_margin()));
        assert!(join.resync_now(10 + RESYNC_LEAD) && host.resync_now(10 + RESYNC_LEAD));
    }

    fn cable_lead_margin() -> u64 { crate::cable::MAX_DELAY }

    #[test]
    fn join_receives_the_pair_in_chunks_and_old_hashes_are_dropped() {
        let (mut host, mut join) = (Sync::every(true, 4), Sync::every(false, 4));
        let at = 20;
        assert!(join.accept(&resync_at_packet(at), 15));
        let pair: Vec<u8> = (0..150_000u32).map(|i| i as u8).collect();
        let packets = resync_packets(&pair);
        assert_eq!(packets.len(), 3);
        for packet in &packets[..2] {
            assert!(join.accept(packet, 18));
            assert!(join.take_state().is_none());
        }
        assert!(join.accept(&packets[2], 19));
        assert_eq!(join.take_state().unwrap(), pair);
        join.resynced(at);
        host.resynced(at);
        assert!(!join.due(20) && join.due(24));
        // A hash from before the resync, still on its way, is ignored.
        assert!(join.accept(&hash_packet(16, 99), 21));
        join.record(16, 1);
        assert_eq!(join.mismatch(), None);
        assert_eq!((host.resyncs(), join.resyncs()), (1, 1));
    }

    #[test]
    fn protocol_breaks_are_refused() {
        let (mut host, mut join) = (Sync::new(true), Sync::new(false));
        assert!(!host.accept(&resync_at_packet(50), 10), "only Host plans");
        assert!(!join.accept(&resync_at_packet(10), 10), "Join already reached that frame");
        assert!(!join.accept(&[KIND_RESYNC, 1, 2], 10), "no pair without a plan");
        assert!(!join.accept(&[KIND_HASH, 1], 10), "short hash");
        assert!(join.accept(&resync_at_packet(30), 10));
        assert!(!join.accept(&resync_at_packet(40), 11), "one plan at a time");
        assert!(is_sync_packet(&[KIND_HASH]) && !is_sync_packet(&[3]));
    }
}

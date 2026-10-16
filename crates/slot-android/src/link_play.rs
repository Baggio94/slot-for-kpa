//! Per-frame emulation side of the original Slot link protocol.
//! The socket worker owns blocking I/O. This adapter only polls packets on
//! GLSurfaceView's GL thread, where libretro, RA evaluation and graphics live.
//! Inspired by Slot emu.rs BeginCable/BeginLink paths (GPL-3.0-or-later).

use slot_retro::{ButtonMask, Link, LinkChannel, RetroCore, NETPACKET_RELIABLE};
use std::time::{Duration, Instant};

use crate::cable::{self, Cable};
use crate::link_kind::LinkKind;
use crate::link_state;
use crate::multiplayer::Role;

const MAX_RECEIVE_PER_FRAME: usize = 256;
const MAX_INITIAL_STATE: usize = 16 * 1024 * 1024;
const MAX_FUTURE_FRAMES: u64 = 256;
// Original Slot cable::state_packets emits at most 60,000 state bytes per
// TCP frame (plus one kind byte). Reject larger or malformed packets before
// they reach Cable::accept, which tolerates duplicate *old input frames*.
const MAX_STATE_CHUNK_PACKET: usize = 60_001;

fn valid_cable_packet_shape(packet: &[u8]) -> bool {
    match packet.first() {
        Some(0) => cable::decode(packet).is_some(),
        Some(1 | 2) => (2..=MAX_STATE_CHUNK_PACKET).contains(&packet.len()),
        Some(3) => packet.len() == 1,
        _ => false,
    }
}


const INITIAL_STATE_TIMEOUT: Duration = Duration::from_secs(12);
// A delayed Mac AVD can miss 60 GL callbacks in ~1 second while TCP is still
// healthy. Use wall-clock time, NOT callback count; never invent peer input.
const CABLE_INPUT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    Disconnected,
    PeerEnded,
    BiosMismatch,
    InvalidState,
    Timeout,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Advanced,
    Waiting,
    Failed(LinkError),
}

/// Diagnostic-only link pacing counters. No gameplay timing or packet
/// scheduling is changed by these measurements. Log every five seconds so
/// AudioTrack underruns can be correlated with network and core bottlenecks.
struct LinkPacing {
    started: Instant,
    attempts: u64,
    advanced: u64,
    paused: u64,
    seeding: u64,
    waiting_input: u64,
    long_calls_16ms: u64,
    long_calls_33ms: u64,
    max_call_us: u64,
    core_total_us: u64,
    core_max_us: u64,
    core_calls: u64,
    packets: u64,
    stall_started: Option<Instant>,
    stall_events: u64,
    max_stall_ms: u64,
}
impl LinkPacing {
    fn new() -> Self {
        Self { started: Instant::now(), attempts: 0, advanced: 0,
            paused: 0, seeding: 0, waiting_input: 0,
            long_calls_16ms: 0, long_calls_33ms: 0, max_call_us: 0,
            core_total_us: 0, core_max_us: 0, core_calls: 0, packets: 0,
            stall_started: None, stall_events: 0, max_stall_ms: 0 }
    }
}

pub struct LinkPlay {
    wire: Box<dyn LinkChannel>,
    cable: Option<Cable>,
    mode: LinkKind,
    net: Link,
    error: Option<LinkError>,
    local_paused: bool,
    state_bytes: usize,
    // Exactly ONE initial state transfer is legal for each TCP Link role.
    // Later state packets could overwrite a running peer or grow unbounded
    // if repeated KIND_STATE_END resets the per-transfer byte counter.
    state_final_received: bool,
    // On linked mGBA, Host must import Join's own battery-backed GBA
    // before sending the authoritative paired state. Never seed a guest
    // with Host's blank peer cartridge.
    awaiting_join_seed: bool,
    started: Instant,
    pacing: LinkPacing,
    // Track an actual contiguous missing-input interval. Unlike the five-
    // second diagnostic window this survives reporting boundaries.
    waiting_for_input_since: Option<Instant>,
    last_poll_at: Instant,
    // TCP guarantees one input packet per sampled future frame; sending the
    // same frame on every waiting draw floods both bounded socket queues.
    last_sent_cable_frame: Option<u64>,
    // The cable protocol deliberately has a 3-frame input delay, as upstream.
    // Wireless passes bytes through the core's registered netpacket ABI.
}

impl LinkPlay {
    /// Host sends the authoritative core state before receiving frame inputs.
    /// The guest restores this state before running any linked frame.
    pub fn begin(
        core: &mut dyn RetroCore,
        role: Role,
        mode: LinkKind,
        mut wire: Box<dyn LinkChannel>,
    ) -> Result<Self, String> {
        let net = core.net();
        net.clear();
        net.set_active(false);
        // Original Slot's Cable fallback models two joypads in a single
        // libretro core. gpSP actually emulates a SECOND GBA through its
        // netpacket interface and needs its serial mode to exchange data;
        // its Cable and Wireless paths must BOTH use netpacket, not lockstep.
        // Keep the original Cable simulation only for upstream mock tests;
        // GameSession rejects any unsupported real core before reaching here.
        let mut awaiting_join_seed = false;
        let cable = match mode {
            LinkKind::Cable if !core.supports_netpacket() => {
                let player = if role == Role::Host { 0 } else { 1 };
                let c = Cable::new(player);
                // Both devices start with their *own* cartridge progress.
                // mGBA's private SLK1 pair contains a blank remote GBA;
                // exchange Join's local half first, then Host combines
                // player 0 + player 1. Upstream single-state mocks retain
                // the old authoritative Host-only protocol.
                let state = core.serialize().map_err(|e| e.to_string())?;
                if state.len() > MAX_INITIAL_STATE {
                    return Err("Link state exceeds the safe transport limit".into());
                }
                if link_state::is_pair(&state) {
                    if role == Role::Host {
                        if link_state::local(&state, 0).is_none() {
                            return Err("Invalid mGBA Host SLK1 state".into());
                        }
                        awaiting_join_seed = true;
                    } else {
                        let own = link_state::local(&state, 1)
                            .ok_or("Invalid mGBA Join SLK1 state")?;
                        if own.len() < 0x400 || own.len() > MAX_INITIAL_STATE / 2 {
                            return Err("Unsafe mGBA Join state size".into());
                        }
                        for packet in cable::state_packets(&own) {
                            wire.send(NETPACKET_RELIABLE, &packet);
                        }
                    }
                } else if role == Role::Host {
                    for packet in cable::state_packets(&state) {
                        wire.send(NETPACKET_RELIABLE, &packet);
                    }
                }
                Some(c)
            }
            LinkKind::Cable | LinkKind::Wireless => {
                if !core.try_start_link(if role == Role::Host { 0 } else { 1 }) {
                    return Err("Emulation core refused the GBA Link peer in its current serial mode".into());
                }
                net.set_active(true);
                None
            }
        };
        Ok(Self {
            wire, cable, mode, net, error: None, local_paused: false, state_bytes: 0,
            state_final_received: false, awaiting_join_seed, started: Instant::now(),
            pacing: LinkPacing::new(),
            waiting_for_input_since: None,
            last_poll_at: Instant::now(),
            last_sent_cable_frame: None,
        })
    }

    pub fn mode(&self) -> LinkKind { self.mode }

    /// The overlay does not advance libretro frames, so the UI must be able
    /// to observe a remote END or TCP loss without calling frame().
    pub fn fault(&self) -> Option<LinkError> {
        self.error.or_else(|| {
            if self.wire.peer_ended() {
                Some(LinkError::PeerEnded)
            } else if self.wire.is_closed() {
                Some(LinkError::Disconnected)
            } else {
                None
            }
        })
    }

    /// Pause changes are transport controls, never gpSP RFU payloads or
    /// mGBA Cable input frames. Only send when the state actually changes.
    pub fn set_local_paused(&mut self, paused: bool) {
        if self.local_paused != paused {
            // A menu interaction is intentional, not a peer-input timeout.
            self.waiting_for_input_since = None;
            self.local_paused = paused;
            self.wire.set_paused(paused);
        }
    }

    pub fn peer_paused(&self) -> bool { self.wire.peer_paused() }


    /// A pending packet must never block a GPU frame. The emulation is held
    /// at the exact same frame until both input masks are available.
    pub fn frame(&mut self, core: &mut dyn RetroCore, buttons: ButtonMask) -> Frame {
        let started = Instant::now();
        let gap = started.saturating_duration_since(self.last_poll_at);
        self.last_poll_at = started;
        if gap > Duration::from_millis(500) && self.waiting_for_input_since.is_some() {
            // The Android GL thread itself did not run during this interval
            // (backgrounding, Mac AVD suspension or device scheduling).
            // Don't count local inactivity as proof that the peer vanished;
            // TCP reader/writer still report real EOF/broken pipes directly.
            self.waiting_for_input_since = None;
            let detail = format!("Link local GL scheduling gap: {gap:?}; suspended input deadline reset");
            #[cfg(target_os = "android")]
            crate::game::log_launch_timing(&detail);
            #[cfg(not(target_os = "android"))]
            eprintln!("SlotKONKR: {detail}");
        }
        let paused = self.local_paused || self.wire.peer_paused();
        let startup = self.awaiting_join_seed || self.cable.as_ref()
            .is_some_and(|cable| !cable.armed());
        let result = self.advance_core_frame(core, buttons);
        let micros = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        let pacing = &mut self.pacing;
        pacing.attempts += 1;
        pacing.max_call_us = pacing.max_call_us.max(micros);
        if micros > 16_667 { pacing.long_calls_16ms += 1; }
        if micros > 33_333 { pacing.long_calls_33ms += 1; }
        match result {
            Frame::Advanced => {
                pacing.advanced += 1;
                if let Some(since) = pacing.stall_started.take() {
                    pacing.stall_events += 1;
                    pacing.max_stall_ms = pacing.max_stall_ms.max(
                        since.elapsed().as_millis().min(u64::MAX as u128) as u64);
                }
            }
            Frame::Waiting => {
                if paused { pacing.paused += 1; }
                else if startup { pacing.seeding += 1; }
                else { pacing.waiting_input += 1; }
                pacing.stall_started.get_or_insert_with(Instant::now);
            }
            Frame::Failed(_) => { pacing.stall_started = None; }
        }
        if pacing.started.elapsed() >= Duration::from_secs(5) {
            let seconds = pacing.started.elapsed().as_secs_f64();
            let avg_core_us = if pacing.core_calls == 0 { 0 }
                else { pacing.core_total_us / pacing.core_calls };
            let report = format!(
                "Link pacing: mode={:?} callbacks={} advanced={} effective_emu_fps={:.1} wait_peer={} wait_setup={} wait_paused={} stall_events={} max_stall_ms={} rx_pkts={} core_us_avg={} core_us_max={} link_call_us_max={} calls_over_16ms={} calls_over_33ms={}",
                self.mode, pacing.attempts, pacing.advanced,
                pacing.advanced as f64 / seconds,
                pacing.waiting_input, pacing.seeding, pacing.paused,
                pacing.stall_events, pacing.max_stall_ms, pacing.packets,
                avg_core_us, pacing.core_max_us, pacing.max_call_us,
                pacing.long_calls_16ms, pacing.long_calls_33ms);
            #[cfg(target_os = "android")]
            crate::game::log_launch_timing(&report);
            #[cfg(not(target_os = "android"))]
            eprintln!("SlotKONKR: {report}");
            *pacing = LinkPacing::new();
        }
        result
    }

    fn advance_core_frame(&mut self, core: &mut dyn RetroCore, buttons: ButtonMask) -> Frame {
        if let Some(error) = self.error { return Frame::Failed(error); }
        if self.wire.peer_ended() { return self.fail(LinkError::PeerEnded); }
        if self.wire.is_closed() { return self.fail(LinkError::Disconnected); }
        // The network reader updates peer_paused even if this core has not
        // advanced a frame for an arbitrarily long pause-menu interaction.
        // Do not pump RFU or sample Cable input until BOTH peers resume.
        if self.local_paused || self.wire.peer_paused() {
            self.waiting_for_input_since = None;
            return Frame::Waiting;
        }

        for _ in 0..MAX_RECEIVE_PER_FRAME {
            let Some(packet) = self.wire.try_recv() else { break };
            self.pacing.packets += 1;
            if let Some(cable) = self.cable.as_mut() {
                // A bad kind, empty chunk, trailing bytes on READY, or an
                // oversized state fragment is a protocol error, not a
                // packet to ignore while the emulators drift apart.
                if !valid_cable_packet_shape(&packet) {
                    return self.fail(LinkError::InvalidState);
                }
                if packet.first() == Some(&3) && self.awaiting_join_seed {
                    // Host cannot be READY until the guest's own GBA state
                    // has been incorporated into the initial SLK1 pair.
                    return self.fail(LinkError::InvalidState);
                }
                if matches!(packet.first(), Some(1 | 2)) {
                    // A second state stream, or any state packet after the
                    // first END, is not part of Cable lockstep. Reject even
                    // when the peer keeps the TCP connection alive.
                    if self.state_final_received ||
                        (cable.primed() && !self.awaiting_join_seed) {
                        return self.fail(LinkError::InvalidState);
                    }
                    self.state_bytes = self.state_bytes.saturating_add(packet.len().saturating_sub(1));
                    if self.state_bytes > MAX_INITIAL_STATE { return self.fail(LinkError::InvalidState); }
                }
                let completed = matches!(packet.first(), Some(2));
                if let Some((frame, _)) = cable::decode(&packet) {
                    if frame > cable.frame().saturating_add(MAX_FUTURE_FRAMES) {
                        return self.fail(LinkError::InvalidState);
                    }
                }
                cable.accept(&packet);
                if completed {
                    self.state_final_received = true;
                    self.state_bytes = 0;
                }
            } else {
                self.net.push_inbound(packet);
            }
        }
        if let Some(cable) = self.cable.as_mut() {
            if self.awaiting_join_seed {
                if let Some(join_state) = cable.take_state() {
                    self.state_bytes = 0;
                    let host_state = match core.serialize() {
                        Ok(state) => state,
                        Err(_) => return self.fail(LinkError::InvalidState),
                    };
                    if !link_state::same_bios(&host_state, &join_state) {
                        return self.fail(LinkError::BiosMismatch);
                    }
                    let combined = match link_state::combine_players(&host_state, &join_state) {
                        Some(state) => state,
                        None => return self.fail(LinkError::InvalidState),
                    };
                    // On a failed restore the patched core rolls BOTH
                    // GBAs back. Never send a potentially invalid pair.
                    if core.unserialize(&combined).is_err() {
                        return self.fail(LinkError::InvalidState);
                    }
                    for packet in cable::state_packets(&combined) {
                        self.wire.send(NETPACKET_RELIABLE, &packet);
                    }
                    self.awaiting_join_seed = false;
                }
                if self.awaiting_join_seed {
                    if self.started.elapsed() >= INITIAL_STATE_TIMEOUT {
                        return self.fail(LinkError::Timeout);
                    }
                    return Frame::Waiting;
                }
            }
            if let Some(state) = cable.take_state() {
                let local = match core.serialize() {
                    Ok(local) => local,
                    Err(_) => return self.fail(LinkError::InvalidState),
                };
                if !link_state::same_bios(&state, &local) {
                    return self.fail(LinkError::BiosMismatch);
                }
                if core.unserialize(&state).is_err() {
                    return self.fail(LinkError::InvalidState);
                }
                cable.prime();
                self.wire.send(NETPACKET_RELIABLE, &cable::ready_packet());
            }
            // Host is primed at construction but MUST NOT advance until
            // Join acknowledges the final combined SLK1 restore.
            if !cable.armed() {
                if self.started.elapsed() >= INITIAL_STATE_TIMEOUT {
                    return self.fail(LinkError::Timeout);
                }
                return Frame::Waiting;
            }
            // TCP is reliable and in-order. Sampling is idempotent while
            // stalled, so send each frame ONCE. A retransmission on every GL
            // callback previously filled the bounded write/receive queues
            // whenever the other device was briefly suspended.
            let frame_index = cable.frame();
            if self.last_sent_cable_frame != Some(frame_index) {
                self.wire.send(NETPACKET_RELIABLE, &cable.sample(buttons));
                self.last_sent_cable_frame = Some(frame_index);
                if self.wire.is_closed() { return self.fail(LinkError::Disconnected); }
            }
            if let Some((p0, p1)) = cable.ready() {
                self.waiting_for_input_since = None;
                let core_started = Instant::now();
                core.run_frame_linked(p0, p1);
                let us = core_started.elapsed().as_micros().min(u64::MAX as u128) as u64;
                self.pacing.core_calls += 1;
                self.pacing.core_total_us = self.pacing.core_total_us.saturating_add(us);
                self.pacing.core_max_us = self.pacing.core_max_us.max(us);
                cable.advance();
                Frame::Advanced
            } else {
                let since = *self.waiting_for_input_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= CABLE_INPUT_TIMEOUT {
                    self.fail(LinkError::Timeout)
                } else {
                    Frame::Waiting
                }
            }
        } else {
            core.pump_link();
            self.flush();
            let core_started = Instant::now();
            core.run_frame(buttons);
            let us = core_started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            self.pacing.core_calls += 1;
            self.pacing.core_total_us = self.pacing.core_total_us.saturating_add(us);
            self.pacing.core_max_us = self.pacing.core_max_us.max(us);
            core.pump_link();
            self.flush();
            Frame::Advanced
        }
    }

    fn flush(&mut self) {
        for _ in 0..MAX_RECEIVE_PER_FRAME {
            let Some(packet) = self.net.take_outbound() else { break };
            self.wire.send(NETPACKET_RELIABLE, &packet);
        }
    }

    fn fail(&mut self, error: LinkError) -> Frame {
        if self.error != Some(error) {
            let elapsed = self.waiting_for_input_since.map_or(0, |t| t.elapsed().as_millis());
            let frame = self.cable.as_ref().map(Cable::frame);
            let detail = format!("Link fault: kind={error:?} mode={:?} frame={frame:?} input_wait_ms={elapsed} peer_paused={} local_paused={} rx_packets_window={}",
                self.mode, self.wire.peer_paused(), self.local_paused, self.pacing.packets);
            #[cfg(target_os = "android")]
            crate::game::log_launch_timing(&detail);
            #[cfg(not(target_os = "android"))]
            eprintln!("SlotKONKR: {detail}");
        }
        self.error = Some(error);
        Frame::Failed(error)
    }

    pub fn end(&mut self, core: &mut dyn RetroCore) {
        if self.cable.is_none() {
            core.stop_link();
        }
        self.wire.send_end();
        self.net.clear();
        self.net.set_active(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::{AtomicBool, Ordering};
    use slot_retro::MockCore;

    struct Wire {
        recv: Arc<Mutex<VecDeque<Vec<u8>>>>,
        send: Arc<Mutex<VecDeque<Vec<u8>>>>,
    }
    impl LinkChannel for Wire {
        fn send(&mut self, _flags: i32, buf: &[u8]) {
            self.send.lock().unwrap().push_back(buf.to_vec());
        }
        fn try_recv(&mut self) -> Option<Vec<u8>> {
            self.recv.lock().unwrap().pop_front()
        }
    }
    fn pair() -> (Box<dyn LinkChannel>, Box<dyn LinkChannel>) {
        let a = Arc::new(Mutex::new(VecDeque::new()));
        let b = Arc::new(Mutex::new(VecDeque::new()));
        (Box::new(Wire { recv: a.clone(), send: b.clone() }),
         Box::new(Wire { recv: b, send: a }))
    }

    #[test]
    fn cable_waits_for_real_wall_clock_timeout_without_retransmitting_duplicate_frames() {
        let inbound = Arc::new(Mutex::new(VecDeque::from([cable::ready_packet().to_vec()])));
        let outbound = Arc::new(Mutex::new(VecDeque::<Vec<u8>>::new()));
        let wire: Box<dyn LinkChannel> = Box::new(Wire {
            recv: inbound, send: outbound.clone(),
        });
        let mut core = MockCore::new();
        let mut link = LinkPlay::begin(&mut core, Role::Host, LinkKind::Cable, wire).unwrap();
        // Three pre-seeded neutral frames never need remote input.
        for _ in 0..cable::DELAY {
            assert_eq!(link.frame(&mut core, ButtonMask(1)), Frame::Advanced);
        }
        let before = core.serialize().unwrap();
        // The remote becomes temporarily non-responsive while TCP is alive.
        assert_eq!(link.frame(&mut core, ButtonMask(1)), Frame::Waiting);
        let count = outbound.lock().unwrap().len();
        for _ in 0..360 {
            assert_eq!(link.frame(&mut core, ButtonMask(3)), Frame::Waiting,
                "Six seconds worth of GL polls must not trigger the old 60-poll timeout");
        }
        assert_eq!(core.serialize().unwrap(), before,
            "Never invent another player's missing Cable input");
        assert_eq!(outbound.lock().unwrap().len(), count,
            "Never resend the same missing frame on every GL callback");
        link.waiting_for_input_since =
            Some(Instant::now() - CABLE_INPUT_TIMEOUT - Duration::from_millis(1));
        assert_eq!(link.frame(&mut core, ButtonMask(4)),
            Frame::Failed(LinkError::Timeout));
        assert_eq!(core.serialize().unwrap(), before);
        assert_eq!(outbound.lock().unwrap().len(), count);
    }

    #[test]
    fn resuming_a_suspended_gl_thread_does_not_mistake_local_sleep_for_peer_loss() {
        let inbound = Arc::new(Mutex::new(VecDeque::from([cable::ready_packet().to_vec()])));
        let outbound = Arc::new(Mutex::new(VecDeque::<Vec<u8>>::new()));
        let wire: Box<dyn LinkChannel> = Box::new(Wire { recv: inbound, send: outbound });
        let mut core = MockCore::new();
        let mut link = LinkPlay::begin(&mut core, Role::Host, LinkKind::Cable, wire).unwrap();
        for _ in 0..cable::DELAY {
            assert_eq!(link.frame(&mut core, ButtonMask(0)), Frame::Advanced);
        }
        assert_eq!(link.frame(&mut core, ButtonMask(0)), Frame::Waiting);
        let original = core.serialize().unwrap();
        link.waiting_for_input_since = Some(Instant::now() - Duration::from_secs(11));
        link.last_poll_at = Instant::now() - Duration::from_secs(11);
        assert_eq!(link.frame(&mut core, ButtonMask(0)), Frame::Waiting);
        assert_eq!(core.serialize().unwrap(), original);
        assert!(link.waiting_for_input_since.is_some());
        assert!(link.waiting_for_input_since.unwrap().elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn cable_pause_clears_missing_input_deadline_without_advancing_the_game() {
        let inbound = Arc::new(Mutex::new(VecDeque::from([cable::ready_packet().to_vec()])));
        let sent = Arc::new(Mutex::new(VecDeque::<Vec<u8>>::new()));
        let wire: Box<dyn LinkChannel> = Box::new(Wire { recv: inbound, send: sent });
        let mut core = MockCore::new();
        let mut link = LinkPlay::begin(&mut core, Role::Host, LinkKind::Cable, wire).unwrap();
        for _ in 0..cable::DELAY {
            assert_eq!(link.frame(&mut core, ButtonMask(0)), Frame::Advanced);
        }
        assert_eq!(link.frame(&mut core, ButtonMask(0)), Frame::Waiting);
        let before = core.serialize().unwrap();
        link.set_local_paused(true);
        link.waiting_for_input_since =
            Some(Instant::now() - CABLE_INPUT_TIMEOUT - Duration::from_millis(1));
        assert_eq!(link.frame(&mut core, ButtonMask(0)), Frame::Waiting);
        assert!(link.waiting_for_input_since.is_none());
        link.set_local_paused(false);
        assert_eq!(link.frame(&mut core, ButtonMask(0)), Frame::Waiting);
        assert!(link.waiting_for_input_since.is_some());
        assert_eq!(core.serialize().unwrap(), before);
    }

    #[test]
    fn pause_menu_stops_netpacket_frames_until_both_players_resume() {
        struct PausableWire {
            sent_local: Arc<AtomicBool>,
            remote: Arc<AtomicBool>,
        }
        impl LinkChannel for PausableWire {
            fn send(&mut self,_flags:i32,_buf:&[u8]){}
            fn try_recv(&mut self)->Option<Vec<u8>>{None}
            fn set_paused(&mut self,paused:bool){self.sent_local.store(paused,Ordering::SeqCst);}
            fn peer_paused(&self)->bool{self.remote.load(Ordering::SeqCst)}
        }
        let remote=Arc::new(AtomicBool::new(false));
        let local=Arc::new(AtomicBool::new(false));
        let mut core=NetworkCore::new();
        let mut link=LinkPlay::begin(&mut core,Role::Host,LinkKind::Wireless,
            Box::new(PausableWire{sent_local:local.clone(),remote:remote.clone()})).unwrap();
        assert_eq!(link.frame(&mut core,ButtonMask(0)),Frame::Advanced);
        let saved=core.serialize().unwrap();
        link.set_local_paused(true);
        assert!(local.load(Ordering::SeqCst));
        for _ in 0..90 {assert_eq!(link.frame(&mut core,ButtonMask(1)),Frame::Waiting);}
        assert_eq!(core.serialize().unwrap(),saved,"local menu must never run the core");
        link.set_local_paused(false);
        remote.store(true,Ordering::SeqCst);
        for _ in 0..90 {assert_eq!(link.frame(&mut core,ButtonMask(2)),Frame::Waiting);}
        assert_eq!(core.serialize().unwrap(),saved,"remote menu must never run the core");
        remote.store(false,Ordering::SeqCst);
        assert_eq!(link.frame(&mut core,ButtonMask(3)),Frame::Advanced);
        assert_ne!(core.serialize().unwrap(),saved,"both returning to game must resume frames");
    }

    #[test]
    fn pause_overlay_observes_remote_end_without_advancing_the_core() {
        let (wire, _closed, ended, _sent_end) = fault_wire();
        let mut core = NetworkCore::new();
        let mut link = LinkPlay::begin(&mut core, Role::Host, LinkKind::Wireless, wire).unwrap();
        link.set_local_paused(true);
        let before = core.serialize().unwrap();
        ended.store(true, Ordering::SeqCst);
        assert_eq!(link.fault(), Some(LinkError::PeerEnded));
        assert_eq!(core.serialize().unwrap(), before);
        assert_eq!(link.frame(&mut core, ButtonMask(0)), Frame::Failed(LinkError::PeerEnded));
        assert_eq!(core.serialize().unwrap(), before);
    }

    // Simulated transport controls let us prove a lost peer NEVER advances
    // either core after failure, even when its socket worker lags a frame.
    struct FaultWire {
        disconnected: Arc<AtomicBool>,
        peer_ended: Arc<AtomicBool>,
        sent_end: Arc<AtomicBool>,
    }
    impl LinkChannel for FaultWire {
        fn send(&mut self, _flags: i32, _buf: &[u8]) {}
        fn try_recv(&mut self) -> Option<Vec<u8>> { None }
        fn is_closed(&self) -> bool { self.disconnected.load(Ordering::SeqCst) }
        fn peer_ended(&self) -> bool { self.peer_ended.load(Ordering::SeqCst) }
        fn send_end(&mut self) { self.sent_end.store(true, Ordering::SeqCst); }
    }
    fn fault_wire() -> (Box<dyn LinkChannel>, Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>) {
        let disconnected = Arc::new(AtomicBool::new(false));
        let peer_ended = Arc::new(AtomicBool::new(false));
        let sent_end = Arc::new(AtomicBool::new(false));
        (Box::new(FaultWire {
            disconnected: disconnected.clone(),
            peer_ended: peer_ended.clone(),
            sent_end: sent_end.clone(),
        }), disconnected, peer_ended, sent_end)
    }

    // Exercises the actual gpSP path, not the original mGBA single-core
    // lockstep simulator. Both visual Cable and Wireless share netpacket.
    struct NetworkCore {
        inner: MockCore,
        channel: Link,
        received: usize,
        began: bool,
        ended: bool,
    }
    impl NetworkCore {
        fn new() -> Self {
            Self { inner: MockCore::new(), channel: Link::default(),
                   received: 0, began: false, ended: false }
        }
    }
    impl RetroCore for NetworkCore {
        fn load(&mut self, path: &std::path::Path) -> Result<(), slot_retro::CoreError> {
            self.inner.load(path)
        }
        fn run_frame(&mut self, input: ButtonMask) {
            self.inner.run_frame(input);
            if self.channel.is_active() {
                self.channel.push_outbound(vec![0x47, input.0 as u8]);
            }
        }
        fn video_xrgb8888(&self) -> &[u8] { self.inner.video_xrgb8888() }
        fn take_audio(&mut self) -> Vec<i16> { self.inner.take_audio() }
        fn serialize(&mut self) -> Result<Vec<u8>, slot_retro::CoreError> {
            self.inner.serialize()
        }
        fn unserialize(&mut self, data: &[u8]) -> Result<(), slot_retro::CoreError> {
            self.inner.unserialize(data)
        }
        fn save_ram(&self) -> Option<Vec<u8>> { self.inner.save_ram() }
        fn load_save_ram(&mut self, data: &[u8]) -> Result<(), slot_retro::CoreError> {
            self.inner.load_save_ram(data)
        }
        fn av_info(&self) -> slot_retro::AvInfo { self.inner.av_info() }
        fn net(&self) -> Link { self.channel.clone() }
        fn supports_netpacket(&self) -> bool { true }
        fn start_link(&mut self, _client_id: u16) { self.began = true; }
        fn try_start_link(&mut self, _client_id: u16) -> bool {
            self.began = true;
            self.channel.set_active(true);
            true
        }
        fn pump_link(&mut self) {
            while self.channel.take_inbound().is_some() { self.received += 1; }
        }
        fn stop_link(&mut self) { self.ended = true; }
    }

    // Minimal dual-GBA adapter: snapshots are real SLK1 containers with
    // distinct player bytes, but no native mGBA is needed for protocol tests.
    struct PairedCore {
        state: Vec<u8>,
        steps: usize,
    }
    impl PairedCore {
        fn new(progress: u8, bios: u32) -> Self {
            let mut single = vec![progress; 0x800];
            single[4..8].copy_from_slice(&bios.to_le_bytes());
            Self { state: link_state::pair(&single), steps: 0 }
        }
    }
    impl RetroCore for PairedCore {
        fn load(&mut self, _: &std::path::Path) -> Result<(), slot_retro::CoreError> { Ok(()) }
        fn run_frame(&mut self, _: ButtonMask) { self.steps += 1; }
        fn run_frame_linked(&mut self, _: ButtonMask, _: ButtonMask) { self.steps += 1; }
        fn video_xrgb8888(&self) -> &[u8] { &[] }
        fn take_audio(&mut self) -> Vec<i16> { Vec::new() }
        fn serialize(&mut self) -> Result<Vec<u8>, slot_retro::CoreError> {
            Ok(self.state.clone())
        }
        fn unserialize(&mut self, state: &[u8]) -> Result<(), slot_retro::CoreError> {
            if link_state::local(state, 0).is_none()
                || link_state::local(state, 1).is_none() {
                return Err(slot_retro::CoreError::State("not a pair".into()));
            }
            self.state = state.to_vec();
            Ok(())
        }
        fn save_ram(&self) -> Option<Vec<u8>> { None }
        fn load_save_ram(&mut self, _: &[u8]) -> Result<(), slot_retro::CoreError> { Ok(()) }
        fn av_info(&self) -> slot_retro::AvInfo {
            slot_retro::AvInfo { fps: 60.0, sample_rate: 32768.0 }
        }
    }

    #[test]
    fn mga_link_merges_distinct_host_and_join_saves_before_first_frame() {
        let (host_wire, guest_wire) = pair();
        let (mut host, mut guest) = (PairedCore::new(0x11, 0xBAAE187F),
                                      PairedCore::new(0x22, 0xBAAE187F));
        let mut h = LinkPlay::begin(&mut host, Role::Host, LinkKind::Cable, host_wire).unwrap();
        // Never advance Host's blank peer before Join supplies its GBA.
        assert_eq!(h.frame(&mut host, ButtonMask(1)), Frame::Waiting);
        assert_eq!(host.steps, 0);
        let mut g = LinkPlay::begin(&mut guest, Role::Join, LinkKind::Cable, guest_wire).unwrap();
        assert_eq!(h.frame(&mut host, ButtonMask(2)), Frame::Waiting);
        assert_eq!(host.steps, 0);
        let host_local = link_state::local(&host.state, 0).unwrap();
        let guest_local = link_state::local(&host.state, 1).unwrap();
        assert_eq!(host_local[12], 0x11);
        assert_eq!(guest_local[12], 0x22);
        // Join must restore the combined pair before receiving any frame.
        assert_eq!(g.frame(&mut guest, ButtonMask(3)), Frame::Advanced);
        assert_eq!(g.state_bytes, 0);
        assert_eq!(h.frame(&mut host, ButtonMask(4)), Frame::Advanced);
        assert_eq!(host.state, guest.state);
        assert_eq!(host.steps, 1);
        assert_eq!(guest.steps, 1);
    }

    #[test]
    fn real_tcp_mgba_link_preserves_two_distinct_solo_states_and_stops_on_end() {
        use crate::link_net::{Cancel, LabGreeting, TcpLink};
        use std::net::{SocketAddr, TcpListener};
        use std::thread;

        // Exercise actual KMLB/2 + framed TCP + reader/writer threads +
        // Slot's paired-GBA seed exchange and lockstep in ONE test.
        // This does not require a Mac AVD, physical KONKR or a ROM.
        let reserved = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address: SocketAddr = reserved.local_addr().unwrap();
        drop(reserved);
        let fingerprint = LabGreeting {
            game: *b"AMAE", backend: 1, mode: 1,
            rom_sha256: [0x11; 32], core_sha256: [0x22; 32],
            bios_sha256: [0x33; 32],
        };
        let host_identity = fingerprint.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let server = thread::spawn(move || TcpLink::host_lab_greeted_until(
            "127.0.0.1", address.port(), Duration::from_secs(5),
            &Cancel::new(), &host_identity, || { let _ = ready_tx.send(()); },
        ).unwrap());
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let guest_wire = TcpLink::join_lab_greeted(address, &fingerprint).unwrap();
        let host_wire = server.join().unwrap();
        let mut host = PairedCore::new(0x11, 0xBAAE187F);
        let mut guest = PairedCore::new(0x22, 0xBAAE187F);
        let mut h = LinkPlay::begin(&mut host, Role::Host, LinkKind::Cable,
            Box::new(host_wire)).unwrap();
        let mut g = LinkPlay::begin(&mut guest, Role::Join, LinkKind::Cable,
            Box::new(guest_wire)).unwrap();
        assert_eq!(host.steps, 0);
        assert_eq!(guest.steps, 0);
        let end = Instant::now() + Duration::from_secs(4);
        while Instant::now() < end && (host.steps == 0 || guest.steps == 0) {
            let a = h.frame(&mut host, ButtonMask(0x01));
            let b = g.frame(&mut guest, ButtonMask(0x02));
            assert!(!matches!(a, Frame::Failed(_)), "Host link failed: {a:?}");
            assert!(!matches!(b, Frame::Failed(_)), "Join link failed: {b:?}");
            thread::sleep(Duration::from_millis(5));
        }
        assert!(host.steps >= 1 && guest.steps >= 1,
            "real TCP peers must both advance after transferring two solo saves");
        assert_eq!(link_state::local(&host.state, 0).unwrap()[12], 0x11);
        assert_eq!(link_state::local(&host.state, 1).unwrap()[12], 0x22);
        assert_eq!(host.state, guest.state);

        g.end(&mut guest);
        // TCP END is delivered asynchronously. Wait for the reader worker to
        // observe it *before* asserting the no-more-frames invariant: valid
        // frames already in flight before End are not unsynchronised solo.
        let stop = Instant::now() + Duration::from_secs(3);
        while Instant::now() < stop &&
            !h.wire.peer_ended() && !h.wire.is_closed() {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(h.wire.peer_ended() || h.wire.is_closed(),
            "TCP End must reach the Host reader");
        let before = host.steps;
        assert!(matches!(h.frame(&mut host, ButtonMask(0)),
            Frame::Failed(LinkError::PeerEnded | LinkError::Disconnected)));
        assert_eq!(host.steps, before, "Host must not run a solo frame after End");
    }

    #[test]
    fn cable_wire_packet_shapes_match_upstream_and_reject_unknown_kinds() {
        assert!(valid_cable_packet_shape(&cable::encode(0, ButtonMask(0))));
        assert!(valid_cable_packet_shape(&cable::ready_packet()));
        assert!(cable::state_packets(&vec![7u8; 60_000]).iter()
            .all(|p| valid_cable_packet_shape(p)));
        assert!(!valid_cable_packet_shape(&[]));
        assert!(!valid_cable_packet_shape(&[0]));
        assert!(!valid_cable_packet_shape(&[3, 0]));
        assert!(!valid_cable_packet_shape(&[4]));
        assert!(!valid_cable_packet_shape(&[1]));
        assert!(!valid_cable_packet_shape(&[2]));
        assert!(!valid_cable_packet_shape(&vec![1u8; MAX_STATE_CHUNK_PACKET + 1]));
    }

    #[test]
    fn malformed_cable_packets_cannot_mutate_or_advance_mgba() {
        for malformed in [
            vec![0u8], vec![3, 0], vec![0xFF], vec![1], vec![2],
            vec![1u8; MAX_STATE_CHUNK_PACKET + 1],
        ] {
            let received = Arc::new(Mutex::new(VecDeque::from([malformed])));
            let sent = Arc::new(Mutex::new(VecDeque::new()));
            let wire: Box<dyn LinkChannel> = Box::new(Wire {recv: received, send: sent});
            let mut core = PairedCore::new(0x44, 0xBAAE187F);
            let before = core.state.clone();
            let mut link = LinkPlay::begin(
                &mut core, Role::Join, LinkKind::Cable, wire).unwrap();
            assert_eq!(link.frame(&mut core, ButtonMask(1)),
                Frame::Failed(LinkError::InvalidState));
            assert_eq!(core.state, before);
            assert_eq!(core.steps, 0);
        }
    }

    #[test]
    fn duplicate_initial_state_end_is_rejected_without_replacing_live_cartridges() {
        let inbound = Arc::new(Mutex::new(VecDeque::<Vec<u8>>::new()));
        let outbound = Arc::new(Mutex::new(VecDeque::<Vec<u8>>::new()));
        let wire: Box<dyn LinkChannel> = Box::new(Wire {
            recv: inbound.clone(), send: outbound,
        });
        let mut host = PairedCore::new(0x11, 0xBAAE187F);
        let before = host.state.clone();
        let mut link = LinkPlay::begin(&mut host, Role::Host, LinkKind::Cable, wire).unwrap();
        let guest = PairedCore::new(0x22, 0xBAAE187F);
        let guest_state = link_state::local(&guest.state, 1).unwrap();
        let initial = cable::state_packets(&guest_state);
        assert_eq!(initial.len(), 1, "fixture must use one final packet");
        let mut queue = inbound.lock().unwrap();
        queue.push_back(initial[0].clone());
        queue.push_back(initial[0].clone());
        drop(queue);
        assert_eq!(link.frame(&mut host, ButtonMask(0)),
            Frame::Failed(LinkError::InvalidState));
        assert_eq!(host.steps, 0);
        assert_eq!(host.state, before,
            "a repeated initial STATE_END cannot rewrite both GBAs");
        assert_eq!(link.frame(&mut host, ButtonMask(0)),
            Frame::Failed(LinkError::InvalidState));
    }

    #[test]
    fn mga_link_rejects_guest_with_different_bios_without_replacing_host() {
        let (host_wire, guest_wire) = pair();
        let (mut host, mut guest) = (PairedCore::new(0x11, 0xBAAE187F),
                                      PairedCore::new(0x22, 0xA6473709));
        let before = host.state.clone();
        let mut h = LinkPlay::begin(&mut host, Role::Host, LinkKind::Cable, host_wire).unwrap();
        let _g = LinkPlay::begin(&mut guest, Role::Join, LinkKind::Cable, guest_wire).unwrap();
        assert_eq!(h.frame(&mut host, ButtonMask(0)), Frame::Failed(LinkError::BiosMismatch));
        assert_eq!(host.state, before);
        assert_eq!(host.steps, 0);
    }

    #[test]
    fn gpsp_netpacket_carries_both_cable_and_wireless_without_input_replay() {
        for mode in [LinkKind::Cable, LinkKind::Wireless] {
            let (hw, gw) = pair();
            let (mut host, mut guest) = (NetworkCore::new(), NetworkCore::new());
            let mut h = LinkPlay::begin(&mut host, Role::Host, mode, hw).unwrap();
            let mut g = LinkPlay::begin(&mut guest, Role::Join, mode, gw).unwrap();
            assert!(host.began && guest.began);
            assert_eq!(h.mode(), mode);
            assert_eq!(g.mode(), mode);
            assert_eq!(h.frame(&mut host, ButtonMask(10)), Frame::Advanced);
            assert_eq!(g.frame(&mut guest, ButtonMask(11)), Frame::Advanced);
            assert_eq!(guest.received, 1, "mode={mode:?} must deliver host netpacket");
            assert_eq!(h.frame(&mut host, ButtonMask(12)), Frame::Advanced);
            assert_eq!(host.received, 1, "mode={mode:?} must deliver guest netpacket");
            h.end(&mut host);
            g.end(&mut guest);
            assert!(host.ended && guest.ended);
        }
    }

    #[test]
    fn host_and_guest_share_exact_snapshot_before_linked_frames() {
        let mut host = MockCore::new();
        let mut guest = MockCore::new();
        for _ in 0..9 { host.run_frame(ButtonMask(0)); }
        let (host_wire, guest_wire) = pair();
        let mut h = LinkPlay::begin(&mut host, Role::Host, LinkKind::Cable, host_wire).unwrap();
        let mut g = LinkPlay::begin(&mut guest, Role::Join, LinkKind::Cable, guest_wire).unwrap();
        assert_eq!(g.frame(&mut guest, ButtonMask(1)), Frame::Advanced);
        assert_eq!(h.frame(&mut host, ButtonMask(2)), Frame::Advanced);
        assert_eq!(host.serialize().unwrap(), guest.serialize().unwrap());
        for _ in 0..3 {
            let _ = g.frame(&mut guest, ButtonMask(1));
            let _ = h.frame(&mut host, ButtonMask(2));
        }
        assert_eq!(host.serialize().unwrap(), guest.serialize().unwrap());
        h.end(&mut host);
        g.end(&mut guest);
    }

    #[test]
    fn lost_gpsp_peer_never_advances_after_disconnect_in_either_serial_mode() {
        for mode in [LinkKind::Cable, LinkKind::Wireless] {
            let (wire, closed, _peer_ended, sent_end) = fault_wire();
            let mut core = NetworkCore::new();
            let mut link = LinkPlay::begin(&mut core, Role::Join, mode, wire).unwrap();
            assert!(core.channel.is_active());
            assert_eq!(link.frame(&mut core, ButtonMask(1)), Frame::Advanced);
            let before = core.serialize().unwrap();
            closed.store(true, Ordering::SeqCst);
            for _ in 0..3 {
                assert_eq!(link.frame(&mut core, ButtonMask(2)),
                    Frame::Failed(LinkError::Disconnected));
                assert_eq!(core.serialize().unwrap(), before,
                    "a disconnected gpSP {mode:?} core must not emulate a solo frame");
            }
            link.end(&mut core);
            assert!(core.ended);
            assert!(sent_end.load(Ordering::SeqCst));
            assert!(!core.channel.is_active());
        }
    }

    #[test]
    fn ended_mgba_peer_keeps_both_cartridges_unchanged_until_safe_eject() {
        let (wire, _closed, peer_ended, sent_end) = fault_wire();
        let mut core = PairedCore::new(0x42, 0xBAAE187F);
        let mut link = LinkPlay::begin(&mut core, Role::Host, LinkKind::Cable, wire).unwrap();
        let before = core.serialize().unwrap();
        assert_eq!(link.frame(&mut core, ButtonMask(1)), Frame::Waiting);
        peer_ended.store(true, Ordering::SeqCst);
        assert_eq!(link.frame(&mut core, ButtonMask(1)),
            Frame::Failed(LinkError::PeerEnded));
        assert_eq!(link.frame(&mut core, ButtonMask(1)),
            Frame::Failed(LinkError::PeerEnded));
        assert_eq!(core.serialize().unwrap(), before);
        assert_eq!(core.steps, 0);
        link.end(&mut core);
        assert!(sent_end.load(Ordering::SeqCst));
    }

    #[test]
    fn empty_network_does_not_advance_guest_or_serialize_bad_state() {
        let mut core = MockCore::new();
        let (wire, _other) = pair();
        let mut link = LinkPlay::begin(&mut core, Role::Join, LinkKind::Cable, wire).unwrap();
        let before = core.serialize().unwrap();
        assert_eq!(link.frame(&mut core, ButtonMask(1)), Frame::Waiting);
        assert_eq!(before, core.serialize().unwrap());
    }
}

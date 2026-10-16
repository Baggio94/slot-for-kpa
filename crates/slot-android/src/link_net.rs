//! GPL-3.0-or-later: fork LAN greeting and original Slot TCP packet framing.
//! Source: https://github.com/pvaibhav/slot/blob/pvaibhav-improvements/crates/slot/src/link_net.rs
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::{fs, thread};
use ring::digest::{digest, SHA256};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use slot_retro::LinkChannel;

#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Cancel {
        Cancel::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn report_link_transport_fault(stage: &str, reason: &str) {
    let message = format!("Link transport fault: stage={stage} reason={reason}");
    #[cfg(target_os = "android")]
    crate::game::log_launch_timing(&message);
    #[cfg(not(target_os = "android"))]
    eprintln!("SlotKONKR: {message}");
}

/// The KMLB/4 trace contains metadata only; never record ROM bytes or hashes.
fn report_link_rom_phase(stage: &str, details: &str) {
    let message = format!("KMLB4 ROM exchange: stage={stage} {details}");
    #[cfg(target_os = "android")]
    crate::game::log_launch_timing(&message);
    #[cfg(not(target_os = "android"))]
    eprintln!("SlotKONKR: {message}");
}
const POLL_MS: u64 = 50;

pub const HOST_BOUND: Duration = Duration::from_secs(30);

/// How long either end gives the other to say or answer the greeting. Short: it is one small
/// packet on a network measured in milliseconds, and a host that cannot manage it in this time
/// is not the one this joiner wants.
const GREET_WAIT: Duration = Duration::from_millis(1500);

/// A greeting is `SLOT`, the greeting's own version, then the game's four-byte header code.
const GREETING_LEN: usize = 9;
const GREETING_VERSION: u8 = 1;
/// What a host answers a greeting it accepts with.
const GREETING_ACK: u8 = 0x06;

/// What opens a connection on a shared network. The game rides along so that two handhelds that
/// found each other but are not running the same cart never become a session; on the private
/// network nothing else could have connected, so nothing needed asking.
fn greeting(game: &str) -> [u8; GREETING_LEN] {
    let mut g = [b' '; GREETING_LEN];
    g[..4].copy_from_slice(b"SLOT");
    g[4] = GREETING_VERSION;
    for (slot, byte) in g[5..].iter_mut().zip(game.bytes()) {
        *slot = byte;
    }
    g
}

/// Read a stranger's greeting and answer it, or say it was not the right one. Never waits
/// longer than `GREET_WAIT`, so one connection that says nothing cannot hold the host up.
fn vet(stream: &mut TcpStream, want: &[u8]) -> bool {
    let mut got = vec![0u8; want.len()];
    stream.set_read_timeout(Some(GREET_WAIT)).is_ok()
        && stream.set_write_timeout(Some(GREET_WAIT)).is_ok()
        && stream.read_exact(&mut got).is_ok()
        && got == want
        && stream.write_all(&[GREETING_ACK]).is_ok()
        && stream.set_read_timeout(None).is_ok()
        && stream.set_write_timeout(None).is_ok()
}

/// Lab wire protocol is intentionally NOT compatible with legacy SLOT/1.
/// A legacy physical Beta5 must never reach Ready or change its emulation core.
const LAB_GREETING_LEN: usize = 4 + 1 + 4 + 1 + 1 + 32 + 32 + 32;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabGreeting {
    pub game: [u8; 4],
    pub backend: u8,
    pub mode: u8,
    pub rom_sha256: [u8; 32],
    pub core_sha256: [u8; 32],
    pub bios_sha256: [u8; 32],
}
impl LabGreeting {
    fn bytes(&self) -> [u8; LAB_GREETING_LEN] {
        let mut bytes = [0u8; LAB_GREETING_LEN];
        bytes[..4].copy_from_slice(b"KMLB");
        bytes[4] = if self.backend == 1 { 5 } else { 3 };
        // KMLB/5: signed device identity precedes mGBA mixed-ROM KMLB/4 transfer.
        // Refuse legacy v4 peers instead of mixing framed setup traffic.
        // gpSP KMLB/3 remains unchanged; both formats reject old peers.
        bytes[5..9].copy_from_slice(&self.game);
        bytes[9] = self.backend;
        bytes[10] = self.mode;
        bytes[11..43].copy_from_slice(&self.rom_sha256);
        bytes[43..75].copy_from_slice(&self.core_sha256);
        bytes[75..107].copy_from_slice(&self.bios_sha256);
        bytes
    }
    fn decode(bytes:&[u8])->Option<Self>{
        if bytes.len()!=LAB_GREETING_LEN || &bytes[0..4]!=b"KMLB" ||
            !matches!((bytes[4], bytes[9]), (5, 1) | (3, 2)) {return None;}
        let mut game=[0;4]; game.copy_from_slice(&bytes[5..9]);
        let mut rom_sha256=[0;32];rom_sha256.copy_from_slice(&bytes[11..43]);
        let mut core_sha256=[0;32];core_sha256.copy_from_slice(&bytes[43..75]);
        let mut bios_sha256=[0;32];bios_sha256.copy_from_slice(&bytes[75..107]);
        Some(Self{game,backend:bytes[9],mode:bytes[10],rom_sha256,core_sha256,bios_sha256})
    }
    fn accepts(&self,other:&Self)->bool{
        // A matching ROM hash must not make an impossible RFU adapter appear
        // on an unsupported cartridge such as Ruby/Sapphire.
        if self.mode==2 && (!crate::link_kind::has_wireless_adapter(
            std::str::from_utf8(&self.game).unwrap_or(""))
            || !crate::link_kind::has_wireless_adapter(
            std::str::from_utf8(&other.game).unwrap_or(""))) { return false; }
        if self.backend!=other.backend || self.mode!=other.mode ||
            self.core_sha256!=other.core_sha256 || self.bios_sha256!=other.bios_sha256{return false;}
        let mode=match self.mode{1=>crate::link_kind::LinkKind::Cable,
            2=>crate::link_kind::LinkKind::Wireless,_=>return false};
        if self.backend==2 {
            for code in [self.game, other.game] {
                let Ok(code) = std::str::from_utf8(&code) else { return false; };
                if !crate::link_kind::gpsp_runtime_mode_supported(code, mode) {return false;}
            }
        }
        if self.game==other.game{return self.rom_sha256==other.rom_sha256;}
        match self.backend {
            1 => mode == crate::link_kind::LinkKind::Cable
                && crate::link_compat::compatible_cross_title(self.game, other.game, mode),
            2 => crate::link_compat::runnable_gpsp_cross_title(self.game, other.game, mode),
            _ => false,
        }
    }
}
fn vet_lab(stream:&mut TcpStream,local:&LabGreeting)->Option<LabGreeting>{
    let mut bytes=[0u8;LAB_GREETING_LEN];
    if stream.set_read_timeout(Some(GREET_WAIT)).is_err() ||
        stream.set_write_timeout(Some(GREET_WAIT)).is_err() ||
        stream.read_exact(&mut bytes).is_err(){return None;}
    let peer=LabGreeting::decode(&bytes)?;
    if !local.accepts(&peer){return None;}
    if stream.write_all(&[GREETING_ACK]).is_err() ||
        stream.write_all(&local.bytes()).is_err(){return None;}
    if stream.set_read_timeout(None).is_err() ||
        stream.set_write_timeout(None).is_err(){return None;}
    Some(peer)
}

const CONTROL_ENDED: u8 = 0x00;
const CONTROL_PAUSED: u8 = 0x01;
const CONTROL_RESUMED: u8 = 0x02;

const BYE_MS: u64 = 100;

/// A paused core does not drain incoming serial packets. Bound both worker
/// queues so a stalled/malicious peer cannot grow the process heap forever.
/// A 16 MiB initial mGBA state needs at most 280 chunks of <=60 KiB; 512
/// leaves headroom for that one legal transfer and some in-flight inputs.
/// Saturation is fatal rather than silently losing a lockstep/RFU packet.
const MAX_QUEUED_PACKETS: usize = 512;

enum Out {
    Packet(Vec<u8>),
    Control(u8, Sender<()>),
}

pub struct TcpLink {
    outbox: SyncSender<Out>,
    inbox: Receiver<Vec<u8>>,
    stream: TcpStream,
    closed: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
    peer_paused: Arc<AtomicBool>,
    peer_identity: Option<LabGreeting>,
}

impl TcpLink {
    /// WiFi a link session actually runs over. On the private network that is the whole
    /// defence, since only the other handheld is there to connect; on the home network
    /// `host_greeted_until` is the one to use, which also turns away anything that does not
    /// open with the right greeting. See the module doc for why this transport does not
    /// hardcode which address that is.
    pub fn host_until(
        addr: &str,
        port: u16,
        bound: Duration,
        cancel: &Cancel,
    ) -> std::io::Result<TcpLink> {
        TcpLink::host_inner(addr, port, bound, cancel, None, None, || {})
    }

    /// `host_until` for a shared network, where anything on it can reach the listener. Only a
    /// peer that opens with this game's greeting is accepted; anything else is dropped without
    /// a word and the wait goes on, still bounded. `listening` runs once the port is bound and
    /// before the first wait, which is when a host on a shared network announces itself: a
    /// joiner that found it any sooner would be refused.
    pub fn host_greeted_until(
        addr: &str,
        port: u16,
        bound: Duration,
        cancel: &Cancel,
        game: &str,
        listening: impl FnOnce(),
    ) -> std::io::Result<TcpLink> {
        TcpLink::host_inner(addr, port, bound, cancel, Some(greeting(game).to_vec()), None, listening)
    }

    /// Lab-only strict negotiation: backend, Cable/Wireless mode, exact ROM,
    /// compiled libretro core and active GBA BIOS must all match BEFORE Ready.
    pub fn host_lab_greeted_until(
        addr: &str, port: u16, bound: Duration, cancel: &Cancel,
        identity: &LabGreeting, listening: impl FnOnce(),
    ) -> std::io::Result<TcpLink> {
        TcpLink::host_inner(addr, port, bound, cancel,
            None, Some(identity.clone()), listening)
    }

    fn host_inner(
        addr: &str,
        port: u16,
        bound: Duration,
        cancel: &Cancel,
        expect: Option<Vec<u8>>,
        lab: Option<LabGreeting>,
        listening: impl FnOnce(),
    ) -> std::io::Result<TcpLink> {
        let listener = TcpListener::bind((addr, port))?;
        listener.set_nonblocking(true)?;
        listening();
        let deadline = Instant::now() + bound;
        loop {
            if cancel.is_cancelled() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "cancelled while waiting for a peer",
                ));
            }
            match listener.accept() {
                Ok((mut stream, _peer)) => {
                    stream.set_nonblocking(false)?;
                    let peer_identity = if let Some(ref local) = lab {
                        let Some(remote) = vet_lab(&mut stream, local) else { continue; };
                        Some(remote)
                    } else { None };
                    if let Some(ref want) = expect {
                        // Vetted before it is anyone's session: a stranger, a scanner or the
                        // wrong game gets dropped here and the host keeps waiting for the
                        // right one.
                        if !vet(&mut stream, want) {
                            continue;
                        }
                    }
                    return TcpLink::wrap_identified(stream, peer_identity);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "no peer arrived",
                ));
            }
            std::thread::sleep(Duration::from_millis(POLL_MS));
        }
    }

    pub fn host(addr: &str, port: u16) -> std::io::Result<TcpLink> {
        TcpLink::host_until(addr, port, HOST_BOUND, &Cancel::new())
    }

    pub fn join_until(
        addr: &str,
        port: u16,
        bound: Duration,
        cancel: &Cancel,
    ) -> std::io::Result<TcpLink> {
        let deadline = Instant::now() + bound;
        loop {
            if cancel.is_cancelled() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "cancelled while reaching for the host",
                ));
            }
            if let Ok(stream) = TcpStream::connect((addr, port)) {
                return TcpLink::wrap(stream);
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "never reached the host",
                ));
            }
            std::thread::sleep(Duration::from_millis(POLL_MS));
        }
    }

    pub fn join(addr: &str, port: u16) -> std::io::Result<TcpLink> {
        TcpLink::wrap(TcpStream::connect((addr, port))?)
    }

    /// One attempt to reach a host on a shared network: connect, open with this game's
    /// greeting, and wait for the host to accept it. Both waits are short and neither retries,
    /// because the caller is walking a list of hosts that answered and the next one is the
    /// retry. A host that turns it away closes the socket, which reads here as an error.
    pub fn join_greeted(addr: std::net::SocketAddr, game: &str) -> std::io::Result<TcpLink> {
        TcpLink::join_with_greeting(addr, &greeting(game))
    }

    pub fn join_lab_greeted(addr: std::net::SocketAddr,
                            identity: &LabGreeting) -> std::io::Result<TcpLink> {
        {
            let mut stream=TcpStream::connect_timeout(&addr,GREET_WAIT)?;
            stream.set_write_timeout(Some(GREET_WAIT))?;
            stream.write_all(&identity.bytes())?;
            stream.set_read_timeout(Some(GREET_WAIT))?;
            let mut ack=[0u8;1];stream.read_exact(&mut ack)?;
            if ack[0]!=GREETING_ACK {return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,"host refused KMLB/3 greeting"));}
            let mut bytes=[0u8;LAB_GREETING_LEN];stream.read_exact(&mut bytes)?;
            let host=LabGreeting::decode(&bytes).ok_or_else(||
                std::io::Error::new(std::io::ErrorKind::InvalidData,"invalid KMLB/3 host identity"))?;
            if !identity.accepts(&host){return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,"incompatible KMLB/3 host identity"));}
            stream.set_read_timeout(None)?;stream.set_write_timeout(None)?;
            TcpLink::wrap_identified(stream, Some(host))
        }
    }

    fn join_with_greeting(addr: std::net::SocketAddr, bytes: &[u8]) -> std::io::Result<TcpLink> {
        let mut stream = TcpStream::connect_timeout(&addr, GREET_WAIT)?;
        stream.set_write_timeout(Some(GREET_WAIT))?;
        stream.write_all(bytes)?;
        stream.set_read_timeout(Some(GREET_WAIT))?;
        let mut ack = [0u8; 1];
        stream.read_exact(&mut ack)?;
        if ack[0] != GREETING_ACK {
            return Err(std::io::Error::other("host did not accept the greeting"));
        }
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(None)?;
        TcpLink::wrap(stream)
    }

    fn wrap(stream: TcpStream) -> std::io::Result<TcpLink> {
        Self::wrap_identified(stream, None)
    }

    fn wrap_identified(stream: TcpStream, peer_identity: Option<LabGreeting>)
        -> std::io::Result<TcpLink> {
        stream.set_nodelay(true)?;
        let mut reader = stream.try_clone()?;
        let mut writer = stream.try_clone()?;
        let (rtx, inbox) = sync_channel(MAX_QUEUED_PACKETS);
        let (wtx, wrx) = sync_channel::<Out>(MAX_QUEUED_PACKETS);
        let closed = Arc::new(AtomicBool::new(false));
        let reader_closed = closed.clone();
        let ended = Arc::new(AtomicBool::new(false));
        let reader_ended = ended.clone();
        let peer_paused = Arc::new(AtomicBool::new(false));
        let reader_paused = peer_paused.clone();

        std::thread::spawn(move || {
            let mut header = [0u8; 2];
            let mut control = false;
            loop {
                if let Err(error) = reader.read_exact(&mut header) {
                    report_link_transport_fault("tcp_read_header", &error.to_string());
                    reader_closed.store(true, Ordering::Release);
                    return;
                }
                let len = u16::from_be_bytes(header) as usize;
                if len == 0 && !control {
                    control = true;
                    continue;
                }
                let mut buf = vec![0u8; len];
                if len > 0 {
                    if let Err(error) = reader.read_exact(&mut buf) {
                        report_link_transport_fault("tcp_read_body", &error.to_string());
                        reader_closed.store(true, Ordering::Release);
                        return;
                    }
                }
                if std::mem::take(&mut control) {
                    match buf.as_slice() {
                        [CONTROL_ENDED] => reader_ended.store(true, Ordering::Release),
                        [CONTROL_PAUSED] => reader_paused.store(true, Ordering::Release),
                        [CONTROL_RESUMED] => reader_paused.store(false, Ordering::Release),
                        _ => {
                            report_link_transport_fault("tcp_control", "invalid control opcode");
                            reader_closed.store(true, Ordering::Release);
                            return;
                        },
                    }
                    continue;
                }
                if rtx.try_send(buf).is_err() {
                    report_link_transport_fault("rx_queue", "saturated or disconnected");
                    // Never drop just one packet and let linked cores
                    // diverge. Close the entire link when the receiver has
                    // stopped draining its finite queue.
                    reader_closed.store(true, Ordering::Release);
                    let _ = reader.shutdown(Shutdown::Both);
                    return;
                }
            }
        });

        let writer_closed = closed.clone();
        std::thread::spawn(move || {
            for out in wrx.iter() {
                let sent = match out {
                    Out::Packet(buf) => write_packet_frame(&mut writer, &buf),
                    Out::Control(op, ack) => {
                        let result = writer.write_all(&[0, 0, 0, 1, op]);
                        if result.is_ok() { let _ = ack.send(()); }
                        result
                    }
                };
                if let Err(error) = sent {
                    report_link_transport_fault("tcp_write", &error.to_string());
                    // A broken write side is a dead Link even when the reader
                    // has not yet observed EOF. Never keep advancing gpSP
                    // serial/RFU frames after a local TCP write failure.
                    writer_closed.store(true, Ordering::Release);
                    let _ = writer.shutdown(Shutdown::Both);
                    return;
                }
            }
        });

        Ok(TcpLink {
            outbox: wtx,
            inbox,
            stream,
            closed,
            ended,
            peer_paused,
            peer_identity,
        })
    }

    /// Authenticated against the local backend/core/BIOS before the socket
    /// ever became visible to the link session. Used by KMLB/4 to prove the
    /// remote ROM transfer matches the negotiated SHA-256 and game code.
    pub fn peer_identity(&self) -> Option<&LabGreeting> {
        self.peer_identity.as_ref()
    }

    /// KMLB/4 only. Transfer the other player's cartridge into a PRIVATE
    /// cache on the background LinkStarter worker. The game never sees
    /// LINKED until the full file/header/SHA has passed. No ROM is written to
    /// the user's selected library. This is not part of gpSP KMLB/3.
    pub fn exchange_mgba_peer_rom(&mut self, local: &LabGreeting,
                                  own_rom: &Path, cancel: &Cancel)
        -> std::io::Result<Option<PathBuf>> {
        fn invalid(message: &str) -> std::io::Error {
            std::io::Error::new(std::io::ErrorKind::InvalidData, message)
        }
        const MAX_ROM: usize = 32 * 1024 * 1024;
        const CHUNK: usize = 60 * 1024;
        // A13 field traces show repeat disconnects about 8s after ROM
        // consent; allow slow Wi-Fi/AVD transfers without unbounded waits.
        const DEADLINE: Duration = Duration::from_secs(180);
        const PACKET_WAIT: Duration = Duration::from_secs(20);
        let peer = self.peer_identity().ok_or_else(||
            invalid("KMLB/4 missing verified peer greeting"))?.clone();
        if local.backend != 1 || peer.backend != 1 || local.mode != 1
            || !local.accepts(&peer) {
            return Err(invalid("Not a negotiated mGBA Cable pair"));
        }
        // Same-ROM Cable must stay byte-for-byte on its proven a7 path.
        // When different codes negotiate, exchange AND authenticate ROM data
        // before any libretro core is replaced.
        if local.game == peer.game {
            if local.rom_sha256 == peer.rom_sha256 { return Ok(None); }
            return Err(invalid("Identical GBA game code with different ROM revisions"));
        }
        let began = Instant::now();
        let own = fs::read(own_rom)?;
        report_link_rom_phase("start", &format!("own_bytes={} peer_game_code={}",
            own.len(), String::from_utf8_lossy(&peer.game)));
        if !(0xC0..=MAX_ROM).contains(&own.len()) ||
            own.get(0xAC..0xB0) != Some(local.game.as_slice()) ||
            digest(&SHA256, &own).as_ref() != local.rom_sha256 {
            return Err(invalid("Own cartridge changed since KMLB/4 negotiation"));
        }
        let cache = own_rom.parent().ok_or_else(||
            invalid("Own ROM has no private cache folder"))?.join("link-peer-cache");
        fs::create_dir_all(&cache)?;

        fn take_frame(link: &mut TcpLink, bound: Duration, cancel: &Cancel)
            -> std::io::Result<Vec<u8>> {
            let limit = Instant::now() + bound;
            loop {
                if cancel.is_cancelled() {
                    return Err(std::io::Error::new(std::io::ErrorKind::Interrupted,
                        "Peer ROM exchange cancelled"));
                }
                if link.peer_ended() || link.is_closed() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof, "Peer closed ROM exchange"));
                }
                if let Some(data) = link.try_recv() { return Ok(data); }
                if Instant::now() >= limit {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut, "Peer ROM exchange timed out"));
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
        fn exchange_frame(link: &mut TcpLink, bytes: &[u8], until: Instant,
                          cancel: &Cancel)
            -> std::io::Result<Vec<u8>> {
            if cancel.is_cancelled() {
                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted,
                    "Peer ROM exchange cancelled"));
            }
            if Instant::now() >= until {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut, "Peer ROM transfer deadline exceeded"));
            }
            link.send(slot_retro::NETPACKET_RELIABLE, bytes);
            if link.is_closed() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe, "Cannot send verified ROM fragment"));
            }
            take_frame(link, PACKET_WAIT, cancel)
        }
        // Both users must have accepted ROM sharing before either side
        // transmits ANY game image bytes. One party can take longer reading
        // the permission dialog; the other waits up to 60s for R4OK instead
        // of timing out after the normal 8s per-packet limit.
        report_link_rom_phase("consent", "awaiting_remote");
        self.send(slot_retro::NETPACKET_RELIABLE, b"R4OK");
        let approval = take_frame(self, Duration::from_secs(60), cancel)
            .map_err(|error| {
                report_link_transport_fault("rom_consent", &error.to_string());
                error
            })?;
        if approval.as_slice() != b"R4OK" {
            return Err(invalid("Mixed-ROM peer did not authorize cartridge exchange"));
        }
        report_link_rom_phase("consent", &format!(
            "authorized elapsed_ms={}", began.elapsed().as_millis()));
        let until = Instant::now() + DEADLINE;
        let mut manifest = Vec::with_capacity(44);
        manifest.extend_from_slice(b"R4M0");
        manifest.extend_from_slice(&(own.len() as u32).to_be_bytes());
        manifest.extend_from_slice(&local.game);
        manifest.extend_from_slice(&local.rom_sha256);
        report_link_rom_phase("manifest", "awaiting_remote");
        let remote_meta = exchange_frame(self, &manifest, until, cancel)
            .map_err(|error| {
                report_link_transport_fault("rom_manifest", &error.to_string());
                error
            })?;
        if remote_meta.len() != 44 || &remote_meta[..4] != b"R4M0" ||
            remote_meta[8..12] != peer.game ||
            remote_meta[12..44] != peer.rom_sha256 {
            return Err(invalid("Remote ROM manifest did not match negotiated KMLB/4 peer"));
        }
        let remote_len = u32::from_be_bytes(remote_meta[4..8].try_into()
            .map_err(|_|invalid("Invalid remote ROM length"))?) as usize;
        if !(0xC0..=MAX_ROM).contains(&remote_len) {
            return Err(invalid("Remote ROM length outside bounded retail GBA range"));
        }
        let local_chunks = own.len().div_ceil(CHUNK);
        let remote_chunks = remote_len.div_ceil(CHUNK);
        report_link_rom_phase("manifest", &format!(
            "accepted remote_bytes={remote_len} chunks={} elapsed_ms={}",
            local_chunks.max(remote_chunks), began.elapsed().as_millis()));
        let mut received = Vec::with_capacity(remote_len);
        for seq in 0..local_chunks.max(remote_chunks) {
            let st = seq * CHUNK;
            let en = own.len().min(st.saturating_add(CHUNK));
            let slice = if st < own.len() { &own[st..en] } else { &[] };
            let mut packet = Vec::with_capacity(6 + slice.len());
            packet.extend_from_slice(b"R4D0");
            packet.extend_from_slice(&(seq as u16).to_be_bytes());
            packet.extend_from_slice(slice);
            let frame = exchange_frame(self, &packet, until, cancel)
                .map_err(|error| {
                    report_link_transport_fault("rom_fragment",
                        &format!("seq={seq}/{} elapsed_ms={} reason={error}",
                            local_chunks.max(remote_chunks), began.elapsed().as_millis()));
                    error
                })?;
            if frame.len() < 6 || &frame[..4] != b"R4D0" ||
                u16::from_be_bytes([frame[4], frame[5]]) != seq as u16 {
                return Err(invalid("Remote ROM fragment sequence mismatch"));
            }
            let expected = if seq < remote_chunks {
                (remote_len - seq * CHUNK).min(CHUNK)
            } else { 0 };
            if frame.len() != expected + 6 {
                return Err(invalid("Remote ROM fragment length mismatch"));
            }
            received.extend_from_slice(&frame[6..]);
            if seq % 32 == 0 || seq + 1 == local_chunks.max(remote_chunks) {
                report_link_rom_phase("fragment", &format!(
                    "seq={}/{} received_bytes={} elapsed_ms={}",
                    seq + 1, local_chunks.max(remote_chunks),
                    received.len(), began.elapsed().as_millis()));
            }
        }
        if cancel.is_cancelled() {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted,
                "Peer ROM exchange cancelled before publishing cache"));
        }
        report_link_rom_phase("verification", &format!(
            "received_bytes={} expected_bytes={} elapsed_ms={}",
            received.len(), remote_len, began.elapsed().as_millis()));
        if received.len() != remote_len ||
            received.get(0xAC..0xB0) != Some(peer.game.as_slice()) ||
            digest(&SHA256, &received).as_ref() != peer.rom_sha256 {
            report_link_transport_fault("rom_verification",
                "remote size/header/SHA256 mismatch");
            return Err(invalid("Remote ROM data failed header or SHA256 authentication"));
        }
        let hex: String = peer.rom_sha256.iter().map(|b| format!("{b:02x}")).collect();
        let dest = cache.join(format!("{hex}.gba"));
        if dest.is_file() {
            let old = fs::read(&dest)?;
            if old == received {
                report_link_rom_phase("ready", &format!(
                    "existing_cache_verified elapsed_ms={}", began.elapsed().as_millis()));
                return Ok(Some(dest));
            }
            return Err(invalid("Verified remote ROM cache changed unexpectedly"));
        }
        // Use a unique private temp generation so an interrupted exchange
        // never prevents the next Host/Join attempt from completing.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| invalid("Invalid clock for peer ROM cache"))?.as_nanos();
        let scratch = cache.join(format!("{hex}.partial-{}-{nonce}", std::process::id()));
        let writing = (|| -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&scratch)?;
            file.write_all(&received)?;
            file.sync_all()?;
            if cancel.is_cancelled() {
                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted,
                    "ROM cache publish cancelled"));
            }
            if dest.exists() {
                return Err(invalid("Another peer ROM publish raced with this transfer"));
            }
            fs::rename(&scratch, &dest)
        })();
        if let Err(error) = writing {
            let _ = fs::remove_file(&scratch);
            return Err(error);
        }
        if fs::read(&dest)? != received {
            return Err(invalid("Cached peer ROM was modified during publishing"));
        }
        report_link_rom_phase("ready", &format!(
            "cache_published elapsed_ms={}", began.elapsed().as_millis()));
        Ok(Some(dest))
    }

    pub fn send_end(&mut self) {
        let (ack, wrote) = channel();
        if self.outbox.try_send(Out::Control(CONTROL_ENDED, ack)).is_err() {
            self.closed.store(true, Ordering::Release);
            let _ = self.stream.shutdown(Shutdown::Both);
            return;
        }
        let _ = wrote.recv_timeout(Duration::from_millis(BYE_MS));
    }

    pub fn nodelay(&self) -> std::io::Result<bool> {
        self.stream.nodelay()
    }
}

/// Every outbound frame must be lossless and fit the fork's u16 wire
/// length. Silently dropping one (or a partial write) would desynchronize
/// the linked emulators, so the writer treats either as a terminal error.
fn write_packet_frame(writer: &mut impl Write, buf: &[u8]) -> std::io::Result<()> {
    let len = u16::try_from(buf.len()).map_err(|_| std::io::Error::new(
        std::io::ErrorKind::InvalidData, "Multiplayer packet exceeds TCP frame limit"))?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(buf)
}

impl Drop for TcpLink {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

impl LinkChannel for TcpLink {
    fn send(&mut self, _flags: i32, buf: &[u8]) {
        if buf.is_empty() {
            return;
        }
        if self.outbox.try_send(Out::Packet(buf.to_vec())).is_err() {
            report_link_transport_fault("tx_queue", "saturated or disconnected");
            // Dropping an input/state packet corrupts the emulated link.
            // Report a dead channel on the next LinkPlay::frame instead.
            self.closed.store(true, Ordering::Release);
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }

    fn try_recv(&mut self) -> Option<Vec<u8>> {
        match self.inbox.try_recv() {
            Ok(p) => Some(p),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    fn send_end(&mut self) {
        TcpLink::send_end(self);
    }

    fn peer_ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }

    fn set_paused(&mut self, paused: bool) {
        // The GL thread never blocks waiting for the writer; in-order TCP
        // guarantees the remote sees preceding game bytes before PAUSE.
        let (ack, _written) = channel();
        let control = if paused { CONTROL_PAUSED } else { CONTROL_RESUMED };
        if self.outbox.try_send(Out::Control(control, ack)).is_err() {
            report_link_transport_fault("tx_control_queue", "saturated or disconnected");
            // A PAUSE/RESUME that cannot be queued is not acknowledged:
            // fail closed instead of allowing the peers to drift.
            self.closed.store(true, Ordering::Release);
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }

    fn peer_paused(&self) -> bool {
        self.peer_paused.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod lab_negotiation_tests {
    use super::*;
    fn identity() -> LabGreeting {
        LabGreeting { game: *b"BPRE", backend: 1, mode: 1,
            rom_sha256: [3;32], core_sha256: [4;32], bios_sha256: [5;32] }
    }
    #[test]
    fn kmlb4_transfers_two_distinct_gba_roms_into_verified_private_caches() {
        use std::net::{SocketAddr, TcpListener};
        let home = std::env::temp_dir().join(format!(
            "slot-kmlb4-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .unwrap().as_nanos()));
        let a_dir = home.join("host");
        let b_dir = home.join("guest");
        fs::create_dir_all(&a_dir).unwrap();
        fs::create_dir_all(&b_dir).unwrap();
        fn cart(code: [u8; 4], size: usize, seed: u8) -> Vec<u8> {
            let mut bytes = vec![seed; size];
            bytes[0xAC..0xB0].copy_from_slice(&code);
            bytes
        }
        // Retail-sized Pokémon images: hundreds of bidirectional packets.
        let host_rom = cart(*b"BPRE", 16 * 1024 * 1024, 0xA5);
        let guest_rom = cart(*b"BPGE", 16 * 1024 * 1024, 0x5A);
        let host_file = a_dir.join("firered.gba");
        let guest_file = b_dir.join("leafgreen.gba");
        fs::write(&host_file, &host_rom).unwrap();
        fs::write(&guest_file, &guest_rom).unwrap();
        let mut h = identity();
        let mut g = h.clone();
        h.rom_sha256.copy_from_slice(digest(&SHA256, &host_rom).as_ref());
        g.game = *b"BPGE";
        g.rom_sha256.copy_from_slice(digest(&SHA256, &guest_rom).as_ref());
        assert!(h.accepts(&g));
        assert_eq!(h.bytes()[4], 5, "A14 mGBA must require KMLB/5 signed device identity");

        let reserve = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr: SocketAddr = reserve.local_addr().unwrap();
        drop(reserve);
        let (ready, listening) = channel();
        let h_copy = h.clone();
        let host = thread::spawn(move || {
            let mut wire = TcpLink::host_lab_greeted_until(
                "127.0.0.1", addr.port(), Duration::from_secs(5),
                &Cancel::new(), &h_copy, || { ready.send(()).unwrap(); }).unwrap();
            wire.exchange_mgba_peer_rom(&h_copy, &host_file, &Cancel::new()).unwrap().unwrap()
        });
        listening.recv_timeout(Duration::from_secs(3)).unwrap();
        let mut guest = TcpLink::join_lab_greeted(addr, &g).unwrap();
        let guest_peer = guest.exchange_mgba_peer_rom(&g, &guest_file, &Cancel::new()).unwrap().unwrap();
        let host_peer = host.join().unwrap();
        assert_eq!(fs::read(&guest_peer).unwrap(), host_rom);
        assert_eq!(fs::read(&host_peer).unwrap(), guest_rom);
        assert_eq!(fs::read(&guest_file).unwrap(), guest_rom);
        // The private peer caches are not added to the user's library.
        assert_eq!(host_peer.parent().unwrap().file_name().unwrap(), "link-peer-cache");
        assert_eq!(guest_peer.parent().unwrap().file_name().unwrap(), "link-peer-cache");
        drop(guest);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn lab_greeting_is_not_legacy_slot_one() {
        assert_ne!(&identity().bytes()[..9], &greeting("BPRE"));
        assert_eq!(&identity().bytes()[..4], b"KMLB");
    }
    #[test]
    fn tcp_packet_framing_is_exact_and_never_silently_truncates_or_drops() {
        let mut out = Vec::new();
        write_packet_frame(&mut out, b"gba").unwrap();
        assert_eq!(out, vec![0, 3, b'g', b'b', b'a']);
        let oversized = vec![7u8; (u16::MAX as usize) + 1];
        assert_eq!(write_packet_frame(&mut out, &oversized).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData);
        assert_eq!(out, vec![0, 3, b'g', b'b', b'a']);
    }

    #[test]
    fn broken_tcp_write_reports_error_before_link_can_assume_delivery() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "peer gone"))
            }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        assert_eq!(write_packet_frame(&mut Broken, b"frame").unwrap_err().kind(),
            std::io::ErrorKind::BrokenPipe);
    }

    /// A real loopback listener exercises the actual TCP handshake and the
    /// reader/writer worker threads, not only serialised greeting bytes.
    /// No device/emulator, BIOS or ROM is needed.
    #[test]
    fn strict_kmlb3_host_rejects_wrong_peers_then_transfers_packets_and_end() {
        use slot_retro::NETPACKET_RELIABLE;
        use std::net::{SocketAddr, TcpListener};
        use std::thread;

        // Reserve an OS-assigned TCP port for this test instead of a fixed
        // 7211; tests can run simultaneously in GitHub Actions.
        let reserve = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr: SocketAddr = reserve.local_addr().unwrap();
        drop(reserve);
        let (started, listening) = channel();
        let expected = identity();
        let wanted = expected.clone();
        let host = thread::spawn(move || {
            TcpLink::host_lab_greeted_until(
                "127.0.0.1", addr.port(), Duration::from_secs(8),
                &Cancel::new(), &wanted,
                || { let _ = started.send(()); },
            ).expect("a compatible peer must still be accepted")
        });
        listening.recv_timeout(Duration::from_secs(3)).unwrap();

        // Every mismatch MUST be rejected without consuming the one live
        // Host session or implicitly falling back to legacy SLOT/1.
        let mut variants = Vec::new();
        let mut wrong = expected.clone();
        wrong.game = *b"AMAE";
        variants.push(wrong);
        let mut wrong = expected.clone();
        wrong.rom_sha256[0] ^= 1;
        variants.push(wrong);
        let mut wrong = expected.clone();
        wrong.core_sha256[0] ^= 1;
        variants.push(wrong);
        let mut wrong = expected.clone();
        wrong.bios_sha256[0] ^= 1;
        variants.push(wrong);
        let mut wrong = expected.clone();
        wrong.backend ^= 1;
        variants.push(wrong);
        let mut wrong = expected.clone();
        wrong.mode ^= 1;
        variants.push(wrong);
        for wrong in variants {
            assert!(TcpLink::join_lab_greeted(addr, &wrong).is_err(),
                "KMLB/3 must refuse a mismatched peer");
        }

        let mut guest = TcpLink::join_lab_greeted(addr, &expected).unwrap();
        let mut server = host.join().unwrap();
        assert!(guest.nodelay().unwrap());
        assert!(server.nodelay().unwrap());

        <TcpLink as LinkChannel>::send(&mut guest, NETPACKET_RELIABLE, b"guest-to-host");
        <TcpLink as LinkChannel>::send(&mut server, NETPACKET_RELIABLE, b"host-to-guest");
        let incoming = |side: &mut TcpLink, expected: &[u8]| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if let Some(packet) = <TcpLink as LinkChannel>::try_recv(side) {
                    assert_eq!(packet, expected);
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("linked peer packet not delivered");
        };
        incoming(&mut server, b"guest-to-host");
        incoming(&mut guest, b"host-to-guest");

        guest.send_end();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && !server.peer_ended() {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(server.peer_ended(),
            "the original five-byte END control must reach the Host");
    }

    #[test]
    fn documented_pair_uses_mutual_kmlb3_validation_and_out_of_band_pause() {
        use std::net::{TcpListener, SocketAddr};
        let reserve=TcpListener::bind(("127.0.0.1",0)).unwrap();
        let addr:SocketAddr=reserve.local_addr().unwrap();drop(reserve);
        let mut firered=identity();firered.backend=2;firered.mode=2;
        let mut leafgreen=firered.clone();
        leafgreen.game=*b"BPGE";leafgreen.rom_sha256=[0x81;32];
        assert!(firered.accepts(&leafgreen));
        assert!(leafgreen.accepts(&firered));
        let (started,listening)=channel();
        let expected=firered.clone();
        let host=std::thread::spawn(move|| TcpLink::host_lab_greeted_until(
            "127.0.0.1",addr.port(),Duration::from_secs(5),&Cancel::new(),
            &expected,||{let _=started.send(());}).unwrap());
        listening.recv_timeout(Duration::from_secs(3)).unwrap();
        let mut guest=TcpLink::join_lab_greeted(addr,&leafgreen).unwrap();
        let mut server=host.join().unwrap();
        assert!(!server.peer_paused());
        guest.set_paused(true);
        let deadline=Instant::now()+Duration::from_secs(2);
        while !server.peer_paused() && Instant::now()<deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(server.peer_paused(),"TCP PAUSE must be delivered even without game frames");
        assert!(server.try_recv().is_none(),"PAUSE must not leak into RFU/cable bytes");
        guest.set_paused(false);
        let deadline=Instant::now()+Duration::from_secs(2);
        while server.peer_paused() && Instant::now()<deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!server.peer_paused(),"RESUME must unfreeze the remote GBA");
        assert!(server.try_recv().is_none(),"RESUME must not reach the cartridge");
        server.send_end();
    }
    #[test]
    fn cross_game_never_relaxes_unrelated_peer_integrity_checks(){
        let mut host=identity();host.backend=2;host.mode=1;
        let mut other=host.clone();other.game=*b"BPGE";other.rom_sha256=[0xAB;32];
        assert!(host.accepts(&other));
        let mut bad=other.clone();bad.core_sha256[1]^=1;assert!(!host.accepts(&bad));
        let mut bad=other.clone();bad.bios_sha256[1]^=1;assert!(!host.accepts(&bad));
        let mut bad=other.clone();bad.backend=1;assert!(!host.accepts(&bad));
        let mut bad=other.clone();bad.game=*b"BRBE";assert!(!host.accepts(&bad));
        let mut bad=other.clone();bad.mode=2;assert!(!host.accepts(&bad));
        // Ruby and FireRed can communicate only over the GBA Cable.
        let mut ruby=other.clone();ruby.game=*b"AXVE";ruby.mode=2;
        let mut wireless_host=host.clone();wireless_host.mode=2;
        assert!(!wireless_host.accepts(&ruby));
        let mut ruby_same=host.clone();
        ruby_same.game=*b"AXVE";ruby_same.mode=2;
        assert!(!ruby_same.accepts(&ruby_same),
            "even identical Ruby ROMs cannot create GBA RFU hardware");
        // Identical header code with modified ROM content is NOT an approved pair.
        let mut patched=host.clone();patched.rom_sha256[0]^=1;
        assert!(!host.accepts(&patched));
        // Physically compatible but unsupported generic Cable protocols
        // must never complete an apparently successful gpSP handshake.
        let mut battle=host.clone();
        battle.game=*b"BRBE";battle.mode=1;
        let mut colonel=battle.clone();
        colonel.game=*b"BRKE";colonel.rom_sha256=[0x22;32];
        assert!(!battle.accepts(&colonel));
        battle.mode=2;colonel.mode=2;
        assert!(battle.accepts(&colonel));
        let mut bn3=battle.clone();bn3.game=*b"A3XE";bn3.mode=1;
        let mut bn3_white=bn3.clone();bn3_white.game=*b"A6BE";
        assert!(!bn3.accepts(&bn3_white));
        // mGBA KMLB/4 may now negotiate compatible dual cartridges;
        // actual Link READY is gated on the verified ROM transfer.
        let mut mgba=host.clone();mgba.backend=1;
        let mut mgba_other=other.clone();mgba_other.backend=1;
        assert!(mgba.accepts(&mgba_other));
        let mut incompatible=mgba_other.clone();incompatible.game=*b"BRBE";
        assert!(!mgba.accepts(&incompatible));
        assert_eq!(mgba.bytes()[4],5);
        assert_eq!(host.bytes()[4],3);
        // Legacy framing/version can never be accepted in KMLB/3.
        let mut legacy=host.bytes();legacy[4]=2;
        assert!(LabGreeting::decode(&legacy).is_none());
        let mut mixed_old=mgba.bytes();mixed_old[4]=4;
        assert!(LabGreeting::decode(&mixed_old).is_none());
    }
    #[test]
    fn stalled_reader_has_bounded_memory_and_disconnects_instead_of_dropping_frames() {
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        peer.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let mut local = TcpLink::wrap(stream).unwrap();
        // Deliberately never drain local.inbox, as during an extended pause.
        // The reader must terminate on MAX_QUEUED_PACKETS rather than
        // buffering unbounded traffic or losing one serial packet silently.
        for _ in 0..MAX_QUEUED_PACKETS + 16 {
            if peer.write_all(&[0, 1, 0x47]).is_err() { break; }
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while !local.is_closed() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(local.is_closed(), "saturated inbound queue must close the Link");
        let mut delivered = 0;
        while local.try_recv().is_some() { delivered += 1; }
        assert_eq!(delivered, MAX_QUEUED_PACKETS,
            "all queued frames are preserved in order until the fail-closed bound");
    }

    #[test]
    fn cancelled_listener_does_not_mark_a_peer_ready() {
        use std::net::TcpListener;
        let reserve = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = reserve.local_addr().unwrap().port();
        drop(reserve);
        let cancel = Cancel::new();
        let stop = cancel.clone();
        let (started, listening) = channel();
        let host = std::thread::spawn(move || TcpLink::host_lab_greeted_until(
            "127.0.0.1", port, Duration::from_secs(5), &stop,
            &identity(), || { let _ = started.send(()); },
        ));
        listening.recv_timeout(Duration::from_secs(3)).unwrap();
        cancel.cancel();
        let error = match host.join().unwrap() {
            Ok(_) => panic!("cancelled Host must not become Ready"),
            Err(e) => e,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    }

    #[test]
    fn all_critical_parameters_are_in_negotiation() {
        let base = identity().bytes();
        let mut another = identity(); another.backend = 2;
        assert_ne!(base, another.bytes());
        let mut another = identity(); another.mode = 2;
        assert_ne!(base, another.bytes());
        let mut another = identity(); another.rom_sha256[2] ^= 1;
        assert_ne!(base, another.bytes());
        let mut another = identity(); another.core_sha256[2] ^= 1;
        assert_ne!(base, another.bytes());
        let mut another = identity(); another.bios_sha256[2] ^= 1;
        assert_ne!(base, another.bytes());
    }
}

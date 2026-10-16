//! Android LAN transport for Slot multiplayer (not an emulation session yet).
//! Wi-Fi association and WifiManager.MulticastLock are the Kotlin host's job.
//! No blocking socket or DNS-SD work may execute on the GLSurfaceView GL thread.
//! On-wire discovery, greeting and TCP frame packing are the fork's GPL code.

use std::io::{self, Read};
use std::fs::File;
use std::path::{Path, PathBuf};
use ring::digest::{Context, SHA256};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use crate::link_lan::{self, Advertiser, HostInfo, Multicast};
use crate::link_net::{Cancel, TcpLink, LabGreeting};
use crate::trusted_link::{self, PeerPermission};
use crate::link_kind::LinkKind;

const PORT: u16 = 7211;
const BOUND: Duration = Duration::from_secs(30);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Role { Host, Join }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Failure { NoNetwork, NobodyCame, IncompatiblePeer, PeerVanished, Cancelled }

pub enum Progress {
    Looking,
    /// Both devices must explicitly approve transmitting their ROM image
    /// to the other player. Only emitted for mixed-ROM mGBA Cable.
    PeerRomConsent(String),
    Ready(TcpLink, Option<PathBuf>),
    Failed(Failure),
}

fn report_setup_error(error: &io::Error) {
    let message = format!("KMLB4 setup failure: kind={:?} reason={error}",
        error.kind());
    #[cfg(target_os = "android")]
    crate::game::log_launch_timing(&message);
    #[cfg(not(target_os = "android"))]
    eprintln!("SlotKONKR: {message}");
}

fn classify(err: &io::Error) -> Failure {
    match err.kind() {
        io::ErrorKind::Interrupted => Failure::Cancelled,
        io::ErrorKind::TimedOut => Failure::NobodyCame,
        // During the strict KMLB/2 welcome, the Host closes the socket
        // without ACK when ROM/core/BIOS/backend/link mode differs. This
        // arrives on Join as EOF. Never mislabel it as a link that was
        // successfully established and subsequently lost.
        io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidData =>
            Failure::IncompatiblePeer,
        _ if err.to_string().contains("did not accept the greeting") =>
            Failure::IncompatiblePeer,
        _ => Failure::PeerVanished,
    }
}

#[cfg(test)]
mod failure_classification_tests {
    use super::*;

    #[test]
    fn strict_handshake_refusal_is_reported_as_incompatible_not_peer_lost() {
        for kind in [io::ErrorKind::UnexpectedEof, io::ErrorKind::InvalidData] {
            assert_eq!(classify(&io::Error::new(kind, "host refused greeting")),
                Failure::IncompatiblePeer);
        }
        assert_eq!(classify(&io::Error::other("host did not accept the greeting")),
            Failure::IncompatiblePeer);
        assert_eq!(classify(&io::Error::new(io::ErrorKind::TimedOut, "wait")),
            Failure::NobodyCame);
        assert_eq!(classify(&io::Error::new(io::ErrorKind::ConnectionReset, "later")),
            Failure::PeerVanished);
    }
}

/// A canonical GBA ROM header code, not a user-facing filename.
/// Reject wildcards and IDs that could alias another advertised game.
pub fn game_code(raw: &str) -> Option<String> {
    (raw.len() == 4 && raw.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
        .then(|| raw.to_owned())
}

/// Keep all identity work off the GL thread. No ROM, core or BIOS bytes
/// are transmitted; a hash mismatch fails before replacing the solo core.
#[derive(Clone, Debug)]
pub struct LinkConfig {
    pub game: String,
    pub rom: PathBuf,
    pub core_file: PathBuf,
    pub bios_file: PathBuf,
    pub backend: Backend,
    pub mode: LinkKind,
    pub trusted_dir: PathBuf,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Backend { MgbaSlotCable, GpspNetpacket }

fn sha256_file(path: &Path, max_size: u64) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    if size == 0 || size > max_size {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            "Invalid Multiplayer fingerprint file size"));
    }
    let mut digest = Context::new(&SHA256);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 { break; }
        digest.update(&buffer[..n]);
    }
    let result = digest.finish();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(result.as_ref());
    Ok(hash)
}

/// The SHA256 of the compiled .so cannot be the network identity: macOS
/// AVD uses x86_64 while KONKR uses arm64. Even with identical pinned source
/// and Link behavior, the ELF bytes are necessarily different. KMLB/2
/// authenticates the exact upstream source pin + Slot patch recipe instead,
/// still distinguishing mGBA and gpSP and failing closed on mismatched ROM,
/// BIOS, backend and Cable/RFU mode.
///
/// The source recipe is embedded at compile time from the repository files
/// actually used by CI's pinned core builders. Any change to these patches,
/// build scripts or audio-sidecar logic changes the negotiated identity.
/// This is a compatibility fingerprint, not a cryptographic attestation of
/// the .so binary (which must still be verified when packaging the APK).
fn portable_core_recipe(backend: Backend) -> [u8; 32] {
    let mut digest = Context::new(&SHA256);
    digest.update(b"slot-konkr-KMLB2-cross-ABI-core-recipe-v1");
    let files: &[&[u8]] = match backend {
        Backend::MgbaSlotCable => &[
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/build-mgba-android.sh")),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../cores/mgba/build.sh")),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../cores/mgba/685023e05-fifo-dma-source-offset.patch")),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../cores/mgba/gb-audio-capacitor.patch")),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../cores/mgba/gb-audio-stopped-channel.patch")),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../cores/mgba/link-mode.patch")),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../cores/mgba/zz-gb-link-mode.patch")),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../cores/mgba/zz-link-peer-frameskip.patch")),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../cores/mgba/zz-link-peer-silence.patch")),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/patch-mgba-speculative-audio.py")),
        ],
        Backend::GpspNetpacket => &[
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/build-gpsp-android.sh")),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/patch-gpsp-speculative-audio.py")),
        ],
    };
    for bytes in files {
        digest.update(&(bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    let mut hash = [0; 32];
    hash.copy_from_slice(digest.finish().as_ref());
    hash
}

impl LinkConfig {
    fn fingerprint(&self) -> io::Result<LabGreeting> {
        let code = game_code(&self.game).ok_or_else(||
            io::Error::new(io::ErrorKind::InvalidInput, "Invalid Multiplayer GBA header code"))?;
        if self.backend == Backend::MgbaSlotCable && self.mode != LinkKind::Cable {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,
                "mGBA Slot Link supports Cable only"));
        }
        let game: [u8; 4] = code.as_bytes().try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid game code"))?;
        let bios_sha256 = if self.bios_file.is_file() {
            sha256_file(&self.bios_file, 1024 * 1024)?
        } else { [0u8; 32] };
        // Refuse a missing/empty/unreasonably sized native core. Its ELF
        // digest is deliberately NOT compared across CPU architectures.
        // The pinned source + Slot patches supply a portable identity.
        let metadata = std::fs::metadata(&self.core_file)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 64 * 1024 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData,
                "Invalid Multiplayer native core file"));
        }
        Ok(LabGreeting {
            game,
            backend: match self.backend {
                Backend::MgbaSlotCable => 1, Backend::GpspNetpacket => 2,
            },
            mode: match self.mode {
                LinkKind::Cable => 1, LinkKind::Wireless => 2,
            },
            rom_sha256: sha256_file(&self.rom, 128 * 1024 * 1024)?,
            core_sha256: portable_core_recipe(self.backend),
            bios_sha256,
        })
    }
}

fn connect(role: Role, local: Ipv4Addr, config: &LinkConfig,
           cancel: &Cancel, ui: &Sender<Progress>, approval: &Receiver<PeerPermission>)
    -> io::Result<(TcpLink, Option<PathBuf>)> {
    if local.is_unspecified() || local.is_loopback() || local.is_multicast()
        || local.is_link_local() || local.is_broadcast() {
        return Err(io::Error::new(io::ErrorKind::AddrNotAvailable,
            "no usable Wi-Fi IPv4 address"));
    }
    let fingerprint = config.fingerprint()?;
    let mut link = match role {
        Role::Host => {
            // Validate multicast and join the correct interface before announcing.
            let wire = Multicast::open(local)?;
            let info = HostInfo {
                instance: link_lan::instance_name(),
                addr: local, port: PORT, game: crate::link_compat::discovery_key(&config.game).to_owned(),
            };
            let mut advert: Option<Advertiser> = None;
            // Bind TCP before publishing mDNS, so discovery never races the listener.
            // New Lab v2 handshake is isolated from legacy SLOT/1 on Beta5.
            let result = TcpLink::host_lab_greeted_until(
                &local.to_string(), PORT, BOUND, cancel, &fingerprint,
                || advert = Some(Advertiser::start_with(Box::new(wire), info)),
            );
            drop(advert); // Publish DNS-SD goodbye on both success and failure.
            result
        }
        Role::Join => {
            let mut wire = Multicast::open(local)?;
            link_lan::find(&mut wire, crate::link_compat::discovery_key(&config.game), cancel, BOUND, |addr| {
                TcpLink::join_lab_greeted(addr.into(), &fingerprint)
            })
        }
    }?;
    let peer_path = stage_mgba_peer(&mut link, &fingerprint, config,
        role == Role::Host, cancel, ui, approval)?;
    Ok((link, peer_path))
}

/// A new KMLB/4 session cannot become READY until the remote cartridge
/// has been fully exchanged, SHA-verified and published inside the private
/// app cache. The GL thread never hashes, copies or downloads ROM data.
fn stage_mgba_peer(link: &mut TcpLink, local: &LabGreeting,
                   config: &LinkConfig, is_host: bool, cancel: &Cancel,
                   ui: &Sender<Progress>, approval: &Receiver<PeerPermission>)
    -> io::Result<Option<PathBuf>> {
    if config.backend == Backend::MgbaSlotCable &&
        link.peer_identity().is_some_and(|peer| peer.game != local.game) {
        // A14: cryptographic proof BEFORE consent; neither ROM is sent at
        // this stage. Trusted keys are installation-specific, not LAN IPs.
        let identity = trusted_link::authenticate_peer(link, &config.trusted_dir,
            local, is_host, cancel)?;
        let trusted = trusted_link::is_trusted(&config.trusted_dir, &identity.public_key)?;
        let mut remember = false;
        if !trusted {
            ui.send(Progress::PeerRomConsent(identity.label)).map_err(|_|
                io::Error::new(io::ErrorKind::Interrupted,
                    "UI no longer accepts peer ROM consent"))?;
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            loop {
                if cancel.is_cancelled() || std::time::Instant::now() >= deadline {
                    return Err(io::Error::new(io::ErrorKind::Interrupted,
                        "Peer ROM consent timed out or was cancelled"));
                }
                match approval.recv_timeout(Duration::from_millis(200)) {
                    Ok(PeerPermission::AllowOnce) => break,
                    Ok(PeerPermission::Remember) => { remember = true; break; },
                    Ok(PeerPermission::Reject) |
                    Err(mpsc::RecvTimeoutError::Disconnected) =>
                        return Err(io::Error::new(io::ErrorKind::Interrupted,
                            "Local ROM exchange was not authorized")),
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                }
            }
        }
        let transferred = link.exchange_mgba_peer_rom(local, &config.rom, cancel)?;
        // Never persist trust merely for opening a socket or clicking a
        // button. The signed peer MUST complete the verified ROM exchange.
        if remember {
            trusted_link::remember_peer(&config.trusted_dir, &identity.public_key)?;
        }
        return Ok(transferred);
    }
    match config.backend {
        Backend::MgbaSlotCable => link.exchange_mgba_peer_rom(local, &config.rom, cancel),
        Backend::GpspNetpacket => Ok(None),
    }
}

#[cfg(test)]
mod cross_abi_fingerprint_tests {
    use super::*;

    #[test]
    fn mac_x86_and_konkr_arm64_match_source_recipe_but_not_unrelated_cores() {
        let root = std::env::temp_dir().join(format!(
            "slot-a6-portable-link-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        let rom = root.join("game.gba");
        let bios = root.join("gba_bios.bin");
        let mac = root.join("mac-x86_64.so");
        let konkr = root.join("konkr-arm64.so");
        std::fs::write(&rom, b"exact same cartridge").unwrap();
        std::fs::write(&bios, b"exact same BIOS").unwrap();
        std::fs::write(&mac, b"ELF-for-x86_64").unwrap();
        std::fs::write(&konkr, b"ELF-for-arm64-different-bytes").unwrap();
        let mut left = LinkConfig {
            game: "BPRE".into(), rom: rom.clone(), core_file: mac,
            bios_file: bios.clone(), backend: Backend::MgbaSlotCable,
            mode: LinkKind::Cable,
            trusted_dir: root.join("trusted-devices"),
        };
        let mut right = left.clone();
        right.core_file = konkr;
        let host = left.fingerprint().unwrap();
        let join = right.fingerprint().unwrap();
        assert_eq!(host, join, "different Android ABIs must negotiate one pinned core source");
        assert_ne!(portable_core_recipe(Backend::MgbaSlotCable),
            portable_core_recipe(Backend::GpspNetpacket));
        right.backend = Backend::GpspNetpacket;
        assert_ne!(host, right.fingerprint().unwrap());
        right.backend = Backend::MgbaSlotCable;
        right.mode = LinkKind::Wireless;
        assert!(right.fingerprint().is_err(), "mGBA cannot advertise unsupported RFU");
        right.mode = LinkKind::Cable;
        std::fs::write(&rom, b"modified or hacked ROM").unwrap();
        assert_ne!(host.rom_sha256, left.fingerprint().unwrap().rom_sha256);
        std::fs::write(&rom, b"exact same cartridge").unwrap();
        std::fs::write(&bios, b"wrong BIOS").unwrap();
        assert_ne!(host.bios_sha256, left.fingerprint().unwrap().bios_sha256);
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// Status is polled by the GL engine; the worker never accesses libretro,
/// RetroAchievements, GL, Android UI or save files.
pub struct LinkStarter {
    events: Receiver<Progress>,
    consent: Sender<PeerPermission>,
    cancel: Cancel,
    // Report unexpected worker exit exactly once, rather than spinner forever.
    done: bool,
}

impl LinkStarter {
    pub fn spawn(role: Role, local: Ipv4Addr, config: LinkConfig) -> io::Result<Self> {
        if game_code(&config.game).is_none() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid GBA Link code"));
        }
        let (tx, events) = mpsc::channel();
        let (consent, approval) = mpsc::channel();
        let cancel = Cancel::new();
        let worker_cancel = cancel.clone();
        thread::spawn(move || {
            let _ = tx.send(Progress::Looking);
            let result = connect(role, local, &config, &worker_cancel, &tx, &approval);
            if let Err(ref error) = result { report_setup_error(error); }
            let report = match result {
                Ok((link, remote)) if !worker_cancel.is_cancelled() =>
                    Progress::Ready(link, remote),
                Ok(_) => Progress::Failed(Failure::Cancelled),
                Err(e) if e.kind() == io::ErrorKind::AddrNotAvailable =>
                    Progress::Failed(Failure::NoNetwork),
                Err(e) => Progress::Failed(classify(&e)),
            };
            let _ = tx.send(report);
        });
        Ok(Self { events, consent, cancel, done: false })
    }

    /// The Emulator NAT does not support IGMP. Direct-IP Join uses the
    /// same strict Lab v2 fingerprint as mDNS and cannot downgrade to SLOT/1.
    pub fn spawn_direct_join(local: Ipv4Addr, host: Ipv4Addr,
                             config: LinkConfig) -> io::Result<Self> {
        if game_code(&config.game).is_none() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid GBA Link code"));
        }
        if local.is_unspecified() || local.is_loopback() || local.is_multicast() ||
            local.is_link_local() || host.is_unspecified() || host.is_loopback() ||
            host.is_link_local() || host.is_multicast() || host.is_broadcast() ||
            !host.is_private() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,
                "Direct Join requires a private LAN IPv4 address"));
        }
        let (tx, events) = mpsc::channel();
        let (consent, approval) = mpsc::channel();
        let cancel = Cancel::new();
        let worker_cancel = cancel.clone();
        thread::spawn(move || {
            let _ = tx.send(Progress::Looking);
            let result = if worker_cancel.is_cancelled() {
                Err(io::Error::new(io::ErrorKind::Interrupted, "Join cancelled"))
            } else {
                config.fingerprint().and_then(|fingerprint| {
                    let mut link = TcpLink::join_lab_greeted(
                        SocketAddrV4::new(host, PORT).into(), &fingerprint)?;
                    let peer = stage_mgba_peer(&mut link, &fingerprint, &config,
                        false, &worker_cancel, &tx, &approval)?;
                    Ok((link, peer))
                })
            };
            if let Err(ref error) = result { report_setup_error(error); }
            let report = match result {
                Ok((link, remote)) if !worker_cancel.is_cancelled() =>
                    Progress::Ready(link, remote),
                Ok(_) => Progress::Failed(Failure::Cancelled),
                Err(error) => Progress::Failed(classify(&error)),
            };
            let _ = tx.send(report);
        });
        Ok(Self { events, consent, cancel, done: false })
    }

    pub fn poll(&mut self) -> Option<Progress> {
        if self.done { return None; }
        match self.events.try_recv() {
            Ok(event) => {
                self.done = matches!(&event, Progress::Ready(..) | Progress::Failed(_));
                Some(event)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.done = true;
                Some(Progress::Failed(Failure::PeerVanished))
            }
        }
    }

    pub fn approve_peer_rom(&self, decision: PeerPermission) {
        let _ = self.consent.send(decision);
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
        let _ = self.consent.send(PeerPermission::Reject);
    }
}

impl Drop for LinkStarter {
    fn drop(&mut self) { self.cancel(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dropped_network_worker_fails_once_instead_of_spinning_forever() {
        let (tx, events) = mpsc::channel::<Progress>();
        let (consent, _approval) = mpsc::channel();
        let mut starter = LinkStarter { events, consent, cancel: Cancel::new(), done: false };
        assert!(starter.poll().is_none(), "an idle worker may still connect");
        drop(tx);
        assert!(matches!(starter.poll(), Some(Progress::Failed(Failure::PeerVanished))));
        assert!(starter.poll().is_none(), "terminal failure must occur once");
    }

    #[test]
    fn completed_network_worker_cannot_trigger_duplicate_errors() {
        let (tx, events) = mpsc::channel::<Progress>();
        assert!(tx.send(Progress::Looking).is_ok());
        assert!(tx.send(Progress::Failed(Failure::Cancelled)).is_ok());
        drop(tx);
        let (consent, _approval) = mpsc::channel();
        let mut starter = LinkStarter { events, consent, cancel: Cancel::new(), done: false };
        assert!(matches!(starter.poll(), Some(Progress::Looking)));
        assert!(matches!(starter.poll(), Some(Progress::Failed(Failure::Cancelled))));
        assert!(starter.poll().is_none(),
            "worker channel closing after a terminal status is not another failure");
    }

    #[test]
    fn accepts_only_exact_rom_header_codes() {
        assert_eq!(game_code("BPRE"), Some("BPRE".into()));
        assert_eq!(game_code("AMAE"), Some("AMAE".into()));
        for bad in ["", "GAME BOY", "bpre", "BP RE", "A", "ABCD!", "😀"] {
            assert!(game_code(bad).is_none(), "{bad}");
        }
    }

    fn test_config(code: &str) -> LinkConfig {
        LinkConfig { game: code.into(), rom: PathBuf::from("unused.gba"),
            core_file: PathBuf::from("unused.so"),
            bios_file: PathBuf::from("unused.bin"), backend: Backend::MgbaSlotCable,
            mode: LinkKind::Cable, trusted_dir: PathBuf::from("unused-trust") }
    }
    #[test]
    fn direct_join_rejects_public_and_loopback_addresses() {
        for invalid in [Ipv4Addr::LOCALHOST, Ipv4Addr::BROADCAST,
            Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(224, 0, 0, 251)] {
            assert!(LinkStarter::spawn_direct_join(Ipv4Addr::new(10,0,2,16), invalid, test_config("BPRE")).is_err());
        }
        assert!(LinkStarter::spawn_direct_join(Ipv4Addr::new(10,0,2,16),
            Ipv4Addr::new(192,168,68,84), test_config("bad")).is_err());
    }

    #[test]
    fn connection_never_uses_loopback_or_unspecified_wifi() {
        for local in [Ipv4Addr::LOCALHOST, Ipv4Addr::UNSPECIFIED,
                      Ipv4Addr::new(224,0,0,251), Ipv4Addr::BROADCAST] {
            let (tx, _events) = mpsc::channel();
            let (_consent, approval) = mpsc::channel();
            let err = connect(Role::Host, local, &test_config("BPRE"),
                &Cancel::new(), &tx, &approval).err().unwrap();
            assert_eq!(err.kind(), io::ErrorKind::AddrNotAvailable);
        }
    }
}

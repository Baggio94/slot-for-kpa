//! A14: installation-bound Ed25519 identity and explicitly pinned LAN peers.
//! This is trust-on-first-use with proof of possession, NOT encryption/TLS.
//! Neither a stable IPv4 address nor the old lab trust flag authorizes ROMs.
//! Auth packets contain only public keys, random challenges and signatures.
//! Both parties must finish this handshake before any ROM byte is sent.

use crate::link_net::{Cancel, LabGreeting, TcpLink};
use ring::{digest::{digest, SHA256}, rand::{SecureRandom, SystemRandom},
           signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519}};
use slot_retro::LinkChannel;
use std::{fs, io, io::Write, path::Path, thread, time::{Duration, Instant}};

const IDENT_TAG: &[u8; 4] = b"K5ID";
const SIGN_TAG: &[u8; 4] = b"K5SG";
const IDENT_SIZE: usize = 4 + 32 + 32 + 4 + 32;
const SIGN_SIZE: usize = 4 + 64;
const MAX_KEYS: usize = 128;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PeerPermission { Reject, AllowOnce, Remember }

pub struct AuthenticatedPeer {
    pub public_key: [u8; 32],
    pub label: String,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn load_identity(dir: &Path) -> io::Result<Ed25519KeyPair> {
    fs::create_dir_all(dir)?;
    let key_path = dir.join("device-ed25519.pkcs8");
    let bytes = match fs::read(&key_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let rand = SystemRandom::new();
            let generated = Ed25519KeyPair::generate_pkcs8(&rand)
                .map_err(|_| invalid("Cannot create Link device identity"))?;
            match fs::OpenOptions::new().write(true).create_new(true).open(&key_path) {
                Ok(mut file) => {
                    file.write_all(generated.as_ref())?;
                    file.sync_all()?;
                    generated.as_ref().to_vec()
                },
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists =>
                    fs::read(&key_path)?,
                Err(error) => return Err(error),
            }
        },
        Err(error) => return Err(error),
    };
    Ed25519KeyPair::from_pkcs8(&bytes)
        .map_err(|_| invalid("Invalid existing Link identity; refuse to replace it"))
}

fn hex(key: &[u8; 32]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn trusted_file(dir: &Path) -> std::path::PathBuf { dir.join("peers.txt") }

fn read_trusted(dir: &Path) -> io::Result<Vec<String>> {
    let bytes = match fs::read(trusted_file(dir)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error),
    };
    if bytes.len() > MAX_KEYS * 65 {
        return Err(invalid("Link trusted device list is too large"));
    }
    let content = std::str::from_utf8(&bytes)
        .map_err(|_| invalid("Invalid Link trusted device list"))?;
    let mut entries = Vec::new();
    for line in content.lines() {
        if line.len() != 64 || !line.bytes().all(|v|
            v.is_ascii_digit() || (b'a'..=b'f').contains(&v)) ||
            entries.iter().any(|prior| prior == line) {
            return Err(invalid("Corrupt or duplicated Link trusted device key"));
        }
        entries.push(line.to_owned());
    }
    if entries.len() > MAX_KEYS { return Err(invalid("Too many remembered Link devices")); }
    Ok(entries)
}

pub fn is_trusted(dir: &Path, key: &[u8; 32]) -> io::Result<bool> {
    Ok(read_trusted(dir)?.iter().any(|item| item == &hex(key)))
}

pub fn remember_peer(dir: &Path, key: &[u8; 32]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let mut existing = read_trusted(dir)?;
    let wanted = hex(key);
    if existing.iter().any(|entry| entry == &wanted) { return Ok(()); }
    if existing.len() >= MAX_KEYS { return Err(invalid("Link trusted device list is full")); }
    existing.push(wanted);
    let data = format!("{}\n", existing.join("\n"));
    slot_store::atomic_write(&trusted_file(dir), data.as_bytes())
}

fn transcript(host: &[u8; IDENT_SIZE], join: &[u8; IDENT_SIZE]) -> Vec<u8> {
    let mut message = Vec::with_capacity(27 + 2 * IDENT_SIZE);
    message.extend_from_slice(b"slot-kmlb5-device-proof-v1");
    message.extend_from_slice(host);
    message.extend_from_slice(join);
    message
}

fn wait_frame(link: &mut TcpLink, cancel: &Cancel) -> io::Result<Vec<u8>> {
    let until = Instant::now() + Duration::from_secs(12);
    loop {
        if cancel.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Link identity cancelled"));
        }
        if link.is_closed() || link.peer_ended() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof,
                "Link peer closed during identity proof"));
        }
        if let Some(frame) = link.try_recv() { return Ok(frame); }
        if Instant::now() >= until {
            return Err(io::Error::new(io::ErrorKind::TimedOut,
                "Link peer identity proof timed out"));
        }
        thread::sleep(Duration::from_millis(2));
    }
}

pub fn authenticate_peer(link: &mut TcpLink, root: &Path,
                         local: &LabGreeting, is_host: bool,
                         cancel: &Cancel) -> io::Result<AuthenticatedPeer> {
    let peer = link.peer_identity().ok_or_else(||
        invalid("Missing negotiated peer for identity exchange"))?.clone();
    if local.backend != 1 || peer.backend != 1 || local.game == peer.game {
        return Err(invalid("KMLB/5 identity requires two different mGBA cartridges"));
    }
    let keys = load_identity(root)?;
    let my_key: [u8; 32] = keys.public_key().as_ref().try_into()
        .map_err(|_| invalid("Unexpected Ed25519 public key size"))?;
    let mut nonce = [0u8; 32];
    SystemRandom::new().fill(&mut nonce)
        .map_err(|_| invalid("No secure random nonce for Link"))?;
    let mut hello = [0u8; IDENT_SIZE];
    hello[..4].copy_from_slice(IDENT_TAG);
    hello[4..36].copy_from_slice(&my_key);
    hello[36..68].copy_from_slice(&nonce);
    hello[68..72].copy_from_slice(&local.game);
    hello[72..104].copy_from_slice(&local.rom_sha256);
    link.send(slot_retro::NETPACKET_RELIABLE, &hello);
    if link.is_closed() { return Err(invalid("Cannot transmit local Link identity")); }
    let peer_hello = wait_frame(link, cancel)?;
    if peer_hello.len() != IDENT_SIZE || &peer_hello[..4] != IDENT_TAG ||
        peer_hello[68..72] != peer.game ||
        peer_hello[72..104] != peer.rom_sha256 {
        return Err(invalid("Link identity does not match peer ROM negotiation"));
    }
    let remote: [u8; IDENT_SIZE] = peer_hello.try_into()
        .map_err(|_| invalid("Invalid Link identity frame size"))?;
    let (host, join) = if is_host { (&hello, &remote) }
        else { (&remote, &hello) };
    let text = transcript(host, join);
    let signed = keys.sign(&text);
    let mut proof = Vec::with_capacity(SIGN_SIZE);
    proof.extend_from_slice(SIGN_TAG);
    proof.extend_from_slice(signed.as_ref());
    link.send(slot_retro::NETPACKET_RELIABLE, &proof);
    if link.is_closed() { return Err(invalid("Cannot transmit Link device proof")); }
    let peer_sig = wait_frame(link, cancel)?;
    if peer_sig.len() != SIGN_SIZE || &peer_sig[..4] != SIGN_TAG {
        return Err(invalid("Invalid Link device proof response"));
    }
    let mut public_key = [0u8; 32];
    public_key.copy_from_slice(&remote[4..36]);
    UnparsedPublicKey::new(&ED25519, &public_key)
        .verify(&text, &peer_sig[4..])
        .map_err(|_| invalid("Peer did not prove its installation identity"))?;
    let fingerprint = digest(&SHA256, &public_key);
    let short = fingerprint.as_ref()[..6].iter()
        .map(|v| format!("{v:02X}")).collect::<String>();
    Ok(AuthenticatedPeer {
        public_key,
        label: format!("DEVICE {} {}", &short[..6], &short[6..]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("slot-trust-a14-{}-{}",
            std::process::id(), std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&p).unwrap();
        p
    }

    #[test]
    fn signed_paired_challenge_rejects_tampering_and_wrong_device() {
        let root = fixture();
        let host = load_identity(&root.join("host")).unwrap();
        let join = load_identity(&root.join("join")).unwrap();
        assert_eq!(host.public_key().as_ref(), load_identity(&root.join("host"))
            .unwrap().public_key().as_ref(), "identity must survive app restart");
        let mut h = [0u8; IDENT_SIZE];
        let mut j = [0u8; IDENT_SIZE];
        h[..4].copy_from_slice(IDENT_TAG); j[..4].copy_from_slice(IDENT_TAG);
        h[4..36].copy_from_slice(host.public_key().as_ref());
        j[4..36].copy_from_slice(join.public_key().as_ref());
        h[36] = 1; j[36] = 2;
        let signed_message = transcript(&h, &j);
        let sig = host.sign(&signed_message);
        assert!(UnparsedPublicKey::new(&ED25519, host.public_key().as_ref())
            .verify(&signed_message, sig.as_ref()).is_ok());
        assert!(UnparsedPublicKey::new(&ED25519, join.public_key().as_ref())
            .verify(&signed_message, sig.as_ref()).is_err());
        j[36] = 3;
        assert!(UnparsedPublicKey::new(&ED25519, host.public_key().as_ref())
            .verify(&transcript(&h, &j), sig.as_ref()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn trust_only_after_explicit_remember_and_reject_corrupt_list() {
        let root = fixture();
        let a = [0xA5; 32]; let b = [0xB5; 32];
        assert!(!is_trusted(&root, &a).unwrap());
        remember_peer(&root, &a).unwrap();
        remember_peer(&root, &a).unwrap();
        assert!(is_trusted(&root, &a).unwrap());
        assert!(!is_trusted(&root, &b).unwrap());
        fs::write(trusted_file(&root), b"invalid\n").unwrap();
        assert!(is_trusted(&root, &a).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

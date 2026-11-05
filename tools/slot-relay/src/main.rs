//! slot-relay: lets two slot. consoles meet over the Internet with a room code.
//!
//! The host dials in and is given a four-digit code; the friend dials in with
//! that code; from then on the relay copies bytes between the two sockets and
//! looks at none of them. Both consoles only ever dial *out*, so home routers,
//! CGNAT and mobile data need no configuration. Protocol: see
//! `crates/slot-android/src/link_relay.rs`, which is also compiled into this
//! program's tests so both ends are proven to agree.
//!
//!     slot-relay [LISTEN_ADDR]        default 0.0.0.0:7212
//!
//! Standard library only. One thread per connection: a link is two sockets
//! of a few kilobytes a second, and a small VPS holds thousands of them.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const MAGIC: &[u8; 4] = b"SLRY";
const VERSION: u8 = 2;
const CODE_LEN: usize = 4;
const OP_HOST: u8 = b'H';
const OP_JOIN: u8 = b'J';
const REPLY_CODE: u8 = b'C';
const REPLY_KEEPALIVE: u8 = b'K';
const REPLY_PAIRED: u8 = b'P';
const REPLY_NO_ROOM: u8 = b'N';
const REPLY_BUSY: u8 = b'B';
const REPLY_FULL: u8 = b'F';
const REPLY_VERSION: u8 = b'V';

#[derive(Clone, Debug)]
pub struct Limits {
    /// How long a room waits for its second player.
    pub room_ttl: Duration,
    /// How often a waiting host is told the relay is still there. Keeps
    /// mobile-carrier NAT from forgetting an idle connection.
    pub keepalive: Duration,
    /// A linked pair that says nothing for this long is closed.
    pub idle: Duration,
    pub max_connections: usize,
    pub max_rooms: usize,
    pub rooms_per_ip: usize,
    /// Wrong codes one address may try per `strike_window`.
    pub strikes: u32,
    pub strike_window: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            room_ttl: Duration::from_secs(15 * 60),
            keepalive: Duration::from_secs(10),
            idle: Duration::from_secs(30 * 60),
            max_connections: 4000,
            // Four digits are 10 000 codes: keep open rooms few enough that
            // a guessed code is almost never someone's (with the strikes).
            max_rooms: 300,
            rooms_per_ip: 4,
            strikes: 12,
            strike_window: Duration::from_secs(10 * 60),
        }
    }
}

struct Room {
    owner: IpAddr,
    /// The host's socket until a joiner takes it.
    host: Mutex<Option<TcpStream>>,
}

#[derive(Default)]
struct State {
    rooms: HashMap<String, Arc<Room>>,
    strikes: HashMap<IpAddr, (u32, Instant)>,
}

pub struct Relay {
    limits: Limits,
    state: Mutex<State>,
    connections: AtomicUsize,
}

fn log(message: &str) {
    eprintln!("slot-relay: {message}");
}

fn random_code() -> String {
    let mut bytes = [0u8; 4];
    loop {
        let ok = std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes)).is_ok();
        if !ok {
            use std::hash::{BuildHasher, Hasher};
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(Instant::now().elapsed().as_nanos()
                ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos()));
            bytes = (h.finish() as u32).to_le_bytes();
        }
        let n = u32::from_le_bytes(bytes);
        // Reject the top sliver so every code is equally likely.
        if n < u32::MAX - (u32::MAX % 10_000) {
            return format!("{:04}", n % 10_000);
        }
    }
}

struct Counted<'a>(&'a AtomicUsize);
impl Drop for Counted<'_> {
    fn drop(&mut self) { self.0.fetch_sub(1, Ordering::SeqCst); }
}

impl Relay {
    pub fn new(limits: Limits) -> Arc<Relay> {
        Arc::new(Relay { limits, state: Mutex::default(), connections: AtomicUsize::new(0) })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn serve(self: &Arc<Relay>, listener: TcpListener) {
        for conn in listener.incoming() {
            let Ok(stream) = conn else { continue };
            let relay = Arc::clone(self);
            let _ = thread::Builder::new().name("conn".into()).spawn(move || {
                relay.connections.fetch_add(1, Ordering::SeqCst);
                let _count = Counted(&relay.connections);
                if let Err(e) = relay.handle(stream) {
                    if !matches!(e.kind(), io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) {
                        log(&format!("connection ended: {e}"));
                    }
                }
            });
        }
    }

    fn handle(&self, mut stream: TcpStream) -> io::Result<()> {
        let peer = stream.peer_addr()?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        if self.connections.load(Ordering::SeqCst) > self.limits.max_connections {
            return stream.write_all(&[REPLY_FULL]);
        }
        let mut hello = [0u8; 6];
        stream.read_exact(&mut hello)?;
        if &hello[..4] != MAGIC { return Ok(()); } // not ours: drop silently
        if hello[4] != VERSION { return stream.write_all(&[REPLY_VERSION]); }
        match hello[5] {
            OP_HOST => self.host(stream, peer),
            OP_JOIN => {
                let mut code = [0u8; CODE_LEN];
                stream.read_exact(&mut code)?;
                self.join(stream, peer, &code)
            }
            _ => Ok(()),
        }
    }

    fn host(&self, mut stream: TcpStream, peer: SocketAddr) -> io::Result<()> {
        let owner = peer.ip();
        let (code, room) = {
            let mut state = self.state();
            let mine = state.rooms.values().filter(|r| r.owner == owner).count();
            if state.rooms.len() >= self.limits.max_rooms || mine >= self.limits.rooms_per_ip {
                drop(state);
                return stream.write_all(&[REPLY_FULL]);
            }
            let code = loop {
                let code = random_code();
                if !state.rooms.contains_key(&code) { break code; }
            };
            let room = Arc::new(Room { owner, host: Mutex::new(Some(stream.try_clone()?)) });
            state.rooms.insert(code.clone(), Arc::clone(&room));
            (code, room)
        };
        let mut reply = vec![REPLY_CODE];
        reply.extend_from_slice(code.as_bytes());
        if let Err(e) = stream.write_all(&reply) {
            self.close_room(&code, &room);
            return Err(e);
        }
        log(&format!("room opened ({} open)", self.state().rooms.len()));
        let opened = Instant::now();
        let mut spoke = Instant::now();
        let tick = Duration::from_millis(200).min(self.limits.keepalive);
        loop {
            thread::sleep(tick);
            let mut slot = room.host.lock().unwrap_or_else(|e| e.into_inner());
            let Some(host) = slot.as_mut() else {
                return Ok(()); // a joiner took it: the pair is theirs now
            };
            // A host that hung up, or speaks out of turn, loses the room.
            host.set_nonblocking(true)?;
            let mut probe = [0u8; 1];
            let gone = match host.peek(&mut probe) {
                Ok(_) => true, // 0 = closed; anything else is not allowed yet
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => false,
                Err(_) => true,
            };
            host.set_nonblocking(false)?;
            let expired = opened.elapsed() >= self.limits.room_ttl;
            if !gone && !expired && spoke.elapsed() >= self.limits.keepalive {
                spoke = Instant::now();
                if host.write_all(&[REPLY_KEEPALIVE]).is_err() {
                    drop(slot);
                    self.close_room(&code, &room);
                    return Ok(());
                }
            }
            if gone || expired {
                let _ = host.shutdown(Shutdown::Both);
                *slot = None;
                drop(slot);
                self.close_room(&code, &room);
                log(if expired { "room expired" } else { "room abandoned" });
                return Ok(());
            }
        }
    }

    fn close_room(&self, code: &str, room: &Arc<Room>) {
        let mut state = self.state();
        if state.rooms.get(code).is_some_and(|r| Arc::ptr_eq(r, room)) {
            state.rooms.remove(code);
        }
    }

    /// Too many wrong codes from one address: refuse it for a while.
    fn struck_out(&self, ip: IpAddr, wrong: bool) -> bool {
        let mut state = self.state();
        let now = Instant::now();
        let window = self.limits.strike_window;
        state.strikes.retain(|_, (_, since)| now.duration_since(*since) < window);
        let entry = state.strikes.entry(ip).or_insert((0, now));
        if wrong { entry.0 += 1; }
        entry.0 >= self.limits.strikes
    }

    fn join(&self, mut joiner: TcpStream, peer: SocketAddr, code: &[u8]) -> io::Result<()> {
        if self.struck_out(peer.ip(), false) {
            return joiner.write_all(&[REPLY_BUSY]);
        }
        let code = std::str::from_utf8(code).unwrap_or("");
        let room = self.state().rooms.remove(code);
        let host = room.and_then(|room|
            room.host.lock().unwrap_or_else(|e| e.into_inner()).take());
        let Some(mut host) = host else {
            let busy = self.struck_out(peer.ip(), true);
            return joiner.write_all(&[if busy { REPLY_BUSY } else { REPLY_NO_ROOM }]);
        };
        if host.write_all(&[REPLY_PAIRED]).is_err() {
            return joiner.write_all(&[REPLY_NO_ROOM]);
        }
        joiner.write_all(&[REPLY_PAIRED])?;
        log("pair linked");
        pipe(host, joiner, self.limits.idle)
    }
}

/// Copy both ways until either side closes or the pair goes idle, then
/// close both so neither console waits on a half-open link.
fn pipe(a: TcpStream, b: TcpStream, idle: Duration) -> io::Result<()> {
    for s in [&a, &b] {
        s.set_read_timeout(Some(idle))?;
        s.set_write_timeout(Some(Duration::from_secs(30)))?;
    }
    let (a2, b2) = (a.try_clone()?, b.try_clone()?);
    let back = thread::Builder::new().name("pipe".into())
        .spawn(move || copy(b2, a2))?;
    copy(a, b);
    let _ = back.join();
    log("pair closed");
    Ok(())
}

fn copy(mut from: TcpStream, mut to: TcpStream) {
    let mut buf = [0u8; 16 * 1024];
    loop {
        match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => if to.write_all(&buf[..n]).is_err() { break },
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    let _ = from.shutdown(Shutdown::Both);
    let _ = to.shutdown(Shutdown::Both);
}

fn main() {
    let listen = std::env::args().nth(1).unwrap_or_else(|| "0.0.0.0:7212".into());
    let listener = match TcpListener::bind(&listen) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("slot-relay: cannot listen on {listen}: {e}");
            std::process::exit(1);
        }
    };
    log(&format!("listening on {listen}"));
    Relay::new(Limits::default()).serve(listener);
}

#[cfg(test)]
#[path = "../../../crates/slot-android/src/link_relay.rs"]
#[allow(dead_code)]
mod client;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn start(limits: Limits) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let relay = Relay::new(limits);
        thread::spawn(move || relay.serve(listener));
        addr
    }

    fn never() -> bool { false }

    /// Host in a thread; returns its code and the eventual paired socket.
    fn open_room(relay: &str, bound: Duration)
        -> (String, thread::JoinHandle<io::Result<TcpStream>>) {
        let (tx, rx) = mpsc::channel();
        let relay = relay.to_owned();
        let handle = thread::spawn(move || {
            client::host(&relay, bound, &never, &mut |code| { tx.send(code.to_owned()).unwrap(); })
        });
        let code = rx.recv_timeout(Duration::from_secs(5)).expect("a code");
        (code, handle)
    }

    #[test]
    fn host_and_join_are_paired_and_bytes_cross_both_ways() {
        let relay = start(Limits::default());
        let (code, host) = open_room(&relay, Duration::from_secs(10));
        assert_eq!(code.len(), 4);
        assert!(code.bytes().all(|b| b.is_ascii_digit()));
        let mut joiner = client::join(&relay, &client::display_code(&code), &never).unwrap();
        let mut host = host.join().unwrap().unwrap();
        // The KMLB greeting is the joiner speaking first, like on Wi-Fi.
        joiner.write_all(b"KMLB-greeting").unwrap();
        let mut got = [0u8; 13];
        host.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"KMLB-greeting");
        host.write_all(&[0x06, 1, 2, 3]).unwrap();
        let mut back = [0u8; 4];
        joiner.read_exact(&mut back).unwrap();
        assert_eq!(back, [0x06, 1, 2, 3]);
        // One side hanging up ends the other's link too.
        drop(host);
        joiner.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut rest = [0u8; 1];
        assert_eq!(joiner.read(&mut rest).unwrap_or(0), 0);
    }

    #[test]
    fn a_code_works_once() {
        let relay = start(Limits::default());
        let (code, host) = open_room(&relay, Duration::from_secs(10));
        let _first = client::join(&relay, &code, &never).unwrap();
        let _host = host.join().unwrap().unwrap();
        let again = client::join(&relay, &code, &never).unwrap_err();
        assert_eq!(again.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_wrong_code_is_refused_and_guessing_is_stopped() {
        let relay = start(Limits { strikes: 3, ..Limits::default() });
        for _ in 0..2 {
            let e = client::join(&relay, "0000", &never).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::NotFound);
        }
        let e = client::join(&relay, "0001", &never).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        // Even a right code is refused while struck out.
        let (code, _host) = open_room(&relay, Duration::from_secs(5));
        let e = client::join(&relay, &code, &never).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_host_waits_through_keepalives() {
        let relay = start(Limits { keepalive: Duration::from_millis(100), ..Limits::default() });
        let (code, host) = open_room(&relay, Duration::from_secs(10));
        thread::sleep(Duration::from_millis(700));
        let _joiner = client::join(&relay, &code, &never).unwrap();
        assert!(host.join().unwrap().is_ok());
    }

    #[test]
    fn a_cancelled_host_gives_up_and_its_room_closes() {
        let relay = start(Limits::default());
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let (r, s) = (relay.clone(), Arc::clone(&stop));
        let host = thread::spawn(move || {
            let cancelled = || s.load(Ordering::SeqCst);
            client::host(&r, Duration::from_secs(30), &cancelled,
                &mut |code| { tx.send(code.to_owned()).unwrap(); })
        });
        let code = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        stop.store(true, Ordering::SeqCst);
        let e = host.join().unwrap().unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Interrupted);
        thread::sleep(Duration::from_millis(600)); // relay notices the hang-up
        let e = client::join(&relay, &code, &never).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_room_nobody_joins_expires_for_the_host() {
        let relay = start(Limits { room_ttl: Duration::from_millis(300), ..Limits::default() });
        let (_code, host) = open_room(&relay, Duration::from_secs(10));
        let e = host.join().unwrap().unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::ConnectionAborted);
    }

    #[test]
    fn the_host_gives_up_at_its_own_bound() {
        let relay = start(Limits::default());
        let (_code, host) = open_room(&relay, Duration::from_millis(400));
        let e = host.join().unwrap().unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn rooms_per_address_are_limited() {
        let relay = start(Limits { rooms_per_ip: 1, ..Limits::default() });
        let (_code, _host) = open_room(&relay, Duration::from_secs(5));
        let r = relay.clone();
        let e = client::host(&r, Duration::from_secs(1), &never, &mut |_| {}).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::ConnectionRefused);
    }

    #[test]
    fn another_version_is_told_so() {
        let relay = start(Limits::default());
        let mut s = TcpStream::connect(&relay).unwrap();
        s.write_all(b"SLRY\x09H").unwrap();
        let mut b = [0u8; 1];
        s.read_exact(&mut b).unwrap();
        assert_eq!(b[0], REPLY_VERSION);
    }

    #[test]
    fn codes_are_four_digits() {
        for _ in 0..1000 {
            let c = random_code();
            assert_eq!(c.len(), 4);
            assert!(c.bytes().all(|b| b.is_ascii_digit()));
        }
    }
}

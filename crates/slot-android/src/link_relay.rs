//! Internet Multiplayer: meeting the other console through slot.'s relay.
//!
//! On a home network the consoles find each other with mDNS and one of them
//! listens. Over the Internet neither can listen (home routers, CGNAT), so
//! both dial out to a small relay instead (`tools/slot-relay`). The host asks
//! for a room and is given a four-digit code; the player who types that code
//! is paired with it, and from then on the relay copies bytes both ways and
//! understands none of them. The KMLB greeting, ROM/BIOS exchange and the
//! game itself run over the relayed socket exactly as they do over Wi-Fi.
//!
//! Wire format, version 2 (version 1 had six-digit codes). Every client
//! opens with `SLRY`, the version, then
//!   `H`               host: the relay answers `C` + four ASCII digits, then
//!                     `K` every few seconds while nobody has come, then `P`
//!   `J` + 4 digits    join: the relay answers `P` (paired) or a refusal
//! Refusals are one byte: `N` no such room, `B` too many tries, `F` relay
//! full, `V` unsupported version. After `P` the socket is the other player's.
//!
//! This file depends on nothing else in the crate so the relay's own tests
//! can include it and prove both ends agree.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub const MAGIC: &[u8; 4] = b"SLRY";
pub const VERSION: u8 = 2;
pub const DEFAULT_PORT: u16 = 7212;
pub const CODE_LEN: usize = 4;

pub const OP_HOST: u8 = b'H';
pub const OP_JOIN: u8 = b'J';
pub const REPLY_CODE: u8 = b'C';
pub const REPLY_KEEPALIVE: u8 = b'K';
pub const REPLY_PAIRED: u8 = b'P';
pub const REPLY_NO_ROOM: u8 = b'N';
pub const REPLY_BUSY: u8 = b'B';
pub const REPLY_FULL: u8 = b'F';
pub const REPLY_VERSION: u8 = b'V';

/// How long one look at the socket lasts, and so how late a cancel is seen.
const LOOK: Duration = Duration::from_millis(250);
const DIAL: Duration = Duration::from_secs(6);
/// The relay speaks at least every few seconds while a host waits; silence
/// this long means the relay or the route to it is gone.
const SILENCE: Duration = Duration::from_secs(45);

/// Four digits, whatever was typed or pasted around them ("48 29",
/// "48-29", "code: 4829"). Anything else is not a room code.
pub fn normalize_code(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    (digits.len() == CODE_LEN).then_some(digits)
}

/// Four digits are read out and shared as they are: "4829".
pub fn display_code(code: &str) -> String {
    code.to_owned()
}

/// `host`, `host:port`, `1.2.3.4` or `[::1]:7212`. Port defaults to 7212.
pub fn relay_addresses(relay: &str) -> io::Result<Vec<SocketAddr>> {
    let relay = relay.trim();
    if relay.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound,
            "no Internet relay is configured in this build"));
    }
    let with_port = if relay.parse::<SocketAddr>().is_ok()
        || (relay.rsplit_once(':').is_some_and(|(h, p)|
            !h.contains(':') && p.parse::<u16>().is_ok())) {
        relay.to_owned()
    } else if relay.contains(':') && !relay.starts_with('[') {
        format!("[{relay}]:{DEFAULT_PORT}")
    } else {
        format!("{relay}:{DEFAULT_PORT}")
    };
    let found: Vec<SocketAddr> = with_port.to_socket_addrs()?.collect();
    if found.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound,
            "the Internet relay's name did not resolve"));
    }
    Ok(found)
}

fn dial(relay: &str) -> io::Result<TcpStream> {
    let mut last = None;
    let addrs = relay_addresses(relay).map_err(|e| io::Error::new(e.kind(),
        format!("Internet relay unreachable: {e}")))?;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, DIAL) {
            Ok(stream) => {
                // Every serial packet is tiny and waited on: never batch.
                stream.set_nodelay(true)?;
                return Ok(stream);
            }
            Err(error) => last = Some(error),
        }
    }
    Err(match last {
        Some(e) => io::Error::new(e.kind(), format!("Internet relay unreachable: {e}")),
        None => io::Error::new(io::ErrorKind::NotFound, "the Internet relay has no address"),
    })
}

fn refusal(byte: u8) -> io::Error {
    match byte {
        REPLY_NO_ROOM => io::Error::new(io::ErrorKind::NotFound,
            "no room has this code"),
        REPLY_BUSY => io::Error::new(io::ErrorKind::PermissionDenied,
            "too many wrong codes, wait a few minutes"),
        REPLY_FULL => io::Error::new(io::ErrorKind::ConnectionRefused,
            "the Internet relay is full"),
        REPLY_VERSION => io::Error::new(io::ErrorKind::InvalidData,
            "the Internet relay does not speak this slot. version"),
        other => io::Error::new(io::ErrorKind::InvalidData,
            format!("unexpected relay reply 0x{other:02x}")),
    }
}

/// Read one byte, checking `cancelled` between short looks. Gives up after
/// `SILENCE` without a byte, or at `deadline`.
fn next_byte(stream: &mut TcpStream, cancelled: &dyn Fn() -> bool,
             deadline: Option<Instant>) -> io::Result<u8> {
    stream.set_read_timeout(Some(LOOK))?;
    let quiet_since = Instant::now();
    let mut byte = [0u8; 1];
    loop {
        if cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted,
                "cancelled while waiting on the relay"));
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "no peer arrived"));
        }
        if quiet_since.elapsed() >= SILENCE {
            return Err(io::Error::new(io::ErrorKind::ConnectionAborted,
                "the Internet relay went quiet"));
        }
        match stream.read(&mut byte) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::ConnectionAborted,
                "the Internet relay closed the connection")),
            Ok(_) => return Ok(byte[0]),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock
                | io::ErrorKind::TimedOut) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

fn read_exact_cancellable(stream: &mut TcpStream, out: &mut [u8],
                          cancelled: &dyn Fn() -> bool) -> io::Result<()> {
    for slot in out.iter_mut() {
        *slot = next_byte(stream, cancelled, None)?;
    }
    Ok(())
}

fn finish(stream: TcpStream) -> io::Result<TcpStream> {
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    Ok(stream)
}

/// Open a room. `on_code` is told the code as soon as the relay hands it out
/// (to show and share it); the call then waits up to `bound` for the other
/// player and returns the paired socket.
pub fn host(relay: &str, bound: Duration, cancelled: &dyn Fn() -> bool,
            on_code: &mut dyn FnMut(&str)) -> io::Result<TcpStream> {
    let mut stream = dial(relay)?;
    stream.set_write_timeout(Some(DIAL))?;
    let mut hello = Vec::with_capacity(6);
    hello.extend_from_slice(MAGIC);
    hello.push(VERSION);
    hello.push(OP_HOST);
    stream.write_all(&hello)?;
    let first = next_byte(&mut stream, cancelled, None)?;
    if first != REPLY_CODE { return Err(refusal(first)); }
    let mut digits = [0u8; CODE_LEN];
    read_exact_cancellable(&mut stream, &mut digits, cancelled)?;
    let code = std::str::from_utf8(&digits).ok().and_then(normalize_code)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData,
            "the relay sent an unreadable room code"))?;
    on_code(&code);
    let deadline = Instant::now() + bound;
    loop {
        match next_byte(&mut stream, cancelled, Some(deadline))? {
            REPLY_KEEPALIVE => continue,
            REPLY_PAIRED => return finish(stream),
            other => return Err(refusal(other)),
        }
    }
}

/// Enter the room `code`. Returns the socket paired with its host.
pub fn join(relay: &str, code: &str, cancelled: &dyn Fn() -> bool)
    -> io::Result<TcpStream> {
    let code = normalize_code(code).ok_or_else(|| io::Error::new(
        io::ErrorKind::InvalidInput, "a room code is four digits"))?;
    let mut stream = dial(relay)?;
    stream.set_write_timeout(Some(DIAL))?;
    let mut hello = Vec::with_capacity(6 + CODE_LEN);
    hello.extend_from_slice(MAGIC);
    hello.push(VERSION);
    hello.push(OP_JOIN);
    hello.extend_from_slice(code.as_bytes());
    stream.write_all(&hello)?;
    match next_byte(&mut stream, cancelled, None)? {
        REPLY_PAIRED => finish(stream),
        other => Err(refusal(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_four_digits_whatever_surrounds_them() {
        assert_eq!(normalize_code("4829").as_deref(), Some("4829"));
        assert_eq!(normalize_code(" 48 29 ").as_deref(), Some("4829"));
        assert_eq!(normalize_code("code: 48-29").as_deref(), Some("4829"));
        assert_eq!(normalize_code("482"), None);
        assert_eq!(normalize_code("48291"), None);
        assert_eq!(display_code("4829"), "4829");
    }

    #[test]
    fn relay_address_gets_the_default_port() {
        let found = relay_addresses("127.0.0.1").unwrap();
        assert_eq!(found[0].port(), DEFAULT_PORT);
        let found = relay_addresses("127.0.0.1:9000").unwrap();
        assert_eq!(found[0].port(), 9000);
        let found = relay_addresses("::1").unwrap();
        assert_eq!(found[0].port(), DEFAULT_PORT);
        assert!(relay_addresses("  ").is_err());
    }
}

//! Gambatte's Game Link over Wi-Fi, the way RetroArch plays it: each console
//! runs only its own Game Boy and the core trades every serial byte with the
//! other over TCP (server = Host, client = Join). Slot's HOST/JOIN pairing
//! finds the other console and fills in the address, so there is nothing to
//! configure. The pairing socket stays open beside it for pause and End Link.
use std::net::Ipv4Addr;

use slot_retro::{LinkChannel, RetroCore};

use crate::link_play::LinkError;
use crate::multiplayer::Role;

/// Gambatte's own first Game Link port.
pub const PORT: &str = "56400";

/// The core options that start Game Link. Gambatte spells the server address
/// as twelve one-digit options, three per octet.
pub fn options(role: Role, host: Ipv4Addr) -> Vec<(String, String)> {
    let mut options = vec![
        ("gambatte_gb_link_network_port".to_owned(), PORT.to_owned()),
    ];
    let digits: String = host.octets().iter().map(|o| format!("{o:03}")).collect();
    for (i, digit) in digits.chars().enumerate() {
        options.push((format!("gambatte_gb_link_network_server_ip_{}", i + 1), digit.to_string()));
    }
    // The mode goes last: it is what makes the core open its socket.
    options.push(("gambatte_gb_link_mode".to_owned(), match role {
        Role::Host => "Network Server",
        Role::Join => "Network Client",
    }.to_owned()));
    options
}

pub struct GameLink {
    wire: Box<dyn LinkChannel>,
    local_paused: bool,
}

impl GameLink {
    pub fn begin(core: &mut dyn RetroCore, role: Role, host: Ipv4Addr,
                 wire: Box<dyn LinkChannel>) -> Self {
        for (key, value) in options(role, host) {
            core.set_option(&key, &value);
        }
        Self { wire, local_paused: false }
    }

    pub fn fault(&self) -> Option<LinkError> {
        if self.wire.peer_ended() {
            Some(LinkError::PeerEnded)
        } else if self.wire.is_closed() {
            Some(LinkError::Disconnected)
        } else {
            None
        }
    }

    /// Both consoles stop while either one has a menu open, so a transfer
    /// never waits on a console that is not running.
    pub fn may_run(&self) -> bool {
        self.fault().is_none() && !self.local_paused && !self.wire.peer_paused()
    }

    pub fn set_local_paused(&mut self, paused: bool) {
        if self.local_paused != paused {
            self.local_paused = paused;
            self.wire.set_paused(paused);
        }
    }

    pub fn end(&mut self, core: &mut dyn RetroCore) {
        core.set_option("gambatte_gb_link_mode", "Not Connected");
        self.wire.send_end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn the_host_address_is_spelled_as_twelve_digits() {
        let options = options(Role::Join, Ipv4Addr::new(192, 168, 1, 23));
        let ip: String = (1..=12).map(|i| options.iter()
            .find(|(k, _)| *k == format!("gambatte_gb_link_network_server_ip_{i}"))
            .unwrap().1.clone()).collect();
        assert_eq!(ip, "192168001023");
        assert_eq!(options.last().unwrap(),
            &("gambatte_gb_link_mode".to_owned(), "Network Client".to_owned()));
        assert_eq!(super::options(Role::Host, Ipv4Addr::UNSPECIFIED).last().unwrap().1,
            "Network Server");
    }

    #[derive(Default)]
    struct Wire { paused: Arc<AtomicBool>, peer_paused: Arc<AtomicBool>,
                  ended: Arc<AtomicBool>, sent_end: Arc<AtomicBool> }
    impl LinkChannel for Wire {
        fn send(&mut self, _flags: i32, _buf: &[u8]) {}
        fn try_recv(&mut self) -> Option<Vec<u8>> { None }
        fn set_paused(&mut self, paused: bool) { self.paused.store(paused, Ordering::SeqCst); }
        fn peer_paused(&self) -> bool { self.peer_paused.load(Ordering::SeqCst) }
        fn peer_ended(&self) -> bool { self.ended.load(Ordering::SeqCst) }
        fn send_end(&mut self) { self.sent_end.store(true, Ordering::SeqCst); }
    }

    #[test]
    fn either_pause_holds_both_and_end_unplugs_the_core() {
        let wire = Wire::default();
        let (paused, peer_paused, ended, sent_end) = (wire.paused.clone(),
            wire.peer_paused.clone(), wire.ended.clone(), wire.sent_end.clone());
        let mut core = slot_retro::MockCore::new();
        let mut link = GameLink::begin(&mut core, Role::Host, Ipv4Addr::LOCALHOST, Box::new(wire));
        assert!(link.may_run());
        link.set_local_paused(true);
        assert!(paused.load(Ordering::SeqCst) && !link.may_run());
        link.set_local_paused(false);
        peer_paused.store(true, Ordering::SeqCst);
        assert!(!link.may_run());
        peer_paused.store(false, Ordering::SeqCst);
        ended.store(true, Ordering::SeqCst);
        assert_eq!(link.fault(), Some(LinkError::PeerEnded));
        link.end(&mut core);
        assert!(sent_end.load(Ordering::SeqCst));
    }
}

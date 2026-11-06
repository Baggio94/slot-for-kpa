//! Settings > Controls: which controller plays on each port, and what each
//! of its keys is to slot.
//!
//! Every controller Android reports is known by a fingerprint: "builtin" for
//! the handheld's own controls (all of its internal input devices are one
//! controller), otherwise Android's input-device descriptor, which stays the
//! same when a Bluetooth controller reconnects. A controller's bindings
//! belong to it, not to its port: they follow it from Port 2 to Port 1.
//!
//! slot. itself only ever sees its own sixteen buttons (BUTTONS, as the
//! Android key codes it has always read) plus the slot. button (MODE), which
//! can't be remapped, so a remapped controller drives the games, the menus
//! and every shortcut alike. Pure: no Android, no GL.
#![allow(unexpected_cfgs)]
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const BUILTIN: &str = "builtin";
/// The slot. button (KONKR top key, Retroid centre key): never remapped.
pub const MODE: i32 = 110;
pub const PORTS: usize = 2;
/// How long a Map or Detect waits for a button.
pub const LISTEN_MS: u64 = 5_000;
pub const FILE: &str = "slot-konkr-controls.json";

/// slot.'s own buttons, in the Port page's order, with the Android key code
/// each has always been read as.
pub const BUTTONS: [(&str, i32); 16] = [
    ("Up", 19), ("Down", 20), ("Left", 21), ("Right", 22),
    ("A", 96), ("B", 97), ("X", 99), ("Y", 100),
    ("L1", 102), ("R1", 103), ("L2", 104), ("R2", 105),
    ("L3", 106), ("R3", 107), ("SELECT", 109), ("START", 108),
];

/// A controller's own settings. Only what differs from the defaults is kept.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pad {
    /// Last name Android gave it, to show it while it's off.
    pub name: String,
    /// slot. button name -> the controller's key code; 0 = no key.
    pub binds: BTreeMap<String, i32>,
    /// The left stick drives the D-pad (true by default).
    pub stick_off: bool,
}

impl Pad {
    pub fn bind(&self, button: usize) -> Option<i32> {
        let (name, default) = BUTTONS[button];
        match self.binds.get(name).copied().unwrap_or(default) {
            0 => None,
            code => Some(code),
        }
    }

    fn set(&mut self, button: usize, code: Option<i32>) {
        let (name, default) = BUTTONS[button];
        let code = code.unwrap_or(0);
        if code == default { self.binds.remove(name); }
        else { self.binds.insert(name.to_owned(), code); }
    }

    /// The slot. button this controller key is, if any.
    pub fn button_for(&self, code: i32) -> Option<usize> {
        (0..BUTTONS.len()).find(|&button| self.bind(button) == Some(code))
    }

    /// Bind `code` to `button`. A key already bound elsewhere swaps with it,
    /// so no key ever does two things.
    pub fn map(&mut self, button: usize, code: i32) {
        let old = self.bind(button);
        if let Some(other) = self.button_for(code).filter(|&other| other != button) {
            self.set(other, old);
        }
        self.set(button, Some(code));
    }

    pub fn clear(&mut self, button: usize) { self.set(button, None); }

    pub fn is_default(&self) -> bool { self.binds.is_empty() && !self.stick_off }

    pub fn reset(&mut self) {
        self.binds.clear();
        self.stick_off = false;
    }
}

/// A controller Android reports right now.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Device {
    pub id: i32,
    pub name: String,
    pub fp: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Controls {
    /// Fingerprint of the controller on each port; "" = none.
    pub ports: [String; PORTS],
    pub pads: BTreeMap<String, Pad>,
}

impl Default for Controls {
    fn default() -> Self {
        Self { ports: [BUILTIN.to_owned(), String::new()], pads: BTreeMap::new() }
    }
}

impl Controls {
    pub fn port_of(&self, fp: &str) -> Option<usize> {
        (!fp.is_empty()).then(|| self.ports.iter().position(|p| p == fp)).flatten()
    }

    pub fn on_port(&self, port: usize) -> Option<&str> {
        self.ports.get(port).map(String::as_str).filter(|fp| !fp.is_empty())
    }

    pub fn pad(&self, fp: &str) -> Pad {
        self.pads.get(fp).cloned().unwrap_or_default()
    }

    pub fn pad_mut(&mut self, fp: &str) -> &mut Pad {
        self.pads.entry(fp.to_owned()).or_default()
    }

    /// What a controller key is to slot.: one of BUTTONS' codes, MODE for the
    /// slot. button, or nothing. A stick direction is the D-pad's unless the
    /// stick is off, whatever the D-pad keys are bound to.
    pub fn slot_code(&self, fp: &str, code: i32, stick: bool) -> Option<i32> {
        let pad = self.pads.get(fp);
        if stick {
            let off = pad.is_some_and(|p| p.stick_off);
            return (!off && (19..=22).contains(&code)).then_some(code);
        }
        if code == MODE { return Some(MODE); }
        let button = match pad {
            Some(pad) => pad.button_for(code),
            None => BUTTONS.iter().position(|(_, default)| *default == code),
        }?;
        Some(BUTTONS[button].1)
    }

    /// Put `fp` on `port`. A controller already on the other port swaps
    /// with this one; Port 1 is never left empty.
    pub fn assign(&mut self, port: usize, fp: &str) {
        if port >= PORTS { return; }
        let old = std::mem::take(&mut self.ports[port]);
        if let Some(other) = self.port_of(fp) {
            self.ports[other] = if other == 0 && old.is_empty() { BUILTIN.to_owned() } else { old };
        }
        self.ports[port] = fp.to_owned();
        if self.ports[0].is_empty() { self.ports[0] = BUILTIN.to_owned(); }
        if self.ports[1] == self.ports[0] { self.ports[1].clear(); }
    }

    /// A Bluetooth controller pressed a button for the first time: it takes
    /// Port 2 if nobody has it. True when it did.
    pub fn claim_free_port(&mut self, fp: &str) -> bool {
        if fp == BUILTIN || fp.is_empty() || self.port_of(fp).is_some() || self.on_port(1).is_some() {
            return false;
        }
        self.ports[1] = fp.to_owned();
        true
    }

    /// The controllers ◀▶ goes through on `port`, in order. `connected` is
    /// every connected controller's fingerprint (built-in included).
    pub fn candidates(&self, port: usize, connected: &[String]) -> Vec<String> {
        let mut list: Vec<String> = Vec::new();
        if port == 1 { list.push(String::new()); }
        let mut push = |fp: &str| if !list.iter().any(|f| f == fp) { list.push(fp.to_owned()); };
        push(BUILTIN);
        for fp in connected { push(fp); }
        // A controller that is off stays a stop while it holds this port.
        if let Some(fp) = self.on_port(port) { push(fp); }
        // An empty Port 2 can't take Port 1's controller: Port 1 would be empty.
        if port == 1 && self.on_port(1).is_none() {
            if let Some(first) = self.on_port(0) { list.retain(|fp| fp != first); }
        }
        list
    }

    /// ◀▶ on a port's Controller row.
    pub fn cycle(&mut self, port: usize, right: bool, connected: &[String]) {
        let list = self.candidates(port, connected);
        if list.is_empty() { return; }
        let current = self.ports[port].as_str();
        let at = list.iter().position(|fp| fp == current).unwrap_or(0);
        let next = if right { (at + 1) % list.len() } else { (at + list.len() - 1) % list.len() };
        let fp = list[next].clone();
        if fp.is_empty() { self.ports[port].clear(); } else { self.assign(port, &fp); }
    }

    /// Settings > Controls > Reset to Default: every controller's keys back
    /// to the defaults. Who is on which port stays.
    pub fn reset_all(&mut self) {
        for pad in self.pads.values_mut() { pad.reset(); }
    }

    /// Remember the name Android gives a controller, for when it's off.
    /// True when something changed.
    pub fn remember(&mut self, device: &Device) -> bool {
        if device.fp == BUILTIN || self.pads.get(&device.fp).is_some_and(|p| p.name == device.name) {
            return false;
        }
        if self.port_of(&device.fp).is_none() && !self.pads.contains_key(&device.fp) { return false; }
        self.pad_mut(&device.fp).name = device.name.clone();
        true
    }
}

/// The name shown for a key of a controller.
pub fn key_name(code: i32) -> String {
    match code {
        19 => "D-Pad Up".into(), 20 => "D-Pad Down".into(),
        21 => "D-Pad Left".into(), 22 => "D-Pad Right".into(),
        96 => "A".into(), 97 => "B".into(), 98 => "C".into(),
        99 => "X".into(), 100 => "Y".into(), 101 => "Z".into(),
        102 => "L1".into(), 103 => "R1".into(), 104 => "L2".into(), 105 => "R2".into(),
        106 => "L3".into(), 107 => "R3".into(),
        108 => "START".into(), 109 => "SELECT".into(), 110 => "MODE".into(),
        // KEYCODE_BUTTON_1..16, sent by generic (DirectInput) controllers.
        188..=203 => format!("Button {}", code - 187),
        4 => "Back".into(), 23 => "D-Pad Center".into(), 66 => "Enter".into(),
        62 => "Space".into(),
        _ => format!("Key {code}"),
    }
}

/// Where a pressed controller key went, so its release goes there too even
/// if a menu opened (or a game started) in between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sink {
    /// slot.'s Port 1 input: games, menus, shortcuts.
    Slot(i32),
    /// The second player's buttons in a NES, SNES or Mega Drive game.
    Port2(i32),
    /// Consumed (a Map or Detect), or ignored.
    Nowhere,
}

/// What the engine is doing when a key arrives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Moment {
    /// A game is on screen with no menu over it.
    pub playing: bool,
    /// That game takes a second player here: a NES, SNES or Mega Drive game
    /// outside Multiplayer.
    pub second_player: bool,
}

/// Where a key pressed on `port` (None: on no port) goes. `code` is already
/// slot.'s (Controls::slot_code).
pub fn sink(port: Option<usize>, code: i32, moment: Moment) -> Sink {
    match port {
        Some(0) => Sink::Slot(code),
        // Port 2's slot. button would let the second player eject.
        Some(_) if code == MODE => Sink::Nowhere,
        Some(_) if moment.playing =>
            if moment.second_player { Sink::Port2(code) } else { Sink::Nowhere },
        // The menus answer every controller.
        Some(_) => Sink::Slot(code),
        // A controller on no port (the handheld, when two Bluetooth
        // controllers play) keeps the slot. button and the menus.
        None if code == MODE => Sink::Slot(code),
        None if moment.playing => Sink::Nowhere,
        None => Sink::Slot(code),
    }
}

/// Presses not yet released, by (Android device id, key code).
#[derive(Debug, Default)]
pub struct Held {
    keys: BTreeMap<(i32, i32), Sink>,
}

impl Held {
    pub fn press(&mut self, device: i32, code: i32, sink: Sink) {
        self.keys.insert((device, code), sink);
    }

    /// The sink of a press, forgotten now. None when the press was never
    /// seen (it began before slot. was in front): ignore the release.
    pub fn release(&mut self, device: i32, code: i32) -> Option<Sink> {
        self.keys.remove(&(device, code))
    }

    /// Every key a device still holds, released: it was disconnected.
    pub fn drop_device(&mut self, device: i32) -> Vec<Sink> {
        let keys: Vec<(i32, i32)> = self.keys.keys().copied().filter(|(d, _)| *d == device).collect();
        keys.into_iter().filter_map(|key| self.keys.remove(&key)).collect()
    }

    pub fn holds(&self, device: i32, code: i32) -> bool { self.keys.contains_key(&(device, code)) }
}

/// A Map (one slot. button of a port's controller) or a Detect (whichever
/// controller presses a button takes the port) waiting for its button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listen {
    Key { port: usize, button: usize, until: u64 },
    Controller { port: usize, until: u64 },
}

impl Listen {
    pub fn until(self) -> u64 {
        match self { Listen::Key { until, .. } | Listen::Controller { until, .. } => until }
    }
    pub fn port(self) -> usize {
        match self { Listen::Key { port, .. } | Listen::Controller { port, .. } => port }
    }
    /// Whole seconds left, for the countdown: 5 .. 1.
    pub fn seconds_left(self, now: u64) -> u64 {
        self.until().saturating_sub(now).div_ceil(1000).max(1)
    }
}

/// A row of a Port page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortRow { Controller, Button(usize), Stick, Reset }

/// A port with no controller only offers its Controller row.
pub fn port_rows(has_controller: bool) -> Vec<PortRow> {
    let mut rows = vec![PortRow::Controller];
    if has_controller {
        rows.extend((0..BUTTONS.len()).map(PortRow::Button));
        rows.push(PortRow::Stick);
        rows.push(PortRow::Reset);
    }
    rows
}

/// Rows a scrolling page shows at once.
pub const VISIBLE_ROWS: usize = 6;

/// First row shown: the selected row stays third from the top while the
/// list scrolls, and the list never scrolls past its end.
pub fn scroll_offset(selected: usize, rows: usize) -> usize {
    selected.saturating_sub(2).min(rows.saturating_sub(VISIBLE_ROWS))
}

#[cfg(not(slot_harness))]
mod file {
    use std::path::Path;
    use super::*;

    impl Controls {
        /// A missing or unreadable file is the defaults: controls must never
        /// keep a player out of slot.
        pub fn load(dir: &Path) -> Self {
            std::fs::read(dir.join(FILE)).ok()
                .and_then(|bytes| serde_json::from_slice::<Controls>(&bytes).ok())
                .map(Controls::sanitized)
                .unwrap_or_default()
        }

        pub fn save(&self, dir: &Path) -> std::io::Result<()> {
            std::fs::create_dir_all(dir)?;
            let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
            slot_store::atomic_write(&dir.join(FILE), &bytes)
        }
    }

    pub fn devices(json: &str) -> Vec<Device> {
        serde_json::from_str(json).unwrap_or_default()
    }
}
#[cfg(not(slot_harness))]
pub use file::devices;

impl Controls {
    /// A hand-edited file can't leave Port 1 empty or one controller on both.
    pub fn sanitized(mut self) -> Self {
        if self.ports[0].is_empty() { self.ports[0] = BUILTIN.to_owned(); }
        if self.ports[1] == self.ports[0] { self.ports[1].clear(); }
        self.pads.retain(|_, pad| !pad.is_default() || !pad.name.is_empty());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAD: &str = "8bitdo";

    #[test]
    fn a_new_install_plays_exactly_as_before() {
        let controls = Controls::default();
        assert_eq!(controls.on_port(0), Some(BUILTIN));
        assert_eq!(controls.on_port(1), None);
        for (_, code) in BUTTONS {
            assert_eq!(controls.slot_code(BUILTIN, code, false), Some(code));
        }
        assert_eq!(controls.slot_code(BUILTIN, MODE, false), Some(MODE));
        assert_eq!(controls.slot_code(BUILTIN, 98, false), None, "BUTTON_C isn't a slot. button");
    }

    #[test]
    fn mapping_a_key_already_used_swaps_the_two_buttons() {
        let mut pad = Pad::default();
        pad.map(4, 97); // A <- the B key
        assert_eq!(pad.bind(4), Some(97));
        assert_eq!(pad.bind(5), Some(96), "B takes A's old key");
        assert_eq!(pad.button_for(96), Some(5));
        pad.map(4, 96);
        pad.map(5, 97);
        assert!(pad.is_default(), "mapping back to the defaults keeps nothing");
    }

    #[test]
    fn a_cleared_button_has_no_key_and_mapping_it_elsewhere_frees_nothing() {
        let mut pad = Pad::default();
        pad.clear(6);
        assert_eq!(pad.bind(6), None);
        assert_eq!(pad.button_for(99), None, "the X key does nothing now");
        pad.map(7, 99); // Y <- the X key: X had no key, so it stays empty
        assert_eq!(pad.bind(7), Some(99));
        assert_eq!(pad.bind(6), None);
    }

    #[test]
    fn mapping_follows_the_controller_not_the_port() {
        let mut controls = Controls::default();
        controls.assign(1, PAD);
        controls.pad_mut(PAD).map(4, 97);
        controls.assign(0, PAD);
        assert_eq!(controls.on_port(0), Some(PAD));
        assert_eq!(controls.on_port(1), Some(BUILTIN), "the two controllers swapped");
        assert_eq!(controls.slot_code(PAD, 97, false), Some(96));
        assert_eq!(controls.slot_code(BUILTIN, 97, false), Some(97));
    }

    #[test]
    fn port_one_is_never_left_empty() {
        let mut controls = Controls::default();
        // Port 2 empty taking Port 1's controller would empty Port 1.
        assert!(!controls.candidates(1, &[BUILTIN.into()]).contains(&BUILTIN.to_owned()));
        controls.assign(1, BUILTIN);
        assert_eq!(controls.on_port(0), Some(BUILTIN));
        assert_eq!(controls.on_port(1), None);
        let mut hand_edited = Controls::default();
        hand_edited.ports = [String::new(), PAD.into()];
        let fixed = hand_edited.sanitized();
        assert_eq!(fixed.on_port(0), Some(BUILTIN));
        assert_eq!(fixed.on_port(1), Some(PAD));
    }

    #[test]
    fn cycling_port_two_goes_through_none_and_every_connected_controller() {
        let mut controls = Controls::default();
        let connected = vec![BUILTIN.to_owned(), PAD.to_owned(), "pro".to_owned()];
        assert_eq!(controls.candidates(1, &connected), vec!["".to_owned(), PAD.into(), "pro".into()]);
        controls.cycle(1, true, &connected);
        assert_eq!(controls.on_port(1), Some(PAD));
        controls.cycle(1, true, &connected);
        assert_eq!(controls.on_port(1), Some("pro"));
        controls.cycle(1, true, &connected);
        assert_eq!(controls.on_port(1), None, "back to None");
        controls.assign(1, PAD);
        controls.cycle(1, false, &connected);
        assert_eq!(controls.on_port(1), Some(BUILTIN), "taken from Port 1 by swapping");
        assert_eq!(controls.on_port(0), Some(PAD));
    }

    #[test]
    fn the_first_bluetooth_controller_takes_a_free_port_two() {
        let mut controls = Controls::default();
        assert!(!controls.claim_free_port(BUILTIN));
        assert!(controls.claim_free_port(PAD));
        assert_eq!(controls.on_port(1), Some(PAD));
        assert!(!controls.claim_free_port("pro"), "Port 2 is taken");
    }

    #[test]
    fn port_two_plays_only_in_games_that_take_a_second_player() {
        let menus = Moment { playing: false, second_player: false };
        let nes = Moment { playing: true, second_player: true };
        let gba = Moment { playing: true, second_player: false };
        assert_eq!(sink(Some(1), 96, nes), Sink::Port2(96));
        assert_eq!(sink(Some(1), 96, gba), Sink::Nowhere);
        assert_eq!(sink(Some(1), 96, menus), Sink::Slot(96), "menus answer Port 2");
        assert_eq!(sink(Some(1), MODE, menus), Sink::Nowhere, "Port 2 can't eject");
        assert_eq!(sink(Some(0), 96, nes), Sink::Slot(96));
        assert_eq!(sink(None, MODE, nes), Sink::Slot(MODE), "the handheld keeps its slot. button");
        assert_eq!(sink(None, 96, nes), Sink::Nowhere);
        assert_eq!(sink(None, 96, menus), Sink::Slot(96));
    }

    #[test]
    fn a_release_goes_where_its_press_went() {
        let mut held = Held::default();
        held.press(7, 96, Sink::Port2(96));
        assert_eq!(held.release(7, 96), Some(Sink::Port2(96)));
        assert_eq!(held.release(7, 96), None, "an unseen press is ignored");
        held.press(7, 96, Sink::Slot(96));
        held.press(7, 97, Sink::Port2(97));
        held.press(8, 96, Sink::Slot(96));
        let mut dropped = held.drop_device(7);
        dropped.sort_by_key(|s| format!("{s:?}"));
        assert_eq!(dropped, vec![Sink::Port2(97), Sink::Slot(96)]);
        assert!(held.holds(8, 96));
    }

    #[test]
    fn the_stick_follows_the_d_pad_unless_it_is_off() {
        let mut controls = Controls::default();
        controls.pad_mut(PAD).map(0, 96); // Up <- the A key
        assert_eq!(controls.slot_code(PAD, 19, true), Some(19), "the stick's up is still Up");
        assert_eq!(controls.slot_code(PAD, 19, false), Some(96), "the D-pad's up key is now A");
        controls.pad_mut(PAD).stick_off = true;
        assert_eq!(controls.slot_code(PAD, 19, true), None);
    }

    #[test]
    fn reset_to_default_keeps_the_ports() {
        let mut controls = Controls::default();
        controls.assign(1, PAD);
        controls.pad_mut(PAD).map(4, 97);
        controls.pad_mut(BUILTIN).stick_off = true;
        controls.reset_all();
        assert_eq!(controls.on_port(1), Some(PAD));
        assert!(controls.pad(PAD).is_default());
        assert!(controls.pad(BUILTIN).is_default());
    }

    #[test]
    fn names_are_remembered_only_for_controllers_slot_keeps() {
        let mut controls = Controls::default();
        let device = Device { id: 9, name: "8BitDo SN30 Pro".into(), fp: PAD.into() };
        assert!(!controls.remember(&device), "a controller on no port leaves no trace");
        controls.assign(1, PAD);
        assert!(controls.remember(&device));
        assert_eq!(controls.pad(PAD).name, "8BitDo SN30 Pro");
        assert!(!controls.remember(&device), "unchanged");
    }

    #[test]
    fn port_pages_scroll_with_the_selection() {
        assert_eq!(port_rows(false), vec![PortRow::Controller]);
        let rows = port_rows(true).len();
        assert_eq!(rows, 19);
        assert_eq!(scroll_offset(0, rows), 0);
        assert_eq!(scroll_offset(2, rows), 0);
        assert_eq!(scroll_offset(3, rows), 1);
        assert_eq!(scroll_offset(18, rows), rows - VISIBLE_ROWS);
        assert_eq!(scroll_offset(3, 4), 0, "a short page never scrolls");
    }

    #[test]
    fn countdown_shows_whole_seconds() {
        let listen = Listen::Key { port: 0, button: 4, until: 10_000 };
        assert_eq!(listen.seconds_left(5_000), 5);
        assert_eq!(listen.seconds_left(5_001), 5);
        assert_eq!(listen.seconds_left(9_999), 1);
        assert_eq!(listen.seconds_left(12_000), 1);
    }

    #[test]
    fn key_names_read_like_the_buttons() {
        assert_eq!(key_name(96), "A");
        assert_eq!(key_name(19), "D-Pad Up");
        assert_eq!(key_name(188), "Button 1");
        assert_eq!(key_name(203), "Button 16");
        assert_eq!(key_name(1234), "Key 1234");
    }
}

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
    /// Android says it has L3 and R3.
    #[serde(default)]
    pub sticks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Controls {
    /// Fingerprint of the controller on each port; "" = none.
    pub ports: [String; PORTS],
    pub pads: BTreeMap<String, Pad>,
    /// Systems > [system] > Controls, by system ("gba", "nes"...).
    pub systems: BTreeMap<String, Layout>,
    /// The in-game Controls, by game (its ROM's library key).
    pub games: BTreeMap<String, Layout>,
    pub hotkeys: Hotkeys,
    /// The handheld's own controls sent L3 or R3 once: they exist.
    pub builtin_sticks: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            ports: [BUILTIN.to_owned(), String::new()], pads: BTreeMap::new(),
            systems: BTreeMap::new(), games: BTreeMap::new(), hotkeys: Hotkeys::default(),
            builtin_sticks: false,
        }
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
        self.know(fp);
        if self.ports[0].is_empty() { self.ports[0] = BUILTIN.to_owned(); }
        if self.ports[1] == self.ports[0] { self.ports[1].clear(); }
    }

    /// slot. has met this controller: it never takes a port by itself again.
    fn know(&mut self, fp: &str) {
        if fp != BUILTIN && !fp.is_empty() { self.pads.entry(fp.to_owned()).or_default(); }
    }

    /// A controller slot. has never met pressed a button: it takes Port 2 if
    /// nobody has it. Only once: a controller the player then moved or set
    /// aside stays where they put it. True when it took Port 2.
    pub fn claim_free_port(&mut self, fp: &str) -> bool {
        if fp == BUILTIN || fp.is_empty() || self.pads.contains_key(fp)
            || self.port_of(fp).is_some() || self.on_port(1).is_some()
        {
            return false;
        }
        self.ports[1] = fp.to_owned();
        self.know(fp);
        true
    }

    /// The controllers ◀▶ goes through on `port`, in order. `connected` is
    /// every connected controller's fingerprint (built-in included).
    /// The same order whatever is chosen now, so ◀▶ always goes round the
    /// same stops: Port 1 starts with the handheld's own controls, Port 2
    /// with None and ends with them.
    pub fn candidates(&self, port: usize, connected: &[String]) -> Vec<String> {
        let mut list: Vec<String> = Vec::new();
        let mut push = |fp: &str| if !list.iter().any(|f| f == fp) { list.push(fp.to_owned()); };
        if port == 1 { push(""); } else { push(BUILTIN); }
        for fp in connected.iter().filter(|fp| fp.as_str() != BUILTIN) { push(fp); }
        // A controller that is off stays a stop while it holds this port.
        if let Some(fp) = self.on_port(port) { push(fp); }
        push(BUILTIN);
        list
    }

    /// ◀▶ on a port's Controller row. A stop that would leave Port 1 with
    /// nothing is passed over.
    pub fn cycle(&mut self, port: usize, right: bool, connected: &[String]) {
        let list = self.candidates(port, connected);
        let n = list.len();
        let current = self.ports[port].clone();
        let at = list.iter().position(|fp| *fp == current).unwrap_or(0);
        for step in 1..=n {
            let fp = list[if right { (at + step) % n } else { (at + n - step % n) % n }].clone();
            if fp == current { return; }
            if fp.is_empty() { self.ports[port].clear(); return; }
            // Port 2 taking Port 1's controller while Port 2 had none: Port 1
            // takes another connected controller, or this stop is skipped.
            if port == 1 && current.is_empty() && self.port_of(&fp) == Some(0) {
                let other = connected.iter()
                    .find(|c| **c != fp && self.port_of(c).is_none()).cloned();
                let Some(other) = other else { continue };
                self.ports[0] = other.clone();
                self.ports[1] = fp.clone();
                self.know(&other);
                self.know(&fp);
                return;
            }
            self.assign(port, &fp);
            return;
        }
    }

    /// Settings > Controls > Reset to Default: every controller's keys back
    /// to the defaults, and the hotkeys too. Who is on which port stays.
    pub fn reset_all(&mut self) {
        for pad in self.pads.values_mut() { pad.reset(); }
        self.hotkeys = Hotkeys::default();
    }

    /// Remember the name Android gives a controller, for when it's off.
    /// True when something changed.
    pub fn remember(&mut self, device: &Device) -> bool {
        if device.fp == BUILTIN || self.pads.get(&device.fp).is_some_and(|p| p.name == device.name) {
            return false;
        }
        if !self.pads.contains_key(&device.fp) { return false; }
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
    /// A console button of a system's or a game's Port page.
    Console { port: usize, button: usize, until: u64 },
    /// A hotkey: the Hotkey Button (optional) then a key.
    Hotkey { action: Action, until: u64 },
}

impl Listen {
    pub fn until(self) -> u64 {
        match self {
            Listen::Key { until, .. } | Listen::Controller { until, .. }
            | Listen::Console { until, .. } | Listen::Hotkey { until, .. } => until,
        }
    }
    pub fn port(self) -> usize {
        match self {
            Listen::Key { port, .. } | Listen::Controller { port, .. }
            | Listen::Console { port, .. } => port,
            Listen::Hotkey { .. } => 0,
        }
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

// ---------------------------------------------------------------------------
// Systems > [system] > Controls, and the in-game Controls (one game): which
// slot. button plays each of the console's buttons, and what X/Y and L1/R1
// do where the console has no such buttons.

/// The libretro joypad bits (slot_retro::ButtonMask), which are also the
/// bits of slot.'s own buttons.
pub mod bit {
    pub const B: u16 = 1 << 0;
    pub const Y: u16 = 1 << 1;
    pub const SELECT: u16 = 1 << 2;
    pub const START: u16 = 1 << 3;
    pub const UP: u16 = 1 << 4;
    pub const DOWN: u16 = 1 << 5;
    pub const LEFT: u16 = 1 << 6;
    pub const RIGHT: u16 = 1 << 7;
    pub const A: u16 = 1 << 8;
    pub const X: u16 = 1 << 9;
    pub const L: u16 = 1 << 10;
    pub const R: u16 = 1 << 11;
}

/// The name of one of slot.'s buttons, by its bit; "None" for no button.
pub fn slot_button_name(slot: u16) -> &'static str {
    match slot {
        bit::UP => "Up", bit::DOWN => "Down", bit::LEFT => "Left", bit::RIGHT => "Right",
        bit::A => "A", bit::B => "B", bit::X => "X", bit::Y => "Y",
        bit::L => "L1", bit::R => "R1", bit::SELECT => "SELECT", bit::START => "START",
        _ => "None",
    }
}

/// A button of the console. `core` is the joypad bit the core reads it
/// from; slot.'s button with that same bit plays it by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleButton { pub name: &'static str, pub core: u16 }

const fn cb(name: &'static str, core: u16) -> ConsoleButton { ConsoleButton { name, core } }
const DPAD: [ConsoleButton; 4] =
    [cb("Up", bit::UP), cb("Down", bit::DOWN), cb("Left", bit::LEFT), cb("Right", bit::RIGHT)];
const GBA: [ConsoleButton; 10] = [DPAD[0], DPAD[1], DPAD[2], DPAD[3],
    cb("A", bit::A), cb("B", bit::B), cb("L", bit::L), cb("R", bit::R),
    cb("SELECT", bit::SELECT), cb("START", bit::START)];
const TWO_BUTTONS: [ConsoleButton; 8] = [DPAD[0], DPAD[1], DPAD[2], DPAD[3],
    cb("A", bit::A), cb("B", bit::B), cb("SELECT", bit::SELECT), cb("START", bit::START)];
const SNES: [ConsoleButton; 12] = [DPAD[0], DPAD[1], DPAD[2], DPAD[3],
    cb("A", bit::A), cb("B", bit::B), cb("X", bit::X), cb("Y", bit::Y),
    cb("L", bit::L), cb("R", bit::R), cb("SELECT", bit::SELECT), cb("START", bit::START)];
/// Genesis Plus GX and PicoDrive read the Mega Drive pad as RetroArch does:
/// A from Y, B from B, C from A, X from L, Y from X, Z from R, Mode from SELECT.
const MD: [ConsoleButton; 12] = [DPAD[0], DPAD[1], DPAD[2], DPAD[3],
    cb("A", bit::Y), cb("B", bit::B), cb("C", bit::A),
    cb("X", bit::L), cb("Y", bit::X), cb("Z", bit::R),
    cb("Mode", bit::SELECT), cb("START", bit::START)];

/// Systems, by the names slot. files their controls under.
pub const SYSTEMS: [&str; 6] = ["gba", "gbc", "gb", "nes", "snes", "md"];

pub fn console_buttons(system: &str) -> &'static [ConsoleButton] {
    match system {
        "gba" => &GBA,
        "snes" => &SNES,
        "md" => &MD,
        _ => &TWO_BUTTONS,
    }
}

/// Only NES, SNES and Mega Drive take a second player.
pub fn takes_port2(system: &str) -> bool { matches!(system, "nes" | "snes" | "md") }

/// What X and Y do on a console without them (Game Boys, NES).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum XyRole {
    /// Original slot.: X is turbo A, Y turbo B.
    TurboAB,
    /// Y is a second A, X a second B.
    BA,
    Off,
}

impl XyRole {
    pub const ALL: [XyRole; 3] = [XyRole::TurboAB, XyRole::BA, XyRole::Off];
    pub fn label(self) -> &'static str {
        match self { XyRole::TurboAB => "Turbo A / B", XyRole::BA => "B / A", XyRole::Off => "None" }
    }
}

/// What L1 and R1 do on a console without them (Game Boy, Color, NES).
/// The first half is L1's, the second R1's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShoulderRole { ScreenSize, States, TurboAB, AB, Palette, Shader, Off }

impl ShoulderRole {
    pub fn label(self) -> &'static str {
        match self {
            ShoulderRole::ScreenSize => "Screen Size",
            ShoulderRole::States => "Load / Save",
            ShoulderRole::TurboAB => "Turbo A / B",
            ShoulderRole::AB => "A / B",
            ShoulderRole::Palette => "Palette - / +",
            ShoulderRole::Shader => "Shader - / +",
            ShoulderRole::Off => "None",
        }
    }

    /// slot.'s own shortcuts are Port 1's: the second player only gets buttons.
    pub fn choices(system: &str, port: usize) -> &'static [ShoulderRole] {
        use ShoulderRole::*;
        if port > 0 { return &[TurboAB, AB, Off]; }
        if system == "gb" { &[ScreenSize, States, TurboAB, AB, Palette, Shader, Off] }
        else { &[ScreenSize, States, TurboAB, AB, Shader, Off] }
    }
}

/// X / Y and L1 / R1 rows: on which systems they are spare.
pub fn spare_xy(system: &str) -> bool { matches!(system, "gba" | "gb" | "gbc" | "nes") }
pub fn spare_shoulders(system: &str) -> bool { matches!(system, "gb" | "gbc" | "nes") }

/// One port's choices at one level. Only what was changed there is kept.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortLayout {
    /// Console button name -> slot.'s button bit; 0 = none.
    pub binds: BTreeMap<String, u16>,
    pub xy: Option<XyRole>,
    pub shoulders: Option<ShoulderRole>,
}

impl PortLayout {
    fn is_empty(&self) -> bool { self.binds.is_empty() && self.xy.is_none() && self.shoulders.is_none() }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout { pub ports: [PortLayout; PORTS] }

impl Layout {
    fn is_empty(&self) -> bool { self.ports.iter().all(PortLayout::is_empty) }
}

/// Where a Controls page edits: a whole system, or one game of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope { System(&'static str), Game { system: &'static str, game: String } }

impl Scope {
    pub fn system(&self) -> &'static str {
        match self { Scope::System(s) | Scope::Game { system: s, .. } => s }
    }
}

/// A port's controls as a game plays them: game, then system, then defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// (slot.'s button bit, console button's core bit) for every bound button.
    pub pairs: Vec<(u16, u16)>,
    /// None where X and Y are console buttons, or play one.
    pub xy: Option<XyRole>,
    /// None where L1 and R1 are console buttons, or play one.
    pub shoulders: Option<ShoulderRole>,
}

impl Resolved {
    /// The joypad the core reads from slot.'s buttons held on `frame`.
    pub fn apply(&self, slot: u16, frame: u32) -> u16 {
        let mut out = 0;
        for &(from, to) in &self.pairs {
            if slot & from != 0 { out |= to; }
        }
        // Original slot. turbo: on for three frames, off for three.
        let turbo_on = (frame / 3) % 2 == 0;
        let mut second = |held: u16, a: u16, b: u16, turbo: bool| {
            if turbo && !turbo_on { return; }
            if slot & held & a != 0 { out |= bit::A; }
            if slot & held & b != 0 { out |= bit::B; }
        };
        match self.xy {
            Some(XyRole::TurboAB) => second(bit::X | bit::Y, bit::X, bit::Y, true),
            Some(XyRole::BA) => second(bit::X | bit::Y, bit::Y, bit::X, false),
            _ => {}
        }
        match self.shoulders {
            Some(ShoulderRole::TurboAB) => second(bit::L | bit::R, bit::L, bit::R, true),
            Some(ShoulderRole::AB) => second(bit::L | bit::R, bit::L, bit::R, false),
            _ => {}
        }
        out
    }
}

impl Controls {
    fn layout(&self, scope: &Scope) -> Option<&Layout> {
        match scope {
            Scope::System(system) => self.systems.get(*system),
            Scope::Game { game, .. } => self.games.get(game),
        }
    }

    fn layout_mut(&mut self, scope: &Scope) -> &mut Layout {
        match scope {
            Scope::System(system) => self.systems.entry((*system).to_owned()).or_default(),
            Scope::Game { game, .. } => self.games.entry(game.clone()).or_default(),
        }
    }

    fn tidy(&mut self) {
        self.systems.retain(|_, layout| !layout.is_empty());
        self.games.retain(|_, layout| !layout.is_empty());
    }

    /// The levels under `scope`, most precise first: a game's page shows
    /// what the system gives it.
    fn levels(&self, scope: &Scope, port: usize) -> Vec<&PortLayout> {
        let mut levels = Vec::new();
        if let Scope::Game { game, .. } = scope {
            if let Some(layout) = self.games.get(game) { levels.push(&layout.ports[port]); }
        }
        if let Some(layout) = self.systems.get(scope.system()) { levels.push(&layout.ports[port]); }
        levels
    }

    /// The slot. button playing console button `index`, and whether this
    /// very level chose it.
    pub fn bind(&self, scope: &Scope, port: usize, index: usize) -> (u16, bool) {
        let button = console_buttons(scope.system())[index];
        let own = self.layout(scope).and_then(|l| l.ports[port].binds.get(button.name)).copied();
        if let Some(slot) = own { return (slot, true); }
        let below = self.levels(scope, port).into_iter()
            .find_map(|level| level.binds.get(button.name).copied());
        (below.unwrap_or(button.core), false)
    }

    /// Console button `index` <- slot.'s button `slot`. A console button
    /// already played by `slot` takes this one's old button: no slot.
    /// button plays two console buttons.
    pub fn map_console(&mut self, scope: &Scope, port: usize, index: usize, slot: u16) {
        let buttons = console_buttons(scope.system());
        let (old, _) = self.bind(scope, port, index);
        let other = (0..buttons.len())
            .find(|&i| i != index && self.bind(scope, port, i).0 == slot);
        let layout = &mut self.layout_mut(scope).ports[port];
        if let Some(other) = other { layout.binds.insert(buttons[other].name.to_owned(), old); }
        layout.binds.insert(buttons[index].name.to_owned(), slot);
        self.tidy();
    }

    pub fn clear_console(&mut self, scope: &Scope, port: usize, index: usize) {
        let name = console_buttons(scope.system())[index].name;
        self.layout_mut(scope).ports[port].binds.insert(name.to_owned(), 0);
        self.tidy();
    }

    /// X / Y and L1 / R1 at this level: (role, chosen here). `default_xy`
    /// is the old X/Y Buttons setting, which still decides until changed.
    pub fn xy(&self, scope: &Scope, port: usize, default_xy: XyRole) -> (XyRole, bool) {
        if let Some(role) = self.layout(scope).and_then(|l| l.ports[port].xy) { return (role, true); }
        let below = self.levels(scope, port).into_iter().find_map(|level| level.xy);
        (below.unwrap_or(default_xy), false)
    }

    pub fn shoulders(&self, scope: &Scope, port: usize) -> (ShoulderRole, bool) {
        if let Some(role) = self.layout(scope).and_then(|l| l.ports[port].shoulders) { return (role, true); }
        let below = self.levels(scope, port).into_iter().find_map(|level| level.shoulders);
        let default = if port == 0 { ShoulderRole::ScreenSize } else { ShoulderRole::Off };
        let role = below.unwrap_or(default);
        // A role the port can't have (Palette on Port 2): back to the default.
        let fits = ShoulderRole::choices(scope.system(), port).contains(&role);
        (if fits { role } else { default }, false)
    }

    pub fn cycle_xy(&mut self, scope: &Scope, port: usize, right: bool, default_xy: XyRole) {
        let (now, _) = self.xy(scope, port, default_xy);
        let all = XyRole::ALL;
        let at = all.iter().position(|r| *r == now).unwrap_or(0);
        let next = all[if right { (at + 1) % all.len() } else { (at + all.len() - 1) % all.len() }];
        self.layout_mut(scope).ports[port].xy = Some(next);
        self.tidy();
    }

    pub fn cycle_shoulders(&mut self, scope: &Scope, port: usize, right: bool) {
        let (now, _) = self.shoulders(scope, port);
        let all = ShoulderRole::choices(scope.system(), port);
        let at = all.iter().position(|r| *r == now).unwrap_or(0);
        let next = all[if right { (at + 1) % all.len() } else { (at + all.len() - 1) % all.len() }];
        self.layout_mut(scope).ports[port].shoulders = Some(next);
        self.tidy();
    }

    /// A port of this level back to what the level below gives it.
    pub fn reset_layout(&mut self, scope: &Scope, port: usize) {
        self.layout_mut(scope).ports[port] = PortLayout::default();
        self.tidy();
    }

    /// Every level of a game removed (in-game Controls > Reset to Default).
    pub fn reset_game(&mut self, game: &str) { self.games.remove(game); }

    /// What a port plays in a game of `system` (`game`: its key, if any).
    pub fn resolve(&self, system: &'static str, game: Option<&str>, port: usize,
                   default_xy: XyRole) -> Resolved {
        let scope = match game {
            Some(game) => Scope::Game { system, game: game.to_owned() },
            None => Scope::System(system),
        };
        let buttons = console_buttons(system);
        let pairs: Vec<(u16, u16)> = (0..buttons.len())
            .map(|i| (self.bind(&scope, port, i).0, buttons[i].core))
            .filter(|&(slot, _)| slot != 0)
            .collect();
        let used = |b: u16| pairs.iter().any(|&(slot, _)| slot & b != 0);
        let xy = (spare_xy(system) && !used(bit::X | bit::Y))
            .then(|| self.xy(&scope, port, default_xy).0);
        let shoulders = (spare_shoulders(system) && !used(bit::L | bit::R))
            .then(|| self.shoulders(&scope, port).0);
        Resolved { pairs, xy, shoulders }
    }
}

/// A row of a system's (or a game's) Port page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutRow { Button(usize), Xy, Shoulders, Reset }

pub fn layout_rows(system: &str) -> Vec<LayoutRow> {
    let mut rows: Vec<LayoutRow> = (0..console_buttons(system).len()).map(LayoutRow::Button).collect();
    if spare_xy(system) { rows.push(LayoutRow::Xy); }
    if spare_shoulders(system) { rows.push(LayoutRow::Shoulders); }
    rows.push(LayoutRow::Reset);
    rows
}

// ---------------------------------------------------------------------------
// Settings > Controls > Hotkeys: slot.'s shortcuts, Port 1's only. A hotkey
// is the Hotkey Button + a key, or a key alone. Global, never per game.

/// What a hotkey does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Action {
    GameMenu, Screen, SaveEject, Multiplayer, SaveState, LoadState, FastForward, Rewind,
    ShaderPrev, ShaderNext, Color, PalettePrev, PaletteNext,
    Bigger, Smaller, RunAheadDown, RunAheadUp, SunDown, SunUp,
}

impl Action {
    pub const ALL: [Action; 19] = [
        Action::GameMenu, Action::Screen, Action::SaveEject, Action::Multiplayer, Action::SaveState,
        Action::LoadState, Action::FastForward, Action::Rewind, Action::ShaderPrev,
        Action::ShaderNext, Action::Color, Action::PalettePrev, Action::PaletteNext,
        Action::Bigger, Action::Smaller, Action::RunAheadDown, Action::RunAheadUp,
        Action::SunDown, Action::SunUp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::GameMenu => "Game Menu", Action::Screen => "Screen",
            Action::SaveEject => "Save and Eject",
            Action::Multiplayer => "Multiplayer", Action::SaveState => "Save State",
            Action::LoadState => "Load State", Action::FastForward => "Fast Forward",
            Action::Rewind => "Rewind", Action::ShaderPrev => "Previous Shader",
            Action::ShaderNext => "Next Shader", Action::Color => "Color Correction",
            Action::PalettePrev => "Previous Palette", Action::PaletteNext => "Next Palette",
            Action::Bigger => "Bigger Screen", Action::Smaller => "Smaller Screen",
            Action::RunAheadDown => "Run-Ahead -", Action::RunAheadUp => "Run-Ahead +",
            Action::SunDown => "Less Sun", Action::SunUp => "More Sun",
        }
    }

    /// slot.'s shortcuts until now, which stay the defaults; the Screen page
    /// took Hotkey + Y from Color Correction, which that page shows live
    /// (Siham 2026-11-06), so Color Correction has none until given one.
    pub fn default_combo(self) -> Option<Combo> {
        let with = |key| Combo { modifier: true, key };
        let alone = |key| Combo { modifier: false, key };
        Some(match self {
            Action::Color => return None,
            Action::GameMenu => with(99), Action::Screen => with(100),
            Action::SaveEject => with(108),
            Action::Multiplayer => with(MODE), Action::SaveState => with(103),
            Action::LoadState => with(102), Action::FastForward => alone(105),
            Action::Rewind => alone(104), Action::ShaderPrev => with(97),
            Action::ShaderNext => with(96),
            Action::PalettePrev => with(104), Action::PaletteNext => with(105),
            Action::Bigger => with(19), Action::Smaller => with(20),
            Action::RunAheadDown => with(21), Action::RunAheadUp => with(22),
            Action::SunDown => alone(106), Action::SunUp => alone(107),
        })
    }

    /// Held down rather than pressed once: its release matters too.
    pub fn held(self) -> bool { matches!(self, Action::FastForward | Action::Rewind) }

    /// Fast Forward (R2), Rewind (L2) and the sun keep their keys.
    pub fn fixed(self) -> bool {
        matches!(self, Action::FastForward | Action::Rewind | Action::SunDown | Action::SunUp)
    }
}

/// Keys no game uses (L2, R2, L3, R3): the only ones a hotkey may have alone.
pub fn spare_key(key: i32) -> bool { matches!(key, 104..=107) }

/// The Hotkey Button (or none) + one key, as slot. reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Combo { pub modifier: bool, pub key: i32 }

/// The Hotkey Button choices: SELECT, then L3 and R3 where the handheld has them.
pub const HOTKEY_BUTTONS: [i32; 3] = [109, 106, 107];

fn yes() -> bool { true }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    /// The Hotkey Button's key code (see hotkey_button()).
    pub button: i32,
    /// Changed hotkeys only; None = no hotkey for that action.
    pub binds: BTreeMap<Action, Option<Combo>>,
    /// The handheld has L3 and R3 (not saved: the runtime tells).
    #[serde(skip, default = "yes")]
    pub sticks: bool,
}

impl Default for Hotkeys {
    fn default() -> Self { Self { button: 109, binds: BTreeMap::new(), sticks: true } }
}

/// A key's name in a hotkey: slot.'s own button names.
pub fn hotkey_key_name(key: i32) -> &'static str {
    match key {
        MODE => "slot.",
        19 => "Up", 20 => "Down", 21 => "Left", 22 => "Right",
        _ => BUTTONS.iter().find(|(_, code)| *code == key).map_or("?", |(name, _)| name),
    }
}

impl Hotkeys {
    /// Nothing changed: slot.'s shortcuts run exactly as they always have.
    pub fn is_default(&self) -> bool { self.hotkey_button() == 109 && self.binds.is_empty() }

    /// The Hotkey Button in use: SELECT unless L3 or R3 was chosen and exists.
    pub fn hotkey_button(&self) -> i32 {
        if self.sticks && HOTKEY_BUTTONS.contains(&self.button) { self.button } else { 109 }
    }

    /// L3 and R3 set the sun alone (unless one is the Hotkey Button).
    fn sun_alone(&self) -> bool {
        self.sticks && !matches!(self.hotkey_button(), 106 | 107)
    }

    /// Every way a fixed action is pressed. The sun is always also the
    /// Hotkey Button + L2 / R2, which only Boktai reads that way.
    fn fixed_combos(&self, action: Action) -> Vec<Combo> {
        let with = |key| Combo { modifier: true, key };
        let alone = |key| Combo { modifier: false, key };
        match action {
            Action::FastForward => vec![alone(105)],
            Action::Rewind => vec![alone(104)],
            Action::SunDown if self.sun_alone() => vec![alone(106), with(104)],
            Action::SunUp if self.sun_alone() => vec![alone(107), with(105)],
            Action::SunDown => vec![with(104)],
            Action::SunUp => vec![with(105)],
            _ => Vec::new(),
        }
    }

    /// The hotkey of an action that can be changed, if it has one now.
    pub fn combo(&self, action: Action) -> Option<Combo> {
        if action.fixed() { return self.fixed_combos(action).first().copied(); }
        let combo = self.binds.get(&action).copied().unwrap_or(action.default_combo())?;
        let usable = combo.key != self.hotkey_button()
            && (combo.modifier || (spare_key(combo.key) && self.fixed_alone(combo.key).is_none()));
        usable.then_some(combo)
    }

    /// The fixed action a key alone belongs to (R2, L2, and L3 / R3).
    fn fixed_alone(&self, key: i32) -> Option<Action> {
        [Action::FastForward, Action::Rewind, Action::SunDown, Action::SunUp].into_iter()
            .find(|a| self.fixed_combos(*a).contains(&Combo { modifier: false, key }))
    }

    /// The action a key starts. In Boktai the sun comes first; elsewhere a
    /// hotkey that shares the sun's Hotkey Button + L2 / R2 wins.
    pub fn action_for(&self, modifier_held: bool, key: i32, boktai: bool) -> Option<Action> {
        let pressed = Combo { modifier: modifier_held, key };
        let fixed = |a: &Action| a.fixed() && self.fixed_combos(*a).contains(&pressed);
        let sun = |a: &Action| matches!(a, Action::SunDown | Action::SunUp);
        let all = Action::ALL;
        all.iter().find(|a| boktai && sun(a) && fixed(a))
            .or_else(|| all.iter().find(|a| !sun(a) && fixed(a)))
            .or_else(|| all.iter().find(|a| !a.fixed() && self.combo(**a) == Some(pressed)))
            .or_else(|| all.iter().find(|a| fixed(a)))
            .copied()
    }

    /// The hotkey for a key pressed while mapping. Game buttons always go
    /// with the Hotkey Button; L2, R2, L3 and R3 only if it was held.
    pub fn combo_for_key(&self, modifier_held: bool, key: i32) -> Combo {
        Combo { modifier: modifier_held || !spare_key(key), key }
    }

    /// `action` <- `combo`. Err: a fixed action owns that key alone.
    /// Ok(Some): another action lost it.
    pub fn set(&mut self, action: Action, combo: Combo) -> Result<Option<Action>, Action> {
        if action.fixed() { return Err(action); }
        if !combo.modifier {
            if let Some(owner) = self.fixed_alone(combo.key) { return Err(owner); }
        }
        let other = Action::ALL.into_iter()
            .find(|a| *a != action && !a.fixed() && self.combo(*a) == Some(combo));
        if let Some(other) = other { self.put(other, None); }
        self.put(action, Some(combo));
        Ok(other)
    }

    pub fn clear(&mut self, action: Action) {
        if !action.fixed() { self.put(action, None); }
    }

    fn put(&mut self, action: Action, combo: Option<Combo>) {
        if combo == action.default_combo() { self.binds.remove(&action); }
        else { self.binds.insert(action, combo); }
    }

    pub fn cycle_button(&mut self, right: bool) {
        let all: &[i32] = if self.sticks { &HOTKEY_BUTTONS } else { &HOTKEY_BUTTONS[..1] };
        let at = all.iter().position(|b| *b == self.hotkey_button()).unwrap_or(0);
        self.button = all[if right { (at + 1) % all.len() } else { (at + all.len() - 1) % all.len() }];
    }

    pub fn button_name(&self) -> &'static str { hotkey_key_name(self.hotkey_button()) }

    fn combo_text(&self, combo: Combo) -> String {
        let key = hotkey_key_name(combo.key);
        if combo.modifier { format!("{} + {key}", self.button_name()) } else { key.to_owned() }
    }

    /// "SELECT + R1", "R2 (hold)", "L3 or SELECT + L2", "None".
    pub fn label(&self, action: Action) -> String {
        if action.fixed() {
            let combos: Vec<String> = self.fixed_combos(action).into_iter()
                .map(|c| self.combo_text(c)).collect();
            let text = combos.join(" or ");
            return if action.held() { format!("{text} (hold)") } else { text };
        }
        self.combo(action).map_or_else(|| "None".into(), |c| self.combo_text(c))
    }
}

/// Rows of the Hotkeys page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyRow { Button, Action(Action), Reset }

pub fn hotkey_rows() -> Vec<HotkeyRow> {
    let mut rows = vec![HotkeyRow::Button];
    rows.extend(Action::ALL.map(HotkeyRow::Action));
    rows.push(HotkeyRow::Reset);
    rows
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
        // Every other controller stays known, even with default keys: a
        // known controller never takes Port 2 by itself again.
        if self.pads.get(BUILTIN).is_some_and(Pad::is_default) { self.pads.remove(BUILTIN); }
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
    fn cycling_port_two_goes_round_the_same_stops_both_ways() {
        let mut controls = Controls::default();
        let connected = vec![BUILTIN.to_owned(), PAD.to_owned(), "pro".to_owned()];
        assert_eq!(controls.candidates(1, &connected),
            vec!["".to_owned(), PAD.into(), "pro".into(), BUILTIN.into()]);
        controls.cycle(1, true, &connected);
        assert_eq!(controls.on_port(1), Some(PAD));
        controls.cycle(1, true, &connected);
        assert_eq!(controls.on_port(1), Some("pro"));
        controls.cycle(1, true, &connected);
        assert_eq!(controls.on_port(1), Some(BUILTIN), "taken from Port 1 by swapping");
        assert_eq!(controls.on_port(0), Some("pro"));
        controls.cycle(1, true, &connected);
        assert_eq!(controls.on_port(1), None, "then None, never a short loop");
        assert_eq!(controls.candidates(0, &connected)[0], BUILTIN, "Port 1 lists the handheld first");
    }

    #[test]
    fn port_two_takes_the_handhelds_controls_only_if_port_one_has_another() {
        let mut alone = Controls::default();
        alone.cycle(1, false, &[BUILTIN.into()]);
        assert_eq!(alone.on_port(0), Some(BUILTIN));
        assert_eq!(alone.on_port(1), None, "nothing else could take Port 1");
        let mut two = Controls::default();
        two.cycle(1, false, &[BUILTIN.into(), PAD.into()]);
        assert_eq!(two.on_port(1), Some(BUILTIN));
        assert_eq!(two.on_port(0), Some(PAD));
    }

    #[test]
    fn a_controller_that_is_off_stays_on_its_port() {
        let mut controls = Controls::default();
        controls.assign(1, PAD);
        assert!(controls.candidates(1, &[BUILTIN.into()]).contains(&PAD.to_owned()));
        controls.cycle(1, false, &[BUILTIN.into()]);
        assert_eq!(controls.on_port(1), None);
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
    fn a_controller_set_aside_never_takes_port_two_again() {
        let mut controls = Controls::default();
        assert!(controls.claim_free_port(PAD));
        controls.cycle(1, false, &[BUILTIN.into(), PAD.into()]);
        assert_eq!(controls.on_port(1), None, "the player chose None");
        assert!(!controls.claim_free_port(PAD), "its next button doesn't undo that");
        // Nor after slot. restarts.
        let mut reloaded = controls.clone().sanitized();
        assert!(!reloaded.claim_free_port(PAD));
        // A controller only ever picked by hand is known too.
        let mut picked = Controls::default();
        picked.assign(1, "pro");
        picked.cycle(1, false, &[BUILTIN.into(), "pro".into()]);
        assert!(!picked.claim_free_port("pro"));
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
        let device = Device { id: 9, name: "8BitDo SN30 Pro".into(), fp: PAD.into(), sticks: true };
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

    // ---- Systems > Controls and the in-game Controls ----

    const NES: Scope = Scope::System("nes");
    fn game() -> Scope { Scope::Game { system: "nes", game: "mario".into() } }

    #[test]
    fn untouched_systems_play_exactly_as_before() {
        let controls = Controls::default();
        for system in SYSTEMS {
            for turbo in [true, false] {
                let default_xy = if turbo { XyRole::TurboAB } else { XyRole::BA };
                let layout = controls.resolve(system, None, 0, default_xy);
                for frame in 0..12u32 {
                    for slot in 0..(1u16 << 12) {
                        assert_eq!(layout.apply(slot, frame), old_pad(system, slot, turbo, frame),
                            "{system} {slot:#x} turbo={turbo} frame={frame}");
                    }
                }
            }
        }
    }

    /// game.rs's pad_buttons before Controls, for the comparison above.
    fn old_pad(system: &str, slot: u16, turbo: bool, frame: u32) -> u16 {
        if matches!(system, "snes" | "md") { return slot; }
        let mut mapped = slot & !(bit::X | bit::Y);
        if turbo {
            if (frame / 3) % 2 == 0 {
                if slot & bit::X != 0 { mapped |= bit::A; }
                if slot & bit::Y != 0 { mapped |= bit::B; }
            }
        } else {
            if slot & bit::Y != 0 { mapped |= bit::A; }
            if slot & bit::X != 0 { mapped |= bit::B; }
        }
        // L1/R1 resized the screen and never reached Game Boy or NES cores.
        if system != "gba" { mapped &= !(bit::L | bit::R); }
        mapped
    }

    #[test]
    fn mapping_a_console_button_swaps_and_shows_where_it_was_chosen() {
        let mut controls = Controls::default();
        let a = 4; // NES A
        controls.map_console(&NES, 0, a, bit::B);
        assert_eq!(controls.bind(&NES, 0, a), (bit::B, true));
        assert_eq!(controls.bind(&NES, 0, 5), (bit::A, true), "B took A's old button");
        let layout = controls.resolve("nes", None, 0, XyRole::TurboAB);
        assert_eq!(layout.apply(bit::B, 0), bit::A);
        // A game sees the system's choice, not as its own.
        assert_eq!(controls.bind(&game(), 0, a), (bit::B, false));
        controls.map_console(&game(), 0, a, bit::A);
        assert_eq!(controls.bind(&game(), 0, a), (bit::A, true));
        assert_eq!(controls.resolve("nes", Some("mario"), 0, XyRole::TurboAB).apply(bit::A, 0), bit::A);
        controls.reset_game("mario");
        assert_eq!(controls.bind(&game(), 0, a), (bit::B, false));
        controls.reset_layout(&NES, 0);
        assert!(controls.systems.is_empty(), "nothing left to keep");
    }

    #[test]
    fn x_y_roles_cycle_and_follow_the_old_setting_until_changed() {
        let mut controls = Controls::default();
        assert_eq!(controls.xy(&NES, 0, XyRole::BA), (XyRole::BA, false));
        controls.cycle_xy(&NES, 0, true, XyRole::BA);
        assert_eq!(controls.xy(&NES, 0, XyRole::BA), (XyRole::Off, true));
        let layout = controls.resolve("nes", None, 0, XyRole::BA);
        assert_eq!(layout.apply(bit::X | bit::Y, 0), 0, "X and Y do nothing now");
    }

    #[test]
    fn shoulder_roles_are_port_one_shortcuts_or_buttons() {
        let mut controls = Controls::default();
        assert_eq!(controls.shoulders(&NES, 0), (ShoulderRole::ScreenSize, false));
        assert_eq!(controls.shoulders(&NES, 1), (ShoulderRole::Off, false));
        assert!(!ShoulderRole::choices("nes", 1).contains(&ShoulderRole::States));
        assert!(ShoulderRole::choices("gb", 0).contains(&ShoulderRole::Palette));
        assert!(!ShoulderRole::choices("nes", 0).contains(&ShoulderRole::Palette));
        controls.cycle_shoulders(&NES, 0, true);
        controls.cycle_shoulders(&NES, 0, true);
        assert_eq!(controls.shoulders(&NES, 0).0, ShoulderRole::TurboAB);
        let layout = controls.resolve("nes", None, 0, XyRole::TurboAB);
        assert_eq!(layout.apply(bit::L, 0), bit::A);
        assert_eq!(layout.apply(bit::R, 3), 0, "turbo is off on frames 3-5");
        // L1 playing a console button: the role steps aside.
        controls.map_console(&NES, 0, 4, bit::L);
        assert_eq!(controls.resolve("nes", None, 0, XyRole::TurboAB).shoulders, None);
    }

    #[test]
    fn mega_drive_buttons_have_their_own_names() {
        let names: Vec<&str> = console_buttons("md").iter().map(|b| b.name).collect();
        assert_eq!(&names[4..], &["A", "B", "C", "X", "Y", "Z", "Mode", "START"]);
        assert!(layout_rows("md").iter().all(|r| !matches!(r, LayoutRow::Xy | LayoutRow::Shoulders)));
        assert!(layout_rows("gba").contains(&LayoutRow::Xy));
        assert!(!layout_rows("gba").contains(&LayoutRow::Shoulders));
        assert!(takes_port2("nes") && !takes_port2("gbc"));
    }

    // ---- Hotkeys ----

    #[test]
    fn default_hotkeys_are_slots_shortcuts() {
        let hk = Hotkeys::default();
        assert!(hk.is_default());
        assert_eq!(hk.action_for(true, 103, false), Some(Action::SaveState));
        assert_eq!(hk.action_for(false, 105, false), Some(Action::FastForward));
        assert_eq!(hk.action_for(true, MODE, false), Some(Action::Multiplayer));
        assert_eq!(hk.action_for(false, 96, false), None, "A alone is the game's");
        assert_eq!(hk.label(Action::SaveState), "SELECT + R1");
        assert_eq!(hk.label(Action::FastForward), "R2 (hold)");
        assert_eq!(hk.label(Action::Multiplayer), "SELECT + slot.");
        assert_eq!(hk.label(Action::SunDown), "L3 or SELECT + L2");
    }

    #[test]
    fn select_l2_is_the_palette_but_the_sun_in_boktai() {
        let hk = Hotkeys::default();
        assert_eq!(hk.action_for(true, 104, false), Some(Action::PalettePrev));
        assert_eq!(hk.action_for(true, 104, true), Some(Action::SunDown));
        assert_eq!(hk.action_for(false, 106, false), Some(Action::SunDown));
    }

    #[test]
    fn game_buttons_always_go_with_the_hotkey_button() {
        let hk = Hotkeys::default();
        assert_eq!(hk.combo_for_key(false, 100), Combo { modifier: true, key: 100 });
        assert_eq!(hk.combo_for_key(false, 105), Combo { modifier: false, key: 105 });
        assert_eq!(hk.combo_for_key(true, 105), Combo { modifier: true, key: 105 });
    }

    #[test]
    fn a_taken_hotkey_moves_and_fixed_keys_stay_fixed() {
        let mut hk = Hotkeys::default();
        let lost = hk.set(Action::LoadState, Combo { modifier: true, key: 103 });
        assert_eq!(lost, Ok(Some(Action::SaveState)));
        assert_eq!(hk.label(Action::SaveState), "None");
        assert_eq!(hk.action_for(true, 103, false), Some(Action::LoadState));
        assert_eq!(hk.action_for(true, 102, false), None, "SELECT + L1 is free now");
        assert_eq!(hk.set(Action::SaveState, Combo { modifier: false, key: 105 }),
            Err(Action::FastForward), "R2 alone is Fast Forward's");
        assert_eq!(hk.set(Action::SaveState, Combo { modifier: false, key: 107 }),
            Err(Action::SunUp));
        assert_eq!(hk.set(Action::FastForward, Combo { modifier: true, key: 99 }),
            Err(Action::FastForward), "Fast Forward can't change");
        assert!(!hk.is_default());
    }

    #[test]
    fn without_l3_r3_select_is_the_only_hotkey_button_and_the_sun_moves() {
        let mut hk = Hotkeys { sticks: false, ..Hotkeys::default() };
        hk.cycle_button(true);
        assert_eq!(hk.hotkey_button(), 109);
        assert_eq!(hk.label(Action::SunDown), "SELECT + L2");
        assert_eq!(hk.action_for(false, 106, true), None, "no L3 here");
        // A file made where L3 existed falls back to SELECT.
        let moved = Hotkeys { button: 106, sticks: false, ..Hotkeys::default() };
        assert_eq!(moved.hotkey_button(), 109);
    }

    #[test]
    fn the_hotkey_button_changes_every_combo_and_never_is_a_hotkey_itself() {
        let mut hk = Hotkeys::default();
        hk.cycle_button(true);
        assert_eq!(hk.hotkey_button(), 106);
        assert_eq!(hk.label(Action::SaveState), "L3 + R1");
        assert_eq!(hk.label(Action::SunDown), "L3 + L2", "L3 is the Hotkey Button now");
        assert_eq!(hk.action_for(true, 103, false), Some(Action::SaveState));
        hk.cycle_button(true);
        hk.cycle_button(true);
        assert!(hk.is_default(), "SELECT, L3, R3, then SELECT again");
    }

    #[test]
    fn hotkey_y_opens_the_screen_page_and_color_correction_has_none() {
        let mut hk = Hotkeys::default();
        assert_eq!(hk.action_for(true, 100, false), Some(Action::Screen));
        assert_eq!(hk.label(Action::Screen), "SELECT + Y");
        assert_eq!(hk.label(Action::Color), "None");
        assert!(hk.is_default());
        assert_eq!(hk.set(Action::Color, Combo { modifier: true, key: 100 }), Ok(Some(Action::Screen)));
        assert_eq!(hk.action_for(true, 100, false), Some(Action::Color));
        assert_eq!(hk.label(Action::Screen), "None");
    }

    #[test]
    fn hotkeys_page_lists_every_action() {
        assert_eq!(hotkey_rows().len(), Action::ALL.len() + 2);
        let mut controls = Controls::default();
        controls.hotkeys.clear(Action::ShaderNext);
        controls.reset_all();
        assert!(controls.hotkeys.is_default(), "Reset to Default resets the hotkeys too");
    }
}

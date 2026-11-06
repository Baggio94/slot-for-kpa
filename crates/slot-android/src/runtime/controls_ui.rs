//! Settings > Controls on the engine: controller keys routed to Port 1, to
//! the second player or to the menus, and the Controls pages themselves.
use super::*;
use crate::controls::{self, Device, LayoutRow, Listen, PortRow, Scope, Sink, XyRole, BUILTIN, MODE, PORTS};

/// A Held key for a stick direction, apart from the same device's D-pad.
const STICK_KEY: i32 = 1 << 20;

/// One prepared row of a Controls page.
pub(super) struct ControlsLine {
    label: String,
    value: String,
    /// ◀▶ changes it: carets around the selected value.
    cycles: bool,
    /// Bright: chosen on this very page. Dim: what the level below gives.
    lit: bool,
}

/// What a Controls page shows this frame; its faces are in `controls_faces`.
#[derive(Default)]
pub(super) struct ControlsView {
    heading: String,
    lines: Vec<ControlsLine>,
    selected: usize,
    offset: usize,
    hints: Vec<(slot_gfx::TexId, u32)>,
}

impl Engine {
    fn controls_now(&self) -> u64 { self.born.elapsed().as_millis() as u64 }

    pub(super) fn persist_controls(&mut self) {
        self.controls_rev = self.controls_rev.wrapping_add(1);
        if let Some((root, _)) = PATHS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            if let Err(error) = self.controls.save(root) {
                set_error_message("Couldn't save controls.", &error);
            }
        }
    }

    fn device_fp(&self, device: i32) -> String {
        self.pad_devices.iter().find(|d| d.id == device)
            .map_or_else(|| BUILTIN.to_owned(), |d| d.fp.clone())
    }

    fn connected_fps(&self) -> Vec<String> {
        let mut fps: Vec<String> = Vec::new();
        for device in &self.pad_devices {
            if !fps.contains(&device.fp) { fps.push(device.fp.clone()); }
        }
        fps
    }

    fn pad_connected(&self, fp: &str) -> bool {
        fp == BUILTIN || self.pad_devices.iter().any(|d| d.fp == fp)
    }

    fn builtin_name() -> &'static str {
        if is_konkr() { "KONKR Controls" } else { "Built-in Controls" }
    }

    /// A controller's name as the Controls pages show it.
    fn pad_name(&self, fp: Option<&str>) -> String {
        let Some(fp) = fp else { return "None".into() };
        if fp == BUILTIN { return Self::builtin_name().into(); }
        let name = self.pad_devices.iter().find(|d| d.fp == fp).map(|d| d.name.clone())
            .or_else(|| Some(self.controls.pad(fp).name).filter(|n| !n.is_empty()))
            .unwrap_or_else(|| "Controller".into());
        let mut short: String = name.chars().take(22).collect();
        if !self.pad_connected(fp) { short.push_str(" (off)"); }
        short
    }

    fn controls_moment(&self) -> controls::Moment {
        let playing = self.game.is_some() && matches!(self.overlay, ShelfOverlay::None);
        controls::Moment {
            playing,
            second_player: playing && self.game.as_ref().is_some_and(GameSession::takes_port2),
        }
    }

    /// The name a system's controls are filed under.
    pub(super) fn system_key(platform: Platform) -> &'static str {
        match platform {
            Platform::Gba => "gba", Platform::Gbc => "gbc", Platform::Gb => "gb",
            Platform::Nes => "nes", Platform::Snes => "snes", Platform::Md => "md",
        }
    }

    /// The game being played, as its Controls are filed: its library URI.
    fn game_key(&self) -> Option<String> {
        self.game.as_ref().and(self.requested_uri.clone())
    }

    fn layout_scope(&self, platform: Platform, game: bool) -> Scope {
        let system = Self::system_key(platform);
        match self.game_key().filter(|_| game) {
            Some(game) => Scope::Game { system, game },
            None => Scope::System(system),
        }
    }

    /// The old X/Y Buttons setting: X/Y's role until a page chooses one.
    /// A game's page starts from the game's own X/Y Buttons, if it had one.
    fn default_xy(&self, game: bool) -> XyRole {
        let turbo = if game { self.live_game_options().turbo } else { self.settings.turbo };
        if turbo { XyRole::TurboAB } else { XyRole::BA }
    }

    /// Give the game being played its ports' Controls when they changed.
    pub(super) fn apply_layouts(&mut self, turbo: bool) {
        let Some(game) = self.game.as_ref() else { return };
        let wanted = (self.controls_rev, turbo);
        if game.has_layouts() && self.layouts_for == Some(wanted) { return; }
        let system = Self::system_key(self.current_cart_platform());
        let key = self.game_key();
        let default_xy = if turbo { XyRole::TurboAB } else { XyRole::BA };
        let layouts = [0, 1].map(|port| self.controls.resolve(system, key.as_deref(), port, default_xy));
        if let Some(game) = self.game.as_mut() { game.set_layouts(layouts); }
        self.layouts_for = Some(wanted);
    }

    /// A key from one of Android's input devices.
    pub(super) fn pad_key(&mut self, device: i32, code: i32, pressed: bool, stick: bool) {
        let held = if stick { code | STICK_KEY } else { code };
        if !self.game.as_ref().is_some_and(GameSession::port2_is_remote) {
            self.port2_remote_noticed = false;
        }
        if !pressed {
            match self.pad_held.release(device, held) {
                Some(Sink::Slot(slot)) => self.handle(Input::Key(slot, false)),
                Some(Sink::Port2(slot)) => self.release_port2(slot),
                Some(Sink::Nowhere) | None => {}
            }
            return;
        }
        if self.pad_held.holds(device, held) { return; }
        let fp = self.device_fp(device);
        if self.controls_listen.is_some() {
            self.pad_held.press(device, held, Sink::Nowhere);
            if !stick { self.listened(&fp, code); }
            return;
        }
        // A handheld whose own controls Android doesn't report as built-in
        // (or none at all): the first controller pressed plays on Port 1.
        if fp == BUILTIN { self.builtin_seen = true; }
        let builtin_here = self.builtin_seen || self.pad_devices.iter().any(|d| d.fp == BUILTIN);
        if !builtin_here && fp != BUILTIN && self.controls.on_port(0) == Some(BUILTIN)
            && self.controls.port_of(&fp).is_none()
        {
            self.controls.assign(0, &fp);
            if let Some(found) = self.pad_devices.iter().find(|d| d.fp == fp).cloned() {
                self.controls.remember(&found);
            }
            self.persist_controls();
        } else if self.controls.claim_free_port(&fp) {
            if let Some(found) = self.pad_devices.iter().find(|d| d.fp == fp).cloned() {
                self.controls.remember(&found);
            }
            self.persist_controls();
            set_message(format!("Controller 2: {}", self.pad_name(Some(fp.as_str()))));
        }
        let port = self.controls.port_of(&fp);
        let Some(slot) = self.controls.slot_code(&fp, code, stick) else {
            self.pad_held.press(device, held, Sink::Nowhere);
            return;
        };
        let sink = controls::sink(port, slot, self.controls_moment());
        self.pad_held.press(device, held, sink);
        match sink {
            Sink::Slot(slot) => self.handle(Input::Key(slot, true)),
            Sink::Port2(slot) => self.press_port2(slot),
            Sink::Nowhere => {
                if port == Some(1) && self.controls_moment().playing
                    && self.game.as_ref().is_some_and(GameSession::port2_is_remote)
                    && !self.port2_remote_noticed
                {
                    self.port2_remote_noticed = true;
                    set_message("Controller 2 is off during Multiplayer.".into());
                }
            }
        }
    }

    /// The button pressed while a Map or Detect waited for one.
    fn listened(&mut self, fp: &str, code: i32) {
        let Some(listen) = self.controls_listen.take() else { return };
        // The slot. button only cancels: it can't be mapped.
        if code == MODE { return; }
        match listen {
            Listen::Controller { port, .. } => {
                self.controls.assign(port, fp);
                if let Some(found) = self.pad_devices.iter().find(|d| d.fp == fp).cloned() {
                    self.controls.remember(&found);
                }
                self.persist_controls();
            }
            Listen::Key { port, button, .. } => {
                if self.controls.on_port(port) != Some(fp) {
                    // Another controller: keep waiting for this port's.
                    self.controls_listen = Some(listen);
                    return;
                }
                self.controls.pad_mut(fp).map(button, code);
                self.persist_controls();
            }
            Listen::Console { port, button, .. } => {
                if self.controls.on_port(port) != Some(fp) {
                    self.controls_listen = Some(listen);
                    return;
                }
                let ShelfOverlay::LayoutPort { platform, game, .. } = self.overlay else { return };
                // The key as slot. reads it (Settings > Controls), then its
                // joypad bit: L2, R2, L3 and R3 are slot.'s, not the game's.
                let slot = self.controls.slot_code(fp, code, false).map_or(0, controls_mask);
                if slot == 0 {
                    set_message("This button can't play in games.".into());
                    return;
                }
                let scope = self.layout_scope(platform, game);
                self.controls.map_console(&scope, port, button, slot);
                self.persist_controls();
            }
        }
    }

    fn press_port2(&mut self, slot: i32) {
        let mask = controls_mask(slot);
        self.buttons2 |= mask;
        self.unseen2 |= mask;
        self.held_releases2 &= !mask;
    }

    fn release_port2(&mut self, slot: i32) {
        let mask = controls_mask(slot);
        if self.unseen2 & mask != 0 { self.held_releases2 |= mask; }
        else { self.buttons2 &= !mask; }
    }

    /// After a real frame read Port 2: apply its releases.
    pub(super) fn port2_frame_done(&mut self) {
        self.unseen2 = 0;
        self.buttons2 &= !self.held_releases2;
        self.held_releases2 = 0;
    }

    /// Android's controller list changed: a controller came or went.
    pub(super) fn controllers_changed(&mut self, devices: Vec<Device>) {
        let gone: Vec<Device> = self.pad_devices.iter()
            .filter(|old| !devices.iter().any(|d| d.id == old.id)).cloned().collect();
        let mut remembered = false;
        for device in &devices { remembered |= self.controls.remember(device); }
        if remembered { self.persist_controls(); }
        let moment = self.controls_moment();
        self.pad_devices = devices;
        for device in gone {
            for sink in self.pad_held.drop_device(device.id) {
                match sink {
                    Sink::Slot(slot) => self.handle(Input::Key(slot, false)),
                    Sink::Port2(slot) => self.release_port2(slot),
                    Sink::Nowhere => {}
                }
            }
            if self.pad_connected(&device.fp) { continue; }
            let Some(port) = self.controls.port_of(&device.fp) else { continue };
            if !moment.playing || (port == 1 && !moment.second_player) { continue; }
            // Pause, as the slot. button does: nobody plays on without a controller.
            self.buttons = 0;
            self.buttons2 = 0;
            self.game_accum = 0.0;
            AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
            self.reset_time_controls();
            RUMBLE_STRENGTH.store(0, Ordering::Release);
            self.overlay = ShelfOverlay::GameMenu { row: 0 };
            set_message(format!("Controller {} disconnected.", port + 1));
        }
    }

    /// Keys of Settings > Controls and its Port pages.
    pub(super) fn controls_menu_key(&mut self, code: i32) {
        // A Map or Detect takes the next button itself (pad_key).
        if self.controls_listen.is_some() { return; }
        let now = self.controls_now();
        match self.overlay {
            ShelfOverlay::ControlsSettings { row } => match code {
                19 | 20 => self.overlay = ShelfOverlay::ControlsSettings {
                    row: move_menu_row(row, code, 3),
                },
                96 if row < PORTS => self.overlay = ShelfOverlay::ControlsPort { port: row, row: 0 },
                96 => {
                    self.controls.reset_all();
                    self.persist_controls();
                    set_message("Controls reset to default.".into());
                }
                97 => self.overlay = ShelfOverlay::Settings { row: 2 },
                108 => self.overlay = ShelfOverlay::None,
                _ => {}
            },
            ShelfOverlay::ControlsPort { port, row } => {
                let fp = self.controls.on_port(port).map(str::to_owned);
                let rows = controls::port_rows(fp.is_some());
                let line = rows.get(row).copied().unwrap_or(PortRow::Controller);
                match (code, line) {
                    (19 | 20, _) => self.overlay = ShelfOverlay::ControlsPort {
                        port, row: move_menu_row(row, code, rows.len()),
                    },
                    (21 | 22, PortRow::Controller) => {
                        let connected = self.connected_fps();
                        self.controls.cycle(port, code == 22, &connected);
                        self.persist_controls();
                        let rows = controls::port_rows(self.controls.on_port(port).is_some());
                        self.overlay = ShelfOverlay::ControlsPort { port, row: row.min(rows.len() - 1) };
                    }
                    (96, PortRow::Controller) => {
                        self.controls_listen = Some(Listen::Controller {
                            port, until: now + controls::LISTEN_MS,
                        });
                    }
                    (96, PortRow::Button(button)) => match fp.as_deref() {
                        Some(fp) if self.pad_connected(fp) => {
                            self.controls_listen = Some(Listen::Key {
                                port, button, until: now + controls::LISTEN_MS,
                            });
                        }
                        _ => set_message("Turn on the controller first.".into()),
                    },
                    (100, PortRow::Button(button)) => if let Some(fp) = fp.as_deref() {
                        self.controls.pad_mut(fp).clear(button);
                        self.persist_controls();
                    },
                    (21 | 22 | 96, PortRow::Stick) => if let Some(fp) = fp.as_deref() {
                        let pad = self.controls.pad_mut(fp);
                        pad.stick_off = !pad.stick_off;
                        self.persist_controls();
                    },
                    (96, PortRow::Reset) => if let Some(fp) = fp.as_deref() {
                        self.controls.pad_mut(fp).reset();
                        self.persist_controls();
                        set_message(format!("Port {} reset to default.", port + 1));
                    },
                    (97, _) => self.overlay = ShelfOverlay::ControlsSettings { row: port },
                    (108, _) => self.overlay = ShelfOverlay::None,
                    _ => {}
                }
            }
            ShelfOverlay::LayoutPorts { platform, game, row } => {
                let system = Self::system_key(platform);
                let ports = if controls::takes_port2(system) { 2 } else { 1 };
                match code {
                    19 | 20 => self.overlay = ShelfOverlay::LayoutPorts {
                        platform, game, row: move_menu_row(row, code, ports + 1),
                    },
                    96 if row < ports => self.overlay = ShelfOverlay::LayoutPort {
                        platform, game, port: row, row: 0,
                    },
                    96 => {
                        match (game, self.game_key()) {
                            (true, Some(key)) => self.controls.reset_game(&key),
                            _ => for port in 0..PORTS {
                                self.controls.reset_layout(&Scope::System(system), port);
                            },
                        }
                        self.persist_controls();
                        set_message("Controls reset to default.".into());
                    }
                    97 if game => self.overlay = ShelfOverlay::PauseSettings { row: 3 },
                    97 => self.overlay = ShelfOverlay::ScreenSystemSettings {
                        platform, row: Self::system_rows(platform).len(),
                    },
                    108 if !game => self.overlay = ShelfOverlay::None,
                    _ => {}
                }
            }
            ShelfOverlay::LayoutPort { platform, game, port, row } => {
                let scope = self.layout_scope(platform, game);
                let rows = controls::layout_rows(scope.system());
                let line = rows.get(row).copied().unwrap_or(LayoutRow::Reset);
                let default_xy = self.default_xy(game);
                match (code, line) {
                    (19 | 20, _) => self.overlay = ShelfOverlay::LayoutPort {
                        platform, game, port, row: move_menu_row(row, code, rows.len()),
                    },
                    (96, LayoutRow::Button(button)) => match self.controls.on_port(port) {
                        Some(fp) if self.pad_connected(fp) => {
                            self.controls_listen = Some(Listen::Console {
                                port, button, until: now + controls::LISTEN_MS,
                            });
                        }
                        _ => set_message(format!("Choose Port {}'s controller in Settings > Controls first.", port + 1)),
                    },
                    (100, LayoutRow::Button(button)) => {
                        self.controls.clear_console(&scope, port, button);
                        self.persist_controls();
                    }
                    (21 | 22 | 96, LayoutRow::Xy) => {
                        self.controls.cycle_xy(&scope, port, code != 21, default_xy);
                        self.persist_controls();
                    }
                    (21 | 22 | 96, LayoutRow::Shoulders) => {
                        self.controls.cycle_shoulders(&scope, port, code != 21);
                        self.persist_controls();
                    }
                    (96, LayoutRow::Reset) => {
                        self.controls.reset_layout(&scope, port);
                        self.persist_controls();
                        set_message(format!("Port {} reset to default.", port + 1));
                    }
                    (97, _) => self.overlay = ShelfOverlay::LayoutPorts { platform, game, row: port },
                    (108, _) if !game => self.overlay = ShelfOverlay::None,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// Rows, selection and scroll of the Controls page on screen, if any.
    fn controls_lines(&self) -> Option<(String, Vec<ControlsLine>, usize, Vec<(&'static str, &'static str)>)> {
        let now = self.controls_now();
        let line = |label: &str, value: String, cycles: bool| ControlsLine {
            label: label.to_owned(), value, cycles, lit: true,
        };
        let dim = |mut line: ControlsLine, lit: bool| { line.lit = lit; line };
        match self.overlay {
            ShelfOverlay::ControlsSettings { row } => Some(("CONTROLS".into(), vec![
                line("Port 1", self.pad_name(self.controls.on_port(0)), false),
                line("Port 2", self.pad_name(self.controls.on_port(1)), false),
                line("Reset to Default", String::new(), false),
            ], row, vec![("A", "Select"), ("B", "Back")])),
            ShelfOverlay::ControlsPort { port, row } => {
                let fp = self.controls.on_port(port);
                let pad = fp.map(|fp| self.controls.pad(fp)).unwrap_or_default();
                let listen = self.controls_listen;
                let lines: Vec<ControlsLine> = controls::port_rows(fp.is_some()).into_iter().map(|item| match item {
                    PortRow::Controller => line("Controller", match listen {
                        Some(l @ Listen::Controller { .. }) =>
                            format!("Press any button... {}", l.seconds_left(now)),
                        _ => self.pad_name(fp),
                    }, true),
                    PortRow::Button(button) => line(controls::BUTTONS[button].0, match listen {
                        Some(l @ Listen::Key { button: b, .. }) if b == button =>
                            format!("Press a button... {}", l.seconds_left(now)),
                        _ => pad.bind(button).map_or_else(|| "None".into(), controls::key_name),
                    }, false),
                    PortRow::Stick => line("Left Stick",
                        if pad.stick_off { "Off".into() } else { "D-Pad".into() }, true),
                    PortRow::Reset => line("Reset to Default", String::new(), false),
                }).collect();
                let item = controls::port_rows(fp.is_some()).get(row).copied();
                let hints = match (listen, item) {
                    (Some(_), _) => vec![("SLOT", "Cancel")],
                    (None, Some(PortRow::Controller)) => vec![("SWAP", "Change"), ("A", "Detect"), ("B", "Back")],
                    (None, Some(PortRow::Button(_))) => vec![("A", "Map"), ("Y", "Clear"), ("B", "Back")],
                    (None, Some(PortRow::Stick)) => vec![("SWAP", "Change"), ("B", "Back")],
                    _ => vec![("A", "Select"), ("B", "Back")],
                };
                Some((format!("PORT {}", port + 1), lines, row, hints))
            }
            ShelfOverlay::LayoutPorts { platform, game, row } => {
                let system = Self::system_key(platform);
                let mut lines = vec![line("Port 1", String::new(), false)];
                if controls::takes_port2(system) { lines.push(line("Port 2", String::new(), false)); }
                lines.push(line("Reset to Default", String::new(), false));
                let heading = if game { "GAME CONTROLS".to_owned() }
                    else { format!("{} CONTROLS", platform.name().to_uppercase()) };
                Some((heading, lines, row, vec![("A", "Select"), ("B", "Back")]))
            }
            ShelfOverlay::LayoutPort { platform, game, port, row } => {
                let scope = self.layout_scope(platform, game);
                let system = scope.system();
                let listen = self.controls_listen;
                let default_xy = self.default_xy(game);
                let rows = controls::layout_rows(system);
                let lines: Vec<ControlsLine> = rows.iter().map(|item| match *item {
                    LayoutRow::Button(button) => {
                        let (slot, own) = self.controls.bind(&scope, port, button);
                        let name = controls::console_buttons(system)[button].name;
                        match listen {
                            Some(l @ Listen::Console { button: b, .. }) if b == button =>
                                line(name, format!("Press a button... {}", l.seconds_left(now)), false),
                            _ => dim(line(name, controls::slot_button_name(slot).into(), false), own),
                        }
                    }
                    LayoutRow::Xy => {
                        let (role, own) = self.controls.xy(&scope, port, default_xy);
                        dim(line("X / Y", role.label().into(), true), own)
                    }
                    LayoutRow::Shoulders => {
                        let (role, own) = self.controls.shoulders(&scope, port);
                        dim(line("L1 / R1", role.label().into(), true), own)
                    }
                    LayoutRow::Reset => line("Reset to Default", String::new(), false),
                }).collect();
                let hints = match (listen, rows.get(row)) {
                    (Some(_), _) => vec![("SLOT", "Cancel")],
                    (None, Some(LayoutRow::Button(_))) => vec![("A", "Map"), ("Y", "Clear"), ("B", "Back")],
                    (None, Some(LayoutRow::Xy | LayoutRow::Shoulders)) => vec![("SWAP", "Change"), ("B", "Back")],
                    _ => vec![("A", "Select"), ("B", "Back")],
                };
                let heading = if game { format!("GAME PORT {}", port + 1) }
                    else { format!("{} PORT {}", platform.name().to_uppercase(), port + 1) };
                Some((heading, lines, row, hints))
            }
            _ => None,
        }
    }

    /// The cache key of a face: dim text is another face.
    fn face_key(text: &str, lit: bool) -> String {
        if lit { text.to_owned() } else { format!("\u{1}{text}") }
    }

    fn controls_face(&mut self, text: &str, lit: bool) {
        let key = Self::face_key(text, lit);
        if text.is_empty() || self.controls_faces.contains_key(&key) { return; }
        // Countdowns and names make a few dozen faces at most; start over
        // rather than grow if a session somehow makes many more.
        if self.controls_faces.len() > 400 {
            let reuse: Vec<slot_gfx::TexId> = self.controls_faces.drain().map(|(_, (tex, _, _))| tex).collect();
            self.controls_spare_faces.extend(reuse);
        }
        let face = quick_value_face(text, lit);
        let tex = match self.controls_spare_faces.pop() {
            Some(tex) => { self.gpu.update_face(tex, &face); tex }
            None => self.gpu.create_face(&face),
        };
        self.controls_faces.insert(key, (tex, face.w, face.h));
    }

    fn controls_hint(&mut self, key: &'static str, label: &'static str) -> (slot_gfx::TexId, u32) {
        if let Some(&hint) = self.controls_hint_faces.get(&(key, label)) { return hint; }
        let face = if key == "SWAP" { arrows_hint_face(label) } else { hint_face(key, label) };
        let hint = (self.gpu.create_face(&face), face.w);
        self.controls_hint_faces.insert((key, label), hint);
        hint
    }

    /// Before drawing: end a Map or Detect that waited too long, and make
    /// the faces of the Controls page on screen.
    pub(super) fn prepare_controls(&mut self) {
        let now = self.controls_now();
        if self.controls_listen.is_some_and(|l| now >= l.until()) { self.controls_listen = None; }
        if !matches!(self.overlay, ShelfOverlay::ControlsSettings { .. } | ShelfOverlay::ControlsPort { .. }
            | ShelfOverlay::LayoutPort { .. }) {
            self.controls_listen = None;
        }
        let Some((heading, lines, selected, hint_keys)) = self.controls_lines() else {
            self.controls_view = ControlsView::default();
            return;
        };
        self.controls_face(&heading, true);
        for line in &lines {
            self.controls_face(&line.label, true);
            self.controls_face(&line.value, line.lit);
        }
        let hints: Vec<(slot_gfx::TexId, u32)> = hint_keys.into_iter().map(|(k, l)| self.controls_hint(k, l)).collect();
        let offset = controls::scroll_offset(selected, lines.len());
        self.controls_view = ControlsView { heading, lines, selected, offset, hints };
    }

    /// The menu cursor's page kind, selected visible row and visible rows.
    pub(super) fn controls_cursor(&self) -> Option<(u8, usize, usize)> {
        let rows = self.controls_view.lines.len();
        match self.overlay {
            ShelfOverlay::ControlsSettings { row } => Some((18, row, 3)),
            ShelfOverlay::ControlsPort { port, row } if rows > 0 => {
                let offset = controls::scroll_offset(row, rows);
                Some((19 + port as u8, row - offset, rows.min(controls::VISIBLE_ROWS)))
            }
            ShelfOverlay::LayoutPorts { row, .. } => Some((21, row, rows)),
            ShelfOverlay::LayoutPort { port, row, .. } if rows > 0 => {
                let offset = controls::scroll_offset(row, rows);
                Some((22 + port as u8, row - offset, rows.min(controls::VISIBLE_ROWS)))
            }
            _ => None,
        }
    }

    pub(super) fn draw_controls(&self, out: &mut Vec<Draw>) {
        let view = &self.controls_view;
        out.push(Draw::Rect { x: 0.0, y: 0.0, w: out_w() as f32, h: OUT_H as f32, colour: opening() });
        let shown = view.lines.len().saturating_sub(view.offset).min(controls::VISIBLE_ROWS);
        let top = settings_row_top(shown);
        let fade = slot_ui::ease((self.menu_opened.elapsed().as_secs_f32() / 0.16).clamp(0.0, 1.0));
        let cursor = if self.menu_cursor_y.is_finite() { self.menu_cursor_y }
            else { top + view.selected.saturating_sub(view.offset) as f32 * QUICK_PITCH };
        out.push(Draw::Rect {
            x: 0.0, y: cursor + 4.0, w: out_w() as f32, h: QUICK_PITCH - 8.0,
            colour: { let mut c = edge(); c[3] *= fade; c },
        });
        let face = |text: &str, lit: bool| self.controls_faces.get(&Self::face_key(text, lit)).copied();
        let width = out_w() as f32;
        if let Some((tex, w, h)) = face(&view.heading, true) {
            let scale = 0.79f32.min((width - 70.0) / w.max(1) as f32);
            out.push(Draw::Tex { x: 32.0, y: 33.0, w: w as f32 * scale, h: h as f32 * scale,
                tex, alpha: 0.68 * fade });
        }
        for (i, line) in view.lines.iter().enumerate().skip(view.offset).take(shown) {
            let visible = i - view.offset;
            let y = top + visible as f32 * QUICK_PITCH + 8.0 + (1.0 - fade) * 8.0;
            let chosen = i == view.selected;
            let mut label_room = width - 70.0;
            let right = width - 34.0;
            if let Some((tex, w, h)) = face(&line.value, line.lit) {
                let carets = chosen && line.cycles;
                let scale = 0.79f32.min((width * 0.55) / w.max(1) as f32);
                let vw = w as f32 * scale;
                let x = right - vw - if carets { 22.0 } else { 0.0 };
                label_room = (x - 37.0 - 16.0 - if carets { 20.0 } else { 0.0 }).max(80.0);
                out.push(Draw::Tex { x, y, w: vw, h: h as f32 * scale, tex,
                    alpha: if chosen { fade } else { 0.62 * fade } });
                if carets {
                    let (left, lw, lh) = self.setting_carets[0];
                    let (right_tex, rw, rh) = self.setting_carets[1];
                    out.push(Draw::Tex { x: x - lw as f32 - 8.0, y, w: lw as f32, h: lh as f32,
                        tex: left, alpha: fade });
                    out.push(Draw::Tex { x: right - rw as f32, y, w: rw as f32, h: rh as f32,
                        tex: right_tex, alpha: fade });
                }
            }
            if let Some((tex, w, h)) = face(&line.label, true) {
                let scale = 0.79f32.min(label_room / w.max(1) as f32);
                out.push(Draw::Tex { x: 37.0, y, w: w as f32 * scale, h: h as f32 * scale,
                    tex, alpha: fade });
            }
        }
        // More rows above or below: the list's edges fade into the page.
        out.push(Draw::Rect { x: 0.0, y: 420.0, w: width, h: 60.0, colour: opening() });
        for (tex, w, x) in centred_hints(&view.hints, LEGEND_GAP) {
            out.push(Draw::Tex { x, y: 427.0, w: w as f32, h: HINT_H as f32, tex, alpha: fade });
        }
    }
}

/// A slot. button's bit in a joypad mask (the same buttons as Port 1).
fn controls_mask(slot: i32) -> u16 { super::controls(slot) }

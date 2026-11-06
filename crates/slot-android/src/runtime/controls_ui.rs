//! Settings > Controls on the engine: controller keys routed to Port 1, to
//! the second player or to the menus, and the Controls pages themselves.
use super::*;
use crate::controls::{self, Device, Listen, PortRow, Sink, BUILTIN, MODE, PORTS};

/// A Held key for a stick direction, apart from the same device's D-pad.
const STICK_KEY: i32 = 1 << 20;

/// One prepared row of a Controls page.
pub(super) struct ControlsLine {
    label: String,
    value: String,
    /// ◀▶ changes it: carets around the selected value.
    cycles: bool,
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

    pub(super) fn persist_controls(&self) {
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
        if self.controls.claim_free_port(&fp) {
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
            _ => {}
        }
    }

    /// Rows, selection and scroll of the Controls page on screen, if any.
    fn controls_lines(&self) -> Option<(String, Vec<ControlsLine>, usize, Vec<(&'static str, &'static str)>)> {
        let now = self.controls_now();
        let line = |label: &str, value: String, cycles: bool| ControlsLine {
            label: label.to_owned(), value, cycles,
        };
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
            _ => None,
        }
    }

    fn controls_face(&mut self, text: &str) {
        if text.is_empty() || self.controls_faces.contains_key(text) { return; }
        // Countdowns and names make a few dozen faces at most; start over
        // rather than grow if a session somehow makes many more.
        if self.controls_faces.len() > 400 {
            let reuse: Vec<slot_gfx::TexId> = self.controls_faces.drain().map(|(_, (tex, _, _))| tex).collect();
            self.controls_spare_faces.extend(reuse);
        }
        let face = quick_value_face(text, true);
        let tex = match self.controls_spare_faces.pop() {
            Some(tex) => { self.gpu.update_face(tex, &face); tex }
            None => self.gpu.create_face(&face),
        };
        self.controls_faces.insert(text.to_owned(), (tex, face.w, face.h));
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
        if !matches!(self.overlay, ShelfOverlay::ControlsSettings { .. } | ShelfOverlay::ControlsPort { .. }) {
            self.controls_listen = None;
        }
        let Some((heading, lines, selected, hint_keys)) = self.controls_lines() else {
            self.controls_view = ControlsView::default();
            return;
        };
        self.controls_face(&heading);
        for line in &lines {
            self.controls_face(&line.label);
            self.controls_face(&line.value);
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
        let face = |text: &str| self.controls_faces.get(text).copied();
        let width = out_w() as f32;
        if let Some((tex, w, h)) = face(&view.heading) {
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
            if let Some((tex, w, h)) = face(&line.value) {
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
            if let Some((tex, w, h)) = face(&line.label) {
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

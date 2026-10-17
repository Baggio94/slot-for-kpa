use slot_gfx::{Draw, TexId, OUT_H, OUT_W};
use slot_store::{Cart, Platform};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::cart::{cart_box, gb_shell_of, label_colour, label_text, label_panel, gb_label_panel, GBC_LABEL_X, GBC_LABEL_Y, GBC_LABEL_W, CART_H, CART_W};
use crate::hud::Millis;
use crate::silhouette::GbShell;
use crate::slot_chrome::{draw_empty_slot, MOUTH_H};

const SIDE_SCALE: f32 = 0.78;
// slot. v1.7 keeps the adjacent cart opaque while browsing.
const SIDE_ALPHA: f32 = 1.0;
const STAR_INSET: f32 = 5.0;

static SLACK: AtomicU32 = AtomicU32::new(0);

pub fn set_shelf_slack(px: f32) {
    SLACK.store(px.max(0.0).to_bits(), Ordering::Relaxed);
}

fn raise() -> f32 {
    f32::from_bits(SLACK.load(Ordering::Relaxed)) / 2.0
}

pub fn panel_middle_y() -> f32 {
    OUT_H as f32 / 2.0 + raise()
}

pub fn rest_y(h: f32) -> f32 {
    let y = if h > CART_H as f32 {
        (OUT_H as f32 - MOUTH_H - h) / 2.0
    } else {
        (OUT_H + CART_H) as f32 / 2.0 - h
    };
    y - raise()
}

pub fn foot_y(h: f32) -> f32 {
    rest_y(h) + h
}
const OMEGA: f32 = 16.0;
const PART: f32 = 130.0;

const SLOTS: i32 = 3;

const REPEAT_DELAY_MS: Millis = 400;
const REPEAT_MS: [Millis; 4] = [110, 85, 65, 50];

pub struct Shelf {
    pub carts: Vec<Cart>,
    pub index: usize,
    pub scroll: f32,
    faces: Vec<Option<TexId>>,
    shadow: Option<TexId>,
    gb_shadow: Option<TexId>,
    gbc_shadow: Option<TexId>,
    shells: Vec<Option<GbShell>>,
    starred: Vec<bool>,
    star: Option<(TexId, f32, f32)>,
    ride: f32,
    vel: f32,
    held: Option<(i32, Millis, usize)>,
    /// Up/Down repeat letter jumps like upstream Slot v1.7. Android drops
    /// framework key repeats; the native shelf clock owns this schedule.
    held_letter: Option<(i32, Millis, usize)>,
}

impl Shelf {
    pub fn new(carts: Vec<Cart>) -> Self {
        Shelf {
            starred: vec![false; carts.len()],
            star: None,
            shells: carts.iter().map(gb_shell_of).collect(),
            faces: vec![None; carts.len()],
            carts,
            index: 0,
            scroll: 0.0,
            shadow: None,
            gb_shadow: None,
            gbc_shadow: None,
            ride: 0.0,
            vel: 0.0,
            held: None,
            held_letter: None,
        }
    }

    /// Reorder the Recents rack without discarding reusable GPU faces or
    /// shadow handles. The returned old/new indices re-key Engine's bounded
    /// texture pool; removed ROM faces remain recyclable there.
    pub fn replace_carts_preserving_visuals(&mut self, carts: Vec<Cart>) -> Vec<(usize, usize)> {
        let before = std::mem::replace(&mut self.carts, carts);
        let faces = std::mem::take(&mut self.faces);
        let previously_selected = before.get(self.index).map(|c| c.rom.clone());
        self.faces = vec![None; self.carts.len()];
        self.shells = self.carts.iter().map(gb_shell_of).collect();
        self.starred = vec![false; self.carts.len()];
        let mut moves = Vec::new();
        for (old, cart) in before.iter().enumerate() {
            if let Some(new) = self.carts.iter().position(|item| item.rom == cart.rom) {
                self.faces[new] = faces[old];
                moves.push((old, new));
            }
        }
        let next = previously_selected
            .and_then(|rom| self.carts.iter().position(|cart| cart.rom == rom))
            .unwrap_or(0);
        self.select(next);
        self.release_hold();
        moves
    }

    pub fn select(&mut self, i: usize) {
        self.index = i;
        self.scroll = i as f32;
        self.ride = i as f32;
        self.vel = 0.0;
    }

    pub fn set_star(&mut self, tex: TexId, w: u32, h: u32) {
        self.star = Some((tex, w as f32, h as f32));
    }
    pub fn mark_starred(&mut self, mut is_favorite: impl FnMut(&Cart) -> bool) {
        self.starred = self.carts.iter().map(&mut is_favorite).collect();
    }

    pub fn set_shadow(&mut self, face: TexId) {
        self.shadow = Some(face);
    }

    pub fn set_gb_shadow(&mut self, shell: GbShell, face: TexId) {
        match shell {
            GbShell::Notched => self.gb_shadow = Some(face),
            GbShell::Rounded => self.gbc_shadow = Some(face),
        }
    }

    pub fn set_faces(&mut self, faces: Vec<TexId>) {
        let mut faces = faces.into_iter();
        for face in &mut self.faces {
            *face = faces.next();
        }
    }

    pub fn face(&self, i: usize) -> Option<TexId> {
        self.faces.get(i).copied().flatten()
    }

    pub fn set_face(&mut self, i: usize, tex: TexId) -> Option<TexId> {
        self.faces.get_mut(i).and_then(|f| f.replace(tex))
    }

    pub fn take_face(&mut self, i: usize) -> Option<TexId> {
        self.faces.get_mut(i).and_then(Option::take)
    }

    pub fn on_screen(&self) -> Vec<usize> {
        self.around(SLOTS)
    }

    pub fn around(&self, radius: i32) -> Vec<usize> {
        let mut out = Vec::new();
        for step in 0..=radius {
            for off in [step, -step] {
                if let Some(i) = self.cart_at_offset(off) {
                    if !out.contains(&i) {
                        out.push(i);
                    }
                }
            }
        }
        out
    }

    pub fn find(&self, stem: &str) -> Option<(&Cart, Option<TexId>)> {
        let i = self.carts.iter().position(|c| c.stem == stem)?;
        Some((&self.carts[i], self.face(i)))
    }

    pub fn left(&mut self) {
        self.step(-1);
    }

    pub fn right(&mut self) {
        self.step(1);
    }

    pub fn jump_next_letter(&mut self) {
        self.jump(1);
    }

    pub fn jump_prev_letter(&mut self) {
        self.jump(-1);
    }

    fn start_of_letter(&self, from: usize) -> usize {
        let n = self.carts.len();
        let letter = slot_store::initial(&self.carts[from].stem);
        let mut at = from;
        for _ in 0..n {
            let before = (at as i32 - 1).rem_euclid(n as i32) as usize;
            if slot_store::initial(&self.carts[before].stem) != letter {
                break;
            }
            at = before;
        }
        at
    }

    fn jump(&mut self, dir: i32) {
        let n = self.carts.len();
        if n < 2 {
            return;
        }
        let wrap = |i: i32| i.rem_euclid(n as i32) as usize;
        let here = slot_store::initial(&self.carts[self.index].stem);
        if self
            .carts
            .iter()
            .all(|c| slot_store::initial(&c.stem) == here)
        {
            return;
        }
        let target = match dir > 0 {
            true => {
                let mut at = self.index;
                for _ in 0..n {
                    at = wrap(at as i32 + 1);
                    if slot_store::initial(&self.carts[at].stem) != here {
                        break;
                    }
                }
                at
            }
            false => {
                let start = self.start_of_letter(self.index);
                match start == self.index {
                    true => self.start_of_letter(wrap(start as i32 - 1)),
                    false => start,
                }
            }
        };
        let ahead = (target as i32 - self.index as i32).rem_euclid(n as i32);
        let delta = match dir > 0 {
            true => ahead,
            false => ahead - n as i32,
        };
        self.index = target;
        self.ride += delta as f32;
    }

    pub fn hold_left(&mut self, now: Millis) {
        self.hold(-1, now);
    }

    pub fn hold_right(&mut self, now: Millis) {
        self.hold(1, now);
    }

    fn hold(&mut self, by: i32, now: Millis) {
        self.held_letter = None;
        self.step(by);
        self.held = Some((by, now + REPEAT_DELAY_MS, 0));
    }

    pub fn hold_up(&mut self, now: Millis) {
        self.hold_letter(-1, now);
    }

    pub fn hold_down(&mut self, now: Millis) {
        self.hold_letter(1, now);
    }

    fn hold_letter(&mut self, by: i32, now: Millis) {
        self.held = None;
        self.jump(by);
        self.held_letter = Some((by, now + REPEAT_DELAY_MS, 0));
    }

    pub fn release_up(&mut self) {
        self.release_letter(-1);
    }

    pub fn release_down(&mut self) {
        self.release_letter(1);
    }

    fn release_letter(&mut self, by: i32) {
        if matches!(self.held_letter, Some((held, _, _)) if held == by) {
            self.held_letter = None;
        }
    }

    pub fn release_left(&mut self) {
        self.release(-1);
    }

    pub fn release_right(&mut self) {
        self.release(1);
    }

    fn release(&mut self, by: i32) {
        if matches!(self.held, Some((held, _, _)) if held == by) {
            self.held = None;
        }
    }

    pub fn release_hold(&mut self) {
        self.held = None;
        self.held_letter = None;
    }

    /// Returns true only if a repeated UP/DOWN changed the selected letter.
    /// Horizontal repeats continue independently with their existing speed.
    pub fn tick(&mut self, now: Millis) -> bool {
        if let Some((by, due, fired)) = self.held_letter {
            if now >= due {
                let before = self.index;
                self.jump(by);
                let gap = REPEAT_MS[fired.min(REPEAT_MS.len() - 1)];
                self.held_letter = Some((by, now + gap, fired + 1));
                return self.index != before;
            }
            return false;
        }
        if let Some((by, due, fired)) = self.held {
            if now >= due {
                self.step(by);
                let gap = REPEAT_MS[fired.min(REPEAT_MS.len() - 1)];
                self.held = Some((by, now + gap, fired + 1));
            }
        }
        false
    }

    fn step(&mut self, by: i32) {
        let n = self.carts.len();
        if n < 2 {
            return;
        }
        self.index = (self.index as i32 + by).rem_euclid(n as i32) as usize;
        self.ride += by as f32;
    }

    pub fn scroll_target(&self) -> f32 {
        let n = self.carts.len();
        if n == 0 {
            return 0.0;
        }
        let from = self.ride;
        let n = n as f32;
        from + (self.index as f32 - from + n / 2.0).rem_euclid(n) - n / 2.0
    }

    pub fn cart_at_offset(&self, off: i32) -> Option<usize> {
        let n = self.carts.len() as i32;
        if n == 0 {
            return None;
        }
        if n == 1 {
            return (off == 0).then_some(self.index);
        }
        Some((self.index as i32 + off).rem_euclid(n) as usize)
    }

    // Upstream slot. v1.7 positions a mixed GBA/GB/GBC rack by pairwise
    // widths. In particular, the GB/GBC shells are not as wide as GBA carts.
    fn slot_width(&self, slot: i32) -> (f32, bool) {
        match self.cart_at_offset(slot).map(|i| self.carts[i].platform) {
            Some(Platform::Gba) | None => (CART_W as f32, true),
            Some(platform) => (cart_box(platform).0 as f32, false),
        }
    }

    fn pitch(&self, a: i32, u: f32) -> f32 {
        let ((wa, gba_a), (wb, gba_b)) =
            (self.slot_width(a), self.slot_width(a + 1));
        let (sa, sb) = (shrink(a as f32 - u), shrink((a + 1) as f32 - u));
        let gap_of = if gba_a || gba_b { CART_W as f32 } else { wa };
        wa * sa / 2.0 + gap_of * (1.0 - (sa + sb) / 2.0) + wb * sb / 2.0
    }

    fn centre(&self, slot: i32) -> f32 {
        let u = self.scroll - self.scroll_target();
        let a = u.floor() as i32;
        let mut centre = OUT_W as f32 / 2.0 - (u - a as f32) * self.pitch(a, u);
        if slot >= a {
            for s in a..slot { centre += self.pitch(s, u); }
        } else {
            for s in (slot..a).rev() { centre -= self.pitch(s, u); }
        }
        centre
    }

    pub fn selected_at(&self) -> (f32, f32) {
        let (w, _) = self.slot_width(0);
        let scale = shrink(self.scroll_target() - self.scroll);
        (self.centre(0) - w * scale / 2.0, scale)
    }

    /// True while spring-driven shelf scrolling is visibly in flight.
    pub fn is_scrolling(&self) -> bool {
        (self.scroll_target()-self.scroll).abs()>0.03 || self.vel.abs()>0.05
    }

    pub fn update(&mut self, dt: f32) {
        let accel = -2.0 * OMEGA * self.vel - OMEGA * OMEGA * (self.scroll - self.scroll_target());
        self.vel += accel * dt;
        self.scroll += self.vel * dt;
    }

    pub fn draw(&self, shake: f32, out: &mut Vec<Draw>) {
        self.draw_row(None, shake, 0.0, 1.0, out);
        draw_empty_slot(out);
    }

    pub fn draw_row(
        &self,
        hidden: Option<&str>,
        shake: f32,
        recede: f32,
        dim: f32,
        out: &mut Vec<Draw>,
    ) {
        let recede = recede.clamp(0.0, 1.0);
        let dim = dim.clamp(0.0, 1.0);
        let target = self.scroll_target();
        for slot in -SLOTS..=SLOTS {
            let Some(i) = self.cart_at_offset(slot) else {
                continue;
            };
            let cart = &self.carts[i];
            if hidden == Some(cart.stem.as_str()) {
                continue;
            }
            let offset = target + slot as f32 - self.scroll;
            let t = offset.abs().min(1.0);
            let scale = shrink(offset);
            let alpha = (1.0 + (SIDE_ALPHA - 1.0) * t) * (1.0 - recede);
            let (cw, ch) = cart_box(cart.platform);
            let (w, h) = (cw as f32 * scale, ch as f32 * scale);
            let away = offset.signum() * (1.0 + offset.abs());
            let x = self.centre(slot) - w / 2.0 + away * PART * recede;
            if x + w <= 0.0 || x >= OUT_W as f32 || alpha <= 0.0 {
                continue;
            }
            let x = x + shake;
            let y = foot_y(ch as f32) - h;
            let backing = match self.shells.get(i).copied().flatten() {
                None => self.shadow,
                Some(GbShell::Notched) => self.gb_shadow,
                Some(GbShell::Rounded) => self.gbc_shadow,
            };
            if alpha < 1.0 && self.face(i).is_some() {
                if let Some(tex) = backing {
                    out.push(Draw::Tex {
                        x, y, w, h, tex, alpha: recede_alpha(alpha),
                    });
                }
            }
            // Never show the old bright solid-colour placeholder. A cartridge
            // awaiting its PNG uses the original GBA/GB/GBC silhouette instead.
            out.push(match self.face(i) {
                Some(tex) => Draw::Tex {
                    x, y, w, h, tex, alpha: alpha * dim,
                },
                None => match backing {
                    Some(tex) => Draw::Tex {
                        x, y, w, h, tex, alpha: alpha * dim,
                    },
                    None => Draw::Rect {
                        x, y, w, h, colour: [0.14, 0.14, 0.16, alpha * dim],
                    },
                },
            });
            // Faithful original v1.7 pixel-star with label-relative geometry.
            if let (Some(true), Some((tex, sw, sh))) = (self.starred.get(i), self.star) {
                let (lx, ly, lw) = match cart.platform {
                    slot_store::Platform::Gba => {
                        let (x,y,right,_) = label_panel(cw,ch);
                        (x as f32,y as f32,(right-x) as f32)
                    },
                    slot_store::Platform::Gb => {
                        let (x,y,right,_) = gb_label_panel(cw,ch);
                        (x as f32,y as f32,(right-x) as f32)
                    },
                    slot_store::Platform::Gbc =>
                        (GBC_LABEL_X as f32,GBC_LABEL_Y as f32,GBC_LABEL_W as f32),
                };
                out.push(Draw::Tex {
                    x: (x + (lx + lw) * scale - sw - STAR_INSET).round(),
                    y: (y + ly * scale + STAR_INSET).round(),
                    w: sw, h: sh, tex, alpha: alpha * dim,
                });
            }
        }
    }
}

fn shrink(offset: f32) -> f32 {
    1.0 + (SIDE_SCALE - 1.0) * offset.abs().min(1.0)
}

fn recede_alpha(face_alpha: f32) -> f32 {
    (face_alpha / SIDE_ALPHA).clamp(0.0, 1.0)
}

#[cfg(test)]
mod konkr_navigation_tests {
    use super::*;

    fn cart(name: &str) -> Cart {
        Cart {
            platform: slot_store::Platform::Gba,
            stem: name.to_string(),
            title: String::new(), code: String::new(),
            rom: std::path::PathBuf::from(format!("{name}.gba")),
            label: None, shell: None,
        }
    }

    #[test]
    fn vertical_letter_jump_repeats_on_native_clock_and_stops_on_release() {
        let mut shelf = Shelf::new(vec![
            cart("Apple"), cart("Apricot"), cart("Banana"), cart("Cherry"),
        ]);
        shelf.hold_down(0);
        assert_eq!(shelf.carts[shelf.index].stem, "Banana");
        assert!(!shelf.tick(399));
        assert_eq!(shelf.carts[shelf.index].stem, "Banana");
        assert!(shelf.tick(400));
        assert_eq!(shelf.carts[shelf.index].stem, "Cherry");
        shelf.release_down();
        assert!(!shelf.tick(10_000));
        assert_eq!(shelf.carts[shelf.index].stem, "Cherry");

        shelf.hold_up(10_000);
        assert_eq!(shelf.carts[shelf.index].stem, "Banana");
        shelf.release_hold();
        assert!(!shelf.tick(20_000));
        assert_eq!(shelf.carts[shelf.index].stem, "Banana");
    }

    #[test]
    fn held_letter_switch_cancels_old_horizontal_hold() {
        let mut shelf = Shelf::new(vec![
            cart("Apple"), cart("Banana"), cart("Cherry"),
        ]);
        shelf.hold_right(0);
        assert_eq!(shelf.carts[shelf.index].stem, "Banana");
        shelf.hold_down(50);
        assert_eq!(shelf.carts[shelf.index].stem, "Cherry");
        assert!(!shelf.tick(449));
        assert!(shelf.tick(450));
        assert_eq!(shelf.carts[shelf.index].stem, "Apple");
    }
}

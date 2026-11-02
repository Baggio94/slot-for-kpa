use slot_gfx::out_w;
use slot_store::{Cart, Core, Platform};

use crate::art;
use crate::bitmaps::{GB_CART_H, GB_CART_W};
use crate::cart::{cart_box, cut_to_shell, is_clear, clean_label, gb_shell_of, CartFace, CART_H, CART_W};
use crate::silhouette::GbShell;
use crate::density::Face as _;
use crate::density::{self, px, Canvas};
use crate::shelf::foot_y;
use crate::shell::shell_for;
use crate::slot_chrome::ease;
use crate::text;

const BOARD_SVG: &str = include_str!("../assets/board.svg");
// The Game Boy pak board and its back shells, drawn in board.svg's style by
// tools/pak-board/gen.py from the layout of assets/gb_board.png.
const GB_BOARD_SVG: &str = include_str!("../assets/gb_board.svg");
const GB_SHELL_SVG: &str = include_str!("../assets/gb_shell.svg");
const GBC_SHELL_SVG: &str = include_str!("../assets/gbc_shell.svg");
const GB_CHIP_SVG: &str = include_str!("../assets/gb_chip.svg");

pub const BOARD_W: u32 = 372;
pub const BOARD_H: u32 = 209;

pub const ROM_X: u32 = 67;
pub const ROM_Y: u32 = 48;
pub const ROM_W: u32 = 71;
pub const ROM_H: u32 = 90;

const PLASTIC: &str = "#ff00ff";
const FLOOR: &str = "#800080";
const DEEP: &str = "#400040";

const MARK_LINE_CHARS: usize = 10;
const MARK_LINES: usize = 3;
const MARK_PAD: u32 = 4;
const MARK_PX: f32 = 10.0;
const MARK_MIN_PX: f32 = 6.0;
const MARK_INK: [u8; 3] = [0xbd, 0xbd, 0xbd];

// The square chip of the GB/GBC pak board carries the title.
const PAK_MARK_X: u32 = 74;
const PAK_MARK_Y: u32 = 108;
const PAK_MARK_SIDE: u32 = 42;
const PAK_MARK_LINE_CHARS: usize = 8;
const PAK_MARK_PX: f32 = 9.0;
const PAK_MARK_MIN_PX: f32 = 5.0;

pub fn rom_marking(stem: &str) -> Vec<String> {
    marking(stem, MARK_LINE_CHARS)
}

fn marking(stem: &str, line_chars: usize) -> Vec<String> {
    let mut title: Vec<String> = Vec::new();
    for word in clean_label(stem).to_uppercase().split_whitespace() {
        match title.last_mut() {
            Some(line) if line.len() + 1 + word.len() <= line_chars => {
                line.push(' ');
                line.push_str(word);
            }
            _ => title.push(word.to_string()),
        }
    }
    if title.len() > MARK_LINES {
        let tail = title.split_off(MARK_LINES - 1).join(" ");
        title.push(tail);
    }
    title
}

pub fn rom_marking_face(stem: &str) -> CartFace {
    marking_face(&rom_marking(stem), ROM_W, ROM_H, MARK_PAD, MARK_PX, MARK_MIN_PX)
}

/// The title on a GB/GBC pak board's small square chip.
pub fn pak_marking_face(stem: &str) -> CartFace {
    let title = marking(stem, PAK_MARK_LINE_CHARS);
    marking_face(&title, PAK_MARK_SIDE, PAK_MARK_SIDE, 3, PAK_MARK_PX, PAK_MARK_MIN_PX)
}

fn marking_face(title: &[String], w: u32, h: u32, pad: u32, px: f32, min_px: f32) -> CartFace {
    let mut face = Canvas::new(w, h).into_cart();
    let Some(font) = text::label_font() else {
        return face;
    };
    let max_w = (w - 2 * pad) as f32;
    let line_h = (px * 1.25).ceil() as u32;
    let mut top = h.saturating_sub(line_h * title.len() as u32) / 2;
    for line in title {
        let layout = text::fit(font, line, max_w, 1, px, min_px);
        ink_band(&mut face, top, line_h, &layout, MARK_INK, 1.0);
        top += line_h;
    }
    face
}

pub fn board_face(cart: &Cart) -> CartFace {
    if gb_shell_of(cart).is_some() {
        // An opened Game Boy / Game Boy Color pak: its own back shell in the
        // game's colour with the pak board seated in it, the same board a
        // clear cart shows through its shell. A clear pak keeps its
        // see-through back, as on the shelf.
        let alpha = if is_clear(cart) { CLEAR_BACK } else { 255 };
        let mut face = pak_shell_face(cart, alpha);
        over(&mut face, &pak_board_face(cart), 0, 0);
        return face;
    }
    let mut face = svg_face(&shaded(BOARD_SVG, shell_for(cart).colour), BOARD_W, BOARD_H);
    over(&mut face, &rom_marking_face(&cart.stem), ROM_X, ROM_Y);
    face
}

/// A clear cart's back plastic: as see-through as cart.rs's interior.
const CLEAR_BACK: u8 = 204;

fn shaded(svg: &str, colour: [u8; 3]) -> String {
    svg.replace(DEEP, &hex(shade(colour, 0.35)))
        .replace(FLOOR, &hex(shade(colour, 0.62)))
        .replace(PLASTIC, &hex(colour))
}

/// A pak's back shell in the game's colour, cut to its outline, with the
/// pegs and screw post the board sits on.
pub(crate) fn pak_shell_face(cart: &Cart, alpha: u8) -> CartFace {
    let shell = gb_shell_of(cart).unwrap_or(GbShell::Rounded);
    let svg = match shell {
        GbShell::Notched => GB_SHELL_SVG,
        GbShell::Rounded => GBC_SHELL_SVG,
    };
    let mut face = svg_face(&shaded(svg, shell_for(cart).colour), GB_CART_W, GB_CART_H);
    cut_to_shell(&mut face, shell);
    if alpha < 255 {
        for px in face.rgba.chunks_exact_mut(4) {
            px[3] = ((u32::from(px[3]) * u32::from(alpha) + 127) / 255) as u8;
        }
    }
    face
}

/// The pak board with both big-chip footprints empty. Like a GBA board, the
/// footprints keep the cores' names and the title is printed on a chip.
fn pak_board_face(cart: &Cart) -> CartFace {
    let mut face = svg_face(GB_BOARD_SVG, GB_CART_W, GB_CART_H);
    for (core, u) in [Core::Mgba, Core::alternate_for(cart.platform)].into_iter().zip(PAK_FOOTPRINTS) {
        over(&mut face, &pak_footprint_face(core), pak_chip_x(u), pak_chip_y());
    }
    over(&mut face, &pak_marking_face(&cart.stem), PAK_MARK_X, PAK_MARK_Y);
    face
}

fn pak_chip_x(u: f32) -> u32 {
    (u - PAK_CHIP.0 / 2.0) as u32
}

fn pak_chip_y() -> u32 {
    (PAK_FOOTPRINT_Y - PAK_CHIP.1 / 2.0) as u32
}

/// What a clear pak shows through its plastic on the shelf: the opened
/// board with a chip seated in the first footprint.
pub(crate) fn pak_shelf_board(cart: &Cart) -> CartFace {
    let mut face = pak_board_face(cart);
    over(&mut face, &pak_chip_body(), pak_chip_x(PAK_FOOTPRINTS[0]), pak_chip_y());
    face
}

/// What a clear GBA cart shows through its plastic on the shelf: the opened
/// board with both sockets and a chip seated, fitted to the cart's size.
pub(crate) fn gba_shelf_board(cart: &Cart) -> CartFace {
    let mut face = board_face(cart);
    let at = |u: f32, v: f32, pad: u32| {
        let (x, y) = on_board(Placed { x: 0.0, y: 0.0, w: BOARD_W as f32, h: BOARD_H as f32 }, u, v);
        ((x.round() as i32 - pad as i32).max(0) as u32, (y.round() as i32 - pad as i32).max(0) as u32)
    };
    for (i, core) in [Core::Mgba, Core::Gpsp].into_iter().enumerate() {
        let (x, y) = at(SOCKET_U[i], SOCKET_V, 0);
        over(&mut face, &socket_face(core), x, y);
    }
    let (x, y) = at(CHIP_U[0], CHIP_V, TURN_PAD);
    over(&mut face, &chip_face(None), x, y);
    let (tw, th) = face.texels();
    let rgba = crate::native_assets::resample(&face.rgba, tw, th, px(CART_W), px(CART_H));
    CartFace { rgba, w: CART_W, h: CART_H }
}

/// SVG art rendered straight at panel density.
fn svg_face(svg: &str, w: u32, h: u32) -> CartFace {
    let (tw, th) = (px(w), px(h));
    let rgba = art::render_svg(svg, tw, th).unwrap_or_else(|| vec![0; (tw * th * 4) as usize]);
    CartFace { rgba, w, h }
}

fn ink_band(
    face: &mut CartFace,
    top: u32,
    band: u32,
    layout: &text::Layout,
    ink: [u8; 3],
    strength: f32,
) {
    let (fw, fh) = face.texels();
    let top = px(top).min(fh);
    let band = px(band).min(fh - top);
    for (i, a) in text::coverage(fw, band, &density::scaled(layout)).into_iter().enumerate() {
        let a = (a as f32 * strength).round() as u8;
        if a == 0 {
            continue;
        }
        let at = ((top * fw) as usize + i) * 4;
        let (a, d_a) = (a as f32 / 255.0, face.rgba[at + 3] as f32 / 255.0);
        let out_a = a + d_a * (1.0 - a);
        for (k, &c) in ink.iter().enumerate() {
            let d_rgb = face.rgba[at + k] as f32 / 255.0;
            let out_rgb = (c as f32 / 255.0 * a + d_rgb * d_a * (1.0 - a)) / out_a;
            face.rgba[at + k] = (out_rgb * 255.0).round() as u8;
        }
        face.rgba[at + 3] = (out_a * 255.0).round() as u8;
    }
}

fn over(dst: &mut CartFace, src: &CartFace, x: u32, y: u32) {
    let ((sw, sh), (dw, dh)) = (src.texels(), dst.texels());
    let (x, y) = (px(x), px(y));
    for row in 0..sh {
        for col in 0..sw {
            let s = ((row * sw + col) * 4) as usize;
            let a = src.rgba[s + 3] as u32;
            let (dx, dy) = (x + col, y + row);
            if a == 0 || dx >= dw || dy >= dh {
                continue;
            }
            let d = ((dy * dw + dx) * 4) as usize;
            for k in 0..3 {
                dst.rgba[d + k] =
                    ((src.rgba[s + k] as u32 * a + dst.rgba[d + k] as u32 * (255 - a)) / 255) as u8;
            }
            dst.rgba[d + 3] = dst.rgba[d + 3].max(a as u8);
        }
    }
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn shade(c: [u8; 3], f: f32) -> [u8; 3] {
    c.map(|v| (v as f32 * f).round().clamp(0.0, 255.0) as u8)
}

const SOCKET_SVG: &str = include_str!("../assets/socket.svg");
const CHIP_SVG: &str = include_str!("../assets/chip.svg");

/// Board centred on the canvas (174 on Slot's 720).
pub fn board_x() -> f32 {
    (out_w() - BOARD_W) as f32 / 2.0
}
pub const BOARD_Y: f32 = 150.0;

pub const TURN_PAD: u32 = 2;

pub const LID_TURN: f32 = -5.0 * std::f32::consts::PI / 180.0;
pub const CHIP_TIP: f32 = 4.0 * std::f32::consts::PI / 180.0;
pub const HOP_LIFT: f32 = 10.0;

pub const SLIDE_SHARE: f32 = 160.0 / 420.0;
pub const SLIDE_UP: f32 = CART_H as f32 / 3.0;

pub fn slide_of(progress: f32) -> f32 {
    ease((progress / SLIDE_SHARE).clamp(0.0, 1.0))
}

pub fn lift_of(progress: f32) -> f32 {
    ease(((progress - SLIDE_SHARE) / (1.0 - SLIDE_SHARE)).clamp(0.0, 1.0))
}

pub const SOCKET_U: [f32; 2] = [99.5, 171.5];
pub const SOCKET_V: f32 = 59.1;
pub const SOCKET_W: u32 = 64;
pub const SOCKET_H: u32 = 46;

pub const CHIP_U: [f32; 2] = [101.0, 173.0];
pub const CHIP_V: f32 = 60.6;
pub const CHIP_W: u32 = 59;
pub const CHIP_H: u32 = 41;

pub const SHADOW_W: u32 = 66;
pub const SHADOW_H: u32 = 12;

const NAME_PX: f32 = 17.0;
const NAME_MIN_PX: f32 = 8.0;
const SOCKET_INK: [u8; 3] = [0xee, 0xf5, 0xe6];
const CHIP_INK: [u8; 3] = [0xf2, 0xf2, 0xf2];

/// The GBA lid, centred above the board (x 288 on Slot's 720).
fn lid_rest_gba() -> Placed {
    Placed { x: (out_w() as f32 - 144.0) / 2.0, y: 30.0, w: 144.0, h: 81.0 }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Placed {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub fn shelf_cart() -> Placed {
    shelf_cart_at((out_w() - CART_W) as f32 / 2.0, 1.0)
}

pub fn shelf_cart_at(x: f32, scale: f32) -> Placed {
    let (w, h) = (CART_W as f32 * scale, CART_H as f32 * scale);
    Placed {
        x,
        y: foot_y(CART_H as f32) - h,
        w,
        h,
    }
}

/// The selected cart as the shelf draws it: GB/GBC carts keep their own
/// taller box, so the lid that lifts off is the same shape as the cart.
pub fn shelf_cart_for(platform: Platform, x: f32, scale: f32) -> Placed {
    let (cw, ch) = cart_box(platform);
    let (w, h) = (cw as f32 * scale, ch as f32 * scale);
    Placed { x, y: foot_y(ch as f32) - h, w, h }
}

/// Where the lid is held above the board. A GB/GBC lid keeps its aspect and
/// stays clear of the board, centred where the GBA lid rests.
pub fn lid_rest(platform: Platform) -> Placed {
    if platform == Platform::Gba { return lid_rest_gba(); }
    // A pak opens like a book: its front rests beside the board.
    let (cw, ch) = cart_box(platform);
    let w = PAK_LID_W;
    let h = w * ch as f32 / cw as f32;
    let board_w = PAK_REST_H * cw as f32 / ch as f32;
    let x = (out_w() as f32 - w - PAK_GAP - board_w) / 2.0;
    Placed { x, y: (480.0 - 120.0 - h) / 2.0 + 40.0, w, h }
}

pub fn lid_toward(shelf: Placed, rest: Placed, progress: f32) -> (Placed, f32) {
    let slid = Placed {
        y: shelf.y - SLIDE_UP * slide_of(progress),
        ..shelf
    };
    let lift = lift_of(progress);
    (lerp(slid, rest, lift), LID_TURN * lift)
}

pub fn board_at(progress: f32) -> Placed {
    board_from(shelf_cart(), progress)
}

pub fn board_from(shelf: Placed, progress: f32) -> Placed {
    let rest = Placed {
        x: board_x(),
        y: BOARD_Y,
        w: BOARD_W as f32,
        h: BOARD_H as f32,
    };
    lerp(shelf, rest, lift_of(progress))
}

pub fn lid_at(progress: f32) -> (Placed, f32) {
    lid_from(shelf_cart(), progress)
}

pub fn lid_from(shelf: Placed, progress: f32) -> (Placed, f32) {
    lid_toward(shelf, lid_rest_gba(), progress)
}

const UNITS_W: f32 = 240.0;
const UNITS_H: f32 = 135.0;

pub fn on_board(board: Placed, u: f32, v: f32) -> (f32, f32) {
    (
        board.x + u * board.w / UNITS_W,
        board.y + v * board.h / UNITS_H,
    )
}

pub fn board_zoom(board: Placed) -> f32 {
    board.w / BOARD_W as f32
}

/// One unit of the GBA board in its own pixels at rest. A pak board uses the
/// same unit, so sockets, chips, hops and shadows keep their GBA sizes.
const UNIT_PX: f32 = BOARD_W as f32 / UNITS_W;

/// Where the board and its two sockets sit for a platform. GBA keeps the
/// original Slot geometry; a GB/GBC pak board is the cart's own outline with
/// its two big chips (ROM and RAM) replaced by the mGBA and Gambatte sockets.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct BoardLayout {
    pub rest: Placed,
    /// The board's face size in its own logical pixels.
    pub native: (f32, f32),
    /// GBA draws two sockets; a pak board has the chip footprints printed on it.
    pub sockets: bool,
    pub socket_u: [f32; 2],
    pub socket_v: f32,
    pub chip_u: [f32; 2],
    pub chip_v: f32,
    /// The chip's size at zoom 1, how high it hops, and where its shadow
    /// falls (offset from the chip's top-left, in board units) and how big.
    pub chip: (f32, f32),
    pub hop: f32,
    pub shadow_at: (f32, f32),
    pub shadow: (f32, f32),
}

/// Centres of the pak board's two big-chip footprints, in its pixels, and
/// the chip's size (gb_board.svg / gb_chip.svg).
const PAK_FOOTPRINTS: [f32; 2] = [99.0, 219.0];
const PAK_FOOTPRINT_Y: f32 = 213.5;
const PAK_CHIP: (f32, f32) = (70.0, 91.0);
const PAK_REST_H: f32 = 300.0;
const PAK_GAP: f32 = 40.0;
const PAK_LID_W: f32 = 168.0;

pub fn board_layout(platform: Platform) -> BoardLayout {
    if platform == Platform::Gba {
        return BoardLayout {
            rest: Placed { x: board_x(), y: BOARD_Y, w: BOARD_W as f32, h: BOARD_H as f32 },
            native: (BOARD_W as f32, BOARD_H as f32),
            sockets: true,
            socket_u: SOCKET_U,
            socket_v: SOCKET_V,
            chip_u: CHIP_U,
            chip_v: CHIP_V,
            chip: (CHIP_W as f32, CHIP_H as f32),
            hop: HOP_LIFT,
            shadow_at: (19.0, 29.4),
            shadow: (SHADOW_W as f32, SHADOW_H as f32),
        };
    }
    let (cw, ch) = cart_box(platform);
    let h = PAK_REST_H;
    let w = h * cw as f32 / ch as f32;
    let lid = lid_rest(platform);
    let x = lid.x + lid.w + PAK_GAP;
    let unit = |px: f32| px / UNIT_PX;
    let (chip_w, chip_h) = PAK_CHIP;
    BoardLayout {
        rest: Placed { x, y: (480.0 - 120.0 - h) / 2.0 + 30.0, w, h },
        native: (cw as f32, ch as f32),
        sockets: false,
        socket_u: PAK_FOOTPRINTS.map(|x| unit(x - chip_w / 2.0)),
        socket_v: unit(PAK_FOOTPRINT_Y - chip_h / 2.0),
        chip_u: PAK_FOOTPRINTS.map(|x| unit(x - chip_w / 2.0)),
        chip_v: unit(PAK_FOOTPRINT_Y - chip_h / 2.0),
        chip: PAK_CHIP,
        hop: unit(22.0),
        shadow_at: (unit(chip_w / 2.0), unit(chip_h + 4.0)),
        shadow: (SHADOW_W as f32 * chip_w / CHIP_W as f32, SHADOW_H as f32 * 1.5),
    }
}

impl BoardLayout {
    pub fn at(&self, board: Placed, u: f32, v: f32) -> (f32, f32) {
        if self.native == (BOARD_W as f32, BOARD_H as f32) {
            return on_board(board, u, v);
        }
        let zoom = self.zoom(board);
        (board.x + u * UNIT_PX * zoom, board.y + v * UNIT_PX * zoom * self.aspect(board))
    }

    /// Screen pixels per board pixel; sockets and chips scale with it.
    pub fn zoom(&self, board: Placed) -> f32 {
        board.w / self.native.0
    }

    fn aspect(&self, board: Placed) -> f32 {
        (board.h / self.native.1) / self.zoom(board)
    }
}

pub fn board_toward(shelf: Placed, rest: Placed, progress: f32) -> Placed {
    lerp(shelf, rest, lift_of(progress))
}

/// The screen columns the opening picker covers: its lid (tilted) and its
/// board, from the selected cart on the shelf toward where they rest. The
/// shelf keeps the other carts out of them on narrow screens.
pub fn picker_span(platform: Platform, shelf: Placed, progress: f32) -> [f32; 2] {
    let (lid, turn) = lid_toward(shelf, lid_rest(platform), progress);
    let half = (lid.w * turn.cos().abs() + lid.h * turn.sin().abs()) / 2.0;
    let mid = lid.x + lid.w / 2.0;
    let board = board_toward(shelf, board_layout(platform).rest, progress);
    [(mid - half).min(board.x), (mid + half).max(board.x + board.w)]
}

pub fn grown(p: Placed, by: f32) -> Placed {
    Placed {
        x: p.x - by,
        y: p.y - by,
        w: p.w + 2.0 * by,
        h: p.h + 2.0 * by,
    }
}

fn lerp(a: Placed, b: Placed, t: f32) -> Placed {
    Placed {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        w: a.w + (b.w - a.w) * t,
        h: a.h + (b.h - a.h) * t,
    }
}

pub fn padded(face: &CartFace, pad: u32) -> CartFace {
    let (tw, th) = face.texels();
    let mut out = Canvas::new(face.w + 2 * pad, face.h + 2 * pad);
    let x = (out.tw as i32 - tw as i32) / 2;
    let y = (out.th as i32 - th as i32) / 2;
    out.blit_texels(&face.rgba, tw, th, x, y);
    out.into_cart()
}

pub fn socket_face(core: Core) -> CartFace {
    let mut face = svg_face(SOCKET_SVG, SOCKET_W, SOCKET_H);
    if let Some(layout) = name_layout(core, (SOCKET_W - 12) as f32) {
        ink_band(&mut face, 4, 37, &layout, SOCKET_INK, 0.5);
    }
    face
}

/// A core's name silkscreened in an empty pak chip footprint, as a GBA
/// socket shows it.
fn pak_footprint_face(core: Core) -> CartFace {
    let (w, h) = (PAK_CHIP.0 as u32, PAK_CHIP.1 as u32);
    let mut face = Canvas::new(w, h).into_cart();
    if let Some(layout) = name_layout(core, (w - 30) as f32) {
        ink_band(&mut face, (h - 34) / 2, 34, &layout, SOCKET_INK, 0.5);
    }
    face
}

pub fn chip_face(core: Option<Core>) -> CartFace {
    let mut face = svg_face(CHIP_SVG, CHIP_W, CHIP_H);
    if let Some(layout) = core.and_then(|c| name_layout(c, (CHIP_W - 8) as f32)) {
        ink_band(&mut face, 4, 34, &layout, CHIP_INK, 1.0);
    }
    padded(&face, TURN_PAD)
}

fn pak_chip_body() -> CartFace {
    svg_face(GB_CHIP_SVG, PAK_CHIP.0 as u32, PAK_CHIP.1 as u32)
}

/// The big pak chip carrying the core's name, as it sits on a GB/GBC board.
pub fn pak_chip_face(core: Option<Core>) -> CartFace {
    let mut face = pak_chip_body();
    if let Some(layout) = core.and_then(|c| name_layout(c, (PAK_CHIP.0 - 26.0).max(8.0))) {
        ink_band(&mut face, (PAK_CHIP.1 as u32 - 34) / 2, 34, &layout, CHIP_INK, 0.9);
    }
    padded(&face, TURN_PAD)
}

pub fn chip_shadow_face() -> CartFace {
    let (w, h) = (px(SHADOW_W), px(SHADOW_H));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 + 0.5 - w as f32 / 2.0) / (w as f32 / 2.0);
            let dy = (y as f32 + 0.5 - h as f32 / 2.0) / (h as f32 / 2.0);
            let fall = (1.0 - (dx * dx + dy * dy)).max(0.0);
            rgba.extend_from_slice(&[0, 0, 0, (fall * fall * 255.0) as u8]);
        }
    }
    CartFace { rgba, w: SHADOW_W, h: SHADOW_H }
}

fn name_layout(core: Core, max_w: f32) -> Option<text::Layout> {
    let font = text::label_font()?;
    let tracking = |px: f32| (px * 0.10).round();
    let mut px = NAME_PX;
    while px > NAME_MIN_PX && text::line_width(font, core.text(), px, tracking(px)) > max_w {
        px -= 1.0;
    }
    Some(text::Layout {
        lines: vec![core.text().to_string()],
        px,
        tracking: tracking(px),
    })
}

use slot_store::gb::Class;
use slot_store::{Cart, Outline, Platform};

use crate::art;
use crate::bitmaps::{self, Mould, BOARD_H, BOARD_W};
use crate::home::{self, home_mould_of, HomeMould};
use crate::density::{self, px};
// The KONKR core-picker still uses its own BOARD_W/H, which differ from the
// official v1.7 visual bitmap 360x212. Cart layers use the bitmap dimensions.
use crate::shell::{shell_for, Finish, Shell};
use crate::silhouette::{cart_mask, dense_mask, gb_cart_mask, GbShell};
use crate::text;

pub const CART_W: u32 = 360;
pub const CART_H: u32 = (CART_W * SEATED_H + SEATED_W / 2) / SEATED_W;

pub const GB_CART_W: u32 = 312;

pub const GB_CART_H: u32 = (GB_CART_W * GB_SEATED_H + SEATED_W / 2) / SEATED_W;

/// The GBA cart's label box, as drawn in `assets/gba_neutral.png`: x 48..312, y 50..186
/// of 360x212.
pub const fn label_panel(w: u32, h: u32) -> (u32, u32, u32, u32) {
    (
        (w * 133 + 500) / 1000,
        (h * 236 + 500) / 1000,
        (w * 867 + 500) / 1000,
        (h * 877 + 500) / 1000,
    )
}

pub const LABEL_X: u32 = label_panel(CART_W, CART_H).0;
pub const LABEL_Y: u32 = label_panel(CART_W, CART_H).1;
pub const LABEL_W: u32 = label_panel(CART_W, CART_H).2 - LABEL_X;
pub const LABEL_H: u32 = label_panel(CART_W, CART_H).3 - LABEL_Y;

pub const fn gb_label_panel(w: u32, h: u32) -> (u32, u32, u32, u32) {
    (
        (w * 147 + 500) / 1000,
        (h * 291 + 500) / 1000,
        (w * 854 + 500) / 1000,
        (h * 868 + 500) / 1000,
    )
}

pub const GB_LABEL_X: u32 = gb_label_panel(GB_CART_W, GB_CART_H).0;
pub const GB_LABEL_Y: u32 = gb_label_panel(GB_CART_W, GB_CART_H).1;
pub const GB_LABEL_W: u32 = gb_label_panel(GB_CART_W, GB_CART_H).2 - GB_LABEL_X;
pub const GB_LABEL_H: u32 = gb_label_panel(GB_CART_W, GB_CART_H).3 - GB_LABEL_Y;

/// The Game Boy Color pak's label box: exactly the flat label well drawn in
/// `assets/gbc_neutral.png`, x 45..267, y 110..308 of 312x337. It is the same 223x199
/// well as the Game Boy's (x 44..266, y 95..293), 1px right and 15px lower.
pub const GBC_LABEL_X: u32 = 45;
pub const GBC_LABEL_Y: u32 = 110;
pub const GBC_LABEL_W: u32 = 223;
pub const GBC_LABEL_H: u32 = 199;

const PAD: u32 = 10;
const MAX_LINES: usize = 3;
const MIN_PX: f32 = 10.0;

pub struct CartFace {
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

struct Spec {
    w: u32,
    h: u32,
    label: (u32, u32, u32, u32),
    mask: &'static [u8],
    mould: Mould,
    body: Body,
}

/// A cart's shape: the GBA cart, a Game Boy pak, or one of the NES, Super
/// Nintendo and Mega Drive moulds.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Body {
    Gba,
    Gb(GbShell),
    Home(HomeMould),
}

pub fn gb_shell_of(cart: &Cart) -> Option<GbShell> {
    match cart.platform {
        Platform::Gba | Platform::Nes | Platform::Snes | Platform::Md => None,
        Platform::Gb | Platform::Gbc => Some(match cart.shell.map(|c| c.outline) {
            Some(Outline::Notched) => GbShell::Notched,
            Some(Outline::Rounded) => GbShell::Rounded,
            _ => match slot_store::gb::class(&cart.rom) {
                Class::ColourOnly => GbShell::Rounded,
                Class::Original | Class::DualMode => GbShell::Notched,
            },
        }),
    }
}

pub fn body_of(cart: &Cart) -> Body {
    if let Some(m) = home_mould_of(cart) {
        return Body::Home(m);
    }
    match gb_shell_of(cart) {
        None => Body::Gba,
        Some(shell) => Body::Gb(shell),
    }
}

/// The shape a system's carts take when nothing picks another.
pub fn platform_body(platform: Platform) -> Body {
    match platform {
        Platform::Gba => Body::Gba,
        Platform::Gb | Platform::Gbc => Body::Gb(GbShell::Notched),
        Platform::Nes | Platform::Snes | Platform::Md => {
            Body::Home(HomeMould::default_for(platform).unwrap_or(HomeMould::Nes))
        }
    }
}

fn shape_of(cart: &Cart) -> Body {
    body_of(cart)
}

fn spec(shape: Body) -> Spec {
    match shape {
        Body::Home(m) => {
            let (w, h) = m.size();
            Spec { w, h, label: m.art().label, mask: home::mask(m), mould: Mould::Home(m), body: shape }
        }
        Body::Gba => Spec {
            w: CART_W,
            h: CART_H,
            label: (LABEL_X, LABEL_Y, LABEL_W, LABEL_H),
            mask: cart_mask(),
            mould: Mould::Gba,
            body: shape,
        },
        Body::Gb(shell) => Spec {
            w: GB_CART_W,
            h: GB_CART_H,
            label: match shell {
                GbShell::Notched => (GB_LABEL_X, GB_LABEL_Y, GB_LABEL_W, GB_LABEL_H),
                GbShell::Rounded => (GBC_LABEL_X, GBC_LABEL_Y, GBC_LABEL_W, GBC_LABEL_H),
            },
            mask: gb_cart_mask(shell),
            mould: match shell {
                GbShell::Notched => Mould::Notched,
                GbShell::Rounded => Mould::Rounded,
            },
            body: shape,
        },
    }
}

pub const SEATED_W: u32 = 240;
const SEATED_H: u32 = 141;
const GB_SEATED_H: u32 = 259;

pub fn seated_box(platform: Platform) -> (u32, u32) {
    body_seated_box(platform_body(platform))
}

/// The cart's size once it sits in the slot.
pub fn body_seated_box(body: Body) -> (u32, u32) {
    match body {
        Body::Gba => (SEATED_W, SEATED_H),
        Body::Gb(_) => (SEATED_W, GB_SEATED_H),
        Body::Home(m) => {
            // Super Nintendo and Mega Drive carts stay 20% bigger in the
            // slot too, whose recess is drawn to fit them.
            let (w, h) = m.size();
            let sw = (SEATED_W as f32 * seat_grow(body)).round() as u32;
            (sw, (sw * h + w / 2) / w)
        }
    }
}

pub fn cart_box(platform: Platform) -> (u32, u32) {
    body_box(platform_body(platform))
}

pub fn body_box(body: Body) -> (u32, u32) {
    let s = spec(body);
    (s.w, s.h)
}

/// This cart's own size: a home console cart's follows its mould.
pub fn cart_box_for(cart: &Cart) -> (u32, u32) {
    body_box(body_of(cart))
}

/// How much larger than its own box a cart is shown on the shelf and as it
/// travels to the slot: Super Nintendo and Mega Drive carts are drawn 20%
/// bigger (Siham 2026-11-02), every other cart at its own size.
/// EA's tall Mega Drive carts are drawn 20% smaller than that (Siham
/// 2026-11-04): at full size they ran off the top of the screen.
pub fn shelf_grow(body: Body) -> f32 {
    match body {
        Body::Home(HomeMould::MegaDriveEa) => 1.2 * 0.8,
        _ => seat_grow(body),
    }
}

/// How much larger a cart is drawn once seated: SNES and Mega Drive carts,
/// EA's included, fill their slot's recess, which is drawn to fit them.
fn seat_grow(body: Body) -> f32 {
    match body {
        Body::Home(m) if m.system() != home::HomeSystem::Nes => 1.2,
        _ => 1.0,
    }
}

/// A cart's box as the shelf shows it, in the same units as `body_box`.
pub fn shelf_box(body: Body) -> (f32, f32) {
    let (w, h) = body_box(body);
    let k = shelf_grow(body);
    (w as f32 * k, h as f32 * k)
}

pub fn shelf_box_for(cart: &Cart) -> (f32, f32) {
    shelf_box(body_of(cart))
}

pub fn seated_box_for(cart: &Cart) -> (u32, u32) {
    body_seated_box(body_of(cart))
}

pub fn cart_shadow() -> CartFace {
    shadow(spec(Body::Gba))
}

pub fn gb_cart_shadow(shell: GbShell) -> CartFace {
    shadow(spec(Body::Gb(shell)))
}

pub fn home_cart_shadow(m: HomeMould) -> CartFace {
    shadow(spec(Body::Home(m)))
}

fn shadow(s: Spec) -> CartFace {
    let mut rgba = Vec::with_capacity((px(s.w) * px(s.h) * 4) as usize);
    for cover in mask_of(&s).iter() {
        rgba.extend_from_slice(&[0, 0, 0, *cover]);
    }
    CartFace { rgba, w: s.w, h: s.h }
}

/// The outline at the current density.
fn mask_of(s: &Spec) -> std::borrow::Cow<'static, [u8]> {
    if density::is_dense() {
        std::borrow::Cow::Owned(match s.body {
            Body::Home(m) => home::dense_mask(m).to_vec(),
            Body::Gb(shell) => dense_mask(Some(shell)).to_vec(),
            Body::Gba => dense_mask(None).to_vec(),
        })
    } else {
        std::borrow::Cow::Borrowed(s.mask)
    }
}

/// The cart's own label, at its well's size in texels.
fn label_art(cart: &Cart, w: u32, h: u32) -> Option<Vec<u8>> {
    let path = cart.label.as_deref()?;
    match body_of(cart) {
        Body::Home(_) => art::fit_label(path, w, h),
        _ => art::cover(path, w, h),
    }
}

pub fn cart_face(cart: &Cart) -> CartFace {
    let (lw, lh) = label_size(cart);
    let art = label_art(cart, px(lw), px(lh));
    cart_face_with(cart, art)
}

pub fn is_clear(cart: &Cart) -> bool {
    see_through(shell_for(cart).finish, matches!(body_of(cart), Body::Home(_)))
}

/// A glitter GB/GBA shell is clear plastic with flecks; a home cart's (the
/// gold Zelda) is opaque.
fn see_through(finish: Finish, home: bool) -> bool {
    finish == Finish::Translucent || (finish == Finish::Glitter && !home)
}

/// The back plastic at 80% opacity. A GBA cart uses `gba_interior.png` with
/// the game's colour blended onto it by the same overlay as the shells; a pak
/// gets its drawn back shell, the same one the core picker opens onto.
pub fn cart_interior(cart: &Cart) -> CartFace {
    if let Body::Home(_) = body_of(cart) {
        return crate::board::home_shell_face(cart, CLEAR_ALPHA);
    }
    if gb_shell_of(cart).is_some() {
        return crate::board::pak_shell_face(cart, CLEAR_ALPHA);
    }
    let colour = shell_for(cart).colour;
    let mut rgba = bitmaps::dense_gba_interior().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        if px[3] == 0 {
            continue;
        }
        for c in 0..3 {
            px[c] = overlay(px[c], colour[c]);
        }
        px[3] = ((u32::from(px[3]) * u32::from(CLEAR_ALPHA) + 127) / 255) as u8;
    }
    CartFace { rgba, w: BOARD_W, h: BOARD_H }
}

/// The board a clear cart's plastic is drawn over, between the back and front
/// shells: the same drawn board the core picker opens onto, so a clear cart
/// shows on the shelf what it shows when it opens.
pub fn cart_backing(cart: &Cart) -> CartFace {
    if let Body::Home(_) = body_of(cart) {
        return crate::board::home_shelf_board(cart);
    }
    match gb_shell_of(cart) {
        Some(_) => crate::board::pak_shelf_board(cart),
        None => crate::board::gba_shelf_board(cart),
    }
}

/// Cut a face drawn at a pak's size to that pak's outline.
pub(crate) fn cut_to_shell(face: &mut CartFace, shell: GbShell) {
    cut_to_body(face, Body::Gb(shell));
}

/// Cut a face drawn at a cart's size to that cart's outline.
pub(crate) fn cut_to_body(face: &mut CartFace, body: Body) {
    let s = spec(body);
    for (px, cover) in face.rgba.chunks_exact_mut(4).zip(mask_of(&s).iter()) {
        px[3] = ((px[3] as u32 * *cover as u32 + 127) / 255) as u8;
    }
}

/// The label well on the cart's face, in its logical pixels: x, y, w, h.
pub fn label_rect(cart: &Cart) -> (u32, u32, u32, u32) {
    spec(shape_of(cart)).label
}

/// Logical label size; artwork for `cart_face_with` is `px()` of it.
pub fn label_size(cart: &Cart) -> (u32, u32) {
    let (_, _, lw, lh) = spec(shape_of(cart)).label;
    (lw, lh)
}

/// Android bridge: compose the *upstream v1.7* authentic cartridge layers once
/// on the label-render worker, never per GL frame. The upstream native frontend
/// renders interior -> circuit board -> plastic as separate textures; the Android
/// worker precomposes those layers so its existing bounded 42-texture shelf
/// keeps its memory and performance behavior while matching the visible face.
/// Separate animated board/chrome layering remains part of the U1 follow-up.
pub fn cart_face_with_material(cart: &Cart, art: Option<Vec<u8>>) -> CartFace {
    let artwork = art.or_else(|| {
        let (w, h) = label_size(cart);
        label_art(cart, px(w), px(h))
    });
    let plastic = cart_face_with(cart, artwork);
    if !is_clear(cart) {
        return plastic;
    }
    let backing = cart_backing(cart);
    let interior = cart_interior(cart);
    assert_eq!((plastic.w, plastic.h), (backing.w, backing.h));
    assert_eq!((plastic.w, plastic.h), (interior.w, interior.h));
    let mut pixels = vec![0u8; plastic.rgba.len()];
    blend_over(&mut pixels, &interior.rgba);
    blend_over(&mut pixels, &backing.rgba);
    blend_over(&mut pixels, &plastic.rgba);
    CartFace { rgba: pixels, w: plastic.w, h: plastic.h }
}

/// Straight-alpha source-over. Channel math uses integers and is independent
/// of the caller's wallpaper or GL driver; transparent plastic stays transparent.
fn blend_over(dst: &mut [u8], src: &[u8]) {
    assert_eq!(dst.len(), src.len());
    for (back, top) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let a = u32::from(top[3]);
        if a == 0 { continue; }
        let da = u32::from(back[3]);
        let out_a = a * 255 + da * (255 - a);
        if out_a == 0 { continue; }
        for c in 0..3 {
            let n = u32::from(top[c]) * a * 255
                + u32::from(back[c]) * da * (255 - a);
            back[c] = ((n + out_a / 2) / out_a).min(255) as u8;
        }
        back[3] = ((out_a + 127) / 255) as u8;
    }
}

pub fn cart_face_with(cart: &Cart, art: Option<Vec<u8>>) -> CartFace {
    let s = spec(shape_of(cart));
    let mut face = shell_face(&s, &shell_for(cart));
    let label = match art {
        Some(rgba) => rgba,
        None => generated_label(&s, &label_text(cart)),
    };
    paste_label(&s, &mut face, &label);
    if let Body::Home(_) = s.body {
        shade_label_well(&s, &mut face);
    }
    clip_to_silhouette(&s, &mut face);
    face
}

/// A home cart's label sits in a well: the well's top and left edges shade
/// it, as on the support-2D captures.
fn shade_label_well(s: &Spec, face: &mut CartFace) {
    let (lx, ly, lw, lh) = s.label;
    let k = density::scale();
    let (x0, y0, x1, y1) = (px(lx), px(ly), px(lx + lw), px(ly + lh));
    let fw = px(s.w);
    for y in y0..y1.min(px(s.h)) {
        for x in x0..x1.min(fw) {
            let d = ((x - x0).min(y - y0) as f32) / k;
            let shade = 0.28 * (1.0 - d / 6.0).max(0.0);
            if shade <= 0.0 {
                continue;
            }
            let i = ((y * fw + x) * 4) as usize;
            for c in 0..3 {
                face.rgba[i + c] = (face.rgba[i + c] as f32 * (1.0 - shade)).round() as u8;
            }
        }
    }
}

fn clip_to_silhouette(s: &Spec, face: &mut CartFace) {
    for (px, cover) in face.rgba.chunks_exact_mut(4).zip(mask_of(s).iter()) {
        px[3] = ((px[3] as u32 * *cover as u32 + 127) / 255) as u8;
    }
}

pub fn label_colour(title: &str) -> [u8; 3] {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in title.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hsv_to_rgb((h % 360) as f32, 0.52, 0.74)
}

pub fn label_text(cart: &Cart) -> String {
    clean_label(&cart.stem)
}

pub fn clean_label(stem: &str) -> String {
    let mut bare = String::with_capacity(stem.len());
    let mut depth = 0u32;
    for ch in stem.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => bare.push(ch),
            _ => {}
        }
    }

    let mut out = String::with_capacity(bare.len());
    for word in bare.split_whitespace().filter(|w| *w != "-") {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    if out.is_empty() {
        stem.to_string()
    } else {
        out
    }
}

pub fn label_tags(stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0u32;
    let mut cur = String::new();
    for ch in stem.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                if depth == 1 {
                    cur.clear();
                    continue;
                }
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let t = cur.trim();
                    if !t.is_empty() {
                        out.push(t.to_string());
                    }
                    continue;
                }
            }
            _ => {}
        }
        if depth >= 1 {
            cur.push(ch);
        }
    }
    out
}

/// How much of a clear shell is opaque: 80%.
const CLEAR_ALPHA: u8 = 204;

/// The neutral shell's flat plastic: 50% grey, the level that leaves the game's colour as it is.
const NEUTRAL: u8 = 128;

/// Photoshop's Overlay blend of one channel: the neutral shell is the base layer and the game's
/// colour the blend layer. A base of 50% grey gives the colour exactly, a darker base darkens it
/// toward black and a lighter base lightens it toward white, so the shell's highlights, shadows
/// and embossed console name all follow whatever colour the game sets.
fn overlay(base: u8, colour: u8) -> u8 {
    let (base, colour) = (i32::from(base), i32::from(colour));
    let out = if base >= i32::from(NEUTRAL) {
        colour + ((255 - colour) * (base - i32::from(NEUTRAL)) + 63) / 127
    } else {
        colour - (colour * (i32::from(NEUTRAL) - base) + 64) / i32::from(NEUTRAL)
    };
    out.clamp(0, 255) as u8
}

/// The shell in the game's colour at panel density, filtered from its master
/// (`bitmaps::dense_shell`) before the colour goes on, so a 3x master stays
/// sharp on every screen.
fn shell_face(s: &Spec, shell: &Shell) -> CartFace {
    let clear = see_through(shell.finish, matches!(s.mould, Mould::Home(_)));
    let src = bitmaps::dense_shell(s.mould, clear);
    let (tw, th) = (px(s.w), px(s.h));
    assert_eq!(src.len(), (tw * th * 4) as usize, "shell bitmap is not {}x{}", tw, th);
    let mut rgba = src.to_vec();
    // The neutral asset is blended with the game's colour everywhere but the label box, which
    // stays the grey of the asset. A clear shell is its own asset with real transparency: what the
    // compositor drew underneath (the board) shows through and stays put while the shell moves.
    // The label box stays opaque.
    let (lx, ly, lw, lh) = s.label;
    let (lx0, ly0, lx1, ly1) = (px(lx), px(ly), px(lx + lw), px(ly + lh));
    for y in 0..th {
        for x in 0..tw {
            let on_label = (lx0..lx1).contains(&x) && (ly0..ly1).contains(&y);
            let i = ((y * tw + x) * 4) as usize;
            if !on_label {
                for c in 0..3 {
                    rgba[i + c] = overlay(rgba[i + c], shell.colour[c]);
                }
            }
            // The asset's own alpha is kept: its transparent corners and sides hold stray RGB
            // (often white), and only the alpha stops that being drawn.
            let own = u32::from(rgba[i + 3]);
            rgba[i + 3] = if on_label {
                255
            } else if clear {
                ((own * u32::from(CLEAR_ALPHA) + 127) / 255) as u8
            } else {
                own as u8
            };
        }
    }
    if let Mould::Home(m) = s.mould {
        if let Some(decal) = home::dense_decal(m) {
            blend_over(&mut rgba, &decal);
        }
    }
    if shell.finish == Finish::Glitter {
        // The flecks keep their logical size and place at any density.
        let k = density::scale();
        for y in 0..th {
            for x in 0..tw {
                if !fleck((x as f32 / k) as u32, (y as f32 / k) as u32) {
                    continue;
                }
                let i = ((y * tw + x) * 4) as usize;
                if rgba[i + 3] == 0 {
                    continue;
                }
                let body = [rgba[i], rgba[i + 1], rgba[i + 2]];
                rgba[i..i + 3].copy_from_slice(&lerp(body, [0xff; 3], 100, 255));
                rgba[i + 3] = 255;
            }
        }
    }
    CartFace {
        rgba,
        w: s.w,
        h: s.h,
    }
}

fn fleck(x: u32, y: u32) -> bool {
    let mut h = x.wrapping_mul(0x9e37_79b9) ^ y.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h.is_multiple_of(100)
}

fn lerp(a: [u8; 3], b: [u8; 3], num: u32, den: u32) -> [u8; 3] {
    let mut out = [0u8; 3];
    for c in 0..3 {
        out[c] = ((a[c] as u32 * (den - num) + b[c] as u32 * num) / den) as u8;
    }
    out
}

fn generated_label(s: &Spec, title: &str) -> Vec<u8> {
    let (_, _, lw, lh) = s.label;
    let bg = label_colour(title);
    let (tw, th) = (px(lw), px(lh));
    let mut rgba = Vec::with_capacity((tw * th * 4) as usize);
    for _ in 0..tw * th {
        rgba.extend_from_slice(&[bg[0], bg[1], bg[2], 255]);
    }

    if let Some(font) = text::label_font() {
        let (max_px, max_h) = match s.mould {
            Mould::Gba => (lh as f32 / (MAX_LINES as f32 * 1.36), f32::INFINITY),
            Mould::Notched | Mould::Rounded | Mould::Home(_) => {
                let room = (lh - 2 * PAD) as f32;
                (room / MAX_LINES as f32, room)
            }
        };
        let layout = text::fit_box(
            font,
            title,
            (lw - 2 * PAD) as f32,
            max_h,
            MAX_LINES,
            max_px,
            MIN_PX,
        );
        text::draw_centred(&mut rgba, tw, th, &density::scaled(&layout), ink(bg));
    }
    rgba
}

fn ink(bg: [u8; 3]) -> [u8; 3] {
    let luma = 0.2126 * bg[0] as f32 + 0.7152 * bg[1] as f32 + 0.0722 * bg[2] as f32;
    if luma > 140.0 {
        [0x1a, 0x18, 0x16]
    } else {
        [0xf4, 0xf1, 0xea]
    }
}

fn paste_label(s: &Spec, face: &mut CartFace, label: &[u8]) {
    let (lx, ly, lw, lh) = s.label;
    let (lx, ly, lw, lh) = (px(lx), px(ly), px(lw), px(lh));
    let (fw, fh) = (px(s.w), px(s.h));
    if label.len() < (lw * lh * 4) as usize {
        return;
    }
    for y in 0..lh.min(fh.saturating_sub(ly)) {
        for x in 0..lw.min(fw.saturating_sub(lx)) {
            let src = ((y * lw + x) * 4) as usize;
            let a = label[src + 3] as u32;
            if a == 0 {
                continue;
            }
            let d = (((y + ly) * fw + x + lx) * 4) as usize;
            for c in 0..3 {
                face.rgba[d + c] =
                    ((label[src + c] as u32 * a + face.rgba[d + c] as u32 * (255 - a) + 127) / 255)
                        as u8;
            }
        }
    }
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    ]
}

#[cfg(test)]
mod dense_tests {
    use super::*;

    /// Every shell is coloured at the panel's own density, from its master.
    #[test]
    fn shells_are_built_at_every_density() {
        let bodies = [Body::Gba, Body::Gb(GbShell::Notched), Body::Gb(GbShell::Rounded)]
            .into_iter()
            .chain(HomeMould::ALL.into_iter().map(Body::Home));
        for body in bodies {
            for k in [1.0, 4.0 / 3.0, 2.25, 3.0] {
                density::with_scale(k, || {
                    let s = spec(body);
                    for finish in [Finish::Solid, Finish::Translucent, Finish::Glitter] {
                        let face = shell_face(&s, &Shell { colour: [200, 40, 40], finish });
                        assert_eq!(face.rgba.len(), (px(s.w) * px(s.h) * 4) as usize, "{body:?} at {k}");
                        assert_eq!(mask_of(&s).len(), (px(s.w) * px(s.h)) as usize, "{body:?} at {k}");
                    }
                });
            }
        }
    }
}

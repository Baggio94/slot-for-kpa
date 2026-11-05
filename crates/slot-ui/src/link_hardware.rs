//! Multiplayer artwork uses the same family and material as the inserted cartridge.
use crate::{bitmaps, density, native_assets, CartFace, Face};

const CABLES: [&[u8]; bitmaps::CONSOLE_SLOTS] = [
    include_bytes!("../assets/link/fc.png"),
    include_bytes!("../assets/link/nes.png"),
    include_bytes!("../assets/link/snes.png"),
    include_bytes!("../assets/link/md.png"),
    include_bytes!("../assets/link/gb.png"),
    include_bytes!("../assets/link/gbc.png"),
    include_bytes!("../assets/link/gba.png"),
];
const RFU: &[u8] = include_bytes!("../assets/link/rfu.png");
pub const BAR_Y: f32 = slot_gfx::OUT_H as f32 - bitmaps::SLOT_H as f32;
const SOCKET_H: u32 = 18;
const SOCKET_PROJECTION: f32 = 0.70;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hardware {
    family: usize,
    theme: usize,
    pub wireless: bool,
}

impl Hardware {
    pub fn new(family: usize, theme: usize, wireless: bool) -> Self {
        assert!(family < CABLES.len() && theme < bitmaps::BAR_THEMES);
        Self {
            family,
            theme,
            wireless: wireless && family == 6,
        }
    }

    pub fn dual(self) -> bool {
        matches!(self.family, 1..=3)
    }
    pub fn side_exit(self) -> bool {
        self.family == 0
    }
    pub fn dark_ink(self) -> bool {
        bitmaps::console_slot_takes_dark_ink(self.family, self.theme)
    }
    /// Translate both endpoints by the old recess depth, preserving the travel.
    pub fn travel_offset(self) -> f32 {
        17.0 + [10.3, 10.0, 7.4, 10.0, 7.3, 6.0, 7.7][self.family] + 8.5
    }
    pub fn shoulder(self) -> f32 {
        if self.wireless {
            127.0
        } else {
            [218., 218., 218., 218., 220., 220., 219.][self.family]
        }
    }
    pub fn offset(self, join: bool) -> f32 {
        if !self.dual() {
            0.0
        } else if join {
            62.0
        } else {
            -62.0
        }
    }
    fn half_width(self) -> f32 {
        [39.85, 28.35, 38.85, 29.65, 23.45, 14.50, 19.45][self.family]
    }
    fn floor(self) -> f32 {
        [15.6, 15.6, 12.8, 14.7, 12.4, 11.2, 13.4][self.family]
    }
}

// All faces stay in Slot's logical units. Each 3x master has three columns:
// Host grip, Join grip, metal shank. The next row holds the three socket themes.
// The RFU master also has the two finishes of the left release button.
pub struct HardwareArt {
    pub wall: CartFace,
    pub front: CartFace,
    pub socket: CartFace,
    pub rim: CartFace,
    pub contact: CartFace,
    pub grips: [CartFace; 2],
    pub nose: CartFace,
    pub buttons: Option<[[CartFace; 2]; 2]>,
}

fn opening_path(half: f32, floor: f32, radius: f32) -> String {
    format!(
        "M{} -4 V{} A{radius} {radius} 0 0 0 {} {floor} H{} A{radius} {radius} 0 0 0 {half} {} V-4",
        -half,
        floor - radius,
        -half + radius,
        half - radius,
        floor - radius
    )
}

fn colour(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

fn blend(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

fn gradient(id: &str, stops: &[(f32, [u8; 3])]) -> String {
    let mut svg = format!(r#"<linearGradient id="{id}" x2="0" y2="1">"#);
    for &(offset, rgb) in stops {
        svg += &format!(r#"<stop offset="{offset}" stop-color="{}"/>"#, colour(rgb));
    }
    svg + "</linearGradient>"
}

fn socket_faces(key: Hardware, wall: &CartFace) -> (CartFace, CartFace, CartFace) {
    let width = if key.wireless {
        168
    } else if key.side_exit() {
        110
    } else {
        100
    };
    let half = key.half_width();
    let floor = key.floor();
    let radius = if key.family == 6 { 2.3 } else { 1.7 };
    let opening = opening_path(half, floor, radius);
    let mut mouths = format!(r#"<path d="{opening}"/>"#);
    if key.wireless {
        let catch = opening_path(4.825, 5.7, 1.1);
        for offset in [-74.25, 74.25] {
            mouths += &format!(r#"<path d="{catch}" transform="translate({offset} 0)"/>"#);
        }
    }
    let guide = if key.family == 2 {
        format!(
            r##"<path d="M3.45 {floor} V1.32 Q3.45 1 3.77 1 H4.93 Q5.25 1 5.25 1.32 V{floor}Z" fill="#535056"/>"##
        )
    } else {
        String::new()
    };
    let (tw, th) = wall.texels();
    let sample = (((th / 2) * tw + tw / 2) * 4) as usize;
    let base = [
        wall.rgba[sample],
        wall.rgba[sample + 1],
        wall.rgba[sample + 2],
    ];
    let dark = base.iter().map(|&v| v as u32).sum::<u32>() < 270;
    let high = if dark {
        blend(base, [212, 208, 216], 0.30)
    } else {
        blend(base, [242, 237, 234], 0.39)
    };
    let face = blend([53, 52, 56], base, 0.105);
    let rim = match (key.family, key.theme) {
        (0, 0) => [
            [196, 174, 135],
            [147, 119, 89],
            [100, 80, 64],
            [199, 172, 135],
        ],
        (0, 1) => [[136, 132, 124], [97, 93, 89], [56, 53, 54], [140, 134, 122]],
        (0, _) => [
            [212, 202, 182],
            [178, 164, 141],
            [123, 114, 102],
            [211, 197, 177],
        ],
        (4..=6, _) => [
            [180, 179, 176],
            [116, 116, 121],
            [68, 67, 72],
            [158, 157, 160],
        ],
        _ => [[119, 116, 121], [81, 78, 82], [51, 50, 53], [115, 111, 115]],
    };
    let mut defs = gradient(
        "outer",
        &[
            (0., blend(base, [13, 12, 16], 0.43)),
            (0.4, blend(base, [18, 17, 22], 0.40)),
            (0.75, high),
            (1., high),
        ],
    );
    defs += &gradient(
        "collar",
        &[
            (0., blend(face, [16, 16, 21], 0.30)),
            (0.24, face),
            (0.77, blend(face, [76, 72, 80], 0.13)),
            (1., blend(face, base, 0.17)),
        ],
    );
    defs += &gradient(
        "rim",
        &[(0., rim[0]), (0.2, rim[1]), (0.76, rim[2]), (1., rim[3])],
    );
    defs += &gradient(
        "well",
        &[(0., [9, 9, 12]), (0.48, [20, 20, 24]), (1., [39, 37, 42])],
    );
    defs += &format!(
        r##"<g id="mouth">{mouths}</g><clipPath id="inside"><use href="#mouth"/></clipPath><mask id="outside" maskUnits="userSpaceOnUse" x="-130" y="-4" width="260" height="30"><rect x="-130" y="-4" width="260" height="30" fill="white"/><use href="#mouth" fill="black"/></mask>"##
    );
    let mut border = String::new();
    for (stroke, thickness) in [
        ("url(#outer)", 5.8),
        ("url(#collar)", 4.15),
        ("#29282e", 2.65),
        ("url(#rim)", 1.6),
    ] {
        border += &format!(
            r##"<use href="#mouth" fill="none" stroke="{stroke}" stroke-width="{thickness}"/>"##
        );
    }
    let lift = [8.6, 8.7, 7.3, 8.1, 7.0, 6.4, 7.45][key.family];
    let pins: Vec<(f32, f32)> = match key.family {
        0 => [(-3.45, 7), (3.6, 8)]
            .into_iter()
            .flat_map(|(y, n)| (0..n).map(move |i| ((i as f32 - (n - 1) as f32 / 2.) * 9.3, y)))
            .collect(),
        1 => [-18., -7., 4.]
            .into_iter()
            .map(|x| (x, -3.))
            .chain([-18., -7., 4., 15.].into_iter().map(|x| (x, 4.)))
            .collect(),
        2 => [-31., -21., -11., -1., 12., 22., 32.]
            .into_iter()
            .map(|x| (x, 0.))
            .collect(),
        3 => [-20., -10., 0., 10., 20.]
            .into_iter()
            .map(|x| (x, -3.5))
            .chain([-15., -5., 5., 15.].into_iter().map(|x| (x, 3.5)))
            .collect(),
        4 => [-2.3, 2.8]
            .into_iter()
            .flat_map(|y| [-11., 0., 11.].map(|x| (x, y)))
            .collect(),
        5 => [-2., 2.]
            .into_iter()
            .flat_map(|y| [-7., 0., 7.].map(|x| (x, y)))
            .collect(),
        _ => [-2.25, 2.35]
            .into_iter()
            .flat_map(|y| [-9., 0., 9.].map(|x| (x, y)))
            .collect(),
    };
    let mut contacts = format!(r#"<g transform="translate(0 {lift})">"#);
    for (x, y) in pins {
        if key.family >= 4 {
            contacts += &format!(
                r##"<rect x="{}" y="{}" width="2.3" height="2.1" rx=".35" fill="#917647"/><rect x="{}" y="{}" width="2.1" height="1.22" rx=".28" fill="#c1a374"/>"##,
                x - 1.15,
                y - 0.9,
                x - 1.05,
                y - 0.88
            );
        } else {
            let metal = if key.side_exit() {
                "#aca797"
            } else {
                "#949391"
            };
            contacts += &format!(
                r##"<circle cx="{x}" cy="{}" r="1.72" fill="#101012"/><circle cx="{x}" cy="{y}" r="1.4" fill="{metal}"/><circle cx="{}" cy="{}" r=".59" fill="#d8d5c9"/>"##,
                y + 0.53,
                x - 0.35,
                y - 0.4
            );
        }
    }
    contacts += "</g>";
    let render = |content: String| {
        crate::link_art::svg_face(
            &format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{SOCKET_H}"><defs>{defs}</defs><g transform="translate({} 0) scale(1 {SOCKET_PROJECTION})" stroke-linejoin="round">{content}</g></svg>"#,
                width as f32 / 2.
            ),
            width,
            SOCKET_H,
        )
    };
    let socket = render(format!(
        r##"{border}<use href="#mouth" fill="url(#well)"/>{contacts}{guide}"##
    ));
    let aperture = render(r##"<use href="#mouth" fill="white"/>"##.into());
    // The narrow inner shadow belongs to the fixed lip. The centre of the
    // inserted shank receives exactly the same pixels as the exposed shank.
    let rim = render(format!(
        r##"<g mask="url(#outside)">{border}</g><g clip-path="url(#inside)" fill="none" stroke="#131313"><use href="#mouth" stroke-width="4.4" opacity=".07"/><use href="#mouth" stroke-width="2" opacity=".16"/></g>{guide}"##
    ));
    (socket, aperture, rim)
}

fn perforated_front(wall: &CartFace, aperture: &CartFace, key: Hardware) -> CartFace {
    let mut front = CartFace {
        w: wall.w,
        h: wall.h,
        rgba: wall.rgba.clone(),
    };
    let (tw, th) = front.texels();
    let (aw, ah) = aperture.texels();
    let centre = wall.w as f32 / 2.;
    let offsets: &[f32] = if key.dual() { &[-62., 62.] } else { &[0.] };
    for offset in offsets {
        let start = ((centre + offset - aperture.w as f32 / 2.) * density::scale()).round() as i32;
        for y in 0..ah.min(th) {
            for x in 0..aw {
                let dx = start + x as i32;
                if dx < 0 || dx >= tw as i32 {
                    continue;
                }
                let dst = ((y * tw + dx as u32) * 4 + 3) as usize;
                let src = ((y * aw + x) * 4 + 3) as usize;
                front.rgba[dst] = 255 - aperture.rgba[src];
            }
        }
    }
    front
}

fn contact_shadow(key: Hardware, width: u32) -> CartFace {
    let radius = if key.wireless {
        104.
    } else {
        key.half_width() + 10.
    };
    crate::link_art::svg_face(
        &format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="2"><defs><radialGradient id="contact"><stop offset="0" stop-color="#101010" stop-opacity=".38"/><stop offset=".67" stop-color="#101010" stop-opacity=".30"/><stop offset=".90" stop-color="#101010" stop-opacity=".14"/><stop offset="1" stop-color="#101010" stop-opacity="0"/></radialGradient></defs><ellipse cx="{}" cy="0" rx="{radius}" ry="1.9" fill="url(#contact)"/></svg>"##,
            width as f32 / 2.
        ),
        width,
        2,
    )
}

fn bar(key: Hardware) -> CartFace {
    let (_, master) = bitmaps::home_slot_png(key.family, key.theme);
    let (h, w) = (bitmaps::SLOT_H, crate::out_w());
    let (tw, th, sw) = (density::px(w), density::px(h), density::px(bitmaps::SLOT_W));
    let source = native_assets::png_at(master, sw, th);
    let guard = density::px(158);
    let span = density::px(112);
    let wing = |d: u32| {
        if d <= guard {
            return d;
        }
        let n = (d - guard) % (2 * span);
        guard - n.min(2 * span - n)
    };
    let columns: Vec<_> = (0..tw)
        .map(|x| {
            let t = ((x as f32 / tw as f32 - 0.44) / 0.12).clamp(0.0, 1.0);
            (wing(x), sw - 1 - wing(tw - 1 - x), t * t * (3.0 - 2.0 * t))
        })
        .collect();
    let mut rgba = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        for &(l, r, t) in &columns {
            let a = ((y * sw + l) * 4) as usize;
            let b = ((y * sw + r) * 4) as usize;
            for c in 0..3 {
                rgba.push(
                    (source[a + c] as f32 + (source[b + c] as f32 - source[a + c] as f32) * t)
                        .round() as u8,
                );
            }
            rgba.push(255);
        }
    }
    let mut face = CartFace { w, h, rgba };
    if key.dual() {
        let ink = if key.dark_ink() {
            [55, 52, 56]
        } else {
            [225, 221, 217]
        };
        for (dx, label) in [(-115.0, "1"), (115.0, "2")] {
            crate::link_art::port_number(&mut face, w as f32 / 2.0 + dx, label, ink);
        }
    }
    face
}

pub fn hardware_art(key: Hardware) -> HardwareArt {
    let (bytes, w, h) = if key.wireless {
        (RFU, 240, 151)
    } else if key.side_exit() {
        (CABLES[key.family], 244, 244)
    } else {
        (CABLES[key.family], 120, 244)
    };
    let (pixels, aw, _) = crate::decode_png(bytes).expect("bundled link artwork");
    let cell = |x: u32, y: u32, w: u32, h: u32| {
        let (sw, sh) = (3 * w, 3 * h);
        let mut src = Vec::with_capacity((sw * sh * 4) as usize);
        for row in 3 * y..3 * y + sh {
            let off = ((row * aw + x * 3) * 4) as usize;
            src.extend_from_slice(&pixels[off..off + (sw * 4) as usize]);
        }
        CartFace {
            w,
            h,
            rgba: native_assets::resample(&src, sw, sh, density::px(w), density::px(h)),
        }
    };
    let wall = bar(key);
    let (socket, aperture, rim) = socket_faces(key, &wall);
    let front = perforated_front(&wall, &aperture, key);
    let contact = contact_shadow(key, w);
    let grips = [cell(0, 0, w, h), cell(w, 0, w, h)].map(|mut grip| {
        let (tw, _) = grip.texels();
        // The moulding's painted outline ends at its physical shoulder.
        for (row, pixels) in grip.rgba.chunks_exact_mut(tw as usize * 4).enumerate() {
            let coverage = (key.shoulder() * density::scale() - row as f32).clamp(0., 1.);
            for pixel in pixels.chunks_exact_mut(4) {
                pixel[3] = (pixel[3] as f32 * coverage).round() as u8;
            }
        }
        grip
    });
    let buttons = key.wireless.then(|| {
        [0, 1].map(|side| {
            let left = cell(side * 40, h + 32, 40, 40);
            let right = crate::link_art::mirror(&left);
            [left, right]
        })
    });
    HardwareArt {
        wall,
        front,
        socket,
        rim,
        contact,
        grips,
        nose: cell(2 * w, 0, w, h),
        buttons,
    }
}

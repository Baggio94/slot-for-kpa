//! Multiplayer's LAN / WAN switch: a hardware slide switch in the housing
//! palette, its knob sliding from a house (LAN) to a globe (WAN) with
//! L1 / R1. No words (Siham 2026-11-06).

use crate::density;
use crate::plate::UndoFace;
use crate::slot_chrome::theme;

/// The recessed track, logical px.
pub const NET_SWITCH_W: u32 = 92;
pub const NET_SWITCH_H: u32 = 38;
/// The raised knob, and its left edge in the track at LAN and at WAN.
pub const NET_KNOB_W: u32 = 42;
pub const NET_KNOB_H: u32 = 32;
pub const NET_KNOB_X: [f32; 2] = [3.0, 47.0];
pub const NET_KNOB_Y: f32 = 3.0;
/// An icon (24 x 24) on the knob.
pub const NET_ICON: u32 = 24;
pub const NET_ICON_X: f32 = 9.0;
pub const NET_ICON_Y: f32 = 4.0;

const LIT: &str = "#f6f4ef";
const DIM: &str = "#6c6c76";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetPart {
    /// The track with both icons engraved (the knob covers one).
    Track,
    Knob,
    /// The lit icons the knob carries.
    House,
    Globe,
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn lift(c: [u8; 3], by: i16) -> [u8; 3] {
    c.map(|v| (v as i16 + by).clamp(0, 255) as u8)
}

fn house(ink: &str) -> String {
    format!(
        "<path d='M3 11.2 L12 3.5 L21 11.2' fill='none' stroke='{ink}' stroke-width='2.4' \
         stroke-linecap='round' stroke-linejoin='round'/>\
         <path d='M5.8 9.6 V20 H18.2 V9.6' fill='none' stroke='{ink}' stroke-width='2.4' \
         stroke-linejoin='round'/>\
         <rect x='10.2' y='14' width='3.6' height='6' rx='0.8' fill='{ink}'/>"
    )
}

fn globe(ink: &str) -> String {
    format!(
        "<circle cx='12' cy='12' r='8.6' fill='none' stroke='{ink}' stroke-width='2.2'/>\
         <ellipse cx='12' cy='12' rx='3.7' ry='8.6' fill='none' stroke='{ink}' stroke-width='2'/>\
         <path d='M3.6 9 H20.4 M3.6 15 H20.4' stroke='{ink}' stroke-width='2' fill='none'/>"
    )
}

fn svg(part: NetPart) -> (String, u32, u32) {
    let t = theme();
    let open = |w: u32, h: u32| {
        format!("<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}' viewBox='0 0 {w} {h}'>")
    };
    match part {
        NetPart::Track => {
            let (w, h) = (NET_SWITCH_W, NET_SWITCH_H);
            let body = format!(
                "{}<defs><linearGradient id='i' x1='0' y1='0' x2='0' y2='1'>\
                 <stop offset='0' stop-color='#000' stop-opacity='.55'/>\
                 <stop offset='.35' stop-color='#000' stop-opacity='0'/></linearGradient></defs>\
                 <rect width='{w}' height='{h}' rx='19' fill='{}'/>\
                 <rect width='{w}' height='{h}' rx='19' fill='url(#i)'/>\
                 <rect x='.5' y='.5' width='{}' height='{}' rx='18.5' fill='none' stroke='{}'/>\
                 <g transform='translate(12 7)'>{}</g><g transform='translate(56 7)'>{}</g></svg>",
                open(w, h), hex(t.recess), w - 1, h - 1, hex(lift(t.recess, 19)),
                house(DIM), globe(DIM),
            );
            (body, w, h)
        }
        NetPart::Knob => {
            let (w, h) = (NET_KNOB_W, NET_KNOB_H);
            let body = format!(
                "{}<defs><linearGradient id='k' x1='0' y1='0' x2='0' y2='1'>\
                 <stop offset='0' stop-color='{}'/><stop offset='1' stop-color='{}'/>\
                 </linearGradient></defs>\
                 <rect width='{w}' height='{h}' rx='16' fill='url(#k)'/>\
                 <rect x='.5' y='.5' width='{}' height='{}' rx='15.5' fill='none' stroke='{}'/>\
                 <path d='M10 2 H32' stroke='#fff' stroke-opacity='.14' stroke-width='1.5' \
                 stroke-linecap='round'/></svg>",
                open(w, h), hex(lift(t.edge, 13)), hex(lift(t.edge, -23)),
                w - 1, h - 1, hex(lift(t.edge, 18)),
            );
            (body, w, h)
        }
        NetPart::House | NetPart::Globe => {
            let icon = if part == NetPart::House { house(LIT) } else { globe(LIT) };
            (format!("{}{icon}</svg>", open(NET_ICON, NET_ICON)), NET_ICON, NET_ICON)
        }
    }
}

/// One part of the switch, at the panel's density.
pub fn net_switch_face(part: NetPart) -> UndoFace {
    let (source, w, h) = svg(part);
    let (tw, th) = (density::px(w), density::px(h));
    let rgba = crate::art::render_svg(&source, tw, th)
        .unwrap_or_else(|| vec![0; (tw * th * 4) as usize]);
    UndoFace { rgba, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_renders_at_its_size_with_ink() {
        for part in [NetPart::Track, NetPart::Knob, NetPart::House, NetPart::Globe] {
            let face = net_switch_face(part);
            assert_eq!(face.rgba.len() as u32, density::px(face.w) * density::px(face.h) * 4);
            assert!(face.rgba.chunks_exact(4).any(|p| p[3] > 200), "{part:?} drew nothing");
        }
    }

    #[test]
    fn the_knob_fits_the_track_at_both_ends() {
        for x in NET_KNOB_X {
            assert!(x >= 0.0 && x + NET_KNOB_W as f32 <= NET_SWITCH_W as f32);
        }
        assert!(NET_KNOB_Y + NET_KNOB_H as f32 <= NET_SWITCH_H as f32);
    }
}

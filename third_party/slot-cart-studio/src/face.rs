//! A cart face for a label held in memory, drawn by slot's own `cart_face`.

use std::path::Path;

use slot_store::Platform;

use crate::{art, cart};

/// The platform a card folder names. It picks the GBA cart or a Game Boy pak; which pak, and
/// its plastic, is the header's to say. Anything unrecognised is GBA, which is the folder the page falls back to
/// as well.
pub fn platform_of(dir: &str) -> Platform {
    match dir {
        "GB" => Platform::Gb,
        "GBC" => Platform::Gbc,
        _ => Platform::Gba,
    }
}

/// Slot's `cart_face` reads a cart's label through `art::cover` and its pak's header through
/// `gb`. Both answer from a stash here, so the cart names the stash keys where file paths
/// would go. The title is left empty because slot's generated label is titled from the stem, not
/// the header. `shell` is the choice empty for Automatic.
fn draw(
    platform: Platform,
    code: &str,
    head: &[u8],
    shell: &str,
    stem: &str,
    key: &Path,
) -> Vec<u8> {
    slot_store::gb::with_header(head, |rom| {
        cart::cart_face(&slot_store::Cart {
            platform,
            stem: stem.to_string(),
            rom: rom.to_path_buf(),
            label: Some(key.to_path_buf()),
            title: String::new(),
            code: code.to_string(),
            shell: slot_store::ShellChoice::parse(shell),
        })
        .rgba
    })
}

/// PNG bytes as a card holds them. Bytes that don't decode fall back to slot's generated label,
/// as they do on the device.
pub fn from_png(
    png: &[u8],
    platform: Platform,
    code: &str,
    head: &[u8],
    shell: &str,
    stem: &str,
) -> Vec<u8> {
    art::with_png(png, |key| draw(platform, code, head, shell, stem, key))
}

/// Straight RGBA, `w` by `h`: a label composed in memory, shown without a PNG round trip.
#[allow(clippy::too_many_arguments)]
pub fn from_rgba(
    rgba: &[u8],
    w: u32,
    h: u32,
    platform: Platform,
    code: &str,
    head: &[u8],
    shell: &str,
    stem: &str,
) -> Vec<u8> {
    art::with_rgba(rgba, w, h, |key| {
        draw(platform, code, head, shell, stem, key)
    })
}

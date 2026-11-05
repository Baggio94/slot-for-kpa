#!/usr/bin/env python3
"""Teach the staged Cart Studio the NES, Super Nintendo and Mega Drive carts.

Runs on the CI copy of third_party/slot-cart-studio, next to the pinned
upstream slot checkout, before wasm-pack. The vendored sources stay exact:
every change is a checked replacement that fails loudly if upstream moved.

- src/konkr_home.rs (from android/studio/home/) draws home carts.
- slot-ui's own home_art.rs (moulds, wells, automatic shells) is compiled in
  by #[path], so Studio and the device share one table.
- lib.rs routes the platforms NES, SNES and MD (optionally NES:<mould>) there.
- label.rs makes a label at a home well's shape and shows it whole.
"""
from pathlib import Path
import argparse
import shutil


def checked_replace(text, old, new):
    if text.count(old) != 1:
        raise RuntimeError("Official Studio changed; review KONKR home carts: " + old[:90])
    return text.replace(old, new)


LABEL_EXT = '''

// KONKR: a home cart's label, made at its well's shape (konkr_home).
impl Label {
    pub fn from_png_at(png: &[u8], deep: [u8; 3], (lw, lh): (u32, u32), band: Option<Band>) -> Option<Label> {
        let (logo, w, h) = art::decode(png)?;
        let rgba = compose(&logo, w, h, lw, lh, deep, pale_for(deep), band);
        Some(Label { logo: Some((logo, w, h)), lw, lh, deep, band, rgba })
    }

    pub fn full_at(png: &[u8], (lw, lh): (u32, u32)) -> Option<Label> {
        let (art, w, h) = art::decode(png)?;
        let mut rgba = art::cover_rgba(&art, w, h, lw, lh)?;
        for p in rgba.chunks_exact_mut(4) {
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = ((*c as u32 * a + 127) / 255) as u8;
            }
            p[3] = 255;
        }
        Some(Label { logo: None, lw, lh, deep: [0; 3], band: None, rgba })
    }

    pub fn konkr_parts(&self) -> (&[u8], u32, u32) {
        (&self.rgba, self.lw, self.lh)
    }
}
'''


def stage(root: Path, studio: Path):
    src = studio / "src"
    shutil.copy2(root / "android/studio/home/konkr_home.rs", src / "konkr_home.rs")
    # The #[path] below is relative to src/, three levels under the repo root
    # both in third_party/ and in the CI's studio-upstream/ copy.
    assert (src / "../../../crates/slot-ui/src/home_art.rs").resolve().is_file()

    lib = (src / "lib.rs").read_text("utf-8")
    lib = checked_replace(lib, "mod art;\n", """mod art;
#[allow(dead_code)]
#[rustfmt::skip]
#[path = "../../../crates/slot-ui/src/home_art.rs"]
mod home_art;
mod konkr_home;
""")
    lib = checked_replace(lib,
        "    let (w, h) = cart::cart_box(face::platform_of(platform));",
        "    let (w, h) = konkr_home::cart_size(platform)\n"
        "        .unwrap_or_else(|| cart::cart_box(face::platform_of(platform)));")
    lib = checked_replace(lib,
        "    face::from_png(png, face::platform_of(platform), code, head, shell, stem)",
        "    if let Some(face) = konkr_home::existing_face(png, platform, head, shell, stem) {\n"
        "        return face;\n"
        "    }\n"
        "    face::from_png(png, face::platform_of(platform), code, head, shell, stem)")
    lib = checked_replace(lib,
        '            return Err(JsError::new("a ground colour needs three channels"));\n        }\n',
        '''            return Err(JsError::new("a ground colour needs three channels"));
        }
        if let Some(size) = konkr_home::label_size(platform) {
            return label::Label::from_png_at(
                logo_png,
                [deep[0], deep[1], deep[2]],
                size,
                label::Band::parse(band_edge, band_size, band_colour),
            )
            .map(JsLabel)
            .ok_or_else(|| JsError::new("not a PNG this studio can read"));
        }
''')
    lib = checked_replace(lib,
        "        label::Label::full(png, face::platform_of(platform))",
        "        match konkr_home::label_size(platform) {\n"
        "            Some(size) => label::Label::full_at(png, size),\n"
        "            None => label::Label::full(png, face::platform_of(platform)),\n"
        "        }")
    lib = checked_replace(lib,
        "        self.0\n            .face(face::platform_of(platform), code, head, shell, stem)",
        "        konkr_home::label_face(&self.0, platform, head, shell, stem).unwrap_or_else(|| {\n"
        "            self.0.face(face::platform_of(platform), code, head, shell, stem)\n"
        "        })")
    lib = checked_replace(lib,
        "    choice::layered_cart_shells(system, labels)\n",
        "    konkr_home::layered(choice::layered_cart_shells(system, labels), system, labels)\n")
    lib += '''
/// KONKR: the shell slot draws a NES, Super Nintendo or Mega Drive cart in
/// with no choice made, from its name and header; empty for other carts.
#[wasm_bindgen]
pub fn home_auto_shell(platform: &str, stem: &str, head: &[u8]) -> String {
    konkr_home::auto_value(platform, stem, head)
}

/// KONKR: the mould a home cart is drawn with, which shapes its label.
#[wasm_bindgen]
pub fn home_outline(platform: &str, stem: &str, head: &[u8], shell: &str) -> String {
    konkr_home::outline(platform, stem, head, shell)
}
'''
    (src / "lib.rs").write_text(lib, encoding="utf-8")

    label = (src / "label.rs").read_text("utf-8")
    label = checked_replace(label,
        "fn shown(lw: u32, lh: u32) -> (u32, u32, u32, u32) {\n",
        "fn shown(lw: u32, lh: u32) -> (u32, u32, u32, u32) {\n"
        "    if crate::konkr_home::is_home_label(lw, lh) {\n"
        "        return (0, 0, lw, lh);\n"
        "    }\n")
    label += LABEL_EXT
    (src / "label.rs").write_text(label, encoding="utf-8")
    print("KONKR home carts staged into", studio)


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--source", default=".")
    ap.add_argument("--studio", required=True)
    args = ap.parse_args()
    stage(Path(args.source), Path(args.studio))

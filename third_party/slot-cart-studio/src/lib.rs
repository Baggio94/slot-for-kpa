//! slot cart studio. slot's own source files are compiled here unchanged, so a preview
//! is the device's cart face rather than an imitation of it. They reach each other through
//! `crate::` paths, which is why they sit at the root. `art` is the one module swapped out:
//! slot's reads a label from disk, and a browser has no disk.

#[allow(dead_code)]
#[rustfmt::skip]
#[path = "../../slot/crates/slot-ui/src/bitmaps.rs"]
mod bitmaps;
#[allow(dead_code)]
#[rustfmt::skip]
#[path = "../../slot/crates/slot-ui/src/cart.rs"]
mod cart;
#[allow(dead_code)]
#[rustfmt::skip]
#[path = "../../slot/crates/slot-ui/src/shell.rs"]
mod shell;
#[allow(dead_code)]
#[rustfmt::skip]
#[path = "../../slot/crates/slot-ui/src/silhouette.rs"]
mod silhouette;
#[allow(dead_code)]
#[rustfmt::skip]
#[path = "../../slot/crates/slot-ui/src/text.rs"]
mod text;

mod art;
mod board;
pub mod choice;
pub mod dat;
pub mod face;
pub mod hue;
pub mod label;
pub mod libretro;
pub mod rom;
pub mod zip;

use wasm_bindgen::prelude::*;

/// The cart face's size for a platform, `[w, h]`, so the page sizes its canvases from slot. A
/// Game Boy Game Pak is the same width as a GBA cart and nearly twice as tall, so one size cannot
/// serve both: a canvas cut for the wrong one refuses the pixels outright.
#[wasm_bindgen]
pub fn cart_size(platform: &str) -> Vec<u32> {
    let (w, h) = cart::cart_box(face::platform_of(platform));
    vec![w, h]
}

/// A label the card already has, on its cart. `platform` is the card folder the rom sits in,
/// `GBA`, `GB` or `GBC`, which is how the page names a platform either side of the wasm boundary.
/// `head` is a Game Boy rom's first 0x150 bytes, which pick its pak; a GBA cart passes none.
/// `shell` is the cart's value, empty for Automatic.
#[wasm_bindgen]
pub fn existing_face(
    png: &[u8],
    platform: &str,
    code: &str,
    head: &[u8],
    shell: &str,
    stem: &str,
) -> Vec<u8> {
    face::from_png(png, face::platform_of(platform), code, head, shell, stem)
}

/// A ROM's CRC32, fed in the chunks the page reads it in.
#[wasm_bindgen(js_name = Crc32)]
#[derive(Default)]
pub struct JsCrc32(rom::Crc32);

#[wasm_bindgen(js_class = Crc32)]
impl JsCrc32 {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, chunk: &[u8]) {
        self.0.update(chunk);
    }

    pub fn finish(self) -> u32 {
        self.0.finish()
    }
}

#[wasm_bindgen(js_name = Dat)]
pub struct JsDat(dat::Dat);

#[wasm_bindgen(js_class = Dat)]
impl JsDat {
    pub fn parse(text: &str) -> JsDat {
        JsDat(dat::Dat::parse(text))
    }

    pub fn game_for(&self, crc: u32) -> Option<String> {
        self.0.game_for(crc).map(str::to_string)
    }

    pub fn crcs_for(&self, name: &str) -> Vec<u32> {
        self.0.crcs_for(name)
    }

    /// Games whose name contains `query`, for the finder a cart opens when its match is wrong.
    pub fn search(&self, query: &str, limit: usize) -> Vec<String> {
        self.0.search(query, limit)
    }

    /// How many dumps the database names, for the page to mention.
    pub fn games(&self) -> usize {
        self.0.len()
    }
}

#[wasm_bindgen]
pub fn header_code(head: &[u8]) -> String {
    rom::header_code(head)
}

#[wasm_bindgen]
pub fn thumbnail_name(game: &str) -> String {
    libretro::thumbnail_name(game)
}

#[wasm_bindgen]
pub fn stub_target(body: &[u8]) -> Option<String> {
    libretro::stub_target(body)
}

#[wasm_bindgen]
pub fn box_hue(png: &[u8]) -> Option<u16> {
    hue::box_hue(png)
}

/// Mean luminance of a logo's opaque pixels, which is what decides whether its ground goes dark
/// under it or pale over it. 255 for bytes that will not decode: such a logo never gets drawn.
#[wasm_bindgen]
pub fn logo_luma(png: &[u8]) -> f32 {
    hue::logo_luma(png)
}

#[wasm_bindgen]
pub fn fallback_hue(stem: &str) -> u16 {
    hue::fallback_hue(stem)
}

/// The title slot puts on a cart with no label art: the stem without its bracketed tags or its
/// spaced-hyphen subtitle break. The page names a cart the way the device does.
#[wasm_bindgen]
pub fn clean_label(stem: &str) -> String {
    cart::clean_label(stem)
}

/// The bracketed groups `clean_label` drops, in order, one per group: `USA, Europe`, `Rev 1`.
#[wasm_bindgen]
pub fn label_tags(stem: &str) -> Vec<String> {
    cart::label_tags(stem)
}

/// The ground's two corners for a hue and the brightness of the logo going on it: `[deep r, g, b,
/// pale r, g, b]`. The page seeds its colour well from the deep one and draws the chip from both,
/// rather than restating the house numbers in CSS where they could drift.
#[wasm_bindgen]
pub fn stop_colours(hue: u16, logo_luma: f32) -> Vec<u8> {
    let (deep, pale) = label::stops(hue, logo_luma);
    vec![deep[0], deep[1], deep[2], pale[0], pale[1], pale[2]]
}

/// The pale corner a chosen deep one produces, so the page can preview a picked colour's sweep
/// without restating how that corner is derived.
#[wasm_bindgen]
pub fn pale_colour(deep: &[u8]) -> Vec<u8> {
    if deep.len() < 3 {
        return Vec::new();
    }
    label::pale_for([deep[0], deep[1], deep[2]]).to_vec()
}

/// A label for one cart: a logo over a ground described by its deep corner, or a whole label.
#[wasm_bindgen(js_name = Label)]
pub struct JsLabel(label::Label);

#[wasm_bindgen(js_class = Label)]
impl JsLabel {
    #[wasm_bindgen(constructor)]
    pub fn new(
        logo_png: &[u8],
        deep: &[u8],
        platform: &str,
        band_edge: &str,
        band_size: f32,
        band_colour: &[u8],
    ) -> Result<JsLabel, JsError> {
        if deep.len() < 3 {
            return Err(JsError::new("a ground colour needs three channels"));
        }
        label::Label::from_png(
            logo_png,
            [deep[0], deep[1], deep[2]],
            face::platform_of(platform),
            label::Band::parse(band_edge, band_size, band_colour),
        )
        .map(JsLabel)
        .ok_or_else(|| JsError::new("not a PNG this studio can read"))
    }

    /// A whole label, used as it is: cropped to fill, never shrunk onto a ground.
    pub fn full(png: &[u8], platform: &str) -> Result<JsLabel, JsError> {
        label::Label::full(png, face::platform_of(platform))
            .map(JsLabel)
            .ok_or_else(|| JsError::new("not a PNG this studio can read"))
    }

    pub fn deep(&self) -> Vec<u8> {
        self.0.deep()
    }

    pub fn set_deep(&mut self, deep: &[u8]) {
        if deep.len() >= 3 {
            self.0.set_deep(deep);
        }
    }

    pub fn face(
        &self,
        platform: &str,
        code: &str,
        head: &[u8],
        shell: &str,
        stem: &str,
    ) -> Vec<u8> {
        self.0
            .face(face::platform_of(platform), code, head, shell, stem)
    }

    pub fn png(&self) -> Vec<u8> {
        self.0.png()
    }
}

/// Labels for a browser that can't write to the card, to unzip at its root.
#[wasm_bindgen(js_name = Zip)]
#[derive(Default)]
pub struct JsZip(zip::Zip);

#[wasm_bindgen(js_class = Zip)]
impl JsZip {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, name: &str, data: &[u8]) {
        self.0.add(name, data);
    }

    pub fn take(&mut self) -> Vec<u8> {
        self.0.take()
    }

    pub fn finish(self) -> Vec<u8> {
        self.0.finish()
    }
}

/// The shell slot draws this cart in with no choice made.
#[wasm_bindgen]
pub fn auto_shell(platform: &str, code: &str, head: &[u8]) -> String {
    choice::auto_shell(face::platform_of(platform), code, head)
}

#[wasm_bindgen]
pub fn shell_presets() -> Vec<String> {
    choice::presets()
}

#[wasm_bindgen]
pub fn layered_cart_shells(system: &str, labels: &str) -> Vec<String> {
    choice::layered_cart_shells(system, labels)
}

#[wasm_bindgen]
pub fn cart_shells(text: &str) -> Vec<String> {
    choice::cart_shells(text)
}

#[wasm_bindgen]
pub fn merge_cart_shells(
    text: &str,
    stems: Vec<String>,
    values: Vec<String>,
) -> Result<String, JsError> {
    choice::merge_cart_shells(text, &stems, &values).map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen]
pub fn shell_key_ok(stem: &str) -> bool {
    choice::shell_key_ok(stem)
}

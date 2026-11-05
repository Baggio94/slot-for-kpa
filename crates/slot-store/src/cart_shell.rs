use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

pub const CART_SHELL_FILE: &str = "Config/cart_shell.ini";
pub const LABELS_SHELL_FILE: &str = "Labels/cart_shell.ini";

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Outline {
    Auto,
    Notched,
    Rounded,
    // NES, Super Nintendo and Mega Drive shells: the region's own mould or
    // a publisher's (Namco and Konami Famicom carts, Electronic Arts).
    Nes,
    Famicom,
    FamicomNamco,
    FamicomKonami,
    Snes,
    SnesUs,
    MegaDrive,
    MegaDriveJp,
    MegaDriveEa,
}

impl Outline {
    pub const NAMES: [(Outline, &'static str); 12] = [
        (Outline::Auto, "auto"),
        (Outline::Notched, "notched"),
        (Outline::Rounded, "rounded"),
        (Outline::Nes, "nes"),
        (Outline::Famicom, "famicom"),
        (Outline::FamicomNamco, "famicom-namco"),
        (Outline::FamicomKonami, "famicom-konami"),
        (Outline::Snes, "snes"),
        (Outline::SnesUs, "snes-us"),
        (Outline::MegaDrive, "megadrive"),
        (Outline::MegaDriveJp, "megadrive-jp"),
        (Outline::MegaDriveEa, "megadrive-ea"),
    ];

    pub fn name(self) -> &'static str {
        Outline::NAMES.iter().find(|(o, _)| *o == self).map_or("auto", |(_, n)| n)
    }

    pub fn parse(name: &str) -> Option<Outline> {
        Outline::NAMES.iter().find(|(_, n)| *n == name).map(|(o, _)| *o)
    }

    /// The system whose carts this outline belongs to; None for auto.
    pub fn platform(self) -> Option<crate::Platform> {
        use crate::Platform;
        match self {
            Outline::Auto => None,
            Outline::Notched | Outline::Rounded => Some(Platform::Gb),
            Outline::Nes | Outline::Famicom | Outline::FamicomNamco | Outline::FamicomKonami => {
                Some(Platform::Nes)
            }
            Outline::Snes | Outline::SnesUs => Some(Platform::Snes),
            Outline::MegaDrive | Outline::MegaDriveJp | Outline::MegaDriveEa => Some(Platform::Md),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ShellFinish {
    Solid,
    Clear,
    Glitter,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ShellChoice {
    pub outline: Outline,
    pub colour: [u8; 3],
    pub finish: ShellFinish,
}

impl ShellChoice {
    pub fn parse(value: &str) -> Option<ShellChoice> {
        let mut words = value.split_whitespace();
        let outline = Outline::parse(words.next()?)?;
        let hex = words.next()?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let colour = [0, 2, 4].map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0));
        let finish = match words.next()? {
            "solid" => ShellFinish::Solid,
            "clear" => ShellFinish::Clear,
            "glitter" => ShellFinish::Glitter,
            _ => return None,
        };
        words.next().is_none().then_some(ShellChoice {
            outline,
            colour,
            finish,
        })
    }

    pub fn to_value(&self) -> String {
        let outline = self.outline.name();
        let finish = match self.finish {
            ShellFinish::Solid => "solid",
            ShellFinish::Clear => "clear",
            ShellFinish::Glitter => "glitter",
        };
        let [r, g, b] = self.colour;
        format!("{outline} {r:02x}{g:02x}{b:02x} {finish}")
    }
}

pub fn key(stem: &str) -> String {
    stem.nfc().collect()
}

pub fn choices(text: &str) -> HashMap<String, ShellChoice> {
    crate::ini::parse(text)
        .into_iter()
        .filter_map(|(stem, value)| Some((key(&stem), ShellChoice::parse(&value)?)))
        .collect()
}

pub fn layered(system: &str, labels: &str) -> HashMap<String, ShellChoice> {
    let mut out = choices(system);
    for (stem, value) in crate::ini::parse(labels) {
        if value.trim() == "auto" {
            out.remove(&key(&stem));
        } else if let Some(choice) = ShellChoice::parse(&value) {
            out.insert(key(&stem), choice);
        }
    }
    out
}

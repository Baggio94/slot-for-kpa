//! A cart's shell choice as the page holds it: empty for Automatic.

use slot_store::{Cart, Outline, Platform, ShellChoice, ShellFinish};
use unicode_normalization::UnicodeNormalization;

use crate::shell::{self, Finish, Shell};

fn value_of(s: Shell) -> String {
    let finish = match s.finish {
        Finish::Solid => ShellFinish::Solid,
        Finish::Translucent => ShellFinish::Clear,
        Finish::Glitter => ShellFinish::Glitter,
    };
    ShellChoice {
        outline: Outline::Auto,
        colour: s.colour,
        finish,
    }
    .to_value()
}

/// The shell slot picks for this cart with no choice made, as a value.
pub fn auto_shell(platform: Platform, code: &str, head: &[u8]) -> String {
    slot_store::gb::with_header(head, |rom| {
        value_of(shell::shell_for(&Cart {
            platform,
            stem: String::new(),
            rom: rom.to_path_buf(),
            label: None,
            title: String::new(),
            code: code.to_string(),
            shell: None,
        }))
    })
}

/// `name\tvalue` per preset.
pub fn presets() -> Vec<String> {
    shell::shell_presets()
        .into_iter()
        .map(|(name, s)| format!("{name}\t{}", value_of(s)))
        .collect()
}

/// Stem, value, stem, value: each cart's choice once `labels` is laid over `system`, as slot reads
/// the card's two files.
pub fn layered_cart_shells(system: &str, labels: &str) -> Vec<String> {
    slot_store::cart_shell::layered(system, labels)
        .into_iter()
        .flat_map(|(stem, c)| [stem, c.to_value()])
        .collect()
}

/// Stem, value, stem, value, for every line of `text` that parses; values normalised.
pub fn cart_shells(text: &str) -> Vec<String> {
    slot_store::cart_shell::choices(text)
        .into_iter()
        .flat_map(|(stem, c)| [stem, c.to_value()])
        .collect()
}

/// `text` with each stem set to its value, or removed where the value is empty. A key that is the
/// stem under a different Unicode normalisation (macOS lists exFAT names decomposed) is treated as
/// the same cart: it is dropped first, so the stem never ends up on two lines.
pub fn merge_cart_shells(
    text: &str,
    stems: &[String],
    values: &[String],
) -> std::io::Result<String> {
    let mut out = text.to_string();
    for (stem, value) in stems.iter().zip(values) {
        let nfc_stem: String = stem.nfc().collect();
        for key in slot_store::ini::parse(&out).into_keys() {
            if key != *stem && key.nfc().collect::<String>() == nfc_stem {
                out = slot_store::ini::set(&out, &key, None)?;
            }
        }
        out = slot_store::ini::set(&out, stem, (!value.is_empty()).then_some(value.as_str()))?;
    }
    Ok(out)
}

pub fn shell_key_ok(stem: &str) -> bool {
    slot_store::ini::set("", stem, None).is_ok()
}

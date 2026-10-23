use std::collections::HashMap;
use std::path::Path;

use crate::Platform;

pub const SELECTED_CORE_FILE: &str = "Config/selected_core.ini";
pub const DEFAULT_CORE_FILE: &str = "Config/default_core.ini";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Core {
    #[default]
    Mgba,
    Gpsp,
    Gambatte,
}

impl Core {
    pub const ALL: [Core; 3] = [Core::Mgba, Core::Gpsp, Core::Gambatte];

    pub fn as_str(&self) -> &'static str {
        match self {
            Core::Mgba => "mgba",
            Core::Gpsp => "gpsp",
            Core::Gambatte => "gambatte",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn text(self) -> &'static str {
        match self {
            Core::Mgba => "mGBA",
            Core::Gpsp => "gpSP",
            Core::Gambatte => "Gambatte",
        }
    }

    pub fn parse(s: &str) -> Option<Core> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mgba" => Some(Core::Mgba),
            "gpsp" => Some(Core::Gpsp),
            "gambatte" => Some(Core::Gambatte),
            _ => None,
        }
    }

    pub fn runs(self, platform: Platform) -> bool {
        match self {
            Core::Mgba => true,
            Core::Gpsp => platform == Platform::Gba,
            Core::Gambatte => platform != Platform::Gba,
        }
    }

    /// The second socket on the board: gpSP for GBA, Gambatte for GB/GBC.
    pub fn alternate_for(platform: Platform) -> Core {
        match platform {
            Platform::Gba => Core::Gpsp,
            Platform::Gb | Platform::Gbc => Core::Gambatte,
        }
    }

    /// Which of the board's two sockets this core sits in.
    pub fn socket(self) -> usize {
        match self {
            Core::Mgba => 0,
            Core::Gpsp | Core::Gambatte => 1,
        }
    }

    pub fn default_for(platform: Platform) -> Core {
        match platform {
            Platform::Gba | Platform::Gb | Platform::Gbc => Core::Mgba,
        }
    }
}

pub fn read_selected_cores(root: &Path) -> HashMap<String, Core> {
    crate::ini::read(root, SELECTED_CORE_FILE)
        .into_iter()
        .filter_map(|(stem, name)| Core::parse(&name).map(|core| (stem, core)))
        .collect()
}

pub fn core_for(root: &Path, stem: &str) -> Core {
    crate::ini::value(root, SELECTED_CORE_FILE, stem)
        .as_deref()
        .and_then(Core::parse)
        .unwrap_or_default()
}

pub fn core_for_platform(root: &Path, stem: &str, platform: Platform) -> Core {
    crate::ini::value(root, SELECTED_CORE_FILE, stem)
        .as_deref()
        .and_then(Core::parse)
        .filter(|core| core.runs(platform))
        .unwrap_or_else(|| default_core(root, platform))
}

fn default_key(platform: Platform) -> &'static str {
    match platform {
        Platform::Gba => "gba",
        Platform::Gb => "gb",
        Platform::Gbc => "gbc",
    }
}

/// The core a system's carts run on unless a cart was given the other one.
pub fn default_core(root: &Path, platform: Platform) -> Core {
    crate::ini::value(root, DEFAULT_CORE_FILE, default_key(platform))
        .as_deref()
        .and_then(Core::parse)
        .filter(|core| core.runs(platform))
        .unwrap_or_else(|| Core::default_for(platform))
}

pub fn write_default_core(root: &Path, platform: Platform, core: Core) -> std::io::Result<()> {
    // Before default cores existed every confirmed pick was written down,
    // mGBA included. Those mGBA lines only meant "the default": drop them
    // once so those carts follow the new default too.
    if !root.join(DEFAULT_CORE_FILE).exists() {
        for (stem, _) in read_selected_cores(root)
            .into_iter()
            .filter(|(_, core)| *core == Core::Mgba)
        {
            crate::ini::write_entry(root, SELECTED_CORE_FILE, &stem, None)?;
        }
    }
    crate::ini::write(root, DEFAULT_CORE_FILE, default_key(platform), core.as_str())
}

/// A cart's pick from the board: only a core other than the system's
/// default is written down, so the cart keeps following the default.
pub fn choose_core(root: &Path, stem: &str, platform: Platform, core: Core) -> std::io::Result<()> {
    let value = (core != default_core(root, platform)).then(|| core.as_str());
    crate::ini::write_entry(root, SELECTED_CORE_FILE, stem, value)
}

pub fn write_selected_core(root: &Path, stem: &str, core: Core) -> std::io::Result<()> {
    crate::ini::write(root, SELECTED_CORE_FILE, stem, core.as_str())
}

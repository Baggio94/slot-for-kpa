use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Platform {
    #[default]
    Gba,
    Gb,
    Gbc,
    /// NES and Famicom: one system, the cart shell follows the region.
    Nes,
    /// Super Nintendo and Super Famicom.
    Snes,
    /// Mega Drive and Genesis.
    Md,
}

impl Platform {
    pub const ALL: [Platform; 6] = [
        Platform::Gba, Platform::Gb, Platform::Gbc,
        Platform::Nes, Platform::Snes, Platform::Md,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Platform::Gba => "Game Boy Advance",
            Platform::Gb => "Game Boy",
            Platform::Gbc => "Game Boy Color",
            Platform::Nes => "NES",
            Platform::Snes => "Super Nintendo",
            Platform::Md => "Mega Drive",
        }
    }

    pub fn dir_name(self) -> &'static str {
        match self {
            Platform::Gba => "GBA",
            Platform::Gb => "GB",
            Platform::Gbc => "GBC",
            Platform::Nes => "NES",
            Platform::Snes => "SNES",
            Platform::Md => "MD",
        }
    }

    pub fn parse(name: &str) -> Option<Platform> {
        Platform::ALL.into_iter().find(|p| name.eq_ignore_ascii_case(p.dir_name()))
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Platform::Gba => &["gba"],
            Platform::Gb | Platform::Gbc => &["gb", "gbc"],
            Platform::Nes => &["nes", "fc", "unf", "unif"],
            Platform::Snes => &["sfc", "smc"],
            // .bin is also used by other systems: Android only files a .bin
            // here when its header says SEGA.
            Platform::Md => &["md", "gen", "smd", "bin"],
        }
    }

    /// The Game Boy family draws into the fixed 240x160 canvas; the home
    /// consoles hand over each frame at the size their core produced.
    pub fn handheld(self) -> bool {
        matches!(self, Platform::Gba | Platform::Gb | Platform::Gbc)
    }

    /// The usual picture: for the home consoles, what the core shows most
    /// of the time (a frame may differ, e.g. SNES hi-res or MD H32).
    pub fn picture(self) -> (u32, u32) {
        match self {
            Platform::Gba => (240, 160),
            Platform::Gb | Platform::Gbc => (160, 144),
            Platform::Nes => (256, 240),
            Platform::Snes => (256, 224),
            Platform::Md => (320, 224),
        }
    }

    pub fn accepts(self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                self.extensions()
                    .iter()
                    .any(|k| ext.eq_ignore_ascii_case(k))
            })
    }
}

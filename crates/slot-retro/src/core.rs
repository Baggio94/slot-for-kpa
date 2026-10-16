use std::fmt;
use std::path::Path;

use crate::link::Link;
use crate::rumble::Rumble;

pub const GBA_W: u32 = 240;
pub const GBA_H: u32 = 160;

#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct ButtonMask(pub u16);

impl ButtonMask {
    pub const B: u16 = 1 << 0;
    pub const Y: u16 = 1 << 1;
    pub const SELECT: u16 = 1 << 2;
    pub const START: u16 = 1 << 3;
    pub const UP: u16 = 1 << 4;
    pub const DOWN: u16 = 1 << 5;
    pub const LEFT: u16 = 1 << 6;
    pub const RIGHT: u16 = 1 << 7;
    pub const A: u16 = 1 << 8;
    pub const X: u16 = 1 << 9;
    pub const L: u16 = 1 << 10;
    pub const R: u16 = 1 << 11;

    pub fn without_turbo(self) -> ButtonMask {
        ButtonMask(self.0 & !(Self::X | Self::Y))
    }

    pub fn turbo(self, frame: u32) -> ButtonMask {
        let mut mask = self.0 & !(Self::X | Self::Y);
        if (frame / 3).is_multiple_of(2) {
            if self.0 & Self::X != 0 {
                mask |= Self::A;
            }
            if self.0 & Self::Y != 0 {
                mask |= Self::B;
            }
        }
        ButtonMask(mask)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct AvInfo {
    pub fps: f64,
    pub sample_rate: f64,
}

#[derive(Debug)]
pub enum CoreError {
    Io(std::io::Error),
    Load(String),
    Unsupported(String),
    State(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Io(e) => write!(f, "io: {e}"),
            CoreError::Load(m) => write!(f, "load: {m}"),
            CoreError::Unsupported(m) => write!(f, "unsupported: {m}"),
            CoreError::State(m) => write!(f, "state: {m}"),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e)
    }
}

pub trait RetroCore: Send {
    /// Copies the physical GBA IWRAM (0x8000), EWRAM (0x40000), and
    /// cartridge SRAM (0x10000) into a bounded 0x58000-byte snapshot.
    /// Returns the valid byte counts; never treat missing RAM as a zero.
    /// GBA only: GB/GBC need distinct RA console memory maps.
    /// Called exclusively on the libretro/GL thread between frames.
    fn achievement_memory(&self, _ram: &mut [u8]) -> [usize; 3] {
        [0; 3]
    }
    /// Game Boy consoles use their own memory-space, not the GBA layout.
    /// The coverage mask prevents unwritten or unmapped addresses from being
    /// incorrectly evaluated as zero-valued achievement conditions.
    fn achievement_memory_for_console(
        &self, ram: &mut [u8], _coverage: &mut [u8], _console: u32
    ) -> [usize; 3] {
        self.achievement_memory(ram)
    }
    fn load(&mut self, rom: &Path) -> Result<(), CoreError>;
    fn run_frame(&mut self, input: ButtonMask);
    /// A speculative video-only frame. Wrappers must not dispatch gameplay events,
    /// achievements or save notifications from this provisional timeline.
    fn run_speculative_frame(&mut self, input: ButtonMask) {
        self.run_frame(input);
    }
    fn run_frame_linked(&mut self, p1: ButtonMask, _p2: ButtonMask) {
        self.run_frame(p1);
    }
    fn set_option(&mut self, _key: &str, _value: &str) {}
    fn set_frame_skip(&mut self, _skip: bool) {}
    fn video_xrgb8888(&self) -> &[u8];
    fn take_audio(&mut self) -> Vec<i16>;
    /// Drop speculative audio without moving the core's output buffer.
    fn discard_audio(&mut self) { let _ = self.take_audio(); }
    /// Optional reusable audio storage for worker-wrapped libretro cores.
    fn recycle_audio(&mut self, _audio: Vec<i16>) {}
    fn serialize(&mut self) -> Result<Vec<u8>, CoreError>;
    /// Reuse a caller-owned buffer for per-frame speculative snapshots.
    /// The fallback preserves compatibility with existing RetroCore mocks.
    fn serialize_into(&mut self, output: &mut Vec<u8>) -> Result<(), CoreError> {
        *output = self.serialize()?;
        Ok(())
    }
    fn unserialize(&mut self, data: &[u8]) -> Result<(), CoreError>;
    /// Revert speculative execution without announcing a user timeline jump.
    fn restore_speculative_state(&mut self, data: &[u8]) -> Result<(), CoreError> {
        self.unserialize(data)
    }
    /// Sidecars capture mutable state deliberately omitted from ordinary
    /// save states (notably gpSP's internal audio mixer ring). They are
    /// never written into a RetroArch-compatible state file.
    fn speculative_audio_supported(&self) -> bool { true }
    fn capture_speculative_audio(&mut self) -> Result<(), CoreError> { Ok(()) }
    fn restore_speculative_audio(&mut self) -> Result<(), CoreError> { Ok(()) }
    fn save_ram(&self) -> Option<Vec<u8>>;
    fn load_save_ram(&mut self, data: &[u8]) -> Result<(), CoreError>;
    /// Optional libretro RETRO_MEMORY_RTC (Game Boy / Color battery clock).
    fn save_rtc(&self) -> Option<Vec<u8>> { None }
    fn load_rtc(&mut self, _data: &[u8]) -> Result<(), CoreError> {
        Err(CoreError::Unsupported("core exposes no RTC".into()))
    }
    fn av_info(&self) -> AvInfo;
    fn rumble(&self) -> Rumble {
        Rumble::default()
    }
    fn net(&self) -> Link {
        Link::default()
    }
    /// Core must have registered libretro netpacket callbacks; the default
    /// mock/foreign core cannot silently pretend to support wireless linking.
    fn supports_netpacket(&self) -> bool { false }
    fn start_link(&mut self, _client_id: u16) {}
    /// Reject sessions when the core refuses a peer (e.g. serial mode
    /// disabled), rather than displaying a false "Connected" message.
    fn try_start_link(&mut self, client_id: u16) -> bool {
        self.start_link(client_id);
        self.net().is_active()
    }
    fn pump_link(&mut self) {}
    fn stop_link(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder(Vec<ButtonMask>);

    impl RetroCore for Recorder {
        fn load(&mut self, _rom: &Path) -> Result<(), CoreError> {
            Ok(())
        }
        fn run_frame(&mut self, input: ButtonMask) {
            self.0.push(input);
        }
        fn video_xrgb8888(&self) -> &[u8] {
            &[]
        }
        fn take_audio(&mut self) -> Vec<i16> {
            Vec::new()
        }
        fn serialize(&mut self) -> Result<Vec<u8>, CoreError> {
            Ok(Vec::new())
        }
        fn unserialize(&mut self, _data: &[u8]) -> Result<(), CoreError> {
            Ok(())
        }
        fn save_ram(&self) -> Option<Vec<u8>> {
            None
        }
        fn load_save_ram(&mut self, _data: &[u8]) -> Result<(), CoreError> {
            Ok(())
        }
        fn av_info(&self) -> AvInfo {
            AvInfo {
                fps: 60.0,
                sample_rate: 48_000.0,
            }
        }
    }

    #[test]
    fn a_core_without_link_mode_runs_player_1_alone() {
        let mut core = Recorder::default();
        core.run_frame_linked(ButtonMask(ButtonMask::A), ButtonMask(ButtonMask::B));
        assert_eq!(core.0, vec![ButtonMask(ButtonMask::A)]);
    }
}

//! Android host for the unmodified Slot cartridge graphics and shelf behavior.
//!
//! Stage M1: display-only preview carts, not executable games.
mod library;
mod cart_disk_cache;
mod settings;
mod recents;
mod favorites;
mod selection;
mod emulation;
// VSync-clock experiment: host-tested, Android-only active mode.
mod ra_clock_lab;
// RETROARCH pacing: display-clocked core + dynamic audio rate control.
mod ra_sync;
// Run-Ahead second instance: predictions run on a private core copy.
mod runahead_secondary;
// Preemptive Frames: roll back and replay only when the buttons change.
#[cfg(any(test, target_os = "android"))]
mod preemptive;
mod pokemon_flash;
// F11: compile and EXECUTE F7's pure-Rust durability and ordering tests on
// the Linux CI host. Android production shares this exact implementation.
#[cfg(any(test, target_os = "android"))]
mod battery_writer;
// F9: both Linux CI tests and Android JNI use one PCM drain implementation.
#[cfg(any(test, target_os = "android"))]
mod audio_pcm;
// F8 beta11: native-frame stage telemetry executes in Linux CI and Android.
#[cfg(any(test, target_os = "android"))]
mod frame_phases;
#[cfg(any(test, target_os = "android"))]
mod presentation_probe;
// Directly compile the original Cart Studio No-Intro DAT matching code and tests.
// Pinned verbatim at third_party/slot-cart-studio, never reinterpret CRCs.
#[allow(dead_code)]
#[path = "../../../third_party/slot-cart-studio/src/dat.rs"]
mod studio_dat;
// Reuse the exact upstream Slot wallpaper ordering and filtering.
mod wallpaper;
// Use Slot's original per-game Actual/Stretch geometry and video_mode.ini.
mod video_mode;
mod thumb;
mod retroarch_state;
// Reuse the upstream 20 MiB XOR/LZ4 rewind ring without forking its algorithm.
mod rewind;
mod core_selection;
// GB/GBC chip swap between mGBA and Gambatte: palettes and battery carry.
#[cfg(any(test, target_os = "android"))]
mod gb_core_switch;
mod core_legend;
// The original Slot TCP framing and cartridge-specific link selection are
// shared directly; only the Linux Wi-Fi radio driver is replaced on Android.
mod link_net;
mod trusted_link;
mod link_kind;
// Fork's LAN/mDNS wire format remains interoperable across devices.
mod link_mdns;
mod link_lan;
mod multiplayer;
mod link_rom;
mod link_compat;
// Every mGBA Link attempt gets a fresh session namespace, preserving historical
// Host/Join progress and never writing into the solo/RetroArch save root.
mod linked_storage;
// Single-threaded libretro link session adapter: core never runs from a socket worker.
mod link_play;
// Compile Slot's exact mechanical plug and wireless adapter renderer from
// upstream instead of recreating SVGs, timings, easing or positions.
mod app;
mod link_screen;

#[cfg(test)]
mod original_link_animation_tests {
    use super::app::{GameMenu, LinkRow, LinkStep, LinkFail};
    use super::link_screen;

    #[test]
    fn original_cable_timings_are_exactly_kept() {
        let host = LinkRow::Host;
        let pick = GameMenu::Pick(host);
        let working = GameMenu::Working { role: host, step: LinkStep::Waiting, since: 0 };
        let linked = GameMenu::Linked { role: host, worked: 0, since: 200, opened: false };
        let failed = GameMenu::Failed { role: host, fail: LinkFail::NobodyCame, worked: 0, since: 200 };
        let unplug = GameMenu::Unplug { role: host, since: 0 };
        assert_eq!(link_screen::plug_tip(pick, 999), 330.0);
        assert_eq!(link_screen::plug_tip(working, 0), 330.0);
        assert_eq!(link_screen::plug_tip(working, 200), 419.0);
        assert_eq!(link_screen::plug_tip(linked, 360), 419.0);
        assert_eq!(link_screen::clicks_alpha(linked, 359), 0.0);
        assert_eq!(link_screen::clicks_alpha(linked, 360), 1.0);
        assert_eq!(link_screen::plug_tip(failed, 450), 276.0);
        assert_eq!(link_screen::plug_tip(unplug, 260), 330.0);
    }

    #[test]
    fn original_wireless_adapter_and_arc_stagger_are_kept() {
        let working = GameMenu::Working { role: LinkRow::Join,
            step: LinkStep::Radio, since: 0 };
        assert_eq!(link_screen::adapter_base(working, 0), 336.0);
        assert_eq!(link_screen::adapter_base(working, 200), slot_ui::PORT_Y);
        assert_eq!(link_screen::arc_alphas(working, 199), [0.0; 3]);
        let visible = link_screen::arc_alphas(working, 800);
        assert!(visible[0] > visible[1] && visible[1] > visible[2]);
    }
}

// Original Slot 3-frame cable input lockstep and paired savestate framing.
// Keep these wire formats byte-compatible with the original Slot.
#[allow(dead_code)]
mod cable;
mod link_state;
#[cfg(any(target_os = "android", all(test, feature = "runtime-tests")))]
mod game;
#[cfg(any(target_os = "android", all(test, feature = "runtime-tests")))]
mod cart_render;
#[cfg(any(target_os = "android", all(test, feature = "runtime-tests")))]
mod runtime;
#[cfg(target_os = "android")]
mod egl_presentation;

// Reuse upstream Slot's unmodified mechanical core picker: 420ms open,
// 180ms chip hop, 320ms close and refusal shake.
#[allow(dead_code)]
mod core_picker;

#[cfg(test)]
mod original_picker_integration_tests {
    use super::core_picker::{CorePicker, Outcome, Press, OPEN_MS, SLIDE_MS, LIFT_MS, HOP_MS, CLOSE_MS};
    use slot_store::Core;

    #[test]
    fn uses_upstream_open_slide_lift_and_close_timing() {
        assert_eq!(OPEN_MS, 420);
        assert_eq!(SLIDE_MS, 160);
        assert_eq!(LIFT_MS, 260);
        assert_eq!(HOP_MS, 180);
        assert_eq!(CLOSE_MS, 320);
        let mut picker = CorePicker::open(Core::Mgba, 0);
        picker.start(0);
        assert_eq!(picker.openness(210), 0.5);
        assert_eq!(picker.openness(420), 1.0);
        assert_eq!(picker.press(Press::Back, 420), Outcome::Nothing);
        assert!(!picker.finished(600));
        assert!(picker.finished(740));
    }

    #[test]
    fn hopping_chip_reaches_the_other_original_socket() {
        let mut picker = CorePicker::open(Core::Mgba, 0);
        picker.start(0);
        assert_eq!(picker.press(Press::Right, 500), Outcome::Nothing);
        assert_eq!(picker.seat(), Core::Gpsp);
        let chip = picker.chip(500 + HOP_MS / 2);
        assert!(chip.lift > 0.9);
        assert!(chip.seated.is_none());
        assert_eq!(picker.chip(500 + HOP_MS).seated, Some(Core::Gpsp));
    }

    #[test]
    fn game_boy_board_swaps_mgba_for_gambatte() {
        let mut picker = CorePicker::open_with(Core::Mgba, Core::Gambatte, 0);
        picker.start(0);
        assert_eq!(picker.press(Press::Right, 500), Outcome::Nothing);
        assert_eq!(picker.seat(), Core::Gambatte);
        let seated = picker.chip(500 + HOP_MS);
        assert_eq!(seated.seated, Some(Core::Gambatte));
        assert_eq!(seated.across, 1.0);
        assert_eq!(picker.press(Press::Right, 1000), Outcome::Refused);
        assert_eq!(picker.press(Press::Left, 1500), Outcome::Nothing);
        assert_eq!(picker.chip(1500 + HOP_MS).across, 0.0);
        assert_eq!(picker.press(Press::Keep, 2000), Outcome::Write(Core::Mgba));
    }
}

#[cfg(test)]
mod upstream_rewind_integration_tests {
    use super::rewind::{Rewind, REWIND_BYTES};
    #[test]
    fn original_ring_walks_back_through_snapshots_with_bounded_memory() {
        let mut ring = Rewind::new(REWIND_BYTES);
        for frame in 0..30u8 {
            let snapshot = vec![frame; 200_000];
            ring.push(&snapshot);
        }
        assert!(ring.bytes_used() <= REWIND_BYTES);
        assert_eq!(ring.depth(), 30);
        for frame in (1..30u8).rev() {
            assert_eq!(ring.pop(), Some(vec![frame; 200_000]));
        }
        assert_eq!(ring.pop(), Some(vec![0u8; 200_000]));
        assert_eq!(ring.pop(), None);
    }
}

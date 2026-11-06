//! Android host for the unmodified Slot cartridge graphics and shelf behavior.
//!
//! Stage M1: display-only preview carts, not executable games.
mod library;
mod cart_disk_cache;
mod settings;
mod controls;
mod frame_blend;
mod gambatte_state;
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
mod gb_netlink;
mod core_legend;
// The original Slot TCP framing and cartridge-specific link selection are
// shared directly; only the Linux Wi-Fi radio driver is replaced on Android.
mod link_net;
mod trusted_link;
mod link_kind;
// Fork's LAN/mDNS wire format remains interoperable across devices.
mod link_mdns;
mod link_lan;
// Internet Multiplayer: both consoles dial slot.'s room-code relay.
mod link_relay;
mod link_internet;
mod multiplayer;
mod link_rom;
mod link_compat;
// Every mGBA Link attempt gets a fresh session namespace, preserving historical
// Host/Join progress and never writing into the solo/RetroArch save root.
mod linked_storage;
// Single-threaded libretro link session adapter: core never runs from a socket worker.
mod link_play;
// Desync check and resync for an mGBA pair in Cable lockstep.
mod link_sync;
// The native Link menu consumes the same transitions as discovery and gameplay.
mod app;
mod link_screen;

#[cfg(test)]
mod link_animation_tests {
    use super::app::{GameMenu, LinkRow::Host, LinkStep::Waiting, LinkFail, ReleaseFrom};
    use super::link_screen::{arc_alphas, clicks_alpha, travel};

    #[test]
    fn only_an_established_link_shows_connection_marks_and_reopening_keeps_them() {
        let working = GameMenu::Working { role: Host, step: Waiting, since: 0 };
        let linked = GameMenu::Linked { role: Host, worked: 0, since: 200, opened: false };
        let failed = GameMenu::Failed { role: Host, fail: LinkFail::NobodyCame, worked: 0, since: 200 };
        assert_eq!(travel(working, 200, false), 1.0);
        assert_eq!(clicks_alpha(working, 20_000), 0.0);
        assert_eq!(clicks_alpha(linked, 360), 1.0);
        assert_eq!(clicks_alpha(failed, 450), 0.0);
        assert!(travel(failed, 450, false) < 0.0);
        assert_eq!(travel(failed, 450, true), 1.0);
        let reopened = GameMenu::Linked { role: Host, worked: 25, since: 99, opened: true };
        assert_eq!(travel(reopened, 99, false), 1.0);
        assert_eq!(clicks_alpha(reopened, 99), 1.0);
        assert_eq!(arc_alphas(reopened, 99), [1.0; 3]);
    }

    #[test]
    fn cancellation_reverses_the_current_motion_and_releases_wireless_buttons_first() {
        let working = GameMenu::Working { role: Host, step: Waiting, since: 0 };
        let cancel = GameMenu::Unplug { role: Host, since: 100, from: ReleaseFrom::Search { worked: 0 } };
        let half = travel(working, 100, false);
        assert!(half > 0.0 && half < 1.0);
        assert_eq!(travel(cancel, 100, false), half);
        assert!(travel(cancel, 170, false) < half);
        assert_eq!(travel(cancel, 159, true), half);
        assert_eq!(travel(cancel, 425, true), 0.0);
        assert_eq!(arc_alphas(cancel, 100), arc_alphas(working, 100));
        let failed = GameMenu::Failed { role: Host, worked: 0, since: 320, fail: LinkFail::Radio };
        let back = GameMenu::Unplug { role: Host, since: 410, from: ReleaseFrom::Failure { worked: 0, failed: 320 } };
        for radio in [false, true] {
            assert_eq!(travel(failed, 410, radio), travel(back, 410, radio));
        }
    }

    #[test]
    fn radio_waves_follow_the_real_search_and_stop_on_failure() {
        let working = GameMenu::Working { role: Host, step: Waiting, since: 0 };
        assert_eq!(arc_alphas(working,199),[0.0;3]);
        let wave = arc_alphas(working,800);
        assert!(wave[0] > wave[1] && wave[1] > wave[2]);
        let failed = GameMenu::Failed { role: Host, fail: LinkFail::PeerVanished, worked: 0, since: 800 };
        assert_eq!(arc_alphas(failed,800),wave);
        assert_eq!(arc_alphas(failed,1050),[0.0;3]);
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

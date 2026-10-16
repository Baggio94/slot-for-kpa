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
mod pokemon_flash;
// Directly compile the original Cart Studio No-Intro DAT matching code and tests.
// Pinned verbatim at third_party/slot-cart-studio, never reinterpret CRCs.
#[allow(dead_code)]
#[path = "../../../third_party/slot-cart-studio/src/dat.rs"]
mod studio_dat;
// Reuse the exact upstream Slot wallpaper ordering and filtering.
#[path = "../../slot/src/wallpaper.rs"]
mod wallpaper;
// Use Slot's original per-game Actual/Stretch geometry and video_mode.ini.
#[path = "../../slot/src/video_mode.rs"]
mod video_mode;
#[path = "../../slot/src/thumb.rs"]
mod thumb;
mod retroarch_state;
// Reuse the upstream 20 MiB XOR/LZ4 rewind ring without forking its algorithm.
#[path = "../../slot/src/rewind.rs"]
mod rewind;
mod core_selection;
mod core_legend;
// The original Slot TCP framing and cartridge-specific link selection are
// shared directly; only the Linux Wi-Fi radio driver is replaced on Android.
mod link_net;
mod trusted_link;
#[path = "../../slot/src/link_kind.rs"]
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
#[path = "../../slot/src/link_screen.rs"]
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
// Import by path so Android and original never diverge on these wire formats.
#[path = "../../slot/src/cable.rs"]
mod cable;
#[path = "../../slot/src/link_state.rs"]
mod link_state;
#[cfg(target_os = "android")]
mod game;
#[cfg(target_os = "android")]
mod cart_render;
#[cfg(target_os = "android")]
mod runtime;

// Reuse upstream Slot's unmodified mechanical core picker: 420ms open,
// 180ms chip hop, 320ms close and refusal shake.
#[path = "../../slot/src/core_picker.rs"]
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

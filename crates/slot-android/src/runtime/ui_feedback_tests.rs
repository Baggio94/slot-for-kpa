use super::*;
use slot_store::Cart;

#[test]
fn updater_about_has_two_actions_in_all_statuses() {
    assert_eq!(about_primary_action(0), 19);
    assert_eq!(about_primary_action(1), 21);
    assert_eq!(about_primary_action(5), 21);
    for status in [2, 3, 4] {
        assert_eq!(about_primary_action(status), 19);
    }
    assert_eq!(move_menu_row(0, 19, 2), 1);
    assert_eq!(move_menu_row(1, 20, 2), 0);
    assert_eq!(about_row_top(), 270.0);
}

#[test]
fn gba_three_two_screen_hides_no_op_bezel_but_keeps_gb_gbc() {
    assert_eq!(Engine::screen_rows(Platform::Gba), &[34,29]);
    assert_eq!(Engine::screen_keys(Platform::Gba), &[0,22]);
    assert_eq!(Engine::screen_rows(Platform::Gbc), &[88,29,90]);
    assert_eq!(Engine::screen_rows(Platform::Gb), &[89,29,36,90]);
}
#[test]
fn ordinary_cart_scrolling_does_not_restart_name_fade() {
    // A no-name-transition title is drawn at alpha 1 immediately;
    // only a letter jump invokes slot_title_alpha().
    assert_eq!(slot_title_alpha(0), 0.0);
    assert_eq!(slot_title_alpha(2200), 1.0);
    assert!(!platform_name_running(2200));
}
#[test]
fn official_game_title_fades_after_name_and_then_remains_visible() {
    // The earlier U1 renderer incorrectly expired Game Titles after
    // 2200ms; upstream instead keeps a fully visible title at rest.
    assert_eq!(slot_title_alpha(0), 0.0);
    assert_eq!(slot_title_alpha(1399), 0.0);
    assert!((slot_title_alpha(1800) - 0.5).abs() < 0.0001);
    assert_eq!(slot_name_alpha(200), 1.0);
    assert_eq!(slot_name_alpha(1400), 1.0);
    assert!((slot_name_alpha(1800) - 0.5).abs() < 0.0001);
    assert_eq!(slot_title_alpha(2200), 1.0);
    assert_eq!(slot_title_alpha(100_000), 1.0);
}


#[test]
fn legacy_multiplayer_heading_helper_mode_and_footer_text_cannot_come_back() {
    // Our previous integration/konkr-rc4-next rendered indices 68..77
    // as extra MULTIPLAYER/CABLE/CHOOSE... overlays, together with an
    // arbitrary 60px black footer and LEFT/RIGHT text keycap.
    // They remain reserved index slots so unrelated menus are stable,
    // but no legacy Multiplayer string can be rendered from MENU_TEXT.
    assert!(MENU_TEXT[68..=77].iter().all(|label| label.is_empty()));
    assert_eq!(MENU_TEXT[66], "Multiplayer"); // still a game menu action
    assert_eq!(MENU_TEXT[67], "End Link");
    assert_eq!(MENU_TEXT[78], "Eject to play solo");
}

#[test]
fn original_slot_link_title_and_bottom_keycaps_use_exact_canvas_geometry() {
    assert_eq!(out_w(), 720);
    assert_eq!(OUT_H, 480);
    assert_eq!(LINK_TEXT_Y, 44.0);
    assert_eq!(LINK_LEGEND_Y, 422.0);
    assert_eq!(LINK_LEGEND_GAP, 40.0);
    for width in [88, 100, 120, 216, 300] {
        let x = link_heading_x(width);
        assert!(((x + width as f32 * 0.5) - 360.0).abs() <= 0.5);
    }
    // The old HOST was hard-coded to x=300 regardless of typography;
    // this rule always centres both translated role title glyph widths.
    assert_eq!(link_heading_x(120), 300.0);
    assert_eq!(link_heading_x(216), 252.0);
}

#[test]
fn android_suspend_forces_eject_menu_when_a_live_link_ended_fullscreen() {
    assert!(!suspend_returns_to_eject_menu(false, false),
        "an ordinary solo game can resume without a modal");
    assert!(suspend_returns_to_eject_menu(false, true),
        "discovery interrupted onPause must leave Multiplayer");
    assert!(suspend_returns_to_eject_menu(true, false),
        "linked gameplay interrupted fullscreen must require Eject");
    assert!(suspend_returns_to_eject_menu(true, true));
}

#[test]
fn link_boot_failures_never_pretend_solo_recovery_succeeded() {
    let ok = link_boot_failure("gpSP Link", "cannot connect", Ok(()));
    assert!(ok.contains("back to solo"));
    let unsafe_state = link_boot_failure("mGBA Link", "invalid peer",
        Err("saved state was rejected".into()));
    assert!(unsafe_state.contains("Eject and reinsert"));
    assert!(!unsafe_state.contains("back to solo"));
}

#[test]
fn multiplayer_bottom_legends_match_all_original_slot_screen_states() {
    use LinkLegendProfile as H;
    assert_eq!(link_legend_profile(LinkMenu::Pick(LinkRow::Host), true),
        H::PickerWithMode);
    assert_eq!(link_legend_profile(LinkMenu::Pick(LinkRow::Join), true),
        H::PickerWithMode);
    assert_eq!(link_legend_profile(LinkMenu::Pick(LinkRow::Host), false),
        H::Picker);
    assert_eq!(link_legend_profile(LinkMenu::Working {
        role: LinkRow::Host, step: LinkStep::Waiting, since: 100,
    }, true), H::Searching);
    assert_eq!(link_legend_profile(LinkMenu::Linked {
        role: LinkRow::Host, worked: 100, since: 500, opened: false,
    }, true), H::None);
    assert_eq!(link_legend_profile(LinkMenu::Linked {
        role: LinkRow::Join, worked: 100, since: 500, opened: true,
    }, true), H::ConnectedMenu);
    assert_eq!(link_legend_profile(LinkMenu::Failed {
        role: LinkRow::Join, fail: LinkFail::PeerVanished,
        worked: 100, since: 500,
    }, true), H::Failed);
    assert_eq!(link_legend_profile(LinkMenu::Unplug {
        role: LinkRow::Join, since: 500,
    }, true), H::None);
}

#[test]
fn paused_game_controls_inherit_globals_until_overridden() {
    let mut settings=crate::settings::Settings::default();
    settings.ff_speed=6;
    settings.turbo=true; // KONKR original X/Y Buttons
    let defaults=LiveGameOptions::from_profile(&settings,None);
    assert_eq!(defaults.ff_speed,6);
    assert!(defaults.turbo);
    let mut profile=EmulationProfile::from_settings(&settings);
    assert!(profile.change_visual_and_gameplay(&settings,Platform::Gba,7,true));
    assert!(profile.change_visual_and_gameplay(&settings,Platform::Gba,4,true));
    let scoped=LiveGameOptions::from_profile(&settings,Some(profile));
    assert_ne!(scoped.ff_speed,defaults.ff_speed);
    assert_eq!(scoped.turbo,!defaults.turbo);
    assert_eq!(settings.ff_speed,6);
    assert!(settings.turbo,"global X/Y Buttons must not be modified");
    // A reset immediately restores the current Settings globals.
    assert_eq!(LiveGameOptions::from_profile(&settings,None).ff_speed,6);
}

#[test]
fn eject_to_play_solo_is_an_action_not_a_noop() {
    assert_eq!(pause_rows(false,false),
        &[PauseRow::Resume,PauseRow::States,PauseRow::Multiplayer,
            PauseRow::Settings,PauseRow::SaveEject]);
    assert_eq!(pause_rows(true,false),
        &[PauseRow::Resume,PauseRow::Multiplayer,PauseRow::Settings,
            PauseRow::SaveEject]);
    assert_eq!(pause_rows(false,true),
        &[PauseRow::SaveEject,PauseRow::Multiplayer]);
    for (linked,needs_eject) in [(false,false),(true,false),(false,true),(true,true)] {
        for (index,item) in pause_rows(linked,needs_eject).iter().enumerate() {
            assert_eq!(game_menu_ejects(index,linked,needs_eject),
                *item==PauseRow::SaveEject,
                "never save/eject from States, Settings or Multiplayer");
        }
    }
    assert!(game_menu_ejects(0,true,true));
    assert!(!game_menu_ejects(0,true,false));
    assert_eq!(pause_row_text(PauseRow::SaveEject,true),78);
    assert_eq!(pause_row_text(PauseRow::Settings,false),25);
    assert!(!pause_rows(true,false).contains(&PauseRow::States));
}

#[test]
fn slot_select_mode_only_allows_real_gpsp_serial_hardware_changes() {
    // Retail FireRed defaults to RFU. SELECT switches to real Cable
    // serial mode, which triggers a fresh gpSP boot before Join/Host.
    assert!(link_mode_can_switch(Core::Gpsp, LinkKind::Wireless,
        "BPRE", "POKEMON FIRE", true));
    assert!(link_mode_can_switch(Core::Gpsp, LinkKind::Cable,
        "BPRE", "POKEMON FIRE", true));
    assert_eq!(link_kind::serial_option(LinkKind::Wireless,
        LinkKind::Wireless, "BPRE", "POKEMON FIRE"), "auto");
    assert_eq!(link_kind::serial_option(LinkKind::Cable,
        LinkKind::Wireless, "BPRE", "POKEMON FIRE"), "mul_poke");
    assert!(!link_mode_can_switch(Core::Mgba, LinkKind::Cable,
        "BPRE", "POKEMON FIRE", true));
    assert!(!link_mode_can_switch(Core::Gpsp, LinkKind::Cable,
        "NONE", "UNKNOWN", false));
    assert!(!link_mode_can_switch(Core::Gpsp, LinkKind::Wireless,
        "BRBE", "MEGAMAN BN5", true),
        "gpSP RFU-only Battle Network must not offer a fake Cable mode");
}

#[test]
fn link_failures_use_upstream_toasts_not_invented_link_sounds() {
    use crate::link_play::LinkError;
    assert_eq!(link_fault_toast(LinkError::BiosMismatch), Toast::BiosMismatch);
    for fault in [LinkError::PeerEnded, LinkError::Disconnected,
                  LinkError::InvalidState, LinkError::Timeout] {
        assert_eq!(link_fault_toast(fault), Toast::PeerEnded);
    }
    assert_eq!(Toast::LinkEnded.text(), "Link ended");
}

#[test]
fn multiplayer_badges_match_original_host_join_broken_and_off() {
    assert_eq!(link_badge_for(LinkRow::Host, false), LinkBadge::Hosting);
    assert_eq!(link_badge_for(LinkRow::Join, false), LinkBadge::Joined);
    assert_eq!(link_badge_for(LinkRow::Host, true), LinkBadge::HostingLost);
    assert_eq!(link_badge_for(LinkRow::Join, true), LinkBadge::JoinedLost);
    for variant in LinkBadge::FACES {
        assert!(variant.badge().is_some());
        assert!(variant.colour().is_some());
        assert!(variant.face_index().is_some());
    }
    assert!(LinkBadge::Off.badge().is_none());
    assert!(LinkBadge::Off.face_index().is_none());
}

#[test]
fn linked_reopen_stays_visible_and_unplug_uses_original_timing() {
    let first = LinkMenu::Linked {
        role: LinkRow::Host, worked: 100, since: 200, opened: false,
    };
    let reopened = LinkMenu::Linked {
        role: LinkRow::Host, worked: 100, since: 200, opened: true,
    };
    assert!(!link_menu_auto_closes(first, 1199));
    assert!(link_menu_auto_closes(first, 1200));
    assert!(!link_menu_auto_closes(reopened, 20000));
    let unplug = LinkMenu::Unplug { role: LinkRow::Join, since: 500 };
    assert!(!link_unplug_complete(unplug, 919));
    assert!(link_unplug_complete(unplug, 920));
    assert!(!link_unplug_complete(first, 10000));
}

#[test]
fn connected_link_keycaps_fit_konkr_screen() {
    let linked_width = slot_ui::hint_width("B", "Back")
        + slot_ui::hint_width("A", "End Link");
    let failed_width = slot_ui::hint_width("A", "OK");
    for width in [linked_width, failed_width] {
        assert!((width as f32) + 15.0 < out_w() as f32 - 60.0);
    }
}

#[test]
fn multiplayer_b_restores_exactly_the_opening_context() {
    assert_eq!(LinkEntry::Shortcut.back(false,false), ShelfOverlay::None);
    assert_eq!(LinkEntry::GameMenu.back(false,false),
        ShelfOverlay::GameMenu { row: 2 });
    assert_eq!(LinkEntry::GameMenu.back(true,false),
        ShelfOverlay::GameMenu { row: 1 });
    assert_eq!(LinkEntry::GameMenu.back(true,true),
        ShelfOverlay::GameMenu { row: 1 });
}

#[test]
fn select_home_opens_link_once_then_consumes_repeat_and_release() {
    let mut consumed = false;
    assert_eq!(select_home_action(true, true, true, true, &mut consumed),
        SelectHomeAction::OpenLink);
    assert!(consumed);
    // Android may issue HOME auto-repeat while SELECT is still held.
    assert_eq!(select_home_action(true, true, true, true, &mut consumed),
        SelectHomeAction::Swallow);
    assert_eq!(select_home_action(false, false, true, true, &mut consumed),
        SelectHomeAction::Swallow);
    assert!(!consumed);
    assert_eq!(select_home_action(false, false, true, true, &mut consumed),
        SelectHomeAction::Pass);
}

#[test]
fn select_home_never_hijacks_shelf_menu_or_state_browser() {
    let mut consumed = false;
    for (select, playing, menu_allowed) in [
        (false, true, true),
        (true, false, true),
        (true, true, false),
    ] {
        assert_eq!(select_home_action(true, select, playing, menu_allowed,
            &mut consumed), SelectHomeAction::Pass);
        assert!(!consumed);
    }
}

#[test]
fn home_alone_keeps_original_quick_menu_and_long_eject_paths() {
    let mut consumed = false;
    assert_eq!(select_home_action(true, false, true, true, &mut consumed),
        SelectHomeAction::Pass);
    assert_eq!(select_home_action(false, false, true, true, &mut consumed),
        SelectHomeAction::Pass);
    assert!(!consumed);
}

#[test]
fn background_library_swap_waits_for_idle_carousel() {
    assert!(may_apply_library_update(false, false, false));
    assert!(!may_apply_library_update(true, false, false));
    assert!(!may_apply_library_update(false, true, false));
    assert!(!may_apply_library_update(false, false, true));
}

#[test]
fn rebuilding_an_empty_library_keeps_selection_safe() {
    let mut shelves: Vec<Shelf> = (0..3).map(|_| Shelf::new(Vec::new())).collect();
    assert_eq!(restore_shelf_focus(&mut shelves, 0, &[]), 0);
    assert_eq!(restore_shelf_focus(&mut shelves, 2, &[None, None, None]), 2);
}

#[test]
fn fresh_avd_without_roms_has_no_selected_artwork_to_prefetch() {
    let shelves: Vec<Shelf> = (0..3).map(|_| Shelf::new(Vec::new())).collect();
    assert_eq!(selected_prefetch_entry(&shelves, 0), None);
    assert_eq!(selected_prefetch_entry(&shelves, 2), None);
    assert_eq!(selected_prefetch_entry(&shelves, 3), None);
}

#[test]
fn in_game_shortcut_keycaps_fit_720x480_without_clipping() {
    let widths = [
        slot_ui::hint_width("B", "Back"),
        slot_ui::hint_width("SEL+R1", "Save"),
        slot_ui::hint_width("SEL+L1", "Load"),
    ];
    let visible = widths.iter()
        .map(|w| w.saturating_sub(HINT_EDGE) as f32)
        .sum::<f32>() + 2.0 * 15.0;
    assert!(visible < out_w() as f32 - 2.0 * 30.0,
        "original keycap hints must stay visible on the KONKR 3:2 screen");
}

#[test]
fn paused_game_menu_dpad_up_down_wrap_correctly() {
    assert_eq!(move_menu_row(0, 19, 3), 2);
    assert_eq!(move_menu_row(2, 20, 3), 0);
    assert_eq!(move_menu_row(1, 19, 3), 0);
    assert_eq!(move_menu_row(1, 20, 3), 2);
    assert_eq!(move_menu_row(1, 96, 3), 1);
}

#[test]
fn original_settings_menu_rows_are_on_screen_and_outside_footer() {
    for rows in [3, 5, 6, 7] {
        let top = settings_row_top(rows);
        assert!(top >= 32.0);
        assert!(top + rows as f32 * QUICK_PITCH < 427.0,
            "settings text must not overlap Slot keycap legend");
        assert_eq!(top + (rows - 1) as f32 * QUICK_PITCH,
            settings_row_top(rows) + (rows - 1) as f32 * QUICK_PITCH);
    }
}

#[test]
fn cartridge_timelines_match_upstream_seating_hold_and_ejection() {
    assert!((SCREEN_POWER_OFF_S - 0.16).abs() < 0.001);
    assert!((EJECT_HOLD_S - 0.35).abs() < 0.001);
    assert!((EJECT_S - 0.45).abs() < 0.001);
    assert!((SCREEN_POWER_OFF_S + EJECT_HOLD_S + EJECT_S - 0.96).abs() < 0.001);
    assert!(advance_cart(1.0, false, SCREEN_POWER_OFF_S) < 1.0);
    assert!(advance_cart(1.0, false, EJECT_S) < 0.0001);
    assert_eq!(advance_cart(0.0, true, SEATED_AT), 1.0);
    assert!((INSERT_S - SEATED_AT - 0.28).abs() < 0.001);
    // The original Slot lead is 97ms: sample impact should coincide
    // with the end of 450ms mechanical seating.
    assert!((SEATED_AT * INSERT_SOUND_PROGRESS + INSERT_SOUND_LEAD_S - SEATED_AT).abs() < 0.001);
    // The requested slightly later eject click must not alter mechanics.
    assert!((EJECT_S * (1.0 - EJECT_SOUND_PROGRESS) - 0.040).abs() < 0.001);
}

#[test]
fn cartridge_sound_thresholds_follow_motion_without_starting_early() {
    // Insertion click is silent at 210ms, unlike the old Android port.
    let mut seat = advance_cart(0.0, true, 0.210);
    assert!(seat < INSERT_SOUND_PROGRESS);
    let previous = seat;
    seat = advance_cart(seat, true, 0.145);
    assert!(previous < INSERT_SOUND_PROGRESS && seat >= INSERT_SOUND_PROGRESS);

    // Ejection keeps the original black hold and waits a few frames
    // of actual motion before handing off the original PCM clip.
    let mut eject = 1.0;
    for _ in 0..2 {
        eject = advance_cart(eject, false, 1.0 / 60.0);
        assert!(eject > EJECT_SOUND_PROGRESS);
    }
    eject = advance_cart(eject, false, 1.0 / 60.0);
    assert!(eject <= EJECT_SOUND_PROGRESS);
}

#[test]
fn ra_notice_bridge_removes_control_characters_and_bounds_payload() {
    let text = ra_notice_payload(&("X".repeat(200) + "\nsecret"), "Line\rTwo", true, None);
    let data: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(data["title"].as_str().unwrap().chars().count(), 100);
    assert!(!data["title"].as_str().unwrap().contains("secret"));
    assert_eq!(data["detail"], "LineTwo");
    assert_eq!(data["earned"], true);
    assert_eq!(data["badge"], "");
}

#[test]
fn ra_notice_bridge_never_builds_an_unbounded_queue() {
    let mut notices = VecDeque::new();
    for index in 0..32 { queue_ra_notice(&mut notices, index.to_string()); }
    assert_eq!(notices.len(), RA_NOTICE_LIMIT);
    assert_eq!(notices.front().map(String::as_str), Some("24"));
    assert_eq!(notices.back().map(String::as_str), Some("31"));
}

#[test]
fn select_chords_have_distinct_consumed_release_bits() {
    let mut combined = 0u16;
    for code in [96, 97, 100, 102, 103, 104, 105] {
        let bit = select_chord_bit(code);
        assert_ne!(bit, 0);
        assert_eq!(bit & combined, 0);
        combined |= bit;
    }
    assert_eq!(select_chord_bit(109), 0);
}

#[test]
fn platform_name_timing_matches_upstream_slot() {
    // Regression: a zero-alpha first frame must not cancel the animation.
    assert!(platform_name_running(0));
    assert!(platform_name_running(1));
    assert!(platform_name_running(199));
    assert!(platform_name_running(2199));
    assert!(!platform_name_running(2200));
    assert!(!platform_name_running(3500));
    assert_eq!(slot_name_alpha(0), 0.0);
    assert!(slot_name_alpha(100) > 0.0);
    // v1.7.0 SLOT_NAME_ALPHA is 1.0, not the old Android 0.4.
    assert!((slot_name_alpha(200) - 1.0).abs() < 0.001);
    assert!((slot_name_alpha(1400) - 1.0).abs() < 0.001);
    assert!(slot_name_alpha(2000) > 0.0);
    assert_eq!(slot_name_alpha(2200), 0.0);
    assert_eq!(slot_name_alpha(3500), 0.0);
    assert_eq!(Platform::Gba.name(), "Game Boy Advance");
    assert_eq!(Platform::Gb.name(), "Game Boy");
    assert_eq!(Platform::Gbc.name(), "Game Boy Color");
    for platform in Platform::ALL {
        let face = platform_name_face(platform.name());
        assert_eq!(face.h, HINT_H * slot_ui::PLATFORM_NAME_SCALE);
        // Original names must fit the Slot canvas at native scale.
        assert!(face.w > 0 && face.w < 720 * slot_ui::PLATFORM_NAME_SCALE);
        assert_eq!(face.rgba.len(), (face.w * face.h * 4) as usize);
    }
}

#[test]
fn empty_shelves_are_skipped_in_both_directions() {
    let mut shelves: Vec<Shelf> = (0..3).map(|_| Shelf::new(vec![])).collect();
    assert_eq!(next_nonempty_shelf(&shelves, 0, 1), 0);
    shelves[0].carts.push(Cart {
        platform: Platform::Gba, stem: "Test".into(), title: "Test".into(),
        code: String::new(), rom: PathBuf::new(), label: None, shell: None,
    });
    assert_eq!(next_nonempty_shelf(&shelves, 0, 1), 0);
    assert_eq!(next_nonempty_shelf(&shelves, 0, -1), 0);
    shelves[2].carts.push(Cart {
        platform: Platform::Gbc, stem: "Color".into(), title: "Color".into(),
        code: String::new(), rom: PathBuf::new(), label: None, shell: None,
    });
    assert_eq!(next_nonempty_shelf(&shelves, 0, 1), 2);
    assert_eq!(next_nonempty_shelf(&shelves, 0, -1), 2);
    assert_eq!(next_nonempty_shelf(&shelves, 2, 1), 0);
}

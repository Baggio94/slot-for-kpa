use slot_store::{
    core_for, core_for_platform, read_selected_cores, write_selected_core, Core, Platform,
};
use tempfile::tempdir;

fn root_with(ini: Option<&str>) -> tempfile::TempDir {
    let d = tempdir().unwrap();
    std::fs::create_dir(d.path().join("Config")).unwrap();
    if let Some(text) = ini {
        std::fs::write(d.path().join("Config/selected_core.ini"), text).unwrap();
    }
    d
}

#[test]
fn absent_file_means_everything_defaults() {
    let d = root_with(None);
    assert!(read_selected_cores(d.path()).is_empty());
    assert_eq!(core_for(d.path(), "Emerald"), Core::Mgba);
}

#[test]
fn a_listed_stem_gets_its_core() {
    let d = root_with(Some("Pokemon - Emerald Version (USA, Europe) = gpsp\n"));
    assert_eq!(
        core_for(d.path(), "Pokemon - Emerald Version (USA, Europe)"),
        Core::Gpsp
    );
}

#[test]
fn an_unlisted_stem_defaults_to_mgba() {
    let d = root_with(Some("Emerald = gpsp\n"));
    assert_eq!(core_for(d.path(), "Metroid Fusion"), Core::Mgba);
}

#[test]
fn malformed_lines_are_ignored_rather_than_fatal() {
    let d = root_with(Some(concat!(
        "\n",
        "# a comment\n",
        "; another comment\n",
        "[cores]\n",
        "no equals sign here\n",
        "Emerald = notacore\n",
        "  Spaced Out   =   gpsp  \n",
        "= gpsp\n",
        "Trailing =\n",
    )));
    let map = read_selected_cores(d.path());
    assert_eq!(
        map.get("Spaced Out"),
        Some(&Core::Gpsp),
        "whitespace not trimmed"
    );
    assert_eq!(
        map.get("Emerald"),
        None,
        "an unknown core name must not be stored"
    );
    assert_eq!(core_for(d.path(), "Emerald"), Core::Mgba);
    assert_eq!(map.len(), 1, "only the one good line should survive");
}

#[test]
fn a_later_duplicate_wins() {
    let d = root_with(Some("Emerald = mgba\nEmerald = gpsp\n"));
    assert_eq!(core_for(d.path(), "Emerald"), Core::Gpsp);
}

#[test]
fn the_last_line_for_a_cart_wins_even_when_it_is_the_typo() {
    let d = root_with(Some("Emerald = gpsp\nEmerald = notacore\n"));
    assert_eq!(
        core_for(d.path(), "Emerald"),
        Core::Mgba,
        "a line the later one superseded is still in force"
    );
    assert_eq!(
        read_selected_cores(d.path()).get("Emerald"),
        None,
        "the map and core_for read the same two lines differently"
    );

    let d = root_with(Some("Emerald = notacore\nEmerald = gpsp\n"));
    assert_eq!(core_for(d.path(), "Emerald"), Core::Gpsp);
    assert_eq!(
        read_selected_cores(d.path()).get("Emerald"),
        Some(&Core::Gpsp)
    );
}

#[test]
fn core_names_round_trip() {
    for c in Core::ALL {
        assert_eq!(Core::parse(c.as_str()), Some(c));
    }
    assert_eq!(
        Core::parse("MGBA"),
        Some(Core::Mgba),
        "case is not the user's problem"
    );
    assert_eq!(Core::parse("nonsense"), None);
    assert_eq!(Core::default(), Core::Mgba);
}

#[test]
fn writing_a_core_creates_the_file_when_absent() {
    let d = root_with(None);
    slot_store::write_selected_core(d.path(), "Emerald", Core::Gpsp).unwrap();
    assert_eq!(core_for(d.path(), "Emerald"), Core::Gpsp);
}

#[test]
fn writing_a_core_replaces_that_carts_line_and_leaves_the_rest_alone() {
    let d = root_with(Some(concat!(
        "# my notes\n",
        "\n",
        "Emerald = mgba\n",
        "Metroid Fusion = gpsp\n",
    )));
    slot_store::write_selected_core(d.path(), "Emerald", Core::Gpsp).unwrap();

    let text = std::fs::read_to_string(d.path().join("Config/selected_core.ini")).unwrap();
    assert!(
        text.contains("# my notes"),
        "a hand-written comment was destroyed"
    );
    assert!(
        text.contains("Metroid Fusion = gpsp"),
        "another cart's entry was lost"
    );
    assert_eq!(core_for(d.path(), "Emerald"), Core::Gpsp);
    assert_eq!(core_for(d.path(), "Metroid Fusion"), Core::Gpsp);
    assert_eq!(
        text.matches("Emerald").count(),
        1,
        "the old line was left behind"
    );
}

#[test]
fn writing_a_core_appends_a_cart_the_file_has_never_seen() {
    let d = root_with(Some("Emerald = gpsp\n"));
    slot_store::write_selected_core(d.path(), "Drill Dozer", Core::Gpsp).unwrap();
    assert_eq!(core_for(d.path(), "Emerald"), Core::Gpsp);
    assert_eq!(core_for(d.path(), "Drill Dozer"), Core::Gpsp);
}

#[test]
fn writing_the_default_still_records_it() {
    let d = root_with(Some("Emerald = gpsp\n"));
    slot_store::write_selected_core(d.path(), "Emerald", Core::Mgba).unwrap();
    assert_eq!(core_for(d.path(), "Emerald"), Core::Mgba);
    let text = std::fs::read_to_string(d.path().join("Config/selected_core.ini")).unwrap();
    assert!(text.contains("Emerald = mgba"));
}

#[test]
fn every_platform_defaults_to_mgba() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(
        core_for_platform(d.path(), "Tetris", Platform::Gb),
        Core::Mgba
    );
    assert_eq!(
        core_for_platform(d.path(), "Tetris Chromatic", Platform::Gbc),
        Core::Mgba
    );
    assert_eq!(
        core_for_platform(d.path(), "Emerald", Platform::Gba),
        Core::Mgba
    );
}

#[test]
fn a_cart_can_ask_for_a_non_default_core_by_hand() {
    let d = tempfile::tempdir().unwrap();
    write_selected_core(d.path(), "Emerald", Core::Gpsp).unwrap();
    assert_ne!(Core::default_for(Platform::Gba), Core::Gpsp);
    assert_eq!(
        core_for_platform(d.path(), "Emerald", Platform::Gba),
        Core::Gpsp
    );
}

#[test]
fn a_line_naming_a_core_the_platform_cannot_run_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    write_selected_core(d.path(), "Tetris", Core::Gpsp).unwrap();
    assert_eq!(
        core_for_platform(d.path(), "Tetris", Platform::Gb),
        Core::Mgba
    );
    assert_eq!(
        core_for_platform(d.path(), "Tetris", Platform::Gbc),
        Core::Mgba
    );
}

#[test]
fn the_picker_board_has_exactly_two_sockets() {
    for platform in [Platform::Gba, Platform::Gb, Platform::Gbc] {
        let pair = [Core::default_for(platform), Core::alternate_for(platform)];
        assert_eq!(pair.map(Core::socket), [0, 1]);
        assert!(pair.iter().all(|core| core.runs(platform)));
    }
}

#[test]
fn gambatte_runs_only_game_boy_carts() {
    let d = tempfile::tempdir().unwrap();
    write_selected_core(d.path(), "Tetris", Core::Gambatte).unwrap();
    assert_eq!(core_for_platform(d.path(), "Tetris", Platform::Gb), Core::Gambatte);
    assert_eq!(core_for_platform(d.path(), "Tetris", Platform::Gbc), Core::Gambatte);
    assert_eq!(core_for_platform(d.path(), "Tetris", Platform::Gba), Core::Mgba);
}

#[test]
fn a_system_default_applies_to_every_cart_without_a_pick() {
    let d = root_with(None);
    slot_store::write_default_core(d.path(), Platform::Gb, Core::Gambatte).unwrap();
    assert_eq!(core_for_platform(d.path(), "Tetris", Platform::Gb), Core::Gambatte);
    assert_eq!(core_for_platform(d.path(), "Tetris DX", Platform::Gbc), Core::Mgba);
    assert_eq!(core_for_platform(d.path(), "Emerald", Platform::Gba), Core::Mgba);
}

#[test]
fn a_default_the_system_cannot_run_is_ignored() {
    let d = root_with(None);
    std::fs::write(d.path().join("Config/default_core.ini"), "gba = gambatte\n").unwrap();
    assert_eq!(slot_store::default_core(d.path(), Platform::Gba), Core::Mgba);
}

#[test]
fn a_pick_other_than_the_default_survives_a_new_default() {
    let d = root_with(None);
    slot_store::choose_core(d.path(), "Zelda", Platform::Gb, Core::Gambatte).unwrap();
    slot_store::choose_core(d.path(), "Tetris", Platform::Gb, Core::Mgba).unwrap();
    let text = std::fs::read_to_string(d.path().join("Config/selected_core.ini")).unwrap();
    assert_eq!(text, "Zelda = gambatte\n", "picking the default writes nothing");

    slot_store::write_default_core(d.path(), Platform::Gb, Core::Gambatte).unwrap();
    slot_store::choose_core(d.path(), "Pokemon", Platform::Gb, Core::Mgba).unwrap();
    slot_store::write_default_core(d.path(), Platform::Gb, Core::Mgba).unwrap();
    assert_eq!(core_for_platform(d.path(), "Zelda", Platform::Gb), Core::Gambatte);
    assert_eq!(core_for_platform(d.path(), "Pokemon", Platform::Gb), Core::Mgba);
    assert_eq!(core_for_platform(d.path(), "Tetris", Platform::Gb), Core::Mgba);
}

#[test]
fn the_first_default_frees_carts_that_only_recorded_mgba() {
    let d = root_with(Some("Tetris = mgba\nZelda = gambatte\nEmerald = gpsp\n"));
    slot_store::write_default_core(d.path(), Platform::Gb, Core::Gambatte).unwrap();
    assert_eq!(core_for_platform(d.path(), "Tetris", Platform::Gb), Core::Gambatte);
    assert_eq!(core_for_platform(d.path(), "Zelda", Platform::Gb), Core::Gambatte);
    assert_eq!(core_for_platform(d.path(), "Emerald", Platform::Gba), Core::Gpsp);
}

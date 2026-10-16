#![cfg(not(target_arch = "wasm32"))]

use slot_cart_studio::choice;

#[test]
fn merging_gives_what_slot_writes() {
    let start = "# keep\nOther = auto 112233 solid\nA = auto 000000 solid\n";
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join(slot_store::CART_SHELL_FILE);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, start).unwrap();
    slot_store::ini::write(
        d.path(),
        slot_store::CART_SHELL_FILE,
        "A",
        "rounded 86b9bf glitter",
    )
    .unwrap();
    slot_store::ini::write(
        d.path(),
        slot_store::CART_SHELL_FILE,
        "B",
        "auto abcdef clear",
    )
    .unwrap();
    let slot = std::fs::read_to_string(&file).unwrap();
    let ours = choice::merge_cart_shells(
        start,
        &["A".into(), "B".into()],
        &["rounded 86b9bf glitter".into(), "auto abcdef clear".into()],
    )
    .unwrap();
    assert_eq!(ours, slot);
    let removed = choice::merge_cart_shells(&ours, &["A".into()], &["".into()]).unwrap();
    assert!(!removed.contains("\nA = ") && removed.contains("# keep\nOther = auto 112233 solid\n"));
}

#[test]
fn nfc_mismatched_stems_are_still_the_same_cart() {
    let start = "Poke\u{301}mon Probe = auto 000000 solid\n";
    let stem = "Pok\u{e9}mon Probe";
    let set = choice::merge_cart_shells(
        start,
        &[stem.to_string()],
        &["rounded 86b9bf glitter".to_string()],
    )
    .unwrap();
    assert_eq!(set.matches(" = ").count(), 1, "{set:?}");
    assert!(set.contains("rounded 86b9bf glitter"), "{set:?}");

    let removed = choice::merge_cart_shells(&set, &[stem.to_string()], &["".to_string()]).unwrap();
    assert_eq!(removed, "");
}

#[test]
fn only_lines_that_parse_are_choices() {
    let got = choice::cart_shells(
        "# c\nA = rounded 86B9BF glitter\nB = rounded\nC = auto 000000 solid\n",
    );
    let mut pairs: Vec<(String, String)> = got
        .chunks(2)
        .map(|p| (p[0].clone(), p[1].clone()))
        .collect();
    pairs.sort();
    assert_eq!(
        pairs,
        [
            ("A".to_string(), "rounded 86b9bf glitter".to_string()),
            ("C".to_string(), "auto 000000 solid".to_string()),
        ]
    );
}

#[test]
fn a_stem_the_file_cannot_key_is_refused() {
    for stem in ["[Hack] Pokemon", "#1 Game", ";x", "a=b", " Edge", "Edge "] {
        assert!(!choice::shell_key_ok(stem), "{stem:?}");
    }
    for stem in [
        "Metal Gear Solid",
        "Pokémon Probe",
        "Advance Wars 2 - Black Hole Rising",
    ] {
        assert!(choice::shell_key_ok(stem), "{stem:?}");
    }
    assert!(choice::merge_cart_shells(
        "",
        &["[Hack] Pokemon".into()],
        &["auto 000000 solid".into()]
    )
    .is_err());
}

#[test]
fn automatic_is_what_slot_would_draw_and_presets_are_values() {
    let mut head = vec![0u8; 0x150];
    head[0x13f..0x143].copy_from_slice(b"BYTE");
    head[0x143] = 0xc0;
    head[0x14a] = 1;
    assert_eq!(
        choice::auto_shell(slot_store::Platform::Gbc, "", &head),
        "auto 86b9bf glitter"
    );
    assert_eq!(
        choice::auto_shell(slot_store::Platform::Gba, "AMTE", &[]),
        "auto 35353a solid"
    );
    let presets = choice::presets();
    assert_eq!(presets.len(), 25);
    for p in presets {
        let (_, value) = p.split_once('\t').expect("name\\tvalue");
        assert!(slot_store::ShellChoice::parse(value).is_some(), "{value}");
    }
}

#[test]
fn the_labels_file_lies_over_config_as_slot_reads_them() {
    let got = choice::layered_cart_shells(
        "Both = rounded 111111 solid\nConfigOnly = notched 222222 clear\nBackToAuto = rounded 333333 glitter\n",
        "Both = auto 444444 clear\nLabelsOnly = auto 555555 solid\nBackToAuto = auto\n",
    );
    let mut pairs: Vec<(String, String)> = got
        .chunks(2)
        .map(|p| (p[0].clone(), p[1].clone()))
        .collect();
    pairs.sort();
    assert_eq!(
        pairs,
        [
            ("Both".to_string(), "auto 444444 clear".to_string()),
            ("ConfigOnly".to_string(), "notched 222222 clear".to_string()),
            ("LabelsOnly".to_string(), "auto 555555 solid".to_string()),
        ]
    );
}

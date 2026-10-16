#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Cable,
    Wireless,
}

impl LinkKind {
    pub fn other(self) -> LinkKind {
        match self {
            LinkKind::Cable => LinkKind::Wireless,
            LinkKind::Wireless => LinkKind::Cable,
        }
    }
}

const WIRELESS: [&str; 51] = [
    "B2WE", "B3AE", "B4UE", "B4UP", "B85A", "B85P", "BDGE", "BDGP", "BG3E", "BKRJ", "BMGD", "BMGE",
    "BMGF", "BMGI", "BMGJ", "BMGP", "BMGS", "BMGU", "BPED", "BPEE", "BPEF", "BPEI", "BPEJ", "BPES",
    "BPGD", "BPGE", "BPGF", "BPGI", "BPGJ", "BPGS", "BPRD", "BPRE", "BPRF", "BPRI", "BPRJ", "BPRS",
    "BR5E", "BR6E", "BRBE", "BRKE",
    "BR5P", "BR6P", "BRBP", "BRKP",
    "BR5J", "BR6J", "BRBJ", "BRKJ", "BTME", "BTMJ", "BTMP",
];

/// RFU requires actual GBA Wireless Adapter support. Excludes Ruby,
/// Sapphire, Advance Wars, Mario Bros., ordinary Cable and bonus transfers.
pub fn has_wireless_adapter(code: &str) -> bool {
    WIRELESS.contains(&code)
}

/// Protocols implemented by our *pinned* libretro gpSP serial core, not
/// every two-cartridge feature that works on original GBA hardware.
/// See gpsp/gba_over.h at 5819380c2ffb0900219d700a382ee68c464ebb99.
/// Generic Cable (Golden Sun, BN1-4, Mario Bros., Medabots...) is not
/// implemented by gpSP's Pokemon/Advance Wars serial emulators.
pub fn gpsp_runtime_mode_supported(code: &str, mode: LinkKind) -> bool {
    match mode {
        LinkKind::Wireless => has_wireless_adapter(code),
        LinkKind::Cable => {
            let pokemon_rfu = has_wireless_adapter(code)
                && ["BPR", "BPG", "BPE"].iter().any(|p| code.starts_with(p));
            let pokemon_original_cable = matches!(code,
                "AXVE" | "AXVJ" | "AXVD" | "AXVI" | "AXVS" | "AXVF"
                | "AXPE" | "AXPJ" | "AXPD" | "AXPI" | "AXPS" | "AXPF");
            pokemon_rfu || pokemon_original_cable
                || matches!(code, "AWRE" | "AWRP" | "AW2E" | "AW2P")
        }
    }
}

pub fn link_kind(code: &str, title: &str, clean: bool) -> LinkKind {
    if pokemon(code, title) {
        let retail = clean
            && WIRELESS.contains(&code)
            && ["POKEMON FIRE", "POKEMON LEAF", "POKEMON EMER"].contains(&title);
        return if retail {
            LinkKind::Wireless
        } else {
            LinkKind::Cable
        };
    }
    if WIRELESS.contains(&code) {
        LinkKind::Wireless
    } else {
        LinkKind::Cable
    }
}

/// Menu preference only: where both gpSP backends are usable, show the
/// original Cable Link first. Retain RFU-only cartridges and the existing
/// automatic core configuration for hardware that cannot do Cable.
pub fn preferred_mode(code: &str, title: &str, clean: bool) -> LinkKind {
    let automatic = link_kind(code, title, clean);
    if gpsp_runtime_mode_supported(code, LinkKind::Cable) {
        LinkKind::Cable
    } else { automatic }
}

pub fn serial_option(chosen: LinkKind, auto: LinkKind, code: &str, title: &str) -> &'static str {
    if chosen == auto {
        // These EU/JP Battle Network 5/6 IDs support genuine RFU on GBA,
        // but are missing FLAGS_SERIAL|FLAGS_RFU overrides in the pinned
        // gpSP gba_over.h. Force the existing gpSP RFU backend explicitly:
        // leaving "auto" would silently DISABLE serial communication.
        if chosen == LinkKind::Wireless && matches!(code,
            "BRBP" | "BRKP" | "BRBJ" | "BRKJ"
            | "BR5P" | "BR6P" | "BR5J" | "BR6J") {
            return "rfu";
        }
        return "auto";
    }
    match chosen {
        LinkKind::Wireless => "rfu",
        LinkKind::Cable if pokemon(code, title) => "mul_poke",
        LinkKind::Cable if code.starts_with("AWR") => "mul_aw1",
        LinkKind::Cable if code.starts_with("AW2") => "mul_aw2",
        LinkKind::Cable => "auto",
    }
}

pub fn link_carried(code: &str, title: &str) -> bool {
    WIRELESS.contains(&code)
        || pokemon(code, title)
        || code.starts_with("AWR")
        || code.starts_with("AW2")
        || ["A3XE","A6BE","B4WE","B4BE","B4WP","B4BP",
            "BRBE","BRKE","BRBP","BRKP","BRBJ","BRKJ",
            "BR5E","BR6E","BR5P","BR6P","BR5J","BR6J",
            "A3XJ","A6BJ","B4WJ","B4BJ",
            "A8BE","A9BE","AK8E","AK9E","AGSE","AGFE",
            "AREE","AE2E","BZ3E",
            "AMAE","AMAP","AMAJ","AA2E","AA2P","AA2J",
            "A3AE","A3AP","A3AJ","AX4E","AX4P","AX4J",
            "A88E","A88P","A88J"].contains(&code)
}

fn pokemon(code: &str, title: &str) -> bool {
    title.starts_with("POKEMON")
        || ["AXV", "AXP", "BPE", "BPR", "BPG"]
            .iter()
            .any(|p| code.starts_with(p))
}

#[cfg(test)]
mod a6_serial_regressions {
    use super::*;

    #[test]
    fn gp_sp_menu_prefers_cable_only_when_real_backend_supports_it() {
        for (code, title) in [
            ("BPRE", "POKEMON FIRE"), ("BPGE", "POKEMON LEAF"),
            ("BPEE", "POKEMON EMER"), ("AXVE", "POKEMON RUBY"),
        ] {
            assert_eq!(preferred_mode(code, title, true), LinkKind::Cable);
        }
        assert_eq!(preferred_mode("BRBE", "MEGAMAN BN5", true), LinkKind::Wireless);
        assert_eq!(preferred_mode("AWRE", "ADVANCE WARS", true), LinkKind::Cable);
        assert_eq!(link_kind("BPRE", "POKEMON FIRE", true), LinkKind::Wireless,
            "retain original auto profile for selecting gpsp_serial correctly");
    }

    #[test]
    fn retail_pokemon_rfu_and_hacked_or_older_cable_stay_distinct() {
        // Preserve upstream original/fork semantics: clean retail FireRed,
        // LeafGreen and Emerald default to RFU. A modified ROM with the
        // same four-letter code must not silently inherit that assumption.
        for (code, title) in [
            ("BPRE", "POKEMON FIRE"),
            ("BPGE", "POKEMON LEAF"),
            ("BPEE", "POKEMON EMER"),
        ] {
            assert_eq!(link_kind(code, title, true), LinkKind::Wireless);
            assert_eq!(serial_option(LinkKind::Wireless, LinkKind::Wireless, code, title), "auto");
            assert_eq!(serial_option(LinkKind::Cable, LinkKind::Wireless, code, title), "mul_poke");
            assert_eq!(link_kind(code, title, false), LinkKind::Cable);
            assert_eq!(serial_option(LinkKind::Wireless, LinkKind::Cable, code, title), "rfu");
            assert!(link_carried(code, title));
        }
        assert_eq!(link_kind("AXVE", "POKEMON RUBY", true), LinkKind::Cable);
        assert_eq!(serial_option(LinkKind::Cable, LinkKind::Cable,
            "AXVE", "POKEMON RUBY"), "auto");
    }

    #[test]
    fn game_specific_cable_modes_are_not_confused_with_lan_transport() {
        assert_eq!(serial_option(LinkKind::Cable, LinkKind::Wireless,
            "AWRE", "ADVANCE WARS"), "mul_aw1");
        assert_eq!(serial_option(LinkKind::Cable, LinkKind::Wireless,
            "AW2E", "ADVANCE WARS 2"), "mul_aw2");
        assert_eq!(serial_option(LinkKind::Wireless, LinkKind::Cable,
            "AWRE", "ADVANCE WARS"), "rfu");
        assert_eq!(serial_option(LinkKind::Cable, LinkKind::Cable,
            "AWRE", "ADVANCE WARS"), "auto");
        assert!(link_carried("AWRE", "ADVANCE WARS"));
        assert!(link_carried("AW2E", "ADVANCE WARS 2"));
        assert!(!link_carried("NONE", "UNSUPPORTED"));
        assert!(!has_wireless_adapter("AXVE"));
        assert!(!has_wireless_adapter("AXPE"));
        assert!(!has_wireless_adapter("AWRE"));
        assert!(!has_wireless_adapter("AMAE"));
        assert!(has_wireless_adapter("BPRE"));
        assert!(has_wireless_adapter("BPEE"));
        assert!(has_wireless_adapter("BRBE"));
        assert!(has_wireless_adapter("BR5E"));
        for code in ["BRBP","BRKP","BRBJ","BRKJ","BR5P","BR6P","BR5J","BR6J"] {
            assert!(has_wireless_adapter(code), "retail EU/JP adapter support: {code}");
        }
        assert_eq!(link_kind("NONE", "UNSUPPORTED", true), LinkKind::Cable);
        for code in ["BPRE", "BPGE", "BPEE", "AXVE", "AXPE", "AWRE", "AW2E"] {
            assert!(gpsp_runtime_mode_supported(code, LinkKind::Cable));
        }
        for code in ["BPRE", "BPGE", "BPEE", "BRBE", "BR5E", "BRBP", "BR5J"] {
            assert!(gpsp_runtime_mode_supported(code, LinkKind::Wireless));
        }
        for code in ["AXVE", "AXPE", "A3XE", "B4WE", "AGSE", "A88E"] {
            assert!(!gpsp_runtime_mode_supported(code, LinkKind::Wireless));
        }
        for code in ["AREE","AE2E","AMAE","A88E","A3XE","A6BE",
                     "B4WE","B4BE","AGSE","AGFE","BZ3E","BRBE","BR5E"] {
            assert!(!gpsp_runtime_mode_supported(code, LinkKind::Cable),
                "no gpSP generic GBA cable serial backend: {code}");
        }
        assert_eq!(serial_option(LinkKind::Wireless, LinkKind::Wireless,
            "BRBP", "MEGAMAN BN5"), "rfu");
        assert_eq!(serial_option(LinkKind::Wireless, LinkKind::Wireless,
            "BR6J", "MEGAMAN BN6"), "rfu");
    }
}

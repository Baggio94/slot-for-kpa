//! Explicit original-GBA cross-cartridge compatibility. A shared mDNS key
//! does not prove a working game link: KMLB/3 authenticates both cartridge IDs,
//! core recipe, BIOS and mode. Each ROM retains its own SHA256 and SRAM.
use crate::link_kind::LinkKind;
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Family { Pokemon3, Bn1To2, Bn3, Bn4, Bn5, Bn6, Medabots, MedabotsAx, GoldenSun, MarioBros }
fn family(code: &str) -> Option<Family> {
    if code.len()!=4 || !code.bytes().all(|c|c.is_ascii_uppercase()||c.is_ascii_digit()) {return None;}
    if matches!(code.as_bytes()[3],b'E'|b'P'|b'D'|b'F'|b'I'|b'S'|b'J') &&
        ["BPR","BPG","BPE","AXV","AXP"].iter().any(|x|code.starts_with(x)) {return Some(Family::Pokemon3);}
    match code {
        // BN1 -> BN2 is one-directional Retro Chip Trader data exchange.
        "AREE"|"AE2E"=>Some(Family::Bn1To2),
        // Mario Bros. multicart works across all four Super Mario Advance
        // titles and Mario & Luigi Superstar Saga, but NOT a multiboot guest.
        "AMAE"|"AMAP"|"AMAJ"|"AA2E"|"AA2P"|"AA2J"|
        "A3AE"|"A3AP"|"A3AJ"|"AX4E"|"AX4P"|"AX4J"|
        "A88E"|"A88P"|"A88J"=>Some(Family::MarioBros),
        "A3XE"|"A6BE"|"A3XJ"|"A6BJ"=>Some(Family::Bn3),
        "B4WE"|"B4BE"|"B4WP"|"B4BP"|"B4WJ"|"B4BJ"=>Some(Family::Bn4),
        "BRBE"|"BRKE"|"BRBP"|"BRKP"|"BRBJ"|"BRKJ"=>Some(Family::Bn5),
        "BR5E"|"BR6E"|"BR5P"|"BR6P"|"BR5J"|"BR6J"=>Some(Family::Bn6),
        "A8BE"|"A9BE"=>Some(Family::Medabots),
        "AK8E"|"AK9E"=>Some(Family::MedabotsAx),
        "AGSE"|"AGFE"=>Some(Family::GoldenSun),
        _=>None,
    }
}
fn rfu(code:&str)->bool {
    (family(code)==Some(Family::Pokemon3) && ["BPR","BPG","BPE"].iter().any(|p|code.starts_with(p)))
        || matches!(family(code),Some(Family::Bn5|Family::Bn6))
}
/// Four-character DNS-SD grouping key; unknown titles retain exact game code.
pub fn discovery_key(code:&str)->&str {
    if code == "BZ3E" { return "BN4A"; } // Mega Man Zero 3 bonus transfer with BN4
    match family(code){
        Some(Family::Pokemon3)=>"PK3A",Some(Family::Bn1To2)=>"BN12",
        Some(Family::MarioBros)=>"MARI",Some(Family::Bn3)=>"BN3A",
        Some(Family::Bn4)=>"BN4A",Some(Family::Bn5)=>"BN5A",
        Some(Family::Bn6)=>"BN6A",Some(Family::Medabots)=>"MDBT",
        Some(Family::MedabotsAx)=>"MDAX",Some(Family::GoldenSun)=>"GSUN",
        None=>code,
    }
}
/// Only *different* original retail codes. Same code still needs identical
/// ROM SHA256. mGBA dual-core SLK1 does not allow mixed ROMs: KMLB/3
/// additionally restricts different codes to gpSP netpacket.
pub fn compatible_cross_title(a:[u8;4], b:[u8;4], mode:LinkKind)->bool {
    if a==b{return false;}
    let (Ok(a),Ok(b))=(std::str::from_utf8(&a),std::str::from_utf8(&b)) else{return false;};
    // Bonus transfer: Mega Man Zero 3 <-> Battle Network 4 (both versions).
    // Mode is strictly GBA Cable, never Wireless.
    if mode==LinkKind::Cable && ((a=="BZ3E" && family(b)==Some(Family::Bn4))
        || (b=="BZ3E" && family(a)==Some(Family::Bn4))) {return true;}
    let Some(group)=family(a) else{return false;};
    family(b)==Some(group) && (mode==LinkKind::Cable || (rfu(a)&&rfu(b)))
}
/// Runtime subset of the original GBA cross-cartridge catalogue. The
/// remaining families stay documented for a future generic Cable backend,
/// but KMLB/3 must not say LINKED when pinned gpSP cannot emulate them.
pub fn runnable_gpsp_cross_title(a:[u8;4], b:[u8;4], mode:LinkKind)->bool {
    if !compatible_cross_title(a, b, mode) { return false; }
    let (Ok(a), Ok(b))=(std::str::from_utf8(&a), std::str::from_utf8(&b)) else {
        return false;
    };
    crate::link_kind::gpsp_runtime_mode_supported(a, mode)
        && crate::link_kind::gpsp_runtime_mode_supported(b, mode)
}

#[cfg(test)] mod tests{
    use super::*;
    fn id(s:&str)->[u8;4]{s.as_bytes().try_into().unwrap()}
    #[test] fn pokemon_pairs(){
        for (a,b) in [("BPRE","BPGE"),("BPGE","BPEE"),("BPRE","BPEE"),
                      ("BPRE","AXVE"),("AXVE","AXPE"),("AXPE","BPEE")]{
            assert!(compatible_cross_title(id(a),id(b),LinkKind::Cable));
            assert_eq!(compatible_cross_title(id(a),id(b),LinkKind::Wireless),!a.starts_with("AX")&&!b.starts_with("AX"));
            assert_eq!(discovery_key(a),discovery_key(b));
        }
    }
    #[test] fn documented_non_pokemon_pairs(){
        for (a,b,wireless) in [("A3XE","A6BE",false),("B4WE","B4BE",false),
                              ("BRBE","BRKE",true),("BRBP","BRKP",true),("BRBJ","BRKJ",true),
                              ("BR5E","BR6E",true),("BR5P","BR6P",true),
                              ("BR5J","BR6J",true),
                              ("A8BE","A9BE",false),("AK8E","AK9E",false),
                              ("AGSE","AGFE",false)]{
            assert!(compatible_cross_title(id(a),id(b),LinkKind::Cable));
            assert_eq!(compatible_cross_title(id(a),id(b),LinkKind::Wireless),wireless);
            assert_eq!(discovery_key(a),discovery_key(b));
        }
    }
    #[test] fn retro_chip_trader_mario_bros_and_zero_three_crossovers(){
        for (a,b) in [("AREE","AE2E"),("AMAE","AA2P"),("AA2E","A3AE"),
                      ("A3AP","AX4E"),("AX4E","A88E"),
                      ("B4WE","BZ3E"),("B4BE","BZ3E")] {
            assert!(compatible_cross_title(id(a),id(b),LinkKind::Cable),
                    "verified hardware link must be discoverable");
            assert!(!compatible_cross_title(id(a),id(b),LinkKind::Wireless),
                    "these cross-game transfers do not use Wireless Adapter");
            assert_eq!(discovery_key(a),discovery_key(b));
        }
    }
    #[test] fn hardware_catalogue_does_not_overpromise_unsupported_gpsp_protocols(){
        for (a,b) in [("AREE","AE2E"),("AMAE","AA2E"),("A3XE","A6BE"),
                      ("B4WE","B4BE"),("AGSE","AGFE"),("A8BE","A9BE"),
                      ("B4WE","BZ3E"),("BRBE","BRKE"),("BR5E","BR6E")] {
            assert!(compatible_cross_title(id(a), id(b), LinkKind::Cable));
            assert!(!runnable_gpsp_cross_title(id(a), id(b), LinkKind::Cable),
                "original GBA Cable compatibility is not gpSP's generic serial support");
        }
        for (a,b) in [("BPRE","BPGE"),("BPRE","BPEE"),("AXVE","AXPE"),
                      ("AXVE","BPRE")] {
            assert!(runnable_gpsp_cross_title(id(a), id(b), LinkKind::Cable));
        }
        for (a,b) in [("BRBE","BRKE"),("BR5E","BR6E"),
                      ("BRBP","BRKP"),("BR5J","BR6J"),("BPRE","BPGE")] {
            assert!(runnable_gpsp_cross_title(id(a), id(b), LinkKind::Wireless));
        }
        assert!(!runnable_gpsp_cross_title(id("AXVE"),id("BPGE"),LinkKind::Wireless));
    }
    #[test] fn unknown_and_unsafe_cross_families_stay_closed(){
        for(a,b)in[("BRBE","BR5E"),("A8BE","AK8E"),("AGSE","BPRE"),
                    ("AAAA","AAAB"),("A3XE","B4WE"),("AXVE","BR6E"),
                    ("BPR1","BPGE"),("BPRe","BPGE"),("BPRE","BPRE")]{
            assert!(!compatible_cross_title(id(a),id(b),LinkKind::Cable));
        }
    }
}

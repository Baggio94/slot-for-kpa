//! Cartridge identification for Slot LAN matching.
//! Read the bounded official header, never read/hash a full ROM on the GL thread.
//! A GBA cart carries its own four-letter game code. A GB/GBC cart has none,
//! so its code is derived from the header (see `gb_code`).
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use crate::multiplayer::game_code;

pub struct GbaIdentity {
    pub code: String,
    pub title: String,
    pub clean_header: bool,
}

fn parse(buf: &[u8]) -> io::Result<GbaIdentity> {
    if buf.len() < 0xb3 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "GBA ROM header truncated"));
    }
    let code = std::str::from_utf8(&buf[0xac..0xb0])
        .ok().and_then(game_code)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid GBA game code"))?;
    let title_bytes = &buf[0xa0..0xac];
    let title = String::from_utf8_lossy(title_bytes)
        .trim_matches(|c: char| c == '\0' || c == ' ').to_string();
    if title.is_empty() || !title.is_ascii() || title.chars().any(|c| c.is_control()) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid GBA title"));
    }
    // gpSP chooses RFU for retail Pokémon but Cable for some modified carts.
    let clean_header = buf[3] == 0xea && buf[0xb2] == 0x96;
    Ok(GbaIdentity { code, title, clean_header })
}

pub fn identify(path: &Path) -> io::Result<GbaIdentity> {
    let mut head = [0u8; 0xb3];
    File::open(path)?.read_exact(&mut head)?;
    parse(&head)
}

const GB_HEADER: usize = 0x150;
const BASE36: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Pokémon Red/Blue/Green/Yellow and Gold/Silver/Crystal trade and battle with
/// each other (Gen 2 through its Time Capsule), but a Japanese cart must never
/// meet an overseas one: the two save formats differ and a trade corrupts them.
fn gb_pokemon(title: &str) -> bool {
    ["POKEMON RED", "POKEMON BLUE", "POKEMON GREEN", "POKEMON YELLOW",
     "POKEMON_GLD", "POKEMON_SLV", "PM_CRYSTAL"].iter().any(|t| title.starts_with(t))
}

/// The four-character Link code of a GB/GBC header. GBA codes never start
/// with a digit, so a Game Boy never meets a GBA on the network:
/// "0P"/"0Q" + 2 is Pokémon (overseas/Japan) and may link with another
/// Pokémon cart of its region; "1" + 3 is any other game, which links with
/// the same ROM only. The rest is a hash of the header, whose checksums
/// tell revisions apart.
fn gb_code(head: &[u8]) -> Option<(String, String)> {
    let head = head.get(..GB_HEADER)?;
    let checksum = head[0x134..0x14d].iter()
        .fold(0u8, |c, b| c.wrapping_sub(*b).wrapping_sub(1));
    // The boot ROM refuses a cart whose header checksum is wrong, so a real
    // cart always passes; a GBA image or random data almost never does.
    if head[0x104..0x108] != [0xce, 0xed, 0x66, 0x66] || head[0x14d] != checksum {
        return None;
    }
    let title: String = head[0x134..0x144].iter()
        .take_while(|b| (0x20..0x7f).contains(*b))
        .map(|b| *b as char).collect::<String>().trim().to_string();
    if title.is_empty() { return None; }
    let hash = head[0x134..GB_HEADER].iter()
        .fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(*b)).wrapping_mul(0x0100_0193));
    let digit = |n: u32| BASE36[(n % 36) as usize] as char;
    let code = if gb_pokemon(&title) {
        let region = if head[0x14a] == 0 { 'Q' } else { 'P' };
        format!("0{region}{}{}", digit(hash), digit(hash / 36))
    } else {
        format!("1{}{}{}", digit(hash), digit(hash / 36), digit(hash / 1296))
    };
    Some((code, title))
}

/// A minimal GB/GBC image with a valid header, for tests.
#[cfg(test)]
pub(crate) fn test_gb_rom(title: &str, japan: bool, size: usize, seed: u8) -> Vec<u8> {
    let mut rom = vec![seed; size.max(GB_HEADER)];
    rom[0x104..0x108].copy_from_slice(&[0xce, 0xed, 0x66, 0x66]);
    rom[0x134..0x144].fill(0);
    rom[0x134..0x134 + title.len()].copy_from_slice(title.as_bytes());
    rom[0x14a] = if japan { 0 } else { 1 };
    rom[0x14d] = rom[0x134..0x14d].iter()
        .fold(0u8, |c, b| c.wrapping_sub(*b).wrapping_sub(1));
    rom
}

pub fn is_gb_code(code: &[u8]) -> bool {
    matches!(code.first(), Some(b'0' | b'1'))
}

/// A NES, SNES or Mega Drive cartridge: "2" and three base-36 digits of its
/// image. The two consoles still compare the whole ROM's SHA-256 before
/// playing; this code only lets them find each other.
pub fn identify_home(path: &Path) -> io::Result<GbaIdentity> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut head = Vec::with_capacity(HOME_SAMPLE);
    file.by_ref().take(HOME_SAMPLE as u64).read_to_end(&mut head)?;
    if head.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty cartridge"));
    }
    let hash = head.iter().chain(size.to_le_bytes().iter())
        .fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(*b)).wrapping_mul(0x0100_0193));
    let digit = |n: u32| BASE36[(n % 36) as usize] as char;
    let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(GbaIdentity {
        code: format!("2{}{}{}", digit(hash), digit(hash / 36), digit(hash / 1296)),
        title, clean_header: true,
    })
}

const HOME_SAMPLE: usize = 256 * 1024;

pub fn identify_gb(path: &Path) -> io::Result<GbaIdentity> {
    let mut head = [0u8; GB_HEADER];
    File::open(path)?.read_exact(&mut head)?;
    let (code, title) = gb_code(&head).ok_or_else(||
        io::Error::new(io::ErrorKind::InvalidData, "invalid GB/GBC header"))?;
    Ok(GbaIdentity { code, title, clean_header: true })
}

/// The Link code a whole ROM image carries, read the same way as its header
/// was when the code was negotiated.
pub fn rom_code(rom: &[u8], gb: bool) -> Option<[u8; 4]> {
    if gb {
        gb_code(rom)?.0.as_bytes().try_into().ok()
    } else {
        rom.get(0xac..0xb0)?.try_into().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_gba_header_is_network_identifier() {
        let mut head = [0u8; 0xb3];
        head[0xa0..0xac].copy_from_slice(b"POKEMON FIRE");
        head[0xac..0xb0].copy_from_slice(b"BPRE");
        head[3] = 0xea;
        head[0xb2] = 0x96;
        let id = parse(&head).unwrap();
        assert_eq!(id.code, "BPRE");
        assert_eq!(id.title, "POKEMON FIRE");
        assert!(id.clean_header);
        head[0xb2] = 0;
        assert!(!parse(&head).unwrap().clean_header);
    }
    #[test]
    fn empty_invalid_and_short_headers_are_rejected_without_guessing() {
        assert!(parse(&[]).is_err());
        let mut head = [0u8; 0xb3];
        head[0xa0..0xa4].copy_from_slice(b"GAME");
        head[0xac..0xb0].copy_from_slice(b"bpre");
        assert!(parse(&head).is_err());
        head[0xac..0xb0].copy_from_slice(b"BPRE");
        head[0xa0..0xa4].fill(0);
        assert!(parse(&head).is_err());
    }

    fn gb_head(title: &str, japan: bool) -> Vec<u8> {
        test_gb_rom(title, japan, GB_HEADER, 0)
    }

    #[test]
    fn gb_codes_never_look_like_gba_codes_and_follow_the_header() {
        let tetris = gb_head("TETRIS", false);
        let (code, title) = gb_code(&tetris).unwrap();
        assert_eq!(title, "TETRIS");
        assert!(code.starts_with('1') && code.len() == 4);
        assert!(game_code(&code).is_some(), "a GB code must pass the wire check");
        assert!(is_gb_code(code.as_bytes()) && !is_gb_code(b"BPRE"));
        assert_eq!(rom_code(&tetris, true), Some(code.as_bytes().try_into().unwrap()));
        let mut revision = tetris.clone();
        revision[0x14c] = 1;
        revision[0x14d] = revision[0x14d].wrapping_sub(1);
        assert_ne!(gb_code(&revision).unwrap().0, code, "a revision is another ROM");
    }

    #[test]
    fn pokemon_carts_share_a_family_per_region() {
        for title in ["POKEMON RED", "POKEMON BLUE", "POKEMON YELLOW", "POKEMON_GLDAAUE",
                      "POKEMON_SLVAAXF", "PM_CRYSTALBYTE"] {
            assert!(gb_code(&gb_head(title, false)).unwrap().0.starts_with("0P"), "{title}");
            assert!(gb_code(&gb_head(title, true)).unwrap().0.starts_with("0Q"), "{title}");
        }
        assert!(gb_code(&gb_head("POKEMON PINBALL", false)).unwrap().0.starts_with('1'));
    }

    #[test]
    fn non_gb_data_has_no_gb_code() {
        let mut bad = gb_head("TETRIS", false);
        bad[0x14d] ^= 1;
        assert!(gb_code(&bad).is_none(), "wrong header checksum");
        assert!(gb_code(&[0u8; 0x100]).is_none(), "too short");
        assert!(gb_code(&gb_head("", false)).is_none(), "no title");
    }

    #[test]
    fn home_cartridges_have_their_own_codes() {
        let dir = std::env::temp_dir().join(format!("slot-home-code-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.sfc"), dir.join("b.sfc"));
        std::fs::write(&a, vec![1u8; 4096]).unwrap();
        std::fs::write(&b, vec![2u8; 4096]).unwrap();
        let (x, y) = (identify_home(&a).unwrap(), identify_home(&b).unwrap());
        assert!(x.code.starts_with('2') && game_code(&x.code).is_some());
        assert_ne!(x.code, y.code);
        assert_eq!(x.code, identify_home(&a).unwrap().code);
        assert!(!is_gb_code(x.code.as_bytes()));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
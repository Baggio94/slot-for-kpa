//! GBA-only cartridge identification for Slot LAN matching.
//! Read the bounded official header, never read/hash a full ROM on the GL thread.
//! GB/GBC link identity needs a different header and is deliberately not guessed.
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
}

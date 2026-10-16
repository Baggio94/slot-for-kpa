/// FireRed/LeafGreen/Ruby/Sapphire/Emerald write alternating 14-sector
/// FLASH1M save banks. A save is durable only when *every* sector of at
/// least one bank has a unique section ID 0..13, a matching generation
/// counter, the 0x08012025 signature and the official additive checksum.
/// Their data is zero-padded to 3968 bytes; summing the whole data region
/// exactly matches pret/pokefirered's variable-section checksum.
/// This is NOT applied to unrelated 128 KiB games or other save types.
pub(crate) fn pokemon_flash_complete_bank_count(bytes: &[u8]) -> usize {
    const BANK_SECTORS: usize = 14;
    const SECTOR: usize = 4096;
    const SIGNATURE: u32 = 0x0801_2025;
    if bytes.len() != 128 * 1024 { return 0; }
    (0usize..2).filter(|&bank| {
        let mut seen = 0u16;
        let mut counter = None;
        for sector_index in 0..BANK_SECTORS {
            let at = (bank * BANK_SECTORS + sector_index) * SECTOR;
            let sector = &bytes[at..at + SECTOR];
            let id = u16::from_le_bytes([sector[0xFF4], sector[0xFF5]]);
            let expected = u16::from_le_bytes([sector[0xFF6], sector[0xFF7]]);
            let signature = u32::from_le_bytes(sector[0xFF8..0xFFC].try_into().unwrap());
            let generation = u32::from_le_bytes(sector[0xFFC..0x1000].try_into().unwrap());
            if signature != SIGNATURE || id >= BANK_SECTORS as u16 ||
                seen & (1 << id) != 0 || counter.is_some_and(|old| old != generation) {
                return false;
            }
            let total = sector[..3968].chunks_exact(4).fold(0u32, |sum, chunk| {
                sum.wrapping_add(u32::from_le_bytes(chunk.try_into().unwrap()))
            });
            let checksum = ((total >> 16) + (total & 0xFFFF)) as u16;
            if checksum != expected { return false; }
            seen |= 1 << id;
            counter = Some(generation);
        }
        seen == (1 << BANK_SECTORS) - 1
    }).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    const SECTOR: usize = 4096;
    fn one_valid_bank() -> Vec<u8> {
        // Synthetic Pokémon FLASH1M sector identities/checksums. Two-bank
        // presence, not file length alone, establishes a committed save.
        let mut bytes = vec![0u8; 128 * 1024];
        for i in 0..14usize {
            let s = &mut bytes[i * SECTOR..(i + 1) * SECTOR];
            s[0xFF4..0xFF6].copy_from_slice(&(i as u16).to_le_bytes());
            s[0xFF6..0xFF8].copy_from_slice(&0u16.to_le_bytes());
            s[0xFF8..0xFFC].copy_from_slice(&0x0801_2025u32.to_le_bytes());
            s[0xFFC..0x1000].copy_from_slice(&23u32.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn accepts_a_complete_committed_bank_and_two_valid_banks() {
        let mut image = one_valid_bank();
        assert_eq!(pokemon_flash_complete_bank_count(&image), 1);
        image.copy_within(0..14 * SECTOR, 14 * SECTOR);
        assert_eq!(pokemon_flash_complete_bank_count(&image), 2);
    }
    #[test]
    fn rejects_missing_sector_checksum_and_mixed_generations() {
        let mut image = one_valid_bank();
        image[2 * SECTOR + 0xFF8] ^= 1;
        assert_eq!(pokemon_flash_complete_bank_count(&image), 0);
        let mut image = one_valid_bank();
        image[3 * SECTOR] ^= 1;
        assert_eq!(pokemon_flash_complete_bank_count(&image), 0);
        let mut image = one_valid_bank();
        image[4 * SECTOR + 0xFFC] = 24;
        assert_eq!(pokemon_flash_complete_bank_count(&image), 0);
        assert_eq!(pokemon_flash_complete_bank_count(&vec![0xFF; 128 * 1024]), 0);
        assert_eq!(pokemon_flash_complete_bank_count(&vec![0; 64 * 1024]), 0);
    }
}

const MAGIC: &[u8; 4] = b"SLK1";

pub fn is_pair(state: &[u8]) -> bool {
    state.starts_with(MAGIC)
}

pub fn local(state: &[u8], player: u8) -> Option<Vec<u8>> {
    let mut rest = state.strip_prefix(MAGIC)?;
    let mut parts = Vec::with_capacity(2);
    for _ in 0..2 {
        let len = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
        parts.push(rest.get(4..4 + len)?);
        rest = &rest[4 + len..];
    }
    rest.is_empty()
        .then(|| parts.get(usize::from(player)).map(|p| p.to_vec()))
        .flatten()
}

pub fn pair(state: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    for _ in 0..2 {
        out.extend_from_slice(&(state.len() as u32).to_le_bytes());
        out.extend_from_slice(state);
    }
    out
}

/// Mixed-cartridge bootstrap: replace ONLY the local player's GBA
/// with that player's existing solo checkpoint. The other GBA retains the
/// fresh state produced by booting its OWN different cartridge. Never
/// unserialize FireRed progress into LeafGreen or vice versa.
pub fn with_local_player(pair: &[u8], player: u8, own: &[u8]) -> Option<Vec<u8>> {
    const LIMIT: usize = 16 * 1024 * 1024;
    if player > 1 || own.len() < 0x400 || own.starts_with(MAGIC) { return None; }
    let mut states = [local(pair, 0)?, local(pair, 1)?];
    states[usize::from(player)] = own.to_vec();
    let size = 12usize.checked_add(states[0].len())?.checked_add(states[1].len())?;
    if size > LIMIT || states.iter().any(|x| x.len() < 0x400) { return None; }
    let mut result = Vec::with_capacity(size);
    result.extend_from_slice(MAGIC);
    for single in &states {
        result.extend_from_slice(&u32::try_from(single.len()).ok()?.to_le_bytes());
        result.extend_from_slice(single);
    }
    Some(result)
}

/// Build the *first* linked snapshot with player 0's in-progress Host
/// cartridge and player 1's own Join cartridge. It is never a solo/RA state.
/// Neither player's save is overwritten: the transport supplies the guest's
/// personal single-GBA state before the host broadcasts the combined pair.
pub fn combine_players(host_pair: &[u8], join_single: &[u8]) -> Option<Vec<u8>> {
    const MAX_LINK_STATE: usize = 16 * 1024 * 1024;
    let host_single = local(host_pair, 0)?;
    // mGBA's ordinary GBA state contains a 0x400-byte header at minimum.
    // Reject another SLK1 pair, truncated packets, overflow and huge payloads.
    if host_single.len() < 0x400 || join_single.len() < 0x400
        || join_single.starts_with(MAGIC)
    {
        return None;
    }
    let total = 12usize.checked_add(host_single.len())?.checked_add(join_single.len())?;
    if total > MAX_LINK_STATE { return None; }
    let mut combined = Vec::with_capacity(total);
    combined.extend_from_slice(MAGIC);
    for part in [&host_single[..], join_single] {
        combined.extend_from_slice(&u32::try_from(part.len()).ok()?.to_le_bytes());
        combined.extend_from_slice(part);
    }
    Some(combined)
}

fn bios(state: &[u8]) -> Option<u32> {
    let gba = if is_pair(state) {
        local(state, 0)?
    } else {
        state.to_vec()
    };
    (gba.len() >= 0x400).then(|| u32::from_le_bytes(gba[4..8].try_into().unwrap()))
}

pub fn same_bios(a: &[u8], b: &[u8]) -> bool {
    match (bios(a), bios(b)) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_gives_each_player_the_state_back() {
        let pair = pair(b"gba");
        assert!(is_pair(&pair));
        assert_eq!(local(&pair, 0).as_deref(), Some(&b"gba"[..]));
        assert_eq!(local(&pair, 1).as_deref(), Some(&b"gba"[..]));
    }

    #[test]
    fn each_player_gets_their_own_gba() {
        let mut state = MAGIC.to_vec();
        for gba in [&b"zero"[..], &b"one!"[..]] {
            state.extend_from_slice(&4u32.to_le_bytes());
            state.extend_from_slice(gba);
        }
        assert_eq!(local(&state, 0).as_deref(), Some(&b"zero"[..]));
        assert_eq!(local(&state, 1).as_deref(), Some(&b"one!"[..]));
    }

    #[test]
    fn anything_but_a_whole_link_state_has_no_local_gba() {
        let whole = pair(b"gba");
        assert_eq!(local(b"a one-GBA state", 0), None);
        assert_eq!(local(&whole[..whole.len() - 1], 0), None);
        assert_eq!(local(&[whole.as_slice(), b"x"].concat(), 0), None);
        assert_eq!(local(&whole, 2), None);
    }

    #[test]
    fn mixed_cart_boot_preserves_the_other_game_state() {
        let original_host = vec![0x10; 0x1100];
        let original_guest = vec![0x20; 0x1800];
        let candidate = combine_players(&pair(&original_host), &original_guest).unwrap();
        let local_host = vec![0x33; 0x1000];
        let swapped_host = with_local_player(&candidate, 0, &local_host).unwrap();
        assert_eq!(local(&swapped_host, 0).unwrap(), local_host);
        assert_eq!(local(&swapped_host, 1).unwrap(), original_guest);
        let local_guest = vec![0x44; 0x1400];
        let swapped_guest = with_local_player(&candidate, 1, &local_guest).unwrap();
        assert_eq!(local(&swapped_guest, 0).unwrap(), original_host);
        assert_eq!(local(&swapped_guest, 1).unwrap(), local_guest);
        assert!(with_local_player(&candidate, 2, &local_guest).is_none());
        assert!(with_local_player(&candidate, 0, &pair(&local_host)).is_none());
        assert!(with_local_player(&candidate, 1, &[0; 12]).is_none());
    }

    #[test]
    fn combined_link_begins_with_each_players_own_progress() {
        let host = vec![0x11u8; 0x1400];
        let join = vec![0x22u8; 0x1400];
        let result = combine_players(&pair(&host), &join).unwrap();
        assert_eq!(local(&result, 0).unwrap(), host);
        assert_eq!(local(&result, 1).unwrap(), join);
        assert_ne!(local(&result, 0), local(&result, 1));
    }

    #[test]
    fn combined_link_rejects_corrupt_and_oversized_inputs() {
        let host = pair(&vec![0x11u8; 0x400]);
        assert!(combine_players(&host, &[0; 3]).is_none());
        assert!(combine_players(&host, &pair(&vec![0u8; 0x400])).is_none());
        assert!(combine_players(&host, &vec![0; 16 * 1024 * 1024]).is_none());
        assert!(combine_players(b"not a pair", &vec![0; 0x400]).is_none());
    }

    fn gba(bios: u32) -> Vec<u8> {
        let mut state = vec![0u8; 0x400];
        state[4..8].copy_from_slice(&bios.to_le_bytes());
        state
    }

    #[test]
    fn states_on_the_same_bios_match_whether_paired_or_not() {
        assert!(same_bios(&pair(&gba(0xBAAE187F)), &gba(0xBAAE187F)));
        assert!(same_bios(&gba(1), &pair(&gba(1))));
    }

    #[test]
    fn states_on_different_bioses_do_not() {
        assert!(!same_bios(&pair(&gba(0xBAAE187F)), &pair(&gba(0xA6473709))));
    }

    #[test]
    fn a_state_too_short_to_be_a_gba_says_nothing() {
        assert!(same_bios(&0u64.to_le_bytes(), &1u64.to_le_bytes()));
    }
}

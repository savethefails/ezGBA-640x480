//! mGBA's link mode saves both GBAs as one state: `SLK1`, then for each player a little-endian
//! u32 length and that GBA's state. The card only ever holds the local player's GBA.

const MAGIC: &[u8; 4] = b"SLK1";

pub fn is_pair(state: &[u8]) -> bool {
    state.starts_with(MAGIC)
}

/// `player`'s GBA out of a link state. Panics on anything but a whole one: every pair comes from
/// mGBA's link mode, so a malformed one is a bug, not something to carry on past.
pub fn local(state: &[u8], player: u8) -> Vec<u8> {
    assert!(
        player < 2,
        "a link state has two players, not {}",
        player + 1
    );
    let mut rest = state.strip_prefix(MAGIC).expect("not a link state");
    let mut parts = Vec::with_capacity(2);
    for _ in 0..2 {
        assert!(rest.len() >= 4, "a link state cut short");
        let len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        assert!(rest.len() >= 4 + len, "a link state cut short");
        parts.push(&rest[4..4 + len]);
        rest = &rest[4 + len..];
    }
    assert!(rest.is_empty(), "{} bytes after a link state", rest.len());
    parts[usize::from(player)].to_vec()
}

/// A one-GBA state as a link state, the same GBA for both players.
pub fn pair(state: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    for _ in 0..2 {
        out.extend_from_slice(&(state.len() as u32).to_le_bytes());
        out.extend_from_slice(state);
    }
    out
}

/// Whether two link states were made on the same BIOS: the checksum mGBA writes at byte 4 of
/// each GBA's state, read off player 1's.
pub fn same_bios(a: &[u8], b: &[u8]) -> bool {
    let bios = |state: &[u8]| {
        let gba = local(state, 0);
        assert!(gba.len() >= 0x400, "a {} byte GBA state", gba.len());
        u32::from_le_bytes(gba[4..8].try_into().unwrap())
    };
    bios(a) == bios(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_gives_each_player_the_state_back() {
        let pair = pair(b"gba");
        assert!(is_pair(&pair));
        assert_eq!(local(&pair, 0), b"gba");
        assert_eq!(local(&pair, 1), b"gba");
    }

    #[test]
    fn each_player_gets_their_own_gba() {
        let mut state = MAGIC.to_vec();
        for gba in [&b"zero"[..], &b"one!"[..]] {
            state.extend_from_slice(&4u32.to_le_bytes());
            state.extend_from_slice(gba);
        }
        assert_eq!(local(&state, 0), b"zero");
        assert_eq!(local(&state, 1), b"one!");
    }

    #[test]
    #[should_panic(expected = "not a link state")]
    fn a_one_gba_state_is_not_a_link_state() {
        local(b"a one-GBA state", 0);
    }

    #[test]
    #[should_panic(expected = "cut short")]
    fn a_link_state_cut_short_fails_loudly() {
        let whole = pair(b"gba");
        local(&whole[..whole.len() - 1], 0);
    }

    #[test]
    #[should_panic(expected = "after a link state")]
    fn trailing_bytes_fail_loudly() {
        local(&[pair(b"gba").as_slice(), b"x"].concat(), 0);
    }

    fn gba(bios: u32) -> Vec<u8> {
        let mut state = vec![0u8; 0x400];
        state[4..8].copy_from_slice(&bios.to_le_bytes());
        pair(&state)
    }

    #[test]
    fn link_states_on_the_same_bios_match_and_on_different_ones_do_not() {
        assert!(same_bios(&gba(0xBAAE187F), &gba(0xBAAE187F)));
        assert!(!same_bios(&gba(0xBAAE187F), &gba(0xA6473709)));
    }
}

//! Gambatte save states across its versions. The format is a list of
//! labelled items (label, NUL, 24-bit big-endian length, bytes) in label
//! order, but the core refuses any state whose size isn't its own. Since
//! May 2026 it saves two Sachen mapper items more: a state from an older
//! RetroArch Gambatte gets them added (0 outside Sachen carts), and slot.
//! leaves them out again when the RetroArch state it replaces had none.

/// Added by gambatte-libretro 4ba4c59/4c1b4c2, one byte each.
const SACHEN: [&[u8]; 2] = [b"schnlct", b"schnomk"];

struct Item {
    label: std::ops::Range<usize>,
    end: usize,
}

/// The items after the version and snapshot header, or None if `raw` isn't
/// a complete Gambatte state.
fn items(raw: &[u8]) -> Option<(usize, Vec<Item>)> {
    let len24 = |at: usize| -> Option<usize> {
        let b = raw.get(at..at + 3)?;
        Some((b[0] as usize) << 16 | (b[1] as usize) << 8 | b[2] as usize)
    };
    if raw.first() != Some(&0) || raw.len() < 5 {
        return None;
    }
    let body = 5 + len24(2)?;
    let mut items = Vec::new();
    let mut at = body;
    while at < raw.len() {
        let nul = at + raw.get(at..)?.iter().position(|&b| b == 0)?;
        let end = nul + 4 + len24(nul + 1)?;
        if end > raw.len() || nul == at {
            return None;
        }
        items.push(Item { label: at..nul, end });
        at = end;
    }
    Some((body, items))
}

fn has(raw: &[u8], items: &[Item], label: &[u8]) -> bool {
    items.iter().any(|item| &raw[item.label.clone()] == label)
}

/// Whether `raw` is a Gambatte state saved before the Sachen items.
pub fn lacks_sachen(raw: &[u8]) -> bool {
    items(raw).is_some_and(|(_, items)| SACHEN.iter().all(|l| !has(raw, &items, l)))
}

/// Add the Sachen items, in label order, to an older state.
pub fn add_sachen(raw: &[u8]) -> Option<Vec<u8>> {
    let (body, items) = items(raw)?;
    if SACHEN.iter().any(|l| has(raw, &items, l)) {
        return None;
    }
    let mut out = raw[..body].to_vec();
    let mut pending = SACHEN.iter().peekable();
    let mut start = body;
    for item in &items {
        while let Some(label) = pending.next_if(|l: &&&[u8]| **l < &raw[item.label.clone()]) {
            out.extend_from_slice(label);
            out.extend_from_slice(&[0, 0, 0, 1, 0]);
        }
        out.extend_from_slice(&raw[start..item.end]);
        start = item.end;
    }
    for label in pending {
        out.extend_from_slice(label);
        out.extend_from_slice(&[0, 0, 0, 1, 0]);
    }
    Some(out)
}

/// Remove the Sachen items, for an older RetroArch Gambatte.
pub fn strip_sachen(raw: &[u8]) -> Option<Vec<u8>> {
    let (body, items) = items(raw)?;
    let mut out = raw[..body].to_vec();
    let mut start = body;
    for item in &items {
        if !SACHEN.contains(&&raw[item.label.clone()]) {
            out.extend_from_slice(&raw[start..item.end]);
        }
        start = item.end;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &[u8], value: &[u8]) -> Vec<u8> {
        let n = value.len();
        [label, &[0, (n >> 16) as u8, (n >> 8) as u8, n as u8], value].concat()
    }

    fn state(labels: &[&[u8]]) -> Vec<u8> {
        let mut raw = vec![0, 1, 0, 0, 0];
        for label in labels {
            raw.extend(item(label, if label.starts_with(b"schn") { &[0] } else { &[7, 7] }));
        }
        raw
    }

    #[test]
    fn an_older_state_gets_exactly_the_current_layout() {
        let old = state(&[b"cc" as &[u8], b"sp", b"sram"]);
        let new = state(&[b"cc" as &[u8], b"schnlct", b"schnomk", b"sp", b"sram"]);
        assert!(lacks_sachen(&old) && !lacks_sachen(&new));
        assert_eq!(add_sachen(&old).unwrap(), new);
        assert_eq!(new.len() - old.len(), 24);
        assert_eq!(strip_sachen(&new).unwrap(), old);
        assert_eq!(add_sachen(&new), None);
    }

    #[test]
    fn anything_else_is_left_alone() {
        assert!(!lacks_sachen(b"RASTATE"));
        assert_eq!(add_sachen(&[1, 2, 3]), None);
        let mut cut = state(&[b"cc" as &[u8], b"sp"]);
        cut.pop();
        assert_eq!(add_sachen(&cut), None);
    }
}

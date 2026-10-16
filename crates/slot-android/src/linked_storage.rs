//! Strictly private, immutable-by-other-session mGBA Cable save namespaces.
//!
//! Historical MLab save directories used .../<game>/<role>/ directly.
//! Every new connection starts a fresh .../<game>/<role>/sessions/<nonce>/.
//! Do not migrate/delete any earlier state or import a previous Link session's
//! battery data when resuming a live solo cartridge. Solo SRAM/RTC are COPIED
//! into the new session, never moved.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_SOLO_SNAPSHOT: usize = 7 * 1024 * 1024;

pub const MAX_LINK_BATTERY_SEED: usize = 1024 * 1024;

/// Copy a live solo SRAM or RTC to one private Link session, atomically.
/// Missing battery files are normal, but corrupt, unreadable, oversized and
/// already seeded files MUST NOT silently become the starting multiplayer
/// save. In particular, do not overwrite any historical Link data.
pub fn copy_solo_battery(source: &Path, target: &Path) -> io::Result<()> {
    if target.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists,
            "Private Link battery seed is not empty"));
    }
    let bytes = match fs::read(source) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if bytes.len() > MAX_LINK_BATTERY_SEED {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            "Oversized Link battery seed"));
    }
    slot_store::atomic_write(target, &bytes)
}

pub fn validate_solo_snapshot(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 0x400 || bytes.len() > MAX_SOLO_SNAPSHOT || bytes.starts_with(b"SLK1") {
        return Err(format!(
            "Unsafe solo-to-Link mGBA snapshot ({} bytes or pair header)",
            bytes.len()
        ));
    }
    Ok(())
}

/// Atomic create_dir is our inter-session lock. Each call allocates an
/// independent namespace, including if a previous connection failed or
/// exited without saving. No rename/delete of historical session data.
pub fn fresh_session(root: &Path, game_id: &str, role: &str) -> io::Result<PathBuf> {
    fresh_session_for_core(root, "mGBA", game_id, role)
}

/// gpSP netpacket links also need per-connection SRAM/RTC isolation. Accept
/// only known core namespaces so no caller can manufacture a save path.
pub fn fresh_session_for_core(root: &Path, core: &str, game_id: &str,
                              role: &str) -> io::Result<PathBuf> {
    if !matches!(core, "mGBA" | "gpSP") {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid Link core"));
    }
    if game_id.is_empty() || !game_id.bytes().all(|b| b.is_ascii_hexdigit()) ||
        !matches!(role, "host" | "join") {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid Link session identity"));
    }
    let role_root = root.join("Multiplayer").join(core).join(game_id).join(role);
    let session_root = role_root.join("sessions");
    fs::create_dir_all(&session_root)?;
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
        .as_micros();
    for attempt in 0..1024u16 {
        let dir = session_root.join(format!("session-{epoch}-{attempt:03}"));
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists,
        "No unused Multiplayer session directory available"))
}


/** A single intended file replacement in a Link -> solo promotion.
 * A None 'after' means leave the file alone (for cartridges without RTC).
 * The expected value is the exact immutable pre-Link image, NOT the
 * currently observed disk image.
 */
pub struct PromotionFile<'a> {
    pub path: &'a Path,
    pub expected: Option<&'a [u8]>,
    pub after: Option<&'a [u8]>,
}

/// Never promote private linked progress over a solo save modified by
/// RetroArch, Syncthing or another thread since the cable was connected.
/// Stage an immutable, unique rollback generation before the first write.
/// One failed atomic write rolls back already written destinations; the
/// backup directory remains available even if the rollback itself fails.
pub fn promote_player_files(backup_root: &Path,
                            items: &[PromotionFile<'_>]) -> Result<PathBuf, String> {
    if items.is_empty() || items.len() > 4 {
        return Err("Link promotion needs 1..=4 owned files".into());
    }
    let mut previous = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        if items[..i].iter().any(|before| before.path == item.path) {
            return Err("Duplicate destination in Link promotion".into());
        }
        let old = read_optional(item.path).map_err(|e|
            format!("Cannot inspect original solo file {}: {e}", item.path.display()))?;
        if old.as_deref() != item.expected {
            return Err(format!(
                "Solo save changed during Link ({}); private Link progress retained",
                item.path.display()));
        }
        if item.after.is_some_and(|bytes| bytes.is_empty() || bytes.len() > MAX_LINK_BATTERY_SEED * 8) {
            return Err("Refusing empty or oversized Link promotion".into());
        }
        previous.push(old);
    }
    let history = fresh_promotion_backup(backup_root)?;
    // Store each original as an immutable copy, plus the promoted image.
    // Even an originally absent file is described by the zero-length marker.
    for (i, (item, old)) in items.iter().zip(&previous).enumerate() {
        let label = format!("file-{i}");
        let before = history.join(format!("{label}.before"));
        let absent = history.join(format!("{label}.absent"));
        match old {
            Some(bytes) => slot_store::atomic_write(&before, bytes),
            None => slot_store::atomic_write(&absent, b""),
        }.map_err(|e| format!("Cannot stage Link rollback: {e}"))?;
        if let Some(new) = item.after {
            slot_store::atomic_write(&history.join(format!("{label}.after")), new)
                .map_err(|e| format!("Cannot stage Link promotion data: {e}"))?;
        }
        slot_store::atomic_write(&history.join(format!("{label}.destination")),
            item.path.to_string_lossy().as_bytes())
            .map_err(|e| format!("Cannot stage Link destination record: {e}"))?;
    }
    let mut completed = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Some(after) = item.after else { continue };
        // Recheck just before each mutation to guard against external edits
        // made while writing the backups.
        let now = read_optional(item.path).map_err(|e| e.to_string())?;
        if now.as_deref() != item.expected {
            let rollback = rollback_promotion(items, &previous, &completed);
            return Err(format!("Solo save changed mid-promotion; rollback={rollback:?}, backup={}",
                history.display()));
        }
        if let Err(error) = slot_store::atomic_write(item.path, after) {
            let rollback = rollback_promotion(items, &previous, &completed);
            return Err(format!("Cannot promote Link file {i}: {error}; rollback={rollback:?}, backup={}",
                history.display()));
        }
        completed.push(i);
    }
    Ok(history)
}

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn rollback_promotion(items: &[PromotionFile<'_>], previous: &[Option<Vec<u8>>],
                      completed: &[usize]) -> Result<(), String> {
    for &i in completed.iter().rev() {
        match &previous[i] {
            Some(old) => slot_store::atomic_write(items[i].path, old)
                .map_err(|e| format!("Rollback of file {i} failed: {e}"))?,
            None => match fs::remove_file(items[i].path) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => return Err(format!("Rollback delete file {i} failed: {e}")),
            },
        }
    }
    Ok(())
}

fn fresh_promotion_backup(root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|e| format!("Cannot create Link backups: {e}"))?;
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?.as_nanos();
    for i in 0..1024 {
        let dir = root.join(format!("promotion-{epoch}-{i:03}"));
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(format!("Cannot allocate Link rollback: {e}")),
        }
    }
    Err("No fresh Link rollback generation available".into())
}

#[cfg(test)]
mod tests {

    #[test]
    fn link_promotion_keeps_immutable_original_and_prevents_stale_writer() {
        let base = fixture();
        let solo = base.join("Saves/gpSP/LeafGreen.srm");
        fs::create_dir_all(solo.parent().unwrap()).unwrap();
        let old = vec![1u8; 131072];
        let next = vec![2u8; 131072];
        fs::write(&solo, &old).unwrap();
        let path = promote_player_files(&base.join("Saves/LinkBackups"), &[
            PromotionFile { path: &solo, expected: Some(&old), after: Some(&next) },
        ]).unwrap();
        assert_eq!(fs::read(&solo).unwrap(), next);
        assert_eq!(fs::read(path.join("file-0.before")).unwrap(), old);
        assert_eq!(fs::read(path.join("file-0.after")).unwrap(), next);
        assert!(promote_player_files(&base.join("Saves/LinkBackups"), &[
            PromotionFile { path: &solo, expected: Some(&old), after: Some(&old) },
        ]).is_err());
        assert_eq!(fs::read(&solo).unwrap(), next);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn link_promotion_is_all_or_nothing_on_preexisting_conflicts() {
        let base = fixture();
        let ram = base.join("Saves/mGBA/Cart.srm");
        let state = base.join("Saves/Cart.mgba.state");
        fs::create_dir_all(ram.parent().unwrap()).unwrap();
        fs::write(&ram, b"ROM A saved").unwrap();
        fs::write(&state, b"old state").unwrap();
        let result = promote_player_files(&base.join("Saves/LinkBackups"), &[
            PromotionFile { path: &ram, expected: Some(b"ROM A saved"), after: Some(b"new SRAM") },
            PromotionFile { path: &state, expected: Some(b"stale state"), after: Some(b"new state") },
        ]);
        assert!(result.unwrap_err().contains("changed during Link"));
        assert_eq!(fs::read(&ram).unwrap(), b"ROM A saved");
        assert_eq!(fs::read(&state).unwrap(), b"old state");
        fs::remove_dir_all(base).unwrap();
    }


    use super::*;

    fn fixture() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "slot-a6-link-sessions-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        ));
        fs::create_dir(&base).unwrap();
        base
    }

    #[test]
    fn accepts_only_one_bounded_solo_state() {
        assert!(validate_solo_snapshot(&vec![0u8; 0x400]).is_ok());
        assert!(validate_solo_snapshot(&vec![0u8; MAX_SOLO_SNAPSHOT]).is_ok());
        assert!(validate_solo_snapshot(&[0u8; 0x3ff]).is_err());
        assert!(validate_solo_snapshot(&vec![0u8; MAX_SOLO_SNAPSHOT + 1]).is_err());
        let mut paired = b"SLK1".to_vec();
        paired.resize(0x404, 0);
        assert!(validate_solo_snapshot(&paired).is_err());
    }

    #[test]
    fn atomic_private_battery_seed_never_mutates_solo_or_old_sessions() {
        let base = fixture();
        let solo = base.join("solo.srm");
        fs::write(&solo, b"solo SRAM before link").unwrap();
        let first = fresh_session_for_core(&base, "gpSP", "abcdef0", "host").unwrap();
        let first_sram = first.join("Pokemon.srm");
        copy_solo_battery(&solo, &first_sram).unwrap();
        assert_eq!(fs::read(&first_sram).unwrap(), b"solo SRAM before link");

        fs::write(&solo, b"new solo SRAM next session").unwrap();
        let second = fresh_session_for_core(&base, "gpSP", "abcdef0", "host").unwrap();
        let second_sram = second.join("Pokemon.srm");
        copy_solo_battery(&solo, &second_sram).unwrap();
        assert_eq!(fs::read(&first_sram).unwrap(), b"solo SRAM before link");
        assert_eq!(fs::read(&second_sram).unwrap(), b"new solo SRAM next session");
        assert_eq!(fs::read(&solo).unwrap(), b"new solo SRAM next session");

        assert_eq!(copy_solo_battery(&solo, &first_sram).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&first_sram).unwrap(), b"solo SRAM before link");

        // A missing RTC does not create a fake battery clock; oversized
        // sources fail closed without affecting historical or solo data.
        let absent_rtc = base.join("absent.rtc");
        let private_rtc = second.join("Pokemon.rtc");
        copy_solo_battery(&absent_rtc, &private_rtc).unwrap();
        assert!(!private_rtc.exists());
        let huge = base.join("oversized.srm");
        fs::write(&huge, vec![0xAB; MAX_LINK_BATTERY_SEED + 1]).unwrap();
        let rejected = second.join("oversized.srm");
        assert_eq!(copy_solo_battery(&huge, &rejected).unwrap_err().kind(),
            io::ErrorKind::InvalidData);
        assert!(!rejected.exists());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn fresh_link_sessions_never_reuse_legacy_or_previous_battery_files() {
        let base = fixture();
        let role = base.join("Multiplayer/mGBA/abcdef0/host");
        fs::create_dir_all(&role).unwrap();
        let old = role.join("Pokemon.srm");
        fs::write(&old, b"historic multiplayer SRAM").unwrap();
        let one = fresh_session(&base, "abcdef0", "host").unwrap();
        fs::write(one.join("Pokemon.srm"), b"first session").unwrap();
        let two = fresh_session(&base, "abcdef0", "host").unwrap();
        assert_ne!(one, two);
        assert!(!two.join("Pokemon.srm").exists());
        assert_eq!(fs::read(old).unwrap(), b"historic multiplayer SRAM");
        assert_eq!(fs::read(one.join("Pokemon.srm")).unwrap(), b"first session");
        let guest = fresh_session(&base, "abcdef0", "join").unwrap();
        assert_ne!(two, guest);
        assert!(fresh_session(&base, "../bad", "host").is_err());
        assert!(fresh_session(&base, "abcdef0", "other").is_err());
        let gpsp_host = fresh_session_for_core(&base, "gpSP", "abcdef0", "host").unwrap();
        let gpsp_join = fresh_session_for_core(&base, "gpSP", "abcdef0", "join").unwrap();
        assert!(gpsp_host.starts_with(base.join("Multiplayer/gpSP/abcdef0/host/sessions")));
        assert!(gpsp_join.starts_with(base.join("Multiplayer/gpSP/abcdef0/join/sessions")));
        assert_ne!(gpsp_host, gpsp_join);
        assert!(fresh_session_for_core(&base, "../escape", "abcdef0", "host").is_err());
        assert!(fresh_session_for_core(&base, "gpSP", "../escape", "host").is_err());
        fs::remove_dir_all(base).unwrap();
    }
}

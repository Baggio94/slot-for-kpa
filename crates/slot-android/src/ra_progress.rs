//! RetroAchievements progress under the selected cartridge. Android's
//! RaSmartCache writes the SAF URI -> full-ROM MD5 index next to the
//! achievement data; this reads it on a worker, never on the GL thread.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const INDEX_FILE: &str = "ra-smart-rom-index-v1.json";
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;
// The Android scan keeps adding hashes in the background; reread at most
// once a minute while the shelf asks for a URI it does not know yet.
const RELOAD_AFTER: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct RomHashes {
    map: Arc<Mutex<HashMap<String, String>>>,
    loading: Arc<AtomicBool>,
    loaded_at: Option<Instant>,
}

impl RomHashes {
    pub fn hash(&mut self, root: &Path, uri: &str) -> Option<String> {
        if let Some(hash) = self.map.lock().unwrap_or_else(|e| e.into_inner()).get(uri) {
            return Some(hash.clone());
        }
        if self.loaded_at.is_none_or(|at| at.elapsed() >= RELOAD_AFTER)
            && !self.loading.swap(true, Ordering::AcqRel)
        {
            self.loaded_at = Some(Instant::now());
            let (map, loading, file) = (self.map.clone(), self.loading.clone(), root.join(INDEX_FILE));
            let spawned = std::thread::Builder::new().name("slot-ra-index".into()).spawn(move || {
                if let Some(read) = read_index(&file) {
                    *map.lock().unwrap_or_else(|e| e.into_inner()) = read;
                }
                loading.store(false, Ordering::Release);
            });
            if spawned.is_err() {
                self.loading.store(false, Ordering::Release);
            }
        }
        None
    }
}

fn read_index(file: &PathBuf) -> Option<HashMap<String, String>> {
    if !file.metadata().is_ok_and(|m| (1..=MAX_INDEX_BYTES).contains(&m.len())) {
        return None;
    }
    parse_index(&std::fs::read_to_string(file).ok()?)
}

fn parse_index(text: &str) -> Option<HashMap<String, String>> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    if json.get("schema")?.as_i64()? != 1 {
        return None;
    }
    Some(json.get("entries")?.as_array()?.iter().filter_map(|entry| {
        let uri = entry.get("uri")?.as_str()?;
        let md5 = entry.get("md5")?.as_str()?;
        (uri.starts_with("content://") && md5.len() == 32
            && md5.bytes().all(|c| c.is_ascii_hexdigit()))
            .then(|| (uri.to_owned(), md5.to_ascii_lowercase()))
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_keeps_only_valid_saf_uris_and_md5s() {
        let map = parse_index(r#"{"schema":1,"root":"t","entries":[
            {"uri":"content://a","size":1,"modified":2,"md5":"0123456789ABCDEF0123456789abcdef"},
            {"uri":"file:///b","md5":"0123456789abcdef0123456789abcdef"},
            {"uri":"content://c","md5":"short"}]}"#).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["content://a"], "0123456789abcdef0123456789abcdef");
        assert!(parse_index(r#"{"schema":2,"entries":[]}"#).is_none());
    }

    #[test]
    fn unknown_uri_loads_the_index_off_thread_then_answers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(INDEX_FILE), r#"{"schema":1,"entries":[
            {"uri":"content://a","md5":"0123456789abcdef0123456789abcdef"}]}"#).unwrap();
        let mut hashes = RomHashes::default();
        assert_eq!(hashes.hash(dir.path(), "content://a"), None);
        let deadline = Instant::now() + Duration::from_secs(5);
        while hashes.loading.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(hashes.hash(dir.path(), "content://a").as_deref(),
            Some("0123456789abcdef0123456789abcdef"));
        assert_eq!(hashes.hash(dir.path(), "content://zz"), None, "no reload storm inside a minute");
        assert!(!hashes.loading.load(Ordering::Acquire));
    }
}

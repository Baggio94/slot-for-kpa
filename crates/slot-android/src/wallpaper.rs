use std::path::{Path, PathBuf};

/// Upstream Slot v1.13 Shelf wallpaper names, in this port's shelf order:
/// the six platform shelves (Platform::ALL), then Recents and Favorites.
pub const SHELF_KEYS: [&str; slot_store::Platform::ALL.len() + 2] = [
    "GBA", "GB", "GBC", "NES", "SNES", "MD", "Recents", "Favorites",
];

/// Shelf wallpapers are picked by shelf, never offered as a single wallpaper
/// nor dealt by Random.
fn is_shelf_file(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, _)| SHELF_KEYS.iter().any(|k| stem.eq_ignore_ascii_case(k)))
}

/// The file in Wallpapers/ that the Shelf wallpaper shows on shelf `index`,
/// matched case-insensitively like upstream (GBA.png, gba.PNG...).
pub fn for_shelf(root: &Path, index: usize) -> Option<String> {
    let key = SHELF_KEYS.get(index)?;
    pngs(&root.join("Wallpapers")).into_iter().find(|file| {
        file.rsplit_once('.').is_some_and(|(stem, _)| stem.eq_ignore_ascii_case(key))
    })
}

/// Where an imported wallpaper goes while Shelf is picked: <shelf>.png.
pub fn shelf_target(index: usize) -> Option<String> {
    SHELF_KEYS.get(index).map(|key| format!("{key}.png"))
}

pub fn list(root: &Path) -> Vec<String> {
    let mut files = pngs(&root.join("Wallpapers"));
    files.retain(|name| !is_shelf_file(name));
    files.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    files
}

pub fn pick(root: &Path, seed: u64) -> Option<PathBuf> {
    let dir = root.join("Wallpapers");
    let mut files = pngs(&dir);
    files.retain(|name| !is_shelf_file(name));
    if files.is_empty() {
        return None;
    }
    files.sort();
    let mut rng = Rng(seed);
    let path = root.join("Config").join("wallpaper.deck");
    let (mut shown, mut queued) = read_deck(&path);
    shown.retain(|n| files.contains(n));
    queued.retain(|n| files.contains(n));
    for n in &files {
        if !shown.contains(n) && !queued.contains(n) {
            let at = rng.below(queued.len() + 1);
            queued.insert(at, n.clone());
        }
    }
    if queued.is_empty() {
        let last = shown.pop();
        queued = files;
        for i in (1..queued.len()).rev() {
            queued.swap(i, rng.below(i + 1));
        }
        if queued.len() > 1 && last.as_ref() == queued.first() {
            let end = queued.len() - 1;
            queued.swap(0, end);
        }
        shown.clear();
    }
    let next = queued.remove(0);
    shown.push(next.clone());
    if let Err(e) = slot_store::atomic_write(&path, write_deck(&shown, &queued).as_bytes()) {
        eprintln!("slot: wallpaper.deck: {e}");
    }
    Some(dir.join(next))
}

fn pngs(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| !slot_store::is_hidden(p))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        .filter_map(|p| Some(p.file_name()?.to_str()?.to_owned()))
        .collect()
}

fn read_deck(path: &Path) -> (Vec<String>, Vec<String>) {
    let Some(text) = std::fs::read(path)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    else {
        return (Vec::new(), Vec::new());
    };
    let mut lines = text.lines();
    let Some(shown) = lines.next().and_then(|l| l.parse::<usize>().ok()) else {
        return (Vec::new(), Vec::new());
    };
    let mut order: Vec<String> = lines.map(str::to_owned).collect();
    let queued = order.split_off(shown.min(order.len()));
    (order, queued)
}

fn write_deck(shown: &[String], queued: &[String]) -> String {
    let mut text = format!("{}\n", shown.len());
    for n in shown.iter().chain(queued) {
        text.push_str(n);
        text.push('\n');
    }
    text
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z % n as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_wallpapers_follow_upstream_names_and_stay_out_of_the_list() {
        let root = std::env::temp_dir().join(format!(
            "slot-shelf-wallpaper-test-{}", std::process::id()));
        let dir = root.join("Wallpapers");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["gba.PNG", "GBC.png", "NES.png", "SNES.png", "MD.png", "Favorites.png", "user.png"] {
            std::fs::write(dir.join(name), b"png").unwrap();
        }
        assert_eq!(for_shelf(&root, 0).as_deref(), Some("gba.PNG"));
        assert_eq!(for_shelf(&root, 1), None);
        assert_eq!(for_shelf(&root, 2).as_deref(), Some("GBC.png"));
        assert_eq!(for_shelf(&root, 3).as_deref(), Some("NES.png"));
        assert_eq!(for_shelf(&root, 4).as_deref(), Some("SNES.png"));
        assert_eq!(for_shelf(&root, 5).as_deref(), Some("MD.png"));
        assert_eq!(for_shelf(&root, 6), None);
        assert_eq!(for_shelf(&root, 7).as_deref(), Some("Favorites.png"));
        assert_eq!(for_shelf(&root, 8), None);
        assert_eq!(list(&root), vec!["user.png".to_string()]);
        assert_eq!(shelf_target(1).as_deref(), Some("GB.png"));
        assert_eq!(shelf_target(3).as_deref(), Some("NES.png"));
        assert_eq!(shelf_target(6).as_deref(), Some("Recents.png"));
        assert_eq!(shelf_target(7).as_deref(), Some("Favorites.png"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

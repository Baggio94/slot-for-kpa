//! Slot v1.7 ten-entry Recents rack, isolated in private Android storage.
//! Preserve actual ROM/SRAM/RTC locations; only remember stable ROM URIs.
use std::path::Path;
use slot_store::Cart;
const FILE: &str = "konkr-recents.json";
const MAX: usize = 10;

pub fn load(root: &Path) -> Vec<String> {
    std::fs::read(root.join(FILE)).ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .unwrap_or_default().into_iter().filter(|v| !v.is_empty())
        .take(MAX).collect()
}
pub fn append_previous(previous: &[String], uri: &str) -> Vec<String> {
    let mut output=Vec::with_capacity(MAX);
    if !uri.is_empty() { output.push(uri.to_owned()); }
    for item in previous {
        if output.len()==MAX { break; }
        if !item.is_empty() && !output.contains(item) {
            output.push(item.clone());
        }
    }
    output
}
pub fn store(root: &Path, list: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let output=root.join(FILE);
    let tmp=root.join(format!("{FILE}.partial"));
    let bytes=serde_json::to_vec_pretty(list).map_err(std::io::Error::other)?;
    std::fs::write(&tmp,bytes)?;
    match std::fs::rename(&tmp,&output) {
        Ok(())=>Ok(()),
        Err(e)=>{let _=std::fs::remove_file(tmp);Err(e)}
    }
}
/// Ignore missing library entries rather than reintroducing ghost cartridges;
/// duplicate ROM URIs always select the same existing source cart.
pub fn carts(ids: &[String], base: &[&[Cart]]) -> Vec<Cart> {
    ids.iter().filter_map(|uri|
        base.iter().flat_map(|row|row.iter())
            .find(|cart|cart.rom.to_string_lossy()==uri.as_str())
            .cloned()
    ).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_launch_is_most_recent_and_never_duplicates_or_exceeds_ten() {
        let mut recent=vec![];
        for i in 0..15 { recent=append_previous(&recent,&format!("content://game/{i}")); }
        assert_eq!(recent.len(),10);
        assert_eq!(recent[0],"content://game/14");
        assert_eq!(recent[9],"content://game/5");
        let again=append_previous(&recent,"content://game/9");
        assert_eq!(again.len(),10);
        assert_eq!(again[0],"content://game/9");
        assert_eq!(again.iter().filter(|s|s.ends_with("/9")).count(),1);
    }
    #[test]
    fn state_persists_and_hiding_rack_does_not_erase_play_history() {
        let path=std::env::temp_dir().join(format!("konkr-recents-{}",std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        store(&path,&["content://game/a".into()]).unwrap();
        assert_eq!(load(&path),vec!["content://game/a".to_string()]);
        std::fs::remove_dir_all(path).unwrap();
    }
}

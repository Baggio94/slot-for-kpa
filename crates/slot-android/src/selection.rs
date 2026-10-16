//! Durable last selected shelf and ROM, without touching game/save data.
use serde::{Serialize,Deserialize};
use std::path::Path;
const FILE: &str="konkr-shelf-focus.json";
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct Focus { pub rack: usize, pub uri: String }
pub fn load(root: &Path) -> Option<Focus> {
    std::fs::read(root.join(FILE)).ok()
        .and_then(|bytes|serde_json::from_slice::<Focus>(&bytes).ok())
        .filter(|focus|focus.rack<=4 && !focus.uri.is_empty())
}
pub fn store(root: &Path, focus: &Focus) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let temp=root.join(format!("{FILE}.partial"));
    let data=serde_json::to_vec(focus).map_err(std::io::Error::other)?;
    std::fs::write(&temp,data)?;
    let final_path=root.join(FILE);
    match std::fs::rename(&temp,&final_path) {
        Ok(())=>Ok(()),
        Err(e)=>{let _=std::fs::remove_file(&temp);Err(e)}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn persist_restore_and_ignore_missing_state() {
        let dir=std::env::temp_dir().join(format!("slot-focus-u2-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load(&dir),None);
        let focus=Focus { rack: 4,uri:"content://roms/emerald".into() };
        store(&dir,&focus).unwrap();
        assert_eq!(load(&dir),Some(focus));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
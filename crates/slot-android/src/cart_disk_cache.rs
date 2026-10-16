//! Compressed on-device cartridge-face cache. No GL calls, no ROM writes.
//! The renderer reads on background workers; a separate bounded writer stores
//! new faces atomically. Cache corruption simply falls back to rerendering.
use std::{fs, io::{self, Write}, path::{Path, PathBuf}, time::SystemTime};
use ring::digest::{digest, SHA256};
use slot_store::Cart;
use slot_ui::CartFace;

const MAGIC: &[u8;8] = b"SLOTCF02";
const MAX_FACE_BYTES: usize = 2 * 1024 * 1024;
const MAX_DISK_BYTES: u64 = 160 * 1024 * 1024;
const MAX_DISK_FILES: usize = 400;

pub(crate) struct DiskCache {
    dir: PathBuf,
    theme: Vec<u8>,
}
impl DiskCache {
    pub fn new(root: &Path) -> Self {
        Self {
            dir: root.join("Cache").join("cartridge-faces-qa102"),
            theme: fs::read(root.join("Config/theme.txt")).unwrap_or_default(),
        }
    }
    pub fn key(&self, cart: &Cart) -> String {
        // Rust Cart debug covers URI, platform, name, code, label and shell.
        // The revision invalidates old faces when rendering algorithms change.
        let mut input = format!("KONKR-cart-r102-v2:{cart:?}").into_bytes();
        input.extend_from_slice(&self.theme);
        for path in [Some(cart.rom.as_path()), cart.label.as_deref()].into_iter().flatten() {
            if let Ok(metadata)=fs::metadata(path) {
                input.extend_from_slice(&metadata.len().to_le_bytes());
                if let Ok(time)=metadata.modified().and_then(|m|
                    m.duration_since(SystemTime::UNIX_EPOCH)
                        .map_err(io::Error::other))
                {
                    input.extend_from_slice(&time.as_nanos().to_le_bytes());
                }
            }
        }
        let hash=digest(&SHA256,&input);
        hash.as_ref().iter().map(|b|format!("{b:02x}")).collect()
    }
    fn path(&self, key: &str) -> PathBuf { self.dir.join(format!("{key}.rgba.lz4")) }
    pub fn read(&self, key: &str) -> Option<CartFace> {
        let path=self.path(key);
        if fs::metadata(&path).ok()?.len() > MAX_FACE_BYTES as u64 + 64 {
            return None;
        }
        let data=fs::read(path).ok()?;
        if data.len()<20 || &data[..8]!=MAGIC {return None;}
        let w=u32::from_le_bytes(data[8..12].try_into().ok()?);
        let h=u32::from_le_bytes(data[12..16].try_into().ok()?);
        let expected=w.checked_mul(h)?.checked_mul(4)? as usize;
        if w==0 || h==0 || expected>MAX_FACE_BYTES || expected<4 {return None;}
        let size=u32::from_le_bytes(data[16..20].try_into().ok()?) as usize;
        if size!=expected {return None;}
        let rgba=lz4_flex::decompress_size_prepended(&data[16..]).ok()?;
        if rgba.len()!=expected{return None;}
        Some(CartFace {rgba,w,h})
    }
    pub fn write(&self, key:&str, face:&CartFace) -> io::Result<()> {
        let raw=face.w.checked_mul(face.h).and_then(|v|v.checked_mul(4))
            .ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"bad face size"))? as usize;
        if raw!=face.rgba.len() || raw>MAX_FACE_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"bad face pixels"));
        }
        fs::create_dir_all(&self.dir)?;
        let target=self.path(key);
        let temporary=self.dir.join(format!("{key}.tmp"));
        let packed=lz4_flex::compress_prepend_size(&face.rgba);
        let mut file=fs::File::create(&temporary)?;
        file.write_all(MAGIC)?;
        file.write_all(&face.w.to_le_bytes())?;
        file.write_all(&face.h.to_le_bytes())?;
        file.write_all(&packed)?;
        drop(file);
        if let Err(e)=fs::rename(&temporary,&target) {
            let _=fs::remove_file(&temporary);
            return Err(e);
        }
        Ok(())
    }
    /// Eviction is off the render/GLES thread; touch only this cache's files.
    pub fn trim(&self) {
        let Ok(files)=fs::read_dir(&self.dir) else {return};
        let mut entries=Vec::new();
        let mut bytes=0u64;
        for file in files.flatten() {
            if !file.file_name().to_string_lossy().ends_with(".rgba.lz4") {continue}
            let Ok(meta)=file.metadata() else {continue};
            if !meta.is_file() {continue}
            bytes=bytes.saturating_add(meta.len());
            entries.push((file.path(),meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),meta.len()));
        }
        if entries.len()<=MAX_DISK_FILES && bytes<=MAX_DISK_BYTES {return}
        entries.sort_by_key(|entry|entry.1);
        let mut count=entries.len();
        for (path,_,size) in entries {
            if count<=MAX_DISK_FILES && bytes<=MAX_DISK_BYTES {break}
            if fs::remove_file(path).is_ok() {
                count-=1;
                bytes=bytes.saturating_sub(size);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Cart {
        Cart {
            platform:slot_store::Platform::Gba,
            stem:"Metroid Fusion".into(),
            title:"Metroid Fusion".into(),
            code:"AMTE".into(),
            rom:"content://roms/metroid.gba".into(),
            shell:None,label:None,
        }
    }
    #[test]
    fn cache_roundtrip_and_invalid_keys() {
        let root=std::env::temp_dir().join(format!("slot-cart-qa102-{}",std::process::id()));
        let _=fs::remove_dir_all(&root);
        let cache=DiskCache::new(&root);
        let cart=sample();
        let key=cache.key(&cart);
        let face=CartFace {rgba:vec![1,2,3,255,4,5,6,200],w:2,h:1};
        cache.write(&key,&face).unwrap();
        let restored=cache.read(&key).unwrap();
        assert_eq!((restored.w,restored.h,restored.rgba),(2,1,face.rgba.clone()));
        let mut renamed=sample();renamed.stem.push('!');
        assert_ne!(cache.key(&renamed),key);
        let mut shell=sample();shell.code="BPRE".into();
        assert_ne!(cache.key(&shell),key);
        assert!(cache.read(&cache.key(&renamed)).is_none());
        fs::write(cache.path(&key),b"broken").unwrap();
        assert!(cache.read(&key).is_none());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupt_large_length_never_allocates() {
        let root=std::env::temp_dir().join(format!("slot-cart-bad-qa102-{}",std::process::id()));
        let _=fs::remove_dir_all(&root);
        let cache=DiskCache::new(&root);
        fs::create_dir_all(&cache.dir).unwrap();
        let key="bad";
        let mut payload=MAGIC.to_vec();
        payload.extend_from_slice(&u32::MAX.to_le_bytes());
        payload.extend_from_slice(&u32::MAX.to_le_bytes());
        payload.extend_from_slice(&u32::MAX.to_le_bytes());
        fs::write(cache.path(key),payload).unwrap();
        assert!(cache.read(key).is_none());
        fs::remove_dir_all(root).unwrap();
    }
}

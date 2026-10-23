//! Run-Ahead "second instance", as in RetroArch.
//!
//! Predicted frames run on a private copy of the core, never on the real
//! one. Each display frame the real core's state is copied into the copy,
//! which then runs the Run-Ahead frames with the current input; only its
//! picture is shown. The real core is never rewound, so its audio, rumble,
//! battery saves and achievements see one continuous timeline and need no
//! speculative audio sidecar or rollback.
use std::path::{Path, PathBuf};
use slot_retro::{ButtonMask, LibretroCore, RetroCore};

/// Everything needed to boot the prediction copy lazily, the first time
/// Run-Ahead is used in a solo session.
#[derive(Clone, Debug)]
pub struct Spec {
    pub core_file: PathBuf,
    pub system_dir: PathBuf,
    /// Private folder for the copied core library and its save directory.
    /// The copy never writes battery or state files of its own.
    pub scratch_dir: PathBuf,
    pub rom: PathBuf,
    pub options: Vec<(String, String)>,
    /// [`LibretroCore::instance_id`] of the real core.
    pub primary_id: usize,
}

impl Spec {
    /// Keep the copy's core options identical to the real core's, so its
    /// pictures match (palettes, colour correction).
    pub fn set_option(&mut self, key: &str, value: &str) {
        match self.options.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value.to_owned(),
            None => self.options.push((key.to_owned(), value.to_owned())),
        }
    }
}

/// The library is copied under its original file name (the libretro
/// wrapper recognises mGBA/gpSP by name) into a private folder, so the
/// dynamic linker loads it a second time with its own globals.
pub fn private_library_path(spec: &Spec) -> Option<PathBuf> {
    spec.core_file.file_name().map(|name| spec.scratch_dir.join(name))
}

fn copy_if_changed(from: &Path, to: &Path) -> std::io::Result<()> {
    let same = match (std::fs::metadata(from), std::fs::metadata(to)) {
        (Ok(a), Ok(b)) => a.len() == b.len()
            && std::fs::read(from)? == std::fs::read(to)?,
        _ => false,
    };
    if same { return Ok(()); }
    let partial = to.with_extension("so.partial");
    std::fs::copy(from, &partial)?;
    std::fs::rename(&partial, to)
}

/// Boot the prediction copy with the same options and ROM, seeded with the
/// real core's battery data. Fails closed: the caller falls back to the
/// single-instance Run-Ahead path.
pub fn open(spec: &Spec, battery: Option<&[u8]>) -> Result<Box<dyn RetroCore>, String> {
    std::fs::create_dir_all(&spec.scratch_dir)
        .map_err(|e| format!("cannot create {}: {e}", spec.scratch_dir.display()))?;
    let library = private_library_path(spec).ok_or("core file has no name")?;
    copy_if_changed(&spec.core_file, &library)
        .map_err(|e| format!("cannot copy core library: {e}"))?;
    let mut core = LibretroCore::open_secondary(&library, &spec.system_dir, &spec.scratch_dir)
        .map_err(|e| e.to_string())?;
    if core.instance_id() == spec.primary_id {
        return Err("dynamic linker reused the real core's library".into());
    }
    for (key, value) in &spec.options {
        core.set_option(key, value);
    }
    core.load(&spec.rom).map_err(|e| e.to_string())?;
    if let Some(battery) = battery {
        core.load_save_ram(battery).map_err(|e| e.to_string())?;
    }
    Ok(Box::new(core))
}

/// Copy `state` (the real core, just after its committed frame) into the
/// prediction copy and run one predicted frame per entry of `inputs`.
/// Returns the predicted picture; the copy's audio is discarded.
pub fn predict<'a>(copy: &'a mut dyn RetroCore, state: &[u8], inputs: &[ButtonMask])
    -> Result<&'a [u8], String>
{
    copy.unserialize(state).map_err(|e| e.to_string())?;
    for &input in inputs {
        copy.run_speculative_frame(input);
        copy.discard_audio();
    }
    Ok(copy.video_xrgb8888())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> Spec {
        Spec {
            core_file: PathBuf::from("/data/app/lib/arm64/libmgba_libretro.so"),
            system_dir: PathBuf::from("/files/BIOS"),
            scratch_dir: PathBuf::from("/files/RunAheadInstance"),
            rom: PathBuf::from("/files/rom.gba"),
            options: vec![("mgba_color_correction".into(), "OFF".into())],
            primary_id: 1,
        }
    }

    #[test]
    fn second_instance_library_keeps_core_name_in_private_folder() {
        assert_eq!(private_library_path(&spec()).unwrap(),
            PathBuf::from("/files/RunAheadInstance/libmgba_libretro.so"));
    }

    #[test]
    fn second_instance_mirrors_live_option_changes() {
        let mut s = spec();
        s.set_option("mgba_color_correction", "Auto");
        s.set_option("mgba_gb_colors", "Grayscale");
        assert_eq!(s.options, vec![
            ("mgba_color_correction".to_string(), "Auto".to_string()),
            ("mgba_gb_colors".to_string(), "Grayscale".to_string()),
        ]);
    }

    #[test]
    fn second_instance_copy_is_refreshed_only_when_the_core_changes() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("core.so");
        let to = dir.path().join("copy").join("core.so");
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&from, b"v1").unwrap();
        copy_if_changed(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"v1");
        std::fs::write(&from, b"v2!").unwrap();
        copy_if_changed(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"v2!");
        assert!(!to.with_extension("so.partial").exists());
    }

    #[test]
    fn second_instance_missing_core_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = spec();
        s.core_file = dir.path().join("missing.so");
        s.scratch_dir = dir.path().join("scratch");
        assert!(open(&s, None).is_err());
    }
}

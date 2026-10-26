//! Exercise the production GameSession, not a second Run-Ahead algorithm.
use super::*;
use slot_retro::{AvInfo, CoreError};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Failure {
    None,
    Snapshot,
    AudioCapture,
    StateRestore,
    AudioRestore,
}
#[derive(Default)]
struct Calls {
    real: usize,
    predicted: Vec<u16>,
    battery: usize,
}
struct CoreFixture {
    frame: u32,
    // Deliberately omitted from serialize(): rollback requires the sidecar.
    mixer: i16,
    captured_mixer: i16,
    pcm: Vec<i16>,
    pixels: Vec<u8>,
    failure: Failure,
    calls: Arc<Mutex<Calls>>,
}
impl CoreFixture {
    fn step(&mut self) {
        self.frame += 1;
        self.mixer += 1;
        self.pcm.extend_from_slice(&[self.mixer, self.mixer]);
        self.pixels = self.frame.to_le_bytes().to_vec();
    }
}
impl RetroCore for CoreFixture {
    fn load(&mut self, _: &Path) -> Result<(), CoreError> {
        Ok(())
    }
    fn run_frame(&mut self, _: ButtonMask) {
        self.calls.lock().unwrap().real += 1;
        self.step();
    }
    fn run_speculative_frame(&mut self, input: ButtonMask) {
        self.calls.lock().unwrap().predicted.push(input.0);
        self.step();
    }
    fn video_xrgb8888(&self) -> &[u8] {
        &self.pixels
    }
    fn take_audio(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.pcm)
    }
    fn serialize(&mut self) -> Result<Vec<u8>, CoreError> {
        if self.failure == Failure::Snapshot {
            return Err(CoreError::State("snapshot".into()));
        }
        Ok(self.frame.to_le_bytes().to_vec())
    }
    fn unserialize(&mut self, bytes: &[u8]) -> Result<(), CoreError> {
        if self.failure == Failure::StateRestore {
            return Err(CoreError::State("rollback".into()));
        }
        self.frame = u32::from_le_bytes(bytes.try_into().unwrap());
        Ok(())
    }
    fn capture_speculative_audio(&mut self) -> Result<(), CoreError> {
        if self.failure == Failure::AudioCapture {
            return Err(CoreError::State("audio capture".into()));
        }
        self.captured_mixer = self.mixer;
        Ok(())
    }
    fn restore_speculative_audio(&mut self) -> Result<(), CoreError> {
        if self.failure == Failure::AudioRestore {
            return Err(CoreError::State("audio rollback".into()));
        }
        self.mixer = self.captured_mixer;
        Ok(())
    }
    fn save_ram(&self) -> Option<Vec<u8>> {
        panic!("must use verified battery accessor")
    }
    fn save_ram_verified(&self) -> Result<Option<Vec<u8>>, CoreError> {
        self.calls.lock().unwrap().battery += 1;
        Ok(Some(self.frame.to_le_bytes().to_vec()))
    }
    fn save_rtc(&self) -> Option<Vec<u8>> {
        Some((self.frame * 2).to_le_bytes().to_vec())
    }
    fn load_save_ram(&mut self, _: &[u8]) -> Result<(), CoreError> {
        Ok(())
    }
    fn av_info(&self) -> AvInfo {
        AvInfo {
            fps: 59.728,
            sample_rate: 65536.0,
        }
    }
}

fn session(root: &Path, failure: Failure) -> (GameSession, Arc<Mutex<Calls>>) {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let core = Box::new(CoreFixture {
        frame: 0,
        mixer: 0,
        captured_mixer: 0,
        pcm: vec![],
        pixels: vec![0; 4],
        failure,
        calls: calls.clone(),
    });
    let session = GameSession {
        core,
        link: None,
        game_link: None,
        link_touched: false,
        linked_boot: None,
        gpsp_serial: "auto",
        rom_path: root.join("test.gba"),
        rom_stem: "test".into(),
        save: root.join("test.srm"),
        rtc: root.join("test.rtc"),
        manual_export_prefix: root.join("manual"),
        state: root.join("test.state"),
        retroarch_export: root.join("test.export"),
        ring: StateRing::new(root, Platform::Gba, Core::Mgba, "test"),
        rewind: RewindThread::spawn(REWIND_BYTES),
        rewind_frames: 0,
        frame_count: 0,
        runahead_video: vec![],
        runahead_state: vec![],
        runahead_unavailable: false,
        runahead_rollback_failed: false,
        runahead_discard_confirm: false,
        runahead_probe: RunaheadProbe::default(),
        runahead_secondary_spec: None,
        runahead_secondary: None,
        runahead_secondary_unavailable: false,
        preempt: crate::preemptive::Preempt::default(),
        last_step_timings: FrameStepTimings::default(),
        mgba_flash_runahead_guard: false,
        sma4_runahead_guard: false,
        solar: false,
        palette_live: false,
        core_kind: Core::Mgba,
        pending_undo: None,
        last_sram: Instant::now(),
        battery_writer: None,
        sample_rate: 65536,
        fps: 59.728,
    };
    (session, calls)
}

#[test]
fn runahead_0_1_2_preserve_real_state_audio_and_battery_while_showing_future_video() {
    for frames in 0..=2 {
        let root = tempfile::tempdir().unwrap();
        let (mut game, calls) = session(root.path(), Failure::None);
        for real in 1..=90u32 {
            assert!(game.advance(ButtonMask::X, true, false));
            assert_eq!(
                game.take_audio(),
                vec![real as i16; 2],
                "only real audio at RA={frames}"
            );
            game.prepare_runahead(ButtonMask::X, true, frames);
            assert_eq!(game.frame(), (real + u32::from(frames)).to_le_bytes());
            assert_eq!(game.core.serialize().unwrap(), real.to_le_bytes());
            assert!(
                game.take_audio().is_empty(),
                "predicted PCM must not escape"
            );
            let battery = game.periodic_battery_snapshot().unwrap();
            assert_eq!(battery.ram.unwrap(), real.to_le_bytes());
            assert_eq!(battery.rtc.unwrap(), (real * 2).to_le_bytes());
        }
        let calls = calls.lock().unwrap();
        assert_eq!(calls.real, 90);
        assert_eq!(calls.predicted.len(), 90 * usize::from(frames));
    }
}

#[test]
fn runahead_predicts_each_future_turbo_phase_across_the_three_frame_boundary() {
    let root = tempfile::tempdir().unwrap();
    let (mut game, calls) = session(root.path(), Failure::None);
    for _ in 0..2 {
        game.advance(ButtonMask::X, true, false);
        game.take_audio();
    }
    game.prepare_runahead(ButtonMask::X, true, 2);
    assert_eq!(calls.lock().unwrap().predicted, vec![ButtonMask::A, 0]);
    game.prepare_runahead(ButtonMask::X | ButtonMask::Y, false, 2);
    assert_eq!(
        &calls.lock().unwrap().predicted[2..],
        &[ButtonMask::A | ButtonMask::B; 2]
    );
}

#[test]
fn runahead_rollback_failure_freezes_session_and_preserves_every_disk_checkpoint() {
    for failure in [Failure::StateRestore, Failure::AudioRestore] {
        let root = tempfile::tempdir().unwrap();
        let (mut game, calls) = session(root.path(), failure);
        let checkpoints = [
            game.save.clone(),
            game.rtc.clone(),
            game.state.clone(),
            game.retroarch_export.clone(),
        ];
        for path in &checkpoints {
            std::fs::write(path, b"previous valid checkpoint").unwrap();
        }
        game.advance(0, true, false);
        game.take_audio();
        game.prepare_runahead(0, true, 2);
        assert!(game.runahead_rollback_failed());
        assert!(game.runahead_video.is_empty());
        assert!(!game.advance(0, true, false));
        assert!(!game.rewind_step());
        assert!(game.periodic_battery_snapshot().is_err());
        assert!(game.save_for_eject(true).is_err());
        assert!(!game.save(true));
        assert!(game.save_manual().is_err());
        assert!(!game.confirm_rollback_discard());
        assert!(game.confirm_rollback_discard());
        assert_eq!(calls.lock().unwrap().real, 1);
        assert_eq!(calls.lock().unwrap().battery, 0);
        drop(game);
        for path in checkpoints {
            assert_eq!(std::fs::read(path).unwrap(), b"previous valid checkpoint");
        }
    }
}

#[test]
fn runahead_snapshot_failure_disables_prediction_but_keeps_real_gameplay() {
    for failure in [Failure::Snapshot, Failure::AudioCapture] {
        let root = tempfile::tempdir().unwrap();
        let (mut game, calls) = session(root.path(), failure);
        game.prepare_runahead(0, true, 2);
        assert!(!game.runahead_supported());
        assert!(!game.runahead_rollback_failed());
        assert!(calls.lock().unwrap().predicted.is_empty());
        assert!(game.advance(0, true, false));
        assert_eq!(game.take_audio(), vec![1; 2]);
    }
}

#[test]
fn runahead_respects_flash_sma4_and_multiplayer_guards() {
    for guard in 0..4 {
        let root = tempfile::tempdir().unwrap();
        let (mut game, calls) = session(root.path(), Failure::None);
        match guard {
            0 => game.mgba_flash_runahead_guard = true,
            1 => game.sma4_runahead_guard = true,
            2 => game.link_touched = true,
            _ => game.linked_boot = Some(Role::Host),
        }
        assert!(!game.runahead_supported());
        game.prepare_runahead(ButtonMask::A, true, 2);
        assert!(calls.lock().unwrap().predicted.is_empty());
        assert_eq!(game.core.serialize().unwrap(), 0u32.to_le_bytes());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn real_egl_recovery_keeps_runahead_session_and_rollback_quarantine() {
    for failure in [Failure::None, Failure::StateRestore] {
        for frames in 0..=2 {
            let root = tempfile::tempdir().unwrap();
            let (mut game, calls) = session(root.path(), failure);
            game.advance(ButtonMask::A, false, false);
            game.take_audio();
            game.prepare_runahead(ButtonMask::A, false, frames);
            let state = game.core.serialize().unwrap();
            let video = game.frame().to_vec();
            let quarantined = game.runahead_rollback_failed;
            let core_address = &*game.core as *const dyn RetroCore as *const ();
            let real = calls.lock().unwrap().real;
            let predicted = calls.lock().unwrap().predicted.len();
            let battery = calls.lock().unwrap().battery;
            // Safety gates must survive independently of context recreation.
            game.mgba_flash_runahead_guard = true;
            game.sma4_runahead_guard = true;
            let mut game = crate::runtime::exercise_session_graphics_recovery(game);
            assert_eq!(&*game.core as *const dyn RetroCore as *const (), core_address);
            assert_eq!(game.core.serialize().unwrap(), state);
            assert_eq!(game.frame(), video);
            assert_eq!(game.runahead_rollback_failed, quarantined);
            assert!(game.mgba_flash_runahead_guard && game.sma4_runahead_guard);
            assert_eq!(calls.lock().unwrap().real, real);
            assert_eq!(calls.lock().unwrap().predicted.len(), predicted);
            assert_eq!(calls.lock().unwrap().battery, battery);
            if quarantined {
                assert!(!game.advance(ButtonMask::A, false, false));
                assert!(!game.save(true));
                assert!(!game.save.exists());
            }
        }
    }
}

fn prediction_copy(failure: Failure) -> (Box<dyn RetroCore>, Arc<Mutex<Calls>>) {
    let calls = Arc::new(Mutex::new(Calls::default()));
    (Box::new(CoreFixture {
        frame: 0, mixer: 0, captured_mixer: 0, pcm: vec![], pixels: vec![0; 4],
        failure, calls: calls.clone(),
    }), calls)
}

#[test]
fn second_instance_predicts_on_the_copy_and_never_rewinds_the_real_core() {
    for frames in 1..=2u8 {
        let root = tempfile::tempdir().unwrap();
        // The real core cannot even capture speculative audio: the
        // single-instance path would refuse, second instance does not need it.
        let (mut game, real_calls) = session(root.path(), Failure::AudioCapture);
        let (copy, copy_calls) = prediction_copy(Failure::None);
        game.runahead_secondary = Some(copy);
        for real in 1..=90u32 {
            assert!(game.advance(ButtonMask::A, false, false));
            // The real mixer is never rolled back: audio stays continuous.
            assert_eq!(game.take_audio(), vec![real as i16; 2]);
            game.prepare_runahead(ButtonMask::A, false, frames);
            assert_eq!(game.frame(), (real + u32::from(frames)).to_le_bytes());
            assert_eq!(game.core.serialize().unwrap(), real.to_le_bytes());
            assert!(game.take_audio().is_empty());
        }
        assert!(real_calls.lock().unwrap().predicted.is_empty());
        assert_eq!(real_calls.lock().unwrap().real, 90);
        assert_eq!(copy_calls.lock().unwrap().predicted.len(), 90 * usize::from(frames));
        assert_eq!(copy_calls.lock().unwrap().real, 0);
        let probe = game.take_runahead_probe();
        assert_eq!(probe.second_instance_calls, 90);
        assert_eq!(probe.failures, 0);
        assert!(!game.runahead_rollback_failed);
    }
}

#[test]
fn second_instance_failure_drops_only_the_copy_and_falls_back_to_single_instance() {
    let root = tempfile::tempdir().unwrap();
    let (mut game, real_calls) = session(root.path(), Failure::None);
    let (copy, _) = prediction_copy(Failure::StateRestore);
    game.runahead_secondary = Some(copy);
    assert!(game.advance(ButtonMask::A, false, false));
    game.take_audio();
    game.prepare_runahead(ButtonMask::A, false, 1);
    assert!(game.runahead_secondary.is_none());
    assert!(game.runahead_secondary_unavailable);
    assert!(!game.runahead_rollback_failed);
    // Same call fell back to the single-instance prediction on the real core.
    assert_eq!(real_calls.lock().unwrap().predicted.len(), 1);
    assert_eq!(game.frame(), 2u32.to_le_bytes());
    assert_eq!(game.core.serialize().unwrap(), 1u32.to_le_bytes());
    assert!(game.advance(ButtonMask::A, false, false));
    assert_eq!(game.take_audio(), vec![2; 2]);
}

#[test]
fn second_instance_respects_existing_guards_and_never_boots_without_a_spec() {
    let root = tempfile::tempdir().unwrap();
    let (mut game, _) = session(root.path(), Failure::None);
    let (copy, copy_calls) = prediction_copy(Failure::None);
    game.runahead_secondary = Some(copy);
    game.sma4_runahead_guard = true;
    game.advance(ButtonMask::A, false, false);
    game.prepare_runahead(ButtonMask::A, false, 2);
    assert!(copy_calls.lock().unwrap().predicted.is_empty());
    // No spec (tests, linked boots): the copy is never booted.
    let (mut solo, calls) = session(root.path(), Failure::None);
    solo.advance(ButtonMask::A, false, false);
    solo.prepare_runahead(ButtonMask::A, false, 1);
    assert!(solo.runahead_secondary.is_none() && solo.runahead_secondary_unavailable);
    assert_eq!(calls.lock().unwrap().predicted.len(), 1);
}

#[test]
fn preemptive_frames_replay_the_window_only_when_the_buttons_change() {
    let root = tempfile::tempdir().unwrap();
    let (mut game, calls) = session(root.path(), Failure::None);
    game.set_preemptive_frames(2);
    for real in 1..=10u32 {
        assert!(game.advance(0, false, false));
        assert_eq!(game.take_audio(), vec![real as i16; 2]);
    }
    assert!(calls.lock().unwrap().predicted.is_empty(), "steady buttons never roll back");
    // A pressed on frame 11: frames 9 and 10 are replayed with A first.
    assert!(game.advance(ButtonMask::A, false, false));
    assert_eq!(calls.lock().unwrap().predicted, vec![ButtonMask::A; 2]);
    assert_eq!(game.core.serialize().unwrap(), 11u32.to_le_bytes());
    assert_eq!(game.frame(), 11u32.to_le_bytes(), "the real picture is shown");
    // Only the real frame's audio comes out, from the new timeline's mixer
    // (frames 9 and 10 replayed): never the old timeline's sound state.
    assert_eq!(game.take_audio(), vec![13; 2], "replayed audio is dropped");
    for _ in 0..5 {
        assert!(game.advance(ButtonMask::A, false, false));
        game.take_audio();
    }
    assert_eq!(calls.lock().unwrap().predicted.len(), 2, "holding A replays nothing");
    let probe = game.take_runahead_probe();
    assert_eq!(probe.preemptive_rollbacks, 1);
    assert_eq!(probe.calls, 16, "one snapshot per real frame");
    assert_eq!(calls.lock().unwrap().real, 16);
}

#[test]
fn preemptive_frames_replay_each_frame_with_its_own_turbo_phase() {
    let root = tempfile::tempdir().unwrap();
    let (mut game, calls) = session(root.path(), Failure::None);
    game.set_preemptive_frames(2);
    for _ in 0..4 {
        game.advance(0, false, false);
        game.take_audio();
    }
    game.advance(ButtonMask::X, true, false);
    let expected: Vec<u16> = (2..4u32)
        .map(|frame| xy_buttons(ButtonMask(ButtonMask::X), true, frame).0)
        .collect();
    assert_eq!(calls.lock().unwrap().predicted, expected);
}

#[test]
fn preemptive_frames_fail_closed_on_a_broken_rollback_and_stop_on_snapshot_errors() {
    let root = tempfile::tempdir().unwrap();
    let (mut game, calls) = session(root.path(), Failure::StateRestore);
    game.set_preemptive_frames(1);
    assert!(game.advance(0, false, false));
    assert!(!game.advance(ButtonMask::A, false, false));
    assert!(game.runahead_rollback_failed());
    assert!(!game.advance(ButtonMask::A, false, false));
    assert!(!game.save(true));
    assert_eq!(calls.lock().unwrap().real, 1);

    {
        let root = tempfile::tempdir().unwrap();
        let (mut game, calls) = session(root.path(), Failure::Snapshot);
        game.set_preemptive_frames(2);
        for buttons in [0, 0, 0, ButtonMask::A, 0, ButtonMask::B] {
            assert!(game.advance(buttons, false, false));
        }
        assert!(!game.runahead_rollback_failed());
        assert!(!game.runahead_supported());
        assert!(calls.lock().unwrap().predicted.is_empty());
        assert_eq!(calls.lock().unwrap().real, 6);
    }
}

#[test]
fn preemptive_frames_respect_the_runahead_guards() {
    for guard in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let (mut game, calls) = session(root.path(), Failure::None);
        match guard {
            0 => game.mgba_flash_runahead_guard = true,
            1 => game.sma4_runahead_guard = true,
            _ => game.link_touched = true,
        }
        game.set_preemptive_frames(2);
        for buttons in [0, 0, 0, ButtonMask::A] { game.advance(buttons, false, false); }
        assert!(calls.lock().unwrap().predicted.is_empty());
    }
}

#[test]
fn pokemon_flash_guard_saves_a_new_game_but_protects_a_complete_save() {
    let root = tempfile::tempdir().unwrap();
    let (mut game, _) = session(root.path(), Failure::None);
    game.mgba_flash_runahead_guard = true;
    // Never saved in-game: nothing on disk to protect, Save & Eject works.
    assert_eq!(game.save_for_eject(false), Ok(true));
    let mut complete = vec![0u8; 128 * 1024];
    for i in 0..14usize {
        let s = &mut complete[i * 4096..(i + 1) * 4096];
        s[0xFF4..0xFF6].copy_from_slice(&(i as u16).to_le_bytes());
        s[0xFF8..0xFFC].copy_from_slice(&0x0801_2025u32.to_le_bytes());
        s[0xFFC..0x1000].copy_from_slice(&7u32.to_le_bytes());
    }
    std::fs::write(&game.save, &complete).unwrap();
    // An incomplete image must never replace a complete in-game save.
    assert!(game.save_for_eject(false).is_err());
    assert_eq!(std::fs::read(&game.save).unwrap(), complete);
}

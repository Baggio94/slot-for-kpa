//! F7: serial, bounded, per-session battery/RTC disk writer.
//!
//! Only immutable copies captured from the REAL core on the GL thread cross
//! this boundary. A single worker writes at most one periodic snapshot at a
//! time. Mandatory suspend/eject/Link checkpoints wait for it to finish before
//! doing their own fresh, verified writes on the calling thread.
use slot_store::atomic_write;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};

pub struct BatterySnapshot {
    pub save: PathBuf,
    pub rtc_path: PathBuf,
    pub ram: Option<Vec<u8>>,
    pub rtc: Option<Vec<u8>>,
}

fn persist(snapshot: BatterySnapshot) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Some(ref bytes) = snapshot.ram {
        if let Err(error) = atomic_write(&snapshot.save, bytes) {
            errors.push(format!("Battery save: {error}"));
        }
    }
    if let Some(ref bytes) = snapshot.rtc {
        if let Err(error) = atomic_write(&snapshot.rtc_path, bytes) {
            errors.push(format!("RTC save: {error}"));
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

pub struct BatteryWriter {
    send: Option<SyncSender<BatterySnapshot>>,
    completed: Receiver<Result<(), String>>,
    worker: Option<JoinHandle<()>>,
    pending: bool,
}

impl BatteryWriter {
    pub fn spawn() -> Result<Self, String> {
        let (send, incoming) = mpsc::sync_channel::<BatterySnapshot>(1);
        let (completed_send, completed) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("slot-battery-writer".into())
            .spawn(move || {
                while let Ok(snapshot) = incoming.recv() {
                    let result = persist(snapshot);
                    // A listener may disappear on abnormal renderer teardown;
                    // never block the writer on a completion notification.
                    if completed_send.send(result).is_err() { break; }
                }
            })
            .map_err(|error| format!("Cannot start battery writer: {error}"))?;
        Ok(Self { send: Some(send), completed, worker: Some(worker), pending: false })
    }

    pub fn busy(&self) -> bool { self.pending }

    pub fn enqueue(&mut self, snapshot: BatterySnapshot) -> Result<(), String> {
        // Do not permit stale queued snapshots, growing memory, or overlapping
        // writes for a single ROM. The caller skips an optional capture if busy.
        if self.pending { return Err("Previous periodic battery write still pending".into()); }
        let sender = self.send.as_ref().ok_or("Battery writer stopped")?;
        match sender.try_send(snapshot) {
            Ok(()) => { self.pending = true; Ok(()) }
            Err(TrySendError::Full(_)) =>
                Err("Battery writer queue full".into()),
            Err(TrySendError::Disconnected(_)) =>
                Err("Battery writer disconnected".into()),
        }
    }

    /// Non-blocking completion check, called only at the 30-second save
    /// interval, never on every display callback.
    pub fn poll(&mut self) -> Result<(), String> {
        if !self.pending { return Ok(()); }
        match self.completed.try_recv() {
            Ok(result) => { self.pending = false; result }
            Err(TryRecvError::Empty) => Ok(()),
            Err(TryRecvError::Disconnected) => {
                self.pending = false;
                Err("Battery writer terminated without durable acknowledgement".into())
            }
        }
    }

    /// Mandatory ordering barrier: a periodic write cannot finish AFTER a
    /// newer suspend/eject/Link checkpoint and overwrite its battery files.
    /// This may wait for disk I/O, but only during an explicit safe checkpoint.
    pub fn flush(&mut self) -> Result<(), String> {
        if !self.pending { return Ok(()); }
        let result = self.completed.recv()
            .map_err(|_| "Battery writer stopped before checkpoint barrier".to_owned());
        self.pending = false;
        result?
    }
}

impl Drop for BatteryWriter {
    fn drop(&mut self) {
        // Do not detach a worker that might later overwrite another session's
        // battery. The sender is disconnected only after its queued item has
        // drained, and join completes before this session fully disappears.
        self.send.take();
        if let Some(handle) = self.worker.take() {
            if handle.join().is_err() {
                eprintln!("slot-konkr: battery writer panicked on shutdown");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root() -> PathBuf {
        let id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let p = std::env::temp_dir().join(format!("slot-battery-f7-{}-{id}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn job(root: &std::path::Path, value: u8) -> BatterySnapshot {
        BatterySnapshot {
            save: root.join("game.srm"),
            rtc_path: root.join("game.rtc"),
            ram: Some(vec![value; 8192]),
            rtc: Some(vec![value; 32]),
        }
    }

    #[test]
    fn periodic_snapshot_is_persisted_before_explicit_barrier() {
        let root = root();
        let mut writer = BatteryWriter::spawn().unwrap();
        writer.enqueue(job(&root, 7)).unwrap();
        assert!(writer.busy());
        writer.flush().unwrap();
        assert!(!writer.busy());
        assert_eq!(fs::read(root.join("game.srm")).unwrap(), vec![7; 8192]);
        assert_eq!(fs::read(root.join("game.rtc")).unwrap(), vec![7; 32]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn newer_mandatory_checkpoint_cannot_be_overwritten_by_prior_async_job() {
        let root = root();
        let mut writer = BatteryWriter::spawn().unwrap();
        writer.enqueue(job(&root, 0x11)).unwrap();
        // This is the exact ordering used by Save & Eject / Suspend / Link.
        writer.flush().unwrap();
        atomic_write(&root.join("game.srm"), &vec![0xAA; 8192]).unwrap();
        atomic_write(&root.join("game.rtc"), &vec![0xBB; 32]).unwrap();
        drop(writer);
        assert_eq!(fs::read(root.join("game.srm")).unwrap(), vec![0xAA; 8192]);
        assert_eq!(fs::read(root.join("game.rtc")).unwrap(), vec![0xBB; 32]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn single_pending_generation_never_queues_stale_second_write() {
        let root = root();
        let mut writer = BatteryWriter::spawn().unwrap();
        writer.enqueue(job(&root, 1)).unwrap();
        assert!(writer.enqueue(job(&root, 2)).is_err());
        writer.flush().unwrap();
        writer.enqueue(job(&root, 3)).unwrap();
        writer.flush().unwrap();
        assert_eq!(fs::read(root.join("game.srm")).unwrap(), vec![3; 8192]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rtc_failure_is_reported_even_when_sram_write_succeeds() {
        let root = root();
        // A directory at the RTC destination makes rename fail, regardless
        // of the host user's permissions. The SRAM must remain good.
        fs::create_dir(root.join("game.rtc")).unwrap();
        let mut writer = BatteryWriter::spawn().unwrap();
        writer.enqueue(job(&root, 8)).unwrap();
        let error = writer.flush().unwrap_err();
        assert!(error.contains("RTC save"), "{error}");
        assert_eq!(fs::read(root.join("game.srm")).unwrap(), vec![8; 8192]);
        fs::remove_dir(root.join("game.rtc")).unwrap();
        writer.enqueue(job(&root, 9)).unwrap();
        writer.flush().unwrap();
        assert_eq!(fs::read(root.join("game.rtc")).unwrap(), vec![9; 32]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn battery_failure_is_not_masked_by_a_successful_rtc_write() {
        let root = root();
        let mut writer = BatteryWriter::spawn().unwrap();
        let mut broken = job(&root, 0x31);
        // Reproducible missing-parent error without platform-specific chmod.
        broken.save = root.join("missing-parent").join("game.srm");
        writer.enqueue(broken).unwrap();
        let error = writer.flush().unwrap_err();
        assert!(error.contains("Battery save"), "{error}");
        assert_eq!(fs::read(root.join("game.rtc")).unwrap(), vec![0x31; 32]);
        // A later, fully valid generation must supersede that partial write.
        writer.enqueue(job(&root, 0x42)).unwrap();
        writer.flush().unwrap();
        assert_eq!(fs::read(root.join("game.srm")).unwrap(), vec![0x42; 8192]);
        assert_eq!(fs::read(root.join("game.rtc")).unwrap(), vec![0x42; 32]);
        drop(writer);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dropping_session_waits_for_queued_write() {
        let root = root();
        {
            let mut writer = BatteryWriter::spawn().unwrap();
            writer.enqueue(job(&root, 4)).unwrap();
        }
        assert_eq!(fs::read(root.join("game.srm")).unwrap(), vec![4; 8192]);
        fs::remove_dir_all(root).unwrap();
    }
}

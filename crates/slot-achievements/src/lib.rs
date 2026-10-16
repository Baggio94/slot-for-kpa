//! Optional, softcore RetroAchievements. Emulator threads only copy RAM into a bounded,
//! nonblocking queue; evaluation/storage and HTTPS each have their own worker.
mod badges;
mod core;
mod library;
mod network;
mod runtime;
mod storage;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

pub use badges::{load_badge, BadgeImage};
use slot_retro::RetroCore;
use storage::{Config, Store, Unlock};

const RAM_SIZE: usize = 0x58000;
const GB_COVERAGE_SIZE: usize = 0x17000;
const QUEUE_SIZE: usize = 8;

/// A passive shelf indicator. Reading it never acquires a worker's lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SyncStatus {
    Disabled,
    Syncing,
    Ready,
    Offline,
    Attention,
}

#[derive(Default)]
struct Status {
    network: AtomicU8,
    unsaved: AtomicBool,
    reconnect: AtomicBool,
    // Zero means unknown; otherwise percent + 1, keeping Default valid.
    progress: AtomicU8,
    pending: AtomicBool,
    // Shared only by the evaluator and HTTP workers, never gameplay/UI.
    presence: Mutex<Option<(u64, String)>>,
}

impl Status {
    fn set(&self, value: SyncStatus) {
        self.network.store(value as u8, Ordering::Release);
    }

    fn get(&self) -> SyncStatus {
        if self.unsaved.load(Ordering::Acquire) {
            return SyncStatus::Attention;
        }
        match self.network.load(Ordering::Acquire) {
            0 => SyncStatus::Disabled,
            1 => SyncStatus::Syncing,
            2 => SyncStatus::Ready,
            3 => SyncStatus::Offline,
            _ => SyncStatus::Attention,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Status,
    Earned,
}

#[derive(Clone, Debug)]
pub struct Notice {
    /// Local cached image, read only by the notification raster worker.
    pub badge: Option<PathBuf>,
    pub generation: u64,
    pub title: String,
    pub detail: String,
    pub kind: NoticeKind,
}

impl Notice {
    fn status(generation: u64, title: &str) -> Self {
        // Background diagnostics only; routine status is represented by the shelf icon.
        eprintln!("slot: {title}");
        Self {
            badge: None,
            generation,
            title: title.into(),
            detail: String::new(),
            kind: NoticeKind::Status,
        }
    }
}

enum Control {
    Load(network::Load),
    Unload(u64),
    Flush(Arc<AtomicBool>),
    Shutdown,
    // Android's SAF scanner sends only full-ROM MD5s; no ROM bytes go over HTTP.
    PrefetchHash(String, u32),
}

struct Frame {
    generation: u64,
    sequence: u64,
    timeline: u64,
    earned_at: u64,
    ram: Vec<u8>,
    valid: [usize; 3],
    console: u32,
    recycle: mpsc::SyncSender<Vec<u8>>,
}

/// Outlives games, so pending unlocks sync while the player is on the shelf, too.
pub struct Service {
    controls: mpsc::Sender<Control>,
    frames: mpsc::SyncSender<Frame>,
    notices: mpsc::Receiver<Notice>,
    enabled: Arc<AtomicBool>,
    current: Arc<AtomicU64>,
    flushing: AtomicBool,
    flushed: Arc<AtomicBool>,
    status: Arc<Status>,
    stopping: AtomicBool,
    evaluator_stopped: Arc<AtomicBool>,
    http_shutdown: Arc<AtomicBool>,
    http_stopped: Arc<AtomicBool>,
}

impl Service {
    pub fn start(root: PathBuf) -> Self {
        Self::start_with(root, network::Http::new())
    }

    fn start_with(root: PathBuf, transport: impl network::Transport + Send + 'static) -> Self {
        let (controls, control_rx) = mpsc::channel();
        let (frames, frame_rx) = mpsc::sync_channel(QUEUE_SIZE);
        let (notice_tx, notices) = mpsc::channel();
        let enabled = Arc::new(AtomicBool::new(false));
        let current = Arc::new(AtomicU64::new(0));
        let flushed = Arc::new(AtomicBool::new(false));
        let worker_flushed = flushed.clone();
        let worker_enabled = enabled.clone();
        let evaluator_stopped = Arc::new(AtomicBool::new(false));
        let http_shutdown = Arc::new(AtomicBool::new(false));
        // No HTTP thread exists until initialization reaches network startup.
        // Early configuration failure must not strand the shutdown barrier.
        let http_stopped = Arc::new(AtomicBool::new(true));
        let eval_done = evaluator_stopped.clone();
        let network_stop = http_shutdown.clone();
        let network_done = http_stopped.clone();
        let status = Arc::new(Status::default());
        let worker_status = status.clone();
        let spawned = std::thread::Builder::new()
            .name("slot-ra".into())
            .spawn(move || {
                run(
                    root,
                    worker_enabled.clone(),
                    control_rx,
                    frame_rx,
                    notice_tx,
                    worker_status,
                    network_stop,
                    network_done,
                    transport,
                );
                worker_enabled.store(false, Ordering::Release);
                worker_flushed.store(true, Ordering::Release);
                eval_done.store(true, Ordering::Release);
            });
        if spawned.is_err() {
            status.set(SyncStatus::Attention);
            flushed.store(true, Ordering::Release);
            evaluator_stopped.store(true, Ordering::Release);
            http_stopped.store(true, Ordering::Release);
            eprintln!("slot: achievements worker could not start");
        }
        Self {
            controls,
            frames,
            notices,
            enabled,
            current,
            flushing: AtomicBool::new(false),
            flushed,
            status,
            stopping: AtomicBool::new(false),
            evaluator_stopped,
            http_shutdown,
            http_stopped,
        }
    }

    pub fn sync_progress(&self) -> Option<u8> {
        self.status.progress.load(Ordering::Acquire).checked_sub(1)
    }

    pub fn sync_pending(&self) -> bool {
        self.status.pending.load(Ordering::Acquire) || self.status.unsaved.load(Ordering::Acquire)
    }

    /// Critical distinction for account sign-out: an award still waiting for
    /// a successful atomic write is NOT safe to discard with the process.
    pub fn has_unsaved_awards(&self) -> bool {
        self.status.unsaved.load(Ordering::Acquire)
    }

    pub fn sync_status(&self) -> SyncStatus {
        self.status.get()
    }

    /// The Wi-Fi worker observed a new connection. Wake retries without blocking the UI.
    pub fn network_available(&self) {
        self.status.reconnect.store(true, Ordering::Release);
    }

    /// Enqueue one uncompressed GBA ROM hash without touching the network or the UI.
    /// The HTTP worker deduplicates hashes and throttles all requests.
    pub fn prefetch_rom_hash(&self, hash: &str) -> bool {
        self.prefetch_rom_hash_for_console(hash, 5)
    }

    pub fn prefetch_rom_hash_for_console(&self, hash: &str, console: u32) -> bool {
        if !matches!(console, 4 | 5 | 6)
            || hash.len() != 32 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
        self.controls.send(Control::PrefetchHash(hash.to_ascii_lowercase(), console)).is_ok()
    }

    /// Poll during power-off. The UI keeps rendering while the evaluator drains final
    /// frames and fsyncs their awards; an HTTP request is never part of this barrier.
    pub fn flush_ready(&self) -> bool {
        if !self.flushing.swap(true, Ordering::AcqRel)
            && self
                .controls
                .send(Control::Flush(self.flushed.clone()))
                .is_err()
        {
            self.flushed.store(true, Ordering::Release);
        }
        self.flushed.load(Ordering::Acquire)
    }

    /// Nonblocking two-worker shutdown for Android in-place account logout.
    /// Caller must first pass flush_ready and the no-unsaved-awards barrier.
    /// Never remove auth/config while either old-account worker may still run.
    pub fn shutdown_ready(&self) -> bool {
        if !self.flushed.load(Ordering::Acquire) || self.has_unsaved_awards() {
            return false;
        }
        if !self.stopping.swap(true, Ordering::AcqRel) {
            self.http_shutdown.store(true, Ordering::Release);
            let _ = self.controls.send(Control::Shutdown);
        }
        self.evaluator_stopped.load(Ordering::Acquire)
            && self.http_stopped.load(Ordering::Acquire)
    }

    pub fn wrap(&self, core: Box<dyn RetroCore>) -> Box<dyn RetroCore> {
        self.wrap_for_console(core, 5)
    }

    /// GB=4, GBA=5, GBC=6; original rcheevos worker shared without forks.
    pub fn wrap_for_console(&self, core: Box<dyn RetroCore>, console: u32) -> Box<dyn RetroCore> {
        assert!(matches!(console, 4 | 5 | 6));
        Box::new(core::Tracked::new(core, self, console))
    }

    pub fn take_notice(&self) -> Option<Notice> {
        while let Ok(notice) = self.notices.try_recv() {
            // Earned notices survive an eject; stale loading notices do not.
            if notice.kind == NoticeKind::Earned
                || notice.generation == 0
                || notice.generation == self.current.load(Ordering::Acquire)
            {
                return Some(notice);
            }
        }
        None
    }
}

struct Playing {
    generation: u64,
    hash: String,
    console: u32,
    runtime: runtime::Runtime,
    achievements: BTreeMap<u32, storage::Achievement>,
    previous: Option<(u64, u64)>,
    warned_gap: bool,
}

fn run(
    root: PathBuf,
    enabled: Arc<AtomicBool>,
    controls: mpsc::Receiver<Control>,
    frames: mpsc::Receiver<Frame>,
    notices: mpsc::Sender<Notice>,
    status: Arc<Status>,
    http_shutdown: Arc<AtomicBool>,
    http_stopped: Arc<AtomicBool>,
    transport: impl network::Transport + Send + 'static,
) {
    let config_path = root.join("Config/retroachievements.toml");
    if !config_path.exists() {
        return;
    }
    let config = match Config::read(&config_path) {
        Ok(config) => config,
        Err(_) => {
            status.set(SyncStatus::Attention);
            let _ = notices.send(Notice::status(0, "Invalid RetroAchievements config"));
            return;
        }
    };
    if !config.enabled {
        return;
    }
    if config.username.trim().is_empty() {
        status.set(SyncStatus::Attention);
        let _ = notices.send(Notice::status(0, "Achievements need an account"));
        return;
    }
    let store = match Store::open(&root, &config.username) {
        Ok(store) => Arc::new(Mutex::new(store)),
        Err(error) => {
            status.set(SyncStatus::Attention);
            let _ = notices.send(Notice::status(0, &error));
            return;
        }
    };
    let (loads, load_rx) = mpsc::channel();
    let (prefetch_tx, prefetch_rx) = mpsc::channel();
    let (prepared_tx, prepared_rx) = mpsc::channel();
    let network_store = store.clone();
    let network_notices = notices.clone();
    let cached_prepared = prepared_tx.clone();
    let network_status = status.clone();
    let network_stop = http_shutdown.clone();
    let network_done = http_stopped.clone();
    status.set(SyncStatus::Syncing);
    // The pending HTTPS worker must explicitly acknowledge exit before the
    // old account's token can be deleted.
    http_stopped.store(false, Ordering::Release);
    if std::thread::Builder::new()
        .name("slot-ra-http".into())
        .spawn(move || {
            network::run(
                root,
                config,
                network_store,
                load_rx,
                prefetch_rx,
                prepared_tx,
                network_notices,
                network_status,
                network_stop,
                transport,
            );
            network_done.store(true, Ordering::Release);
        })
        .is_err()
    {
        http_stopped.store(true, Ordering::Release);
        status.set(SyncStatus::Attention);
        let _ = notices.send(Notice::status(0, "Achievements network worker failed"));
        return;
    }
    enabled.store(true, Ordering::Release);
    let mut generation = 0;
    let mut playing: Option<Playing> = None;
    let mut unsaved = BTreeMap::new();
    let mut retried = Instant::now();
    let mut presence_updated = Instant::now();
    let mut last_frame_received = Instant::now();
    loop {
        if retried.elapsed() >= Duration::from_secs(1) {
            retry_unsaved(&mut unsaved, &store, &notices);
            retried = Instant::now();
        }
        loop {
            // Frames queued before an eject/load must be evaluated before retiring that game.
            for _ in 0..QUEUE_SIZE {
                let Ok(frame) = frames.try_recv() else {
                    break;
                };
                evaluate(frame, &mut playing, &store, &notices, &mut unsaved);
            }
            match controls.try_recv() {
                Ok(Control::PrefetchHash(hash, console)) => {
                    let _ = prefetch_tx.send((hash, console));
                }
                Ok(Control::Flush(done)) => {
                    retry_unsaved(&mut unsaved, &store, &notices);
                    done.store(true, Ordering::Release);
                }
                Ok(Control::Shutdown) => {
                    retry_unsaved(&mut unsaved, &store, &notices);
                    status.unsaved.store(!unsaved.is_empty(), Ordering::Release);
                    if unsaved.is_empty() {
                        // The HTTP worker has its own cancellation flag and
                        // must separately acknowledge exit before auth deletion.
                        return;
                    }
                }
                Ok(Control::Load(load)) => {
                    generation = load.generation;
                    playing = None;
                    *status.presence.lock().unwrap() = None;
                    // Cached games can start evaluating even while the HTTP worker is
                    // waiting on a timeout or preparing another ROM in the library.
                    let dir = store.lock().unwrap().dir.clone();
                    if storage::read::<storage::Auth>(&dir.join("auth.json")).is_ok() {
                        if let Ok(hash) = library::hash(&dir, &load.path) {
                            if let Ok((game, unlocked)) =
                                storage::read::<(storage::Game, std::collections::BTreeSet<u32>)>(
                                    &dir.join(format!("{hash}.json")),
                                )
                            {
                                if matches!(game.console, 4 | 5 | 6) {
                                    let _ = cached_prepared.send(network::Prepared {
                                        generation,
                                        hash,
                                        game,
                                        unlocked,
                                        cached: true,
                                    });
                                }
                            }
                        }
                    }
                    let _ = loads.send(Some(load));
                }
                Ok(Control::Unload(id)) if id == generation => {
                    generation = 0;
                    playing = None;
                    *status.presence.lock().unwrap() = None;
                    let _ = loads.send(None);
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        for prepared in prepared_rx.try_iter() {
            if prepared.generation != generation {
                continue;
            }
            if let Some(game) = playing.as_mut() {
                // Preserve hits for unchanged definitions. Retire removed/unlocked entries
                // and activate new or revised definitions when online data arrives.
                game.runtime.activate_presence(&prepared.game.presence);
                let store = store.lock().unwrap();
                let mut updated = BTreeMap::new();
                for achievement in prepared.game.achievements {
                    if achievement.flags != 3
                        || prepared.unlocked.contains(&achievement.id)
                        || store.unlocks.contains_key(&achievement.id)
                        || unsaved.contains_key(&achievement.id)
                    {
                        continue;
                    }
                    let unchanged = game
                        .achievements
                        .get(&achievement.id)
                        .is_some_and(|old| old.definition == achievement.definition);
                    if unchanged
                        || game
                            .runtime
                            .activate(achievement.id, &achievement.definition)
                    {
                        updated.insert(achievement.id, achievement);
                    }
                }
                for id in game
                    .achievements
                    .keys()
                    .filter(|id| !updated.contains_key(id))
                {
                    game.runtime.deactivate(*id);
                }
                game.achievements = updated;
                continue;
            }
            let Some(mut runtime) = runtime::Runtime::new() else {
                continue;
            };
            runtime.activate_presence(&prepared.game.presence);
            let store = store.lock().unwrap();
            // Server unlocks plus locally recorded (including pending-sync)
            // awards, counted once and limited to core/official achievements.
            let total = prepared.game.achievements.iter()
                .filter(|achievement| achievement.flags == 3).count();
            let unlocked_count = prepared.game.achievements.iter()
                .filter(|achievement| achievement.flags == 3
                    && (prepared.unlocked.contains(&achievement.id)
                        || store.unlocks.contains_key(&achievement.id)
                        || unsaved.contains_key(&achievement.id)))
                .count();
            let game_icon = badges::game_icon_path(&store.dir, &prepared.game.image_icon);
            let mut achievements = BTreeMap::new();
            let mut unsupported = 0;
            for achievement in prepared.game.achievements {
                if achievement.flags != 3
                    || prepared.unlocked.contains(&achievement.id)
                    || store.unlocks.contains_key(&achievement.id)
                {
                    continue;
                }
                if runtime.activate(achievement.id, &achievement.definition) {
                    achievements.insert(achievement.id, achievement);
                } else {
                    unsupported += 1;
                }
            }
            drop(store);
            let title = if prepared.game.id == 0 {
                "No achievements for this ROM".into()
            } else {
                prepared.game.title
            };
            let detail = if prepared.game.id == 0 {
                String::new()
            } else if unsupported == 0 {
                format!("You have {unlocked_count} of {total} achievements unlocked")
            } else {
                format!("You have {unlocked_count} of {total} achievements unlocked ({unsupported} unsupported)")
            };
            let _ = notices.send(Notice {
                badge: game_icon,
                generation,
                title,
                detail,
                kind: NoticeKind::Status,
            });
            playing = Some(Playing {
                generation,
                hash: prepared.hash,
                console: prepared.game.console,
                runtime,
                achievements,
                previous: None,
                warned_gap: false,
            });
        }
        // Keep normal 5ms responsiveness during gameplay. When Android
        // suspends the GL thread and no frames arrive, park for up to 40ms
        // (still below the 8-frame queue at 120Hz). On the empty shelf the
        // evaluator only needs a 100ms control wakeup. HTTP prefetch proceeds
        // independently while the device is awake.
        let wait = if playing.is_none() { 100 }
            else if last_frame_received.elapsed() > Duration::from_millis(250) { 40 }
            else { 5 };
        match frames.recv_timeout(Duration::from_millis(wait)) {
            Ok(frame) => {
                last_frame_received = Instant::now();
                evaluate(frame, &mut playing, &store, &notices, &mut unsaved);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if presence_updated.elapsed() >= Duration::from_secs(1) {
            *status.presence.lock().unwrap() = playing
                .as_ref()
                .map(|game| (game.generation, game.runtime.presence()));
            presence_updated = Instant::now();
        }
        status.unsaved.store(!unsaved.is_empty(), Ordering::Release);
    }
}

fn evaluate(
    frame: Frame,
    playing: &mut Option<Playing>,
    store: &Arc<Mutex<Store>>,
    notices: &mpsc::Sender<Notice>,
    unsaved: &mut BTreeMap<u32, (Unlock, Notice)>,
) {
    if let Some(game) = playing
        .as_mut()
        .filter(|g| g.generation == frame.generation)
    {
        if let Some((sequence, timeline)) = game.previous {
            if timeline != frame.timeline || sequence + 1 != frame.sequence {
                game.runtime.reset();
                if timeline == frame.timeline && !game.warned_gap {
                    let _ = notices.send(Notice::status(
                        game.generation,
                        "Achievement tracking interrupted",
                    ));
                    game.warned_gap = true;
                }
            }
        }
        game.previous = Some((frame.sequence, frame.timeline));
        if frame.valid[0] == 0 || (frame.console == 5 && frame.valid[1] == 0) {
            if !game.warned_gap {
                let _ = notices.send(Notice::status(
                    game.generation,
                    "Core does not expose achievement RAM",
                ));
                game.warned_gap = true;
            }
        } else {
            let earned = if game.console == frame.console {
                game.runtime.frame(&frame.ram, &frame.valid, frame.console).to_vec()
            } else {
                // ROM with an incompatible RA console ID: no false awards.
                Vec::new()
            };
            for id in earned {
                let unlock = Unlock {
                    id,
                    hash: game.hash.clone(),
                    earned_at: frame.earned_at,
                    synced: false,
                };
                if let Some(achievement) = game.achievements.get(&id) {
                    let notice = Notice {
                        badge: badges::path(&store.lock().unwrap().dir, &achievement.badge),
                        generation: game.generation,
                        title: format!("{} (+{})", achievement.title, achievement.points),
                        detail: achievement.description.clone(),
                        kind: NoticeKind::Earned,
                    };
                    match store.lock().unwrap().record(unlock.clone()) {
                        Ok(true) => {
                            let _ = notices.send(notice);
                        }
                        Ok(false) => {}
                        Err(error) => {
                            let _ = notices.send(Notice::status(game.generation, &error));
                            unsaved.insert(id, (unlock, notice));
                        }
                    }
                }
                game.runtime.deactivate(id);
            }
        }
    }
    let _ = frame.recycle.try_send(frame.ram);
}

fn retry_unsaved(
    unsaved: &mut BTreeMap<u32, (Unlock, Notice)>,
    store: &Arc<Mutex<Store>>,
    notices: &mpsc::Sender<Notice>,
) {
    unsaved.retain(
        |_, (unlock, notice)| match store.lock().unwrap().record(unlock.clone()) {
            Ok(true) => {
                let _ = notices.send(notice.clone());
                false
            }
            Ok(false) => false,
            Err(_) => true,
        },
    );
}

/// Canonical title from the achievement cache identified by this ROM's content hash.
/// Call on an I/O worker. This does not authenticate, send ROM data, or update the shared
/// library index. A renamed ROM can reuse the same cached identity immediately.
pub fn artwork_title(root: &std::path::Path, rom: &std::path::Path) -> Option<String> {
    let config = storage::Config::read(&root.join("Config/retroachievements.toml")).ok()?;
    if !config.enabled || config.username.is_empty() {
        return None;
    }
    let account = format!("{:x}", md5::compute(config.username.to_lowercase()));
    let dir = root.join("Saves/RetroAchievements").join(account);
    let hash = library::content_hash(rom).ok()?;
    let (game, _): (storage::Game, std::collections::BTreeSet<u32>) =
        storage::read(&dir.join(format!("{hash}.json"))).ok()?;
    (matches!(game.console, 4 | 5 | 6)
        && game.id != 0
        && !game.title.trim().is_empty()
        && !game.title.starts_with("Unsupported Game Version"))
    .then_some(game.title)
}

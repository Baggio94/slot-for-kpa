use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::{self, Auth, Config, Game, Store, Unlock};
use crate::{Notice, Status, SyncStatus};

const API: &str = "https://retroachievements.org/dorequest.php";
const VERSION: &str = "12.2.1";

pub(crate) struct Load {
    pub generation: u64,
    pub path: PathBuf,
    pub console: u32,
}

pub(crate) struct Prepared {
    pub generation: u64,
    pub hash: String,
    pub game: Game,
    pub unlocked: BTreeSet<u32>,
    pub cached: bool,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Failure {
    Network,
    Rejected,
    Authentication,
    Invalid,
}

pub(crate) trait Transport {
    fn call(&mut self, fields: &[(&str, String)]) -> Result<Value, Failure>;
    fn badge(&mut self, _name: &str) -> Result<Vec<u8>, Failure> {
        Err(Failure::Invalid)
    }
    fn game_icon(&mut self, _id: &str) -> Result<Vec<u8>, Failure> {
        Err(Failure::Invalid)
    }
}

pub(crate) struct Http(ureq::Agent);

impl Http {
    pub(crate) fn new() -> Self {
        Self(
            ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(5)))
                .max_redirects(0)
                .user_agent(concat!(
                    "slot/",
                    env!("CARGO_PKG_VERSION"),
                    " rcheevos/12.2.1"
                ))
                .build()
                .into(),
        )
    }
}

impl Transport for Http {
    fn game_icon(&mut self, id: &str) -> Result<Vec<u8>, Failure> {
        // Strict numeric ImageIcon identifier; never accept caller-supplied URL.
        if id.is_empty() || id.len() > 12 || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Failure::Invalid);
        }
        self.0
            .get(format!("https://media.retroachievements.org/Images/{id}.png"))
            .call()
            .map_err(|_| Failure::Network)?
            .body_mut()
            .with_config()
            .limit(crate::badges::MAX_BYTES)
            .read_to_vec()
            .map_err(|_| Failure::Invalid)
    }
    fn badge(&mut self, name: &str) -> Result<Vec<u8>, Failure> {
        crate::badges::path(std::path::Path::new(""), name).ok_or(Failure::Invalid)?;
        self.0
            .get(format!(
                "https://media.retroachievements.org/Badge/{name}.png"
            ))
            .call()
            .map_err(|_| Failure::Network)?
            .body_mut()
            .with_config()
            .limit(crate::badges::MAX_BYTES)
            .read_to_vec()
            .map_err(|_| Failure::Invalid)
    }
    fn call(&mut self, fields: &[(&str, String)]) -> Result<Value, Failure> {
        let form: Vec<_> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let mut response = self
            .0
            .post(API)
            .send_form(form)
            .map_err(|_| Failure::Network)?;
        response
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_json()
            .map_err(|_| Failure::Invalid)
    }
}

fn success(value: Value) -> Result<Value, Failure> {
    if value.get("Success") == Some(&Value::Bool(true)) {
        Ok(value)
    } else {
        Err(Failure::Rejected)
    }
}

fn call(
    http: &mut impl Transport,
    auth: &Auth,
    request: &str,
    extra: &[(&str, String)],
) -> Result<Value, Failure> {
    let mut fields = vec![
        ("r", request.into()),
        ("u", auth.username.clone()),
        ("t", auth.token.clone()),
    ];
    fields.extend_from_slice(extra);
    success(http.call(&fields)?)
}

fn login(
    http: &mut impl Transport,
    config: &Config,
    remembered: Option<&Auth>,
) -> Result<Auth, Failure> {
    let mut fields = vec![("r", "login2".into()), ("u", config.username.clone())];
    if !config.token.is_empty() {
        fields.push(("t", config.token.clone()));
    } else if let Some(auth) = remembered {
        fields.push(("t", auth.token.clone()));
    } else {
        fields.push(("p", config.password.clone()));
    }
    let response = match http.call(&fields).and_then(success) {
        Err(Failure::Rejected) if config.token.is_empty() && !config.password.is_empty() => {
            // A remembered token can expire. An explicitly supplied password can renew it.
            success(http.call(&[
                ("r", "login2".into()),
                ("u", config.username.clone()),
                ("p", config.password.clone()),
            ])?)?
        }
        result => result?,
    };
    let username = response["User"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(Failure::Invalid)?;
    let token = response["Token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(Failure::Invalid)?;
    Ok(Auth {
        username: username.into(),
        token: token.into(),
    })
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Matches rc_api_init_award_achievement_request, including the original unlock age.
pub(crate) fn award(
    http: &mut impl Transport,
    auth: &Auth,
    unlock: &Unlock,
    now: u64,
) -> Result<(), Failure> {
    let age = now
        .saturating_sub(unlock.earned_at)
        .min(u64::from(u32::MAX));
    let mut signature = format!("{}{}0", unlock.id, auth.username);
    if age > 0 {
        signature.push_str(&format!("{}{age}", unlock.id));
    }
    let mut extra = vec![
        ("a", unlock.id.to_string()),
        ("h", "0".into()),
        ("m", unlock.hash.clone()),
        ("v", format!("{:x}", md5::compute(signature))),
    ];
    if age > 0 {
        extra.push(("o", age.to_string()));
    }
    let mut fields = vec![
        ("r", "awardachievement".into()),
        ("u", auth.username.clone()),
        ("t", auth.token.clone()),
    ];
    fields.extend(extra);
    let response = http.call(&fields)?;
    // A retry after a lost response may say the server already has this unlock.
    if response["Success"] == true
        || response["Error"]
            .as_str()
            .is_some_and(|s| s.starts_with("User already has"))
    {
        Ok(())
    } else {
        Err(Failure::Rejected)
    }
}

fn unlocked(response: &Value) -> Result<BTreeSet<u32>, Failure> {
    let mut ids = BTreeSet::new();
    for field in ["Unlocks", "HardcoreUnlocks"] {
        // Like rc_api_process_start_session_response, accept omitted empty lists.
        if response[field].is_null() {
            continue;
        }
        let entries = response[field].as_array().ok_or(Failure::Invalid)?;
        for entry in entries {
            let id = entry["ID"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(Failure::Invalid)?;
            ids.insert(id);
        }
    }
    Ok(ids)
}

struct Active {
    generation: u64,
    hash: String,
    console: u32,
    ready: bool,
    online: bool,
    last_ping: Instant,
}

fn prefetch(
    http: &mut impl Transport,
    auth: &Auth,
    dir: &std::path::Path,
    hash: &str,
    console: u32,
) -> Result<Game, Failure> {
    let resolved = call(http, auth, "gameid", &[("m", hash.into())])?;
    let id = resolved["GameID"].as_u64().ok_or(Failure::Invalid)?;
    let (game, ids) = if id == 0 {
        (
            Game {
                presence: String::new(),
                id: 0,
                console,
                title: "No achievements for this ROM".into(),
                image_icon: String::new(),
                achievements: vec![],
            },
            BTreeSet::new(),
        )
    } else {
        let patch = call(http, auth, "patch", &[("g", id.to_string())])?;
        let game: Game =
            serde_json::from_value(patch["PatchData"].clone()).map_err(|_| Failure::Invalid)?;
        if game.console != console || u64::from(game.id) != id {
            return Err(Failure::Invalid);
        }
        let unlocks = call(
            http,
            auth,
            "unlocks",
            &[("g", id.to_string()), ("h", "0".into())],
        )?;
        let ids: BTreeSet<u32> =
            serde_json::from_value(unlocks["UserUnlocks"].clone()).map_err(|_| Failure::Invalid)?;
        (game, ids)
    };
    storage::write(&dir.join(format!("{hash}.json")), &(&game, ids))
        .map_err(|_| Failure::Invalid)?;
    Ok(game)
}

/// Durable window prevents repeated launches from bypassing the server's
/// bulk-preparation budget. A reservation is persisted before making HTTP calls.
#[derive(Default, Serialize, Deserialize)]
struct PrefetchBudget {
    attempts: Vec<u64>,
}
impl PrefetchBudget {
    const WINDOW: u64 = 30 * 60;
    const LIMIT: usize = 100;

    fn reserve(&mut self, dir: &std::path::Path, at: u64) -> Result<bool, Failure> {
        self.attempts.retain(|t| *t <= at && at.saturating_sub(*t) < Self::WINDOW);
        if self.attempts.len() >= Self::LIMIT { return Ok(false); }
        self.attempts.push(at);
        if storage::write(&dir.join("smart-prefetch-budget.json"), self).is_err() {
            self.attempts.pop();
            return Err(Failure::Invalid);
        }
        Ok(true)
    }

    fn delay_seconds(&self, at: u64) -> u64 {
        self.attempts.first().map_or(30, |t| t.saturating_add(Self::WINDOW).saturating_sub(at).max(1))
    }
}

fn already_prepared(dir: &std::path::Path, hash: &str, console: u32) -> bool {
    storage::read::<(Game, BTreeSet<u32>)>(&dir.join(format!("{hash}.json")))
        .is_ok_and(|(game, _)| game.console == console)
}

/// Older prepared PatchData can lack ImageIcon, although the ROM and award
/// cache are complete. Repair only the missing game icon metadata; never
/// refresh unlocks, create a session, or touch the offline award ledger.
#[derive(Default, Serialize, Deserialize)]
struct IconRefreshJournal {
    // ROM hash -> (RA GameID, last successful check). A game without an
    // official image must not be fetched on every launch.
    checked: BTreeMap<String, (u32, u64)>,
}
impl IconRefreshJournal {
    const FILE: &'static str = "icon-refresh-checks-v1.json";
    const RETRY_AFTER: u64 = 7 * 24 * 60 * 60;

    fn load(dir: &std::path::Path) -> Self {
        storage::read(&dir.join(Self::FILE)).unwrap_or_default()
    }

    fn needs_refresh(&self, dir: &std::path::Path, hash: &str, console: u32, at: u64) -> bool {
        let Ok((game, _)) =
            storage::read::<(Game, BTreeSet<u32>)>(&dir.join(format!("{hash}.json")))
        else { return false };
        game.console == console && game.id != 0
            && crate::badges::game_icon_path(dir, &game.image_icon).is_none()
            && self.checked.get(hash).is_none_or(|(id, checked_at)| {
                *id != game.id || at.saturating_sub(*checked_at) >= Self::RETRY_AFTER
            })
    }

    fn record(&mut self, dir: &std::path::Path, hash: &str, id: u32, at: u64)
        -> Result<(), Failure>
    {
        let before = self.checked.insert(hash.into(), (id, at));
        if storage::write(&dir.join(Self::FILE), self).is_err() {
            if let Some(old) = before { self.checked.insert(hash.into(), old); }
            else { self.checked.remove(hash); }
            return Err(Failure::Invalid);
        }
        Ok(())
    }
}

fn repair_missing_game_icon(
    http: &mut impl Transport,
    auth: &Auth,
    dir: &std::path::Path,
    hash: &str,
    console: u32,
) -> Result<Game, Failure> {
    let file = dir.join(format!("{hash}.json"));
    let (mut game, ids): (Game, BTreeSet<u32>) =
        storage::read(&file).map_err(|_| Failure::Invalid)?;
    if game.console != console || game.id == 0 { return Err(Failure::Invalid); }
    let patch = call(http, auth, "patch", &[("g", game.id.to_string())])?;
    let data = &patch["PatchData"];
    if data["ID"].as_u64() != Some(u64::from(game.id))
        || data["ConsoleID"].as_u64() != Some(u64::from(console)) {
        return Err(Failure::Invalid);
    }
    let icon = data["ImageIcon"].as_str().unwrap_or_default();
    if crate::badges::game_icon_path(dir, icon).is_some() {
        game.image_icon = icon.into();
        // Keep the existing achievements and server unlock set exactly as-is.
        storage::write(&file, &(&game, &ids)).map_err(|_| Failure::Invalid)?;
    }
    Ok(game)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    root: PathBuf,
    config: Config,
    store: Arc<Mutex<Store>>,
    loads: mpsc::Receiver<Option<Load>>,
    smart_hashes: mpsc::Receiver<(String, u32)>,
    prepared: mpsc::Sender<Prepared>,
    notices: mpsc::Sender<Notice>,
    status: Arc<Status>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    mut http: impl Transport,
) {
    let dir = store.lock().unwrap().dir.clone();
    let mut auth: Option<Auth> = storage::read(&dir.join("auth.json")).ok();
    let mut verified = false;
    let mut active: Option<Active> = None;
    let mut next_attempt = Instant::now();
    let mut delay = 30;
    let mut warned = false;
    let mut last_award = 0;
    let mut library = crate::library::pending(&root, &dir, now());
    let mut library_total = library.len();
    let mut library_failed = false;
    let mut rescanned = Instant::now();
    let mut badges = crate::badges::Queue::new(&dir);
    let mut smart_queue: VecDeque<(String, u32)> = VecDeque::new();
    let mut queued_hashes: BTreeSet<String> = BTreeSet::new();
    let mut icon_refresh = IconRefreshJournal::load(&dir);
    let quota_file = dir.join("smart-prefetch-budget.json");
    let mut budget: PrefetchBudget = if quota_file.exists() {
        // Fail closed if the quota journal is corrupted: repeated restarts
        // must not bypass the server's request budget.
        storage::read(&quota_file).unwrap_or_else(|_| PrefetchBudget {
            attempts: vec![now(); PrefetchBudget::LIMIT],
        })
    } else { PrefetchBudget::default() };
    loop {
        // Stop accepting fresh HTTP work on account logout. An in-flight
        // request may take up to the five-second global ureq timeout, but
        // auth must remain on disk until this worker exits completely.
        if shutdown.load(std::sync::atomic::Ordering::Acquire) { return; }
        // Android SAF fingerprints arrive independently of opening a game.
        // Keep the HTTP queue small and deduplicated, with no ROM bytes retained.
        for (hash, console) in smart_hashes.try_iter().take(32) {
            if !matches!(console, 4 | 5 | 6) || hash.len() != 32 ||
                !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let hash = hash.to_ascii_lowercase();
            if (already_prepared(&dir, &hash, console)
                    && !icon_refresh.needs_refresh(&dir, &hash, console, now()))
                || queued_hashes.contains(&hash)
                || smart_queue.len() >= 5000
            {
                continue;
            }
            queued_hashes.insert(hash.clone());
            smart_queue.push_back((hash, console));
        }
        match loads.recv_timeout(Duration::from_millis(200)) {
            Ok(mut load) => {
                while let Ok(newer) = loads.try_recv() {
                    load = newer;
                }
                active = load.and_then(|load| {
                    // GBA identification is the MD5 of the complete uncompressed ROM.
                    // File I/O and hashing happen here, never on the emulator/UI threads.
                    let hash = crate::library::hash(&dir, &load.path).ok()?;
                    let mut game = Active {
                        generation: load.generation,
                        hash,
                        console: load.console,
                        ready: false,
                        online: false,
                        last_ping: Instant::now(),
                    };
                    if auth.is_some() {
                        let cache = dir.join(format!("{}.json", game.hash));
                        if let Ok((data, ids)) = storage::read::<(Game, BTreeSet<u32>)>(&cache) {
                            if data.console == game.console {
                                badges.enqueue(&data);
                                let _ = prepared.send(Prepared {
                                    generation: game.generation,
                                    hash: game.hash.clone(),
                                    game: data,
                                    unlocked: ids,
                                    cached: true,
                                });
                                game.ready = true;
                            }
                        }
                    }
                    Some(game)
                });
                next_attempt = Instant::now();
                warned = false;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if shutdown.load(std::sync::atomic::Ordering::Acquire) { return; }
        // Metadata preparation and badge downloads each occupy half of a cache refresh.
        status.pending.store(
            !library.is_empty()
                || library_failed
                || !smart_queue.is_empty()
                || badges.pending()
                || store.lock().unwrap().unlocks.values().any(|u| !u.synced)
                || active.as_ref().is_some_and(|game| !game.ready),
            std::sync::atomic::Ordering::Release,
        );
        status.progress.store(
            cache_progress(
                library_total,
                library.len(),
                badges.pending(),
                badges.percent(),
            )
            .map_or(0, |percent| percent + 1),
            std::sync::atomic::Ordering::Release,
        );
        if status
            .reconnect
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            next_attempt = Instant::now();
        }
        if Instant::now() < next_attempt {
            continue;
        }
        if rescanned.elapsed() >= Duration::from_secs(300) {
            library = crate::library::pending(&root, &dir, now());
            library_total = library.len();
            library_failed = false;
            badges.scan();
            rescanned = Instant::now();
        }
        if !verified
            || !library.is_empty()
            || !smart_queue.is_empty()
            || (badges.pending() && !badges.waiting)
            || store.lock().unwrap().unlocks.values().any(|u| !u.synced)
            || active.as_ref().is_some_and(|game| {
                !game.online || (game.ready && game.last_ping.elapsed() >= Duration::from_secs(120))
            })
        {
            status.set(SyncStatus::Syncing);
        }
        let result = (|| -> Result<(), Failure> {
            if !verified {
                let logged_in = login(&mut http, &config, auth.as_ref()).map_err(|e| {
                    if e == Failure::Rejected {
                        Failure::Authentication
                    } else {
                        e
                    }
                })?;
                storage::write(&dir.join("auth.json"), &logged_in).map_err(|_| Failure::Invalid)?;
                auth = Some(logged_in);
                verified = true;
            }
            let auth = auth.as_ref().ok_or(Failure::Invalid)?;
            // Rotate through pending awards so one rejected/deleted achievement does not
            // prevent other games from syncing or the library from being prepared.
            let mut deferred_error = None;
            let pending = {
                let store = store.lock().unwrap();
                store
                    .unlocks
                    .range((
                        std::ops::Bound::Excluded(last_award),
                        std::ops::Bound::Unbounded,
                    ))
                    .map(|(_, u)| u)
                    .find(|u| !u.synced)
                    .or_else(|| store.unlocks.values().find(|u| !u.synced))
                    .cloned()
            };
            if let Some(unlock) = pending {
                last_award = unlock.id;
                match award(&mut http, auth, &unlock, now()) {
                    Ok(()) => {
                        let mut store = store.lock().unwrap();
                        store.ack(unlock.id).map_err(|_| Failure::Invalid)?;
                        if store.unlocks.values().all(|u| u.synced) {
                            let _ = notices.send(Notice::status(0, "Achievements synced"));
                        }
                    }
                    Err(Failure::Network) => return Err(Failure::Network),
                    Err(error) => deferred_error = Some(error),
                }
            }
            if let Some(game) = active.as_mut() {
                if !game.online {
                    let resolved = call(&mut http, auth, "gameid", &[("m", game.hash.clone())])?;
                    let id = resolved["GameID"].as_u64().ok_or(Failure::Invalid)?;
                    if id == 0 {
                        let _ = notices.send(Notice::status(
                            game.generation,
                            "No achievements for this ROM",
                        ));
                        game.online = true;
                        game.ready = false;
                    } else {
                        let patch = call(&mut http, auth, "patch", &[("g", id.to_string())])?;
                        let data: Game = serde_json::from_value(patch["PatchData"].clone())
                            .map_err(|_| Failure::Invalid)?;
                        if data.console != game.console || u64::from(data.id) != id {
                            return Err(Failure::Invalid);
                        }
                        let session = call(
                            &mut http,
                            auth,
                            "startsession",
                            &[
                                ("g", id.to_string()),
                                ("h", "0".into()),
                                ("m", game.hash.clone()),
                                ("l", VERSION.into()),
                            ],
                        )?;
                        let ids = unlocked(&session)?;
                        storage::write(&dir.join(format!("{}.json", game.hash)), &(&data, &ids))
                            .map_err(|_| Failure::Invalid)?;
                        badges.enqueue(&data);
                        ping(&mut http, auth, &status, game, &data)?;
                        let _ = prepared.send(Prepared {
                            generation: game.generation,
                            hash: game.hash.clone(),
                            game: data,
                            unlocked: ids,
                            cached: false,
                        });
                        game.ready = true;
                        game.online = true;
                        game.last_ping = Instant::now();
                    }
                } else if game.ready && game.last_ping.elapsed() >= Duration::from_secs(120) {
                    let cached: (Game, BTreeSet<u32>) =
                        storage::read(&dir.join(format!("{}.json", game.hash)))
                            .map_err(|_| Failure::Invalid)?;
                    ping(&mut http, auth, &status, game, &cached.0)?;
                    game.last_ping = Instant::now();
                }
            }
            // Prefer gameplay and pending unlocks over Smart Cache background work.
            // Preparing a hash uses gameid/patch/unlocks, never starts a session.
            if active.is_none() {
                if let Some(hash) = smart_queue.front() {
                    let repair = icon_refresh.needs_refresh(&dir, &hash.0, hash.1, now());
                    if already_prepared(&dir, &hash.0, hash.1) && !repair {
                        let done = smart_queue.pop_front().unwrap();
                        queued_hashes.remove(&done.0);
                    } else if budget.reserve(&dir, now())? {
                        let result = if repair {
                            // One PatchData call only; preserve previously earned
                            // unlocks and current offline progress unmodified.
                            repair_missing_game_icon(&mut http, auth, &dir, &hash.0, hash.1)
                                .and_then(|game| {
                                    icon_refresh.record(&dir, &hash.0, game.id, now())?;
                                    Ok(game)
                                })
                        } else {
                            prefetch(&mut http, auth, &dir, &hash.0, hash.1)
                        };
                        match result {
                            Ok(game) => {
                                badges.enqueue(&game);
                                let done = smart_queue.pop_front().unwrap();
                                queued_hashes.remove(&done.0);
                            }
                            Err(Failure::Network) => return Err(Failure::Network),
                            Err(error) => {
                                // A rejected/bad ROM must not spin on a live queue.
                                // The next SAF pass may offer it again, rate limited.
                                let failed = smart_queue.pop_front().unwrap();
                                queued_hashes.remove(&failed.0);
                                deferred_error = Some(error);
                            }
                        }
                    }
                }
            }
            // One ROM per pass, with a one-second gap. Prefetch uses `unlocks`, not
            // `startsession`, so preparing a library doesn't claim the user played it.
            if let Some(path) = library.front() {
                if let Ok(hash) = crate::library::hash(&dir, path) {
                    match prefetch(&mut http, auth, &dir, &hash, 5) {
                        Ok(game) => {
                            crate::library::checked(&dir, path, now());
                            badges.enqueue(&game);
                        }
                        Err(Failure::Network) => return Err(Failure::Network),
                        Err(error) => {
                            library_failed = true;
                            deferred_error = Some(error);
                        }
                    }
                } else {
                    library_failed = true;
                }
                // Removed/unreadable ROMs are reconsidered on the next library scan.
                library.pop_front();
                if library.is_empty() && !library_failed && !badges.pending() {
                    let _ = notices.send(Notice::status(0, "Offline achievement cache ready"));
                }
            }
            badges.step(&mut http);
            deferred_error.map_or(Ok(()), Err)
        })();
        status.pending.store(
            !library.is_empty()
                || library_failed
                || !smart_queue.is_empty()
                || badges.pending()
                || store.lock().unwrap().unlocks.values().any(|u| !u.synced)
                || active.as_ref().is_some_and(|game| !game.ready),
            std::sync::atomic::Ordering::Release,
        );
        status.progress.store(
            cache_progress(
                library_total,
                library.len(),
                badges.pending(),
                badges.percent(),
            )
            .map_or(0, |percent| percent + 1),
            std::sync::atomic::Ordering::Release,
        );
        match result {
            Ok(()) => {
                status.set(if library_failed || badges.waiting {
                    SyncStatus::Attention
                } else if !library.is_empty()
                    || !smart_queue.is_empty()
                    || badges.pending()
                    || store.lock().unwrap().unlocks.values().any(|u| !u.synced)
                {
                    SyncStatus::Syncing
                } else {
                    SyncStatus::Ready
                });
                delay = 30;
                warned = false;
                // Respect the persisted rolling 100-game/30-minute budget.
                let limit_reached = !smart_queue.is_empty()
                    && budget.attempts.iter()
                        .filter(|at| now().saturating_sub(**at) < PrefetchBudget::WINDOW)
                        .count() >= PrefetchBudget::LIMIT;
                next_attempt = Instant::now()
                    + Duration::from_secs(if limit_reached {
                        budget.delay_seconds(now())
                    } else { 1 });
            }
            Err(error) => {
                status.set(if error == Failure::Network {
                    SyncStatus::Offline
                } else {
                    SyncStatus::Attention
                });
                // Reauthenticate after errors. No rejected or malformed response consumes an award.
                verified = false;
                let quiet_offline = error == Failure::Network
                    && (active.as_ref().is_some_and(|g| g.ready)
                        || (active.is_none() && auth.is_some()));
                if !warned && !quiet_offline {
                    let generation = active.as_ref().map_or(0, |g| g.generation);
                    let title = match error {
                        Failure::Network if active.as_ref().is_some_and(|g| g.ready) => {
                            "Achievements offline - sync later"
                        }
                        Failure::Network => "Achievements need internet once",
                        Failure::Rejected => "Achievement sync needs attention",
                        Failure::Authentication => "Achievements: check account config",
                        Failure::Invalid => "Achievements: data or storage error",
                    };
                    let _ = notices.send(Notice::status(generation, title));
                    warned = true;
                }
                next_attempt = Instant::now() + Duration::from_secs(delay);
                delay = (delay * 2).min(300);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn smart_cache_quota_persists_and_refuses_101st_new_game() {
        let root = tempfile::tempdir().unwrap();
        let mut budget = PrefetchBudget::default();
        for i in 0..PrefetchBudget::LIMIT {
            assert!(budget.reserve(root.path(), 42_000 + i as u64).unwrap());
        }
        let mut reloaded: PrefetchBudget =
            storage::read(&root.path().join("smart-prefetch-budget.json")).unwrap();
        assert!(!reloaded.reserve(root.path(), 42_100).unwrap());
        assert_eq!(reloaded.attempts.len(), 100);
        assert!(reloaded.reserve(root.path(), 43_800).unwrap());
        assert_eq!(reloaded.attempts.len(), 100);
    }

    #[test]
    fn smart_cache_only_skips_valid_prepared_gba_patches() {
        let root = tempfile::tempdir().unwrap();
        let hash = "0123456789abcdef0123456789abcdef";
        assert!(!already_prepared(root.path(), hash, 5));
        storage::write(&root.path().join(format!("{hash}.json")),
            &(Game {
                presence: String::new(),
                id: 1,
                console: 5,
                title: "fixture".into(),
                image_icon: String::new(),
                achievements: Vec::new(),
            }, BTreeSet::<u32>::new())).unwrap();
        assert!(already_prepared(root.path(), hash, 5));
        std::fs::write(root.path().join(format!("{hash}.json")), b"broken").unwrap();
        assert!(!already_prepared(root.path(), hash, 5));
    }


    #[test]
    fn old_prepared_patch_without_icon_is_repaired_once_and_unrecognized_rom_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let hash = "0123456789abcdef0123456789abcdef";
        let file = dir.join(format!("{hash}.json"));
        let game = Game {
            presence: String::new(), id: 123, console: 5,
            title: "Old prepared game".into(), image_icon: String::new(),
            achievements: Vec::new(),
        };
        storage::write(&file, &(&game, BTreeSet::<u32>::new())).unwrap();
        let mut journal = IconRefreshJournal::load(dir);
        assert!(already_prepared(dir, hash, 5));
        assert!(journal.needs_refresh(dir, hash, 5, 10_000_000));
        journal.record(dir, hash, 123, 10_000_000).unwrap();
        assert!(!IconRefreshJournal::load(dir).needs_refresh(dir, hash, 5, 10_000_001));
        assert!(IconRefreshJournal::load(dir).needs_refresh(
            dir, hash, 5, 10_000_000 + IconRefreshJournal::RETRY_AFTER));
        let unknown = Game { id: 0, ..game };
        storage::write(&file, &(&unknown, BTreeSet::<u32>::new())).unwrap();
        assert!(!journal.needs_refresh(dir, hash, 5, 10_000_000));
    }

    #[test]
    fn icon_repair_uses_patch_only_and_preserves_achievements_and_unlocks() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let hash = "fedcba9876543210fedcba9876543210";
        let file = dir.join(format!("{hash}.json"));
        let original = Game {
            presence: "Presence".into(), id: 42, console: 5,
            title: "Test".into(), image_icon: String::new(),
            achievements: vec![storage::Achievement {
                badge: "12345".into(), id: 7, title: "Won".into(),
                description: "Earned offline".into(), points: 5, flags: 3,
                definition: "0xH000000=1".into(),
            }],
        };
        let unlocks = BTreeSet::from([7u32, 11u32]);
        storage::write(&file, &(&original, &unlocks)).unwrap();
        let mut transport = Fake {
            reply: json!({"Success": true, "PatchData": {
                "ID": 42, "ConsoleID": 5, "ImageIcon": "/Images/067895.png"
            }}),
            fields: vec![],
        };
        let auth = Auth { username: "Player".into(), token: "redacted".into() };
        let repaired = repair_missing_game_icon(&mut transport, &auth, dir, hash, 5).unwrap();
        assert_eq!(repaired.image_icon, "/Images/067895.png");
        assert!(transport.fields.contains(&("r".into(), "patch".into())));
        assert!(!transport.fields.iter().any(|(_, value)| value == "unlocks" || value == "startsession"));
        let (saved, saved_unlocks): (Game, BTreeSet<u32>) = storage::read(&file).unwrap();
        assert_eq!(saved_unlocks, unlocks);
        assert_eq!(saved.achievements.len(), 1);
        assert_eq!(saved.achievements[0].id, 7);
        assert_eq!(saved.presence, "Presence");
        assert!(!IconRefreshJournal::default().needs_refresh(dir, hash, 5, 10_000_000));
    }

    #[test]
    fn icon_repair_rejects_wrong_game_id_without_changing_cache() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let file = dir.join(format!("{hash}.json"));
        let original = Game {
            presence: String::new(), id: 42, console: 6,
            title: "Test".into(), image_icon: String::new(),
            achievements: vec![],
        };
        storage::write(&file, &(&original, BTreeSet::<u32>::new())).unwrap();
        let mut transport = Fake {
            reply: json!({"Success": true, "PatchData": {
                "ID": 43, "ConsoleID": 6, "ImageIcon": "/Images/234.png"
            }}),
            fields: vec![],
        };
        let auth = Auth { username: "Player".into(), token: "redacted".into() };
        assert_eq!(repair_missing_game_icon(&mut transport, &auth, dir, hash, 6)
            .map(|_| ()), Err(Failure::Invalid));
        let (saved, _): (Game, BTreeSet<u32>) = storage::read(&file).unwrap();
        assert!(saved.image_icon.is_empty());
    }

    #[test]
    fn empty_session_unlock_lists_may_be_omitted() {
        assert!(unlocked(&json!({"Success":true})).unwrap().is_empty());
        assert_eq!(
            unlocked(&json!({"Unlocks":[{"ID":7,"When":123}]})).unwrap(),
            [7].into()
        );
        assert!(unlocked(&json!({"Unlocks":"invalid"})).is_err());
    }
    struct Fake {
        reply: Value,
        fields: Vec<(String, String)>,
    }
    impl Transport for Fake {
        fn call(&mut self, fields: &[(&str, String)]) -> Result<Value, Failure> {
            self.fields = fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
            Ok(self.reply.clone())
        }
    }
    #[test]
    fn delayed_awards_use_original_time_and_softcore_signature() {
        let mut http = Fake {
            reply: json!({"Success":true}),
            fields: vec![],
        };
        let auth = Auth {
            username: "Player".into(),
            token: "secret".into(),
        };
        let unlock = Unlock {
            id: 123,
            hash: "abcdef".into(),
            earned_at: 1000,
            synced: false,
        };
        award(&mut http, &auth, &unlock, 1060).unwrap();
        let fields: std::collections::BTreeMap<_, _> = http.fields.into_iter().collect();
        assert_eq!(fields["o"], "60");
        assert_eq!(fields["h"], "0");
        assert_eq!(
            fields["v"],
            format!("{:x}", md5::compute("123Player012360"))
        );
    }
    #[test]
    fn an_http_success_is_not_an_award_acknowledgment() {
        let auth = Auth {
            username: "Player".into(),
            token: "secret".into(),
        };
        let unlock = Unlock {
            id: 1,
            hash: "a".into(),
            earned_at: 100,
            synced: false,
        };
        for reply in [json!({"Success":false,"Error":"Expired token"}), json!({})] {
            let mut http = Fake {
                reply,
                fields: vec![],
            };
            assert_eq!(award(&mut http, &auth, &unlock, 50), Err(Failure::Rejected));
            assert!(!http.fields.iter().any(|(k, _)| k == "o"));
        }
    }
}

fn ping(
    http: &mut impl Transport,
    auth: &Auth,
    status: &Status,
    active: &Active,
    game: &Game,
) -> Result<(), Failure> {
    let presence = status
        .presence
        .lock()
        .unwrap()
        .as_ref()
        .filter(|(generation, text)| *generation == active.generation && !text.is_empty())
        .map(|(_, text)| text.clone())
        .unwrap_or_else(|| format!("Playing {}", game.title));
    call(
        http,
        auth,
        "ping",
        &[
            ("g", game.id.to_string()),
            ("h", "0".into()),
            ("x", active.hash.clone()),
            ("m", presence),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod presence_tests {
    use super::*;
    #[test]
    fn heartbeat_uses_current_generation_and_separate_hash_field() {
        struct Capture(Vec<(String, String)>);
        impl Transport for Capture {
            fn call(&mut self, fields: &[(&str, String)]) -> Result<Value, Failure> {
                self.0 = fields
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect();
                Ok(serde_json::json!({"Success":true}))
            }
        }
        let mut http = Capture(vec![]);
        let auth = Auth {
            username: "test".into(),
            token: "fixture".into(),
        };
        let status = Status::default();
        let active = Active {
            generation: 2,
            hash: "rom-hash".into(),
            console: 5,
            ready: true,
            online: true,
            last_ping: Instant::now(),
        };
        let game = Game {
            id: 1,
            console: 5,
            title: "Test".into(),
                image_icon: String::new(),
            presence: String::new(),
            achievements: vec![],
        };
        for (generation, text, expected) in [
            (1, "Old game", "Playing Test"),
            (2, "Level 3", "Level 3"),
            (2, "", "Playing Test"),
        ] {
            *status.presence.lock().unwrap() = Some((generation, text.into()));
            ping(&mut http, &auth, &status, &active, &game).unwrap();
            assert!(http.0.contains(&("m".into(), expected.into())));
            assert!(http.0.contains(&("x".into(), "rom-hash".into())));
        }
    }
}

// A fixed phase split keeps newly discovered badges from moving progress backwards.
fn cache_progress(
    total: usize,
    remaining: usize,
    badges_pending: bool,
    badge_percent: u8,
) -> Option<u8> {
    if remaining > 0 {
        Some((total.saturating_sub(remaining) as u64 * 50 / total.max(1) as u64) as u8)
    } else if badges_pending {
        Some(if total > 0 {
            50 + badge_percent / 2
        } else {
            badge_percent
        })
    } else {
        None
    }
}

#[cfg(test)]
mod cache_progress_tests {
    use super::cache_progress;
    #[test]
    fn progress_covers_metadata_then_badges_and_hides_when_finished() {
        assert_eq!(cache_progress(10, 10, false, 100), Some(0));
        assert_eq!(cache_progress(10, 5, true, 20), Some(25));
        assert_eq!(cache_progress(10, 1, true, 10), Some(45));
        assert_eq!(cache_progress(10, 0, true, 20), Some(60));
        assert_eq!(cache_progress(10, 0, false, 100), None);
        assert_eq!(cache_progress(0, 0, true, 62), Some(62));
        assert_eq!(cache_progress(0, 0, false, 100), None);
    }
}

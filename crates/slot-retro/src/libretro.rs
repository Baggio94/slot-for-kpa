use std::cell::Cell;
use std::ffi::{c_char, c_int, c_uint, c_void, CString};
use std::marker::PhantomData;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use libloading::Library;

use crate::core::{AvInfo, ButtonMask, CoreError, RetroCore, GBA_H, GBA_W};
use crate::ffi::*;
use crate::link::Link;
use crate::rumble::Rumble;

const VIDEO_BYTES: usize = (GBA_W * GBA_H * 4) as usize;

static LIVE: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, PartialEq, Eq)]
enum PixelFormat {
    Xrgb8888,
    Rgb565,
}

struct Host {
    video: Vec<u8>,
    format: PixelFormat,
    audio: Vec<i16>,
    inputs: [u16; 2],
    // Borrowed memory descriptors, valid only while game/core are loaded.
    memory: Vec<MemoryDescriptor>,
    system_dir: CString,
    save_dir: CString,
    rumble: Rumble,
    asked_for_rumble: bool,
    netpacket: Option<NetpacketCallback>,
    net: Link,
    net_peer: Option<u16>,
    audio_status: Option<AudioBufferStatusFn>,
    options: std::collections::HashMap<String, std::ffi::CString>,
    options_dirty: bool,
    declared: std::collections::HashMap<String, Vec<String>>,
}

thread_local! {
    static ACTIVE: Cell<*mut Host> = const { Cell::new(ptr::null_mut()) };
}

struct Active<'a>(PhantomData<&'a mut Host>);

impl Active<'_> {
    fn bind(host: &mut Host) -> Active<'_> {
        ACTIVE.with(|a| a.set(host as *mut Host));
        Active(PhantomData)
    }
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        ACTIVE.with(|a| a.set(ptr::null_mut()));
    }
}

unsafe fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    let p = ACTIVE.with(|a| a.get());
    if p.is_null() {
        return None;
    }
    Some(f(&mut *p))
}

unsafe extern "C" fn log_noop(_level: c_uint, _fmt: *const c_char) {}

unsafe extern "C" fn set_rumble_state(port: c_uint, effect: c_uint, strength: u16) -> bool {
    with_host(|h| h.rumble.set(port, effect, strength)).unwrap_or(false)
}

unsafe extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
    match cmd {
        SET_MEMORY_MAPS => {
            if data.is_null() { return false; }
            let map = &*(data as *const MemoryMap);
            if map.num_descriptors > 1024 ||
               (map.num_descriptors != 0 && map.descriptors.is_null()) { return false; }
            with_host(|h| {
                h.memory = if map.num_descriptors == 0 { Vec::new() }
                    else { std::slice::from_raw_parts(map.descriptors,
                        map.num_descriptors as usize).to_vec() };
            }).is_some()
        }
        GET_CAN_DUPE => {
            if data.is_null() {
                return false;
            }
            *(data as *mut bool) = true;
            true
        }
        SET_PIXEL_FORMAT => {
            if data.is_null() {
                return false;
            }
            let want = match *(data as *const c_uint) {
                PIXEL_FORMAT_XRGB8888 => PixelFormat::Xrgb8888,
                PIXEL_FORMAT_RGB565 => PixelFormat::Rgb565,
                _ => return false,
            };
            with_host(|h| h.format = want).is_some()
        }
        GET_SYSTEM_DIRECTORY | GET_SAVE_DIRECTORY => {
            if data.is_null() {
                return false;
            }
            with_host(|h| {
                let dir = if cmd == GET_SYSTEM_DIRECTORY {
                    &h.system_dir
                } else {
                    &h.save_dir
                };
                *(data as *mut *const c_char) = dir.as_ptr();
                true
            })
            .unwrap_or(false)
        }
        GET_VARIABLE => {
            if data.is_null() {
                return false;
            }
            let var = &mut *(data as *mut Variable);
            if var.key.is_null() {
                return false;
            }
            let Ok(key) = std::ffi::CStr::from_ptr(var.key).to_str() else {
                return false;
            };
            with_host(|h| match h.options.get(key) {
                Some(value) => {
                    var.value = value.as_ptr();
                    true
                }
                None => {
                    var.value = ptr::null();
                    false
                }
            })
            .unwrap_or(false)
        }
        GET_VARIABLE_UPDATE => {
            if data.is_null() {
                return false;
            }
            with_host(|h| {
                *(data as *mut bool) = h.options_dirty;
                h.options_dirty = false;
                true
            })
            .unwrap_or(false)
        }
        SET_VARIABLES => {
            if data.is_null() {
                return with_host(|h| h.declared.clear()).is_some();
            }
            let mut list = std::collections::HashMap::new();
            let mut p = data as *const Variable;
            for _ in 0..4096 {
                let var = &*p;
                if var.key.is_null() {
                    break;
                }
                let Ok(key) = std::ffi::CStr::from_ptr(var.key).to_str() else {
                    break;
                };
                let values = if var.value.is_null() {
                    Vec::new()
                } else {
                    std::ffi::CStr::from_ptr(var.value)
                        .to_str()
                        .ok()
                        .and_then(|v| v.split_once(';'))
                        .map(|(_, values)| {
                            values
                                .trim()
                                .split('|')
                                .map(|s| s.trim().to_string())
                                .collect()
                        })
                        .unwrap_or_default()
                };
                list.insert(key.to_string(), values);
                p = p.add(1);
            }
            with_host(|h| h.declared = list).is_some()
        }
        GET_RUMBLE_INTERFACE => {
            if data.is_null() {
                return false;
            }
            with_host(|h| {
                h.asked_for_rumble = true;
                (*(data as *mut RumbleInterface)).set_rumble_state = set_rumble_state;
            })
            .is_some()
        }
        GET_LOG_INTERFACE => {
            if data.is_null() {
                return false;
            }
            (*(data as *mut LogCallback)).log = log_noop as *const c_void;
            true
        }
        SET_AUDIO_BUFFER_STATUS_CALLBACK => {
            if data.is_null() {
                return with_host(|h| h.audio_status = None).is_some();
            }
            let cb = std::ptr::read(data as *const AudioBufferStatusCallback);
            with_host(|h| h.audio_status = cb.callback).is_some()
        }
        SET_NETPACKET_INTERFACE => {
            if data.is_null() {
                return with_host(|h| h.netpacket = None).is_some();
            }
            with_host(|h| {
                h.netpacket = Some(std::ptr::read(data as *const NetpacketCallback));
            })
            .is_some()
        }
        _ => false,
    }
}

// gpSP 5819380c deliberately sets poll=NULL. In libretro's ABI it is an
// OPTIONAL host poll callback; start/receive/stop are what actual games need.
fn netpacket_ready(cb: &NetpacketCallback) -> bool {
    cb.start.is_some() && cb.receive.is_some() && cb.stop.is_some()
}

unsafe extern "C" fn netpacket_send(
    _flags: c_int,
    buf: *const c_void,
    len: usize,
    _client_id: u16,
) {
    if buf.is_null() || len == 0 {
        return;
    }
    let bytes = std::slice::from_raw_parts(buf as *const u8, len).to_vec();
    with_host(|h| h.net.push_outbound(bytes));
}

unsafe extern "C" fn netpacket_poll_receive() {
    let Some((receive, net, client_id)) = with_host(|h| {
        let receive = h.netpacket.as_ref().and_then(|cb| cb.receive)?;
        let client_id = h.net_peer?;
        Some((receive, h.net.clone(), client_id))
    })
    .flatten() else {
        return;
    };
    if !net.is_active() {
        return;
    }
    while let Some(packet) = net.take_inbound() {
        receive(packet.as_ptr() as *const c_void, packet.len(), client_id);
    }
}

unsafe fn begin_link(client_id: u16) -> bool {
    let Some(start) = with_host(|h| h.netpacket.as_ref().and_then(|cb| cb.start)).flatten() else {
        return false;
    };
    start(client_id, netpacket_send, netpacket_poll_receive);

    let peer = 1u16.wrapping_sub(client_id);
    let connected = with_host(|h| {
        h.net_peer = Some(peer);
        h.netpacket.as_ref().and_then(|cb| cb.connected)
    }).flatten();
    // gpSP returns false if the actual serial mode cannot accept a peer.
    // Calling 'disconnected' here would decrement its client counter even
    // though connected never incremented it; stop directly instead.
    if connected.is_some_and(|callback| !callback(peer)) {
        let stop = with_host(|h| {
            h.net_peer = None;
            h.net.set_active(false);
            h.net.clear();
            h.netpacket.as_ref().and_then(|cb| cb.stop)
        }).flatten();
        if let Some(stop) = stop { stop(); }
        return false;
    }
    with_host(|h| h.net.set_active(true));
    true
}

unsafe fn halt_link() {
    let Some((stop, peer, disconnected)) = with_host(|h| {
        let stop = h.netpacket.as_ref().and_then(|cb| cb.stop);
        let peer = h.net_peer.take();
        let disconnected = h.netpacket.as_ref().and_then(|cb| cb.disconnected);
        (stop, peer, disconnected)
    }) else {
        return;
    };
    if let (Some(peer), Some(disconnected)) = (peer, disconnected) {
        disconnected(peer);
    }
    if let Some(stop) = stop {
        stop();
    }
}

unsafe fn drain_link() {
    let Some((receive, poll, net, client_id)) = with_host(|h| {
        let cb = h.netpacket.as_ref()?;
        let client_id = h.net_peer?;
        Some((cb.receive, cb.poll, h.net.clone(), client_id))
    })
    .flatten() else {
        return;
    };
    if !net.is_active() {
        return;
    }
    if let Some(receive) = receive {
        while let Some(packet) = net.take_inbound() {
            receive(packet.as_ptr() as *const c_void, packet.len(), client_id);
        }
    }
    if let Some(poll) = poll {
        poll();
    }
}

unsafe fn report_audio_status(skip: bool) {
    let Some(status) = with_host(|h| h.audio_status).flatten() else {
        return;
    };
    status(true, if skip { 0 } else { 100 }, skip);
}

unsafe extern "C" fn video_refresh(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
) {
    if data.is_null() {
        return;
    }
    with_host(|h| {
        let cols = width.min(GBA_W) as usize;
        let rows = height.min(GBA_H) as usize;
        let ox = (GBA_W as usize - cols) / 2;
        let oy = (GBA_H as usize - rows) / 2;
        if cols != GBA_W as usize || rows != GBA_H as usize {
            h.video.fill(0);
        }
        for y in 0..rows {
            let src = (data as *const u8).add(y * pitch);
            let row = ((y + oy) * GBA_W as usize + ox) * 4;
            match h.format {
                PixelFormat::Xrgb8888 => {
                    ptr::copy_nonoverlapping(src, h.video.as_mut_ptr().add(row), cols * 4);
                }
                PixelFormat::Rgb565 => {
                    for x in 0..cols {
                        let p = ptr::read_unaligned((src as *const u16).add(x));
                        let r = ((p >> 11) & 0x1f) as u8;
                        let g = ((p >> 5) & 0x3f) as u8;
                        let b = (p & 0x1f) as u8;
                        let o = row + x * 4;
                        h.video[o] = (b << 3) | (b >> 2);
                        h.video[o + 1] = (g << 2) | (g >> 4);
                        h.video[o + 2] = (r << 3) | (r >> 2);
                        h.video[o + 3] = 0;
                    }
                }
            }
        }
    });
}

unsafe extern "C" fn audio_sample(left: i16, right: i16) {
    with_host(|h| h.audio.extend_from_slice(&[left, right]));
}

unsafe extern "C" fn audio_batch(data: *const i16, frames: usize) -> usize {
    if !data.is_null() {
        with_host(|h| {
            h.audio
                .extend_from_slice(std::slice::from_raw_parts(data, frames * 2))
        });
    }
    frames
}

unsafe extern "C" fn input_poll() {}

unsafe extern "C" fn input_state(port: c_uint, device: c_uint, _index: c_uint, id: c_uint) -> i16 {
    if device != DEVICE_JOYPAD {
        return 0;
    }
    with_host(|h| {
        let Some(&input) = h.inputs.get(port as usize) else {
            return 0;
        };
        match id {
            JOYPAD_MASK => input as i16,
            _ if id < 16 => ((input >> id) & 1) as i16,
            _ => 0,
        }
    })
    .unwrap_or(0)
}

// Frontend-private per-core audio sidecar ABI. Ordinary .state files remain
// unchanged: this memory exists only between speculative save and restore.
type AudioSidecarFn = unsafe extern "C" fn(*mut c_void, usize, bool) -> bool;
type AudioSidecarSizeFn = unsafe extern "C" fn() -> usize;
struct AudioSidecar {
    transfer: AudioSidecarFn,
    snapshot: Vec<u8>,
    captured: bool,
}

pub struct LibretroCore {
    api: Api,
    is_gpsp: bool,
    gpsp_audio: Option<AudioSidecar>,
    is_mgba: bool,
    mgba_audio_exports: Option<(AudioSidecarSizeFn, AudioSidecarFn)>,
    mgba_audio: Option<AudioSidecar>,
    // mGBA's retro_serialize_size() serializes the whole core internally;
    // skip this duplicate work on stable-sized speculative snapshots.
    mgba_runahead_size_age: u16,
    host: Box<Host>,
    rom: Vec<u8>,
    rom_path: Option<CString>,
    av: AvInfo,
    loaded: bool,
    _lib: Library,
}

fn cdir(path: &Path) -> Result<CString, CoreError> {
    CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| CoreError::Load(format!("{} contains a nul", path.display())))
}

impl LibretroCore {
    pub fn open(dylib: &Path) -> Result<Self, CoreError> {
        let dir = dylib.parent().unwrap_or(Path::new(".")).to_path_buf();
        Self::open_with(dylib, &dir, &dir)
    }

    pub fn open_with(dylib: &Path, system_dir: &Path, save_dir: &Path) -> Result<Self, CoreError> {
        if LIVE.swap(true, Ordering::SeqCst) {
            return Err(CoreError::Unsupported("a core is already open".into()));
        }
        Self::open_inner(dylib, system_dir, save_dir)
            .inspect_err(|_| LIVE.store(false, Ordering::SeqCst))
    }

    fn try_init_mgba_audio(&mut self) {
        if self.mgba_audio.is_some() || !self.loaded {
            return;
        }
        // mGBA may defer its audio queue allocation until the first
        // retro_run(); retry on the real frame path before enabling run-ahead.
        self.mgba_audio = self.mgba_audio_exports.and_then(|(size_fn, transfer)| {
            let size = unsafe { size_fn() };
            if (4..=1024 * 1024).contains(&size) {
                Some(AudioSidecar {
                    transfer,
                    snapshot: vec![0; size],
                    captured: false,
                })
            } else { None }
        });
    }

    pub fn reported_system_dir(&self) -> String {
        self.host.system_dir.to_string_lossy().into_owned()
    }

    pub fn reported_save_dir(&self) -> String {
        self.host.save_dir.to_string_lossy().into_owned()
    }

    pub fn asked_for_rumble(&self) -> bool {
        self.host.asked_for_rumble
    }

    pub fn set_option(&mut self, key: &str, value: &str) {
        if !self.host.declared.is_empty() {
            match self.host.declared.get(key) {
                None => eprintln!("slot-retro: core declares no option {key:?}, setting it anyway"),
                Some(values) if !values.is_empty() && !values.iter().any(|v| v == value) => {
                    eprintln!("slot-retro: core declares {key:?} as {values:?}, not {value:?}")
                }
                Some(_) => {}
            }
        }
        let Ok(value) = CString::new(value) else {
            return;
        };
        self.host.options.insert(key.to_string(), value);
        self.host.options_dirty = true;
    }

    pub fn options(&self) -> Vec<(String, String)> {
        self.host
            .options
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.to_str().ok()?.to_string())))
            .collect()
    }

    pub fn declared_options(&self) -> &std::collections::HashMap<String, Vec<String>> {
        &self.host.declared
    }

    pub fn option(&self, key: &str) -> Option<String> {
        self.host
            .options
            .get(key)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    fn open_inner(dylib: &Path, system_dir: &Path, save_dir: &Path) -> Result<Self, CoreError> {
        let lib = unsafe { Library::new(dylib) }.map_err(|e| CoreError::Load(e.to_string()))?;
        let api = unsafe { Api::load(&lib) }?;
        let name = dylib.file_name().and_then(|name| name.to_str());
        let is_gpsp = name == Some("libgpsp_libretro.so");
        let is_mgba = name == Some("libmgba_libretro.so");
        // mGBA sizes its audio ring after a ROM is loaded. Resolve its
        // private exports now, but allocate only after core.load_game().
        let mgba_audio_exports = if is_mgba {
            let size = unsafe { lib.get::<AudioSidecarSizeFn>(b"slot_mgba_audio_sidecar_size\0") };
            let transfer = unsafe { lib.get::<AudioSidecarFn>(b"slot_mgba_audio_sidecar\0") };
            match (size, transfer) {
                (Ok(size), Ok(transfer)) => Some((*size, *transfer)),
                _ => None,
            }
        } else { None };
        // An unpatched/foreign gpSP must not run speculative frames: core
        // savestates intentionally omit its audio mixer ring.
        let gpsp_audio = if is_gpsp {
            let size_fn = unsafe {
                lib.get::<AudioSidecarSizeFn>(b"slot_gpsp_audio_sidecar_size\0")
            };
            let transfer = unsafe {
                lib.get::<AudioSidecarFn>(b"slot_gpsp_audio_sidecar\0")
            };
            match (size_fn, transfer) {
                (Ok(size_fn), Ok(transfer)) => {
                    let size = unsafe { (*size_fn)() };
                    if (4..=512 * 1024).contains(&size) {
                        Some(AudioSidecar {
                            transfer: *transfer,
                            snapshot: vec![0; size],
                            captured: false,
                        })
                    } else { None }
                }
                _ => None,
            }
        } else { None };
        let version = unsafe { (api.api_version)() };
        if version != API_VERSION {
            return Err(CoreError::Unsupported(format!("libretro api {version}")));
        }
        let mut host = Box::new(Host {
            video: vec![0; VIDEO_BYTES],
            format: PixelFormat::Xrgb8888,
            audio: Vec::new(),
            inputs: [0; 2],
            memory: Vec::new(),
            system_dir: cdir(system_dir)?,
            save_dir: cdir(save_dir)?,
            rumble: Rumble::default(),
            asked_for_rumble: false,
            netpacket: None,
            net: Link::default(),
            net_peer: None,
            audio_status: None,
            options: std::collections::HashMap::new(),
            options_dirty: false,
            declared: std::collections::HashMap::new(),
        });
        unsafe {
            let _a = Active::bind(&mut host);
            (api.set_environment)(environment);
            (api.set_video_refresh)(video_refresh);
            (api.set_audio_sample)(audio_sample);
            (api.set_audio_sample_batch)(audio_batch);
            (api.set_input_poll)(input_poll);
            (api.set_input_state)(input_state);
            (api.init)();
        }
        Ok(LibretroCore {
            api,
            is_gpsp,
            gpsp_audio,
            is_mgba,
            mgba_audio_exports,
            mgba_audio: None,
            mgba_runahead_size_age: 0,
            host,
            rom: Vec::new(),
            rom_path: None,
            av: AvInfo {
                fps: 0.0,
                sample_rate: 0.0,
            },
            loaded: false,
            _lib: lib,
        })
    }

    fn unload(&mut self) {
        if !self.loaded {
            return;
        }
        {
            let _a = Active::bind(&mut self.host);
            unsafe { (self.api.unload_game)() };
        }
        // This snapshot belongs to the currently loaded ROM and must never
        // be reused when another ROM allocates a different audio queue.
        self.mgba_audio = None;
        self.mgba_runahead_size_age = 0;
        // Release the borrowed descriptor pointers only after libretro
        // returns and Active drops its exclusive Host borrow.
        self.host.memory.clear();
        self.loaded = false;
    }
}

impl Drop for LibretroCore {
    fn drop(&mut self) {
        self.unload();
        unsafe {
            let _a = Active::bind(&mut self.host);
            (self.api.deinit)();
        }
        LIVE.store(false, Ordering::SeqCst);
    }
}

// rcheevos' GBA address space is IWRAM, EWRAM, SRAM in that order.
// libretro does not require a single descriptor per region: gpSP can expose
// EWRAM as eight 32-KiB windows, with nonzero backing-buffer offsets.
// Assemble only the *contiguous prefix* of each region. The C evaluator's
// valid[] bounds then reject any absent pages instead of reading stale RAM.
fn copy_gba_achievement_map(descriptors: &[MemoryDescriptor], ram: &mut [u8]) -> [usize; 3] {
    if ram.len() < 0x58000 {
        return [0; 3];
    }
    let mut valid = [0; 3];
    for (region, (start, destination, size)) in [
        (0x03000000usize, 0usize, 0x8000usize),
        (0x02000000, 0x8000, 0x40000),
        (0x0e000000, 0x48000, 0x10000),
    ].into_iter().enumerate() {
        let mut copied = 0usize;
        while copied < size {
            let address = start + copied;
            let Some(desc) = descriptors.iter().find(|desc| {
                !desc.ptr.is_null()
                    && desc.disconnect == 0
                    && desc.start <= address
                    && desc.start.checked_add(desc.len).is_some_and(|end| address < end)
            }) else { break };
            let delta = address - desc.start;
            let bytes = (desc.len - delta).min(size - copied);
            let Some(source) = desc.offset.checked_add(delta) else { break };
            // Pointer arithmetic must not wrap; descriptor pointers themselves
            // remain borrowed from the trusted libretro core until unload.
            if source.checked_add(bytes).is_none_or(|end| end > isize::MAX as usize) {
                break;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(
                    (desc.ptr as *const u8).add(source),
                    ram[destination + copied..].as_mut_ptr(),
                    bytes,
                );
            }
            copied += bytes;
        }
        valid[region] = copied;
    }
    valid
}

/// mGBA's GB/GBC libretro memory descriptors use actual GB CPU addresses:
/// ROM 0000-7fff, VRAM 8000-9fff, battery A000-bfff, WRAM C000-dfff,
/// IO ff00-ff7f and HRAM ff80-ffff. GBC extended banks occupy 10000+.
/// Assemble the address space without inventing values for unmapped windows.
fn copy_gb_achievement_map(
    descriptors: &[MemoryDescriptor], ram: &mut [u8], coverage: &mut [u8],
) -> usize {
    const LIMIT: usize = 0x17000;
    if ram.len() < LIMIT || coverage.len() < LIMIT { return 0; }
    coverage[..LIMIT].fill(0);
    let mut filled = 0usize;
    for desc in descriptors {
        if desc.ptr.is_null() || desc.disconnect != 0 { continue; }
        let start = desc.start;
        if start >= LIMIT { continue; }
        let len = desc.len.min(LIMIT - start);
        if len == 0 { continue; }
        if desc.offset.checked_add(len).is_none_or(|end| end > isize::MAX as usize) {
            continue;
        }
        // mGBA descriptors can carry select masks for OAM/HRAM. These
        // windows still expose a contiguous, explicitly sized span.
        unsafe {
            std::ptr::copy_nonoverlapping(
                (desc.ptr as *const u8).add(desc.offset),
                ram[start..start + len].as_mut_ptr(), len,
            );
        }
        for flag in &mut coverage[start..start + len] {
            if *flag == 0 { filled += 1; }
            *flag = 1;
        }
    }
    filled
}

impl RetroCore for LibretroCore {
    fn achievement_memory_for_console(
        &self, ram: &mut [u8], coverage: &mut [u8], console: u32
    ) -> [usize; 3] {
        if !self.loaded { return [0; 3]; }
        if console == 4 || console == 6 {
            let filled = copy_gb_achievement_map(&self.host.memory, ram, coverage);
            // An actual mapped WRAM window is required, not merely ROM/VRAM.
            return [if filled > 0 && coverage.get(0xc000) == Some(&1) { filled } else { 0 }, 0, 0];
        }
        self.achievement_memory(ram)
    }

    fn achievement_memory(&self, ram: &mut [u8]) -> [usize; 3] {
        if ram.len() < 0x58000 || !self.loaded {
            return [0; 3];
        }
        let mut valid = copy_gba_achievement_map(&self.host.memory, ram);
        // Some cores expose save memory only through RETRO_MEMORY_SAVE_RAM.
        // Prefer this complete region when the descriptor map is incomplete.
        if valid[2] < 0x10000 {
            let source = unsafe { (self.api.get_memory_data)(MEMORY_SAVE_RAM) };
            let bytes = unsafe { (self.api.get_memory_size)(MEMORY_SAVE_RAM) }.min(0x10000);
            if !source.is_null() && bytes > valid[2] {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        source as *const u8, ram[0x48000..].as_mut_ptr(), bytes,
                    );
                }
                valid[2] = bytes;
            }
        }
        valid
    }
    fn set_option(&mut self, key: &str, value: &str) {
        LibretroCore::set_option(self, key, value);
    }

    fn load(&mut self, rom: &Path) -> Result<(), CoreError> {
        self.unload();
        self.rom = std::fs::read(rom)?;
        self.rom_path = Some(
            CString::new(rom.as_os_str().as_encoded_bytes())
                .map_err(|_| CoreError::Load("rom path contains a nul".into()))?,
        );
        let info = GameInfo {
            path: self.rom_path.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
            data: self.rom.as_ptr() as *const c_void,
            size: self.rom.len(),
            meta: ptr::null(),
        };
        let ok = {
            let _a = Active::bind(&mut self.host);
            unsafe { (self.api.load_game)(&info) }
        };
        if !ok {
            self.rom = Vec::new();
            self.rom_path = None;
            return Err(CoreError::Load(format!("core refused {}", rom.display())));
        }
        self.loaded = true;
        self.mgba_runahead_size_age = 0;
        self.try_init_mgba_audio();
        let mut av = SystemAvInfo::default();
        unsafe { (self.api.get_system_av_info)(&mut av) };
        self.av = AvInfo {
            fps: av.timing.fps,
            sample_rate: av.timing.sample_rate,
        };
        unsafe { (self.api.set_controller_port_device)(0, DEVICE_JOYPAD) };
        Ok(())
    }

    fn run_frame(&mut self, input: ButtonMask) {
        self.run_frame_linked(input, ButtonMask::default());
    }

    fn run_frame_linked(&mut self, p1: ButtonMask, p2: ButtonMask) {
        if !self.loaded {
            return;
        }
        self.host.inputs = [p1.0, p2.0];
        {
            let _a = Active::bind(&mut self.host);
            unsafe { (self.api.run)() };
        }
        // Retry after deferred initialization; never run an mGBA
        // speculative frame without a correctly sized audio snapshot.
        if self.is_mgba && self.mgba_audio.is_none() {
            self.try_init_mgba_audio();
        }
    }

    fn set_frame_skip(&mut self, skip: bool) {
        let _a = Active::bind(&mut self.host);
        unsafe { report_audio_status(skip) };
    }

    fn video_xrgb8888(&self) -> &[u8] {
        &self.host.video
    }

    fn take_audio(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.host.audio)
    }

    fn discard_audio(&mut self) {
        // Speculative run-ahead audio is intentionally inaudible. Clearing
        // preserves capacity for the next speculative libretro frame.
        self.host.audio.clear();
    }

    fn serialize(&mut self) -> Result<Vec<u8>, CoreError> {
        let mut buf = Vec::new();
        self.serialize_into(&mut buf)?;
        Ok(buf)
    }

    fn serialize_into(&mut self, buf: &mut Vec<u8>) -> Result<(), CoreError> {
        // Unlike gpSP, pinned mGBA's retro_serialize_size() calls
        // mCoreSaveStateNamed() and allocates/writes an entire extra state.
        // Calling it before every retro_serialize() doubles the state-work
        // in Run-Ahead. Only reuse a known nonempty caller-owned buffer;
        // normal save/rewind calls supply an empty Vec and remain unchanged.
        // Revalidate periodically and retry immediately if the state grows.
        let reuse_mgba = self.is_mgba && !buf.is_empty()
            && self.mgba_runahead_size_age < 120;
        let size = if reuse_mgba {
            self.mgba_runahead_size_age += 1;
            buf.len()
        } else {
            self.mgba_runahead_size_age = 0;
            unsafe { (self.api.serialize_size)() }
        };
        if size == 0 {
            return Err(CoreError::State("core reports no state".into()));
        }
        buf.resize(size, 0);
        let mut ok = {
            let _a = Active::bind(&mut self.host);
            unsafe { (self.api.serialize)(buf.as_mut_ptr() as *mut c_void, size) }
        };
        if !ok && reuse_mgba {
            // A newly required save-data/RTC section may increase size.
            // Never continue Run-Ahead with a truncated libretro state.
            let fresh = unsafe { (self.api.serialize_size)() };
            if fresh == 0 {
                return Err(CoreError::State("core reports no state".into()));
            }
            self.mgba_runahead_size_age = 0;
            buf.resize(fresh, 0);
            ok = {
                let _a = Active::bind(&mut self.host);
                unsafe { (self.api.serialize)(buf.as_mut_ptr() as *mut c_void, fresh) }
            };
        }
        if ok { Ok(()) } else { Err(CoreError::State("serialize refused".into())) }
    }

    fn unserialize(&mut self, data: &[u8]) -> Result<(), CoreError> {
        let ok = {
            let _a = Active::bind(&mut self.host);
            unsafe { (self.api.unserialize)(data.as_ptr() as *const c_void, data.len()) }
        };
        if ok {
            Ok(())
        } else {
            Err(CoreError::State("unserialize refused".into()))
        }
    }

    fn speculative_audio_supported(&self) -> bool {
        (!self.is_gpsp || self.gpsp_audio.is_some())
            && (!self.is_mgba || self.mgba_audio.is_some())
    }

    fn capture_speculative_audio(&mut self) -> Result<(), CoreError> {
        let state = if self.is_gpsp {
            self.gpsp_audio.as_mut()
        } else if self.is_mgba {
            self.mgba_audio.as_mut()
        } else {
            return Ok(());
        }.ok_or_else(|| {
            CoreError::Unsupported("core has no safe speculative audio snapshot".into())
        })?;
        state.captured = false;
        let ok = unsafe {
            (state.transfer)(state.snapshot.as_mut_ptr().cast(),
                state.snapshot.len(), false)
        };
        if !ok { return Err(CoreError::State("core audio capture failed".into())); }
        state.captured = true;
        Ok(())
    }

    fn restore_speculative_audio(&mut self) -> Result<(), CoreError> {
        let state = if self.is_gpsp {
            self.gpsp_audio.as_mut()
        } else if self.is_mgba {
            self.mgba_audio.as_mut()
        } else {
            return Ok(());
        }.ok_or_else(|| {
            CoreError::Unsupported("core has no safe speculative audio snapshot".into())
        })?;
        if !state.captured {
            return Err(CoreError::State("missing speculative audio snapshot".into()));
        }
        state.captured = false;
        let ok = unsafe {
            (state.transfer)(state.snapshot.as_mut_ptr().cast(),
                state.snapshot.len(), true)
        };
        if ok { Ok(()) } else { Err(CoreError::State("core audio restore failed".into())) }
    }

    fn save_ram(&self) -> Option<Vec<u8>> {
        let data = unsafe { (self.api.get_memory_data)(MEMORY_SAVE_RAM) };
        let len = unsafe { (self.api.get_memory_size)(MEMORY_SAVE_RAM) };
        if data.is_null() || len == 0 {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts(data as *const u8, len) }.to_vec())
    }

    fn load_save_ram(&mut self, data: &[u8]) -> Result<(), CoreError> {
        let dst = unsafe { (self.api.get_memory_data)(MEMORY_SAVE_RAM) };
        let len = unsafe { (self.api.get_memory_size)(MEMORY_SAVE_RAM) };
        if dst.is_null() || len == 0 {
            return Err(CoreError::Unsupported("core exposes no save ram".into()));
        }
        let n = len.min(data.len());
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), dst as *mut u8, n) };
        Ok(())
    }

    fn save_rtc(&self) -> Option<Vec<u8>> {
        let data = unsafe { (self.api.get_memory_data)(MEMORY_RTC) };
        let len = unsafe { (self.api.get_memory_size)(MEMORY_RTC) };
        if data.is_null() || len == 0 || len > 4096 { return None; }
        Some(unsafe { std::slice::from_raw_parts(data as *const u8, len) }.to_vec())
    }

    fn load_rtc(&mut self, data: &[u8]) -> Result<(), CoreError> {
        let dst = unsafe { (self.api.get_memory_data)(MEMORY_RTC) };
        let len = unsafe { (self.api.get_memory_size)(MEMORY_RTC) };
        if dst.is_null() || len == 0 {
            return Err(CoreError::Unsupported("core exposes no RTC".into()));
        }
        if len != data.len() || len > 4096 {
            return Err(CoreError::State(format!(
                "RTC length mismatch: core expects {len}, file has {}", data.len()
            )));
        }
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), dst as *mut u8, len) };
        Ok(())
    }

    fn av_info(&self) -> AvInfo {
        self.av
    }

    fn rumble(&self) -> Rumble {
        self.host.rumble.clone()
    }

    fn net(&self) -> Link {
        self.host.net.clone()
    }

    fn supports_netpacket(&self) -> bool {
        self.host.netpacket.as_ref().is_some_and(netpacket_ready)
    }

    fn start_link(&mut self, client_id: u16) {
        let _a = Active::bind(&mut self.host);
        let _ = unsafe { begin_link(client_id) };
    }

    fn try_start_link(&mut self, client_id: u16) -> bool {
        let _a = Active::bind(&mut self.host);
        unsafe { begin_link(client_id) }
    }

    fn pump_link(&mut self) {
        let _a = Active::bind(&mut self.host);
        unsafe { drain_link() };
    }

    fn stop_link(&mut self) {
        let _a = Active::bind(&mut self.host);
        unsafe { halt_link() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ffi::CStr;

    fn host_with(options: HashMap<String, CString>, options_dirty: bool) -> Box<Host> {
        Box::new(Host {
            video: vec![0; VIDEO_BYTES],
            format: PixelFormat::Xrgb8888,
            audio: Vec::new(),
            inputs: [0; 2],
            memory: Vec::new(),
            system_dir: CString::new(".").unwrap(),
            save_dir: CString::new(".").unwrap(),
            rumble: Rumble::default(),
            asked_for_rumble: false,
            netpacket: None,
            net: Link::default(),
            net_peer: None,
            audio_status: None,
            options,
            options_dirty,
            declared: HashMap::new(),
        })
    }

    fn declaration(key: &str, value: &str) -> (Variable, CString, CString) {
        let key = CString::new(key).unwrap();
        let value = CString::new(value).unwrap();
        let var = Variable {
            key: key.as_ptr(),
            value: value.as_ptr(),
        };
        (var, key, value)
    }

    fn end_of_list() -> Variable {
        Variable {
            key: ptr::null(),
            value: ptr::null(),
        }
    }

    fn test_ra_descriptor(memory: &mut [u8], start: usize, offset: usize, len: usize) -> MemoryDescriptor {
        MemoryDescriptor {
            flags: 0,
            ptr: memory.as_mut_ptr().cast(),
            offset,
            start,
            select: 0,
            disconnect: 0,
            len,
            addrspace: ptr::null(),
        }
    }

    #[test]
    fn gb_and_gbc_memory_descriptor_windows_respect_validity_and_extended_wram() {
        let mut wram = vec![0x15u8; 0x8000];
        let mut hram = vec![0x87u8; 0x80];
        let mut oam = vec![0x55u8; 0xa0];
        let mut regions = vec![
            test_ra_descriptor(&mut wram, 0xc000, 0, 0x1000),
            test_ra_descriptor(&mut wram, 0xd000, 0x1000, 0x1000),
            test_ra_descriptor(&mut wram, 0x10000, 0x2000, 0x6000),
            test_ra_descriptor(&mut hram, 0xff80, 0, 0x7f),
            test_ra_descriptor(&mut oam, 0xfe00, 0, 0xa0),
        ];
        regions[3].select = 0xFFFFFF80;
        let mut ram = vec![0xCC; 0x58000];
        let mut coverage = vec![0; 0x17000];
        assert!(copy_gb_achievement_map(&regions, &mut ram, &mut coverage) > 0);
        assert_eq!(ram[0xc000], 0x15);
        assert_eq!(ram[0x10000], 0x15);
        assert_eq!(ram[0xff80], 0x87);
        assert_eq!(coverage[0xc000], 1);
        assert_eq!(coverage[0xe000], 0, "echo RAM must never imply valid gameplay RAM");
        assert_eq!(coverage[0xff7f], 0);
        assert_eq!(coverage[0xff80], 1);
        assert_eq!(coverage[0x16fff], 0);
    }

    #[test]
    fn ra_gba_mgba_full_memory_regions_are_copied_in_ra_order() {
        let mut iwram = vec![0x11u8; 0x8000];
        let mut ewram = vec![0x22u8; 0x40000];
        let mut sram = vec![0x33u8; 0x10000];
        let mut map = vec![
            test_ra_descriptor(&mut iwram, 0x03000000, 0, 0x8000),
            test_ra_descriptor(&mut ewram, 0x02000000, 0, 0x40000),
            test_ra_descriptor(&mut sram, 0x0e000000, 0, 0x10000),
        ];
        map[0].select = 0xFF000000; // Real mGBA uses a region selection mask.
        map[1].select = 0xFF000000;
        let mut ram = vec![0xEE; 0x58000];
        assert_eq!(copy_gba_achievement_map(&map, &mut ram), [0x8000, 0x40000, 0x10000]);
        assert!(ram[..0x8000].iter().all(|b| *b == 0x11));
        assert!(ram[0x8000..0x48000].iter().all(|b| *b == 0x22));
        assert!(ram[0x48000..].iter().all(|b| *b == 0x33));
    }

    #[test]
    fn ra_gba_gpsp_split_ewram_uses_every_descriptor_and_its_offset() {
        let mut iwram = vec![0u8; 0x10000];
        iwram[0x8000..].fill(0x99);
        let mut ewram = vec![0u8; 0x80000];
        let mut map = vec![test_ra_descriptor(&mut iwram, 0x03000000, 0x8000, 0x8000)];
        // gpSP-style EWRAM: 8 descriptors, each 0x8000 bytes, with
        // backing offsets 0x8000, 0x18000, ..., 0x78000.
        for index in 0..8 {
            let offset = 0x8000 + index * 0x10000;
            ewram[offset..offset + 0x8000].fill(0x40 + index as u8);
            map.push(test_ra_descriptor(
                &mut ewram, 0x02000000 + index * 0x8000, offset, 0x8000,
            ));
        }
        let mut ram = vec![0xEE; 0x58000];
        let valid = copy_gba_achievement_map(&map, &mut ram);
        assert_eq!(valid, [0x8000, 0x40000, 0]);
        assert_eq!(ram[0], 0x99);
        for index in 0..8 {
            let begin = 0x8000 + index * 0x8000;
            assert!(ram[begin..begin + 0x8000].iter().all(|b| *b == 0x40 + index as u8));
        }
    }

    #[test]
    fn ra_gba_missing_page_stops_at_contiguous_prefix_and_rejects_bad_offsets() {
        let mut ewram = vec![0x37u8; 0x10000];
        let first = test_ra_descriptor(&mut ewram, 0x02000000, 0, 0x8000);
        let third = test_ra_descriptor(&mut ewram, 0x02010000, 0x8000, 0x8000);
        let mut ram = vec![0xEE; 0x58000];
        let valid = copy_gba_achievement_map(&[first, third], &mut ram);
        assert_eq!(valid, [0, 0x8000, 0]);
        assert_eq!(ram[0x8000 + 0x8000], 0xEE);
        let malformed = test_ra_descriptor(&mut ewram, 0x03000000, usize::MAX - 2, 0x8000);
        assert_eq!(copy_gba_achievement_map(&[malformed], &mut ram), [0, 0, 0]);
    }

    #[test]
    fn ra_memory_maps_are_copied_and_cleared_without_leaking_emulator_pointers() {
        let mut host = host_with(HashMap::new(), false);
        let mut iwram = vec![0x37u8; 0x8000];
        let desc = MemoryDescriptor {
            flags: 0,
            ptr: iwram.as_mut_ptr().cast(),
            offset: 0,
            start: 0x03000000,
            select: 0,
            disconnect: 0,
            len: iwram.len(),
            addrspace: ptr::null(),
        };
        let map = MemoryMap {
            descriptors: &desc,
            num_descriptors: 1,
        };
        {
            let _active = Active::bind(&mut host);
            assert!(unsafe { environment(SET_MEMORY_MAPS,
                (&map as *const MemoryMap).cast_mut().cast()) });
        }
        assert_eq!(host.memory.len(), 1);
        assert_eq!(host.memory[0].start, 0x03000000);
        assert_eq!(host.memory[0].len, 0x8000);
        // Core may replace its descriptors at runtime.
        let empty = MemoryMap {
            descriptors: ptr::null(),
            num_descriptors: 0,
        };
        {
            let _active = Active::bind(&mut host);
            assert!(unsafe { environment(SET_MEMORY_MAPS,
                (&empty as *const MemoryMap).cast_mut().cast()) });
        }
        assert!(host.memory.is_empty());
    }

    #[test]
    fn ra_memory_maps_reject_invalid_and_unbounded_descriptor_arrays() {
        let mut host = host_with(HashMap::new(), false);
        let malformed = MemoryMap {
            descriptors: ptr::null(),
            num_descriptors: 1,
        };
        let enormous = MemoryMap {
            descriptors: ptr::null(),
            num_descriptors: 1025,
        };
        {
            let _active = Active::bind(&mut host);
            assert!(!unsafe { environment(SET_MEMORY_MAPS,
                (&malformed as *const MemoryMap).cast_mut().cast()) });
            assert!(!unsafe { environment(SET_MEMORY_MAPS,
                (&enormous as *const MemoryMap).cast_mut().cast()) });
        }
        assert!(host.memory.is_empty());
    }

    #[test]
    fn set_variables_records_what_the_core_declared() {
        let mut host = host_with(HashMap::new(), false);
        let (a, _ak, _av) = declaration("mgba_sgb_borders", "Use Super Game Boy Borders; ON|OFF");
        let (b, _bk, _bv) =
            declaration("mgba_gb_colors_preset", "Game Boy Palette Preset; 0|1|2|3");
        let list = [a, b, end_of_list()];
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe { environment(SET_VARIABLES, list.as_ptr() as *mut c_void) }
        };

        assert!(ok);
        assert_eq!(
            host.declared.get("mgba_sgb_borders").map(Vec::as_slice),
            Some(["ON".to_string(), "OFF".to_string()].as_slice())
        );
        assert_eq!(
            host.declared
                .get("mgba_gb_colors_preset")
                .map(Vec::as_slice),
            Some(["0", "1", "2", "3"].map(str::to_string).as_slice())
        );
        assert_eq!(
            host.declared.len(),
            2,
            "the walk ran past the terminator and read whatever was after it"
        );
        assert!(
            !host.declared.contains_key("mgba_sgb_border"),
            "a key the core never declared came back declared"
        );
    }

    #[test]
    fn set_variables_with_no_list_declares_nothing() {
        let mut host = host_with(HashMap::new(), false);
        host.declared
            .insert("stale".to_string(), vec!["x".to_string()]);

        let ok = {
            let _active = Active::bind(&mut host);
            unsafe { environment(SET_VARIABLES, ptr::null_mut()) }
        };

        assert!(ok);
        assert!(host.declared.is_empty(), "a stale declaration survived");
    }

    #[test]
    fn get_variable_writes_the_options_pointer_for_the_core_to_read() {
        let mut options = HashMap::new();
        options.insert("gpsp_serial".to_string(), CString::new("rfu").unwrap());
        let mut host = host_with(options, false);
        let _active = Active::bind(&mut host);

        let key = CString::new("gpsp_serial").unwrap();
        let mut var = Variable {
            key: key.as_ptr(),
            value: ptr::null(),
        };
        let ok = unsafe { environment(GET_VARIABLE, &mut var as *mut Variable as *mut c_void) };

        assert!(ok);
        let value = unsafe { CStr::from_ptr(var.value) };
        assert_eq!(value.to_str().unwrap(), "rfu");
    }

    #[test]
    fn get_variable_reports_false_for_an_unknown_key() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);

        let key = CString::new("nope").unwrap();
        let mut var = Variable {
            key: key.as_ptr(),
            value: ptr::null(),
        };
        let ok = unsafe { environment(GET_VARIABLE, &mut var as *mut Variable as *mut c_void) };

        assert!(!ok);
        assert!(var.value.is_null());
    }

    #[test]
    fn get_variable_update_reports_and_clears_the_dirty_flag() {
        let mut host = host_with(HashMap::new(), true);
        let _active = Active::bind(&mut host);

        let mut dirty = false;
        let ok =
            unsafe { environment(GET_VARIABLE_UPDATE, &mut dirty as *mut bool as *mut c_void) };
        assert!(ok);
        assert!(dirty, "first call must report the pending change");

        let mut dirty_again = true;
        let ok = unsafe {
            environment(
                GET_VARIABLE_UPDATE,
                &mut dirty_again as *mut bool as *mut c_void,
            )
        };
        assert!(ok);
        assert!(!dirty_again, "flag must be cleared after being read once");
    }

    #[test]
    fn get_variable_refuses_a_null_key() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);

        let mut var = Variable {
            key: ptr::null(),
            value: ptr::null(),
        };
        let ok = unsafe { environment(GET_VARIABLE, &mut var as *mut Variable as *mut c_void) };
        assert!(!ok);
    }

    thread_local! {
        static TEST_AUDIO_STATUS: RefCell<Vec<(bool, c_uint, bool)>> =
            const { RefCell::new(Vec::new()) };
    }

    unsafe extern "C" fn test_audio_status(active: bool, occupancy: c_uint, underrun: bool) {
        TEST_AUDIO_STATUS.with(|r| r.borrow_mut().push((active, occupancy, underrun)));
    }

    #[test]
    fn set_audio_buffer_status_callback_stores_what_the_core_hands_over() {
        let mut host = host_with(HashMap::new(), false);
        let mut cb = AudioBufferStatusCallback {
            callback: Some(test_audio_status),
        };
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe {
                environment(
                    SET_AUDIO_BUFFER_STATUS_CALLBACK,
                    &mut cb as *mut AudioBufferStatusCallback as *mut c_void,
                )
            }
        };

        assert!(ok, "refusing this turns the cores' frameskip off entirely");
        assert!(host.audio_status.is_some(), "the callback was never stored");
    }

    #[test]
    fn set_audio_buffer_status_callback_null_withdraws_it() {
        let mut host = host_with(HashMap::new(), false);
        host.audio_status = Some(test_audio_status);
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe { environment(SET_AUDIO_BUFFER_STATUS_CALLBACK, ptr::null_mut()) }
        };

        assert!(ok, "withdrawing is a legal call and must be answered true");
        assert!(host.audio_status.is_none());
    }

    #[test]
    fn set_frame_skip_tells_the_core_to_skip_by_reporting_an_underrun() {
        TEST_AUDIO_STATUS.with(|r| r.borrow_mut().clear());
        let mut host = host_with(HashMap::new(), false);
        host.audio_status = Some(test_audio_status);
        {
            let _active = Active::bind(&mut host);
            unsafe {
                report_audio_status(true);
                report_audio_status(false);
            }
        }

        TEST_AUDIO_STATUS.with(|r| {
            assert_eq!(
                r.borrow().as_slice(),
                &[(true, 0, true), (true, 100, false)],
                "the skip and the draw did not read as an underrun and a healthy buffer"
            );
        });
    }

    #[test]
    fn set_frame_skip_is_a_noop_when_the_core_registered_no_callback() {
        TEST_AUDIO_STATUS.with(|r| r.borrow_mut().clear());
        let mut host = host_with(HashMap::new(), false);
        {
            let _active = Active::bind(&mut host);
            unsafe { report_audio_status(true) };
        }

        TEST_AUDIO_STATUS.with(|r| assert!(r.borrow().is_empty()));
    }

    thread_local! {
        static TEST_RECEIVED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
        static TEST_RECEIVED_CLIENT_IDS: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
        static TEST_POLLS: Cell<u32> = const { Cell::new(0) };
        static TEST_START_CLIENT: Cell<Option<u16>> = const { Cell::new(None) };
        static TEST_STOP_CALLS: Cell<u32> = const { Cell::new(0) };
        static TEST_CONNECTED_CLIENT: Cell<Option<u16>> = const { Cell::new(None) };
        static TEST_DISCONNECTED_CLIENT: Cell<Option<u16>> = const { Cell::new(None) };
    }

    fn reset_test_netpacket_recorders() {
        TEST_RECEIVED.with(|r| r.borrow_mut().clear());
        TEST_RECEIVED_CLIENT_IDS.with(|c| c.borrow_mut().clear());
        TEST_POLLS.with(|p| p.set(0));
        TEST_START_CLIENT.with(|c| c.set(None));
        TEST_STOP_CALLS.with(|c| c.set(0));
        TEST_CONNECTED_CLIENT.with(|c| c.set(None));
        TEST_DISCONNECTED_CLIENT.with(|c| c.set(None));
    }

    unsafe extern "C" fn test_receive(buf: *const c_void, len: usize, client_id: u16) {
        let bytes = std::slice::from_raw_parts(buf as *const u8, len).to_vec();
        TEST_RECEIVED.with(|r| r.borrow_mut().push(bytes));
        TEST_RECEIVED_CLIENT_IDS.with(|c| c.borrow_mut().push(client_id));
    }

    unsafe extern "C" fn test_receive_reentrant(buf: *const c_void, len: usize, client_id: u16) {
        test_receive(buf, len, client_id);
        let reentrant = b"reentrant";
        netpacket_send(0, reentrant.as_ptr() as *const c_void, reentrant.len(), 0);
    }

    unsafe extern "C" fn test_start_reentrant(
        client_id: u16,
        _send: NetpacketSend,
        poll_receive: NetpacketPollReceive,
    ) {
        TEST_START_CLIENT.with(|c| c.set(Some(client_id)));
        poll_receive();
    }

    unsafe extern "C" fn test_poll() {
        TEST_POLLS.with(|p| p.set(p.get() + 1));
    }

    unsafe extern "C" fn test_start(
        client_id: u16,
        _send: NetpacketSend,
        _poll_receive: NetpacketPollReceive,
    ) {
        TEST_START_CLIENT.with(|c| c.set(Some(client_id)));
    }

    unsafe extern "C" fn test_stop() {
        TEST_STOP_CALLS.with(|c| c.set(c.get() + 1));
    }

    unsafe extern "C" fn test_connected(client_id: u16) -> bool {
        TEST_CONNECTED_CLIENT.with(|c| c.set(Some(client_id)));
        true
    }

    unsafe extern "C" fn test_disconnected(client_id: u16) {
        TEST_DISCONNECTED_CLIENT.with(|c| c.set(Some(client_id)));
    }

    fn test_netpacket_callback() -> NetpacketCallback {
        NetpacketCallback {
            start: Some(test_start),
            receive: Some(test_receive),
            stop: None,
            poll: Some(test_poll),
            connected: None,
            disconnected: None,
            protocol_version: ptr::null(),
        }
    }

    unsafe extern "C" fn test_reject_link(_client_id: u16) -> bool { false }

    #[test]
    fn gp_sp_refusing_serial_peer_never_activates_fake_link() {
        let mut host = host_with(HashMap::new(), false);
        let mut cb = test_netpacket_callback();
        cb.connected = Some(test_reject_link);
        cb.stop = Some(test_stop);
        host.netpacket = Some(cb);
        let accepted = {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) }
        };
        assert!(!accepted, "Rejected serial peer cannot be marked connected");
        assert!(!host.net.is_active());
        assert!(host.net_peer.is_none());
    }

    #[test]
    fn gpsp_null_poll_is_valid_for_real_netpacket_mode() {
        // Pinned gpSP (5819380c) registers start/receive/stop and
        // deliberately sets poll=NULL. The emulator supplies its own
        // netpacket_poll_receive callback to the frontend during start().
        let mut callback = test_netpacket_callback();
        callback.stop = Some(test_stop);
        callback.poll = None;
        assert!(netpacket_ready(&callback));
        callback.start = None;
        assert!(!netpacket_ready(&callback));
        callback.start = Some(test_start);
        // A NULL callback is optional; this must never disable gpSP Wireless.
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(callback);
        assert!(host.netpacket.as_ref().is_some_and(netpacket_ready));
    }

    #[test]
    fn set_netpacket_interface_stores_the_callback_the_core_hands_over() {
        let mut host = host_with(HashMap::new(), false);
        let mut cb = test_netpacket_callback();
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe {
                environment(
                    SET_NETPACKET_INTERFACE,
                    &mut cb as *mut NetpacketCallback as *mut c_void,
                )
            }
        };

        assert!(ok);
        assert!(host.netpacket.is_some(), "the callback was never stored");
    }

    #[test]
    fn set_netpacket_interface_null_data_withdraws_the_interface() {
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        let ok = {
            let _active = Active::bind(&mut host);
            unsafe { environment(SET_NETPACKET_INTERFACE, ptr::null_mut()) }
        };

        assert!(ok, "withdrawing is a legal call and must be answered true");
        assert!(
            host.netpacket.is_none(),
            "a NULL data pointer must clear a previously registered callback"
        );
    }

    #[test]
    fn netpacket_send_pushes_the_cores_packet_onto_outbound() {
        let mut host = host_with(HashMap::new(), false);
        let packet = b"link cable byte";
        {
            let _active = Active::bind(&mut host);
            unsafe {
                netpacket_send(
                    NETPACKET_RELIABLE,
                    packet.as_ptr() as *const c_void,
                    packet.len(),
                    0,
                )
            };
        }

        assert_eq!(host.net.take_outbound().as_deref(), Some(&packet[..]));
        assert_eq!(host.net.take_outbound(), None, "only one packet was sent");
    }

    #[test]
    fn netpacket_send_ignores_a_null_or_empty_packet() {
        let mut host = host_with(HashMap::new(), false);
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_send(0, ptr::null(), 4, 0) };
            unsafe { netpacket_send(0, [1u8].as_ptr() as *const c_void, 0, 0) };
        }

        assert_eq!(
            host.net.take_outbound(),
            None,
            "a null buffer or zero length must not enqueue a phantom packet"
        );
    }

    #[test]
    fn netpacket_poll_receive_drains_inbound_into_the_cores_receive_in_order() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.set_active(true);
        host.net_peer = Some(1);
        host.net.push_inbound(b"first".to_vec());
        host.net.push_inbound(b"second".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        TEST_RECEIVED.with(|r| {
            assert_eq!(
                r.borrow().as_slice(),
                &[b"first".to_vec(), b"second".to_vec()],
                "order was not kept"
            );
        });
        assert_eq!(host.net.take_inbound(), None, "queue must be drained");
    }

    #[test]
    fn netpacket_poll_receive_does_nothing_without_a_receive_callback() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.net.set_active(true);
        host.net.push_inbound(b"stranded".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        TEST_RECEIVED.with(|r| assert!(r.borrow().is_empty()));
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"stranded"[..]),
            "with nowhere to hand the packet it must be left queued, not dropped"
        );
    }

    #[test]
    fn netpacket_poll_receive_does_nothing_once_the_session_is_no_longer_active() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.push_inbound(b"stale".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        TEST_RECEIVED.with(|r| assert!(r.borrow().is_empty(), "an ended session reached the core"));
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"stale"[..]),
            "the packet must be left queued, not delivered to a session that already ended"
        );
    }

    #[test]
    fn netpacket_poll_receive_does_nothing_without_a_recorded_peer() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.set_active(true);
        host.net.push_inbound(b"orphaned".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        TEST_RECEIVED.with(|r| {
            assert!(
                r.borrow().is_empty(),
                "a packet must not be delivered with no peer to tag it"
            )
        });
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"orphaned"[..]),
            "the packet must be left queued, not delivered mislabelled as our own id"
        );
    }

    #[test]
    fn netpacket_poll_receive_tags_packets_with_the_peers_client_id() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }
        reset_test_netpacket_recorders();
        host.net.push_inbound(b"from the peer".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        TEST_RECEIVED_CLIENT_IDS.with(|c| {
            assert_eq!(
                c.borrow().as_slice(),
                &[1],
                "packets must be tagged with the peer's id, not the host's own"
            );
        });
    }

    #[test]
    fn drain_link_hands_the_core_everything_waiting_then_polls() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.set_active(true);
        host.net_peer = Some(1);
        host.net.push_inbound(b"queued".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| assert_eq!(r.borrow().as_slice(), &[b"queued".to_vec()]));
        TEST_POLLS.with(|p| assert_eq!(p.get(), 1, "poll must run once a frame"));
    }

    #[test]
    fn drain_link_polls_even_with_nothing_inbound() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.set_active(true);
        host.net_peer = Some(1);
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_POLLS.with(|p| assert_eq!(p.get(), 1));
    }

    #[test]
    fn drain_link_skips_poll_when_the_core_offered_none() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            poll: None,
            ..test_netpacket_callback()
        });
        host.net.set_active(true);
        host.net_peer = Some(1);
        host.net.push_inbound(b"still delivered".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| {
            assert_eq!(r.borrow().as_slice(), &[b"still delivered".to_vec()]);
        });
        TEST_POLLS.with(|p| assert_eq!(p.get(), 0, "poll is optional and was not offered"));
    }

    #[test]
    fn drain_link_is_a_noop_when_the_core_never_registered_netpacket() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.net.set_active(true);
        host.net.push_inbound(b"nobody asked".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| assert!(r.borrow().is_empty()));
        TEST_POLLS.with(|p| assert_eq!(p.get(), 0));
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"nobody asked"[..]),
            "with no registered core the packet must be left queued, not lost"
        );
    }

    #[test]
    fn drain_link_does_nothing_once_the_session_is_no_longer_active() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.push_inbound(b"stale".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| assert!(r.borrow().is_empty(), "an ended session reached the core"));
        TEST_POLLS.with(|p| assert_eq!(p.get(), 0, "an ended session must not be polled either"));
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"stale"[..]),
            "the packet must be left queued, not delivered to a session that already ended"
        );
    }

    #[test]
    fn drain_link_does_nothing_without_a_recorded_peer() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        host.net.set_active(true);
        host.net.push_inbound(b"orphaned".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| {
            assert!(
                r.borrow().is_empty(),
                "a packet must not be delivered with no peer to tag it"
            )
        });
        TEST_POLLS.with(|p| {
            assert_eq!(
                p.get(),
                0,
                "must not even poll with no peer to tag a receive"
            )
        });
        assert_eq!(
            host.net.take_inbound().as_deref(),
            Some(&b"orphaned"[..]),
            "the packet must be left queued, not delivered mislabelled as our own id"
        );
    }

    #[test]
    fn drain_link_tags_packets_with_the_peers_client_id() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }
        reset_test_netpacket_recorders();
        host.net.push_inbound(b"from the peer".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED_CLIENT_IDS.with(|c| {
            assert_eq!(
                c.borrow().as_slice(),
                &[1],
                "packets must be tagged with the peer's id, not our own"
            );
        });
    }

    #[test]
    fn drain_link_survives_the_reentrancy_gpsp_documents() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            receive: Some(test_receive_reentrant),
            ..test_netpacket_callback()
        });
        host.net.set_active(true);
        host.net_peer = Some(1);
        host.net.push_inbound(b"queued".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { drain_link() };
        }

        TEST_RECEIVED.with(|r| assert_eq!(r.borrow().as_slice(), &[b"queued".to_vec()]));
        assert_eq!(
            host.net.take_outbound().as_deref(),
            Some(&b"reentrant"[..]),
            "the reentrant netpacket_send call must still reach outbound"
        );
    }

    #[test]
    fn netpacket_poll_receive_survives_the_reentrancy_gpsp_documents() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            receive: Some(test_receive_reentrant),
            ..test_netpacket_callback()
        });
        host.net.set_active(true);
        host.net_peer = Some(1);
        host.net.push_inbound(b"queued".to_vec());
        {
            let _active = Active::bind(&mut host);
            unsafe { netpacket_poll_receive() };
        }

        assert_eq!(host.net.take_outbound().as_deref(), Some(&b"reentrant"[..]));
    }

    #[test]
    fn begin_link_survives_a_core_that_reenters_from_start() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            start: Some(test_start_reentrant),
            ..test_netpacket_callback()
        });
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }

        TEST_START_CLIENT.with(|c| assert_eq!(c.get(), Some(0)));
        assert!(host.net.is_active());
    }

    #[test]
    fn begin_link_hands_the_core_its_client_id_and_marks_the_session_active() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());
        assert!(
            !host.net.is_active(),
            "a fresh link is not backing a session"
        );
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(1) };
        }

        TEST_START_CLIENT.with(|c| assert_eq!(c.get(), Some(1), "client id was not forwarded"));
        assert!(
            host.net.is_active(),
            "starting a session must mark the link active"
        );
    }

    #[test]
    fn begin_link_is_a_noop_when_the_core_never_registered_netpacket() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }

        TEST_START_CLIENT.with(|c| assert_eq!(c.get(), None, "nothing to start, nothing called"));
        assert!(!host.net.is_active());
    }

    #[test]
    fn begin_link_calls_connected_with_the_peers_client_id() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            connected: Some(test_connected),
            ..test_netpacket_callback()
        });
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }

        TEST_CONNECTED_CLIENT.with(|c| {
            assert_eq!(
                c.get(),
                Some(1),
                "connected was not called with the peer's id"
            )
        });
    }

    #[test]
    fn begin_link_is_fine_with_no_connected_callback() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());

        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(0) };
        }

        assert!(
            host.net.is_active(),
            "a missing connected callback must not stop the session from starting"
        );
    }

    #[test]
    fn halt_link_calls_the_cores_stop_when_it_registered_one() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            stop: Some(test_stop),
            ..test_netpacket_callback()
        });
        {
            let _active = Active::bind(&mut host);
            unsafe { halt_link() };
        }

        TEST_STOP_CALLS.with(|c| assert_eq!(c.get(), 1, "stop was never called"));
    }

    #[test]
    fn halt_link_is_a_noop_when_the_core_never_offered_a_stop() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(test_netpacket_callback());

        {
            let _active = Active::bind(&mut host);
            unsafe { halt_link() };
        }

        TEST_STOP_CALLS.with(|c| assert_eq!(c.get(), 0));
    }

    #[test]
    fn halt_link_calls_disconnected_with_the_peers_client_id() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            disconnected: Some(test_disconnected),
            ..test_netpacket_callback()
        });
        {
            let _active = Active::bind(&mut host);
            unsafe { begin_link(1) };
        }
        {
            let _active = Active::bind(&mut host);
            unsafe { halt_link() };
        }

        TEST_DISCONNECTED_CLIENT.with(|c| {
            assert_eq!(
                c.get(),
                Some(0),
                "disconnected was not called with the peer's id"
            )
        });
    }

    #[test]
    fn halt_link_does_not_call_disconnected_when_no_session_ever_started() {
        reset_test_netpacket_recorders();
        let mut host = host_with(HashMap::new(), false);
        host.netpacket = Some(NetpacketCallback {
            disconnected: Some(test_disconnected),
            ..test_netpacket_callback()
        });

        {
            let _active = Active::bind(&mut host);
            unsafe { halt_link() };
        }

        TEST_DISCONNECTED_CLIENT.with(|c| assert_eq!(c.get(), None, "nothing was ever connected"));
    }

    #[test]
    fn input_state_answers_each_port_from_its_own_mask() {
        let mut host = host_with(HashMap::new(), false);
        host.inputs = [ButtonMask::A, ButtonMask::B | ButtonMask::START];
        let _active = Active::bind(&mut host);

        let mask = |port| unsafe { input_state(port, DEVICE_JOYPAD, 0, JOYPAD_MASK) } as u16;
        assert_eq!(mask(0), ButtonMask::A);
        assert_eq!(mask(1), ButtonMask::B | ButtonMask::START);
    }

    #[test]
    fn input_state_answers_single_buttons_on_port_1() {
        let mut host = host_with(HashMap::new(), false);
        host.inputs = [0, ButtonMask::B];
        let _active = Active::bind(&mut host);

        let button = |id| unsafe { input_state(1, DEVICE_JOYPAD, 0, id) };
        assert_eq!(button(0), 1, "B is bit 0 of the libretro joypad");
        assert_eq!(button(8), 0, "A is not held on port 1");
    }

    #[test]
    fn input_state_answers_nothing_past_port_1() {
        let mut host = host_with(HashMap::new(), false);
        host.inputs = [u16::MAX, u16::MAX];
        let _active = Active::bind(&mut host);

        assert_eq!(unsafe { input_state(2, DEVICE_JOYPAD, 0, JOYPAD_MASK) }, 0);
    }

    #[test]
    fn a_small_picture_is_centred_in_the_buffer() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let frame = vec![0xffu8; 160 * 144 * 4];
        unsafe { video_refresh(frame.as_ptr() as *const c_void, 160, 144, 160 * 4) };

        let lit =
            |x: usize, y: usize| unsafe { with_host(|h| h.video[(y * 240 + x) * 4]) }.unwrap() != 0;
        assert!(lit(40, 8), "the top left of the picture is not at (40, 8)");
        assert!(lit(199, 151), "the bottom right is not at (199, 151)");
        assert!(!lit(39, 8), "the picture starts one column too early");
        assert!(!lit(40, 7), "the picture starts one row too early");
    }

    #[test]
    fn the_margin_is_cleared_when_the_picture_shrinks() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        unsafe {
            let full = vec![0xffu8; 240 * 160 * 4];
            video_refresh(full.as_ptr() as *const c_void, 240, 160, 240 * 4);
            let small = vec![0x11u8; 160 * 144 * 4];
            video_refresh(small.as_ptr() as *const c_void, 160, 144, 160 * 4);
        }
        let corner = unsafe { with_host(|h| h.video[0]) }.unwrap();
        assert_eq!(corner, 0, "the previous picture is still in the margin");
    }

    fn gradient(w: usize, h: usize) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                px[o] = x as u8;
                px[o + 1] = y as u8;
                px[o + 2] = (x ^ y) as u8;
            }
        }
        px
    }

    #[test]
    fn a_full_size_picture_is_unmoved() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let frame = gradient(GBA_W as usize, GBA_H as usize);
        unsafe {
            video_refresh(
                frame.as_ptr() as *const c_void,
                GBA_W,
                GBA_H,
                GBA_W as usize * 4,
            )
        };

        assert_eq!(unsafe { with_host(|h| h.video.clone()) }.unwrap(), frame);
    }

    #[test]
    fn get_can_dupe_tells_a_core_the_frontend_keeps_the_last_frame() {
        assert_eq!(GET_CAN_DUPE, 3);

        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);

        let mut can_dupe = false;
        let ok = unsafe { environment(GET_CAN_DUPE, &mut can_dupe as *mut bool as *mut c_void) };

        assert!(ok, "a core that asks has to be answered, not ignored");
        assert!(
            can_dupe,
            "the frontend does keep the last frame, so it must say so"
        );
    }

    #[test]
    fn get_can_dupe_refuses_a_null_pointer() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);

        assert!(!unsafe { environment(GET_CAN_DUPE, ptr::null_mut()) });
    }

    #[test]
    fn a_duplicate_frame_leaves_the_previous_picture_exactly_as_it_was() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let frame = gradient(GBA_W as usize, GBA_H as usize);
        unsafe {
            video_refresh(
                frame.as_ptr() as *const c_void,
                GBA_W,
                GBA_H,
                GBA_W as usize * 4,
            );
            video_refresh(ptr::null(), GBA_W, GBA_H, GBA_W as usize * 4);
        }

        assert_eq!(
            unsafe { with_host(|h| h.video.clone()) }.unwrap(),
            frame,
            "a duplicate frame changed the picture instead of keeping it"
        );
    }

    #[test]
    fn a_duplicate_frame_keeps_a_centred_game_boy_picture_where_it_is() {
        let mut host = host_with(HashMap::new(), false);
        let _active = Active::bind(&mut host);
        let frame = gradient(160, 144);
        unsafe { video_refresh(frame.as_ptr() as *const c_void, 160, 144, 160 * 4) };
        let drawn = unsafe { with_host(|h| h.video.clone()) }.unwrap();

        unsafe {
            video_refresh(ptr::null(), 160, 144, 160 * 4);
            video_refresh(ptr::null(), 160, 144, 160 * 4);
        }

        let kept = unsafe { with_host(|h| h.video.clone()) }.unwrap();
        assert_eq!(kept, drawn, "the picture did not survive two duplicates");
        let lit = (58 * GBA_W as usize + 140) * 4;
        assert_eq!(
            &kept[lit..lit + 3],
            &[100u8, 50, 100 ^ 50],
            "the picture was blanked or moved where a duplicate should have kept it"
        );
    }
}

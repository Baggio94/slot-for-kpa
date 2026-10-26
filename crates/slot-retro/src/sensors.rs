//! Device motion and light for cores, through the libretro sensor
//! interface. Android writes the readings; the core reads them on the
//! emulation thread. Units and axes follow RetroArch's Android driver:
//! acceleration in g, rotation rate in rad/s, X/Y turned to the way the
//! handheld is held.
use std::ffi::c_uint;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Mutex;

pub const ACCELEROMETER: u8 = 1;
pub const GYROSCOPE: u8 = 2;
pub const ILLUMINANCE: u8 = 4;

const GRAVITY: f32 = 9.80665;

// retro_sensor_action and the RETRO_SENSOR_* input ids.
const ACCELEROMETER_ENABLE: c_uint = 0;
const ACCELEROMETER_DISABLE: c_uint = 1;
const GYROSCOPE_ENABLE: c_uint = 2;
const GYROSCOPE_DISABLE: c_uint = 3;
const ILLUMINANCE_ENABLE: c_uint = 4;
const ILLUMINANCE_DISABLE: c_uint = 5;
const ACCELEROMETER_X: c_uint = 0;
const ACCELEROMETER_Z: c_uint = 2;
const GYROSCOPE_X: c_uint = 3;
const GYROSCOPE_Z: c_uint = 5;
const ILLUMINANCE_ID: c_uint = 6;

static AVAILABLE: AtomicU8 = AtomicU8::new(0);
// How many loaded cores want each sensor: the Run-Ahead and link
// instances enable and disable on their own.
static WANTED: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
// f32 bits: raw accelerometer xyz (m/s²), gyroscope xyz, illuminance.
static VALUES: [AtomicU32; 7] = [const { AtomicU32::new(0) }; 7];
// Screen rotation as RetroArch detects it (0..3, clockwise quarters).
static ROTATION: AtomicU8 = AtomicU8::new(0);
static GRAVITY_SAMPLES: Mutex<Calibration> = Mutex::new(Calibration::new());

struct Calibration {
    done: bool,
    x: f32,
    y: f32,
    count: u32,
}

impl Calibration {
    const fn new() -> Self {
        Self { done: false, x: 0.0, y: 0.0, count: 0 }
    }

    /// RetroArch's gravity check: average 30 samples and, once the
    /// handheld is tilted at least ~30° from flat, take the axis gravity
    /// pulls on as "down". A flat handheld starts over.
    fn sample(&mut self, x: f32, y: f32) -> Option<u8> {
        if self.done {
            return None;
        }
        self.x += x;
        self.y += y;
        self.count += 1;
        if self.count < 30 {
            return None;
        }
        let (x, y) = (self.x / 30.0, self.y / 30.0);
        *self = Self::new();
        if x.abs() <= 5.0 && y.abs() <= 5.0 {
            return None;
        }
        self.done = true;
        Some(if y.abs() >= x.abs() {
            if y > 0.0 { 0 } else { 2 }
        } else if x < 0.0 { 1 } else { 3 })
    }
}

fn bit_index(sensor: u8) -> usize {
    match sensor {
        ACCELEROMETER => 0,
        GYROSCOPE => 1,
        _ => 2,
    }
}

fn store(index: usize, value: f32) {
    VALUES[index].store(value.to_bits(), Ordering::Relaxed);
}

fn load(index: usize) -> f32 {
    f32::from_bits(VALUES[index].load(Ordering::Relaxed))
}

/// Sensors this device has, as a mask of ACCELEROMETER, GYROSCOPE and
/// ILLUMINANCE. A core is refused any sensor that is not listed.
pub fn set_available(mask: u8) {
    AVAILABLE.store(mask, Ordering::Release);
}

pub fn available() -> u8 {
    AVAILABLE.load(Ordering::Acquire)
}

/// Sensors a loaded core has switched on, for Android to listen to.
pub fn wanted() -> u8 {
    [ACCELEROMETER, GYROSCOPE, ILLUMINANCE]
        .into_iter()
        .filter(|&s| WANTED[bit_index(s)].load(Ordering::Acquire) > 0)
        .fold(0, |mask, s| mask | s)
}

pub fn push_accelerometer(x: f32, y: f32, z: f32) {
    store(0, x);
    store(1, y);
    store(2, z);
    let rotation = GRAVITY_SAMPLES.lock().unwrap_or_else(|e| e.into_inner()).sample(x, y);
    if let Some(rotation) = rotation {
        ROTATION.store(rotation, Ordering::Relaxed);
    }
}

pub fn push_gyroscope(x: f32, y: f32, z: f32) {
    store(3, x);
    store(4, y);
    store(5, z);
}

pub fn push_illuminance(lux: f32) {
    store(6, lux);
}

/// One core's switched-on sensors, released when the core unloads.
#[derive(Default)]
pub(crate) struct Enabled(u8);

impl Enabled {
    fn set(&mut self, sensor: u8, on: bool) -> bool {
        if on && available() & sensor == 0 {
            return false;
        }
        if on == (self.0 & sensor != 0) {
            return true;
        }
        let count = &WANTED[bit_index(sensor)];
        if on {
            self.0 |= sensor;
            if count.fetch_add(1, Ordering::AcqRel) == 0 {
                begin(sensor);
            }
        } else {
            self.0 &= !sensor;
            count.fetch_sub(1, Ordering::AcqRel);
        }
        true
    }

    pub(crate) fn action(&mut self, action: c_uint) -> bool {
        match action {
            ACCELEROMETER_ENABLE => self.set(ACCELEROMETER, true),
            ACCELEROMETER_DISABLE => self.set(ACCELEROMETER, false),
            GYROSCOPE_ENABLE => self.set(GYROSCOPE, true),
            GYROSCOPE_DISABLE => self.set(GYROSCOPE, false),
            ILLUMINANCE_ENABLE => self.set(ILLUMINANCE, true),
            ILLUMINANCE_DISABLE => self.set(ILLUMINANCE, false),
            _ => false,
        }
    }

    pub(crate) fn release(&mut self) {
        for sensor in [ACCELEROMETER, GYROSCOPE, ILLUMINANCE] {
            self.set(sensor, false);
        }
    }

    pub(crate) fn input(&self, id: c_uint) -> f32 {
        let sensor = match id {
            ACCELEROMETER_X..=ACCELEROMETER_Z => ACCELEROMETER,
            GYROSCOPE_X..=GYROSCOPE_Z => GYROSCOPE,
            ILLUMINANCE_ID => ILLUMINANCE,
            _ => return 0.0,
        };
        if self.0 & sensor == 0 {
            return 0.0;
        }
        read(id, ROTATION.load(Ordering::Relaxed))
    }
}

impl Drop for Enabled {
    fn drop(&mut self) {
        self.release();
    }
}

/// A sensor switched on from nothing starts from rest, and the
/// accelerometer looks for the screen's orientation again.
fn begin(sensor: u8) {
    match sensor {
        ACCELEROMETER => {
            (0..3).for_each(|i| store(i, 0.0));
            *GRAVITY_SAMPLES.lock().unwrap_or_else(|e| e.into_inner()) = Calibration::new();
            ROTATION.store(0, Ordering::Relaxed);
        }
        GYROSCOPE => (3..6).for_each(|i| store(i, 0.0)),
        _ => store(6, 0.0),
    }
}

/// RetroArch's input_get_sensor_state on Android: X and Y follow the
/// detected rotation (90°: X->Y, Y->-X), Z and light are untouched.
fn read(id: c_uint, rotation: u8) -> f32 {
    let (base, scale) = match id {
        ACCELEROMETER_X..=ACCELEROMETER_Z => (0, 1.0 / GRAVITY),
        GYROSCOPE_X..=GYROSCOPE_Z => (3, 1.0),
        _ => return load(6),
    };
    let axis = (id - if base == 0 { ACCELEROMETER_X } else { GYROSCOPE_X }) as usize;
    let (axis, sign) = match (axis, rotation) {
        (2, _) | (_, 0) => (axis, 1.0),
        (0, 1) => (1, 1.0),
        (1, 1) => (0, -1.0),
        (_, 2) => (axis, -1.0),
        (0, _) => (1, -1.0),
        _ => (0, 1.0),
    };
    load(base + axis) * scale * sign
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_picks_the_axis_gravity_pulls_on() {
        let detect = |x: f32, y: f32| {
            let mut c = Calibration::new();
            (0..30).find_map(|_| c.sample(x, y))
        };
        assert_eq!(detect(0.0, 8.0), Some(0));
        assert_eq!(detect(0.0, -8.0), Some(2));
        assert_eq!(detect(-8.0, 1.0), Some(1));
        assert_eq!(detect(8.0, 1.0), Some(3));
        assert_eq!(detect(1.0, 2.0), None, "a flat handheld is not calibrated");
    }

    #[test]
    fn calibration_happens_once() {
        let mut c = Calibration::new();
        assert_eq!((0..30).find_map(|_| c.sample(0.0, 9.0)), Some(0));
        assert_eq!((0..30).find_map(|_| c.sample(-9.0, 0.0)), None);
    }

    #[test]
    fn readings_follow_retroarch_rotation() {
        push_gyroscope(1.0, 2.0, 3.0);
        let gyro = |rotation| [3, 4, 5].map(|id| read(id, rotation));
        assert_eq!(gyro(0), [1.0, 2.0, 3.0]);
        assert_eq!(gyro(1), [2.0, -1.0, 3.0]);
        assert_eq!(gyro(2), [-1.0, -2.0, 3.0]);
        assert_eq!(gyro(3), [-2.0, 1.0, 3.0]);
    }

    #[test]
    fn a_core_gets_only_sensors_the_device_has_and_lets_them_go() {
        set_available(ACCELEROMETER);
        let mut core = Enabled::default();
        assert!(!core.action(GYROSCOPE_ENABLE));
        assert!(core.action(ACCELEROMETER_ENABLE));
        let mut other = Enabled::default();
        assert!(other.action(ACCELEROMETER_ENABLE));
        assert_eq!(wanted(), ACCELEROMETER);
        drop(other);
        assert_eq!(wanted(), ACCELEROMETER, "still wanted by the first core");
        push_accelerometer(0.0, GRAVITY, 0.0);
        assert!((core.input(1) - 1.0).abs() < 1e-6);
        assert_eq!(core.input(3), 0.0, "gyroscope was refused");
        core.release();
        assert_eq!(wanted(), 0);
        assert_eq!(core.input(1), 0.0);
    }
}

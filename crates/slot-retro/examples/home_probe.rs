//! Boot a home console core the way slot. does (frames at their own size)
//! and report what the frontend relies on: frame size, aspect, timing,
//! battery, save states and determinism after a state reload.
//!
//! usage: home_probe CORE ROM [--frames N] [--ppm OUT] [--second COPY]
use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use slot_retro::{ButtonMask, LibretroCore, RetroCore};

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn save_ppm(path: &PathBuf, video: &[u8], (w, h): (u32, u32)) -> Result<(), Box<dyn Error>> {
    let mut out = format!("P6 {w} {h} 255\n").into_bytes();
    for px in video.chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    std::fs::write(path, out)?;
    Ok(())
}

fn open(core: &PathBuf, rom: &PathBuf, dir: &PathBuf) -> Result<LibretroCore, Box<dyn Error>> {
    let mut c = LibretroCore::open_with(core, dir, dir)?;
    c.set_native_video(true);
    c.load(rom)?;
    Ok(c)
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let core = PathBuf::from(args.next().ok_or("CORE")?);
    let rom = PathBuf::from(args.next().ok_or("ROM")?);
    let (mut frames, mut png_out, mut second) = (300u32, None, None);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--frames" => frames = args.next().ok_or("--frames N")?.parse()?,
            "--ppm" => png_out = Some(PathBuf::from(args.next().ok_or("--ppm OUT")?)),
            "--second" => second = Some(PathBuf::from(args.next().ok_or("--second COPY")?)),
            _ => return Err(format!("unknown {flag}").into()),
        }
    }
    let dir = std::env::temp_dir().join("home_probe");
    std::fs::create_dir_all(&dir)?;
    let mut c = open(&core, &rom, &dir)?;
    let av = c.av_info();
    println!("fps {:.4} rate {:.1} aspect {:.4}", av.fps, av.sample_rate, c.display_aspect());
    let mut samples = 0usize;
    let pad = ButtonMask(ButtonMask::START);
    for i in 0..frames {
        c.run_frame(if i % 60 < 5 { pad } else { ButtonMask::default() });
        samples += c.take_audio().len() / 2;
    }
    println!("frame {:?} aspect {:.4} audio {:.1} frames/video-frame", c.video_size(), c.display_aspect(),
        samples as f64 / f64::from(frames));
    println!("battery {:?}", c.save_ram().map(|r| r.len()));
    let console = match rom.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "nes" => 7, "sfc" | "smc" => 3, _ => 1,
    };
    let (mut ram, mut cover) = (vec![0u8; 0x58000], vec![0u8; 0x40000]);
    let valid = c.achievement_memory_for_console(&mut ram, &mut cover, console);
    let mapped: Vec<(usize, usize)> = {
        let mut spans = Vec::new();
        let mut i = 0;
        while i < cover.len() {
            if cover[i] == 1 {
                let s = i;
                while i < cover.len() && cover[i] == 1 { i += 1; }
                spans.push((s, i));
            } else { i += 1; }
        }
        spans
    };
    println!("achievements console {console}: valid {valid:?} spans {:x?} ram[0..4] {:02x?}",
        mapped, &ram[..4]);
    let state = c.serialize()?;
    let mut hashes = Vec::new();
    for _ in 0..2 {
        c.unserialize(&state)?;
        for _ in 0..30 { c.run_frame(ButtonMask::default()); c.take_audio(); }
        hashes.push(fnv1a(c.video_xrgb8888()));
    }
    println!("state {} bytes, replay deterministic: {}", state.len(), hashes[0] == hashes[1]);
    if let Some(path) = &png_out {
        save_ppm(path, c.video_xrgb8888(), c.video_size())?;
        println!("ppm {}", path.display());
    }
    if let Some(copy) = second {
        std::fs::copy(&core, &copy)?;
        let mut p = LibretroCore::open_secondary(&copy, &dir, &dir, &[])?;
        println!("second instance distinct: {}", p.instance_id() != c.instance_id());
        p.set_native_video(true);
        p.load(&rom)?;
        let s = c.serialize()?;
        p.unserialize(&s)?;
        let mut same = Vec::new();
        for _ in 0..3 {
            c.run_frame(ButtonMask::default());
            p.run_speculative_frame(ButtonMask::default());
            same.push(fnv1a(c.video_xrgb8888()) == fnv1a(p.video_xrgb8888()));
        }
        println!("second instance predicts the same frames: {same:?}");
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

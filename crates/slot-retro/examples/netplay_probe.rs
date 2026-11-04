//! Play a home console the way Multiplayer does: Join loads Host's state,
//! then both run the same frames with player 1 and player 2 buttons. One
//! core runs at a time here, so the same instance plays Host, then wanders
//! off in its own game like Join, loads Host's state and replays the frames:
//! every frame must come out identical, and player 2 must reach the game.
//!
//! usage: netplay_probe CORE ROM [--frames N]
use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use slot_retro::{ButtonMask, LibretroCore, RetroCore};

fn open(core: &PathBuf, rom: &PathBuf, dir: &PathBuf) -> Result<LibretroCore, Box<dyn Error>> {
    let mut c = LibretroCore::open_with(core, dir, dir)?;
    c.set_native_video(true);
    c.load(rom)?;
    Ok(c)
}

fn pad(seed: &mut u32) -> ButtonMask {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    ButtonMask((*seed & 0x0ff3) as u16)
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let core = PathBuf::from(args.next().ok_or("CORE")?);
    let rom = PathBuf::from(args.next().ok_or("ROM")?);
    let mut frames = 600u32;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--frames" => frames = args.next().ok_or("--frames N")?.parse()?,
            _ => return Err(format!("unknown {flag}").into()),
        }
    }
    let dir = std::env::temp_dir().join("netplay_probe");
    std::fs::create_dir_all(&dir)?;
    let mut c = open(&core, &rom, &dir)?;
    let (mut a, mut b) = (0x1234_5678u32, 0x9abc_def1u32);
    for _ in 0..120 { c.run_frame(pad(&mut a)); }
    let start = c.serialize()?;
    let inputs: Vec<_> = (0..frames).map(|_| (pad(&mut a), pad(&mut b))).collect();
    // Host: loads the state it sends, so both consoles start from a load
    // (Genesis Plus GX resets its pads on a load; the pads aren't in a state).
    c.unserialize(&start)?;
    let mut host = Vec::new();
    for &(p1, p2) in &inputs {
        c.run_frame_linked(p1, p2);
        let _ = c.take_audio();
        host.push((c.serialize()?, c.video_xrgb8888().to_vec()));
    }
    // Join: somewhere else in its own game, then Host's state.
    for _ in 0..47 { c.run_frame(pad(&mut b)); }
    c.unserialize(&start)?;
    for (frame, (&(p1, p2), (state, video))) in inputs.iter().zip(&host).enumerate() {
        c.run_frame_linked(p1, p2);
        let _ = c.take_audio();
        if &c.serialize()? != state {
            return Err(format!("consoles desynchronised at frame {frame}").into());
        }
        if c.video_xrgb8888() != &video[..] {
            return Err(format!("pictures differ at frame {frame}").into());
        }
    }
    // Same frames without player 2.
    c.unserialize(&start)?;
    let mut diverged = false;
    for (&(p1, _), (state, _)) in inputs.iter().zip(&host) {
        c.run_frame_linked(p1, ButtonMask::default());
        let _ = c.take_audio();
        diverged |= &c.serialize()? != state;
    }
    if !diverged { return Err("player 2's buttons never reached the game".into()); }
    println!("ok: {frames} linked frames identical, player 2 reaches the game");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("netplay_probe: {error}");
            ExitCode::FAILURE
        }
    }
}

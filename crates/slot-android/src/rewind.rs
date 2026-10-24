use std::collections::VecDeque;

pub const REWIND_BYTES: usize = 20 * 1024 * 1024;

pub struct Rewind {
    budget: usize,
    cur: Option<Vec<u8>>,
    deltas: VecDeque<Vec<u8>>,
    bytes: usize,
    scratch: Vec<u8>,
}

impl Rewind {
    pub fn new(budget_bytes: usize) -> Self {
        Rewind {
            budget: budget_bytes,
            cur: None,
            deltas: VecDeque::new(),
            bytes: 0,
            scratch: Vec::new(),
        }
    }

    pub fn push(&mut self, state: &[u8]) {
        let Some(prev) = self.cur.replace(state.to_vec()) else {
            return;
        };
        if prev.len() != state.len() {
            self.deltas.clear();
            self.bytes = 0;
            return;
        }
        self.scratch.clear();
        self.scratch
            .extend(prev.iter().zip(state).map(|(a, b)| a ^ b));
        let entry = lz4_flex::compress(&self.scratch);
        self.bytes += entry.len();
        self.deltas.push_back(entry);
        while self.bytes > self.budget {
            match self.deltas.pop_front() {
                Some(oldest) => self.bytes -= oldest.len(),
                None => break,
            }
        }
    }

    pub fn pop(&mut self) -> Option<Vec<u8>> {
        let cur = self.cur.take()?;
        let Some(entry) = self.deltas.pop_back() else {
            return Some(cur);
        };
        self.bytes -= entry.len();
        let mut prev = vec![0u8; cur.len()];
        match lz4_flex::decompress_into(&entry, &mut prev) {
            Ok(_) => {
                for (p, c) in prev.iter_mut().zip(&cur) {
                    *p ^= c;
                }
                self.cur = Some(prev);
            }
            Err(e) => {
                eprintln!("slot: rewind: {e}");
                self.deltas.clear();
                self.bytes = 0;
            }
        }
        Some(cur)
    }

    #[cfg(test)]
    pub fn bytes_used(&self) -> usize {
        self.bytes
    }

    #[cfg(test)]
    pub fn depth(&self) -> usize {
        self.deltas.len() + usize::from(self.cur.is_some())
    }

    pub fn fill(&self) -> u8 {
        if self.budget == 0 {
            return 0;
        }
        (self.bytes * 100 / self.budget).min(100) as u8
    }
}

pub struct RewindThread {
    tx: std::sync::mpsc::SyncSender<Msg>,
    fill: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

enum Msg {
    Push(Vec<u8>),
    Pop(std::sync::mpsc::SyncSender<Option<Vec<u8>>>),
}

impl RewindThread {
    pub fn spawn(budget_bytes: usize) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(4);
        let fill = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let published = fill.clone();
        std::thread::Builder::new()
            .name("slot-rewind".into())
            .spawn(move || {
                let mut rewind = Rewind::new(budget_bytes);
                while let Ok(msg) = rx.recv() {
                    match msg {
                        Msg::Push(state) => {
                            rewind.push(&state);
                            published.store(rewind.fill(), std::sync::atomic::Ordering::Relaxed);
                        }
                        Msg::Pop(reply) => {
                            let out = rewind.pop();
                            published.store(rewind.fill(), std::sync::atomic::Ordering::Relaxed);
                            let _ = reply.send(out);
                        }
                    }
                }
            })
            .expect("rewind thread");
        RewindThread { tx, fill }
    }

    pub fn push(&self, state: Vec<u8>) {
        // Compression lives on a worker but the old bounded send could block
        // the GL/libretro thread once four snapshots were queued. A rewind
        // snapshot is optional history, never save data: skip an overloaded
        // capture rather than freeze gameplay/audio waiting for compression.
        // Pop remains ordered/synchronous so the player only sees completed
        // and fully restored historical states.
        let _ = self.tx.try_send(Msg::Push(state));
    }

    pub fn pop(&self) -> Option<Vec<u8>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(0);
        if self.tx.send(Msg::Pop(tx)).is_err() {
            return None;
        }
        rx.recv().ok().flatten()
    }

    pub fn fill(&self) -> u8 {
        self.fill.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(n: u32) -> Vec<u8> {
        // Real save states contain slowly changing areas plus bursts of
        // entropy; deliberately fill the very small test history budget.
        let mut out=vec![0u8;8192];
        let mut seed=n.wrapping_mul(747796405).wrapping_add(2891336453);
        for b in out[1000..5000].iter_mut() {
            seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *b=(seed>>24) as u8;
        }
        out[..4].copy_from_slice(&n.to_le_bytes());
        out
    }

    #[test]
    fn a_full_rewind_meter_evicts_history_without_ending_the_session() {
        // The displayed 100% is compressed ring usage, NOT a signal to
        // eject, unload the core, exit slot., or reset the ROM.
        let mut rewind=Rewind::new(32*1024);
        for i in 0..250 {rewind.push(&state(i));}
        assert!(rewind.fill()>=90);
        assert!(rewind.bytes_used()<=32*1024);
        let mut kept=0usize;
        while let Some(frame)=rewind.pop() {
            if kept==0 { assert_eq!(frame,state(249)); }
            kept+=1;
        }
        assert!(kept>1 && kept<250, "compressed budget must evict old entries");
        assert_eq!(rewind.fill(),0);
        assert_eq!(rewind.depth(),0);
        // The game can resume capturing after exhausting its rewind history.
        rewind.push(&state(300));
        rewind.push(&state(301));
        assert_eq!(rewind.pop(),Some(state(301)));
        assert_eq!(rewind.pop(),Some(state(300)));
        assert_eq!(rewind.pop(),None);
    }

    #[test]
    fn changing_rewind_state_size_drops_only_incompatible_history() {
        let mut rewind=Rewind::new(REWIND_BYTES);
        rewind.push(&state(1));
        rewind.push(&state(2));
        let different=vec![7u8;4500];
        rewind.push(&different);
        assert_eq!(rewind.pop(),Some(different));
        assert_eq!(rewind.pop(),None);
    }
}

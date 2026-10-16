//! Cartridge PNG/embossing raster work, moved off the Android GL thread.
//! Like upstream Slot's CartFaces, CPU raster happens in bounded workers,
//! while texture creation and uploads remain on the GLES owner thread.
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use crate::cart_disk_cache::DiskCache;
use std::sync::{mpsc::{self, Receiver, TryRecvError}, Arc, Condvar, Mutex};
use std::thread;

use slot_store::Cart;
use slot_ui::{cart_face_with_material, cart_face, cart_interior, cart_backing,
    gb_shell_of, is_clear, CartFace};

type Key = (usize, usize);
type WorkKey = (Key, u64);
type Job = (WorkKey, Cart, bool);
/// Three source-faithful clear-cart textures for a *selected* cartridge:
/// original back, PCB, translucent front/label. Side carts keep one face.
pub(crate) type Layers = [CartFace; 3];
type ResultFace = (WorkKey, CartFace, Option<Layers>);

#[derive(Default)]
struct WorkQueue {
    jobs: VecDeque<Job>,
    closing: bool,
}

pub(crate) struct CartRender {
    // A short-held priority queue lets rapid D-pad / L1-R1 changes discard
    // outdated *queued* artwork rather than processing obsolete FIFO jobs.
    work: Arc<(Mutex<WorkQueue>, Condvar)>,
    finished: Receiver<ResultFace>,
    pending: HashSet<WorkKey>,
    wanted: Vec<Key>,
    // Recents/Favorites can reuse the same index for a different ROM.
    epochs: [u64; 5],
}

impl CartRender {
    pub(crate) fn start(cache_root: Option<&Path>) -> Result<Self, String> {
        let (ready, finished) = mpsc::channel::<ResultFace>();
        let work = Arc::new((Mutex::new(WorkQueue::default()), Condvar::new()));
        // Disk writes and LZ4 compression never block the GL owner or its two
        // raster workers. A bounded channel drops only optional cache writes.
        let disk_cache = cache_root.map(|root|Arc::new(DiskCache::new(root)));
        let writer = disk_cache.as_ref().and_then(|cache| {
            let (tx,rx)=mpsc::sync_channel::<(String,CartFace)>(8);
            let cache=Arc::clone(cache);
            thread::Builder::new().name("slot-kpa-cart-disk".into())
                .spawn(move || {
                    cache.trim();
                    let mut written=0usize;
                    while let Ok((key,face))=rx.recv() {
                        if cache.write(&key,&face).is_ok() {
                            written+=1;
                            if written%16==0 {cache.trim();}
                        }
                    }
                }).ok().map(|_|tx)
        });
        for index in 0..2 {
            let source = Arc::clone(&work);
            let sink = ready.clone();
            let disk = disk_cache.clone();
            let writer = writer.clone();
            thread::Builder::new()
                .name(format!("slot-kpa-cart-{index}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let (lock, notify) = &*source;
                            let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
                            while queue.jobs.is_empty() && !queue.closing {
                                queue = notify.wait(queue)
                                    .unwrap_or_else(|e| e.into_inner());
                            }
                            if queue.closing { None } else { queue.jobs.pop_front() }
                        };
                        let Some((key, cart, include_material)) = job else { break };
                        // No PNG decoding occurs on the GLES thread. At most
                        // two previously started jobs can become obsolete.
                        // An unchanged cartridge can be restored from LZ4 on
                        // either raster worker, avoiding PNG/emboss/upscale on
                        // subsequent launches; never access disk on the GL thread.
                        let disk_key=disk.as_ref().map(|cache|cache.key(&cart));
                        let cached=match (disk.as_ref(),disk_key.as_deref()) {
                            (Some(cache),Some(key)) => cache.read(key),
                            _ => None,
                        };
                        let needs_write=cached.is_none();
                        let pixels=cached.unwrap_or_else(|| {
                            let original=cart_face_with_material(&cart,None);
                            let (rgba,w,h)=slot_ui::native_rgba(
                                &original.rgba,original.w,original.h);
                            CartFace {rgba,w,h}
                        });
                        let layers = if include_material && is_clear(&cart) {
                            let original = [
                                cart_interior(&cart),
                                cart_backing(gb_shell_of(&cart)),
                                cart_face(&cart),
                            ];
                            Some(original.map(|face| {
                                let (rgba,w,h)=slot_ui::native_rgba(&face.rgba,face.w,face.h);
                                CartFace{rgba,w,h}
                            }))
                        } else {None};
                        // Clone only on cache miss. Hand the face to the GL
                        // uploader before best-effort persistence is queued.
                        let save=if needs_write {
                            writer.as_ref().zip(disk_key).map(|(writer,name)| {
                                (writer.clone(),name,CartFace {
                                    rgba:pixels.rgba.clone(),w:pixels.w,h:pixels.h
                                })
                            })
                        } else {None};
                        if sink.send((key,pixels,layers)).is_err() { break }
                        if let Some((writer,name,face))=save {
                            let _=writer.try_send((name,face));
                        }
                    }
                })
                .map_err(|e| format!("Cannot start cartridge renderer: {e}"))?;
        }
        Ok(Self {
            work, finished, pending: HashSet::new(), wanted: Vec::new(), epochs: [0; 5],
        })
    }

    /// The shelf may reduce its display rate only after every requested face
    /// was consumed on the GL thread. Workers never depend on a running GL loop.
    pub(crate) fn is_idle(&self) -> bool { self.pending.is_empty() }

    pub(crate) fn queued(&self, key: Key) -> bool {
        self.epochs.get(key.0).is_some_and(|epoch| self.pending.contains(&(key, *epoch)))
    }

    pub(crate) fn invalidate_shelf(&mut self, shelf: usize) {
        if let Some(epoch) = self.epochs.get_mut(shelf) {
            *epoch = epoch.wrapping_add(1);
            self.pending.retain(|(key, _)| key.0 != shelf);
            let (lock, _) = &*self.work;
            let mut q=lock.lock().unwrap_or_else(|e|e.into_inner());
            q.jobs.retain(|((key,_),_,_)| key.0 != shelf);
        }
    }

    /// Called every GL frame with the new navigation priorities.
    /// Only queued (not already decoding) artwork may be cancelled. This
    /// keeps the CPU/memory cap fixed while ensuring a freshly selected cart
    /// is ahead of any old background prefetch.
    pub(crate) fn prioritize(&mut self, desired: &[Key]) {
        self.wanted.clear();
        self.wanted.extend_from_slice(desired);
        let (lock, _) = &*self.work;
        let mut queue=lock.lock().unwrap_or_else(|e|e.into_inner());
        let mut dropped=Vec::new();
        queue.jobs.retain(|(work,_,_)| {
            let keep=desired.contains(&work.0);
            if !keep { dropped.push(*work); }
            keep
        });
        for work in dropped { self.pending.remove(&work); }
        // In-flight obsolete results can no longer clog GPU uploads after a
        // rapid system switch. The workers may finish, but ready() discards it.
        self.pending.retain(|(key,_)|desired.contains(key));
        queue.jobs.make_contiguous().sort_by_key(|job| {
            desired.iter().position(|key|*key==job.0.0).unwrap_or(usize::MAX)
        });
    }

    pub(crate) fn request(&mut self, key: Key, cart: &Cart, include_material: bool) {
        let Some(epoch) = self.epochs.get(key.0).copied() else { return };
        let work = (key, epoch);
        if self.pending.len() >= 16 || self.pending.contains(&work) { return }
        let (lock, notify) = &*self.work;
        let mut queue=lock.lock().unwrap_or_else(|e|e.into_inner());
        if queue.closing || queue.jobs.len() >= 16 { return }
        let rank=self.wanted.iter().position(|item|*item==key).unwrap_or(usize::MAX);
        let at=queue.jobs.iter().position(|item| {
            self.wanted.iter().position(|k|*k==item.0.0)
                .unwrap_or(usize::MAX)>rank
        }).unwrap_or(queue.jobs.len());
        queue.jobs.insert(at,(work,cart.clone(),include_material));
        self.pending.insert(work);
        notify.notify_one();
    }

    pub(crate) fn ready(&mut self) -> Option<(Key, CartFace, Option<Layers>)> {
        loop {
            match self.finished.try_recv() {
                Ok(((key, epoch), face, layers))
                    if self.pending.remove(&(key, epoch)) &&
                        self.epochs.get(key.0).copied() == Some(epoch) =>
                    return Some((key, face, layers)),
                Ok(_) => continue,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return None,
            }
        }
    }
}

impl Drop for CartRender {
    fn drop(&mut self) {
        let (lock, notify)=&*self.work;
        let mut queue=lock.lock().unwrap_or_else(|e|e.into_inner());
        queue.closing=true;
        queue.jobs.clear();
        notify.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_is_a_real_render_rack_and_discards_stale_results_on_reorder() {
        let work=Arc::new((Mutex::new(WorkQueue::default()),Condvar::new()));
        let (completed, finished) = mpsc::channel();
        let mut renderer = CartRender {
            work:work.clone(), finished, pending: HashSet::new(), wanted: Vec::new(), epochs: [0; 5],
        };
        // A fresh fifth rack must schedule work, rather than leaving
        // fallback label-colour rectangles visible forever.
        let favorite = slot_store::Cart {
            platform: slot_store::Platform::Gba,
            stem: "Tetris".into(),
            title: String::new(), code: String::new(),
            rom: std::path::PathBuf::from("content://roms/tetris.gba"),
            label: None, shell: None,
        };
        renderer.request((4, 0), &favorite, false);
        assert!(renderer.queued((4, 0)));
        let (((key, epoch), _, _)) = work.0.lock().unwrap().jobs.pop_front().expect("FAVORITES job");
        assert_eq!(key, (4, 0));
        assert_eq!(epoch, 0);
        renderer.invalidate_shelf(4);
        assert!(!renderer.queued((4, 0)));
        renderer.request((4, 0), &favorite, false);
        assert!(renderer.queued((4, 0)));
        let (((new_key, new_epoch), _, _)) = work.0.lock().unwrap().jobs.pop_front().expect("new FAVORITES job");
        assert_eq!((new_key,new_epoch), ((4,0),1));
        let face = || CartFace { rgba: vec![255; 4], w: 1, h: 1 };
        completed.send((((4,0),0),face(),None)).unwrap();
        completed.send((((4,0),1),face(),None)).unwrap();
        assert_eq!(renderer.ready().map(|result|result.0),Some((4,0)));
        assert!(renderer.ready().is_none());
        assert!(!renderer.queued((4,0)));
    }

    #[test]
    fn fast_system_switch_cancels_old_prefetch_and_promotes_active_cart() {
        let work=Arc::new((Mutex::new(WorkQueue::default()),Condvar::new()));
        let (_complete,finished)=mpsc::channel::<ResultFace>();
        let mut renderer=CartRender {
            work:work.clone(), finished, pending:HashSet::new(),
            wanted:Vec::new(),epochs:[0;5],
        };
        let cart=slot_store::Cart {
            platform:slot_store::Platform::Gba, stem:"Tetris".into(),
            title:String::new(),code:String::new(),
            rom:std::path::PathBuf::from("content://roms/tetris.gba"),
            label:None,shell:None,
        };
        renderer.prioritize(&[(0,0),(0,1),(1,0)]);
        renderer.request((0,0),&cart,false);
        renderer.request((0,1),&cart,false);
        renderer.request((1,0),&cart,false);
        assert_eq!(work.0.lock().unwrap().jobs.len(),3);
        // L1/R1 changes selected system while previous PNGs are still queued.
        renderer.prioritize(&[(2,5),(1,0),(2,6)]);
        assert!(!renderer.queued((0,0)));
        assert!(!renderer.queued((0,1)));
        assert!(renderer.queued((1,0)));
        renderer.request((2,6),&cart,false);
        renderer.request((2,5),&cart,false);
        let queue=work.0.lock().unwrap();
        let ids:Vec<_>=queue.jobs.iter().map(|job|job.0.0).collect();
        assert_eq!(ids,vec![(2,5),(1,0),(2,6)]);
        drop(queue);
        // Reversing direction still keeps the current cart first.
        renderer.prioritize(&[(2,6),(2,5)]);
        let queue=work.0.lock().unwrap();
        assert_eq!(queue.jobs.front().unwrap().0.0,(2,6));
    }

    #[test]
    fn rebuilding_recents_rejects_an_old_worker_face_with_same_index() {
        let work=Arc::new((Mutex::new(WorkQueue::default()),Condvar::new()));
        let (completed, finished) = mpsc::channel();
        let mut renderer = CartRender {
            work:work.clone(), finished, pending: HashSet::new(), wanted: Vec::new(), epochs: [0; 5],
        };
        renderer.pending.insert(((3, 0), 0));
        renderer.invalidate_shelf(3);
        assert!(!renderer.queued((3, 0)));
        renderer.pending.insert(((3, 0), 1));
        let face = || CartFace { rgba: vec![255; 4], w: 1, h: 1 };
        completed.send((((3, 0), 0), face(), None)).unwrap();
        completed.send((((3, 0), 1), face(), None)).unwrap();
        let accepted = renderer.ready().expect("fresh Recents face");
        assert_eq!(accepted.0, (3, 0));
        assert!(renderer.ready().is_none());
        assert!(!renderer.queued((3, 0)));
    }
}

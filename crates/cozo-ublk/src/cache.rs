//! Bounded write-back cache of decoded 64 KiB chunks.
//!
//! Reads and sub-chunk read-modify-writes are served from memory; writes only
//! dirty a cached chunk. Dirty chunks reach the store in ONE write batch with
//! ONE generation per writeback ([`ChunkCache::writeback`]): on every FLUSH or
//! FUA request (synced, so the request completes only once durable), from a
//! background thread when the dirty set passes a soft limit or ages, and
//! inline when it passes a hard limit. The device advertises a volatile write
//! cache, so the kernel/btrfs only relies on data the FLUSH/FUA path made
//! durable; that is exactly what the cache guarantees.
//!
//! Locking: chunks are spread over [`STRIPES`] mutex-protected stripes keyed
//! by chunk index (the per-chunk lock). A stripe is held for the whole
//! operation on one of its chunks, including a store load on a miss, so two
//! queues never race a read-modify-write of the same chunk. Writebacks and
//! discards are serialised by `writeback_lock`, which also orders their store
//! generations. A chunk being written back is pinned (never evicted) until
//! its commit settles; if the commit fails it stays dirty.

use op_cozo_store::{BlockDevice, CozoBlockStore};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Number of lock stripes (power of two).
pub const STRIPES: usize = 64;
/// Background writeback when chunks have been dirty this long.
const DIRTY_EXPIRE: Duration = Duration::from_secs(5);

struct Entry {
    data: Box<[u8]>,
    /// Bumped on every modification.
    seq: u64,
    /// `seq` last committed to the store; dirty iff `seq != flushed_seq`.
    flushed_seq: u64,
    /// Snapshotted by an in-progress writeback: never evict.
    pinned: bool,
    /// Recency for eviction.
    tick: u64,
}

impl Entry {
    fn dirty(&self) -> bool {
        self.seq != self.flushed_seq
    }
}

#[derive(Default)]
struct Stripe {
    map: HashMap<u64, Entry>,
}

pub struct ChunkCache {
    stripes: Vec<Mutex<Stripe>>,
    chunk: usize,
    per_stripe_cap: usize,
    soft_dirty: usize,
    hard_dirty: usize,
    dirty: AtomicUsize,
    tick: AtomicUsize,
    /// A commit since the last durability barrier was not synced.
    unsynced: AtomicBool,
    writeback_lock: Mutex<()>,
    wake: Condvar,
    wake_lock: Mutex<()>,
}

impl ChunkCache {
    /// `capacity_chunks` bounds clean + dirty chunks (dirty chunks may exceed
    /// a stripe's share briefly; the hard dirty limit bounds the total).
    pub fn new(chunk: usize, capacity_chunks: usize) -> Self {
        let capacity = capacity_chunks.max(STRIPES * 4);
        Self {
            stripes: (0..STRIPES).map(|_| Mutex::new(Stripe::default())).collect(),
            chunk,
            per_stripe_cap: capacity / STRIPES,
            soft_dirty: capacity / 4,
            hard_dirty: capacity / 2,
            dirty: AtomicUsize::new(0),
            tick: AtomicUsize::new(0),
            unsynced: AtomicBool::new(false),
            writeback_lock: Mutex::new(()),
            wake: Condvar::new(),
            wake_lock: Mutex::new(()),
        }
    }

    pub fn dirty_chunks(&self) -> usize {
        self.dirty.load(Ordering::Relaxed)
    }

    pub fn over_hard_limit(&self) -> bool {
        self.dirty_chunks() >= self.hard_dirty
    }

    fn stripe(&self, idx: u64) -> MutexGuard<'_, Stripe> {
        self.stripes[(idx as usize) & (STRIPES - 1)]
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn next_tick(&self) -> u64 {
        self.tick.fetch_add(1, Ordering::Relaxed) as u64
    }

    /// Copy `dst.len()` bytes at `at` inside chunk `idx` into `dst`.
    pub fn read(
        &self,
        store: &CozoBlockStore,
        device: &BlockDevice,
        idx: u64,
        at: usize,
        dst: &mut [u8],
    ) -> Result<(), String> {
        let mut stripe = self.stripe(idx);
        let tick = self.next_tick();
        if let Some(entry) = stripe.map.get_mut(&idx) {
            entry.tick = tick;
            dst.copy_from_slice(&entry.data[at..at + dst.len()]);
            return Ok(());
        }
        let data = self.load(store, device, idx)?;
        dst.copy_from_slice(&data[at..at + dst.len()]);
        stripe.map.insert(
            idx,
            Entry {
                data,
                seq: 0,
                flushed_seq: 0,
                pinned: false,
                tick,
            },
        );
        self.evict(&mut stripe);
        Ok(())
    }

    /// Overwrite `src.len()` bytes at `at` inside chunk `idx` (a whole-chunk
    /// write needs no load; a partial one is a read-modify-write in memory).
    pub fn write(
        &self,
        store: &CozoBlockStore,
        device: &BlockDevice,
        idx: u64,
        at: usize,
        src: &[u8],
    ) -> Result<(), String> {
        if at == 0 && src.len() == self.chunk {
            return self.modify_whole(idx, |data| data.copy_from_slice(src));
        }
        self.modify(store, device, idx, |data| {
            data[at..at + src.len()].copy_from_slice(src)
        })
    }

    /// Zero `len` bytes at `at` inside chunk `idx`.
    pub fn zero(
        &self,
        store: &CozoBlockStore,
        device: &BlockDevice,
        idx: u64,
        at: usize,
        len: usize,
    ) -> Result<(), String> {
        if at == 0 && len == self.chunk {
            return self.modify_whole(idx, |data| data.fill(0));
        }
        self.modify(store, device, idx, |data| data[at..at + len].fill(0))
    }

    fn modify(
        &self,
        store: &CozoBlockStore,
        device: &BlockDevice,
        idx: u64,
        f: impl FnOnce(&mut [u8]),
    ) -> Result<(), String> {
        let mut stripe = self.stripe(idx);
        let tick = self.next_tick();
        if let Some(entry) = stripe.map.get_mut(&idx) {
            if !entry.dirty() {
                self.dirty.fetch_add(1, Ordering::Relaxed);
            }
            entry.seq += 1;
            entry.tick = tick;
            f(&mut entry.data);
            return Ok(());
        }
        // Whole-chunk overwrites are detected by the caller through
        // `modify_whole`; anything else needs the current bytes.
        let mut data = self.load(store, device, idx)?;
        f(&mut data);
        self.insert_dirty(&mut stripe, idx, data, tick);
        Ok(())
    }

    /// Whole-chunk overwrite: never loads the old bytes.
    pub fn modify_whole(&self, idx: u64, f: impl FnOnce(&mut [u8])) -> Result<(), String> {
        let mut stripe = self.stripe(idx);
        let tick = self.next_tick();
        if let Some(entry) = stripe.map.get_mut(&idx) {
            if !entry.dirty() {
                self.dirty.fetch_add(1, Ordering::Relaxed);
            }
            entry.seq += 1;
            entry.tick = tick;
            f(&mut entry.data);
            return Ok(());
        }
        let mut data = vec![0u8; self.chunk].into_boxed_slice();
        f(&mut data);
        self.insert_dirty(&mut stripe, idx, data, tick);
        Ok(())
    }

    fn insert_dirty(&self, stripe: &mut Stripe, idx: u64, data: Box<[u8]>, tick: u64) {
        stripe.map.insert(
            idx,
            Entry {
                data,
                seq: 1,
                flushed_seq: 0,
                pinned: false,
                tick,
            },
        );
        self.dirty.fetch_add(1, Ordering::Relaxed);
        self.evict(stripe);
    }

    fn load(
        &self,
        store: &CozoBlockStore,
        device: &BlockDevice,
        idx: u64,
    ) -> Result<Box<[u8]>, String> {
        let mut data = vec![0u8; self.chunk].into_boxed_slice();
        store
            .read_block(device, idx, &mut data)
            .map_err(|e| e.to_string())?;
        Ok(data)
    }

    /// Drop least-recently-used clean, unpinned chunks above the stripe's share.
    fn evict(&self, stripe: &mut Stripe) {
        while stripe.map.len() > self.per_stripe_cap {
            let victim = stripe
                .map
                .iter()
                .filter(|(_, e)| !e.dirty() && !e.pinned)
                .min_by_key(|(_, e)| e.tick)
                .map(|(idx, _)| *idx);
            match victim {
                Some(idx) => {
                    stripe.map.remove(&idx);
                }
                None => break,
            }
        }
    }

    /// Commit every dirty chunk in ONE store write batch (one generation).
    /// With `sync`, the batch (or, when nothing is dirty, a WAL barrier) is
    /// durable when this returns, together with every earlier commit.
    pub fn writeback(
        &self,
        store: &CozoBlockStore,
        device: &BlockDevice,
        sync: bool,
    ) -> Result<(), String> {
        let _serial = self
            .writeback_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut items: Vec<(u64, u64, Box<[u8]>)> = Vec::new();
        for stripe in &self.stripes {
            let mut stripe = stripe.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            for (idx, entry) in stripe.map.iter_mut() {
                if entry.dirty() {
                    entry.pinned = true;
                    items.push((*idx, entry.seq, entry.data.clone()));
                }
            }
        }
        if items.is_empty() {
            if sync && self.unsynced.swap(false, Ordering::AcqRel) {
                if let Err(err) = store.flush() {
                    self.unsynced.store(true, Ordering::Release);
                    return Err(err.to_string());
                }
            }
            return Ok(());
        }
        items.sort_unstable_by_key(|(idx, _, _)| *idx);
        let batch: Vec<(u64, &[u8])> = items
            .iter()
            .map(|(idx, _, data)| (*idx, &data[..]))
            .collect();
        let result = store
            .write_blocks(device, &batch, sync)
            .map_err(|e| e.to_string());
        for (idx, seq, _) in &items {
            let mut stripe = self.stripe(*idx);
            if let Some(entry) = stripe.map.get_mut(idx) {
                entry.pinned = false;
                if result.is_ok() && entry.flushed_seq < *seq {
                    entry.flushed_seq = *seq;
                    if !entry.dirty() {
                        self.dirty.fetch_sub(1, Ordering::Relaxed);
                    }
                }
                self.evict(&mut stripe);
            }
        }
        match &result {
            Ok(()) if sync => self.unsynced.store(false, Ordering::Release),
            Ok(()) => self.unsynced.store(true, Ordering::Release),
            Err(_) => {}
        }
        result
    }

    /// Discard whole chunks `lo..hi`: drop them from the cache, then leave
    /// store tombstones for those that hold data. Serialised with writebacks,
    /// so no older writeback can commit after the tombstones.
    pub fn discard(
        &self,
        store: &CozoBlockStore,
        device: &BlockDevice,
        lo: u64,
        hi: u64,
    ) -> Result<(), String> {
        let _serial = self
            .writeback_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for stripe in &self.stripes {
            let mut stripe = stripe.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            stripe.map.retain(|idx, entry| {
                let drop = *idx >= lo && *idx < hi;
                if drop && entry.dirty() {
                    self.dirty.fetch_sub(1, Ordering::Relaxed);
                }
                !drop
            });
        }
        let result = store
            .discard_blocks(device, lo, hi)
            .map_err(|e| e.to_string());
        if result.is_ok() {
            self.unsynced.store(true, Ordering::Release);
        }
        result
    }

    /// Ask the background writer to look now.
    pub fn nudge(&self) {
        self.wake.notify_one();
    }

    /// Background writer: writes back when the dirty set passes the soft
    /// limit or has been dirty for [`DIRTY_EXPIRE`]. Never returns.
    pub fn background_writer(&self, store: &CozoBlockStore, device: &BlockDevice) -> ! {
        let mut dirty_since: Option<Instant> = None;
        loop {
            {
                let guard = self
                    .wake_lock
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let _ = self.wake.wait_timeout(guard, Duration::from_secs(1));
            }
            let dirty = self.dirty_chunks();
            if dirty == 0 {
                dirty_since = None;
                continue;
            }
            let since = *dirty_since.get_or_insert_with(Instant::now);
            if dirty >= self.soft_dirty || since.elapsed() >= DIRTY_EXPIRE {
                if let Err(err) = self.writeback(store, device, false) {
                    eprintln!("cozo-ublk: background writeback: {err}");
                } else {
                    dirty_since = None;
                }
            }
        }
    }

    pub fn over_soft_limit(&self) -> bool {
        self.dirty_chunks() >= self.soft_dirty
    }
}

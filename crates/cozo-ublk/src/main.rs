//! Foreground ublk target backed by the op-cozo-store block store.
//!
//! Serves one registered device (default `pool-default-persist`) as
//! `/dev/ublkb<UBLK_DEV_ID>`: 4 KiB logical blocks, 64 KiB chunks.
//!
//! - Several hardware queues (`OP_UBLK_QUEUES`, default 4, 1..=8), each on
//!   its own thread, share one chunk cache with per-chunk (striped) locks, so
//!   requests on different chunks run in parallel and a partial-chunk
//!   read-modify-write never races another request on the same chunk.
//! - Each queue drains every request that one io_uring wakeup delivered,
//!   applies them in order against the cache, and, if any of them is a FLUSH
//!   or FUA write, commits the whole dirty set in ONE store write batch with
//!   ONE generation (synced) before completing the batch.
//! - The cache is a bounded write-back cache of decoded chunks
//!   (`OP_UBLK_CACHE_MB`, default 256). The device advertises a volatile
//!   write cache, so only FLUSH/FUA promise durability, which they get.
//!
//! Never open or mount `/dev/ublkbN` from this process: that deadlocks the
//! queue. The runit service mounts it.
//!
//! Environment:
//!   OP_UBLK_COZO_DB_PATH  store path (default /var/lib/op-dbus/ublk-cozo)
//!   UBLK_DEVICE           registered device id (default pool-default-persist)
//!   UBLK_DEV_ID           ublk device number (default 1)
//!   OP_UBLK_QUEUES        hardware queues, 1..=8 (default 4)
//!   OP_UBLK_QUEUE_DEPTH   tags per queue, 16..=256 (default 64)
//!   OP_UBLK_CACHE_MB      chunk cache size in MiB, 16..=8192 (default 256)
//!   OP_UBLK_DEBUG         1 = per-request debug lines (buffered, one write
//!                         per io_uring batch)

mod cache;

use cache::ChunkCache;
use libublk::ctrl::{UblkCtrl, UblkCtrlBuilder};
use libublk::helpers::IoBuf;
use libublk::io::{BufDescList, UblkDev, UblkIOCtx, UblkQueue};
use libublk::{BufDesc, UblkFlags, UblkIORes};
use op_cozo_store::{BlockDevice, CozoBlockStore, POOL_DEFAULT_PERSIST_DEVICE};
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const IO_BUF_BYTES: u32 = 512 * 1024;
/// Discard carries no data, so one request may cover far more than a buffer.
const MAX_DISCARD_BYTES: u64 = 1 << 30;

static DEBUG: AtomicBool = AtomicBool::new(false);

thread_local! {
    static LOG: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(64 * 1024));
}

/// Append one debug line to this thread's buffer (no syscall).
macro_rules! debug_line {
    ($($arg:tt)*) => {
        if DEBUG.load(Ordering::Relaxed) {
            LOG.with(|log| {
                let mut log = log.borrow_mut();
                let _ = writeln!(log, $($arg)*);
            });
        }
    };
}

/// Write this thread's buffered debug lines with one `write` call.
fn flush_debug_log() {
    if !DEBUG.load(Ordering::Relaxed) {
        return;
    }
    LOG.with(|log| {
        let mut log = log.borrow_mut();
        if !log.is_empty() {
            let _ = std::io::stderr().lock().write_all(&log);
            log.clear();
        }
    });
}

fn main() {
    if let Err(err) = run() {
        eprintln!("cozo-ublk: {err}");
        std::process::exit(1);
    }
}

struct Target {
    store: CozoBlockStore,
    device: BlockDevice,
    cache: ChunkCache,
}

fn env_number(name: &str, default: u64, min: u64, max: u64) -> Result<u64, String> {
    match std::env::var(name) {
        Ok(text) if !text.is_empty() => {
            let value: u64 = text.parse().map_err(|e| format!("{name}: {e}"))?;
            if value < min || value > max {
                return Err(format!("{name}={value} is outside {min}..={max}"));
            }
            Ok(value)
        }
        _ => Ok(default),
    }
}

fn run() -> Result<(), String> {
    DEBUG.store(
        std::env::var("OP_UBLK_DEBUG").is_ok_and(|v| v == "1"),
        Ordering::Relaxed,
    );
    let device_id = std::env::var("UBLK_DEVICE")
        .ok()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| POOL_DEFAULT_PERSIST_DEVICE.to_string());
    let dev_id: i32 = match std::env::var("UBLK_DEV_ID") {
        Ok(text) if !text.is_empty() => text.parse().map_err(|e| format!("UBLK_DEV_ID: {e}"))?,
        _ => 1,
    };
    let nr_queues = env_number("OP_UBLK_QUEUES", 4, 1, 8)? as u16;
    let depth = env_number("OP_UBLK_QUEUE_DEPTH", 64, 16, 256)? as u16;
    let cache_mb = env_number("OP_UBLK_CACHE_MB", 256, 16, 8192)?;
    let store = CozoBlockStore::open_default().map_err(|e| e.to_string())?;
    let device = store
        .device(&device_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "device {device_id} is not registered in {}",
                store.path().display()
            )
        })?;
    if device.block_size != 4096 || device.chunk_size % device.block_size != 0 {
        return Err(format!("unsupported geometry {device:?}"));
    }
    let size = device.size_bytes;
    let chunk = device.chunk_size;
    let cache_chunks = (cache_mb << 20) / u64::from(chunk);
    eprintln!(
        "store {} device {} size {} chunk {} queues {} depth {} cache {} MiB",
        store.path().display(),
        device.id,
        device.size_bytes,
        device.chunk_size,
        nr_queues,
        depth,
        cache_mb
    );
    let cache = ChunkCache::new(chunk as usize, cache_chunks as usize);
    let target = Arc::new(Target {
        store,
        device,
        cache,
    });

    {
        let writer = Arc::clone(&target);
        std::thread::Builder::new()
            .name("cozo-writeback".into())
            .spawn(move || {
                writer
                    .cache
                    .background_writer(&writer.store, &writer.device)
            })
            .map_err(|e| format!("spawn writeback thread: {e}"))?;
    }

    // A daemon that exits without tearing down (SIGTERM, crash) leaves its
    // device registered; adding the same id again would fail with EEXIST.
    if let Ok(stale) = UblkCtrl::new_simple(dev_id) {
        eprintln!("removing stale ublk dev {dev_id}");
        let _ = stale.kill_dev();
        stale
            .del_dev()
            .map_err(|e| format!("delete stale ublk dev {dev_id}: {e}"))?;
    }

    let ctrl = UblkCtrlBuilder::default()
        .name("cozo")
        .id(dev_id)
        .nr_queues(nr_queues)
        .depth(depth)
        .io_buf_bytes(IO_BUF_BYTES)
        .dev_flags(UblkFlags::UBLK_DEV_F_ADD_DEV)
        .build()
        .map_err(|err| err.to_string())?;

    let tgt_init = move |dev: &mut UblkDev| {
        dev.set_default_params(size);
        let p = &mut dev.tgt.params;
        p.types |= libublk::sys::UBLK_PARAM_TYPE_DISCARD;
        p.basic.attrs = libublk::sys::UBLK_ATTR_VOLATILE_CACHE | libublk::sys::UBLK_ATTR_FUA;
        p.basic.logical_bs_shift = 12;
        p.basic.physical_bs_shift = 12;
        p.basic.io_min_shift = 12;
        p.basic.io_opt_shift = chunk.trailing_zeros() as u8;
        p.discard.discard_alignment = 0;
        p.discard.discard_granularity = chunk;
        p.discard.max_discard_sectors = (MAX_DISCARD_BYTES >> 9) as u32;
        p.discard.max_write_zeroes_sectors = (MAX_DISCARD_BYTES >> 9) as u32;
        p.discard.max_discard_segments = 1;
        Ok(())
    };

    let for_queue = Arc::clone(&target);
    let q_fn = move |qid: u16, dev: &UblkDev| serve_queue(qid, dev, Arc::clone(&for_queue));
    let started = move |ctrl: &UblkCtrl| {
        eprintln!("ublk dev {} serving {size} bytes", ctrl.dev_info().dev_id);
    };
    let result = ctrl
        .run_target(tgt_init, q_fn, started)
        .map_err(|err| err.to_string());
    // The device is gone: persist whatever the cache still holds.
    if let Err(err) = target.cache.writeback(&target.store, &target.device, true) {
        eprintln!("cozo-ublk: final writeback: {err}");
    }
    result?;
    Ok(())
}

impl Target {
    fn chunk(&self) -> u64 {
        u64::from(self.device.chunk_size)
    }

    /// Call `f(chunk_idx, offset_in_chunk, len, offset_in_request)` per chunk.
    fn each_chunk(
        &self,
        off: u64,
        len: u64,
        mut f: impl FnMut(u64, usize, usize, usize) -> Result<(), String>,
    ) -> Result<(), String> {
        let chunk = self.chunk();
        let mut done = 0u64;
        while done < len {
            let pos = off + done;
            let in_chunk = pos % chunk;
            let n = (chunk - in_chunk).min(len - done);
            f(pos / chunk, in_chunk as usize, n as usize, done as usize)?;
            done += n;
        }
        Ok(())
    }

    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), String> {
        self.each_chunk(off, dst.len() as u64, |idx, at, n, done| {
            self.cache
                .read(&self.store, &self.device, idx, at, &mut dst[done..done + n])
        })
    }

    fn write_at(&self, off: u64, src: &[u8]) -> Result<(), String> {
        self.each_chunk(off, src.len() as u64, |idx, at, n, done| {
            self.cache
                .write(&self.store, &self.device, idx, at, &src[done..done + n])
        })
    }

    /// Discard and write-zeroes: whole chunks become store tombstones in one
    /// range commit, partial edge chunks are zeroed in the cache.
    fn zero_at(&self, off: u64, len: u64) -> Result<(), String> {
        let chunk = self.chunk();
        let end = off + len;
        let first_full = off.div_ceil(chunk);
        let last_full = end / chunk;
        if first_full < last_full {
            self.cache
                .discard(&self.store, &self.device, first_full, last_full)?;
        }
        let zero_edge = |from: u64, to: u64| -> Result<(), String> {
            if from >= to {
                return Ok(());
            }
            let idx = from / chunk;
            let at = (from % chunk) as usize;
            self.cache
                .zero(&self.store, &self.device, idx, at, (to - from) as usize)
        };
        if first_full >= last_full {
            // The range lies within one or two chunks with no full chunk.
            let split = (off / chunk + 1) * chunk;
            zero_edge(off, end.min(split))?;
            zero_edge(split.max(off), end)
        } else {
            zero_edge(off, first_full * chunk)?;
            zero_edge(last_full * chunk, end)
        }
    }

    /// Durability barrier for FLUSH / FUA: one synced batch.
    fn flush(&self) -> Result<(), String> {
        self.cache.writeback(&self.store, &self.device, true)
    }
}

/// What one request needs after the in-order cache pass.
struct Pending {
    tag: u16,
    res: i32,
    needs_barrier: bool,
    started: Instant,
}

/// Apply one request to the cache. Durability (FLUSH, FUA) is deferred to the
/// batch barrier; returns `(result, needs_barrier)`.
fn handle_io(
    q: &UblkQueue<'_>,
    dev_id: i32,
    tag: u16,
    buf: &IoBuf<u8>,
    t: &Target,
) -> (i32, bool) {
    let iod = q.get_iod(tag);
    let op = iod.op_flags & 0xff;
    let fua = iod.op_flags & libublk::sys::UBLK_IO_F_FUA != 0;
    let off = iod.start_sector << 9;
    let bytes = (iod.nr_sectors as u64) << 9;
    debug_line!(
        "debug ublk dev={} queue={} tag={} begin op={} op_flags={:#x} sector={} sectors={}",
        dev_id,
        q.get_qid(),
        tag,
        op_name(op),
        iod.op_flags,
        iod.start_sector,
        iod.nr_sectors
    );
    let fail = |err: String| {
        eprintln!("io op {op} off {off} len {bytes}: {err}");
        -libc::EIO
    };
    if op != libublk::sys::UBLK_IO_OP_FLUSH
        && (off.saturating_add(bytes) > t.device.size_bytes || bytes > i32::MAX as u64)
    {
        return (-libc::EIO, false);
    }
    match op {
        libublk::sys::UBLK_IO_OP_READ | libublk::sys::UBLK_IO_OP_WRITE => {
            let n = bytes as usize;
            // SAFETY: the queue owns this tag's buffer for the duration of the request.
            let io = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()) };
            if n > io.len() {
                return (-libc::EIO, false);
            }
            let result = if op == libublk::sys::UBLK_IO_OP_READ {
                t.read_at(off, &mut io[..n])
            } else {
                t.write_at(off, &io[..n])
            };
            match result {
                Ok(()) => (bytes as i32, op == libublk::sys::UBLK_IO_OP_WRITE && fua),
                Err(err) => (fail(err), false),
            }
        }
        libublk::sys::UBLK_IO_OP_FLUSH => (0, true),
        libublk::sys::UBLK_IO_OP_DISCARD | libublk::sys::UBLK_IO_OP_WRITE_ZEROES => {
            match t.zero_at(off, bytes) {
                Ok(()) => (0, fua),
                Err(err) => (fail(err), false),
            }
        }
        _ => (-libc::EOPNOTSUPP, false),
    }
}

fn op_name(op: u32) -> &'static str {
    match op {
        libublk::sys::UBLK_IO_OP_READ => "read",
        libublk::sys::UBLK_IO_OP_WRITE => "write",
        libublk::sys::UBLK_IO_OP_FLUSH => "flush",
        libublk::sys::UBLK_IO_OP_DISCARD => "discard",
        libublk::sys::UBLK_IO_OP_WRITE_ZEROES => "write_zeroes",
        _ => "other",
    }
}

/// Run the cache pass for every request of one io_uring wakeup, then one
/// barrier if any of them needs it, then complete them all.
fn process_batch(
    q: &UblkQueue<'_>,
    dev_id: i32,
    tags: &mut Vec<u16>,
    bufs: &[IoBuf<u8>],
    target: &Target,
) {
    let mut done: Vec<Pending> = Vec::with_capacity(tags.len());
    for tag in tags.drain(..) {
        let started = Instant::now();
        let (res, needs_barrier) = handle_io(q, dev_id, tag, &bufs[tag as usize], target);
        done.push(Pending {
            tag,
            res,
            needs_barrier,
            started,
        });
    }
    if done.iter().any(|p| p.needs_barrier && p.res >= 0) {
        if let Err(err) = target.flush() {
            eprintln!("flush barrier: {err}");
            for pending in done.iter_mut().filter(|p| p.needs_barrier && p.res >= 0) {
                pending.res = -libc::EIO;
            }
        }
    } else if target.cache.over_hard_limit() {
        if let Err(err) = target.cache.writeback(&target.store, &target.device, false) {
            eprintln!("inline writeback: {err}");
        }
    } else if target.cache.over_soft_limit() {
        target.cache.nudge();
    }
    for pending in &done {
        let slice = bufs[pending.tag as usize].as_slice();
        if let Err(err) = q.complete_io_cmd_unified(
            pending.tag,
            BufDesc::Slice(slice),
            Ok(UblkIORes::Result(pending.res)),
        ) {
            eprintln!("complete io tag {}: {err}", pending.tag);
        }
        debug_line!(
            "debug ublk dev={} queue={} tag={} commit_submitted result={} elapsed_us={}",
            dev_id,
            q.get_qid(),
            pending.tag,
            pending.res,
            pending.started.elapsed().as_micros()
        );
    }
    flush_debug_log();
}

fn serve_queue(qid: u16, dev: &UblkDev, target: Arc<Target>) {
    let bufs_rc = Rc::new(dev.alloc_queue_io_bufs());
    let bufs = Rc::clone(&bufs_rc);
    let depth = dev.dev_info.queue_depth as usize;
    let dev_id = dev.dev_info.dev_id as i32;
    let mut pending: Vec<u16> = Vec::with_capacity(depth);
    let io_handler = move |q: &UblkQueue<'_>, tag: u16, io: &UblkIOCtx<'_>| {
        pending.push(tag);
        // Every request of this wakeup has been seen at the batch's last CQE.
        if io.is_last_cqe() || pending.len() >= depth {
            process_batch(q, dev_id, &mut pending, &bufs, &target);
        }
    };
    let queue = match UblkQueue::new(qid, dev)
        .and_then(|queue| queue.submit_fetch_commands_unified(BufDescList::Slices(Some(&bufs_rc))))
    {
        Ok(queue) => queue,
        Err(err) => {
            eprintln!("queue {qid} setup failed: {err}");
            std::process::exit(1);
        }
    };
    queue.wait_and_handle_io(io_handler);
}

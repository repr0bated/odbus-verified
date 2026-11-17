//! Foreground ublk target backed by the op-cozo-store block store.
//!
//! Serves one registered device (default `pool-default-persist`) as
//! `/dev/ublkb<UBLK_DEV_ID>`: 4 KiB logical blocks, 64 KiB chunks in Cozo.
//! One queue handled synchronously, so requests never overlap and a partial
//! chunk read-modify-write cannot race another request on the same chunk.
//! FLUSH and FUA writes call the store's durability barrier before completing.
//!
//! Never open or mount `/dev/ublkbN` from this process: that deadlocks the
//! queue. The runit service mounts it.
//!
//! Environment:
//!   OP_UBLK_COZO_DB_PATH  store path (default /var/lib/op-dbus/ublk-cozo)
//!   UBLK_DEVICE           registered device id (default pool-default-persist)
//!   UBLK_DEV_ID           ublk device number (default 1)

use libublk::ctrl::{UblkCtrl, UblkCtrlBuilder};
use libublk::helpers::IoBuf;
use libublk::io::{BufDescList, UblkDev, UblkIOCtx, UblkQueue};
use libublk::{BufDesc, UblkFlags, UblkIORes};
use op_cozo_store::{BlockDevice, CozoBlockStore, POOL_DEFAULT_PERSIST_DEVICE};
use std::rc::Rc;
use std::sync::Arc;

const IO_BUF_BYTES: u32 = 512 * 1024;
/// Discard carries no data, so one request may cover far more than a buffer.
const MAX_DISCARD_BYTES: u64 = 1 << 30;

fn main() {
    if let Err(err) = run() {
        eprintln!("cozo-ublk: {err}");
        std::process::exit(1);
    }
}

struct Target {
    store: CozoBlockStore,
    device: BlockDevice,
}

fn run() -> Result<(), String> {
    let device_id = std::env::var("UBLK_DEVICE")
        .ok()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| POOL_DEFAULT_PERSIST_DEVICE.to_string());
    let dev_id: i32 = match std::env::var("UBLK_DEV_ID") {
        Ok(text) if !text.is_empty() => text.parse().map_err(|e| format!("UBLK_DEV_ID: {e}"))?,
        _ => 1,
    };
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
    eprintln!(
        "store {} device {} size {} chunk {}",
        store.path().display(),
        device.id,
        device.size_bytes,
        device.chunk_size
    );
    let size = device.size_bytes;
    let chunk = device.chunk_size;
    let target = Arc::new(Target { store, device });

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
        .nr_queues(1)
        .depth(64)
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
    ctrl.run_target(tgt_init, q_fn, started)
        .map_err(|err| err.to_string())?;
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
        let mut buf = vec![0u8; self.device.chunk_size as usize];
        self.each_chunk(off, dst.len() as u64, |idx, at, n, done| {
            self.store
                .read_block(&self.device, idx, &mut buf)
                .map_err(|e| e.to_string())?;
            dst[done..done + n].copy_from_slice(&buf[at..at + n]);
            Ok(())
        })
    }

    fn write_at(&self, off: u64, src: &[u8]) -> Result<(), String> {
        let chunk = self.device.chunk_size as usize;
        let mut buf = vec![0u8; chunk];
        self.each_chunk(off, src.len() as u64, |idx, at, n, done| {
            if n == chunk {
                return self
                    .store
                    .write_block(&self.device, idx, &src[done..done + n])
                    .map_err(|e| e.to_string());
            }
            self.store
                .read_block(&self.device, idx, &mut buf)
                .map_err(|e| e.to_string())?;
            buf[at..at + n].copy_from_slice(&src[done..done + n]);
            self.store
                .write_block(&self.device, idx, &buf)
                .map_err(|e| e.to_string())
        })
    }

    /// Discard and write-zeroes: whole chunks are deleted in one range query,
    /// partial edge chunks are zeroed by read-modify-write.
    fn zero_at(&self, off: u64, len: u64) -> Result<(), String> {
        let chunk = self.chunk();
        let end = off + len;
        let first_full = off.div_ceil(chunk);
        let last_full = end / chunk;
        if first_full < last_full {
            self.store
                .discard_blocks(&self.device, first_full, last_full)
                .map_err(|e| e.to_string())?;
        }
        let mut buf = vec![0u8; chunk as usize];
        let mut zero_edge = |from: u64, to: u64| -> Result<(), String> {
            if from >= to {
                return Ok(());
            }
            let idx = from / chunk;
            if !self
                .store
                .read_block(&self.device, idx, &mut buf)
                .map_err(|e| e.to_string())?
            {
                return Ok(());
            }
            buf[(from % chunk) as usize..((to - 1) % chunk + 1) as usize].fill(0);
            self.store
                .write_block(&self.device, idx, &buf)
                .map_err(|e| e.to_string())
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

    fn flush(&self) -> Result<(), String> {
        self.store.flush().map_err(|e| e.to_string())
    }
}

fn handle_io(q: &UblkQueue<'_>, tag: u16, buf: &IoBuf<u8>, t: &Target) -> i32 {
    let iod = q.get_iod(tag);
    let op = iod.op_flags & 0xff;
    let fua = iod.op_flags & libublk::sys::UBLK_IO_F_FUA != 0;
    let off = iod.start_sector << 9;
    let bytes = (iod.nr_sectors as u64) << 9;
    let fail = |err: String| {
        eprintln!("io op {op} off {off} len {bytes}: {err}");
        -libc::EIO
    };
    if op != libublk::sys::UBLK_IO_OP_FLUSH
        && (off.saturating_add(bytes) > t.device.size_bytes || bytes > i32::MAX as u64)
    {
        return -libc::EIO;
    }
    match op {
        libublk::sys::UBLK_IO_OP_READ | libublk::sys::UBLK_IO_OP_WRITE => {
            let n = bytes as usize;
            // SAFETY: the queue owns this tag's buffer for the duration of the request.
            let io = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()) };
            if n > io.len() {
                return -libc::EIO;
            }
            let result = if op == libublk::sys::UBLK_IO_OP_READ {
                t.read_at(off, &mut io[..n])
            } else {
                t.write_at(off, &io[..n])
                    .and_then(|()| if fua { t.flush() } else { Ok(()) })
            };
            match result {
                Ok(()) => bytes as i32,
                Err(err) => fail(err),
            }
        }
        libublk::sys::UBLK_IO_OP_FLUSH => match t.flush() {
            Ok(()) => 0,
            Err(err) => fail(err),
        },
        libublk::sys::UBLK_IO_OP_DISCARD | libublk::sys::UBLK_IO_OP_WRITE_ZEROES => {
            match t
                .zero_at(off, bytes)
                .and_then(|()| if fua { t.flush() } else { Ok(()) })
            {
                Ok(()) => 0,
                Err(err) => fail(err),
            }
        }
        _ => -libc::EOPNOTSUPP,
    }
}

fn serve_queue(qid: u16, dev: &UblkDev, target: Arc<Target>) {
    let bufs_rc = Rc::new(dev.alloc_queue_io_bufs());
    let bufs = Rc::clone(&bufs_rc);
    let io_handler = move |q: &UblkQueue<'_>, tag: u16, _io: &UblkIOCtx<'_>| {
        let res = handle_io(q, tag, &bufs[tag as usize], &target);
        let slice = bufs[tag as usize].as_slice();
        if let Err(err) =
            q.complete_io_cmd_unified(tag, BufDesc::Slice(slice), Ok(UblkIORes::Result(res)))
        {
            eprintln!("complete io tag {tag}: {err}");
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

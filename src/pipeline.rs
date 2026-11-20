//! Parsing on several threads: a reader thread walks the frames and scans
//! entity headers, decoding threads decode entity fields, and the calling
//! thread applies them and runs the visitor, in demo order.
//!
//! The visitor sees exactly what a single-threaded parse delivers. Decoding an
//! entity's fields needs only its class and where its data is, both known from
//! the scan (see `entities::job`); everything that depends on the game state
//! happens on the calling thread, in order.

use crate::Context;
use crate::bitreader::CoordParams;
use crate::container::{Frame, FrameIter};
use crate::entities::{Class, EntityJob, MAX_ENTITIES, NO_CLASS, RecordKind};
use crate::error::{Error, Result, decode_proto};
use crate::parser::{
    DemoParser, Flow, ParseOptions, SVC_PACKET_ENTITIES, SVC_SERVER_INFO, Visitor, Wants, build_classes,
    for_each_message,
};
use crate::proto::{CDemoClassInfo, CsvcMsgPacketEntities, CsvcMsgServerInfo, EDemoCommands};
use crate::sendtables::SendTables;
use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// A batch is sent once it holds this much entity data or this many
/// `svc_PacketEntities` messages. Small batches are still in the CPU caches
/// when the calling thread gets them, and the buffers each one keeps for
/// reuse stay small.
const BATCH_BYTES: usize = 64 * 1024;
const BATCH_JOBS: usize = 32;

/// Something the calling thread does, in demo order.
enum Item {
    /// A frame starts; its tick is handled first. `stop`: it is `DEM_Stop`.
    Frame {
        tick: i32,
        stop: bool,
    },
    SendTables(Box<SendTables>),
    Classes(Vec<Option<Arc<Class>>>),
    /// A net message for the calling thread, at `Batch::bytes[start..end]`.
    Message {
        msg_type: u32,
        start: usize,
        end: usize,
    },
    /// A `svc_PacketEntities` message: `Batch::jobs[i]`.
    Entities(usize),
    /// From the frame at this offset on, the calling thread parses alone.
    Sequential {
        offset: usize,
    },
    Error(Error),
}

/// Consecutive items, sent by the reader through a decoding thread to the
/// calling thread, then back to the reader to be refilled.
#[derive(Default)]
struct Batch {
    seq: u64,
    items: Vec<Item>,
    bytes: Vec<u8>,
    /// `jobs[..used]` belong to this batch; the others keep their buffers.
    jobs: Vec<EntityJob>,
    used: usize,
    /// Classes and coordinate encoding the jobs were scanned with.
    tables: Option<Arc<Tables>>,
    entity_bytes: usize,
    /// A decoding thread panicked on this batch: the calling thread panics
    /// with this when it gets there, as a single-threaded parse would.
    panic: Option<Box<dyn std::any::Any + Send>>,
}

/// Lengths to roll a batch back to the start of a frame.
struct Mark {
    items: usize,
    bytes: usize,
    used: usize,
    entity_bytes: usize,
}

impl Batch {
    fn reset(&mut self) {
        self.items.clear();
        self.bytes.clear();
        self.used = 0;
        self.tables = None;
        self.entity_bytes = 0;
    }

    fn next_job(&mut self) -> (usize, &mut EntityJob) {
        if self.used == self.jobs.len() {
            self.jobs.push(EntityJob::default());
        }
        self.used += 1;
        (self.used - 1, &mut self.jobs[self.used - 1])
    }

    fn mark(&self) -> Mark {
        Mark {
            items: self.items.len(),
            bytes: self.bytes.len(),
            used: self.used,
            entity_bytes: self.entity_bytes,
        }
    }

    fn rollback(&mut self, m: Mark) {
        self.items.truncate(m.items);
        self.bytes.truncate(m.bytes);
        self.used = m.used;
        self.entity_bytes = m.entity_bytes;
    }

    fn decode(&mut self) {
        let Some(t) = &self.tables else { return };
        for job in &mut self.jobs[..self.used] {
            job.decode(&t.classes, &t.coord);
        }
    }
}

struct Tables {
    classes: Vec<Option<Arc<Class>>>,
    coord: CoordParams,
}

/// What the reader does after a frame.
enum Next {
    Continue,
    /// The classes changed: send the batch, later jobs use the new ones.
    NewTables,
    /// Nothing more to read (end of the demo, stop, error).
    Done,
    /// This frame can't be scanned: the calling thread takes over from it.
    Sequential,
}

/// Walks the frames, scans entity headers and fills batches.
struct Reader<'a> {
    data: &'a [u8],
    opts: ParseOptions,
    class_id_bits: u32,
    send_tables: Option<SendTables>,
    pending_class_info: Option<CDemoClassInfo>,
    tables: Arc<Tables>,
    /// Class id of the entity at each index, or `NO_CLASS`.
    class_of: Vec<u32>,
    frame_buf: Vec<u8>,
    msg_buf: Vec<u8>,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8], opts: ParseOptions) -> Self {
        Reader {
            data,
            opts,
            class_id_bits: 0,
            send_tables: None,
            pending_class_info: None,
            tables: Arc::new(Tables {
                classes: Vec::new(),
                coord: CoordParams::default(),
            }),
            class_of: vec![NO_CLASS; MAX_ENTITIES],
            frame_buf: Vec::new(),
            msg_buf: Vec::new(),
        }
    }

    /// Fills batches until the end of the demo, or until the calling thread
    /// stops taking them.
    fn run(mut self, stop: &AtomicBool, out: Outputs, recycled: Receiver<Batch>, mut budget: usize) {
        let mut take = |seq: u64, tables: &Arc<Tables>| -> Option<Batch> {
            let mut batch = if budget > 0 {
                budget -= 1;
                Batch::default()
            } else {
                recycled.recv().ok()?
            };
            batch.reset();
            batch.seq = seq;
            batch.tables = Some(tables.clone());
            Some(batch)
        };
        let mut seq = 0;
        let Some(mut batch) = take(seq, &self.tables) else {
            return;
        };
        let mut frames = FrameIter::new(self.data);
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let next = match frames.next() {
                None => Next::Done,
                Some(Err(e)) => {
                    batch.items.push(Item::Error(e));
                    Next::Done
                }
                Some(Ok(frame)) => {
                    let mark = batch.mark();
                    let next = self.frame(&frame, &mut batch);
                    if let Next::Sequential = next {
                        batch.rollback(mark);
                        batch.items.push(Item::Sequential { offset: frame.offset });
                    }
                    next
                }
            };
            let last = matches!(next, Next::Done | Next::Sequential);
            let full = batch.entity_bytes >= BATCH_BYTES || batch.used >= BATCH_JOBS;
            if last || full || matches!(next, Next::NewTables) {
                if out.send(batch).is_err() || last {
                    return;
                }
                seq += 1;
                match take(seq, &self.tables) {
                    Some(b) => batch = b,
                    None => return,
                }
            }
        }
    }

    fn frame(&mut self, frame: &Frame, batch: &mut Batch) -> Next {
        let stop = frame.cmd == EDemoCommands::DemStop as u32;
        batch.items.push(Item::Frame {
            tick: frame.tick,
            stop,
        });
        // The calling thread stops at this frame.
        if stop || (frame.tick >= 0 && self.opts.end_tick.is_some_and(|end| frame.tick > end)) {
            return Next::Done;
        }
        match self.handle_frame(frame, batch) {
            Ok(next) => next,
            Err(e) => {
                batch.items.push(Item::Error(e));
                Next::Done
            }
        }
    }

    fn handle_frame(&mut self, frame: &Frame, batch: &mut Batch) -> Result<Next> {
        let Some(cmd) = frame.command() else {
            return Ok(Next::Continue);
        };
        match cmd {
            EDemoCommands::DemPacket | EDemoCommands::DemSignonPacket => {
                let mut buf = std::mem::take(&mut self.frame_buf);
                let result = match frame.data(&mut buf) {
                    Ok(payload) => self.messages(payload, batch),
                    Err(e) => Err(e),
                };
                self.frame_buf = buf;
                result
            }
            EDemoCommands::DemSendTables => {
                let mut buf = Vec::new();
                let st = SendTables::from_demo_send_tables(frame.data(&mut buf)?)?;
                batch.items.push(Item::SendTables(Box::new(st.clone())));
                self.send_tables = Some(st);
                self.update_tables(batch)
            }
            EDemoCommands::DemClassInfo => {
                let mut buf = Vec::new();
                let info: CDemoClassInfo = decode_proto("CDemoClassInfo", frame.data(&mut buf)?)?;
                self.pending_class_info = Some(info);
                self.update_tables(batch)
            }
            _ => Ok(Next::Continue),
        }
    }

    /// Builds the classes once both the send tables and the class info are
    /// in, like the calling thread does.
    fn update_tables(&mut self, batch: &mut Batch) -> Result<Next> {
        let mut classes = self.tables.classes.clone();
        if let (Some(info), Some(st)) = (&self.pending_class_info, &self.send_tables) {
            classes = build_classes(info, st, &self.opts.skip)?;
            self.pending_class_info = None;
            batch.items.push(Item::Classes(classes.clone()));
        }
        let coord = self
            .send_tables
            .as_ref()
            .map(|s| s.coord_params)
            .unwrap_or_default();
        self.tables = Arc::new(Tables { classes, coord });
        Ok(Next::NewTables)
    }

    fn messages(&mut self, payload: &[u8], batch: &mut Batch) -> Result<Next> {
        let mut msg = std::mem::take(&mut self.msg_buf);
        let wants = Wants::new(&self.opts);
        let mut next = Next::Continue;
        let result = for_each_message(payload, wants, &mut msg, |msg_type, data| {
            if msg_type == SVC_PACKET_ENTITIES {
                match self.entities(data, batch)? {
                    Next::Continue => return Ok(ControlFlow::Continue(())),
                    other => {
                        next = other;
                        return Ok(ControlFlow::Break(()));
                    }
                }
            }
            if msg_type == SVC_SERVER_INFO {
                let info: CsvcMsgServerInfo = decode_proto("CSVCMsg_ServerInfo", data)?;
                let max_classes = info.max_classes.unwrap_or(0).max(1) as f64;
                self.class_id_bits = max_classes.log2() as u32 + 1;
            }
            let start = batch.bytes.len();
            batch.bytes.extend_from_slice(data);
            batch.items.push(Item::Message {
                msg_type,
                start,
                end: batch.bytes.len(),
            });
            Ok(ControlFlow::Continue(()))
        });
        self.msg_buf = msg;
        result.map(|_| next)
    }

    fn entities(&mut self, data: &[u8], batch: &mut Batch) -> Result<Next> {
        let mut m: CsvcMsgPacketEntities = decode_proto("CSVCMsg_PacketEntities", data)?;
        if self.tables.classes.is_empty() {
            return Err(Error::malformed(
                "packet entities before class info / send tables",
            ));
        }
        let class_of = &self.class_of;
        let (i, job) = batch.next_job();
        let scanned = job.scan(
            m.entity_data.as_deref().unwrap_or(&[]),
            m.serialized_entities.as_deref(),
            m.updated_entries.unwrap_or(0),
            m.has_pvs_vis_bits_deprecated.unwrap_or(0) > 0,
            self.class_id_bits,
            &self.tables.classes,
            &|idx| class_of[idx],
        );
        if !scanned {
            return Ok(Next::Sequential);
        }
        job.set_data(m.entity_data.take().unwrap_or_default());
        for rec in job.records() {
            match rec.kind() {
                RecordKind::Create => self.class_of[rec.index()] = rec.class_id(),
                RecordKind::Delete => self.class_of[rec.index()] = NO_CLASS,
                RecordKind::Update | RecordKind::Leave => {}
            }
        }
        let failed = job.has_error();
        batch.entity_bytes += job.data_len();
        batch.items.push(Item::Entities(i));
        Ok(if failed { Next::Done } else { Next::Continue })
    }
}

/// Where the reader sends its batches.
struct Outputs<'s> {
    /// To the calling thread, decoded.
    done: Sender<Batch>,
    /// To the decoding threads, and how many batches wait for one.
    work: Option<(Sender<Batch>, &'s AtomicUsize)>,
    decoders: usize,
}

impl Outputs<'_> {
    /// Hands a batch to a decoding thread, or decodes it here when they all
    /// have work waiting (the reader is faster than one decoding thread).
    fn send(&self, mut batch: Batch) -> Result<(), ()> {
        if let Some((work, queued)) = &self.work
            && queued.load(Ordering::Relaxed) < self.decoders
        {
            queued.fetch_add(1, Ordering::Relaxed);
            return work.send(batch).map_err(|_| ());
        }
        batch.decode();
        self.done.send(batch).map_err(|_| ())
    }
}

fn decode_batches(batches: &Mutex<Receiver<Batch>>, queued: &AtomicUsize, out: Sender<Batch>) {
    loop {
        let batch = batches.lock().unwrap_or_else(|e| e.into_inner()).recv();
        let Ok(mut batch) = batch else { return };
        queued.fetch_sub(1, Ordering::Relaxed);
        // Losing the batch would leave the calling thread waiting for it.
        if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| batch.decode())) {
            batch.panic = Some(panic);
        }
        if out.send(batch).is_err() {
            return;
        }
    }
}

/// How the calling thread's part ended.
enum Outcome {
    Finished { stopped: bool },
    Sequential { offset: usize },
}

impl DemoParser<'_> {
    /// Parses on `threads` threads (at least 2), the calling thread included.
    pub(crate) fn run_parallel<V: Visitor + ?Sized>(
        mut self,
        visitor: &mut V,
        threads: usize,
    ) -> Result<Context> {
        visitor.on_start(&self.ctx);
        let decoders = threads.saturating_sub(2);
        let budget = 2 * decoders.max(1) + 2;
        let stop = &AtomicBool::new(false);
        let queued = &AtomicUsize::new(0);
        let (data, opts) = (self.data, self.opts.clone());
        let outcome = std::thread::scope(|s| {
            let (done_tx, done_rx) = mpsc::channel();
            let (recycle_tx, recycle_rx) = mpsc::channel();
            let reader = Reader::new(data, opts);
            let mut out = Outputs {
                done: done_tx.clone(),
                work: None,
                decoders,
            };
            if decoders > 0 {
                let (work_tx, work_rx) = mpsc::channel();
                let work_rx = Arc::new(Mutex::new(work_rx));
                for _ in 0..decoders {
                    let (rx, done) = (work_rx.clone(), done_tx.clone());
                    s.spawn(move || decode_batches(&rx, queued, done));
                }
                out.work = Some((work_tx, queued));
            }
            s.spawn(move || reader.run(stop, out, recycle_rx, budget));
            drop(done_tx);
            let outcome = self.consume(visitor, done_rx, recycle_tx);
            stop.store(true, Ordering::Relaxed);
            outcome
        });
        let stopped = match outcome? {
            Outcome::Finished { stopped } => stopped,
            Outcome::Sequential { offset } => self.run_frames(offset, visitor)?,
        };
        Ok(self.finish(stopped, visitor))
    }

    /// Runs the items of the batches in order.
    fn consume<V: Visitor + ?Sized>(
        &mut self,
        visitor: &mut V,
        batches: Receiver<Batch>,
        recycle: Sender<Batch>,
    ) -> Result<Outcome> {
        let mut next = 0;
        let mut early = BTreeMap::new();
        loop {
            let mut batch = match early.remove(&next) {
                Some(b) => b,
                None => match batches.recv() {
                    Ok(b) if b.seq != next => {
                        early.insert(b.seq, b);
                        continue;
                    }
                    Ok(b) => b,
                    // The reader is done and every batch was run.
                    Err(_) => return Ok(Outcome::Finished { stopped: false }),
                },
            };
            next += 1;
            if let Some(panic) = batch.panic.take() {
                std::panic::resume_unwind(panic);
            }
            let Batch {
                items, bytes, jobs, ..
            } = &mut batch;
            for item in items.drain(..) {
                match item {
                    Item::Frame { tick, stop } => {
                        if self.begin_frame(tick, visitor) == Flow::Stopped {
                            return Ok(Outcome::Finished { stopped: true });
                        }
                        if stop {
                            return Ok(Outcome::Finished { stopped: false });
                        }
                    }
                    Item::SendTables(st) => self.ctx.set_send_tables(*st),
                    Item::Classes(classes) => self.ctx.set_classes(classes),
                    Item::Message { msg_type, start, end } => {
                        self.handle_message(msg_type, &bytes[start..end], visitor)?
                    }
                    Item::Entities(i) => self.apply_entities(&mut jobs[i], visitor)?,
                    Item::Sequential { offset } => return Ok(Outcome::Sequential { offset }),
                    Item::Error(e) => return Err(e),
                }
            }
            let _ = recycle.send(batch);
        }
    }
}

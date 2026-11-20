//! The streaming parser: walks frames, decodes net messages and maintains the
//! game state ([`Context`]) while calling back into a user [`Visitor`].

use crate::bitreader::BitReader;
use crate::container::{DemoHeader, Frame, FrameIter, read_header};
use crate::entities::{
    Class, DecodeEnv, Entity, EntityDecoder, EntityDiagnostics, EntityJob, EntityOp, EntityStore, FieldPath,
    NO_CLASS, PacketEntitiesCursor,
};
use crate::error::{Error, Result, decode_proto};
use crate::events::{EventDescriptors, EventValue, GameEvent};
use crate::proto::{
    CDemoClassInfo, CMsgSource1LegacyGameEvent, CMsgSource1LegacyGameEventList, CsvcMsgCreateStringTable,
    CsvcMsgPacketEntities, CsvcMsgServerInfo, CsvcMsgUpdateStringTable, EDemoCommands,
};
use crate::sendtables::SendTables;
use crate::stringtables::StringTables;
use std::ops::ControlFlow;
use std::sync::Arc;

// Net message ids (NET_Messages / SVC_Messages / EBaseGameEvents).
const NET_TICK: u32 = 4;
pub(crate) const SVC_SERVER_INFO: u32 = 40;
const SVC_CREATE_STRING_TABLE: u32 = 44;
const SVC_UPDATE_STRING_TABLE: u32 = 45;
const SVC_CLEAR_ALL_STRING_TABLES: u32 = 51;
pub(crate) const SVC_PACKET_ENTITIES: u32 = 55;
const GE_SOURCE1_LEGACY_GAME_EVENT_LIST: u32 = 205;
const GE_SOURCE1_LEGACY_GAME_EVENT: u32 = 207;

/// What to decode. Skipping entities makes event-only analysis several times faster.
#[derive(Debug, Clone)]
pub struct ParseOptions {
    /// Decode `svc_PacketEntities` (positions, health, money, ...). Default: true.
    pub entities: bool,
    /// Decode game events. Default: true.
    pub game_events: bool,
    /// Deliver net messages the parser does not use itself (user messages, temp
    /// entities, user commands, ...) to [`Visitor::on_net_message`]. Default:
    /// false, which lets the parser skip them without copying.
    pub raw_net_messages: bool,
    /// Stop after this tick (inclusive).
    pub end_tick: Option<i32>,
    /// Threads a parse with entities may use, the calling thread included.
    /// With more than one, a reader thread walks the frames and other threads
    /// decode entity data ahead, while the calling thread applies it and runs
    /// the visitor, in demo order: the callbacks are the same as with one
    /// thread. `0` (the default) uses up to [`ParseOptions::AUTO_THREADS`],
    /// fewer on smaller machines; `1` parses everything on the calling thread.
    pub threads: usize,
    /// Entity data the parse may leave out because the visitor doesn't use
    /// it, to go faster. Default: nothing.
    pub skip: EntitySkip,
}

/// Entity data a parse may leave out (see [`ParseOptions::skip`]).
///
/// * Entities of a skipped class are still created, leave and are deleted
///   (with their baseline properties), but their updates aren't decoded or
///   reported to [`Visitor::on_entity`].
/// * Skipped properties aren't stored, and aren't listed in the changed paths
///   given to [`Visitor::on_entity`]. An update that changes nothing else
///   isn't reported.
///
/// Skipping is a hint: demos from builds that don't give each entity's data
/// length (older than those tested) are decoded in full.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntitySkip {
    /// Class names, e.g. `CChicken`.
    pub classes: Vec<String>,
    /// Dotted property names, e.g. `m_nTickBase` or
    /// `CBodyComponent.m_SerializePoseRecipeAG2Dynamic`, skipped in every
    /// class that has them. Skipping an array, vector or object skips all of it.
    pub props: Vec<String>,
}

impl EntitySkip {
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty() && self.props.is_empty()
    }
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions {
            entities: true,
            game_events: true,
            raw_net_messages: false,
            end_tick: None,
            threads: 0,
            skip: EntitySkip::default(),
        }
    }
}

impl ParseOptions {
    /// Most threads used when [`ParseOptions::threads`] is `0`. Past this,
    /// the calling thread (applying entities and running the visitor) is the
    /// bottleneck, and more decoding threads don't help.
    pub const AUTO_THREADS: usize = 8;

    pub fn events_only() -> Self {
        ParseOptions {
            entities: false,
            ..Default::default()
        }
    }

    /// Threads this parse will use, the calling thread included.
    pub(crate) fn thread_count(&self) -> usize {
        if !self.entities {
            return 1;
        }
        match self.threads {
            0 => std::thread::available_parallelism()
                .map_or(1, |n| n.get())
                .min(Self::AUTO_THREADS),
            n => n,
        }
    }
}

/// Callbacks invoked during parsing. Every method has an empty default.
/// Return `ControlFlow::Break(())` from [`Visitor::on_tick_end`] to stop early.
#[allow(unused_variables)]
pub trait Visitor {
    /// Once, before the first frame.
    fn on_start(&mut self, ctx: &Context) {}

    fn on_game_event(&mut self, ctx: &Context, event: &GameEvent) {}

    /// An entity was created / updated / left the PVS / is about to be deleted.
    /// `changed` lists the field paths written by this update (empty for Left/Deleted);
    /// resolve them with `entity.class().serializer.name_for(path.as_slice())`.
    fn on_entity(&mut self, ctx: &Context, entity: &Entity, op: EntityOp, changed: &[FieldPath]) {}

    /// A string table was created or updated. `entries` are the touched indices.
    fn on_string_table(&mut self, ctx: &Context, table: &str, entries: &[usize]) {}

    /// Any net message the parser does not consume itself (user messages,
    /// temp entities, ...). `data` is the protobuf payload.
    /// Only called when [`ParseOptions::raw_net_messages`] is set.
    fn on_net_message(&mut self, ctx: &Context, msg_type: u32, data: &[u8]) {}

    /// After all frames of a tick were processed.
    fn on_tick_end(&mut self, ctx: &Context) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    /// Once, after the last frame.
    fn on_finish(&mut self, ctx: &Context) {}
}

impl Visitor for () {}

/// Runs two visitors in one parse: every callback goes to `A`, then to `B`.
/// Nest tuples for more: `(a, (b, c))`. The parse stops early as soon as
/// either visitor's [`Visitor::on_tick_end`] breaks.
///
/// ```no_run
/// # use cs2_demo::{Demo, ParseOptions, StatsCollector, Context, Visitor};
/// # struct MyAnalysis;
/// # impl Visitor for MyAnalysis {}
/// let demo = Demo::open("match.dem")?;
/// let mut both = (StatsCollector::new(), MyAnalysis);
/// let ctx = demo.parse(ParseOptions::default(), &mut both)?;
/// let stats = both.0.into_stats(&ctx);
/// # Ok::<(), cs2_demo::Error>(())
/// ```
impl<A: Visitor, B: Visitor> Visitor for (A, B) {
    fn on_start(&mut self, ctx: &Context) {
        self.0.on_start(ctx);
        self.1.on_start(ctx);
    }

    fn on_game_event(&mut self, ctx: &Context, event: &GameEvent) {
        self.0.on_game_event(ctx, event);
        self.1.on_game_event(ctx, event);
    }

    fn on_entity(&mut self, ctx: &Context, entity: &Entity, op: EntityOp, changed: &[FieldPath]) {
        self.0.on_entity(ctx, entity, op, changed);
        self.1.on_entity(ctx, entity, op, changed);
    }

    fn on_string_table(&mut self, ctx: &Context, table: &str, entries: &[usize]) {
        self.0.on_string_table(ctx, table, entries);
        self.1.on_string_table(ctx, table, entries);
    }

    fn on_net_message(&mut self, ctx: &Context, msg_type: u32, data: &[u8]) {
        self.0.on_net_message(ctx, msg_type, data);
        self.1.on_net_message(ctx, msg_type, data);
    }

    fn on_tick_end(&mut self, ctx: &Context) -> ControlFlow<()> {
        let a = self.0.on_tick_end(ctx);
        let b = self.1.on_tick_end(ctx);
        if a.is_break() || b.is_break() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn on_finish(&mut self, ctx: &Context) {
        self.0.on_finish(ctx);
        self.1.on_finish(ctx);
    }
}

/// Game state at the current point of the parse.
pub struct Context {
    header: DemoHeader,
    tick: i32,
    server_info: Option<CsvcMsgServerInfo>,
    class_id_bits: u32,
    classes: Vec<Option<Arc<Class>>>,
    pending_class_info: Option<CDemoClassInfo>,
    send_tables: Option<SendTables>,
    string_tables: StringTables,
    entities: EntityStore,
    entity_dec: EntityDecoder,
    events: EventDescriptors,
    game_rules_idx: Option<usize>,
    team_idx: Vec<usize>,
    rounds: RoundSynth,
}

/// Recent CS2 demos no longer contain `round_start` / `round_end` events; they are
/// re-created from the game rules' round counters (see `emit_synthetic_round_events`).
#[derive(Default)]
struct RoundSynth {
    /// A real round_start/round_end event was seen: never synthesize.
    real_events: bool,
    last_start_count: Option<u32>,
    last_end_count: Option<u32>,
}

impl Context {
    fn new(header: DemoHeader) -> Self {
        Context {
            header,
            tick: -1,
            server_info: None,
            class_id_bits: 0,
            classes: Vec::new(),
            pending_class_info: None,
            send_tables: None,
            string_tables: StringTables::default(),
            entities: EntityStore::default(),
            entity_dec: EntityDecoder::default(),
            events: EventDescriptors::default(),
            game_rules_idx: None,
            team_idx: Vec::new(),
            rounds: RoundSynth::default(),
        }
    }

    pub fn header(&self) -> &DemoHeader {
        &self.header
    }

    /// Current game tick (-1 during signon).
    pub fn tick(&self) -> i32 {
        self.tick
    }

    pub fn tick_rate(&self) -> f32 {
        self.server_info
            .as_ref()
            .and_then(|s| s.tick_interval)
            .filter(|&t| t > 0.0)
            .map(|t| (1.0 / t).round())
            .unwrap_or_else(|| self.header.tick_rate())
    }

    pub fn server_info(&self) -> Option<&CsvcMsgServerInfo> {
        self.server_info.as_ref()
    }

    pub fn entities(&self) -> &EntityStore {
        &self.entities
    }

    pub fn entity(&self, index: usize) -> Option<&Entity> {
        self.entities.get(index)
    }

    pub fn string_tables(&self) -> &StringTables {
        &self.string_tables
    }

    pub fn send_tables(&self) -> Option<&SendTables> {
        self.send_tables.as_ref()
    }

    pub fn classes(&self) -> impl Iterator<Item = &Arc<Class>> {
        self.classes.iter().filter_map(Option::as_ref)
    }

    pub fn class_by_name(&self, name: &str) -> Option<&Arc<Class>> {
        self.classes().find(|c| &*c.name == name)
    }

    pub fn event_descriptors(&self) -> &EventDescriptors {
        &self.events
    }

    /// Entity updates that could not be decoded exactly (should be empty; a
    /// non-zero count means this build uses an encoding the parser does not know).
    pub fn entity_diagnostics(&self) -> &EntityDiagnostics {
        &self.entity_dec.diagnostics
    }

    pub(crate) fn game_rules_index(&self) -> Option<usize> {
        self.game_rules_idx
    }

    pub(crate) fn team_indices(&self) -> &[usize] {
        &self.team_idx
    }

    fn build_classes(&mut self, skip: &EntitySkip) -> Result<()> {
        let (Some(info), Some(st)) = (&self.pending_class_info, &self.send_tables) else {
            return Ok(());
        };
        self.classes = build_classes(info, st, skip)?;
        self.pending_class_info = None;
        Ok(())
    }

    pub(crate) fn set_send_tables(&mut self, st: SendTables) {
        self.send_tables = Some(st);
    }

    pub(crate) fn set_classes(&mut self, classes: Vec<Option<Arc<Class>>>) {
        self.classes = classes;
        self.pending_class_info = None;
    }

    pub(crate) fn coord_params(&self) -> crate::bitreader::CoordParams {
        self.send_tables
            .as_ref()
            .map(|s| s.coord_params)
            .unwrap_or_default()
    }
}

/// The classes announced by `DEM_ClassInfo`, with their serializers.
pub(crate) fn build_classes(
    info: &CDemoClassInfo,
    st: &SendTables,
    skip: &EntitySkip,
) -> Result<Vec<Option<Arc<Class>>>> {
    let mut classes = Vec::new();
    for c in &info.classes {
        let id = c.class_id.unwrap_or(-1);
        let name = c.network_name.as_deref().unwrap_or("");
        let Some(ser) = st.get(name) else {
            return Err(Error::malformed(format!("class {name} ({id}) has no serializer")));
        };
        let id = usize::try_from(id).map_err(|_| Error::malformed("negative class id"))?;
        if classes.len() <= id {
            classes.resize(id + 1, None);
        }
        let mut class = Class::new(id as u32, ser.name.clone(), ser.clone());
        class.set_skip(skip);
        classes[id] = Some(Arc::new(class));
    }
    Ok(classes)
}

/// Streaming demo parser over an in-memory (usually memory-mapped) demo.
pub struct DemoParser<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) opts: ParseOptions,
    pub(crate) ctx: Context,
    frame_buf: Vec<u8>,
    msg_buf: Vec<u8>,
    job: EntityJob,
}

/// What the frame loop does after a frame's tick was handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    Continue,
    /// The visitor or `end_tick` stopped the parse before this frame.
    Stopped,
}

impl<'a> DemoParser<'a> {
    pub fn new(data: &'a [u8], opts: ParseOptions) -> Result<Self> {
        Ok(Self::with_header(data, opts, read_header(data)?))
    }

    pub(crate) fn with_header(data: &'a [u8], opts: ParseOptions, header: DemoHeader) -> Self {
        DemoParser {
            data,
            opts,
            ctx: Context::new(header),
            frame_buf: Vec::new(),
            msg_buf: Vec::new(),
            job: EntityJob::default(),
        }
    }

    pub fn context(&self) -> &Context {
        &self.ctx
    }

    /// Parses the whole demo, returning the final game state. Uses the
    /// threads allowed by [`ParseOptions::threads`].
    pub fn run<V: Visitor + ?Sized>(mut self, visitor: &mut V) -> Result<Context> {
        let threads = self.opts.thread_count();
        if threads > 1 {
            return self.run_parallel(visitor, threads);
        }
        visitor.on_start(&self.ctx);
        let stopped = self.run_frames(16, visitor)?;
        Ok(self.finish(stopped, visitor))
    }

    /// Runs the frames from byte offset `pos` to the end on this thread.
    /// Returns whether the parse was stopped early.
    pub(crate) fn run_frames<V: Visitor + ?Sized>(&mut self, pos: usize, visitor: &mut V) -> Result<bool> {
        for frame in FrameIter::at(self.data, pos) {
            let frame = frame?;
            if self.begin_frame(frame.tick, visitor) == Flow::Stopped {
                return Ok(true);
            }
            if frame.cmd == EDemoCommands::DemStop as u32 {
                break;
            }
            self.handle_frame(&frame, visitor)?;
        }
        Ok(false)
    }

    /// Ends the previous tick when a frame starts a new one.
    pub(crate) fn begin_frame<V: Visitor + ?Sized>(&mut self, tick: i32, visitor: &mut V) -> Flow {
        if tick != self.ctx.tick && tick >= 0 {
            if self.ctx.tick >= 0 {
                self.emit_synthetic_round_events(visitor);
            }
            if self.ctx.tick >= 0 && visitor.on_tick_end(&self.ctx).is_break() {
                return Flow::Stopped;
            }
            if self.opts.end_tick.is_some_and(|end| tick > end) {
                return Flow::Stopped;
            }
            self.ctx.tick = tick;
        }
        Flow::Continue
    }

    /// Ends the last tick (unless stopped) and the parse.
    pub(crate) fn finish<V: Visitor + ?Sized>(mut self, stopped: bool, visitor: &mut V) -> Context {
        if !stopped && self.ctx.tick >= 0 {
            self.emit_synthetic_round_events(visitor);
            let _ = visitor.on_tick_end(&self.ctx);
        }
        visitor.on_finish(&self.ctx);
        self.ctx
    }

    fn handle_frame<V: Visitor + ?Sized>(&mut self, frame: &Frame, visitor: &mut V) -> Result<()> {
        let Some(cmd) = frame.command() else { return Ok(()) };
        match cmd {
            EDemoCommands::DemPacket | EDemoCommands::DemSignonPacket => {
                let mut buf = std::mem::take(&mut self.frame_buf);
                let result = frame
                    .data(&mut buf)
                    .and_then(|payload| self.handle_demo_packet(payload, visitor));
                self.frame_buf = buf;
                result
            }
            EDemoCommands::DemSendTables => {
                let mut buf = Vec::new();
                let st = SendTables::from_demo_send_tables(frame.data(&mut buf)?)?;
                self.ctx.send_tables = Some(st);
                self.ctx.build_classes(&self.opts.skip)
            }
            EDemoCommands::DemClassInfo => {
                let mut buf = Vec::new();
                let info: CDemoClassInfo = decode_proto("CDemoClassInfo", frame.data(&mut buf)?)?;
                self.ctx.pending_class_info = Some(info);
                self.ctx.build_classes(&self.opts.skip)
            }
            // Full packets are keyframes (complete snapshots) used for seeking;
            // the sequential delta stream already contains the same state.
            _ => Ok(()),
        }
    }

    fn handle_demo_packet<V: Visitor + ?Sized>(&mut self, payload: &[u8], visitor: &mut V) -> Result<()> {
        let mut msg = std::mem::take(&mut self.msg_buf);
        let wants = Wants::new(&self.opts);
        let result = for_each_message(payload, wants, &mut msg, |msg_type, data| {
            self.handle_message(msg_type, data, visitor)?;
            Ok(ControlFlow::Continue(()))
        });
        self.msg_buf = msg;
        result.map(|_| ())
    }

    pub(crate) fn handle_message<V: Visitor + ?Sized>(
        &mut self,
        msg_type: u32,
        data: &[u8],
        visitor: &mut V,
    ) -> Result<()> {
        match msg_type {
            NET_TICK => {}
            SVC_SERVER_INFO => {
                let info: CsvcMsgServerInfo = decode_proto("CSVCMsg_ServerInfo", data)?;
                let max_classes = info.max_classes.unwrap_or(0).max(1) as f64;
                self.ctx.class_id_bits = max_classes.log2() as u32 + 1;
                self.ctx.server_info = Some(info);
            }
            SVC_CREATE_STRING_TABLE => {
                let m: CsvcMsgCreateStringTable = decode_proto("CSVCMsg_CreateStringTable", data)?;
                let (id, touched) = self.ctx.string_tables.create(&m)?;
                self.after_string_table(id, &touched, visitor);
            }
            SVC_UPDATE_STRING_TABLE => {
                let m: CsvcMsgUpdateStringTable = decode_proto("CSVCMsg_UpdateStringTable", data)?;
                let (id, touched) = self.ctx.string_tables.update(&m)?;
                self.after_string_table(id, &touched, visitor);
            }
            SVC_CLEAR_ALL_STRING_TABLES => {
                self.ctx.string_tables.clear();
                self.ctx.entity_dec.invalidate_baselines();
            }
            SVC_PACKET_ENTITIES => {
                if self.opts.entities {
                    let m: CsvcMsgPacketEntities = decode_proto("CSVCMsg_PacketEntities", data)?;
                    self.handle_packet_entities(m, visitor)?;
                }
            }
            GE_SOURCE1_LEGACY_GAME_EVENT_LIST => {
                let list: CMsgSource1LegacyGameEventList =
                    decode_proto("CMsgSource1LegacyGameEventList", data)?;
                self.ctx.events.load(&list);
            }
            GE_SOURCE1_LEGACY_GAME_EVENT => {
                if self.opts.game_events {
                    let m: CMsgSource1LegacyGameEvent = decode_proto("CMsgSource1LegacyGameEvent", data)?;
                    if let Some(ev) = self.ctx.events.decode(m, self.ctx.tick) {
                        if matches!(&*ev.name, "round_start" | "round_end") {
                            self.ctx.rounds.real_events = true;
                        }
                        visitor.on_game_event(&self.ctx, &ev);
                    }
                }
            }
            _ => visitor.on_net_message(&self.ctx, msg_type, data),
        }
        Ok(())
    }

    fn after_string_table<V: Visitor + ?Sized>(&mut self, id: usize, touched: &[usize], visitor: &mut V) {
        if let Some(t) = self.ctx.string_tables.by_id(id) {
            if t.name == "instancebaseline" {
                self.ctx.entity_dec.invalidate_baselines();
            }
            visitor.on_string_table(&self.ctx, &t.name, touched);
        }
    }

    /// Emits `round_end` / `round_start` events derived from the game rules'
    /// `m_nRoundEndCount` / `m_nRoundStartCount` counters, for demos that do not
    /// contain the real events. Synthetic events carry `synthetic = true`.
    fn emit_synthetic_round_events<V: Visitor + ?Sized>(&mut self, visitor: &mut V) {
        if self.ctx.rounds.real_events {
            return;
        }
        let Some(gr) = self.ctx.game_rules() else { return };
        let (start, end) = (gr.round_start_count(), gr.round_end_count());
        let rounds = &self.ctx.rounds;
        let end_changed = rounds.last_end_count.is_some_and(|c| c != end);
        let start_changed = rounds.last_start_count.is_some_and(|c| c != start);
        let tick = self.ctx.tick;
        let mut events = Vec::new();
        if end_changed {
            let g = |k: &str| gr.entity().get::<i32>(&format!("m_pGameRules.{k}")).unwrap_or(0);
            events.push(GameEvent {
                tick,
                name: Arc::from("round_end"),
                fields: vec![
                    (Arc::from("winner"), EventValue::Int(g("m_iRoundEndWinnerTeam"))),
                    (Arc::from("reason"), EventValue::Int(g("m_eRoundEndReason"))),
                    (
                        Arc::from("message"),
                        EventValue::String(
                            gr.entity()
                                .get::<String>("m_pGameRules.m_sRoundEndMessage")
                                .unwrap_or_default(),
                        ),
                    ),
                    (Arc::from("legacy"), EventValue::Int(g("m_iRoundEndLegacy"))),
                    (
                        Arc::from("player_count"),
                        EventValue::Int(g("m_iRoundEndPlayerCount")),
                    ),
                    (Arc::from("synthetic"), EventValue::Bool(true)),
                ],
            });
        }
        if start_changed {
            let g = |k: &str| gr.entity().get::<i32>(&format!("m_pGameRules.{k}")).unwrap_or(0);
            events.push(GameEvent {
                tick,
                name: Arc::from("round_start"),
                fields: vec![
                    (Arc::from("timelimit"), EventValue::Int(g("m_iRoundTime"))),
                    (Arc::from("round"), EventValue::Int(g("m_iRoundStartRoundNumber"))),
                    (Arc::from("synthetic"), EventValue::Bool(true)),
                ],
            });
        }
        self.ctx.rounds.last_start_count = Some(start);
        self.ctx.rounds.last_end_count = Some(end);
        for ev in &events {
            visitor.on_game_event(&self.ctx, ev);
        }
    }

    fn handle_packet_entities<V: Visitor + ?Sized>(
        &mut self,
        mut m: CsvcMsgPacketEntities,
        visitor: &mut V,
    ) -> Result<()> {
        if self.ctx.classes.is_empty() {
            return Err(Error::malformed(
                "packet entities before class info / send tables",
            ));
        }
        let mut job = std::mem::take(&mut self.job);
        let entities = &self.ctx.entities;
        let scanned = job.scan(
            m.entity_data.as_deref().unwrap_or(&[]),
            m.serialized_entities.as_deref(),
            m.updated_entries.unwrap_or(0),
            m.has_pvs_vis_bits_deprecated.unwrap_or(0) > 0,
            self.ctx.class_id_bits,
            &self.ctx.classes,
            &|idx| entities.get(idx).map_or(NO_CLASS, |e| e.class().id),
        );
        let result = if scanned {
            job.set_data(m.entity_data.take().unwrap_or_default());
            let cp = self.ctx.coord_params();
            job.decode(&self.ctx.classes, &cp);
            self.apply_entities(&mut job, visitor)
        } else {
            self.handle_packet_entities_unsized(&m, visitor)
        };
        self.job = job;
        result
    }

    /// Applies a scanned and decoded `svc_PacketEntities` message.
    pub(crate) fn apply_entities<V: Visitor + ?Sized>(
        &mut self,
        job: &mut EntityJob,
        visitor: &mut V,
    ) -> Result<()> {
        let mut cursor = job.cursor();
        // String tables and classes don't change within a message.
        let env = DecodeEnv {
            class_id_bits: self.ctx.class_id_bits,
            classes: &self.ctx.classes,
            coord_params: self.ctx.coord_params(),
            baselines: self.ctx.string_tables.get("instancebaseline"),
        };
        while let Some((idx, op, range)) =
            cursor.next(&mut self.ctx.entities, &mut self.ctx.entity_dec, &env)?
        {
            track_entity(
                &self.ctx.entities,
                &mut self.ctx.game_rules_idx,
                &mut self.ctx.team_idx,
                idx,
                op,
            );
            let ctx = &self.ctx;
            if let Some(entity) = ctx.entities.get(idx) {
                visitor.on_entity(ctx, entity, op, cursor.paths(range));
            }
        }
        cursor.finish(&mut self.ctx.entities)
    }

    /// Decodes and applies a message that doesn't give the bit length of
    /// each entity's fields (older builds), one entity at a time.
    fn handle_packet_entities_unsized<V: Visitor + ?Sized>(
        &mut self,
        m: &CsvcMsgPacketEntities,
        visitor: &mut V,
    ) -> Result<()> {
        // Recent builds announce the bit length of every entity's field data.
        let sizes = match m.serialized_entities.as_deref() {
            Some(raw) if !raw.is_empty() => {
                let mut sizes = std::mem::take(&mut self.ctx.entity_dec.sizes);
                sizes.clear();
                let mut pos = 0;
                while pos < raw.len() {
                    sizes.push(read_varint(raw, &mut pos)? as u32);
                }
                Some(sizes)
            }
            _ => None,
        };

        let mut cursor = PacketEntitiesCursor::new(
            m.entity_data.as_deref().unwrap_or(&[]),
            m.updated_entries.unwrap_or(0),
            m.has_pvs_vis_bits_deprecated.unwrap_or(0) > 0,
            sizes,
        );
        loop {
            let step = {
                let Context {
                    classes,
                    send_tables,
                    string_tables,
                    entities,
                    entity_dec,
                    class_id_bits,
                    ..
                } = &mut self.ctx;
                let env = DecodeEnv {
                    class_id_bits: *class_id_bits,
                    classes,
                    coord_params: send_tables.as_ref().map(|s| s.coord_params).unwrap_or_default(),
                    baselines: string_tables.get("instancebaseline"),
                };
                cursor.next(entities, entity_dec, &env)?
            };
            let Some((idx, op)) = step else { break };
            track_entity(
                &self.ctx.entities,
                &mut self.ctx.game_rules_idx,
                &mut self.ctx.team_idx,
                idx,
                op,
            );
            let ctx = &self.ctx;
            if let Some(entity) = ctx.entities.get(idx) {
                visitor.on_entity(ctx, entity, op, ctx.entity_dec.paths());
            }
        }
        if let Some(sizes) = cursor.finish(&mut self.ctx.entities)? {
            self.ctx.entity_dec.sizes = sizes;
        }
        Ok(())
    }
}

/// Follows the entities the context finds by index (game rules, teams).
fn track_entity(
    entities: &EntityStore,
    game_rules_idx: &mut Option<usize>,
    team_idx: &mut Vec<usize>,
    idx: usize,
    op: EntityOp,
) {
    if !matches!(op, EntityOp::Created | EntityOp::Deleted) {
        return;
    }
    let Some(entity) = entities.get(idx) else {
        return;
    };
    match (entity.class_name(), op) {
        ("CCSGameRulesProxy", EntityOp::Created) => *game_rules_idx = Some(idx),
        ("CCSGameRulesProxy", EntityOp::Deleted) => *game_rules_idx = None,
        ("CCSTeam", EntityOp::Created) if !team_idx.contains(&idx) => team_idx.push(idx),
        ("CCSTeam", EntityOp::Deleted) => team_idx.retain(|&i| i != idx),
        _ => {}
    }
}

/// The net messages a parse needs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wants {
    entities: bool,
    game_events: bool,
    raw_net_messages: bool,
}

impl Wants {
    pub fn new(opts: &ParseOptions) -> Self {
        Wants {
            entities: opts.entities,
            game_events: opts.game_events,
            raw_net_messages: opts.raw_net_messages,
        }
    }

    /// Whether a message's payload is needed at all (others are skipped uncopied).
    fn message(self, msg_type: u32) -> bool {
        match msg_type {
            NET_TICK => false,
            SVC_SERVER_INFO
            | SVC_CREATE_STRING_TABLE
            | SVC_UPDATE_STRING_TABLE
            | SVC_CLEAR_ALL_STRING_TABLES
            | GE_SOURCE1_LEGACY_GAME_EVENT_LIST => true,
            SVC_PACKET_ENTITIES => self.entities,
            GE_SOURCE1_LEGACY_GAME_EVENT => self.game_events,
            _ => self.raw_net_messages,
        }
    }
}

/// Calls `f` with every net message of a `CDemoPacket` payload that `wants`
/// asks for (the others are skipped without being copied), until it breaks.
/// `CDemoPacket { bytes data = 3; }` is decoded by hand to avoid a copy.
pub(crate) fn for_each_message(
    payload: &[u8],
    wants: Wants,
    msg: &mut Vec<u8>,
    mut f: impl FnMut(u32, &[u8]) -> Result<ControlFlow<()>>,
) -> Result<ControlFlow<()>> {
    let mut pos = 0;
    while pos < payload.len() {
        let key = read_varint(payload, &mut pos)?;
        match key & 7 {
            0 => {
                read_varint(payload, &mut pos)?;
            }
            2 => {
                let len = read_varint(payload, &mut pos)? as usize;
                let body = payload
                    .get(pos..pos + len)
                    .ok_or(Error::UnexpectedEof("CDemoPacket"))?;
                pos += len;
                if key >> 3 == 3 {
                    let mut r = BitReader::new(body);
                    while r.remaining_bits() >= 8 {
                        let msg_type = r.read_ubitvar();
                        let size = r.read_varu32() as usize;
                        if size * 8 > r.remaining_bits() {
                            return Err(Error::UnexpectedEof("net message"));
                        }
                        if !wants.message(msg_type) {
                            r.skip_bits(size * 8);
                            continue;
                        }
                        msg.clear();
                        r.read_bytes_into(msg, size);
                        r.check_overflow("net message")?;
                        if f(msg_type, msg)?.is_break() {
                            return Ok(ControlFlow::Break(()));
                        }
                    }
                }
            }
            5 => pos += 4,
            1 => pos += 8,
            w => return Err(Error::malformed(format!("CDemoPacket: unexpected wire type {w}"))),
        }
    }
    Ok(ControlFlow::Continue(()))
}

pub(crate) fn read_varint(data: &[u8], pos: &mut usize) -> Result<u64> {
    let mut result = 0u64;
    let mut shift = 0;
    loop {
        let b = *data.get(*pos).ok_or(Error::UnexpectedEof("varint"))?;
        *pos += 1;
        result |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
        if shift >= 70 {
            return Err(Error::malformed("varint too long"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder {
        calls: Vec<&'static str>,
        stop: bool,
    }

    impl Visitor for Recorder {
        fn on_start(&mut self, _: &Context) {
            self.calls.push("start");
        }
        fn on_game_event(&mut self, _: &Context, _: &GameEvent) {
            self.calls.push("event");
        }
        fn on_string_table(&mut self, _: &Context, _: &str, _: &[usize]) {
            self.calls.push("table");
        }
        fn on_net_message(&mut self, _: &Context, _: u32, _: &[u8]) {
            self.calls.push("message");
        }
        fn on_tick_end(&mut self, _: &Context) -> ControlFlow<()> {
            self.calls.push("tick");
            if self.stop {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }
        fn on_finish(&mut self, _: &Context) {
            self.calls.push("finish");
        }
    }

    #[test]
    fn tuple_visitor_forwards_every_callback() {
        let ctx = Context::new(DemoHeader::default());
        let ev = GameEvent {
            tick: 0,
            name: Arc::from("x"),
            fields: Vec::new(),
        };
        let mut v = (
            Recorder::default(),
            Recorder {
                stop: true,
                ..Default::default()
            },
        );
        v.on_start(&ctx);
        v.on_game_event(&ctx, &ev);
        v.on_string_table(&ctx, "t", &[]);
        v.on_net_message(&ctx, 1, &[]);
        assert!(v.on_tick_end(&ctx).is_break(), "the second visitor breaks");
        v.on_finish(&ctx);
        let expected = ["start", "event", "table", "message", "tick", "finish"];
        assert_eq!(v.0.calls, expected);
        assert_eq!(v.1.calls, expected);
    }
}

//! The session engine: one task that owns the roaster connection, the safety
//! layer and the running roast. Everything else talks to it through
//! [`EngineHandle`], so there is exactly one place where state changes.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::inventory::InventoryStore;
use crate::learned::LearnedStore;
use crate::profiles::ProfileStore;
use roast_control::{
    Outlook, PreheatController, PreheatInput, PreheatPhase, PreheatReason, PreheatTuning,
    RecipeInput, RecipeRun, StepChange, StepState, StepView, Trim, TrimDecision, TrimInput,
    TrimTuning, trigger_label,
};
use roast_core::{
    Actuator, ActuatorValues, BatchInfo, CommandOutcome, CommandRequest, CommandSource,
    ControlLogEntry, EventDetector, EventKind, EventOrigin, Note, Phases, PlannedAction,
    Projection, ReferenceCurve, ReferenceKind, RejectReason, Roast, RoastEvent, RoastOrigin,
    RoastPlan, RoastState, RorConfig, RorTracker, Sample, phases, project,
};
use roast_driver::{RoasterDriver, SimulatedMachine, simulator_profile};
use roast_machine::MachineProfile;
use roast_safety::{Safety, SafetyConfig, SafetyReaction, SafetyStatus};
use roast_store::Store;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::driver_thread::{DriverCmd, DriverEvent, spawn_driver};
use crate::files::{self, Archive};

/// How long the curve is kept while not recording, seconds.
const PREVIEW_KEEP_S: f64 = 600.0;
/// Readings older than this count as stale, seconds.
const STALE_AFTER_S: f64 = 5.0;
/// Autosave interval of a running recording.
const AUTOSAVE_EVERY: Duration = Duration::from_secs(20);
/// Alarms kept for the snapshot.
const ALARMS_KEPT: usize = 20;

/// A message to the engine task.
pub enum Msg {
    /// Run a closure on the engine (API calls).
    Job(Box<dyn FnOnce(&mut Engine) + Send>),
    /// Something from the driver thread of generation `.0`.
    Driver(u64, DriverEvent),
    /// Periodic safety tick.
    Tick,
}

/// A cheap, cloneable handle to the engine.
#[derive(Clone)]
pub struct EngineHandle {
    tx: mpsc::UnboundedSender<Msg>,
    /// Pre-serialised WebSocket messages.
    pub updates: broadcast::Sender<Arc<str>>,
}

impl EngineHandle {
    /// Runs `f` on the engine and returns its result.
    pub async fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Engine) -> R + Send + 'static,
    ) -> anyhow::Result<R> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Msg::Job(Box::new(move |e| {
                let _ = tx.send(f(e));
            })))
            .map_err(|_| anyhow::anyhow!("the engine stopped"))?;
        Ok(rx.await?)
    }
}

/// Settings of the engine.
#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// The machine profile used for real connections.
    pub profile: MachineProfile,
    /// The database: roasts, inventory, profiles, settings (ADR 0003).
    pub store: Arc<Store>,
    /// Where the roasts' open file copies are written (ADR 0008).
    pub roasts_dir: PathBuf,
    /// Append-only action log (JSON lines).
    pub audit_path: PathBuf,
    /// `inventory.json` of the time before the database, taken over once.
    pub inventory_path: PathBuf,
    /// `control.json` of the time before the database, taken over once.
    pub control_path: PathBuf,
    /// `profiles.json` of the time before the database, taken over once.
    pub profiles_path: PathBuf,
    /// Simulator speed factor.
    pub sim_speed: f64,
}

/// An API failure.
#[derive(Debug, Clone, Serialize)]
pub struct Fail {
    /// HTTP status.
    #[serde(skip)]
    pub status: u16,
    /// Message for people.
    pub error: String,
    /// Safety reason, if a rule refused it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<RejectReason>,
}

impl Fail {
    pub(crate) fn bad(msg: impl Into<String>) -> Self {
        Fail {
            status: 400,
            error: msg.into(),
            reason: None,
        }
    }
    pub(crate) fn conflict(msg: impl Into<String>, reason: Option<RejectReason>) -> Self {
        Fail {
            status: 409,
            error: msg.into(),
            reason,
        }
    }
    pub(crate) fn not_found(msg: impl Into<String>) -> Self {
        Fail {
            status: 404,
            error: msg.into(),
            reason: None,
        }
    }
    pub(crate) fn internal(msg: impl Into<String>) -> Self {
        Fail {
            status: 500,
            error: msg.into(),
            reason: None,
        }
    }
}

/// Static facts about the connected machine.
#[derive(Debug, Clone, Serialize)]
pub struct MachineInfo {
    id: String,
    name: String,
    simulated: bool,
    port: Option<String>,
    actuators: Vec<ActuatorSpec>,
    switches: Vec<Actuator>,
    has_et: bool,
    sampling_interval_s: f64,
    max_bt_c: f64,
    link_loss_tested: bool,
    /// What the machine does when the link breaks, for people; only once tested.
    link_lost: Option<String>,
    /// Actuators on which a person's command ends AI control (ADR 0019).
    takeover_actuators: Vec<Actuator>,
}

#[derive(Debug, Clone, Serialize)]
struct ActuatorSpec {
    actuator: Actuator,
    min: f64,
    max: f64,
    step: f64,
    /// How people read the value when not in percent (the drum in rpm).
    #[serde(skip_serializing_if = "Option::is_none")]
    display: Option<roast_machine::DisplayScale>,
}

/// Lease holder name of the preheat controller.
const PREHEAT_HOLDER: &str = "preheat";
/// The lease holder of the autopilot a person switched on in the app
/// (ADR 0020).
const AUTOPILOT_HOLDER: &str = "autopilot";
/// Ambient temperature when the machine reports none, °C.
const DEFAULT_AMBIENT_C: f64 = 22.0;

/// A running preheat: brings the bean probe to a target and holds it with
/// the step-wait-learn controller (ADR 0015). It is an autopilot: its
/// commands pass the safety layer with the AI rules (lease, presence,
/// bounded raises), and anything a person does ends it.
#[derive(Debug, Clone)]
struct Preheat {
    ctl: PreheatController,
    /// Machine and air setting its learning is filed under.
    machine_key: String,
    air: f64,
    /// Recording time of the reading it last looked at.
    last_seen_t: Option<f64>,
    last_raise: Option<f64>,
    /// The AI that took over before CHARGE; the preheat holds under its
    /// lease until CHARGE (ADR 0019).
    ai_holder: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PreheatStatus {
    target_c: f64,
    state: &'static str,
}

/// One point of the live curve.
#[derive(Debug, Clone, Serialize)]
pub struct Point {
    t: f64,
    bt: Option<f64>,
    et: Option<f64>,
    ror: Option<f64>,
    actuators: ActuatorValues,
}

/// An alarm or notice for people.
#[derive(Debug, Clone, Serialize)]
pub struct Alarm {
    id: u64,
    at: DateTime<Utc>,
    severity: &'static str,
    kind: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct RecordingInfo {
    id: uuid::Uuid,
    title: String,
    started_at: DateTime<Utc>,
    green_weight_g: Option<f64>,
    beans: Option<String>,
    lot_id: Option<uuid::Uuid>,
    plan: Option<RoastPlan>,
    /// The batch number for the label (not in the simulator).
    batch_no: Option<String>,
}

/// The state shown by the UI and the AI.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    version: &'static str,
    machine: Option<MachineInfo>,
    connection: &'static str,
    connection_error: Option<String>,
    state: RoastState,
    recording: Option<RecordingInfo>,
    t: Option<f64>,
    charge_t: Option<f64>,
    bt: Option<f64>,
    et: Option<f64>,
    ror: Option<f64>,
    events: Vec<RoastEvent>,
    phases: Option<Phases>,
    safety: SafetyStatus,
    alarms: Vec<Alarm>,
    /// Number of control log entries of the running recording; the UI reloads
    /// the log when it changes, so actions show up before the next sample.
    control_seq: usize,
    reference: Option<ReferenceStatus>,
    projection: Option<Projection>,
    preheat: Option<PreheatStatus>,
    /// An AI that asked for control but needs a presence confirmation first;
    /// the app offers "I'm at the roaster" while this is set.
    control_request: Option<String>,
    /// The profile's recipe in this roast (ADR 0019).
    recipe: Option<RecipeStatus>,
    /// The autopilot, while it is switched on (ADR 0020).
    autopilot: Option<AutopilotStatus>,
}

/// The autopilot: it runs the recipe and keeps the roast on the plan's curve.
#[derive(Debug, Clone)]
struct Autopilot {
    trim: Trim,
    /// Roast clock of the trim's last correction, s.
    corrected_at: Option<f64>,
    corrections: usize,
}

/// The autopilot as the app shows it.
#[derive(Debug, Clone, Serialize)]
struct AutopilotStatus {
    /// `armed` (takes over at CHARGE), `active`, or `waiting` (for an AI's
    /// lease or a presence confirmation).
    state: &'static str,
    /// How far its corrections moved the burner away from the recipe, points.
    offset: f64,
    /// How many corrections it made.
    corrections: usize,
}

/// The recipe of the running roast.
#[derive(Debug, Clone, Serialize)]
struct RecipeStatus {
    /// The daemon sets the steps itself: an AI holds the lease.
    automatic: bool,
    /// Some step was changed for this roast.
    adjusted: bool,
    steps: Vec<StepView>,
}

/// How long an AI's refused control request keeps the presence prompt up.
const CONTROL_REQUEST_SHOWN_FOR: Duration = Duration::from_secs(120);

/// The reference curve at the current moment of the roast: an earlier
/// roast, or a profile's plan with its recipe.
#[derive(Debug, Clone, Serialize)]
struct ReferenceStatus {
    id: uuid::Uuid,
    title: String,
    kind: ReferenceKind,
    /// Its bean temperature at CHARGE (0:00), °C: where to preheat to.
    charge_bt: Option<f64>,
    /// Reference bean temperature at the same time since CHARGE.
    bt_now: Option<f64>,
    /// Live bean temperature minus `bt_now`.
    delta_bt: Option<f64>,
    ror_now: Option<f64>,
    /// Live rate of rise minus `ror_now`.
    delta_ror: Option<f64>,
    /// Reference events, `t` relative to its CHARGE.
    events: Vec<RoastEvent>,
    /// A profile's next recipe step the machine does not have yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    next_step: Option<NextStep>,
}

/// The next step of a profile's recipe.
#[derive(Debug, Clone, Serialize)]
struct NextStep {
    /// Roast time it is due, s since CHARGE; `None` while the event it
    /// follows has neither come nor is expected.
    at_s: Option<f64>,
    /// Seconds until then (negative: due since); `None` before CHARGE.
    in_s: Option<f64>,
    /// `at_s` is an estimate: the event or temperature has not come yet.
    projected: bool,
    /// "1:30", "FC +0:30", "185 °C".
    label: String,
    /// The daemon sets it itself (an AI holds the lease); otherwise it is a
    /// cue for the person.
    automatic: bool,
    /// What changes then, and to which value, %.
    set: Vec<StepValue>,
}

#[derive(Debug, Clone, Serialize)]
struct StepValue {
    /// The step's number in the recipe (for `adjust_recipe`).
    step: usize,
    actuator: Actuator,
    value: f64,
    /// The value now, %.
    current: Option<f64>,
}

/// The bean cooling goes on this long before the planned DROP, s.
const COOL_BEFORE_DROP_S: f64 = 30.0;
/// After the safety layer refused a recipe step, the runner tries again
/// this much later, s.
const RECIPE_RETRY_S: f64 = 5.0;

/// The curve so far.
#[derive(Debug, Clone, Serialize)]
pub struct Live {
    points: Vec<Point>,
    events: Vec<RoastEvent>,
    notes: Vec<Note>,
    control_log: Vec<ControlLogEntry>,
}

/// Body of `POST /api/connect`.
#[derive(Debug, Deserialize)]
pub struct ConnectReq {
    #[serde(default)]
    simulator: bool,
    port: Option<String>,
}

/// Body of `POST /api/recording/start` and `PATCH /api/recording`.
#[derive(Debug, Default, Deserialize)]
pub struct RecordingMeta {
    title: Option<String>,
    green_weight_g: Option<f64>,
    roasted_weight_g: Option<f64>,
    /// Free-text bean name, for a roast without a lot.
    beans: Option<String>,
    /// The green coffee lot roasted (ADR 0014); its stock is debited on save.
    lot_id: Option<uuid::Uuid>,
    /// Goal, reference roast and targets.
    plan: Option<RoastPlan>,
    /// The batch number for the label, e.g. `BRYB-002` (not in the simulator).
    batch_no: Option<String>,
}

/// Paths of a saved roast.
#[derive(Debug, Clone, Serialize)]
pub struct Saved {
    id: uuid::Uuid,
    json_path: PathBuf,
    alog_path: PathBuf,
}

/// Granted lease.
#[derive(Debug, Clone, Serialize)]
pub struct LeaseInfo {
    holder: String,
    expires_in_s: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Conn {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

/// The engine state. Only the engine task touches it.
pub struct Engine {
    opts: EngineOptions,
    tx: mpsc::UnboundedSender<Msg>,
    updates: broadcast::Sender<Arc<str>>,
    epoch: Instant,

    profile: Option<MachineProfile>,
    machine: Option<MachineInfo>,
    driver: Option<std_mpsc::Sender<DriverCmd>>,
    generation: u64,
    conn: Conn,
    conn_error: Option<String>,
    time_scale: f64,

    safety: Safety,
    zero: Instant,
    points: Vec<Point>,
    ror: RorTracker,
    detector: EventDetector,
    roast: Option<Roast>,
    state: RoastState,
    last_bt: Option<f64>,
    last_et: Option<f64>,
    last_ror: Option<f64>,
    last_ambient: Option<f64>,
    /// Recording time of the latest reading.
    last_reading_t: Option<f64>,
    last_reading: Option<Instant>,
    stale_alarmed: bool,
    /// The bean cooling went on ahead of DROP in this roast (once per roast).
    cooled_before_drop: bool,
    last_autosave: Instant,

    alarms: VecDeque<Alarm>,
    next_alarm: u64,
    audit: Option<File>,
    reference: Option<ReferenceCurve>,
    preheat: Option<Preheat>,
    /// What automatic control learned about each machine.
    learned: LearnedStore,
    /// Green coffee lots and their stock (ADR 0014).
    pub(crate) inventory: InventoryStore,
    /// Roast profiles and their versions.
    pub(crate) profiles: ProfileStore,
    /// Holder and time of a control request refused for a missing presence
    /// confirmation.
    control_request: Option<(String, Instant)>,
    /// The profile's recipe in this roast (ADR 0019).
    recipe: Option<RecipeRun>,
    /// The saved roasts.
    archive: Archive,
    /// The autopilot, while it is switched on (ADR 0020).
    autopilot: Option<Autopilot>,
    /// Roast clock of the last burner change by anyone, s.
    burner_changed_at: Option<f64>,
    /// How far the autopilot's corrections moved this roast's recipe,
    /// burner points; kept when it is switched off and on again.
    trim_offset: f64,
}

/// Starts the engine task and its safety ticker.
pub fn start(opts: EngineOptions) -> EngineHandle {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (updates, _) = broadcast::channel(256);
    let audit = files::open_append(&opts.audit_path).ok();
    let archive = Archive::new(Arc::clone(&opts.store), opts.roasts_dir.clone());
    match archive.sync() {
        Ok(0) => {}
        Ok(n) => tracing::info!("{n} roast files taken into the database"),
        Err(e) => tracing::error!("the roasts folder could not be read into the database: {e}"),
    }
    let inventory = InventoryStore::open(Arc::clone(&opts.store), &opts.inventory_path);
    let learned = LearnedStore::open(Arc::clone(&opts.store), &opts.control_path);
    let profiles = ProfileStore::open(Arc::clone(&opts.store), &opts.profiles_path);
    let mut engine = Engine {
        archive,
        safety: Safety::new(SafetyConfig::from_profile(&opts.profile)),
        opts,
        tx: tx.clone(),
        updates: updates.clone(),
        epoch: Instant::now(),
        profile: None,
        machine: None,
        driver: None,
        generation: 0,
        conn: Conn::Disconnected,
        conn_error: None,
        time_scale: 1.0,
        zero: Instant::now(),
        points: Vec::new(),
        ror: RorTracker::default(),
        detector: EventDetector::default(),
        roast: None,
        state: RoastState::Disconnected,
        last_bt: None,
        last_et: None,
        last_ror: None,
        last_ambient: None,
        last_reading_t: None,
        last_reading: None,
        stale_alarmed: false,
        cooled_before_drop: false,
        control_request: None,
        last_autosave: Instant::now(),
        alarms: VecDeque::new(),
        next_alarm: 1,
        audit,
        reference: None,
        preheat: None,
        recipe: None,
        autopilot: None,
        burner_changed_at: None,
        trim_offset: 0.0,
        learned,
        inventory,
        profiles,
    };
    if let Some(problem) = engine.inventory.problem().map(str::to_owned) {
        engine.alarm("critical", "inventory", problem);
    }
    engine.recover_autosave();

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                Msg::Job(f) => f(&mut engine),
                Msg::Driver(generation, ev) => {
                    if generation == engine.generation {
                        engine.on_driver(ev);
                    }
                }
                Msg::Tick => engine.on_tick(),
            }
        }
    });
    let ticker = tx.clone();
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(Duration::from_millis(500));
        loop {
            iv.tick().await;
            if ticker.send(Msg::Tick).is_err() {
                break;
            }
        }
    });
    EngineHandle { tx, updates }
}

impl Engine {
    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    fn t(&self) -> f64 {
        self.zero.elapsed().as_secs_f64() * self.time_scale
    }

    fn latest_t(&self) -> f64 {
        self.points.last().map_or_else(|| self.t(), |p| p.t)
    }

    // ----------------------------------------------------------------- output

    /// The current snapshot.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let now = self.now();
        let events = self
            .roast
            .as_ref()
            .map(|r| r.events.clone())
            .unwrap_or_default();
        let charge_t = events
            .iter()
            .find(|e| e.kind == EventKind::Charge)
            .map(|e| e.t);
        let t = self.points.last().map(|p| p.t);
        Snapshot {
            version: env!("CARGO_PKG_VERSION"),
            machine: self.machine.clone(),
            connection: match self.conn {
                Conn::Disconnected => "disconnected",
                Conn::Connecting => "connecting",
                Conn::Connected => "connected",
                Conn::Error => "error",
            },
            connection_error: self.conn_error.clone(),
            state: self.state,
            recording: self.roast.as_ref().map(|r| RecordingInfo {
                id: r.id,
                title: r.title.clone(),
                started_at: r.roasted_at,
                green_weight_g: r.batch.green_weight_g,
                beans: r.batch.beans.clone(),
                lot_id: r.green_lot_id,
                plan: r.plan.clone(),
                batch_no: r.batch.batch_no.clone(),
            }),
            t,
            charge_t,
            bt: self.last_bt,
            et: self.last_et,
            ror: self.last_ror,
            phases: self
                .roast
                .as_ref()
                .and_then(|r| phases(r, t.unwrap_or(0.0))),
            reference: self.reference_status(t, charge_t, now),
            recipe: self.recipe_status(now),
            autopilot: self.autopilot_status(now),
            projection: self.projection(&events, charge_t),
            control_request: self
                .control_request
                .as_ref()
                .filter(|(_, at)| at.elapsed() < CONTROL_REQUEST_SHOWN_FOR)
                .map(|(holder, _)| holder.clone()),
            preheat: self.preheat.as_ref().map(|p| PreheatStatus {
                target_c: p.ctl.target_c(),
                state: match p.ctl.phase() {
                    PreheatPhase::Heating => "heating",
                    PreheatPhase::Holding => "holding",
                },
            }),
            events,
            safety: self.safety.status(now),
            alarms: self.alarms.iter().cloned().collect(),
            control_seq: self.roast.as_ref().map_or(0, |r| r.control_log.len()),
        }
    }

    fn reference_status(
        &self,
        t: Option<f64>,
        charge_t: Option<f64>,
        now: f64,
    ) -> Option<ReferenceStatus> {
        let r = self.reference.as_ref()?;
        let since = t.zip(charge_t).map(|(t, c)| t - c);
        let (bt_now, ror_now) = since.map_or((None, None), |s| r.at(s));
        let next_step = self.next_step(since, now);
        Some(ReferenceStatus {
            id: r.id,
            title: r.title.clone(),
            kind: r.kind,
            charge_bt: r.at(0.0).0,
            bt_now,
            delta_bt: self.last_bt.zip(bt_now).map(|(live, rf)| live - rf),
            ror_now,
            delta_ror: self.last_ror.zip(ror_now).map(|(live, rf)| live - rf),
            events: r.events.clone(),
            next_step,
        })
    }

    /// Projection from the last minute of the curve, between CHARGE and DROP.
    fn projection(&self, events: &[RoastEvent], charge_t: Option<f64>) -> Option<Projection> {
        let charge_t = charge_t?;
        if events.iter().any(|e| e.kind == EventKind::Drop) {
            return None;
        }
        let end = self.points.last()?.t;
        let recent: Vec<(f64, f64, f64)> = self
            .points
            .iter()
            .rev()
            .take_while(|p| p.t >= end - 60.0 && p.t >= charge_t)
            .filter_map(|p| Some((p.t, p.bt?, p.ror?)))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        project(&recent, events, self.reference.as_ref())
    }

    // ----------------------------------------------------------------- recipe

    /// The roast's events, `t` in seconds since CHARGE.
    fn recipe_events(&self) -> Vec<(EventKind, f64)> {
        let Some(roast) = &self.roast else {
            return Vec::new();
        };
        let Some(charge) = roast.charge_t() else {
            return Vec::new();
        };
        roast
            .events
            .iter()
            .map(|e| (e.kind, e.t - charge))
            .collect()
    }

    /// The recipe of a profile plan, fresh for a new roast; none for a roast
    /// as reference.
    fn reset_recipe(&mut self) {
        self.recipe = self
            .reference
            .as_ref()
            .filter(|r| r.kind == ReferenceKind::Profile && !r.actions.is_empty())
            .map(|r| RecipeRun::new(&r.actions));
        // The autopilot's corrections belonged to the old recipe.
        self.trim_offset = 0.0;
        if let Some(ap) = self.autopilot.as_mut() {
            ap.trim = Trim::new(TrimTuning::default());
            ap.corrected_at = None;
        }
    }

    /// Whether the daemon sets the recipe's steps itself: an AI holds the
    /// lease (ADR 0019). Without one the steps are cues for the person.
    fn recipe_automatic(&self, now: f64) -> bool {
        self.recipe.is_some()
            && self.safety.ai_in_control(now)
            && self.safety.status(now).lease_holder.as_deref() != Some(PREHEAT_HOLDER)
    }

    /// Where the roast expects dry end and first crack, and when it reaches a
    /// temperature: the projection for the next milestone, else the plan.
    fn with_outlook<R>(&self, since: Option<f64>, f: impl FnOnce(&Outlook<'_>) -> R) -> R {
        let events = self.roast.as_ref().map_or(&[][..], |r| r.events.as_slice());
        let charge_t = self.roast.as_ref().and_then(Roast::charge_t);
        let projection = self.projection(events, charge_t);
        let planned = |kind| {
            self.reference
                .as_ref()
                .and_then(|r| r.event(kind))
                .map(|e| e.t)
        };
        let expected = |kind| {
            projection
                .and_then(|p| p.next)
                .filter(|m| m.kind == kind)
                .and_then(|m| m.eta_s)
                .zip(since)
                .map(|(eta, s)| s + eta)
                .or_else(|| planned(kind))
        };
        let reaches_bt = |bt: f64| {
            self.reference.as_ref().and_then(|r| {
                r.points
                    .iter()
                    .find(|p| p.t > 60.0 && p.bt.is_some_and(|b| b >= bt))
                    .map(|p| p.t)
            })
        };
        f(&Outlook {
            dry_end: expected(EventKind::DryEnd),
            first_crack: expected(EventKind::FirstCrackStart),
            reaches_bt: &reaches_bt,
        })
    }

    fn since_charge(&self) -> Option<f64> {
        let charge = self.roast.as_ref().and_then(Roast::charge_t)?;
        Some(self.latest_t() - charge)
    }

    fn recipe_status(&self, now: f64) -> Option<RecipeStatus> {
        let run = self.recipe.as_ref()?;
        let events = self.recipe_events();
        let steps = self.with_outlook(self.since_charge(), |o| run.view(&events, o));
        Some(RecipeStatus {
            automatic: self.recipe_automatic(now),
            adjusted: run.adjusted(),
            steps,
        })
    }

    fn next_step(&self, since: Option<f64>, now: f64) -> Option<NextStep> {
        let run = self.recipe.as_ref()?;
        let events = self.recipe_events();
        let current = self.safety.status(now).actuators;
        let next = self.with_outlook(since, |o| run.next(&events, o, &current));
        let first = next.first()?;
        Some(NextStep {
            at_s: first.at_s,
            in_s: since.zip(first.at_s).map(|(s, at)| at - s),
            projected: first.projected,
            label: first.label.clone(),
            automatic: self.recipe_automatic(now),
            set: next
                .iter()
                .map(|v| StepValue {
                    step: v.step,
                    actuator: v.actuator,
                    value: v.value,
                    current: current.get(&v.actuator).copied(),
                })
                .collect(),
        })
    }

    /// Follows the recipe after CHARGE: steps whose trigger came are set
    /// while an AI holds the lease, through the safety layer as the
    /// autopilot, paced by its limits (ADR 0019).
    fn run_recipe(&mut self) {
        let now = self.now();
        let Some(since) = self.since_charge() else {
            return;
        };
        if self.preheat.is_some() || self.recipe.is_none() {
            return;
        }
        let events = self.recipe_events();
        let current = self.safety.status(now).actuators;
        let bt = self.last_bt;
        let automatic = self.recipe_automatic(now);
        let Some(run) = self.recipe.as_mut() else {
            return;
        };
        run.update(&RecipeInput {
            since_charge: since,
            bt,
            events: &events,
            current: &current,
        });
        if !automatic {
            return;
        }
        let todo: Vec<(usize, PlannedAction, bool)> = run
            .to_apply(since)
            .iter()
            .map(|s| (s.step, s.action, s.sent))
            .collect();
        for (step, action, sent_before) in todo {
            let a = action.actuator;
            if self
                .safety
                .rate_limited(a, action.value, CommandSource::Autopilot, now)
            {
                continue;
            }
            let label = trigger_label(&action.trigger);
            let previous = current.get(&a).copied();
            let outcome = self.command(CommandRequest {
                actuator: a,
                value: action.value,
                source: CommandSource::Autopilot,
                reason: Some(format!("recipe {label}: {a} {:.0} %", action.value)),
            });
            match outcome {
                CommandOutcome::Applied { value, .. } => {
                    if let Some(run) = self.recipe.as_mut() {
                        run.sent(step, previous, value);
                    }
                    if !sent_before {
                        self.alarm(
                            "info",
                            "recipe",
                            format!("Recipe {label}: {a} {:.0} %.", action.value),
                        );
                    }
                }
                CommandOutcome::Rejected { reason } => {
                    tracing::debug!("recipe step {step} refused: {reason:?}");
                    if let Some(run) = self.recipe.as_mut() {
                        run.refused(step, since + RECIPE_RETRY_S);
                    }
                }
                CommandOutcome::AwaitingConfirmation { .. } => {}
            }
        }
    }

    /// Changes the remaining recipe for this roast only (ADR 0019): move,
    /// change or skip steps. All changes apply, or none.
    pub fn adjust_recipe(
        &mut self,
        changes: &[StepChange],
        reason: Option<String>,
        origin: EventOrigin,
    ) -> Result<Snapshot, Fail> {
        let run = self
            .recipe
            .as_ref()
            .ok_or_else(|| Fail::conflict("the plan has no recipe (set a profile)", None))?;
        if changes.is_empty() {
            return Err(Fail::bad("no changes"));
        }
        let mut next = run.clone();
        for c in changes {
            next.adjust(c).map_err(|e| Fail::bad(e.to_string()))?;
        }
        let describe = |c: &StepChange| {
            let s = &next.steps()[c.step];
            let what = if s.state == StepState::Skipped {
                "skipped".to_owned()
            } else {
                format!(
                    "{} {} {:.0} %",
                    trigger_label(&s.action.trigger),
                    s.action.actuator,
                    s.action.value
                )
            };
            format!(
                "step {} ({}): {what}",
                c.step,
                trigger_label(&s.planned.trigger)
            )
        };
        let summary = changes.iter().map(describe).collect::<Vec<_>>().join("; ");
        self.recipe = Some(next);
        let who = match origin {
            EventOrigin::Ai => "AI",
            _ => "person",
        };
        let text = match &reason {
            Some(r) => format!("Recipe for this roast ({who}): {summary}. {r}"),
            None => format!("Recipe for this roast ({who}): {summary}."),
        };
        self.audit(serde_json::json!({
            "type": "recipe_adjusted",
            "changes": summary,
            "reason": reason,
            "origin": origin,
        }));
        let t = self.points.last().map(|p| p.t);
        if let Some(roast) = &mut self.roast {
            roast.notes.push(Note {
                t,
                text: text.clone(),
                origin,
            });
        }
        self.alarm("info", "recipe", text);
        self.run_recipe();
        self.publish(None);
        Ok(self.snapshot())
    }

    /// CHARGE, marked or detected: the preheat ends, the recipe's CHARGE
    /// step may jump for 10 s, and a person who marked it in the app is at
    /// the roaster - that confirms presence (ADR 0019).
    fn on_charge(&mut self, origin: EventOrigin) {
        let now = self.now();
        if origin == EventOrigin::Human {
            self.safety.confirm_presence(now);
            self.control_request = None;
            self.audit(serde_json::json!({"type": "presence", "why": "charge"}));
        }
        self.safety.allow_charge_jump(now);
        if self.preheat.is_some() {
            let ai = self
                .safety
                .status(now)
                .lease_holder
                .filter(|h| h != PREHEAT_HOLDER);
            let message = match (&ai, &self.recipe) {
                (Some(h), Some(_)) => {
                    format!("CHARGE - preheat ended; AI ({h}) follows the recipe.")
                }
                (Some(h), None) => format!("CHARGE - preheat ended; AI ({h}) has control."),
                (None, _) if self.autopilot.is_some() => {
                    "CHARGE - preheat ended; the autopilot follows the profile.".to_owned()
                }
                (None, _) => "CHARGE - preheat ended, you have control.".to_owned(),
            };
            self.end_preheat(&message);
        }
        self.autopilot_lease();
        self.run_recipe();
    }

    // -------------------------------------------------------------- autopilot

    fn autopilot_status(&self, now: f64) -> Option<AutopilotStatus> {
        let ap = self.autopilot.as_ref()?;
        let charged = self.since_charge().is_some() && self.preheat.is_none();
        Some(AutopilotStatus {
            state: if !charged {
                "armed"
            } else if self.safety.ai_in_control(now) {
                "active"
            } else {
                "waiting"
            },
            offset: ap.trim.offset(),
            corrections: ap.corrections,
        })
    }

    /// Switches the autopilot on (ADR 0020): from CHARGE on the daemon sets
    /// the recipe's steps and keeps the bean temperature on the profile's
    /// curve with small, rare burner corrections. Before CHARGE it is armed
    /// and the preheat goes on.
    ///
    /// Only a person switches it on, in the app: that allows automation,
    /// counts as presence, and the daemon holds the lease itself, so there
    /// is no AI tool for it. It ends at DROP, on a manual burner, air or
    /// drum command, an emergency stop, a lost link or a missed presence
    /// confirmation.
    pub fn start_autopilot(&mut self) -> Result<Snapshot, Fail> {
        if self.autopilot.is_some() {
            return Ok(self.snapshot());
        }
        if !self
            .reference
            .as_ref()
            .is_some_and(|r| r.kind == ReferenceKind::Profile)
        {
            return Err(Fail::conflict(
                "the autopilot follows a profile: choose one as the plan",
                None,
            ));
        }
        if self
            .roast
            .as_ref()
            .is_some_and(|r| r.event(EventKind::Drop).is_some())
        {
            return Err(Fail::conflict("the roast has ended (DROP)", None));
        }
        let now = self.now();
        let status = self.safety.status(now);
        if !status.connected {
            let reason = RejectReason::Disconnected;
            return Err(Fail::conflict(reject_text(reason), Some(reason)));
        }
        if status.emergency_stop {
            let reason = RejectReason::EmergencyStopLatched;
            return Err(Fail::conflict(reject_text(reason), Some(reason)));
        }
        self.safety.resume_ai();
        self.safety.confirm_presence(now);
        self.control_request = None;
        self.autopilot = Some(Autopilot {
            trim: Trim::resumed(TrimTuning::default(), self.trim_offset),
            corrected_at: None,
            corrections: 0,
        });
        self.audit(serde_json::json!({"type": "autopilot", "on": true}));
        let message = if self.since_charge().is_some() && self.preheat.is_none() {
            "Autopilot on: it follows the recipe and keeps the roast on the profile's curve. Any manual burner, air or drum change ends it."
        } else {
            "Autopilot armed: it takes over at CHARGE. Any manual burner, air or drum change ends it."
        };
        self.alarm("info", "autopilot", message.to_owned());
        self.autopilot_lease();
        self.run_recipe();
        self.publish(None);
        Ok(self.snapshot())
    }

    /// Switches the autopilot off; the actuators hold and the person has
    /// control.
    pub fn stop_autopilot(&mut self) -> Snapshot {
        self.end_autopilot("Autopilot off - you have control.");
        self.snapshot()
    }

    fn end_autopilot(&mut self, message: &str) {
        let Some(ap) = self.autopilot.take() else {
            return;
        };
        if self.safety.status(self.now()).lease_holder.as_deref() == Some(AUTOPILOT_HOLDER) {
            self.safety.release_lease();
        }
        self.audit(serde_json::json!({
            "type": "autopilot",
            "on": false,
            "message": message,
            "corrections": ap.corrections,
            "offset": ap.trim.offset(),
        }));
        self.alarm("info", "autopilot", message.to_owned());
        self.publish(None);
    }

    /// Ends the autopilot when a person took over (or the safety layer
    /// stepped in), and after CHARGE holds its lease: taken when nobody has
    /// one, renewed while it is its own. Under an AI's lease it only trims.
    fn autopilot_lease(&mut self) {
        if self.autopilot.is_none() {
            return;
        }
        let now = self.now();
        let status = self.safety.status(now);
        if status.emergency_stop {
            self.end_autopilot("Emergency stop - the autopilot is off.");
            return;
        }
        if status.ai_suspended || !status.connected {
            self.end_autopilot("Autopilot off - you have control.");
            return;
        }
        if !self
            .reference
            .as_ref()
            .is_some_and(|r| r.kind == ReferenceKind::Profile)
        {
            self.end_autopilot("The plan is not a profile any more - the autopilot is off.");
            return;
        }
        if self.since_charge().is_none() || self.preheat.is_some() {
            return;
        }
        if status
            .lease_holder
            .as_deref()
            .is_some_and(|h| h != AUTOPILOT_HOLDER)
        {
            return;
        }
        match self.safety.acquire_lease(AUTOPILOT_HOLDER, now) {
            Ok(_) => {
                if status.lease_holder.is_none() {
                    self.audit(serde_json::json!({"type": "lease", "holder": AUTOPILOT_HOLDER}));
                }
            }
            Err(RejectReason::PresenceRequired) => self.ask_for_presence("The autopilot"),
            Err(_) => {}
        }
    }

    /// The autopilot's fine control: one burner correction when the bean
    /// temperature heads out of the band around the plan, through the safety
    /// layer as the autopilot. The correction stays for the recipe's later
    /// burner steps.
    fn autopilot_trim(&mut self) {
        let now = self.now();
        let roast_t = self.latest_t();
        let Some(since) = self.since_charge() else {
            return;
        };
        if self.preheat.is_some() || !self.safety.ai_in_control(now) {
            return;
        }
        let (Some(bt), Some(ror)) = (self.last_bt, self.last_ror) else {
            return;
        };
        // Past the plan's end there is nothing to keep to.
        let Some((Some(plan_bt), Some(plan_ror))) = self.reference.as_ref().map(|r| r.at(since))
        else {
            return;
        };
        let Some(burner) = self
            .safety
            .status(now)
            .actuators
            .get(&Actuator::Burner)
            .copied()
        else {
            return;
        };
        let since_burner_change = self.burner_changed_at.map_or(f64::MAX, |t| roast_t - t);
        let Some(ap) = self.autopilot.as_ref() else {
            return;
        };
        let decision = ap.trim.decide(&TrimInput {
            since_charge: since,
            bt,
            ror,
            plan_bt,
            plan_ror,
            burner,
            since_burner_change,
            since_correction: ap.corrected_at.map(|t| roast_t - t),
        });
        let TrimDecision::Step {
            burner: to,
            predicted_error_c,
            ..
        } = decision
        else {
            return;
        };
        if self
            .safety
            .rate_limited(Actuator::Burner, to, CommandSource::Autopilot, now)
        {
            return;
        }
        let side = if predicted_error_c > 0.0 {
            "above"
        } else {
            "below"
        };
        let why = format!(
            "the roast heads {:.0} °C {side} the plan",
            predicted_error_c.abs()
        );
        let outcome = self.command(CommandRequest {
            actuator: Actuator::Burner,
            value: to,
            source: CommandSource::Autopilot,
            reason: Some(format!("autopilot: {why}")),
        });
        let CommandOutcome::Applied { value, .. } = outcome else {
            return;
        };
        let delta = value - burner;
        if delta.abs() < 1e-9 {
            return;
        }
        if let Some(ap) = self.autopilot.as_mut() {
            ap.trim.applied(delta);
            ap.corrected_at = Some(roast_t);
            ap.corrections += 1;
            self.trim_offset = ap.trim.offset();
        }
        if let Some(run) = self.recipe.as_mut() {
            run.shift_open(Actuator::Burner, delta);
        }
        self.alarm(
            "info",
            "autopilot",
            format!("Autopilot: burner {value:.0} % ({delta:+.0}), {why}."),
        );
    }

    // ---------------------------------------------------------------- preheat

    /// Starts preheating to `target_c` (bean probe) at `air` (ADR 0018: the
    /// air the roast starts with; `None` keeps the air that is applied). A
    /// person starts it in the app, which confirms presence and allows
    /// automation; the air is set first as that person's own command. It ends
    /// at CHARGE, on any manual command, an emergency stop or a lost link.
    pub fn start_preheat(&mut self, target_c: f64, air: Option<f64>) -> Result<Snapshot, Fail> {
        let max = self.safety.config().max_bt_c - 10.0;
        if !(50.0..=max).contains(&target_c) {
            return Err(Fail::bad(format!(
                "the preheat target must be between 50 and {max:.0} °C"
            )));
        }
        if air.is_some_and(|a| !(0.0..=100.0).contains(&a)) {
            return Err(Fail::bad("the preheat air must be between 0 and 100 %"));
        }
        if self.roast.as_ref().is_some_and(|r| r.charge_t().is_some()) {
            return Err(Fail::conflict("the roast has started (CHARGE)", None));
        }
        if let Some(air) = air {
            // The person's own command, through the safety layer like any other.
            let outcome = self.command(CommandRequest {
                actuator: Actuator::Air,
                value: air,
                source: CommandSource::Human,
                reason: Some(format!("preheat at air {air:.0} %")),
            });
            if let CommandOutcome::Rejected { reason } = outcome {
                return Err(Fail::conflict(reject_text(reason), Some(reason)));
            }
        }
        let now = self.now();
        self.safety.resume_ai();
        self.safety.confirm_presence(now);
        // Before CHARGE the preheat confirms less often (ADR 0018).
        self.safety.set_preheat_holder(Some(PREHEAT_HOLDER));
        if let Err(r) = self.safety.acquire_lease(PREHEAT_HOLDER, now) {
            self.safety.set_preheat_holder(None);
            return Err(Fail::conflict(reject_text(r), Some(r)));
        }
        let status = self.safety.status(now);
        let air = status.actuators.get(&Actuator::Air).copied().unwrap_or(0.0);
        let machine_key = self.machine.as_ref().map_or_else(String::new, |m| {
            if m.simulated {
                "simulator".to_owned()
            } else {
                m.id.clone()
            }
        });
        let plant = self.learned.preheat(&machine_key, air);
        let ambient = self.last_ambient.unwrap_or(DEFAULT_AMBIENT_C);
        self.preheat = Some(Preheat {
            ctl: PreheatController::new(target_c, ambient, PreheatTuning::default(), plant),
            machine_key,
            air,
            last_seen_t: None,
            last_raise: None,
            ai_holder: None,
        });
        self.audit(serde_json::json!({
            "type": "preheat_started",
            "target_c": target_c,
            "air": air,
            "ror_per_pct": plant.ror_per_pct,
            "hold_pct_200": plant.hold_pct_200,
        }));
        let every_min = self.safety.config().preheat_presence_interval_s / 60.0;
        self.alarm(
            "info",
            "preheat",
            format!(
                "Preheating to {target_c:.0} °C at air {air:.0} %. Confirm presence every {every_min:.0} min while it runs (a gong sounds before)."
            ),
        );
        self.preheat_step(now);
        self.publish(None);
        Ok(self.snapshot())
    }

    /// Stops preheating; the burner keeps its value and you have control.
    pub fn stop_preheat(&mut self) -> Snapshot {
        self.end_preheat("Preheat stopped - you have control.");
        self.snapshot()
    }

    fn end_preheat(&mut self, message: &str) {
        let Some(p) = self.preheat.take() else {
            return;
        };
        self.safety.set_preheat_holder(None);
        let learned = p.ctl.plant();
        if let Err(e) = self.learned.set_preheat(&p.machine_key, p.air, learned) {
            tracing::warn!("could not save what the preheat learned: {e}");
        }
        if self.safety.status(self.now()).lease_holder.as_deref() == Some(PREHEAT_HOLDER) {
            self.safety.release_lease();
        }
        self.audit(serde_json::json!({
            "type": "preheat_ended",
            "message": message,
            "ror_per_pct": learned.ror_per_pct,
            "hold_pct_200": learned.hold_pct_200,
        }));
        self.alarm("info", "preheat", message.to_owned());
        self.publish(None);
    }

    fn preheat_step(&mut self, now: f64) {
        let Some(p) = self.preheat.as_ref() else {
            return;
        };
        if self.roast.as_ref().is_some_and(|r| r.charge_t().is_some()) {
            self.end_preheat("CHARGE - preheat ended, you have control.");
            return;
        }
        let status = self.safety.status(now);
        let ai_before = p.ai_holder.clone();
        match status.lease_holder.as_deref() {
            Some(PREHEAT_HOLDER) => {}
            // An AI took over before CHARGE: the preheat keeps holding under
            // its lease, with its presence interval (ADR 0019).
            Some(holder) => {
                let holder = holder.to_owned();
                if let Some(p) = self.preheat.as_mut() {
                    p.ai_holder = Some(holder);
                }
            }
            None if status.ai_suspended => {
                // A person took over (or presence was missed). Without an AI
                // in control, do not report the AI as paused.
                if ai_before.is_none() {
                    self.safety.resume_ai();
                }
                self.end_preheat("Preheat ended - you have control.");
                return;
            }
            // The AI released control or stopped renewing its lease before
            // CHARGE: the preheat holds on under its own lease.
            None => {
                if let Some(p) = self.preheat.as_mut() {
                    p.ai_holder = None;
                }
                if self.safety.acquire_lease(PREHEAT_HOLDER, now).is_err() {
                    self.end_preheat(
                        "Preheat ended: presence not confirmed or control not allowed.",
                    );
                    return;
                }
                if ai_before.is_some() {
                    self.alarm(
                        "info",
                        "preheat",
                        "The preheat holds on until CHARGE.".to_owned(),
                    );
                    self.publish(None);
                }
            }
        }
        let own_lease = self.safety.status(now).lease_holder.as_deref() == Some(PREHEAT_HOLDER);
        // One decision per reading.
        let Some(t) = self.last_reading_t else {
            return;
        };
        let Some(p) = self.preheat.as_ref() else {
            return;
        };
        if p.last_seen_t.is_some_and(|seen| seen >= t) {
            return;
        }
        // Under its own lease the preheat renews it; under an AI's lease
        // only the AI does (the lease is its sign of life).
        if own_lease && self.safety.acquire_lease(PREHEAT_HOLDER, now).is_err() {
            self.end_preheat("Preheat ended: presence not confirmed or control not allowed.");
            return;
        }
        let Some(bt) = self.last_bt else {
            return;
        };
        let burner = status
            .actuators
            .get(&Actuator::Burner)
            .copied()
            .unwrap_or(0.0);
        let min_interval = self
            .safety
            .config()
            .ai_limits
            .get(&Actuator::Burner)
            .map_or(0.0, |l| l.min_interval_s);
        let ror = self.last_ror;
        let Some(p) = self.preheat.as_mut() else {
            return;
        };
        p.last_seen_t = Some(t);
        let Some(d) = p.ctl.step(PreheatInput { t, bt, ror, burner }) else {
            return;
        };
        let raise = d.burner > burner;
        // The safety layer paces automatic raises; asking sooner is refused.
        if raise && p.last_raise.is_some_and(|l| now - l < min_interval) {
            return;
        }
        if raise {
            p.last_raise = Some(now);
        }
        let target = p.ctl.target_c();
        let reason = match d.reason {
            PreheatReason::Heat => format!("preheat to {target:.0} °C: full heat"),
            PreheatReason::StepToHold => {
                format!("preheat to {target:.0} °C: step down to the holding power")
            }
            PreheatReason::Correct => format!("preheat to {target:.0} °C: correction"),
            PreheatReason::Cut => format!("preheat to {target:.0} °C: far too hot, heat off"),
        };
        self.command(CommandRequest {
            actuator: Actuator::Burner,
            value: d.burner,
            source: CommandSource::Autopilot,
            reason: Some(reason),
        });
    }

    // -------------------------------------------------------------- reference

    /// Shows a saved roast as the reference curve, or none.
    pub fn set_reference(&mut self, id: Option<uuid::Uuid>) -> Result<Snapshot, Fail> {
        self.reference = match id {
            None => None,
            Some(id) => {
                let roast = self
                    .archive
                    .find(id)
                    .map_err(|e| Fail::internal(e.to_string()))?
                    .ok_or_else(|| Fail::not_found(format!("no saved roast {id}")))?;
                Some(ReferenceCurve::from_roast(&roast, &RorConfig::default()))
            }
        };
        self.reset_recipe();
        self.audit(serde_json::json!({"type": "reference", "id": id}));
        self.publish(None);
        Ok(self.snapshot())
    }

    /// The connected machine's profile id ("simulator" in the simulator), for
    /// filing what is made for it.
    pub(crate) fn machine_id(&self) -> Option<String> {
        self.machine.as_ref().map(|m| {
            if m.simulated {
                "simulator".to_owned()
            } else {
                m.id.clone()
            }
        })
    }

    /// Shows a profile version's target curve as the reference (plan line).
    pub fn set_reference_profile(&mut self, version_id: uuid::Uuid) -> Result<Snapshot, Fail> {
        let (profile, version) = self.profiles.version(version_id)?;
        self.reference = Some(ReferenceCurve::from_profile(&profile, &version));
        self.reset_recipe();
        self.audit(serde_json::json!({"type": "reference", "profile_version": version_id}));
        self.publish(None);
        Ok(self.snapshot())
    }

    /// The reference curve, if one is set.
    #[must_use]
    pub fn reference(&self) -> Option<ReferenceCurve> {
        self.reference.clone()
    }

    /// The curve of the recording, or the preview.
    #[must_use]
    pub fn live(&self) -> Live {
        let (events, notes, control_log) = self.roast.as_ref().map_or_else(
            || (Vec::new(), Vec::new(), Vec::new()),
            |r| (r.events.clone(), r.notes.clone(), r.control_log.clone()),
        );
        Live {
            points: self.points.clone(),
            events,
            notes,
            control_log,
        }
    }

    fn publish(&self, point: Option<&Point>) {
        if self.updates.receiver_count() == 0 {
            return;
        }
        let msg = serde_json::json!({
            "type": "tick",
            "snapshot": self.snapshot(),
            "point": point,
        });
        let _ = self.updates.send(Arc::from(msg.to_string()));
    }

    fn publish_reset(&self) {
        let _ = self.updates.send(Arc::from(r#"{"type":"reset"}"#));
        self.publish(None);
    }

    fn alarm(&mut self, severity: &'static str, kind: &str, message: impl Into<String>) {
        let message = message.into();
        if severity == "info" {
            tracing::info!(kind, "{message}");
        } else {
            tracing::warn!(kind, severity, "{message}");
        }
        self.audit(serde_json::json!({"type": "alarm", "severity": severity, "kind": kind, "message": message}));
        self.alarms.push_back(Alarm {
            id: self.next_alarm,
            at: Utc::now(),
            severity,
            kind: kind.to_owned(),
            message,
        });
        self.next_alarm += 1;
        while self.alarms.len() > ALARMS_KEPT {
            self.alarms.pop_front();
        }
    }

    fn audit(&mut self, mut entry: serde_json::Value) {
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("at".into(), serde_json::json!(Utc::now()));
            if self.roast.is_some() {
                obj.insert("t".into(), serde_json::json!(self.latest_t()));
            }
        }
        if let Some(f) = &mut self.audit {
            let _ = writeln!(f, "{entry}");
        }
    }

    // ------------------------------------------------------------- connection

    /// Connects to the simulator or a serial port.
    pub fn connect(&mut self, req: ConnectReq) -> Result<Snapshot, Fail> {
        self.disconnect_inner();
        let (profile, driver, port, scale): (MachineProfile, Box<dyn RoasterDriver>, _, f64) =
            if req.simulator {
                let p = simulator_profile(&self.opts.profile);
                let speed = self.opts.sim_speed;
                (p, Box::new(SimulatedMachine::new(speed)), None, speed)
            } else {
                let port = req
                    .port
                    .filter(|p| !p.trim().is_empty())
                    .ok_or_else(|| Fail::bad("choose a port or the simulator"))?;
                let p = self.opts.profile.clone();
                let d = roast_driver::open_port(p.clone(), &port)
                    .map_err(|e| Fail::bad(e.to_string()))?;
                (p, d, Some(port), 1.0)
            };

        let same_machine = self.profile.as_ref().is_some_and(|p| p.id == profile.id);
        if !same_machine {
            // Keep the latch across reconnects of the same machine.
            let estop = self.safety.status(self.now()).emergency_stop;
            self.safety = Safety::new(SafetyConfig::from_profile(&profile));
            if estop {
                self.safety.emergency_stop("still latched from before");
            }
        }
        self.machine = Some(machine_info(&profile, req.simulator, port.clone()));
        self.generation += 1;
        let interval = Duration::from_secs_f64((profile.sampling_interval_s / scale).max(0.1));
        self.driver = Some(spawn_driver(
            driver,
            interval,
            self.generation,
            self.tx.clone(),
        ));
        self.profile = Some(profile);
        self.time_scale = scale;
        self.conn = Conn::Connecting;
        self.conn_error = None;
        self.reset_curve();
        self.audit(
            serde_json::json!({"type": "connect", "simulator": req.simulator, "port": port}),
        );
        self.publish_reset();
        Ok(self.snapshot())
    }

    /// Closes the connection. The machine keeps its current settings.
    pub fn disconnect(&mut self) -> Snapshot {
        self.disconnect_inner();
        self.audit(serde_json::json!({"type": "disconnect"}));
        self.publish_reset();
        self.snapshot()
    }

    fn disconnect_inner(&mut self) {
        if let Some(d) = self.driver.take() {
            let _ = d.send(DriverCmd::Stop);
        }
        self.generation += 1;
        self.safety.set_connected(false);
        self.end_autopilot("Disconnected - the autopilot is off.");
        self.conn = Conn::Disconnected;
        self.conn_error = None;
        self.machine = None;
        if self.roast.is_none() {
            self.state = RoastState::Disconnected;
        }
        self.last_bt = None;
        self.last_et = None;
        self.last_ror = None;
        self.last_reading = None;
    }

    /// Daemon shutdown: if the burner is on, the roaster goes to the safe
    /// state, because nobody controls it any more.
    pub fn shutdown(&mut self) {
        let burner_on = self
            .safety
            .status(self.now())
            .actuators
            .get(&Actuator::Burner)
            .is_some_and(|&b| b > 0.0);
        if burner_on && self.conn == Conn::Connected {
            let r = self.safety.emergency_stop("daemon shutting down");
            self.apply_reaction(&r);
        }
        if self.roast.is_some() {
            let _ = self.stop_recording();
        }
        if let Some(d) = self.driver.take() {
            let _ = d.send(DriverCmd::Stop);
        }
    }

    fn on_driver(&mut self, ev: DriverEvent) {
        match ev {
            DriverEvent::Connected(warnings) => {
                // The port is open; the machine counts as connected only with
                // its first reading (see `on_reading`).
                for w in &warnings {
                    self.alarm("info", "init", w.clone());
                }
                self.publish(None);
            }
            DriverEvent::GaveUp(e) => {
                let bluetooth = self
                    .machine
                    .as_ref()
                    .and_then(|m| m.port.as_deref())
                    .is_some_and(|p| p.starts_with(roast_driver::BLUETOOTH_PREFIX));
                let hint = if bluetooth {
                    format!(
                        "{e}. Check: the roaster is switched on, paired and in range, and no other program (Artisan) is connected to it; \
                         then connect again. Details: logs/daemon.log (RUST_LOG=roast_driver=trace shows every line)."
                    )
                } else {
                    format!(
                        "{e}. Check: the roaster is switched on and paired, no other program (Artisan, a second RoastMCP) holds the port, \
                         then switch the roaster off and on and connect again. Details: logs/daemon.log (RUST_LOG=roast_driver=trace shows every line)."
                    )
                };
                self.conn = Conn::Error;
                self.conn_error = Some(hint.clone());
                self.driver = None;
                self.alarm("critical", "connect_failed", hint);
                self.publish(None);
            }
            DriverEvent::Reading(r) => self.on_reading(r),
            DriverEvent::Error(e) => {
                tracing::debug!("driver: {e}");
                self.conn_error = Some(e);
            }
            DriverEvent::Lost(e) => {
                self.conn = Conn::Error;
                self.conn_error = Some(e.clone());
                if let Some(r) = self.safety.set_connected(false) {
                    self.alarm("critical", "link_lost", format!("{} ({e})", r.message));
                }
                self.end_autopilot("The link is lost - the autopilot is off.");
                self.publish(None);
            }
        }
    }

    fn on_reading(&mut self, r: roast_driver::Reading) {
        self.safety.observe(&r.actuators);
        self.last_reading = Some(Instant::now());
        self.stale_alarmed = false;
        // A reading arrived: a passing driver error (one bad line) is over.
        if self.conn == Conn::Connected && self.conn_error.is_some() {
            self.conn_error = None;
        }
        if self.conn != Conn::Connected {
            self.conn = Conn::Connected;
            self.conn_error = None;
            if self.roast.is_none() {
                self.state = RoastState::Connected;
            }
            let name = self
                .machine
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_default();
            // A machine may start at its own defaults on connect (the Cubean+
            // heats at 100 %), so the safety layer sets the safe state at once.
            match self.safety.set_connected(true) {
                Some(mut r) => {
                    r.message = format!("Connected to {name}. {}", r.message);
                    self.apply_reaction(&r);
                }
                None => self.alarm("info", "connected", format!("Connected to {name}.")),
            }
        }
        let t = self.t();
        let ror = r.bt.and_then(|bt| self.ror.push(t, bt));
        self.last_bt = r.bt;
        self.last_et = r.et;
        self.last_ror = ror;
        self.last_ambient = r.ambient_c.or(self.last_ambient);
        self.last_reading_t = Some(t);
        let actuators = self.safety.status(self.now()).actuators;
        let point = Point {
            t,
            bt: r.bt,
            et: r.et,
            ror,
            actuators: actuators.clone(),
        };

        let mut found = Vec::new();
        if let Some(roast) = &mut self.roast {
            roast.samples.push(Sample {
                t,
                bt: r.bt,
                et: r.et,
                actuators,
            });
            if let Some(bt) = r.bt {
                found = self.detector.push(t, bt, &roast.events);
                roast.events.extend(found.iter().cloned());
            }
        } else {
            let keep_from = t - PREVIEW_KEEP_S * self.time_scale.max(1.0);
            let cut = self.points.partition_point(|p| p.t < keep_from);
            if cut > 0 {
                self.points.drain(..cut);
            }
        }
        let mut dropped = false;
        let mut charged = false;
        for e in found {
            match e.kind {
                EventKind::Charge if self.state == RoastState::Recording => {
                    self.state = RoastState::Charged;
                    charged = true;
                    self.alarm("info", "auto_event", "CHARGE detected.".to_owned());
                }
                EventKind::Drop => {
                    self.state = RoastState::Dropped;
                    dropped = true;
                }
                _ => {}
            }
            // As for a marked event (the simulator takes its beans out at DROP).
            if let Some(d) = &self.driver {
                let _ = d.send(DriverCmd::Event(e.kind));
            }
            self.audit(serde_json::json!({"type": "event", "event": e}));
        }
        self.points.push(point.clone());
        if charged {
            // A detected CHARGE does not confirm presence: nobody pressed anything.
            self.on_charge(EventOrigin::Auto);
        } else if !dropped {
            self.autopilot_lease();
            self.run_recipe();
            self.autopilot_trim();
        }
        self.cool_before_drop();
        self.publish(Some(&point));
        if dropped {
            self.after_drop();
            // Same as the DROP button: the roast is saved at once. The event
            // is dated at the peak, where the beans left the drum.
            self.alarm(
                "info",
                "auto_event",
                "DROP detected: the roast is saved.".to_owned(),
            );
            if let Err(e) = self.stop_recording() {
                tracing::warn!("saving after the detected DROP failed: {}", e.error);
            }
        }
    }

    fn on_tick(&mut self) {
        let now = self.now();
        for r in self.safety.tick(now, self.last_bt) {
            self.apply_reaction(&r);
        }
        self.preheat_step(now);
        // The daemon's own lease must not run out between readings, and a
        // takeover or a safety reaction ends the autopilot at once.
        self.autopilot_lease();
        if self.conn == Conn::Connected
            && !self.stale_alarmed
            && self
                .last_reading
                .is_some_and(|l| l.elapsed().as_secs_f64() > STALE_AFTER_S)
        {
            self.stale_alarmed = true;
            self.alarm(
                "warning",
                "stale",
                "No new readings from the roaster for 5 seconds.",
            );
            self.publish(None);
        }
        if self.roast.is_some() && self.last_autosave.elapsed() >= AUTOSAVE_EVERY {
            self.last_autosave = Instant::now();
            if let Some(roast) = &self.roast {
                let _ = self.archive.autosave(roast);
            }
        }
    }

    fn apply_reaction(&mut self, r: &SafetyReaction) {
        let severity = match r.kind {
            roast_safety::ReactionKind::LeaseExpired => "warning",
            // Routine on a first connect; worth a sound when a roast lost its heat.
            roast_safety::ReactionKind::Connected if self.roast.is_some() => "warning",
            roast_safety::ReactionKind::Connected => "info",
            _ => "critical",
        };
        let kind = serde_json::to_value(r.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        for c in &r.commands {
            if let Some(d) = &self.driver {
                let _ = d.send(DriverCmd::Apply(*c));
            }
            let entry = ControlLogEntry {
                t: self.roast.as_ref().map(|_| self.latest_t()),
                at: Utc::now(),
                // The safety layer already moved its state; the old value is not known here.
                previous: None,
                request: CommandRequest {
                    actuator: c.actuator,
                    value: c.value,
                    source: CommandSource::Safety,
                    reason: Some(r.message.clone()),
                },
                outcome: CommandOutcome::Applied {
                    value: c.value,
                    clamped_from: None,
                },
            };
            if let Some(roast) = &mut self.roast {
                roast.control_log.push(entry);
            }
        }
        if let Some(roast) = &mut self.roast {
            let t = self.points.last().map(|p| p.t);
            roast.notes.push(Note {
                t,
                text: format!("Safety: {}", r.message),
                origin: EventOrigin::Auto,
            });
        }
        self.audit(serde_json::json!({"type": "reaction", "reaction": r}));
        self.alarm(severity, &kind, r.message.clone());
        self.publish(None);
    }

    // --------------------------------------------------------------- commands

    /// Passes a command through the safety layer.
    pub fn command(&mut self, req: CommandRequest) -> CommandOutcome {
        let now = self.now();
        let ai_before = self.safety.ai_in_control(now);
        let previous = self
            .safety
            .status(now)
            .actuators
            .get(&req.actuator)
            .copied();
        let (outcome, cmd) = self.safety.request(&req, now);
        if let (Some(c), Some(d)) = (&cmd, &self.driver) {
            let _ = d.send(DriverCmd::Apply(*c));
        }
        if let Some(c) = &cmd
            && c.actuator == Actuator::Burner
            && previous.is_none_or(|p| (p - c.value).abs() > 1e-9)
        {
            self.burner_changed_at = Some(self.latest_t());
        }
        // The AI's own command takes the place of the recipe step due on that
        // actuator; later steps still come (ADR 0019).
        if req.source == CommandSource::Ai
            && cmd.is_some()
            && let Some(run) = self.recipe.as_mut()
        {
            run.overridden(req.actuator);
        }
        let entry = ControlLogEntry {
            t: self.roast.as_ref().map(|_| self.latest_t()),
            at: Utc::now(),
            previous,
            request: req.clone(),
            outcome: outcome.clone(),
        };
        self.audit(serde_json::json!({"type": "command", "entry": entry}));
        if let Some(roast) = &mut self.roast {
            roast.control_log.push(entry);
        }
        if ai_before && !self.safety.ai_in_control(now) && req.source == CommandSource::Human {
            self.alarm(
                "info",
                "human_takeover",
                "You took over. AI control is suspended until you allow it again.",
            );
        }
        self.publish(None);
        outcome
    }

    /// Emergency stop from any source.
    pub fn emergency_stop(&mut self, reason: &str) -> Snapshot {
        let r = self.safety.emergency_stop(reason);
        self.apply_reaction(&r);
        self.snapshot()
    }

    /// A person releases the emergency-stop latch.
    pub fn reset_emergency_stop(&mut self) -> Snapshot {
        self.safety.reset_emergency_stop();
        self.audit(serde_json::json!({"type": "emergency_stop_reset"}));
        self.alarm(
            "info",
            "emergency_stop_reset",
            "Emergency stop released by a person.",
        );
        self.publish(None);
        self.snapshot()
    }

    /// A person confirmed being at the roaster.
    pub fn confirm_presence(&mut self) -> Snapshot {
        let now = self.now();
        self.safety.confirm_presence(now);
        // The AI asks again itself; the prompt has done its job.
        self.control_request = None;
        self.audit(serde_json::json!({"type": "presence"}));
        self.publish(None);
        self.snapshot()
    }

    /// A person allows AI control again.
    pub fn resume_ai(&mut self) -> Snapshot {
        self.safety.resume_ai();
        self.audit(serde_json::json!({"type": "ai_resume"}));
        self.publish(None);
        self.snapshot()
    }

    /// Grants or renews the AI lease.
    pub fn acquire_lease(&mut self, holder: &str) -> Result<LeaseInfo, Fail> {
        // The daemon's own controllers hold their leases under these names.
        if holder == PREHEAT_HOLDER || holder == AUTOPILOT_HOLDER {
            return Err(Fail::bad(format!("the holder name '{holder}' is reserved")));
        }
        let now = self.now();
        let before = self.safety.status(now).lease_holder;
        match self.safety.acquire_lease(holder, now) {
            Ok(l) => {
                self.control_request = None;
                if before.as_deref() != Some(holder) {
                    self.audit(serde_json::json!({"type": "lease", "holder": holder}));
                    let message = match (&self.preheat, &self.recipe) {
                        (Some(p), Some(_)) => format!(
                            "AI ({holder}) has control. The preheat holds {:.0} °C until CHARGE; then the recipe runs.",
                            p.ctl.target_c()
                        ),
                        (Some(p), None) => format!(
                            "AI ({holder}) has control. The preheat holds {:.0} °C until CHARGE.",
                            p.ctl.target_c()
                        ),
                        (None, Some(_)) => {
                            format!("AI ({holder}) has control and follows the recipe.")
                        }
                        (None, None) => format!("AI ({holder}) has control."),
                    };
                    self.alarm("info", "ai_control", message);
                    self.run_recipe();
                    self.publish(None);
                }
                Ok(LeaseInfo {
                    holder: l.holder,
                    expires_in_s: l.expires_at - now,
                })
            }
            Err(reason) => {
                if reason == RejectReason::PresenceRequired {
                    self.ask_for_presence(holder);
                }
                Err(Fail::conflict(reject_text(reason), Some(reason)))
            }
        }
    }

    /// An AI wants control but nobody confirmed being at the roaster: tell the
    /// person (once per request burst) and let the app offer the button.
    fn ask_for_presence(&mut self, holder: &str) {
        let fresh = self
            .control_request
            .as_ref()
            .is_some_and(|(h, at)| h == holder && at.elapsed() < CONTROL_REQUEST_SHOWN_FOR);
        self.control_request = Some((holder.to_owned(), Instant::now()));
        if !fresh {
            self.alarm(
                "warning",
                "control_request",
                format!("{holder} asks for control. Press \u{201c}I'm at the roaster\u{201d} in the app to allow it."),
            );
        }
        self.publish(None);
    }

    /// The AI gives up control; actuators hold. The preheat's and the
    /// autopilot's own leases are not the AI's to release.
    pub fn release_lease(&mut self) -> Snapshot {
        let holder = self.safety.status(self.now()).lease_holder;
        if holder
            .as_deref()
            .is_some_and(|h| h != PREHEAT_HOLDER && h != AUTOPILOT_HOLDER)
        {
            self.safety.release_lease();
            self.audit(serde_json::json!({"type": "lease_released"}));
            self.alarm(
                "info",
                "ai_released",
                "AI released control - you have control.",
            );
            // A running preheat or autopilot takes its lease back and says so.
            self.preheat_step(self.now());
            self.autopilot_lease();
            self.publish(None);
        }
        self.snapshot()
    }

    // -------------------------------------------------------------- recording

    fn reset_curve(&mut self) {
        self.zero = Instant::now();
        self.points.clear();
        self.ror.reset();
        self.detector = EventDetector::default();
    }

    /// Starts a new recording.
    pub fn start_recording(&mut self, meta: RecordingMeta) -> Result<Snapshot, Fail> {
        if self.conn != Conn::Connected {
            return Err(Fail::conflict("connect a roaster first", None));
        }
        if self.roast.is_some() {
            return Err(Fail::conflict("a recording is already running", None));
        }
        let lot = match meta.lot_id {
            Some(id) => Some(self.inventory.lot(id)?.lot),
            None => None,
        };
        let plan = meta.plan.filter(|p| !p.is_empty());
        let profile_version = plan.as_ref().and_then(|p| p.profile_version_id);
        if let Some(version) = profile_version {
            // A profile's target curve is the plan line on the chart.
            self.set_reference_profile(version)?;
        } else if let Some(reference) = plan.as_ref().and_then(|p| p.reference_id) {
            // The plan's reference roast is the plan line on the chart.
            self.set_reference(Some(reference))?;
        }
        let now = chrono::Local::now();
        let title = meta
            .title
            .filter(|t| !t.trim().is_empty())
            .or_else(|| lot.as_ref().map(|l| l.name.clone()))
            .unwrap_or_else(|| format!("Roast {}", now.format("%Y-%m-%d %H:%M")));
        let simulated = self.machine.as_ref().is_some_and(|m| m.simulated);
        // The simulator roasts no real coffee: no batch number.
        let batch_no = meta
            .batch_no
            .map(|b| b.trim().to_owned())
            .filter(|b| !b.is_empty() && !simulated);
        if let (Some(l), Some(b)) = (lot.as_ref(), batch_no.as_deref()) {
            self.inventory.adopt_batch_code(l.id, b);
        }
        self.cooled_before_drop = false;
        let profile = self.profile.as_ref();
        self.roast = Some(Roast {
            id: uuid::Uuid::new_v4(),
            title,
            roasted_at: Utc::now(),
            machine: profile.map(|p| p.id.clone()),
            sampling_interval_s: profile.map_or(1.0, |p| p.sampling_interval_s),
            batch: BatchInfo {
                green_weight_g: meta.green_weight_g,
                roasted_weight_g: meta.roasted_weight_g,
                // The lot's name, so files and .alog stay readable on their own.
                beans: lot.as_ref().map(|l| l.name.clone()).or(meta.beans),
                batch_no,
            },
            ambient: None,
            roast_degree: None,
            green_lot_id: lot.as_ref().map(|l| l.id),
            plan,
            profile_version_id: profile_version,
            experiment_id: None,
            origin: if simulated {
                RoastOrigin::Simulated
            } else {
                RoastOrigin::Recorded
            },
            samples: Vec::new(),
            events: Vec::new(),
            control_log: Vec::new(),
            notes: Vec::new(),
        });
        self.reset_curve();
        self.reset_recipe();
        self.state = RoastState::Recording;
        self.last_autosave = Instant::now();
        self.audit(serde_json::json!({"type": "recording_started"}));
        self.publish_reset();
        Ok(self.snapshot())
    }

    /// A saved roast was deleted (its files are in the trash): its stock
    /// debit is taken back, and it stops being the reference. Returns the
    /// grams credited back.
    pub(crate) fn roast_deleted(&mut self, id: uuid::Uuid, title: &str) -> Result<f64, Fail> {
        let credited_g = self.inventory.remove_roast_debit(id)?;
        if self.reference.as_ref().is_some_and(|r| r.id == id) {
            self.reference = None;
            self.reset_recipe();
        }
        self.audit(serde_json::json!({
            "type": "roast_deleted",
            "roast_id": id,
            "title": title,
            "credited_g": credited_g,
        }));
        self.publish(None);
        Ok(credited_g)
    }

    /// Changes the running recording's details.
    pub fn update_recording(&mut self, meta: RecordingMeta) -> Result<Snapshot, Fail> {
        let roast = self
            .roast
            .as_mut()
            .ok_or_else(|| Fail::conflict("no recording is running", None))?;
        if let Some(t) = meta.title.filter(|t| !t.trim().is_empty()) {
            roast.title = t;
        }
        if meta.green_weight_g.is_some() {
            roast.batch.green_weight_g = meta.green_weight_g;
        }
        if meta.roasted_weight_g.is_some() {
            roast.batch.roasted_weight_g = meta.roasted_weight_g;
        }
        if meta.beans.is_some() {
            roast.batch.beans = meta.beans;
        }
        if let Some(b) = meta.batch_no
            && !matches!(roast.origin, RoastOrigin::Simulated)
        {
            let b = b.trim().to_owned();
            roast.batch.batch_no = Some(b).filter(|b| !b.is_empty());
        }
        if let Some(id) = meta.lot_id {
            let lot = self.inventory.lot(id)?.lot;
            let roast = self
                .roast
                .as_mut()
                .ok_or_else(|| Fail::conflict("no recording is running", None))?;
            roast.green_lot_id = Some(lot.id);
            roast.batch.beans = Some(lot.name);
        }
        if let Some(plan) = meta.plan {
            let reference = plan.reference_id;
            let profile_version = plan.profile_version_id;
            if let Some(roast) = self.roast.as_mut() {
                roast.plan = Some(plan).filter(|p| !p.is_empty());
                roast.profile_version_id = profile_version;
            }
            if let Some(version) = profile_version {
                self.set_reference_profile(version)?;
            } else if reference.is_some() {
                self.set_reference(reference)?;
            }
        }
        self.publish(None);
        Ok(self.snapshot())
    }

    /// Stops and saves the recording.
    pub fn stop_recording(&mut self) -> Result<Saved, Fail> {
        let roast = self
            .roast
            .take()
            .ok_or_else(|| Fail::conflict("no recording is running", None))?;
        let saved = self.archive.save(&roast);
        self.state = if self.conn == Conn::Connected {
            RoastState::Connected
        } else {
            RoastState::Disconnected
        };
        self.reset_curve();
        let (json_path, alog_path) = match saved {
            Ok(p) => p,
            Err(e) => {
                // Keep the data: put the roast back so nothing is lost.
                let msg = format!("Saving failed: {e}");
                self.roast = Some(roast);
                self.state = RoastState::Recording;
                self.alarm("critical", "save_failed", msg.clone());
                return Err(Fail::internal(msg));
            }
        };
        self.archive.clear_autosave();
        self.end_autopilot("The recording ended - the autopilot is off.");
        // A copy elsewhere, if the person set a backup folder (ADR 0003).
        crate::backup::spawn(&self.opts.store);
        // The next roast starts the recipe afresh.
        self.reset_recipe();
        if let Err(e) = self.inventory.sync_roast_debit(&roast) {
            self.alarm(
                "warning",
                "inventory",
                format!(
                    "The roast is saved, but its green weight was not taken off the stock: {}",
                    e.error
                ),
            );
        }
        self.audit(serde_json::json!({"type": "recording_saved", "path": json_path}));
        self.alarm("info", "saved", format!("Saved \"{}\".", roast.title));
        self.publish_reset();
        Ok(Saved {
            id: roast.id,
            json_path,
            alog_path,
        })
    }

    /// Marks an event at the latest reading. A person's CHARGE starts a
    /// recording if none is running; an AI's is refused, because that
    /// recording would have no beans, weight or plan (ADR 0018).
    pub fn mark_event(&mut self, kind: EventKind, origin: EventOrigin) -> Result<RoastEvent, Fail> {
        if self.roast.is_none() {
            if kind == EventKind::Charge && origin == EventOrigin::Ai {
                return Err(Fail::conflict(
                    "start a recording first (start_recording with the lot, weight and plan)",
                    None,
                ));
            }
            if kind == EventKind::Charge {
                self.start_recording(RecordingMeta::default())?;
            } else {
                return Err(Fail::conflict("start a recording first", None));
            }
        }
        let t = self.latest_t();
        let event = RoastEvent {
            t,
            kind,
            origin,
            bt: self.last_bt,
        };
        // A marked event replaces one of the same kind, a detected one
        // included: phases, DTR, the plan and the archive use the marked one.
        let mut replaced = None;
        let first_charge = kind == EventKind::Charge
            && self.roast.as_ref().is_some_and(|r| r.charge_t().is_none());
        if let Some(roast) = &mut self.roast {
            let detected = roast
                .events
                .iter()
                .find(|e| e.kind == kind && e.origin == EventOrigin::Auto)
                .map(|e| e.t);
            replaced = detected.zip(roast.charge_t());
            roast.events.retain(|e| e.kind != kind);
            roast.events.push(event.clone());
            roast.events.sort_by(|a, b| a.t.total_cmp(&b.t));
        }
        if let Some((old, charge)) = replaced.filter(|_| kind != EventKind::Charge) {
            let clock = |t: f64| {
                let s = (t - charge).max(0.0).round() as i64;
                format!("{}:{:02}", s / 60, s % 60)
            };
            self.alarm(
                "info",
                "event_replaced",
                format!(
                    "{kind} marked at {}; it replaces the one detected at {} (phases, DTR and the plan use the marked one).",
                    clock(t),
                    clock(old)
                ),
            );
        }
        match kind {
            EventKind::Charge => self.state = RoastState::Charged,
            EventKind::Drop => self.state = RoastState::Dropped,
            EventKind::CoolEnd => self.state = RoastState::Cooling,
            _ => {}
        }
        if let Some(d) = &self.driver {
            let _ = d.send(DriverCmd::Event(kind));
        }
        self.audit(serde_json::json!({"type": "event", "event": event}));
        if first_charge {
            self.on_charge(origin);
        } else if kind == EventKind::Charge && origin == EventOrigin::Human {
            // Marked over a detected CHARGE: the person is at the roaster.
            self.safety.confirm_presence(self.now());
        } else if kind != EventKind::Drop {
            // Steps after dry end or first crack start from the marked event.
            self.run_recipe();
        }
        self.publish(None);
        if kind == EventKind::Drop {
            self.after_drop();
            // Nothing after the drop is measured usefully (the only probe sits
            // in the drum), so the roast is saved right away.
            self.stop_recording()?;
        }
        Ok(event)
    }

    /// At DROP the beans are out: the burner goes off and the bean cooling on,
    /// whoever was in control, and an AI hands control back to the person.
    /// Both commands come from the safety layer, so they work without a lease
    /// and do not count as the person taking over.
    fn after_drop(&mut self) {
        let now = self.now();
        let holder = self.safety.status(now).lease_holder;
        self.command(CommandRequest {
            actuator: Actuator::Burner,
            value: 0.0,
            source: CommandSource::Safety,
            reason: Some("DROP: burner off".to_owned()),
        });
        let cooling_on = self
            .safety
            .status(now)
            .actuators
            .get(&Actuator::Cooling)
            .is_some_and(|&v| v > 0.0);
        if self.has_switch(Actuator::Cooling) && !cooling_on {
            self.command(CommandRequest {
                actuator: Actuator::Cooling,
                value: 100.0,
                source: CommandSource::Safety,
                reason: Some("DROP: bean cooling on".to_owned()),
            });
        }
        let autopilot = self.autopilot.is_some();
        if autopilot {
            self.end_autopilot("DROP: burner off, bean cooling on. The autopilot is off.");
        }
        if let Some(h) = holder.filter(|h| h != PREHEAT_HOLDER && h != AUTOPILOT_HOLDER) {
            self.safety.release_lease();
            self.audit(serde_json::json!({"type": "lease_released", "holder": h, "why": "drop"}));
            self.alarm(
                "info",
                "ai_control",
                format!("DROP: burner off, bean cooling on. AI ({h}) handed control back to you."),
            );
        } else if !autopilot {
            self.alarm(
                "info",
                "drop",
                "DROP: burner off, bean cooling on.".to_owned(),
            );
        }
    }

    /// Switches the bean cooling on once the bean temperature will reach the
    /// planned DROP within 30 s, so the tray is turning when the beans
    /// arrive. Once per roast: if the person switches it off again, it stays
    /// off until DROP.
    fn cool_before_drop(&mut self) {
        if self.state != RoastState::Charged
            || self.cooled_before_drop
            || !self.has_switch(Actuator::Cooling)
        {
            return;
        }
        let Some(roast) = &self.roast else {
            return;
        };
        let target = roast.plan.as_ref().and_then(|p| p.drop_bt_c).or_else(|| {
            self.reference
                .as_ref()
                .and_then(|r| r.event(EventKind::Drop))
                .and_then(|e| e.bt)
        });
        let (Some(target), Some(bt), Some(ror)) = (target, self.last_bt, self.last_ror) else {
            return;
        };
        if bt + ror.max(0.0) * COOL_BEFORE_DROP_S / 60.0 < target {
            return;
        }
        self.cooled_before_drop = true;
        let now = self.now();
        let on = self
            .safety
            .status(now)
            .actuators
            .get(&Actuator::Cooling)
            .is_some_and(|&v| v > 0.0);
        if !on {
            self.command(CommandRequest {
                actuator: Actuator::Cooling,
                value: 100.0,
                source: CommandSource::Safety,
                reason: Some(format!(
                    "DROP expected within {COOL_BEFORE_DROP_S:.0} s: bean cooling on"
                )),
            });
            self.alarm(
                "info",
                "cooling",
                format!("DROP at {target:.0} °C expected within {COOL_BEFORE_DROP_S:.0} s: bean cooling on."),
            );
        }
    }

    fn has_switch(&self, a: Actuator) -> bool {
        self.machine
            .as_ref()
            .is_some_and(|m| m.switches.contains(&a))
    }

    /// Removes an event (undo).
    pub fn unmark_event(&mut self, kind: EventKind) -> Result<Snapshot, Fail> {
        let roast = self
            .roast
            .as_mut()
            .ok_or_else(|| Fail::conflict("no recording is running", None))?;
        roast.events.retain(|e| e.kind != kind);
        let has = |k| roast.events.iter().any(|e| e.kind == k);
        self.state = if has(EventKind::Drop) {
            RoastState::Dropped
        } else if has(EventKind::Charge) {
            RoastState::Charged
        } else {
            RoastState::Recording
        };
        self.audit(serde_json::json!({"type": "event_removed", "kind": kind}));
        self.publish(None);
        Ok(self.snapshot())
    }

    /// Adds a note to the recording.
    pub fn add_note(&mut self, text: String, origin: EventOrigin) -> Result<Snapshot, Fail> {
        let t = self.points.last().map(|p| p.t);
        let roast = self
            .roast
            .as_mut()
            .ok_or_else(|| Fail::conflict("no recording is running", None))?;
        roast.notes.push(Note {
            t,
            text: text.clone(),
            origin,
        });
        self.audit(serde_json::json!({"type": "note", "text": text, "origin": origin}));
        self.publish(None);
        Ok(self.snapshot())
    }

    fn recover_autosave(&mut self) {
        match self.archive.recover_autosave() {
            Ok(Some(path)) => self.alarm(
                "warning",
                "recovered",
                format!(
                    "A recording was interrupted; it was recovered to {}.",
                    path.display()
                ),
            ),
            Ok(None) => {}
            Err(e) => self.alarm("warning", "recover_failed", format!("Recovery failed: {e}")),
        }
    }
}

fn machine_info(p: &MachineProfile, simulated: bool, port: Option<String>) -> MachineInfo {
    let has_et = match &p.protocol {
        roast_machine::ProtocolConfig::Tc4(t) => t.et_present,
    };
    MachineInfo {
        id: p.id.clone(),
        name: p.name.clone(),
        simulated,
        port,
        actuators: p
            .actuators
            .iter()
            .map(|a| ActuatorSpec {
                actuator: a.actuator,
                min: a.min,
                max: a.max,
                step: a.step,
                display: a.display.clone(),
            })
            .collect(),
        switches: p.switches.iter().map(|s| s.actuator).collect(),
        has_et,
        sampling_interval_s: p.sampling_interval_s,
        max_bt_c: p.limits.max_bt_c,
        link_loss_tested: p.link_loss.tested,
        link_lost: p.link_loss.tested.then(|| p.link_loss.link_lost.clone()),
        takeover_actuators: p.limits.takeover_actuators(),
    }
}

/// A sentence for a rejection reason.
#[must_use]
pub fn reject_text(r: RejectReason) -> &'static str {
    match r {
        RejectReason::EmergencyStopLatched => "The emergency stop is latched.",
        RejectReason::AiSuspendedByHuman => {
            "A person took over; AI control is suspended until they allow it again in the app."
        }
        RejectReason::NoLease => "AI control needs a lease: call request_control first.",
        RejectReason::PresenceRequired => {
            "A person must confirm being at the roaster in the app first. The app now asks them; request control again once they have."
        }
        RejectReason::OutOfRange => "The value is out of range.",
        RejectReason::RateLimited => "Too soon after the previous change.",
        RejectReason::NotSupported => "The machine has no such actuator.",
        RejectReason::Disconnected => "No roaster is connected.",
        RejectReason::Overheat => "Bean temperature is above the limit.",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn options(dir: &std::path::Path) -> EngineOptions {
        let profile = MachineProfile::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../machines/skywalker-cubean-plus.toml"
        ))
        .unwrap();
        EngineOptions {
            profile,
            store: Arc::new(Store::open(&dir.join("roastmcp.sqlite")).unwrap()),
            roasts_dir: dir.join("roasts"),
            audit_path: dir.join("actions.jsonl"),
            inventory_path: dir.join("inventory.json"),
            control_path: dir.join("control.json"),
            profiles_path: dir.join("profiles.json"),
            sim_speed: 10.0,
        }
    }

    /// The engine connected to the simulator, after its first reading.
    async fn simulator(dir: &std::path::Path) -> EngineHandle {
        let h = start(options(dir));
        h.call(|e| {
            e.connect(ConnectReq {
                simulator: true,
                port: None,
            })
        })
        .await
        .unwrap()
        .unwrap();
        for _ in 0..100 {
            if h.call(|e| e.conn == Conn::Connected).await.unwrap() {
                return h;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the simulator did not connect");
    }

    #[tokio::test]
    async fn the_preheat_sets_its_air_as_a_persons_command_and_confirms_less_often() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        // Connecting applied the safe state: air 100 %.
        let air = |s: &Snapshot| s.safety.actuators.get(&Actuator::Air).copied();
        let before = h.call(|e| e.snapshot()).await.unwrap();
        assert_eq!(air(&before), Some(100.0));

        let s = h
            .call(|e| e.start_preheat(200.0, Some(20.0)))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(air(&s), Some(20.0));
        assert!(s.preheat.is_some());
        assert_eq!(s.safety.lease_holder.as_deref(), Some(PREHEAT_HOLDER));
        let due = s.safety.presence_due_in_s.unwrap();
        assert!(due > 590.0, "presence due in {due} s");
        let log = h
            .call(|e| std::fs::read_to_string(&e.opts.audit_path).unwrap())
            .await
            .unwrap();
        assert!(log.contains(r#""source":"human""#), "{log}");

        // A person's command ends it, and the normal interval is back.
        h.call(|e| {
            e.command(CommandRequest {
                actuator: Actuator::Air,
                value: 30.0,
                source: CommandSource::Human,
                reason: None,
            })
        })
        .await
        .unwrap();
        let s = h
            .call(|e| {
                e.on_tick();
                e.snapshot()
            })
            .await
            .unwrap();
        assert!(s.preheat.is_none());
        assert!(s.safety.presence_due_in_s.unwrap() < 181.0);
        assert!(!s.safety.ai_suspended, "no AI was in control");
    }

    #[tokio::test]
    async fn an_ais_charge_needs_a_recording_a_persons_starts_one() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        let ai = h
            .call(|e| e.mark_event(EventKind::Charge, EventOrigin::Ai))
            .await
            .unwrap();
        let err = ai.unwrap_err();
        assert_eq!(err.status, 409);
        assert!(err.error.contains("start a recording first"));
        assert!(h.call(|e| e.roast.is_none()).await.unwrap());

        let person = h
            .call(|e| e.mark_event(EventKind::Charge, EventOrigin::Human))
            .await
            .unwrap();
        assert!(person.is_ok());
        assert!(h.call(|e| e.roast.is_some()).await.unwrap());
    }

    #[tokio::test]
    async fn a_passing_driver_error_clears_with_the_next_reading() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        h.call(|e| e.on_driver(DriverEvent::Error("bad line".into())))
            .await
            .unwrap();
        assert!(
            h.call(|e| e.snapshot().connection_error.is_some())
                .await
                .unwrap()
        );
        for _ in 0..100 {
            if h.call(|e| e.snapshot().connection_error.is_none())
                .await
                .unwrap()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("connection_error stuck after new readings");
    }

    fn actuator(s: &Snapshot, a: Actuator) -> Option<f64> {
        s.safety.actuators.get(&a).copied()
    }

    #[tokio::test]
    async fn drop_turns_the_burner_off_the_cooling_on_and_ends_ai_control() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        h.call(|e| e.mark_event(EventKind::Charge, EventOrigin::Human))
            .await
            .unwrap()
            .unwrap();
        h.call(|e| {
            e.command(CommandRequest {
                actuator: Actuator::Burner,
                value: 60.0,
                source: CommandSource::Human,
                reason: None,
            });
            e.resume_ai();
            e.confirm_presence();
            e.acquire_lease("claude").map(|_| ())
        })
        .await
        .unwrap()
        .unwrap();
        let s = h
            .call(|e| {
                e.mark_event(EventKind::Drop, EventOrigin::Human).unwrap();
                e.snapshot()
            })
            .await
            .unwrap();
        assert_eq!(actuator(&s, Actuator::Burner), Some(0.0));
        assert_eq!(actuator(&s, Actuator::Cooling), Some(100.0));
        assert_eq!(s.safety.lease_holder, None);
        assert!(!s.safety.ai_suspended, "handing back is not a takeover");
        assert!(
            s.alarms
                .iter()
                .any(|a| a.message.contains("handed control back")),
            "{:?}",
            s.alarms
        );
    }

    #[tokio::test]
    async fn the_cooling_starts_once_30_s_before_the_planned_drop() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        h.call(|e| {
            e.start_recording(RecordingMeta {
                plan: Some(RoastPlan {
                    drop_bt_c: Some(205.0),
                    ..RoastPlan::default()
                }),
                ..RecordingMeta::default()
            })
            .map(|_| ())
        })
        .await
        .unwrap()
        .unwrap();
        h.call(|e| {
            e.mark_event(EventKind::Charge, EventOrigin::Human)
                .map(|_| ())
        })
        .await
        .unwrap()
        .unwrap();
        let cooling_at = |bt: f64, ror: f64| {
            h.call(move |e| {
                e.last_bt = Some(bt);
                e.last_ror = Some(ror);
                e.cool_before_drop();
                actuator(&e.snapshot(), Actuator::Cooling)
            })
        };
        // 40 s away at 6 °C/min: not yet.
        assert_ne!(cooling_at(201.0, 6.0).await.unwrap(), Some(100.0));
        // 30 s away: on.
        assert_eq!(cooling_at(202.0, 6.0).await.unwrap(), Some(100.0));
        // Switched off by the person: it stays off until DROP.
        h.call(|e| {
            e.command(CommandRequest {
                actuator: Actuator::Cooling,
                value: 0.0,
                source: CommandSource::Human,
                reason: None,
            })
        })
        .await
        .unwrap();
        assert_eq!(cooling_at(203.0, 6.0).await.unwrap(), Some(0.0));
    }

    #[tokio::test]
    async fn a_marked_dry_end_replaces_the_detected_one_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        h.call(|e| {
            e.mark_event(EventKind::Charge, EventOrigin::Human)
                .map(|_| ())
        })
        .await
        .unwrap()
        .unwrap();
        let s = h
            .call(|e| {
                let roast = e.roast.as_mut().unwrap();
                let charge = roast.charge_t().unwrap();
                roast.events.push(RoastEvent {
                    t: charge - 45.0,
                    kind: EventKind::DryEnd,
                    origin: EventOrigin::Auto,
                    bt: Some(150.0),
                });
                e.mark_event(EventKind::DryEnd, EventOrigin::Human).unwrap();
                e.snapshot()
            })
            .await
            .unwrap();
        let dry: Vec<_> = s
            .events
            .iter()
            .filter(|e| e.kind == EventKind::DryEnd)
            .collect();
        assert_eq!(dry.len(), 1);
        assert_eq!(dry[0].origin, EventOrigin::Human);
        assert!(
            s.alarms
                .iter()
                .any(|a| a.kind == "event_replaced"
                    && a.message.contains("replaces the one detected")),
            "{:?}",
            s.alarms
        );
    }

    /// A profile whose recipe sets the burner to 85 % at CHARGE and to 60 %
    /// 30 s after first crack; returns its version's id.
    fn profile_with_recipe(e: &mut Engine) -> uuid::Uuid {
        use roast_core::{ActionTrigger, RecipeAnchor};
        let m = roast_core::design::TEMPLATE_MILESTONES;
        let curve = roast_core::design::template_curve(&m);
        let actions = [
            PlannedAction {
                trigger: ActionTrigger::Time { roast_time_s: 0.0 },
                actuator: Actuator::Burner,
                value: 85.0,
            },
            PlannedAction {
                trigger: ActionTrigger::Event {
                    event: RecipeAnchor::FirstCrackStart,
                    offset_s: 30.0,
                },
                actuator: Actuator::Burner,
                value: 60.0,
            },
        ];
        let target = roast_core::design::to_target(&curve, &m, &actions);
        let detail = e
            .profiles
            .create(
                crate::profiles::NewProfile {
                    name: "Test".into(),
                    description: None,
                    intended_beans: None,
                    batch_size_g: None,
                    target,
                    change_note: None,
                },
                None,
            )
            .unwrap();
        let v = serde_json::to_value(&detail).unwrap();
        uuid::Uuid::parse_str(v["versions"][0]["id"].as_str().unwrap()).unwrap()
    }

    fn with_profile(e: &mut Engine) {
        let version = profile_with_recipe(e);
        e.start_recording(RecordingMeta {
            plan: Some(RoastPlan {
                profile_version_id: Some(version),
                ..RoastPlan::default()
            }),
            ..RecordingMeta::default()
        })
        .unwrap();
    }

    #[tokio::test]
    async fn an_ai_takes_over_during_the_preheat_which_holds_until_charge() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        let s = h
            .call(|e| {
                e.start_preheat(200.0, Some(20.0)).unwrap();
                e.acquire_lease("mcp:claude").unwrap();
                e.on_tick();
                e.snapshot()
            })
            .await
            .unwrap();
        assert!(s.preheat.is_some(), "the preheat keeps holding");
        assert_eq!(s.safety.lease_holder.as_deref(), Some("mcp:claude"));
        assert!(
            s.safety.presence_due_in_s.unwrap() < 181.0,
            "the AI confirms every 3 min"
        );
        // The AI lets go before CHARGE: the preheat takes its lease back.
        let s = h
            .call(|e| {
                e.release_lease();
                e.snapshot()
            })
            .await
            .unwrap();
        assert!(s.preheat.is_some());
        assert_eq!(s.safety.lease_holder.as_deref(), Some(PREHEAT_HOLDER));
        // A person's filter does not end anything; their burner does.
        let s = h
            .call(|e| {
                e.acquire_lease("mcp:claude").unwrap();
                e.command(CommandRequest {
                    actuator: Actuator::Filter,
                    value: 60.0,
                    source: CommandSource::Human,
                    reason: None,
                });
                e.on_tick();
                e.snapshot()
            })
            .await
            .unwrap();
        assert!(s.preheat.is_some());
        assert_eq!(s.safety.lease_holder.as_deref(), Some("mcp:claude"));
        let s = h
            .call(|e| {
                e.command(CommandRequest {
                    actuator: Actuator::Burner,
                    value: 30.0,
                    source: CommandSource::Human,
                    reason: None,
                });
                e.on_tick();
                e.snapshot()
            })
            .await
            .unwrap();
        assert!(s.preheat.is_none());
        assert!(s.safety.ai_suspended, "an AI was in control");
    }

    #[tokio::test]
    async fn the_recipe_runs_under_the_ai_from_a_persons_charge() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        let s = h
            .call(|e| {
                with_profile(e);
                e.start_preheat(200.0, Some(20.0)).unwrap();
                e.acquire_lease("mcp:claude").unwrap();
                // Holding power, as the preheat would have it.
                e.command(CommandRequest {
                    actuator: Actuator::Burner,
                    value: 36.0,
                    source: CommandSource::Ai,
                    reason: Some("test".into()),
                });
                e.mark_event(EventKind::Charge, EventOrigin::Human).unwrap();
                e.snapshot()
            })
            .await
            .unwrap();
        assert!(s.preheat.is_none());
        assert_eq!(s.safety.lease_holder.as_deref(), Some("mcp:claude"));
        // The CHARGE step jumps: no 10-point ramp in the first 10 s.
        assert_eq!(actuator(&s, Actuator::Burner), Some(85.0));
        let recipe = s.recipe.unwrap();
        assert!(recipe.automatic);
        assert_eq!(recipe.steps[0].state, StepState::Done);
        let next = s.reference.unwrap().next_step.unwrap();
        assert!(next.automatic && next.projected);
        assert_eq!(next.label, "FC +0:30");

        // First crack, marked 40 s ago: its step comes now.
        let s = h
            .call(|e| {
                e.mark_event(EventKind::FirstCrackStart, EventOrigin::Ai)
                    .unwrap();
                let roast = e.roast.as_mut().unwrap();
                for ev in &mut roast.events {
                    if ev.kind == EventKind::FirstCrackStart {
                        ev.t -= 40.0;
                    }
                }
                e.run_recipe();
                e.snapshot()
            })
            .await
            .unwrap();
        assert_eq!(actuator(&s, Actuator::Burner), Some(60.0));
        assert!(
            s.alarms
                .iter()
                .any(|a| a.message == "Recipe FC +0:30: burner 60 %."),
            "{:?}",
            s.alarms
        );
        let log = h
            .call(|e| e.roast.as_ref().unwrap().control_log.clone())
            .await
            .unwrap();
        assert!(
            log.iter()
                .any(|l| l.request.source == CommandSource::Autopilot
                    && l.request.reason.as_deref() == Some("recipe 0:00: burner 85 %"))
        );
    }

    #[tokio::test]
    async fn the_autopilot_takes_over_at_charge_trims_and_yields_to_a_hand() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        // It follows a profile: without one it is refused.
        let refused = h.call(|e| e.start_autopilot()).await.unwrap();
        assert_eq!(refused.unwrap_err().status, 409);
        let s = h
            .call(|e| {
                with_profile(e);
                e.start_preheat(200.0, Some(20.0)).unwrap();
                e.start_autopilot().unwrap()
            })
            .await
            .unwrap();
        assert_eq!(s.autopilot.as_ref().unwrap().state, "armed");
        assert_eq!(s.safety.lease_holder.as_deref(), Some(PREHEAT_HOLDER));

        // CHARGE: the autopilot holds the lease and the recipe runs.
        let s = h
            .call(|e| {
                e.mark_event(EventKind::Charge, EventOrigin::Human).unwrap();
                e.snapshot()
            })
            .await
            .unwrap();
        assert!(s.preheat.is_none());
        assert_eq!(s.safety.lease_holder.as_deref(), Some(AUTOPILOT_HOLDER));
        assert_eq!(s.autopilot.as_ref().unwrap().state, "active");
        assert!(s.recipe.as_ref().unwrap().automatic);
        assert_eq!(actuator(&s, Actuator::Burner), Some(85.0));

        // Five minutes in, 8 °C below the plan and no faster than it: one
        // correction of five points, which the recipe's later step keeps.
        let s = h
            .call(|e| {
                let roast = e.roast.as_mut().unwrap();
                for ev in &mut roast.events {
                    if ev.kind == EventKind::Charge {
                        ev.t -= 300.0;
                    }
                }
                let since = e.since_charge().unwrap();
                let (plan_bt, plan_ror) = e.reference.as_ref().unwrap().at(since);
                e.last_bt = Some(plan_bt.unwrap() - 8.0);
                e.last_ror = plan_ror;
                e.burner_changed_at = Some(e.latest_t() - 100.0);
                // The safety layer paces automatic raises.
                e.safety.allow_charge_jump(e.now());
                e.autopilot_trim();
                let first = e.snapshot();
                // Until the correction shows, it leaves the burner alone.
                e.burner_changed_at = Some(e.latest_t() - 100.0);
                e.autopilot_trim();
                assert_eq!(e.snapshot().autopilot.unwrap().corrections, 1);
                first
            })
            .await
            .unwrap();
        assert_eq!(actuator(&s, Actuator::Burner), Some(90.0), "{:?}", s.alarms);
        let ap = s.autopilot.as_ref().unwrap();
        assert_eq!((ap.offset, ap.corrections), (5.0, 1));
        let later = &s.recipe.as_ref().unwrap().steps[1];
        assert!((later.value - 65.0).abs() < 1e-9, "{later:?}");

        // A hand on the burner ends it.
        let s = h
            .call(|e| {
                e.command(CommandRequest {
                    actuator: Actuator::Burner,
                    value: 70.0,
                    source: CommandSource::Human,
                    reason: None,
                });
                e.autopilot_lease();
                e.snapshot()
            })
            .await
            .unwrap();
        assert!(s.autopilot.is_none());
        assert!(s.safety.ai_suspended);
        assert_eq!(s.safety.lease_holder, None);
        assert_eq!(actuator(&s, Actuator::Burner), Some(70.0));

        // Switching it on again is the person's word that automation may go
        // on; the earlier correction still counts.
        let s = h.call(|e| e.start_autopilot()).await.unwrap().unwrap();
        let ap = s.autopilot.unwrap();
        assert_eq!((ap.state, ap.offset, ap.corrections), ("active", 5.0, 0));
        assert_eq!(s.safety.lease_holder.as_deref(), Some(AUTOPILOT_HOLDER));
    }

    #[tokio::test]
    async fn a_persons_charge_confirms_presence_a_detected_one_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        let (detected, marked) = h
            .call(|e| {
                e.start_recording(RecordingMeta::default()).unwrap();
                e.on_charge(EventOrigin::Auto);
                let detected = e.snapshot().safety.presence_due_in_s;
                e.mark_event(EventKind::Charge, EventOrigin::Human).unwrap();
                (detected, e.snapshot().safety.presence_due_in_s)
            })
            .await
            .unwrap();
        assert_eq!(detected, None);
        assert!(marked.is_some_and(|d| d > 170.0), "{marked:?}");
    }

    #[tokio::test]
    async fn without_an_ai_the_recipe_is_a_cue_and_can_be_adjusted_for_this_roast() {
        let dir = tempfile::tempdir().unwrap();
        let h = simulator(dir.path()).await;
        let s = h
            .call(|e| {
                with_profile(e);
                e.mark_event(EventKind::Charge, EventOrigin::Human).unwrap();
                e.snapshot()
            })
            .await
            .unwrap();
        assert_ne!(actuator(&s, Actuator::Burner), Some(85.0), "a cue only");
        let recipe = s.recipe.unwrap();
        assert!(!recipe.automatic);
        assert!(!s.reference.unwrap().next_step.unwrap().automatic);

        let unknown = h
            .call(|e| {
                e.adjust_recipe(
                    &[StepChange {
                        step: 7,
                        skip: Some(true),
                        ..StepChange::default()
                    }],
                    None,
                    EventOrigin::Ai,
                )
                .map(|_| ())
            })
            .await
            .unwrap();
        assert_eq!(unknown.unwrap_err().status, 400);
        let s = h
            .call(|e| {
                e.adjust_recipe(
                    &[StepChange {
                        step: 1,
                        shift_s: Some(15.0),
                        value: Some(55.0),
                        skip: None,
                    }],
                    Some("first crack runs hot".into()),
                    EventOrigin::Ai,
                )
                .unwrap()
            })
            .await
            .unwrap();
        let recipe = s.recipe.unwrap();
        assert!(recipe.adjusted);
        assert_eq!(recipe.steps[1].label, "FC +0:45");
        assert_eq!(recipe.steps[1].planned_value, Some(60.0));
        let notes = h
            .call(|e| e.roast.as_ref().unwrap().notes.clone())
            .await
            .unwrap();
        assert!(
            notes.iter().any(|n| n.text.contains("FC +0:45 burner 55 %")
                && n.text.contains("first crack runs hot")),
            "{notes:?}"
        );
    }
}

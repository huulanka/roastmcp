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
use roast_control::{PreheatController, PreheatInput, PreheatPhase, PreheatReason, PreheatTuning};
use roast_core::{
    Actuator, ActuatorValues, BatchInfo, CommandOutcome, CommandRequest, CommandSource,
    ControlLogEntry, EventDetector, EventKind, EventOrigin, Note, Phases, Projection,
    ReferenceCurve, RejectReason, Roast, RoastEvent, RoastOrigin, RoastPlan, RoastState, RorConfig,
    RorTracker, Sample, phases, project,
};
use roast_driver::{RoasterDriver, SimulatedMachine, simulator_profile};
use roast_machine::MachineProfile;
use roast_safety::{Safety, SafetyConfig, SafetyReaction, SafetyStatus};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::driver_thread::{DriverCmd, DriverEvent, spawn_driver};
use crate::files;

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
    /// Where roasts are saved.
    pub roasts_dir: PathBuf,
    /// Append-only action log (JSON lines).
    pub audit_path: PathBuf,
    /// The green coffee inventory (ADR 0014).
    pub inventory_path: PathBuf,
    /// What automatic control has learned about each machine (ADR 0015).
    pub control_path: PathBuf,
    /// Roast profiles and their versions.
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
}

/// How long an AI's refused control request keeps the presence prompt up.
const CONTROL_REQUEST_SHOWN_FOR: Duration = Duration::from_secs(120);

/// The reference curve at the current moment of the roast.
#[derive(Debug, Clone, Serialize)]
struct ReferenceStatus {
    id: uuid::Uuid,
    title: String,
    /// Reference bean temperature at the same time since CHARGE.
    bt_now: Option<f64>,
    /// Live bean temperature minus `bt_now`.
    delta_bt: Option<f64>,
    ror_now: Option<f64>,
    /// Reference events, `t` relative to its CHARGE.
    events: Vec<RoastEvent>,
}

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
}

/// Starts the engine task and its safety ticker.
pub fn start(opts: EngineOptions) -> EngineHandle {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (updates, _) = broadcast::channel(256);
    let audit = files::open_append(&opts.audit_path).ok();
    let inventory = InventoryStore::load(opts.inventory_path.clone());
    let learned = LearnedStore::load(opts.control_path.clone());
    let profiles = ProfileStore::load(opts.profiles_path.clone());
    let mut engine = Engine {
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
        control_request: None,
        last_autosave: Instant::now(),
        alarms: VecDeque::new(),
        next_alarm: 1,
        audit,
        reference: None,
        preheat: None,
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
            reference: self.reference_status(t, charge_t),
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

    fn reference_status(&self, t: Option<f64>, charge_t: Option<f64>) -> Option<ReferenceStatus> {
        let r = self.reference.as_ref()?;
        let (bt_now, ror_now) = match (t, charge_t) {
            (Some(t), Some(c)) => r.at(t - c),
            _ => (None, None),
        };
        Some(ReferenceStatus {
            id: r.id,
            title: r.title.clone(),
            bt_now,
            delta_bt: self.last_bt.zip(bt_now).map(|(live, rf)| live - rf),
            ror_now,
            events: r.events.clone(),
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

    // ---------------------------------------------------------------- preheat

    /// Starts preheating to `target_c` (bean probe). A person starts it in
    /// the app, which confirms presence and allows automation; it ends at
    /// CHARGE, on any manual command, an emergency stop or a lost link.
    pub fn start_preheat(&mut self, target_c: f64) -> Result<Snapshot, Fail> {
        let max = self.safety.config().max_bt_c - 10.0;
        if !(50.0..=max).contains(&target_c) {
            return Err(Fail::bad(format!(
                "the preheat target must be between 50 and {max:.0} °C"
            )));
        }
        if self.roast.as_ref().is_some_and(|r| r.charge_t().is_some()) {
            return Err(Fail::conflict("the roast has started (CHARGE)", None));
        }
        let now = self.now();
        self.safety.resume_ai();
        self.safety.confirm_presence(now);
        self.safety
            .acquire_lease(PREHEAT_HOLDER, now)
            .map_err(|r| Fail::conflict(reject_text(r), Some(r)))?;
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
        });
        self.audit(serde_json::json!({
            "type": "preheat_started",
            "target_c": target_c,
            "air": air,
            "ror_per_pct": plant.ror_per_pct,
            "hold_pct_per_c": plant.hold_pct_per_c,
        }));
        self.alarm(
            "info",
            "preheat",
            format!("Preheating to {target_c:.0} °C. Confirm presence while it runs."),
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
            "hold_pct_per_c": learned.hold_pct_per_c,
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
        if status.lease_holder.as_deref() != Some(PREHEAT_HOLDER) {
            // A manual command ended preheat; that suspends automation, but
            // no AI was in control, so do not report the AI as paused.
            if status.lease_holder.is_none() && status.ai_suspended {
                self.safety.resume_ai();
            }
            self.end_preheat("Preheat ended - you have control.");
            return;
        }
        // One decision per reading.
        let Some(t) = self.last_reading_t else {
            return;
        };
        if p.last_seen_t.is_some_and(|seen| seen >= t) {
            return;
        }
        if self.safety.acquire_lease(PREHEAT_HOLDER, now).is_err() {
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
                let (_, roast) = files::find(&self.opts.roasts_dir, id)
                    .ok_or_else(|| Fail::not_found(format!("no saved roast {id}")))?;
                Some(ReferenceCurve::from_roast(&roast, &RorConfig::default()))
            }
        };
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
                self.publish(None);
            }
        }
    }

    fn on_reading(&mut self, r: roast_driver::Reading) {
        self.safety.observe(&r.actuators);
        self.last_reading = Some(Instant::now());
        self.stale_alarmed = false;
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
        for e in found {
            match e.kind {
                EventKind::Charge if self.state == RoastState::Recording => {
                    self.state = RoastState::Charged;
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
        self.publish(Some(&point));
        if dropped {
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
                let _ = files::autosave(&self.opts.roasts_dir, roast);
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
        let now = self.now();
        let before = self.safety.status(now).lease_holder;
        match self.safety.acquire_lease(holder, now) {
            Ok(l) => {
                self.control_request = None;
                if before.as_deref() != Some(holder) {
                    self.audit(serde_json::json!({"type": "lease", "holder": holder}));
                    self.alarm("info", "ai_control", format!("AI ({holder}) has control."));
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

    /// The AI gives up control; actuators hold.
    pub fn release_lease(&mut self) -> Snapshot {
        if self.safety.status(self.now()).lease_holder.is_some() {
            self.safety.release_lease();
            self.audit(serde_json::json!({"type": "lease_released"}));
            self.alarm(
                "info",
                "ai_released",
                "AI released control - you have control.",
            );
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
        let saved = files::save(&self.opts.roasts_dir, &roast);
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
        files::clear_autosave(&self.opts.roasts_dir);
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

    /// Marks an event at the latest reading. Starts a recording on CHARGE if
    /// none is running.
    pub fn mark_event(&mut self, kind: EventKind, origin: EventOrigin) -> Result<RoastEvent, Fail> {
        if self.roast.is_none() {
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
        if let Some(roast) = &mut self.roast {
            roast.events.retain(|e| e.kind != kind);
            roast.events.push(event.clone());
            roast.events.sort_by(|a, b| a.t.total_cmp(&b.t));
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
        self.publish(None);
        if kind == EventKind::Drop {
            // Nothing after the drop is measured usefully (the only probe sits
            // in the drum), so the roast is saved right away. Cooling stays a
            // switch under the controls; the safety layer keeps running.
            self.stop_recording()?;
        }
        Ok(event)
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
        match files::recover_autosave(&self.opts.roasts_dir) {
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

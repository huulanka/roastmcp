//! HTTP and WebSocket API (`docs/api.md`), and the static web UI.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use roast_core::{CommandRequest, CommandSource, EventKind, EventOrigin};
use serde::Deserialize;
use tower_http::services::{ServeDir, ServeFile};

use crate::backup::{self, BackupConfig, BackupStatus};
use crate::engine::{ConnectReq, EngineHandle, Fail, RecordingMeta};
use crate::experiments::{ExperimentPatch, Experiments, NewExperiment};
use crate::files::{self, Archive};
use crate::inventory::{AdjustReq, LotPatch, NewLot, PurchaseReq};

/// Shared state of the handlers.
#[derive(Clone)]
pub struct AppState {
    /// The engine.
    pub engine: EngineHandle,
    /// The bearer token.
    pub token: Arc<str>,
    /// The saved roasts.
    pub archive: Archive,
    /// The database (ADR 0003).
    pub store: Arc<roast_store::Store>,
    /// Signalled by `POST /api/shutdown`.
    pub shutdown: Arc<tokio::sync::Notify>,
}

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self)).into_response()
    }
}

type ApiResult<T> = Result<Json<T>, Fail>;

fn engine_gone(e: &anyhow::Error) -> Fail {
    Fail::internal(e.to_string())
}

/// The router: `/api/*` with token auth, the UI at `/`.
pub fn router(state: AppState, ui_dir: Option<PathBuf>) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/live", get(live))
        .route("/ports", get(ports))
        .route("/connect", post(connect))
        .route("/disconnect", post(disconnect))
        .route("/command", post(command))
        .route("/emergency-stop", post(emergency_stop))
        .route("/emergency-stop/reset", post(emergency_stop_reset))
        .route("/presence", post(presence))
        .route("/ai/resume", post(ai_resume))
        .route("/ai/lease", post(lease).delete(release))
        .route("/recording/start", post(recording_start))
        .route("/recording", axum::routing::patch(recording_patch))
        .route("/recording/stop", post(recording_stop))
        .route("/events", post(mark_event))
        .route("/events/{kind}", delete(unmark_event))
        .route("/notes", post(note))
        .route("/recipe/adjust", post(recipe_adjust))
        .route("/roasts", get(roasts))
        .route(
            "/roasts/import",
            post(roast_import).layer(axum::extract::DefaultBodyLimit::max(IMPORT_MAX_BYTES)),
        )
        .route("/experiments", get(experiments).post(experiment_add))
        .route(
            "/experiments/{id}",
            get(experiment)
                .patch(experiment_patch)
                .delete(experiment_delete),
        )
        .route("/roasts/compare", get(roast_compare))
        .route("/consistency", get(consistency))
        .route(
            "/roasts/{id}",
            get(roast).patch(roast_patch).delete(roast_delete),
        )
        .route("/roasts/{id}/alog", get(roast_alog))
        .route("/ws", get(ws))
        .route("/shutdown", post(shutdown))
        .route("/reference", get(reference).put(set_reference))
        .route("/preheat", post(preheat_start).delete(preheat_stop))
        .route("/autopilot", post(autopilot_start).delete(autopilot_stop))
        .route("/profiles", get(profiles_list).post(profile_add))
        .route("/profiles/{id}", get(profile_get).patch(profile_patch))
        .route("/profiles/{id}/versions", post(profile_version_add))
        .route("/profile-versions/{id}", get(profile_version_get))
        .route("/backup", get(backup_status).put(backup_set))
        .route("/backup/run", post(backup_run))
        .route("/lots", get(lots).post(lot_add))
        .route("/lots/{id}", get(lot).patch(lot_patch))
        .route("/lots/{id}/movements", get(lot_movements))
        .route("/lots/{id}/purchases", post(lot_purchase))
        .route("/lots/{id}/adjustments", post(lot_adjust))
        .layer(middleware::from_fn_with_state(state.clone(), auth));

    let mut app = Router::new().nest("/api", api);
    if let Some(dir) = ui_dir {
        app = app.merge(ui_router(dir));
    }
    app.with_state(state)
}

/// The web UI: files from `dir`, `index.html` for any other path (the app
/// routes on the client), with cache headers.
fn ui_router<S: Clone + Send + Sync + 'static>(dir: PathBuf) -> Router<S> {
    let index = dir.join("index.html");
    Router::new()
        .fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
        .layer(middleware::from_fn(ui_cache_headers))
}

/// Cache rule for a path of the web UI. The build puts content-hashed files
/// under `/assets/`; they never change, so they may be cached for good.
/// Everything else (`index.html`, and the page served for any other path)
/// must be revalidated, or the desktop app's web view keeps showing the old
/// UI after an update.
fn ui_cache_control(path: &str) -> &'static str {
    if path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

async fn ui_cache_headers(req: Request, next: Next) -> Response {
    let rule = ui_cache_control(req.uri().path());
    let mut res = next.run(req).await;
    if res.status().is_success() || res.status() == StatusCode::NOT_MODIFIED {
        res.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static(rule),
        );
    }
    res
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

async fn auth(
    State(st): State<AppState>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let ok = bearer.or(q.token.as_deref()) == Some(&*st.token);
    if ok {
        next.run(req).await
    } else {
        Fail {
            status: 401,
            error: "missing or invalid token".into(),
            reason: None,
        }
        .into_response()
    }
}

#[derive(Deserialize)]
struct PreheatBody {
    target_c: f64,
    /// The air to preheat at, % (ADR 0018); without it the applied air stays.
    #[serde(default)]
    air: Option<f64>,
}

async fn preheat_start(
    State(st): State<AppState>,
    Json(b): Json<PreheatBody>,
) -> impl IntoResponse {
    match st
        .engine
        .call(move |e| e.start_preheat(b.target_c, b.air))
        .await
    {
        Ok(Ok(s)) => Json(s).into_response(),
        Ok(Err(f)) => f.into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

async fn preheat_stop(State(st): State<AppState>) -> impl IntoResponse {
    match st.engine.call(|e| e.stop_preheat()).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

/// The app's autopilot switch (ADR 0020). Switching it on is a person's
/// word that automation may run and that they are at the roaster.
async fn autopilot_start(State(st): State<AppState>) -> impl IntoResponse {
    match st.engine.call(|e| e.start_autopilot()).await {
        Ok(Ok(s)) => Json(s).into_response(),
        Ok(Err(f)) => f.into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

async fn autopilot_stop(State(st): State<AppState>) -> impl IntoResponse {
    match st.engine.call(|e| e.stop_autopilot()).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

#[derive(Deserialize)]
struct ReferenceBody {
    /// A saved roast; neither this nor a profile version clears the reference.
    #[serde(default)]
    id: Option<uuid::Uuid>,
    /// A profile version: its target curve and recipe become the plan line.
    #[serde(default)]
    profile_version_id: Option<uuid::Uuid>,
}

async fn reference(State(st): State<AppState>) -> impl IntoResponse {
    match st.engine.call(|e| e.reference()).await {
        Ok(r) => Json(r).into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

async fn set_reference(
    State(st): State<AppState>,
    Json(b): Json<ReferenceBody>,
) -> impl IntoResponse {
    let set = move |e: &mut crate::engine::Engine| match b.profile_version_id {
        Some(v) => e.set_reference_profile(v),
        None => e.set_reference(b.id),
    };
    match st.engine.call(set).await {
        Ok(Ok(s)) => Json(s).into_response(),
        Ok(Err(f)) => f.into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

/// Asks the daemon to exit gracefully (safe state if the burner is on, the
/// recording saved), as Ctrl-C does. Used by the desktop app on quit.
async fn shutdown(State(st): State<AppState>) -> StatusCode {
    st.shutdown.notify_one();
    StatusCode::ACCEPTED
}

async fn status(State(st): State<AppState>) -> impl IntoResponse {
    match st.engine.call(|e| e.snapshot()).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

async fn live(State(st): State<AppState>) -> impl IntoResponse {
    match st.engine.call(|e| e.live()).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

async fn ports() -> Json<Vec<String>> {
    Json(
        tokio::task::spawn_blocking(roast_driver::list_ports)
            .await
            .unwrap_or_default(),
    )
}

async fn connect(State(st): State<AppState>, Json(req): Json<ConnectReq>) -> impl IntoResponse {
    flat(st.engine.call(move |e| e.connect(req)).await)
}

async fn disconnect(State(st): State<AppState>) -> impl IntoResponse {
    ok(st.engine.call(|e| e.disconnect()).await)
}

#[derive(Deserialize)]
struct CommandBody {
    actuator: roast_core::Actuator,
    value: f64,
    #[serde(default)]
    source: Option<CommandSource>,
    reason: Option<String>,
}

async fn command(State(st): State<AppState>, Json(b): Json<CommandBody>) -> impl IntoResponse {
    let source = match b.source {
        None | Some(CommandSource::Human) => CommandSource::Human,
        Some(CommandSource::Ai) => CommandSource::Ai,
        Some(_) => return Fail::bad_request("source must be human or ai").into_response(),
    };
    let req = CommandRequest {
        actuator: b.actuator,
        value: b.value,
        source,
        reason: b.reason,
    };
    match st.engine.call(move |e| e.command(req)).await {
        Ok(outcome) => Json(serde_json::json!({ "outcome": outcome })).into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

#[derive(Deserialize, Default)]
struct ReasonBody {
    reason: Option<String>,
}

async fn emergency_stop(
    State(st): State<AppState>,
    body: Option<Json<ReasonBody>>,
) -> impl IntoResponse {
    let reason = body
        .and_then(|Json(b)| b.reason)
        .unwrap_or_else(|| "button".into());
    ok(st.engine.call(move |e| e.emergency_stop(&reason)).await)
}

async fn emergency_stop_reset(State(st): State<AppState>) -> impl IntoResponse {
    ok(st.engine.call(|e| e.reset_emergency_stop()).await)
}

async fn presence(State(st): State<AppState>) -> impl IntoResponse {
    ok(st.engine.call(|e| e.confirm_presence()).await)
}

async fn ai_resume(State(st): State<AppState>) -> impl IntoResponse {
    ok(st.engine.call(|e| e.resume_ai()).await)
}

#[derive(Deserialize)]
struct LeaseBody {
    holder: String,
}

async fn lease(State(st): State<AppState>, Json(b): Json<LeaseBody>) -> impl IntoResponse {
    flat(st.engine.call(move |e| e.acquire_lease(&b.holder)).await)
}

async fn release(State(st): State<AppState>) -> impl IntoResponse {
    ok(st.engine.call(|e| e.release_lease()).await)
}

async fn recording_start(
    State(st): State<AppState>,
    body: Option<Json<RecordingMeta>>,
) -> impl IntoResponse {
    let meta = body.map(|Json(m)| m).unwrap_or_default();
    flat(st.engine.call(move |e| e.start_recording(meta)).await)
}

async fn recording_patch(
    State(st): State<AppState>,
    Json(meta): Json<RecordingMeta>,
) -> impl IntoResponse {
    flat(st.engine.call(move |e| e.update_recording(meta)).await)
}

async fn recording_stop(State(st): State<AppState>) -> impl IntoResponse {
    flat(st.engine.call(|e| e.stop_recording()).await)
}

#[derive(Deserialize)]
struct EventBody {
    kind: EventKind,
    #[serde(default)]
    origin: Option<EventOrigin>,
}

async fn mark_event(State(st): State<AppState>, Json(b): Json<EventBody>) -> impl IntoResponse {
    let origin = match b.origin {
        Some(EventOrigin::Ai) => EventOrigin::Ai,
        _ => EventOrigin::Human,
    };
    flat(st.engine.call(move |e| e.mark_event(b.kind, origin)).await)
}

async fn unmark_event(State(st): State<AppState>, Path(kind): Path<String>) -> impl IntoResponse {
    let Ok(kind) = serde_json::from_value::<EventKind>(serde_json::Value::String(kind)) else {
        return Fail::bad_request("unknown event kind").into_response();
    };
    flat(st.engine.call(move |e| e.unmark_event(kind)).await).into_response()
}

#[derive(Deserialize)]
struct NoteBody {
    text: String,
    #[serde(default)]
    origin: Option<EventOrigin>,
}

async fn note(State(st): State<AppState>, Json(b): Json<NoteBody>) -> impl IntoResponse {
    let origin = match b.origin {
        Some(EventOrigin::Ai) => EventOrigin::Ai,
        _ => EventOrigin::Human,
    };
    flat(st.engine.call(move |e| e.add_note(b.text, origin)).await)
}

#[derive(Deserialize)]
struct RecipeAdjustBody {
    changes: Vec<roast_control::StepChange>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    origin: Option<EventOrigin>,
}

async fn recipe_adjust(
    State(st): State<AppState>,
    Json(b): Json<RecipeAdjustBody>,
) -> impl IntoResponse {
    let origin = match b.origin {
        Some(EventOrigin::Ai) => EventOrigin::Ai,
        _ => EventOrigin::Human,
    };
    flat(
        st.engine
            .call(move |e| e.adjust_recipe(&b.changes, b.reason, origin))
            .await,
    )
}

/// Runs a blocking archive or database call off the async threads.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> Result<T, Fail> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Fail::internal(e.to_string()))?
        .map_err(|e| Fail::internal(e.to_string()))
}

/// Runs a blocking database call that fails with its own status.
async fn blocking_fail<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, Fail> + Send + 'static,
) -> ApiResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Fail::internal(e.to_string()))?
        .map(Json)
}

fn experiments_of(st: &AppState) -> Experiments {
    Experiments::new(st.store.clone(), st.archive.clone())
}

fn experiment_id(id: &str) -> Result<uuid::Uuid, Fail> {
    id.parse()
        .map_err(|_| Fail::not_found("no such experiment"))
}

async fn experiments(State(st): State<AppState>) -> ApiResult<Vec<roast_core::Experiment>> {
    let x = experiments_of(&st);
    blocking_fail(move || x.list()).await
}

async fn experiment_add(
    State(st): State<AppState>,
    Json(new): Json<NewExperiment>,
) -> ApiResult<roast_core::Experiment> {
    let x = experiments_of(&st);
    blocking_fail(move || x.create(new)).await
}

async fn experiment(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<roast_core::Experiment> {
    let (x, id) = (experiments_of(&st), experiment_id(&id)?);
    blocking_fail(move || x.get(id)).await
}

async fn experiment_patch(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(p): Json<ExperimentPatch>,
) -> ApiResult<roast_core::Experiment> {
    let (x, id) = (experiments_of(&st), experiment_id(&id)?);
    blocking_fail(move || x.update(id, p)).await
}

async fn experiment_delete(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let (x, id) = (experiments_of(&st), experiment_id(&id)?);
    blocking_fail(move || x.delete(id).map(|()| serde_json::json!({}))).await
}

async fn roasts(State(st): State<AppState>) -> ApiResult<Vec<serde_json::Value>> {
    let archive = st.archive.clone();
    blocking(move || archive.summaries()).await.map(Json)
}

async fn find(st: &AppState, id: &str) -> Result<roast_core::Roast, Fail> {
    let id: uuid::Uuid = id.parse().map_err(|_| Fail::not_found("no such roast"))?;
    let archive = st.archive.clone();
    blocking(move || archive.find(id))
        .await?
        .ok_or_else(|| Fail::not_found("no such roast"))
}

async fn roast(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<serde_json::Value> {
    let r = find(&st, &id).await?;
    with_ror(&r).map(Json)
}

#[derive(Deserialize)]
struct CompareQuery {
    a: String,
    b: String,
    /// What the curves are lined up on; CHARGE without it.
    #[serde(default)]
    align: roast_core::Alignment,
    /// Seconds between the rows of the curve; 30 without it.
    #[serde(default)]
    step_s: Option<f64>,
}

/// A roast's name for a comparison or a group.
fn roast_line(r: &roast_core::Roast) -> serde_json::Value {
    let line = files::summary(r);
    serde_json::json!({
        "id": r.id,
        "title": r.title,
        "roasted_at": r.roasted_at,
        "batch_no": serde_json::to_value(&line)
            .ok()
            .and_then(|v| v.get("batch_no").cloned()),
    })
}

/// Two saved roasts side by side: figures, differences `a − b` and both
/// curves on one clock (`roast-core::compare`).
async fn roast_compare(
    State(st): State<AppState>,
    Query(q): Query<CompareQuery>,
) -> ApiResult<serde_json::Value> {
    let step = q.step_s.unwrap_or(30.0);
    if !(1.0..=600.0).contains(&step) {
        return Err(Fail::bad("step_s must be between 1 and 600"));
    }
    let (a, b) = (find(&st, &q.a).await?, find(&st, &q.b).await?);
    let c = roast_core::compare(&a, &b, q.align, step);
    let mut out = serde_json::to_value(&c).map_err(|e| Fail::internal(e.to_string()))?;
    if let Some(o) = out.as_object_mut() {
        o.insert("roast_a".into(), roast_line(&a));
        o.insert("roast_b".into(), roast_line(&b));
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
struct ConsistencyQuery {
    /// Roasts of this green coffee lot.
    #[serde(default)]
    lot_id: Option<String>,
    /// Roasts that followed a version of this profile.
    #[serde(default)]
    profile_id: Option<String>,
    /// These roasts, ids separated by commas.
    #[serde(default)]
    ids: Option<String>,
    /// Count simulator roasts too.
    #[serde(default)]
    simulated: bool,
}

/// How alike the roasts of a lot, of a profile or of a chosen set are: the
/// spread of their figures and curves, and each roast's own figures.
/// `unfinished` counts the recordings without CHARGE or DROP that were left
/// out.
async fn consistency(
    State(st): State<AppState>,
    Query(q): Query<ConsistencyQuery>,
) -> ApiResult<serde_json::Value> {
    let group = match (q.lot_id, q.profile_id, q.ids) {
        (Some(lot), None, None) => files::Group::Lot(lot_id(&lot)?),
        (None, Some(profile), None) => {
            let id = profile_id(&profile)?;
            let versions = st
                .engine
                .call(move |e| e.profiles.version_ids(id))
                .await
                .map_err(|e| engine_gone(&e))??;
            files::Group::Versions(versions)
        }
        (None, None, Some(ids)) => files::Group::Ids(
            ids.split(',')
                .map(|id| id.trim().parse())
                .collect::<Result<_, _>>()
                .map_err(|_| Fail::bad("ids must be roast ids separated by commas"))?,
        ),
        _ => return Err(Fail::bad("give one of lot_id, profile_id or ids")),
    };
    let archive = st.archive.clone();
    let simulated = q.simulated;
    let mut roasts = blocking(move || archive.group(&group, simulated)).await?;
    // A recording that was stopped before DROP is not a roast to compare.
    let found = roasts.len();
    roasts.retain(|r| roast_core::figures(r).total_s.is_some());
    let unfinished = found - roasts.len();
    let refs: Vec<&roast_core::Roast> = roasts.iter().collect();
    let roast_lines: Vec<serde_json::Value> = roasts
        .iter()
        .map(|r| {
            let mut line = roast_line(r);
            if let Some(o) = line.as_object_mut() {
                o.insert(
                    "figures".into(),
                    serde_json::to_value(roast_core::figures(r)).unwrap_or_default(),
                );
            }
            line
        })
        .collect();
    Ok(Json(serde_json::json!({
        "consistency": roast_core::consistency(&refs),
        "roasts": roast_lines,
        "unfinished": unfinished,
    })))
}

/// Largest `.alog` an import accepts (a long roast is well under 1 MB).
const IMPORT_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
struct ImportQuery {
    /// The uploaded file's name; it titles a roast without a title.
    #[serde(default)]
    name: String,
}

/// Imports an uploaded Artisan `.alog` into the archive.
async fn roast_import(
    State(st): State<AppState>,
    Query(q): Query<ImportQuery>,
    body: axum::body::Bytes,
) -> ApiResult<serde_json::Value> {
    let archive = st.archive.clone();
    let imported = tokio::task::spawn_blocking(move || archive.import_alog_bytes(&q.name, &body))
        .await
        .map_err(|e| Fail::internal(e.to_string()))?
        .map_err(|e| Fail::bad(format!("not a readable .alog file: {e}")))?;
    let (id, already_there) = match imported {
        files::Imported::Saved(id) => (id, false),
        files::Imported::AlreadyThere(id) => (id, true),
    };
    let r = find(&st, &id.to_string()).await?;
    Ok(Json(serde_json::json!({
        "id": r.id,
        "title": r.title,
        "already_there": already_there,
    })))
}

/// A roast as JSON with each sample's rate of rise (`ror`, °C/min, one
/// decimal), computed exactly as the live value, so a saved curve looks like
/// it did during the roast. The files themselves carry no RoR.
fn with_ror(r: &roast_core::Roast) -> Result<serde_json::Value, Fail> {
    let rors = roast_core::ror_series(&r.samples, &roast_core::RorConfig::default());
    let mut v = serde_json::to_value(r).map_err(|e| Fail::internal(e.to_string()))?;
    if let Some(samples) = v.get_mut("samples").and_then(|s| s.as_array_mut()) {
        for (s, ror) in samples.iter_mut().zip(rors) {
            if let (Some(obj), Some(ror)) = (s.as_object_mut(), ror) {
                obj.insert("ror".into(), ((ror * 10.0).round() / 10.0).into());
            }
        }
    }
    Ok(v)
}

/// Deletes a roast: its files move to the archive's `trash/` folder and its
/// stock debit is taken back.
async fn roast_delete(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let r = find(&st, &id).await?;
    let (rid, title) = (r.id, r.title.clone());
    let archive = st.archive.clone();
    let moved = tokio::task::spawn_blocking(move || archive.trash(rid))
        .await
        .map_err(|e| Fail::internal(e.to_string()))?
        .map_err(|e| Fail::internal(format!("the roast could not be moved to the trash: {e}")))?;
    let credited_g = st
        .engine
        .call(move |e| e.roast_deleted(rid, &title))
        .await
        .map_err(|e| engine_gone(&e))??;
    let x = experiments_of(&st);
    let Json(()) = blocking_fail(move || x.roast_deleted(rid)).await?;
    Ok(Json(serde_json::json!({
        "credited_g": credited_g,
        "trash_path": moved,
    })))
}

#[derive(Deserialize)]
struct RoastPatch {
    title: Option<String>,
    roasted_weight_g: Option<f64>,
    green_weight_g: Option<f64>,
    notes: Option<String>,
    /// Who wrote the note; a person without it.
    #[serde(default)]
    note_origin: Option<EventOrigin>,
    /// Measured roast colour; replaces the stored one.
    roast_degree: Option<roast_core::RoastDegree>,
    /// Room temperature, humidity and air pressure during the roast;
    /// replaces the stored ones.
    #[serde(default)]
    ambient: Option<roast_core::Ambient>,
}

async fn roast_patch(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(p): Json<RoastPatch>,
) -> ApiResult<serde_json::Value> {
    let mut r = find(&st, &id).await?;
    if let Some(t) = p.title.filter(|t| !t.trim().is_empty()) {
        r.title = t;
    }
    if p.roasted_weight_g.is_some() {
        r.batch.roasted_weight_g = p.roasted_weight_g;
    }
    if p.green_weight_g.is_some() {
        r.batch.green_weight_g = p.green_weight_g;
    }
    if let Some(d) = p.roast_degree {
        // No reading at all clears the measurement.
        r.roast_degree = (d.whole_bean.is_some() || d.ground.is_some()).then_some(d);
    }
    if let Some(a) = p.ambient {
        let plausible = a.temperature_c.is_none_or(|t| (-30.0..=60.0).contains(&t))
            && a.humidity_pct.is_none_or(|h| (0.0..=100.0).contains(&h))
            && a.pressure_hpa.is_none_or(|p| (500.0..=1100.0).contains(&p));
        if !plausible {
            return Err(Fail::bad(
                "ambient: temperature -30 to 60 °C, humidity 0 to 100 %, pressure 500 to 1100 hPa",
            ));
        }
        // Nothing measured clears them.
        r.ambient = (a != roast_core::Ambient::default()).then_some(a);
    }
    if let Some(text) = p.notes.filter(|t| !t.trim().is_empty()) {
        r.notes.push(roast_core::Note {
            t: None,
            text: text.trim().to_owned(),
            origin: match p.note_origin {
                Some(EventOrigin::Ai) => EventOrigin::Ai,
                _ => EventOrigin::Human,
            },
        });
    }
    let saved = r.clone();
    let archive = st.archive.clone();
    blocking(move || archive.update(&saved)).await?;
    if p.green_weight_g.is_some() {
        // One debit per roast: a corrected green weight replaces it.
        let debit = r.clone();
        st.engine
            .call(move |e| e.inventory.sync_roast_debit(&debit))
            .await
            .map_err(|e| engine_gone(&e))??;
    }
    with_ror(&r).map(Json)
}

// --------------------------------------------------------------- backup

async fn backup_status(State(st): State<AppState>) -> Json<BackupStatus> {
    Json(backup::status(&st.store))
}

#[derive(Deserialize)]
struct BackupSet {
    /// The backup folder (a full path); `null` or empty turns backups off.
    dir: Option<String>,
    /// How many backups stay in the folder.
    keep: Option<usize>,
}

async fn backup_set(
    State(st): State<AppState>,
    Json(req): Json<BackupSet>,
) -> ApiResult<BackupStatus> {
    let store = Arc::clone(&st.store);
    tokio::task::spawn_blocking(move || {
        let config = BackupConfig {
            dir: req
                .dir
                .map(|d| d.trim().to_owned())
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
            keep: req.keep.unwrap_or_else(|| backup::config(&store).keep),
        };
        backup::set_config(&store, &config).map_err(Fail::bad)?;
        Ok(Json(backup::status(&store)))
    })
    .await
    .map_err(|e| Fail::internal(e.to_string()))?
}

/// Makes a backup now.
async fn backup_run(State(st): State<AppState>) -> ApiResult<BackupStatus> {
    let store = Arc::clone(&st.store);
    tokio::task::spawn_blocking(move || {
        backup::run(&store).map_err(Fail::bad)?;
        Ok(Json(backup::status(&store)))
    })
    .await
    .map_err(|e| Fail::internal(e.to_string()))?
}

// ------------------------------------------------------------- profiles

fn profile_id(id: &str) -> Result<uuid::Uuid, Fail> {
    id.parse().map_err(|_| Fail::not_found("no such profile"))
}

#[derive(Deserialize)]
struct ArchivedQuery {
    #[serde(default)]
    archived: bool,
}

async fn profiles_list(
    State(st): State<AppState>,
    Query(q): Query<ArchivedQuery>,
) -> ApiResult<Vec<crate::profiles::ProfileSummary>> {
    st.engine
        .call(move |e| e.profiles.list(q.archived))
        .await
        .map(Json)
        .map_err(|e| engine_gone(&e))
}

async fn profile_add(
    State(st): State<AppState>,
    Json(req): Json<crate::profiles::NewProfile>,
) -> ApiResult<crate::profiles::ProfileDetail> {
    st.engine
        .call(move |e| {
            let machine = e.machine_id();
            e.profiles.create(req, machine)
        })
        .await
        .map_err(|e| engine_gone(&e))?
        .map(Json)
}

async fn profile_get(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<crate::profiles::ProfileDetail> {
    let id = profile_id(&id)?;
    st.engine
        .call(move |e| e.profiles.get(id))
        .await
        .map_err(|e| engine_gone(&e))?
        .map(Json)
}

async fn profile_patch(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(p): Json<crate::profiles::ProfilePatch>,
) -> ApiResult<crate::profiles::ProfileDetail> {
    let id = profile_id(&id)?;
    st.engine
        .call(move |e| e.profiles.update(id, p))
        .await
        .map_err(|e| engine_gone(&e))?
        .map(Json)
}

/// One version with its profile, by the version's id (a roast's plan names it).
async fn profile_version_get(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let id = profile_id(&id)?;
    let (profile, version) = st
        .engine
        .call(move |e| e.profiles.version(id))
        .await
        .map_err(|e| engine_gone(&e))??;
    Ok(Json(
        serde_json::json!({ "profile": profile, "version": version }),
    ))
}

async fn profile_version_add(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<crate::profiles::NewVersion>,
) -> ApiResult<crate::profiles::ProfileDetail> {
    let id = profile_id(&id)?;
    st.engine
        .call(move |e| e.profiles.add_version(id, req))
        .await
        .map_err(|e| engine_gone(&e))?
        .map(Json)
}

// ------------------------------------------------------------- inventory

#[derive(Deserialize)]
struct LotsQuery {
    #[serde(default)]
    archived: bool,
}

fn lot_id(id: &str) -> Result<uuid::Uuid, Fail> {
    id.parse()
        .map_err(|_| Fail::not_found("no such green coffee lot"))
}

async fn lots(State(st): State<AppState>, Query(q): Query<LotsQuery>) -> impl IntoResponse {
    ok(st.engine.call(move |e| e.inventory.lots(q.archived)).await)
}

async fn lot(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    match lot_id(&id) {
        Ok(id) => flat(st.engine.call(move |e| e.inventory.lot(id)).await),
        Err(f) => f.into_response(),
    }
}

async fn lot_movements(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    match lot_id(&id) {
        Ok(id) => flat(st.engine.call(move |e| e.inventory.movements(id)).await),
        Err(f) => f.into_response(),
    }
}

async fn lot_add(State(st): State<AppState>, Json(b): Json<NewLot>) -> impl IntoResponse {
    flat(st.engine.call(move |e| e.inventory.add_lot(b)).await)
}

async fn lot_patch(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<LotPatch>,
) -> Response {
    match lot_id(&id) {
        Ok(id) => flat(st.engine.call(move |e| e.inventory.update_lot(id, b)).await),
        Err(f) => f.into_response(),
    }
}

async fn lot_purchase(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<PurchaseReq>,
) -> Response {
    match lot_id(&id) {
        Ok(id) => flat(
            st.engine
                .call(move |e| e.inventory.add_purchase(id, b))
                .await,
        ),
        Err(f) => f.into_response(),
    }
}

async fn lot_adjust(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<AdjustReq>,
) -> Response {
    match lot_id(&id) {
        Ok(id) => flat(st.engine.call(move |e| e.inventory.adjust(id, b)).await),
        Err(f) => f.into_response(),
    }
}

async fn roast_alog(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    match find(&st, &id).await {
        Ok(r) => {
            let name = format!("{}.alog", r.title.replace(['"', '/', '\\'], "_"));
            (
                [
                    (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_owned()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{name}\""),
                    ),
                ],
                roast_alog::write_alog(&r),
            )
                .into_response()
        }
        Err(f) => f.into_response(),
    }
}

async fn ws(State(st): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| ws_loop(socket, st))
}

async fn ws_loop(mut socket: WebSocket, st: AppState) {
    let mut rx = st.engine.updates.subscribe();
    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Ok(text) => {
                    if socket.send(Message::Text(text.as_ref().into())).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Too slow: make the client reload the curve.
                    if socket.send(Message::Text(r#"{"type":"reset"}"#.into())).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
}

fn ok<T: serde::Serialize>(r: anyhow::Result<T>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

fn flat<T: serde::Serialize>(r: anyhow::Result<Result<T, Fail>>) -> Response {
    match r {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(f)) => f.into_response(),
        Err(e) => engine_gone(&e).into_response(),
    }
}

impl Fail {
    fn bad_request(msg: &str) -> Self {
        Fail {
            status: 400,
            error: msg.to_owned(),
            reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    async fn cache_header(app: Router, path: &str) -> (StatusCode, Option<String>) {
        let res = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let cc = res
            .headers()
            .get(header::CACHE_CONTROL)
            .map(|v| v.to_str().unwrap().to_owned());
        (res.status(), cc)
    }

    #[tokio::test]
    async fn the_page_is_revalidated_and_hashed_assets_are_cached() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<!doctype html>").unwrap();
        std::fs::create_dir(dir.path().join("assets")).unwrap();
        std::fs::write(dir.path().join("assets/index-abc123.js"), "").unwrap();
        let app: Router = ui_router(dir.path().to_path_buf());

        for path in ["/", "/index.html", "/archive"] {
            let (status, cc) = cache_header(app.clone(), path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(cc.as_deref(), Some("no-cache"), "{path}");
        }
        let (status, cc) = cache_header(app, "/assets/index-abc123.js").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cc.as_deref(), Some("public, max-age=31536000, immutable"));
    }

    #[test]
    fn a_saved_roast_comes_with_the_live_rate_of_rise() {
        let r = roast_core::Roast {
            id: uuid::Uuid::new_v4(),
            title: "t".into(),
            roasted_at: chrono::Utc::now(),
            machine: None,
            sampling_interval_s: 1.5,
            batch: roast_core::BatchInfo::default(),
            ambient: None,
            roast_degree: None,
            green_lot_id: None,
            plan: None,
            profile_version_id: None,
            experiment_id: None,
            origin: roast_core::RoastOrigin::Recorded,
            samples: (0..40)
                .map(|i| {
                    let t = f64::from(i) * 1.5;
                    roast_core::Sample::bt(t, 150.0 + 0.2 * t)
                })
                .collect(),
            events: Vec::new(),
            control_log: Vec::new(),
            notes: Vec::new(),
        };
        let v = with_ror(&r).unwrap();
        let samples = v["samples"].as_array().unwrap();
        assert!(samples[0].get("ror").is_none(), "too few readings at first");
        let last = samples.last().unwrap()["ror"].as_f64().unwrap();
        assert!(
            (last - 12.0).abs() < 0.05,
            "0.2 °C/s is 12 °C/min, got {last}"
        );
    }
}

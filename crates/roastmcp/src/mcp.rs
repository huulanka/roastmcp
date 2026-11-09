//! A Model Context Protocol server on stdio (newline-delimited JSON-RPC 2.0).
//! It forwards every tool to the daemon's HTTP API, so all rules of the
//! safety layer apply; it cannot confirm presence or reset an emergency stop.

use std::io::{BufRead as _, Write as _};
use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;
use roast_core::design::{self, CurvePoint, Milestones};
use roast_core::{ActionTrigger, Actuator, PlannedAction, ProfileTarget, Roast};
use serde_json::{Value, json};

const PROTOCOL_VERSION: &str = "2025-06-18";

const INSTRUCTIONS: &str = "RoastMCP connects you to a coffee roaster. Temperatures are °C, \
times seconds, actuator values percent. Read with get_status and get_curve; use wait to follow \
a roast. get_status also shows the reference (an earlier roast, or a profile's plan): its BT and \
RoR at the same time since CHARGE and the difference to now, and for a profile the next recipe \
step and when it is due; plus a projection (BT in 60 s, the next milestone and when it is \
reached). set_reference picks a roast or a profile. Profiles are the roaster's plans: read them \
with list_profiles and get_profile, develop them with the person through create_profile and \
add_profile_version (every change is a new version they see at once in the app's Profile tab), \
and roast one with start_recording(profile). When you control a roast that follows a profile, \
make the recipe's moves yourself around their time, and correct only when the deviation from \
the plan persists. The person does CHARGE and DROP by hand; announce them. \
To control the roaster, a person must first confirm presence in the RoastMCP app, then \
call request_control. Your commands pass a safety layer: cutting the burner or adding air is \
never limited; raising the burner is limited to 10 points per command and one raise per 10 s, \
lowering the air to 20 points per 5 s. Act like an experienced roaster: anticipate the thermal \
lag, make few deliberate moves (for example cut heat well before first crack) instead of many \
small corrections. Anything the person does in the app overrides you and suspends your control. Keep calling wait or get_status at least every 60 s \
while in control, or control returns to the person. Always give a short reason with each \
command; it is logged with the roast. Use emergency_stop if anything looks unsafe.";

struct Daemon {
    base: String,
    token: String,
    agent: ureq::Agent,
    holder: String,
    holding: bool,
}

impl Daemon {
    fn connect(data_dir: &Path) -> anyhow::Result<Self> {
        let info = roastd::read_daemon_info(data_dir)?;
        Ok(Daemon {
            base: format!("http://127.0.0.1:{}/api", info.port),
            token: info.token,
            // Error statuses are read like any answer: their body says what went wrong.
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(40)))
                .http_status_as_error(false)
                .build()
                .into(),
            holder: "mcp".into(),
            holding: false,
        })
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> anyhow::Result<Value> {
        let url = format!("{}{path}", self.base);
        let auth = format!("Bearer {}", self.token);
        let res = match (method, body) {
            ("GET", _) => self.agent.get(&url).header("Authorization", &auth).call(),
            ("DELETE", _) => self
                .agent
                .delete(&url)
                .header("Authorization", &auth)
                .call(),
            ("POST", Some(b)) => self
                .agent
                .post(&url)
                .header("Authorization", &auth)
                .send_json(b),
            ("POST", None) => self
                .agent
                .post(&url)
                .header("Authorization", &auth)
                .send_empty(),
            ("PUT", Some(b)) => self
                .agent
                .put(&url)
                .header("Authorization", &auth)
                .send_json(b),
            ("PATCH", Some(b)) => self
                .agent
                .patch(&url)
                .header("Authorization", &auth)
                .send_json(b),
            (m, _) => anyhow::bail!("unsupported request method {m}"),
        };
        let mut r = res.context("the RoastMCP daemon is not reachable")?;
        let status = r.status();
        let text = r.body_mut().read_to_string().unwrap_or_default();
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            Ok(v)
        } else {
            // Our errors are `{"error": …}`; a body the router rejected
            // (malformed field) is plain text: pass that on, the AI can fix it.
            let msg = v
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if text.trim().is_empty() {
                        format!("request failed ({status})")
                    } else {
                        text.trim().to_owned()
                    }
                });
            anyhow::bail!("{msg}")
        }
    }

    fn renew(&mut self) {
        if self.holding {
            let r = self.call("POST", "/ai/lease", Some(json!({ "holder": self.holder })));
            if r.is_err() {
                self.holding = false;
            }
        }
    }
}

/// Input of `add_lot`: the fields of an importer's offer sheet (ADR 0014).
fn add_lot_schema() -> Value {
    let num = |desc: &str| json!({"type": "number", "description": desc});
    json!({"type": "object", "properties": {
            "name": {"type": "string", "description": "Short name used everywhere, e.g. \"GL-BR-001 Brasilien Amarelo\"."},
            "grams": num("Grams bought."),
            "date": {"type": "string", "description": "Purchase day, YYYY-MM-DD (default today)."},
            "origin_country": {"type": "string"}, "region": {"type": "string"},
            "producer": {"type": "string", "description": "Producer, farm, cooperative or washing station."},
            "altitude_min_m": num("Altitude, metres (or the lower bound)."), "altitude_max_m": num("Upper bound of the altitude, metres."),
            "species": {"type": "string", "enum": ["arabica", "robusta", "liberica", "excelsa"]},
            "varieties": {"type": "array", "items": {"type": "string"}},
            "process": {"type": "string", "enum": ["washed", "natural", "honey", "pulped_natural", "semi_washed", "wet_hulled", "other"], "description": "Base method; fermentation techniques go in fermentation."},
            "honey": {"type": "string", "enum": ["white", "yellow", "red", "black"], "description": "Only for process honey."},
            "fermentation": {"type": "array", "items": {"type": "string", "enum": ["anaerobic", "carbonic_maceration", "thermal_shock", "extended", "inoculated", "infused"]}},
            "fermentation_hours": num("Fermentation time, hours."),
            "drying": {"type": "string", "enum": ["raised_beds", "patio", "greenhouse", "mechanical"]},
            "decaf": {"type": "string", "enum": ["swiss_water", "mountain_water", "ethyl_acetate", "co2", "methylene_chloride"]},
            "process_detail": {"type": "string", "description": "Processing as on the offer sheet, free text."},
            "harvest": {"type": "string", "description": "Crop year, e.g. 2025/26."},
            "arrival_date": {"type": "string", "description": "YYYY-MM-DD."},
            "grade": {"type": "string", "description": "E.g. Excelso EP, SHB, AA, G1, NY2/3."},
            "screen_size": {"type": "string", "description": "E.g. 15+, 17/18."},
            "moisture_pct": num("Moisture, percent."), "water_activity": num("Water activity, aw (0-1)."),
            "density_g_per_l": num("Bulk density, g/l."), "cupping_score": num("The supplier's SCA score."),
            "flavor_notes": {"type": "string", "description": "The supplier's tasting notes."},
            "supplier": {"type": "string", "description": "Importer or shop."},
            "supplier_lot": {"type": "string", "description": "The supplier's lot number or code."},
            "price_per_kg": num("Price per kg in EUR."),
            "packaging": {"type": "string", "enum": ["grainpro", "vacuum", "jute", "ecotact", "other"]},
            "storage_location": {"type": "string"},
            "certifications": {"type": "array", "items": {"type": "string", "enum": ["organic", "fairtrade", "rainforest_alliance"]}},
            "notes": {"type": "string"}},
          "required": ["name"]})
}

/// Input of the profile tools: where the roast should be when.
fn milestones_schema() -> Value {
    let point = |what: &str| {
        json!({"type": "object", "description": what, "properties": {
            "time_s": {"type": "number", "description": "Roast time, seconds since CHARGE."},
            "bt_c": {"type": "number", "description": "Bean temperature, °C."}}})
    };
    json!({"type": "object", "description": "Any of these; the rest stay as in the starting curve.", "properties": {
        "charge_bt_c": {"type": "number", "description": "Bean temperature at CHARGE, °C (also the preheat target)."},
        "dry_end": point("Dry end."),
        "first_crack": point("First crack start."),
        "drop": point("Drop.")}})
}

/// Input of the profile tools: burner and air steps.
fn recipe_schema() -> Value {
    json!({"type": "array", "description": "The whole recipe: burner and air steps by roast time (or by bean temperature after the turning point). Include the settings at CHARGE (at_s 0); air at 0 is also the preheat air.",
        "items": {"type": "object", "properties": {
            "actuator": {"type": "string", "enum": ["burner", "air"]},
            "value": {"type": "number", "description": "Percent, 0-100."},
            "at_s": {"type": "number", "description": "Roast time, seconds since CHARGE."},
            "at_bt_c": {"type": "number", "description": "Or: when BT first reaches this after the turning point, °C."}},
          "required": ["actuator", "value"]}})
}

/// m:ss for people.
fn clock(s: f64) -> String {
    // Roast times are minutes, not years.
    #[allow(clippy::cast_possible_truncation)]
    let s = s.round().max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The milestones as an AI reads them.
fn milestones_json(m: &Milestones) -> Value {
    let p = |c: CurvePoint| json!({"time_s": c.t.round(), "clock": clock(c.t), "bt_c": (c.bt * 10.0).round() / 10.0});
    let total = m.drop.t.max(1.0);
    let pct = |v: f64| (v / total * 1000.0).round() / 10.0;
    json!({
        "charge_bt_c": (m.charge * 10.0).round() / 10.0,
        "dry_end": p(m.dry_end),
        "first_crack": p(m.first_crack),
        "drop": p(m.drop),
        "phases_pct": {
            "drying": pct(m.dry_end.t),
            "maillard": pct(m.first_crack.t - m.dry_end.t),
            "development": pct(m.drop.t - m.first_crack.t),
        },
    })
}

/// A recipe as an AI reads and writes it.
fn recipe_json(actions: &[PlannedAction]) -> Value {
    actions
        .iter()
        .map(|a| {
            let mut v = json!({"actuator": a.actuator, "value": a.value});
            match a.trigger {
                ActionTrigger::Time { roast_time_s } => {
                    v["at_s"] = json!(roast_time_s);
                    v["clock"] = json!(clock(roast_time_s));
                }
                ActionTrigger::Bt { bt_c } => v["at_bt_c"] = json!(bt_c),
            }
            v
        })
        .collect()
}

fn parse_recipe(v: &Value) -> anyhow::Result<Vec<PlannedAction>> {
    let items = v.as_array().context("recipe must be a list of steps")?;
    items
        .iter()
        .map(|i| {
            let actuator = match i.get("actuator").and_then(Value::as_str) {
                Some("burner") => Actuator::Burner,
                Some("air") => Actuator::Air,
                other => {
                    anyhow::bail!("recipe step: actuator must be burner or air, not {other:?}")
                }
            };
            let value = i
                .get("value")
                .and_then(Value::as_f64)
                .filter(|v| (0.0..=100.0).contains(v))
                .context("recipe step: value must be 0-100 %")?;
            let trigger = match (
                i.get("at_s").and_then(Value::as_f64),
                i.get("at_bt_c").and_then(Value::as_f64),
            ) {
                (Some(t), _) if (0.0..=1800.0).contains(&t) => ActionTrigger::Time {
                    roast_time_s: t.round(),
                },
                (None, Some(bt)) if (50.0..=300.0).contains(&bt) => ActionTrigger::Bt { bt_c: bt },
                _ => anyhow::bail!(
                    "recipe step: give at_s (0-1800 s since CHARGE) or at_bt_c (50-300 °C)"
                ),
            };
            Ok(PlannedAction {
                trigger,
                actuator,
                value,
            })
        })
        .collect()
}

/// Milestones changed by what the AI passed.
fn apply_milestones(mut m: Milestones, v: Option<&Value>) -> anyhow::Result<Milestones> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(m);
    };
    if let Some(c) = v.get("charge_bt_c").and_then(Value::as_f64) {
        m.charge = c;
    }
    for (key, point) in [
        ("dry_end", &mut m.dry_end),
        ("first_crack", &mut m.first_crack),
        ("drop", &mut m.drop),
    ] {
        if let Some(p) = v.get(key) {
            if let Some(t) = p.get("time_s").and_then(Value::as_f64) {
                point.t = t;
            }
            if let Some(bt) = p.get("bt_c").and_then(Value::as_f64) {
                point.bt = bt;
            }
        }
    }
    if let Some(problem) = design::milestones_problem(&m) {
        anyhow::bail!("{problem}");
    }
    Ok(m)
}

/// A saved roast's curve, milestones and recipe.
fn roast_plan(
    d: &Daemon,
    id: &str,
) -> anyhow::Result<(Vec<CurvePoint>, Milestones, Vec<PlannedAction>)> {
    let v = d.call("GET", &format!("/roasts/{}", id.trim()), None)?;
    let roast: Roast = serde_json::from_value(v).context("the roast could not be read")?;
    let (curve, m) = design::from_roast(&roast)
        .with_context(|| format!("roast {id} has no CHARGE or too few readings to start from"))?;
    Ok((curve, m, design::recipe_from_roast(&roast, 8.0)))
}

/// The plan to save: a starting curve moved to the milestones, and the recipe.
fn build_plan(
    d: &Daemon,
    base: Option<&ProfileTarget>,
    args: &Value,
) -> anyhow::Result<ProfileTarget> {
    let from_roast = args.get("from_roast").and_then(Value::as_str);
    let (curve, m, actions) = match (from_roast, base) {
        (Some(id), _) => roast_plan(d, id)?,
        (None, Some(t)) => {
            let (curve, m) = design::from_target(t);
            (curve, m, t.actions.clone())
        }
        (None, None) => {
            let m = design::TEMPLATE_MILESTONES;
            (design::template_curve(&m), m, Vec::new())
        }
    };
    let to = apply_milestones(m, args.get("milestones"))?;
    let curve = if to == m {
        curve
    } else {
        design::warp(&curve, &m, &to)
    };
    let actions = match args.get("recipe").filter(|v| !v.is_null()) {
        Some(r) => parse_recipe(r)?,
        None => actions,
    };
    let mut target = design::to_target(&curve, &to, &actions);
    if let Some(b) = base.filter(|_| from_roast.is_none()) {
        target.tolerance = b.tolerance;
    }
    Ok(target)
}

/// A profile as an AI reads it: milestones, recipe and a coarse curve per version.
fn profile_json(detail: &Value, versions: usize) -> anyhow::Result<Value> {
    let mut out = detail.clone();
    let list = detail
        .get("versions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let compact: Vec<Value> = list
        .iter()
        .take(versions)
        .map(|v| {
            let target: ProfileTarget = serde_json::from_value(v["target"].clone())
                .context("a profile version could not be read")?;
            let (_, m) = design::from_target(&target);
            let mut next = f64::MIN;
            let curve: Vec<Value> = target
                .points
                .iter()
                .filter(|p| {
                    let keep = p.roast_time_s >= next
                        || Some(p.roast_time_s) == target.points.last().map(|l| l.roast_time_s);
                    if keep {
                        next = p.roast_time_s + 15.0;
                    }
                    keep
                })
                .map(|p| json!({"t": p.roast_time_s, "bt": p.bt_c, "ror": p.ror_c_per_min}))
                .collect();
            Ok(json!({
                "id": v["id"],
                "version": v["version"],
                "parent_version": v["parent_version"],
                "created_at": v["created_at"],
                "change_note": v["change_note"],
                "milestones": milestones_json(&m),
                "recipe": recipe_json(&target.actions),
                "curve_every_15s": curve,
            }))
        })
        .collect::<anyhow::Result<_>>()?;
    out["versions_total"] = json!(list.len());
    out["versions"] = Value::Array(compact);
    Ok(out)
}

fn tools() -> Value {
    let num = |desc: &str| json!({"type": "number", "description": desc});
    let reason = json!({"type": "string", "description": "Why - logged with the roast."});
    let set = |what: &str, range: &str| {
        json!({
            "type": "object",
            "properties": { "value": num(&format!("{what}, percent ({range}).")), "reason": reason.clone() },
            "required": ["value", "reason"]
        })
    };
    let kinds = json!([
        "charge",
        "turning_point",
        "dry_end",
        "first_crack_start",
        "first_crack_end",
        "second_crack_start",
        "second_crack_end",
        "drop",
        "cool_end"
    ]);
    json!([
        {"name": "get_status", "description": "Current state: connection, roast state, BT, rate of rise (°C/min), actuators, events, phases, safety (lease, presence, emergency stop), recent alarms.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "get_curve", "description": "The curve of the running roast (or the preview), downsampled. Times are recording seconds; subtract charge_t for roast time.",
         "inputSchema": {"type": "object", "properties": {
            "every_s": num("Spacing between points, seconds (default 10)."),
            "last_s": num("Only the last N seconds (default: all).")}}},
        {"name": "wait", "description": "Waits up to 30 s, then returns get_status. Keeps your control lease alive.",
         "inputSchema": {"type": "object", "properties": {"seconds": num("1-30")}, "required": ["seconds"]}},
        {"name": "mark_event", "description": "Marks a roast event at the current time (e.g. first_crack_start when you hear it reported). charge needs a running recording: call start_recording first.",
         "inputSchema": {"type": "object", "properties": {"kind": {"type": "string", "enum": kinds}}, "required": ["kind"]}},
        {"name": "add_note", "description": "Adds a note to the running roast.",
         "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}},
        {"name": "request_control", "description": "Asks for control of the roaster. Needs a recent presence confirmation by a person in the app.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "release_control", "description": "Gives control back to the person; actuators hold.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "set_burner", "description": "Sets the burner (heater). Cuts are unlimited; raises at most 10 points per command, one raise per 10 s.", "inputSchema": set("Burner", "0-100")},
        {"name": "set_air", "description": "Sets the airflow (fan). More air is unlimited; less air at most 20 points per 5 s.", "inputSchema": set("Air", "0-100")},
        {"name": "set_drum", "description": "Sets the drum speed, in percent of the machine's range. People think in rpm: get_status lists machine.actuators[].display where the machine knows it (rpm = percent × scale + offset; Cubean+: 80 % ≈ 55 rpm, 100 % ≈ 68 rpm), so convert before calling and speak rpm to the person.", "inputSchema": set("Drum", "machine range")},
        {"name": "emergency_stop", "description": "Emergency stop: burner off, air high, drum running; latched until a person resets it in the app.",
         "inputSchema": {"type": "object", "properties": {"reason": {"type": "string"}}, "required": ["reason"]}},
        {"name": "list_roasts", "description": "Saved roasts, newest first.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "list_profiles", "description": "Roast profiles the roaster developed: name, intended beans, batch size and the newest version (id, number, change note). A version's curve, milestones and recipe come with get_profile.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "get_profile", "description": "One profile with its versions, newest first: each with its milestones (charge BT; dry end, first crack and drop as roast time and BT), phase shares, recipe (burner and air steps) and target curve every 15 s (roast time, BT, RoR).",
         "inputSchema": {"type": "object", "properties": {
            "id": {"type": "string", "description": "Profile id or exact name (see list_profiles)."},
            "all_versions": {"type": "boolean", "description": "All versions instead of the newest three."}}, "required": ["id"]}},
        {"name": "create_profile", "description": "Creates a profile (version 1) with the person: a target curve shaped by milestones, and a recipe. The curve starts from a saved roast (from_roast: its curve and recipe) or from a typical 200 g Cubean+ espresso roast, then moves to the milestones you give (the curve between them is stretched, keeping its natural shape). Agree name, milestones and recipe with the person first; it appears at once in the app's Profile tab.",
         "inputSchema": {"type": "object", "properties": {
            "name": {"type": "string"},
            "description": {"type": "string"},
            "intended_beans": {"type": "string", "description": "The coffee it is for, e.g. a lot name."},
            "batch_size_g": num("Green weight it is made for, grams."),
            "from_roast": {"type": "string", "description": "Id of a saved roast to start from (see list_roasts)."},
            "milestones": milestones_schema(),
            "recipe": recipe_schema(),
            "change_note": {"type": "string", "description": "Why it looks like this; shown with version 1."}},
          "required": ["name"]}},
        {"name": "add_profile_version", "description": "Saves a changed plan as the profile's next version: it starts from the newest version (or from_roast) and changes only what you pass. Milestones you leave out stay where they are; a recipe you pass replaces the whole recipe. Always give the reason as change_note.",
         "inputSchema": {"type": "object", "properties": {
            "profile": {"type": "string", "description": "Profile id or exact name (see list_profiles)."},
            "from_roast": {"type": "string", "description": "Start from this saved roast's curve and recipe instead of the newest version."},
            "milestones": milestones_schema(),
            "recipe": recipe_schema(),
            "change_note": {"type": "string", "description": "What changed and why, e.g. \"FC 15 s later: BR011 developed too fast\"."}},
          "required": ["profile", "change_note"]}},
        {"name": "get_roast", "description": "A saved roast with its curve downsampled to every 10 s.",
         "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}},
        {"name": "list_lots", "description": "The green coffee in stock: lots with name, origin, process, supplier, purchase date and stock_g (grams; negative if more was roasted than recorded as bought). Archived lots only when asked.",
         "inputSchema": {"type": "object", "properties": {"include_archived": {"type": "boolean"}}}},
        {"name": "add_lot", "description": "Records a new green coffee lot (a coffee not in the inventory yet) with the grams bought and, as far as known, the details of the importer's offer sheet. Use add_purchase for more of a lot that exists. Only name is required; leave out what you do not know.",
         "inputSchema": add_lot_schema()},
        {"name": "add_purchase", "description": "Records a purchase of more of an existing lot.",
         "inputSchema": {"type": "object", "properties": {
            "lot": {"type": "string", "description": "Lot id or exact name (see list_lots)."},
            "grams": num("Grams bought."),
            "date": {"type": "string", "description": "Purchase day, YYYY-MM-DD (default today)."},
            "note": {"type": "string"}},
          "required": ["lot", "grams"]}},
        {"name": "start_recording", "description": "Prepares a roast exactly like the app's start dialog and starts the recording: the lot (its stock is debited by the green weight when the roast is saved after CHARGE), green weight, goal, the reference roast as plan line, and the targets. Moves no actuator; the person charges the beans (CHARGE is detected or marked). Ask the person for anything you are unsure of.",
         "inputSchema": {"type": "object", "properties": {
            "lot": {"type": "string", "description": "Lot id or exact name (see list_lots)."},
            "green_weight_g": num("Green weight, grams."),
            "goal": {"type": "string", "enum": ["espresso", "filter", "omni", "other"]},
            "reference": {"type": "string", "description": "Id of a saved roast to follow (see list_roasts); its curve becomes the plan line."},
            "profile": {"type": "string", "description": "A profile to follow instead of a roast: profile id or exact name (its newest version; see list_profiles). Its curve becomes the plan line and its recipe is announced to the person."},
            "drop_bt_c": num("Bean temperature to drop at, °C."),
            "dtr_pct": num("Development time ratio to drop at, percent."),
            "batch_no": {"type": "string", "description": "Batch number for the bag's label, the lot's code and its next running number, e.g. BRYB-002 (list_roasts shows the numbers used). Ignored in the simulator."},
            "title": {"type": "string", "description": "Default: the lot's name."}}}},
        {"name": "set_reference", "description": "Shows a saved roast (id from list_roasts) or a profile (its newest version) as the reference behind the live roast, or clears it (both omitted). A profile brings its recipe: get_status then names the next step. The person sees it in the app.",
         "inputSchema": {"type": "object", "properties": {
            "id": {"type": "string", "description": "A saved roast's id."},
            "profile": {"type": "string", "description": "Profile id or exact name, instead of a roast."}}}}
    ])
}

fn downsample(points: &[Value], every_s: f64, last_s: Option<f64>) -> Vec<Value> {
    let t_of = |p: &Value| p.get("t").and_then(Value::as_f64).unwrap_or(0.0);
    let end = points.last().map_or(0.0, t_of);
    let from = last_s.map_or(f64::MIN, |l| end - l);
    let mut out = Vec::new();
    let mut next = f64::MIN;
    for p in points {
        let t = t_of(p);
        if t < from || t < next {
            continue;
        }
        next = t + every_s;
        let round = |k: &str| {
            p.get(k)
                .and_then(Value::as_f64)
                .map(|v| (v * 10.0).round() / 10.0)
        };
        out.push(json!({"t": t.round(), "bt": round("bt"), "ror": round("ror"), "actuators": p.get("actuators")}));
    }
    out
}

fn call_tool(d: &mut Daemon, name: &str, args: &Value) -> anyhow::Result<Value> {
    let arg_f = |k: &str| args.get(k).and_then(Value::as_f64);
    let arg_s = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_owned);
    match name {
        "get_status" => {
            d.renew();
            d.call("GET", "/status", None)
        }
        "get_curve" => {
            d.renew();
            let live = d.call("GET", "/live", None)?;
            let points = live
                .get("points")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let every = arg_f("every_s").unwrap_or(10.0).max(1.0);
            Ok(json!({
                "points": downsample(&points, every, arg_f("last_s")),
                "events": live.get("events"),
                "notes": live.get("notes"),
            }))
        }
        "wait" => {
            let s = arg_f("seconds").unwrap_or(10.0).clamp(1.0, 30.0);
            std::thread::sleep(Duration::from_secs_f64(s));
            d.renew();
            d.call("GET", "/status", None)
        }
        "mark_event" => d.call(
            "POST",
            "/events",
            Some(json!({"kind": arg_s("kind"), "origin": "ai"})),
        ),
        "add_note" => d.call(
            "POST",
            "/notes",
            Some(json!({"text": arg_s("text"), "origin": "ai"})),
        ),
        "request_control" => {
            let r = d.call("POST", "/ai/lease", Some(json!({"holder": d.holder})))?;
            d.holding = true;
            Ok(r)
        }
        "release_control" => {
            d.holding = false;
            d.call("DELETE", "/ai/lease", None)
        }
        "set_burner" | "set_air" | "set_drum" => {
            let actuator = name.trim_start_matches("set_");
            let value = arg_f("value").context("value is required")?;
            d.call(
                "POST",
                "/command",
                Some(json!({"actuator": actuator, "value": value, "source": "ai", "reason": arg_s("reason")})),
            )
        }
        "emergency_stop" => {
            d.holding = false;
            let reason = arg_s("reason").unwrap_or_else(|| "AI".into());
            d.call(
                "POST",
                "/emergency-stop",
                Some(json!({"reason": format!("AI: {reason}")})),
            )
        }
        "list_roasts" => d.call("GET", "/roasts", None),
        "list_profiles" => d.call("GET", "/profiles", None),
        "get_profile" => {
            let id = resolve_profile(d, &arg_s("id").context("id is required")?)?;
            let detail = d.call("GET", &format!("/profiles/{id}"), None)?;
            let all = args.get("all_versions").and_then(Value::as_bool) == Some(true);
            profile_json(&detail, if all { usize::MAX } else { 3 })
        }
        "create_profile" => {
            let target = build_plan(d, None, args)?;
            let detail = d.call(
                "POST",
                "/profiles",
                Some(json!({
                    "name": arg_s("name").context("name is required")?,
                    "description": arg_s("description"),
                    "intended_beans": arg_s("intended_beans"),
                    "batch_size_g": arg_f("batch_size_g"),
                    "change_note": arg_s("change_note"),
                    "target": target,
                })),
            )?;
            profile_json(&detail, 1)
        }
        "add_profile_version" => {
            let id = resolve_profile(d, &arg_s("profile").context("profile is required")?)?;
            let detail = d.call("GET", &format!("/profiles/{id}"), None)?;
            let newest: Option<ProfileTarget> = detail
                .pointer("/versions/0/target")
                .map(|t| serde_json::from_value(t.clone()))
                .transpose()
                .context("the newest version could not be read")?;
            let target = build_plan(d, newest.as_ref(), args)?;
            let detail = d.call(
                "POST",
                &format!("/profiles/{id}/versions"),
                Some(json!({
                    "target": target,
                    "change_note": arg_s("change_note").context("change_note is required")?,
                })),
            )?;
            profile_json(&detail, 2)
        }
        "list_lots" => {
            let all = args.get("include_archived").and_then(Value::as_bool) == Some(true);
            d.call(
                "GET",
                if all { "/lots?archived=true" } else { "/lots" },
                None,
            )
        }
        "add_lot" => {
            let mut body = json!({
                "name": arg_s("name").context("name is required")?,
                "purchase_g": arg_f("grams"),
                "purchase_date": arg_s("date"),
            });
            // Offer-sheet details pass through as the API takes them.
            for k in [
                "origin_country",
                "region",
                "producer",
                "species",
                "varieties",
                "process",
                "honey",
                "fermentation",
                "fermentation_hours",
                "drying",
                "decaf",
                "process_detail",
                "harvest",
                "arrival_date",
                "grade",
                "screen_size",
                "moisture_pct",
                "water_activity",
                "density_g_per_l",
                "cupping_score",
                "flavor_notes",
                "supplier",
                "supplier_lot",
                "packaging",
                "storage_location",
                "certifications",
                "notes",
            ] {
                if let Some(v) = args.get(k).filter(|v| !v.is_null()) {
                    body[k] = v.clone();
                }
            }
            if let Some(min) = arg_f("altitude_min_m") {
                // Whole metres: the API takes integers.
                let metres = |v: f64| v.round().clamp(0.0, 10_000.0) as u32;
                body["altitude"] =
                    json!({"min_m": metres(min), "max_m": arg_f("altitude_max_m").map(metres)});
            }
            if let Some(p) = arg_f("price_per_kg") {
                body["price_per_kg"] = json!({"amount": p, "currency": "EUR"});
            }
            d.call("POST", "/lots", Some(body))
        }
        "add_purchase" => {
            let lot = resolve_lot(d, &arg_s("lot").context("lot is required")?)?;
            d.call(
                "POST",
                &format!("/lots/{lot}/purchases"),
                Some(json!({"grams": arg_f("grams").context("grams is required")?, "date": arg_s("date"), "note": arg_s("note")})),
            )
        }
        "start_recording" => {
            let lot = match arg_s("lot") {
                Some(l) => Some(resolve_lot(d, &l)?),
                None => None,
            };
            let profile_version = match arg_s("profile") {
                Some(p) => {
                    let id = resolve_profile(d, &p)?;
                    let detail = d.call("GET", &format!("/profiles/{id}"), None)?;
                    detail
                        .pointer("/versions/0/id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                }
                None => None,
            };
            let plan = json!({
                "goal": arg_s("goal"),
                "reference_id": arg_s("reference"),
                "profile_version_id": profile_version,
                "drop_bt_c": arg_f("drop_bt_c"),
                "dtr_pct": arg_f("dtr_pct"),
            });
            d.call(
                "POST",
                "/recording/start",
                Some(json!({
                    "title": arg_s("title"),
                    "lot_id": lot,
                    "green_weight_g": arg_f("green_weight_g"),
                    "batch_no": arg_s("batch_no"),
                    "plan": plan,
                })),
            )
        }
        "set_reference" => {
            let body = match arg_s("profile") {
                Some(p) => {
                    let id = resolve_profile(d, &p)?;
                    let detail = d.call("GET", &format!("/profiles/{id}"), None)?;
                    let version = detail
                        .pointer("/versions/0/id")
                        .and_then(Value::as_str)
                        .context("the profile has no version")?
                        .to_owned();
                    json!({ "profile_version_id": version })
                }
                None => json!({ "id": arg_s("id") }),
            };
            d.call("PUT", "/reference", Some(body))
        }
        "get_roast" => {
            let id = arg_s("id").context("id is required")?;
            let mut r = d.call("GET", &format!("/roasts/{id}"), None)?;
            if let Some(samples) = r.get("samples").and_then(Value::as_array).cloned() {
                r["samples"] = Value::Array(downsample(&samples, 10.0, None));
            }
            if let Some(obj) = r.as_object_mut() {
                obj.remove("control_log");
            }
            Ok(r)
        }
        _ => anyhow::bail!("unknown tool {name}"),
    }
}

/// A lot id from an id or a name: exact name first (ignoring case), then a
/// name that contains the text, if only one does.
/// A profile id from an id or an exact (case-insensitive) name.
fn resolve_profile(d: &mut Daemon, profile: &str) -> anyhow::Result<String> {
    if uuid::Uuid::parse_str(profile.trim()).is_ok() {
        return Ok(profile.trim().to_owned());
    }
    let list = d.call("GET", "/profiles?archived=true", None)?;
    let wanted = profile.trim().to_lowercase();
    list.as_array()
        .into_iter()
        .flatten()
        .find(|p| {
            p.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.to_lowercase() == wanted)
        })
        .and_then(|p| p.get("id").and_then(Value::as_str).map(str::to_owned))
        .with_context(|| format!("no profile named {profile:?}; see list_profiles"))
}

fn resolve_lot(d: &mut Daemon, lot: &str) -> anyhow::Result<String> {
    if uuid::Uuid::parse_str(lot.trim()).is_ok() {
        return Ok(lot.trim().to_owned());
    }
    let lots = d.call("GET", "/lots?archived=true", None)?;
    let lots = lots.as_array().cloned().unwrap_or_default();
    let name_of = |l: &Value| {
        l.get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let id_of = |l: &Value| l.get("id").and_then(Value::as_str).map(str::to_owned);
    let wanted = lot.trim().to_lowercase();
    if let Some(id) = lots
        .iter()
        .find(|l| name_of(l).to_lowercase() == wanted)
        .and_then(id_of)
    {
        return Ok(id);
    }
    let partial: Vec<&Value> = lots
        .iter()
        .filter(|l| name_of(l).to_lowercase().contains(&wanted))
        .collect();
    match partial.as_slice() {
        [one] => id_of(one).context("lot without id"),
        [] => anyhow::bail!(
            "no lot named {lot:?}; lots: {}",
            lots.iter().map(name_of).collect::<Vec<_>>().join(", ")
        ),
        many => anyhow::bail!(
            "{lot:?} matches several lots: {}",
            many.iter()
                .map(|l| name_of(l))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Runs the MCP server until stdin closes.
pub fn serve(data_dir: &Path) -> anyhow::Result<()> {
    let mut daemon: Option<Daemon> = None;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            let err = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}});
            writeln!(stdout, "{err}")?;
            stdout.flush()?;
            continue;
        };
        let Some(id) = msg.get("id").cloned() else {
            continue; // a notification
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result: Result<Value, (i64, String)> = match method {
            "initialize" => {
                if let Some(name) = params.pointer("/clientInfo/name").and_then(Value::as_str) {
                    let holder = format!("mcp:{name}");
                    if let Some(d) = &mut daemon {
                        d.holder = holder;
                    } else if let Ok(mut d) = Daemon::connect(data_dir) {
                        d.holder = holder;
                        daemon = Some(d);
                    }
                }
                let version = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(PROTOCOL_VERSION);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "roastmcp", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                if daemon.is_none() {
                    daemon = Daemon::connect(data_dir).ok();
                }
                let out = match &mut daemon {
                    Some(d) => call_tool(d, name, &args),
                    None => Err(anyhow::anyhow!(
                        "the RoastMCP daemon is not running - start it with `roastmcp daemon`"
                    )),
                };
                Ok(match out {
                    Ok(v) => json!({"content": [{"type": "text", "text": v.to_string()}]}),
                    Err(e) => {
                        json!({"content": [{"type": "text", "text": format!("Error: {e:#}")}], "isError": true})
                    }
                })
            }
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        let reply = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, message)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            }
        };
        writeln!(stdout, "{reply}")?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_have_schemas() {
        let t = tools();
        let list = t.as_array().unwrap();
        assert!(list.len() >= 10);
        assert!(list.iter().all(|t| t["inputSchema"]["type"] == "object"));
        assert!(!list.iter().any(|t| t["name"] == "confirm_presence"));
        // Starting a preheat confirms presence: only a person may (ADR 0018).
        assert!(
            !list
                .iter()
                .any(|t| t["name"].as_str().unwrap().contains("preheat"))
        );
    }

    #[test]
    fn a_recipe_is_read_and_written_back_alike() {
        let input = json!([
            {"actuator": "burner", "value": 80, "at_s": 0},
            {"actuator": "air", "value": 20, "at_s": 0},
            {"actuator": "burner", "value": 60, "at_bt_c": 185}
        ]);
        let recipe = parse_recipe(&input).unwrap();
        assert_eq!(recipe.len(), 3);
        let back = recipe_json(&recipe);
        assert_eq!(back[2]["at_bt_c"], 185.0);
        assert_eq!(back[0]["clock"], "0:00");
        assert!(parse_recipe(&json!([{"actuator": "drum", "value": 80, "at_s": 0}])).is_err());
        assert!(parse_recipe(&json!([{"actuator": "burner", "value": 120, "at_s": 0}])).is_err());
        assert!(parse_recipe(&json!([{"actuator": "burner", "value": 50}])).is_err());
    }

    #[test]
    fn milestones_change_only_what_is_given_and_are_checked() {
        let m = design::TEMPLATE_MILESTONES;
        let moved = apply_milestones(m, Some(&json!({"first_crack": {"time_s": 540}}))).unwrap();
        assert_eq!(moved.first_crack.t, 540.0);
        assert_eq!(moved.first_crack.bt, m.first_crack.bt);
        assert_eq!(moved.drop, m.drop);
        let err = apply_milestones(m, Some(&json!({"drop": {"time_s": 500}}))).unwrap_err();
        assert!(err.to_string().contains("30 s"), "{err}");
        let shown = milestones_json(&m);
        assert_eq!(shown["first_crack"]["clock"], "8:45");
    }

    #[test]
    fn downsampling_keeps_spacing() {
        let points: Vec<Value> = (0..100)
            .map(|i| json!({"t": f64::from(i), "bt": 100.0}))
            .collect();
        let d = downsample(&points, 10.0, None);
        assert_eq!(d.len(), 10);
        let d = downsample(&points, 10.0, Some(30.0));
        assert_eq!(d.len(), 4);
    }
}

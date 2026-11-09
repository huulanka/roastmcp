//! A Model Context Protocol server on stdio (newline-delimited JSON-RPC 2.0).
//! It forwards every tool to the daemon's HTTP API, so all rules of the
//! safety layer apply; it cannot confirm presence or reset an emergency stop.

use std::io::{BufRead as _, Write as _};
use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;
use serde_json::{Value, json};

const PROTOCOL_VERSION: &str = "2025-06-18";

const INSTRUCTIONS: &str = "RoastMCP connects you to a coffee roaster. Temperatures are °C, \
times seconds, actuator values percent. Read with get_status and get_curve; use wait to follow \
a roast. get_status also shows the reference curve (an earlier roast: its BT at the same time \
since CHARGE and the difference to now) and a projection (BT in 60 s, the next milestone from the \
reference and when it is reached); set_reference picks the reference from list_roasts. \
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
            (m, _) => anyhow::bail!("unsupported request method {m}"),
        };
        let mut r = res.context("the RoastMCP daemon is not reachable")?;
        let ok = r.status().is_success();
        let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
        if ok {
            Ok(v)
        } else {
            anyhow::bail!(
                "{}",
                v.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("request failed")
            )
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
        {"name": "mark_event", "description": "Marks a roast event at the current time (e.g. first_crack_start when you hear it reported).",
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
        {"name": "get_roast", "description": "A saved roast with its curve downsampled to every 10 s.",
         "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}},
        {"name": "list_lots", "description": "The green coffee in stock: lots with name, origin, process, supplier, purchase date and stock_g (grams; negative if more was roasted than recorded as bought). Archived lots only when asked.",
         "inputSchema": {"type": "object", "properties": {"include_archived": {"type": "boolean"}}}},
        {"name": "add_lot", "description": "Records a new green coffee lot (a purchase of a coffee not in the inventory yet), with the grams bought. Use add_purchase for more of a lot that exists.",
         "inputSchema": {"type": "object", "properties": {
            "name": {"type": "string", "description": "Short name used everywhere, e.g. \"GL-BR-001 Brasilien Amarelo\"."},
            "grams": num("Grams bought."),
            "date": {"type": "string", "description": "Purchase day, YYYY-MM-DD (default today)."},
            "origin_country": {"type": "string"}, "region": {"type": "string"}, "producer": {"type": "string"},
            "variety": {"type": "string"},
            "process": {"type": "string", "enum": ["washed", "natural", "honey", "anaerobic", "wet_hulled", "other"]},
            "process_detail": {"type": "string"}, "supplier": {"type": "string"},
            "price_per_kg": num("Price per kg in EUR."), "notes": {"type": "string"}},
          "required": ["name"]}},
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
            "drop_bt_c": num("Bean temperature to drop at, °C."),
            "dtr_pct": num("Development time ratio to drop at, percent."),
            "title": {"type": "string", "description": "Default: the lot's name."}}}},
        {"name": "set_reference", "description": "Shows a saved roast (id from list_roasts) as the reference curve behind the live roast, or clears it (id omitted). The person sees it in the app.",
         "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}}}
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
            for k in [
                "origin_country",
                "region",
                "producer",
                "variety",
                "process",
                "process_detail",
                "supplier",
                "notes",
            ] {
                if let Some(v) = arg_s(k) {
                    body[k] = json!(v);
                }
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
            let plan = json!({
                "goal": arg_s("goal"),
                "reference_id": arg_s("reference"),
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
                    "plan": plan,
                })),
            )
        }
        "set_reference" => d.call(
            "PUT",
            "/reference",
            Some(serde_json::json!({ "id": arg_s("id") })),
        ),
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

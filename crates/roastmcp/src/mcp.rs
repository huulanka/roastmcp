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
a roast. To control the roaster, a person must first confirm presence in the RoastMCP app, then \
call request_control. Your commands pass a safety layer: burner changes at most 5 points per \
command and one change per 5 s, air at most 10 points; anything the person does in the app \
overrides you and suspends your control. Keep calling wait or get_status at least every 60 s \
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
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(40))
                .build(),
            holder: "mcp".into(),
            holding: false,
        })
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> anyhow::Result<Value> {
        let req = self
            .agent
            .request(method, &format!("{}{path}", self.base))
            .set("Authorization", &format!("Bearer {}", self.token));
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        match res {
            Ok(r) => Ok(r.into_json()?),
            Err(ureq::Error::Status(_, r)) => {
                let v: Value = r.into_json().unwrap_or(Value::Null);
                anyhow::bail!(
                    "{}",
                    v.get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("request failed")
                )
            }
            Err(e) => Err(e).context("the RoastMCP daemon is not reachable"),
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
        {"name": "set_burner", "description": "Sets the burner (heater). Max 5 points per change, one change per 5 s.", "inputSchema": set("Burner", "0-100")},
        {"name": "set_air", "description": "Sets the airflow (fan). Max 10 points per change.", "inputSchema": set("Air", "0-100")},
        {"name": "set_drum", "description": "Sets the drum speed.", "inputSchema": set("Drum", "machine range")},
        {"name": "emergency_stop", "description": "Emergency stop: burner off, air high, drum running; latched until a person resets it in the app.",
         "inputSchema": {"type": "object", "properties": {"reason": {"type": "string"}}, "required": ["reason"]}},
        {"name": "list_roasts", "description": "Saved roasts, newest first.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "get_roast", "description": "A saved roast with its curve downsampled to every 10 s.",
         "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}}
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

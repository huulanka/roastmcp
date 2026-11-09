//! `.alog` → [`Roast`].

use std::path::Path;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Utc};
use roast_core::{
    Actuator, ActuatorValues, Ambient, BatchInfo, ColorScale, CommandOutcome, CommandRequest,
    CommandSource, ControlLogEntry, EventKind, EventOrigin, Note, Roast, RoastDegree, RoastEvent,
    RoastOrigin, Sample, f_to_c,
};
use uuid::Uuid;

use crate::format::{
    CUPPING_NOTE_PREFIX, DEFAULT_ETYPES, TIMEINDEX_KINDS, actuator_for_name,
    event_value_to_percent, format_percent,
};
use crate::text::decode_text;
use crate::{AlogError, PyValue, parse};

/// Reads the text of an `.alog` file into a [`Roast`].
///
/// The roast's origin is [`RoastOrigin::ImportedAlog`] with an empty file name;
/// [`read_alog_file`] fills it in.
///
/// # Errors
///
/// Fails if the text is not a Python dictionary literal or lacks the curve
/// (`timex`, `temp2`).
pub fn read_alog(text: &str) -> Result<Roast, AlogError> {
    from_py_value(&parse(text)?)
}

/// Reads an `.alog` file into a [`Roast`], recording the file name in the
/// roast's origin. Files that are not valid UTF-8 are read as Latin-1.
///
/// # Errors
///
/// Fails if the file cannot be read or [`read_alog`] fails.
pub fn read_alog_file(path: impl AsRef<Path>) -> Result<Roast, AlogError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| AlogError::Io {
        path: path.to_owned(),
        source,
    })?;
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => e.into_bytes().into_iter().map(char::from).collect(),
    };
    let mut roast = read_alog(&text)?;
    if let RoastOrigin::ImportedAlog { file_name, .. } = &mut roast.origin {
        *file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    Ok(roast)
}

/// Converts a parsed `.alog` dictionary into a [`Roast`].
///
/// # Errors
///
/// Fails if the value is not a dictionary, lacks `timex` or `temp2`, or holds
/// values of the wrong type in them.
pub fn from_py_value(alog: &PyValue) -> Result<Roast, AlogError> {
    if alog.as_dict().is_none() {
        return Err(AlogError::NotADict(alog.type_name()));
    }
    let fahrenheit = string(alog, "mode").is_some_and(|m| m.trim().eq_ignore_ascii_case("F"));
    let to_c = |v: f64| if fahrenheit { f_to_c(v) } else { v };

    let timex = list(alog, "timex")?.ok_or(AlogError::MissingKey("timex"))?;
    let timex = timex
        .iter()
        .map(PyValue::as_f64)
        .collect::<Option<Vec<f64>>>()
        .ok_or(AlogError::WrongType {
            key: "timex",
            expected: "a list of numbers",
            found: "list with other values",
        })?;
    let temp2 = numbers(list(alog, "temp2")?.ok_or(AlogError::MissingKey("temp2"))?);
    let temp1 = list(alog, "temp1")?.map(numbers).unwrap_or_default();
    let n = timex.len().min(temp2.len());

    let roasted_at = roasted_at(alog);
    let etypes = etypes(alog)?;
    let channels = actuator_channels(alog, &etypes)?;

    let reading = |v: Option<&Option<f64>>| {
        v.copied()
            .flatten()
            .filter(|x| x.is_finite() && *x > 0.0)
            .map(to_c)
    };
    let samples: Vec<Sample> = (0..n)
        .map(|i| {
            let mut actuators = ActuatorValues::new();
            for channel in &channels {
                if let Some(v) = channel.value_at(i, timex[i], n) {
                    actuators.insert(channel.actuator, v);
                }
            }
            Sample {
                t: timex[i],
                bt: reading(temp2.get(i)),
                et: reading(temp1.get(i)),
                actuators,
            }
        })
        .collect();

    let mut events = timeindex_events(alog, &samples)?;
    if let Some(tp) = turning_point(alog, &samples, &events) {
        events.push(tp);
    }
    events.sort_by(|a, b| a.t.total_cmp(&b.t).then(a.kind.cmp(&b.kind)));

    let mut notes = Vec::new();
    if let Some(text) = text(alog, "roastingnotes") {
        notes.push(Note {
            t: None,
            text,
            origin: EventOrigin::Import,
        });
    }
    if let Some(text) = text(alog, "cuppingnotes") {
        notes.push(Note {
            t: None,
            text: format!("{CUPPING_NOTE_PREFIX}{text}"),
            origin: EventOrigin::Import,
        });
    }
    let control_log = special_events(alog, &samples, &etypes, roasted_at, &mut notes)?;

    let sampling_interval_s = number(alog, "samplinginterval")
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or_else(|| derived_interval(&timex));

    Ok(Roast {
        id: string(alog, "roastUUID")
            .and_then(|s| Uuid::parse_str(s.trim()).ok())
            .unwrap_or_else(Uuid::new_v4),
        title: text(alog, "title").unwrap_or_default(),
        roasted_at,
        machine: text(alog, "roastertype"),
        sampling_interval_s,
        batch: batch(alog),
        ambient: ambient(alog, to_c),
        roast_degree: roast_degree(alog),
        green_lot_id: None,
        plan: None,
        profile_version_id: None,
        experiment_id: None,
        origin: RoastOrigin::ImportedAlog {
            file_name: String::new(),
            artisan_version: string(alog, "version").map(str::to_owned),
        },
        samples,
        events,
        control_log,
        notes,
    })
}

// ---------------------------------------------------------------------------
// Field access
// ---------------------------------------------------------------------------

fn string<'a>(alog: &'a PyValue, key: &str) -> Option<&'a str> {
    alog.get(key).and_then(PyValue::as_str)
}

/// A free-text field, decoded, or `None` when missing or blank.
fn text(alog: &PyValue, key: &str) -> Option<String> {
    string(alog, key)
        .map(decode_text)
        .filter(|s| !s.trim().is_empty())
}

fn number(alog: &PyValue, key: &str) -> Option<f64> {
    alog.get(key).and_then(PyValue::as_f64)
}

/// A list (or tuple) value. A missing key or `None` is `Ok(None)`; any other
/// type is an error.
fn list<'a>(alog: &'a PyValue, key: &'static str) -> Result<Option<&'a [PyValue]>, AlogError> {
    match alog.get(key) {
        None | Some(PyValue::None) => Ok(None),
        Some(v) => v.as_list().map(Some).ok_or(AlogError::WrongType {
            key,
            expected: "a list",
            found: v.type_name(),
        }),
    }
}

/// Numbers in a list; anything else (`None`, strings) becomes `None`.
fn numbers(items: &[PyValue]) -> Vec<Option<f64>> {
    items.iter().map(PyValue::as_f64).collect()
}

fn derived_interval(timex: &[f64]) -> f64 {
    match (timex.first(), timex.last()) {
        (Some(first), Some(last)) if timex.len() > 1 && last > first => {
            #[allow(clippy::cast_precision_loss)]
            let steps = (timex.len() - 1) as f64;
            (last - first) / steps
        }
        _ => 1.0,
    }
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

/// When the recording started: `roastepoch` if present, else `roastisodate`
/// and `roasttime` (local time, corrected by `roasttzoffset`, which counts
/// seconds west of UTC), else the Unix epoch.
fn roasted_at(alog: &PyValue) -> DateTime<Utc> {
    if let Some(epoch) = number(alog, "roastepoch").filter(|e| e.is_finite() && *e > 0.0) {
        #[allow(clippy::cast_possible_truncation)]
        let millis = (epoch * 1000.0).round() as i64;
        if let Some(at) = DateTime::from_timestamp_millis(millis) {
            return at;
        }
    }
    let date = string(alog, "roastisodate")
        .and_then(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok());
    if let Some(date) = date {
        let time = string(alog, "roasttime")
            .and_then(|t| NaiveTime::parse_from_str(t.trim(), "%H:%M:%S").ok())
            .unwrap_or(NaiveTime::MIN);
        let west = alog
            .get("roasttzoffset")
            .and_then(PyValue::as_i64)
            .unwrap_or(0);
        let local = NaiveDateTime::new(date, time);
        return (local + TimeDelta::seconds(west)).and_utc();
    }
    DateTime::<Utc>::UNIX_EPOCH
}

fn batch(alog: &PyValue) -> BatchInfo {
    let weight = alog.get("weight").and_then(PyValue::as_list).unwrap_or(&[]);
    let factor = weight
        .get(2)
        .and_then(PyValue::as_str)
        .map_or(1.0, grams_per_unit);
    let grams = |i: usize| {
        weight
            .get(i)
            .and_then(PyValue::as_f64)
            .filter(|w| w.is_finite() && *w > 0.0)
            .map(|w| w * factor)
    };
    // Artisan's batch counter: a prefix and a number, 0 when unused.
    let batch_no = alog
        .get("roastbatchnr")
        .and_then(PyValue::as_i64)
        .filter(|n| *n > 0)
        .map(|n| {
            format!(
                "{}{n:03}",
                text(alog, "roastbatchprefix").unwrap_or_default()
            )
        });
    BatchInfo {
        green_weight_g: grams(0),
        roasted_weight_g: grams(1),
        beans: text(alog, "beans"),
        batch_no,
    }
}

/// Grams per weight unit as Artisan names them. Unknown units count as grams.
fn grams_per_unit(unit: &str) -> f64 {
    match unit.trim().to_ascii_lowercase().as_str() {
        "kg" => 1000.0,
        "lb" | "lbs" => 453.592_37,
        "oz" => 28.349_523_125,
        _ => 1.0,
    }
}

fn ambient(alog: &PyValue, to_c: impl Fn(f64) -> f64) -> Option<Ambient> {
    // Artisan writes 0.0 for "not measured".
    let known = |key| number(alog, key).filter(|v| v.is_finite() && *v != 0.0);
    let ambient = Ambient {
        temperature_c: known("ambientTemp").map(to_c),
        humidity_pct: known("ambient_humidity").filter(|v| *v > 0.0),
        pressure_hpa: known("ambient_pressure").filter(|v| *v > 0.0),
    };
    (ambient != Ambient::default()).then_some(ambient)
}

fn roast_degree(alog: &PyValue) -> Option<RoastDegree> {
    let known = |key| number(alog, key).filter(|v| v.is_finite() && *v > 0.0);
    let whole_bean = known("whole_color");
    let ground = known("ground_color");
    if whole_bean.is_none() && ground.is_none() {
        return None;
    }
    let system = string(alog, "color_system")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let scale = if system.contains("agtron") {
        ColorScale::Agtron
    } else if system.contains("lighttells") {
        ColorScale::Lighttells
    } else {
        ColorScale::Other
    };
    Some(RoastDegree {
        scale,
        whole_bean,
        ground,
    })
}

fn etypes(alog: &PyValue) -> Result<Vec<String>, AlogError> {
    Ok(match list(alog, "etypes")? {
        Some(items) => items
            .iter()
            .map(|v| v.as_str().map(decode_text).unwrap_or_default())
            .collect(),
        None => DEFAULT_ETYPES.iter().map(|s| (*s).to_owned()).collect(),
    })
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn event_at(samples: &[Sample], index: i64, kind: EventKind) -> Option<RoastEvent> {
    let sample = samples.get(usize::try_from(index).ok()?)?;
    Some(RoastEvent {
        t: sample.t,
        kind,
        origin: EventOrigin::Import,
        bt: sample.bt,
    })
}

fn timeindex_events(alog: &PyValue, samples: &[Sample]) -> Result<Vec<RoastEvent>, AlogError> {
    let Some(timeindex) = list(alog, "timeindex")? else {
        return Ok(Vec::new());
    };
    Ok(TIMEINDEX_KINDS
        .iter()
        .zip(timeindex)
        .filter_map(|(&kind, index)| {
            let index = index.as_i64()?;
            // CHARGE uses -1 for "not set" (so sample 0 is valid); the others use 0.
            let set = if kind == EventKind::Charge {
                index >= 0
            } else {
                index > 0
            };
            if set {
                event_at(samples, index, kind)
            } else {
                None
            }
        })
        .collect())
}

/// The turning point, which Artisan does not mark but stores among its
/// computed values (`computed['TP_idx']`).
fn turning_point(alog: &PyValue, samples: &[Sample], events: &[RoastEvent]) -> Option<RoastEvent> {
    let charge_t = events.iter().find(|e| e.kind == EventKind::Charge)?.t;
    let index = alog.get("computed")?.get("TP_idx")?.as_i64()?;
    event_at(samples, index, EventKind::TurningPoint).filter(|tp| tp.t > charge_t)
}

/// Special events: actuator changes become control log entries, everything
/// else (damper, untyped events) becomes a timed note.
fn special_events(
    alog: &PyValue,
    samples: &[Sample],
    etypes: &[String],
    roasted_at: DateTime<Utc>,
    notes: &mut Vec<Note>,
) -> Result<Vec<ControlLogEntry>, AlogError> {
    let Some(indices) = list(alog, "specialevents")? else {
        return Ok(Vec::new());
    };
    let types = list(alog, "specialeventstype")?.unwrap_or(&[]);
    let values = list(alog, "specialeventsvalue")?.unwrap_or(&[]);
    let labels = list(alog, "specialeventsStrings")?.unwrap_or(&[]);

    let mut log = Vec::new();
    for (k, index) in indices.iter().enumerate() {
        let Some(sample) = index
            .as_i64()
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| samples.get(i))
        else {
            continue;
        };
        let t = sample.t;
        let etype = types.get(k).and_then(PyValue::as_i64).unwrap_or(4);
        let pct = event_value_to_percent(values.get(k).and_then(PyValue::as_f64).unwrap_or(0.0));
        let label = labels
            .get(k)
            .and_then(PyValue::as_str)
            .map(decode_text)
            .unwrap_or_default();
        let name = usize::try_from(etype)
            .ok()
            .filter(|&i| i < 4)
            .and_then(|i| etypes.get(i));

        match name.and_then(|n| actuator_for_name(n)) {
            Some(actuator) => log.push(ControlLogEntry {
                t: Some(t),
                at: at_recording_time(roasted_at, t),
                previous: None,
                request: CommandRequest {
                    actuator,
                    value: pct,
                    source: CommandSource::Human,
                    reason: None,
                },
                outcome: CommandOutcome::Applied {
                    value: pct,
                    clamped_from: None,
                },
            }),
            None => {
                let text = match name {
                    Some(name) if label.trim().is_empty() => {
                        format!("{} {}", name.trim(), format_percent(pct))
                    }
                    Some(name) => format!("{} {} ({})", name.trim(), format_percent(pct), label),
                    None if label.trim().is_empty() => "Event".to_owned(),
                    None => label,
                };
                notes.push(Note {
                    t: Some(t),
                    text,
                    origin: EventOrigin::Import,
                });
            }
        }
    }
    Ok(log)
}

fn at_recording_time(roasted_at: DateTime<Utc>, t: f64) -> DateTime<Utc> {
    #[allow(clippy::cast_possible_truncation)]
    let millis = (t * 1000.0).round() as i64;
    roasted_at + TimeDelta::milliseconds(millis)
}

// ---------------------------------------------------------------------------
// Reported actuator values (extra devices)
// ---------------------------------------------------------------------------

/// An extra-device channel that carries an actuator value reported by the
/// machine.
struct ActuatorChannel {
    actuator: Actuator,
    times: Vec<Option<f64>>,
    values: Vec<Option<f64>>,
}

impl ActuatorChannel {
    /// The channel's value for sample `i` at time `t`. Channels recorded in
    /// step with the main curve are matched by index; others by the last
    /// reading at or before `t`. Negative values (`-1`) mean "no reading".
    fn value_at(&self, i: usize, t: f64, n: usize) -> Option<f64> {
        let j = if self.values.len() == n {
            i
        } else {
            self.times
                .iter()
                .take(self.values.len())
                .rposition(|x| x.is_some_and(|x| x <= t + 1e-6))?
        };
        self.values
            .get(j)
            .copied()
            .flatten()
            .filter(|v| v.is_finite() && *v >= 0.0)
    }
}

/// Finds the extra channels that report an actuator. A channel name such as
/// `{3}` stands for `etypes[3]`; a plain name is matched directly. The first
/// channel for an actuator wins.
fn actuator_channels(alog: &PyValue, etypes: &[String]) -> Result<Vec<ActuatorChannel>, AlogError> {
    let times = list(alog, "extratimex")?.unwrap_or(&[]);
    let mut channels: Vec<ActuatorChannel> = Vec::new();
    for (names_key, temps_key) in [("extraname1", "extratemp1"), ("extraname2", "extratemp2")] {
        let names = list(alog, names_key)?.unwrap_or(&[]);
        let temps = list(alog, temps_key)?.unwrap_or(&[]);
        for (device, name) in names.iter().enumerate() {
            let Some(name) = name.as_str().map(decode_text) else {
                continue;
            };
            let Some(actuator) = resolve_channel_name(&name, etypes).and_then(actuator_for_name)
            else {
                continue;
            };
            if channels.iter().any(|c| c.actuator == actuator) {
                continue;
            }
            let Some(values) = temps.get(device).and_then(PyValue::as_list) else {
                continue;
            };
            channels.push(ActuatorChannel {
                actuator,
                times: times
                    .get(device)
                    .and_then(PyValue::as_list)
                    .map(numbers)
                    .unwrap_or_default(),
                values: numbers(values),
            });
        }
    }
    // Keep a stable order regardless of the device layout.
    channels.sort_by_key(|c| c.actuator);
    Ok(channels)
}

/// Resolves an `{n}` placeholder to `etypes[n]`; other names are returned
/// unchanged.
fn resolve_channel_name<'a>(name: &'a str, etypes: &'a [String]) -> Option<&'a str> {
    let trimmed = name.trim();
    match trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        Some(index) => etypes
            .get(index.trim().parse::<usize>().ok()?)
            .map(String::as_str),
        None => Some(trimmed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "{'version': '4.2.0', 'mode': 'C', 'title': 'Test\\\\nroast', \
        'beans': 'Catua\\\\xed', 'weight': [0.2, 0.17, 'Kg'], \
        'roastisodate': '2026-06-19', 'roasttime': '13:05:00', 'roasttzoffset': -7200, \
        'roastUUID': '106a6ee743c4403394f3995665d24ece', 'samplinginterval': 1.5, \
        'timeindex': [0, 2, 3, 0, 0, 0, 4, 0], \
        'timex': [0.5, 2.0, 3.5, 5.0, 6.5], \
        'temp1': [0.0, 0.0, 0.0, 0.0, 0.0], 'temp2': [200.0, -1, 150.0, 170.0, 190.0], \
        'etypes': ['Air', 'Drum', 'Damper', 'Burner', '--'], \
        'specialevents': [0, 1, 2, 3], 'specialeventstype': [3, 2, 0, 4], \
        'specialeventsvalue': [9.0, 4.0, 0.0, 0.0], \
        'specialeventsStrings': ['80%', '', 'Q0', 'smoke'], \
        'roastingnotes': 'good', 'cuppingnotes': '', \
        'extradevices': [32], 'extraname1': ['{3}'], 'extraname2': ['{0}'], \
        'extratimex': [[0.5, 2.0, 3.5, 5.0, 6.5]], \
        'extratemp1': [[80.0, 80.0, 75.0, -1, 70.0]], 'extratemp2': [[20.0, 20.0, 30.0, 30.0, 40.0]]}";

    #[test]
    fn reads_a_minimal_profile() {
        let roast = read_alog(MINIMAL).unwrap();
        assert_eq!(roast.title, "Test\nroast");
        assert_eq!(roast.batch.beans.as_deref(), Some("Catuaí"));
        assert_eq!(roast.batch.green_weight_g, Some(200.0));
        assert_eq!(roast.batch.roasted_weight_g, Some(170.0));
        assert_eq!(
            roast.id.simple().to_string(),
            "106a6ee743c4403394f3995665d24ece"
        );
        assert_eq!(roast.roasted_at.to_rfc3339(), "2026-06-19T11:05:00+00:00");
        assert_eq!(roast.sampling_interval_s, 1.5);
        assert_eq!(
            roast.origin,
            RoastOrigin::ImportedAlog {
                file_name: String::new(),
                artisan_version: Some("4.2.0".into())
            }
        );

        assert_eq!(roast.samples.len(), 5);
        assert_eq!(roast.samples[1].bt, None, "-1 is no reading");
        assert!(
            roast.samples.iter().all(|s| s.et.is_none()),
            "constant 0.0 ET is no probe"
        );
        assert_eq!(
            roast.samples[0].actuators.get(&Actuator::Burner),
            Some(&80.0)
        );
        assert_eq!(roast.samples[0].actuators.get(&Actuator::Air), Some(&20.0));
        assert_eq!(roast.samples[3].actuators.get(&Actuator::Burner), None);

        let kinds: Vec<_> = roast.events.iter().map(|e| (e.kind, e.t)).collect();
        assert_eq!(
            kinds,
            [
                (EventKind::Charge, 0.5),
                (EventKind::DryEnd, 3.5),
                (EventKind::FirstCrackStart, 5.0),
                (EventKind::Drop, 6.5)
            ]
        );
        assert_eq!(roast.events[0].bt, Some(200.0));
        assert!(roast.events.iter().all(|e| e.origin == EventOrigin::Import));

        assert_eq!(roast.control_log.len(), 2);
        let burner = &roast.control_log[0];
        assert_eq!(burner.request.actuator, Actuator::Burner);
        assert_eq!(burner.request.value, 80.0);
        assert_eq!(burner.request.source, CommandSource::Human);
        assert_eq!(burner.t, Some(0.5));
        assert_eq!(burner.at.to_rfc3339(), "2026-06-19T11:05:00.500+00:00");
        let air = &roast.control_log[1];
        assert_eq!(
            (air.request.actuator, air.request.value),
            (Actuator::Air, 0.0)
        );

        let notes: Vec<_> = roast.notes.iter().map(|n| (n.t, n.text.as_str())).collect();
        assert_eq!(
            notes,
            [
                (None, "good"),
                (Some(2.0), "Damper 30%"),
                (Some(5.0), "smoke")
            ]
        );
    }

    #[test]
    fn converts_fahrenheit_and_prefers_roastepoch() {
        let roast = read_alog(
            "{'mode': 'F', 'timex': [0, 1], 'temp1': [-1, 212.0], 'temp2': [392.0, 32.5], \
             'roastepoch': 1794135049, 'roastisodate': '2000-01-01', 'ambientTemp': 68.0, \
             'weight': [1.0, 0, 'lb'], 'timeindex': [-1, 0, 0, 0, 0, 0, 0, 0]}",
        )
        .unwrap();
        assert_eq!(roast.samples[0].bt, Some(200.0));
        assert_eq!(roast.samples[0].et, None);
        assert_eq!(roast.samples[1].et, Some(100.0));
        assert!((roast.samples[1].bt.unwrap() - 0.277_777).abs() < 1e-3);
        assert_eq!(roast.roasted_at.to_rfc3339(), "2026-11-08T10:50:49+00:00");
        assert_eq!(roast.ambient.unwrap().temperature_c, Some(20.0));
        assert_eq!(roast.batch.green_weight_g, Some(453.592_37));
        assert_eq!(roast.batch.roasted_weight_g, None);
        assert!(roast.events.is_empty(), "nothing marked");
        assert_eq!(roast.sampling_interval_s, 1.0, "derived from timex");
        assert_eq!(roast.title, "");
    }

    #[test]
    fn rejects_what_is_not_a_profile() {
        assert!(matches!(
            read_alog("[1, 2]"),
            Err(AlogError::NotADict("list"))
        ));
        assert!(matches!(
            read_alog("{'temp2': []}"),
            Err(AlogError::MissingKey("timex"))
        ));
        assert!(matches!(
            read_alog("{'timex': 'x', 'temp2': []}"),
            Err(AlogError::WrongType { key: "timex", .. })
        ));
        assert!(matches!(
            read_alog("{'timex': [}"),
            Err(AlogError::Parse(_))
        ));
    }

    #[test]
    fn unaligned_channels_use_the_last_reading() {
        let roast = read_alog(
            "{'timex': [0.0, 1.0, 2.0], 'temp2': [100.0, 101.0, 102.0], \
             'extraname1': ['Air'], 'extraname2': [''], 'extratimex': [[0.4, 1.4]], \
             'extratemp1': [[10.0, 20.0]], 'extratemp2': [[-1, -1]]}",
        )
        .unwrap();
        let air: Vec<_> = roast
            .samples
            .iter()
            .map(|s| s.actuators.get(&Actuator::Air).copied())
            .collect();
        assert_eq!(air, [None, Some(10.0), Some(20.0)]);
    }
}

//! [`Roast`] → `.alog`.

use chrono::{FixedOffset, Local, Offset, TimeZone};
use roast_core::{Actuator, ColorScale, CommandOutcome, EventKind, Roast, Sample};

use crate::PyValue;
use crate::format::{
    COMPUTED_EVENT_PREFIXES, CUPPING_NOTE_PREFIX, DEFAULT_ETYPES, TIMEINDEX_KINDS, UNTYPED_EVENT,
    etype_index, format_percent, percent_to_event_value,
};
use crate::text::encode_text;

/// The Artisan version written into exported files. It is the oldest version
/// among the reference files whose layout the export follows (no `hash` or
/// `signature` keys).
pub const ARTISAN_FORMAT_VERSION: &str = "4.0.2";

/// Extra device type used for the channels that carry reported actuator
/// values. Real Cubean+ files record reported burner and air on the extra
/// device `+ArduinoTC4 56`, stored as type 32, with the channels named after
/// the event types (`{3}` = Burner, `{0}` = Air); the export uses the same
/// layout.
pub const REPORTED_ACTUATOR_DEVICE: i64 = 32;

/// Writes a roast as the text of an `.alog` file, with dates and times in the
/// computer's local time zone.
#[must_use]
pub fn write_alog(roast: &Roast) -> String {
    let offset = Local
        .offset_from_utc_datetime(&roast.roasted_at.naive_utc())
        .fix();
    write_alog_with_offset(roast, offset)
}

/// Writes a roast as the text of an `.alog` file, with dates and times in the
/// given time zone offset.
#[must_use]
pub fn write_alog_with_offset(roast: &Roast, offset: FixedOffset) -> String {
    to_py_value(roast, offset).to_literal()
}

/// Converts a roast into the dictionary of an `.alog` file.
///
/// What is written: the curves (`timex`, `temp1` = ET, `temp2` = BT, in °C;
/// missing readings as `-1`), the events (`timeindex`, and the turning point in
/// `computed`), applied actuator commands of the control log as special
/// events, timed notes as untyped special events, roast-wide notes as
/// `roastingnotes`/`cuppingnotes`, reported actuator values as extra device
/// channels, and the metadata Artisan shows (title, beans, weights, dates,
/// roaster, ambient, colour).
#[must_use]
pub fn to_py_value(roast: &Roast, offset: FixedOffset) -> PyValue {
    let samples = &roast.samples;
    let local = roast.roasted_at.with_timezone(&offset);
    let mut d = PyValue::Dict(Vec::new());
    let mut set = |key: &str, value: PyValue| d.insert(key, value);

    set("version", ARTISAN_FORMAT_VERSION.into());
    set("mode", "C".into());
    set("timeindex", timeindex(roast).into());
    set("title", encode_text(&roast.title).into());
    set(
        "beans",
        encode_text(roast.batch.beans.as_deref().unwrap_or_default()).into(),
    );
    set(
        "weight",
        PyValue::List(vec![
            roast.batch.green_weight_g.unwrap_or(0.0).into(),
            roast.batch.roasted_weight_g.unwrap_or(0.0).into(),
            "g".into(),
        ]),
    );
    // Artisan's batch counter: `BRYB-002` as prefix "BRYB-" and number 2.
    if let Some((prefix, nr)) = roast.batch.batch_no.as_deref().and_then(split_batch_no) {
        set("roastbatchprefix", encode_text(prefix).into());
        set("roastbatchnr", PyValue::Int(nr));
    }
    set(
        "roastertype",
        encode_text(roast.machine.as_deref().unwrap_or_default()).into(),
    );
    let degree = roast.roast_degree.as_ref();
    set(
        "whole_color",
        degree.and_then(|r| r.whole_bean).unwrap_or(0.0).into(),
    );
    set(
        "ground_color",
        degree.and_then(|r| r.ground).unwrap_or(0.0).into(),
    );
    set(
        "color_system",
        match degree.map(|r| r.scale) {
            Some(ColorScale::Agtron) => "Agtron",
            Some(ColorScale::Lighttells) => "Lighttells",
            Some(ColorScale::Other) | None => "",
        }
        .into(),
    );
    set("roastdate", local.format("%a %b %-d %Y").to_string().into());
    set("roastisodate", local.format("%Y-%m-%d").to_string().into());
    set("roasttime", local.format("%H:%M:%S").to_string().into());
    set("roastepoch", roast.roasted_at.timestamp().into());
    // Seconds west of UTC, as Python's `time.timezone` counts.
    set("roasttzoffset", i64::from(-offset.local_minus_utc()).into());
    set("roastUUID", roast.id.simple().to_string().into());

    let events = special_events(roast);
    set(
        "specialevents",
        PyValue::List(events.iter().map(|e| PyValue::Int(e.index)).collect()),
    );
    set(
        "specialeventstype",
        PyValue::List(events.iter().map(|e| PyValue::Int(e.etype)).collect()),
    );
    set(
        "specialeventsvalue",
        PyValue::List(events.iter().map(|e| PyValue::Float(e.value)).collect()),
    );
    set(
        "specialeventsStrings",
        PyValue::List(events.iter().map(|e| e.label.clone().into()).collect()),
    );
    set("etypes", DEFAULT_ETYPES.to_vec().into());

    let (roasting, cupping) = roast_notes(roast);
    set("roastingnotes", encode_text(&roasting).into());
    set("cuppingnotes", encode_text(&cupping).into());

    set(
        "timex",
        PyValue::List(samples.iter().map(|s| s.t.into()).collect()),
    );
    set("temp1", reading_list(samples, |s| s.et));
    set("temp2", reading_list(samples, |s| s.bt));

    let ambient = roast.ambient.clone().unwrap_or_default();
    set("ambientTemp", ambient.temperature_c.unwrap_or(0.0).into());
    set(
        "ambient_humidity",
        ambient.humidity_pct.unwrap_or(0.0).into(),
    );
    set(
        "ambient_pressure",
        ambient.pressure_hpa.unwrap_or(0.0).into(),
    );

    extra_devices(samples, &mut set);
    set("samplinginterval", roast.sampling_interval_s.into());
    set("computed", computed(roast));
    d
}

// ---------------------------------------------------------------------------

fn reading_list(samples: &[Sample], reading: impl Fn(&Sample) -> Option<f64>) -> PyValue {
    PyValue::List(
        samples
            .iter()
            .map(|s| reading(s).filter(|v| v.is_finite()).unwrap_or(-1.0).into())
            .collect(),
    )
}

/// Index of the sample nearest to recording time `t`.
fn nearest_index(samples: &[Sample], t: f64) -> Option<usize> {
    samples
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (a.t - t).abs().total_cmp(&(b.t - t).abs()))
        .map(|(i, _)| i)
}

fn index_i64(i: usize) -> i64 {
    i64::try_from(i).unwrap_or(i64::MAX)
}

fn timeindex(roast: &Roast) -> Vec<i64> {
    TIMEINDEX_KINDS
        .iter()
        .map(|&kind| {
            let unset = if kind == EventKind::Charge { -1 } else { 0 };
            roast
                .event(kind)
                .and_then(|e| nearest_index(&roast.samples, e.t))
                .map_or(unset, index_i64)
        })
        .collect()
}

struct SpecialEvent {
    index: i64,
    etype: i64,
    value: f64,
    label: String,
}

/// Applied actuator commands and timed notes as special events, ordered by
/// sample index.
fn special_events(roast: &Roast) -> Vec<SpecialEvent> {
    let mut events = Vec::new();
    for entry in &roast.control_log {
        let CommandOutcome::Applied { value, .. } = entry.outcome else {
            continue;
        };
        let Some(index) = entry.t.and_then(|t| nearest_index(&roast.samples, t)) else {
            continue;
        };
        let actuator = entry.request.actuator;
        events.push(match etype_index(actuator) {
            Some(etype) => SpecialEvent {
                index: index_i64(index),
                etype,
                value: percent_to_event_value(value),
                label: format_percent(value),
            },
            None => SpecialEvent {
                index: index_i64(index),
                etype: UNTYPED_EVENT,
                value: 0.0,
                label: encode_text(&format!(
                    "{} {}",
                    capitalized(actuator),
                    format_percent(value)
                )),
            },
        });
    }
    for note in &roast.notes {
        let Some(index) = note.t.and_then(|t| nearest_index(&roast.samples, t)) else {
            continue;
        };
        events.push(SpecialEvent {
            index: index_i64(index),
            etype: UNTYPED_EVENT,
            value: 0.0,
            label: encode_text(&note.text),
        });
    }
    events.sort_by_key(|e| e.index);
    events
}

fn capitalized(actuator: Actuator) -> String {
    let name = actuator.as_str();
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_ascii_uppercase().to_string() + chars.as_str()
    })
}

/// Roast-wide notes, split into roasting notes and cupping notes.
fn roast_notes(roast: &Roast) -> (String, String) {
    let mut roasting = Vec::new();
    let mut cupping = Vec::new();
    for note in roast.notes.iter().filter(|n| n.t.is_none()) {
        match note.text.strip_prefix(CUPPING_NOTE_PREFIX) {
            Some(text) => cupping.push(text),
            None => roasting.push(note.text.as_str()),
        }
    }
    (roasting.join("\n\n"), cupping.join("\n\n"))
}

/// Channel name of a reported actuator: an `{n}` reference to the default
/// event types where one exists, the role's name otherwise.
fn channel_name(actuator: Actuator) -> String {
    match etype_index(actuator) {
        Some(i) => format!("{{{i}}}"),
        None => capitalized(actuator),
    }
}

/// Reported actuator values as extra devices, two channels per device.
fn extra_devices(samples: &[Sample], set: &mut impl FnMut(&str, PyValue)) {
    let reported: Vec<Actuator> = Actuator::ALL
        .into_iter()
        .filter(|a| samples.iter().any(|s| s.actuators.contains_key(a)))
        .collect();
    let devices: Vec<&[Actuator]> = reported.chunks(2).collect();
    let n = devices.len();

    let channel = |actuator: Option<&Actuator>| -> (PyValue, PyValue) {
        match actuator {
            Some(&a) => (
                channel_name(a).into(),
                PyValue::List(
                    samples
                        .iter()
                        .map(|s| s.actuators.get(&a).copied().unwrap_or(-1.0).into())
                        .collect(),
                ),
            ),
            None => (
                "".into(),
                PyValue::List(vec![PyValue::Float(-1.0); samples.len()]),
            ),
        }
    };
    let (mut names1, mut temps1, mut names2, mut temps2) = (vec![], vec![], vec![], vec![]);
    for device in &devices {
        let (name, temps) = channel(device.first());
        names1.push(name);
        temps1.push(temps);
        let (name, temps) = channel(device.get(1));
        names2.push(name);
        temps2.push(temps);
    }
    let timex = PyValue::List(samples.iter().map(|s| s.t.into()).collect());
    let repeat = |v: PyValue| PyValue::List(vec![v; n]);

    set(
        "extradevices",
        PyValue::List(vec![PyValue::Int(REPORTED_ACTUATOR_DEVICE); n]),
    );
    set("extraname1", PyValue::List(names1));
    set("extraname2", PyValue::List(names2));
    set("extratimex", repeat(timex));
    set("extratemp1", PyValue::List(temps1));
    set("extratemp2", PyValue::List(temps2));
    for suffix in ["1", "2"] {
        let key = |name: &str| format!("{name}{suffix}");
        set(&key("extramathexpression"), repeat("".into()));
        set(
            &key("extradevicecolor"),
            repeat(if suffix == "1" { "#ad0427" } else { "#48abff" }.into()),
        );
        set(&key("extraLCDvisibility"), repeat(true.into()));
        set(&key("extraCurveVisibility"), repeat(true.into()));
        set(&key("extraDelta"), repeat(false.into()));
        set(&key("extraFill"), repeat(0_i64.into()));
        set(&key("extramarkersizes"), repeat(6.0.into()));
        set(&key("extramarkers"), repeat("None".into()));
        set(&key("extralinewidths"), repeat(1.0.into()));
        set(&key("extralinestyles"), repeat("-".into()));
        set(&key("extradrawstyles"), repeat("default".into()));
        set(&key("extraNoneTempHint"), repeat(false.into()));
    }
}

/// The subset of Artisan's `computed` values that follows from the events:
/// times are seconds after CHARGE, temperatures bean temperature in °C, and
/// the turning point's sample index (`TP_idx`).
fn computed(roast: &Roast) -> PyValue {
    let mut c = PyValue::Dict(Vec::new());
    let Some(charge) = roast.event(EventKind::Charge) else {
        return c;
    };
    c.insert("CHARGE_time", round1(charge.t).into());
    if let Some(bt) = charge.bt {
        c.insert("CHARGE_BT", round1(bt).into());
    }
    for (prefix, kind) in COMPUTED_EVENT_PREFIXES {
        let Some(event) = roast.event(kind).filter(|e| e.t > charge.t) else {
            continue;
        };
        if kind == EventKind::TurningPoint {
            if let Some(index) = nearest_index(&roast.samples, event.t) {
                c.insert("TP_idx", index_i64(index).into());
            }
        }
        c.insert(&format!("{prefix}_time"), round1(event.t - charge.t).into());
        if let Some(bt) = event.bt {
            c.insert(&format!("{prefix}_BT"), round1(bt).into());
        }
    }
    if let Some(drop) = roast.event(EventKind::Drop).filter(|e| e.t > charge.t) {
        c.insert("totaltime", round1(drop.t - charge.t).into());
    }
    if let Some(g) = roast.batch.green_weight_g {
        c.insert("weightin", g.into());
    }
    if let Some(r) = roast.batch.roasted_weight_g {
        c.insert("weightout", r.into());
    }
    if let Some(loss) = roast.weight_loss_pct() {
        c.insert("weight_loss", round1(loss).into());
    }
    c
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Splits a batch number into Artisan's prefix and number: the trailing
/// digits are the number.
fn split_batch_no(batch_no: &str) -> Option<(&str, i64)> {
    let digits = batch_no.len()
        - batch_no
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .len();
    if digits == 0 {
        return None;
    }
    let (prefix, nr) = batch_no.split_at(batch_no.len() - digits);
    nr.parse().ok().filter(|n| *n > 0).map(|n| (prefix, n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse, read_alog};
    use chrono::{DateTime, Utc};
    use roast_core::{
        BatchInfo, CommandRequest, CommandSource, ControlLogEntry, EventOrigin, Note, RejectReason,
        RoastEvent, RoastOrigin,
    };
    use uuid::Uuid;

    fn sample(t: f64, bt: f64, burner: Option<f64>) -> Sample {
        let mut s = Sample::bt(t, bt);
        if let Some(b) = burner {
            s.actuators.insert(Actuator::Burner, b);
        }
        s
    }

    fn entry(t: f64, actuator: Actuator, value: f64, outcome: CommandOutcome) -> ControlLogEntry {
        ControlLogEntry {
            t: Some(t),
            at: DateTime::<Utc>::UNIX_EPOCH,
            previous: None,
            request: CommandRequest {
                actuator,
                value,
                source: CommandSource::Ai,
                reason: Some("test".into()),
            },
            outcome,
        }
    }

    fn roast() -> Roast {
        Roast {
            id: Uuid::from_u128(0x1234),
            title: "Grüße \"quoted\"".into(),
            roasted_at: DateTime::from_timestamp(1_794_135_049, 0).unwrap(),
            machine: Some("Skywalker Series - Cubean +".into()),
            sampling_interval_s: 1.0,
            batch: BatchInfo {
                green_weight_g: Some(200.0),
                roasted_weight_g: Some(170.0),
                beans: Some("Catuaí\nline 2".into()),
                batch_no: None,
            },
            ambient: None,
            roast_degree: None,
            green_lot_id: None,
            plan: None,
            profile_version_id: None,
            experiment_id: None,
            sound: None,
            origin: RoastOrigin::Recorded,
            samples: vec![
                sample(0.0, 200.0, Some(80.0)),
                sample(1.0, 150.0, None),
                sample(2.0, 120.0, Some(70.0)),
                Sample {
                    t: 3.0,
                    bt: None,
                    et: Some(250.0),
                    actuators: Default::default(),
                },
            ],
            events: vec![
                RoastEvent {
                    t: 0.0,
                    kind: EventKind::Charge,
                    origin: EventOrigin::Human,
                    bt: Some(200.0),
                },
                RoastEvent {
                    t: 2.0,
                    kind: EventKind::TurningPoint,
                    origin: EventOrigin::Auto,
                    bt: Some(120.0),
                },
                RoastEvent {
                    t: 3.0,
                    kind: EventKind::Drop,
                    origin: EventOrigin::Human,
                    bt: None,
                },
            ],
            control_log: vec![
                entry(
                    1.0,
                    Actuator::Burner,
                    70.0,
                    CommandOutcome::Applied {
                        value: 65.0,
                        clamped_from: Some(70.0),
                    },
                ),
                entry(
                    0.0,
                    Actuator::Air,
                    40.0,
                    CommandOutcome::Applied {
                        value: 40.0,
                        clamped_from: None,
                    },
                ),
                entry(
                    2.0,
                    Actuator::Burner,
                    0.0,
                    CommandOutcome::Rejected {
                        reason: RejectReason::NoLease,
                    },
                ),
                entry(
                    2.0,
                    Actuator::Cooling,
                    100.0,
                    CommandOutcome::Applied {
                        value: 100.0,
                        clamped_from: None,
                    },
                ),
            ],
            notes: vec![
                Note {
                    t: None,
                    text: "whole roast".into(),
                    origin: EventOrigin::Human,
                },
                Note {
                    t: None,
                    text: format!("{CUPPING_NOTE_PREFIX}sweet"),
                    origin: EventOrigin::Human,
                },
                Note {
                    t: Some(1.0),
                    text: "smoke".into(),
                    origin: EventOrigin::Ai,
                },
            ],
        }
    }

    #[test]
    fn the_batch_number_goes_into_artisans_batch_counter_and_back() {
        let mut r = roast();
        r.batch.batch_no = Some("BRYB-002".into());
        let text = write_alog_with_offset(&r, FixedOffset::east_opt(0).unwrap());
        let d = parse(&text).unwrap();
        assert_eq!(d.get("roastbatchprefix").unwrap().to_literal(), "'BRYB-'");
        assert_eq!(d.get("roastbatchnr").unwrap().to_literal(), "2");
        let back = crate::read_alog(&text).unwrap();
        assert_eq!(back.batch.batch_no.as_deref(), Some("BRYB-002"));
        assert_eq!(split_batch_no("BR011"), Some(("BR", 11)));
        assert_eq!(split_batch_no("BRYB"), None);
    }

    #[test]
    fn writes_the_expected_keys() {
        let utc = FixedOffset::east_opt(3600).unwrap();
        let text = write_alog_with_offset(&roast(), utc);
        assert!(text.is_ascii());
        let d = parse(&text).unwrap();
        let get = |k: &str| {
            d.get(k)
                .unwrap_or_else(|| panic!("{k} missing"))
                .to_literal()
        };
        assert_eq!(get("timeindex"), "[0, 0, 0, 0, 0, 0, 3, 0]");
        assert_eq!(get("title"), r#"'Gr\\xfc\\xdfe "quoted"'"#);
        assert_eq!(get("beans"), r"'Catua\\xed\\nline 2'");
        assert_eq!(get("weight"), "[200.0, 170.0, 'g']");
        assert_eq!(get("roastisodate"), "'2026-11-08'");
        assert_eq!(get("roasttime"), "'11:50:49'");
        assert_eq!(get("roastdate"), "'Sun Nov 8 2026'");
        assert_eq!(get("roasttzoffset"), "-3600");
        assert_eq!(get("roastepoch"), "1794135049");
        assert_eq!(get("roastUUID"), "'00000000000000000000000000001234'");
        assert_eq!(get("specialevents"), "[0, 1, 1, 2]");
        assert_eq!(get("specialeventstype"), "[0, 3, 4, 4]");
        assert_eq!(get("specialeventsvalue"), "[5.0, 7.5, 0.0, 0.0]");
        assert_eq!(
            get("specialeventsStrings"),
            "['40%', '65%', 'smoke', 'Cooling 100%']"
        );
        assert_eq!(get("roastingnotes"), "'whole roast'");
        assert_eq!(get("cuppingnotes"), "'sweet'");
        assert_eq!(get("temp1"), "[-1.0, -1.0, -1.0, 250.0]");
        assert_eq!(get("temp2"), "[200.0, 150.0, 120.0, -1.0]");
        assert_eq!(get("extradevices"), "[32]");
        assert_eq!(get("extraname1"), "['{3}']");
        assert_eq!(get("extraname2"), "['']");
        assert_eq!(get("extratemp1"), "[[80.0, -1.0, 70.0, -1.0]]");
        assert_eq!(
            get("computed"),
            "{'CHARGE_time': 0.0, 'CHARGE_BT': 200.0, 'DROP_time': 3.0, \
            'TP_idx': 2, 'TP_time': 2.0, 'TP_BT': 120.0, 'totaltime': 3.0, 'weightin': 200.0, \
            'weightout': 170.0, 'weight_loss': 15.0}"
        );
    }

    #[test]
    fn written_profiles_read_back() {
        let original = roast();
        let back = read_alog(&write_alog(&original)).unwrap();
        assert_eq!(back.id, original.id);
        assert_eq!(back.title, original.title);
        assert_eq!(back.roasted_at, original.roasted_at);
        assert_eq!(back.batch, original.batch);
        assert_eq!(back.machine, original.machine);
        assert_eq!(back.samples, original.samples);
        let kinds: Vec<_> = back.events.iter().map(|e| (e.kind, e.t, e.bt)).collect();
        assert_eq!(
            kinds,
            [
                (EventKind::Charge, 0.0, Some(200.0)),
                (EventKind::TurningPoint, 2.0, Some(120.0)),
                (EventKind::Drop, 3.0, None)
            ]
        );
        let log: Vec<_> = back
            .control_log
            .iter()
            .map(|e| (e.t, e.request.actuator, e.request.value))
            .collect();
        assert_eq!(
            log,
            [
                (Some(0.0), Actuator::Air, 40.0),
                (Some(1.0), Actuator::Burner, 65.0)
            ]
        );
        let notes: Vec<_> = back.notes.iter().map(|n| (n.t, n.text.as_str())).collect();
        assert_eq!(
            notes,
            [
                (None, "whole roast"),
                (None, "Cupping notes: sweet"),
                (Some(1.0), "smoke"),
                (Some(2.0), "Cooling 100%")
            ]
        );
    }

    #[test]
    fn an_empty_roast_is_still_a_profile() {
        let mut r = roast();
        r.samples.clear();
        r.events.clear();
        r.control_log.clear();
        r.notes.clear();
        let back = read_alog(&write_alog(&r)).unwrap();
        assert!(back.samples.is_empty());
        assert!(back.events.is_empty());
    }
}

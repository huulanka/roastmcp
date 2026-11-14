//! `roastmcp audio …`: play recorded roast sound through the crack detector.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone as _, Utc};
use roast_audio::offline::{self, Analysis};
use roast_audio::{Band, Sensitivity};
use roast_core::{EventKind, Roast};

/// Options of `roastmcp audio analyze`.
pub struct AnalyzeArgs {
    pub file: PathBuf,
    /// A roast id from the archive, or a `.roast.json` / `.alog` file.
    pub roast: Option<String>,
    /// Local start time of the sound, `YYYY-MM-DD HH:MM:SS`.
    pub start: Option<String>,
    pub sensitivity: Sensitivity,
    pub json: bool,
}

/// When the sound started: `--start`, else a name like
/// `2026-11-14_19-02-11.flac`, else the file's modification time minus its
/// length.
fn sound_start(args: &AnalyzeArgs, seconds: f64) -> anyhow::Result<DateTime<Utc>> {
    let local = |s: &str, f: &str| {
        NaiveDateTime::parse_from_str(s, f)
            .ok()
            .and_then(|n| Local.from_local_datetime(&n).single())
            .map(|d| d.with_timezone(&Utc))
    };
    if let Some(s) = &args.start {
        return local(s, "%Y-%m-%d %H:%M:%S")
            .with_context(|| format!("--start {s}: expected YYYY-MM-DD HH:MM:SS"));
    }
    let stem = args
        .file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if let Some(t) = stem.get(..19).and_then(|s| local(s, "%Y-%m-%d_%H-%M-%S")) {
        return Ok(t);
    }
    let modified: DateTime<Utc> = std::fs::metadata(&args.file)?.modified()?.into();
    Ok(modified - chrono::Duration::milliseconds((seconds * 1000.0) as i64))
}

fn load_roast(
    data_dir: &Path,
    which: Option<&str>,
    start: DateTime<Utc>,
    seconds: f64,
) -> anyhow::Result<(Option<Roast>, Vec<serde_json::Value>)> {
    let store = crate::open_store(data_dir).ok();
    let archive = store.map(|s| roastd::files::Archive::new(s, data_dir.join("roasts")));
    let summaries = archive
        .as_ref()
        .and_then(|a| a.summaries().ok())
        .unwrap_or_default();
    let roast = match which {
        Some(w) if w.ends_with(".alog") => Some(roast_alog::read_alog_file(w)?),
        Some(w) if w.ends_with(".json") => {
            let text = std::fs::read_to_string(w).with_context(|| format!("cannot read {w}"))?;
            Some(serde_json::from_str(&text).with_context(|| format!("{w} is no roast file"))?)
        }
        Some(id) => {
            let id: uuid::Uuid = id.parse().context("--roast: an id, .roast.json or .alog")?;
            let a = archive.as_ref().context("the archive cannot be opened")?;
            Some(
                a.find(id)?
                    .with_context(|| format!("no roast {id} in the archive"))?,
            )
        }
        None => {
            // The real roast whose recording started while the sound ran
            // (or up to 30 min after it started).
            let end = start + chrono::Duration::seconds(seconds as i64);
            let found = summaries.iter().find_map(|s| {
                let sim = s.get("simulated").and_then(serde_json::Value::as_bool) == Some(true);
                let at: DateTime<Utc> = s.get("roasted_at")?.as_str()?.parse().ok()?;
                let id: uuid::Uuid = s.get("id")?.as_str()?.parse().ok()?;
                (!sim && at >= start - chrono::Duration::minutes(30) && at <= end).then_some(id)
            });
            match (found, archive.as_ref()) {
                (Some(id), Some(a)) => a.find(id)?,
                _ => None,
            }
        }
    };
    Ok((roast, summaries))
}

fn clock(s: f64) -> String {
    let sign = if s < 0.0 { "−" } else { "" };
    let s = s.abs().round() as i64;
    format!("{sign}{}:{:02}", s / 60, s % 60)
}

fn name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::FirstCrackStart => "first crack",
        EventKind::FirstCrackEnd => "first crack end",
        EventKind::SecondCrackStart => "second crack",
        _ => "event",
    }
}

/// `roastmcp audio analyze`.
pub fn analyze(data_dir: &Path, args: &AnalyzeArgs) -> anyhow::Result<()> {
    let sound = roast_audio::file::read(&args.file)?;
    let seconds = sound.seconds();
    let start = sound_start(args, seconds)?;
    let (roast, summaries) = load_roast(data_dir, args.roast.as_deref(), start, seconds)?;
    let offset_s = roast.as_ref().map_or(0.0, |r| {
        (start - r.roasted_at).num_milliseconds() as f64 / 1000.0
    });
    let expectation = roastd::files::crack_expectation(
        &summaries,
        roast.as_ref().and_then(|r| r.green_lot_id),
        false,
        roast.as_ref().map(|r| r.id),
    );
    let a = offline::analyze(
        &sound.samples,
        sound.rate,
        args.sensitivity,
        roast.as_ref(),
        offset_s,
        &expectation,
    );
    if args.json {
        println!("{}", serde_json::to_string_pretty(&a)?);
        return Ok(());
    }
    report(args, &a, roast.as_ref(), start, offset_s, &expectation);
    Ok(())
}

fn report(
    args: &AnalyzeArgs,
    a: &Analysis,
    roast: Option<&Roast>,
    start: DateTime<Utc>,
    offset_s: f64,
    e: &roast_core::CrackExpectation,
) {
    println!(
        "Sound: {} · {:.1} kHz · {} · from {}",
        args.file.display(),
        f64::from(a.rate) / 1000.0,
        clock(a.seconds),
        start.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S")
    );
    if a.rate < roast_audio::spectrum::MIN_RATE_FOR_HIGH_BAND {
        println!("  below 32 kHz: second crack is not listened for");
    }
    let charge = roast.and_then(Roast::charge_t).unwrap_or(0.0);
    match roast {
        Some(r) => println!(
            "Roast: {} ({}) · the sound starts at {} on its clock",
            r.title,
            r.roasted_at.with_timezone(&Local).format("%H:%M:%S"),
            clock(offset_s - charge)
        ),
        None => println!("Roast: none found; the temperature is assumed inside the window"),
    }
    let basis = match e.basis {
        roast_core::CrackBasis::Lot => "this lot",
        roast_core::CrackBasis::Roasts => "all roasts",
        roast_core::CrackBasis::Typical => "a typical value",
    };
    println!(
        "First crack believed at {:.0}–{:.0} °C (usually {:.1} °C, from {basis}, {} roasts)",
        e.low, e.high, e.bt, e.roasts
    );
    println!();
    println!("Pops per 10 s (time since CHARGE; █ crack band, ░ high band):");
    for &(t, crack, high) in &a.per_10s {
        if crack + high == 0 {
            continue;
        }
        let bt = roast
            .and_then(|r| offline::bt_at(r, t + 5.0))
            .map(|b| format!("{b:5.1} °C"))
            .unwrap_or_default();
        println!(
            "  {:>6}  {bt:>8}  {}{}",
            clock(t - charge),
            "█".repeat(crack),
            "░".repeat(high)
        );
    }
    println!();
    let crack = a.pops.iter().filter(|p| p.band == Band::Crack).count();
    let high = a.pops.len() - crack;
    println!("{crack} pops in the crack band, {high} in the high band.");
    let mut heard = a.heard.clone();
    heard.sort_by(|x, y| x.t.total_cmp(&y.t));
    for h in &heard {
        let bt = roast
            .and_then(|r| offline::bt_at(r, h.t))
            .map(|b| format!(" · {b:.1} °C"))
            .unwrap_or_default();
        println!(
            "Heard {}: {}{bt} ({} pops)",
            name(h.kind),
            clock(h.t - charge),
            h.pops
        );
    }
    if a.heard.is_empty() {
        println!("Heard no crack.");
    }
    for d in &a.deviations {
        match d.delta_s {
            Some(delta) => println!(
                "Marked {} at {}: heard {:+.0} s {}",
                name(d.kind),
                clock(d.marked_s),
                delta,
                if delta.abs() <= 10.0 {
                    "✓"
                } else {
                    "✗ (more than 10 s)"
                }
            ),
            None => println!(
                "Marked {} at {}: not heard ✗",
                name(d.kind),
                clock(d.marked_s)
            ),
        }
    }
}

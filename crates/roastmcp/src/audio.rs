//! `roastmcp audio …`: play recorded roast sound through the crack detector.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone as _, Utc};
use roast_audio::offline::{self, Analysis};
use roast_audio::{Band, Sensitivity};
use roast_core::{EventKind, Roast};

/// Options of `roastmcp audio analyze`.
pub struct AnalyzeArgs {
    /// The sound file; without one, the sound kept with `roast`.
    pub file: Option<PathBuf>,
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
fn sound_start(args: &AnalyzeArgs, file: &Path, seconds: f64) -> anyhow::Result<DateTime<Utc>> {
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
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if let Some(t) = stem.get(..19).and_then(|s| local(s, "%Y-%m-%d_%H-%M-%S")) {
        return Ok(t);
    }
    let modified: DateTime<Utc> = std::fs::metadata(file)?.modified()?.into();
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
/// The sound file kept with a roast (ADR 0021 §9), and where it starts on
/// the roast's clock.
fn kept_sound(data_dir: &Path, roast: &Roast) -> Option<(PathBuf, f64)> {
    let sound = roast.sound.as_ref()?;
    let name = sound.file.as_ref()?;
    let flac = data_dir.join("roasts").join("sound").join(name);
    let path = [flac.clone(), flac.with_extension("wav")]
        .into_iter()
        .find(|p| p.exists())?;
    Some((path, sound.start_t.unwrap_or(0.0)))
}

/// Everything about one analysed sound.
struct Run {
    file: PathBuf,
    analysis: Analysis,
    roast: Option<Roast>,
    start: DateTime<Utc>,
    offset_s: f64,
    expectation: roast_core::CrackExpectation,
}

fn run(data_dir: &Path, args: &AnalyzeArgs) -> anyhow::Result<Run> {
    let (file, kept) = match &args.file {
        Some(f) => (f.clone(), None),
        None => {
            let id = args
                .roast
                .as_deref()
                .context("give a sound file, or --roast <id> of a roast whose sound was kept")?;
            let (roast, _) = load_roast(data_dir, Some(id), Utc::now(), 0.0)?;
            let roast = roast.context("no such roast")?;
            let (path, start_t) =
                kept_sound(data_dir, &roast).context("this roast's sound was not kept")?;
            (path, Some((roast, start_t)))
        }
    };
    let sound = roast_audio::file::read(&file)?;
    let seconds = sound.seconds();
    let (start, roast, summaries, offset_s) = match kept {
        Some((roast, start_t)) => {
            let (_, summaries) = load_roast(data_dir, None, Utc::now(), 0.0)?;
            let start =
                roast.roasted_at + chrono::Duration::milliseconds((start_t * 1000.0) as i64);
            (start, Some(roast), summaries, start_t)
        }
        None => {
            let start = sound_start(args, &file, seconds)?;
            let (roast, summaries) = load_roast(data_dir, args.roast.as_deref(), start, seconds)?;
            let offset_s = roast.as_ref().map_or(0.0, |r| {
                (start - r.roasted_at).num_milliseconds() as f64 / 1000.0
            });
            (start, roast, summaries, offset_s)
        }
    };
    let expectation = roastd::files::crack_expectation(
        &summaries,
        roast.as_ref().and_then(|r| r.green_lot_id),
        false,
        roast.as_ref().map(|r| r.id),
    );
    let analysis = offline::analyze(
        &sound.samples,
        sound.rate,
        args.sensitivity,
        roast.as_ref(),
        offset_s,
        &expectation,
    );
    Ok(Run {
        file,
        analysis,
        roast,
        start,
        offset_s,
        expectation,
    })
}

/// `roastmcp audio analyze`.
pub fn analyze(data_dir: &Path, args: &AnalyzeArgs) -> anyhow::Result<()> {
    let r = run(data_dir, args)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&r.analysis)?);
        return Ok(());
    }
    report(
        &r.file,
        &r.analysis,
        r.roast.as_ref(),
        r.start,
        r.offset_s,
        &r.expectation,
    );
    Ok(())
}

/// `roastmcp audio eval`: every roast with its sound (kept by the daemon, or
/// recorded into `dir`), heard against marked first crack.
pub fn eval(data_dir: &Path, dir: Option<&Path>, sensitivity: Sensitivity) -> anyhow::Result<()> {
    let mut jobs: Vec<AnalyzeArgs> = Vec::new();
    let store = crate::open_store(data_dir)?;
    let archive = roastd::files::Archive::new(store, data_dir.join("roasts"));
    for s in archive.summaries()? {
        let Some(id) = s.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Ok(Some(roast)) = archive.find(id.parse()?) else {
            continue;
        };
        if kept_sound(data_dir, &roast).is_some() {
            jobs.push(AnalyzeArgs {
                file: None,
                roast: Some(id.to_owned()),
                start: None,
                sensitivity,
                json: false,
            });
        }
    }
    if let Some(dir) = dir {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .with_context(|| format!("cannot read {}", dir.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    e.eq_ignore_ascii_case("flac") || e.eq_ignore_ascii_case("wav")
                })
            })
            .collect();
        files.sort();
        jobs.extend(files.into_iter().map(|f| AnalyzeArgs {
            file: Some(f),
            roast: None,
            start: None,
            sensitivity,
            json: false,
        }));
    }
    if jobs.is_empty() {
        println!("No roast sound found (switch on keeping the sound in the app, or pass --dir).");
        return Ok(());
    }
    let (mut within, mut compared) = (0, 0);
    println!(
        "{:<34} {:>8} {:>8} {:>7}",
        "Roast", "marked", "heard", "Δ s"
    );
    for job in &jobs {
        let r = match run(data_dir, job) {
            Ok(r) => r,
            Err(e) => {
                println!("{:<34} {e}", short(&job_name(job)));
                continue;
            }
        };
        let Some(roast) = &r.roast else {
            println!(
                "{:<34} no roast recorded at that time",
                short(&job_name(job))
            );
            continue;
        };
        let fc = r
            .analysis
            .deviations
            .iter()
            .find(|d| d.kind == EventKind::FirstCrackStart);
        let title = roast
            .batch
            .batch_no
            .clone()
            .unwrap_or_else(|| roast.title.clone());
        match fc {
            Some(d) => {
                compared += 1;
                if d.delta_s.is_some_and(|x| x.abs() <= 10.0) {
                    within += 1;
                }
                println!(
                    "{:<34} {:>8} {:>8} {:>7}",
                    short(&title),
                    clock(d.marked_s),
                    d.heard_s.map_or_else(|| "–".into(), clock),
                    d.delta_s.map_or_else(|| "–".into(), |x| format!("{x:+.0}"))
                );
            }
            None => println!("{:<34} {:>8}", short(&title), "no mark"),
        }
    }
    println!();
    println!(
        "{within} of {compared} roasts heard within ±10 s of the marked first crack (M6 asks for most)."
    );
    Ok(())
}

fn job_name(job: &AnalyzeArgs) -> String {
    job.file
        .as_ref()
        .and_then(|f| f.file_name())
        .map(|f| f.to_string_lossy().into_owned())
        .or_else(|| job.roast.clone())
        .unwrap_or_default()
}

fn short(s: &str) -> String {
    if s.chars().count() <= 33 {
        s.to_owned()
    } else {
        format!("{}…", s.chars().take(32).collect::<String>())
    }
}

/// Options of `roastmcp audio mix`.
pub struct MixArgs {
    pub out: PathBuf,
    pub noise: Option<PathBuf>,
    pub pops: Vec<PathBuf>,
    pub spec: roast_audio::mix::MixSpec,
}

/// `roastmcp audio mix`: a test sound with exact labels (ADR 0021 §10).
pub fn mix(args: &MixArgs) -> anyhow::Result<()> {
    use roast_audio::mix;
    let noise = args
        .noise
        .as_deref()
        .map(roast_audio::file::read)
        .transpose()?;
    let rate = noise.as_ref().map_or(48_000, |n| n.rate);
    let mut fc_bank = Vec::new();
    for p in &args.pops {
        fc_bank.extend(mix::cut_pops(&roast_audio::file::read(p)?, 200));
    }
    if fc_bank.is_empty() {
        fc_bank = mix::synthetic_pops(rate, 24, false, args.spec.seed);
    }
    let sc_bank = mix::synthetic_pops(rate, 24, true, args.spec.seed ^ 1);
    let m = mix::mix(noise.as_ref(), &fc_bank, &sc_bank, &args.spec, rate);
    roast_audio::file::write_wav(&args.out, m.sound.rate, &m.sound.samples)?;
    let labels = args.out.with_extension("json");
    std::fs::write(&labels, serde_json::to_string_pretty(&m.labels)?)?;
    println!(
        "Mixed {} ({} pops from {}, {}): {}",
        clock(args.spec.seconds),
        m.labels.pops.len(),
        if args.pops.is_empty() {
            "the synthesizer".to_owned()
        } else {
            format!("{} cut pops", fc_bank.len())
        },
        if noise.is_some() {
            "your noise"
        } else {
            "synthetic fan noise"
        },
        args.out.display()
    );
    println!("Labels: {}", labels.display());
    let e = roast_core::CrackExpectation::from_roasts(&[], &[]);
    let a = offline::analyze(
        &m.sound.samples,
        m.sound.rate,
        Sensitivity::Normal,
        None,
        0.0,
        &e,
    );
    for (kind, label) in [
        (EventKind::FirstCrackStart, m.labels.first_crack_start),
        (EventKind::SecondCrackStart, m.labels.second_crack_start),
    ] {
        let Some(label) = label else { continue };
        let heard = a.heard.iter().find(|h| h.kind == kind).map(|h| h.t);
        println!(
            "{}: placed {}, heard {}",
            name(kind),
            clock(label),
            heard.map_or_else(
                || "not at all".to_owned(),
                |h| format!("{} ({:+.0} s)", clock(h), h - label)
            )
        );
    }
    Ok(())
}

fn report(
    file: &Path,
    a: &Analysis,
    roast: Option<&Roast>,
    start: DateTime<Utc>,
    offset_s: f64,
    e: &roast_core::CrackExpectation,
) {
    println!(
        "Sound: {} · {:.1} kHz · {} · from {}",
        file.display(),
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
